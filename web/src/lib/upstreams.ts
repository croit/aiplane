import type { BackendPriority, PriorityMapping } from './priority.js';
export interface LiveBackend {
	detected_priority: PriorityMapping | null;
	healthy: boolean;
	enabled: boolean;
	auth_failed: boolean;
	inflight: number;
	max_inflight: number;
	models: string[];
	pool: string | null;
	/** What kind of server this turned out to be. Never shown as a setting. */
	profile: BackendProfileName;
	detected_version: string | null;
	/** Requests the server said it runs at once; null = it did not say. */
	detected_max_parallel: number | null;
	/**
	 * When this backend was last identified (RFC3339), or null if never.
	 *
	 * Identification runs at process start and on every topology apply, unlike
	 * the context reading beside it, which the health probe refreshes every
	 * tick — so how long ago it ran is worth knowing.
	 */
	detected_at: string | null;
}

export type BackendProfileName = 'generic' | 'vllm' | 'ollama' | 'llamacpp' | 'sglang';

/**
 * Whether the configured in-flight ceiling promises more concurrency than the
 * server said it has.
 *
 * Only llama.cpp says — it reports a slot count. Ollama, which has the more
 * dangerous default (one request per model, queueing up to 512 rather than
 * rejecting, so the picker sees free slots and back-pressure quietly stops
 * meaning anything), reports nothing, and this returns null for it. The badge
 * is therefore a help where the answer is known, not a guard against the case
 * that motivated wanting one.
 */
export function parallelismMismatch(
	detected: number | null | undefined,
	configured: number
): number | null {
	if (!detected) return null;
	return detected < configured ? detected : null;
}

export interface BackendAlias {
	alias: string;
	target: string | null;
}

export type ProviderKind = 'openai_compatible' | 'openrouter';

export interface Backend {
	priority: BackendPriority;
	/** `openrouter` reads the account catalog and injects the price policy. */
	provider: ProviderKind;
	name: string;
	base_url: string;
	api_key_env: string | null;
	api_key_env_set: boolean;
	has_stored_key: boolean;
	api_key_hint: string | null;
	weight: number;
	max_inflight: number;
	health_path: string;
	supports_edit: boolean;
	enabled: boolean;
	/** Extra headers sent with every request to this backend; empty = none. */
	extra_headers: Record<string, string>;
	models: string[];
	aliases: BackendAlias[];
	live: LiveBackend | null;
}

export interface Voice {
	lang: string;
	voice: string;
}

export interface Pool {
	name: string;
	kind: string;
	strategy: string;
	fallback_offline: string | null;
	enforce_limits: boolean;
	sort_order: number;
	allowed_groups: string[];
	backends: string[];
	live_backends?: Record<string, LiveBackend>;
	models: string[];
	voices: Voice[];
	offer_voices: string[];
}

export interface Coverage {
	name: string;
	serving: number;
	total: number;
}

export interface BackendTestResult {
	outcome: 'success' | 'warning' | 'error';
	code: 'ok' | 'ok_no_models' | 'auth_failed' | 'http_error' | 'unreachable' | 'timeout' | 'base_url_required';
	status?: number;
	url?: string;
	detail?: string;
	timeout_seconds?: number;
	model_count?: number;
	models: string[];
	key_source?: { kind: 'typed' | 'stored' | 'env' | 'env_unset' | 'none'; name?: string };
	/** Identification runs alongside the reachability test — see `profile.rs`. */
	profile?: BackendProfileName;
	detected_version?: string | null;
	detected_max_parallel?: number | null;
	/**
	 * The tightest context window this server reports, if it reports one.
	 * Same name and meaning as `AdminModel.detected_context_window`.
	 */
	detected_context_window?: number | null;
}

const BACKEND_TEST_FAILURES = ['auth_failed', 'http_error', 'unreachable', 'timeout', 'base_url_required'] as const;

/**
 * A failed connection test is a refusal (400/502/504) whose envelope carries
 * the test's `code` and details; this turns it back into the panel's result.
 * `null` for any other refusal, which the caller reports as an error.
 */
export function backendTestFailure(body: string | undefined): BackendTestResult | null {
	if (!body) return null;
	let error: unknown;
	try {
		error = (JSON.parse(body) as { error?: unknown }).error;
	} catch {
		return null;
	}
	if (!error || typeof error !== 'object') return null;
	const { code, status, url, detail, timeout_seconds, key_source } = error as Record<string, unknown>;
	if (!BACKEND_TEST_FAILURES.includes(code as (typeof BACKEND_TEST_FAILURES)[number])) return null;
	return {
		outcome: 'error',
		code: code as BackendTestResult['code'],
		...(typeof status === 'number' ? { status } : {}),
		...(typeof url === 'string' ? { url } : {}),
		...(typeof detail === 'string' ? { detail } : {}),
		...(typeof timeout_seconds === 'number' ? { timeout_seconds } : {}),
		...(key_source && typeof key_source === 'object' ? { key_source: key_source as BackendTestResult['key_source'] } : {}),
		models: []
	};
}

export interface Topology {
	pools: Pool[];
	backends: Backend[];
	fallbacks: Record<string, string>;
	usage_last_hour: Record<string, number[]>;
	pool_kinds: string[];
	pool_strategies: string[];
	groups: string[];
	fallback_kinds: string[];
	all_models?: string[];
	coverage?: Record<string, Coverage[]>;
}

export function backendAssignments(pools: Pool[], backends: Backend[]) {
	const indexed = new Map(backends.map((backend) => [backend.name, backend]));
	const assigned = new Set(pools.flatMap((pool) => pool.backends));
	return {
		byPool: new Map(
			pools.map((pool) => [
				pool.name,
				pool.backends.flatMap((name) => {
					const backend = indexed.get(name);
					return backend ? [backend] : [];
				})
			])
		),
		unassigned: backends.filter((backend) => !assigned.has(backend.name))
	};
}

export function activityCounts(buckets: number[]) {
	const tail = (count: number) => buckets.slice(-count).reduce((sum, value) => sum + value, 0);
	return { m15: tail(3), m30: tail(6), m60: tail(12) };
}

function resolvedAliases(backend: Backend): string[] {
	const served = backend.live?.models ?? [];
	return backend.aliases.flatMap(({ alias, target }) => {
		if (target !== null) return served.includes(target) ? [alias] : [];
		return served.length === 1 ? [alias] : [];
	});
}

export function poolCoverage(pool: Pool, backends: Backend[]): Coverage[] {
	const members = pool.backends.flatMap((name) => {
		const backend = backends.find((candidate) => candidate.name === name);
		return backend ? [backend] : [];
	});
	const available = members.map((backend) => {
		if (!backend.live?.healthy || !backend.live.enabled) return new Set<string>();
		return new Set([...(backend.live.models ?? []), ...resolvedAliases(backend)]);
	});
	const names = new Set(pool.models);
	for (const models of available) for (const model of models) names.add(model);
	return [...names]
		.sort((left, right) => left.localeCompare(right))
		.map((name) => ({
			name,
			serving: available.filter((models) => models.has(name)).length,
			total: members.length
		}));
}

export interface BoardGroup {
	pool: Pool | null;
	backends: Backend[];
}

/** One board row as the filter sees it: the backend and its live state. */
export interface BoardRow {
	backend: Backend;
	live: LiveBackend | null;
}

/**
 * Whether a backend row matches the board's free-text filter: its name, its
 * endpoint, every model it advertises, or any alias it answers to.
 */
export function boardRowMatches(row: BoardRow, query: string): boolean {
	const needle = query.trim().toLowerCase();
	if (!needle) return true;
	return [
		row.backend.name,
		row.backend.base_url,
		...(row.live?.models ?? []),
		...row.backend.aliases.map((alias) => alias.alias)
	].some((value) => value.toLowerCase().includes(needle));
}

/**
 * Whether a band survives the query: its pool name matches, or one of its rows
 * does. Kept separate from [`boardRowMatches`] so typing a pool name shows the
 * whole band, including the empty pool the operator may be hunting for.
 */
export function boardPoolMatches(pool: string | null, rows: BoardRow[], query: string): boolean {
	const needle = query.trim().toLowerCase();
	if (!needle) return true;
	if ((pool ?? '').toLowerCase().includes(needle)) return true;
	return rows.some((row) => boardRowMatches(row, query));
}

/**
 * The board's grouping: every pool in its declared order, then the backends
 * no pool holds. An empty pool still gets a band — it is a topology row that
 * needs editing, not one that can be hidden.
 */
export function boardGroups(pools: Pool[], backends: Backend[]): BoardGroup[] {
	const byName = new Map(backends.map((backend) => [backend.name, backend]));
	const assigned = new Set(pools.flatMap((pool) => pool.backends));
	const groups: BoardGroup[] = [...pools]
		.sort((left, right) => left.sort_order - right.sort_order || left.name.localeCompare(right.name))
		.map((pool) => ({
			pool,
			backends: pool.backends.flatMap((name) => {
				const backend = byName.get(name);
				return backend ? [backend] : [];
			})
		}));
	const unassigned = backends
		.filter((backend) => !assigned.has(backend.name))
		.sort((left, right) => left.name.localeCompare(right.name));
	if (unassigned.length) groups.push({ pool: null, backends: unassigned });
	return groups;
}

/**
 * The live view of a backend *within* one pool. A backend's status event
 * carries the pool the gateway currently routes it through; when that is a
 * different pool, its own counters say nothing about this one.
 */
export function liveForPool(pool: Pool, backend: Backend): LiveBackend | null {
	if (backend.live?.pool === pool.name) return backend.live;
	return pool.live_backends?.[backend.name] ?? backend.live;
}

export interface PoolProblem {
	key: string;
	params: Record<string, string>;
}

/**
 * Every way a pool can be configured but not work, as message keys. The board
 * renders these inline under the pool's band; the copy lives in the catalogs.
 */
export function poolProblems(pool: Pool, backends: Backend[], coverage: Coverage[]): PoolProblem[] {
	const members = pool.backends.flatMap((name) => {
		const backend = backends.find((candidate) => candidate.name === name);
		return backend ? [backend] : [];
	});
	const resolved = members.map((backend) => ({ backend, live: liveForPool(pool, backend) }));
	const withLive = resolved.flatMap(({ backend, live }) => (live ? [{ backend, live }] : []));
	const problems: PoolProblem[] = [];
	if (pool.backends.length === 0) problems.push({ key: 'upstreams-problem-no-backends', params: {} });
	if (withLive.length && withLive.every(({ live }) => !live.enabled)) {
		problems.push({ key: 'upstreams-problem-all-drained', params: {} });
	} else if (withLive.length && withLive.every(({ live }) => !live.healthy || !live.enabled)) {
		problems.push({ key: 'upstreams-problem-all-down', params: {} });
	}
	const rejected = withLive.filter(({ live }) => live.auth_failed).map(({ backend }) => backend.name);
	if (rejected.length) problems.push({ key: 'upstreams-problem-auth', params: { backends: rejected.join(', ') } });
	// An OpenRouter backend with an empty allowlist is unconfigured, not
	// broken — its selection happens in the account panel.
	const modelless = withLive
		.filter(({ backend, live }) => {
			if (live.models.length > 0) return false;
			return !(backend.provider === 'openrouter' && pool.models.length === 0);
		})
		.map(({ backend }) => backend.name);
	if (modelless.length) problems.push({ key: 'upstreams-problem-no-models', params: { backends: modelless.join(', ') } });
	const brokenAliases = members.flatMap((backend) => {
		const live = liveForPool(pool, backend);
		return backend.aliases
			.filter(({ target }) => (target ? !live?.models.includes(target) : live?.models.length !== 1))
			.map(({ alias }) => `${backend.name}:${alias}`);
	});
	if (brokenAliases.length) {
		problems.push({ key: 'upstreams-problem-broken-aliases', params: { aliases: brokenAliases.join(', ') } });
	}
	const partial = coverage
		.filter(({ serving, total }) => serving > 0 && serving < total)
		.map(({ name, serving, total }) => `${name} (${serving}/${total})`);
	if (partial.length) {
		problems.push({ key: 'upstreams-problem-partial-coverage', params: { models: partial.join(', ') } });
	}
	const unserved = coverage.filter(({ serving }) => serving === 0).map(({ name }) => name);
	if (unserved.length && withLive.length) {
		problems.push({ key: 'upstreams-problem-unserved-allowlist', params: { models: unserved.join(', ') } });
	}
	const missing = pool.backends.filter((name) => !backends.some((backend) => backend.name === name));
	if (missing.length) problems.push({ key: 'upstreams-problem-missing-backends', params: { backends: missing.join(', ') } });
	return problems;
}

export function splitList(value: string): string[] {
	return value.split(',').map((item) => item.trim()).filter(Boolean);
}

export function splitLines(value: string): string[] {
	return value.split('\n').map((item) => item.trim()).filter(Boolean);
}

export function parseAliases(value: string): BackendAlias[] {
	return splitLines(value).map((line) => {
		const separator = line.indexOf('=');
		return separator < 0
			? { alias: line, target: null }
			: { alias: line.slice(0, separator).trim(), target: line.slice(separator + 1).trim() || null };
	});
}

/**
 * The editor's extra-headers field: one `name: value` per line, the same
 * grammar as the aliases field. A line without a separator is dropped — the
 * server rejects it on save, and the editor should not ship it in the first
 * place. The value keeps everything after the first `:`, so a header whose
 * value contains a colon (a URL) survives.
 */
export function parseHeaders(value: string): Record<string, string> {
	const headers: Record<string, string> = {};
	for (const line of splitLines(value)) {
		const separator = line.indexOf(':');
		if (separator <= 0) continue;
		const name = line.slice(0, separator).trim();
		const headerValue = line.slice(separator + 1).trim();
		if (!name || !headerValue) continue;
		headers[name] = headerValue;
	}
	return headers;
}

export function formatHeaders(headers: Record<string, string>): string {
	return Object.entries(headers)
		.map(([name, value]) => `${name}: ${value}`)
		.join('\n');
}

export function completeAliasLine(value: string, cursor: number, model: string) {
	const lineStart = value.lastIndexOf('\n', Math.max(0, cursor - 1)) + 1;
	const nextNewline = value.indexOf('\n', cursor);
	const lineEnd = nextNewline < 0 ? value.length : nextNewline;
	const alias = value.slice(lineStart, lineEnd).split('=')[0].trim();
	const replacement = alias ? `${alias}=${model}` : model;
	return {
		value: value.slice(0, lineStart) + replacement + value.slice(lineEnd),
		cursor: lineStart + replacement.length
	};
}

export function parseVoices(value: string): Voice[] {
	return splitLines(value).flatMap((line) => {
		const separator = line.indexOf('=');
		return separator < 1
			? []
			: [{ lang: line.slice(0, separator).trim(), voice: line.slice(separator + 1).trim() }];
	});
}

/**
 * Natural ("human") order for model ids.
 *
 * Plain lexicographic sorting puts `gpt-5.10` before `gpt-5.2` and
 * `llama-3.2-1b` before `llama-3.2-3b`'s siblings in ways nobody scanning a
 * list expects. A numeric collator compares digit runs as numbers, which is
 * how a person reads a version. Case-insensitive too, so `Qwen` and `qwen`
 * sort together rather than in two blocks.
 *
 * The server already sorts these — that is what stops the list reshuffling on
 * every status tick — but it sorts by byte order, which is the stable choice
 * for a payload and the wrong one for a human. Display order is decided here.
 */
const naturalCollator = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' });

export function naturalSort(values: readonly string[]): string[] {
	return [...values].sort((a, b) => naturalCollator.compare(a, b));
}

/** One model from the OpenRouter account catalog, with its policy. */
export interface OpenRouterModel {
	id: string;
	name: string;
	description: string;
	context_length: number | null;
	max_completion_tokens: number | null;
	input_modalities: string[];
	output_modalities: string[];
	supports_tools: boolean;
	supports_structured_output: boolean;
	/** The voices this TTS model speaks; empty for non-speech models. */
	supported_voices: string[];
	/** The model's list price in USD per 1M tokens. */
	prompt_price: number;
	completion_price: number;
	selected: boolean;
	available: boolean;
	policy: {
		max_prompt_price: number;
		max_completion_price: number;
		strategy: string;
		min_throughput: number | null;
	} | null;
	cheapest_provider: string | null;
	cheapest_prompt_price: number | null;
	cheapest_completion_price: number | null;
	endpoints: OpenRouterEndpoint[];
}

export interface OpenRouterEndpoint {
	provider: string;
	prompt_price: number;
	completion_price: number;
	/** USD per 1M tokens; null when the provider does not report one. */
	cache_read_price: number | null;
	throughput_p50: number | null;
	latency_p50: number | null;
	uptime_30m: number | null;
	quantization: string | null;
	context_length: number | null;
}

/** Whether a provider fits both ceilings, in USD per 1M tokens. */
export function endpointFits(
	endpoint: OpenRouterEndpoint,
	maxPrompt: number,
	maxCompletion: number
): boolean {
	return endpoint.prompt_price <= maxPrompt && endpoint.completion_price <= maxCompletion;
}

/** The provider with the lowest input price, then output; null for no providers. */
export function cheapestEndpoint(
	endpoints: readonly OpenRouterEndpoint[]
): OpenRouterEndpoint | null {
	let best: OpenRouterEndpoint | null = null;
	for (const endpoint of endpoints) {
		if (
			!best ||
			endpoint.prompt_price < best.prompt_price ||
			(endpoint.prompt_price === best.prompt_price &&
				endpoint.completion_price < best.completion_price)
		) {
			best = endpoint;
		}
	}
	return best;
}

export interface OpenRouterPool {
	name: string;
	kind: string;
	modality: string | null;
	/** Other backends in this pool; a pool has one model list. */
	other_backends: string[];
}

export interface OpenRouterView {
	backend: string;
	connected: boolean;
	key: {
		label: string | null;
		usage: number | null;
		limit: number | null;
		limit_remaining: number | null;
		is_free_tier: boolean;
	} | null;
	pools: OpenRouterPool[];
	pool: string;
	catalog: OpenRouterModel[];
	error: string | null;
}

export interface OpenRouterSelection {
	model: string;
	max_prompt_price: number;
	max_completion_price: number;
	strategy: string;
	min_throughput: number | null;
}
