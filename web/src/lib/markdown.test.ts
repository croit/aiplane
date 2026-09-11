import assert from 'node:assert/strict';
import test from 'node:test';

import { markdownMarkup } from './markdown.ts';

test('fenced code blocks expose an accessible copy control', () => {
	const html = markdownMarkup('```json\n{"ok":true}\n```', {
		copy: 'Copy code',
		copied: 'Copied'
	});
	assert.match(html, /data-code-copy/);
	assert.match(html, /aria-label="Copy code"/);
	assert.match(html, /data-copied-label="Copied"/);
	assert.match(html, /<code class="language-json">/);
});

test('ordinary markdown stays free of copy chrome', () => {
	assert.doesNotMatch(markdownMarkup('Hello **world**'), /data-code-copy/);
});
