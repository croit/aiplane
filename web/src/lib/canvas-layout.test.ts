import test from 'node:test';
import assert from 'node:assert/strict';

import { canvasBounds, clampCanvasWidth } from './canvas-layout.ts';

test('canvas width is based on its container and preserves a usable chat column', () => {
	assert.deepEqual(canvasBounds(1280), { minimum: 320, maximum: 588, preferred: 486 });
	assert.deepEqual(canvasBounds(960), { minimum: 320, maximum: 388, preferred: 365 });
});

test('canvas width is capped even on a very wide display', () => {
	assert.deepEqual(canvasBounds(2560), { minimum: 320, maximum: 768, preferred: 600 });
});

test('stored canvas widths are clamped to the current container', () => {
	const bounds = canvasBounds(1280);
	assert.equal(clampCanvasWidth(1024, bounds), 588);
	assert.equal(clampCanvasWidth(100, bounds), 320);
});
