export type AdminTokenState = 'active' | 'expired' | 'revoked';

export interface AdminTokenLimit {
	id: string;
	model: string | null;
	dimension: 'requests' | 'tokens' | 'cost';
	window: 'hour' | 'day' | 'week' | 'month';
	value: number;
}

export interface AdminToken {
	id: string;
	name: string;
	owner_id: string;
	owner_email: string;
	created_at: string;
	last_used_at: string | null;
	expires_at: string;
	revoked: boolean;
	owner_models: string[] | null;
	admin_models: string[] | null;
	limits: AdminTokenLimit[];
	usage_this_month: { requests: number; total_tokens: number; cost: number };
}

export function tokenState(token: { revoked: boolean; expires_at: string }, now = Date.now()): AdminTokenState {
	if (token.revoked) return 'revoked';
	return Date.parse(token.expires_at) < now ? 'expired' : 'active';
}

export function visibleTokenModels(ownerModels: string[] | null): string[] | null {
	return ownerModels === null ? null : [...ownerModels];
}
