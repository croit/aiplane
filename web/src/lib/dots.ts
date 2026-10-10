import { request } from './api.js';
import type { Dot } from './dot-state.js';
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
		})
};
