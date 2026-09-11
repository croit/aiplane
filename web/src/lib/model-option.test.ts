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
