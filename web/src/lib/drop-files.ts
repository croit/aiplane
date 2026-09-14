// Drag-and-drop file staging for the conversation page.
//
// The composer already accepted a drop, but only on the `<textarea>` itself —
// a 44px-tall target that is `disabled` (and therefore fires no drop events at
// all) for as long as a turn is streaming. Dropping a file anywhere else on the
// conversation handed it to the *browser*, which navigated away from the chat
// to render the PDF. This module is the logic behind a page-wide drop target;
// `Conversation.svelte` owns the markup and the staged-file list.
//
// Two things here are less obvious than they look, and both are why this is a
// module with tests rather than three inline handlers:
//
//   1. **During a drag there are no files yet.** `dataTransfer.files` is empty
//      until the `drop` event — the drag-over phase only exposes `types` (and
//      `items` with no data). So "is this drag carrying files?", which decides
//      whether we show the overlay and call `preventDefault()`, must be
//      answered from `types`.
//   2. **`dragenter`/`dragleave` fire per element, not per zone.** Moving the
//      pointer from the transcript onto a chat bubble inside it fires `leave`
//      then `enter`; a naive boolean flickers the overlay on every bubble
//      crossed. The fix is to count enters and leaves and only drop the
//      overlay when the count returns to zero.

/** Whether a drag is carrying files (as opposed to selected text, a link, …). */
export function carriesFiles(transfer: DataTransfer | null | undefined): boolean {
	if (!transfer) return false;
	// `types` is a DOMStringList in older engines — not an array — so read it
	// through `Array.from` rather than calling `.includes` on it directly.
	return Array.from(transfer.types ?? []).includes('Files');
}

/**
 * The files of a completed drop, with directories dropped.
 *
 * A dropped folder arrives as a zero-byte, type-less `File` that would upload
 * as a corrupt empty attachment. `webkitGetAsEntry()` is the only way to tell
 * one from a genuinely empty file, and it is only available on `items` — so we
 * consult `items` when the browser provides it and fall back to `files` when it
 * does not.
 */
export function filesFrom(transfer: DataTransfer | null | undefined): File[] {
	if (!transfer) return [];
	const items = transfer.items;
	if (items && items.length > 0) {
		const out: File[] = [];
		let sawEntry = false;
		for (const item of Array.from(items)) {
			if (item.kind !== 'file') continue;
			const entry = item.webkitGetAsEntry?.();
			if (entry) {
				sawEntry = true;
				if (entry.isDirectory) continue;
			}
			const file = item.getAsFile();
			if (file) out.push(file);
		}
		// Only trust the `items` walk when it actually yielded something (or
		// when the browser told us about entries at all); otherwise a partial
		// `DataTransferItemList` implementation would silently swallow the drop.
		if (out.length > 0 || sawEntry) return out;
	}
	return Array.from(transfer.files ?? []);
}

/** Whether a drop contained something, but every item of it was a folder. */
export function droppedOnlyDirectories(transfer: DataTransfer | null | undefined): boolean {
	if (!transfer) return false;
	const items = Array.from(transfer.items ?? []).filter((item) => item.kind === 'file');
	if (items.length === 0) return false;
	return (
		items.every((item) => item.webkitGetAsEntry?.()?.isDirectory === true) &&
		filesFrom(transfer).length === 0
	);
}

/**
 * Enter/leave counter for one drop zone.
 *
 * `enter()` and `leave()` return whether the zone is highlighted *after* the
 * event, so a caller can assign the result straight to its reactive flag.
 */
export class DragDepth {
	private depth = 0;

	enter(): boolean {
		this.depth += 1;
		return true;
	}

	leave(): boolean {
		this.depth = Math.max(0, this.depth - 1);
		return this.depth > 0;
	}

	/** A drop (or a cancelled drag) ends the whole gesture at once. */
	reset(): boolean {
		this.depth = 0;
		return false;
	}

	get active(): boolean {
		return this.depth > 0;
	}
}
