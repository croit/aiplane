/**
 * What a paused turn waits for, as one card shows it wherever it is answered:
 * the inbox, the agent builder's test chat and a person's own chat
 * (`docs/ui.md` → "Suspension card"). The wire shapes and the pure rules the
 * card follows: which decisions it offers, how a slot is named, how a value is
 * typed.
 */
import { slotTitle } from './agent-setup.ts';

export type SuspensionKind = 'approval' | 'secure_input' | 'human_answer' | 'browser_handover';
export type DecisionKind = 'allow_once' | 'deny' | 'value';

/** A slot of a hand-off's context: its value, or who vouched for it when the model may not see it. */
export interface HandoffSlot {
	slot: string;
	/** The description its manager gave the slot, when there is one. */
	label?: string;
	value?: unknown;
	set_by?: string;
}

/** What a hand-off to a person carries next to the question (`agents::human`). */
export interface HandoffContext {
	route?: string;
	question?: string;
	visitor_message?: string | null;
	slots?: HandoffSlot[];
	lang?: string;
	inbox?: string | null;
	notify?: string[] | null;
	transcript?: { role: 'user' | 'assistant'; text: string }[];
}

/** A turn's suspension as the `suspended` frame and a snapshot's turn carry it. */
export interface SuspensionView {
	request_id: string;
	kind: SuspensionKind;
	message?: string;
	tool_call_id?: string;
	tool?: string;
	options: DecisionKind[];
	expires_at: string;
}

/** What one card shows: the request, and what is known around it. */
export interface Waiting {
	kind: SuspensionKind;
	/** A hand-off's question. */
	question?: string | null;
	/** What an approved call would do, in the tool's own words (`schedule_action`'s preview). */
	detail?: string | null;
	/** An approval's call, with its raw arguments. */
	call?: { name: string; arguments: string } | null;
	context?: HandoffContext | null;
	/** The decisions the viewer may send, as the server offered them. */
	options: DecisionKind[];
	expires_at: string;
}

export interface Answer {
	decision: DecisionKind;
	value?: string;
}

/** A card from a suspension view, with the call it waits on when the transcript has it. */
export function waitingFrom(
	view: SuspensionView,
	calls: { id: string; name: string; arguments_json: string }[] = [],
	context: HandoffContext | null = null
): Waiting {
	const call = calls.find((c) => c.id === view.tool_call_id);
	return {
		kind: view.kind,
		question: view.kind === 'human_answer' ? (view.message ?? null) : null,
		detail: view.kind === 'human_answer' ? null : (view.message ?? null),
		call: call ? { name: call.name, arguments: call.arguments_json } : view.tool ? { name: view.tool, arguments: '' } : null,
		context,
		options: view.options,
		expires_at: view.expires_at
	};
}

/**
 * The Fluent key of the line that leads a card: in a person's own chat, what
 * their conversation waits for; in the test chat, what the agent waits for
 * and from whom.
 */
export function waitingLead(kind: SuspensionKind, where: 'chat' | 'test'): string {
	const keys: Record<SuspensionKind, [string, string]> = {
		approval: ['chat-waiting-approval', 'agents-test-waiting-approval'],
		secure_input: ['chat-waiting-value', 'agents-test-waiting-secure-input'],
		human_answer: ['chat-waiting-answer', 'agents-test-waiting-human'],
		browser_handover: ['chat-waiting-browser-handover', 'agents-test-waiting-approval']
	};
	return keys[kind][where === 'chat' ? 0 : 1];
}

/**
 * The turn a conversation waits on for a decision, if any. Until it is
 * answered the conversation takes no new message: one sent now would run
 * before the paused call, in a context the decision was not asked about.
 */
export function pausedTurn(turns: { turn: { id: string; status: string } }[]): string | null {
	return turns.find((live) => live.turn.status === 'suspended')?.turn.id ?? null;
}

/** The buttons a card offers, exactly the decisions it was offered; `value` is the answer form's own button. */
export function decisionButtons(waiting: Pick<Waiting, 'kind' | 'options'>): { decision: DecisionKind; key: string; primary: boolean }[] {
	const buttons: { decision: DecisionKind; key: string; primary: boolean }[] = [];
	if (waiting.options.includes('allow_once')) buttons.push({ decision: 'allow_once', key: 'suspension-approve', primary: true });
	if (waiting.options.includes('deny')) {
		buttons.push({ decision: 'deny', key: waiting.kind === 'approval' ? 'suspension-deny' : 'suspension-decline', primary: false });
	}
	return buttons;
}

/**
 * How a `value` is typed: a code the visitor would type is masked, a staff
 * member's answer to a hand-off is plain text they read back.
 */
export function answerField(kind: SuspensionKind): { secret: boolean; label: string; submit: string } {
	return kind === 'human_answer'
		? { secret: false, label: 'suspension-answer-label', submit: 'suspension-send-answer' }
		: { secret: true, label: 'suspension-value-label', submit: 'suspension-send-value' };
}

/** A slot's value for display: strings as they are, everything else as JSON. */
export function slotText(slot: HandoffSlot): string | null {
	if (slot.value === undefined) return null;
	return typeof slot.value === 'string' ? slot.value : JSON.stringify(slot.value);
}

/** A slot as the card names it and shows it: by its label, a value the model may not see by who vouched for it. */
export function slotLine(slot: HandoffSlot, tr: (key: string, args?: Record<string, string | number>) => string): { label: string; value: string } {
	return {
		label: slotTitle(slot.slot, slot.label, tr),
		value: slotText(slot) ?? tr('suspension-slot-trusted', { by: slot.set_by ?? '' })
	};
}

/** An approval's arguments, pretty-printed when they are JSON. */
export function prettyArguments(raw: string): string {
	try {
		return JSON.stringify(JSON.parse(raw), null, 2);
	} catch {
		return raw;
	}
}

/** Minutes until the deadline, never negative; `null` for an unreadable date. */
export function minutesLeft(expiresAt: string, now = Date.now()): number | null {
	const at = Date.parse(expiresAt);
	if (Number.isNaN(at)) return null;
	return Math.max(0, Math.ceil((at - now) / 60000));
}

/**
 * Send `decision` with the typed `text` through `send`, which resolves
 * whether it went out. What the field holds afterwards: cleared once sent,
 * kept when sending failed or there was nothing to send (`missing`).
 */
export async function submitAnswer(
	decision: DecisionKind,
	text: string,
	send: (answer: Answer) => Promise<boolean>
): Promise<{ text: string; missing: boolean }> {
	const answer = answerFor(decision, text);
	if (!answer) return { text, missing: true };
	return { text: (await send(answer)) ? '' : text, missing: false };
}

/** The answer a decision sends; `null` when a `value` answer has no text. */
export function answerFor(decision: DecisionKind, text: string): Answer | null {
	if (decision !== 'value') return { decision };
	const value = text.trim();
	return value ? { decision, value } : null;
}
