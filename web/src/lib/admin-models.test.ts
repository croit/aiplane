import test from 'node:test';
import assert from 'node:assert/strict';
import { configuredFacets, matchesModelFilter, pricingUnitFor } from './admin-models.ts';
import type { AdminModel } from './admin-models.ts';

const model: AdminModel = {
	name: 'Qwen/Qwen3',
	kind: 'chat',
	alias_target: null,
	configured: true,
	resolved_reasoning_style: 'qwen',
	uses_token_budget: true,
	effort_levels: [],
	defaults: {
		defaults_toml: 'temperature = 0.7',
		reasoning_style: null,
		context_window: 32768,
		input_price: 1,
		output_price: null,
		pricing_unit: 'tokens',
		budget_standard: 1024,
		budget_deep: null,
		budget_max: null,
		effort_standard: null,
		effort_deep: null,
		effort_max: null,
		capabilities: { vision: true, tools: null }
	}
};

test('model filters combine kind, alias, configuration, and name', () => {
	assert.equal(matchesModelFilter(model, 'chat', 'qwen3'), true);
	assert.equal(matchesModelFilter(model, 'other', ''), false);
	assert.equal(matchesModelFilter({ ...model, alias_target: 'target', configured: false }, 'alias', 'qwen'), true);
	assert.equal(matchesModelFilter(model, 'configured', ''), true);
});

test('configured facets retain every production summary badge', () => {
	assert.deepEqual(configuredFacets(model), ['price', 'context', 'budget', 'capabilities', 'toml']);
});

test('unconfigured non-chat models use their domain pricing unit', () => {
	assert.equal(pricingUnitFor({ ...model, kind: 'image', defaults: null }), 'images');
	assert.equal(pricingUnitFor({ ...model, kind: 'speech', defaults: null }), 'characters');
	assert.equal(pricingUnitFor({ ...model, kind: 'transcription', defaults: null }), 'seconds');
	assert.equal(pricingUnitFor({ ...model, kind: 'embedding', defaults: null }), 'tokens');
});
