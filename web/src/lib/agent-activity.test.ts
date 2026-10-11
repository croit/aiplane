import assert from 'node:assert/strict';
import test from 'node:test';

import {
	KIND_GROUPS,
	activityQuery,
	conversationsOf,
	exportQuery,
	groupByTurn,
	summarize,
	type ActivityEvent,
	type ActivityFilter
} from './agent-activity.ts';

const event = (over: Partial<ActivityEvent>): ActivityEvent => ({
	cursor: 1,
	id: 'e',
	kind: 'tool_call',
	ts: '2026-10-01T00:00:00Z',
	principal_id: 'p',
	actor_id: null,
	agent_id: 'p',
	version: 1,
	conversation_id: 's1',
	session_id: 's1',
	turn_id: 't1',
	round: 0,
	call_id: null,
	visitor_id: null,
	caller_id: null,
	duration_ms: null,
	run_chain: null,
	detail: {},
	chain_key: 'conversation:s1',
	seq: 1,
	prev_hash: null,
	hash: 'h',
	...over
});

const none: ActivityFilter = { conversation: '', group: 'all', from: '', to: '' };

test('the topic guard decision is filed once, with the routing decisions', () => {
	assert.ok((KIND_GROUPS.routing as readonly string[]).includes('scope_decision'));
	const filed = Object.values(KIND_GROUPS)
		.flat()
		.filter((kind) => kind === 'scope_decision');
	assert.equal(filed.length, 1);
});

test('a prompt-assistant call is filed once, with the management events', () => {
	assert.ok((KIND_GROUPS.management as readonly string[]).includes('assist_suggested'));
	const filed = Object.values(KIND_GROUPS)
		.flat()
		.filter((kind) => kind === 'assist_suggested');
	assert.equal(filed.length, 1);
});

test('the query names only the filters set, and a conversation reads oldest first', () => {
	assert.equal(activityQuery(none), '');
	assert.equal(
		activityQuery({ conversation: 's1', group: 'exchanges', from: '2026-10-01', to: '' }, 42),
		'conversation=s1&order=asc&kind=llm_exchange&from=2026-10-01&cursor=42'
	);
	assert.equal(activityQuery({ ...none, group: 'tools' }), 'kind=tool_call%2Ctool_result%2Cinjection_detected');
});

test('the export takes the same filters without the order', () => {
	assert.equal(exportQuery({ ...none, conversation: 's1', to: '2026-10-02' }), 'conversation=s1&to=2026-10-02');
});

test('events fold into consecutive turns, a sub-agent turn between its caller', () => {
	const groups = groupByTurn([
		event({ id: 'a', turn_id: 't1' }),
		event({ id: 'b', turn_id: 't1' }),
		event({ id: 'c', turn_id: 'sub' }),
		event({ id: 'd', turn_id: 't1' }),
		event({ id: 'e', turn_id: null })
	]);
	assert.deepEqual(
		groups.map((g) => [g.turn, g.events.map((e) => e.id)]),
		[
			['t1', ['a', 'b']],
			['sub', ['c']],
			['t1', ['d']],
			[null, ['e']]
		]
	);
});

test('conversations are listed once, in the order they appear', () => {
	assert.deepEqual(
		conversationsOf([
			event({ conversation_id: 's2' }),
			event({ conversation_id: null }),
			event({ conversation_id: 's1' }),
			event({ conversation_id: 's2' })
		]),
		['s2', 's1']
	);
});

test('an exchange sums up as its model, tokens and finish reason, or its error', () => {
	assert.deepEqual(
		summarize(
			event({
				kind: 'llm_exchange',
				detail: { model: 'alias', real_model: 'qwen', response: { finish_reason: 'stop', usage: { total_tokens: 12 } } }
			})
		),
		{ key: 'agents-act-sum-llm', args: { model: 'qwen', tokens: 12, finish: 'stop' } }
	);
	assert.equal(
		summarize(event({ kind: 'llm_exchange', detail: { model: 'm', error: 'upstream 500' } }))?.key,
		'agents-act-sum-llm-error'
	);
});

test('a long message is clipped and a kind without a line has none', () => {
	const long = 'x'.repeat(500);
	const line = summarize(event({ kind: 'turn_started', detail: { message: long } }));
	assert.equal(line?.key, 'agents-act-sum-message');
	assert.equal(String(line?.args.text).length, 120);
	assert.equal(summarize(event({ kind: 'grant_added' })), null);
	assert.equal(summarize(event({ kind: 'turn_started', detail: { resumed: true } }))?.key, 'agents-act-sum-resumed');
});

test('a personal Dot chat start has a localized activity summary', () => {
	assert.deepEqual(summarize(event({ kind: 'personal_dot_session_started' })), {
		key: 'dot-activity-session-started',
		args: {}
	});
});

test('a Dot communication permission change has a neutral localized summary without metadata labels', () => {
	assert.deepEqual(summarize(event({
		kind: 'personal_dot_delegation_changed',
		detail: { caller_dot_id: 'harald-id', receiver_dot_id: 'clara-id', outbound_enabled: true, inbound_enabled: false }
	})), { key: 'dot-activity-communication-changed', args: {} });
});
