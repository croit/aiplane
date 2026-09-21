export interface LiveBackend {
	healthy: boolean;
	enabled: boolean;
	auth_failed: boolean;
	inflight: number;
	max_inflight: number;
	models: string[];
	withheld: string[];
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

export interface Backend {
	name: string;
	base_url: string;
	api_key_env: string | null;
	api_key_env_set: boolean;
	has_stored_key: boolean;
	weight: number;
	max_inflight: number;
	health_path: string;
	probe_models: boolean;
	supports_edit: boolean;
	enabled: boolean;
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
	compliance_gdpr: boolean;
	compliance_nda: boolean;
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

export interface PendingChange {
	code: 'pool_added' | 'pool_removed' | 'pool_kind' | 'pool_strategy' | 'backend_joins' | 'backend_leaves' | 'backend_url' | 'backend_limits' | 'backend_health_path';
	pool?: string;
	backend?: string;
	from?: string;
	to?: string;
	weight?: number;
	inflight?: number;
}

export interface Topology {
	pools: Pool[];
	backends: Backend[];
	fallbacks: Record<string, string>;
	usage_last_hour: Record<string, number[]>;
	dirty: number;
	pool_kinds: string[];
	pool_strategies: string[];
	groups?: string[];
	fallback_kinds: string[];
	all_models?: string[];
	coverage?: Record<string, Coverage[]>;
	pending_changes: PendingChange[];
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
