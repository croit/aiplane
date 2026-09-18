/**
 * The composer's outbox: what the user typed while a turn was running.
 *
 * The composer no longer locks during a turn, which means there is now a gap
 * between "typed" and "accepted by the server". Anything in that gap lives
 * here — in one plain, framework-free module so the rules are unit-testable
 * and the Svelte component stays a renderer.
 *
 * Two things end up in the queue:
 *
 * 1. Messages the user chose to send next. They go out, oldest first, as soon
 *    as the running turn finishes.
 * 2. Interjections that never reached the model. The turn ended before a
 *    round could carry them, so they come back as ordinary messages —
 *    carrying `redeemSteers` so the server can settle the rows they stand in
 *    for, and so two open tabs cannot both send them.
 *
 * Nothing here talks to the network. The caller owns sending, and hands back
 * success or failure; that split is what lets a failed send stay visible in
 * the queue with its reason instead of disappearing.
 */

import type { LiveTurn } from './chat-protocol';

export interface QueuedMessage {
	/** Client-side id — the queue is local, so this never leaves the browser. */
	id: string;
	text: string;
	/**
	 * Attachments typed with the message. Not persisted across a reload:
	 * `File` handles cannot be serialised, and re-sending a message without
	 * the file it referred to is worse than asking for it again.
	 */
	files: File[];
	/** Ids of the interjections this entry re-sends, if any. */
	redeemSteers: string[];
	/**
	 * Set on an entry restored from storage that was typed with attachments.
	 *
	 * The files could not be stored (a `File` handle cannot be serialised), so
	 * sending this now would send "summarise the attached report" with no
	 * report — a message whose answer is guaranteed to be wrong. A held entry
	 * keeps its place and its text, is skipped by the drain, and waits for the
	 * user to re-attach or discard it.
	 */
	held?: boolean;
	/** Why the last attempt failed, when one did. Cleared on retry. */
	error?: string;
}

let counter = 0;

/** A queue entry from what the composer currently holds. */
export function draftEntry(text: string, files: File[] = []): QueuedMessage {
	counter += 1;
	return { id: `q${counter}-${Date.now()}`, text, files, redeemSteers: [] };
}

/** A queue entry standing in for an interjection the turn never carried. */
export function steerEntry(steerId: string, text: string): QueuedMessage {
	counter += 1;
	return { id: `s${counter}-${steerId}`, text, files: [], redeemSteers: [steerId] };
}

export function removeEntry(queue: QueuedMessage[], id: string): QueuedMessage[] {
	return queue.filter((entry) => entry.id !== id);
}

/**
 * Move an entry one place towards the front (`-1`) or back (`1`).
 *
 * Order is the whole meaning of a queue of instructions — "first do X, then
 * Y" is not the same request as the reverse — so it has to be correctable
 * after the fact, not only at typing time.
 */
export function moveEntry(queue: QueuedMessage[], id: string, by: -1 | 1): QueuedMessage[] {
	const from = queue.findIndex((entry) => entry.id === id);
	if (from < 0) return queue;
	const to = from + by;
	if (to < 0 || to >= queue.length) return queue;
	const next = [...queue];
	const [moved] = next.splice(from, 1);
	next.splice(to, 0, moved);
	return next;
}

/**
 * Fold every interjection that never reached the model back into the queue.
 *
 * Called when a turn finalises. Two rules, both of which showed themselves
 * only when driving the real thing:
 *
 * - They go to the **front**. An interjection was typed *before* whatever
 *   else is waiting and it is about the answer that just ended, so sending it
 *   after two later messages reads as a correction to the wrong thing.
 * - Entries already holding a note are left alone. A `pending` row stays
 *   pending until the send settles it, so without the check every finalize
 *   would queue the same sentence again.
 */
export function withUndeliveredSteers(
	queue: QueuedMessage[],
	turns: LiveTurn[]
): QueuedMessage[] {
	const claimed = new Set(queue.flatMap((entry) => entry.redeemSteers));
	const additions = turns
		.flatMap((turn) => turn.steers)
		.filter((steer) => steer.status === 'pending' && !claimed.has(steer.id))
		.map((steer) => steerEntry(steer.id, steer.text));
	return additions.length === 0 ? queue : [...additions, ...queue];
}

/**
 * What a reload should bring back: the text of each entry, in order.
 *
 * Attachments and the local ids are dropped deliberately — a `File` cannot be
 * serialised, and an id that outlives the page it was minted on buys nothing.
 * That an entry *had* attachments is recorded, because it decides whether the
 * restored entry may be sent unattended.
 */
export function serializeQueue(queue: QueuedMessage[]): string {
	return JSON.stringify(
		queue.map((entry) => ({
			text: entry.text,
			redeemSteers: entry.redeemSteers,
			...(entry.files.length > 0 || entry.held ? { held: true } : {})
		}))
	);
}

/**
 * Restore a serialised queue, ignoring anything that is not the shape written
 * by `serializeQueue` — the payload comes from browser storage, which another
 * tab, an extension or a stale release may have written.
 */
export function deserializeQueue(raw: string | null): QueuedMessage[] {
	if (!raw) return [];
	let parsed: unknown;
	try {
		parsed = JSON.parse(raw);
	} catch {
		return [];
	}
	if (!Array.isArray(parsed)) return [];
	return parsed
		.filter(
			(entry): entry is { text: string; redeemSteers?: unknown; held?: unknown } =>
				typeof entry === 'object' && entry !== null && typeof (entry as { text?: unknown }).text === 'string'
		)
		.map((entry) => {
			const ids = Array.isArray(entry.redeemSteers)
				? entry.redeemSteers.filter((id): id is string => typeof id === 'string')
				: [];
			counter += 1;
			return {
				id: `r${counter}-${Date.now()}`,
				text: entry.text,
				files: [],
				redeemSteers: ids,
				...(entry.held === true ? { held: true } : {})
			};
		})
		.filter((entry) => entry.text.trim().length > 0 || entry.redeemSteers.length > 0);
}

/**
 * The next entry the drain may send: the first one that is not held.
 *
 * Held entries keep their place rather than being skipped over permanently —
 * they are waiting for the user, not for the server — so a queue whose first
 * entry lost its attachment still sends the rest.
 */
export function nextSendable(queue: QueuedMessage[]): QueuedMessage | undefined {
	return queue.find((entry) => !entry.held);
}

/** Storage key for one conversation's queue. Scoped per conversation. */
export function queueStorageKey(sessionId: string): string {
	return `gw:composer-queue:${sessionId}`;
}
