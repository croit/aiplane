import type { ChatCapability } from './api';

export type CapabilityStateFilter = ChatCapability['state'] | 'all';

export interface CapabilityFilters {
	group: string | null;
	state: CapabilityStateFilter;
	query: string;
}

export function filterCapabilities(capabilities: ChatCapability[], filters: CapabilityFilters): ChatCapability[] {
	const needle = filters.query.trim().toLocaleLowerCase();
	return capabilities.filter((capability) =>
		(filters.group === null || capability.group === filters.group) &&
		(filters.state === 'all' || capability.state === filters.state) &&
		(!needle || `${capability.title} ${capability.description}`.toLocaleLowerCase().includes(needle))
	);
}

export function capabilityCounts(capabilities: ChatCapability[]): Record<ChatCapability['state'], number> {
	return capabilities.reduce(
		(counts, capability) => ({ ...counts, [capability.state]: counts[capability.state] + 1 }),
		{ on: 0, auto: 0, off: 0 }
	);
}
