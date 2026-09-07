// SPA mode: disable prerendering so adapter-static emits the `index.html`
// fallback and the client does the data loading (via `GET /api/v0/me`).
// Prerendering would bake the anonymous shell into every route — the
// identity render would always flash "signed out" on first paint anyway,
// and `/app` is a private surface that must not emit per-route HTML.
export const prerender = false;
export const ssr = false;
