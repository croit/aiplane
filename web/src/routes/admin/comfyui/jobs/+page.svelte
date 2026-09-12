<script lang="ts">
	import { onMount } from 'svelte';
	import { adminJson } from '$lib/admin-client';
	import {
		filterJobs,
		formatDuration,
		jobFilterCounts,
		jobTone,
		workflowStats,
		JOB_FILTERS,
		type ComfyuiCatalog,
		type ComfyuiJobFilter
	} from '$lib/admin-comfyui';
	import ComfyuiJobsTable from '$lib/components/admin/ComfyuiJobsTable.svelte';
	import ComfyuiTabs from '$lib/components/admin/ComfyuiTabs.svelte';
	import { n, t } from '$lib/i18n.svelte';

	let data = $state<ComfyuiCatalog | null>(null);
	let error = $state<string | null>(null);
	let refreshing = $state(false);
	let filter = $state<ComfyuiJobFilter>('all');

	let jobs = $derived(data?.jobs ?? []);
	let counts = $derived(jobFilterCounts(jobs));
	let shown = $derived(filterJobs(jobs, filter));
	let stats = $derived(workflowStats(jobs));
	let failed = $derived(jobs.filter((job) => jobTone(job.status) === 'error').length);

	const filterLabel: Record<ComfyuiJobFilter, string> = {
		all: 'admin-comfyui-filter-all',
		completed: 'admin-comfyui-filter-completed',
		pending: 'admin-comfyui-filter-pending',
		failed: 'admin-comfyui-filter-failed'
	};

	async function refresh() {
		refreshing = true;
		try {
			data = await adminJson<ComfyuiCatalog>('/api/v0/comfyui/catalog');
			error = null;
		} catch (caught) {
			error = String(caught);
		} finally {
			refreshing = false;
		}
	}

	onMount(refresh);
</script>

<div class="w-full">
	<div class="flex flex-wrap items-start justify-between gap-3">
		<div class="min-w-0">
			<h1 class="m-0 text-2xl font-semibold">{t('admin-comfyui-recent-jobs')}</h1>
			<p class="mb-4 mt-1 max-w-prose text-sm text-base-content/60">{t('admin-comfyui-jobs-intro')}</p>
		</div>
		<button type="button" class="btn btn-sm shrink-0" disabled={refreshing} onclick={refresh}>
			{t('admin-comfyui-refresh')}
		</button>
	</div>

	<ComfyuiTabs current="jobs" {failed} />

	{#if error}<div class="alert alert-error mb-4 text-sm"><span>{error}</span></div>{/if}

	{#if data}
		{#if jobs.length === 0}
			<div class="card border border-base-300">
				<div class="card-body"><p class="m-0 text-sm text-base-content/70">{t('admin-comfyui-jobs-empty')}</p></div>
			</div>
		{:else}
			<!-- Reliability first: "which workflow is broken?" is answerable
			     from three columns, without reading the run list at all. -->
			<section class="mb-6">
				<h2 class="mb-2 text-xs font-medium uppercase tracking-wide text-base-content/50">{t('admin-comfyui-stats-heading')}</h2>
				<div class="overflow-x-auto rounded-box border border-base-300">
					<table class="table table-sm">
						<thead>
							<tr>
								<th>{t('admin-comfyui-col-workflow')}</th>
								<th class="text-right">{t('admin-comfyui-col-runs')}</th>
								<th class="text-right">{t('admin-comfyui-col-failed')}</th>
								<th class="text-right">{t('admin-comfyui-col-median')}</th>
							</tr>
						</thead>
						<tbody>
							{#each stats as row (row.workflow_id)}
								<tr>
									<td class="font-mono text-xs">{row.workflow_id}</td>
									<td class="text-right tabular-nums">{n(row.runs)}</td>
									<td class="text-right tabular-nums {row.failed ? 'font-semibold text-error' : 'text-base-content/40'}">{n(row.failed)}</td>
									<td class="text-right font-mono text-xs tabular-nums">{formatDuration(row.median_ms)}</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			</section>

			<div class="mb-3 flex flex-wrap items-center justify-between gap-2">
				<div role="group" class="join" aria-label={t('admin-comfyui-recent-jobs')}>
					{#each JOB_FILTERS as option (option)}
						<button
							type="button"
							class="btn join-item btn-xs {filter === option ? 'btn-active' : ''}"
							aria-pressed={filter === option}
							onclick={() => (filter = option)}
						>
							{t(filterLabel[option])}
							<span class="tabular-nums opacity-60">{n(counts[option])}</span>
						</button>
					{/each}
				</div>
				<span class="text-xs text-base-content/50">{t('admin-comfyui-jobs-window', { count: jobs.length })}</span>
			</div>

			<ComfyuiJobsTable jobs={shown} />
		{/if}
	{:else if !error}
		<div class="flex flex-col gap-4"><div class="skeleton h-32 w-full"></div><div class="skeleton h-64 w-full"></div></div>
	{/if}
</div>
