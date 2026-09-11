import test from 'node:test';
import assert from 'node:assert';

import {
	activityCounts,
	backendAssignments,
	completeAliasLine,
	poolCoverage,
	type Backend,
	type Pool
} from './upstreams.ts';

const backends: Backend[] = [
	{
		name: 'gpu-0',
		base_url: 'http://gpu-0/v1',
		api_key_env: null,
		api_key_env_set: false,
		has_stored_key: true,
		weight: 1,
		max_inflight: 8,
		health_path: '/models',
		probe_models: true,
		supports_edit: false,
		enabled: true,
		models: [],
		aliases: [{ alias: 'default', target: 'model-a' }],
		live: {
			healthy: true,
			enabled: true,
			auth_failed: false,
			inflight: 2,
			max_inflight: 8,
			models: ['model-a'],
			withheld: [],
			pool: 'chat'
		}
	},
	{
		name: 'gpu-1',
		base_url: 'http://gpu-1/v1',
		api_key_env: null,
		api_key_env_set: false,
		has_stored_key: true,
		weight: 1,
		max_inflight: 8,
		health_path: '/models',
		probe_models: true,
		supports_edit: false,
		enabled: true,
		models: [],
		aliases: [{ alias: 'default', target: 'model-b' }],
		live: {
			healthy: true,
			enabled: true,
			auth_failed: false,
			inflight: 0,
			max_inflight: 8,
			models: ['model-b'],
			withheld: [],
			pool: 'chat'
		}
	},
	{
		name: 'orphan',
		base_url: 'http://orphan/v1',
		api_key_env: null,
		api_key_env_set: false,
		has_stored_key: false,
		weight: 1,
		max_inflight: 4,
		health_path: '/models',
		probe_models: false,
		supports_edit: false,
		enabled: true,
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
	compliance_gdpr: true,
	compliance_nda: true,
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
