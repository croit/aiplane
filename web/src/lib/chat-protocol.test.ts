/**
 * Unit tests for the pure chat event protocol (run by `mise run test-web`
 * via `node --test`, Node's native type stripping — same setup as the
 * legacy `ui/ts` suite).
 */
import { test } from 'node:test';
import assert from 'node:assert/strict';

import {
	applyEvent,
	newConversationState,
	parseSseBlock,
	parseUserContent,
	sessionTitle,
	type ChatEvent,
	type ConversationState,
	type TurnWithTools
} from './chat-protocol.ts';

function turn(id: string, role: 'user' | 'assistant', extra: Partial<TurnWithTools['turn']> = {}): TurnWithTools {
	return {
		turn: {
			id,
			session_id: 's1',
			seq: 0,
			role,
			user_content: role === 'user' ? 'hello' : null,
			model: null,
			content: null,
			reasoning: null,
			reasoning_started_at: null,
			reasoning_elapsed_ms: null,
			status: 'completed',
			error_message: null,
			created_at: '2026-01-01T00:00:00Z',
			completed_at: null,
			...extra
		},
		tool_calls: []
	};
}

test('a snapshot replaces the whole conversation and marks liveness', () => {
	const state = newConversationState();
	applyEvent(state, {
		type: 'snapshot',
		live_turn_id: 'a2',
		turns: [turn('u1', 'user'), turn('a2', 'assistant', { status: 'in_progress' })]
	});
	assert.deepEqual(state.turns.map((t) => t.turn.id), ['u1', 'a2']);
	assert.equal(state.liveTurnId, 'a2');
	assert.equal(state.idle, false);

	applyEvent(state, { type: 'snapshot', turns: [turn('u1', 'user')] });
	assert.equal(state.liveTurnId, null);
	assert.equal(state.idle, true, 'no live turn on the wire means idle');
});

test('deltas append to their own turn buffer', () => {
	const state = newConversationState();
	applyEvent(state, { type: 'turn_delta', turn_id: 'a1', text_delta: 'Hel' });
	applyEvent(state, { type: 'reasoning_delta', turn_id: 'a1', text_delta: 'hmm' });
	applyEvent(state, { type: 'turn_delta', turn_id: 'a1', text_delta: 'lo' });
	const live = state.turns.find((t) => t.turn.id === 'a1');
	assert.equal(live?.turn.content, 'Hello');
	assert.equal(live?.turn.reasoning, 'hmm');
});

test('a flagged full-text delta replaces instead of appending', () => {
	const state = newConversationState();
	applyEvent(state, { type: 'turn_delta', turn_id: 'a1', text_delta: 'long first draft' });
	// Server reset its cursor and resent everything after a rewrite.
	applyEvent(state, { type: 'turn_delta', turn_id: 'a1', text_delta: 'short', full: true });
	applyEvent(state, { type: 'turn_delta', turn_id: 'a1', text_delta: ' + more' });
	assert.equal(state.turns.find((t) => t.turn.id === 'a1')?.turn.content, 'short + more');
});

test('tool calls announce, update, and never duplicate', () => {
	const state = newConversationState();
	applyEvent(state, {
		type: 'tool_call_started',
		turn_id: 'a1',
		tool_call_id: 'c1',
		name: 'search',
		arguments: '{}'
	});
	applyEvent(state, {
		type: 'tool_call_started',
		turn_id: 'a1',
		tool_call_id: 'c1',
		name: 'search',
		arguments: '{}'
	});
	assert.equal(state.turns.find((t) => t.turn.id === 'a1')?.tool_calls.length, 1);

	applyEvent(state, {
		type: 'tool_call_done',
		turn_id: 'a1',
		tool_call_id: 'c1',
		status: 'completed',
		output: '{"hits":1}'
	});
	const call = state.turns.find((t) => t.turn.id === 'a1')?.tool_calls[0];
	assert.equal(call?.status, 'completed');
	assert.equal(call?.output_json, '{"hits":1}');
});

test('finalizing the live turn makes the conversation idle', () => {
	const state = newConversationState();
	applyEvent(state, { type: 'snapshot', live_turn_id: 'a1', turns: [turn('a1', 'assistant', { status: 'in_progress' })] });
	applyEvent(state, { type: 'turn_delta', turn_id: 'a1', text_delta: 'done' });
	applyEvent(state, { type: 'turn_finalized', turn_id: 'a1', status: 'errored', error_message: 'upstream 502', duration_ms: 12 });
	const live = state.turns.find((t) => t.turn.id === 'a1');
	assert.equal(live?.turn.status, 'errored');
	assert.equal(live?.turn.error_message, 'upstream 502');
	assert.equal(state.liveTurnId, null);
	assert.equal(state.idle, true);
});

test('info banners and tool prompts set and clear', () => {
	const state = newConversationState();
	applyEvent(state, { type: 'info', message: 'vision fallback' });
	assert.equal(state.info, 'vision fallback');
	applyEvent(state, {
		type: 'tool_prompt',
		action: 'show',
		turn_id: 'a1',
		kind: 'ask_user',
		question: 'Which region?',
		options: ['eu', 'us']
	});
	assert.equal(state.prompt?.action === 'show' ? state.prompt.question : null, 'Which region?');
	applyEvent(state, { type: 'tool_prompt', action: 'hide', turn_id: 'a1' });
	assert.equal(state.prompt, null);
});

test('an SSE block parses into its event', () => {
	const event = parseSseBlock('event: turn_delta\ndata: {"type":"turn_delta","turn_id":"a1","text_delta":"hi"}\n');
	assert.deepEqual(event, { type: 'turn_delta', turn_id: 'a1', text_delta: 'hi' });
	assert.equal(parseSseBlock('garbage'), null);
});

test('session titles fall back to the first user line, truncated', () => {
	const long = 'a very long first line that goes past the sixty character limit for sure yes';
	const turns = [
		{ turn: { role: 'user', user_content: long } as never, tool_calls: [] }
	];
	assert.equal(
		sessionTitle({ title: null } as never, turns as never).endsWith('…'),
		true
	);
	assert.equal(
		sessionTitle({ title: '  ' } as never, [
			{ turn: { role: 'user', user_content: 'first line\nsecond' } as never, tool_calls: [] }
		] as never),
		'first line'
	);
});

test('attachment markers parse into chips and vanish from the text', () => {
	const content =
		'look at this\n\n[gw-attachment file="shot.png" mime="image/png" url="/chat/attachment/t1/shot.png" size=123]\n\nand this\n[gw-attachment file="data.csv" mime="text/csv" url="/chat/attachment/t1/data.csv" size=45]';
	const { text, attachments } = parseUserContent(content);
	assert.equal(text, 'look at this\n\nand this');
	assert.deepEqual(
		attachments.map((a) => [a.filename, a.mime, a.size]),
		[
			['shot.png', 'image/png', 123],
			['data.csv', 'text/csv', 45]
		]
	);
	assert.equal(attachments[0].url, '/chat/attachment/t1/shot.png');
});

test('content without markers parses to itself', () => {
	const { text, attachments } = parseUserContent('plain message');
	assert.equal(text, 'plain message');
	assert.equal(attachments.length, 0);
	assert.deepEqual(parseUserContent(null), { text: '', attachments: [] });
});
