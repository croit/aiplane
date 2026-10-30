import test from 'node:test';
import assert from 'node:assert/strict';

import { filterTokenModels, groupTokenModels, selectionSummary, type TokenModel } from './token-models.ts';

function model(id: string, overrides: Partial<TokenModel> = {}): TokenModel {
	return { id, kind: 'chat', alias_of: null, price: null, ...overrides };
}

const qwen = model('qwen');
const kimi = model('kimi', { price: { input: 0.6, output: 2.5, unit: 'tokens' } });
const glm = model('glm', { price: { input: 0.3, output: 1.2, unit: 'tokens' } });
const bge = model('bge', { kind: 'embedding' });
const reranker = model('bge-reranker', { kind: 'rerank' });
const image = model('gpt-image-2', { kind: 'image', price: { input: null, output: 0.04, unit: 'images' } });

test('models group by kind in a fixed order, so chat models come first', () => {
	const groups = groupTokenModels([image, reranker, bge, qwen]);
	assert.deepEqual(groups.map((group) => group.kind), ['chat', 'embedding', 'rerank', 'image']);
	assert.deepEqual(groups[0].models, [qwen]);
});

test('a kind the SPA does not know yet still shows, after the known ones', () => {
	const odd = model('odd', { kind: 'future' });
	assert.deepEqual(groupTokenModels([odd, qwen]).map((group) => group.kind), ['chat', 'future']);
});

test('filters narrow by name and by price together', () => {
	const all = [qwen, kimi, glm, bge, image];
	assert.deepEqual(filterTokenModels(all, { query: 'KI', free: false }), [kimi]);
	assert.deepEqual(filterTokenModels(all, { query: '', free: false }), all);
	assert.deepEqual(filterTokenModels(all, { query: '', free: true }), [qwen, bge]);
});

test('the summary counts the selection and the dearest token price', () => {
	const all = [qwen, kimi, glm, bge, image];
	assert.deepEqual(selectionSummary(all, ['qwen', 'bge']), { count: 2, maxOutputPerMillion: null });
	assert.deepEqual(selectionSummary(all, ['qwen', 'kimi', 'glm', 'gpt-image-2']), { count: 4, maxOutputPerMillion: 2.5 });
});

test('the summary of an unrestricted token covers every model it can reach', () => {
	assert.deepEqual(selectionSummary([qwen, kimi], null), { count: 2, maxOutputPerMillion: 2.5 });
});

test('a selected model that has since disappeared is not counted', () => {
	assert.deepEqual(selectionSummary([qwen], ['qwen', 'gone']), { count: 1, maxOutputPerMillion: null });
});
