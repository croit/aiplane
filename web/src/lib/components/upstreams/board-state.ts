import type { Backend } from '#lib/upstreams.js';

export type BoardTone = 'green' | 'amber' | 'red' | 'gray';

/** Which state wins when a backend appears in several pools. */
const TONE_RANK: Record<BoardTone, number> = { green: 0, gray: 1, amber: 2, red: 3 };

/**
 * One count per backend, at its worst state across the pools it serves.
 *
 * The rows are per (pool, backend) — an OpenRouter account shows in seven
 * bands — so counting rows would inflate the header. Counting `backend.live`
 * would hide a pool whose instance is down. Worst wins: a backend that is
 * failing anywhere is not "on time".
 */
export function summaryCounts(
	entries: readonly { backend: string; tone: BoardTone }[]
): Record<BoardTone, number> {
	const worst = new Map<string, BoardTone>();
	for (const { backend, tone } of entries) {
		const current = worst.get(backend);
		if (!current || TONE_RANK[tone] > TONE_RANK[current]) worst.set(backend, tone);
	}
	const counts: Record<BoardTone, number> = { green: 0, amber: 0, red: 0, gray: 0 };
	for (const tone of worst.values()) counts[tone] += 1;
	return counts;
}

/**
 * The board's state lamp, shared by the row and the header summary so they
 * can never disagree: what is wrong, in the order an operator checks it
 * (never probed, rejected key, drained, unreachable, saturated, healthy).
 */
export function boardTone(backend: Backend, live: Backend['live'] = backend.live): BoardTone {
	if (!live) return 'gray';
	if (live.auth_failed) return 'amber';
	if (!live.enabled) return 'gray';
	if (!live.healthy) return 'red';
	const max = live.max_inflight || backend.max_inflight;
	if (max > 0 && live.inflight / max >= 0.85) return 'amber';
	return 'green';
}
