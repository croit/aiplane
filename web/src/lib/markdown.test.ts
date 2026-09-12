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

test('relative generated images resolve through conversation assets', () => {
	const html = markdownMarkup('![Result](generated.png)', undefined, {
		images: [{ filename: 'generated.png', url: '/chat/attachment/turn/generated.png' }]
	});
	assert.match(html, /src="\/chat\/attachment\/turn\/generated\.png"/);
});

test('markdown does not repeat an attachment already shown by the message gallery', () => {
	const html = markdownMarkup('Before\n\n![Result](generated.png)\n\nAfter', undefined, {
		images: [{ filename: 'generated.png', url: '/chat/attachment/turn/generated.png' }],
		hiddenImageUrls: new Set(['/chat/attachment/turn/generated.png'])
	});
	assert.doesNotMatch(html, /<img/);
	assert.match(html, /Before/);
	assert.match(html, /After/);
});

test('a table scrolls inside its own container instead of widening the message', () => {
	const html = markdownMarkup('| Probe | File paths hit |\n| --- | --- |\n| `rag_grep` | a/very/long/path.pm |');
	assert.match(html, /<div class="overflow-x-auto"><table>/);
	assert.match(html, /<\/table>\s*<\/div>/);
	assert.match(html, /<th>Probe<\/th>/);
});
