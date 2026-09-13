// Drift guard for the feedback widget's cross-file contracts.
//
// Three of them are invisible when they break — the widget keeps working,
// it just does the wrong thing — so a grep over the sources is worth more
// here than a rendered-DOM assertion:
//
//   1. The screenshot excludes the widget's own chrome by data attribute.
//      Lose the attribute and every report arrives with a picture of the
//      dialog that produced it, over the page nobody can now see.
//   2. The FAB and dialog are mounted in the ROOT layout. Move them into a
//      route and they unmount on navigation, taking the open dialog and the
//      half-typed report with them.
//   3. Capture starts in the root layout's `load`, the earliest client code
//      in the app. Start it later and the console/network buffers miss
//      exactly the startup requests a "the app didn't load" report is about.

import test from 'node:test';
import assert from 'node:assert';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

const ROOT = new URL('../..', import.meta.url).pathname;
const read = (rel: string): string => readFileSync(join(ROOT, rel), 'utf8');

test('every selector the capture excludes is carried by a component', () => {
	const capture = read('src/lib/feedback-screenshot.ts');
	const excluded = [...capture.matchAll(/\[data-feedback-[a-z-]+\]/g)].map((m) => m[0]);
	assert.ok(excluded.length >= 3, `expected the exclusion list, found ${excluded.length}`);

	const markup = [
		read('src/lib/components/feedback/FeedbackFab.svelte'),
		read('src/lib/components/feedback/FeedbackDialog.svelte'),
		read('src/routes/+layout.svelte')
	].join('\n');

	for (const selector of excluded) {
		const attribute = selector.slice(1, -1); // [data-feedback-fab] -> data-feedback-fab
		assert.ok(
			markup.includes(attribute),
			`${selector} is excluded from the screenshot but no element carries ${attribute}`
		);
	}
});

test('the FAB and the dialog are mounted by the root layout', () => {
	const layout = read('src/routes/+layout.svelte');
	assert.match(layout, /components\/feedback\/FeedbackFab\.svelte/);
	assert.match(layout, /components\/feedback\/FeedbackDialog\.svelte/);
	assert.match(layout, /<FeedbackFab\b/);
	assert.match(layout, /<FeedbackDialog\b/);
});

// The floating button and the chat composer both live in the bottom-right
// corner, and the composer wins: send/stop/mic/attach are there. So the layout
// suppresses the FAB on a conversation page — which only works as long as the
// composer keeps its own entry point, or chat pages lose feedback entirely.
test('a conversation page suppresses the FAB and the composer carries the entry point', () => {
	const layout = read('src/routes/+layout.svelte');
	assert.match(
		layout,
		/\{#if !isChatActive\(\)\}\s*<FeedbackFab \/>/,
		'the FAB is not gated on isChatActive() — it will sit on top of the composer'
	);
	// The dialog itself must stay mounted on chat pages; the composer opens it.
	assert.ok(
		layout.indexOf('<FeedbackDialog />') > layout.indexOf('{#if !isChatActive()}'),
		'the dialog must be mounted outside the non-chat guard'
	);

	const composer = read('src/lib/Conversation.svelte');
	assert.match(composer, /openDialog as openFeedback/);
	assert.match(composer, /data-feedback-fab/, 'the composer button is not excluded from the capture');
	assert.doesNotMatch(
		composer,
		/2xl:hidden[^>]*openFeedback|openFeedback[^>]*2xl:hidden/,
		'the composer entry point must not be width-gated — it is the only one on chat pages'
	);
});

test('diagnostics capture is started from the root layout load', () => {
	const layoutLoad = read('src/routes/+layout.ts');
	assert.match(layoutLoad, /initFeedbackCapture\(\)/);
	assert.match(layoutLoad, /from '\$lib\/feedback-capture'/);
});

test('the dialog posts to the versioned feedback endpoints', () => {
	// The three endpoints live under /api/v0 like the rest of the API; a
	// report that silently posts to the pre-SPA `/feedback` path 404s.
	const store = read('src/lib/feedback.svelte.ts');
	for (const path of [
		'/api/v0/feedback/config',
		'/api/v0/feedback/extract',
		'/api/v0/feedback',
		'/api/v0/transcriptions'
	]) {
		assert.ok(store.includes(`'${path}'`), `feedback store never calls ${path}`);
	}
});
