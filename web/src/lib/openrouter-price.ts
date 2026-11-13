/** What an OpenRouter price is billed per. */
export type PriceUnit = 'token' | 'second';

/**
 * Render one price in its model's unit.
 *
 * Token prices are USD per 1M tokens and read fine with four decimals. Audio
 * seconds are USD per second and are three orders of magnitude smaller, so
 * four decimals would round every whisper provider to `$0.0000`; below
 * `$0.00001` the extra digits are what distinguishes one provider from
 * another.
 */
export function formatOpenRouterPrice(value: number | null, unit: PriceUnit): string {
	if (value === null) return '—';
	if (unit === 'second') {
		return `$${value.toFixed(value > 0 && value < 0.00001 ? 8 : 6)}`;
	}
	return `$${value.toFixed(4)}`;
}

/** The input step that lets a duration price be entered exactly. */
export function openRouterPriceStep(unit: PriceUnit): string {
	return unit === 'second' ? '0.0000001' : '0.0001';
}
