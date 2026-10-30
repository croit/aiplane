/** One model a token can be limited to, as `/api/v0/tokens/details` and
 *  `/api/v0/admin/tokens` list it. An alias is priced as its target. */
export interface TokenModel {
	id: string;
	kind: string;
	alias_of: string | null;
	price: { input: number | null; output: number | null; unit: string } | null;
}

export interface TokenModelFilter {
	query: string;
	free: boolean;
}

export interface TokenModelSummary {
	count: number;
	maxOutputPerMillion: number | null;
}

const KIND_ORDER = ['chat', 'transcription', 'speech', 'embedding', 'rerank', 'image', 'system_one'];

function isFree(model: TokenModel): boolean {
	return !model.price || (!model.price.input && !model.price.output);
}

export function groupTokenModels(models: TokenModel[]): { kind: string; models: TokenModel[] }[] {
	const kinds = [...new Set(models.map((model) => model.kind))];
	const rank = (kind: string) => (KIND_ORDER.includes(kind) ? KIND_ORDER.indexOf(kind) : KIND_ORDER.length);
	kinds.sort((left, right) => rank(left) - rank(right));
	return kinds.map((kind) => ({ kind, models: models.filter((model) => model.kind === kind) }));
}

export function filterTokenModels(models: TokenModel[], filter: TokenModelFilter): TokenModel[] {
	const query = filter.query.trim().toLowerCase();
	return models.filter((model) =>
		(!query || model.id.toLowerCase().includes(query) || (model.alias_of ?? '').toLowerCase().includes(query))
		&& (!filter.free || isFree(model)));
}

/** `selected === null` is an unrestricted token: it reaches every model. */
export function selectionSummary(models: TokenModel[], selected: string[] | null): TokenModelSummary {
	const chosen = selected === null ? models : models.filter((model) => selected.includes(model.id));
	const tokenPrices = chosen
		.filter((model) => model.price?.unit === 'tokens' && model.price.output)
		.map((model) => model.price?.output ?? 0);
	return {
		count: chosen.length,
		maxOutputPerMillion: tokenPrices.length ? Math.max(...tokenPrices) : null
	};
}
