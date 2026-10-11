import test from 'node:test';
import assert from 'node:assert/strict';
import { actionRows, setActionRule, accessSelections } from './dot-action-rules.ts';
import type { DotAccessTool } from './dots.ts';

function tool(patch: Partial<DotAccessTool> = {}): DotAccessTool {
	return {
		id: 'browse_page', label: 'Browser', description: 'Actual browser tool', selected: true,
		permission: 'always_ask', read_only: false, identity: 'builtin', connector_key: null,
		connector_mode: null, actions: [{ id: 'read', description: 'Read current page' }, { id: 'fill', description: 'Fill a field' }],
		action_rules: [], ...patch
	};
}

test('catalogue actions inherit the tool rule unless explicitly overridden', () => {
	const current = tool();
	assert.deepEqual(actionRows(current), [
		{ id: 'read', description: 'Read current page', available: true, rule: null },
		{ id: 'fill', description: 'Fill a field', available: true, rule: null }
	]);
	const edited = setActionRule(setActionRule(current, 'read', 'allow'), 'fill', 'deny');
	assert.equal(edited.permission, 'always_ask');
	assert.deepEqual(accessSelections([edited]), [{ id: 'browse_page', permission: 'always_ask', actions: [
		{ id: 'read', rule: 'allow' }, { id: 'fill', rule: 'deny' }
	] }]);
	assert.deepEqual(current.action_rules, []);
});

test('restoring inheritance removes only the chosen override and preserves others', () => {
	const edited = tool({ action_rules: [{ id: 'read', rule: 'allow' }, { id: 'fill', rule: 'deny' }] });
	assert.deepEqual(setActionRule(edited, 'read', null).action_rules, [{ id: 'fill', rule: 'deny' }]);
});

test('unavailable saved actions remain visible and block replacement until explicitly removed', () => {
	const stale = tool({ action_rules: [{ id: 'removed_action', rule: 'deny' }] });
	assert.deepEqual(actionRows(stale).at(-1), { id: 'removed_action', description: null, available: false, rule: 'deny' });
	assert.equal(accessSelections([stale]), null);
	assert.deepEqual(accessSelections([setActionRule(stale, 'removed_action', null)]), [{ id: 'browse_page', permission: 'always_ask', actions: [] }]);
});

test('unknown saved rules require correction without replacing them with an allow default', () => {
	const invalid = tool({ action_rules: [{ id: 'read', rule: 'unknown_rule' }] });
	assert.equal(actionRows(invalid)[0].rule, 'unknown_rule');
	assert.equal(accessSelections([invalid]), null);
	assert.deepEqual(accessSelections([setActionRule(invalid, 'read', 'ask')]), [{ id: 'browse_page', permission: 'always_ask', actions: [{ id: 'read', rule: 'ask' }] }]);
});

test('unsupported tools do not have invented actions and deselection sends no stale rules', () => {
	assert.deepEqual(actionRows(tool({ actions: null })), []);
	const invalid = tool({ selected: false, actions: null, action_rules: [{ id: 'old', rule: 'deny' }] });
	assert.deepEqual(accessSelections([invalid]), []);
	assert.deepEqual(actionRows(invalid), [{ id: 'old', description: null, available: false, rule: 'deny' }]);
});
