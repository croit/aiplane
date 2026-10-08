export const TOKEN_TABS = ['tokens', 'guides'] as const;
export type TokenTab = (typeof TOKEN_TABS)[number];

export const GUIDE_TABS = ['opencode', 'claude', 'pi', 'omp', 'python'] as const;
export type GuideTab = (typeof GUIDE_TABS)[number];

export function selectedTokenTab(search: string): TokenTab {
	const selected = new URLSearchParams(search).get('tab');
	return TOKEN_TABS.find((tab) => tab === selected) ?? 'tokens';
}

export function selectedGuideTab(search: string): GuideTab {
	const selected = new URLSearchParams(search).get('client');
	return GUIDE_TABS.find((tab) => tab === selected) ?? 'opencode';
}
