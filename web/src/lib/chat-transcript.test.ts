import assert from 'node:assert/strict';
import test from 'node:test';

import { prettyToolPayload, summarizeToolCalls, presentedToolCallStatus, searchProviderFromOutput } from './chat-transcript.ts';
import type { Turn } from './chat-protocol.ts';

test('suspended turns present unfinished tool calls as waiting', () => {
	assert.equal(presentedToolCallStatus('running', 'suspended'), 'waiting');
	assert.equal(presentedToolCallStatus('running', 'in_progress'), 'running');
});

test('completed and errored tool calls retain their result while the turn waits', () => {
	const statuses: Turn['status'][] = ['in_progress', 'suspended', 'completed', 'cancelled', 'errored'];
	for (const status of statuses) {
		assert.equal(presentedToolCallStatus('completed', status), 'completed');
		assert.equal(presentedToolCallStatus('errored', status), 'errored');
	}
});

test('only an actually suspended turn gives a running call the waiting presentation', () => {
	const statuses: Turn['status'][] = ['in_progress', 'completed', 'cancelled', 'errored'];
	for (const status of statuses) assert.equal(presentedToolCallStatus('running', status), 'running');
});

test('tool payloads are pretty printed and bounded for the browser', () => {
	assert.equal(prettyToolPayload('{"query":"nginx"}', 100).text, '{\n  "query": "nginx"\n}');
	const large = prettyToolPayload(JSON.stringify({ text: 'x'.repeat(200) }), 32);
	assert.equal(large.truncated, true);
	assert.ok(large.text.length < 80);
});

test('tool call summaries preserve first-seen order and tally repeats', () => {
	assert.equal(
		summarizeToolCalls([{ name: 'rag_search' }, { name: 'fetch_url' }, { name: 'rag_search' }]),
		'rag_search ×2, fetch_url'
	);
});

test('the search provider is read from a search_web result, or absent', () => {
	assert.equal(searchProviderFromOutput(JSON.stringify({ provider: 'tavily', results: [] })), 'tavily');
	assert.equal(searchProviderFromOutput(JSON.stringify({ provider: 'brave', results: [] })), 'brave');
	assert.equal(searchProviderFromOutput(JSON.stringify({ provider: 'searxng', results: [] })), 'searxng');
	assert.equal(searchProviderFromOutput(null), null);
	assert.equal(searchProviderFromOutput(JSON.stringify({ results: [] })), null);
	assert.equal(searchProviderFromOutput(JSON.stringify({ provider: '' })), null);
	assert.equal(searchProviderFromOutput('not json'), null);
});
