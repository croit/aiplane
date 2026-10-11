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
