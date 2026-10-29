import test from 'node:test';
import assert from 'node:assert/strict';
import { agentAccessApi } from './owned-agents.ts';
import { ApiError } from './api.ts';

test('access replacement sends only explicitly selected tools and approval rules', async (context) => {
	const originalFetch = globalThis.fetch;
	context.after(() => { globalThis.fetch = originalFetch; });
	let received: unknown;
	globalThis.fetch = async (url, options) => {
		assert.equal(url, '/api/v0/agents/personal%2Fagent/access');
		assert.equal(options?.method, 'PUT');
		received = JSON.parse(options?.body as string);
		return Response.json({ revision: 'draft-revision-1', tools: [] });
	};
	await agentAccessApi.save('personal/agent', 'draft-revision-1', [{ id: 'browse_page', permission: 'always_ask', actions: [{ id: 'read', rule: 'allow' }, { id: 'fill', rule: 'deny' }] }]);
	assert.deepEqual(received, { revision: 'draft-revision-1', tools: [{ id: 'browse_page', permission: 'always_ask', actions: [{ id: 'read', rule: 'allow' }, { id: 'fill', rule: 'deny' }] }] });
});

test('a revoked tool refusal retains the server error for reload guidance', async (context) => {
	const originalFetch = globalThis.fetch;
	context.after(() => { globalThis.fetch = originalFetch; });
	globalThis.fetch = async () => Response.json({ error: { code: 'agent_access_unavailable', message: 'Reload access.' } }, { status: 422 });
	await assert.rejects(agentAccessApi.save('agent', 'draft-revision-2', []), (caught: unknown) => {
		assert.ok(caught instanceof ApiError);
		assert.equal(caught.serverMessage, 'Reload access.');
		return true;
	});
});
