import assert from 'node:assert/strict';
import test from 'node:test';

import type { ToolCall } from './chat-protocol.ts';
import { changesAgent, conversationTitle, dotPlanCanApply, groupedDotTools, setupPath, toolLabel, undoTarget, type DotSetupPlan } from './architect.ts';

const call = (name: string, output: unknown, status: ToolCall['status'] = 'completed'): ToolCall => ({
	id: `${name}-1`,
	turn_id: 't',
	seq: 0,
	name,
	arguments_json: '{}',
	output_json: output === null ? null : JSON.stringify(output),
	status,
	created_at: '',
	completed_at: null
});

test('a completed draft change can be undone to the revision it kept', () => {
	const done = call('update_agent_draft', { agent_id: 'a1', revision: 7, setup_url: '/agents/a1' });
	assert.deepEqual(undoTarget(done), { agentId: 'a1', revision: 7 });
	assert.equal(undoTarget(call('update_agent_draft', { agent_id: 'a1', revision: null })), null, 'nothing changed');
	assert.equal(undoTarget(call('update_agent_draft', null, 'errored')), null);
	assert.equal(undoTarget(call('read_agent', { agent_id: 'a1', revision: 3 })), null);
	assert.equal(undoTarget({ ...done, output_json: 'not json' }), null);
});

test('only an app path from a call that names a setup page becomes a link', () => {
	assert.equal(setupPath(call('create_agent_draft', { setup_url: '/agents/a1' })), '/agents/a1');
	assert.equal(setupPath(call('create_agent_draft', { setup_url: 'https://evil.example/' })), null);
	assert.equal(setupPath(call('list_agents', { setup_url: '/agents/a1' })), null);
	assert.equal(setupPath(call('create_agent_draft', null, 'running')), null);
});

test('a page reloads after a create or a draft change, not after a read', () => {
	assert.ok(changesAgent(call('create_agent_draft', {})));
	assert.ok(changesAgent(call('update_agent_draft', {})));
	assert.ok(!changesAgent(call('update_agent_draft', null, 'errored')));
	assert.ok(!changesAgent(call('read_agent', {})));
});

test('the conversation is titled after the agent, or as a new one', () => {
	const tr = (key: string, args?: Record<string, string | number>) => `${key}${args ? `:${String(args.name)}` : ''}`;
	assert.equal(conversationTitle(tr, ' Harald '), 'architect-conversation-title:Harald');
	assert.equal(conversationTitle(tr, ''), 'architect-conversation-new');
	assert.equal(conversationTitle(tr, null), 'architect-conversation-new');
});

test('an architect tool reads as what it did, any other keeps its name', () => {
	const tr = (key: string) => `<${key}>`;
	assert.equal(toolLabel(tr)('run_test_turn'), '<architect-tool-run-test-turn>');
	assert.equal(toolLabel(tr)('list_grantable'), '<architect-tool-list-grantable>');
	assert.equal(toolLabel(tr)('propose_dot_setup'), '<architect-tool-propose-dot-setup>');
	assert.equal(toolLabel(tr)('fetch_url'), 'fetch_url');
});

test('Dot setup requires a completed reviewed plan without unresolved or dropped requirements', () => {
	const plan: DotSetupPlan = { revision: 'r1', name: 'Harald', display: 'Harald', description: '', draft_spec: {}, knowledge: null, tools: [], dropped: [], unresolved: [] };
	assert.equal(dotPlanCanApply(null, false, false), false);
	assert.equal(dotPlanCanApply(plan, false, false), true);
	assert.equal(dotPlanCanApply(plan, true, false), false, 'a running turn may replace the reviewed plan');
	assert.equal(dotPlanCanApply(plan, false, true), false, 'prevent duplicate submissions');
	assert.equal(dotPlanCanApply({ ...plan, unresolved: ['Connect the account'] }, false, false), false);
	assert.equal(dotPlanCanApply({ ...plan, dropped: [{ step: 'abilities', reason: 'Not authorized' }] }, false, false), false);
});

test('a Dot preview groups actual capability identities while retaining different approval rules', () => {
	const tools = [
		{ id: 'remember', capability_key: 'memory', label: 'Memory', permission: 'always_allow' },
		{ id: 'recall', capability_key: 'memory', label: 'Memory', permission: 'always_allow' },
		{ id: 'forget', capability_key: 'memory', label: 'Memory', permission: 'always_ask' },
		{ id: 'fetch_url', capability_key: 'fetch_url', label: 'Web', permission: 'always_allow' },
		{ id: 'search_web', capability_key: 'search_web', label: 'Web', permission: 'always_allow' }
	];
	const rows = groupedDotTools(tools);
	assert.equal(rows.length, 4, 'matching display names are not matching resource identities');
	assert.deepEqual(rows[0].tools.map((tool) => tool.id), ['remember', 'recall']);
	assert.equal(rows[1].permission, 'always_ask');
	assert.deepEqual(groupedDotTools([{ id: 'actual-tool', capability_key: 'actual-key', permission: 'always_ask' }])[0].label, 'actual-key');
});
