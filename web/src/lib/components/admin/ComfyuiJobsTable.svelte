<script lang="ts">
	import { base } from '$app/paths';
	import {
		formatDuration,
		jobDurationMs,
		jobTone,
		relativeTime,
		type ComfyuiJob
	} from '$lib/admin-comfyui';
	import { dt, locale, t } from '$lib/i18n.svelte';

	let { jobs }: { jobs: ComfyuiJob[] } = $props();

	// One tick a minute keeps "14 minutes ago" honest on a page an operator
	// leaves open, without polling the server for data that has not changed.
	let now = $state(Date.now());
	$effect(() => {
		const timer = setInterval(() => (now = Date.now()), 60_000);
		return () => clearInterval(timer);
	});

	const toneClass = {
		success: 'badge-success',
		error: 'badge-error',
		warning: 'badge-warning',
		neutral: 'badge-ghost'
	} as const;
</script>

<div class="overflow-x-auto rounded-box border border-base-300">
	<table class="table table-sm">
		<thead>
			<tr>
				<th class="w-16">#</th>
				<th>{t('admin-comfyui-col-status')}</th>
				<th>{t('admin-comfyui-col-workflow')}</th>
				<th class="text-right">{t('admin-comfyui-col-duration')}</th>
				<th>{t('admin-comfyui-col-when')}</th>
				<th>{t('admin-comfyui-col-result')}</th>
			</tr>
		</thead>
		<tbody>
			{#each jobs as job (job.id)}
				{@const tone = jobTone(job.status)}
				<tr data-testid={`comfyui-job-${job.id}`}>
					<td class="font-mono text-xs text-base-content/60">{job.id}</td>
					<td><span class="badge badge-sm {toneClass[tone]}">{job.status}</span></td>
					<td class="font-mono text-xs">{job.workflow_id}</td>
					<td class="text-right font-mono text-xs tabular-nums">{formatDuration(jobDurationMs(job))}</td>
					<td class="whitespace-nowrap text-xs text-base-content/60" title={dt(job.created_at)}>
						{relativeTime(job.created_at, locale.current, now)}
					</td>
					<td class="min-w-0">
						{#if job.status === 'pending'}
							<span class="text-xs text-base-content/50">{t('admin-comfyui-job-still-running')}</span>
						{:else if job.error_message}
							<span class="break-all font-mono text-xs text-error/80">{job.error_message}</span>
						{:else if job.output_filename}
							<span class="break-all font-mono text-xs text-base-content/60">{job.output_filename}</span>
						{/if}
						{#if job.session_id}
							<a class="link link-hover ml-2 whitespace-nowrap text-xs" href="{base}/chat/{job.session_id}">
								{t('admin-comfyui-job-open')} ↗
							</a>
						{/if}
					</td>
				</tr>
			{/each}
		</tbody>
	</table>
</div>
