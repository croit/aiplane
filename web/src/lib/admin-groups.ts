import { GRANT_WILDCARD } from './multi-select.ts';
import type { AdminGroup, McpTool, ToolFamily } from './components/admin/AdminGroupForm.svelte';

/**
 * The group admin surface: which tab is showing, and the grant matrices behind
 * the Tools and Skills tabs.
 *
 * Kept free of Svelte so it runs under `node --test`, which is where the SPA's
 * logic is covered — the components stay thin enough to read.
 */

export const GROUP_ADMIN_TABS = ['groups', 'identity', 'tools', 'skills'] as const;
export type GroupAdminTab = (typeof GROUP_ADMIN_TABS)[number];

export function selectedGroupAdminTab(search: string): GroupAdminTab {
	const selected = new URLSearchParams(search).get('tab');
	return GROUP_ADMIN_TABS.find((tab) => tab === selected) ?? 'groups';
}

const COMFYUI_KEY = 'comfyui';
const COMFYUI_PREFIX = 'comfyui_';
const MCP_PREFIX = 'mcp__';

/** `mcp__slack__post` → `mcp__slack`; anything else is its own key. */
function mcpServerKey(id: string): string | null {
	if (!id.startsWith(MCP_PREFIX)) return null;
	const rest = id.slice(MCP_PREFIX.length);
	const sep = rest.indexOf('__');
	return sep === -1 ? null : id.slice(0, MCP_PREFIX.length + sep);
}

/**
 * How a group holds one row.
 *
 * `covered` is the load-bearing case: the group reaches this tool through a
 * wider grant it holds, but does not name it. The matrix must render that
 * differently from `granted` and refuse the click — toggling it off would mean
 * rewriting the wider grant as a list of today's ids, silently dropping whatever
 * ships next.
 */
export type Coverage = 'granted' | 'covered' | 'none';

export function coverageOf(granted: readonly string[], value: string): Coverage {
	if (granted.includes(value)) return 'granted';
	if (granted.includes(GRANT_WILDCARD)) return 'covered';
	if (value.startsWith(COMFYUI_PREFIX) && granted.includes(COMFYUI_KEY)) return 'covered';
	const server = mcpServerKey(value);
	if (server !== null && granted.includes(server)) return 'covered';
	return 'none';
}

export type GrantRowKind = 'wildcard' | 'family' | 'tool';

export interface GrantRow {
	value: string;
	kind: GrantRowKind;
	label: string;
	description?: string;
}

/**
 * The rows of the Tools matrix, widest grant first: the wildcard, then the
 * families, then the individual ids. A reader scanning for "all of X" should
 * meet it before a list it would otherwise have to check row by row.
 */
export function toolMatrixRows(
	toolIds: readonly string[],
	families: readonly ToolFamily[],
	mcpTools: readonly McpTool[],
	labels: { wildcard?: string; family?: (family: ToolFamily) => string } = {}
): GrantRow[] {
	const rows: GrantRow[] = [
		{ value: GRANT_WILDCARD, kind: 'wildcard', label: labels.wildcard ?? GRANT_WILDCARD }
	];
	const seen = new Set<string>([GRANT_WILDCARD]);
	for (const family of families) {
		if (seen.has(family.id)) continue;
		seen.add(family.id);
		rows.push({
			value: family.id,
			kind: 'family',
			label: labels.family?.(family) ?? family.id,
			description: family.id
		});
	}
	for (const id of toolIds) {
		if (seen.has(id)) continue;
		seen.add(id);
		rows.push({ value: id, kind: 'tool', label: id });
	}
	for (const tool of mcpTools) {
		if (seen.has(tool.id)) continue;
		seen.add(tool.id);
		rows.push({ value: tool.id, kind: 'tool', label: tool.id, description: tool.description });
	}
	return rows;
}

/** The rows of the Skills matrix — no families, so just the wildcard and the names. */
export function skillMatrixRows(
	skillNames: readonly string[],
	labels: { wildcard?: string } = {}
): GrantRow[] {
	return [
		{ value: GRANT_WILDCARD, kind: 'wildcard', label: labels.wildcard ?? GRANT_WILDCARD },
		...skillNames.map((name): GrantRow => ({ value: name, kind: 'tool', label: name }))
	];
}

export type GrantFilter = 'all' | 'families' | 'granted' | 'ungranted';

export function matchesGrantFilter(
	row: GrantRow,
	filter: GrantFilter,
	query: string,
	groups: readonly AdminGroup[],
	held: (group: AdminGroup) => readonly string[] = (group) => group.tools
): boolean {
	const needle = query.trim().toLocaleLowerCase();
	if (needle && !`${row.value} ${row.label}`.toLocaleLowerCase().includes(needle)) return false;
	if (filter === 'families') return row.kind !== 'tool';
	// "Granted" means reachable, not merely named: a group holding `*` can call
	// this tool, and a filter that said otherwise would hide the rows an operator
	// most wants to audit.
	const anyone = groups.some((group) => coverageOf(held(group), row.value) !== 'none');
	if (filter === 'granted') return anyone;
	if (filter === 'ungranted') return !anyone;
	return true;
}

export interface IdentityRow {
	value: string;
	/** Groups this claim value maps into, in listing order. */
	groups: string[];
	/** Whether the value has actually arrived on a login. */
	seen: boolean;
}

/**
 * One row per OIDC claim value, from both directions.
 *
 * A value that arrived on a login and maps to nothing is the silent failure the
 * whole table exists for — the user signs in and quietly gets only the default
 * group. The mirror case, a mapping no login has ever matched, is usually a
 * value mistyped when the group was created, so it is worth showing too.
 */
export function identityRows(
	observed: readonly string[],
	groups: readonly AdminGroup[]
): IdentityRow[] {
	const rows = new Map<string, IdentityRow>();
	const row = (value: string, seen: boolean) => {
		const existing = rows.get(value);
		if (existing) {
			existing.seen ||= seen;
			return existing;
		}
		const created = { value, groups: [], seen };
		rows.set(value, created);
		return created;
	};
	for (const value of observed) row(value, true);
	for (const group of groups) {
		for (const value of group.oidc_values) row(value, false).groups.push(group.name);
	}
	return [...rows.values()].sort((a, b) => a.value.localeCompare(b.value));
}
