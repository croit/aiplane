// The authenticated session state, loaded once per app start.
//
// `me` is null while unknown (not yet loaded) and stays null on 401 — the
// layout turns "null after load" into a redirect to the OIDC login.
// Phase 2 will grow this into the full app store (chat list, toasts);
// phase 1 only needs the identity proof.
import { api, ApiError } from '#lib/api.js';
import { reportLocalTimezone } from '#lib/timezones.js';
import { browser } from '$app/env';

export const me = $state<{ value: Awaited<ReturnType<typeof api.me>> | null; loaded: boolean }>({
	value: null,
	loaded: false
});

let started = false;

/** Kick off the `GET /api/v0/me` fetch (idempotent per app start). */
export function loadMe(): void {
	if (!browser || started) return;
	started = true;
	api.me()
		.then((m) => {
			me.value = m;
			// Only once we know the session is live: the report is refused
			// without one, and the signed-out shell must not spend a request.
			reportLocalTimezone();
		})
		.catch((err) => {
			if (err instanceof ApiError && err.status === 401) {
				me.value = null;
			} else {
				console.error('loading /api/v0/me failed', err);
				me.value = null;
			}
		})
		.finally(() => {
			me.loaded = true;
		});
}
