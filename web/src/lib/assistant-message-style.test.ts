import { readFileSync } from 'node:fs';
import assert from 'node:assert/strict';
import test from 'node:test';

const conversation = readFileSync(new URL('./Conversation.svelte', import.meta.url), 'utf8');

test('assistant messages use a subtle theme-aware surface instead of the bright ghost bubble', () => {
	assert.doesNotMatch(conversation, /chat-bubble-ghost/);
	assert.match(conversation, /border-base-300\/60 bg-base-200\/35/);
});

test('user messages use a translucent theme-aware surface instead of a solid primary bubble', () => {
	assert.doesNotMatch(conversation, /chat-bubble-primary/);
	assert.match(conversation, /border-primary\/20 bg-primary\/10 text-base-content/);
});
