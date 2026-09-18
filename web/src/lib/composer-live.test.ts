// Drift guard for the composer staying usable while a turn runs.
//
// The rules it pins are the ones that fail silently — the composer keeps
// looking right and quietly drops, duplicates or mis-routes what was typed:
//
//   1. The textarea is never `disabled`. That single attribute is what the
//      whole feature exists to remove.
//   2. There is exactly one submit path, and it does not decide where the
//      message goes. Placement is the server's call — it is the only party
//      that knows whether a worker is running — and a client that decided for
//      itself is how "sent" became a fact only one browser knew.
//   3. Sending clears the composer immediately and puts the text back if
//      nothing was accepted. Either half alone loses a message or sends it
//      twice.
//   4. Interjections render with what became of them. A note shown without
//      saying whether the model read it is the failure the four-value status
//      exists to prevent.

import test from 'node:test';
import assert from 'node:assert';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

const ROOT = new URL('../..', import.meta.url).pathname;
const read = (rel: string): string => readFileSync(join(ROOT, rel), 'utf8');
const conversation = read('src/lib/Conversation.svelte');
const api = read('src/lib/api.ts');

/** The composer's textarea element, start tag only. */
function composerTextarea(source: string): string {
	const start = source.indexOf('<textarea');
	assert.ok(start >= 0, 'the composer has a textarea');
	const end = source.indexOf('></textarea>', start);
	assert.ok(end > start, 'the textarea tag is closed');
	return source.slice(start, end);
}

const sendFn = /async function send\(\)[\s\S]*?\n\t\}/.exec(conversation)?.[0] ?? '';

test('the composer textarea is never disabled', () => {
	const tag = composerTextarea(conversation);
	assert.ok(tag.includes('bind:value={draft}'), 'sanity: found the composer textarea');
	assert.ok(
		!/\bdisabled=/.test(tag),
		'the composer must stay usable while a turn streams — that is the whole point'
	);
});

test('Enter means one thing again: send', () => {
	const onKeydown = /function onKeydown[\s\S]*?\n\t\}/.exec(conversation)?.[0] ?? '';
	assert.ok(onKeydown.includes('void send()'), 'Enter sends');
	assert.ok(
		!/metaKey|ctrlKey/.test(onKeydown),
		'no modifier picks a destination — the server places the message'
	);
});

// Two submit paths meant every refusal and every new field had to be
// remembered twice, and they had already drifted once.
test('there is exactly one submit path', () => {
	assert.ok(sendFn.length > 0, 'sanity: found send()');
	assert.ok(
		sendFn.includes('api.sendChatMessage(') && sendFn.includes('api.sendChatMessageWithFiles('),
		'send() submits both shapes'
	);
	// Voice mode has its own submit (it carries `voice: true`), which is the
	// only other caller allowed.
	assert.ok(
		(conversation.match(/api\.sendChatMessage\(/g) ?? []).length === 2,
		'only send() and the voice path submit a plain message'
	);
	assert.ok(
		(conversation.match(/api\.sendChatMessageWithFiles\(/g) ?? []).length === 1,
		'one place submits a message with attachments'
	);
	assert.ok(
		!sendFn.includes('FormData') && !sendFn.includes('fetch('),
		'the component builds no request body of its own — api.ts owns that, and with it the typed errors'
	);
});

// The client no longer runs a queue, so nothing in it may hold messages back,
// remember them across reloads, or coordinate between tabs.
test('the client keeps no outbox of its own', () => {
	for (const gone of [
		'composer-queue',
		'flushQueue',
		'nextSendable',
		'draftRedeem',
		'redeem_steers',
		'localStorage'
	]) {
		assert.ok(
			!conversation.includes(gone),
			`${gone} belongs to the browser-side queue that was removed`
		);
	}
	assert.ok(
		!api.includes('redeem_steers'),
		'the claim protocol went with it — the server re-queues what no round carried'
	);
});

// Clearing without restoring loses the text when the request fails; restoring
// without clearing invites sending the same message twice.
test('sending clears the composer and puts the text back only if nothing was accepted', () => {
	assert.ok(/draft = '';\s*\n\s*files = \[\];/.test(sendFn), 'the composer is cleared on send');
	// Prepended, not assigned: the composer stays live during a submit, so the
	// user may already be typing the next message — overwriting that would
	// lose what they wrote while waiting.
	assert.ok(
		/catch[\s\S]*?draft = draft\.trim\(\) \? `\$\{sentText\}\\n\$\{draft\}` : sentText;/.test(sendFn),
		'a failed send hands the text back without clobbering what was typed since'
	);
	assert.ok(
		/files = \[\.\.\.sentFiles, \.\.\.files\]/.test(sendFn),
		'and the same for attachments'
	);
	assert.ok(/catch[\s\S]*?notice = String\(err\)/.test(sendFn), 'and says what went wrong');
});

// Sending folds the text into the running turn, which only reaches the model
// if another round follows. Stopping straight afterwards removes that "if" —
// the server re-queues the undelivered addition as the next message.
test('interrupt-and-re-aim is send followed by stop, in that order', () => {
	const fn = /async function interruptAndReaim\(\)[\s\S]*?\n\t\}/.exec(conversation)?.[0] ?? '';
	assert.ok(fn.includes('await send()'), 'the text goes out');
	assert.ok(fn.includes('await stop()'), 'and the running answer is stopped');
	assert.ok(
		fn.indexOf('await send()') < fn.indexOf('await stop()'),
		'send first: stopping before it would leave nothing to re-queue'
	);
});

// The page does not poll for a waiting message to start: the stream stays
// open and the server hands over to the new worker when the scheduler starts
// it. A re-check timer here would be a poll standing in for an event the
// server already has.
test('a waiting message is not polled for', () => {
	assert.ok(!conversation.includes('waitingTimer'), 'no re-check timer');
	const chatJson = readFileSync(
		join(ROOT, '../crates/session-core/src/chat_json.rs'),
		'utf8'
	);
	assert.ok(
		chatJson.includes('stream_until_started'),
		'the server holds the stream open instead'
	);
});

// EventSource dispatches only to listeners registered for that event name, so
// a frame the controller does not name is silently dropped: interjections
// appeared for the sender (who re-attaches and gets a snapshot) and never for
// anyone else.
test('every event the server can send is subscribed to', () => {
	const controller = read('src/lib/chat.svelte.ts');
	const names = /const EVENT_NAMES = \[([\s\S]*?)\] as const/.exec(controller)?.[1] ?? '';
	const protocol = read('src/lib/chat-protocol.ts');
	const sent = [...protocol.matchAll(/type: '([a-z_]+)'/g)].map((m) => m[1]);
	assert.ok(sent.includes('steer'), 'sanity: the protocol knows the steer event');
	for (const name of new Set(sent)) {
		assert.ok(names.includes(`'${name}'`), `${name} is never listened for`);
	}
});

// `send` swallows its own errors (it hands the draft back and shows a notice),
// so stopping unconditionally would throw away the answer in flight *and* send
// nothing.
test('interrupting only stops the turn if the message actually went out', () => {
	const fn = /async function interruptAndReaim\(\)[\s\S]*?\n\t\}/.exec(conversation)?.[0] ?? '';
	assert.ok(/if \(await send\(\)\) await stop\(\)/.test(fn), 'the stop is gated on the send');
	assert.ok(
		/async function send\(\): Promise<boolean>/.test(conversation),
		'so send() has to report whether it succeeded'
	);
});

test('an interjection renders with what became of it', () => {
	const en = read('src/lib/locales/en.ts');
	for (const key of [
		'chat-steer-delivered',
		'chat-steer-resent',
		'chat-steer-pending',
		'chat-steer-discarded'
	]) {
		assert.ok(conversation.includes(key), `${key} is rendered`);
		assert.ok(en.includes(key), `${key} is translated`);
	}
});

test('the transcript draws interjections from the turn they belong to', () => {
	assert.ok(
		/\{#each entry\.steers as note \(note\.id\)\}/.test(conversation),
		'keyed on the row id so a status change re-labels rather than re-adds'
	);
});
