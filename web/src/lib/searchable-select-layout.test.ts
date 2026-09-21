import { readFileSync } from 'node:fs';
import assert from 'node:assert/strict';
import { describe, test } from 'node:test';

const searchableSelect = readFileSync(new URL('./components/SearchableSelect.svelte', import.meta.url), 'utf8');

describe('searchable select option layout', () => {
	test('uses compact aligned rows without daisyUI menu layout', () => {
		assert.doesNotMatch(searchableSelect, /class="menu [^"]*" role="listbox"/);
		assert.doesNotMatch(searchableSelect, /class="btn btn-outline/);
		assert.match(searchableSelect, /border-base-300 bg-base-200/);
		assert.match(searchableSelect, /focus-within:outline-info/);
		assert.doesNotMatch(searchableSelect, /border-primary bg-base-200/);
		assert.match(searchableSelect, /grid-cols-\[1rem_minmax\(0,1fr\)\]/);
		assert.match(searchableSelect, /badge badge-success badge-xs/);
		assert.match(searchableSelect, /badge badge-error badge-xs[^\"]*line-through/);
		assert.match(searchableSelect, /justify-between gap-x-3/);
		assert.match(searchableSelect, /justify-end gap-1/);
	});

	// Multi-value grant fields (a group's tools/skills, a resource's allowed
	// groups) share this component rather than forking the popup: the placement
	// below is the subtle part, and two copies of it would drift.
	test('multiple mode marks the listbox, keeps the panel open, and offers a reset', () => {
		assert.match(searchableSelect, /aria-multiselectable=\{multiple \? true : undefined\}/);
		assert.match(searchableSelect, /aria-selected=\{isChosen\(option\.value\)\}/);
		// `choose` returns before `hide()` in multiple mode — picking several
		// grants must not close the panel between every click.
		assert.match(searchableSelect, /values = toggleValue\(values, option\.value\);/);
		assert.match(searchableSelect, /multi-select-clear/);
		// `popover="manual"` turns off the browser's light dismiss, and in multiple
		// mode a click leaves focus on the option button — so every control inside
		// the panel has to be able to close it, not just the search field.
		assert.match(searchableSelect, /function onEscape\(event: KeyboardEvent\)/);
		assert.match(searchableSelect, /onkeydown=\{onEscape\}[\s\S]*onclick=\{\(\) => choose\(option\)\}/);
	});

	// The popup used to be an absolutely positioned `dropdown-content`, which
	// every scrolling ancestor was free to clip — in the document canvas it cut
	// the option rows off at the panel edge, leaving a box that looked empty and
	// passed every click through to the chat behind it. It now lives in the
	// browser's top layer at viewport coordinates.
	test('raises the popup into the top layer instead of a clipped absolute panel', () => {
		assert.doesNotMatch(searchableSelect, /dropdown-content/);
		assert.doesNotMatch(searchableSelect, /absolute right-0 top-full/);
		assert.match(searchableSelect, /popover="manual"/);
		assert.match(searchableSelect, /node\.showPopover\(\)/);
		assert.match(searchableSelect, /class="fixed [^"]*"/);
		assert.match(searchableSelect, /style:left=/);
		assert.match(searchableSelect, /style:max-height=/);
		// Nothing moves the top layer with the trigger, so the popup has to
		// follow it itself while open.
		assert.match(searchableSelect, /addEventListener\('scroll', follow, true\)/);
		assert.match(searchableSelect, /addEventListener\('resize', follow\)/);
	});
});
