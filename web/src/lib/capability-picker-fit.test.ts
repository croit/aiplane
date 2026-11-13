import test from 'node:test';
import assert from 'node:assert';
import { fittingCapabilities } from './capability-picker.ts';

const widths = [80, 90, 70];
const trigger = 90;
const more = 60;
const gap = 6;

test('every badge stays when the row has room for all of them', () => {
	// 90 + 6+80 + 6+90 + 6+70 = 348
	assert.equal(fittingCapabilities(widths, trigger, more, 1000, gap), 3);
	assert.equal(fittingCapabilities(widths, trigger, more, 348, gap), 3);
});

test('the last badges fold into one "+N more" that keeps its own room', () => {
	// One badge fits: 90 + 6+80 = 176, plus 6+60 for the fold = 242.
	assert.equal(fittingCapabilities(widths, trigger, more, 242, gap), 1);
	assert.equal(fittingCapabilities(widths, trigger, more, 241, gap), 0);
	// Two fit: 90 + 86 + 96 = 272, plus 66 for the fold = 338.
	assert.equal(fittingCapabilities(widths, trigger, more, 338, gap), 2);
	assert.equal(fittingCapabilities(widths, trigger, more, 337, gap), 1);
	// All three fit without a fold, so the fold's width is not reserved.
	assert.equal(fittingCapabilities(widths, trigger, more, 347, gap), 2);
});

test('a row narrower than its trigger folds everything', () => {
	assert.equal(fittingCapabilities(widths, trigger, more, 90, gap), 0);
	assert.equal(fittingCapabilities(widths, trigger, more, 0, gap), 0);
	assert.equal(fittingCapabilities([], trigger, more, 400, gap), 0);
});
