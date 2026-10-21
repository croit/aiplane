import assert from 'node:assert/strict';
import test from 'node:test';
import { formatModelPrice } from './model-price.ts';

const n = (value: number, options?: Intl.NumberFormatOptions) =>
	new Intl.NumberFormat('en', options).format(value);

/** A translator that returns the key and records what it was asked for. */
function mockT() {
	const calls: Array<[string, Record<string, string | number> | undefined]> = [];
	const t = (key: string, args?: Record<string, string | number>) => {
		calls.push([key, args]);
		return key;
	};
	return { t, calls };
}

test('no price to show (a route) yields no label', () => {
	assert.equal(formatModelPrice(null, 'USD', () => 'x', n), null);
	assert.equal(formatModelPrice(undefined, 'USD', () => 'x', n), null);
});

test('an unpriced model reads as free', () => {
	const { t, calls } = mockT();
	assert.equal(formatModelPrice({ input: null, output: null, unit: 'tokens' }, 'USD', t, n), 'tokens-models-price-free');
	assert.equal(calls[0][0], 'tokens-models-price-free');
});

test('a token-priced model formats in and out per million', () => {
	const { t, calls } = mockT();
	formatModelPrice({ input: 3, output: 15, unit: 'tokens' }, 'USD', t, n);
	assert.equal(calls[0][0], 'tokens-models-price-tokens');
	assert.equal(calls[0][1]?.input, '3 USD');
	assert.equal(calls[0][1]?.output, '15 USD');
});

test('a non-token unit prices per unit from the output side', () => {
	const { t, calls } = mockT();
	formatModelPrice({ input: null, output: 0.04, unit: 'images' }, 'USD', t, n);
	assert.equal(calls[0][0], 'tokens-models-price-per-images');
	assert.equal(calls[0][1]?.price, '0.04 USD');
});
