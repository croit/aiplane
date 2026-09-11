<script lang="ts">
	import { t } from '$lib/i18n.svelte';
	import type { ToolEntry } from '$lib/tools';

	let { sections, saving, ontoggle }: {
		sections: [string, ToolEntry[]][];
		saving: string | null;
		ontoggle: (tool: ToolEntry) => void;
	} = $props();
</script>

{#each sections as [category, entries] (category)}
	<section class="card mb-6 border border-base-300">
		<div class="card-body">
			<h2 class="card-title text-base">{category}</h2>
			<ul class="flex flex-col divide-y divide-base-300">
				{#each entries as tool (tool.key)}
					<li class="flex items-center gap-4 py-3">
						<div class="min-w-0 flex-1">
							<div class="flex flex-wrap items-baseline gap-2">
								<span class="text-sm font-medium text-base-content">{tool.title}</span>
								<code class="font-mono text-xs text-base-content/50">{tool.tech}</code>
							</div>
							<div class="mt-0.5 text-xs text-base-content/60">{tool.description}</div>
						</div>
						<input
							type="checkbox"
							class="toggle toggle-primary"
							checked={tool.enabled}
							disabled={saving === tool.key}
							onchange={() => ontoggle(tool)}
							aria-label={t('tools-toggle-aria', { name: tool.title })}
						/>
					</li>
				{/each}
			</ul>
		</div>
	</section>
{/each}
