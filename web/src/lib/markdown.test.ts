import assert from 'node:assert/strict';
import test from 'node:test';

import { documentMarkdown, markdownMarkup } from './markdown.ts';

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

test('a json canvas document renders pretty-printed inside a code block', () => {
	const html = markdownMarkup(documentMarkdown('json', '{"deck":{"slides":[]}}'));
	assert.match(html, /<code class="language-json">/);
	assert.match(html, /\{\n  &quot;deck&quot;: \{\n    &quot;slides&quot;: \[\]\n  \}\n\}/);
});

test('a json canvas document that fails to parse keeps its raw text', () => {
	assert.equal(documentMarkdown('json', '{ broken'), '```json\n{ broken\n```');
});

test('source formats fence their content verbatim', () => {
	assert.equal(documentMarkdown('typst', '= Hi\n'), '```typst\n= Hi\n```');
	assert.equal(documentMarkdown('yaml', 'a: 1\n'), '```yaml\na: 1\n```');
	assert.equal(documentMarkdown('text', 'plain'), '```\nplain\n```');
});

test('markdown and html canvas documents pass through untouched', () => {
	assert.equal(documentMarkdown('markdown', '# Hi'), '# Hi');
	assert.equal(documentMarkdown('html', '<b>Hi</b>'), '<b>Hi</b>');
});

test('a fence lengthens when the content itself contains triple backticks', () => {
	assert.equal(documentMarkdown('text', 'a ``` b'), '````\na ``` b\n````');
});
