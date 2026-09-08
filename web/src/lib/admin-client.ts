/**
 * Small helpers shared by the SPA's admin views: same-origin JSON fetches
 * with the error envelope surfaced as an Error.
 */
export async function adminJson<T>(
	path: string,
	init?: RequestInit
): Promise<T> {
	const res = await fetch(path, {
		credentials: 'same-origin',
		...init,
		headers: { accept: 'application/json', ...init?.headers }
	});
	if (!res.ok) {
		let message = `${res.status} ${res.statusText}`;
		try {
			const body = (await res.json()) as { error?: { message?: string } };
			if (body.error?.message) message = body.error.message;
		} catch {
			/* keep the status line */
		}
		throw new Error(message);
	}
	if (res.status === 204) return undefined as T;
	return (await res.json()) as T;
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

export async function adminDelete(path: string): Promise<void> {
	await adminJson<void>(path, { method: 'DELETE' });
}
