import test from 'node:test';
import assert from 'node:assert/strict';
import { activeDotCount, canCreateDot, type Dot } from './dot-state.ts';

const dot = (active: boolean): Dot => ({
	id: 'dot',
	name: 'assistant',
	display: 'Assistant',
	description: '',
	active
});

test('activeDotCount counts enabled Dots only', () => {
	assert.equal(activeDotCount([dot(true), dot(false), dot(true)]), 2);
	assert.equal(activeDotCount([]), 0);
});

test('canCreateDot is disabled at the active limit', () => {
	assert.equal(canCreateDot(3, 4), true);
	assert.equal(canCreateDot(4, 4), false);
	assert.equal(canCreateDot(5, 4), false);
});

test('Dot memory API retains actual scope authority and sends explicit scopes', async (context) => {
	const { dotMemoriesApi } = await import('./dots.ts');
	const original = globalThis.fetch;
	const calls: { path: string; method: string; body: unknown }[] = [];
	const result = {
		scopes: [
			{ scope: 'dot', name: 'Harald', writable: true },
			{ scope: 'organization', name: 'Acme Support', writable: false }
		],
		memories: [{ id: 'note', kind: 'fact', content: 'A reviewed lesson.', created_at: '2026-10-11', scope: 'dot' }]
	};
	globalThis.fetch = async (input, init) => {
		calls.push({ path: String(input), method: init?.method ?? 'GET', body: init?.body ? JSON.parse(String(init.body)) : null });
		return new Response(JSON.stringify(result), { status: 200 });
	};
	context.after(() => globalThis.fetch = original);
	assert.deepEqual(await dotMemoriesApi.list('dot/id'), result);
	const body = { scope: 'organization' as const, kind: 'fact' as const, content: 'Reviewed shared knowledge.' };
	await dotMemoriesApi.create('dot/id', body);
	await dotMemoriesApi.update('dot/id', 'note/id', body);
	await dotMemoriesApi.delete('dot/id', 'note/id', 'organization');
	assert.deepEqual(calls, [
		{ path: '/api/v0/dots/dot%2Fid/memories', method: 'GET', body: null },
		{ path: '/api/v0/dots/dot%2Fid/memories', method: 'POST', body },
		{ path: '/api/v0/dots/dot%2Fid/memories/note%2Fid', method: 'PUT', body },
		{ path: '/api/v0/dots/dot%2Fid/memories/note%2Fid?scope=organization', method: 'DELETE', body: null }
	]);
});

test('Dot memory sections preserve actual names and isolate same-kind notes by declared scope', async () => {
	const { dotMemoryAuthorityRevoked, dotMemorySections } = await import('./dots.ts');
	const lesson = { id: 'lesson', kind: 'fact' as const, content: 'A reviewed lesson.', created_at: '2026-10-11' };
	const shared = { ...lesson, id: 'shared', content: 'Reviewed shared knowledge.' };
	const result = {
		scopes: [
			{ scope: 'dot' as const, name: 'Harald', writable: true },
			{ scope: 'organization' as const, name: 'Acme Support', writable: false }
		],
		memories: [{ ...lesson, scope: 'dot' as const }, { ...shared, scope: 'organization' as const }]
	};
	const sections = dotMemorySections(result);
	assert.deepEqual(sections.map(({ scope, name, writable }) => ({ scope, name, writable })), result.scopes);
	assert.deepEqual(sections[0].grouped.fact, [result.memories[0]]);
	assert.deepEqual(sections[1].grouped.fact, [result.memories[1]]);
	assert.deepEqual(sections[0].grouped.preference, []);
	assert.deepEqual(sections[1].grouped.project, []);
	const revoked = dotMemorySections({ ...result, scopes: result.scopes.map((scope) => ({ ...scope, writable: false })) });
	assert.equal(revoked.every((scope) => !scope.writable), true);
	assert.deepEqual(revoked.map((section) => section.grouped), sections.map((section) => section.grouped));
	assert.deepEqual(dotMemorySections({ ...result, scopes: [] }), []);
	assert.deepEqual(dotMemorySections(null), []);
	assert.deepEqual(dotMemorySections(undefined), []);
	assert.equal([401, 403, 404].every(dotMemoryAuthorityRevoked), true);
	assert.equal([400, 409, 500].some(dotMemoryAuthorityRevoked), false);
});

test('Dot connection API sends exact scoped versions without credential authority fields', async (context) => {
	const { dotConnectionsApi } = await import('./dots.ts');
	const original = globalThis.fetch;
	const calls: { path: string; method: string; body: unknown }[] = [];
	globalThis.fetch = async (input, init) => {
		calls.push({ path: String(input), method: init?.method ?? 'GET', body: init?.body ? JSON.parse(String(init.body)) : null });
		return new Response(JSON.stringify({ dot: dot(true), connections: [], authorization_url: 'https://provider.example/authorize' }), { status: 200 });
	};
	context.after(() => globalThis.fetch = original);
	await dotConnectionsApi.load('dot/id');
	const change = { mode: 'principal' as const, expected_binding_generation: 'original', connection_id: 'current', connection_generation: 'version' };
	await dotConnectionsApi.save('dot/id', 'workspace/key', change);
	await dotConnectionsApi.connect('dot/id', 'workspace/key', { connection_id: null, connection_generation: null });
	await dotConnectionsApi.disconnect('dot/id', 'workspace/key', { connection_id: 'current', connection_generation: 'version' });
	assert.deepEqual(calls, [
		{ path: '/api/v0/dots/dot%2Fid/connections', method: 'GET', body: null },
		{ path: '/api/v0/dots/dot%2Fid/connections/workspace%2Fkey', method: 'PUT', body: change },
		{ path: '/api/v0/dots/dot%2Fid/connections/workspace%2Fkey/connect', method: 'POST', body: { connection_id: null, connection_generation: null } },
		{ path: '/api/v0/dots/dot%2Fid/connections/workspace%2Fkey', method: 'DELETE', body: { connection_id: 'current', connection_generation: 'version' } }
	]);
});

test('Dot inventory preserves an unavailable own erasure version without user credential data', async (context) => {
	const { dotConnectionsApi } = await import('./dots.ts');
	const empty = { id: null, generation: null, connected: false, errored: false, needs_reauth: false, authorizing: false, account_label: null };
	const row = {
		key: 'actual-key', connector: null, supported_modes: [],
		selected: { mode: 'user', binding_generation: null, connection_id: null, connection_generation: null },
		user_connection: empty,
		principal_connection: { ...empty, id: 'actual-own-id', generation: 'actual-own-generation', connected: true },
		can_connect: false, can_disconnect: true
	};
	const original = globalThis.fetch;
	globalThis.fetch = async () => new Response(JSON.stringify({ dot: dot(true), connections: [row] }), { status: 200 });
	context.after(() => globalThis.fetch = original);
	assert.deepEqual((await dotConnectionsApi.load('dot')).connections, [row]);
});

test('Dot workspace client retains availability and actual metadata and encodes paths and cursors', async (context) => {
	const { dotWorkspaceApi } = await import('./dots.ts');
	const original = globalThis.fetch;
	const calls: string[] = [];
	const result = {
		dot_id: 'dot/id', dot_name: 'Harald', available: true, busy: true,
		path: 'project notes', entries: [{ name: 'a&b.txt', kind: 'file', size_bytes: null, modified_at: null }],
		next_cursor: 'opaque/&cursor'
	};
	globalThis.fetch = async (input) => {
		calls.push(String(input));
		return new Response(JSON.stringify(result), { status: 200 });
	};
	context.after(() => globalThis.fetch = original);
	assert.deepEqual(await dotWorkspaceApi.list('dot/id', 'project notes', 'opaque/&cursor'), result);
	assert.deepEqual(calls, ['/api/v0/dots/dot%2Fid/workspace?path=project+notes&cursor=opaque%2F%26cursor']);
	assert.equal(dotWorkspaceApi.downloadUrl('dot/id', 'project notes/a&b.txt'), '/api/v0/dots/dot%2Fid/workspace/file?path=project+notes%2Fa%26b.txt');
});

test('Dot workspace writes raw file bytes and explicit folder and entry paths through the shared transport', async (context) => {
	const { dotWorkspaceApi } = await import('./dots.ts');
	const original = globalThis.fetch;
	const calls: { path: string; method: string; body: BodyInit | null | undefined; type: string | null }[] = [];
	globalThis.fetch = async (input, init) => {
		calls.push({ path: String(input), method: init?.method ?? 'GET', body: init?.body, type: new Headers(init?.headers).get('content-type') });
		return new Response(null, { status: 204 });
	};
	context.after(() => globalThis.fetch = original);
	const file = new File(['A UTF-8 boundary: ß'], 'notes.txt');
	await dotWorkspaceApi.upload('dot/id', 'project/notes.txt', file);
	await dotWorkspaceApi.createFolder('dot/id', 'project/new folder');
	await dotWorkspaceApi.remove('dot/id', 'project/notes.txt');
	assert.deepEqual(calls, [
		{ path: '/api/v0/dots/dot%2Fid/workspace/file?path=project%2Fnotes.txt', method: 'PUT', body: file, type: 'application/octet-stream' },
		{ path: '/api/v0/dots/dot%2Fid/workspace/folder', method: 'POST', body: JSON.stringify({ path: 'project/new folder' }), type: 'application/json' },
		{ path: '/api/v0/dots/dot%2Fid/workspace/entry?path=project%2Fnotes.txt', method: 'DELETE', body: undefined, type: null }
	]);
});

test('topics are read one per line or comma separated, blanks dropped', async () => {
    const { parseTopics } = await import('./dots.ts');
    assert.deepEqual(parseTopics('Ceph, Proxmox\n\n  Chef  \n,'), ['Ceph', 'Proxmox', 'Chef']);
    assert.deepEqual(parseTopics(''), []);
});

test('an admin changing another user\'s personal agent gets only its settings tabs', async () => {
    const { dotDetailTabs } = await import('./dots.ts');
    assert.deepEqual(dotDetailTabs(false), ['task', 'tools']);
    assert.deepEqual(dotDetailTabs(true), ['task', 'tools', 'memory', 'activity', 'communication', 'workspace', 'automation']);
});
