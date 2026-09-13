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
//       <input class="input input-bordered input-sm" />
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

// A modal has to read as a layer ABOVE the page, and in the dark theme
// daisyUI's defaults do not achieve it: the box and the page are both
// `base-100` (oklch 14%), the scrim between them is `black / 0.4` — which over
// a near-black page is a handful of lightness points — and the elevation
// shadow is `black / 0.25`, invisible on black. The result was a dialog whose
// edges you could not find.
//
// `app.css` overrides all three. They are one rule each, outside
// `@layer daisyui` so they win without `!important`, and losing any of them
// silently restores the unreadable dialog — nothing errors, it just stops
// looking like a modal. Hence this grep.
test('app.css gives every modal a visible edge over a dark page', () => {
	const css = readFileSync(new URL('../app.css', import.meta.url).pathname, 'utf8');

	const box = /\.modal-box\s*\{([^}]*)\}/.exec(css);
	assert.ok(box, '.modal-box has no override in app.css');
	assert.match(box[1], /border:\s*1px solid var\(--color-base-300\)/, 'modal box has no border');
	assert.match(box[1], /box-shadow:/, 'modal box has no elevation shadow');

	// The scrim: daisyUI ships 0.4, which is not enough separation on a
	// near-black surface.
	const scrim = /\.modal\.modal-open,[\s\S]*?\{([^}]*)\}/.exec(css);
	assert.ok(scrim, 'the open-modal scrim has no override in app.css');
	const opacity = /background-color:\s*oklch\(0% 0 0 \/ ([\d.]+)\)/.exec(scrim[1]);
	assert.ok(opacity, 'the scrim override does not set a background colour');
	assert.ok(
		Number(opacity[1]) > 0.4,
		`scrim opacity ${opacity[1]} is no darker than daisyUI's own 0.4`
	);
});
