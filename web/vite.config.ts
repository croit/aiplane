import adapter from '@sveltejs/adapter-static';
import tailwindcss from '@tailwindcss/vite';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

/**
 * The gateway's SPA (issue #22).
 *
 * Built as a pure static shell: `adapter-static` emits `build/` (content-hashed
 * assets + an `index.html` fallback), which the gateway binary serves at
 * `/app` from `GATEWAY_STATIC_DIR` — no Node at runtime, one container.
 *
 * `paths.base = '/app'` makes every emitted asset URL and every in-app
 * navigation root-relative to that mount, so the same build works behind the
 * gateway's `/app` prefix. History fallback is `index.html`: the Rust SPA
 * handler (`crates/gateway/src/rama_server/spa.rs`) serves it for any `/app/*`
 * path that is not a real file, which is what the client router needs.
 *
 * Dev loop: `vite dev` on :5173 with the API surface proxied to the running
 * gateway on :8080 (see `build.proxy`). Session cookies are SameSite=Lax and
 * the proxy preserves the Origin/Host, so the browser-side session cookie set
 * by `/auth/callback` rides along on proxied `/api/v0/*` calls.
 */
export default defineConfig({
	plugins: [tailwindcss(), sveltekit({
		compilerOptions: {
			// Runes mode for our components (Svelte 5).
			runes: ({ filename }) =>
				filename.split(/[/\\]/).includes('node_modules') ? undefined : true
		},
		paths: {
			base: '/app'
		},
		adapter: adapter({
			pages: 'build',
			assets: 'build',
			// SPA fallback — the Rust handler serves this for unknown /app/* paths.
			fallback: 'index.html',
			// The app authenticates per-request; a prerendered shell must not bake
			// in an auth state. Nothing here is a static marketing page.
			precompress: false,
			strict: true
		})
	})],
	server: {
		proxy: {
			// Everything dynamic belongs to the gateway; the dev server only
			// serves the HMR'd SPA shell. `/v1` is proxied for the (future)
			// in-app streaming client; `/auth` carries the OIDC browser flow.
			'/api': 'http://localhost:8080',
			'/v1': 'http://localhost:8080',
			'/auth': 'http://localhost:8080'
		}
	}
});
