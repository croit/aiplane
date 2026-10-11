import type { ChatModelOption } from './model-option';
import type { Dot } from './dot-state.ts';
import type { DotBinding } from './dot-background.ts';

export interface Webhook extends DotBinding {
	id: string;
	name: string;
	prompt: string;
	model: string;
	tools_enabled: boolean;
	synchronous: boolean;
	reuse_conversation: boolean;
	reuse_rounds: number;
	enabled: boolean;
	last_fired_at: string | null;
	last_status: string | null;
	last_session_id: string | null;
	/** That chat has since been deleted; list responses only. */
	last_chat_deleted?: boolean;
	last_error: string | null;
	has_payload: boolean;
	/** Recorded fires. */
	run_count: number;
	/** Distinct chats those fires opened — one, for a reusing webhook. */
	chat_count: number;
}

export interface WebhookRun {
	id: string;
	status: string | null;
	error: string | null;
	fired_at: string;
	source: string;
	session_id: string | null;
	assistant_turn_id: string | null;
	/** The run opened a chat the owner has since deleted. */
	chat_deleted: boolean;
	prompt: string;
	payload: string;
}

export interface WebhooksData {
	webhooks: Webhook[];
	dots: Dot[];
	models: ChatModelOption[];
}

function compactDate(value: string, locale: string, withSeconds: boolean): string {
	const date = new Date(value);
	if (Number.isNaN(date.getTime())) return '';
	const parts = new Intl.DateTimeFormat(locale, {
		month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit',
		second: withSeconds ? '2-digit' : undefined, hourCycle: 'h23', timeZone: 'UTC'
	}).formatToParts(date);
	const part = (type: Intl.DateTimeFormatPartTypes) => parts.find((candidate) => candidate.type === type)?.value ?? '';
	return `${part('month')} ${part('day')}, ${part('hour')}:${part('minute')}${withSeconds ? `:${part('second')}` : ''}`;
}

export const formatWebhookFire = (value: string, locale: string): string => compactDate(value, locale, false);
export const formatWebhookRun = (value: string, locale: string): string => compactDate(value, locale, true);
