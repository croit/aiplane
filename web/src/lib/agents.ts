/**
 * The agent builder's data layer: wire types, the typed API calls, and the
 * pure helpers the editor components lean on (`docs/agents.md`).
 *
 * The spec is edited as a plain object that mirrors the stored JSON. The
 * components bind to it directly, so [`ensureShape`] gives every container
 * they bind into a home before editing starts and [`cleanSpec`] takes the
 * blanks back out before it is saved. What the spec means is the server's to
 * judge: nothing here validates, it only keeps the validator's answers
 * attached to the field they are about (by `path`).
 */
import type { ActivityPage, Verification } from './agent-activity.ts';
import type { Answer, HandoffContext } from './suspension.ts';
import type { AgentAnalytics } from './agent-analytics.ts';
import type { CaseBody, TestCase, TestRun, TestsListing } from './agent-tests.ts';
import { ApiError, request, type ApiIssue, type CapabilityItem } from './api.ts';
import type { ChatModelOption } from './model-option.ts';

/* eslint-disable @typescript-eslint/no-explicit-any -- a spec is open-ended JSON */
export type Spec = Record<string, any>;

/** One problem the server found in a spec, at a dotted/indexed path such as `main.tools[0]`. */
export type SpecIssue = ApiIssue;

export type OrganizationOwner = { kind: 'organization'; id: string; name: string };
/** A person; `name` and `email` are null once the user is no longer known. */
export type UserOwner = { kind: 'user'; id: string; name: string | null; email: string | null };
export interface AgentDotMetadata { name: string; active: boolean }
/** Whom an agent works for: its owner alone, an organization's members, or outside visitors. */
export type AgentKind = 'personal' | 'team' | 'public';

export interface AgentSummary {
	kind: AgentKind;
	/** The user or organization it belongs to; a public agent's creator. */
	owner: UserOwner | OrganizationOwner;
	dot: AgentDotMetadata | null;
	id: string;
	name: string;
	display: string;
	description: string;
	created_by: string;
	created_at: string;
	updated_at: string;
	disabled_at: string | null;
	/** `null` until the agent was published once. */
	live_version: number | null;
	access: 'read' | 'write';
}

/** `GET /api/v0/agents`: the caller's workspace, or every user's agents for an admin who asked. */
export interface AgentList {
	scope: 'self' | 'all';
	can_view_all: boolean;
	agents: AgentSummary[];
}

export function organizationOwner(agent: Pick<AgentSummary, 'owner'> | null | undefined): OrganizationOwner | null {
    return agent?.owner.kind === 'organization' ? agent.owner : null;
}

/** How an owner is named on screen: their name, else their address, else their id. */
export function ownerLabel(owner: AgentSummary['owner']): string {
    return owner.kind === 'organization' ? owner.name : (owner.name ?? owner.email ?? owner.id);
}

export function agentPublicName(agent: Pick<AgentSummary, 'dot' | 'name'>): string {
    return agent.dot?.name ?? agent.name;
}

export function agentUiCapabilities(agent: (Pick<AgentSummary, 'owner'> & Partial<Pick<AgentSummary, 'access'>>) | null | undefined) {
    const enabled = !!agent && !organizationOwner(agent);
    const draftChat = enabled || (!!organizationOwner(agent) && agent?.access === 'write');
    return { draftChat, execution: enabled, site: enabled, channels: enabled, embed: enabled };
}

export type PrivateChatSource = 'draft' | 'published';

export function testChatSources(agent: Pick<AgentSummary, 'owner' | 'live_version'> | null | undefined): PrivateChatSource[] {
	return organizationOwner(agent) && agent?.live_version != null && agent.live_version > 0
		? ['draft', 'published'] : ['draft'];
}

export interface TestMessageStarted {
	session_id: string;
	turn_id: string;
	source: PrivateChatSource;
	agent_version: number;
}

export interface Grant {
	kind: GrantKind;
	ref: string;
	granted_by: string;
	granted_at: string;
}

export type GrantKind = 'model' | 'tool' | 'connector' | 'skill' | 'rag_collection';
export const GRANT_KINDS: GrantKind[] = ['model', 'tool', 'connector', 'skill', 'rag_collection'];

/** `respond` answers the agent's inbox items and needs no agent-management permission; `read` and `write` include it and need it. */
export type ShareAccess = 'respond' | 'read' | 'write';
export const SHARE_ACCESS: ShareAccess[] = ['respond', 'read', 'write'];

export interface Share {
	subject_kind: 'user' | 'group';
	subject_id: string;
	access: ShareAccess;
	/** A person's display name, when they have one. */
	name?: string | null;
}

/** What `GET /api/v0/agents/{id}/share-subjects?q=` found: a search, never the roster (nothing under two characters, a few matches, an address only for an exact one). */
export interface ShareSubjectMatches {
	truncated: boolean;
	users: { id: string; name: string | null; email?: string }[];
	groups: string[];
}

/** The matches of `kind`, for the picker: a person by name, else the address they were found by, else their id. */
export function shareSubjectOptions(found: ShareSubjectMatches | null | undefined, kind: Share['subject_kind']): { value: string; label: string }[] {
	if (kind === 'group') return (found?.groups ?? []).map((g) => ({ value: g, label: g }));
	return (found?.users ?? []).map((u) => ({ value: u.id, label: u.name ?? u.email ?? u.id }));
}

/** A share's subject as the list shows it: a person's name, else the stored id. */
export function shareSubjectLabel(share: Pick<Share, 'subject_id' | 'name'>): string {
	return share.name ?? share.subject_id;
}

export type ChannelKind = 'slack' | 'discord';
export const CHANNEL_KINDS: ChannelKind[] = ['slack', 'discord'];

/** A Slack or Discord incoming webhook the agent's waiting turns are announced on. The URL is never read back. */
export interface NotifyChannel {
	id: string;
	kind: ChannelKind;
	name: string;
	url_host: string;
	details: boolean;
	lang: string;
	created_by: string;
	created_at: string;
}

/** A key a website embeds the agent with (`docs/embed.md`). The key itself is shown once, when it is created. */
export interface EmbedKey {
	id: string;
	name: string;
	origins: string[];
	created_by: string;
	created_at: string;
	revoked_at: string | null;
}

/** The script tag a website owner pastes before `</body>`. */
export function embedSnippet(scriptUrl: string, key: string): string {
	return `<script src="${scriptUrl}" data-agent-key="${key}" async></script>`;
}

export interface NewChannel {
	kind: ChannelKind;
	name: string;
	url: string;
	details: boolean;
	lang: string;
}

export interface AuditEntry {
	kind: string;
	actor_id: string | null;
	detail: unknown;
	created_at: string;
}

export interface AgentDetail extends AgentSummary {
	draft_spec: Spec;
	live_spec: Spec | null;
	publish_issues: SpecIssue[];
	grants: Grant[];
	shares: Share[];
	audit: AuditEntry[];
}

export interface AgentVersion {
	version: number;
	spec: Spec;
	published_by: string;
	published_at: string;
}

/** What the signed-in manager holds, and so may grant (`GET /api/v0/agent-resources`). */
export interface AgentResources {
	/** The models of each kind the manager may use and so grant, by the name the chat picker shows; the gateway default first. */
	models?: Record<ModelKind, ChatModelOption[]>;
	/** The tools, connectors, skills and knowledge bases the manager may grant, as the chat picker shows them. */
	items: GrantableItem[];
	/** The gateway's default model of each kind: what a spec key left unset runs on. */
	defaults?: ModelDefaults;
}

export type GrantableKind = 'tool' | 'connector' | 'skill' | 'rag_collection';

/** A resource the manager may give an agent: its capability row, the grant it takes and the tool ids it adds to `main.tools`. */
export interface GrantableItem extends CapabilityItem {
	kind: 'tool' | 'skill' | 'rag_collection';
	grant: { kind: GrantableKind; refs: string[] };
	tools: string[];
	editable: boolean;
	config_url: string | null;
}

/** The items granted by `kind`. */
export function grantable(resources: Pick<AgentResources, 'items'> | null | undefined, kind: GrantableKind): GrantableItem[] {
	return (resources?.items ?? []).filter((item) => item.grant.kind === kind);
}

/** The references a grant of `kind` may name, by title; an item granting several adds the reference itself, so no two read alike. */
export function grantOptions(resources: Pick<AgentResources, 'items'> | null | undefined, kind: GrantableKind): { value: string; label: string }[] {
	return grantable(resources, kind).flatMap((item) =>
		item.grant.refs.map((ref) => ({ value: ref, label: item.grant.refs.length > 1 ? `${item.title}: ${ref}` : item.title }))
	);
}

/** A tool id as its catalog entry names it; the id itself when the manager holds no such tool. */
export function toolTitle(resources: AgentResources | null | undefined, id: string): string {
	return grantable(resources, 'tool').find((item) => item.grant.refs.includes(id))?.title ?? id;
}

export type ModelKind = 'chat' | 'transcription' | 'speech';
export type ModelDefaults = Record<ModelKind, string | null>;

export interface Unmet {
	path: string;
	slot?: string;
	kind: string;
	message: string;
	/** `not_equal`: the value; `not_in`: the values. */
	expected?: unknown;
	/** `wrong_provenance`: who must have written the slot. */
	required?: string;
	/** `too_old`: how old it may be. */
	max_age?: string;
}

export interface SlotDebug {
	slot: string;
	status: 'set' | 'missing' | 'invalid';
	reason?: string;
	value?: unknown;
	provenance?: string;
	set_at?: string;
	set_by: string[];
}

export interface RouteDebug {
	route: string;
	description?: string;
	open: boolean;
	missing: Unmet[];
}

/** The topic guard's decision on a turn's message, under a strict scope. */
export interface ScopeDebug {
	verdict: 'in_scope' | 'out_of_scope' | 'failed';
	topics: string[];
	/** Why the guard could not decide; the message was refused all the same. */
	error?: string;
}

export interface TestDebug {
	slots: SlotDebug[];
	routes: RouteDebug[];
	routing: { routes?: unknown; picked: string | null; reason?: string }[];
	sub_agents: {
		route?: string;
		sub_agent?: string;
		sub_agent_id?: string;
		/** An `a2a` route's external agent, by its card's name and URL. */
		remote_agent?: string | null;
		card_url?: string;
		/** A `loop` route's child run: which iteration, worker or critic. */
		loop?: { route: string; iteration: number; role: 'worker' | 'critic' };
		version?: number;
		outcome?: { status: string; result?: unknown; reason?: unknown; summary?: string };
	}[];
	tool_calls: { tool: string; decision: string; policy: string }[];
	loops?: { event: 'loop_iteration' | 'loop_finished'; route: string; iteration?: number; iterations?: number; accepted: boolean; feedback?: string; stopped?: string }[];
	/** `null` unless the draft's scope is strict. */
	scope?: ScopeDebug | null;
}

/** A private agent conversation's debug view, read once its turn stopped running. */
export interface TestTurnView {
	turn_id: string;
	source: PrivateChatSource;
	agent_version: number;
	debug: TestDebug;
	/** What the turn hands to a person while it waits on one: what the Inbox would show whoever answers it. */
	handoff: HandoffContext | null;
}

/** A proposal of the prompt assistant (#117, `POST …/assist/suggest`): every piece already checked against the draft. */
export interface AssistSuggestion {
	steps: {
		task?: { orchestration: string } | null;
		/** `chips` are tone ids (`TONES`); `language` is `visitor`, an answer language code, or `null`; `response` the rest. */
		tone?: { response: string; chips: string[]; language?: string | null } | null;
		scope?: { topics: string[]; refusal: string; strict: boolean } | null;
		/** Tools by id, `name` the title their card shows; knowledge search comes as `knowledge` instead. */
		abilities?: { id: string; name: string; why: string }[];
		/** Knowledge bases (RAG collections) by id, switched on like their cards. */
		knowledge?: { id: string; name: string; why: string }[];
		/** Subjects no knowledge base the manager may grant covers. */
		missing_knowledge?: string[];
		slots?: { name: string; label: string; type: string; def: Spec }[];
		identity?: { method: string; why: string } | null;
		/** Rules of the hand-off step: waiting for all details and/or a confirmed identity. */
		handoffs?: { name: string; topic: string; target: string; target_name: string; details: boolean; identity: boolean; route: Spec }[];
		tests?: { name: string; kind: string; script: CaseBody['script']; expect: CaseBody['expect'] }[];
	};
	/** What the assistant left out, and why, in words. */
	dropped: { step: string; item?: string; reason: string }[];
	model: string;
}

export type ImproveField = 'task' | 'tone' | 'refusal';

/* ---- errors --------------------------------------------------------- */

/** A failed agent call, with the validator's per-field issues when it was a 422. */
export interface AgentError {
	status: number;
	code?: string;
	message: string;
	issues: SpecIssue[];
	/** Seconds to wait, from the refusal's `Retry-After`. */
	retryAfter?: number;
}

/** A failed agent call as the agent views show it: the envelope `ApiError` already read. */
export function parseSpecError(err: unknown): AgentError {
	if (!(err instanceof ApiError)) {
		return { status: 0, message: err instanceof Error ? err.message : String(err), issues: [] };
	}
	return {
		status: err.status,
		code: err.code,
		message: err.serverMessage ?? err.message,
		issues: err.issues,
		retryAfter: err.retryAfter
	};
}

async function call<T>(path: string, init?: RequestInit): Promise<T> {
	try {
		return await request<T>(path, init);
	} catch (err) {
		throw parseSpecError(err);
	}
}

const json = (method: string, body?: unknown): RequestInit => ({
	method,
	headers: { 'content-type': 'application/json' },
	body: body === undefined ? undefined : JSON.stringify(body)
});

/** Every call rejects with an [`AgentError`]. */
export const agentsApi = {
	list: (scope: 'self' | 'all' = 'self') =>
		call<AgentList>(scope === 'all' ? '/api/v0/agents?scope=all' : '/api/v0/agents'),
	create: (body: { name: string; display?: string; description?: string; spec?: Spec }) =>
		call<{ agent: AgentSummary }>('/api/v0/agents', json('POST', body)).then((r) => r.agent),
	get: (id: string) => call<{ agent: AgentDetail }>(`/api/v0/agents/${id}`).then((r) => r.agent),
	saveDraft: (id: string, spec: Spec) =>
		call<{ draft_spec: Spec; live_version: number | null }>(`/api/v0/agents/${id}/draft`, json('PUT', { spec })),
	/** Make a kept earlier draft the draft again (the architect's Undo). */
	restoreDraft: (id: string, revision: number) =>
		call<{ draft_spec: Spec; revision: number | null }>(`/api/v0/agents/${id}/draft/restore`, json('POST', { revision })),
	/** The person's architect conversation about `agent_id` (or a new agent). */
	startArchitect: (body: { agent_id?: string; title: string; fresh?: boolean; dot_owner?: 'personal' }) =>
		call<{ session_id: string; model: string; agent_id: string | null; resumed: boolean }>(
			'/api/v0/agent-architect',
			json('POST', body)
		),
	dotPlan: (sessionId: string) =>
		call<{ proposal: import('./architect.ts').DotSetupPlan | null; dot_id: string | null }>(`/api/v0/agent-architect/${sessionId}/dot-plan`),
	applyDotPlan: (sessionId: string, revision: string) =>
		call<{ dot: import('./dots.ts').Dot; session_id: string }>(`/api/v0/agent-architect/${sessionId}/dot-plan`, json('POST', { revision })),
	publish: (id: string) =>
		call<{ version: number; live_version: number }>(`/api/v0/agents/${id}/publish`, json('POST', {})),
	versions: (id: string) =>
		call<{ live_version: number | null; versions: AgentVersion[] }>(`/api/v0/agents/${id}/versions`),
	setLive: (id: string, version: number) =>
		call<{ live_version: number }>(`/api/v0/agents/${id}/live`, json('POST', { version })),
	analytics: (id: string, query: string) =>
		call<AgentAnalytics>(`/api/v0/agents/${id}/analytics?${query}`),
	activity: (id: string, query: string) => call<ActivityPage>(`/api/v0/agents/${id}/activity?${query}`),
	verifyActivity: (id: string) => call<Verification>(`/api/v0/agents/${id}/activity/verify`),
	tests: (id: string) => call<TestsListing>(`/api/v0/agents/${id}/tests`),
	createTest: (id: string, body: CaseBody) => call<{ case: TestCase }>(`/api/v0/agents/${id}/tests`, json('POST', body)),
	updateTest: (id: string, caseId: string, body: CaseBody) =>
		call<{ case: TestCase }>(`/api/v0/agents/${id}/tests/${caseId}`, json('PUT', body)),
	deleteTest: (id: string, caseId: string) => call<void>(`/api/v0/agents/${id}/tests/${caseId}`, json('DELETE')),
	runTests: (id: string, source: string) => call<TestRun>(`/api/v0/agents/${id}/tests/run`, json('POST', { source })),
	testRuns: (id: string) => call<{ runs: TestRun[] }>(`/api/v0/agents/${id}/test-runs`),
	testRun: (id: string, runId: string) => call<TestRun>(`/api/v0/agents/${id}/test-runs/${runId}`),
	remove: (id: string) => call<void>(`/api/v0/agents/${id}`, json('DELETE')),
	share: (id: string, share: Share) => call<Share>(`/api/v0/agents/${id}/shares`, json('POST', share)),
	revokeShare: (id: string, share: Pick<Share, 'subject_kind' | 'subject_id'>) =>
		call<void>(`/api/v0/agents/${id}/shares/revoke`, json('POST', share)),
	shareSubjects: (id: string, q: string) =>
		call<ShareSubjectMatches>(`/api/v0/agents/${id}/share-subjects?q=${encodeURIComponent(q)}`),
	channels: (id: string) =>
		call<{ channels: NotifyChannel[] }>(`/api/v0/agents/${id}/channels`).then((r) => r.channels),
	/** Whether this gateway sends Web Push at all (`GET /api/v0/push/config`). */
	pushEnabled: () => call<{ enabled: boolean }>('/api/v0/push/config').then((r) => r.enabled),
	addChannel: (id: string, channel: NewChannel) =>
		call<{ channel: NotifyChannel }>(`/api/v0/agents/${id}/channels`, json('POST', channel)).then((r) => r.channel),
	removeChannel: (id: string, channelId: string) =>
		call<void>(`/api/v0/agents/${id}/channels/${channelId}`, json('DELETE')),
	embedKeys: (id: string) =>
		call<{ embed_keys: EmbedKey[] }>(`/api/v0/agents/${id}/embed-keys`).then((r) => r.embed_keys),
	createEmbedKey: (id: string, body: { name: string; origins: string[] }) =>
		call<{ embed_key: EmbedKey; key: string }>(`/api/v0/agents/${id}/embed-keys`, json('POST', body)),
	revokeEmbedKey: (id: string, keyId: string) =>
		call<void>(`/api/v0/agents/${id}/embed-keys/${keyId}/revoke`, json('POST')),
	grant: (id: string, kind: GrantKind, ref: string) =>
		call<{ added: boolean }>(`/api/v0/system-principals/${id}/grants`, json('POST', { kind, ref })),
	revokeGrant: (id: string, kind: GrantKind, ref: string) =>
		call<void>(`/api/v0/system-principals/${id}/grants/revoke`, json('POST', { kind, ref })),
	resources: (id?: string) => call<AgentResources>(`/api/v0/agent-resources${id ? `?agent_id=${encodeURIComponent(id)}` : ''}`),
	suggest: (id: string, body: { scenario: string; template?: string; current_draft?: Spec }) =>
		call<AssistSuggestion>(`/api/v0/agents/${id}/assist/suggest`, json('POST', body)),
	improve: (id: string, field: ImproveField, text: string) =>
		call<{ suggestion: string; why: string }>(`/api/v0/agents/${id}/assist/improve`, json('POST', { field, text })),
	/** Send a private message with its explicit source; subsequent turns retain the server's pinned version. */
	sendTestMessage: (id: string, message: string, sessionId: string | null, source: PrivateChatSource) =>
		call<TestMessageStarted>(
			`/api/v0/agents/${id}/test/messages`,
			json('POST', sessionId ? { message, session_id: sessionId, source } : { message, source })
		),
	testEventsUrl: (id: string, sessionId: string) =>
		`/api/v0/agents/${encodeURIComponent(id)}/test/${encodeURIComponent(sessionId)}/events`,
	cancelTestConversation: (id: string, sessionId: string, turnId: string) =>
		call<{ ok: true }>(`/api/v0/agents/${encodeURIComponent(id)}/test/${encodeURIComponent(sessionId)}/cancel`, json('POST', { turn_id: turnId })),
	testTurnView: (id: string, sessionId: string, turnId: string) =>
		call<TestTurnView>(`/api/v0/agents/${id}/test/${sessionId}/turns/${turnId}/debug`),
	resumeTurn: (id: string, sessionId: string, turnId: string, requestId: string, answer: Answer) =>
		call<{ turn_id: string }>(
			`/api/v0/agents/${id}/conversations/${sessionId}/turns/${turnId}/resume`,
			json('POST', { ...answer, request_id: requestId })
		)
};

/* ---- issues, by path ------------------------------------------------ */

/** The messages for exactly `path`. */
export function issuesAt(issues: SpecIssue[], path: string): string[] {
	return issues.filter((i) => i.path === path).map((i) => i.message);
}

/** The issues at `path` or anywhere below it (`path.x`, `path[0]`). */
export function issuesUnder(issues: SpecIssue[], path: string): SpecIssue[] {
	return issues.filter(
		(i) => i.path === path || i.path.startsWith(`${path}.`) || i.path.startsWith(`${path}[`)
	);
}

/**
 * The issues of the last refused save that still point into `spec`. One
 * under an entry of a named map (a route, slot, verifier, pattern, …) or a
 * list item the manager has since removed is dropped; one naming a key
 * missing from an entry that still exists (`state.issue.values`) is kept,
 * since that is what it asks to add.
 */
export function liveIssues(issues: SpecIssue[], spec: Spec): SpecIssue[] {
	return issues.filter((issue) => {
		const parts = issue.path.match(/[^.[\]]+/g) ?? [];
		let node: unknown = spec;
		for (const [i, part] of parts.entries()) {
			const container = node as Record<string, unknown> | null;
			if (container !== null && typeof container === 'object' && part in container) {
				node = container[part];
				continue;
			}
			return i === parts.length - 1 && !/^\d+$/.test(part) && !NAMED_MAPS.has(parts[i - 1]);
		}
		return true;
	});
}

/* ---- the spec as an editing buffer ---------------------------------- */

/** A deep copy that also accepts a Svelte `$state` proxy, which `structuredClone` refuses. */
const clone = <T>(value: T): T => JSON.parse(JSON.stringify(value));

/** `spec` with every container the editor binds into present. Unknown keys are kept. */
export function ensureShape(spec: Spec): Spec {
	const out: Spec = clone(spec ?? {});
	const main = (out.main ??= {});
	const instructions = (main.instructions ??= {});
	instructions.orchestration ??= '';
	instructions.response ??= '';
	main.tools ??= [];
	main.skills ??= [];
	main.tool_resources ??= {};
	main.budget ??= {};
	out.state ??= {};
	out.routes ??= {};
	return out;
}

/** Containers whose entries are named: an empty entry is a mistake to report, not noise to drop. */
const NAMED_MAPS = new Set(['state', 'verifiers', 'routes', 'tool_resources', 'patterns']);
/** Subtrees the editor treats as opaque JSON: never pruned. */
const OPAQUE = new Set(['when', 'schema', 'human']);

const blank = (v: unknown): boolean =>
	v === undefined ||
	v === null ||
	v === '' ||
	(typeof v === 'number' && Number.isNaN(v)) ||
	(Array.isArray(v) && v.length === 0) ||
	(typeof v === 'object' && !Array.isArray(v) && Object.keys(v as object).length === 0);

function prune(value: unknown, key: string): unknown {
	if (Array.isArray(value)) return value.map((v) => prune(v, '')).filter((v) => !blank(v));
	if (value && typeof value === 'object') {
		const out: Record<string, unknown> = {};
		for (const [k, v] of Object.entries(value)) {
			if (OPAQUE.has(k)) {
				out[k] = v;
				continue;
			}
			const kept = prune(v, k);
			if (!blank(kept) || (NAMED_MAPS.has(key) && v && typeof v === 'object')) out[k] = kept;
		}
		return out;
	}
	return value;
}

/** The spec to save: blanks removed, `main` always present, input untouched. */
export function cleanSpec(spec: Spec): Spec {
	const pruned = prune(clone(spec), '') as Spec;
	return { ...pruned, main: pruned.main ?? {} };
}

/* ---- gates ---------------------------------------------------------- */

export type CondKind = 'all' | 'any' | 'not' | 'leaf';

export function condKind(cond: Record<string, unknown>): CondKind {
	if ('all' in cond) return 'all';
	if ('any' in cond) return 'any';
	if ('not' in cond) return 'not';
	return 'leaf';
}

/** Read a leaf's typed text as the slot's type says; left as typed when it does not parse. */
export function slotValueFromText(type: string, text: string): unknown {
	if (type === 'integer' || type === 'number') {
		const n = Number(text);
		return text.trim() !== '' && Number.isFinite(n) ? n : text;
	}
	if (type === 'boolean') return text === 'true' ? true : text === 'false' ? false : text;
	return text;
}

/** A comma- or line-separated list, trimmed, without blanks. */
export function splitList(text: string): string[] {
	return text
		.split(/[,\n]/)
		.map((s) => s.trim())
		.filter(Boolean);
}

/* ---- binds ---------------------------------------------------------- */

export type BindKind = 'state' | 'route' | 'const';

/** The spec form of a bind source: `state.<path>`, `route.<name>` or `{const}`. */
export function bindSource(kind: BindKind, value: string): string | { const: unknown } {
	if (kind === 'state') return `state.${value}`;
	if (kind === 'route') return `route.${value}`;
	const n = Number(value);
	if (value.trim() !== '' && Number.isFinite(n)) return { const: n };
	if (value === 'true' || value === 'false') return { const: value === 'true' };
	return { const: value };
}

export function parseBindSource(source: unknown): { kind: BindKind; value: string } {
	if (typeof source === 'string') {
		if (source.startsWith('state.')) return { kind: 'state', value: source.slice(6) };
		if (source.startsWith('route.')) return { kind: 'route', value: source.slice(6) };
		return { kind: 'const', value: source };
	}
	if (source && typeof source === 'object' && 'const' in source) {
		return { kind: 'const', value: String((source as { const: unknown }).const) };
	}
	return { kind: 'const', value: '' };
}

export function describeBindSource(source: unknown): string {
	const { kind, value } = parseBindSource(source);
	return kind === 'const' ? value : `${kind}.${value}`;
}

/* ---- the test chat -------------------------------------------------- */

const GATE_HINTS: Record<string, string> = {
	missing: 'agents-gate-missing',
	invalid: 'agents-gate-invalid',
	must_be_unset: 'agents-gate-must-be-unset',
	not_equal: 'agents-gate-not-equal',
	not_in: 'agents-gate-not-in',
	wrong_provenance: 'agents-gate-wrong-provenance',
	too_old: 'agents-gate-too-old'
};

const shown = (v: unknown) => (v === undefined ? '' : typeof v === 'string' ? `“${v}”` : JSON.stringify(v));

/**
 * What keeps a gate closed, in words a manager reads: the slot by its
 * `label`, the condition from the catalog. A condition without a slot (an
 * excluded branch, the classifier's veto) keeps the server's message.
 */
export function gateHint(unmet: Unmet, label: string, tr: (key: string, args?: Record<string, string | number>) => string): string {
	const key = GATE_HINTS[unmet.kind];
	if (!key || !unmet.slot) return unmet.message;
	const expected = Array.isArray(unmet.expected) ? unmet.expected.map(shown).join(', ') : shown(unmet.expected);
	return tr(key, { slot: label, expected, required: unmet.required ?? '', age: unmet.max_age ?? '' });
}

type TestTurnState = { turn: { id: string; role: string; status: string }; suspension?: { request_id: string } | null };

/** Whether `live` still waits on the pause `requestId` was answered for. */
const stillOn = (live: TestTurnState | undefined, requestId: string | undefined) =>
	requestId !== undefined && live?.turn.status === 'suspended' && live.suspension?.request_id === requestId;

/**
 * The test-chat answers that stopped running and whose debug view was not
 * read yet. A pause just answered (`answered`: turn → the request answered)
 * is not one while the transcript still shows that pause: the stream has not
 * shown the turn moving on yet.
 */
export function turnsToRead(turns: TestTurnState[], read: Record<string, unknown>, answered: Record<string, string> = {}): string[] {
	return turns
		.filter(
			(live) =>
				live.turn.role === 'assistant' &&
				live.turn.status !== 'in_progress' &&
				!(live.turn.id in read) &&
				!stillOn(live, answered[live.turn.id])
		)
		.map((live) => live.turn.id);
}

/** `answered` without the turns the transcript shows past the pause they were answered on. */
export function settleAnswered(turns: TestTurnState[], answered: Record<string, string>): Record<string, string> {
	return Object.fromEntries(
		Object.entries(answered).filter(([id, requestId]) => stillOn(turns.find((live) => live.turn.id === id), requestId))
	);
}

/** What a failed debug read means: a turn not settled yet is read again once it is, anything else is shown. */
export function readFailure(err: AgentError): 'retry' | string {
	return err.status === 409 && err.code === 'turn_in_progress' ? 'retry' : err.message;
}

export function testTurnLabel(status: string): string {
	switch (status) {
		case 'completed':
			return 'agents-test-status-completed';
		case 'errored':
			return 'agents-test-status-errored';
		case 'suspended':
			return 'agents-test-status-suspended';
		default:
			return 'agents-test-status-other';
	}
}

/* ---- named maps ----------------------------------------------------- */

/** `map` with key `from` renamed to `to`, keeping its position. Unchanged when `to` is taken. */
export function renameKey<T>(map: Record<string, T>, from: string, to: string): Record<string, T> {
	if (from === to || to in map || !(from in map)) return map;
	return Object.fromEntries(Object.entries(map).map(([k, v]) => [k === from ? to : k, v]));
}

/** The first `<prefix>_<n>` not yet a key of `map`. */
export function freshName(map: Record<string, unknown>, prefix: string): string {
	for (let n = 1; ; n++) if (!(`${prefix}_${n}` in map)) return `${prefix}_${n}`;
}

/** Writers a slot can name: the model, the host, and one per declared verifier. */
export function writerOptions(spec: Spec): string[] {
	return ['llm', 'host', ...Object.keys(spec.verifiers ?? {}).map((id) => `verifier:${id}`)];
}

/** What a gate or bind editor needs to know about each declared slot. */
export interface SlotInfo {
	name: string;
	type: string;
	values: string[];
	writers: string[];
}

export function slotInfos(spec: Spec): SlotInfo[] {
	return Object.entries((spec.state ?? {}) as Record<string, Spec>).map(([name, def]) => ({
		name,
		type: def?.type ?? 'string',
		values: Array.isArray(def?.values) ? def.values.map(String) : [],
		writers: Array.isArray(def?.set_by) ? def.set_by.map(String) : []
	}));
}

/* ---- routes ---------------------------------------------------------- */

/** A route as the form builder and the canvas both create it: a gate on no slot yet, and no sub-agent chosen. */
export const newRoute = (): Spec => ({ when: { slot: '', set: true }, agent: '', task: '' });

/** Adds a fresh route to `spec.routes` and returns its name. */
export function addRoute(spec: Spec): string {
	spec.routes ??= {};
	const name = freshName(spec.routes, 'route');
	spec.routes[name] = newRoute();
	return name;
}

/** Removes route `name`, and its place in `router.order`: the server refuses an order naming a route that does not exist. */
export function removeRoute(spec: Spec, name: string): void {
	delete spec.routes?.[name];
	setRouterOrder(spec, (order) => order.filter((r) => r !== name));
}

/** Renames route `from` to `to` (trimmed), keeping its position, in `router.order` too. A blank or taken name changes nothing. Returns the name the route has afterwards. */
export function renameRoute(spec: Spec, from: string, to: string): string {
	const name = to.trim();
	if (!name || name === from || name in spec.routes || !(from in spec.routes)) return from;
	spec.routes = renameKey(spec.routes, from, name);
	setRouterOrder(spec, (order) => order.map((r) => (r === from ? name : r)));
	return name;
}

function setRouterOrder(spec: Spec, edit: (order: string[]) => string[]): void {
	if (!Array.isArray(spec.router?.order)) return;
	const order = edit(spec.router.order);
	if (order.length) spec.router.order = order;
	else delete spec.router.order;
}

/** What the agent's principal has been granted, for the editor's pickers. */
export interface Granted {
	models: string[];
	/** The gateway's default models, which an unset model key runs on. */
	defaults: ModelDefaults | null;
	tools: { id: string; name: string }[];
	skills: string[];
}

const GERMAN_LETTERS: Record<string, string> = { ä: 'ae', ö: 'oe', ü: 'ue', ß: 'ss' };

/**
 * The agent's id (its system principal's name: lowercase letters, digits and
 * dashes, at most 48) derived from the name a person types, so creating an
 * agent asks for one name, not two.
 */
export function agentIdFromName(name: string): string {
	const id = name
		.toLowerCase()
		.replace(/[äöüß]/g, (c) => GERMAN_LETTERS[c])
		.normalize('NFKD')
		.replace(/[̀-ͯ]/g, '')
		.replace(/[^a-z0-9]+/g, '-')
		.replace(/^-+|-+$/g, '')
		.slice(0, 48)
		.replace(/-+$/, '');
	return id || 'agent';
}
