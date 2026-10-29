import assert from 'node:assert/strict';
import test from 'node:test';
import { ownedAgentCommunicationApi } from './owned-agents.ts';
import { ApiError } from './api.ts';

test('communication load and one directional save preserve actual inventory and opaque generations', async (context) => {
	const originalFetch = globalThis.fetch;
	context.after(() => { globalThis.fetch = originalFetch; });
	const inventory = { agent: { id: 'harald', name: 'Harald', display: '', description: '', active: true }, peers: [] };
	let sent: unknown;
	globalThis.fetch = async (url, options) => {
		assert.equal(url, '/api/v0/agents/personal%2Fagent/communication');
		if (options?.method === 'PUT') sent = JSON.parse(options.body as string);
		return Response.json(inventory);
	};
	assert.deepEqual(await ownedAgentCommunicationApi.load('personal/agent'), inventory);
	const change = { peer_id: 'clara', direction: 'inbound' as const, enabled: true, expected_generation: null };
	assert.deepEqual(await ownedAgentCommunicationApi.save('personal/agent', change), inventory);
	assert.deepEqual(sent, change);
});

test('stale communication saves retain the structured conflict and server reload guidance', async (context) => {
	const originalFetch = globalThis.fetch;
	context.after(() => { globalThis.fetch = originalFetch; });
	globalThis.fetch = async () => Response.json({ error: { code: 'conflict', message: 'Reload communication permissions.' } }, { status: 409 });
	await assert.rejects(ownedAgentCommunicationApi.save('harald', { peer_id: 'clara', direction: 'outbound', enabled: false, expected_generation: 'stale' }), (caught: unknown) => {
		assert.ok(caught instanceof ApiError);
		assert.equal(caught.status, 409);
		assert.equal(caught.code, 'conflict');
		assert.equal(caught.serverMessage, 'Reload communication permissions.');
		return true;
	});
});
