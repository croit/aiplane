import assert from 'node:assert/strict';
import test from 'node:test';
import type { CapabilityItem, ChatCapability } from './api.ts';
import { capabilityCounts, capabilityGroups, descriptionOf, editLink, rankCapabilities, searchCapabilities } from './capability-picker.ts';

const capabilities: ChatCapability[] = [
	{ key: 'search', kind: 'tool', title: 'Search web', description: 'Find current information', group: 'Web & Network', order: 1, state: 'on', can_disable: true, icon: null },
	{ key: 'fetch', kind: 'tool', title: 'Fetch URL', description: 'Read a web page', group: 'Web & Network', order: 2, state: 'auto', can_disable: true, icon: null },
	{ key: 'memory', kind: 'tool', title: 'Memory', description: 'Recall saved context', group: 'Memory', order: 3, state: 'off', can_disable: true, icon: null }
];

test('tool search matches titles and descriptions without case sensitivity', () => {
	assert.deepEqual(searchCapabilities(capabilities, 'CURRENT').map((row) => row.key), ['search']);
	assert.deepEqual(searchCapabilities(capabilities, 'page').map((row) => row.key), ['fetch']);
});

test('tool state counts support the dialog summary and filters', () => {
	assert.deepEqual(capabilityCounts(capabilities), { on: 1, auto: 1, off: 1 });
});

const row = (key: string, extra: Partial<CapabilityItem> = {}): CapabilityItem => ({
	key, kind: 'tool', title: key.toUpperCase(), description: '', group: 'g', order: 0, icon: null, ...extra
});

test('search matches actual resource keys with trimmed case insensitive queries', () => {
	const browser = row('browse_page', { title: 'Browse a website', description: 'Control a browser session' });
	const rows = [browser, row('remember', { title: 'Memory', description: 'Store a lesson' })];
	assert.deepEqual(searchCapabilities(rows, '  BROWSE_PAGE  '), [browser]);
	assert.deepEqual(searchCapabilities(rows, 'website'), [browser]);
	assert.deepEqual(searchCapabilities(rows, 'browser session'), [browser]);
	assert.deepEqual(searchCapabilities(rows, 'missing_key'), []);
});

test('search matches the agent rows like the chat rows, and an empty query keeps them all', () => {
	const rows = [row('a', { description: 'Reads the manual' }), row('b')];
	assert.deepEqual(searchCapabilities(rows, ' manual ').map((r) => r.key), ['a']);
	assert.deepEqual(searchCapabilities(rows, '  ').map((r) => r.key), ['a', 'b']);
});

test('groups keep the order they first appear in', () => {
	const rows = [row('a', { group: 'web' }), row('b', { group: 'skills' }), row('c', { group: 'web' })];
	assert.deepEqual(capabilityGroups(rows).map((g) => [g.name, g.rows.map((r) => r.key)]), [['web', ['a', 'c']], ['skills', ['b']]]);
});

test('ranking puts selected, then suggested rows first and keeps the rest in server order', () => {
	const rows = [row('a'), row('b'), row('c'), row('d')];
	const on = new Set(['c']);
	const suggested = new Set(['d', 'c']);
	const ranked = rankCapabilities(rows, (r) => (on.has(r.key) ? 0 : suggested.has(r.key) ? 1 : 2));
	assert.deepEqual(ranked.map((r) => r.key), ['c', 'd', 'a', 'b']);
});

test('only an editable row links to its edit page', () => {
	assert.equal(editLink(row('a', { editable: true, config_url: '/rag/1/edit' })), '/rag/1/edit');
	assert.equal(editLink(row('a', { editable: false, config_url: '/rag/1/edit' })), null);
	assert.equal(editLink(row('a')), null);
});

test('a row shows its own description, never a stand-in', () => {
	assert.deepEqual(descriptionOf(row('a', { description: 'The manual.' })), { text: 'The manual.' });
	assert.equal(descriptionOf(row('a', { description: '  ' })), null, 'no description: the name alone');
	assert.deepEqual(descriptionOf(row('a', { editable: true, config_url: '/admin/skills' })), { missing: '/admin/skills' });
});
