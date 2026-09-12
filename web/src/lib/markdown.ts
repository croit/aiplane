/**
 * Markdown rendering for chat replies (issue #22 phase 2).
 *
 * The legacy UI renders markdown server-side (Plait renderers in
 * session-core); the SPA renders client-side instead — that is the point of
 * the protocol change: the wire carries markdown text, the client owns the
 * pixels. `marked` parses, DOMPurify sanitises; model output is untrusted
 * input like any other, so the pipeline is parse → sanitise → `{@html}`.
 */
import { marked, Renderer } from 'marked';
import DOMPurify from 'dompurify';

marked.setOptions({ gfm: true, breaks: true });

export interface CodeCopyLabels {
	copy: string;
	copied: string;
}

export interface MarkdownImage {
	filename: string;
	url: string;
}

export interface MarkdownOptions {
	images?: MarkdownImage[];
	hiddenImageUrls?: ReadonlySet<string>;
}

function escapeHtml(value: string): string {
	return value
		.replaceAll('&', '&amp;')
		.replaceAll('<', '&lt;')
		.replaceAll('>', '&gt;')
		.replaceAll('"', '&quot;')
		.replaceAll("'", '&#39;');
}

function relativeFilename(href: string): string | null {
	if (!href || href.startsWith('/') || /^[a-z][a-z\d+.-]*:/i.test(href)) return null;
	const pathname = href.split(/[?#]/, 1)[0];
	const filename = pathname.split('/').at(-1);
	if (!filename) return null;
	try {
		return decodeURIComponent(filename);
	} catch {
		return filename;
	}
}

export function markdownMarkup(md: string | null | undefined, labels?: CodeCopyLabels, options: MarkdownOptions = {}): string {
	if (!md) return '';
	const renderer = new Renderer();
	const defaultRenderer = new Renderer();
	renderer.image = (token) => {
		const filename = relativeFilename(token.href);
		const image = filename ? options.images?.find((candidate) => candidate.filename === filename) : undefined;
		if (!image) return defaultRenderer.image(token);
		if (options.hiddenImageUrls?.has(image.url)) return '';
		const title = token.title ? ` title="${escapeHtml(token.title)}"` : '';
		return `<img src="${escapeHtml(image.url)}" alt="${escapeHtml(token.text)}"${title}>`;
	};
	// A wide table (file paths, log lines) must scroll inside its container
	// instead of pushing the chat bubble off-screen.
	const renderTable = Renderer.prototype.table;
	renderer.table = function (token) {
		return `<div class="overflow-x-auto">${renderTable.call(this, token)}</div>`;
	};
	if (labels) {
		renderer.code = ({ text, lang }) => {
			const language = lang ? ` class="language-${escapeHtml(lang)}"` : '';
			return `<div class="relative"><button type="button" class="btn btn-ghost btn-xs absolute right-1 top-1" data-code-copy aria-label="${escapeHtml(labels.copy)}" title="${escapeHtml(labels.copy)}" data-copied-label="${escapeHtml(labels.copied)}">${escapeHtml(labels.copy)}</button><pre><code${language}>${escapeHtml(text)}\n</code></pre></div>`;
		};
	}
	return marked.parse(md, { async: false, renderer }) as string;
}

export function renderMarkdown(md: string | null | undefined, labels?: CodeCopyLabels, options?: MarkdownOptions): string {
	return DOMPurify.sanitize(markdownMarkup(md, labels, options));
}
