import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

import { editableRoute, newRouteBlocker } from './automatic-routes.ts';

test('a new automatic route requires a selector and two distinct candidate models', () => {
	assert.equal(newRouteBlocker([], ['fast', 'expert']), 'selector');
	assert.equal(newRouteBlocker(['jev'], []), 'candidates');
	assert.equal(newRouteBlocker(['jev'], ['fast']), 'candidates');
	assert.equal(newRouteBlocker(['jev'], ['fast', 'fast']), 'candidates');
	assert.equal(newRouteBlocker(['jev'], ['fast', 'expert']), null);
});

test('editing copies a route without retaining Svelte-style nested state references', () => {
	const original = {
		alias: 'default',
		candidates: [{ key: 'fast', target: 'small', description: 'Routine work' }]
	};
	const editing = editableRoute(original);

	editing.alias = 'changed';
	editing.candidates[0].description = 'Changed description';

	assert.equal(original.alias, 'default');
	assert.equal(original.candidates[0].description, 'Routine work');
});

test('every route fieldset is allowed to shrink inside its responsive grid', () => {
	const component = readFileSync(
		new URL('./components/admin/AutomaticRoutesCard.svelte', import.meta.url),
		'utf8'
	);
	const fieldsets = [...component.matchAll(/<fieldset class="([^"]+)"/g)].map((match) => match[1]);

	assert.ok(fieldsets.length > 0);
	for (const classes of fieldsets) {
		assert.match(classes, /\bw-full\b/);
		assert.match(classes, /\bmin-w-0\b/);
	}
});

test('the session-affinity label can wrap at mobile width', () => {
	const component = readFileSync(
		new URL('./components/admin/AutomaticRoutesCard.svelte', import.meta.url),
		'utf8'
	);

	assert.match(component, /<label class="label[^"]*\bwhitespace-normal\b/);
});

test('the selector prompt and candidate descriptions explain their free-text format', () => {
	const component = readFileSync(
		new URL('./components/admin/AutomaticRoutesCard.svelte', import.meta.url),
		'utf8'
	);

	assert.match(component, /auto-route-instructions-help/);
	assert.match(component, /auto-route-instructions-placeholder/);
	assert.match(component, /auto-route-candidate-description-help/);
	assert.match(component, /auto-route-candidate-description-placeholder/);
});
