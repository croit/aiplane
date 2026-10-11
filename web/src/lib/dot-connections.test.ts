import test from 'node:test';
import assert from 'node:assert/strict';
import { connectionChange, connectionModes } from './dot-connections.ts';

const credential = { id: 'credential', generation: 'credential-version', connected: true, errored: false, needs_reauth: false, authorizing: false, account_label: null };
const row = () => ({ key: 'workspace', connector: { title: 'Workspace', description: null, icon: null, auth_type: 'oauth2' as const, scope: 'per_user' as const, needs_setup: false }, supported_modes: ['user', 'principal'] as const, selected: { mode: 'user' as const, binding_generation: 'original', connection_id: 'user-id', connection_generation: 'user-version' }, user_connection: { ...credential, id: 'user-id', generation: 'user-version' }, principal_connection: credential, can_connect: true });

test('connecting never changes saved identity and unchanged selection has no mutation', () => {
	assert.equal(connectionChange(row(), 'user'), null);
	assert.equal(row().selected.mode, 'user');
});

test('explicit own selection pins actual credential and original binding generation', () => {
	assert.deepEqual(connectionChange(row(), 'principal'), { mode: 'principal', expected_binding_generation: 'original', connection_id: 'credential', connection_generation: 'credential-version' });
});

test('missing, authorizing and reconnect-required credentials cannot become selected', () => {
	for (const principal_connection of [null, { ...credential, connected: false }, { ...credential, authorizing: true }, { ...credential, needs_reauth: true }, { ...credential, errored: true }]) {
		assert.equal(connectionChange({ ...row(), principal_connection }, 'principal'), null);
	}
});

test('saved unavailable ownership stays visible without adding unsupported choices', () => {
	const unavailable = { ...row(), connector: null, supported_modes: [], selected: { ...row().selected, mode: 'principal' as const }, principal_connection: null };
	assert.deepEqual(connectionModes(unavailable), ['principal']);
	assert.equal(connectionChange(unavailable, 'user'), null);
});

test('replacing a credential requires explicit same-mode rebinding with original generation', () => {
	const source = row();
	const replaced = { ...source, selected: { ...source.selected, mode: 'principal' as const, connection_id: 'credential', connection_generation: 'previous-version' } };
	assert.deepEqual(connectionChange(replaced, 'principal'), { mode: 'principal', expected_binding_generation: 'original', connection_id: 'credential', connection_generation: 'credential-version' });
});

test('a saved connection stays unavailable after credential replacement until explicitly rebound', async () => {
	const { selectedConnectionAvailable } = await import('./dot-connections.ts');
	const source = row();
	assert.equal(selectedConnectionAvailable(source), true);
	assert.equal(selectedConnectionAvailable({ ...source, user_connection: { ...source.user_connection, generation: 'replacement' } }), false);
	assert.equal(selectedConnectionAvailable({ ...source, user_connection: { ...source.user_connection, connected: false } }), false);
	assert.equal(selectedConnectionAvailable({ ...source, connector: null }), false);
});
