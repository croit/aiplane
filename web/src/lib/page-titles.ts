export interface PageTitleDescriptor {
	key: string;
	branded: boolean;
}

const exactTitles: Record<string, PageTitleDescriptor> = {
	'/': { key: 'chat-default-title', branded: true },
	'/chat': { key: 'chat-default-title', branded: true },
	'/memory': { key: 'memory-heading', branded: true },
	'/scheduled': { key: 'scheduled-heading', branded: true },
	'/webhooks': { key: 'webhooks-heading', branded: true },
	'/integrations': { key: 'integrations-heading', branded: true },
	'/skills': { key: 'my-skills-heading', branded: true },
	'/tools': { key: 'tools-heading', branded: true },
	'/tokens': { key: 'tokens-page-heading', branded: true },
	'/usage': { key: 'usage-title-mine', branded: false },
	'/admin/users': { key: 'admin-users-heading', branded: true },
	'/admin/tokens': { key: 'admin-tokens-heading', branded: false },
	'/admin/groups': { key: 'groups-heading', branded: false },
	'/admin/upstreams': { key: 'upstreams-heading', branded: true },
	'/admin/models': { key: 'admin-page-title', branded: false },
	'/rag': { key: 'rag-heading', branded: true },
	'/rag/profiles': { key: 'rag-profile-heading', branded: true },
	'/admin/skills': { key: 'skills-heading', branded: true },
	'/admin/connectors': { key: 'connectors-page-title', branded: false },
	'/admin/comfyui': { key: 'admin-comfyui-page-title', branded: false },
	'/admin/limits': { key: 'limits-heading', branded: false },
	'/admin/settings': { key: 'settings-heading', branded: false }
};

export function pageTitleDescriptor(pathname: string): PageTitleDescriptor | null {
	const path = pathname.length > 1 ? pathname.replace(/\/+$/, '') : pathname;
	const exact = exactTitles[path];
	if (exact) return exact;
	if (/^\/chat\/[^/]+$/.test(path)) return { key: 'chat-default-title', branded: true };
	if (/^\/scheduled\/[^/]+\/edit$/.test(path)) return { key: 'scheduled-edit-heading', branded: true };
	if (/^\/webhooks\/[^/]+\/edit$/.test(path)) return { key: 'webhooks-edit-heading', branded: true };
	if (/^\/webhooks\/[^/]+\/rerun$/.test(path)) return { key: 'webhooks-rerun-page-name', branded: true };
	if (/^\/webhooks\/[^/]+\/runs$/.test(path)) return { key: 'webhooks-runs-page-name', branded: true };
	return null;
}
