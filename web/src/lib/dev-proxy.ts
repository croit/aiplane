export interface DevProxyTarget {
	target: string;
}

export function gatewayDevProxy(target: string): Record<string, DevProxyTarget> {
	const gateway = { target };
	return {
		'/api': gateway,
		'/v1': gateway,
		'/auth': gateway,
		'/chat/attachment': gateway,
		'/hooks': gateway,
		'/healthz': gateway,
		'/readyz': gateway,
		'/openapi.json': gateway,
		'/__dev': gateway,
		'^/rag/(?:[^/]+/connect|oauth/callback)$': gateway,
		'^/integrations/(?:callback|[^/]+/(?:connect|retry))$': gateway
	};
}
