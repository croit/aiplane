import type { Backend } from '#lib/upstreams.js';

export type BoardTone = 'green' | 'amber' | 'red' | 'gray';

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
