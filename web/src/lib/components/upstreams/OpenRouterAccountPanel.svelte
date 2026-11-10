<script lang="ts">
	import { onMount } from 'svelte';
	import { adminJson, adminPost, adminPut } from '#lib/admin-client.js';
	import { t, n } from '#lib/i18n.svelte.js';
	import type { Backend, OpenRouterEndpoint, OpenRouterModel, OpenRouterSelection, OpenRouterView, Pool } from '#lib/upstreams.js';
	import { cheapestEndpoint, endpointFits } from '#lib/upstreams.js';

	interface Props {
		backends: Backend[];
		pools: Pool[];
		poolKinds: string[];
		onChanged: () => void | Promise<void>;
	}

	let { backends, pools: topologyPools, poolKinds, onChanged }: Props = $props();

	interface Draft {
		selected: boolean;
		prompt: number;
		completion: number;
		strategy: string;
		min: number | null;
	}

	let backendName = $state(
		backends.find((backend) => backend.provider === 'openrouter')?.name ?? ''
	);
	let openrouterBackends = $derived(backends.filter((backend) => backend.provider === 'openrouter'));
	let kinds = $derived(poolKinds.filter((kind) => kind !== 'ocr'));

	/** The kinds this account serves: the pools it is a member of, by kind. */
	let servedKinds = $derived.by(() => {
		const result: Record<string, { pool: Pool; selected: number }> = {};
		for (const pool of topologyPools) {
			if (pool.backends.includes(backendName)) {
				result[pool.kind] = { pool, selected: pool.models.length };
			}
		}
		return result;
	});

	let expanded = $state<string | null>(null);
	let views = $state<Record<string, OpenRouterView | null>>({});
	let drafts = $state<Record<string, Record<string, Draft>>>({});
	let search = $state('');
	let maxPrice = $state<number | null>(null);
	let sort = $state<'price' | 'name'>('price');
	let loadingKind = $state<string | null>(null);
	let savingKind = $state<string | null>(null);
	let refreshing = $state(false);
	let togglingKind = $state<string | null>(null);
	let error = $state<string | null>(null);
	let savedKind = $state<string | null>(null);

	// Provider prices for one model: persisted ones exist only for models that
	// were already selected, so an unselected row reads them on demand.
	interface ProviderState {
		status: 'loading' | 'ok' | 'error';
		endpoints: OpenRouterEndpoint[];
		error?: string;
	}
	let openModel = $state<string | null>(null);
	let providerStates = $state<Record<string, ProviderState>>({});

	// Key change
	let changingKey = $state(false);
	let newKey = $state('');
	let savingKey = $state(false);
	let keySaved = $state(false);

	let currentView = $derived(expanded ? views[expanded] ?? null : null);
	let models = $derived(currentView?.catalog ?? []);
	let filtered = $derived.by(() => {
		const needle = search.trim().toLowerCase();
		const rows = models.filter(
			(model) =>
				(!needle ||
					model.id.toLowerCase().includes(needle) ||
					model.name.toLowerCase().includes(needle) ||
					model.description.toLowerCase().includes(needle)) &&
				(maxPrice === null ||
					!Number.isFinite(Number(maxPrice)) ||
					model.prompt_price <= Number(maxPrice))
		);
		return [...rows].sort((a, b) =>
			sort === 'name'
				? a.id.localeCompare(b.id)
				: a.prompt_price - b.prompt_price || a.id.localeCompare(b.id)
		);
	});
	let selectedCount = $derived(
		expanded ? Object.values(drafts[expanded] ?? {}).filter((entry) => entry.selected).length : 0
	);

	function poolNameOf(kind: string) {
		return `openrouter-${kind}`;
	}

	function draftFor(model: OpenRouterModel): Draft {
		const prompt = model.policy?.max_prompt_price ?? Number(model.prompt_price.toFixed(4));
		const completion =
			model.policy?.max_completion_price ?? Number(model.completion_price.toFixed(4));
		return {
			selected: model.selected,
			prompt,
			completion,
			strategy: model.policy?.strategy ?? 'balanced',
			min: model.policy?.min_throughput ?? null
		};
	}

	/**
	 * Draft guard: the baseline a kind loaded with. A kind is dirty when its
	 * draft diverges from that baseline — disabling a dirty kind, or leaving
	 * the page with a dirty kind, asks before discarding.
	 */
	let baselines = $state<Record<string, Record<string, Draft>>>({});

	function draftDiverges(kind: string, id: string): boolean {
		const draft = (drafts[kind] ?? {})[id];
		const base = (baselines[kind] ?? {})[id];
		if (!draft) return false;
		if (!base) return draft.selected;
		return (
			draft.selected !== base.selected ||
			draft.prompt !== base.prompt ||
			draft.completion !== base.completion ||
			draft.strategy !== base.strategy ||
			draft.min !== base.min
		);
	}

	function changedCount(kind: string): number {
		return Object.keys(drafts[kind] ?? {}).filter((id) => draftDiverges(kind, id)).length;
	}

	function isDirty(kind: string): boolean {
		return changedCount(kind) > 0;
	}

	let dirtyKinds = $derived(kinds.filter((kind) => isDirty(kind)));

	$effect(() => {
		dirtyKinds;
		const handler = (event: BeforeUnloadEvent) => {
			if (dirtyKinds.length > 0) {
				event.preventDefault();
				event.returnValue = '';
			}
		};
		window.addEventListener('beforeunload', handler);
		return () => window.removeEventListener('beforeunload', handler);
	});

	function resetDraft(next: OpenRouterView, kind: string) {
		const nextDraft: Record<string, Draft> = {};
		for (const model of next.catalog) nextDraft[model.id] = draftFor(model);
		drafts = { ...drafts, [kind]: nextDraft };
		// Deep copy per entry: the baseline must not share the draft's objects,
		// or a bound mutation (bind:checked) would move both and the diff
		// would never be seen.
		const base: Record<string, Draft> = {};
		for (const [id, entry] of Object.entries(nextDraft)) base[id] = { ...entry };
		baselines = { ...baselines, [kind]: base };
	}

	async function loadKind(kind: string) {
		if (!backendName) return;
		loadingKind = kind;
		error = null;
		try {
			const query = new URLSearchParams({ backend: backendName, pool: poolNameOf(kind) });
			const next = await adminJson<OpenRouterView>(
				`/api/v0/admin/upstreams/openrouter?${query.toString()}`
			);
			views = { ...views, [kind]: next };
			resetDraft(next, kind);
		} catch (err) {
			error = String(err);
		} finally {
			loadingKind = null;
		}
	}

	function toggleKind(kind: string) {
		if (expanded === kind) {
			expanded = null;
			return;
		}
		expanded = kind;
		if (!views[kind]) void loadKind(kind);
	}

	async function setKind(kind: string, enabled: boolean) {
		if (!backendName) return;
		if (!enabled && isDirty(kind) && !confirm(t('openrouter-dirty-confirm', { kind }))) return;
		togglingKind = kind;
		error = null;
		savedKind = null;
		try {
			await adminPost('/api/v0/admin/upstreams/openrouter/kind', { backend: backendName, kind, enabled });
			expanded = null;
			views = { ...views, [kind]: null };
			drafts = { ...drafts, [kind]: {} };
			baselines = { ...baselines, [kind]: {} };
			await onChanged();
		} catch (err) {
			error = String(err);
		} finally {
			togglingKind = null;
		}
	}

	async function refreshPrices() {
		if (!backendName) return;
		if (expanded && isDirty(expanded) && !confirm(t('openrouter-dirty-confirm', { kind: expanded }))) return;
		refreshing = true;
		error = null;
		try {
			await adminPost('/api/v0/admin/upstreams/openrouter/refresh', { backend: backendName });
			if (expanded && views[expanded]) await loadKind(expanded);
			await onChanged();
		} catch (err) {
			error = String(err);
		} finally {
			refreshing = false;
		}
	}

	async function saveKind() {
		if (!backendName || !expanded) return;
		const kind = expanded;
		const selections: OpenRouterSelection[] = Object.entries(drafts[kind] ?? {})
			.filter(([, entry]) => entry.selected)
			.map(([model, entry]) => ({
				model,
				max_prompt_price: Number(entry.prompt),
				max_completion_price: Number(entry.completion),
				strategy: entry.strategy,
				min_throughput:
					entry.strategy === 'cheapest' && entry.min !== null && entry.min !== undefined && `${entry.min}` !== ''
						? Number(entry.min)
						: null
			}));
		if (selections.length === 0) {
			error = t('openrouter-select-model-first');
			return;
		}
		const invalid = selections.find(
			(selection) =>
				!Number.isFinite(selection.max_prompt_price) ||
				!Number.isFinite(selection.max_completion_price) ||
				selection.max_prompt_price < 0 ||
				selection.max_completion_price < 0
		);
		if (invalid) {
			error = t('openrouter-ceiling-required');
			return;
		}
		savingKind = kind;
		error = null;
		savedKind = null;
		try {
			await adminPost('/api/v0/admin/upstreams/openrouter/select', {
				backend: backendName,
				pool: poolNameOf(kind),
				models: selections
			});
			savedKind = kind;
			await loadKind(kind);
			await onChanged();
		} catch (err) {
			error = String(err);
		} finally {
			savingKind = null;
		}
	}

	function selectAll(value: boolean) {
		if (!expanded) return;
		const entries = drafts[expanded] ?? {};
		for (const model of filtered) {
			const entry = entries[model.id];
			if (entry) entry.selected = value;
		}
		drafts = { ...drafts, [expanded]: entries };
	}

	async function saveKey() {
		const backend = openrouterBackends.find((candidate) => candidate.name === backendName);
		if (!backend || !newKey.trim()) return;
		savingKey = true;
		error = null;
		keySaved = false;
		try {
			await adminPut('/api/v0/admin/backends', {
				name: backend.name,
				provider: backend.provider,
				base_url: backend.base_url,
				api_key_env: backend.api_key_env ?? '',
				api_key: newKey.trim(),
				priority: backend.priority,
				weight: backend.weight,
				max_inflight: backend.max_inflight,
				health_path: backend.health_path,
				supports_edit: backend.supports_edit,
				extra_headers: backend.extra_headers,
				models: backend.models,
				aliases: backend.aliases,
				pools: topologyPools.filter((pool) => pool.backends.includes(backend.name)).map((pool) => pool.name),
				new_pools: [],
				overwrite: true
			});
			changingKey = false;
			newKey = '';
			keySaved = true;
			await onChanged();
		} catch (err) {
			error = String(err);
		} finally {
			savingKey = false;
		}
	}

	function price(value: number | null): string {
		return value === null ? '—' : `$${value.toFixed(4)}`;
	}

	function throughput(value: number | null): string {
		return value === null ? '—' : `${n(Math.round(value))} tok/s`;
	}

	/** The provider rows a row can show: fetched on demand, else the stored ones. */
	function endpointsFor(model: OpenRouterModel): OpenRouterEndpoint[] {
		const loaded = providerStates[model.id];
		if (loaded?.status === 'ok') return loaded.endpoints;
		return model.endpoints;
	}

	async function toggleModel(model: OpenRouterModel) {
		if (openModel === model.id) {
			openModel = null;
			return;
		}
		openModel = model.id;
		const cached = providerStates[model.id];
		// A failed read retries on the next expand; a stored or successful one
		// stays.
		if ((cached && cached.status !== 'error') || model.endpoints.length > 0) return;
		providerStates = { ...providerStates, [model.id]: { status: 'loading', endpoints: [] } };
		try {
			const query = new URLSearchParams({ backend: backendName, model: model.id });
			const next = await adminJson<{ endpoints: OpenRouterEndpoint[] }>(
				`/api/v0/admin/upstreams/openrouter/endpoints?${query.toString()}`
			);
			providerStates = {
				...providerStates,
				[model.id]: { status: 'ok', endpoints: next.endpoints }
			};
		} catch (err) {
			providerStates = {
				...providerStates,
				[model.id]: { status: 'error', endpoints: [], error: String(err) }
			};
		}
	}

	/** How many known providers fit the ceilings currently in the draft. */
	function fittingCount(model: OpenRouterModel, entry: Draft): number {
		const prompt = Number(entry.prompt);
		const completion = Number(entry.completion);
		return endpointsFor(model).filter((endpoint) =>
			endpointFits(endpoint, prompt, completion)
		).length;
	}

	/** Ceilings carry four decimals; round up so the cheapest provider fits. */
	function ceiling(value: number): number {
		return Math.ceil(value * 10_000) / 10_000;
	}

	function useCheapest(entry: Draft, endpoint: OpenRouterEndpoint) {
		entry.prompt = ceiling(endpoint.prompt_price);
		entry.completion = ceiling(endpoint.completion_price);
	}

	onMount(() => {
		// Open the first served kind, so the panel lands on actionable content.
		const first = kinds.find((kind) => servedKinds[kind]);
		if (first) void toggleKind(first);
	});

	$effect(() => {
		// The served set changed (kind toggled, topology edited): the catalog
		// of a kind that stopped being served is stale; drop it.
		for (const kind of Object.keys(views)) {
			if (!servedKinds[kind] && views[kind]) views = { ...views, [kind]: null };
		}
		if (expanded && !servedKinds[expanded]) expanded = null;
	});
</script>

{#if openrouterBackends.length > 0}
	<section class="card card-border bg-base-100" data-testid="openrouter-panel" id="openrouter-account">
		<div class="card-body gap-4">
			<header class="flex flex-wrap items-start justify-between gap-3">
				<div class="max-w-3xl">
					<h3 class="card-title text-base" tabindex="-1" data-openrouter-heading>{t('openrouter-heading')}</h3>
					<p class="text-sm text-base-content/70">{t('openrouter-description')}</p>
				</div>
				<div class="flex flex-wrap items-end gap-2">
					{#if openrouterBackends.length > 1}
						<label class="flex flex-col gap-1">
							<span class="text-xs text-base-content/70">{t('openrouter-backend')}</span>
							<select class="select select-sm" bind:value={backendName} onchange={() => { expanded = null; views = {}; drafts = {}; baselines = {}; openModel = null; providerStates = {}; }}>
								{#each openrouterBackends as backend (backend.name)}
									<option value={backend.name}>{backend.name}</option>
								{/each}
							</select>
						</label>
					{/if}
					<button class="btn btn-sm" type="button" disabled={refreshing} onclick={() => void refreshPrices()}>
						{refreshing ? '…' : t('openrouter-refresh')}
					</button>
				</div>
			</header>

			{#key backendName}
				{@const backend = openrouterBackends.find((candidate) => candidate.name === backendName)}
				{#if backend}
					<div class="flex flex-wrap items-center gap-3 text-sm">
						<span class="font-medium">{backend.name}</span>
						{#if currentView?.key}
							<span class="badge badge-success badge-sm">{t('openrouter-connected')}</span>
							{#if currentView.key.label}<span class="font-mono text-xs">{t('openrouter-key-label')}: {currentView.key.label}</span>{/if}
							{#if currentView.key.limit_remaining !== null}
								<span class="text-xs text-base-content/70">
									{t('openrouter-credits')}: {n(currentView.key.limit_remaining)} / {n(currentView.key.limit ?? 0)}
								</span>
							{:else if currentView.key.usage !== null}
								<span class="text-xs text-base-content/70">{t('openrouter-credits')}: {n(currentView.key.usage)}</span>
							{/if}
						{/if}
						<span class="text-xs text-base-content/50">{t('backends-field-api-key-stored', { hint: backend.api_key_hint ?? '—' })}</span>
						{#if changingKey}
							<div class="flex items-center gap-2">
								<input class="input input-sm font-mono w-72" type="password" autocomplete="off" bind:value={newKey} placeholder={t('openrouter-key-placeholder')} />
								<button class="btn btn-primary btn-xs" type="button" disabled={savingKey || !newKey.trim()} onclick={() => void saveKey()}>
									{savingKey ? '…' : t('openrouter-key-save')}
								</button>
								<button class="btn btn-ghost btn-xs" type="button" onclick={() => { changingKey = false; newKey = ''; }}>{t('upstreams-cancel')}</button>
							</div>
						{:else}
							<button class="btn btn-ghost btn-xs" type="button" onclick={() => (changingKey = true)}>{t('openrouter-key-change')}</button>
						{/if}
						{#if keySaved}<span class="badge badge-success badge-xs" role="status">{t('openrouter-key-saved')}</span>{/if}
					</div>
				{/if}
			{/key}

			{#if currentView?.error}
				<div class="alert alert-warning py-2 text-sm" role="alert"><span>{t('openrouter-error', { message: currentView.error })}</span></div>
			{/if}
			{#if error}
				<div class="alert alert-error py-2 text-sm" role="alert"><span>{error}</span></div>
			{/if}

			<!-- Kinds -->
			<div class="divider text-xs uppercase">{t('openrouter-kinds')}</div>
			<div class="flex flex-col gap-1">
				{#each kinds as kind (kind)}
					{@const served = servedKinds[kind]}
					<div class="flex items-center gap-3 {expanded === kind ? 'rounded-box border border-base-300 p-2' : 'px-2 py-1 hover:bg-base-200/50'}">
						<input
							class="checkbox checkbox-sm"
							type="checkbox"
							checked={!!served}
							disabled={togglingKind === kind}
							aria-label={kind}
							onchange={(event) => void setKind(kind, event.currentTarget.checked)}
						/>
						<button class="flex flex-1 items-center gap-3 text-left" type="button" onclick={() => toggleKind(kind)} disabled={!served}>
							<span class="text-sm font-medium">{kind}</span>
							{#if served}
								<span class="text-xs text-base-content/60">
									{t('openrouter-kind-selected-count', { count: served.selected })}
									{#if views[kind]} · {t('openrouter-kind-catalog-count', { count: views[kind]?.catalog.length ?? 0 })}{/if}
								</span>
							{:else}
								<span class="text-xs text-base-content/50">{t('openrouter-kind-not-enabled')}</span>
							{/if}
						</button>
						{#if served}
							<span class="text-base-content/50">{expanded === kind ? '−' : '+'}</span>
						{/if}
					</div>
					{#if expanded === kind && served}
						<div class="mx-2 mb-2 flex flex-col gap-3 border-t border-base-300 pt-3">
							{#if savedKind === kind}
								<div class="alert alert-success py-2 text-sm" role="status"><span>{t('openrouter-saved')}</span></div>
							{/if}
							<div class="flex flex-wrap items-end gap-3">
								<label class="flex flex-col gap-1">
									<span class="text-xs text-base-content/70">{t('openrouter-search')}</span>
									<input class="input input-sm w-56" bind:value={search} placeholder="deepseek, claude, …" />
								</label>
								<label class="flex flex-col gap-1">
									<span class="text-xs text-base-content/70">{t('openrouter-max-price')}</span>
									<input class="input input-sm w-28 font-mono" type="number" min="0" step="0.01" bind:value={maxPrice} placeholder="—" />
								</label>
								<label class="flex flex-col gap-1">
									<span class="text-xs text-base-content/70">{t('openrouter-sort')}</span>
									<select class="select select-sm" bind:value={sort}>
										<option value="price">{t('openrouter-price')}</option>
										<option value="name">{t('openrouter-model')}</option>
									</select>
								</label>
								<div class="flex items-center gap-2">
									<button class="btn btn-xs" type="button" onclick={() => selectAll(true)}>{t('openrouter-select-all')}</button>
									<button class="btn btn-xs" type="button" onclick={() => selectAll(false)}>{t('openrouter-deselect-all')}</button>
									<span class="text-xs text-base-content/60">{t('openrouter-selected-count', { count: selectedCount })}</span>
								</div>
							</div>

							{#if loadingKind === kind}
								<div class="flex justify-center py-6"><span class="loading loading-spinner"></span></div>
							{:else if filtered.length === 0}
								<div class="alert py-2 text-sm"><span>{t('openrouter-no-models')}</span></div>
							{:else}
								<div class="overflow-x-auto">
									<table class="table table-sm" data-testid="openrouter-catalog">
										<thead>
											<tr>
												<th class="w-8"></th>
												<th>{t('openrouter-model')}</th>
												<th class="text-end">{t('openrouter-price')}</th>
												<th class="text-end">{t('openrouter-context')}</th>
												<th>{t('openrouter-ceiling-prompt')}</th>
												<th>{t('openrouter-ceiling-completion')}</th>
												<th>{t('openrouter-strategy')}</th>
												<th>{t('openrouter-cheapest-provider')}</th>
												<th></th>
											</tr>
										</thead>
										<tbody>
											{#each filtered as model (model.id)}
												{@const entry = (drafts[kind] ?? {})[model.id]}
												{#if entry}
													<tr class={entry.selected ? '' : 'opacity-60'} data-testid={`openrouter-model-${model.id}`}>
														<td>
															<input
																class="checkbox checkbox-sm"
																type="checkbox"
																bind:checked={entry.selected}
																aria-label={model.id}
															/>
														</td>
														<td class="max-w-md">
															<button class="flex w-full items-start gap-1 text-left" type="button" aria-expanded={openModel === model.id} onclick={() => void toggleModel(model)}>
																<span class="mt-0.5 text-xs text-base-content/50">{openModel === model.id ? '▾' : '▸'}</span>
																<span class="min-w-0">
																	<span class="block font-mono text-xs break-all">{model.id}</span>
																	<span class="flex items-center gap-2 text-xs text-base-content/60">
																		{model.name}
																		{#if model.supported_voices.length > 0}
																			<span class="badge badge-outline badge-xs">{model.supported_voices.length} {t('openrouter-voices')}</span>
																		{/if}
																	</span>
																	{#if model.description}<span class="mt-1 line-clamp-2 text-xs text-base-content/50">{model.description}</span>{/if}
																</span>
															</button>
														</td>
														<td class="text-end font-mono text-xs tabular-nums">
															<div>{price(model.prompt_price)}</div>
															<div class="text-base-content/50">{price(model.completion_price)}</div>
														</td>
														<td class="text-end font-mono text-xs tabular-nums">{model.context_length ? n(model.context_length) : '—'}</td>
														<td>
															<input class="input input-xs w-24 font-mono" type="number" min="0" step="0.0001" bind:value={entry.prompt} disabled={!entry.selected} aria-label={`${model.id} input ceiling`} />
														</td>
														<td>
															<input class="input input-xs w-24 font-mono" type="number" min="0" step="0.0001" bind:value={entry.completion} disabled={!entry.selected} aria-label={`${model.id} output ceiling`} />
														</td>
														<td>
															<div class="flex flex-col gap-1">
																<select class="select select-xs" bind:value={entry.strategy} disabled={!entry.selected} aria-label={`${model.id} strategy`}>
																	<option value="cheapest">{t('openrouter-strategy-cheapest')}</option>
																	<option value="fastest">{t('openrouter-strategy-fastest')}</option>
																	<option value="balanced">{t('openrouter-strategy-balanced')}</option>
																</select>
																{#if entry.strategy === 'cheapest'}
																	<input class="input input-xs w-24 font-mono" type="number" min="0" step="1" placeholder={t('openrouter-min-throughput')} bind:value={entry.min} disabled={!entry.selected} aria-label={`${model.id} min throughput`} />
																{/if}
															</div>
														</td>
														<td class="text-xs">
															{#if model.cheapest_provider}
																<div class="font-mono">{model.cheapest_provider}</div>
																<div class="text-base-content/50">{price(model.cheapest_prompt_price)} / {price(model.cheapest_completion_price)}</div>
															{:else}
																<div class="text-base-content/50">{model.prompt_price.toFixed(4)} / {model.completion_price.toFixed(4)}</div>
															{/if}
														</td>
														<td>
															{#if entry.selected}
																{#if endpointsFor(model).length === 0}
																	<span class="badge badge-ghost badge-xs" title={t('openrouter-price-unknown-hint')}>{t('openrouter-price-unknown')}</span>
																{:else}
																	{@const fitting = fittingCount(model, entry)}
																	{#if fitting === 0}
																		<span class="badge badge-warning badge-xs">{t('openrouter-over-ceiling')}</span>
																	{:else}
																		<span class="badge badge-success badge-xs">{t('openrouter-providers-fit-short', { fit: fitting, total: endpointsFor(model).length })}</span>
																	{/if}
																{/if}
															{/if}
														</td>
													</tr>
													{#if openModel === model.id}
														{@const providers = endpointsFor(model)}
														{@const state = providerStates[model.id] ?? null}
														{@const cheapest = cheapestEndpoint(providers)}
														<tr class="bg-base-200/40">
															<td colspan="9" class="p-3">
																{#if state?.status === 'loading'}
																	<span class="loading loading-spinner loading-sm"></span>
																{:else if state?.status === 'error'}
																	<p class="text-xs text-error">{t('openrouter-provider-error', { message: state.error ?? '' })}</p>
																{:else if providers.length === 0}
																	<p class="text-xs text-base-content/60">{t('openrouter-provider-none')}</p>
																{:else}
																	<div class="overflow-x-auto">
																		<table class="table table-xs">
																			<thead>
																				<tr>
																					<th>{t('openrouter-provider')}</th>
																					<th class="text-end">{t('openrouter-provider-input')}</th>
																					<th class="text-end">{t('openrouter-provider-output')}</th>
																					<th class="text-end">{t('openrouter-provider-cache-read')}</th>
																					<th class="text-end">{t('openrouter-context')}</th>
																					<th class="text-end">{t('openrouter-provider-throughput')}</th>
																					<th class="text-end">{t('openrouter-provider-latency')}</th>
																					<th class="text-end">{t('openrouter-provider-uptime')}</th>
																					<th>{t('openrouter-provider-limit')}</th>
																				</tr>
																			</thead>
																			<tbody>
																				{#each providers as endpoint, index (endpoint.provider + index)}
																					{@const fits = endpointFits(endpoint, Number(entry.prompt), Number(entry.completion))}
																					<tr class={fits ? '' : 'opacity-60'}>
																						<td class="font-mono text-xs">
																							{endpoint.provider}
																							{#if endpoint.quantization}<span class="badge badge-ghost badge-xs ml-1">{endpoint.quantization}</span>{/if}
																						</td>
																						<td class="text-end font-mono text-xs tabular-nums">{price(endpoint.prompt_price)}</td>
																						<td class="text-end font-mono text-xs tabular-nums">{price(endpoint.completion_price)}</td>
																						<td class="text-end font-mono text-xs tabular-nums">{price(endpoint.cache_read_price)}</td>
																						<td class="text-end font-mono text-xs tabular-nums">{endpoint.context_length !== null ? n(endpoint.context_length) : '—'}</td>
																						<td class="text-end font-mono text-xs tabular-nums">{throughput(endpoint.throughput_p50)}</td>
																						<td class="text-end font-mono text-xs tabular-nums">{endpoint.latency_p50 !== null ? `${endpoint.latency_p50.toFixed(2)} s` : '—'}</td>
																						<td class="text-end font-mono text-xs tabular-nums">{endpoint.uptime_30m !== null ? `${Math.round(endpoint.uptime_30m)} %` : '—'}</td>
																						<td class="text-xs">{fits ? t('openrouter-provider-fits') : t('openrouter-provider-excluded')}</td>
																					</tr>
																				{/each}
																			</tbody>
																		</table>
																	</div>
																	<div class="mt-2 flex flex-wrap items-center gap-2 text-xs">
																		{#if cheapest}
																			<span>{t('openrouter-cheapest', { provider: cheapest.provider, prompt: price(cheapest.prompt_price), completion: price(cheapest.completion_price) })}</span>
																		{/if}
																		<span class="text-base-content/60">{t('openrouter-providers-fit', { fit: fittingCount(model, entry), total: providers.length })}</span>
																		{#if cheapest}
																			<button class="btn btn-xs" type="button" onclick={() => useCheapest(entry, cheapest)}>{t('openrouter-use-cheapest')}</button>
																		{/if}
																		<a class="btn btn-ghost btn-xs" href={`https://openrouter.ai/${model.id}`} target="_blank" rel="noreferrer">{t('openrouter-compare')} ↗</a>
																	</div>
																{/if}
															</td>
														</tr>
													{/if}
												{/if}
											{/each}
										</tbody>
									</table>
								</div>
							{/if}

							<div class="flex items-center justify-end gap-2">
								<button class="btn btn-primary btn-sm" type="button" disabled={savingKind === kind || loadingKind === kind} onclick={() => void saveKind()}>
									{savingKind === kind ? '…' : (isDirty(kind) ? t('openrouter-save-changed', { count: changedCount(kind) }) : t('openrouter-save'))}
								</button>
							</div>
						</div>
					{/if}
				{/each}
			</div>
		</div>
	</section>
{/if}
