<script lang="ts">
	import { onMount } from 'svelte';
	import { adminJson, adminPost } from '$lib/admin-client';
	import type { ComfyuiCatalog } from '$lib/admin-comfyui';
	import ComfyuiJobRow from '$lib/components/admin/ComfyuiJobRow.svelte';
	import ComfyuiWorkflowRow from '$lib/components/admin/ComfyuiWorkflowRow.svelte';
	import { t } from '$lib/i18n.svelte';

	let data = $state<ComfyuiCatalog | null>(null);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let reloading = $state(false);
	let pendingJobs = $derived(data?.jobs.filter((job) => job.status === 'pending').length ?? 0);

	async function refresh() {
		try { data = await adminJson<ComfyuiCatalog>('/api/v0/comfyui/catalog'); error = null; }
		catch (caught) { error = String(caught); }
	}

	async function reload() {
		reloading = true;
		try {
			const response = await adminPost<{ report: { total: number; skipped: { source: string; reason: string }[] } }>('/api/v0/comfyui/reload');
			notice = response.report.skipped.length
				? t('admin-comfyui-reloaded-skipped', { count: response.report.total, skipped: response.report.skipped.length })
				: t('admin-comfyui-reloaded', { count: response.report.total });
			await refresh();
		} catch (caught) { error = String(caught); }
		finally { reloading = false; }
	}

	onMount(refresh);
</script>

<div class="w-full">
	<div class="mb-6 flex flex-wrap items-start justify-between gap-3">
		<div class="min-w-0 flex-1"><h1 class="m-0 text-2xl font-semibold">{t('admin-comfyui-heading')}</h1><p class="mb-0 mt-1 text-sm text-base-content/60">{t('admin-comfyui-intro')}</p></div>
		<button type="button" class="btn btn-primary btn-sm shrink-0" disabled={!data?.configured || reloading} onclick={reload}>{t('admin-comfyui-reload')}</button>
	</div>
	{#if error}<div class="alert alert-error mb-6"><span>{error}</span></div>{/if}
	{#if notice}<div class="alert alert-success mb-6"><span>{notice}</span></div>{/if}
	{#if data}
		{#if !data.configured}
			<div class="card mb-6 border border-base-300"><div class="card-body"><h2 class="card-title text-base">{t('admin-comfyui-not-configured')}</h2><p class="m-0 text-sm text-base-content/70">{t('admin-comfyui-not-configured-help')}</p></div></div>
		{:else}
			<div class="card mb-6 border border-base-300"><div class="card-body"><h2 class="card-title text-base">{t('admin-comfyui-operator-config')}</h2><div class="grid grid-cols-1 gap-4 text-sm md:grid-cols-2">
				<div><div class="text-base-content/60">{t('admin-comfyui-worker-url')}</div><div class="break-all font-mono">{data.base_url}</div></div>
				<div><div class="text-base-content/60">{t('admin-comfyui-content-directory')}</div><div class="break-all font-mono">{data.content_dir}</div></div>
				<div><div class="text-base-content/60">{t('admin-comfyui-timeout')}</div><div>{data.timeout_secs} s</div></div>
				<div><div class="text-base-content/60">{t('admin-comfyui-poll-interval')}</div><div>{data.queue_poll_interval_ms} ms</div></div>
			</div><p class="mb-0 mt-4 text-xs text-base-content/60">{t('admin-comfyui-config-help')}</p></div></div>
			<div class="card border border-base-300"><div class="card-body"><h2 class="card-title text-base">{t('admin-comfyui-loaded-workflows')}<span class="badge badge-outline ml-2">{data.workflows.length}</span></h2>
				{#if data.workflows.length}<div class="flex flex-col divide-y divide-base-300">{#each data.workflows as workflow (workflow.id)}<ComfyuiWorkflowRow {workflow} />{/each}</div>{:else}<p class="m-0 text-sm text-base-content/70">{t('admin-comfyui-empty')}</p>{/if}
			</div></div>
			{#if data.jobs.length}<div class="card mt-6 border border-base-300"><div class="card-body"><h2 class="card-title text-base">{t('admin-comfyui-recent-jobs')}<span class="badge badge-outline ml-2">{data.jobs.length}</span>{#if pendingJobs}<span class="badge badge-warning badge-sm ml-1">{t('admin-comfyui-pending', { count: pendingJobs })}</span>{/if}</h2><div class="flex flex-col divide-y divide-base-300">{#each data.jobs as job (job.id)}<ComfyuiJobRow {job} />{/each}</div></div></div>{/if}
		{/if}
	{:else if !error}<div class="flex flex-col gap-4"><div class="skeleton h-44 w-full"></div><div class="skeleton h-72 w-full"></div></div>{/if}
</div>
