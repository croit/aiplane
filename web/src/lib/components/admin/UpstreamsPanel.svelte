<script lang="ts">
	import { onMount } from 'svelte';
	import { adminDelete, adminJson, adminPost, adminPut } from '#lib/admin-client.js';
	import { t } from '#lib/i18n.svelte.js';
	import { naturalSort, type Backend, type LiveBackend, type Pool, type Topology } from '#lib/upstreams.js';
	import BackendEditor from '#lib/components/upstreams/BackendEditor.svelte';
	import EditModal from '#lib/components/EditModal.svelte';
	import PoolEditor from '#lib/components/upstreams/PoolEditor.svelte';
	import OpenRouterEditor from '#lib/components/upstreams/OpenRouterEditor.svelte';
	import OpenRouterAccountPanel from '#lib/components/upstreams/OpenRouterAccountPanel.svelte';
	import ConcourseBoard from '#lib/components/upstreams/ConcourseBoard.svelte';
	import SearchableSelect from '#lib/components/SearchableSelect.svelte';

	interface StatusEvent extends LiveBackend { name: string; usage: number[]; }

	let data = $state<Topology | null>(null);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let addingPool = $state(false);
	let addingBackend = $state(false);
	let addingOpenRouter = $state(false);
	let editingBackend = $state<Backend | null>(null);
	let editModalOpen = $state(false);
	let editingPool = $state<Pool | null>(null);
	let poolModalOpen = $state(false);
	let togglingBackend = $state<string | null>(null);

	async function toggleEnabled(backend: Backend) {
		togglingBackend = backend.name;
		try {
			await adminPost(`/api/v0/admin/backends/${encodeURIComponent(backend.name)}/enabled`, { enabled: !backend.enabled });
			await refresh();
		} catch (caught) {
			notice = String(caught);
		} finally {
			togglingBackend = null;
		}
	}

	async function refresh() {
		try { data = await adminJson<Topology>('/api/v0/admin/upstreams'); error = null; }
		catch (caught) { error = String(caught); }
	}

	async function saveFallback(kind: string, model: string) {
		try {
			await adminPut('/api/v0/admin/upstreams/fallback', { kind, model });
			if (data) data = { ...data, fallbacks: { ...data.fallbacks, [kind]: model } };
		} catch (caught) { notice = String(caught); }
	}

	function editPool(pool: Pool) {
		editingPool = pool;
		poolModalOpen = true;
	}

	async function deletePool(pool: Pool) {
		await adminDelete(`/api/v0/admin/pools/${encodeURIComponent(pool.name)}`);
		poolModalOpen = false;
		editingPool = null;
		await refresh();
	}

	/** Models a pool's own backends advertise — the picklist when pinning its list. */
	function knownModels(pool: Pool | null): string[] {
		if (!pool || !data) return [];
		const names = new Set(pool.models);
		for (const name of pool.backends) {
			const backend = data.backends.find((candidate) => candidate.name === name);
			for (const model of backend?.live?.models ?? []) names.add(model);
		}
		return naturalSort([...names]);
	}

	function openRouterAccount() {
		const target = document.getElementById('openrouter-account');
		if (!target) return;
		target.scrollIntoView({ behavior: 'smooth', block: 'start' });
		target.querySelector<HTMLElement>('[data-openrouter-heading]')?.focus({ preventScroll: true });
	}

	function applyStatus(status: StatusEvent) {
		if (!data) return;
		const previous = data.backends.find((backend) => backend.name === status.name)?.live ?? null;
		const live = status.pool === null ? null : {
			healthy: status.healthy, enabled: status.enabled, auth_failed: status.auth_failed,
			inflight: status.inflight, max_inflight: status.max_inflight, models: status.models,
			pool: status.pool, profile: previous?.profile ?? 'generic',
			detected_version: previous?.detected_version ?? null,
			detected_max_parallel: previous?.detected_max_parallel ?? null,
			detected_at: previous?.detected_at ?? null,
			detected_priority: previous?.detected_priority ?? null
		};
		data = { ...data, backends: data.backends.map((backend) => backend.name === status.name ? { ...backend, live } : backend), usage_last_hour: { ...data.usage_last_hour, [status.name]: status.usage } };
	}

	onMount(() => {
		void refresh();
		const events = new EventSource('/api/v0/admin/upstreams/events');
		events.addEventListener('status', (event) => applyStatus(JSON.parse(event.data) as StatusEvent));
		return () => events.close();
	});
</script>

{#if error}<div class="alert alert-error" role="alert"><span>{error}</span></div>{/if}
{#if notice}<div class="alert alert-warning" role="status"><span>{notice}</span></div>{/if}

{#if data}
	<header class="flex flex-wrap items-start justify-between gap-3"><div class="max-w-3xl"><h2 class="text-xl font-semibold">{t('upstreams-heading')}</h2><p class="mt-1 text-sm text-base-content/70">{t('upstreams-description')}</p></div><div class="flex flex-wrap gap-2"><button class="btn btn-primary btn-sm" type="button" onclick={() => (addingBackend = true)}>+ {t('upstreams-add-backend')}</button><button class="btn btn-sm" type="button" onclick={() => (addingPool = true)}>+ {t('upstreams-add-pool')}</button><button class="btn btn-sm" type="button" onclick={() => (addingOpenRouter = true)}>+ {t('openrouter-add-heading')}</button></div></header>
	<div class="flex flex-col gap-4">
		{#if data.pools.length === 0 && data.backends.length === 0}
			<div class="alert"><span>{t('upstreams-empty')}</span></div>
		{:else}
			<ConcourseBoard
				backends={data.backends}
				pools={data.pools}
				usage={data.usage_last_hour}
				coverage={data.coverage ?? {}}
				onEdit={(backend) => { editingBackend = backend; editModalOpen = true; }}
				onEditPool={editPool}
				onToggleEnabled={(backend) => void toggleEnabled(backend)}
				onOpenRouterAccount={openRouterAccount}
				toggling={togglingBackend}
			/>
		{/if}
		<OpenRouterAccountPanel backends={data.backends} pools={data.pools} poolKinds={data.pool_kinds} onChanged={refresh} />
		<section class="card card-border"><div class="card-body gap-3"><h3 class="card-title text-base">{t('pools-fallbacks-heading')}</h3><p class="text-sm text-base-content/70">{t('pools-fallbacks-description')}</p><div class="grid grid-cols-1 gap-3 sm:grid-cols-2">{#each data.fallback_kinds as kind (kind)}<label class="flex flex-col gap-1"><span class="font-mono text-xs text-base-content/70">{kind}</span><SearchableSelect options={[{ value: '', label: t('admin-cap-no-fallback') }, ...(data.all_models ?? []).map((model) => ({ value: model, label: model }))]} value={data.fallbacks[kind] ?? ''} onchange={(value) => void saveFallback(kind, value)} ariaLabel={kind} size="sm" class="w-full" /></label>{/each}</div></div></section>
	</div>
	<EditModal bind:open={editModalOpen} wide footer="none" title={t('upstreams-edit-backend')} cancellabel={t('upstreams-cancel')}>{#if editingBackend}{@const editTarget = editingBackend}<BackendEditor backend={editTarget} currentPool={data.pools.find((pool) => pool.backends.includes(editTarget.name))?.name ?? null} pools={data.pools} poolKinds={data.pool_kinds} existingNames={data.backends.map((backend) => backend.name)} onSaved={refresh} onCancel={() => { editModalOpen = false; editingBackend = null; }} />{/if}</EditModal>
	<EditModal bind:open={addingPool} wide footer="none" title={t('pools-add-heading')} cancellabel={t('upstreams-cancel')}><PoolEditor backends={data.backends} knownModels={[]} poolKinds={data.pool_kinds} poolStrategies={data.pool_strategies} groups={data.groups} existingNames={data.pools.map((pool) => pool.name)} sortOrder={Math.max(-1, ...data.pools.map((pool) => pool.sort_order)) + 1} onSaved={() => { addingPool = false; return refresh(); }} onCancel={() => (addingPool = false)} /></EditModal>
	<EditModal bind:open={addingBackend} wide footer="none" title={t('backends-add-heading')} cancellabel={t('upstreams-cancel')}><BackendEditor pools={data.pools} poolKinds={data.pool_kinds} existingNames={data.backends.map((backend) => backend.name)} onSaved={() => { addingBackend = false; return refresh(); }} onCancel={() => (addingBackend = false)} /></EditModal>
	<EditModal bind:open={addingOpenRouter} wide footer="none" title={t('openrouter-add-heading')} cancellabel={t('upstreams-cancel')}><OpenRouterEditor pools={data.pools} poolKinds={data.pool_kinds} existingNames={data.backends.map((backend) => backend.name)} onSaved={() => { addingOpenRouter = false; return refresh(); }} onCancel={() => (addingOpenRouter = false)} /></EditModal>
	<EditModal bind:open={poolModalOpen} wide footer="none" title={t('upstreams-edit-pool')} description={editingPool?.name ?? null} cancellabel={t('upstreams-cancel')}>{#if editingPool}{@const poolTarget = editingPool}<PoolEditor pool={poolTarget} backends={data.backends} knownModels={knownModels(poolTarget)} poolKinds={data.pool_kinds} poolStrategies={data.pool_strategies} groups={data.groups} existingNames={data.pools.map((pool) => pool.name)} sortOrder={poolTarget.sort_order} onDelete={() => deletePool(poolTarget)} onSaved={refresh} onCancel={() => { poolModalOpen = false; editingPool = null; }} />{/if}</EditModal>
{/if}
