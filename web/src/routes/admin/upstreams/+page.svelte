<script lang="ts">
	import { onMount } from 'svelte';
	import { adminJson, adminPut, adminPost, adminDelete } from '$lib/admin-client';

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
			notice = 'Topology applied.';
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
		if (!confirm(`Delete backend ${name}? Apply afterwards.`)) return;
		try {
			await adminDelete(`/api/v0/admin/backends/${encodeURIComponent(name)}`);
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	async function deletePool(name: string) {
		if (!confirm(`Delete pool ${name}? Apply afterwards.`)) return;
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
			<span>{data.dirty} unsaved topology change{data.dirty === 1 ? '' : 's'} — not live yet.</span>
			<button class="btn btn-primary btn-sm" onclick={apply}>Apply changes</button>
		</div>
	{/if}

	<div class="flex justify-between items-center mb-4 flex-wrap gap-2">
		<h2 class="text-lg font-semibold">Pools</h2>
		<button class="btn btn-primary btn-sm" onclick={() => (showPoolForm = !showPoolForm)}>New pool</button>
	</div>

	{#if showPoolForm}
		<div class="card border border-base-300 mb-4">
			<div class="card-body">
				<div class="flex flex-wrap gap-3 items-end">
					<label class="flex flex-col gap-1"><span class="label-text">Name</span>
						<input class="input input-bordered input-sm" bind:value={pName} /></label>
					<label class="flex flex-col gap-1"><span class="label-text">Kind</span>
						<select class="select select-bordered select-sm" bind:value={pKind}>
							<option>chat</option><option>transcription</option><option>embedding</option>
							<option>image</option><option>speech</option><option>ocr</option>
						</select></label>
					<label class="flex flex-col gap-1 flex-1 min-w-48"><span class="label-text">Backends (comma-sep)</span>
						<input class="input input-bordered input-sm" bind:value={pBackends} placeholder={data.backends.map((b) => b.name).join(', ')} /></label>
					<label class="flex flex-col gap-1 flex-1 min-w-48"><span class="label-text">Models (comma-sep)</span>
						<input class="input input-bordered input-sm" bind:value={pModels} /></label>
					<button class="btn btn-primary btn-sm" onclick={savePool} disabled={!pName.trim()}>Save</button>
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
						<button class="btn btn-ghost btn-xs text-error" onclick={() => deletePool(pool.name)}>Delete</button>
					</div>
					<div class="text-xs text-base-content/60">
						{pool.backends.length} backend(s): {pool.backends.join(', ') || '—'}
					</div>
					<div class="text-xs text-base-content/60">
						{pool.models.length} model(s): {pool.models.join(', ') || '—'}
					</div>
				</div>
			</div>
		{/each}
	</div>

	<div class="flex justify-between items-center mb-4 flex-wrap gap-2">
		<h2 class="text-lg font-semibold">Backends</h2>
		<button class="btn btn-primary btn-sm" onclick={() => (showBackendForm = !showBackendForm)}>New backend</button>
	</div>

	{#if showBackendForm}
		<div class="card border border-base-300 mb-4">
			<div class="card-body">
				<div class="flex flex-wrap gap-3 items-end">
					<label class="flex flex-col gap-1"><span class="label-text">Name</span>
						<input class="input input-bordered input-sm" bind:value={bName} /></label>
					<label class="flex flex-col gap-1 flex-1 min-w-56"><span class="label-text">Base URL</span>
						<input class="input input-bordered input-sm" bind:value={bUrl} placeholder="https://api.example.com/v1" /></label>
					<label class="flex flex-col gap-1"><span class="label-text">API key</span>
						<input class="input input-bordered input-sm" type="password" bind:value={bKey} placeholder="blank = keep/env" /></label>
					<label class="flex flex-col gap-1"><span class="label-text">Pool</span>
						<select class="select select-bordered select-sm" bind:value={bPool}>
							<option value="">(none)</option>
							{#each data.pools as pool (pool.name)}<option value={pool.name}>{pool.name}</option>{/each}
						</select></label>
					<label class="label cursor-pointer gap-1">
						<input type="checkbox" class="checkbox checkbox-sm" bind:checked={bOverwrite} />
						<span class="label-text text-xs">overwrite existing</span>
					</label>
					<button class="btn btn-primary btn-sm" onclick={saveBackend} disabled={!bName.trim() || !bUrl.trim()}>Save</button>
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
									<span class="badge badge-warning badge-sm">drained</span>
								{:else if backend.live.healthy}
									<span class="badge badge-success badge-sm">up</span>
								{:else}
									<span class="badge badge-error badge-sm">down</span>
								{/if}
							{:else}
								<span class="badge badge-ghost badge-sm">pending apply</span>
							{/if}
							<span class="font-medium">{backend.name}</span>
							<span class="text-xs text-base-content/50 truncate max-w-64">{backend.base_url}</span>
							{#if backend.live}
								<span class="text-xs text-base-content/50">
									{backend.live.inflight}/{backend.live.max_inflight} in-flight · {lastHour(backend.name)} req/h
								</span>
							{/if}
							<span class="flex-1"></span>
							{#if backend.live?.enabled}
								<button class="btn btn-ghost btn-xs" onclick={() => toggleBackend(backend.name, false)}>Drain</button>
							{:else}
								<button class="btn btn-ghost btn-xs" onclick={() => toggleBackend(backend.name, true)}>Undrain</button>
							{/if}
							<button class="btn btn-ghost btn-xs text-error" onclick={() => deleteBackend(backend.name)}>Delete</button>
						</div>
					</div>
				</div>
			</li>
		{/each}
	</ul>
{/if}
