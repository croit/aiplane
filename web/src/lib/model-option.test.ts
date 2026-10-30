import assert from 'node:assert/strict';
import test from 'node:test';
import { modelSelectOptions } from './model-option.ts';

test('model options carry the id, the description and no badge without a price', () => {
	assert.deepEqual(
		modelSelectOptions([{ id: 'plain' }, { id: 'described', description: 'Fast one' }]),
		[
			{ value: 'plain', label: 'plain', badges: [] },
			{ value: 'described', label: 'described', description: 'Fast one', badges: [] }
		]
	);
});

test('a supplied price formatter adds a neutral price badge, and nothing for a route', () => {
	const format = (price: { input: number | null; output: number | null; unit: string } | null | undefined) =>
		price == null ? null : price.input || price.output ? `${price.input}/${price.output}` : 'free';
	assert.deepEqual(
		modelSelectOptions(
			[
				{ id: 'priced', price: { input: 3, output: 15, unit: 'tokens' } },
				{ id: 'free', price: { input: null, output: null, unit: 'tokens' } },
				{ id: 'route', price: null }
			],
			format
		),
		[
			{ value: 'priced', label: 'priced', badges: [{ label: '3/15', tone: 'neutral', mono: true }] },
			{ value: 'free', label: 'free', badges: [{ label: 'free', tone: 'neutral', mono: true }] },
			{ value: 'route', label: 'route', badges: [] }
		]
	);
});
