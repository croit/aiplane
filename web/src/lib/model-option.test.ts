import assert from 'node:assert/strict';
import test from 'node:test';
import { modelSelectOptions } from './model-option.ts';

test('model options expose GDPR and NDA status badges for every model', () => {
	assert.deepEqual(
		modelSelectOptions(
			[
				{ id: 'safe', gdpr: true, nda: true },
				{ id: 'restricted', gdpr: false, nda: false }
			],
			{ gdpr: 'GDPR', nda: 'NDA' }
		),
		[
			{
				value: 'safe',
				label: 'safe',
				badges: [
					{ label: 'GDPR', tone: 'success' },
					{ label: 'NDA', tone: 'success' }
				]
			},
			{
				value: 'restricted',
				label: 'restricted',
				badges: [
					{ label: 'GDPR', tone: 'error' },
					{ label: 'NDA', tone: 'error' }
				]
			}
		]
	);
});

test('a supplied price formatter prepends a neutral price badge', () => {
	const format = (price: { input: number | null; output: number | null; unit: string } | null | undefined) =>
		price == null ? null : price.input || price.output ? `${price.input}/${price.output}` : 'free';
	assert.deepEqual(
		modelSelectOptions(
			[
				{ id: 'priced', gdpr: true, nda: true, price: { input: 3, output: 15, unit: 'tokens' } },
				{ id: 'free', gdpr: true, nda: true, price: { input: null, output: null, unit: 'tokens' } },
				{ id: 'route', gdpr: true, nda: true, price: null }
			],
			{ gdpr: 'GDPR', nda: 'NDA' },
			format
		),
		[
			{
				value: 'priced',
				label: 'priced',
				badges: [
					{ label: '3/15', tone: 'neutral', mono: true },
					{ label: 'GDPR', tone: 'success' },
					{ label: 'NDA', tone: 'success' }
				]
			},
			{
				value: 'free',
				label: 'free',
				badges: [
					{ label: 'free', tone: 'neutral', mono: true },
					{ label: 'GDPR', tone: 'success' },
					{ label: 'NDA', tone: 'success' }
				]
			},
			{
				value: 'route',
				label: 'route',
				badges: [
					{ label: 'GDPR', tone: 'success' },
					{ label: 'NDA', tone: 'success' }
				]
			}
		]
	);
});
