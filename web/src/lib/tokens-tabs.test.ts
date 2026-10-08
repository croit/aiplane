import test from 'node:test';
import assert from 'node:assert/strict';
import { selectedTokenTab, selectedGuideTab } from './tokens-tabs.ts';

test('tokens page defaults to management and accepts only known tabs', () => {
	assert.equal(selectedTokenTab(''), 'tokens');
	assert.equal(selectedTokenTab('?tab=guides'), 'guides');
	assert.equal(selectedTokenTab('?tab=account'), 'tokens');
	assert.equal(selectedTokenTab('?tab=unknown'), 'tokens');
});

test('guides default to OpenCode and accept only known clients', () => {
	assert.equal(selectedGuideTab(''), 'opencode');
	assert.equal(selectedGuideTab('?client=claude'), 'claude');
	assert.equal(selectedGuideTab('?client=pi'), 'pi');
	assert.equal(selectedGuideTab('?client=omp'), 'omp');
	assert.equal(selectedGuideTab('?client=python'), 'python');
	assert.equal(selectedGuideTab('?client=openwebui'), 'opencode');
	assert.equal(selectedGuideTab('?client=unknown'), 'opencode');
});
