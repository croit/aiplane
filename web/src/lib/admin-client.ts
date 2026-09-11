/**
 * Small helpers shared by the SPA's admin views: same-origin JSON fetches
 * with the error envelope surfaced as an Error.
 */
import { request, ApiError } from './api';

/** Admin-surface fetch: same transport as `api.*`, but admin error
 * envelopes surface as plain Errors with the server's message. */
export async function adminJson<T>(path: string, init?: RequestInit): Promise<T> {
	try {
		return await request<T>(path, init);
	} catch (err) {
		if (err instanceof ApiError) {
			const detail = err.message.split(' — ')[1];
			throw new Error(detail || err.message);
		}
		throw err;
	}
}

export function adminPut<T = unknown>(path: string, body: unknown): Promise<T> {
	return adminJson<T>(path, {
		method: 'PUT',
		headers: { 'content-type': 'application/json' },
		body: JSON.stringify(body)
	});
}

export function adminPost<T = unknown>(path: string, body?: unknown): Promise<T> {
	return adminJson<T>(path, {
		method: 'POST',
		headers: { 'content-type': 'application/json' },
		body: body === undefined ? undefined : JSON.stringify(body)
	});
}

export function adminPatch<T = unknown>(path: string, body: unknown): Promise<T> {
	return adminJson<T>(path, {
		method: 'PATCH',
		headers: { 'content-type': 'application/json' },
		body: JSON.stringify(body)
	});
}

export async function adminDelete(path: string): Promise<void> {
	await adminJson<void>(path, { method: 'DELETE' });
}
