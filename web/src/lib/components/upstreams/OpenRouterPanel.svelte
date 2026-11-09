<script lang="ts">
	import { onMount } from 'svelte';
	import { adminJson, adminPost } from '#lib/admin-client.js';
	import { t, n } from '#lib/i18n.svelte.js';
	import type { Backend, OpenRouterModel, OpenRouterSelection, OpenRouterView, Pool } from '#lib/upstreams.js';

	interface Props {
		backends: Backend[];
		pools: Pool[];
		onChanged: () => void | Promise<void>;
	}

	let { backends, pools: topologyPools, onChanged }: Props = $props();

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
	let poolName = $state('');
	let view = $state<OpenRouterView | null>(null);
	let draft = $state<Record<string, Draft>>({});
	let search = $state('');
	let maxPrice = $state<number | null>(null);
	let sort = $state<'price' | 'name'>('price');
	let loading = $state(false);
	let saving = $state(false);
	let refreshing = $state(false);
	let error = $state<string | null>(null);
	let saved = $state(false);

	let openrouterBackends = $derived(backends.filter((backend) => backend.provider === 'openrouter'));
	let pools = $derived(view?.pools ?? []);
	let models = $derived(view?.catalog ?? []);
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
	let selectedCount = $derived(Object.values(draft).filter((entry) => entry.selected).length);

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

	function resetDraft(next: OpenRouterView) {
		const nextDraft: Record<string, Draft> = {};
		for (const model of next.catalog) nextDraft[model.id] = draftFor(model);
		draft = nextDraft;
	}

	async function load() {
		if (!backendName) return;
		loading = true;
		error = null;
		try {
			const query = new URLSearchParams({ backend: backendName });
			if (poolName) query.set('pool', poolName);
			const next = await adminJson<OpenRouterView>(
				`/api/v0/admin/upstreams/openrouter?${query.toString()}`
			);
			view = next;
			poolName = next.pool || next.pools[0]?.name || '';
			resetDraft(next);
		} catch (err) {
			error = String(err);
		} finally {
			loading = false;
		}
	}

	async function refreshPrices() {
		if (!backendName) return;
		refreshing = true;
		error = null;
		try {
			await adminPost('/api/v0/admin/upstreams/openrouter/refresh', { backend: backendName });
			await load();
			await onChanged();
		} catch (err) {
			error = String(err);
		} finally {
			refreshing = false;
		}
	}

	async function save() {
		if (!backendName || !poolName) return;
		const selections: OpenRouterSelection[] = Object.entries(draft)
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
		saving = true;
		error = null;
		saved = false;
		try {
			await adminPost('/api/v0/admin/upstreams/openrouter/select', {
				backend: backendName,
				pool: poolName,
				models: selections
			});
			saved = true;
			await load();
			await onChanged();
		} catch (err) {
			error = String(err);
		} finally {
			saving = false;
		}
	}

	function selectAll(value: boolean) {
		for (const model of filtered) {
			const entry = draft[model.id];
			if (entry) entry.selected = value;
		}
	}

	function price(value: number | null): string {
		return value === null ? '—' : `$${value.toFixed(4)}`;
	}

	function throughput(value: number | null): string {
		return value === null ? '—' : `${n(Math.round(value))} tok/s`;
	}

	onMount(() => {
		void load();
	});

	$effect(() => {
		// A pool added or removed in the topology editor changes which model
		// lists this backend can offer; reload the panel without a page reload.
		const signature = topologyPools
			.filter((pool) => pool.backends.includes(backendName))
			.map((pool) => `${pool.name}:${pool.kind}`)
			.sort()
			.join(',');
		if (!view) return;
		const shown = view.pools
			.map((pool) => `${pool.name}:${pool.kind}`)
			.sort()
			.join(',');
		if (signature !== shown) void load();
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
							<select class="select select-sm" bind:value={backendName} onchange={() => void load()}>
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

			{#if view?.key}
				<div class="flex flex-wrap items-center gap-2 text-sm">
					<span class="badge badge-success badge-sm">{t('openrouter-connected')}</span>
					{#if view.key.label}<span class="font-mono text-xs">{t('openrouter-key-label')}: {view.key.label}</span>{/if}
					{#if view.key.limit_remaining !== null}
						<span class="text-xs text-base-content/70">
							{t('openrouter-credits')}: {n(view.key.limit_remaining)} / {n(view.key.limit ?? 0)}
						</span>
					{:else if view.key.usage !== null}
						<span class="text-xs text-base-content/70">{t('openrouter-credits')}: {n(view.key.usage)}</span>
					{/if}
				</div>
			{/if}

			{#if view?.error}
				<div class="alert alert-warning py-2 text-sm" role="alert"><span>{t('openrouter-error', { message: view.error })}</span></div>
			{/if}
			{#if error}
				<div class="alert alert-error py-2 text-sm" role="alert"><span>{error}</span></div>
			{/if}
			{#if saved}
				<div class="alert alert-success py-2 text-sm" role="status"><span>{t('openrouter-saved')}</span></div>
			{/if}

			{#if pools.length === 0}
				<div class="alert py-2 text-sm"><span>{t('openrouter-pool-empty')}</span></div>
			{:else}
				{@const currentPool = pools.find((pool) => pool.name === poolName)}
				{#if currentPool && currentPool.other_backends.length > 0}
					<div class="alert alert-warning py-2 text-sm" role="status">
						<span>{t('openrouter-mixed-pool', { backends: currentPool.other_backends.join(', ') })}</span>
					</div>
				{/if}
				<div class="flex flex-wrap items-end gap-3">
					<label class="flex flex-col gap-1">
						<span class="text-xs text-base-content/70">{t('openrouter-pool')}</span>
						<select class="select select-sm" bind:value={poolName} onchange={() => void load()}>
							{#each pools as pool (pool.name)}
								<option value={pool.name}>{pool.name} · {pool.kind}</option>
							{/each}
						</select>
					</label>
					<label class="flex flex-col gap-1">
						<span class="text-xs text-base-content/70">{t('openrouter-search')}</span>
						<input class="input input-sm w-56" bind:value={search} placeholder="deepseek, claude, …" />
					</label>
					<label class="flex flex-col gap-1">
						<span class="text-xs text-base-content/70">{t('openrouter-max-price')}</span>
						<input class="input input-sm w-28 font-mono" type="number" min="0" step="0.01" bind:value={maxPrice} placeholder="—" />
					</label>
					<label class="flex flex-col gap-1">
						<span class="text-xs text-base-content/70">Sort</span>
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

				{#if loading}
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
									{@const entry = draft[model.id]}
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
					<button class="btn btn-primary btn-sm" type="button" disabled={saving || loading} onclick={() => void save()} data-testid="openrouter-save">
						{saving ? '…' : t('openrouter-save')}
					</button>
				</div>
			{/if}
		</div>
	</section>
{/if}
