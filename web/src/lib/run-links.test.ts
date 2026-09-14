import assert from 'node:assert/strict';
import test from 'node:test';

import { runLinks, type RunLinkSource } from './run-links.ts';

const source = (fields: Partial<RunLinkSource>): RunLinkSource => ({
	id: 'row-1',
	last_session_id: null,
	run_count: 0,
	chat_count: 0,
	...fields
});

test('something that has never fired offers nothing to open', () => {
	assert.deepEqual(runLinks(source({}), '/scheduled'), { chat: null, runs: null });
});

test('one run that opened one chat links straight to it, with no history detour', () => {
	assert.deepEqual(runLinks(source({ run_count: 1, chat_count: 1, last_session_id: 'sess-1' }), '/scheduled'), {
		chat: '/chat/sess-1',
		runs: null
	});
});

test('a reusing row keeps the direct chat link however often it has fired', () => {
	// Many runs, one conversation: the run list would be the same link twelve
	// times, so the chat stays one click away and the history stays available
	// for *when* each fire happened.
	assert.deepEqual(
		runLinks(source({ run_count: 12, chat_count: 1, last_session_id: 'sess-1' }), '/scheduled'),
		{ chat: '/chat/sess-1', runs: '/scheduled/row-1/runs' }
	);
});

test('a row that opens a fresh chat each time links to the list, not to one of them', () => {
	assert.deepEqual(runLinks(source({ run_count: 4, chat_count: 4, last_session_id: 'sess-4' }), '/scheduled'), {
		chat: null,
		runs: '/scheduled/row-1/runs'
	});
});

test('runs that produced no chat are still reachable', () => {
	// Over quota, or the model never answered: no session, but the failure is
	// the thing the owner came to find.
	assert.deepEqual(runLinks(source({ run_count: 2, chat_count: 0 }), '/scheduled'), {
		chat: null,
		runs: '/scheduled/row-1/runs'
	});
});

test('the same rule serves webhooks, under their own section', () => {
	// The two pages must not teach the user two different rules.
	assert.deepEqual(runLinks(source({ run_count: 4, chat_count: 4, last_session_id: 'sess-4' }), '/webhooks'), {
		chat: null,
		runs: '/webhooks/row-1/runs'
	});
	assert.deepEqual(runLinks(source({ run_count: 1, chat_count: 1, last_session_id: 'sess-1' }), '/webhooks'), {
		chat: '/chat/sess-1',
		runs: null
	});
});

test('a webhook keeps its history after one fire, because a run records the payload', () => {
	// The asymmetry with a schedule: a webhook run stores the request payload
	// and the rendered prompt, so "the chat link already shows everything" is
	// false for it even when there is exactly one of each.
	assert.deepEqual(
		runLinks(source({ run_count: 1, chat_count: 1, last_session_id: 'sess-1' }), '/webhooks', {
			runsCarryMore: true
		}),
		{ chat: '/chat/sess-1', runs: '/webhooks/row-1/runs' }
	);
	// And still nothing to reach when it has never fired.
	assert.deepEqual(runLinks(source({}), '/webhooks', { runsCarryMore: true }), {
		chat: null,
		runs: null
	});
});
