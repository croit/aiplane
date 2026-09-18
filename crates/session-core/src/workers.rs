// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Per-user registry of in-flight session worker tasks.
//!
//! A single user has at most one worker streaming at a time. The
//! worker writes deltas (content, reasoning, tool calls) to SQLite
//! as they arrive from the upstream — an OpenAI-compatible HTTP
//! stream for the gateway — and emits a `Tick` on its broadcast
//! channel after each DB write.
//! HTTP subscribers (the original `/chat/{id}/messages` POST plus any
//! `GET /chat/{id}/tail` reconnects) re-read the DB on each tick and
//! re-emit the relevant patches.
//!
//! The DB-is-source-of-truth design is the simple-on-purpose answer
//! to the subscribe-vs-write race: subscribers always render whatever's
//! in the row at recv-time, so a missed tick just means the next tick
//! catches up. There's no in-memory snapshot to keep in sync with the
//! DB.
//!
//! Workers run to completion even when every subscriber has dropped —
//! a backgrounded phone reconnects later and finds the finished turn
//! in the DB; a still-streaming worker shows the rest live via tail.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::broadcast;

/// Heartbeat for live subscribers. The worker emits one of these after
/// every DB write so subscribers know to re-read.
///
/// Not `Copy` because [`TurnUpdate::Prompt`] carries an `Arc`. `Clone`
/// is enough — the broadcast channel clones per subscriber, and an `Arc`
/// clone is just a refcount bump.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TurnUpdate {
    Tick,
    Finalized,
    SidebarChanged,
    /// Transient info banner shown alongside the reply (e.g. "vision fallback
    /// activated — image described by {model}").
    InfoMessage(String),
    /// A human-in-loop tool prompt (`ask_user`, `get_user_location`) that the
    /// client renders itself. See [`ToolPromptEvent`].
    Prompt(Arc<ToolPromptEvent>),
}

/// A human-in-loop tool prompt for JSON subscribers: render a prompt,
/// collect the user's answer, POST it to the session API
/// (`/api/v0/me/ask/feedback/{turn_id}` or `/location/feedback/{turn_id}`),
/// and the parked tool picks it up.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ToolPrompt {
    /// The assistant turn the prompt belongs to (also the feedback
    /// endpoint's path parameter).
    pub turn_id: String,
    /// Which tool is asking — drives the client's rendering and the
    /// reply shape.
    pub kind: ToolPromptKind,
    /// The question to put in front of the user. For
    /// [`ToolPromptKind::Location`] this is the fallback explanation shown
    /// next to the browser's own permission prompt.
    pub question: String,
    /// Pre-supplied answers for [`ToolPromptKind::AskUser`]; empty when
    /// the model wants free text.
    pub options: Vec<PromptOption>,
    /// Optional short heading above the question — the model uses it to name
    /// what is being decided when the question alone is ambiguous.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
    /// Whether more than one option may be chosen. Only meaningful alongside
    /// `options`.
    #[serde(default)]
    pub multi_select: bool,
}

/// One offered answer: what the button says, and optionally what picking it
/// means.
///
/// The two travel as separate fields rather than one pre-joined string. They
/// were joined once (`"Postgres — the primary"`), which cost twice: the card
/// drew a row of buttons whose labels were whole sentences, and that same
/// sentence is what came back to the model as the user's answer. The label is
/// the answer; the description is context for the human choosing.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PromptOption {
    /// Short answer text — the button, and what the tool reports as chosen.
    pub label: String,
    /// One line on what choosing this means. Rendered under the label,
    /// as markdown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Optional worked example of what this option produces — an ASCII
    /// layout, a code snippet, a small diagram — shown inside the option.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<PromptPreview>,
}

/// A block of example content attached to an option.
///
/// Typed rather than sniffed. The two kinds render through completely
/// different paths — verbatim monospace text versus a sanitised SVG document
/// — and which one the client is looking at decides how much of the content
/// it is willing to trust. Guessing from the first characters would put that
/// decision in the hands of whoever wrote the string.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PromptPreview {
    pub kind: PreviewKind,
    pub content: String,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewKind {
    /// Preformatted text: ASCII diagrams, table sketches, code. Rendered
    /// verbatim in a monospace block, never parsed.
    Text,
    /// An inline SVG document, sanitised before it reaches the page.
    Svg,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolPromptKind {
    AskUser,
    Location,
}

/// Show or tear down a [`ToolPrompt`] on the structured chat event stream.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ToolPromptEvent {
    Show(ToolPrompt),
    /// Remove the prompt again — answered, timed out, or the turn ended.
    Hide {
        turn_id: String,
    },
}

/// A mid-turn interjection: something the user typed while the turn was
/// already running, meant for *this* turn rather than the next one.
///
/// Carries its DB id because the outcome is recorded on the row — the
/// transcript has to be able to say whether the running turn actually saw the
/// note or whether it arrived too late and was re-sent as an ordinary message.
/// A note the model acted on but that the history cannot account for is the
/// same class of bug as a dropped parameter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SteerNote {
    /// `chat_turn_steers.id`.
    pub id: String,
    /// What the user typed.
    pub text: String,
}

/// The queue of interjections one running turn has been handed but not yet
/// folded into its prompt.
///
/// A handle rather than a field so the driver can hold it without holding the
/// whole worker entry: `SessionContext` carries a clone, the HTTP handler
/// pushes through the registry's clone, and both see the same queue.
#[derive(Clone, Default)]
pub struct SteerInbox {
    inner: Arc<Mutex<Vec<SteerNote>>>,
}

impl SteerInbox {
    /// Queue an interjection for the running turn.
    ///
    /// Whether it is ever *delivered* is not decided here: the driver drains
    /// the queue at the top of each tool round, so a note that arrives while
    /// the model is writing its closing answer stays queued and is never
    /// taken. The caller is responsible for telling the user which happened.
    pub fn push(&self, note: SteerNote) {
        self.inner.lock().unwrap().push(note);
    }

    /// Take every queued interjection, leaving the queue empty. Called at a
    /// round boundary, where they go into the prompt.
    pub fn take(&self) -> Vec<SteerNote> {
        std::mem::take(&mut *self.inner.lock().unwrap())
    }

    /// Drop one interjection before it is folded in — the user discarded it.
    /// Reports whether it was still here to drop.
    ///
    /// The answer is the whole point: a note that is no longer in the queue
    /// has already been drained into a prompt, and calling that "discarded"
    /// would label as thrown-away a sentence the model demonstrably read.
    /// Settling the row without asking is what produced exactly that.
    pub fn remove(&self, id: &str) -> bool {
        let mut notes = self.inner.lock().unwrap();
        let before = notes.len();
        notes.retain(|note| note.id != id);
        notes.len() != before
    }
}

/// One live session worker, indexed by `(user id, session id)` in
/// `SessionWorkers`. Holds the cancel flag the worker polls between upstream
/// chunks, the queue of mid-turn interjections it drains between rounds, and
/// the broadcast channel subscribers attach to.
#[derive(Clone)]
pub struct ActiveWorker {
    /// DB id of the assistant turn this worker is filling in. Used by
    /// HTTP handlers to confirm a /tail attach is for the right turn
    /// (e.g. user navigated to a different session mid-stream).
    pub turn_id: String,
    /// Session this worker belongs to. Same purpose as `turn_id`.
    pub session_id: String,
    /// Worker polls this flag between upstream chunks. `POST
    /// /chat/{id}/cancel` flips it; the stop button on the composer
    /// flips it; `register()` flips the *previous* worker's flag when
    /// a fresh submit lands.
    pub cancel: Arc<AtomicBool>,
    /// Broadcast back to all attached subscribers. Capacity is
    /// generous — bursts of deltas land in tight loops and we don't
    /// want lagged subscribers (a slow phone over LTE) to miss frames.
    pub broadcast: broadcast::Sender<TurnUpdate>,
    /// Interjections waiting to be folded into the running turn, oldest
    /// first. The HTTP handler pushes; the driver drains at a round boundary.
    pub steers: SteerInbox,
}

/// Result of trying to register a fresh worker.
pub enum RegisterOutcome {
    /// Nothing was running in this conversation and the user was under
    /// their ceiling — caller may spawn one with `worker`.
    Registered { worker: ActiveWorker },
    /// A worker was already running *in this conversation*. Caller should
    /// refuse the new submit (return 409 / toast). The existing worker is
    /// returned so the caller can decide whether to subscribe to it instead.
    ///
    /// This one is not configurable and never will be: two workers filling
    /// the same transcript would interleave their writes into it.
    Busy { existing: ActiveWorker },
    /// Other conversations of this user are already using every parallel
    /// slot the operator allowed. Distinct from `Busy` because the remedy is
    /// different — wait for *another* chat, or raise the limit — and the
    /// message the user gets should say so rather than claim this
    /// conversation is busy when it is idle.
    AtCapacity { running: usize, limit: usize },
}

/// Capacity of the per-worker broadcast channel. ~256 frames buffered
/// before slow subscribers start seeing `RecvError::Lagged`. Each
/// frame is `TurnUpdate` (16-byte enum) so the buffer is < 4 KB total.
const BROADCAST_CAPACITY: usize = 256;

/// How a worker is addressed: one user's one conversation.
///
/// The registry used to key on the user alone, which made "one turn at a
/// time" a property of the *person* rather than of the conversation — so
/// asking something in a second chat while the first was still thinking was
/// refused, with nothing technical behind the refusal. Keying on the pair
/// keeps the invariant that actually matters (one writer per transcript) and
/// lets the operator decide how many conversations may run at once.
type WorkerKey = (String, String);

/// A worker has just been registered for this conversation.
///
/// The one thing the per-turn broadcast cannot carry: it belongs to a worker
/// that did not exist yet. A viewer attached to a conversation whose answer
/// has not started — a message waiting for a free slot — has nothing to listen
/// to until this fires.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkerStarted {
    pub user_id: String,
    pub session_id: String,
    pub turn_id: String,
}

/// Capacity of the registry-wide start channel. Starts are rare compared to
/// deltas (one per turn), and a lagged subscriber only means one viewer
/// re-reads a moment later, so a small buffer is plenty.
const STARTS_CAPACITY: usize = 64;

/// (user id, session id) → ActiveWorker. Wrapped in a Mutex (not RwLock)
/// because every access is short and we want strict order between the
/// capacity check and the insert in `register`.
///
/// Single-tenant callers can pass a constant per-process id for
/// `user_id` — the registry doesn't care what the string contains.
pub struct SessionWorkers {
    inner: Mutex<HashMap<WorkerKey, ActiveWorker>>,
    /// Announces every worker this registry creates. See [`WorkerStarted`].
    starts: broadcast::Sender<WorkerStarted>,
}

impl Default for SessionWorkers {
    fn default() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            starts: broadcast::channel(STARTS_CAPACITY).0,
        }
    }
}

impl SessionWorkers {
    /// Try to register a new worker for this user's conversation.
    ///
    /// Three outcomes, three different situations:
    ///
    /// * the conversation already has a worker → `Busy`, always, whatever
    ///   the limit says. Callers refuse the submit; they don't quietly
    ///   cancel. (The old `CancelRegistry::register` *always* cancelled the
    ///   prior worker and inserted the new one, which is what caused the
    ///   duplication-on-retry bug: a client retry after a network blip raced
    ///   a brand-new worker against the still-finishing previous one.)
    /// * the user's other conversations already fill `max_parallel` slots →
    ///   `AtCapacity`, reporting both numbers so the message can name the
    ///   limit.
    /// * otherwise a fresh worker, registered and returned.
    ///
    /// `max_parallel` is passed per call rather than held on the registry so
    /// that session-core stays free of the gateway's config: the operator's
    /// current setting is read at submit time, which also means raising the
    /// limit takes effect on the next submit rather than on restart. Values
    /// below 1 are treated as 1 — a ceiling of zero would refuse every turn,
    /// and silently doing nothing is a better failure than a gateway that
    /// cannot chat.
    pub fn register(
        &self,
        user_id: &str,
        turn_id: &str,
        session_id: &str,
        max_parallel: usize,
    ) -> RegisterOutcome {
        let mut g = self.inner.lock().unwrap();
        let key = (user_id.to_string(), session_id.to_string());
        if let Some(existing) = g.get(&key) {
            return RegisterOutcome::Busy {
                existing: existing.clone(),
            };
        }
        let limit = max_parallel.max(1);
        let running = g.keys().filter(|(uid, _)| uid == user_id).count();
        if running >= limit {
            return RegisterOutcome::AtCapacity { running, limit };
        }
        let (broadcast, _) = broadcast::channel(BROADCAST_CAPACITY);
        let worker = ActiveWorker {
            turn_id: turn_id.to_string(),
            session_id: session_id.to_string(),
            cancel: Arc::new(AtomicBool::new(false)),
            broadcast,
            steers: SteerInbox::default(),
        };
        g.insert(key, worker.clone());
        // Announced while the lock is held, so a subscriber that has already
        // looked and found nothing cannot miss the start that happened in
        // between: it sees either the worker in the map or this frame.
        let _ = self.starts.send(WorkerStarted {
            user_id: user_id.to_string(),
            session_id: session_id.to_string(),
            turn_id: turn_id.to_string(),
        });
        RegisterOutcome::Registered { worker }
    }

    /// How many turns are running right now — what a shutdown waits on.
    ///
    /// A turn outlives the HTTP connection that started it (that is what makes
    /// resume-on-reconnect work), so draining connections says nothing about
    /// whether work is still in flight. This is the number that does.
    pub fn active_count(&self) -> usize {
        self.inner.lock().unwrap().len()
    }

    /// Ask every running turn to stop at its next checkpoint.
    ///
    /// Used on shutdown once the drain deadline is in sight: a turn that
    /// notices the flag finalises its own row, which is strictly better than
    /// being killed mid-write and swept as `errored` on the next boot.
    pub fn cancel_all(&self) -> usize {
        let g = self.inner.lock().unwrap();
        for w in g.values() {
            w.cancel.store(true, Ordering::SeqCst);
        }
        g.len()
    }

    /// Hand out this conversation's worker (if any) — used by the tail
    /// handler to attach to a still-running stream.
    pub fn get(&self, user_id: &str, session_id: &str) -> Option<ActiveWorker> {
        self.inner
            .lock()
            .unwrap()
            .get(&(user_id.to_string(), session_id.to_string()))
            .cloned()
    }

    /// Listen for workers being registered.
    ///
    /// Subscribe *before* checking whether a conversation has one: the two
    /// steps have a gap between them, and a worker that starts inside it would
    /// otherwise be missed by exactly the viewer waiting for it.
    pub fn subscribe_starts(&self) -> broadcast::Receiver<WorkerStarted> {
        self.starts.subscribe()
    }

    /// How many of this user's conversations are streaming right now.
    ///
    /// The ceiling check in [`Self::register`] is the reason this exists; it
    /// is also what a test waits on to know a worker has cleared.
    pub fn running_for_user(&self, user_id: &str) -> usize {
        self.inner
            .lock()
            .unwrap()
            .keys()
            .filter(|(uid, _)| uid == user_id)
            .count()
    }

    /// Which of this user's conversations are being answered right now.
    ///
    /// The scheduler's other half: `running_for_user` says whether there is
    /// room, this says where not to look. Returned as a list rather than
    /// checked one conversation at a time because the claim query takes it as
    /// an exclusion set — the whole point is to pick a startable turn in one
    /// statement, not to ask per candidate.
    pub fn sessions_for_user(&self, user_id: &str) -> Vec<String> {
        self.inner
            .lock()
            .unwrap()
            .keys()
            .filter(|(uid, _)| uid == user_id)
            .map(|(_, session_id)| session_id.clone())
            .collect()
    }

    /// Flip the cancel flag on this conversation's worker (if any).
    /// Used by `POST /chat/{id}/cancel`. No-op when no worker is
    /// active.
    pub fn cancel(&self, user_id: &str, session_id: &str) {
        if let Some(w) = self
            .inner
            .lock()
            .unwrap()
            .get(&(user_id.to_string(), session_id.to_string()))
        {
            w.cancel.store(true, Ordering::SeqCst);
        }
    }

    /// Remove the worker entry iff it's the same one we registered.
    /// Matching by `Arc::ptr_eq` on the cancel flag keeps a slow
    /// finalising worker from yanking a newer worker's entry —
    /// belt-and-braces, since `register` now refuses to insert when a
    /// worker exists.
    pub fn clear(&self, user_id: &str, worker: &ActiveWorker) {
        let mut g = self.inner.lock().unwrap();
        let key = (user_id.to_string(), worker.session_id.clone());
        if let Some(current) = g.get(&key)
            && Arc::ptr_eq(&current.cancel, &worker.cancel)
        {
            g.remove(&key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Today's default: one turn at a time per user, across every chat.
    const SERIAL: usize = 1;

    fn registered(r: &SessionWorkers, user: &str, turn: &str, session: &str) -> ActiveWorker {
        match r.register(user, turn, session, SERIAL) {
            RegisterOutcome::Registered { worker } => worker,
            _ => panic!("expected a fresh registration"),
        }
    }

    /// What a shutdown waits on. A turn outlives its HTTP connection, so
    /// draining connections proves nothing about work in flight — this count
    /// is the only thing that does.
    #[test]
    fn active_count_tracks_registered_turns() {
        let r = SessionWorkers::default();
        assert_eq!(r.active_count(), 0);

        let w1 = registered(&r, "u1", "t1", "s1");
        registered(&r, "u2", "t2", "s2");
        assert_eq!(r.active_count(), 2);

        r.clear("u1", &w1);
        assert_eq!(r.active_count(), 1, "a finished turn stops being counted");
    }

    /// At the drain deadline every straggler is asked to stop, so it finalises
    /// its own row instead of being killed mid-write and swept as an orphan on
    /// the next boot.
    #[test]
    fn cancel_all_flags_every_running_turn() {
        let r = SessionWorkers::default();
        let a = registered(&r, "u1", "t1", "s1");
        let b = registered(&r, "u2", "t2", "s2");
        assert!(!a.cancel.load(Ordering::SeqCst));
        assert!(!b.cancel.load(Ordering::SeqCst));

        assert_eq!(r.cancel_all(), 2, "reports how many it signalled");
        assert!(a.cancel.load(Ordering::SeqCst));
        assert!(b.cancel.load(Ordering::SeqCst));
    }

    /// Cancelling nothing is not an error — the common shutdown, on an idle
    /// gateway, must not log or behave as if work were lost.
    #[test]
    fn cancel_all_on_an_idle_registry_is_a_no_op() {
        let r = SessionWorkers::default();
        assert_eq!(r.cancel_all(), 0);
        assert_eq!(r.active_count(), 0);
    }

    #[test]
    fn register_returns_registered_when_empty() {
        let r = SessionWorkers::default();
        let outcome = r.register("u1", "turn-1", "sess-1", SERIAL);
        assert!(matches!(outcome, RegisterOutcome::Registered { .. }));
    }

    /// Two workers on one transcript would interleave their writes into it,
    /// so this refusal holds no matter how generous the parallel limit is.
    #[test]
    fn register_returns_busy_for_the_same_conversation_even_with_slots_free() {
        let r = SessionWorkers::default();
        let _first = r.register("u1", "turn-1", "sess-1", 8);
        let outcome = r.register("u1", "turn-2", "sess-1", 8);
        match outcome {
            RegisterOutcome::Busy { existing } => assert_eq!(existing.turn_id, "turn-1"),
            _ => panic!("expected Busy"),
        }
    }

    /// The point of the re-key: a second conversation is not the first one.
    #[test]
    fn a_second_conversation_runs_in_parallel_when_the_limit_allows() {
        let r = SessionWorkers::default();
        let _first = r.register("u1", "t1", "s1", 2);
        let outcome = r.register("u1", "t2", "s2", 2);
        assert!(
            matches!(outcome, RegisterOutcome::Registered { .. }),
            "a different chat gets its own worker"
        );
        assert_eq!(r.running_for_user("u1"), 2);
        assert_eq!(r.running_for_user("nobody"), 0);
    }

    /// Over the ceiling the refusal must be distinguishable from `Busy`: this
    /// conversation is idle, somebody else's slot is the problem, and the
    /// message the user reads should say which.
    #[test]
    fn register_reports_capacity_with_both_numbers() {
        let r = SessionWorkers::default();
        r.register("u1", "t1", "s1", 2);
        r.register("u1", "t2", "s2", 2);
        match r.register("u1", "t3", "s3", 2) {
            RegisterOutcome::AtCapacity { running, limit } => {
                assert_eq!((running, limit), (2, 2));
            }
            _ => panic!("expected AtCapacity"),
        }
    }

    /// The default keeps today's behaviour exactly: a second chat waits.
    #[test]
    fn a_limit_of_one_keeps_the_old_serial_behaviour() {
        let r = SessionWorkers::default();
        r.register("u1", "t1", "s1", SERIAL);
        assert!(matches!(
            r.register("u1", "t2", "s2", SERIAL),
            RegisterOutcome::AtCapacity { limit: 1, .. }
        ));
    }

    /// A misconfigured zero must not brick chat; it reads as "one".
    #[test]
    fn a_ceiling_of_zero_is_read_as_one() {
        let r = SessionWorkers::default();
        assert!(matches!(
            r.register("u1", "t1", "s1", 0),
            RegisterOutcome::Registered { .. }
        ));
    }

    /// One user's ceiling is their own — a busy colleague must not block you.
    #[test]
    fn the_ceiling_is_per_user() {
        let r = SessionWorkers::default();
        r.register("u1", "t1", "s1", 1);
        assert!(matches!(
            r.register("u2", "t2", "s2", 1),
            RegisterOutcome::Registered { .. }
        ));
    }

    /// A viewer attached to a conversation with nothing running has no
    /// per-turn channel to listen to — the worker does not exist yet. This is
    /// what tells them it now does, instead of them asking again every few
    /// seconds.
    #[test]
    fn registering_a_worker_announces_it() {
        let r = SessionWorkers::default();
        let mut starts = r.subscribe_starts();
        registered(&r, "u1", "t1", "s1");

        assert_eq!(
            starts.try_recv().unwrap(),
            WorkerStarted {
                user_id: "u1".into(),
                session_id: "s1".into(),
                turn_id: "t1".into(),
            }
        );
        // A refused registration is not a start.
        r.register("u1", "t2", "s1", SERIAL);
        assert!(starts.try_recv().is_err());
    }

    /// What the scheduler asks before claiming a waiting turn: where a worker
    /// already is, so it does not put a second one on the same transcript.
    #[test]
    fn sessions_for_user_lists_only_that_users_busy_conversations() {
        let r = SessionWorkers::default();
        registered(&r, "u1", "t1", "s1");
        let RegisterOutcome::Registered { worker } = r.register("u1", "t2", "s2", 2) else {
            unreachable!()
        };
        registered(&r, "u2", "t3", "s3");

        let mut mine = r.sessions_for_user("u1");
        mine.sort();
        assert_eq!(mine, ["s1", "s2"]);
        assert!(r.sessions_for_user("nobody").is_empty());

        r.clear("u1", &worker);
        assert_eq!(r.sessions_for_user("u1"), ["s1"]);
    }

    #[test]
    fn cancel_flips_the_flag() {
        let r = SessionWorkers::default();
        let worker = registered(&r, "u1", "t", "s");
        assert!(!worker.cancel.load(Ordering::SeqCst));
        r.cancel("u1", "s");
        assert!(worker.cancel.load(Ordering::SeqCst));
    }

    /// Cancelling names a conversation, so the user's *other* running chat
    /// keeps going — that is the whole point of running two.
    #[test]
    fn cancel_only_touches_the_named_conversation() {
        let r = SessionWorkers::default();
        let a = registered(&r, "u1", "t1", "s1");
        let RegisterOutcome::Registered { worker: b } = r.register("u1", "t2", "s2", 2) else {
            unreachable!()
        };
        r.cancel("u1", "s1");
        assert!(a.cancel.load(Ordering::SeqCst));
        assert!(!b.cancel.load(Ordering::SeqCst), "s2 keeps streaming");
    }

    #[test]
    fn cancel_on_unknown_user_is_a_noop() {
        let r = SessionWorkers::default();
        r.cancel("nobody", "nowhere"); // must not panic
    }

    #[test]
    fn get_is_scoped_to_the_conversation() {
        let r = SessionWorkers::default();
        registered(&r, "u1", "t1", "s1");
        assert!(r.get("u1", "s1").is_some());
        assert!(
            r.get("u1", "s2").is_none(),
            "a worker in another chat is not this chat's worker"
        );
    }

    #[test]
    fn clear_removes_only_matching_worker() {
        let r = SessionWorkers::default();
        let first = registered(&r, "u1", "t1", "s");
        // Pretend a second register raced through (it wouldn't, given
        // `Busy`, but exercise the ptr_eq guard anyway).
        r.clear("u1", &first);
        assert!(r.get("u1", "s").is_none());

        let second = registered(&r, "u1", "t2", "s");
        r.clear("u1", &first); // wrong token: must not remove second
        assert!(r.get("u1", "s").is_some());
        r.clear("u1", &second);
        assert!(r.get("u1", "s").is_none());
    }

    #[test]
    fn broadcast_round_trips_tick_then_finalized() {
        let r = SessionWorkers::default();
        let worker = registered(&r, "u1", "t", "s");
        let mut rx = worker.broadcast.subscribe();
        worker.broadcast.send(TurnUpdate::Tick).unwrap();
        worker.broadcast.send(TurnUpdate::Finalized).unwrap();
        assert_eq!(rx.try_recv().unwrap(), TurnUpdate::Tick);
        assert_eq!(rx.try_recv().unwrap(), TurnUpdate::Finalized);
    }

    /// Interjections queue in arrival order and are handed over exactly once:
    /// a note delivered twice would read to the model as the user repeating
    /// themselves, which is its own kind of wrong answer.
    #[test]
    fn steers_drain_in_order_and_only_once() {
        let r = SessionWorkers::default();
        let worker = registered(&r, "u1", "t", "s");
        worker.steers.push(SteerNote {
            id: "n1".into(),
            text: "use metric units".into(),
        });
        worker.steers.push(SteerNote {
            id: "n2".into(),
            text: "and keep it short".into(),
        });
        let taken = worker.steers.take();
        assert_eq!(
            taken.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(),
            ["n1", "n2"]
        );
        assert!(worker.steers.take().is_empty(), "drained exactly once");
    }

    /// Removing reports whether the note was still there — the caller needs
    /// that to know whether "discarded" is the truth.
    #[test]
    fn removing_a_steer_says_whether_it_was_still_queued() {
        let r = SessionWorkers::default();
        let worker = registered(&r, "u1", "t", "s");
        worker.steers.push(SteerNote {
            id: "n1".into(),
            text: "never mind".into(),
        });

        assert!(worker.steers.remove("n1"), "it was queued");
        assert!(!worker.steers.remove("n1"), "and now it is not");
        assert!(!worker.steers.remove("never-existed"));
    }

    /// The queue rides on the handle's clone, not on a copy of it — the HTTP
    /// handler pushes through one clone and the driver drains through
    /// another.
    #[test]
    fn steers_are_shared_across_handle_clones() {
        let r = SessionWorkers::default();
        let worker = registered(&r, "u1", "t", "s");
        let from_registry = r.get("u1", "s").expect("registered");
        from_registry.steers.push(SteerNote {
            id: "n1".into(),
            text: "context".into(),
        });
        assert_eq!(worker.steers.take().len(), 1);
    }
}
