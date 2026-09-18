// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Routing keys for prefix-cache affinity.
//!
//! Every replica of a self-hosted model keeps its own KV cache, and vLLM's
//! automatic prefix caching means a conversation is cheap to continue **on the
//! machine that already holds its prefix** and expensive anywhere else. Agent
//! traffic is the extreme case: a Claude Code turn resends a huge, unchanged
//! system prompt plus the whole growing history, so a cache hit is most of the
//! request and a miss is a full prefill.
//!
//! Load-balancing strategies are blind to that. `round_robin` and
//! `least_inflight` will happily alternate one session's consecutive turns
//! between two GPUs, and each turn then lands on the instance whose cached
//! prefix is one turn stale — paying full prefill every single time, on both
//! machines. The fix is to send a conversation back where it came from, which
//! needs a key that is *stable across a session's turns* and *different between
//! sessions*.
//!
//! ## There is no session id, so this does not pretend to find one
//!
//! Nothing in the Anthropic or OpenAI wire format identifies a conversation.
//! `metadata.user_id` exists but is per **user**, which is the wrong
//! granularity precisely when it matters — several parallel agent sessions from
//! one person would collapse onto one replica. So a client that *can* name its
//! own session should, and one that can't gets a derived key:
//!
//!   1. **`x-aiplane-affinity`** (or its former spelling
//!      `x-gateway-affinity`), if the client sends it. Authoritative,
//!      because it is the only thing that is actually *told* to us rather than
//!      inferred. Claude Code can set it per session via
//!      `ANTHROPIC_CUSTOM_HEADERS`, which is read once at launch — so one value
//!      per terminal is exactly one value per session. See `docs/upstreams.md`.
//!   2. Otherwise, a hash of the request's **conversation prefix**.
//!
//! ## What the derived key hashes: the first user message, and nothing else
//!
//! Strictly speaking it identifies a *conversation prefix*, not a session — and
//! that is the right thing to key on, since the prefix is what the KV cache
//! holds.
//!
//! The **first user message** is the whole key. It is the only element of a
//! conversation that is genuinely immutable while the conversation grows, and it
//! is what separates two sessions that share a system prompt verbatim — two
//! agent windows open on the same repo.
//!
//! The system prompt is deliberately **excluded**, which is the opposite of the
//! obvious choice. It is the largest shared prefix, so it looks like the natural
//! thing to key on; it is also the *volatile* half. An agent's system context
//! carries environment material — today's date, the working directory, a git
//! snapshot — and any of that entering the key would move a live conversation to
//! a different replica the moment it changed, discarding a warm cache for
//! nothing. There is no window size that is safe in general, because nothing
//! specifies where in the prompt a client puts the volatile part.
//!
//! The cost is narrow and bounded: two conversations whose *opening message is
//! byte-identical* share a replica. For humans that is rare; for automation that
//! always opens with the same string it is real, and the answer is the header
//! above — a client that knows it has distinct sessions can simply say so, which
//! beats any amount of guessing from the body.
//!
//! ## Stability across restarts
//!
//! The hash is FNV-1a rather than `DefaultHasher`, whose output is not
//! specified to be stable between processes. A gateway restart that reshuffled
//! every conversation to a different replica would throw away every warm cache
//! at once — the exact failure this module exists to prevent, caused by the
//! module itself.

use serde_json::Value;

/// FNV-1a offset basis / prime (64-bit).
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// How many bytes of the first user message to fold in. It can't change within
/// a conversation, so a generous window costs nothing and buys discrimination.
const USER_WINDOW: usize = 4096;

fn fnv1a(seed: u64, bytes: &[u8]) -> u64 {
    let mut h = seed;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// Hash `text`'s leading `window` bytes into `seed`. `with_length` additionally
/// folds in the full byte length, which discriminates two texts that share a
/// long identical opening — safe only for text that cannot change within a
/// conversation.
fn fold_text(seed: u64, text: &str, window: usize, with_length: bool) -> u64 {
    // Slice on a char boundary: a window that lands mid-codepoint would panic.
    let mut end = text.len().min(window);
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    let h = fnv1a(seed, &text.as_bytes()[..end]);
    if with_length {
        fnv1a(h, &(text.len() as u64).to_le_bytes())
    } else {
        h
    }
}

/// Everything routing knows about *where this request would like to land*.
///
/// Two independent mechanisms, in priority order, because they answer different
/// questions and the better answer is not always available:
///
///   - `key` — an **exact** conversation identity, from the client's
///     `x-aiplane-affinity` header or from a caller that owns the conversation
///     (the chat UI's session id). Nothing beats being told.
///   - `blocks` — the request's prefix chain, matched block-wise against what
///     each replica was recently sent (see [`super::prefix_index`]). Works for
///     clients that identify nothing, and gets two things an exact key cannot:
///     a brand-new session starts on a replica that already holds the shared
///     system prompt, and a conversation whose history was rewritten
///     (compaction, an edited turn) keeps the part that still matches.
///
/// Empty (`key: None`, `blocks: []`) means "no preference", and routing falls
/// back to load — which is the right answer for embeddings, OCR, and anything
/// else with no reusable prefix.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AffinityHint {
    pub key: Option<u64>,
    pub blocks: Vec<u64>,
}

impl AffinityHint {
    /// Whether routing has anything to work with.
    pub fn is_empty(&self) -> bool {
        self.key.is_none() && self.blocks.is_empty()
    }

    /// An exact hint for a caller that owns the conversation — the chat UI,
    /// which has a session id and therefore needs no guessing at all.
    pub fn for_conversation(conversation_id: &str) -> Self {
        Self {
            key: Some(key_for_conversation(conversation_id)),
            blocks: Vec::new(),
        }
    }
}

/// The affinity hint for an incoming request: the client's explicit key if it
/// sent one, plus the prefix chain of the whole prefill either way.
///
/// Both are computed even when the header is present, so an operator can switch
/// between the exact and the approximate mechanism without the request path
/// changing.
pub fn hint_for_request(headers: &rama::http::HeaderMap, body: &Value) -> AffinityHint {
    AffinityHint {
        key: key_for_request_header(headers),
        blocks: super::prefix_index::prefix_chain(&routing_text(body)),
    }
}

/// The prefill, flattened to the text worth matching on: the system prompt,
/// then every message in order.
///
/// Deterministic and **complete** — not just the opening. Keying on the first
/// message alone is a documented mistake: it makes routing decisions that are
/// dominated by system-prompt overlap shared by every session, rather than by
/// the multi-turn conversation prefix that actually drives cache reuse. Roles
/// are included so a user and an assistant turn with identical text can't blur
/// together.
///
/// This is an approximation of the upstream's rendered prompt, and only ever
/// compared against other outputs of this same function, so exact fidelity to
/// any chat template is beside the point.
fn routing_text(body: &Value) -> String {
    let mut out = String::new();
    if let Some(system) = body.get("system").and_then(text_of) {
        out.push_str("system\n");
        out.push_str(&system);
        out.push('\n');
    }
    if let Some(messages) = body.get("messages").and_then(Value::as_array) {
        for m in messages {
            let role = m.get("role").and_then(Value::as_str).unwrap_or("");
            let Some(text) = m.get("content").and_then(text_of) else {
                continue;
            };
            out.push_str(role);
            out.push('\n');
            out.push_str(&text);
            out.push('\n');
        }
    }
    out
}

/// The client's explicit affinity key, if it sent the header.
fn key_for_request_header(headers: &rama::http::HeaderMap) -> Option<u64> {
    headers
        .get(AFFINITY_HEADER)
        .or_else(|| headers.get(LEGACY_AFFINITY_HEADER))
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        // Hashed rather than used raw so the value can be anything the client
        // finds convenient (a uuid, a pid, a branch name) without the router
        // caring, and so it lands in the same key space as a derived one.
        .map(|v| fold_text(FNV_OFFSET, v, v.len(), true))
}

/// The header a client can use to name its own conversation, which overrides
/// everything derived from the body.
///
/// Claude Code sets custom headers once per launch
/// (`ANTHROPIC_CUSTOM_HEADERS="x-aiplane-affinity: <something-unique>"`), so one
/// value per terminal is one value per session — an exact answer instead of an
/// inferred one.
pub const AFFINITY_HEADER: &str = "x-aiplane-affinity";

/// The same header under the name it had before the project was renamed to
/// croit AIplane. Still read, because it is a wire contract already baked into
/// client launch scripts; [`AFFINITY_HEADER`] wins when a request carries both.
/// Neither is forwarded upstream. See `docs/renaming.md`.
pub const LEGACY_AFFINITY_HEADER: &str = "x-gateway-affinity";

/// The affinity key for a caller that already knows its conversation's identity
/// — the gateway's own chat UI, which has a session id.
///
/// Exact rather than derived, so a chat conversation stays on one replica even
/// across the history rewrites (compaction, edited turns) that move a
/// body-derived key.
pub fn key_for_conversation(conversation_id: &str) -> u64 {
    fold_text(FNV_OFFSET, conversation_id, conversation_id.len(), true)
}

/// The affinity key for a chat request body, or `None` when the body carries
/// nothing stable to key on (in which case the caller routes by load, exactly as
/// before).
///
/// Wire-shape independent: both the Anthropic and OpenAI bodies carry the
/// conversation in `messages` with the same roles, and the one place they differ
/// — where the system prompt lives — is exactly the part this does not read.
pub fn key_for_body(body: &Value) -> Option<u64> {
    // The first user turn. Deliberately the *first*, not the last: it is the
    // only user message guaranteed not to change as the conversation grows, and
    // the last one changes every turn — which would re-roll the key every turn
    // and make this strictly worse than round-robin.
    let text = body
        .get("messages")
        .and_then(Value::as_array)?
        .iter()
        .find(|m| m.get("role").and_then(Value::as_str) == Some("user"))
        .and_then(|m| m.get("content"))
        .and_then(text_of)
        .filter(|t| !t.is_empty())?;
    Some(fold_text(FNV_OFFSET, &text, USER_WINDOW, true))
}

/// Flatten a content field to the text worth hashing.
///
/// A plain string is itself; a content-block list contributes the `text` of its
/// text blocks in order. Non-text blocks (images, tool results, documents) are
/// skipped: they carry no stable identity worth keying on, and an image's base64
/// payload would dominate the window for nothing.
fn text_of(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Array(items) => {
            let mut out = String::new();
            for it in items {
                if let Some(t) = it.get("text").and_then(Value::as_str) {
                    out.push_str(t);
                }
            }
            Some(out)
        }
        _ => None,
    }
}

/// Rendezvous ("highest random weight") score of one backend for one key,
/// scaled by the backend's weight.
///
/// Rendezvous hashing rather than a modulo or a hash ring because of what
/// happens when the set changes: draining one backend for maintenance must move
/// *only that backend's* share, leaving every other conversation on the machine
/// that holds its cache. A modulo over the candidate count reshuffles all of
/// them — the exact cache wipe this whole module exists to avoid.
///
/// `weight` enters as an exponent (`score^(1/weight)`), the standard weighted
/// variant: a backend weighted 3 wins roughly three times as many keys as one
/// weighted 1, and the mapping stays stable when weights don't change.
pub fn score(key: u64, backend_name: &str, weight: u32) -> f64 {
    let mixed = fnv1a(
        fold_text(key, backend_name, backend_name.len(), true),
        &key.to_le_bytes(),
    );
    // Map into (0, 1); never exactly 0, so the exponent below is well-defined.
    let unit = (mixed >> 11) as f64 / (1u64 << 53) as f64;
    let unit = unit.max(f64::MIN_POSITIVE);
    unit.powf(1.0 / f64::from(weight.max(1)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The key has to survive a growing conversation: that is the whole point.
    /// Same system prompt + same opening message ⇒ same key, however many turns
    /// have been appended since.
    #[test]
    fn the_key_is_stable_as_a_conversation_grows() {
        let turn1 = json!({
            "system": "You are Claude Code. cwd=/repo",
            "messages": [{"role": "user", "content": "fix the flaky test"}],
        });
        let turn9 = json!({
            "system": "You are Claude Code. cwd=/repo",
            "messages": [
                {"role": "user", "content": "fix the flaky test"},
                {"role": "assistant", "content": "Looking..."},
                {"role": "user", "content": "now run it again"},
                {"role": "assistant", "content": "Done."},
                {"role": "user", "content": "and commit"},
            ],
        });
        assert_eq!(key_for_body(&turn1), key_for_body(&turn9));
    }

    /// Two sessions in the same project share the system prompt entirely, and
    /// must still key differently — otherwise every session in a repo piles onto
    /// one GPU and the second one is pointless.
    #[test]
    fn sessions_sharing_a_system_prompt_still_differ() {
        let a = json!({
            "system": "You are Claude Code. cwd=/repo",
            "messages": [{"role": "user", "content": "fix the flaky test"}],
        });
        let b = json!({
            "system": "You are Claude Code. cwd=/repo",
            "messages": [{"role": "user", "content": "add a migration"}],
        });
        assert_ne!(key_for_body(&a), key_for_body(&b));
    }

    /// The OpenAI shape carries its system prompt inside `messages`; the key
    /// skips it and still finds the first *user* turn, so both wire formats key
    /// identically and just as stably.
    #[test]
    fn the_openai_shape_keys_on_its_first_user_turn() {
        let a = json!({"messages": [
            {"role": "system", "content": "shared preamble"},
            {"role": "user", "content": "one"},
        ]});
        let a_later = json!({"messages": [
            {"role": "system", "content": "shared preamble"},
            {"role": "user", "content": "one"},
            {"role": "assistant", "content": "..."},
            {"role": "user", "content": "two"},
        ]});
        let b = json!({"messages": [
            {"role": "system", "content": "shared preamble"},
            {"role": "user", "content": "different"},
        ]});
        assert_eq!(key_for_body(&a), key_for_body(&a_later));
        assert_ne!(key_for_body(&a), key_for_body(&b));
    }

    /// Content-block form (Anthropic's, and OpenAI's multimodal parts) keys on
    /// the text and ignores everything else — an image's base64 would otherwise
    /// fill the hash window with a payload that says nothing about identity.
    #[test]
    fn content_blocks_key_on_their_text_only() {
        let with_image = json!({
            "system": [{"type": "text", "text": "preamble"}],
            "messages": [{"role": "user", "content": [
                {"type": "text", "text": "look at this"},
                {"type": "image", "source": {"data": "AAAABBBBCCCC"}},
            ]}],
        });
        let text_only = json!({
            "system": "preamble",
            "messages": [{"role": "user", "content": "look at this"}],
        });
        assert_eq!(key_for_body(&with_image), key_for_body(&text_only));
    }

    /// The property that matters most in practice, and that the first cut got
    /// wrong: **nothing in the system prompt may move a live conversation.**
    ///
    /// An agent's system context carries a date, a working directory, a git
    /// snapshot. Any of that entering the key would hop the conversation to a
    /// different replica the moment it changed and discard a warm cache for
    /// nothing. There is no safe window size — nothing specifies where a client
    /// puts the volatile part — so the system prompt is excluded outright.
    #[test]
    fn nothing_in_the_system_prompt_moves_the_key() {
        let monday = json!({
            "system": "You are Claude Code. Today's date is 2026-09-06. git: clean",
            "messages": [{"role": "user", "content": "fix the flaky test"}],
        });
        let tuesday = json!({
            "system": "You are Claude Code. Today's date is 2026-09-07. git: 3 files changed",
            "messages": [{"role": "user", "content": "fix the flaky test"}],
        });
        let none_at_all = json!({
            "messages": [{"role": "user", "content": "fix the flaky test"}],
        });
        assert_eq!(key_for_body(&monday), key_for_body(&tuesday));
        assert_eq!(key_for_body(&monday), key_for_body(&none_at_all));

        // The documented cost, asserted so it stays a deliberate trade and not
        // a surprise: two conversations that open with the identical message
        // share a replica. `x-aiplane-affinity` is the way out.
        let other_project = json!({
            "system": "You are a translation assistant.",
            "messages": [{"role": "user", "content": "fix the flaky test"}],
        });
        assert_eq!(key_for_body(&monday), key_for_body(&other_project));
    }

    /// A multi-byte character straddling the window boundary must not panic —
    /// the window is a byte count and prompts are not ASCII.
    #[test]
    fn the_window_never_splits_a_codepoint() {
        let padded = format!("{}\u{1F600}ff", "a".repeat(USER_WINDOW - 2));
        let body = json!({"messages": [{"role": "user", "content": padded}]});
        assert!(key_for_body(&body).is_some());
    }

    /// A client that names its own session wins over anything inferred — it is
    /// the only identity the gateway is actually *told*, and it survives the
    /// history rewrites (compaction, an edited turn) that shift a derived key.
    #[test]
    fn the_affinity_header_produces_an_exact_key() {
        use rama::http::{HeaderMap, HeaderValue};
        // Two requests that are byte-identical, so anything derived from the
        // body would treat them as one conversation...
        let body = json!({
            "system": "shared",
            "messages": [{"role": "user", "content": "same opening"}],
        });

        // ...are separated by the header, which is the point: two terminals in
        // one repo that happen to start the same way still get their own
        // replica.
        let mut a = HeaderMap::new();
        a.insert(AFFINITY_HEADER, HeaderValue::from_static("session-a"));
        let mut b = HeaderMap::new();
        b.insert(AFFINITY_HEADER, HeaderValue::from_static("session-b"));

        let ha = hint_for_request(&a, &body);
        let hb = hint_for_request(&b, &body);
        assert!(ha.key.is_some() && hb.key.is_some());
        assert_ne!(ha.key, hb.key);

        // Same header, same key, whatever the body says.
        let compacted = json!({
            "system": "shared",
            "messages": [{"role": "user", "content": "[summary of earlier turns]"}],
        });
        assert_eq!(ha.key, hint_for_request(&a, &compacted).key);

        // Blank or absent leaves the exact key unset, so routing falls through
        // to prefix matching rather than to a constant.
        let mut blank = HeaderMap::new();
        blank.insert(AFFINITY_HEADER, HeaderValue::from_static("   "));
        assert_eq!(key_for_request_header(&blank), None);

        // The pre-rename spelling still names a session, and the canonical
        // one wins when both are present.
        let mut legacy = rama::http::HeaderMap::new();
        legacy.insert(
            LEGACY_AFFINITY_HEADER,
            HeaderValue::from_static("session-a"),
        );
        assert_eq!(key_for_request_header(&legacy), key_for_request_header(&a));
        let mut both = rama::http::HeaderMap::new();
        both.insert(AFFINITY_HEADER, HeaderValue::from_static("session-a"));
        both.insert(
            LEGACY_AFFINITY_HEADER,
            HeaderValue::from_static("session-b"),
        );
        assert_eq!(key_for_request_header(&both), key_for_request_header(&a));
        assert!(hint_for_request(&blank, &body).key.is_none());
        assert!(hint_for_request(&HeaderMap::new(), &body).key.is_none());
    }

    /// The routing text covers the **whole** prefill, not just the opening.
    ///
    /// Keying on the first message alone is a documented mistake in this class
    /// of router: decisions end up dominated by the system-prompt overlap that
    /// every session shares, instead of the multi-turn prefix that actually
    /// drives cache reuse. A conversation that has grown must therefore produce
    /// a *longer chain that still starts with the old one* — that is the whole
    /// mechanism by which turn N lands where turn N-1 did.
    #[test]
    fn the_prefix_chain_covers_the_whole_conversation_and_only_grows() {
        let long = "x".repeat(400);
        let turn1 = json!({
            "system": long.clone(),
            "messages": [{"role": "user", "content": long.clone()}],
        });
        let turn2 = json!({
            "system": long.clone(),
            "messages": [
                {"role": "user", "content": long.clone()},
                {"role": "assistant", "content": long.clone()},
                {"role": "user", "content": "and now this"},
            ],
        });
        let h1 = hint_for_request(&rama::http::HeaderMap::new(), &turn1);
        let h2 = hint_for_request(&rama::http::HeaderMap::new(), &turn2);
        assert!(!h1.blocks.is_empty());
        assert!(h2.blocks.len() > h1.blocks.len(), "the chain must extend");
        assert_eq!(
            h1.blocks[..],
            h2.blocks[..h1.blocks.len()],
            "the earlier chain must survive verbatim, or affinity breaks every turn"
        );

        // Two different conversations that share only the system prompt agree
        // over exactly that much and then diverge — which is what lets a new
        // session start on a replica that already holds the shared preamble.
        let other = json!({
            "system": long.clone(),
            "messages": [{"role": "user", "content": "completely different".repeat(30)}],
        });
        let ho = hint_for_request(&rama::http::HeaderMap::new(), &other);
        let shared = h1
            .blocks
            .iter()
            .zip(ho.blocks.iter())
            .take_while(|(a, b)| a == b)
            .count();
        assert!(shared > 0, "the shared system prompt must match");
        assert!(shared < h1.blocks.len(), "and the rest must not");
    }

    /// A body with nothing to key on routes by load instead — no key, rather
    /// than a constant one that would pin all such traffic to one backend.
    #[test]
    fn a_keyless_body_yields_none() {
        assert_eq!(key_for_body(&json!({"model": "m"})), None);
        assert_eq!(key_for_body(&json!({"messages": []})), None);
        assert_eq!(
            key_for_body(&json!({"messages": [{"role": "user", "content": ""}]})),
            None
        );
    }

    /// Rendezvous scoring: deterministic, and a heavier backend scores higher on
    /// more keys.
    #[test]
    fn weighted_scores_are_deterministic_and_favour_weight() {
        assert_eq!(score(42, "gpu0", 1), score(42, "gpu0", 1));
        assert_ne!(score(42, "gpu0", 1), score(42, "gpu1", 1));

        let mut heavy_wins = 0;
        for key in 0..2000u64 {
            if score(key, "big", 3) > score(key, "small", 1) {
                heavy_wins += 1;
            }
        }
        // 3:1 in expectation; assert the direction with generous slack.
        assert!(
            (1200..1800).contains(&heavy_wins),
            "weighting looks wrong: {heavy_wins}/2000"
        );
    }
}
