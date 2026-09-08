/**
 * Sidebar state shared across the app shell: the conversation list, its
 * search box, and the mobile drawer. The chat pages call `refresh()` after
 * anything that changes a conversation (create, delete, pin, title
 * generation) so the sidebar never drifts.
 */
import { adminJson } from './admin-client';

export interface SidebarSession {
	id: string;
	title: string | null;
	updated_at: string;
	pinned: boolean;
	/** FTS hit snippet — only present while searching. */
	snippet?: string;
}

export const sidebar = $state({
	open: false, // mobile drawer
	searching: false,
	query: '',
	sessions: [] as SidebarSession[]
});

let searchTimer: ReturnType<typeof setTimeout> | null = null;

export async function refreshSidebar(): Promise<void> {
	try {
		if (sidebar.searching && sidebar.query.trim()) {
			const data = await adminJson<{ sessions: SidebarSession[] }>(
				`/api/v0/chat/sessions?q=${encodeURIComponent(sidebar.query.trim())}`
			);
			sidebar.sessions = data.sessions;
		} else {
			const data = await adminJson<{ sessions: SidebarSession[] }>('/api/v0/chat/sessions');
			sidebar.sessions = data.sessions;
		}
	} catch {
		/* signed out / offline — the layout's own redirect handles 401 */
	}
}

/** Debounced live search while the query box is open. */
export function searchAsYouType(query: string): void {
	sidebar.query = query;
	if (searchTimer) clearTimeout(searchTimer);
	searchTimer = setTimeout(() => void refreshSidebar(), 250);
}

export function openSearch(): void {
	sidebar.searching = true;
}

export function closeSearch(): void {
	sidebar.searching = false;
	sidebar.query = '';
	if (searchTimer) clearTimeout(searchTimer);
	void refreshSidebar();
}
