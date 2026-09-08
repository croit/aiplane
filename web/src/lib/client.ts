/**
 * THE OpenAPI-generated API client (issue #22): every `/api/v0` call in the
 * SPA goes through `client` below — typed by `docs/openapi.json` via
 * `schema.d.ts` (regenerate with `mise run gen-api-client`; the openapi_drift
 * test keeps the spec and the router in sync, this file keeps the client and
 * the spec in sync).
 *
 * Error envelopes: openapi-fetch returns `{ data, error }`; the helpers in
 * `api.ts` unwrap that into the `api.*` shape the pages already use.
 */
import createClient from 'openapi-fetch';
import type { paths } from './schema.d.ts';

export const client = createClient<paths>({
	baseUrl: '/api/v0',
	credentials: 'same-origin',
	headers: { accept: 'application/json' }
});
