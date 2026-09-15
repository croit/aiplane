import test from 'node:test';
import assert from 'node:assert/strict';
import { configuredFacets, contextWindowHint, matchesModelFilter, pricingUnitFor } from './admin-models.ts';
import type { AdminModel } from './admin-models.ts';

const model: AdminModel = {
	name: 'Qwen/Qwen3',
	kind: 'chat',
	alias_target: null,
	configured: true,
	resolved_reasoning_style: 'qwen',
	uses_token_budget: true,
	detected_context_window: null,
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

test('an empty context field reports what the backend said, or admits it said nothing', () => {
	assert.deepEqual(contextWindowHint('', 262144), {
		tone: 'info',
		key: 'admin-context-detected',
		window: 262144
	});
	// Ollama and hosted providers: no ceiling exists to show, and inventing a
	// default that looks like knowledge is the bug this replaces.
	assert.deepEqual(contextWindowHint('', null), {
		tone: 'info',
		key: 'admin-context-unreported'
	});
});

test('lowering the context below what the backend serves is legitimate and passes quietly', () => {
	assert.deepEqual(contextWindowHint('32768', 262144), {
		tone: 'info',
		key: 'admin-context-detected',
		window: 262144
	});
	// Exactly equal is not "exceeds".
	assert.equal(contextWindowHint('262144', 262144)?.tone, 'info');
});

test('raising the context above what the backend serves warns', () => {
	assert.deepEqual(contextWindowHint('262144', 32768), {
		tone: 'warning',
		key: 'admin-context-exceeds-detected',
		window: 32768
	});
});

test('without a detected value no warning can honestly be made', () => {
	assert.deepEqual(contextWindowHint('999999', null), {
		tone: 'info',
		key: 'admin-context-unreported'
	});
});

test('a non-numeric or nonsensical entry says nothing rather than guessing', () => {
	assert.equal(contextWindowHint('abc', 32768), null);
	assert.equal(contextWindowHint('0', 32768), null);
	assert.equal(contextWindowHint('-5', 32768), null);
});

/**
 * The field is `<input type="number">`, so Svelte's binding hands back a
 * number — or `null` when it is cleared. The first cut declared `string` and
 * threw on the first keystroke; the tests passed because they only ever used
 * string literals.
 */
test('the hint survives the number and null the number input actually binds', () => {
	assert.deepEqual(contextWindowHint(262144, 32768), {
		tone: 'warning',
		key: 'admin-context-exceeds-detected',
		window: 32768
	});
	assert.deepEqual(contextWindowHint(16384, 32768), {
		tone: 'info',
		key: 'admin-context-detected',
		window: 32768
	});
	// Cleared field: same as empty string, not a crash.
	assert.deepEqual(contextWindowHint(null, 32768), {
		tone: 'info',
		key: 'admin-context-detected',
		window: 32768
	});
	assert.deepEqual(contextWindowHint(undefined, null), {
		tone: 'info',
		key: 'admin-context-unreported'
	});
});
