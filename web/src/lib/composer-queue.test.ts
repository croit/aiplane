import test from 'node:test';
import assert from 'node:assert/strict';

import type { LiveTurn, SteerStatus } from './chat-protocol.ts';
import {
	deserializeQueue,
	draftEntry,
	moveEntry,
	nextSendable,
	removeEntry,
	serializeQueue,
	steerEntry,
	withUndeliveredSteers
} from './composer-queue.ts';

/** A finished assistant turn carrying interjections in the given states. */
const turnWithSteers = (states: [string, SteerStatus][]): LiveTurn => ({
	turn: {
		id: 'a1',
		session_id: 's1',
		seq: 1,
		role: 'assistant',
		user_content: null,
		model: 'm',
		content: 'partial',
		reasoning: null,
		reasoning_elapsed_ms: null,
		reasoning_started_at: null,
		status: 'completed',
		error_message: null,
		created_at: '',
		completed_at: null
	},
	tool_calls: [],
	steers: states.map(([text, status], seq) => ({
		id: `n${seq}`,
		turn_id: 'a1',
		seq,
		text,
		status,
		created_at: '',
		settled_at: null
	}))
});

test('entries keep the order they were typed in', () => {
	const queue = [draftEntry('first'), draftEntry('second')];
	assert.deepEqual(
		queue.map((e) => e.text),
		['first', 'second']
	);
});

test('an entry can be dropped before it is sent', () => {
	const [a, b] = [draftEntry('first'), draftEntry('second')];
	assert.deepEqual(
		removeEntry([a, b], a.id).map((e) => e.text),
		['second']
	);
});

// Order is the meaning of a queue of instructions, so it has to be
// correctable after the fact — "first X then Y" is a different request from
// its reverse.
test('an entry moves one place at a time and never off the ends', () => {
	const [a, b, c] = [draftEntry('a'), draftEntry('b'), draftEntry('c')];
	const queue = [a, b, c];
	assert.deepEqual(
		moveEntry(queue, c.id, -1).map((e) => e.text),
		['a', 'c', 'b']
	);
	assert.deepEqual(
		moveEntry(queue, a.id, -1).map((e) => e.text),
		['a', 'b', 'c'],
		'already first: unchanged'
	);
	assert.deepEqual(
		moveEntry(queue, c.id, 1).map((e) => e.text),
		['a', 'b', 'c'],
		'already last: unchanged'
	);
	assert.deepEqual(
		moveEntry(queue, 'nope', 1).map((e) => e.text),
		['a', 'b', 'c'],
		'unknown id: unchanged'
	);
});

// The whole point of the fallback: a note the model never got is not lost,
// it becomes the next message.
test('an interjection the turn never carried comes back as a message', () => {
	const turns = [turnWithSteers([['too late', 'pending']])];
	const queue = withUndeliveredSteers([], turns);
	assert.equal(queue.length, 1);
	assert.equal(queue[0].text, 'too late');
	assert.deepEqual(queue[0].redeemSteers, ['n0']);
});

// It was typed before whatever else is waiting, and it is about the answer
// that just ended: sending it after two later messages would read as a
// correction to the wrong thing.
test('a returned interjection goes to the front of the queue', () => {
	const queue = withUndeliveredSteers(
		[draftEntry('typed later')],
		[turnWithSteers([['in euros', 'pending']])]
	);
	assert.deepEqual(
		queue.map((e) => e.text),
		['in euros', 'typed later']
	);
});

test('interjections the model did read are not re-sent', () => {
	const turns = [
		turnWithSteers([
			['the model saw this', 'delivered'],
			['already re-sent', 'resent']
		])
	];
	assert.deepEqual(withUndeliveredSteers([], turns), []);
});

// Finalize can fire more than once per conversation (a retry, a second turn),
// and every firing re-reads the same rows: without the claim check the same
// sentence would be queued again each time.
test('a note already waiting in the queue is not queued twice', () => {
	const turns = [turnWithSteers([['too late', 'pending']])];
	const once = withUndeliveredSteers([], turns);
	const twice = withUndeliveredSteers(once, turns);
	assert.equal(twice.length, 1);
	assert.equal(twice, once, 'nothing to add: the same array comes back');
});

test('a queued message survives a reload, its attachments do not', () => {
	const file = new File(['x'], 'notes.txt');
	const queue = [draftEntry('with a file', [file]), steerEntry('n0', 'too late')];
	const restored = deserializeQueue(serializeQueue(queue));
	assert.deepEqual(
		restored.map((e) => e.text),
		['with a file', 'too late']
	);
	assert.deepEqual(restored[0].files, []);
	assert.deepEqual(restored[1].redeemSteers, ['n0'], 'a note keeps its claim across a reload');
});

// "Summarise the attached report" with no report is a guaranteed wrong answer,
// so an entry whose files could not be stored is held until the user deals
// with it — while the entries behind it still go out.
test('an entry that had attachments comes back held, and blocks only itself', () => {
	const file = new File(['x'], 'report.pdf');
	const queue = [draftEntry('summarise this', [file]), draftEntry('and then the sources')];
	const restored = deserializeQueue(serializeQueue(queue));
	assert.equal(restored[0].held, true);
	assert.equal(restored[1].held, undefined);
	assert.equal(nextSendable(restored)?.text, 'and then the sources');
	assert.equal(nextSendable(restored.slice(0, 1)), undefined, 'a held entry is never sendable');
	// Held survives a further reload: the file is not coming back on its own.
	assert.equal(deserializeQueue(serializeQueue(restored))[0].held, true);
});

test('an entry with no attachments is sendable in the order it was queued', () => {
	const queue = [draftEntry('first'), draftEntry('second')];
	assert.equal(nextSendable(queue)?.text, 'first');
	assert.equal(nextSendable([]), undefined);
});

// Browser storage is written by other tabs, extensions and older releases of
// this page, so everything read back out of it is treated as untrusted input.
test('junk in storage restores as an empty queue, never a crash', () => {
	assert.deepEqual(deserializeQueue(null), []);
	assert.deepEqual(deserializeQueue('not json'), []);
	assert.deepEqual(deserializeQueue('{"text":"not an array"}'), []);
	assert.deepEqual(deserializeQueue('[1, null, {"nope": true}]'), []);
	assert.deepEqual(deserializeQueue('[{"text": "   "}]'), [], 'a blank entry is nothing to send');
	const mixed = deserializeQueue('[{"text": "keep me", "redeemSteers": ["n0", 7]}]');
	assert.equal(mixed.length, 1);
	assert.deepEqual(mixed[0].redeemSteers, ['n0']);
});
