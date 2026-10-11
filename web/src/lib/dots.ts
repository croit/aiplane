import { request } from './api.ts';
import type { Dot } from './dot-state.ts';
import type { ActivityPage } from './agent-activity.js';
import type { Memory, MemoryKind } from './memory.js';
export { activeDotCount, canCreateDot } from './dot-state.ts';
export type { Dot } from './dot-state.ts';

export interface DotList {
	dots: Dot[];
	active_limit: number;
}

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
	user_connection: DotCredential;
	principal_connection: DotCredential | null;
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
	tools: DotAccessTool[];
}

export const dotAccessApi = {
	load: (dotId: string) => request<DotAccess>(`/api/v0/dots/${encodeURIComponent(dotId)}/access`),
	save: (dotId: string, tools: DotAccessSelection[]) =>
		request<DotAccess>(`/api/v0/dots/${encodeURIComponent(dotId)}/access`, {
			method: 'PUT',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ tools })
		})
};

export const dotsApi = {
	list: () => request<DotList>('/api/v0/dots'),
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

export const dotMemoriesApi = {
	list: async (dotId: string) =>
		(await request<{ memories: Memory[] }>(`/api/v0/dots/${encodeURIComponent(dotId)}/memories`)).memories,
	create: (dotId: string, body: { kind: MemoryKind; content: string }) =>
		request<Memory>(`/api/v0/dots/${encodeURIComponent(dotId)}/memories`, {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify(body)
		}),
	update: (dotId: string, memoryId: string, body: { kind: MemoryKind; content: string }) =>
		request<Memory>(
			`/api/v0/dots/${encodeURIComponent(dotId)}/memories/${encodeURIComponent(memoryId)}`,
			{
				method: 'PUT',
				headers: { 'content-type': 'application/json' },
				body: JSON.stringify(body)
			}
		),
	delete: (dotId: string, memoryId: string) =>
		request<void>(
			`/api/v0/dots/${encodeURIComponent(dotId)}/memories/${encodeURIComponent(memoryId)}`,
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
