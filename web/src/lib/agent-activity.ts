/**
 * The pure half of the agent Activity tab: the shape of
 * `GET /api/v0/agents/{id}/activity` and `…/activity/verify`, the query a
 * view asks with, how a conversation's events fold into turns, and the one
 * line each event is summed up as. No framework imports, so `node --test`
 * covers it.
 */

export interface ActivityEvent {
	cursor: number;
	id: string;
	kind: string;
	ts: string;
	principal_id: string;
	actor_id: string | null;
	agent_id: string | null;
	version: number | null;
	conversation_id: string | null;
	session_id: string | null;
	turn_id: string | null;
	round: number | null;
	call_id: string | null;
	visitor_id: string | null;
	caller_id: string | null;
	duration_ms: number | null;
	run_chain: unknown;
	detail: Record<string, unknown>;
	chain_key: string | null;
	seq: number | null;
	prev_hash: string | null;
	hash: string | null;
}

export interface ActivityPage {
	events: ActivityEvent[];
	next_cursor: number | null;
	order: 'asc' | 'desc';
}

export interface Verification {
	ok: boolean;
	chains: number;
	events: number;
	checked: number;
	unanchored: number;
	head: { chain_key: string; seq: number; hash: string | null } | null;
	broken: { chain_key: string; seq: number; event_id: string | null; reason: string } | null;
}

/** The filter groups the tab offers; `all` filters nothing. */
export const KIND_GROUPS = {
	all: [],
	turns: ['turn_started', 'turn_finished', 'output_blocked'],
	exchanges: ['llm_exchange'],
	tools: ['tool_call', 'tool_result', 'injection_detected'],
	state: ['state_written', 'verifier_outcome', 'host_identity'],
	routing: ['scope_decision', 'route_decision', 'sub_agent_dispatched', 'sub_agent_finished', 'loop_iteration', 'loop_finished'],
	people: ['run_suspended', 'run_resumed', 'human_handoff', 'a2a_task', 'limit_refused'],
	management: [
		'agent_created',
		'agent_draft_updated',
		'agent_published',
		'agent_live_version_set',
		'agent_share_set',
		'agent_share_removed',
		'agent_deleted',
		'personal_dot_session_started',
		'assist_suggested',
		'principal_created',
		'principal_disabled',
		'grant_added',
		'grant_removed',
		'token_issued',
		'token_revoked',
		'embed_key_created',
		'embed_key_revoked',
		'channel_created',
		'channel_deleted',
		'conversations_swept',
		'activity_swept',
		'chain_anchored',
		'chain_checkpoint'
	]
} as const satisfies Record<string, readonly string[]>;

export type KindGroup = keyof typeof KIND_GROUPS;

export interface ActivityFilter {
	conversation: string;
	group: KindGroup;
	/** `YYYY-MM-DD`, UTC; empty for no bound. */
	from: string;
	to: string;
}

/** The query string for `filter`, continuing at `cursor` when given. */
export function activityQuery(filter: ActivityFilter, cursor: number | null = null): string {
	const params = new URLSearchParams();
	if (filter.conversation) {
		params.set('conversation', filter.conversation);
		params.set('order', 'asc');
	}
	const kinds = KIND_GROUPS[filter.group];
	if (kinds.length) params.set('kind', kinds.join(','));
	if (filter.from) params.set('from', filter.from);
	if (filter.to) params.set('to', filter.to);
	if (cursor !== null) params.set('cursor', String(cursor));
	return params.toString();
}

/** The export's query: the same filters, without paging or order. */
export function exportQuery(filter: ActivityFilter): string {
	const params = new URLSearchParams(activityQuery(filter));
	params.delete('order');
	return params.toString();
}

export interface TurnGroup {
	/** `null` for events outside any turn (the agent's own changes). */
	turn: string | null;
	events: ActivityEvent[];
}

/**
 * Consecutive events of one turn, in the order given. A sub-agent's run has
 * turns of its own, so it shows as its own group between the main turn's.
 */
export function groupByTurn(events: ActivityEvent[]): TurnGroup[] {
	const out: TurnGroup[] = [];
	for (const event of events) {
		const last = out.at(-1);
		if (last && last.turn === event.turn_id) last.events.push(event);
		else out.push({ turn: event.turn_id, events: [event] });
	}
	return out;
}

/** The conversations a page names, in the order they first appear. */
export function conversationsOf(events: ActivityEvent[]): string[] {
	const seen: string[] = [];
	for (const e of events) {
		if (e.conversation_id && !seen.includes(e.conversation_id)) seen.push(e.conversation_id);
	}
	return seen;
}

/** One line for an event: a catalog key and its arguments, or `null`. */
export interface Summary {
	key: string;
	args: Record<string, string | number>;
}

const str = (v: unknown): string => (typeof v === 'string' ? v : v === undefined || v === null ? '' : JSON.stringify(v));

const clip = (s: string, max = 120): string => (s.length > max ? `${s.slice(0, max - 1)}…` : s);

export function summarize(event: ActivityEvent): Summary | null {
	const d = event.detail ?? {};
	switch (event.kind) {
		case 'llm_exchange': {
			const response = (d.response ?? {}) as Record<string, unknown>;
			const usage = (response.usage ?? {}) as Record<string, unknown>;
			if (d.error) return { key: 'agents-act-sum-llm-error', args: { model: str(d.model), error: clip(str(d.error)) } };
			return {
				key: 'agents-act-sum-llm',
				args: {
					model: str(d.real_model ?? d.model),
					tokens: Number(usage.total_tokens ?? 0),
					finish: str(response.finish_reason ?? d.picked ?? '')
				}
			};
		}
		case 'tool_result':
			return { key: 'agents-act-sum-tool', args: { tool: str(d.tool), status: str(d.status) } };
		case 'tool_call':
			return { key: 'agents-act-sum-tool', args: { tool: str(d.tool), status: str(d.decision) } };
		case 'state_written':
			return { key: 'agents-act-sum-state', args: { slot: str(d.slot), provenance: str(d.provenance) } };
		case 'personal_dot_session_started':
			return { key: 'dot-activity-session-started', args: {} };
		case 'turn_started':
			return d.resumed
				? { key: 'agents-act-sum-resumed', args: {} }
				: { key: 'agents-act-sum-message', args: { text: clip(str(d.message)) } };
		case 'turn_finished':
			return { key: 'agents-act-sum-finished', args: { status: str(d.status), text: clip(str(d.answer ?? d.error)) } };
		case 'route_decision':
			return { key: 'agents-act-sum-route', args: { route: str(d.picked ?? d.reason) } };
		case 'sub_agent_dispatched':
		case 'sub_agent_finished':
			return { key: 'agents-act-sum-route', args: { route: str(d.route) } };
		default:
			return null;
	}
}
