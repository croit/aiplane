import test from 'node:test';
import assert from 'node:assert/strict';
import { api } from './api.ts';

test('chat session lookup keeps all, ordinary, and Dot scopes distinct', async (context) => {
	const original = globalThis.fetch;
	context.after(() => { globalThis.fetch = original; });
	const urls: string[] = [];
	globalThis.fetch = async (url) => { urls.push(String(url)); return Response.json({ sessions: [] }); };
	await api.listChatSessions();
	await api.listChatSessions(null);
	await api.listChatSessions('personal/dot');
	assert.deepEqual(urls, ['/api/v0/chat/sessions', '/api/v0/chat/sessions?dot_id=', '/api/v0/chat/sessions?dot_id=personal%2Fdot']);
});
