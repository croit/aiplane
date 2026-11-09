<script lang="ts">
	import { onMount } from 'svelte';
	import { adminJson, adminPost, adminPut } from '#lib/admin-client.js';
	import { t, n } from '#lib/i18n.svelte.js';
	import type { Backend, OpenRouterModel, OpenRouterSelection, OpenRouterView, Pool } from '#lib/upstreams.js';

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

	function resetDraft(next: OpenRouterView, kind: string) {
		const nextDraft: Record<string, Draft> = {};
		for (const model of next.catalog) nextDraft[model.id] = draftFor(model);
		drafts = { ...drafts, [kind]: nextDraft };
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
		togglingKind = kind;
		error = null;
		savedKind = null;
		try {
			await adminPost('/api/v0/admin/upstreams/openrouter/kind', { backend: backendName, kind, enabled });
			expanded = null;
			views = { ...views, [kind]: null };
			await onChanged();
		} catch (err) {
			error = String(err);
		} finally {
			togglingKind = null;
		}
	}

	async function refreshPrices() {
		if (!backendName) return;
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
	<section class="card card-border bg-base-100" data-testid="openrouter-panel">
		<div class="card-body gap-4">
			<header class="flex flex-wrap items-start justify-between gap-3">
				<div class="max-w-3xl">
					<h3 class="card-title text-base">{t('openrouter-heading')}</h3>
					<p class="text-sm text-base-content/70">{t('openrouter-description')}</p>
				</div>
				<div class="flex flex-wrap items-end gap-2">
					{#if openrouterBackends.length > 1}
						<label class="flex flex-col gap-1">
							<span class="text-xs text-base-content/70">{t('openrouter-backend')}</span>
							<select class="select select-sm" bind:value={backendName} onchange={() => { expanded = null; views = {}; drafts = {}; }}>
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
												<th>{t('openrouter-provider')}</th>
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
															<div class="font-mono text-xs break-all">{model.id}</div>
															<div class="flex items-center gap-2 text-xs text-base-content/60">
																{model.name}
																{#if model.supported_voices.length > 0}
																	<span class="badge badge-outline badge-xs">{model.supported_voices.length} {t('openrouter-voices')}</span>
																{/if}
															</div>
															{#if model.description}<div class="mt-1 line-clamp-2 text-xs text-base-content/50">{model.description}</div>{/if}
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
															{#if entry.selected && !model.available}
																<span class="badge badge-warning badge-xs">{t('openrouter-over-ceiling')}</span>
															{:else if entry.selected}
																<span class="badge badge-success badge-xs">{t('openrouter-available')}</span>
															{/if}
															{#if model.endpoints.length > 0}
																<details class="mt-1">
																	<summary class="cursor-pointer text-xs text-base-content/50">{t('openrouter-endpoints')} ({model.endpoints.length})</summary>
																	<ul class="mt-1 space-y-1">
																		{#each model.endpoints as endpoint (endpoint.provider)}
																			<li class="font-mono text-xs">
																				{endpoint.provider}: {price(endpoint.prompt_price)} / {price(endpoint.completion_price)}
																				<span class="text-base-content/50">· {throughput(endpoint.throughput_p50)} · {endpoint.uptime_30m !== null ? `${endpoint.uptime_30m.toFixed(1)}%` : '—'}</span>
																			</li>
																		{/each}
																	</ul>
																</details>
															{/if}
														</td>
													</tr>
												{/if}
											{/each}
										</tbody>
									</table>
								</div>
							{/if}

							<div class="flex items-center justify-end gap-2">
								<button class="btn btn-primary btn-sm" type="button" disabled={savingKind === kind || loadingKind === kind} onclick={() => void saveKind()}>
									{savingKind === kind ? '…' : t('openrouter-save')}
								</button>
							</div>
						</div>
					{/if}
				{/each}
			</div>
		</div>
	</section>
{/if}
