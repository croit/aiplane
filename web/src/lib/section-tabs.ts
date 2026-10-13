export type SectionTab = { path: string; label: string; feature?: string };

export const sectionTabs = {
	tools: [
		{ path: '/tools', label: 'tools-heading' },
		{ path: '/tools/integrations', label: 'integrations-heading' },
		{ path: '/tools/skills', label: 'my-skills-heading', feature: 'skills' },
		{ path: '/tools/browser', label: 'tools-browser-tab' }
	],
	settings: [
		{ path: '/settings', label: 'tokens-tab-account' },
		{ path: '/settings/notifications', label: 'tokens-push-heading', feature: 'push' },
		{ path: '/settings/memory', label: 'memory-heading' },
		{ path: '/settings/tokens', label: 'tokens-page-heading' }
	],
	adminAccess: [
		{ path: '/admin/organizations', label: 'organizations-heading' },
		{ path: '/admin/users', label: 'nav-users' },
		{ path: '/admin/tokens', label: 'nav-admin-tokens' },
		{ path: '/admin/groups', label: 'nav-groups' },
		{ path: '/admin/limits', label: 'nav-limits', feature: 'limits' }
	]
} satisfies Record<string, SectionTab[]>;

export function selectedSectionTab(pathname: string, tabs: SectionTab[]): SectionTab | undefined {
	return tabs.find((tab) => tab.path === pathname);
}
