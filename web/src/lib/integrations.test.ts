import assert from 'node:assert/strict';
import test from 'node:test';
import {
	withAllToolModes,
	withToolMode,
	type IntegrationConnector
} from './integrations.ts';

const connector: IntegrationConnector = {
	key: 'docs',
	title: 'Docs',
	description: null,
	icon: null,
	auth_type: 'oauth2',
	is_global: false,
	needs_setup: false,
	connected: true,
	errored: false,
	needs_reauth: false,
	tool_error: null,
	tools: [
		{ name: 'read', description: '', read_only: true, mode: 'always' },
		{ name: 'write', description: '', read_only: false, mode: 'ask' }
	]
};

test('withToolMode updates only the selected tool without mutating the connector', () => {
	const updated = withToolMode(connector, 'write', 'off');
	assert.deepEqual(updated.tools?.map((tool) => tool.mode), ['always', 'off']);
	assert.equal(connector.tools?.[1]?.mode, 'ask');
});

test('withAllToolModes updates every tool', () => {
	assert.deepEqual(
		withAllToolModes(connector, 'ask').tools?.map((tool) => tool.mode),
		['ask', 'ask']
	);
});
