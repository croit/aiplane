import type { ChatCapability } from './api';
import type { TokenModel } from './token-models';
import type { UsageLimit } from './usage-types';
import type { Priority } from './priority';

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
	priority: Priority | null;
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
	quota_status: UsageLimit[];
	usage: TokenUsage | null;
}

export interface TokenManagementDetails {
	owner_priority: Priority;
	max_priority: Priority;
	priority_enabled: boolean;
	tokens: ManagedToken[];
	capabilities: Omit<ChatCapability, 'state'>[];
	models: TokenModel[];
	owner_limits: UsageLimit[];
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

const DAY_MS = 24 * 60 * 60 * 1000;

/** Whole days left before the token expires (0 = today), `'expired'` once it
 *  has, or `null` while expiry is more than `warnDays` away — far enough that
 *  the card needs no warning. */
export function daysUntilExpiry(expiresAt: string, now: number, warnDays = 14): number | 'expired' | null {
	const left = Date.parse(expiresAt) - now;
	if (Number.isNaN(left) || left > warnDays * DAY_MS) return null;
	if (left <= 0) return 'expired';
	return Math.floor(left / DAY_MS);
}

export type NewQuota = Pick<TokenQuota, 'dimension' | 'window' | 'value'>;

/** What saving the budget dialog sends: the owner's rules it dropped and the
 *  rules it added. An admin's rule is never the owner's to remove. */
export function quotaChanges(saved: TokenQuota[], kept: string[], added: NewQuota[]): { remove: string[]; add: NewQuota[] } {
	return {
		remove: saved.filter((quota) => quota.managed_by === 'owner' && !kept.includes(quota.id)).map((quota) => quota.id),
		add: added
	};
}

/** The spend recorded against a rule, matched by what the rule limits. */
export function quotaUsed(quota: Pick<TokenQuota, 'model' | 'dimension' | 'window'>, status: UsageLimit[]): number | null {
	return status.find((entry) => entry.model === quota.model && entry.dimension === quota.dimension && entry.window === quota.window)?.used ?? null;
}
