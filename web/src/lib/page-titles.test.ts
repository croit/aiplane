import test from 'node:test';
import assert from 'node:assert/strict';
import { pageTitleDescriptor } from './page-titles.ts';

const branded = [
	['/', 'chat-default-title'],
	['/chat', 'chat-default-title'],
	['/chat/abc', 'chat-default-title'],
	['/memory', 'memory-heading'],
	['/scheduled', 'scheduled-heading'],
	['/scheduled/abc/edit', 'scheduled-edit-heading'],
	['/webhooks', 'webhooks-heading'],
	['/webhooks/abc/edit', 'webhooks-edit-heading'],
	['/webhooks/abc/rerun', 'webhooks-rerun-page-name'],
	['/webhooks/abc/runs', 'webhooks-runs-page-name'],
	['/integrations', 'integrations-heading'],
	['/skills', 'my-skills-heading'],
	['/tools', 'tools-heading'],
	['/tokens', 'tokens-page-heading'],
	['/admin/users', 'admin-users-heading'],
	['/admin/upstreams', 'upstreams-heading'],
	['/rag', 'rag-heading'],
	['/rag/profiles', 'rag-profile-heading'],
	['/admin/skills', 'skills-heading']
] as const;

for (const [path, key] of branded) {
	test(`${path} has its production browser title`, () => {
		assert.deepEqual(pageTitleDescriptor(path), { key, branded: true });
	});
}

const plain = [
	['/admin/tokens', 'admin-tokens-heading'],
	['/admin/groups', 'groups-heading'],
	['/admin/models', 'admin-page-title'],
	['/admin/connectors', 'connectors-page-title'],
	['/admin/comfyui', 'admin-comfyui-page-title'],
	['/admin/comfyui/jobs', 'admin-comfyui-jobs-page-title'],
	['/admin/connectors/new', 'connectors-add-page-title'],
	['/admin/connectors/atlassian/edit', 'connectors-edit-page-title'],
	['/admin/models/edit', 'admin-edit-model-page-title'],
	['/rag/new', 'rag-new-page-title'],
	['/rag/42/edit', 'rag-edit-page-title'],
	['/admin/limits', 'limits-heading'],
	['/admin/settings', 'settings-heading'],
	['/usage', 'usage-title-mine']
] as const;

for (const [path, key] of plain) {
	test(`${path} preserves its unbranded production title`, () => {
		assert.deepEqual(pageTitleDescriptor(path), { key, branded: false });
	});
}

test('connector audit pages defer to the connector name loaded by the page', () => {
	assert.equal(pageTitleDescriptor('/admin/connectors/discord/audit'), null);
});

test('dynamic and public pages keep their page-owned titles', () => {
	assert.equal(pageTitleDescriptor('/login'), null);
	assert.equal(pageTitleDescriptor('/setup'), null);
});
