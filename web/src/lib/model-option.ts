import type { SearchOption, SearchOptionBadge } from './searchable-select';

export interface ChatModelOption {
	id: string;
	/** The operator's picker description, when one is configured. */
	description?: string;
	/** A speech model's voices (`GET /api/v0/agent-resources`). */
	voices?: string[];
	/**
	 * The model's configured price, when the source exposes it. `null`/absent
	 * means "no price to show" — an unpriced model renders "free" (the formatter
	 * decides) while a route renders nothing at all.
	 */
	price?: { input: number | null; output: number | null; unit: string } | null;
}

export function modelSelectOptions(
	models: ChatModelOption[],
	/**
	 * Optional price line, shown as a neutral badge. Supplied only by pickers whose source carries prices (the chat
	 * picker); it returns `null` to omit the line (a route has no price).
	 */
	formatPrice?: (price: ChatModelOption['price']) => string | null
): SearchOption[] {
	return models.map((model) => {
		const badges: SearchOptionBadge[] = [];
		const price = formatPrice?.(model.price);
		if (price) badges.push({ label: price, tone: 'neutral', mono: true });
		return {
			value: model.id,
			label: model.id,
			...(model.description ? { description: model.description } : {}),
			badges
		};
	});
}
