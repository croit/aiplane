import test from 'node:test';
import assert from 'node:assert';
import {
	defaultModelSort,
	defaultProviderSort,
	nextSort,
	sortEndpoints,
	sortModels
} from './openrouter-sort.ts';

const model = (id: string, prompt: number, completion: number, context: number | null) => ({
	id,
	prompt_price: prompt,
	completion_price: completion,
	context_length: context
});

test('the catalog opens on the cheapest output tokens, ties by model id', () => {
	const rows = [model('b', 0.03, 0.5, 1000), model('a', 0.04, 0.25, 2000), model('c', 0.01, 0.25, 1500)];
	assert.deepEqual(
		sortModels(rows, defaultModelSort()).map((row) => row.id),
		['a', 'c', 'b']
	);
});

test('a header click flips its own column and starts other columns ascending', () => {
	const first = nextSort(defaultModelSort(), 'in');
	assert.deepEqual(first, { key: 'in', direction: 'asc' });
	assert.deepEqual(nextSort(first, 'in'), { key: 'in', direction: 'desc' });
	assert.deepEqual(nextSort({ key: 'in', direction: 'desc' }, 'context'), { key: 'context', direction: 'asc' });
});

test('a missing context sorts last in both directions', () => {
	const rows = [model('a', 0.01, 0.1, null), model('b', 0.02, 0.2, 4096)];
	assert.deepEqual(sortModels(rows, { key: 'context', direction: 'asc' }).map((row) => row.id), ['b', 'a']);
	assert.deepEqual(sortModels(rows, { key: 'context', direction: 'desc' }).map((row) => row.id), ['b', 'a']);
});

const endpoint = (provider: string, prompt: number, completion: number, extra: Partial<{ cache_read_price: number | null; latency_p50: number | null; throughput_p50: number | null; context_length: number | null; uptime_30m: number | null }> = {}) => ({
	provider,
	prompt_price: prompt,
	completion_price: completion,
	cache_read_price: extra.cache_read_price ?? null,
	context_length: extra.context_length ?? null,
	throughput_p50: extra.throughput_p50 ?? null,
	latency_p50: extra.latency_p50 ?? null,
	uptime_30m: extra.uptime_30m ?? null
});

test('providers open on the cheapest output tokens, ties by provider', () => {
	const rows = [endpoint('Wafer', 0.038, 0.5), endpoint('Particle AI', 0.03, 0.25), endpoint('Ionstream', 0.0775, 0.2325)];
	assert.deepEqual(
		sortEndpoints(rows, defaultProviderSort()).map((row) => row.provider),
		['Ionstream', 'Particle AI', 'Wafer']
	);
});

test('a duration-priced model opens its providers on the cheapest audio second', () => {
	const rows = [endpoint('Groq', 0.000031, 0), endpoint('DeepInfra', 0.0000075, 0), endpoint('Together', 0.000025, 0)];
	assert.deepEqual(
		sortEndpoints(rows, defaultProviderSort('second')).map((row) => row.provider),
		['DeepInfra', 'Together', 'Groq']
	);
	assert.deepEqual(defaultProviderSort('second'), { key: 'in', direction: 'asc' });
});

test('a measured column sorts numerically and puts unmeasured providers last', () => {
	const rows = [
		endpoint('a', 0.1, 0.1, { latency_p50: null }),
		endpoint('b', 0.1, 0.1, { latency_p50: 900 }),
		endpoint('c', 0.1, 0.1, { latency_p50: 250 })
	];
	assert.deepEqual(
		sortEndpoints(rows, { key: 'latency', direction: 'asc' }).map((row) => row.provider),
		['c', 'b', 'a']
	);
	assert.deepEqual(
		sortEndpoints(rows, { key: 'latency', direction: 'desc' }).map((row) => row.provider),
		['b', 'c', 'a']
	);
});
