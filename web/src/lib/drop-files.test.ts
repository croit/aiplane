// The three drag-and-drop facts that are easy to get wrong and impossible to
// notice in review, pinned without a browser.
//
// Node has no `DataTransfer`, so the fakes below are hand-built objects with
// exactly the surface the module reads — which is the point: if the module
// starts reaching for `dataTransfer.files` during a *drag* (where it is always
// empty) instead of `types`, these tests fail.

import test from 'node:test';
import assert from 'node:assert/strict';

import { DragDepth, carriesFiles, droppedOnlyDirectories, filesFrom } from './drop-files.ts';

/** A drag in flight: types are known, files are not readable yet. */
function dragging(types: string[]): DataTransfer {
	return { types, files: [], items: [] } as unknown as DataTransfer;
}

function fileItem(file: File | null, isDirectory: boolean | null = null): DataTransferItem {
	return {
		kind: 'file',
		getAsFile: () => file,
		webkitGetAsEntry: isDirectory === null ? undefined : () => ({ isDirectory })
	} as unknown as DataTransferItem;
}

function dropped(items: DataTransferItem[], files: File[] = []): DataTransfer {
	return { types: ['Files'], items, files } as unknown as DataTransfer;
}

const FILE = new File(['hello'], 'notes.txt', { type: 'text/plain' });

test('a drag is recognised as carrying files before the drop', () => {
	// The whole point: during dragover `files` is empty, so the decision to
	// show the overlay and preventDefault() has to come from `types`.
	assert.equal(carriesFiles(dragging(['Files'])), true);
	assert.equal(carriesFiles(dragging(['text/plain'])), false);
	assert.equal(carriesFiles(dragging([])), false);
	assert.equal(carriesFiles(null), false);
});

test('selected text and links dragged into the chat are ignored', () => {
	// Dragging a highlighted word inside the transcript must not raise the
	// overlay — the user is selecting, not attaching.
	assert.equal(carriesFiles(dragging(['text/plain', 'text/html'])), false);
	assert.equal(carriesFiles(dragging(['text/uri-list'])), false);
});

test('a dropped folder is not staged as a zero-byte attachment', () => {
	const folder = new File([], 'photos', { type: '' });
	const transfer = dropped([fileItem(folder, true), fileItem(FILE, false)], [folder, FILE]);
	assert.deepEqual(
		filesFrom(transfer).map((f) => f.name),
		['notes.txt']
	);
	assert.equal(droppedOnlyDirectories(transfer), false);
});

test('a drop of nothing but folders is reported, not silently empty', () => {
	const folder = new File([], 'photos', { type: '' });
	const transfer = dropped([fileItem(folder, true)], [folder]);
	assert.deepEqual(filesFrom(transfer), []);
	assert.equal(droppedOnlyDirectories(transfer), true);
});

test('a browser without entry inspection still yields its files', () => {
	// `items` present but `webkitGetAsEntry` missing: fall back rather than
	// swallowing the drop.
	const transfer = dropped([fileItem(FILE)], [FILE]);
	assert.deepEqual(
		filesFrom(transfer).map((f) => f.name),
		['notes.txt']
	);
});

test('a browser with no DataTransferItemList still yields its files', () => {
	const transfer = { types: ['Files'], items: [], files: [FILE] } as unknown as DataTransfer;
	assert.deepEqual(
		filesFrom(transfer).map((f) => f.name),
		['notes.txt']
	);
	assert.deepEqual(filesFrom(null), []);
});

test('the overlay survives crossing a child element', () => {
	// dragenter(zone) → dragenter(bubble) → dragleave(zone): a naive boolean
	// would hide the overlay here, mid-drag, over the very element the user is
	// aiming at.
	const depth = new DragDepth();
	assert.equal(depth.enter(), true);
	assert.equal(depth.enter(), true);
	assert.equal(depth.leave(), true);
	assert.equal(depth.leave(), false);
});

test('leaving the window for good clears the overlay', () => {
	const depth = new DragDepth();
	depth.enter();
	assert.equal(depth.leave(), false);
	assert.equal(depth.active, false);
	// Stray extra leaves (they happen) must not drive the count negative, or
	// the next drag would need two enters before the overlay reappeared.
	assert.equal(depth.leave(), false);
	assert.equal(depth.enter(), true);
	assert.equal(depth.active, true);
});

test('a drop ends the gesture in one step', () => {
	const depth = new DragDepth();
	depth.enter();
	depth.enter();
	assert.equal(depth.reset(), false);
	assert.equal(depth.active, false);
});
