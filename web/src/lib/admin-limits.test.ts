import test from 'node:test';
import assert from 'node:assert/strict';
import { limitSubjectLabel, limitValue } from './admin-limits.ts';

const data = {
	users: [{ id: 'user-1', email: 'dev@example.com' }],
	tokens: [{ id: 'token-1', name: 'Production API', owner: 'dev@example.com' }]
};

const names = {
	global: 'Everyone (default)',
	role: 'Role',
	user: 'User',
	token: 'API token'
};

test('limit subjects retain friendly identities for every assignment kind', () => {
	assert.equal(limitSubjectLabel({ subject_type: 'global', subject_id: '' }, data, names), 'Everyone (default)');
	assert.equal(limitSubjectLabel({ subject_type: 'role', subject_id: 'engineering' }, data, names), 'Role: engineering');
	assert.equal(limitSubjectLabel({ subject_type: 'user', subject_id: 'user-1' }, data, names), 'User: dev@example.com');
	assert.equal(limitSubjectLabel({ subject_type: 'token', subject_id: 'token-1' }, data, names), 'API token: Production API (dev@example.com)');
});

test('limit values preserve grouped counts and fixed-precision cost', () => {
	const format = (value: number, options?: Intl.NumberFormatOptions) => new Intl.NumberFormat('en-US', options).format(value);
	assert.equal(limitValue({ dimension: 'tokens', value: 10_000_000 }, 'USD', format), '10,000,000');
	assert.equal(limitValue({ dimension: 'cost', value: 5 }, 'USD', format), '5.00 USD');
});
