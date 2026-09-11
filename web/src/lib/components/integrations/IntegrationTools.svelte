<script lang="ts">
	import { t } from '$lib/i18n.svelte';
	import type { IntegrationConnector, IntegrationMode } from '$lib/integrations';
	import IntegrationModePicker from './IntegrationModePicker.svelte';

	let { connector, onmode, onall }: {
		connector: IntegrationConnector;
		onmode: (tool: string, mode: IntegrationMode) => void | Promise<void>;
		onall: (mode: IntegrationMode) => void | Promise<void>;
	} = $props();
	let saving = $state(false);

	async function setMode(tool: string, mode: IntegrationMode) {
		saving = true;
		try { await onmode(tool, mode); } finally { saving = false; }
	}

	async function setAll(mode: IntegrationMode) {
		saving = true;
		try { await onall(mode); } finally { saving = false; }
	}
</script>

<div class="min-w-0 border-t border-base-300 pt-3">
	{#if connector.tools === null}
		<p class="m-0 text-xs text-error">{t('integrations-tools-error-prefix')} <span class="break-all">{connector.tool_error ?? t('integrations-error-connection-unavailable')}</span></p>
		<p class="m-0 mt-1 text-xs text-base-content/50">{t(connector.needs_reauth ? 'integrations-tools-error-hint-reauth' : 'integrations-tools-error-hint')}</p>
	{:else if connector.tools.length === 0}
		<p class="m-0 text-xs text-base-content/50">{t('integrations-tools-empty')}</p>
	{:else}
		<div class="mb-1 flex flex-wrap items-center justify-between gap-3">
			<span class="text-xs font-medium text-base-content/70">{t('integrations-tools-header', { count: connector.tools.length })}</span>
			<div class="flex items-center gap-1"><span class="mr-1 text-xs text-base-content/50">{t('integrations-set-all-label')}</span><IntegrationModePicker onchange={setAll} disabled={saving} /></div>
		</div>
		<details>
			<summary class="cursor-pointer select-none py-1 text-xs text-base-content/60">{t('integrations-tools-toggle')}</summary>
			<div class="mt-1 flex flex-col divide-y divide-base-200">
				{#each connector.tools as tool (tool.name)}
					<div class="flex flex-col gap-2 py-2 sm:flex-row sm:items-center sm:gap-3">
						<div class="min-w-0 flex-1">
							<div class="flex items-center gap-2"><code class="font-mono text-xs break-all">{tool.name}</code><span class="badge badge-ghost badge-xs">{t(tool.read_only ? 'integrations-tool-kind-read' : 'integrations-tool-kind-write')}</span></div>
							{#if tool.description}<p class="m-0 mt-0.5 line-clamp-2 text-xs text-base-content/50">{tool.description}</p>{/if}
						</div>
						<div class="self-end sm:self-auto"><IntegrationModePicker current={tool.mode} onchange={(mode) => setMode(tool.name, mode)} disabled={saving} /></div>
					</div>
				{/each}
			</div>
		</details>
	{/if}
</div>
