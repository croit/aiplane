/**
 * Markdown rendering for chat replies (issue #22 phase 2).
 *
 * The legacy UI renders markdown server-side (Plait renderers in
 * session-core); the SPA renders client-side instead — that is the point of
 * the protocol change: the wire carries markdown text, the client owns the
 * pixels. `marked` parses, DOMPurify sanitises; model output is untrusted
 * input like any other, so the pipeline is parse → sanitise → `{@html}`.
 */
import { marked } from 'marked';
import DOMPurify from 'dompurify';

marked.setOptions({ gfm: true, breaks: true });

export function renderMarkdown(md: string | null | undefined): string {
	if (!md) return '';
	return DOMPurify.sanitize(marked.parse(md, { async: false }) as string);
}
