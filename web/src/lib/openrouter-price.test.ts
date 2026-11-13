import assert from 'node:assert/strict';
import { test } from 'node:test';
import { formatOpenRouterPrice, openRouterPriceStep } from './openrouter-price.ts';

test('token prices keep the four-decimal per-million form', () => {
	assert.equal(formatOpenRouterPrice(0.27, 'token'), '$0.2700');
	assert.equal(formatOpenRouterPrice(3.33, 'token'), '$3.3300');
	assert.equal(formatOpenRouterPrice(null, 'token'), '—');
});

test('audio-second prices keep the digits that separate providers', () => {
	assert.equal(formatOpenRouterPrice(0.00000333, 'second'), '$0.00000333');
	assert.equal(formatOpenRouterPrice(0.0000111111111111, 'second'), '$0.000011');
	assert.equal(formatOpenRouterPrice(0.0001, 'second'), '$0.000100');
	assert.equal(formatOpenRouterPrice(0, 'second'), '$0.000000');
	assert.equal(formatOpenRouterPrice(null, 'second'), '—');
});

test('the ceiling step follows the unit', () => {
	assert.equal(openRouterPriceStep('token'), '0.0001');
	assert.equal(openRouterPriceStep('second'), '0.0000001');
});
