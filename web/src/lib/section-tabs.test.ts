import test from 'node:test';
import assert from 'node:assert/strict';
import { sectionTabs, selectedSectionTab } from './section-tabs.ts';

test('tools tabs link to each user capability page', () => {
	assert.deepEqual(sectionTabs.tools.map(({ path }) => path), ['/tools', '/tools/integrations', '/tools/skills', '/tools/browser']);
});

test('settings tabs keep account, notifications, memory, and tokens separate', () => {
	assert.deepEqual(sectionTabs.settings.map(({ path }) => path), ['/settings', '/settings/notifications', '/settings/memory', '/settings/tokens']);
});

test('admin access tabs keep users, tokens, groups, and limits separate', () => {
	assert.deepEqual(sectionTabs.adminAccess.map(({ path }) => path), ['/admin/organizations', '/admin/users', '/admin/tokens', '/admin/groups', '/admin/limits']);
});

test('tab selection uses exact paths', () => {
	assert.equal(selectedSectionTab('/tools/skills', sectionTabs.tools)?.path, '/tools/skills');
	assert.equal(selectedSectionTab('/admin/skills', sectionTabs.tools), undefined);
	assert.equal(selectedSectionTab('/settings/tokens/other', sectionTabs.settings), undefined);
});

test('admin organization metadata reuses the existing access navigation', () => {
    assert.equal(selectedSectionTab('/admin/organizations', sectionTabs.adminAccess)?.label, 'organizations-heading');
});
