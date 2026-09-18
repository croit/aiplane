// The feedback widget's diagnostics buffers are the one part of the report
// that the reporter does not read before sending, so the guarantees they make
// have to be pinned rather than assumed:
//
//   - credential-shaped values never enter a buffer, and
//   - nothing in a buffer is unbounded.
//
// A regression in either is invisible in the UI and only shows up as a token
// sitting in a public issue tracker.

import test from 'node:test';
import assert from 'node:assert';
import { REDACTED, extractQuery, sanitizeBody } from './feedback-capture.ts';

test('query parameters that look like credentials are redacted', () => {
	const query = extractQuery('https://aiplane.example.com/x?token=sekrit&page=2');
	assert.ok(query);
	const params = new URLSearchParams(query);
	assert.equal(params.get('token'), REDACTED);
	// Everything else survives — a redacted log nobody can read is no better
	// than no log.
	assert.equal(params.get('page'), '2');
});

test('redaction is case insensitive and covers every credential spelling', () => {
	for (const key of ['Authorization', 'API_KEY', 'client_secret', 'Refresh_Token']) {
		const query = extractQuery(`https://aiplane.example.com/x?${key}=value`);
		assert.equal(new URLSearchParams(query!).get(key), REDACTED, key);
	}
});

test('a URL without a query yields nothing rather than an empty string', () => {
	assert.equal(extractQuery('https://aiplane.example.com/x'), undefined);
	// A relative URL still resolves — fetch is called with those constantly.
	assert.equal(extractQuery('/api/v0/me'), undefined);
});

test('a malformed URL is skipped, never thrown on', () => {
	assert.equal(extractQuery('http://[::bad'), undefined);
});

test('JSON request bodies have their credential keys replaced', () => {
	const body = sanitizeBody(JSON.stringify({ password: 'hunter2', email: 'a@example.com' }));
	const parsed = JSON.parse(body!) as Record<string, string>;
	assert.equal(parsed.password, REDACTED);
	assert.equal(parsed.email, 'a@example.com');
});

test('bodies are truncated so one upload cannot fill the issue', () => {
	const body = sanitizeBody('x'.repeat(5000));
	assert.equal(body!.length, 500);
	assert.ok(body!.endsWith('...'));
});

test('a long JSON body is truncated too, not left whole because it parsed', () => {
	const body = sanitizeBody(JSON.stringify({ note: 'y'.repeat(5000) }));
	assert.equal(body!.length, 500);
});

test('binary bodies are named, never transcribed', () => {
	assert.equal(sanitizeBody(new ArrayBuffer(32)), '[ArrayBuffer 32B]');
	assert.equal(sanitizeBody(new Uint8Array(8)), '[TypedArray 8B]');
	assert.equal(sanitizeBody(new FormData()), '[FormData]');
	assert.equal(sanitizeBody(null), undefined);
	assert.equal(sanitizeBody(''), undefined);
});

test('non-JSON text passes through unchanged when it is short', () => {
	assert.equal(sanitizeBody('grant_type=refresh'), 'grant_type=refresh');
});
