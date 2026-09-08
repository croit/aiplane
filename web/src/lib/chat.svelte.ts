/**
 * The reactive chat controller (issue #22 phase 2): one instance per
 * conversation view. Owns the `ConversationState` from chat-protocol.ts
 * (the pure fold) plus the EventSource lifecycle around it.
 *
 * Stream lifecycle: open on mount, apply every event, and CLOSE on
 * `turn_finalized`/`idle` — the server ends the stream there, and letting
 * EventSource auto-reconnect would loop snapshot/idle forever on a quiet
 * session. After a submit (or any suspected change) `attach()` reopens:
 * the fresh snapshot is the replay, so nothing is lost between streams.
 *
 * Built as a factory (not a class): `$state` in a `.svelte.ts` closure is
 * the documented universal-reactivity pattern, and the returned object's
 * methods close over it directly.
 */
import { applyEvent, newConversationState, parseSseBlock, type ChatEvent } from './chat-protocol';
import { api } from './api';

export type SidebarChangedCallback = () => void;

export interface ConversationController {
	readonly id: string;
	readonly state: ReturnType<typeof newConversationState>;
	/** Set by the view; fired whenever the session list may have changed. */
	onSidebarChanged: SidebarChangedCallback | null;
	/** (Re)open the events stream. Safe to call repeatedly. */
	attach(): void;
	apply(event: ChatEvent): void;
	closeStream(): void;
	destroy(): void;
}

const EVENT_NAMES = [
	'snapshot',
	'turn_delta',
	'reasoning_delta',
	'tool_call_started',
	'tool_call_done',
	'turn_finalized',
	'sidebar_changed',
	'info',
	'tool_prompt',
	'idle'
] as const;

export function createConversationController(sessionId: string): ConversationController {
	// The state object is plain data (see ConversationState for why the
	// turns are an array) — `$state`'s deep proxy tracks every mutation
	// applyEvent makes to it.
	const state = $state(newConversationState());
	let source: EventSource | null = null;

	const controller: ConversationController = {
		id: sessionId,
		state,
		onSidebarChanged: null,

		attach() {
			closeCurrent();
			const es = new EventSource(api.chatEventsUrl(sessionId));
			source = es;
			es.onerror = () => {
				// A dropped connection mid-turn: close and re-attach once — the
				// snapshot replays whatever was missed. `idle`/finalized streams
				// are closed in `apply` before EventSource can retry them.
				if (!state.idle) {
					controller.attach();
				} else {
					closeCurrent();
				}
			};
			for (const name of EVENT_NAMES) {
				es.addEventListener(name, (ev) => {
					const event = parseSseBlock(`event: ${name}\ndata: ${(ev as MessageEvent).data}`);
					if (event) controller.apply(event);
				});
			}
		},

		apply(event) {
			applyEvent(state, event);
			if (event.type === 'sidebar_changed') controller.onSidebarChanged?.();
			if (event.type === 'turn_finalized' || event.type === 'idle') closeCurrent();
		},

		closeStream: closeCurrent,

		destroy() {
			closeCurrent();
		}
	};

	function closeCurrent() {
		source?.close();
		source = null;
	}

	return controller;
}
