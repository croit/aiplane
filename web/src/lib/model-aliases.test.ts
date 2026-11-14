import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
	aliasNameProblem,
	aliasTargetProblem,
	draftFromAlias,
	servedTargetCount,
	type ModelAlias
} from './model-aliases.ts';

test('an alias name is a token, not a model id', () => {
	assert.equal(aliasNameProblem('', []), 'model-alias-name-required');
	assert.equal(aliasNameProblem('   ', []), 'model-alias-name-required');
	assert.equal(aliasNameProblem('fast lane', []), 'model-alias-name-invalid');
	assert.equal(aliasNameProblem('fast\nlane', []), 'model-alias-name-invalid');
	assert.equal(aliasNameProblem('gpt-5', ['gpt-5']), 'model-alias-name-taken');
	assert.equal(aliasNameProblem('  fast  ', ['gpt-5']), null);
});

test('one target per backend and at least one target', () => {
	assert.equal(aliasTargetProblem([]), 'model-alias-targets-required');
	assert.equal(aliasTargetProblem([{ backend: '', target: '' }]), 'model-alias-target-missing');
	assert.equal(
		aliasTargetProblem([
			{ backend: 'gpu-a', target: 'model-a' },
			{ backend: 'gpu-a', target: 'model-b' }
		]),
		'model-alias-target-duplicate'
	);
	assert.equal(
		aliasTargetProblem([
			{ backend: 'gpu-a', target: 'model-a' },
			{ backend: 'gpu-b', target: '' }
		]),
		null
	);
});

test('a draft keeps the sole-model choice as an empty target', () => {
	const alias: ModelAlias = {
		name: 'fast',
		targets: [
			{ backend: 'gpu-a', target: 'model-a', resolves_to: 'model-a', served: true },
			{ backend: 'gpu-b', target: null, resolves_to: 'model-b', served: true }
		]
	};
	assert.deepEqual(draftFromAlias(alias), {
		name: 'fast',
		targets: [
			{ backend: 'gpu-a', target: 'model-a' },
			{ backend: 'gpu-b', target: '' }
		]
	});
	assert.equal(servedTargetCount(alias), 2);
});
