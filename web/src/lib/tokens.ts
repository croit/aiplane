import type { ChatCapability } from './api';

export interface TokenQuota {
	id: string;
	model: string | null;
	dimension: 'requests' | 'tokens' | 'cost';
	window: 'hour' | 'day' | 'week' | 'month';
	value: number;
	managed_by: 'owner' | 'admin';
}

export interface TokenUsage {
	requests: number;
	tokens: number;
	cost: number;
}

export interface ManagedToken {
	id: string;
	name: string;
	created_at: string;
	last_used_at: string | null;
	expires_at: string;
	revoked: boolean;
	tools_enabled: boolean;
	tool_states: Record<string, 'on' | 'auto' | 'off'>;
	owner_models: string[] | null;
	admin_models: string[] | null;
	mcp_allow: boolean;
	quotas: TokenQuota[];
	usage: TokenUsage | null;
}

export interface TokenManagementDetails {
	tokens: ManagedToken[];
	capabilities: Omit<ChatCapability, 'state'>[];
	models: string[];
	usage_enabled: boolean;
	currency: string;
	push_enabled: boolean;
	timezone: string;
	account: {
		email: string;
		user_id: string;
		oidc_roles: string[];
		rbac_roles: string[];
	};
}

export function tokenDate(timestamp: string, timezone: string): string {
	const date = new Date(timestamp);
	if (Number.isNaN(date.valueOf())) return timestamp;
	const parts = new Intl.DateTimeFormat('en', {
		timeZone: timezone,
		year: 'numeric',
		month: '2-digit',
		day: '2-digit'
	}).formatToParts(date);
	const part = (type: Intl.DateTimeFormatPartTypes) => parts.find((entry) => entry.type === type)?.value ?? '';
	return `${part('year')}-${part('month')}-${part('day')}`;
}
