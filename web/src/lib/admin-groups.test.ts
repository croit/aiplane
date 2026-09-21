import assert from 'node:assert/strict';
import test from 'node:test';
import {
	coverageOf,
	identityRows,
	matchesGrantFilter,
	selectedGroupAdminTab,
	toolMatrixRows,
	type GrantRow
} from './admin-groups.ts';
import type { AdminGroup } from './components/admin/AdminGroupForm.svelte';

function group(name: string, tools: string[], skills: string[] = [], oidc: string[] = []): AdminGroup {
	return {
		name,
		description: '',
		is_admin: false,
		is_default: false,
		oidc_values: oidc,
		tools,
		skills
	};
}

test('the tab comes from the query string and falls back to the list', () => {
	assert.equal(selectedGroupAdminTab(''), 'groups');
	assert.equal(selectedGroupAdminTab('?tab=identity'), 'identity');
	assert.equal(selectedGroupAdminTab('?tab=tools'), 'tools');
	assert.equal(selectedGroupAdminTab('?tab=skills'), 'skills');
	assert.equal(selectedGroupAdminTab('?tab=nonsense'), 'groups');
});

test('a grant the group names directly reads as granted', () => {
	assert.equal(coverageOf(['search_web'], 'search_web'), 'granted');
	assert.equal(coverageOf(['search_web'], 'rag_search'), 'none');
});

// The matrix must not invite the operator to "uncheck" something that is not
// individually granted: clicking would rewrite the wider grant into a list of
// today's ids and quietly drop whatever ships next.
test('a wildcard covers every row without granting any of them individually', () => {
	assert.equal(coverageOf(['*'], '*'), 'granted');
	assert.equal(coverageOf(['*'], 'search_web'), 'covered');
	assert.equal(coverageOf(['*'], 'comfyui_upscale'), 'covered');
	assert.equal(coverageOf(['*'], 'mcp__slack__post'), 'covered');
});

test('the comfyui family covers its workflows and nothing else', () => {
	assert.equal(coverageOf(['comfyui'], 'comfyui'), 'granted');
	assert.equal(coverageOf(['comfyui'], 'comfyui_upscale'), 'covered');
	assert.equal(coverageOf(['comfyui'], 'search_web'), 'none');
});

test('an mcp server key covers that server and nothing else', () => {
	assert.equal(coverageOf(['mcp__slack'], 'mcp__slack'), 'granted');
	assert.equal(coverageOf(['mcp__slack'], 'mcp__slack__post'), 'covered');
	assert.equal(coverageOf(['mcp__slack'], 'mcp__jira__create'), 'none');
});

test('an explicit tool id inside a covered family still reads as granted', () => {
	assert.equal(coverageOf(['comfyui', 'comfyui_upscale'], 'comfyui_upscale'), 'granted');
});

test('the tool matrix orders the wildcard, then families, then ids', () => {
	const rows = toolMatrixRows(
		['search_web', 'comfyui_upscale'],
		[{ id: 'comfyui', subject: '' }, { id: 'mcp__slack', subject: 'Slack' }],
		[{ id: 'mcp__slack__post', connector: 'slack', description: 'Post' }]
	);
	assert.deepEqual(
		rows.map((row) => row.value),
		['*', 'comfyui', 'mcp__slack', 'search_web', 'comfyui_upscale', 'mcp__slack__post']
	);
	assert.equal(rows[0]?.kind, 'wildcard');
	assert.equal(rows[1]?.kind, 'family');
	assert.equal(rows[3]?.kind, 'tool');
});

test('a row is not listed twice when an id is also offered as a family', () => {
	const rows = toolMatrixRows(['comfyui'], [{ id: 'comfyui', subject: '' }], []);
	assert.deepEqual(rows.map((row) => row.value), ['*', 'comfyui']);
});

const rows: GrantRow[] = [
	{ value: '*', kind: 'wildcard', label: 'Every tool' },
	{ value: 'comfyui', kind: 'family', label: 'Every ComfyUI workflow' },
	{ value: 'search_web', kind: 'tool', label: 'search_web' }
];

test('the filter matches value and label, and can narrow to families', () => {
	const groups = [group('devs', ['search_web'])];
	assert.equal(matchesGrantFilter(rows[2] as GrantRow, 'all', 'web', groups), true);
	assert.equal(matchesGrantFilter(rows[2] as GrantRow, 'all', 'comfy', groups), false);
	assert.equal(matchesGrantFilter(rows[1] as GrantRow, 'all', 'comfy', groups), true);
	assert.equal(matchesGrantFilter(rows[2] as GrantRow, 'families', '', groups), false);
	assert.equal(matchesGrantFilter(rows[1] as GrantRow, 'families', '', groups), true);
});

test('the granted filter counts a covering grant, not just a direct one', () => {
	const wildcard = [group('admin', ['*'])];
	assert.equal(matchesGrantFilter(rows[2] as GrantRow, 'granted', '', wildcard), true);
	assert.equal(matchesGrantFilter(rows[2] as GrantRow, 'ungranted', '', wildcard), false);
	const none = [group('devs', [])];
	assert.equal(matchesGrantFilter(rows[2] as GrantRow, 'granted', '', none), false);
	assert.equal(matchesGrantFilter(rows[2] as GrantRow, 'ungranted', '', none), true);
});

// A claim value nobody maps is the silent failure: the user signs in and gets
// only the default group's grants, with nothing anywhere saying why.
test('identity rows flag a seen value that maps to no group', () => {
	const got = identityRows(['grp-dev', 'grp-contractors'], [group('devs', [], [], ['grp-dev'])]);
	assert.deepEqual(got, [
		{ value: 'grp-contractors', groups: [], seen: true },
		{ value: 'grp-dev', groups: ['devs'], seen: true }
	]);
});

// The mirror case: a mapping that no login has ever matched, which usually means
// the value was mistyped when the group was created.
test('identity rows include a mapping nobody has presented yet', () => {
	const got = identityRows([], [group('devs', [], [], ['CN=devs,OU=corp'])]);
	assert.deepEqual(got, [{ value: 'CN=devs,OU=corp', groups: ['devs'], seen: false }]);
});

test('identity rows collect every group a value maps into', () => {
	const got = identityRows(
		['shared'],
		[group('a', [], [], ['shared']), group('b', [], [], ['shared'])]
	);
	assert.deepEqual(got, [{ value: 'shared', groups: ['a', 'b'], seen: true }]);
});
