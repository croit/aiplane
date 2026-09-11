export type IntegrationMode = 'always' | 'ask' | 'off';

export interface IntegrationTool {
	name: string;
	description: string;
	read_only: boolean;
	mode: IntegrationMode;
}

export interface IntegrationConnector {
	key: string;
	title: string;
	description: string | null;
	icon: string | null;
	auth_type: 'oauth2' | 'none' | 'static_bearer';
	is_global: boolean;
	needs_setup: boolean;
	connected: boolean;
	errored: boolean;
	needs_reauth: boolean;
	tools: IntegrationTool[] | null;
	tool_error: string | null;
}

export function withToolMode(
	connector: IntegrationConnector,
	toolName: string,
	mode: IntegrationMode
): IntegrationConnector {
	return {
		...connector,
		tools: connector.tools?.map((tool) => (tool.name === toolName ? { ...tool, mode } : tool)) ?? null
	};
}

export function withAllToolModes(
	connector: IntegrationConnector,
	mode: IntegrationMode
): IntegrationConnector {
	return {
		...connector,
		tools: connector.tools?.map((tool) => ({ ...tool, mode })) ?? null
	};
}
