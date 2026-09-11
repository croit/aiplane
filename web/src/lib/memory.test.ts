import assert from 'node:assert/strict';
import test from 'node:test';

import { groupMemories, MEMORY_KINDS, type Memory } from './memory.ts';

test('memories are grouped in the product-defined order without changing row order', () => {
	const memories: Memory[] = [
		{ id: 'fact-1', kind: 'fact', content: 'One', created_at: '2026-01-01' },
		{ id: 'pref-1', kind: 'preference', content: 'Two', created_at: '2026-01-02' },
		{ id: 'fact-2', kind: 'fact', content: 'Three', created_at: '2026-01-03' }
	];

	assert.deepEqual(MEMORY_KINDS, ['preference', 'project', 'fact']);
	assert.deepEqual(groupMemories(memories), {
		preference: [memories[1]],
		project: [],
		fact: [memories[0], memories[2]]
	});
});
