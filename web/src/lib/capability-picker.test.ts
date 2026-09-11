import assert from 'node:assert/strict';
import test from 'node:test';
import type { ChatCapability } from './api.ts';
import { capabilityCounts, filterCapabilities } from './capability-picker.ts';

const capabilities: ChatCapability[] = [
	{ key: 'search', kind: 'tool', title: 'Search web', description: 'Find current information', group: 'Web & Network', order: 1, state: 'on', can_disable: true, icon: null },
	{ key: 'fetch', kind: 'tool', title: 'Fetch URL', description: 'Read a web page', group: 'Web & Network', order: 2, state: 'auto', can_disable: true, icon: null },
	{ key: 'memory', kind: 'tool', title: 'Memory', description: 'Recall saved context', group: 'Memory', order: 3, state: 'off', can_disable: true, icon: null }
];

test('tool filtering combines category, state, and text', () => {
	assert.deepEqual(
		filterCapabilities(capabilities, { group: 'Web & Network', state: 'auto', query: 'page' }).map((row) => row.key),
		['fetch']
	);
});

test('tool search matches titles and descriptions without case sensitivity', () => {
	assert.deepEqual(
		filterCapabilities(capabilities, { group: null, state: 'all', query: 'CURRENT' }).map((row) => row.key),
		['search']
	);
});

test('tool state counts support the dialog summary and filters', () => {
	assert.deepEqual(capabilityCounts(capabilities), { on: 1, auto: 1, off: 1 });
});
