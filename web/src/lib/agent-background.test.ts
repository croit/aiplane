import assert from 'node:assert/strict';
import test from 'node:test';
import { agentRunOptions } from './agent-background.ts';

const labels = { person: 'Personal assistant', inactive: 'Inactive', unavailable: (id: string) => `Unavailable agent (${id})` };
const agents = [
	{ id: 'a', name: 'Harald', display: '', description: 'Research', active: true },
	{ id: 'b', name: 'Betty', display: 'Betty', description: '', active: false }
];

test('new bindings offer active owned agents and preserve actual labels', () => {
	const options = agentRunOptions(agents, null, labels);
	assert.equal(options[0]?.value, '');
	assert.equal(options[1]?.label, 'Harald');
	assert.equal(options[1]?.description, 'Research');
	assert.equal(options[2]?.disabled, true);
});

test('an unchanged inactive binding remains selectable for unrelated edits', () => {
	const options = agentRunOptions(agents, 'b', labels);
	assert.equal(options.find((option) => option.value === 'b')?.disabled, false);
	assert.equal(options.find((option) => option.value === 'b')?.description, 'Inactive');
});

test('a deleted binding remains visibly unavailable without becoming ordinary execution', () => {
	const option = agentRunOptions(agents, 'deleted', labels).find((option) => option.value === 'deleted');
	assert.equal(option?.label, 'Unavailable agent (deleted)');
	assert.equal(option?.value, 'deleted');
	assert.equal(option?.disabled, false);
});
