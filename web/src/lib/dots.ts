import { request } from './api.js';
import type { Dot } from './dot-state.js';
import type { ActivityPage } from './agent-activity.js';
import type { Memory, MemoryKind } from './memory.js';
export { activeDotCount, canCreateDot } from './dot-state.js';
export type { Dot } from './dot-state.js';

export interface DotList {
	dots: Dot[];
	active_limit: number;
}

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
