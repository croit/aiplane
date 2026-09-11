import assert from 'node:assert/strict';
import test from 'node:test';

import { prettyToolPayload, summarizeToolCalls } from './chat-transcript.ts';

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
