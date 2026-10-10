import test from 'node:test';
import assert from 'node:assert/strict';
import { activeDotCount, canCreateDot, type Dot } from './dot-state.ts';

const dot = (active: boolean): Dot => ({
	id: 'dot',
	name: 'assistant',
	display: 'Assistant',
	description: '',
	active
});

test('activeDotCount counts enabled Dots only', () => {
	assert.equal(activeDotCount([dot(true), dot(false), dot(true)]), 2);
	assert.equal(activeDotCount([]), 0);
});

test('canCreateDot is disabled at the active limit', () => {
	assert.equal(canCreateDot(3, 4), true);
	assert.equal(canCreateDot(4, 4), false);
	assert.equal(canCreateDot(5, 4), false);
});
