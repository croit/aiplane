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

/**
 * Sanitise a model-authored SVG preview for an `ask_user` option.
 *
 * Separate from {@link renderMarkdown} and deliberately much stricter, because
 * of where the output lands. The ask_user card exists to get a trustworthy
 * decision out of a person; graphics drawn by the model — which means, through
 * anything the model read this turn, graphics an attacker can influence — are
 * the last thing that should be able to navigate, fetch, or impersonate the
 * page's own controls inside it.
 *
 * So beyond the obvious scripting:
 *   - `a`, `href` and `xlink:href` go, killing navigation from inside the card
 *     and the `<animate attributeName="href">` trick that smuggles a
 *     javascript: URL past a naive tag filter;
 *   - `use` and `image` go, so nothing is fetched from off-origin and no part
 *     of the drawing arrives after the sanitiser has looked at it;
 *   - `foreignObject` goes, because its whole purpose is to re-enter HTML and
 *     it is the standard way to smuggle arbitrary markup through an
 *     SVG-shaped hole.
 *
 * What is left is drawing: shapes, paths, text, gradients, transforms.
 */
export function sanitizeSvgPreview(svg: string): string {
	return DOMPurify.sanitize(svg, {
		USE_PROFILES: { svg: true, svgFilters: true },
		FORBID_TAGS: [
			'script',
			'foreignObject',
			'a',
			'use',
			'image',
			'animate',
			'animateMotion',
			'animateTransform',
			'set',
			'handler',
			'style'
		],
		FORBID_ATTR: ['href', 'xlink:href', 'formaction', 'from', 'to', 'values', 'attributeName'],
		ALLOW_DATA_ATTR: false
	});
}

/**
 * Markdown for the `ask_user` card's question and option descriptions.
 *
 * Not {@link renderMarkdown}. That one runs DOMPurify's default profile, which
 * allows raw HTML and, notably, SVG — so this passed through it untouched:
 *
 *     <svg><a href="https://evil.example.com"><text>Click to approve</text></a></svg>
 *
 * An off-site link, drawn to look like one of the card's own buttons, inside
 * the one control whose entire job is to obtain a trustworthy decision from
 * the person reading it. That is fine-ish in the transcript, where model
 * output is plainly the model talking; it is not fine here.
 *
 * So this is an allowlist of text formatting and nothing else. No raw HTML, no
 * SVG, no attributes, no links — `KEEP_CONTENT` leaves an anchor's words
 * behind while the anchor itself goes, so a model that writes a link still
 * gets its sentence across. Drawings come in only through `preview`, typed as
 * such by the tool call and sanitised by {@link sanitizeSvgPreview}: one
 * explicit door instead of two, one of them ajar.
 */
export function renderPromptMarkdown(md: string | null | undefined): string {
	if (!md) return '';
	return DOMPurify.sanitize(marked.parse(md, { async: false }) as string, {
		ALLOWED_TAGS: ['p', 'br', 'strong', 'em', 'del', 'code', 'pre', 'ul', 'ol', 'li'],
		ALLOWED_ATTR: [],
		KEEP_CONTENT: true
	});
}
