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

/** The single same-origin JSON fetch path. Every client — tokens, chat,
 * admin surfaces — funnels through here so error envelopes, credentials and
 * headers live in exactly one place. */
export async function request<T>(path: string, init?: RequestInit): Promise<T> {
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
	// --- Chat (issue #22 phase 2; wire shapes in chat-protocol.ts) --------

	/** GET /api/v0/chat/sessions — the sidebar list (pinned first). */
	listChatSessions: () =>
		request<{ sessions: import('./chat-protocol.js').ChatSession[] }>('/api/v0/chat/sessions'),

	/** POST /api/v0/chat/sessions — mint an empty conversation. */
	createChatSession: () =>
		request<{ session: import('./chat-protocol.js').ChatSession }>('/api/v0/chat/sessions', {
			method: 'POST'
		}),

	/** GET /api/v0/chat/sessions/{id} — full snapshot (owner or shared). */
	getChatSession: (id: string) =>
		request<{
			session: import('./chat-protocol.js').ChatSession;
			turns: import('./chat-protocol.js').TurnWithTools[];
		}>(`/api/v0/chat/sessions/${encodeURIComponent(id)}`),

	/** DELETE /api/v0/chat/sessions/{id} — owner-only. */
	deleteChatSession: async (id: string) => {
		await fetch(`/api/v0/chat/sessions/${encodeURIComponent(id)}`, {
			method: 'DELETE',
			credentials: 'same-origin'
		});
	},

	/** POST /api/v0/chat/sessions/{id}/pin — explicit value, idempotent. */
	pinChatSession: (id: string, pinned: boolean) =>
		request<{ pinned: boolean }>(`/api/v0/chat/sessions/${encodeURIComponent(id)}/pin`, {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ pinned })
		}),

	/** POST /api/v0/chat/sessions/{id}/messages — 202, reply on the events stream. */
	sendChatMessage: (id: string, body: { model: string; message: string; voice?: boolean }) =>
		request<{ user_turn_id: string; assistant_turn_id: string }>(
			`/api/v0/chat/sessions/${encodeURIComponent(id)}/messages`,
			{
				method: 'POST',
				headers: { 'content-type': 'application/json' },
				body: JSON.stringify(body)
			}
		),

	/** POST /api/v0/chat/sessions/{id}/cancel — idempotent stop request. */
	cancelChatTurn: (id: string) =>
		request<{ cancelled: boolean }>(
			`/api/v0/chat/sessions/${encodeURIComponent(id)}/cancel`,
			{ method: 'POST' }
		),

	/** The events stream URL — for `new EventSource` (cookies ride along same-origin). */
	chatEventsUrl: (id: string) => `/api/v0/chat/sessions/${encodeURIComponent(id)}/events`,

	/** GET /api/v0/models — the caller's chat models (compliance flags included). */
	listChatModels: () =>
		request<{ models: { id: string; gdpr: boolean; nda: boolean }[] }>('/api/v0/models'),

	/** GET /api/v0/chat/sessions/{id}/capabilities — the tool overlay. */
	listChatCapabilities: (id: string) =>
		request<{ tools: { key: string; title: string; enabled: boolean }[] }>(
			`/api/v0/chat/sessions/${encodeURIComponent(id)}/capabilities`
		),

	/** POST /api/v0/chat/sessions/{id}/capabilities — set one overlay state. */
	setChatCapability: (id: string, toolKey: string, enabled: boolean) =>
		request<{ tool_key: string; enabled: boolean }>(
			`/api/v0/chat/sessions/${encodeURIComponent(id)}/capabilities`,
			{
				method: 'POST',
				headers: { 'content-type': 'application/json' },
				body: JSON.stringify({ tool_key: toolKey, enabled })
			}
		),

	/** POST /api/v0/chat/sessions/{id}/effort — reasoning effort knob. */
	setChatEffort: (id: string, effort: string) =>
		request<{ effort: string }>(`/api/v0/chat/sessions/${encodeURIComponent(id)}/effort`, {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ effort })
		}),

	/** GET /api/v0/tools — the caller's tool toggles. */
	listTools: () =>
		request<{
			tools: { key: string; title: string; description: string; category: string; enabled: boolean }[];
		}>('/api/v0/tools'),

	/** POST /api/v0/tools/toggle — set one tool's state (explicit, idempotent). */
	toggleTool: (toolKey: string, enabled: boolean) =>
		request<{ tool_key: string; enabled: boolean }>('/api/v0/tools/toggle', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ tool_key: toolKey, enabled })
		}),

	/** GET /api/v0/usage — the usage dashboard aggregates (issue #22 P3). */
	usage: (params: { period?: string; scope?: 'all' | 'self'; source?: string; backend?: string; token?: string } = {}) => {
		const qs = new URLSearchParams(
			Object.entries(params).filter(([, v]) => v !== undefined && v !== '') as [string, string][]
		);
		const suffix = qs.size > 0 ? `?${qs}` : '';
		return request<import('./usage-types.js').UsageResponse>(`/api/v0/usage${suffix}`);
	},

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
