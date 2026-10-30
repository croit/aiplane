import assert from 'node:assert/strict';
import test from 'node:test';
import { lastUsedModel, openingModel } from './conversation-model.ts';

const offered = [{ id: 'fast' }, { id: 'smart' }];

test('the last used model is the newest turn that names one', () => {
	assert.equal(
		lastUsedModel([
			{ model: 'fast' },
			{ model: 'smart' },
			{ model: null }
		]),
		'smart'
	);
	assert.equal(lastUsedModel([{ model: null }]), null);
	assert.equal(lastUsedModel([]), null);
});

test('a conversation opens on the model it was last sent to, not the first offered', () => {
	assert.deepEqual(openingModel('smart', offered), { model: 'smart', unavailable: null });
});

test('a conversation without a used model opens on the first offered model', () => {
	assert.deepEqual(openingModel(null, offered), { model: 'fast', unavailable: null });
});

test('a last used model that is no longer offered is named, and the first offered takes its place', () => {
	assert.deepEqual(openingModel('retired', offered), { model: 'fast', unavailable: 'retired' });
});

test('without an offered list the free-text picker keeps the last used model', () => {
	assert.deepEqual(openingModel('smart', []), { model: 'smart', unavailable: null });
	assert.deepEqual(openingModel(null, []), { model: '', unavailable: null });
});
