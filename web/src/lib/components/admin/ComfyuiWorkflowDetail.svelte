<script lang="ts">
	import { inlineCodeSpans, requiredParams, type ComfyuiWorkflow } from '$lib/admin-comfyui';
	import { n, t } from '$lib/i18n.svelte';

	// The right half: one workflow's full contract. The parameter prose is
	// written for the model, not the operator, so it goes in a table where a
	// long description stays in its own column instead of running the page
	// width — the failure the old flat list had.
	let { workflow }: { workflow: ComfyuiWorkflow } = $props();
	let required = $derived(requiredParams(workflow));
</script>

<section class="min-w-0 flex-1" data-testid={`comfyui-workflow-${workflow.id}`}>
	<header class="border-b border-base-300 pb-4">
		<h2 class="m-0 text-xl font-semibold">{workflow.title}</h2>
		<div class="mt-1.5 flex flex-wrap items-center gap-2 text-xs">
			<code class="rounded bg-base-200 px-1.5 py-0.5 font-mono">{workflow.tool_id}</code>
			<span class="badge badge-outline badge-sm">{workflow.output_kind}</span>
			<span class="text-base-content/50">{t('admin-comfyui-node', { id: workflow.output_node_id })}</span>
			<span class="text-base-content/30" aria-hidden="true">·</span>
			<span class="text-base-content/50">
				{t('admin-comfyui-filename-prefix')}
				<code class="font-mono">{workflow.filename_prefix}</code>
			</span>
		</div>
		<p class="mb-0 mt-3 max-w-prose text-sm leading-relaxed text-base-content/80">
			{#each inlineCodeSpans(workflow.description) as span}{#if span.code}<code class="rounded bg-base-200 px-1 font-mono text-xs">{span.text}</code>{:else}{span.text}{/if}{/each}
		</p>
	</header>

	<div class="mt-5">
		<div class="mb-2 flex items-baseline justify-between gap-2">
			<h3 class="m-0 text-xs font-medium uppercase tracking-wide text-base-content/50">{t('admin-comfyui-parameters')}</h3>
			{#if workflow.params.length}
				<span class="text-xs text-base-content/50">
					{t('admin-comfyui-required-count', { required: n(required.length), total: n(workflow.params.length) })}
				</span>
			{/if}
		</div>
		{#if workflow.params.length === 0}
			<p class="m-0 text-sm text-base-content/60">{t('admin-comfyui-no-params')}</p>
		{:else}
			<div class="overflow-x-auto rounded-box border border-base-300">
				<table class="table table-sm">
					<thead>
						<tr>
							<th class="w-48">{t('admin-comfyui-param-column')}</th>
							<th>{t('admin-comfyui-param-description-column')}</th>
						</tr>
					</thead>
					<tbody>
						{#each workflow.params as parameter (parameter.key)}
							<tr>
								<td class="align-top">
									<div class="font-mono text-xs">{parameter.key}</div>
									{#if parameter.required}
										<span class="badge badge-primary badge-xs mt-1">{t('admin-comfyui-required')}</span>
									{/if}
								</td>
								<td class="align-top text-sm text-base-content/70"
									>{#each inlineCodeSpans(parameter.description) as span}{#if span.code}<code class="rounded bg-base-200 px-1 font-mono text-xs">{span.text}</code>{:else}{span.text}{/if}{/each}</td
								>
							</tr>
						{/each}
					</tbody>
				</table>
			</div>
		{/if}
	</div>
</section>
