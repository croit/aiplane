import type { CapabilityItem, ChatCapability } from './api';

export type CapabilityStateFilter = ChatCapability['state'] | 'all';

export const capabilityId = (item: Pick<CapabilityItem, 'kind' | 'key'>) => `${item.kind}:${item.key}`;

export function searchCapabilities<T extends CapabilityItem>(items: T[], query: string): T[] {
	const needle = query.trim().toLocaleLowerCase();
	if (!needle) return items;
	return items.filter((item) => `${item.key} ${item.title} ${item.description}`.toLocaleLowerCase().includes(needle));
}

/** The rows by group, in the order the groups first appear. */
export function capabilityGroups<T extends CapabilityItem>(items: T[]): { name: string; rows: T[] }[] {
	return Array.from(new Set(items.map((item) => item.group))).map((name) => ({
		name,
		rows: items.filter((item) => item.group === name)
	}));
}

/** Lower ranks first; rows of one rank keep the order they came in (the server's: group, then title). */
export function rankCapabilities<T>(items: T[], rank: (item: T) => number): T[] {
	return items.map((item, index) => ({ item, index, rank: rank(item) }))
		.sort((a, b) => a.rank - b.rank || a.index - b.index)
		.map(({ item }) => item);
}

/** Where the viewer maintains a row's resource; `null` unless they may. */
export function editLink(item: CapabilityItem): string | null {
	return item.editable && item.config_url ? item.config_url : null;
}

/**
 * What a row says under its title: the resource's own description, or —
 * when there is none — "no description" with its edit page for whoever may
 * add one, and nothing for everyone else. Never a stand-in text.
 */
export function descriptionOf(item: CapabilityItem): { text: string } | { missing: string } | null {
	const text = item.description.trim();
	if (text) return { text };
	const link = editLink(item);
	return link ? { missing: link } : null;
}

export function capabilityCounts(capabilities: ChatCapability[]): Record<ChatCapability['state'], number> {
	return capabilities.reduce(
		(counts, capability) => ({ ...counts, [capability.state]: counts[capability.state] + 1 }),
		{ on: 0, auto: 0, off: 0 }
	);
}
