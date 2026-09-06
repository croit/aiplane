// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Which replica has probably seen this prompt's prefix.
//!
//! An approximate, per-pool index of prefix → backend, and the reason it exists
//! rather than a single sticky hash per conversation.
//!
//! # Why block matching beats one hash
//!
//! A sticky key can only hash something that does not change between turns, so
//! it hashes the opening of the conversation and nothing else. That works, and
//! it is what "session affinity" means everywhere it is offered — but it throws
//! away two things:
//!
//!   - **Cross-session sharing.** Several agent windows on one repo send the
//!     *same* enormous system prompt. Whichever replica already holds those
//!     blocks can start a brand-new session warm. A per-conversation hash
//!     scatters new sessions at random instead.
//!   - **Recovery after a rewrite.** Compaction, an edited turn, a branch — any
//!     of these change the conversation's opening, so a sticky key re-rolls and
//!     the session lands somewhere cold. Block matching still matches the
//!     unchanged head of the prompt and stays put.
//!
//! So instead of asking "whose conversation is this", ask "who has the longest
//! matching prefix". This is the design every serious inference router
//! converged on: SGLang's cache-aware policy keeps an approximate radix tree
//! per worker; llm-d's `approx-prefix-cache-producer` splits the prompt into
//! fixed-size blocks, chains a rolling hash, and keeps an LRU index of which
//! prefix hash went to which pod; vLLM's production-stack calls the same idea
//! prefix-aware routing. This module is the llm-d shape, which needs no
//! tokenizer and no model-server cooperation.
//!
//! # Approximate, and honest about it
//!
//! The index records what the gateway *sent*, not what a replica actually still
//! holds — a real KV cache evicts under pressure and the gateway is never told.
//! Every entry therefore expires ([`ENTRY_TTL`]) and the whole index is capped
//! ([`MAX_ENTRIES`]). Being wrong is cheap: a mispredicted hit costs one
//! ordinary prefill, exactly what load-only routing pays every single time.
//!
//! Blocks are counted in **characters**, not tokens, because a proxy has no
//! tokenizer. That mis-estimates block boundaries, which does not matter: the
//! index only ever compares its own hashes with its own hashes, so a consistent
//! approximation is as good as an exact one for deciding *who saw this before*.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Characters per block. Roughly vLLM's 16-token page at ~4 characters per
/// token — the granularity at which a match is worth acting on. Smaller blocks
/// mean a finer match and a bigger index; larger blocks blur short prompts into
/// one block and lose all resolution.
const BLOCK_CHARS: usize = 64;

/// Longest prefix worth indexing, in blocks. A conversation grows without
/// bound; the value of matching more of it falls off long before that, and each
/// block costs an index entry per request. 1024 blocks ≈ 64 K characters ≈ the
/// head of a long agent conversation.
const MAX_BLOCKS: usize = 1024;

/// How long an entry is trusted. Past this the replica has probably evicted
/// those pages, and a confident wrong answer is worse than no answer.
const ENTRY_TTL: Duration = Duration::from_secs(600);

/// Hard cap on indexed prefixes per pool, evicted in insertion order. Bounds
/// memory on a busy deployment; the hot prefixes are re-inserted on every
/// request that uses them, so eviction costs at most one cold turn.
const MAX_ENTRIES: usize = 200_000;

/// FNV-1a, as in [`super::affinity`]: cheap, and stable across processes so a
/// restart doesn't invalidate the world.
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv1a(seed: u64, bytes: &[u8]) -> u64 {
    let mut h = seed;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// The rolling chain of prefix hashes for one prompt: entry `i` identifies
/// *the first `i + 1` blocks*, so two prompts share a leading run of chain
/// entries exactly as far as their text agrees.
///
/// Splitting on character boundaries (not byte boundaries) keeps this from
/// panicking on non-ASCII prompts and keeps the blocks stable — a multi-byte
/// character must land wholly in one block or the same text could chunk
/// differently depending on where it starts.
pub fn prefix_chain(text: &str) -> Vec<u64> {
    let mut chain = Vec::new();
    let mut running = FNV_OFFSET;
    let mut block = String::with_capacity(BLOCK_CHARS);
    for ch in text.chars() {
        block.push(ch);
        if block.chars().count() == BLOCK_CHARS {
            running = fnv1a(running, block.as_bytes());
            chain.push(running);
            block.clear();
            if chain.len() == MAX_BLOCKS {
                return chain;
            }
        }
    }
    // Deliberately drop a trailing partial block: it is the part of the prompt
    // most likely to differ between turns (the newest message), and indexing it
    // would add an entry that never matches again.
    chain
}

/// Per-pool index of prefix hash → the backends that were recently sent it.
#[derive(Default)]
pub struct PrefixIndex {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    /// A prefix can legitimately live on several replicas (a shared system
    /// prompt does), so this is a small list rather than one backend.
    seen: HashMap<u64, Vec<(String, Instant)>>,
    /// For each prefix, the distinct blocks that have followed it — capped.
    ///
    /// This is what tells a **shared preamble** from a conversation's own
    /// history, and without it the router funnels a whole fleet onto one
    /// replica. Every session of one client sends the same system prompt, so
    /// its blocks are held by whichever replica warmed up first; matching them
    /// looks like a cache hit but says nothing about where the conversation
    /// belongs. A prefix that several *different* conversations have extended
    /// differently has more than one successor — it is a branch point, and
    /// everything up to the last one is shared boilerplate.
    ///
    /// A flat hash map cannot express that; a radix tree can, and this is the
    /// one piece of a radix tree that the decision actually needs.
    succ: HashMap<u64, Vec<u64>>,
    /// Insertion order, for the size cap.
    order: VecDeque<u64>,
}

/// How many distinct successors to remember per prefix. Two is all the decision
/// needs — "more than one" is the whole question — but a little slack keeps the
/// answer stable if one of them is evicted.
const MAX_SUCC: usize = 4;

impl PrefixIndex {
    /// How many leading blocks of `chain` this backend was recently sent.
    ///
    /// Stops at the first block it has no record of: the answer is a *prefix*
    /// length, and a later isolated match says nothing about cache residency.
    pub fn match_len(&self, chain: &[u64], backend: &str) -> usize {
        let Ok(inner) = self.inner.lock() else {
            return 0;
        };
        let now = Instant::now();
        let mut n = 0;
        for h in chain {
            let hit = inner.seen.get(h).is_some_and(|owners| {
                owners
                    .iter()
                    .any(|(name, at)| name == backend && now.duration_since(*at) < ENTRY_TTL)
            });
            if !hit {
                break;
            }
            n += 1;
        }
        n
    }

    /// How many of the leading matched blocks are **specific to this
    /// conversation** — the match that remains after the shared boilerplate.
    ///
    /// Walks the leading run this backend holds, notes the last block within it
    /// that several different conversations have extended differently (a branch
    /// point), and counts only what follows. A brand-new conversation therefore
    /// scores 0 on every replica, however much of the shared system prompt they
    /// hold, and gets balanced; its second turn scores above 0 on exactly the
    /// replica that served the first.
    pub fn discriminating_match_len(&self, chain: &[u64], backend: &str) -> usize {
        let matched = self.match_len(chain, backend);
        if matched == 0 {
            return 0;
        }
        let Ok(inner) = self.inner.lock() else {
            return 0;
        };
        // The last branch point at or before the end of the matched run. Blocks
        // after it were only ever followed one way, so they belong to one
        // conversation.
        let last_branch = chain[..matched]
            .iter()
            .rposition(|h| inner.succ.get(h).is_some_and(|s| s.len() > 1));
        match last_branch {
            Some(i) => matched - (i + 1),
            None => matched,
        }
    }

    /// Record that `backend` was sent this prefix. Called after a backend is
    /// actually chosen, so the index tracks what was dispatched.
    pub fn record(&self, chain: &[u64], backend: &str) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        let now = Instant::now();
        // Successor links first: they describe the *prompt*, not the routing, so
        // they are recorded whoever served it.
        for pair in chain.windows(2) {
            let succ = inner.succ.entry(pair[0]).or_default();
            if !succ.contains(&pair[1]) && succ.len() < MAX_SUCC {
                succ.push(pair[1]);
            }
        }
        for h in chain {
            let owners = inner.seen.entry(*h).or_default();
            match owners.iter_mut().find(|(name, _)| name == backend) {
                Some((_, at)) => *at = now,
                None => {
                    owners.push((backend.to_string(), now));
                    // A prefix on more than a handful of replicas is a prefix
                    // that tells us nothing; keep the list short.
                    if owners.len() > 8 {
                        owners.remove(0);
                    }
                }
            }
            inner.order.push_back(*h);
        }
        while inner.order.len() > MAX_ENTRIES {
            if let Some(old) = inner.order.pop_front() {
                inner.seen.remove(&old);
                inner.succ.remove(&old);
            }
        }
    }

    /// Entry count, for tests and diagnostics.
    pub fn len(&self) -> usize {
        self.inner.lock().map(|i| i.seen.len()).unwrap_or(0)
    }

    /// Whether the index has learned anything yet.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The chain has to agree exactly as far as the text does, and no further —
    /// that property is what makes "longest match" mean "most shared prefix".
    #[test]
    fn chains_agree_over_the_shared_prefix_and_diverge_after_it() {
        let shared = "S".repeat(BLOCK_CHARS * 3);
        let a = prefix_chain(&format!("{shared}{}", "a".repeat(BLOCK_CHARS * 2)));
        let b = prefix_chain(&format!("{shared}{}", "b".repeat(BLOCK_CHARS * 2)));
        assert_eq!(a[..3], b[..3], "the shared blocks must hash identically");
        assert_ne!(a[3], b[3], "the first differing block must diverge");
    }

    /// A conversation that grows keeps every earlier chain entry, which is the
    /// mechanism by which turn N lands where turn N-1 did — no stable key
    /// required.
    #[test]
    fn growing_a_conversation_extends_the_chain_without_rewriting_it() {
        let turn1 = "sys".repeat(BLOCK_CHARS);
        let turn2 = format!("{turn1}{}", "more".repeat(BLOCK_CHARS));
        let c1 = prefix_chain(&turn1);
        let c2 = prefix_chain(&turn2);
        assert!(c2.len() > c1.len());
        assert_eq!(c1[..], c2[..c1.len()]);
    }

    /// A partial trailing block is dropped: it is the newest, most volatile part
    /// of the prompt, and indexing it would add entries that never match again.
    #[test]
    fn a_partial_trailing_block_is_not_indexed() {
        assert!(prefix_chain(&"x".repeat(BLOCK_CHARS - 1)).is_empty());
        assert_eq!(prefix_chain(&"x".repeat(BLOCK_CHARS)).len(), 1);
        assert_eq!(prefix_chain(&"x".repeat(BLOCK_CHARS * 2 - 1)).len(), 1);
    }

    /// Multi-byte text must not panic and must chunk by characters, so the same
    /// text always produces the same blocks.
    #[test]
    fn non_ascii_text_chunks_by_character() {
        let text = "ü".repeat(BLOCK_CHARS * 2);
        let chain = prefix_chain(&text);
        assert_eq!(chain.len(), 2, "two character-blocks, not four byte-blocks");
        assert_eq!(chain, prefix_chain(&text), "must be deterministic");
    }

    /// The core query: longest matching prefix per backend, and it stops at the
    /// first gap rather than counting isolated later hits.
    #[test]
    fn match_len_measures_the_leading_run_only() {
        let index = PrefixIndex::default();
        let chain = prefix_chain(&"z".repeat(BLOCK_CHARS * 5));
        index.record(&chain[..3], "gpu0");

        assert_eq!(index.match_len(&chain, "gpu0"), 3);
        assert_eq!(index.match_len(&chain, "gpu1"), 0);

        // A later block recorded in isolation must not extend the run.
        index.record(&chain[4..5], "gpu0");
        assert_eq!(index.match_len(&chain, "gpu0"), 3);
    }

    /// A shared prefix on two replicas is normal — a system prompt is exactly
    /// that — so both must be reported as holding it.
    #[test]
    fn several_backends_can_hold_the_same_prefix() {
        let index = PrefixIndex::default();
        let chain = prefix_chain(&"q".repeat(BLOCK_CHARS * 2));
        index.record(&chain, "gpu0");
        index.record(&chain, "gpu1");
        assert_eq!(index.match_len(&chain, "gpu0"), 2);
        assert_eq!(index.match_len(&chain, "gpu1"), 2);
    }

    /// The index is bounded. Being wrong costs one prefill; being unbounded
    /// costs the process.
    #[test]
    fn the_index_is_capped() {
        let index = PrefixIndex::default();
        // Two entries per call, far more calls than the cap allows.
        for i in 0..(MAX_ENTRIES / 2 + 100) {
            let chain = vec![i as u64, (i as u64) << 32];
            index.record(&chain, "gpu0");
        }
        assert!(
            index.len() <= MAX_ENTRIES,
            "index grew past its cap: {}",
            index.len()
        );
    }
}
