import assert from 'node:assert/strict';
import test from 'node:test';

import { tokenDate } from './tokens.ts';

test('tokenDate uses the account timezone without locale-dependent punctuation', () => {
	assert.equal(tokenDate('2026-09-05T23:30:00Z', 'Asia/Tokyo'), '2026-09-06');
});

test('tokenDate leaves an unexpected value visible', () => {
	assert.equal(tokenDate('unknown', 'UTC'), 'unknown');
});
