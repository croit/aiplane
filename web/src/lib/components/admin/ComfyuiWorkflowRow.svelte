<script lang="ts">
	import type { ComfyuiWorkflow } from '$lib/admin-comfyui';
	import { t } from '$lib/i18n.svelte';
	let { workflow } = $props<{ workflow: ComfyuiWorkflow }>();
</script>

<div class="py-4" data-testid={`comfyui-workflow-${workflow.id}`}>
	<div class="flex flex-wrap items-baseline gap-2">
		<span class="font-mono text-sm font-semibold">{workflow.tool_id}</span>
		<span class="badge badge-outline badge-sm">{workflow.output_kind}</span>
		<span class="text-sm text-base-content/60">{t('admin-comfyui-node', { id: workflow.output_node_id })}</span>
	</div>
	<p class="mt-1 text-base-content/80">{workflow.description}</p>
	<div class="mt-2 text-xs text-base-content/60">{t('admin-comfyui-workflow-meta', { title: workflow.title, prefix: workflow.filename_prefix })}</div>
	{#if workflow.params.length}
		<div class="mt-3">
			<div class="mb-1 text-xs uppercase tracking-wide text-base-content/60">{t('admin-comfyui-parameters')}</div>
			<ul class="space-y-1">
				{#each workflow.params as parameter}
					<li class="text-sm"><span class="font-mono text-base-content/80">{parameter.key}</span>{#if parameter.required}<span class="badge badge-primary badge-xs ml-1">{t('admin-comfyui-required')}</span>{/if} — <span class="text-base-content/70">{parameter.description}</span></li>
				{/each}
			</ul>
		</div>
	{/if}
</div>
