<script lang="ts">
	import SearchableSelect from '#lib/components/SearchableSelect.svelte';
	import { agentRunOptions } from '#lib/agent-background.js';
	import type { OwnedAgent } from '#lib/agent-state.js';
	import { t } from '#lib/i18n.svelte.js';

	let { agents, initialId, value = $bindable(''), onchange }: {
		agents: OwnedAgent[];
		initialId: string | null;
		value?: string;
		onchange: () => void;
	} = $props();
	const options = $derived(agentRunOptions(agents, initialId, {
		person: t('agent-background-person'), inactive: t('agent-background-inactive'),
		unavailable: (id) => t('agent-background-unavailable', { id })
	}));
	const selected = $derived(agents.find((agent) => agent.id === value));
</script>

<fieldset class="fieldset w-full min-w-0">
	<legend class="fieldset-legend">{t('agent-background-run-as')}</legend>
	<SearchableSelect {options} bind:value ariaLabel={t('agent-background-run-as')} class="w-full" {onchange} />
	{#if value}
		<p class="label whitespace-normal">{t('agent-background-policy')}</p>
		{#if selected}
			<a class="link link-hover text-sm" href={`/agents/${encodeURIComponent(value)}?tab=automation`}>{t('agent-background-configure')}</a>
			{#if !selected.active}<p class="text-sm text-warning" role="status">{t('agent-background-inactive-help')}</p>{/if}
		{:else}<p class="text-sm text-warning" role="status">{t('agent-background-unavailable-help')}</p>{/if}
	{/if}
</fieldset>
