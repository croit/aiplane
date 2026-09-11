import assert from 'node:assert/strict';
import test from 'node:test';

import { request } from './api.ts';

test('successful empty responses complete without JSON parsing', async (context) => {
	const originalFetch = globalThis.fetch;
	context.after(() => { globalThis.fetch = originalFetch; });
	globalThis.fetch = async () => new Response(null, { status: 204 });
	assert.equal(await request<void>('/empty'), undefined);
});
