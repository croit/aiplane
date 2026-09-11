<script lang="ts">
	import { comfyuiJobPresentation, shortPromptId, type ComfyuiJob } from '$lib/admin-comfyui';
	import { t } from '$lib/i18n.svelte';
	let { job } = $props<{ job: ComfyuiJob }>();
	let presentation = $derived(comfyuiJobPresentation(job.status));
	let detail = $derived(job.status === 'completed' ? job.output_filename : job.error_message);
</script>

<div class="flex items-start gap-3 py-3" data-testid={`comfyui-job-${job.id}`}>
	<span class="text-lg" aria-hidden="true">{presentation.icon}</span>
	<div class="min-w-0 flex-1">
		<div class="flex flex-wrap items-baseline gap-2"><span class="font-mono text-sm font-semibold">#{job.id}</span><span class="badge badge-sm {presentation.badge}">{job.status}</span><span class="text-sm text-base-content/70">{job.workflow_id}</span></div>
		<div class="mt-1 break-words text-xs text-base-content/50">{t('admin-comfyui-job-meta', { prompt: shortPromptId(job.prompt_id), created: job.created_at })}{#if job.completed_at} · {t('admin-comfyui-job-completed', { completed: job.completed_at })}{/if}</div>
		{#if detail}<div class="mt-1 break-all font-mono text-xs text-base-content/60">{detail}</div>{/if}
	</div>
</div>
