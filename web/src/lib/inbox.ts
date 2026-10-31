/**
 * The human-in-the-loop inbox (`docs/agent-hil.md`): wire
 * types, the calls, and the pure helpers the page leans on.
 *
 * An item is something a paused turn waits for: an approval of a tool call
 * or a visitor's request for a person, in an agent conversation, or a
 * person's own scheduled or webhook run that paused. Who sees it is the
 * server's decision; `standing` says why the viewer does, and decides which
 * links the page may offer — a responder sees the item and nothing of the
 * agent behind it.
 */
import { ApiError, request } from './api.ts';
import type { Answer, DecisionKind, HandoffContext, SuspensionKind } from './suspension.ts';

export type InboxKind = Exclude<SuspensionKind, 'secure_input'>;
export type Standing = 'manager' | 'responder' | 'owner';

export interface InboxItem {
	id: string;
	kind: InboxKind;
	standing: Standing;
	agent?: { id: string; name: string; display: string };
	session_id: string;
	turn_id: string;
	title?: string;
	question?: string;
	call?: { name: string; arguments: string };
	/** The round's other calls the same decision settles. */
	bundled?: { name: string; arguments: string }[];
	/** What an approved call would do, in the tool's own words. */
	detail?: string;
	context?: HandoffContext;
	options: DecisionKind[];
	created_at: string;
	expires_at: string;
}

/** The message and code of a failed inbox call, out of the gateway's envelope. */
export function errorMessage(err: unknown): { code?: string; message: string } {
	if (!(err instanceof ApiError)) return { message: err instanceof Error ? err.message : String(err) };
	return { code: err.code, message: err.serverMessage ?? err.message };
}

const post = (body: unknown): RequestInit => ({
	method: 'POST',
	headers: { 'content-type': 'application/json' },
	body: JSON.stringify(body)
});

export const INBOX_EVENTS = '/api/v0/agents/inbox/events';

export const inboxApi = {
	list: () => request<{ items: InboxItem[]; count: number; answers: boolean }>('/api/v0/agents/inbox').then((r) => r.items),
	answer: (id: string, answer: Answer) =>
		request<{ turn_id: string }>(`/api/v0/agents/inbox/${encodeURIComponent(id)}/answer`, post(answer))
};

/** What one `inbox` frame says: the waiting items and whether the viewer answers for a published agent. */
export interface InboxFrame {
	count: number;
	answers: boolean;
}

export function frameOf(data: string): InboxFrame | null {
	try {
		const parsed = JSON.parse(data);
		if (!parsed || parsed.type !== 'inbox' || !Number.isInteger(parsed.count) || parsed.count < 0) return null;
		return { count: parsed.count, answers: parsed.answers === true };
	} catch {
		return null;
	}
}

/** The sidebar shows the inbox only where something can arrive: an item waits, the viewer answers for an agent, or it is open. */
export function inboxShown(frame: InboxFrame, open: boolean): boolean {
	return frame.count > 0 || frame.answers || open;
}

/** Fluent key of an item's kind badge. */
export function kindLabel(kind: InboxKind): string {
	return kind === 'approval' ? 'inbox-kind-approval' : 'inbox-kind-handoff';
}

/** What the item's heading names: the agent, or the viewer's own run. */
export function itemHeading(item: InboxItem): { key: string; name: string } {
	if (item.agent) return { key: 'inbox-item-agent', name: item.agent.display || item.agent.name };
	return { key: 'inbox-item-run', name: item.title ?? '' };
}

/**
 * Where the item may link to. Only a manager may open the agent; a
 * responder gets nothing beyond the item. An owner opens their own chat.
 */
export function itemLink(item: InboxItem): { href: string; key: string } | null {
	if (item.standing === 'owner') return { href: `/chat/${item.session_id}`, key: 'inbox-open-chat' };
	if (item.standing === 'manager' && item.agent) return { href: `/agents/${item.agent.id}`, key: 'inbox-open-agent' };
	return null;
}

/** The item a notification link (`?item=…`) names, if it is still listed. */
export function focused(items: InboxItem[], search: Pick<URLSearchParams, 'get'>): string | null {
	const id = search.get('item');
	return id && items.some((i) => i.id === id) ? id : null;
}
