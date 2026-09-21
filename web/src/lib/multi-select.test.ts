import assert from 'node:assert/strict';
import test from 'node:test';
import {
	GRANT_WILDCARD,
	isWildcardSelected,
	multiSelectOptions,
	normalizeSelection,
	summarizeSelection,
	toggleValue,
	unknownValues
} from './multi-select.ts';
import type { SearchOption } from './searchable-select.ts';

const tools: SearchOption[] = [
	{ value: 'search_web', label: 'search_web' },
	{ value: 'fetch_url', label: 'fetch_url' },
	{ value: 'rag_search', label: 'rag_search' }
];

test('toggling adds and removes without disturbing the rest', () => {
	assert.deepEqual(toggleValue(['search_web'], 'fetch_url'), ['search_web', 'fetch_url']);
	assert.deepEqual(toggleValue(['search_web', 'fetch_url'], 'search_web'), ['fetch_url']);
});

// `*` is not sugar for "every box checked": the resolver expands it against the
// live registry at call time, and it is what also unlocks the dynamically-loaded
// ComfyUI workflows that are in no registry. Expanding it into explicit ids on
// save would silently narrow the seeded admin group to today's tool list.
test('selecting the wildcard collapses the selection to itself', () => {
	assert.deepEqual(toggleValue(['search_web', 'fetch_url'], GRANT_WILDCARD), [GRANT_WILDCARD]);
	assert.deepEqual(normalizeSelection([GRANT_WILDCARD, 'search_web']), [GRANT_WILDCARD]);
});

test('selecting a concrete value while the wildcard is held replaces the wildcard', () => {
	assert.deepEqual(toggleValue([GRANT_WILDCARD], 'search_web'), ['search_web']);
});

test('clearing the wildcard empties the selection', () => {
	assert.deepEqual(toggleValue([GRANT_WILDCARD], GRANT_WILDCARD), []);
});

test('normalizing drops blanks and duplicates but keeps order', () => {
	assert.deepEqual(normalizeSelection(['fetch_url', '', 'search_web', 'fetch_url', '  ']), [
		'fetch_url',
		'search_web'
	]);
});

test('normalizing trims values so a pasted list does not mis-grant', () => {
	assert.deepEqual(normalizeSelection([' search_web ', 'fetch_url']), ['search_web', 'fetch_url']);
});

test('the wildcard reads as selected only when actually held', () => {
	assert.equal(isWildcardSelected([GRANT_WILDCARD]), true);
	assert.equal(isWildcardSelected(['search_web']), false);
	assert.equal(isWildcardSelected([]), false);
});

// A group granting a tool that a later release removed, or a pool naming a group
// somebody deleted, is not in the option list. Rendering only known options would
// delete that value the next time anyone opened the editor and pressed Save.
test('values with no matching option are reported so the form can keep them', () => {
	assert.deepEqual(unknownValues(['search_web', 'retired_tool'], tools), ['retired_tool']);
	assert.deepEqual(unknownValues(['search_web'], tools), []);
});

test('the wildcard never counts as an unknown value', () => {
	assert.deepEqual(unknownValues([GRANT_WILDCARD], tools), []);
});

test('options carry the wildcard first and shadow the rest once it is held', () => {
	const offered = multiSelectOptions(tools, { wildcardLabel: 'Every tool', shadowLabel: 'via ✱' }, [
		GRANT_WILDCARD
	]);
	assert.equal(offered[0]?.value, GRANT_WILDCARD);
	assert.equal(offered[0]?.disabled, undefined);
	assert.deepEqual(
		offered.slice(1).map((option) => option.disabled),
		[true, true, true]
	);
});

test('options stay selectable while the wildcard is not held', () => {
	const offered = multiSelectOptions(tools, { wildcardLabel: 'Every tool', shadowLabel: 'via ✱' }, []);
	assert.deepEqual(
		offered.slice(1).map((option) => option.disabled),
		[undefined, undefined, undefined]
	);
});

test('options omit the wildcard row where no wildcard grant exists', () => {
	const offered = multiSelectOptions(tools, {}, []);
	assert.deepEqual(
		offered.map((option) => option.value),
		['search_web', 'fetch_url', 'rag_search']
	);
});

test('an unknown held value is offered so it can be seen and removed', () => {
	const offered = multiSelectOptions(tools, { unknownLabel: 'unknown' }, ['retired_tool']);
	const retired = offered.find((option) => option.value === 'retired_tool');
	assert.equal(retired?.description, 'unknown');
});

// An empty option list means the subsystem is unconfigured or unreachable (the
// skills directory, say), not that every grant the group holds has been removed.
// Calling them all "not registered" there reads as an instruction to delete
// working grants.
test('held values are not called unknown when there are no options to compare against', () => {
	const offered = multiSelectOptions([], { unknownLabel: 'unknown' }, ['release-notes-writer']);
	assert.equal(offered.length, 1);
	assert.equal(offered[0]?.value, 'release-notes-writer');
	assert.equal(offered[0]?.description, undefined);
});

test('the summary names a lone selection and counts a longer one', () => {
	assert.equal(summarizeSelection([], tools, { empty: 'Nothing', counted: (n) => `${n} selected` }), 'Nothing');
	assert.equal(
		summarizeSelection(['search_web'], tools, { empty: 'Nothing', counted: (n) => `${n} selected` }),
		'search_web'
	);
	assert.equal(
		summarizeSelection(['search_web', 'fetch_url'], tools, {
			empty: 'Nothing',
			counted: (n) => `${n} selected`
		}),
		'2 selected'
	);
});

test('the summary shows the wildcard by its label rather than as a count', () => {
	assert.equal(
		summarizeSelection([GRANT_WILDCARD], tools, {
			empty: 'Nothing',
			counted: (n) => `${n} selected`,
			wildcard: 'Every tool'
		}),
		'Every tool'
	);
});
