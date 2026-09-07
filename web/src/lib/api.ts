/**
 * Minimal typed client for the gateway's session-authenticated API.
 *
 * Shapes mirror `crates/shared/src/api.rs` (the wire types the server
 * serialises). Once phase 1's OpenAPI schema lands this file is replaced
 * by a generated client — hand-written for now, deliberately tiny.
 *
 * All calls are same-origin (`/api/v0/*` reaches the gateway directly in
 * production, through the Vite proxy in dev). The session cookie is
 * sent automatically; a 401 means "signed out" and callers decide what
 * to do with it (the layout redirects to the OIDC login).
 */

export interface ToolSummary {
	id: string;
	name: string;
	description?: string;
}

/** Mirrors `shared::api::Me`. */
export interface Me {
	id: string;
	email: string;
	name: string | null;
	/** Raw OIDC claim values (e.g. groups). */
	roles: string[];
	/** RBAC role IDs after `[rbac.mapping]`. */
	role_ids: string[];
	allowed_tools: ToolSummary[];
}

/** Mirrors `shared::api::TokenSummary`. Timestamps are jiff RFC 3339 strings. */
export interface TokenSummary {
	id: string;
	name: string;
	created_at: string;
	last_used_at: string | null;
	expires_at: string;
	revoked: boolean;
	/** Master tool switch; false (default) means pure passthrough. */
	tools_enabled: boolean;
	/** Toggle keys disabled for this token; unlisted capabilities are on. */
	disabled_tools: string[];
}

export interface CreateTokenRequest {
	name: string;
	ttl_days?: number | null;
	tools_enabled?: boolean | null;
	disabled_tools?: string[];
}

/** The token summary plus its plaintext secret, shown exactly once. */
export interface CreateTokenResponse {
	token: TokenSummary;
	plaintext: string;
}

export interface RevokeResponse {
	/** false if already revoked / missing / not the caller's. */
	revoked: boolean;
}

export interface DeleteResponse {
	/** false if missing / not the caller's / still active. */
	deleted: boolean;
}

export interface UpdateTokenToolsRequest {
	tools_enabled: boolean;
	disabled_tools: string[];
}

export class ApiError extends Error {
	readonly status: number;
	constructor(status: number, message: string) {
		super(message);
		this.status = status;
	}
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
	const res = await fetch(path, {
		credentials: 'same-origin',
		...init,
		headers: { accept: 'application/json', ...init?.headers }
	});
	if (!res.ok) {
		// The gateway answers auth failures with an OpenAI-style envelope;
		// surface status so callers can distinguish 401 (sign in) from 5xx.
		const detail = await res.text().catch(() => '');
		throw new ApiError(res.status, `${res.status} ${res.statusText}${detail ? ` — ${detail}` : ''}`);
	}
	return (await res.json()) as T;
}

export const api = {
	/** GET /api/v0/me — identity + role grants; 401 when signed out. */
	me: () => request<Me>('/api/v0/me'),

	/** POST /auth/logout — clear the session (form-style endpoint). */
	logout: async () => {
		await fetch('/auth/logout', {
			method: 'POST',
			credentials: 'same-origin'
		});
	},

	// --- Tokens (gateway bearer secrets for the /v1 surface) --------------

	/** GET /api/v0/tokens — the caller's tokens (revoked included). */
	listTokens: () => request<TokenSummary[]>('/api/v0/tokens'),

	/** POST /api/v0/tokens — mint a token; the plaintext is shown once. */
	createToken: (body: CreateTokenRequest) =>
		request<CreateTokenResponse>('/api/v0/tokens', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify(body)
		}),

	/** POST /api/v0/tokens/{id}/revoke — revoke an active token. */
	revokeToken: (id: string) =>
		request<RevokeResponse>(`/api/v0/tokens/${encodeURIComponent(id)}/revoke`, { method: 'POST' }),

	/** POST /api/v0/tokens/{id}/rotate — re-mint the secret (shown once). */
	rotateToken: (id: string) =>
		request<CreateTokenResponse>(`/api/v0/tokens/${encodeURIComponent(id)}/rotate`, { method: 'POST' }),

	/** DELETE /api/v0/tokens/{id} — hard-delete a revoked token. */
	deleteToken: (id: string) =>
		request<DeleteResponse>(`/api/v0/tokens/${encodeURIComponent(id)}`, { method: 'DELETE' }),

	/** PUT /api/v0/tokens/{id}/tools — replace the token's tool config. */
	updateTokenTools: (id: string, body: UpdateTokenToolsRequest) =>
		request<{ ok: boolean; tools_enabled: boolean; disabled_tools: string[] }>(
			`/api/v0/tokens/${encodeURIComponent(id)}/tools`,
			{ method: 'PUT', headers: { 'content-type': 'application/json' }, body: JSON.stringify(body) }
		)
};

/**
 * Where to bounce the browser for sign-in. `return_to` is the OIDC
 * callback's same-origin redirect target (validated server-side by
 * `is_safe_return_to`); without it the callback lands on `/`, i.e. the
 * legacy page, not the SPA.
 */
export function loginUrl(returnTo: string): string {
	return `/auth/login?return_to=${encodeURIComponent(returnTo)}`;
}
