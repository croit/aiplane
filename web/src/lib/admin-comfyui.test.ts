import test from 'node:test';
import assert from 'node:assert/strict';
import { comfyuiJobPresentation, shortPromptId } from './admin-comfyui.ts';

test('ComfyUI job states retain the production status semantics', () => {
	assert.deepEqual(comfyuiJobPresentation('pending'), { icon: '⏳', badge: 'badge-warning' });
	assert.deepEqual(comfyuiJobPresentation('completed'), { icon: '✅', badge: 'badge-success' });
	assert.deepEqual(comfyuiJobPresentation('timeout'), { icon: '⚠️', badge: 'badge-error' });
	assert.deepEqual(comfyuiJobPresentation('failed'), { icon: '⚠️', badge: 'badge-error' });
});

test('long ComfyUI prompt ids use the legacy twelve-character summary', () => {
	assert.equal(shortPromptId('1234567890123456'), '123456789012…');
	assert.equal(shortPromptId('short'), 'short');
});
