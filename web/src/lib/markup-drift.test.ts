// Drift guard for daisyUI classes that no longer exist in v5.
//
// daisyUI 4 shipped `form-control` (the label-plus-control wrapper) and
// `label-text-alt` (the dimmed, shrunk hint). Neither is a selector in v5, so
// an element carrying one gets NO layout: the label and its input fall back to
// `inline` and sit side by side, help text wedges between a label and its box,
// and the vertical rhythm turns into wrapped-paragraph line height. Nothing
// errors — it just looks broken, and adjusting `gap-*` does nothing because
// the gaps are not gaps.
//
// The server-rendered stack had two Rust tests pinning this. They went with it,
// so this is their replacement: the same check over the Svelte sources. It is a
// grep rather than a rendered-DOM assertion on purpose — the failure is the
// mere presence of the class, and a grep needs no browser.
//
// The house pattern for a labelled control:
//
//     <label class="flex flex-col gap-1">
//       <span class="text-xs">Name</span>
//       <input class="input" />
//       <span class="text-xs opacity-60">why this field exists</span>
//     </label>
//
// `label-text` is inert in v5 too, but harmless: v4 gave it `text-sm`, which a
// bare span inherits anyway. It is deliberately not failed on here.

import test from 'node:test';
import assert from 'node:assert';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';

/** Every `.svelte` file under `web/src`. */
function svelteFiles(dir: string, out: string[] = []): string[] {
	for (const entry of readdirSync(dir)) {
		const path = join(dir, entry);
		if (statSync(path).isDirectory()) svelteFiles(path, out);
		else if (entry.endsWith('.svelte')) out.push(path);
	}
	return out;
}

const DROPPED = ['form-control', 'label-text-alt'];

// daisyUI 4's `input-bordered` / `select-bordered` / `textarea-bordered` /
// `file-input-bordered` are inert in v5 — every field is bordered. Unlike the
// two above they break nothing visibly, which is why they spread: copied from
// one form to the next, they suggested a borderless default that does not
// exist and made two identical fields look like two different variants.
const INERT = ['input-bordered', 'select-bordered', 'textarea-bordered', 'file-input-bordered'];

test('no component carries an inert daisyUI 4 field variant', () => {
	const root = new URL('../..', import.meta.url).pathname;
	const offenders: string[] = [];
	for (const file of svelteFiles(join(root, 'src'))) {
		readFileSync(file, 'utf8')
			.split('\n')
			.forEach((line, i) => {
				for (const cls of INERT) {
					if (new RegExp(`(?<![\\w-])${cls}(?![\\w-])`).test(line)) offenders.push(`${file.slice(root.length)}:${i + 1}: ${cls}`);
				}
			});
	}
	assert.deepEqual(offenders, [], `daisyUI 5 fields are always bordered — drop the class:\n` + offenders.join('\n'));
});

test('no component uses a class daisyUI 5 dropped', () => {
	const root = new URL('../..', import.meta.url).pathname;
	const offenders: string[] = [];
	for (const file of svelteFiles(join(root, 'src'))) {
		const body = readFileSync(file, 'utf8');
		body.split('\n').forEach((line, i) => {
			for (const cls of DROPPED) {
				// Word-boundary match so `form-control-ish` names don't trip it.
				if (new RegExp(`\\b${cls}\\b`).test(line)) {
					offenders.push(`${file.slice(root.length)}:${i + 1}: ${cls}`);
				}
			}
		});
	}
	assert.deepEqual(
		offenders,
		[],
		`daisyUI 5 has no such class — the element gets no layout at all.\n` +
			`Use <label class="flex flex-col gap-1"> with the hint after the input:\n` +
			offenders.join('\n')
	);
});

// The app was written against the daisyUI component vocabulary; the token
// layer in app.css keeps those names. Any source that carries a class from
// that vocabulary must find it defined in app.css — otherwise the element
// renders unstyled and nothing errors. Tailwind utilities never match the
// pattern below, so they are not checked.
const COMPONENT_PATTERN =
	/^(accordion|alert|avatar|badge|btn|card|chat|checkbox|collapse|divider|dropdown|fieldset|file|indicator|input|join|kbd|label|legend|link|loading|menu|mockup|modal|navbar|pagination|placeholder|progress|range|radio|rating|select|skeleton|stat|stats|steps|tab|table|tabs|textarea|toast|toggle|tooltip|tree)(-[a-z0-9]+)*$/;

// Tailwind utilities that look like component variants: user-select.
const TAILWIND_LOOKALIKES = new Set(['select-all', 'select-none']);

test('every component class the sources use is defined in app.css', () => {
	const root = new URL('../..', import.meta.url).pathname;
	const css = readFileSync(join(root, 'src/app.css'), 'utf8');
	const offenders: string[] = [];
	const seen = new Set<string>();
	for (const file of svelteFiles(join(root, 'src'))) {
		const body = readFileSync(file, 'utf8');
		const tokens: string[] = [];
		for (const m of body.matchAll(/class="([^"]*)"/g)) tokens.push(...m[1].split(/\s+/));
		for (const m of body.matchAll(/class:([a-z0-9-]+)/g)) tokens.push(m[1]);
		for (const token of tokens) {
			if (!COMPONENT_PATTERN.test(token) || TAILWIND_LOOKALIKES.has(token) || seen.has(token)) continue;
			seen.add(token);
			if (!new RegExp(`\\.${token}([^\\w-]|$)`).test(css)) {
				offenders.push(`${file.slice(root.length)}: ${token}`);
			}
		}
	}
	assert.deepEqual(
		offenders,
		[],
		`these classes are part of the component vocabulary but app.css does not define them —\n` +
			`the element renders unstyled. Implement them in app.css, or drop the class if it is inert:\n` +
			offenders.join('\n')
	);
});

// A modal has to read as a layer ABOVE the page. In the dark theme the box
// and the page are both near-black, so without an edge of their own a dialog
// has no visible boundary: the box carries a `base-300` border plus a deep
// shadow, and the native `<dialog>` backdrop carries the scrim. Losing any of
// those silently restores the unreadable dialog — nothing errors, it just
// stops looking like a modal. Hence this grep.
test('app.css gives every modal a visible edge over a dark page', () => {
	const css = readFileSync(new URL('../app.css', import.meta.url).pathname, 'utf8');

	const box = /\.modal-box\s*\{([^}]*)\}/.exec(css);
	assert.ok(box, '.modal-box has no override in app.css');
	assert.match(box[1], /border:\s*1px solid var\(--color-base-300\)/, 'modal box has no border');
	assert.match(box[1], /box-shadow:/, 'modal box has no elevation shadow');

	// The scrim: over a near-black page it has to pull away from it.
	const scrim = /dialog\.modal(?:\[open\])?::backdrop\s*\{([^}]*)\}/.exec(css);
	assert.ok(scrim, 'the dialog backdrop has no override in app.css');
	const opacity = /background-color:\s*oklch\(0% 0 0 \/ ([\d.]+)\)/.exec(scrim[1]);
	assert.ok(opacity, 'the backdrop override does not set a background colour');
	assert.ok(
		Number(opacity[1]) > 0.4,
		`backdrop opacity ${opacity[1]} is too thin to separate the dialog from the page`
	);
});

// A popover menu is laid out against its trigger through CSS anchor
// positioning. The `dropdown` class is a relative-positioned wrapper for a
// trigger; on the popover itself it overrides the top layer's fixed placement
// and the menu lands wherever it happens to sit in the flow — in the chat
// header that was over the title, floating across the page.
test('a popover menu anchors below its trigger instead of carrying the dropdown wrapper class', () => {
	const css = readFileSync(new URL('../app.css', import.meta.url).pathname, 'utf8');
	const rule = /\.dropdown-content\s*\{([^}]*)\}/.exec(css);
	assert.ok(rule, '.dropdown-content has no rule in app.css');
	assert.match(rule[1], /position-area:\s*bottom center/, 'the menu is not scoped to the trigger\'s column');
	assert.match(rule[1], /justify-self:\s*end/, 'the menu is not aligned to the trigger\'s inline end');

	const root = new URL('../..', import.meta.url).pathname;
	const offenders: string[] = [];
	for (const file of svelteFiles(join(root, 'src'))) {
		readFileSync(file, 'utf8').split('\n').forEach((line, i) => {
			if (!/\bpopover\b/.test(line)) return;
			for (const m of line.matchAll(/class="([^"]*)"/g)) {
				if (m[1].split(/\s+/).includes('dropdown')) offenders.push(`${file.slice(root.length)}:${i + 1}`);
			}
		});
	}
	assert.deepEqual(offenders, [], 'the popover takes its placement from .dropdown-content, not the .dropdown wrapper:\n' + offenders.join('\n'));
});

// A file field is a field like any other. Without the token layer's
// `.file-input` the browser draws its own control — OS-language "Choose
// file" chrome in the middle of a dark card, matching no other field.
test('a file field draws the same field as every text input', () => {
	const css = readFileSync(new URL('../app.css', import.meta.url).pathname, 'utf8');
	const field = /\.file-input\s*\{([^}]*)\}/.exec(css);
	assert.ok(field, '.file-input has no rule in app.css');
	assert.match(field[1], /border:\s*1px solid var\(--color-base-300\)/);
	assert.match(field[1], /border-radius:\s*var\(--radius-field\)/);
	const button = /\.file-input::file-selector-button\s*\{([^}]*)\}/.exec(css);
	assert.ok(button, 'the native file picker button keeps the browser chrome');
	assert.match(button[1], /background-color:\s*var\(--color-base-200\)/);
	assert.match(button[1], /border-radius:\s*var\(--radius-selector\)/);
});

// A search field that wraps its input draws the field on the <label> and the
// input inside is meant to be text only. Without the child rule the input
// keeps the browser's default width (narrower than the field) and its own
// background, border and focus outline — the field shows a second box inside
// its own edge, and the typed text sits in it instead of filling the field.
test('a field that wraps its control draws one field, not two', () => {
	const css = readFileSync(new URL('../app.css', import.meta.url).pathname, 'utf8');
	const field = /label\.input\s*\{([^}]*)\}/.exec(css);
	assert.ok(field, 'app.css does not lay out a wrapping label.input');
	assert.match(field[1], /display:\s*flex/, 'the icon and the control do not share the field row');
	const control = /label\.input > input\s*\{([^}]*)\}/.exec(css);
	assert.ok(control, 'the input inside a wrapping label.input gets no override');
	for (const [property, expected] of [
		['flex', /flex:\s*1/],
		['border', /border:\s*0/],
		['outline', /outline:\s*none/],
		['min-width', /min-width:\s*0/]
	] as const) {
		assert.match(control[1], expected, `the inner input keeps its own ${property}`);
	}
});

// The same trap as `.collapse`, inverted: a closed popover is hidden by the// UA stylesheet only, and the component classes set a `display` of their own
// (`.menu` is a flex column). Author display beats the UA rule, so without an
// unlayered guard every `popover` menu is visible while closed — permanently
// expanded, and toggling it changes nothing on screen.
test('a closed popover stays hidden whatever display its classes set', () => {
	const css = readFileSync(new URL('../app.css', import.meta.url).pathname, 'utf8');
	assert.match(
		css,
		/^\[popover\]:not\(:popover-open\)\s*\{\s*display:\s*none;?\s*\}/m,
		'a closed popover is shown by its component display — add the unlayered guard'
	);
});

// Tailwind ships a `.collapse` utility whose whole body is `visibility:
// collapse` — and utilities outrank the components layer. The app's disclosure
// is `<details class="collapse">`, so without an unlayered declaration the
// closed <details> hides its own summary and every disclosure in the app looks
// empty. Nothing errors; the control simply vanishes.
test('the disclosure wins the `.collapse` name against the Tailwind utility', () => {
	const css = readFileSync(new URL('../app.css', import.meta.url).pathname, 'utf8');
	const rule = /^\.collapse\s*\{([^}]*)\}/m.exec(css);
	assert.ok(rule, '.collapse has no unlayered override in app.css');
	assert.match(rule[1], /visibility:\s*visible/, 'the closed disclosure would hide its summary');
});
