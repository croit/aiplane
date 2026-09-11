import assert from 'node:assert/strict';
import test from 'node:test';
import { filterSearchOptions, nextEnabledOptionIndex, type SearchOption } from './searchable-select.ts';

const options: SearchOption[] = [
	{ value: 'mistralai/voxtral', label: 'Voxtral Mini', description: 'Realtime transcription' },
	{ value: 'glm-5.2', label: 'GLM 5.2', badges: [{ label: 'GDPR', tone: 'error' }, { label: 'NDA', tone: 'error' }], keywords: ['non-GDPR', 'confidential-restricted'] },
	{ value: 'qwen-3.8', label: 'Qwen 3.8', keywords: ['EU hosted'] }
];

test('search matches labels, values, descriptions, badges, and keywords', () => {
	assert.deepEqual(filterSearchOptions(options, 'voxtral realtime').map((option) => option.value), ['mistralai/voxtral']);
	assert.deepEqual(filterSearchOptions(options, 'glm confidential').map((option) => option.value), ['glm-5.2']);
	assert.deepEqual(filterSearchOptions(options, 'eu hosted').map((option) => option.value), ['qwen-3.8']);
});

test('search is case and accent insensitive and keeps source ordering', () => {
	const countries: SearchOption[] = [
		{ value: 'aland', label: 'Åland Islands' },
		{ value: 'albania', label: 'Albania' }
	];
	assert.deepEqual(filterSearchOptions(countries, 'ALAND').map((option) => option.value), ['aland']);
	assert.deepEqual(filterSearchOptions(options, '').map((option) => option.value), options.map((option) => option.value));
});

test('keyboard movement wraps and skips disabled options', () => {
	const rows: SearchOption[] = [
		{ value: 'a', label: 'A' },
		{ value: 'b', label: 'B', disabled: true },
		{ value: 'c', label: 'C' }
	];
	assert.equal(nextEnabledOptionIndex(rows, 0, 1), 2);
	assert.equal(nextEnabledOptionIndex(rows, 2, 1), 0);
	assert.equal(nextEnabledOptionIndex(rows, 0, -1), 2);
});
