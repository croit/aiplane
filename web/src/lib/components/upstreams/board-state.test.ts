import test from 'node:test';
import assert from 'node:assert/strict';

import { summaryCounts } from './board-state.ts';

test('a backend counts once, at its worst state across the pools it serves', () => {
	assert.deepEqual(
		summaryCounts([
			{ backend: 'shared', tone: 'green' },
			{ backend: 'shared', tone: 'red' },
			{ backend: 'other', tone: 'green' },
			{ backend: 'drained', tone: 'gray' },
			{ backend: 'busy', tone: 'amber' }
		]),
		{ green: 1, amber: 1, red: 1, gray: 1 }
	);
});

test('an empty board counts nothing', () => {
	assert.deepEqual(summaryCounts([]), { green: 0, amber: 0, red: 0, gray: 0 });
});
