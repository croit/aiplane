/**
 * The price one model costs, as a short label for a picker row.
 *
 * Shared by the chat model picker and the token model picker so the two format
 * a price identically. The translation and number formatters are passed in
 * (rather than imported from the reactive i18n module) so this stays a plain,
 * testable function. Returns `null` when there is no price to show at all — an
 * automatic route reaches several models, so it has no single price, and the
 * caller omits the price entirely rather than showing a misleading number.
 */
export function formatModelPrice(
	price: { input: number | null; output: number | null; unit: string } | null | undefined,
	currency: string,
	t: (key: string, args?: Record<string, string | number>) => string,
	n: (value: number, options?: Intl.NumberFormatOptions) => string
): string | null {
	if (price === null || price === undefined) return null;
	const money = (value: number) => `${n(value, { maximumFractionDigits: 4 })} ${currency}`;
	if (!price.input && !price.output) return t('tokens-models-price-free');
	if (price.unit === 'tokens')
		return t('tokens-models-price-tokens', {
			input: money(price.input ?? 0),
			output: money(price.output ?? 0)
		});
	return t(`tokens-models-price-per-${price.unit}`, { price: money(price.output ?? price.input ?? 0) });
}
