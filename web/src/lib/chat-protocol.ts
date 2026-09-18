/**
 * The chat event protocol, client side (issue #22 phase 2).
 *
 * Mirrors `session_core::chat_json` on the wire: one SSE `event:` name per
 * `ChatEvent` variant, one JSON `data:` line carrying `{type, …}`. This
 * module is deliberately framework-free — no Svelte, no DOM — so the
 * state-machine below is unit-testable under `node --test` and the `.svelte.ts`
 * store stays a thin reactive wrapper.
 *
 * Client contract for text buffers: `turn_delta`/`reasoning_delta` append,
 * unless they carry `full: true` — the row was rewritten, the server reset
 * its cursor, and `text_delta` is the WHOLE text: replace, don't append.
 * No delete events exist by design.
 */

export interface ToolCall {
	id: string;
	turn_id: string;
	seq: number;
	name: string;
	arguments_json: string;
	output_json: string | null;
	status: 'running' | 'completed' | 'errored';
	created_at: string;
	completed_at: string | null;
}

export interface Turn {
	id: string;
	session_id: string;
	seq: number;
	role: 'user' | 'assistant';
	user_content: string | null;
	model: string | null;
	content: string | null;
	reasoning: string | null;
	reasoning_elapsed_ms: number | null;
	reasoning_started_at: string | null;
	status: 'in_progress' | 'completed' | 'cancelled' | 'errored';
	error_message: string | null;
	created_at: string;
	completed_at: string | null;
}

/** What became of a mid-turn interjection. Mirrors `SteerStatus` on the server. */
export type SteerStatus = 'pending' | 'delivered' | 'resent' | 'discarded';

/**
 * Something the user typed while this turn was already running.
 *
 * `status` is the honest part: `delivered` means the model was handed it
 * mid-turn, `resent` means the turn ended first and it was submitted as an
 * ordinary message, `pending` means neither has happened yet.
 */
export interface TurnSteer {
	id: string;
	turn_id: string;
	seq: number;
	text: string;
	status: SteerStatus;
	created_at: string;
	settled_at: string | null;
}

export interface TurnWithTools {
	turn: Turn;
	tool_calls: ToolCall[];
	steers: TurnSteer[];
}

export interface ChatSession {
	id: string;
	user_id: string;
	title: string | null;
	created_at: string;
	updated_at: string;
	shared: boolean;
	pinned: boolean;
}

export type ChatEvent =
	| { type: 'snapshot'; live_turn_id?: string; turns: TurnWithTools[] }
	| { type: 'turn_delta'; turn_id: string; text_delta: string; full?: boolean }
	| { type: 'reasoning_delta'; turn_id: string; text_delta: string; full?: boolean }
	| {
			type: 'tool_call_started';
			turn_id: string;
			tool_call_id: string;
			name: string;
			arguments: string;
	  }
	| {
			type: 'tool_call_done';
			turn_id: string;
			tool_call_id: string;
			status: string;
			output?: string;
	  }
	| {
			type: 'turn_finalized';
			turn_id: string;
			status: string;
			error_message?: string;
			model?: string;
			duration_ms?: number;
	  }
	| { type: 'steer'; turn_id: string; id: string; text: string; status: SteerStatus }
	| { type: 'sidebar_changed' }
	| { type: 'info'; message: string }
	| {
			type: 'tool_prompt';
			action: 'show';
			turn_id: string;
			kind: 'ask_user' | 'location';
			question: string;
			options: PromptOption[];
			/** Optional heading naming what is being decided. */
			header?: string;
			/** Whether more than one option may be picked. */
			multi_select?: boolean;
	  }
	| { type: 'tool_prompt'; action: 'hide'; turn_id: string }
	| { type: 'idle' };

/**
 * One answer a `tool_prompt` offers.
 *
 * `label` is the answer — it is what the button says and what goes back to the
 * model. `description` is for the person choosing and never travels back.
 */
export interface PromptOption {
	label: string;
	description?: string;
	preview?: PromptPreview;
}

/**
 * A worked example attached to an option.
 *
 * The kind is carried, never sniffed: `text` renders verbatim in a monospace
 * block and `svg` goes through a strict sanitiser first, and which of those
 * the client does must not be decided by the content itself.
 */
export interface PromptPreview {
	kind: 'text' | 'svg';
	content: string;
}

/** A turn as the UI holds it: the wire shape plus live-streamed buffers. */
export interface LiveTurn {
	turn: Turn;
	tool_calls: ToolCall[];
	steers: TurnSteer[];
}

/**
 * The mutable conversation state `applyEvent` folds events into.
 *
 * `turns` is a plain array, not a Map, on purpose: the reactive wrapper
 * holds this whole object in Svelte's `$state`, whose deep proxy tracks
 * arrays and nested objects — while a `Map` would need `SvelteMap`, whose
 * *values* pass through unproxied (mutations to `turn.content` would
 * silently never re-render). Conversation sizes make the id scan free.
 */
export interface ConversationState {
	/** Turns in arrival order (snapshots arrive in seq order). */
	turns: LiveTurn[];
	/** The turn currently streaming, when one is (also the worker's id). */
	liveTurnId: string | null;
	/** Set once `idle` or `turn_finalized` says nothing more will arrive. */
	idle: boolean;
	/** Transient banner text from `info` events. */
	info: string | null;
	/** The human-in-loop prompt, when one is showing. */
	prompt: Extract<ChatEvent, { type: 'tool_prompt' }> | null;
}

export function newConversationState(): ConversationState {
	return { turns: [], liveTurnId: null, idle: false, info: null, prompt: null };
}

function ensureTurn(state: ConversationState, id: string): LiveTurn {
	let existing = state.turns.find((t) => t.turn.id === id);
	if (!existing) {
		// An event for a turn the snapshot didn't carry (a turn that started
		// after attach — the 202 gave the SPA the ids, but the stream's own
		// snapshot was read before the rows landed). Materialise a stub; the
		// server never references a turn it didn't announce.
		existing = {
			turn: {
				id,
				session_id: '',
				seq: -1,
				role: 'assistant',
				user_content: null,
				model: null,
				content: '',
				reasoning: '',
				reasoning_started_at: null,
				reasoning_elapsed_ms: null,
				status: 'in_progress',
				error_message: null,
				created_at: '',
				completed_at: null
			},
			tool_calls: [],
			steers: []
		};
		state.turns.push(existing);
	}
	return existing;
}

function append(buffer: string | null, delta: string, full?: boolean): string {
	return full ? delta : (buffer ?? '') + delta;
}

/**
 * Fold one event into `state`. Mutates — the reactive wrapper owns
 * change notification; keeping this allocation-free keeps streaming cheap.
 *
 * Returns the event's effect so callers can react (refetch the session
 * list on `sidebar_changed`, close the EventSource on `idle`/finalized).
 */
export function applyEvent(state: ConversationState, event: ChatEvent): void {
	switch (event.type) {
		case 'snapshot': {
			state.turns = event.turns.map((row) => ({
				turn: row.turn,
				tool_calls: [...row.tool_calls],
				steers: [...row.steers]
			}));
			state.liveTurnId = event.live_turn_id ?? null;
			state.idle = !event.live_turn_id;
			return;
		}
		case 'turn_delta': {
			const live = ensureTurn(state, event.turn_id);
			live.turn.content = append(live.turn.content, event.text_delta, event.full);
			return;
		}
		case 'reasoning_delta': {
			const live = ensureTurn(state, event.turn_id);
			live.turn.reasoning = append(live.turn.reasoning, event.text_delta, event.full);
			return;
		}
		case 'tool_call_started': {
			const live = ensureTurn(state, event.turn_id);
			if (!live.tool_calls.some((c) => c.id === event.tool_call_id)) {
				live.tool_calls.push({
					id: event.tool_call_id,
					turn_id: event.turn_id,
					seq: live.tool_calls.length,
					name: event.name,
					arguments_json: event.arguments,
					output_json: null,
					status: 'running',
					created_at: '',
					completed_at: null
				});
			}
			return;
		}
		case 'tool_call_done': {
			const live = ensureTurn(state, event.turn_id);
			const call = live.tool_calls.find((c) => c.id === event.tool_call_id);
			if (call) {
				call.status = event.status as ToolCall['status'];
				call.output_json = event.output ?? call.output_json;
			}
			return;
		}
		case 'turn_finalized': {
			const live = ensureTurn(state, event.turn_id);
			live.turn.status = event.status as Turn['status'];
			live.turn.error_message = event.error_message ?? live.turn.error_message;
			if (event.model) live.turn.model = event.model;
			live.turn.completed_at = new Date().toISOString();
			if (state.liveTurnId === event.turn_id) {
				state.liveTurnId = null;
				state.idle = true;
			}
			return;
		}
		case 'steer': {
			// One event covers both "typed" and "settled" — merge on id so a
			// client that attached late, and first learns of a note when it is
			// already `resent`, ends up with the same state as one that
			// watched it from `pending`.
			const live = ensureTurn(state, event.turn_id);
			const existing = live.steers.find((s) => s.id === event.id);
			if (existing) {
				existing.status = event.status;
				existing.text = event.text;
				return;
			}
			live.steers.push({
				id: event.id,
				turn_id: event.turn_id,
				seq: live.steers.length,
				text: event.text,
				status: event.status,
				created_at: new Date().toISOString(),
				settled_at: null
			});
			return;
		}
		case 'sidebar_changed':
			return;
		case 'info':
			state.info = event.message;
			return;
		case 'tool_prompt':
			state.prompt = event.action === 'show' ? event : null;
			return;
		case 'idle':
			state.liveTurnId = null;
			state.idle = true;
			return;
	}
}

/** Parse one SSE block (`event:` + `data:` lines) into a `ChatEvent`. */
export function parseSseBlock(block: string): ChatEvent | null {
	const eventLine = block.split('\n').find((l) => l.startsWith('event: '));
	const dataLine = block.split('\n').find((l) => l.startsWith('data: '));
	if (!eventLine || !dataLine) return null;
	return JSON.parse(dataLine.slice('data: '.length)) as ChatEvent;
}

/** The sidebar's display title: the stored one, else the first user message. */
export function sessionTitle(session: ChatSession, turns: Iterable<LiveTurn>): string {
	if (session.title?.trim()) return session.title;
	for (const t of turns) {
		if (t.turn.role === 'user' && t.turn.user_content) {
			const first = t.turn.user_content.split('\n').find((l) => l.trim()) ?? '';
			return first.length > 60 ? `${first.slice(0, 60)}…` : first || 'Untitled chat';
		}
	}
	return 'Untitled chat';
}

// ---------------------------------------------------------------------------
// Chat attachments (user turns carry `[gw-attachment …]` markers inline)

export interface ChatAttachment {
	filename: string;
	mime: string;
	url: string;
	size: number;
	link?: string;
}

const MARKER_RE =
	/\[gw-attachment file="([^"]*)" mime="([^"]*)" url="([^"]*)" size=(\d+)(?: link="([^"]*)")?\]/g;

/** Split a user message into plain text + parsed attachment markers. */
export function parseUserContent(
	content: string | null
): { text: string; attachments: ChatAttachment[] } {
	if (!content) return { text: '', attachments: [] };
	const attachments: ChatAttachment[] = [];
	const text = content
		.replace(MARKER_RE, (_m, file: string, mime: string, url: string, size: string, link?: string) => {
			attachments.push({
				filename: file,
				mime,
				url,
				size: Number(size),
				link: link || undefined
			});
			return '';
		})
		.replace(/\n{3,}/g, '\n\n')
		.trim();
	return { text, attachments };
}

export function replaceUserText(content: string, text: string): string {
	const markers = [...content.matchAll(MARKER_RE)].map((match) => match[0]);
	return [text.trim(), ...markers].filter(Boolean).join('\n\n');
}
