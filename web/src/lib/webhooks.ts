import type { ChatModelOption } from './model-option';
import type { OwnedAgent } from './agent-state.ts';
import type { AgentBinding } from './agent-background.ts';
import type { AgentVersion } from './agents.ts';
import { adminDelete, adminJson, adminPost, adminPut } from './admin-client.ts';
import { request } from './api.ts';

export interface Webhook extends AgentBinding {
	id: string;
	name: string;
	prompt: string;
	model: string | null;
	principal_id: string | null;
	agent_version: number | null;
	configuration_generation: string;
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
	agent_version: number | null;
	configuration_generation: string | null;
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
	agents: OwnedAgent[];
	models: ChatModelOption[];
	principal_id: string | null;
	can_write: boolean;
	versions: AgentVersion[];
}

export interface WebhookBody {
	name: string;
	prompt: string;
	model?: string;
	personal_agent_id?: string | null;
	principal_id?: string;
	agent_version?: number;
	tools_enabled: boolean;
	synchronous: boolean;
	reuse_conversation: boolean;
	reuse_rounds: number;
	linked_session_id?: string | null;
}

export const webhooksApi = {
	list: (principalId?: string | null) => adminJson<WebhooksData>(`/api/v0/webhooks${principalId ? `?principal_id=${encodeURIComponent(principalId)}` : ''}`),
	create: (body: WebhookBody) => adminPost<{ webhook: Webhook; secret: string }>('/api/v0/webhooks', body),
	update: (id: string, body: WebhookBody & { expected_configuration_generation: string }) => adminPut(`/api/v0/webhooks/${encodeURIComponent(id)}`, body),
	toggle: (id: string, enabled: boolean, generation: string) => adminPost<{ enabled: boolean; configuration_generation: string }>(`/api/v0/webhooks/${encodeURIComponent(id)}/toggle`, { enabled, expected_configuration_generation: generation }),
	rotate: (id: string, generation: string) => adminPost<{ secret: string }>(`/api/v0/webhooks/${encodeURIComponent(id)}/rotate`, { expected_configuration_generation: generation }),
	remove: (id: string) => adminDelete(`/api/v0/webhooks/${encodeURIComponent(id)}`),
	runs: (id: string) => adminJson<{ runs: WebhookRun[] }>(`/api/v0/webhooks/${encodeURIComponent(id)}/runs`),
	rerun: (id: string, body: { prompt: string; run?: string; expected_configuration_generation: string }) => request<{ session_id: string }>(`/api/v0/webhooks/${encodeURIComponent(id)}/rerun`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(body) })
};

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
