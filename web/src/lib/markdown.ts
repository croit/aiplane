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

function escapeHtml(value: string): string {
	return value
		.replaceAll('&', '&amp;')
		.replaceAll('<', '&lt;')
		.replaceAll('>', '&gt;')
		.replaceAll('"', '&quot;')
		.replaceAll("'", '&#39;');
}

export function markdownMarkup(md: string | null | undefined, labels?: CodeCopyLabels): string {
	if (!md) return '';
	const renderer = new Renderer();
	if (labels) {
		renderer.code = ({ text, lang }) => {
			const language = lang ? ` class="language-${escapeHtml(lang)}"` : '';
			return `<div class="relative"><button type="button" class="btn btn-ghost btn-xs absolute right-1 top-1" data-code-copy aria-label="${escapeHtml(labels.copy)}" title="${escapeHtml(labels.copy)}" data-copied-label="${escapeHtml(labels.copied)}">${escapeHtml(labels.copy)}</button><pre><code${language}>${escapeHtml(text)}\n</code></pre></div>`;
		};
	}
	return marked.parse(md, { async: false, renderer }) as string;
}

export function renderMarkdown(md: string | null | undefined, labels?: CodeCopyLabels): string {
	return DOMPurify.sanitize(markdownMarkup(md, labels));
}
