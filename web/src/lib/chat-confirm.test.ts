// Wiring guard: the chat surface has no native browser dialogs.
//
// Chrome auto-dismisses a window.confirm when the browser window has not
// (re-)gained OS focus — on macOS the click that opens it often lands mid
// focus-transfer, so the dialog flashed and the destructive action (save &
// regenerate, retry, attachment removal) silently no-op'd. The confirmations
// are two-step buttons in the UI instead; this pins the wiring so a native
// dialog cannot creep back in.

import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const conversation = readFileSync(new URL('./Conversation.svelte', import.meta.url), 'utf8');
const attachments = readFileSync(new URL('./components/chat/MessageAttachments.svelte', import.meta.url), 'utf8');

const NATIVE_CONFIRM = /window\.confirm\(|[^.\w]confirm\(/;

test('the chat surface shows no native browser dialog', () => {
	assert.ok(!NATIVE_CONFIRM.test(conversation), 'Conversation.svelte opens a native confirm dialog');
	assert.ok(!NATIVE_CONFIRM.test(attachments), 'MessageAttachments.svelte opens a native confirm dialog');
});

test('save & regenerate is an in-UI two-step', () => {
	assert.ok(
		/let editConfirming = \$state\(false\)/.test(conversation),
		'the edit dialog has no armed state'
	);
	assert.ok(
		/editConfirming \? t\('render-edit-confirm'\) : t\('render-edit-save'\)/.test(conversation),
		'the save button does not switch to the confirmation wording'
	);
});

test('retry is an in-UI two-step', () => {
	assert.ok(
		/let retryConfirming = \$state<string \| null>\(null\)/.test(conversation),
		'the retry button has no armed turn'
	);
	assert.ok(
		/retryConfirming === entry\.turn\.id \? t\('render-retry-confirm'\) : t\('render-retry-button'\)/.test(
			conversation
		),
		'the retry button does not switch to the confirmation wording'
	);
});

test('removing an attachment is an in-UI two-step', () => {
	assert.ok(/let armed = \$state<string \| null>\(null\)/.test(attachments), 'the remove button has no armed state');
	assert.ok(
		/function handleRemove\(filename: string\) \{\s*if \(armed !== filename\) \{\s*armed = filename;\s*return;\s*\}\s*armed = null;\s*onremove\(filename\)/.test(
			attachments
		),
		'the remove button fires onremove without a confirming second click'
	);
});
