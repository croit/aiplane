import assert from 'node:assert/strict';
import test from 'node:test';

import { formatWebhookFire, formatWebhookRun } from './webhooks.ts';

test('personal webhook calls retain their existing shared JSON transport and bodies', async (context) => {
	const { webhooksApi } = await import('./webhooks.ts');
	const original = globalThis.fetch;
	const calls: { path: string; method: string; body: unknown }[] = [];
	globalThis.fetch = async (input, init) => {
		calls.push({ path: String(input), method: init?.method ?? 'GET', body: init?.body ? JSON.parse(String(init.body)) : null });
		return new Response(JSON.stringify({ webhooks: [], agents: [], models: [], secret: 'fixture-secret', session_id: 'session' }), { status: 200 });
	};
	context.after(() => globalThis.fetch = original);
	const body = { name: 'Personal support', prompt: 'Review the event', model: 'demo-model', personal_agent_id: null, tools_enabled: false, synchronous: true, reuse_conversation: false, reuse_rounds: 5 };
	await webhooksApi.list();
	await webhooksApi.create(body);
	await webhooksApi.update('hook', { ...body, expected_configuration_generation: 'personal-generation' });
	await webhooksApi.toggle('hook', false, 'personal-generation');
	await webhooksApi.rotate('hook', 'personal-generation');
	await webhooksApi.runs('hook');
	await webhooksApi.rerun('hook', { prompt: 'Review again', run: 'recorded-run', expected_configuration_generation: 'personal-generation' });
	await webhooksApi.remove('hook');
	assert.deepEqual(calls, [
		{ path: '/api/v0/webhooks', method: 'GET', body: null },
		{ path: '/api/v0/webhooks', method: 'POST', body },
		{ path: '/api/v0/webhooks/hook', method: 'PUT', body: { ...body, expected_configuration_generation: 'personal-generation' } },
		{ path: '/api/v0/webhooks/hook/toggle', method: 'POST', body: { enabled: false, expected_configuration_generation: 'personal-generation' } },
		{ path: '/api/v0/webhooks/hook/rotate', method: 'POST', body: { expected_configuration_generation: 'personal-generation' } },
		{ path: '/api/v0/webhooks/hook/runs', method: 'GET', body: null },
		{ path: '/api/v0/webhooks/hook/rerun', method: 'POST', body: { prompt: 'Review again', run: 'recorded-run', expected_configuration_generation: 'personal-generation' } },
		{ path: '/api/v0/webhooks/hook', method: 'DELETE', body: null }
	]);
});

test('webhook timestamps retain the original compact 24-hour layout', () => {
	assert.equal(formatWebhookFire('2026-09-10T09:08:07Z', 'en'), 'Sep 10, 09:08');
	assert.equal(formatWebhookRun('2026-09-10T09:08:07Z', 'en'), 'Sep 10, 09:08:07');
});

test('organization webhook inventory scopes the existing endpoint and retains actual authority and versions', async (context) => {
	const { webhooksApi } = await import('./webhooks.ts');
	const original = globalThis.fetch;
	const paths: string[] = [];
	const actual = { webhooks: [], agents: [{ id: 'principal/id', name: 'Harald', active: false }], models: [], principal_id: 'principal/id', can_write: true, versions: [{ version: 3, spec: { main: { model: 'demo-model' } }, published_by: 'writer', published_at: '2026-10-11T00:00:00Z' }] };
	globalThis.fetch = async (input) => {
		paths.push(String(input));
		return new Response(JSON.stringify(actual), { status: 200 });
	};
	context.after(() => globalThis.fetch = original);
	assert.deepEqual(await webhooksApi.list('principal/id'), actual);
	await webhooksApi.list();
	assert.deepEqual(paths, ['/api/v0/webhooks?principal_id=principal%2Fid', '/api/v0/webhooks']);
});

test('organization webhook edits, pause and rotation compare the current configuration generation', async (context) => {
	const { webhooksApi } = await import('./webhooks.ts');
	const original = globalThis.fetch;
	const calls: { path: string; method: string; body: unknown }[] = [];
	globalThis.fetch = async (input, init) => {
		calls.push({ path: String(input), method: init?.method ?? 'GET', body: init?.body ? JSON.parse(String(init.body)) : null });
		return new Response(JSON.stringify({ enabled: false, configuration_generation: 'original', secret: 'fixture-secret', session_id: 'private-session' }), { status: 200 });
	};
	context.after(() => globalThis.fetch = original);
	const body = { principal_id: 'principal', agent_version: 3, name: 'Review support', prompt: 'Review this event', tools_enabled: true, synchronous: false, reuse_conversation: false, reuse_rounds: 5 };
	await webhooksApi.create(body);
	await webhooksApi.update('hook', { ...body, expected_configuration_generation: 'original' });
	await webhooksApi.toggle('hook', false, 'original');
	await webhooksApi.rotate('hook', 'original');
	await webhooksApi.rerun('hook', { prompt: 'Review again', run: 'recorded-run', expected_configuration_generation: 'original' });
	assert.deepEqual(calls, [
		{ path: '/api/v0/webhooks', method: 'POST', body },
		{ path: '/api/v0/webhooks/hook', method: 'PUT', body: { ...body, expected_configuration_generation: 'original' } },
		{ path: '/api/v0/webhooks/hook/toggle', method: 'POST', body: { enabled: false, expected_configuration_generation: 'original' } },
		{ path: '/api/v0/webhooks/hook/rotate', method: 'POST', body: { expected_configuration_generation: 'original' } },
		{ path: '/api/v0/webhooks/hook/rerun', method: 'POST', body: { prompt: 'Review again', run: 'recorded-run', expected_configuration_generation: 'original' } }
	]);
});
