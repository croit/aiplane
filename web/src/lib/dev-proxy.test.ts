import assert from 'node:assert/strict';
import test from 'node:test';

import { gatewayDevProxy } from './dev-proxy.ts';

test('the public HMR origin proxies every gateway-owned route to the private backend', () => {
	const target = 'http://127.0.0.1:8081';
	const proxy = gatewayDevProxy(target);

	for (const route of [
		'/api',
		'/v1',
		'/auth',
		'/chat/attachment',
		'/hooks',
		'/healthz',
		'/readyz',
		'/openapi.json',
		'/__dev'
	]) {
		assert.deepEqual(proxy[route], { target });
	}
});

test('OAuth round trips are proxied without swallowing their SPA pages', () => {
	const proxy = gatewayDevProxy('http://127.0.0.1:8081');
	assert.ok('^/rag/(?:[^/]+/connect|oauth/callback)$' in proxy);
	assert.ok('^/integrations/(?:callback|[^/]+/(?:connect|retry))$' in proxy);
	assert.equal('/chat' in proxy, false);
	assert.equal('/rag' in proxy, false);
	assert.equal('/integrations' in proxy, false);
});
