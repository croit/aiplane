import test from 'node:test';
import assert from 'node:assert';

import {
	activityCounts,
	backendTestFailure,
	backendAssignments,
	boardGroups,
	boardPoolMatches,
	boardRowMatches,
	liveForPool,
	poolProblems,
	parallelismMismatch,
	completeAliasLine,
	poolCoverage,
	parseHeaders,
	formatHeaders,
	type Backend,
	type Coverage,
	type LiveBackend,
	type Pool, naturalSort } from './upstreams.ts';

const backends: Backend[] = [
	{
		priority: { mode: 'auto' },
		provider: 'openai_compatible',
		name: 'gpu-0',
		base_url: 'http://gpu-0/v1',
		api_key_env: null,
		api_key_env_set: false,
		has_stored_key: true,
		api_key_hint: 'gpu-…key4',
		weight: 1,
		max_inflight: 8,
		health_path: '/models',
		supports_edit: false,
		enabled: true,
		extra_headers: {},
		models: [],
		aliases: [{ alias: 'default', target: 'model-a' }],
		live: {
			detected_priority: null,
			healthy: true,
			enabled: true,
			auth_failed: false,
			inflight: 2,
			max_inflight: 8,
			models: ['model-a'],
			pool: 'chat',
			profile: 'generic' as const,
			detected_version: null,
			detected_max_parallel: null,
		detected_at: null
		}
	},
	{
		priority: { mode: 'auto' },
		provider: 'openai_compatible',
		name: 'gpu-1',
		base_url: 'http://gpu-1/v1',
		api_key_env: null,
		api_key_env_set: false,
		has_stored_key: true,
		api_key_hint: 'gpu-…key4',
		weight: 1,
		max_inflight: 8,
		health_path: '/models',
		supports_edit: false,
		enabled: true,
		extra_headers: {},
		models: [],
		aliases: [{ alias: 'default', target: 'model-b' }],
		live: {
			detected_priority: null,
			healthy: true,
			enabled: true,
			auth_failed: false,
			inflight: 0,
			max_inflight: 8,
			models: ['model-b'],
			pool: 'chat',
			profile: 'generic' as const,
			detected_version: null,
			detected_max_parallel: null,
		detected_at: null
		}
	},
	{
		priority: { mode: 'auto' },
		provider: 'openai_compatible',
		name: 'orphan',
		base_url: 'http://orphan/v1',
		api_key_env: null,
		api_key_env_set: false,
		has_stored_key: false,
		api_key_hint: null,
		weight: 1,
		max_inflight: 4,
		health_path: '/models',
		supports_edit: false,
		enabled: true,
		extra_headers: {},
		models: ['manual'],
		aliases: [],
		live: null
	}
];

const pool: Pool = {
	name: 'chat',
	kind: 'chat',
	strategy: 'prefix_affinity',
	fallback_offline: null,
	enforce_limits: true,
	sort_order: 0,
	allowed_groups: [],
	backends: ['gpu-0', 'gpu-1'],
	models: [],
	voices: [],
	offer_voices: []
};

test('backend assignments preserve pool order and retain unassigned backends', () => {
	const assignments = backendAssignments([pool], backends);
	assert.deepEqual(assignments.byPool.get('chat')?.map((backend) => backend.name), ['gpu-0', 'gpu-1']);
	assert.deepEqual(assignments.unassigned.map((backend) => backend.name), ['orphan']);
});

test('activity counts use the newest 15, 30, and 60 minute buckets', () => {
	assert.deepEqual(activityCounts([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]), {
		m15: 33,
		m30: 57,
		m60: 78
	});
});

test('pool coverage exposes partial replicas and aliases exactly once', () => {
	assert.deepEqual(poolCoverage(pool, backends), [
		{ name: 'default', serving: 2, total: 2 },
		{ name: 'model-a', serving: 1, total: 2 },
		{ name: 'model-b', serving: 1, total: 2 }
	]);
});

test('a discovered model completes only the alias line at the cursor', () => {
	assert.deepEqual(completeAliasLine('fast=model-a\ndefault\nother=model-c', 18, 'repo/model-b'), {
		value: 'fast=model-a\ndefault=repo/model-b\nother=model-c',
		cursor: 33
	});
	assert.deepEqual(completeAliasLine('', 0, 'repo/model-a'), {
		value: 'repo/model-a',
		cursor: 12
	});
});

test('model ids sort the way a person reads a version, not by byte', () => {
	// Byte order puts `5.10` before `5.2` and `-11b` before `-7b`; both are
	// wrong to anyone scanning the list for a model.
	assert.deepEqual(
		naturalSort(['gpt-5.10', 'gpt-5.2', 'gpt-5.1']),
		['gpt-5.1', 'gpt-5.2', 'gpt-5.10']
	);
	assert.deepEqual(
		naturalSort(['llama-3.2-11b', 'llama-3.2-7b', 'llama-3.2-1b']),
		['llama-3.2-1b', 'llama-3.2-7b', 'llama-3.2-11b']
	);
	// Case must not split one family into two blocks.
	assert.deepEqual(naturalSort(['qwen-b', 'Qwen-a']), ['Qwen-a', 'qwen-b']);
	// The input is never mutated — it comes straight off the live status.
	const input = ['b', 'a'];
	naturalSort(input);
	assert.deepEqual(input, ['b', 'a']);
});

/**
 * Ollama runs one request per model by default and queues up to 512 rather
 * than rejecting, so a `max_inflight` of 16 produces no error anywhere — the
 * picker just keeps dispatching into a queue and back-pressure stops meaning
 * anything. The badge is the only place that can say so.
 */
test('a server that runs fewer requests than we send it is flagged', () => {
	const live = {
		healthy: true,
		enabled: true,
		auth_failed: false,
		inflight: 0,
		max_inflight: 16,
		models: [],
		pool: 'chat',
		profile: 'ollama' as const,
		detected_version: '0.34.0',
		detected_max_parallel: 1,
		detected_at: null
	};
	assert.equal(parallelismMismatch(live.detected_max_parallel, live.max_inflight), 1);
	// Configured at or below what the server runs: nothing to say.
	assert.equal(parallelismMismatch(live.detected_max_parallel, 1), null);
	// A server that did not say is not a mismatch, it is an unknown.
	assert.equal(parallelismMismatch(null, 16), null);
	assert.equal(parallelismMismatch(undefined, 16), null);
});

test('a refused connection test becomes the panel result its code names', () => {
	const body = JSON.stringify({
		error: { message: 'x', type: 'auth_failed', code: 'auth_failed', status: 401, key_source: { kind: 'none' } }
	});
	assert.deepEqual(backendTestFailure(body), {
		outcome: 'error',
		code: 'auth_failed',
		status: 401,
		key_source: { kind: 'none' },
		models: []
	});
});

test('a refusal that is not a connection-test outcome is left to the caller', () => {
	assert.equal(backendTestFailure(JSON.stringify({ error: { message: 'no', code: 'forbidden' } })), null);
	assert.equal(backendTestFailure('not json'), null);
	assert.equal(backendTestFailure(undefined), null);
});

test('extra headers parse one name: value per line and keep the rest of the value', () => {
	assert.deepEqual(
		parseHeaders('anthropic-version: 2023-06-01\nx-trace-id: abc-123'),
		{ 'anthropic-version': '2023-06-01', 'x-trace-id': 'abc-123' }
	);
	// A colon inside the value is part of the value.
	assert.deepEqual(parseHeaders('x-url: https://example.test/x'), { 'x-url': 'https://example.test/x' });
});

test('malformed extra-header lines are dropped, not guessed', () => {
	assert.deepEqual(parseHeaders(''), {});
	assert.deepEqual(parseHeaders('\n  \n'), {});
	// No separator, empty name, empty value: all unparseable.
	assert.deepEqual(parseHeaders('no-separator\n: value\nname:'), {});
});

test('extra headers format back to the editor field', () => {
	assert.equal(
		formatHeaders({ 'anthropic-version': '2023-06-01', 'x-trace-id': 'abc' }),
		'anthropic-version: 2023-06-01\nx-trace-id: abc'
	);
	assert.equal(formatHeaders({}), '');
});

function liveView(overrides: Partial<LiveBackend> = {}): LiveBackend {
	return {
		detected_priority: null,
		healthy: true,
		enabled: true,
		auth_failed: false,
		inflight: 0,
		max_inflight: 8,
		models: ['model-a'],
		pool: 'chat',
		profile: 'generic',
		detected_version: null,
		detected_max_parallel: null,
		detected_at: null,
		...overrides
	};
}

function backendWith(overrides: Partial<Backend> = {}): Backend {
	return { ...backends[0], aliases: [], ...overrides };
}

test('the board groups backends by pool order and keeps an empty pool visible', () => {
	const later: Pool = { ...pool, name: 'gpu', sort_order: -1, backends: [] };
	const groups = boardGroups([pool, later], backends);
	assert.deepEqual(groups.map((group) => group.pool?.name ?? null), ['gpu', 'chat', null]);
	// A pool without members is still a topology row that needs editing.
	assert.deepEqual(groups[0].backends, []);
	assert.deepEqual(groups[1].backends.map((backend) => backend.name), ['gpu-0', 'gpu-1']);
	// Backends no pool holds land in one trailing band.
	assert.deepEqual(groups[2].backends.map((backend) => backend.name), ['orphan']);
});

test('a status event from another pool never overrides the view this pool has', () => {
	const poolView: Pool = { ...pool, live_backends: { 'gpu-0': liveView({ inflight: 2 }) } };
	const moved = backendWith({ live: liveView({ pool: 'other', inflight: 9 }) });
	assert.equal(liveForPool(poolView, moved)?.inflight, 2);
	// The pool's own view is used as-is.
	assert.equal(liveForPool(pool, backends[0])?.inflight, 2);
	// Without a per-pool view, the last status is better than an empty row.
	assert.equal(liveForPool({ ...pool, live_backends: undefined }, moved)?.inflight, 9);
});

test('a healthy pool with full coverage and working aliases has nothing to report', () => {
	const coverage: Coverage[] = [{ name: 'model-a', serving: 2, total: 2 }];
	assert.deepEqual(poolProblems(pool, backends, coverage), []);
});

test('an empty pool says nothing can serve a request', () => {
	assert.deepEqual(poolProblems({ ...pool, backends: [] }, backends, []), [
		{ key: 'upstreams-problem-no-backends', params: {} }
	]);
});

test('a pool whose every backend is drained is named as paused, not down', () => {
	const drained = [backendWith({ name: 'gpu-0', live: liveView({ enabled: false }) })];
	assert.deepEqual(poolProblems({ ...pool, backends: ['gpu-0'] }, drained, []).map((problem) => problem.key), [
		'upstreams-problem-all-drained'
	]);
	const down = [backendWith({ name: 'gpu-0', live: liveView({ healthy: false }) })];
	assert.deepEqual(poolProblems({ ...pool, backends: ['gpu-0'] }, down, []).map((problem) => problem.key), [
		'upstreams-problem-all-down'
	]);
});

test('a rejected key and an empty model list name the backends they affect', () => {
	const rejected = [backendWith({ name: 'gpu-0', live: liveView({ auth_failed: true, models: [] }) })];
	const problems = poolProblems({ ...pool, backends: ['gpu-0'] }, rejected, []);
	assert.deepEqual(problems.find((problem) => problem.key === 'upstreams-problem-auth')?.params, {
		backends: 'gpu-0'
	});
	assert.deepEqual(problems.find((problem) => problem.key === 'upstreams-problem-no-models')?.params, {
		backends: 'gpu-0'
	});
});

test('an OpenRouter account with no allowlist is unconfigured, not broken', () => {
	const account = [
		backendWith({ name: 'openrouter', provider: 'openrouter', live: liveView({ models: [] }) })
	];
	const problems = poolProblems({ ...pool, backends: ['openrouter'], models: [] }, account, []);
	assert.deepEqual(problems.map((problem) => problem.key), []);
});

test('partial coverage, unserved allowlist entries and ghost members are reported', () => {
	const coverage: Coverage[] = [
		{ name: 'model-a', serving: 1, total: 2 },
		{ name: 'model-b', serving: 0, total: 2 }
	];
	const problems = poolProblems({ ...pool, backends: ['gpu-0', 'gpu-1', 'ghost'] }, backends, coverage);
	assert.deepEqual(problems.find((problem) => problem.key === 'upstreams-problem-partial-coverage')?.params, {
		models: 'model-a (1/2)'
	});
	assert.deepEqual(problems.find((problem) => problem.key === 'upstreams-problem-unserved-allowlist')?.params, {
		models: 'model-b'
	});
	assert.deepEqual(problems.find((problem) => problem.key === 'upstreams-problem-missing-backends')?.params, {
		backends: 'ghost'
	});
});

test('the board filter matches a row by name, endpoint, model or alias', () => {
	const row = { backend: backends[0], live: backends[0].live };
	assert.equal(boardRowMatches(row, ''), true, 'no query shows every row');
	assert.equal(boardRowMatches(row, 'GPU'), true, 'name matching ignores case');
	assert.equal(boardRowMatches(row, 'gpu-0/v1'), true, 'the endpoint is part of the row');
	assert.equal(boardRowMatches(row, 'model-a'), true, 'an advertised model matches');
	assert.equal(boardRowMatches(row, 'default'), true, 'an alias matches');
	assert.equal(boardRowMatches(row, 'model-b'), false);
});

test('a pool name keeps its whole band, including an empty one', () => {
	const rows = [{ backend: backends[0], live: backends[0].live }];
	assert.equal(boardPoolMatches('chat', rows, 'cha'), true);
	assert.equal(boardPoolMatches('chat', rows, 'model-a'), true, 'one matching row keeps the band');
	assert.equal(boardPoolMatches('chat', rows, 'embedding'), false);
	assert.equal(boardPoolMatches('chat', [], 'chat'), true, 'an empty pool is still findable by name');
	assert.equal(boardPoolMatches('chat', [], 'anything'), false);
});
