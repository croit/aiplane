/**
 * What the canvas panel should show after the conversation's document list
 * changed underneath it.
 *
 * The panel fetches a document once (content + history) and then holds it in
 * `opened`. A tool call that writes to the canvas only pushes "the documents
 * changed, re-read them" down the event stream, so the *list* refreshes while
 * the open document does not — which is how the panel ended up showing v4 of
 * a document the model had already grown to v9, until the page was reloaded.
 *
 * The rule is: compare the head version the list reports against the head the
 * open view was fetched at, and re-read when they differ. Kept out of the
 * component (and free of Svelte) so the decision itself is testable, because
 * every wrong answer here is invisible — the panel keeps working, it just
 * shows something stale, or throws away a hand edit.
 */

/** As much of a document listing entry as the decision needs. */
export interface DocumentHead {
	id: string;
	current_ver: number;
}

/** As much of the open document as the decision needs. */
export interface OpenedDocument {
	id: string;
	/** The version whose content is on screen. */
	shownVersion: number;
	/** The head version as of the fetch that produced this view. */
	knownHead: number;
}

export type CanvasRefresh =
	/** Nothing to do — the open view is current. */
	| { action: 'none' }
	/** No documents left: the panel has nothing to show. */
	| { action: 'clear' }
	/** (Re)read this document; `version` absent means "the current one". */
	| { action: 'open'; documentId: string; version?: number }
	/** A newer version exists but a hand edit is in progress — say so. */
	| { action: 'notify'; documentId: string; version: number };

export interface CanvasState {
	documents: DocumentHead[];
	selectedId: string;
	opened: OpenedDocument | null;
	/** The panel's textarea is open with an unsaved draft. */
	editing: boolean;
}

export function canvasRefresh({
	documents,
	selectedId,
	opened,
	editing
}: CanvasState): CanvasRefresh {
	if (documents.length === 0) return { action: 'clear' };
	const selected = documents.find((document) => document.id === selectedId);
	// The selection is gone (deleted, or nothing selected yet) — fall back to
	// the first document the conversation has.
	if (!selected) return { action: 'open', documentId: documents[0].id };
	if (!opened || opened.id !== selected.id) return { action: 'open', documentId: selected.id };
	if (selected.current_ver === opened.knownHead) return { action: 'none' };
	// A newer version landed. Never clobber a half-typed hand edit with it:
	// tell the panel, and let the person decide when to drop their draft.
	if (editing) return { action: 'notify', documentId: selected.id, version: selected.current_ver };
	// Reading the head: follow it. Parked on an older revision on purpose:
	// stay there, but re-read anyway so the version list stops pretending the
	// document ends where it did when the panel opened it.
	return opened.shownVersion === opened.knownHead
		? { action: 'open', documentId: selected.id }
		: { action: 'open', documentId: selected.id, version: opened.shownVersion };
}
