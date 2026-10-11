import test from 'node:test';
import assert from 'node:assert/strict';
import { dotAccessApi } from './dots.ts';
import { ApiError } from './api.ts';

test('access replacement sends only explicitly selected tools and approval rules', async (context) => {
	const originalFetch = globalThis.fetch;
	context.after(() => { globalThis.fetch = originalFetch; });
	let received: unknown;
	globalThis.fetch = async (url, options) => {
		assert.equal(url, '/api/v0/dots/personal%2Fdot/access');
		assert.equal(options?.method, 'PUT');
		received = JSON.parse(options?.body as string);
		return Response.json({ tools: [] });
	};
	await dotAccessApi.save('personal/dot', [{ id: 'browse_page', permission: 'always_ask', actions: [{ id: 'read', rule: 'allow' }, { id: 'fill', rule: 'deny' }] }]);
	assert.deepEqual(received, { tools: [{ id: 'browse_page', permission: 'always_ask', actions: [{ id: 'read', rule: 'allow' }, { id: 'fill', rule: 'deny' }] }] });
});

test('a revoked tool refusal retains the server error for reload guidance', async (context) => {
	const originalFetch = globalThis.fetch;
	context.after(() => { globalThis.fetch = originalFetch; });
	globalThis.fetch = async () => Response.json({ error: { code: 'dot_access_unavailable', message: 'Reload access.' } }, { status: 422 });
	await assert.rejects(dotAccessApi.save('dot', []), (caught: unknown) => {
		assert.ok(caught instanceof ApiError);
		assert.equal(caught.serverMessage, 'Reload access.');
		return true;
	});
});
