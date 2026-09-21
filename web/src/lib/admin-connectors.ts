export type ConnectorAuth = 'oauth2' | 'static_bearer' | 'none';
export type ConnectorScope = 'per_user' | 'global';

export interface AdminConnector {
	key: string;
	title: string;
	description: string | null;
	icon: string | null;
	category: string | null;
	base_url: string;
	auth_type: ConnectorAuth;
	scope: ConnectorScope;
	scopes: string[];
	enabled: boolean;
	audit: boolean;
	use_dcr: boolean;
	client_id: string | null;
	has_secret: boolean;
	authorize_url: string | null;
	token_url: string | null;
	registration_url: string | null;
	groups: string[];
	seeded: boolean;
	needs_setup: boolean;
}

export interface AdminConnectorsData {
	connectors: AdminConnector[];
	groups: string[];
	redirect_uri: string;
}

export interface ConnectorFormValue {
	key: string;
	title: string;
	description: string;
	icon: string;
	category: string;
	base_url: string;
	auth_type: ConnectorAuth;
	scope: ConnectorScope;
	scopes: string;
	client_id: string;
	client_secret: string;
	client_json: string;
	use_dcr: boolean;
	authorize_url: string;
	token_url: string;
	registration_url: string;
	groups: string[];
	audit: boolean;
	overwrite: boolean;
}

export function connectorForm(connector?: AdminConnector): ConnectorFormValue {
	return {
		key: connector?.key ?? '',
		title: connector?.title ?? '',
		description: connector?.description ?? '',
		icon: connector?.icon ?? '',
		category: connector?.category ?? '',
		base_url: connector?.base_url ?? '',
		auth_type: connector?.auth_type ?? 'oauth2',
		scope: connector?.scope ?? 'per_user',
		scopes: connector?.scopes.join(' ') ?? '',
		client_id: connector?.client_id ?? '',
		client_secret: '',
		client_json: '',
		use_dcr: connector?.use_dcr ?? true,
		authorize_url: connector?.authorize_url ?? '',
		token_url: connector?.token_url ?? '',
		registration_url: connector?.registration_url ?? '',
		groups: connector?.groups.slice() ?? [],
		audit: connector?.audit ?? false,
		overwrite: connector !== undefined
	};
}

export function connectorUsesOAuth(auth: string): boolean {
	return auth === 'oauth2';
}

export type ConnectorOAuthHelpProvider = 'google_workspace' | 'google' | 'github' | 'slack' | 'dcr' | 'fallback';

export function connectorOAuthHelpProvider(connector: Pick<ConnectorFormValue, 'key' | 'category' | 'use_dcr'>): ConnectorOAuthHelpProvider {
	if (connector.use_dcr) return connector.key === 'google_workspace' ? 'google_workspace' : 'dcr';
	if (connector.category === 'Google' || connector.key.startsWith('google') || connector.key === 'gmail') return 'google';
	if (connector.key === 'github') return 'github';
	if (connector.key === 'slack') return 'slack';
	return 'fallback';
}

export function connectorBadges(connector: AdminConnector): string[] {
	return [
		connector.enabled ? 'enabled' : 'disabled',
		connector.scope === 'global' ? 'global' : null,
		connector.audit ? 'audited' : null,
		connector.seeded ? 'default' : null,
		connector.auth_type === 'oauth2' && connector.use_dcr ? 'dcr' : null,
		connector.needs_setup ? 'needs_setup' : null
	].filter((badge): badge is string => badge !== null);
}

export function splitConnectorValues(value: string): string[] {
	return value.split(/[\s,]+/).map((part) => part.trim()).filter(Boolean);
}
