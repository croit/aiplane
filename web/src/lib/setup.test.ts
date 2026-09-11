import test from 'node:test';
import assert from 'node:assert/strict';
import { decodeSetupClaim, encodeSetupClaim, setupClaimChoices } from './setup.ts';

test('setup offers meaningful string claims and filters protocol plumbing', () => {
	assert.deepEqual(setupClaimChoices({ iss: 'https://id.example', exp: 42, sub: 'person-1', groups: ['admins', 'engineering'], email_verified: true }), [
		{ claim: 'groups', value: 'admins', label: 'groups = admins' },
		{ claim: 'groups', value: 'engineering', label: 'groups = engineering' },
		{ claim: 'sub', value: 'person-1', label: 'sub = person-1' }
	]);
});

test('setup claim pairs round-trip values containing punctuation', () => {
	const encoded = encodeSetupClaim('groups', 'cn=admins:platform/team');
	assert.deepEqual(decodeSetupClaim(encoded), ['groups', 'cn=admins:platform/team']);
	assert.equal(decodeSetupClaim('missing-separator'), null);
});
