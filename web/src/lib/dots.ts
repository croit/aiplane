import { request } from './api.ts';
import type { Dot } from './dot-state.ts';
import type { ActivityPage } from './agent-activity.js';
import { groupMemories, type Memory, type MemoryKind } from './memory.ts';
export { activeDotCount, canCreateDot } from './dot-state.ts';
export type { Dot } from './dot-state.ts';

export interface DotList {
	dots: Dot[];
	active_limit: number;
}

export interface DotWorkspaceEntry {
	name: string;
	kind: 'file' | 'directory';
	size_bytes: number | null;
	modified_at: string | null;
}

export interface DotWorkspaceList {
	dot_id: string;
	dot_name: string;
	available: boolean;
	busy: boolean | null;
	path: string;
	entries: DotWorkspaceEntry[] | null;
	next_cursor: string | null;
}

function workspaceUrl(dotId: string, endpoint = '', path?: string, cursor?: string | null): string {
	const query = new URLSearchParams();
	if (path !== undefined) query.set('path', path);
	if (cursor !== undefined && cursor !== null) query.set('cursor', cursor);
	return `/api/v0/dots/${encodeURIComponent(dotId)}/workspace${endpoint}${query.size ? `?${query}` : ''}`;
}

export const dotWorkspaceApi = {
	list: (dotId: string, path = '', cursor: string | null = null) =>
		request<DotWorkspaceList>(workspaceUrl(dotId, '', path, cursor)),
	downloadUrl: (dotId: string, path: string) => workspaceUrl(dotId, '/file', path),
	upload: (dotId: string, path: string, file: File) => request<void>(workspaceUrl(dotId, '/file', path), {
		method: 'PUT', headers: { 'content-type': 'application/octet-stream' }, body: file
	}),
	createFolder: (dotId: string, path: string) => request<void>(workspaceUrl(dotId, '/folder'), {
		method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ path })
	}),
	remove: (dotId: string, path: string) => request<void>(workspaceUrl(dotId, '/entry', path), { method: 'DELETE' })
};

export type DotCommunicationDirection = 'outbound' | 'inbound';

export interface DotCommunicationEdge {
	outbound_enabled: boolean;
	inbound_enabled: boolean;
	generation: string | null;
}

export interface DotCommunicationPeer {
	dot: Dot;
	outgoing: DotCommunicationEdge;
	incoming: DotCommunicationEdge;
	/** The peer is an organization Dot; its `incoming` is the organization's choice. */
	cross_organization: boolean;
}

export interface DotCommunication {
	dot: Dot;
	peers: DotCommunicationPeer[];
}

export interface DotCommunicationChange {
	peer_id: string;
	direction: DotCommunicationDirection;
	enabled: boolean;
	expected_generation: string | null;
}

export interface AgentCommunicationCaller {
	dot: Dot;
	owner: { id: string; name: string | null };
	incoming: DotCommunicationEdge;
}

export interface AgentCommunication {
	dot: Dot;
	callers: AgentCommunicationCaller[];
}

export const agentCommunicationApi = {
	load: (agentId: string) => request<AgentCommunication>(`/api/v0/agents/${encodeURIComponent(agentId)}/communication`),
	save: (agentId: string, change: { peer_id: string; enabled: boolean; expected_generation: string | null }) =>
		request<AgentCommunication>(`/api/v0/agents/${encodeURIComponent(agentId)}/communication`, {
			method: 'PUT',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify(change)
		})
};

export const dotCommunicationApi = {
	load: (dotId: string) => request<DotCommunication>(`/api/v0/dots/${encodeURIComponent(dotId)}/communication`),
	save: (dotId: string, change: DotCommunicationChange) =>
		request<DotCommunication>(`/api/v0/dots/${encodeURIComponent(dotId)}/communication`, {
			method: 'PUT',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify(change)
		})
};

export type DotConnectionMode = 'user' | 'principal';
export interface DotCredential {
	id: string | null;
	generation: string | null;
	connected: boolean;
	errored: boolean;
	needs_reauth: boolean;
	authorizing: boolean;
	account_label: string | null;
}
export interface DotConnection {
	key: string;
	connector: { title: string; description: string | null; icon: string | null; auth_type: 'oauth2' | 'none' | 'static_bearer'; scope: 'per_user' | 'global' | 'agent'; needs_setup: boolean } | null;
	supported_modes: readonly DotConnectionMode[];
	selected: { mode: DotConnectionMode; binding_generation: string | null; connection_id: string | null; connection_generation: string | null };
	user_connection: DotCredential | null;
	principal_connection: DotCredential | null;
	can_select: boolean;
	can_connect: boolean;
	can_disconnect: boolean;
}
export interface DotConnections { dot: Dot; connections: DotConnection[] }
export interface DotConnectionChange {
	mode: DotConnectionMode;
	expected_binding_generation: string | null;
	connection_id: string;
	connection_generation: string;
}
export interface DotCredentialVersion { connection_id: string | null; connection_generation: string | null }
export const dotConnectionsApi = {
	load: (dotId: string) => request<DotConnections>(`/api/v0/dots/${encodeURIComponent(dotId)}/connections`),
	save: (dotId: string, key: string, body: DotConnectionChange) => request<DotConnections>(`/api/v0/dots/${encodeURIComponent(dotId)}/connections/${encodeURIComponent(key)}`, {
		method: 'PUT', headers: { 'content-type': 'application/json' }, body: JSON.stringify(body)
	}),
	connect: (dotId: string, key: string, body: DotCredentialVersion) => request<{ authorization_url: string }>(`/api/v0/dots/${encodeURIComponent(dotId)}/connections/${encodeURIComponent(key)}/connect`, {
		method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(body)
	}),
	disconnect: (dotId: string, key: string, body: DotCredentialVersion) => request<DotConnections>(`/api/v0/dots/${encodeURIComponent(dotId)}/connections/${encodeURIComponent(key)}`, {
		method: 'DELETE', headers: { 'content-type': 'application/json' }, body: JSON.stringify(body)
	})
};

export type DotToolPermission = 'always_allow' | 'always_ask';
export type DotActionRule = 'allow' | 'ask' | 'deny';

export interface DotAccessSelection {
	id: string;
	permission: DotToolPermission;
	actions: { id: string; rule: DotActionRule }[];
}

export interface DotAccessTool {
	id: string;
	label: string | null;
	description: string | null;
	available: boolean;
	selected: boolean;
	permission: DotToolPermission;
	read_only: boolean;
	identity: 'builtin' | 'user' | 'shared' | 'principal' | 'unavailable';
	connector_key: string | null;
	connector_mode: 'always' | 'ask' | 'off' | null;
	actions: { id: string; description: string }[] | null;
	action_rules: { id: string; rule: string }[];
}

export interface DotAccess {
	revision: string;
	tools: DotAccessTool[];
}

export const dotAccessApi = {
	load: (dotId: string) => request<DotAccess>(`/api/v0/dots/${encodeURIComponent(dotId)}/access`),
	save: (dotId: string, revision: string, tools: DotAccessSelection[]) =>
		request<DotAccess>(`/api/v0/dots/${encodeURIComponent(dotId)}/access`, {
			method: 'PUT',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ revision, tools })
		})
};

/** What a personal agent is called and told to do; `revision` guards a save against a concurrent change. */
export interface DotProfile {
	id: string;
	/** The Dot name it was created with; it does not change. */
	name: string;
	/** The name people see on its card and in chat. */
	display: string;
	description: string;
	task: string;
	tone: string;
	topics: string[];
	active: boolean;
	owner: { id: string; name: string | null; email: string };
	revision: string;
}

export type DotProfileBody = Pick<DotProfile, 'revision' | 'display' | 'description' | 'task' | 'tone' | 'topics'>;

export type DotDetailTab = 'task' | 'tools' | 'memory' | 'activity' | 'communication' | 'workspace' | 'automation';

/** Each tab of a personal agent's editor with its label key. */
export const DOT_DETAIL_TABS: Record<DotDetailTab, string> = {
	task: 'agents-tab-task',
	tools: 'dots-tab-access',
	memory: 'dots-tab-memory',
	activity: 'dots-tab-activity',
	communication: 'dots-tab-communication',
	workspace: 'dots-tab-workspace',
	automation: 'dots-tab-automation'
};

/**
 * The tabs a viewer gets: the owner every one; an admin changing another
 * user's personal agent only its settings, never its memory, activity,
 * files, collaboration or automation.
 */
export function dotDetailTabs(owner: boolean): DotDetailTab[] {
	return owner ? (Object.keys(DOT_DETAIL_TABS) as DotDetailTab[]) : ['task', 'tools'];
}

/** Topics as typed: one per line or separated by commas, blanks dropped. */
export function parseTopics(text: string): string[] {
	return text.split(/[\n,]/).map((topic) => topic.trim()).filter(Boolean);
}

export const dotsApi = {
	list: () => request<DotList>('/api/v0/dots'),
	profile: (id: string) => request<DotProfile>(`/api/v0/dots/${encodeURIComponent(id)}/profile`),
	saveProfile: (id: string, body: DotProfileBody) =>
		request<DotProfile>(`/api/v0/dots/${encodeURIComponent(id)}/profile`, {
			method: 'PUT',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify(body)
		}),
	create: (body: { name: string; display?: string; description?: string }) =>
		request<{ dot: Dot }>('/api/v0/dots', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify(body)
		}),
	setActive: (id: string, active: boolean) =>
		request<{ id: string; active: boolean }>(`/api/v0/dots/${encodeURIComponent(id)}/active`, {
			method: 'PUT',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ active })
		}),
	createSession: (id: string) =>
		request<{ session: { id: string } }>(`/api/v0/dots/${encodeURIComponent(id)}/sessions`, {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({})
		})
};

export type DotMemoryScope = 'dot' | 'organization';
export interface DotMemory extends Memory { scope: DotMemoryScope }
export interface DotMemoryScopeView { scope: DotMemoryScope; name: string; writable: boolean }
export interface DotMemoryList { memories: DotMemory[]; scopes: DotMemoryScopeView[] }
export interface DotMemoryBody { kind: MemoryKind; content: string; scope: DotMemoryScope }

export function dotMemorySections(list: DotMemoryList | null | undefined) {
	return (list?.scopes ?? []).map((scope) => ({
		...scope,
		grouped: groupMemories((list?.memories ?? []).filter((memory) => memory.scope === scope.scope)) as Record<MemoryKind, DotMemory[]>
	}));
}

export function dotMemoryAuthorityRevoked(status: number): boolean {
	return status === 401 || status === 403 || status === 404;
}

export const dotMemoriesApi = {
	list: async (dotId: string) =>
		request<DotMemoryList>(`/api/v0/dots/${encodeURIComponent(dotId)}/memories`),
	create: (dotId: string, body: DotMemoryBody) =>
		request<DotMemory>(`/api/v0/dots/${encodeURIComponent(dotId)}/memories`, {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify(body)
		}),
	update: (dotId: string, memoryId: string, body: DotMemoryBody) =>
		request<DotMemory>(
			`/api/v0/dots/${encodeURIComponent(dotId)}/memories/${encodeURIComponent(memoryId)}`,
			{
				method: 'PUT',
				headers: { 'content-type': 'application/json' },
				body: JSON.stringify(body)
			}
		),
	delete: (dotId: string, memoryId: string, scope: DotMemoryScope) =>
		request<void>(
			`/api/v0/dots/${encodeURIComponent(dotId)}/memories/${encodeURIComponent(memoryId)}?${new URLSearchParams({ scope })}`,
			{ method: 'DELETE' }
		)
};

export const dotActivityApi = {
	list: (dotId: string, cursor: number | null = null, limit = 20) => {
		const query = new URLSearchParams({ limit: String(limit) });
		if (cursor !== null) query.set('cursor', String(cursor));
		return request<ActivityPage>(`/api/v0/dots/${encodeURIComponent(dotId)}/activity?${query}`);
	}
};
