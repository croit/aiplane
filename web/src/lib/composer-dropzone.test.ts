// Wiring guard for drag-and-drop attachment staging.
//
// Every failure mode here is silent — the app looks fine and the file just
// doesn't attach (or worse, the browser navigates the tab to it and the
// conversation is gone) — so the contract is pinned as a grep over the markup
// rather than left to a rendered-DOM test that would need a browser to drive a
// real drag in the first place. The logic behind the handlers has its own unit
// tests in `drop-files.test.ts`.
//
// The assertions are `assert.ok` with a message on purpose: a failed
// `assert.match` against a 40 KB component dumps the whole component.

import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

import { en } from './locales/en.ts';

const conversation = readFileSync(new URL('./Conversation.svelte', import.meta.url), 'utf8');

const has = (pattern: RegExp, what: string) =>
	assert.ok(pattern.test(conversation), `Conversation.svelte no longer ${what}`);

test('the whole conversation is the drop target, not just the composer box', () => {
	has(/data-chat-dropzone/, 'marks a page-wide dropzone');
	const zone = conversation.slice(conversation.indexOf('data-chat-dropzone'));
	for (const handler of [
		'ondragenter={onDragEnter}',
		'ondragover={onDragOver}',
		'ondragleave={onDragLeave}',
		'ondrop={onDrop}'
	]) {
		assert.ok(zone.includes(handler), `the dropzone is missing ${handler}`);
	}
});

test('dragover is prevented, or no drop event ever fires', () => {
	// The browser's default for dragover is "this is not a drop target"; a
	// handler that forgets preventDefault() shows the overlay and then eats
	// the file. On drop it is what stops the tab navigating to the file.
	has(/function onDragOver[\s\S]{0,400}e\.preventDefault\(\)/, 'prevents the dragover default');
	has(/function onDrop[\s\S]{0,400}e\.preventDefault\(\)/, 'prevents the drop default');
});

test('a drop is staged exactly once', () => {
	// The composer textarea used to carry its own ondrop. Left in place
	// alongside the page-wide one, a drop on it bubbles to both handlers and
	// the file attaches twice.
	const elements = conversation.match(/<textarea[\s\S]*?<\/textarea>/g) ?? [];
	assert.ok(elements.length > 0, 'expected the composer textarea');
	for (const element of elements) {
		assert.ok(!/ondrop=/.test(element), 'a textarea still handles ondrop itself');
	}
});

test('the overlay cannot intercept the drop it is advertising', () => {
	const overlay = conversation.indexOf('data-chat-drop-overlay');
	assert.ok(overlay > 0, 'expected the drag overlay');
	assert.ok(
		/pointer-events-none/.test(conversation.slice(overlay, overlay + 400)),
		'the overlay would swallow the drag events it is drawn for'
	);
});

test('the edit dialog stages into the message being edited', () => {
	// The dialog renders outside the dropzone element, so it needs handlers of
	// its own; `stageFiles` is what routes them to `editFiles`.
	const dialog = conversation.slice(conversation.indexOf('{#if editingTurn}'), -1).slice(0, 900);
	assert.ok(/ondrop=\{onDrop\}/.test(dialog), 'the edit dialog no longer accepts a drop');
	has(
		/function stageFiles\([\s\S]{0,300}if \(editingTurn\) editFiles =/,
		'routes a drop to the message being edited'
	);
});

test('the drag overlay speaks the user language', () => {
	for (const key of [
		'render-drop-overlay',
		'render-drop-overlay-hint',
		'render-drop-folders-unsupported'
	]) {
		assert.ok(key in en, `${key} is missing from the catalog`);
		has(new RegExp(`t\\('${key}'\\)`), `uses ${key}`);
	}
});
