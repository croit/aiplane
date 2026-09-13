// The two pure decisions in the capture path.
//
// `captureScale` is the one that has actually been wrong before: a fixed 2×
// under-samples every modern phone (fractional DPR) and blows up a 4K desktop,
// so the rounding and the cap are the behaviour, not an implementation detail.

import test from 'node:test';
import assert from 'node:assert';
import { captureScale, stripDataUrlPrefix } from './feedback-screenshot.ts';

test('capture scale rounds a fractional DPR up and caps at 3', () => {
	assert.equal(captureScale(1), 1);
	assert.equal(captureScale(2), 2);
	// A phone at 2.4375 must gain pixels, not lose them.
	assert.equal(captureScale(2.4375), 3);
	// A 4K/high-DPR surface is capped so the canvas stays inside the timeout.
	assert.equal(captureScale(4), 3);
});

test('a nonsensical DPR falls back to 1 instead of producing an empty canvas', () => {
	assert.equal(captureScale(0), 1);
	assert.equal(captureScale(-2), 1);
	assert.equal(captureScale(Number.NaN), 1);
	assert.equal(captureScale(Number.POSITIVE_INFINITY), 1);
});

test('the data: prefix is stripped, and a bare base64 string is left alone', () => {
	assert.equal(stripDataUrlPrefix('data:image/png;base64,QUJD'), 'QUJD');
	assert.equal(stripDataUrlPrefix('QUJD'), 'QUJD');
});
