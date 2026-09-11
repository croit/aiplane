import test from 'node:test';
import assert from 'node:assert/strict';

import { tokenState, visibleTokenModels } from './admin-tokens.ts';

test('token state distinguishes revoked, expired, and active credentials', () => {
	const now = Date.parse('2026-09-10T00:00:00Z');
	assert.equal(tokenState({ revoked: true, expires_at: '2030-01-01T00:00:00Z' }, now), 'revoked');
	assert.equal(tokenState({ revoked: false, expires_at: '2026-09-09T00:00:00Z' }, now), 'expired');
	assert.equal(tokenState({ revoked: false, expires_at: '2026-09-11T00:00:00Z' }, now), 'active');
});

test('the owner model list is distinct from the operator restriction', () => {
	assert.equal(visibleTokenModels(null), null);
	assert.deepEqual(visibleTokenModels(['model-b', 'model-a']), ['model-b', 'model-a']);
});
