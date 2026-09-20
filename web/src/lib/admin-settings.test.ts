import test from 'node:test';
import assert from 'node:assert/strict';
import { categorySummary, fieldDraft, selectedSettingsCategory } from './admin-settings.ts';

test('settings category selection is bookmarkable and falls back to chat', () => {
	assert.equal(selectedSettingsCategory('?tab=tools'), 'tools');
	assert.equal(selectedSettingsCategory('?tab=web-search'), 'web-search');
	assert.equal(selectedSettingsCategory('?tab=unknown'), 'chat');
	assert.equal(selectedSettingsCategory(''), 'chat');
});

test('settings category summaries count only switchable sections', () => {
	assert.deepEqual(categorySummary([{ enabled: true }, { enabled: false }, { enabled: null }]), { on: 1, switchable: 2 });
});

test('setting drafts preserve booleans and make stored lists editable', () => {
	assert.equal(fieldDraft({ kind: 'bool', value: 'true' }), 'true');
	assert.equal(fieldDraft({ kind: 'list', value: '["feedback","support"]' }), 'feedback, support');
	assert.equal(fieldDraft({ kind: 'list', value: 'hand-edited' }), 'hand-edited');
});
