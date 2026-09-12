<script lang="ts">
	import { base } from '$app/paths';
	import { groupWorkflowsByKind, type ComfyuiWorkflow } from '$lib/admin-comfyui';
	import { n, t } from '$lib/i18n.svelte';

	// The left half of the master–detail view: one searchable list, grouped
	// by output kind. The rail scrolls on its own so the page never becomes
	// the endless column the old catalog was.
	let {
		workflows,
		total,
		selected,
		query = $bindable('')
	}: {
		workflows: ComfyuiWorkflow[];
		/** The whole catalog, so the header can say how much the search hides. */
		total: number;
		selected: ComfyuiWorkflow | null;
		query?: string;
	} = $props();

	let groups = $derived(groupWorkflowsByKind(workflows));
</script>

<aside class="flex w-full shrink-0 flex-col gap-2 sm:sticky sm:top-6 sm:w-64">
	<div class="flex items-baseline justify-between gap-2">
		<h2 class="m-0 text-xs font-medium uppercase tracking-wide text-base-content/50">{t('admin-comfyui-loaded-workflows')}</h2>
		<span class="text-xs tabular-nums text-base-content/40">{n(total)}</span>
	</div>
	<label class="input input-sm input-bordered flex w-full items-center gap-2">
		<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2" class="shrink-0 opacity-60" aria-hidden="true"><circle cx="11" cy="11" r="7" /><path d="m20 20-3.5-3.5" /></svg>
		<input
			type="search"
			class="grow"
			bind:value={query}
			placeholder={t('admin-comfyui-search-placeholder')}
			aria-label={t('admin-comfyui-search-placeholder')}
		/>
	</label>

	<div class="card border border-base-300">
		<div class="card-body max-h-[60vh] gap-1 overflow-y-auto p-2">
			{#if workflows.length === 0}
				<p class="m-0 px-2 py-1 text-sm text-base-content/50">{t('admin-comfyui-search-empty')}</p>
			{/if}
			{#each groups as group (group.kind)}
				<div class="flex items-center justify-between px-2 pb-0.5 pt-2">
					<span class="text-xs font-medium uppercase tracking-wide text-base-content/50">{group.kind}</span>
					<span class="text-xs tabular-nums text-base-content/40">{group.workflows.length}</span>
				</div>
				{#each group.workflows as workflow (workflow.id)}
					<a
						href="{base}/admin/comfyui?workflow={encodeURIComponent(workflow.id)}"
						aria-current={selected?.id === workflow.id ? 'true' : undefined}
						class="block truncate rounded px-2 py-1.5 font-mono text-sm transition-colors {selected?.id ===
						workflow.id
							? 'bg-base-300 font-semibold text-base-content'
							: 'text-base-content/70 hover:bg-base-200'}"
						title={workflow.tool_id}
					>
						{workflow.id}
					</a>
				{/each}
			{/each}
		</div>
	</div>
</aside>
