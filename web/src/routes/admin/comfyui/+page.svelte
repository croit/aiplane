<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import { adminJson, adminPost } from '$lib/admin-client';
	import {
		filterWorkflows,
		jobTone,
		selectedWorkflow,
		type ComfyuiCatalog,
		type ComfyuiHealth
	} from '$lib/admin-comfyui';
	import ComfyuiCatalogRail from '$lib/components/admin/ComfyuiCatalogRail.svelte';
	import ComfyuiTabs from '$lib/components/admin/ComfyuiTabs.svelte';
	import ComfyuiWorkerStatus from '$lib/components/admin/ComfyuiWorkerStatus.svelte';
	import ComfyuiWorkflowDetail from '$lib/components/admin/ComfyuiWorkflowDetail.svelte';
	import { t } from '$lib/i18n.svelte';

	let data = $state<ComfyuiCatalog | null>(null);
	let health = $state<ComfyuiHealth | null>(null);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let reloading = $state(false);
	let query = $state('');

	// `?workflow=` drives the selection, so a link to one workflow's contract
	// is shareable — the thing an operator wants when answering "what does
	// this tool take?" in a chat thread.
	let requested = $derived(page.url.searchParams.get('workflow'));
	let matching = $derived(filterWorkflows(data?.workflows ?? [], query));
	let selected = $derived(selectedWorkflow(matching, requested));
	let failedJobs = $derived(data?.jobs.filter((job) => jobTone(job.status) === 'error').length ?? 0);

	async function refresh() {
		try {
			data = await adminJson<ComfyuiCatalog>('/api/v0/comfyui/catalog');
			error = null;
		} catch (caught) {
			error = String(caught);
		}
	}

	/** Separate from the catalog: the probe talks to another host, and a slow
	 *  or dead worker must not hold up the workflow list. */
	async function probe() {
		if (!data?.configured) return;
		try {
			health = await adminJson<ComfyuiHealth>('/api/v0/comfyui/health');
		} catch (caught) {
			health = { reachable: false, base_url: data.base_url ?? '', error: String(caught), worker: null };
		}
	}

	async function reload() {
		reloading = true;
		try {
			const response = await adminPost<{ report: { total: number; skipped: { source: string; reason: string }[] } }>('/api/v0/comfyui/reload');
			notice = response.report.skipped.length
				? t('admin-comfyui-reloaded-skipped', { count: response.report.total, skipped: response.report.skipped.length })
				: t('admin-comfyui-reloaded', { count: response.report.total });
			await refresh();
			await probe();
		} catch (caught) {
			error = String(caught);
		} finally {
			reloading = false;
		}
	}

	onMount(async () => {
		await refresh();
		await probe();
	});
</script>

<div class="w-full">
	<h1 class="m-0 text-2xl font-semibold">{t('admin-comfyui-heading')}</h1>
	<p class="mb-4 mt-1 max-w-prose text-sm text-base-content/60">{t('admin-comfyui-intro')}</p>

	<ComfyuiTabs current="workflows" failed={failedJobs} />

	{#if error}<div class="alert alert-error mb-4 text-sm"><span>{error}</span></div>{/if}
	{#if notice}<div class="alert alert-success mb-4 text-sm"><span>{notice}</span></div>{/if}

	{#if data}
		{#if !data.configured}
			<div class="card border border-base-300">
				<div class="card-body">
					<h2 class="card-title text-base">{t('admin-comfyui-not-configured')}</h2>
					<p class="m-0 text-sm text-base-content/70">{t('admin-comfyui-not-configured-help')}</p>
				</div>
			</div>
		{:else}
			<ComfyuiWorkerStatus catalog={data} {health} {reloading} onreload={reload} />

			{#if data.workflows.length === 0}
				<div class="card border border-base-300">
					<div class="card-body"><p class="m-0 text-sm text-base-content/70">{t('admin-comfyui-empty')}</p></div>
				</div>
			{:else}
				<div class="flex flex-col items-start gap-6 sm:flex-row">
					<ComfyuiCatalogRail workflows={matching} total={data.workflows.length} {selected} bind:query />
					{#if selected}
						{#key selected.id}<ComfyuiWorkflowDetail workflow={selected} />{/key}
					{:else}
						<p class="min-w-0 flex-1 pt-2 text-sm text-base-content/60">{t('admin-comfyui-detail-empty')}</p>
					{/if}
				</div>
				<p class="mt-6 max-w-prose text-xs text-base-content/50">{t('admin-comfyui-config-help')}</p>
			{/if}
		{/if}
	{:else if !error}
		<div class="flex flex-col gap-4"><div class="skeleton h-32 w-full"></div><div class="skeleton h-72 w-full"></div></div>
	{/if}
</div>
