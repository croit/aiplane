<script lang="ts">
	import { onMount } from 'svelte';
	import { adminJson, adminPost, adminPut } from '$lib/admin-client';
	import { t, n } from '$lib/i18n.svelte';
	import { backendAssignments, type LiveBackend, type PendingChange, type Topology } from '$lib/upstreams';
	import BackendCard from '$lib/components/upstreams/BackendCard.svelte';
	import BackendEditor from '$lib/components/upstreams/BackendEditor.svelte';
	import PoolCard from '$lib/components/upstreams/PoolCard.svelte';
	import EditModal from '$lib/components/EditModal.svelte';
	import PoolEditor from '$lib/components/upstreams/PoolEditor.svelte';
	import SearchableSelect from '$lib/components/SearchableSelect.svelte';

	interface StatusEvent extends LiveBackend {
		name: string;
		dirty: number;
		usage: number[];
	}

	let data = $state<Topology | null>(null);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	// One flag each, because `EditModal.open` is bindable: Escape and the
	// backdrop write back through it, and a derived expression would swallow that.
	let addingPool = $state(false);
	let addingBackend = $state(false);
	let assignments = $derived(backendAssignments(data?.pools ?? [], data?.backends ?? []));
	let sortedPools = $derived(
		(data?.pools ?? []).slice().sort((left, right) => left.sort_order - right.sort_order || left.name.localeCompare(right.name))
	);

	async function refresh() {
		try {
			data = await adminJson<Topology>('/api/v0/admin/upstreams');
			error = null;
		} catch (err) {
			error = String(err);
		}
	}

	async function apply() {
		try {
			await adminPost('/api/v0/admin/upstreams/reload');
			notice = t('upstreams-applied');
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	async function saveFallback(kind: string, model: string) {
		try {
			await adminPut('/api/v0/admin/upstreams/fallback', { kind, model });
			if (data) data = { ...data, fallbacks: { ...data.fallbacks, [kind]: model } };
		} catch (err) {
			notice = String(err);
		}
	}

	function applyStatus(status: StatusEvent) {
		if (!data) return;
		const live = status.pool === null ? null : {
			healthy: status.healthy,
			enabled: status.enabled,
			auth_failed: status.auth_failed,
			inflight: status.inflight,
			max_inflight: status.max_inflight,
			models: status.models,
			withheld: status.withheld,
			pool: status.pool
		};
		data = {
			...data,
			dirty: status.dirty,
			backends: data.backends.map((backend) => backend.name === status.name ? { ...backend, live } : backend),
			usage_last_hour: { ...data.usage_last_hour, [status.name]: status.usage }
		};
	}

	function pendingChangeMessage(change: PendingChange) {
		switch (change.code) {
			case 'pool_added': return t('upstreams-diff-pool-added', { pool: change.pool ?? '' });
			case 'pool_removed': return t('upstreams-diff-pool-removed', { pool: change.pool ?? '' });
			case 'pool_kind': return t('upstreams-diff-pool-kind', { pool: change.pool ?? '', from: change.from ?? '', to: change.to ?? '' });
			case 'pool_strategy': return t('upstreams-diff-pool-strategy', { pool: change.pool ?? '', from: change.from ?? '', to: change.to ?? '' });
			case 'backend_joins': return t('upstreams-diff-backend-joins', { backend: change.backend ?? '', pool: change.pool ?? '' });
			case 'backend_leaves': return t('upstreams-diff-backend-leaves', { backend: change.backend ?? '', pool: change.pool ?? '' });
			case 'backend_url': return t('upstreams-diff-backend-url', { backend: change.backend ?? '', from: change.from ?? '', to: change.to ?? '' });
			case 'backend_limits': return t('upstreams-diff-backend-limits', { backend: change.backend ?? '', weight: change.weight ?? 1, inflight: change.inflight ?? 1 });
			case 'backend_health_path': return t('upstreams-diff-backend-health-path', { backend: change.backend ?? '', to: change.to ?? '' });
		}
	}

	onMount(() => {
		void refresh();
		const events = new EventSource('/api/v0/admin/upstreams/events');
		events.addEventListener('status', (event) => applyStatus(JSON.parse(event.data) as StatusEvent));
		return () => events.close();
	});
</script>

{#if error}<div class="alert alert-error mb-4" role="alert"><span>{error}</span></div>{/if}
{#if notice}<div class="alert alert-warning mb-4" role="status"><span>{notice}</span></div>{/if}

{#if data}
	{#if data.dirty > 0}
		<div class="alert alert-warning sticky top-3 z-30 mb-4 items-start shadow" role="status">
			<div class="flex-1">
				<strong>{n(data.dirty)} {t('upstreams-apply-count')}</strong>
				<span> {t('upstreams-apply-note')}</span>
				{#if data.pending_changes.length}
					<details class="mt-1 text-sm">
						<summary class="cursor-pointer select-none">{t('upstreams-apply-diff-summary')}</summary>
						<ul class="mt-1 list-disc ps-5">
							{#each data.pending_changes as change}<li class="font-mono text-xs">{pendingChangeMessage(change)}</li>{/each}
						</ul>
					</details>
				{/if}
			</div>
			<button class="btn btn-sm" type="button" onclick={() => void apply()}>{t('backends-apply-changes')}</button>
		</div>
	{/if}

	<header class="mb-4 flex flex-wrap items-start justify-between gap-3">
		<div class="max-w-3xl">
			<h1 class="text-2xl font-semibold">{t('upstreams-heading')}</h1>
			<p class="mt-1 text-sm text-base-content/70">{t('upstreams-description')}</p>
		</div>
		<div class="flex gap-2">
			<button class="btn btn-sm" type="button" onclick={() => (addingPool = true)}>+ {t('upstreams-add-pool')}</button>
			<button class="btn btn-sm" type="button" onclick={() => (addingBackend = true)}>+ {t('upstreams-add-backend')}</button>
		</div>
	</header>

	<!-- The same dialog the cards' Edit buttons open. Inline, these two pushed
	     the whole topology down the page to make room for a form nobody had
	     asked to see yet. -->
	<EditModal
		bind:open={addingPool}
		wide
		footer="none"
		title={t('pools-add-heading')}
		cancellabel={t('upstreams-cancel')}
	>
		<PoolEditor backends={data.backends} poolKinds={data.pool_kinds} poolStrategies={data.pool_strategies} existingNames={data.pools.map((pool) => pool.name)} sortOrder={Math.max(-1, ...data.pools.map((pool) => pool.sort_order)) + 1} onSaved={() => { addingPool = false; return refresh(); }} onCancel={() => (addingPool = false)} />
	</EditModal>

	<EditModal
		bind:open={addingBackend}
		wide
		footer="none"
		title={t('backends-add-heading')}
		cancellabel={t('upstreams-cancel')}
	>
		<BackendEditor pools={data.pools} existingNames={data.backends.map((backend) => backend.name)} onSaved={() => { addingBackend = false; return refresh(); }} onCancel={() => (addingBackend = false)} />
	</EditModal>

	<div class="flex flex-col gap-4">
		{#each sortedPools as pool (pool.name)}
			<PoolCard {pool} backends={assignments.byPool.get(pool.name) ?? []} allBackends={data.backends} pools={data.pools} usage={data.usage_last_hour} coverage={data.coverage?.[pool.name]} poolKinds={data.pool_kinds} poolStrategies={data.pool_strategies} onChanged={refresh} />
		{/each}

		{#if assignments.unassigned.length}
			<section class="card card-border bg-base-100">
				<div class="card-body gap-3">
					<header>
						<h2 class="card-title text-base">{t('upstreams-unassigned-heading')}</h2>
						<p class="text-sm text-base-content/70">{t('upstreams-unassigned-description')}</p>
					</header>
					{#each assignments.unassigned as backend (backend.name)}
						<BackendCard {backend} pools={data.pools} backendNames={data.backends.map((candidate) => candidate.name)} usage={data.usage_last_hour[backend.name] ?? []} onChanged={refresh} />
					{/each}
				</div>
			</section>
		{/if}

		{#if data.pools.length === 0 && data.backends.length === 0}
			<div class="alert"><span>{t('upstreams-empty')}</span></div>
		{/if}

		<section class="card card-border bg-base-100">
			<div class="card-body gap-3">
				<h2 class="card-title text-base">{t('pools-fallbacks-heading')}</h2>
				<p class="text-sm text-base-content/70">{t('pools-fallbacks-description')}</p>
				<div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
					{#each data.fallback_kinds as kind (kind)}
						<label class="flex flex-col gap-1">
							<span class="font-mono text-xs text-base-content/70">{kind}</span>
							<SearchableSelect options={[{ value: '', label: t('admin-cap-no-fallback') }, ...(data.all_models ?? []).map((model) => ({ value: model, label: model }))]} value={data.fallbacks[kind] ?? ''} onchange={(value) => void saveFallback(kind, value)} ariaLabel={kind} size="sm" class="w-full" />
						</label>
					{/each}
				</div>
			</div>
		</section>
	</div>
{/if}
