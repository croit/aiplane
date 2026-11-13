import type { OpenRouterEndpoint, OpenRouterModel } from './upstreams.ts';

/** The two orders a table header toggles between. */
export type SortDirection = 'asc' | 'desc';

export interface Sort<Key extends string = string> {
	key: Key;
	direction: SortDirection;
}

export type ModelSortKey = 'model' | 'in' | 'out' | 'context';
export type ProviderSortKey =
	| 'provider'
	| 'in'
	| 'out'
	| 'cache'
	| 'context'
	| 'throughput'
	| 'latency'
	| 'uptime';

/** Clicking the active column flips it; a new column starts ascending. */
export function nextSort<Key extends string>(sort: Sort<Key>, key: Key): Sort<Key> {
	if (sort.key !== key) return { key, direction: 'asc' };
	return { key, direction: sort.direction === 'asc' ? 'desc' : 'asc' };
}

/** What the model catalog opens with: the cheapest output tokens first. */
export const defaultModelSort = (): Sort<ModelSortKey> => ({ key: 'out', direction: 'asc' });

/**
 * What a model's provider table opens with. Duration-priced transcription
 * puts its whole cost on the input side and always reports `0` output, so
 * there the cheapest input is the same question.
 */
export const defaultProviderSort = (
	unit: 'token' | 'second' = 'token'
): Sort<ProviderSortKey> =>
	unit === 'second' ? { key: 'in', direction: 'asc' } : { key: 'out', direction: 'asc' };

type Cell = string | number | null;

/** Missing values (`—`) sort last whichever way the column runs. */
function compare(left: Cell, right: Cell, direction: SortDirection): number {
	if (left === null && right === null) return 0;
	if (left === null) return 1;
	if (right === null) return -1;
	const factor = direction === 'asc' ? 1 : -1;
	if (typeof left === 'number' && typeof right === 'number') return (left - right) * factor;
	return String(left).localeCompare(String(right)) * factor;
}

function sortRows<T>(rows: readonly T[], sort: Sort, value: (row: T) => Cell, tie: (row: T) => string): T[] {
	return [...rows].sort((left, right) => compare(value(left), value(right), sort.direction) || tie(left).localeCompare(tie(right)));
}

const modelValues: Record<ModelSortKey, (model: Pick<OpenRouterModel, 'id' | 'prompt_price' | 'completion_price' | 'context_length'>) => Cell> = {
	model: (model) => model.id,
	in: (model) => model.prompt_price,
	out: (model) => model.completion_price,
	context: (model) => model.context_length
};

export function sortModels<T extends Pick<OpenRouterModel, 'id' | 'prompt_price' | 'completion_price' | 'context_length'>>(
	models: readonly T[],
	sort: Sort<ModelSortKey>
): T[] {
	const value = modelValues[sort.key] ?? modelValues.out;
	return sortRows(models, sort, value, (model) => model.id);
}

type ProviderRow = Pick<
	OpenRouterEndpoint,
	'provider' | 'prompt_price' | 'completion_price' | 'cache_read_price' | 'context_length' | 'throughput_p50' | 'latency_p50' | 'uptime_30m'
>;

const providerValues: Record<ProviderSortKey, (endpoint: ProviderRow) => Cell> = {
	provider: (endpoint) => endpoint.provider,
	in: (endpoint) => endpoint.prompt_price,
	out: (endpoint) => endpoint.completion_price,
	cache: (endpoint) => endpoint.cache_read_price,
	context: (endpoint) => endpoint.context_length,
	throughput: (endpoint) => endpoint.throughput_p50,
	latency: (endpoint) => endpoint.latency_p50,
	uptime: (endpoint) => endpoint.uptime_30m
};

export function sortEndpoints<T extends ProviderRow>(
	endpoints: readonly T[],
	sort: Sort<ProviderSortKey>
): T[] {
	const value = providerValues[sort.key] ?? providerValues.out;
	return sortRows(endpoints, sort, value, (endpoint) => endpoint.provider);
}
