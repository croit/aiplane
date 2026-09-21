import assert from 'node:assert/strict';
import test from 'node:test';
import {
	coverageOf,
	groupGrantRows,
	FAMILY_SECTION,
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
		[
			{ id: 'search_web', category: 'Web & Network', order: 0 },
			{ id: 'comfyui_upscale', category: 'ComfyUI workflows', order: 4 }
		],
		[{ id: 'comfyui', subject: '' }, { id: 'mcp__slack', subject: 'Slack' }],
		[{ id: 'mcp__slack__post', connector: 'slack', description: 'Post', category: 'Integrations', order: 9 }]
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
	const rows = toolMatrixRows([{ id: 'comfyui', category: 'Utility', order: 10 }], [{ id: 'comfyui', subject: '' }], []);
	assert.deepEqual(rows.map((row) => row.value), ['*', 'comfyui']);
});

// The matrix lists every registry tool, every workflow and every cached MCP
// tool; without sections that is one undifferentiated scroll. Grouping reuses
// the catalog's own `Category`, so this page sections the same way `/tools` and
// `/tokens` already do.
test('rows group into the families section first, then by category in catalog order', () => {
	const grouped = groupGrantRows(
		[
			{ value: '*', kind: 'wildcard', label: 'Every tool' },
			{ value: 'comfyui', kind: 'family', label: 'Every ComfyUI workflow' },
			{ value: 'run_in_sandbox', kind: 'tool', label: 'run_in_sandbox', category: 'Code & Sandbox', order: 6 },
			{ value: 'search_web', kind: 'tool', label: 'search_web', category: 'Web & Network', order: 0 }
		]
	);
	assert.deepEqual(
		grouped.map(([section, entries]) => [section, entries.map((row) => row.value)]),
		[
			[FAMILY_SECTION, ['*', 'comfyui']],
			['Web & Network', ['search_web']],
			['Code & Sandbox', ['run_in_sandbox']]
		]
	);
});

test('rows with no category fall into one unnamed section', () => {
	const grouped = groupGrantRows(
		[{ value: 'brand', kind: 'tool', label: 'brand' }]
	);
	assert.deepEqual(grouped, [['', [{ value: 'brand', kind: 'tool', label: 'brand' }]]]);
});

test('an empty families section is not emitted', () => {
	const grouped = groupGrantRows(
		[{ value: 'search_web', kind: 'tool', label: 'search_web', category: 'Web & Network', order: 0 }]
	);
	assert.deepEqual(grouped.map(([section]) => section), ['Web & Network']);
});

// The skills matrix has no categories, so its rows land in a section whose label
// is empty. Rendering that as a heading puts a blank grey bar above the list.
test('an uncategorised section keeps the empty label for the view to skip', () => {
	const grouped = groupGrantRows(
		[
			{ value: '*', kind: 'wildcard', label: 'Every skill' },
			{ value: 'brand', kind: 'tool', label: 'brand' }
		]
	);
	assert.deepEqual(grouped.map(([section]) => section), [FAMILY_SECTION, '']);
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
