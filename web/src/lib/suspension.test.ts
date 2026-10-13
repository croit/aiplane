import test from 'node:test';
import assert from 'node:assert/strict';
import {
	answerField,
	answerFor,
	decisionButtons,
	minutesLeft,
	prettyArguments,
	slotLine,
	slotText,
	pausedTurn,
	submitAnswer,
	waitingFrom,
	waitingLead,
	type SuspensionView
} from './suspension.ts';

const tr = (key: string, args?: Record<string, string | number>) => (args ? `«${key} ${JSON.stringify(args)}»` : `«${key}»`);

test('a card offers exactly the decisions it was offered', () => {
	assert.deepEqual(decisionButtons({ kind: 'approval', options: ['allow_once', 'deny'] }), [
		{ decision: 'allow_once', key: 'suspension-approve', primary: true },
		{ decision: 'deny', key: 'suspension-deny', primary: false }
	]);
	assert.deepEqual(
		decisionButtons({ kind: 'approval', options: [] }),
		[],
		'a visitor waiting for staff gets no button, not even Deny'
	);
	assert.deepEqual(decisionButtons({ kind: 'human_answer', options: ['value', 'deny'] }), [
		{ decision: 'deny', key: 'suspension-decline', primary: false }
	]);
	assert.deepEqual(decisionButtons({ kind: 'secure_input', options: ['value'] }), []);
});

test('the lead says what the conversation waits for, in the chat and in the test chat', () => {
	assert.equal(waitingLead('approval', 'chat'), 'chat-waiting-approval');
	assert.equal(waitingLead('secure_input', 'chat'), 'chat-waiting-value');
	assert.equal(waitingLead('human_answer', 'chat'), 'chat-waiting-answer');
	assert.equal(waitingLead('browser_handover', 'chat'), 'chat-waiting-browser-handover');
	assert.equal(waitingLead('secure_input', 'test'), 'agents-test-waiting-secure-input');
	assert.equal(waitingLead('approval', 'test'), 'agents-test-waiting-approval');
	assert.equal(waitingLead('human_answer', 'test'), 'agents-test-waiting-human');
});

test('a conversation is held while one of its turns waits for a decision', () => {
	const live = (id: string, status: string) => ({ turn: { id, status } });
	assert.equal(pausedTurn([live('u1', 'completed'), live('a1', 'suspended')]), 'a1');
	assert.equal(pausedTurn([live('u1', 'completed'), live('a1', 'completed')]), null);
	assert.equal(pausedTurn([]), null);
});

test('a staff answer is typed in the clear, a visitor secret masked', () => {
	assert.deepEqual(answerField('human_answer'), {
		secret: false,
		label: 'suspension-answer-label',
		submit: 'suspension-send-answer'
	});
	assert.deepEqual(answerField('secure_input'), {
		secret: true,
		label: 'suspension-value-label',
		submit: 'suspension-send-value'
	});
});

test('a slot is named by its label, a managed slot by its catalog name, else by its key', () => {
	assert.deepEqual(slotLine({ slot: 'issue', label: 'What the visitor needs', value: 'refund' }, tr), {
		label: 'What the visitor needs',
		value: 'refund'
	});
	assert.deepEqual(slotLine({ slot: 'firma_name', value: { id: 1 } }, tr), { label: 'firma_name', value: '{"id":1}' });
	assert.equal(slotLine({ slot: 'topic', value: 'x' }, tr).label, '«agents-slot-label-topic»');
	assert.deepEqual(slotLine({ slot: 'verified_subject', label: '  ', set_by: 'host' }, tr), {
		label: 'verified_subject',
		value: '«suspension-slot-trusted {"by":"host"}»'
	});
});

test('a card from a suspended turn finds its call in the transcript', () => {
	const view: SuspensionView = {
		request_id: 'r1',
		kind: 'approval',
		tool_call_id: 'c2',
		tool: 'mcp__crm__delete_contact',
		options: ['allow_once', 'deny'],
		expires_at: '2026-10-02T10:30:00Z'
	};
	const calls = [
		{ id: 'c1', name: 'search', arguments_json: '{}' },
		{ id: 'c2', name: 'mcp__crm__delete_contact', arguments_json: '{"id":7}' }
	];
	assert.deepEqual(waitingFrom(view, calls).call, { name: 'mcp__crm__delete_contact', arguments: '{"id":7}' });
	assert.deepEqual(waitingFrom(view).call, { name: 'mcp__crm__delete_contact', arguments: '' });
	assert.equal(waitingFrom({ ...view, message: '“Weekly”: At 08:00, on Mon.' }).detail, '“Weekly”: At 08:00, on Mon.');
	const handoff = waitingFrom({ ...view, kind: 'human_answer', message: 'May we refund?', tool: undefined, tool_call_id: undefined }, [], {
		visitor_message: 'hi'
	});
	assert.equal(handoff.question, 'May we refund?');
	assert.equal(handoff.detail, null, "a hand-off's message is its question");
	assert.equal(handoff.call, null);
	assert.deepEqual(handoff.context, { visitor_message: 'hi' });
});

test('arguments are pretty when they are JSON and kept when they are not', () => {
	assert.equal(prettyArguments('{"a":1}'), '{\n  "a": 1\n}');
	assert.equal(prettyArguments('not json'), 'not json');
	assert.equal(slotText({ slot: 'x', set_by: 'host' }), null);
});

test('a value answer needs text, the other decisions do not', () => {
	assert.equal(answerFor('value', '  '), null);
	assert.deepEqual(answerFor('value', ' yes '), { decision: 'value', value: 'yes' });
	assert.deepEqual(answerFor('deny', 'ignored'), { decision: 'deny' });
	assert.deepEqual(answerFor('allow_once', ''), { decision: 'allow_once' });
});

test('the deadline counts down', () => {
	const now = Date.parse('2026-10-02T10:00:00Z');
	assert.equal(minutesLeft('2026-10-02T10:30:00Z', now), 30);
	assert.equal(minutesLeft('2026-10-02T09:00:00Z', now), 0);
	assert.equal(minutesLeft('garbage', now), null);
});

test('a typed answer is kept when sending it failed, and cleared once it went out', async () => {
	const sent: unknown[] = [];
	const ok = async (answer: unknown) => (sent.push(answer), true);
	const failed = async () => false;
	assert.deepEqual(await submitAnswer('value', ' 4711 ', ok), { text: '', missing: false });
	assert.deepEqual(sent, [{ decision: 'value', value: '4711' }]);
	assert.deepEqual(await submitAnswer('value', ' 4711 ', failed), { text: ' 4711 ', missing: false });
	assert.deepEqual(await submitAnswer('value', '  ', ok), { text: '  ', missing: true }, 'nothing to send');
	assert.equal(sent.length, 1);
	assert.deepEqual(await submitAnswer('deny', 'draft', failed), { text: 'draft', missing: false });
});
