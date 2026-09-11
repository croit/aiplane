import assert from 'node:assert/strict';
import test from 'node:test';

import { limitPercent, usageCost, usageInteger, usageSearch } from './usage.ts';

test('usageSearch keeps every selected filter and omits empty values', () => {
	assert.equal(
		usageSearch({
			period: '24h',
			scope: 'all',
			source: 'chat',
			backend: 'croit',
			token: ''
		}),
		'?period=24h&scope=all&source=chat&backend=croit'
	);
});

test('limitPercent clamps invalid, exceeded, and partial limits', () => {
	assert.equal(limitPercent(25, 100), 25);
	assert.equal(limitPercent(120, 100), 100);
	assert.equal(limitPercent(1, 0), 100);
});

test('usage values retain the compact production grouping and currency', () => {
	assert.equal(usageInteger(1234567), '1\u202f234\u202f567');
	assert.equal(usageInteger(-1000), '-1\u202f000');
	assert.equal(usageCost(1234.5, 'USD'), '1\u202f234.50\u202fUSD');
});
