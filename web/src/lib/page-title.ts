import { writable } from 'svelte/store';

export interface PageTitleOverride {
	pathname: string;
	title: string | null;
}

export const pageTitleOverride = writable<PageTitleOverride>({ pathname: '', title: null });

export function setPageTitleOverride(pathname: string, title: string): void {
	pageTitleOverride.set({ pathname, title });
}

export function clearPageTitleOverride(pathname: string): void {
	pageTitleOverride.update((current) =>
		current.pathname === pathname ? { pathname: '', title: null } : current
	);
}
