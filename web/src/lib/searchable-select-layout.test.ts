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
});
