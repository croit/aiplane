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
	/**
	 * Optional features the operator has switched on at `/admin/settings`,
	 * named after their sections (`comfyui`, `rag`, `skills`, …). Absent from
	 * an older gateway, which `featureEnabled` reads as "not gating".
	 */
	features?: string[];
}

/** A canvas document belonging to a conversation. */
export interface CanvasDocument {
	id: string;
	title: string;
	format: string;
	current_ver: number;
	created_at: string;
	updated_at: string;
}

export interface ChatCapability {
	key: string;
	kind: 'tool' | 'skill';
	title: string;
	description: string;
	group: string;
	order: number;
	state: 'off' | 'auto' | 'on';
	can_disable: boolean;
	icon: string | null;
}

export interface ChatAsset {
	id: string;
	turn_id: string;
	filename: string;
	mime: string;
	size: number;
	url: string;
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
	if (res.status === 204) return undefined as T;
	return (await res.json()) as T;
}

export const api = {
	// --- Chat (issue #22 phase 2; wire shapes in chat-protocol.ts) --------

	/** GET /api/v0/chat/sessions — the sidebar list (pinned first). */
	listChatSessions: () =>
		request<{ sessions: import('./chat-protocol.js').ChatSession[] }>('/api/v0/chat/sessions'),

	/** GET /api/v0/chat/landing — latest conversation, or the caller's first. */
	chatLanding: () =>
		request<{ session: import('./chat-protocol.js').ChatSession }>('/api/v0/chat/landing'),

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
			compacted_up_to_seq: number | null;
			assets: ChatAsset[];
		}>(`/api/v0/chat/sessions/${encodeURIComponent(id)}`),

	/** DELETE /api/v0/chat/sessions/{id} — owner-only. */
	deleteChatSession: async (id: string) => {
		const res = await fetch(`/api/v0/chat/sessions/${encodeURIComponent(id)}`, {
			method: 'DELETE',
			credentials: 'same-origin'
		});
		// Checked like every other call: a swallowed failure here means the
		// row is still there while the UI has already dropped it from the list.
		if (!res.ok) throw new ApiError(res.status, `${res.status} ${res.statusText}`);
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

	/**
	 * POST /api/v0/chat/sessions/{id}/fork — copy a conversation shared with
	 * you into your own chats. Refused (409) for one you already own.
	 */
	forkChatSession: (id: string) =>
		request<{ id: string; title: string | null }>(
			`/api/v0/chat/sessions/${encodeURIComponent(id)}/fork`,
			{ method: 'POST' }
		),

	/** GET /api/v0/chat/sessions/{id}/documents — the canvas documents. */
	listChatDocuments: (id: string) =>
		request<{ documents: CanvasDocument[] }>(
			`/api/v0/chat/sessions/${encodeURIComponent(id)}/documents`
		),

	/** GET /api/v0/chat/sessions/{id}/documents/{docId} — content + history. */
	getChatDocument: (id: string, docId: string, version?: number) =>
		request<{
			document: CanvasDocument;
			version: { version: number; content: string; summary: string; author: string; created_at: string };
			history: { version: number; summary: string; created_at: string; chars: number; author: string }[];
		}>(
			`/api/v0/chat/sessions/${encodeURIComponent(id)}/documents/${encodeURIComponent(docId)}${version === undefined ? '' : `?version=${version}`}`
		),

	/** PUT /api/v0/chat/sessions/{id}/documents/{docId} — save a hand edit. */
	editChatDocument: (id: string, docId: string, content: string) =>
		request<{ document: CanvasDocument; unchanged: boolean }>(
			`/api/v0/chat/sessions/${encodeURIComponent(id)}/documents/${encodeURIComponent(docId)}`,
			{
				method: 'PUT',
				headers: { 'content-type': 'application/json' },
				body: JSON.stringify({ content })
			}
		),

	/**
	 * DELETE …/turns/{turnId}/attachments/{filename} — drop one attachment.
	 *
	 * The filename is encoded but its case is preserved end to end: the server
	 * matches the marker and the stored object verbatim.
	 */
	removeChatAttachment: (id: string, turnId: string, filename: string) =>
		request<{ removed: string }>(
			`/api/v0/chat/sessions/${encodeURIComponent(id)}/turns/${encodeURIComponent(turnId)}` +
				`/attachments/${encodeURIComponent(filename)}`,
			{ method: 'DELETE' }
		),

	/** GET /api/v0/models — the caller's chat models (compliance flags included). */
	listChatModels: () =>
		request<{ models: { id: string; gdpr: boolean; nda: boolean; reasoning: boolean }[] }>('/api/v0/models'),

	/** Voice input and spoken-reply choices available to this user. */
	chatVoiceConfig: () =>
		request<{
			data: string[];
			speech_available: boolean;
			speech_voices: string[];
			speech_voice: string | null;
		}>('/api/v0/transcription_models'),

	/** Persist the voice used for spoken replies; empty restores the pool default. */
	setSpeechVoice: (voice: string) =>
		request<{ ok: boolean; voice: string | null }>('/api/v0/me/speech_voice', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ voice })
		}),

	/** GET /api/v0/chat/sessions/{id}/capabilities — the tool overlay. */
	listChatCapabilities: (id: string) =>
		request<{ tools: ChatCapability[] }>(
			`/api/v0/chat/sessions/${encodeURIComponent(id)}/capabilities`
		),

	/**
	 * POST /api/v0/chat/sessions/{id}/capabilities — set one overlay state.
	 *
	 * Three states, not two. `'on'` pins the tool for this conversation and
	 * `'off'` blocks it for the rest of the conversation — `enable_tools` and
	 * the driver both refuse a blocked key. `'auto'` (no override) is what a
	 * composer checkbox means when it is unticked: "not pinned", not "banned".
	 */
	setChatCapability: (id: string, kind: 'tool' | 'skill', key: string, state: 'on' | 'auto' | 'off') =>
		request<{ kind: string; key: string; state: string }>(
			`/api/v0/chat/sessions/${encodeURIComponent(id)}/capabilities`,
			{
				method: 'POST',
				headers: { 'content-type': 'application/json' },
				body: JSON.stringify({ kind, key, state })
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
			tools: { key: string; title: string; tech: string; description: string; category: string; enabled: boolean }[];
			location: { shared: boolean; accuracy: number | null } | null;
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
		const res = await fetch('/auth/logout', {
			method: 'POST',
			credentials: 'same-origin'
		});
		// A sign-out that failed server-side leaves the session cookie live.
		// Telling the user they are signed out when they are not is the one
		// outcome this must never produce silently.
		if (!res.ok) throw new ApiError(res.status, `${res.status} ${res.statusText}`);
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
		),

	/** POST /api/v0/me/browser/feedback/{turn_id} — what the paired browser
	 * extension did with a `browser_control` batch. See browser-bridge.ts. */
	browserFeedback: (turnId: string, body: import('./chat-protocol.js').BrowserFeedbackBody) =>
		request<{ ok: boolean }>(`/api/v0/me/browser/feedback/${encodeURIComponent(turnId)}`, {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify(body)
		})
};
