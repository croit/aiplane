import adapter from '@sveltejs/adapter-static';
import tailwindcss from '@tailwindcss/vite';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';
import { gatewayDevProxy } from './src/lib/dev-proxy.ts';

/**
 * The gateway's SPA (issue #22).
 *
 * Built as a pure static shell: `adapter-static` emits `build/` (content-hashed
 * assets + an `index.html` fallback), which the gateway binary serves from
 * `AIPLANE_STATIC_DIR` — no Node at runtime, one container.
 *
 * The app is served from the root — it is the whole UI — so no `paths.base`
 * is set. History fallback is `index.html`: the Rust SPA handler
 * (`crates/aiplane/src/rama_server/spa.rs`) serves it for any path that is not
 * a real file, which is what the client router needs.
 *
 * Dev loop: `mise run dev` exposes Vite on :8080 with the complete dynamic
 * surface proxied to the private gateway on :8081. Session cookies are SameSite=Lax and
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
		adapter: adapter({
			pages: 'build',
			assets: 'build',
			// SPA fallback — the Rust handler serves this for any path that is
			// not a real file.
			fallback: 'index.html',
			// The app authenticates per-request; a prerendered shell must not bake
			// in an auth state. Nothing here is a static marketing page.
			precompress: false,
			strict: true
		})
	})],
	server: {
		host: '127.0.0.1',
		port: Number(process.env.AIPLANE_DEV_PUBLIC_PORT ?? 8080),
		strictPort: true,
		proxy: gatewayDevProxy(process.env.AIPLANE_DEV_BACKEND_ORIGIN ?? 'http://127.0.0.1:8081')
	}
});
