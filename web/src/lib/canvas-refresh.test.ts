import test from 'node:test';
import assert from 'node:assert/strict';

import { readFileSync } from 'node:fs';
import { join } from 'node:path';

import { canvasRefresh, type CanvasState } from './canvas-refresh.ts';

const ROOT = new URL('../..', import.meta.url).pathname;
const read = (rel: string): string => readFileSync(join(ROOT, rel), 'utf8');

const state = (overrides: Partial<CanvasState> = {}): CanvasState => ({
	documents: [{ id: 'doc-1', current_ver: 4 }],
	selectedId: 'doc-1',
	opened: { id: 'doc-1', shownVersion: 4, knownHead: 4 },
	editing: false,
	...overrides
});

test('nothing to do while the open view is at the head the list reports', () => {
	assert.deepEqual(canvasRefresh(state()), { action: 'none' });
});

// The regression this module exists for: the model edits the document
// mid-turn, the list refreshes, and the panel kept showing the old version
// until the page was reloaded.
test('a version written by the model re-reads the open document', () => {
	assert.deepEqual(canvasRefresh(state({ documents: [{ id: 'doc-1', current_ver: 9 }] })), {
		action: 'open',
		documentId: 'doc-1'
	});
});

test('a revision opened on purpose stays put, but is re-read for its history', () => {
	assert.deepEqual(
		canvasRefresh(
			state({
				documents: [{ id: 'doc-1', current_ver: 9 }],
				opened: { id: 'doc-1', shownVersion: 2, knownHead: 4 }
			})
		),
		{ action: 'open', documentId: 'doc-1', version: 2 }
	);
});

test('a hand edit in progress is never overwritten — the panel is told instead', () => {
	assert.deepEqual(
		canvasRefresh(state({ documents: [{ id: 'doc-1', current_ver: 9 }], editing: true })),
		{ action: 'notify', documentId: 'doc-1', version: 9 }
	);
});

test('an unchanged document does not interrupt a hand edit', () => {
	assert.deepEqual(canvasRefresh(state({ editing: true })), { action: 'none' });
});

test('the first document is opened when nothing is selected yet', () => {
	assert.deepEqual(canvasRefresh(state({ selectedId: '', opened: null })), {
		action: 'open',
		documentId: 'doc-1'
	});
});

test('a deleted selection falls back to the first remaining document', () => {
	assert.deepEqual(
		canvasRefresh(
			state({
				documents: [{ id: 'doc-2', current_ver: 1 }],
				opened: { id: 'doc-1', shownVersion: 4, knownHead: 4 }
			})
		),
		{ action: 'open', documentId: 'doc-2' }
	);
});

test('the last document going away clears the panel', () => {
	assert.deepEqual(canvasRefresh(state({ documents: [] })), { action: 'clear' });
});

// The decision above is only reached if the panel asks it, and only asked
// with fresh data if the view re-reads the document list when the
// conversation says it changed. Both are one line each and neither shows up
// as a broken page when it goes missing — the canvas just quietly goes stale
// again, which is the bug this module was written for.
test('the canvas panel reconciles the open document against the list', () => {
	const panel = read('src/lib/components/chat/ConversationCanvas.svelte');
	assert.match(panel, /from '\$lib\/canvas-refresh'/);
	assert.match(panel, /\$effect\(\(\) => \{[\s\S]*?canvasRefresh\(/);
});

test('both live cues re-read the document list', () => {
	const view = read('src/lib/Conversation.svelte');
	for (const cue of ['onSidebarChanged', 'onTurnFinalized']) {
		const start = view.indexOf(`c.${cue} =`);
		assert.ok(start >= 0, `${cue} is not wired at all`);
		const end = view.indexOf('\n\t\tc.', start + 1);
		assert.match(view.slice(start, end), /loadDocuments\(\)/, `${cue} must re-read the documents`);
	}
});

// The cue itself. A turn that finishes before the browser re-attaches its
// event stream is reported as a snapshot plus `idle` — no `turn_finalized`,
// no `sidebar_changed` — so without this branch a fast turn (a cached reply,
// a tool-only round) leaves the canvas and the conversation title behind.
test('a turn that finishes before the re-attach still counts as finished', () => {
	const controller = read('src/lib/chat.svelte.ts');
	assert.match(controller, /event\.type === 'idle' && attaches > 1/);
	assert.match(controller, /attaches \+= 1;/);
});
