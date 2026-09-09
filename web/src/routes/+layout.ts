// SPA mode: disable prerendering so adapter-static emits the `index.html`
// fallback and the client does the data loading (via `GET /api/v0/me`).
// Prerendering would bake the anonymous shell into every route — the
// identity render would always flash "signed out" on first paint anyway,
// and `/app` is a private surface that must not emit per-route HTML.
export const prerender = false;
export const ssr = false;

import { detect, loadCatalog } from '$lib/i18n.svelte';

/**
 * Get the reader's catalog in memory before anything renders.
 *
 * Only English is bundled with the app; the other five are separate chunks
 * (see `$lib/i18n.svelte`). Awaiting here — in the root layout's load, which
 * SvelteKit runs before the first route paints — is what keeps a German
 * session from flashing an English app and then re-rendering. It costs one
 * request the service worker has usually already cached, and nothing at all
 * for an English reader.
 */
export async function load() {
	await loadCatalog(detect());
}
