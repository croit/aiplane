import test from 'node:test';
import assert from 'node:assert/strict';
import { get } from 'svelte/store';
import {
	clearPageTitleOverride,
	pageTitleOverride,
	setPageTitleOverride
} from './page-title.ts';

test('a page can publish and clear its data-driven browser title', () => {
	setPageTitleOverride('/chat/one', 'Conversation one — LLM Gateway');
	assert.deepEqual(get(pageTitleOverride), {
		pathname: '/chat/one',
		title: 'Conversation one — LLM Gateway'
	});

	clearPageTitleOverride('/chat/one');
	assert.deepEqual(get(pageTitleOverride), { pathname: '', title: null });
});

test('a departing page cannot clear the next route title', () => {
	setPageTitleOverride('/chat/two', 'Conversation two — LLM Gateway');
	clearPageTitleOverride('/chat/one');
	assert.equal(get(pageTitleOverride).pathname, '/chat/two');
});
