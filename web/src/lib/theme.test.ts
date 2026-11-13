// The two theme token blocks in app.css (`:root` light, `[data-theme="dark"]`
// dark) are one design in two lightnesses. These checks pin the parts of that
// contract a reviewer cannot see in a diff: both themes define the same
// tokens, the text/fill pairs stay readable, and the app-wide field states
// (focus ring, error) are wired.

import test from 'node:test';
import assert from 'node:assert';
import { readFileSync } from 'node:fs';

const css = readFileSync(new URL('../app.css', import.meta.url).pathname, 'utf8');

function themes(): Map<string, Map<string, string>> {
	const out = new Map<string, Map<string, string>>();
	const blocks: Array<[string, RegExp]> = [
		['light', /:root\s*\{([^}]*)\}/],
		['dark', /\[data-theme="dark"\]\s*\{([^}]*)\}/]
	];
	for (const [name, pattern] of blocks) {
		const block = css.match(pattern);
		assert.ok(block, `the ${name} theme block is missing`);
		const tokens = new Map<string, string>();
		for (const m of block[1].matchAll(/(--t-[\w-]+):\s*([^;]+);/g)) tokens.set(m[1], m[2].trim());
		out.set(name, tokens);
	}
	return out;
}

function luminance(hex: string): number {
	const m = /^#([0-9a-f]{6})$/i.exec(hex);
	assert.ok(m, `${hex} is not a #rrggbb colour — the contrast check needs hex tokens`);
	const [r, g, b] = [0, 2, 4].map((i) => {
		const v = parseInt(m[1].slice(i, i + 2), 16) / 255;
		return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
	});
	return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function contrast(a: string, b: string): number {
	const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
	return (hi + 0.05) / (lo + 0.05);
}

test('app.css defines a light and a dark theme with the same tokens', () => {
	const all = themes();
	assert.deepEqual([...all.keys()].sort(), ['dark', 'light']);
	const [light, dark] = [all.get('light')!, all.get('dark')!];
	assert.ok(light.size > 15, 'the light theme defines no tokens');
	assert.deepEqual([...light.keys()].sort(), [...dark.keys()].sort());
});

test('every text-on-fill pair in both themes reaches WCAG AA (4.5:1)', () => {
	const pairs = [
		['--t-base-content', '--t-base-100'],
		['--t-base-content', '--t-base-200'],
		['--t-primary-content', '--t-primary'],
		['--t-link', '--t-base-100'],
		['--t-link', '--t-base-200'],
		['--t-neutral-content', '--t-neutral'],
		['--t-success-content', '--t-success'],
		['--t-warning-content', '--t-warning'],
		['--t-error-content', '--t-error'],
		['--t-info-content', '--t-info']
	];
	for (const [name, tokens] of themes()) {
		for (const [fg, bg] of pairs) {
			const ratio = contrast(tokens.get(fg)!, tokens.get(bg)!);
			assert.ok(ratio >= 4.5, `${name}: ${fg} on ${bg} is ${ratio.toFixed(2)}:1`);
		}
	}
});

test('the croit VI pins hold: violet primary and 18px box radius', () => {
	for (const [name, tokens] of themes()) {
		assert.equal(tokens.get('--t-primary')!.toLowerCase(), '#8e54e9', `${name}: the action accent is not croit violet`);
	}
	assert.match(css, /--radius-box:\s*1\.125rem/);
});

test('rendered markdown takes its colours from the theme, not the prose defaults', () => {
	const prose = /:root \.prose\s*\{([^}]*)\}/.exec(css);
	assert.ok(prose, 'app.css does not map the prose colours onto the tokens');
	for (const variable of ['--tw-prose-body', '--tw-prose-headings', '--tw-prose-links']) {
		const themed = new RegExp(`${variable}:\\s*(var\\(--color-[\\w-]+\\)|color-mix\\(in oklab, var\\(--color-base-content\\))`);
		assert.match(prose[1], themed, `${variable} keeps the typography plugin's light default`);
	}
});

test('a plain button keeps a visible edge on a card', () => {
	assert.match(css, /\.btn\s*\{[^}]*border:\s*1px solid var\(--color-base-300\)/s);
});

test('focused fields warm up in the primary colour, invalid ones read as errors', () => {
	assert.match(
		css,
		/:where\(\.input, \.select, \.textarea\):is\(:focus, :focus-within\)\s*\{\s*outline:\s*none;\s*border-color:\s*color-mix\(in oklab, var\(--color-primary\)/,
		'no primary focus edge on fields'
	);
	assert.match(
		css,
		/:where\(\.input, \.select, \.textarea\):is\(:focus, :focus-within\)\s*\{[^}]*box-shadow:\s*0 0 0 3px color-mix\(in oklab, var\(--color-primary\)/,
		'no soft primary halo on focused fields'
	);
	assert.match(css, /\.input\[aria-invalid="true"\][\s\S]*?border-color:\s*var\(--color-error\)/, 'aria-invalid fields are not marked');
	assert.ok(
		css.indexOf('.input[aria-invalid="true"]') > css.indexOf(':where(.input, .select, .textarea)'),
		'the error rule must follow the focus rule, or a focused invalid field rings in primary'
	);
});
