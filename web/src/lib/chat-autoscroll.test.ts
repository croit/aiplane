import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

import {
	distanceFromBottom,
	endScrollTop,
	nextFollow,
	shouldFollow,
	stickyThreshold
} from './chat-autoscroll.ts';

const ROOT = new URL('../..', import.meta.url).pathname;
const read = (rel: string): string => readFileSync(join(ROOT, rel), 'utf8');

/** A 900px window over a 5000px transcript, scrolled to `scrollTop`. */
const at = (scrollTop: number) => ({ scrollTop, scrollHeight: 5000, clientHeight: 900 });

test('the end of the transcript is where the last line is in view', () => {
	assert.equal(endScrollTop(at(0)), 4100);
	assert.equal(distanceFromBottom(at(4100)), 0);
	assert.equal(distanceFromBottom(at(4000)), 100);
});

test('the slack scales with the window, with a floor for a short one', () => {
	assert.equal(stickyThreshold(900), 90);
	assert.equal(stickyThreshold(2000), 200);
	assert.equal(stickyThreshold(400), 64);
});

test('sitting at the end follows what arrives', () => {
	assert.equal(shouldFollow(at(4100)), true);
});

test('a nudge off the end still follows', () => {
	assert.equal(shouldFollow(at(4100 - 89)), true);
});

// The half the reader actually notices: scrolled back into the conversation,
// a streaming reply must not drag the page along behind them.
test('scrolled back into the conversation stops following', () => {
	assert.equal(shouldFollow(at(4100 - 91)), false);
	assert.equal(shouldFollow(at(0)), false);
});

test('returning to the end starts following again', () => {
	assert.equal(shouldFollow(at(1000)), false);
	assert.equal(shouldFollow(at(4100)), true);
});

test('a transcript that fits its window always follows', () => {
	assert.equal(shouldFollow({ scrollTop: 0, scrollHeight: 400, clientHeight: 900 }), true);
});

// Overscroll (rubber-banding on macOS/iOS) reports a scrollTop past the end,
// which a naive subtraction turns into a negative distance — harmless here,
// but only because the distance is clamped.
test('overscrolling past the end is still the end', () => {
	assert.equal(distanceFromBottom(at(4200)), 0);
	assert.equal(shouldFollow(at(4200)), true);
});

test('scrolling back into the conversation is what stops the following', () => {
	assert.equal(nextFollow(true, 4100, at(2000)), false);
	assert.equal(nextFollow(true, 4100, at(4050)), true, 'a nudge is not leaving');
});

test('reaching the end again re-arms the following', () => {
	assert.equal(nextFollow(false, 1000, at(4100)), true);
	assert.equal(nextFollow(false, 1000, at(2000)), false, 'still mid-conversation');
});

// The regression: our own scroll to the end is reported a frame late, by
// which time the reply has grown past it. Read as a fresh position that is no
// longer the end, it switched following off one message after every send.
test('a stale report of our own scroll never cancels the following', () => {
	const grown = { scrollTop: 874, scrollHeight: 1423, clientHeight: 325 };
	assert.equal(shouldFollow(grown), false, 'the stale position really is short of the end');
	assert.equal(nextFollow(true, 0, grown), true);
});

// The rules above only matter if the view is wired to them: the transcript
// has to report its scrolling, follow its own growth, and re-arm on send.
// Each is one line, and each fails silently — the transcript simply stops
// following again, which is the bug this module was written for.
test('the transcript is wired to follow its own content', () => {
	const view = read('src/lib/Conversation.svelte');
	assert.ok(view.includes("from '$lib/chat-autoscroll'"), 'the rules are not imported');
	const tag = view.slice(view.indexOf('<main'), view.indexOf('>', view.indexOf('<main')) + 1);
	assert.ok(tag.includes('data-chat-transcript'), `not the transcript element: ${tag}`);
	assert.ok(tag.includes('onscroll='), 'the transcript does not report its scrolling');
	assert.ok(tag.includes('bind:this='), 'the transcript element is never captured');
	assert.ok(view.includes('new ResizeObserver('), 'nothing watches the transcript growing');
});

test('sending re-arms following so the new message is on screen', () => {
	const view = read('src/lib/Conversation.svelte');
	for (const [name, body] of functionBodies(view)) {
		if (name !== 'send' && name !== 'submitVoiceTurn') continue;
		assert.match(body, /followEnd\(\)/, `${name} must put the sent message on screen`);
	}
});

/** Crude but sufficient: each `async function x(` up to the next one. */
function functionBodies(source: string): [string, string][] {
	const heads = [...source.matchAll(/\basync function (\w+)\(/g)];
	return heads.map((match, index) => [
		match[1],
		source.slice(match.index, heads[index + 1]?.index ?? source.length)
	]);
}
