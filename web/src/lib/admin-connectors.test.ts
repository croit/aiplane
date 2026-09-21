import test from 'node:test';
import assert from 'node:assert/strict';
import { connectorBadges, connectorForm, connectorOAuthHelpProvider, connectorUsesOAuth } from './admin-connectors.ts';
import type { AdminConnector } from './admin-connectors.ts';

const connector: AdminConnector = {
	key: 'discord', title: 'Discord', description: 'Chat', icon: null, category: null,
	base_url: 'http://discord/mcp', auth_type: 'none', scope: 'global', scopes: [],
	enabled: true, audit: true, use_dcr: true, client_id: null, has_secret: false,
	authorize_url: null, token_url: null, registration_url: null, groups: [],
	seeded: true, needs_setup: false
};

test('connector badges retain operational state and provenance', () => {
	assert.deepEqual(connectorBadges(connector), ['enabled', 'global', 'audited', 'default']);
});

test('the edit form preserves every non-secret connector field', () => {
	assert.deepEqual(connectorForm(connector), {
		key: 'discord', title: 'Discord', description: 'Chat', icon: '', category: '',
		base_url: 'http://discord/mcp', auth_type: 'none', scope: 'global', scopes: '',
		client_id: '', client_secret: '', client_json: '', use_dcr: true,
		authorize_url: '', token_url: '', registration_url: '', groups: [], audit: true,
		overwrite: true
	});
});

test('only OAuth connectors expose the OAuth application fields', () => {
	assert.equal(connectorUsesOAuth('oauth2'), true);
	assert.equal(connectorUsesOAuth('static_bearer'), false);
	assert.equal(connectorUsesOAuth('none'), false);
});

test('OAuth help follows the connector provider and DCR mode', () => {
	assert.equal(connectorOAuthHelpProvider({ key: 'google_workspace', category: '', use_dcr: true }), 'google_workspace');
	assert.equal(connectorOAuthHelpProvider({ key: 'gmail', category: '', use_dcr: false }), 'google');
	assert.equal(connectorOAuthHelpProvider({ key: 'calendar', category: 'Google', use_dcr: false }), 'google');
	assert.equal(connectorOAuthHelpProvider({ key: 'github', category: '', use_dcr: false }), 'github');
	assert.equal(connectorOAuthHelpProvider({ key: 'slack', category: '', use_dcr: false }), 'slack');
	assert.equal(connectorOAuthHelpProvider({ key: 'custom', category: '', use_dcr: true }), 'dcr');
	assert.equal(connectorOAuthHelpProvider({ key: 'custom', category: '', use_dcr: false }), 'fallback');
});
