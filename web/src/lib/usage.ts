export interface UsageFilters {
	period: string;
	scope: 'all' | 'self';
	source: string;
	backend: string;
	token: string;
}

export const USAGE_PERIODS = [
	['today', 'usage-period-today'],
	['24h', 'usage-period-24h'],
	['this_week', 'usage-period-this-week'],
	['last_week', 'usage-period-last-week'],
	['this_month', 'usage-period-this-month'],
	['last_month', 'usage-period-last-month']
] as const;

export const USAGE_SOURCES = [
	['', 'usage-source-all'],
	['v1_api', 'usage-source-api'],
	['chat', 'usage-source-chat'],
	['scheduled', 'usage-source-scheduled']
] as const;

export function usageSearch(filters: UsageFilters): string {
	const params = new URLSearchParams();
	for (const [key, value] of Object.entries(filters)) {
		if (value && !(key === 'scope' && value === 'self')) params.set(key, value);
	}
	return params.size > 0 ? `?${params}` : '';
}

export function limitPercent(used: number, limit: number): number {
	if (limit <= 0) return 100;
	return Math.min(100, Math.max(0, Math.round((used / limit) * 100)));
}

export function usageInteger(value: number): string {
	const integer = Math.trunc(value);
	const digits = Math.abs(integer).toString().replace(/\B(?=(\d{3})+(?!\d))/g, '\u202f');
	return integer < 0 ? `-${digits}` : digits;
}

export function usageCost(value: number, currency: string): string {
	const cents = Math.round(value * 100);
	return `${usageInteger(Math.trunc(cents / 100))}.${String(Math.abs(cents % 100)).padStart(2, '0')}\u202f${currency}`;
}
