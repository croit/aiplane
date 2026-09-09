<script lang="ts">
	import { onMount } from 'svelte';
	import { adminJson, adminPut, adminPost, adminDelete } from '$lib/admin-client';
	import { t, n } from '$lib/i18n.svelte';

	interface Live {
		healthy: boolean;
		enabled: boolean;
		auth_failed: boolean;
		inflight: number;
		max_inflight: number;
		models: string[];
		pool: string | null;
	}
	interface Backend {
		name: string;
		base_url: string;
		api_key_env: string | null;
		has_stored_key: boolean;
		weight: number;
		max_inflight: number;
		health_path: string;
		probe_models: boolean;
		enabled: boolean;
		models: string[];
		aliases: { alias: string; target: string | null }[];
		live: Live | null;
	}
	interface Pool {
		name: string;
		kind: string;
		strategy: string;
		backends: string[];
		models: string[];
		allowed_groups: string[];
	}
	interface Topology {
		pools: Pool[];
		backends: Backend[];
		fallbacks: Record<string, string>;
		usage_last_hour: Record<string, number[]>;
		dirty: number;
		/// Served from `PoolKind::ALL` so the picker cannot fall behind the enum.
		pool_kinds: string[];
	}

	let data = $state<Topology | null>(null);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);

	// Backend form.
	let showBackendForm = $state(false);
	let bName = $state('');
	let bUrl = $state('');
	let bKey = $state('');
	let bPool = $state('');
	let bOverwrite = $state(false);

	// Pool form.
	let showPoolForm = $state(false);
	let pName = $state('');
	let pKind = $state('chat');
	let pBackends = $state('');
	let pModels = $state('');
	let pOverwrite = $state(false);

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

	async function saveBackend() {
		notice = null;
		try {
			await adminPut('/api/v0/admin/backends', {
				name: bName,
				base_url: bUrl,
				api_key: bKey,
				pool: bPool === '' ? null : bPool,
				overwrite: bOverwrite
			});
			showBackendForm = false;
			bName = ''; bUrl = ''; bKey = ''; bPool = ''; bOverwrite = false;
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	async function savePool() {
		notice = null;
		try {
			await adminPut('/api/v0/admin/pools', {
				name: pName,
				kind: pKind,
				backends: pBackends.split(',').map((s) => s.trim()).filter(Boolean),
				models: pModels.split(',').map((s) => s.trim()).filter(Boolean),
				overwrite: pOverwrite
			});
			showPoolForm = false;
			pName = ''; pBackends = ''; pModels = ''; pOverwrite = false;
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	async function toggleBackend(name: string, enabled: boolean) {
		try {
			await adminPost(`/api/v0/admin/backends/${encodeURIComponent(name)}/enabled`, { enabled });
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	async function deleteBackend(name: string) {
		if (!confirm(t('backends-delete-confirm', { name }))) return;
		try {
			await adminDelete(`/api/v0/admin/backends/${encodeURIComponent(name)}`);
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	async function deletePool(name: string) {
		if (!confirm(t('pools-delete-confirm', { name }))) return;
		try {
			await adminDelete(`/api/v0/admin/pools/${encodeURIComponent(name)}`);
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	function lastHour(name: string): number {
		return (data?.usage_last_hour[name] ?? []).reduce((a, b) => a + b, 0);
	}

	onMount(refresh);
</script>

{#if error}<div class="alert alert-error mb-4"><span>{error}</span></div>{/if}
{#if notice}<div class="alert alert-warning mb-4"><span>{notice}</span></div>{/if}

{#if data}
	{#if data.dirty > 0}
		<div class="alert alert-warning mb-4 sticky top-0 z-10 shadow">
			<span>{n(data.dirty)} {t('upstreams-apply-count')} {t('upstreams-apply-note')}</span>
			<button class="btn btn-primary btn-sm" onclick={apply}>{t('backends-apply-changes')}</button>
		</div>
	{/if}

	<div class="flex justify-between items-center mb-4 flex-wrap gap-2">
		<h2 class="text-lg font-semibold">{t('pools-heading')}</h2>
		<button class="btn btn-primary btn-sm" onclick={() => (showPoolForm = !showPoolForm)}>
			{t('pools-add-pool')}
		</button>
	</div>

	{#if showPoolForm}
		<div class="card border border-base-300 mb-4">
			<div class="card-body">
				<div class="flex flex-wrap gap-3 items-end">
					<label class="flex flex-col gap-1"><span class="label-text">{t('pools-field-name')}</span>
						<input class="input input-bordered input-sm" bind:value={pName} /></label>
					<label class="flex flex-col gap-1"><span class="label-text">{t('pools-field-kind')}</span>
						<select class="select select-bordered select-sm" bind:value={pKind}>
							<!-- From the server, which reads PoolKind::ALL. A hardcoded list
							     here is how `rerank` went missing from the picker. -->
							{#each data.pool_kinds ?? [] as kind (kind)}
								<option>{kind}</option>
							{/each}
						</select></label>
					<label class="flex flex-col gap-1 flex-1 min-w-48"><span class="label-text">{t('pools-field-backends')}</span>
						<input class="input input-bordered input-sm" bind:value={pBackends} placeholder={data.backends.map((b) => b.name).join(', ')} /></label>
					<label class="flex flex-col gap-1 flex-1 min-w-48"><span class="label-text">{t('pools-field-models')}</span>
						<input class="input input-bordered input-sm" bind:value={pModels} /></label>
					<button class="btn btn-primary btn-sm" onclick={savePool} disabled={!pName.trim()}>
						{t('pools-save-pool')}
					</button>
				</div>
			</div>
		</div>
	{/if}

	<div class="grid md:grid-cols-2 gap-3 mb-8">
		{#each data.pools as pool (pool.name)}
			<div class="card border border-base-300">
				<div class="card-body py-3">
					<div class="flex items-center gap-2 flex-wrap">
						<span class="font-medium">{pool.name}</span>
						<span class="badge badge-outline badge-sm">{pool.kind}</span>
						<span class="flex-1"></span>
						<button class="btn btn-ghost btn-xs text-error" onclick={() => deletePool(pool.name)}>
							{t('pools-delete-pool')}
						</button>
					</div>
					<div class="text-xs text-base-content/60">
						{t('pools-summary-backends', {
							count: pool.backends.length,
							list: pool.backends.join(', ') || '—'
						})}
					</div>
					<div class="text-xs text-base-content/60">
						{t('pools-summary-models', {
							count: pool.models.length,
							list: pool.models.join(', ') || '—'
						})}
					</div>
				</div>
			</div>
		{/each}
	</div>

	<div class="flex justify-between items-center mb-4 flex-wrap gap-2">
		<h2 class="text-lg font-semibold">{t('backends-heading')}</h2>
		<button class="btn btn-primary btn-sm" onclick={() => (showBackendForm = !showBackendForm)}>
			{t('backends-add-backend')}
		</button>
	</div>

	{#if showBackendForm}
		<div class="card border border-base-300 mb-4">
			<div class="card-body">
				<div class="flex flex-wrap gap-3 items-end">
					<label class="flex flex-col gap-1"><span class="label-text">{t('backends-field-name')}</span>
						<input class="input input-bordered input-sm" bind:value={bName} /></label>
					<label class="flex flex-col gap-1 flex-1 min-w-56"><span class="label-text">{t('backends-field-base-url')}</span>
						<input class="input input-bordered input-sm" bind:value={bUrl} placeholder="https://api.example.com/v1" /></label>
					<label class="flex flex-col gap-1"><span class="label-text">{t('backends-field-api-key')}</span>
						<input class="input input-bordered input-sm" type="password" bind:value={bKey} placeholder={t('backends-field-api-key-keep')} /></label>
					<label class="flex flex-col gap-1"><span class="label-text">{t('backends-field-pool')}</span>
						<select class="select select-bordered select-sm" bind:value={bPool}>
							<option value="">{t('backends-field-pool-none')}</option>
							{#each data.pools as pool (pool.name)}<option value={pool.name}>{pool.name}</option>{/each}
						</select></label>
					<label class="label cursor-pointer gap-1">
						<input type="checkbox" class="checkbox checkbox-sm" bind:checked={bOverwrite} />
						<span class="label-text text-xs">{t('admin-overwrite-existing')}</span>
					</label>
					<button class="btn btn-primary btn-sm" onclick={saveBackend} disabled={!bName.trim() || !bUrl.trim()}>
						{t('backends-save-backend')}
					</button>
				</div>
			</div>
		</div>
	{/if}

	<ul class="flex flex-col gap-3">
		{#each data.backends as backend (backend.name)}
			<li>
				<div class="card border border-base-300">
					<div class="card-body py-3">
						<div class="flex items-center gap-3 flex-wrap">
							{#if backend.live}
								{#if !backend.live.enabled}
									<span class="badge badge-warning badge-sm">{t('backends-status-drained')}</span>
								{:else if backend.live.healthy}
									<span class="badge badge-success badge-sm">{t('backends-status-up')}</span>
								{:else}
									<span class="badge badge-error badge-sm">{t('backends-status-down')}</span>
								{/if}
							{:else}
								<span class="badge badge-ghost badge-sm">{t('upstreams-backend-pending')}</span>
							{/if}
							<span class="font-medium">{backend.name}</span>
							<span class="text-xs text-base-content/50 truncate max-w-64">{backend.base_url}</span>
							{#if backend.live}
								<span class="text-xs text-base-content/50">
									{t('backends-inflight-label', {
										load: `${n(backend.live.inflight)}/${n(backend.live.max_inflight)}`
									})} · {t('backends-requests-per-hour', { count: n(lastHour(backend.name)) })}
								</span>
							{/if}
							<span class="flex-1"></span>
							{#if backend.live?.enabled}
								<button class="btn btn-ghost btn-xs" onclick={() => toggleBackend(backend.name, false)}>
									{t('backends-drain-button')}
								</button>
							{:else}
								<button class="btn btn-ghost btn-xs" onclick={() => toggleBackend(backend.name, true)}>
									{t('backends-undrain-button')}
								</button>
							{/if}
							<button class="btn btn-ghost btn-xs text-error" onclick={() => deleteBackend(backend.name)}>
								{t('backends-delete-backend')}
							</button>
						</div>
					</div>
				</div>
			</li>
		{/each}
	</ul>
{/if}
