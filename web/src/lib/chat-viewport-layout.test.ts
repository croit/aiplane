import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const conversation = readFileSync(new URL('./Conversation.svelte', import.meta.url), 'utf8');
const shell = readFileSync(new URL('../routes/+layout.svelte', import.meta.url), 'utf8');

test('the application shell gives chat its own bounded viewport', () => {
	assert.match(shell, /h-dvh overflow-hidden/);
	assert.match(shell, /isChatActive\(\) \? 'overflow-hidden'/);
	assert.match(shell, /isChatActive\(\) \? 'h-full/);
});

test('the transcript scrolls independently while the full-width composer stays outside it', () => {
	const transcript = conversation.indexOf('data-chat-transcript');
	const canvas = conversation.indexOf('<ConversationCanvas');
	const composer = conversation.indexOf('data-chat-composer');

	assert.ok(transcript >= 0);
	assert.ok(canvas > transcript);
	assert.ok(composer > canvas);
	assert.match(conversation, /data-chat-transcript[\s\S]*?overflow-y-auto/);
	assert.match(conversation, /data-chat-composer[\s\S]*?w-full/);
});
