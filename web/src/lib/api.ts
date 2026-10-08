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

/** The reasoning-effort scale, least thinking first. */
export const EFFORTS = ['off', 'low', 'medium', 'high', 'xhigh'] as const;
export type Effort = (typeof EFFORTS)[number];

/** One entry of `GET /api/v0/models`. */
export interface ChatModelChoice {
	id: string;
	gdpr: boolean;
	nda: boolean;
	/** The effort levels this model offers; empty when effort changes nothing. */
	efforts: Effort[];
}

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
	/** May build agents; the Agents section is shown only when true. */
	can_manage_agents?: boolean;
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

/**
 * One row of a capability list (`tool_toggles::CapabilityEntry`): the chat
 * picker and the agent setup build theirs the same way on the server, so a
 * resource reads alike in both. `description` is the resource's own, or empty.
 */
export interface CapabilityItem {
	key: string;
	kind: string;
	title: string;
	description: string;
	group: string;
	order: number;
	icon: string | null;
	/** Whether the viewer maintains the resource on `config_url`. */
	editable?: boolean;
	config_url?: string | null;
}

export interface ChatCapability extends CapabilityItem {
	kind: 'tool' | 'skill';
	state: 'off' | 'auto' | 'on';
	can_disable: boolean;
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
	/** Explicit capability states; unlisted capabilities use discovery. */
	tool_states: Record<string, 'on' | 'auto' | 'off'>;
}

export interface CreateTokenRequest {
	name: string;
	ttl_days?: number | null;
	tools_enabled?: boolean | null;
	tool_states?: Record<string, 'on' | 'auto' | 'off'>;
}

/** The token summary plus its plaintext secret, shown exactly once. */
export interface CreateTokenResponse {
	token: TokenSummary;
	plaintext: string;
}

export interface RevokeResponse {
	/** Always true; a missing or foreign token is a 404, an already revoked one a 409. */
	revoked: boolean;
}

export interface DeleteResponse {
	/** Always true; a missing or foreign token is a 404, a still active one a 409. */
	deleted: boolean;
}

export interface UpdateTokenToolsRequest {
	tools_enabled: boolean;
	tool_states: Record<string, 'on' | 'auto' | 'off'>;
}

export class ApiError extends Error {
	readonly status: number;
	/**
	 * The gateway's own error code (`error.code` in the response envelope),
	 * when it sent one.
	 *
	 * Several refusals share a status and differ only here — a `409` on a
	 * chat submit is `turn_in_progress`, `at_capacity` or
	 * `steer_already_settled`, and each wants a different reaction. Carrying
	 * the code means callers branch on it rather than pattern-matching the
	 * human-readable message, which is translated and free to change.
	 */
	readonly code?: string;
	/** Seconds from a `Retry-After` header, when a refusal (a `429`) sent one. */
	readonly retryAfter?: number;
	/** The response body as it came, when there was one. */
	readonly detail?: string;
	/** The envelope's own sentence (`error.message`), when the body was one. */
	readonly serverMessage?: string;
	/**
	 * A validator's per-field findings (`error.issues`): an agent spec's or a
	 * test case's. Empty for every other refusal.
	 */
	readonly issues: ApiIssue[];
	constructor(status: number, message: string, code?: string, retryAfter?: number, detail?: string) {
		super(message);
		const envelope = errorEnvelope(detail);
		this.status = status;
		this.code = code ?? envelope?.code;
		this.retryAfter = retryAfter;
		this.detail = detail;
		this.serverMessage = envelope?.message;
		this.issues = envelope?.issues ?? [];
	}
}

/** One finding of a validator, at a path into what was sent. */
export interface ApiIssue {
	path: string;
	message: string;
}

/** The gateway's error envelope (`{error: {message, code, issues?}}`) out of a body; undefined for anything else. */
function errorEnvelope(detail: string | undefined): { code?: string; message?: string; issues?: ApiIssue[] } | undefined {
	if (!detail) return undefined;
	try {
		const error = (JSON.parse(detail) as { error?: unknown })?.error;
		if (!error || typeof error !== 'object') return undefined;
		const { code, message, issues } = error as { code?: unknown; message?: unknown; issues?: unknown };
		return {
			code: typeof code === 'string' ? code : undefined,
			message: typeof message === 'string' ? message : undefined,
			issues: Array.isArray(issues) ? (issues as ApiIssue[]) : undefined
		};
	} catch {
		return undefined;
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
	if (!res.ok) throw await responseError(res);
	if (res.status === 204) return undefined as T;
	return (await res.json()) as T;
}

/**
 * A refused response as an {@link ApiError}, for the calls that cannot go
 * through {@link request} (multipart uploads, audio answers). The gateway
 * answers with an OpenAI-style envelope; the status lets callers tell 401
 * (sign in) from 5xx.
 */
export async function responseError(res: Response): Promise<ApiError> {
	const detail = await res.text().catch(() => '');
	const retry = Number(res.headers.get('retry-after'));
	return new ApiError(
		res.status,
		`${res.status} ${res.statusText}${detail ? ` — ${detail}` : ''}`,
		undefined,
		Number.isFinite(retry) && retry > 0 ? retry : undefined,
		detail
	);
}

/**
 * A refusal in words for the user: the server's own sentence from its
 * envelope, or, when it sent none, a catalog message naming the status —
 * never an empty notice.
 */
export function refusalSentence(
	err: ApiError,
	tr: (key: string, args?: Record<string, string | number>) => string
): string {
	return err.serverMessage?.trim().slice(0, 200) || tr('error-request-failed', { status: err.status });
}

/**
 * What the server did with an accepted message.
 *
 * `placement` is the honest half: the client no longer decides where a message
 * goes, so it is told. `started` carries both turn ids, `folded` the turn it
 * was added to, `queued` the user turn now waiting for a slot.
 */
export interface ChatSubmitAck {
	placement: 'started' | 'folded' | 'queued';
	user_turn_id?: string;
	assistant_turn_id?: string;
	steer_id?: string;
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
			effort: Effort;
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

	/**
	 * POST /api/v0/chat/sessions/{id}/messages — `202`, reply on the events
	 * stream.
	 *
	 * The response's `placement` says what the server did with it: `started` a
	 * turn, `folded` it into the answer already being written, or `queued` it
	 * until one of this user's conversations frees a slot. The client does not
	 * choose — only the server knows whether a worker is running.
	 */
	sendChatMessage: (
		id: string,
		body: { model: string; message: string; voice?: boolean }
	) =>
		request<ChatSubmitAck>(
			`/api/v0/chat/sessions/${encodeURIComponent(id)}/messages`,
			{
				method: 'POST',
				headers: { 'content-type': 'application/json' },
				body: JSON.stringify(body)
			}
		),

	/**
	 * POST /api/v0/chat/sessions/{id}/messages as multipart — a message with
	 * attachments.
	 *
	 * Here rather than in the component because of what `request` does to a
	 * failure: it throws `ApiError` carrying `error.code`. A hand-rolled
	 * `fetch` throws a plain `Error`, which silently disables every
	 * `err.code` branch the caller has — a queued message with a file refused
	 * with `at_capacity` never armed its retry and sat in the queue forever.
	 */
	sendChatMessageWithFiles: (
		id: string,
		body: { model: string; message: string; files: File[] }
	) => {
		const fd = new FormData();
		fd.append('model', body.model);
		fd.append('message', body.message);
		for (const file of body.files) fd.append('attachment', file);
		return request<ChatSubmitAck>(
			`/api/v0/chat/sessions/${encodeURIComponent(id)}/messages`,
			{ method: 'POST', body: fd }
		);
	},

	/**
	 * DELETE /api/v0/chat/sessions/{id}/turns/{turn_id} — take back a message
	 * that has been sent but not answered yet.
	 *
	 * Refused with `409 already_answered` once an answer exists or is being
	 * written; that is what cancelling the turn is for.
	 */
	deleteChatTurn: (id: string, turnId: string) =>
		request<{ deleted: string }>(
			`/api/v0/chat/sessions/${encodeURIComponent(id)}/turns/${encodeURIComponent(turnId)}`,
			{ method: 'DELETE' }
		),

	/** POST /api/v0/chat/sessions/{id}/cancel — idempotent stop request. */
	cancelChatTurn: (id: string) =>
		request<{ cancelled: boolean }>(
			`/api/v0/chat/sessions/${encodeURIComponent(id)}/cancel`,
			{ method: 'POST' }
		),

	/**
	 * POST /api/v0/chat/sessions/{id}/turns/{turn_id}/resume — answer what a
	 * paused turn waits for. `202` once it runs again; the rest of the turn
	 * arrives on the events stream.
	 */
	resumeChatTurn: (id: string, turnId: string, requestId: string, answer: { decision: string; value?: string }) =>
		request<{ assistant_turn_id: string }>(
			`/api/v0/chat/sessions/${encodeURIComponent(id)}/turns/${encodeURIComponent(turnId)}/resume`,
			{
				method: 'POST',
				headers: { 'content-type': 'application/json' },
				body: JSON.stringify({ ...answer, request_id: requestId })
			}
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
		request<{ models: ChatModelChoice[] }>('/api/v0/models'),

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
	setChatEffort: (id: string, effort: Effort) =>
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
		request<{ ok: boolean; tools_enabled: boolean; tool_states: Record<string, 'on' | 'auto' | 'off'> }>(
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
