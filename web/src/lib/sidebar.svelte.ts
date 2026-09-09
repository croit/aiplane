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

// Which refresh is the current one. Responses do not necessarily arrive in
// the order they were asked for — a broad query is slower than the narrower
// one typed after it — so a slow earlier response would otherwise land last
// and leave the list showing results for a query the box no longer holds.
let refreshSeq = 0;

export async function refreshSidebar(): Promise<void> {
	const seq = ++refreshSeq;
	try {
		const query = sidebar.searching ? sidebar.query.trim() : '';
		const url = query
			? `/api/v0/chat/sessions?q=${encodeURIComponent(query)}`
			: '/api/v0/chat/sessions';
		const data = await adminJson<{ sessions: SidebarSession[] }>(url);
		if (seq !== refreshSeq) return; // superseded while in flight
		sidebar.sessions = data.sessions;
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
