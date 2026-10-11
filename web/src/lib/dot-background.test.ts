import assert from 'node:assert/strict';
import test from 'node:test';
import { dotRunOptions } from './dot-background.ts';

const labels = { person: 'Personal assistant', inactive: 'Inactive', unavailable: (id: string) => `Unavailable Dot (${id})` };
const dots = [
	{ id: 'a', name: 'Harald', display: '', description: 'Research', active: true },
	{ id: 'b', name: 'Betty', display: 'Betty', description: '', active: false }
];

test('new bindings offer active owned Dots and preserve actual labels', () => {
	const options = dotRunOptions(dots, null, labels);
	assert.equal(options[0]?.value, '');
	assert.equal(options[1]?.label, 'Harald');
	assert.equal(options[1]?.description, 'Research');
	assert.equal(options[2]?.disabled, true);
});

test('an unchanged inactive binding remains selectable for unrelated edits', () => {
	const options = dotRunOptions(dots, 'b', labels);
	assert.equal(options.find((option) => option.value === 'b')?.disabled, false);
	assert.equal(options.find((option) => option.value === 'b')?.description, 'Inactive');
});

test('a deleted binding remains visibly unavailable without becoming ordinary execution', () => {
	const option = dotRunOptions(dots, 'deleted', labels).find((option) => option.value === 'deleted');
	assert.equal(option?.label, 'Unavailable Dot (deleted)');
	assert.equal(option?.value, 'deleted');
	assert.equal(option?.disabled, false);
});
