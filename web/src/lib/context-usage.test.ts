import test from 'node:test';
import assert from 'node:assert';
import { contextUsage } from './context-usage.ts';

test('a conversation nothing has measured yet shows nothing', () => {
	assert.equal(contextUsage({ usedTokens: null, window: 131_072, compactsAt: 111_411 }), null);
	assert.equal(contextUsage({ usedTokens: 0, window: 131_072, compactsAt: 111_411 }), null);
});

test('the share is a whole percent of the model context window', () => {
	assert.deepEqual(contextUsage({ usedTokens: 12_430, window: 131_072, compactsAt: 111_411 }), {
		tokens: 12_430,
		window: 131_072,
		percent: 9,
		compactsAt: 111_411,
		folding: false
	});
	assert.equal(
		contextUsage({ usedTokens: 65_536, window: 65_536, compactsAt: 49_536 })?.percent,
		100
	);
});

test('a route without a single window reports its size without a share', () => {
	assert.deepEqual(contextUsage({ usedTokens: 12_430, window: null, compactsAt: null }), {
		tokens: 12_430,
		window: null,
		percent: null,
		compactsAt: null,
		folding: false
	});
	// A window of zero is no window, not a divide-by-zero.
	assert.equal(contextUsage({ usedTokens: 12_430, window: 0, compactsAt: 0 })?.percent, null);
});

test('folding starts at the ceiling the compaction trigger uses', () => {
	const at = (used: number, compactsAt: number | null) =>
		contextUsage({ usedTokens: used, window: 100_000, compactsAt })?.folding;
	assert.equal(at(83_999, 84_000), false);
	assert.equal(at(84_000, 84_000), true);
	assert.equal(at(99_000, 84_000), true);
	// Compaction switched off: nothing will ever fold, whatever the size.
	assert.equal(at(99_000, null), false);
	assert.equal(at(99_000, 0), false);
});

test('an oversized prompt reads as more than a full window rather than a capped lie', () => {
	const usage = contextUsage({ usedTokens: 120_000, window: 100_000, compactsAt: 84_000 });
	assert.equal(usage?.percent, 120);
	assert.equal(usage?.folding, true);
});
