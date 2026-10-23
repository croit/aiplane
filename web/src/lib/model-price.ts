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
	const symbol = currencySymbol(currency);
	const money = (value: number) => `${n(value, { maximumFractionDigits: 4 })}${symbol}`;
	if (!price.input && !price.output) return t('tokens-models-price-free');
	if (price.unit === 'tokens')
		return t('tokens-models-price-tokens', {
			input: n(price.input ?? 0, { maximumFractionDigits: 4 }),
			output: n(price.output ?? 0, { maximumFractionDigits: 4 }),
			symbol
		});
	return t(`tokens-models-price-per-${price.unit}`, { price: money(price.output ?? price.input ?? 0) });
}

/** The narrow symbol of an ISO 4217 code ("USD" → "$"); the code itself when the symbol is unknown. */
export function currencySymbol(code: string): string {
	try {
		// formatToParts isolates the currency mark; an unrecognised code yields
		// the generic currency sign (¤), not a symbol.
		const part = new Intl.NumberFormat('en', {
			style: 'currency',
			currency: code,
			currencyDisplay: 'narrowSymbol'
		})
			.formatToParts(0)
			.find((entry) => entry.type === 'currency');
		return part && !part.value.startsWith('¤') ? part.value : code;
	} catch {
		return code;
	}
}
