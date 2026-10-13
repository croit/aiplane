import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { stripTypeScriptTypes } from 'node:module';
import { compileModule } from 'svelte/compiler';
import type { BrowserActionEvent } from './chat-protocol.ts';

const sourceUrl = new URL('./chat.svelte.ts', import.meta.url);
const source = stripTypeScriptTypes(readFileSync(sourceUrl, 'utf8'));
const compiled = compileModule(source, { filename: sourceUrl.pathname, generate: 'server' }).js.code
	.replace(/from ['"](svelte\/[^'"]+)['"]/g, (_match, name) => `from ${JSON.stringify(import.meta.resolve(name))}`)
	.replace(/from ['"]\.\/(chat-protocol|api|browser-bridge)['"]/g, (_match, name) => `from ${JSON.stringify(new URL(`./${name}.ts`, import.meta.url).href)}`);
const { createConversationController } = await import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`);
const browserEvent: BrowserActionEvent = { type: 'browser_action', turn_id: 'turn', request_id: 'call', actions: [] };

test('the real controller records read-only browser events without executing or posting feedback', async (context) => {
	context.mock.timers.enable({ apis: ['setTimeout'] });
	const originalFetch = globalThis.fetch;
	const responses: string[] = [];
	// The browser's same-origin HTTP transport is unavailable in Node.
	globalThis.fetch = async (input) => { responses.push(String(input)); return new Response('{}'); };
	try {
		let writable = false;
		const controller = createConversationController('child', '/events', () => writable);
		controller.apply(browserEvent);
		context.mock.timers.tick(8000);
		await new Promise((resolve) => setImmediate(resolve));
		assert.deepEqual(responses, []);
		writable = true;
		controller.apply(browserEvent);
		context.mock.timers.tick(8000);
		await new Promise((resolve) => setImmediate(resolve));
		assert.equal(responses.length, 1);
		assert.match(responses[0], /browser\/feedback/);
		writable = false;
		controller.apply(browserEvent);
		context.mock.timers.tick(8000);
		await new Promise((resolve) => setImmediate(resolve));
		assert.equal(responses.length, 1);
		controller.destroy();
	} finally { globalThis.fetch = originalFetch; }
});

test('existing agent-test controller caller retains browser feedback by default', async (context) => {
	context.mock.timers.enable({ apis: ['setTimeout'] });
	const originalFetch = globalThis.fetch;
	const responses: string[] = [];
	globalThis.fetch = async (input) => { responses.push(String(input)); return new Response('{}'); };
	try {
		const controller = createConversationController('agent-test', '/agent-events');
		controller.apply(browserEvent);
		context.mock.timers.tick(8000);
		await new Promise((resolve) => setImmediate(resolve));
		assert.equal(responses.length, 1);
		controller.destroy();
	} finally { globalThis.fetch = originalFetch; }
});
