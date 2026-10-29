import { request } from './api.ts';
import type { OwnedAgent } from './agent-state.ts';
import type { ActivityPage } from './agent-activity.js';
import { groupMemories, type Memory, type MemoryKind } from './memory.ts';
export { activeAgentCount, canCreateAgent } from './agent-state.ts';
export type { OwnedAgent } from './agent-state.ts';

export interface AgentWorkspaceEntry {
	name: string;
	kind: 'file' | 'directory';
	size_bytes: number | null;
	modified_at: string | null;
}

export interface AgentWorkspaceList {
	agent_id: string;
	agent_name: string;
	available: boolean;
	busy: boolean | null;
	path: string;
	entries: AgentWorkspaceEntry[] | null;
	next_cursor: string | null;
}

function workspaceUrl(agentId: string, endpoint = '', path?: string, cursor?: string | null): string {
	const query = new URLSearchParams();
	if (path !== undefined) query.set('path', path);
	if (cursor !== undefined && cursor !== null) query.set('cursor', cursor);
	return `/api/v0/agents/${encodeURIComponent(agentId)}/workspace${endpoint}${query.size ? `?${query}` : ''}`;
}

export const agentWorkspaceApi = {
	list: (agentId: string, path = '', cursor: string | null = null) =>
		request<AgentWorkspaceList>(workspaceUrl(agentId, '', path, cursor)),
	downloadUrl: (agentId: string, path: string) => workspaceUrl(agentId, '/file', path),
	upload: (agentId: string, path: string, file: File) => request<void>(workspaceUrl(agentId, '/file', path), {
		method: 'PUT', headers: { 'content-type': 'application/octet-stream' }, body: file
	}),
	createFolder: (agentId: string, path: string) => request<void>(workspaceUrl(agentId, '/folder'), {
		method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ path })
	}),
	remove: (agentId: string, path: string) => request<void>(workspaceUrl(agentId, '/entry', path), { method: 'DELETE' })
};

export type OwnedAgentCommunicationDirection = 'outbound' | 'inbound';

export interface OwnedAgentCommunicationEdge {
	outbound_enabled: boolean;
	inbound_enabled: boolean;
	generation: string | null;
}

export interface OwnedAgentCommunicationPeer {
	agent: OwnedAgent;
	outgoing: OwnedAgentCommunicationEdge;
	incoming: OwnedAgentCommunicationEdge;
	/** The peer is an organization agent; its `incoming` is the organization's choice. */
	cross_organization: boolean;
}

export interface OwnedAgentCommunication {
	agent: OwnedAgent;
	peers: OwnedAgentCommunicationPeer[];
}

export interface OwnedAgentCommunicationChange {
	peer_id: string;
	direction: OwnedAgentCommunicationDirection;
	enabled: boolean;
	expected_generation: string | null;
}

export interface AgentCommunicationCaller {
	agent: OwnedAgent;
	owner: { id: string; name: string | null };
	incoming: OwnedAgentCommunicationEdge;
}

export interface AgentCommunication {
	agent: OwnedAgent;
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

export const ownedAgentCommunicationApi = {
	load: (agentId: string) => request<OwnedAgentCommunication>(`/api/v0/agents/${encodeURIComponent(agentId)}/communication`),
	save: (agentId: string, change: OwnedAgentCommunicationChange) =>
		request<OwnedAgentCommunication>(`/api/v0/agents/${encodeURIComponent(agentId)}/communication`, {
			method: 'PUT',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify(change)
		})
};

export type AgentConnectionMode = 'user' | 'principal';
export interface AgentCredential {
	id: string | null;
	generation: string | null;
	connected: boolean;
	errored: boolean;
	needs_reauth: boolean;
	authorizing: boolean;
	account_label: string | null;
}
export interface AgentConnection {
	key: string;
	connector: { title: string; description: string | null; icon: string | null; auth_type: 'oauth2' | 'none' | 'static_bearer'; scope: 'per_user' | 'global' | 'agent'; needs_setup: boolean } | null;
	supported_modes: readonly AgentConnectionMode[];
	selected: { mode: AgentConnectionMode; binding_generation: string | null; connection_id: string | null; connection_generation: string | null };
	user_connection: AgentCredential | null;
	principal_connection: AgentCredential | null;
	can_select: boolean;
	can_connect: boolean;
	can_disconnect: boolean;
}
export interface AgentConnections { agent: OwnedAgent; connections: AgentConnection[] }
export interface AgentConnectionChange {
	mode: AgentConnectionMode;
	expected_binding_generation: string | null;
	connection_id: string;
	connection_generation: string;
}
export interface AgentCredentialVersion { connection_id: string | null; connection_generation: string | null }
export const agentConnectionsApi = {
	load: (agentId: string) => request<AgentConnections>(`/api/v0/agents/${encodeURIComponent(agentId)}/connections`),
	save: (agentId: string, key: string, body: AgentConnectionChange) => request<AgentConnections>(`/api/v0/agents/${encodeURIComponent(agentId)}/connections/${encodeURIComponent(key)}`, {
		method: 'PUT', headers: { 'content-type': 'application/json' }, body: JSON.stringify(body)
	}),
	connect: (agentId: string, key: string, body: AgentCredentialVersion) => request<{ authorization_url: string }>(`/api/v0/agents/${encodeURIComponent(agentId)}/connections/${encodeURIComponent(key)}/connect`, {
		method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(body)
	}),
	disconnect: (agentId: string, key: string, body: AgentCredentialVersion) => request<AgentConnections>(`/api/v0/agents/${encodeURIComponent(agentId)}/connections/${encodeURIComponent(key)}`, {
		method: 'DELETE', headers: { 'content-type': 'application/json' }, body: JSON.stringify(body)
	})
};

export type AgentToolPermission = 'always_allow' | 'always_ask';
export type AgentActionRule = 'allow' | 'ask' | 'deny';

export interface AgentAccessSelection {
	id: string;
	permission: AgentToolPermission;
	actions: { id: string; rule: AgentActionRule }[];
}

export interface AgentAccessTool {
	id: string;
	label: string | null;
	description: string | null;
	available: boolean;
	selected: boolean;
	permission: AgentToolPermission;
	read_only: boolean;
	identity: 'builtin' | 'user' | 'shared' | 'principal' | 'unavailable';
	connector_key: string | null;
	connector_mode: 'always' | 'ask' | 'off' | null;
	actions: { id: string; description: string }[] | null;
	action_rules: { id: string; rule: string }[];
}

export interface AgentAccess {
	revision: string;
	tools: AgentAccessTool[];
}

export const agentAccessApi = {
	load: (agentId: string) => request<AgentAccess>(`/api/v0/agents/${encodeURIComponent(agentId)}/access`),
	save: (agentId: string, revision: string, tools: AgentAccessSelection[]) =>
		request<AgentAccess>(`/api/v0/agents/${encodeURIComponent(agentId)}/access`, {
			method: 'PUT',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ revision, tools })
		})
};

/** What a personal agent is called and told to do; `revision` guards a save against a concurrent change. */
export interface AgentProfile {
	id: string;
	/** The agent name it was created with; it does not change. */
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

export type AgentProfileBody = Pick<AgentProfile, 'revision' | 'display' | 'description' | 'task' | 'tone' | 'topics'>;

export type AgentDetailTab = 'task' | 'tools' | 'memory' | 'activity' | 'communication' | 'workspace' | 'automation';

/** Each tab of a personal agent's editor with its label key. */
export const AGENT_DETAIL_TABS: Record<AgentDetailTab, string> = {
	task: 'agents-tab-task',
	tools: 'agents-tab-access',
	memory: 'agents-tab-memory',
	activity: 'agents-tab-activity',
	communication: 'agents-tab-communication',
	workspace: 'agents-tab-workspace',
	automation: 'agents-tab-automation'
};

/**
 * The tabs a viewer gets: the owner every one; an admin changing another
 * user's personal agent only its settings, never its memory, activity,
 * files, collaboration or automation.
 */
export function agentDetailTabs(owner: boolean): AgentDetailTab[] {
	return owner ? (Object.keys(AGENT_DETAIL_TABS) as AgentDetailTab[]) : ['task', 'tools'];
}

/** Topics as typed: one per line or separated by commas, blanks dropped. */
export function parseTopics(text: string): string[] {
	return text.split(/[\n,]/).map((topic) => topic.trim()).filter(Boolean);
}

export const ownedAgentsApi = {
	profile: (id: string) => request<AgentProfile>(`/api/v0/agents/${encodeURIComponent(id)}/profile`),
	saveProfile: (id: string, body: AgentProfileBody) =>
		request<AgentProfile>(`/api/v0/agents/${encodeURIComponent(id)}/profile`, {
			method: 'PUT',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify(body)
		}),
	create: (body: { name: string; display?: string; description?: string }) =>
		request<{ agent: OwnedAgent }>('/api/v0/agents', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ kind: 'personal', ...body })
		}),
	setActive: (id: string, active: boolean) =>
		request<{ id: string; active: boolean }>(`/api/v0/agents/${encodeURIComponent(id)}/active`, {
			method: 'PUT',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ active })
		}),
	createSession: (id: string) =>
		request<{ session: { id: string } }>(`/api/v0/agents/${encodeURIComponent(id)}/sessions`, {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({})
		})
};

export type AgentMemoryScope = 'agent' | 'organization';
export interface AgentMemory extends Memory { scope: AgentMemoryScope }
export interface AgentMemoryScopeView { scope: AgentMemoryScope; name: string; writable: boolean }
export interface AgentMemoryList { memories: AgentMemory[]; scopes: AgentMemoryScopeView[] }
export interface AgentMemoryBody { kind: MemoryKind; content: string; scope: AgentMemoryScope }

export function agentMemorySections(list: AgentMemoryList | null | undefined) {
	return (list?.scopes ?? []).map((scope) => ({
		...scope,
		grouped: groupMemories((list?.memories ?? []).filter((memory) => memory.scope === scope.scope)) as Record<MemoryKind, AgentMemory[]>
	}));
}

export function agentMemoryAuthorityRevoked(status: number): boolean {
	return status === 401 || status === 403 || status === 404;
}

export const agentMemoriesApi = {
	list: async (agentId: string) =>
		request<AgentMemoryList>(`/api/v0/agents/${encodeURIComponent(agentId)}/memories`),
	create: (agentId: string, body: AgentMemoryBody) =>
		request<AgentMemory>(`/api/v0/agents/${encodeURIComponent(agentId)}/memories`, {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify(body)
		}),
	update: (agentId: string, memoryId: string, body: AgentMemoryBody) =>
		request<AgentMemory>(
			`/api/v0/agents/${encodeURIComponent(agentId)}/memories/${encodeURIComponent(memoryId)}`,
			{
				method: 'PUT',
				headers: { 'content-type': 'application/json' },
				body: JSON.stringify(body)
			}
		),
	delete: (agentId: string, memoryId: string, scope: AgentMemoryScope) =>
		request<void>(
			`/api/v0/agents/${encodeURIComponent(agentId)}/memories/${encodeURIComponent(memoryId)}?${new URLSearchParams({ scope })}`,
			{ method: 'DELETE' }
		)
};

export const agentActivityApi = {
	list: (agentId: string, cursor: number | null = null, limit = 20) => {
		const query = new URLSearchParams({ limit: String(limit) });
		if (cursor !== null) query.set('cursor', String(cursor));
		return request<ActivityPage>(`/api/v0/agents/${encodeURIComponent(agentId)}/activity?${query}`);
	}
};
