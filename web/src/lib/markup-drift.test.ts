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
