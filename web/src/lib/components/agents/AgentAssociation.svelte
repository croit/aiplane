<script lang="ts">
	import type { AgentBinding } from '#lib/agent-background.js';
	import { t } from '#lib/i18n.svelte.js';
	let { binding }: { binding: AgentBinding } = $props();
</script>

{#if binding.personal_agent_id}
	<div class="mt-2 flex flex-wrap items-center gap-2 text-sm">
		{#if binding.agent}
			<a class="link link-hover" href={`/agents/${encodeURIComponent(binding.personal_agent_id)}`}>{t('agent-background-association', { name: binding.agent.display || binding.agent.name })}</a>
			{#if !binding.agent.active}<span class="badge badge-warning badge-sm">{t('agents-status-inactive')}</span>{/if}
		{:else}<span class="badge badge-warning badge-sm break-all whitespace-normal">{t('agent-background-unavailable', { id: binding.personal_agent_id })}</span>{/if}
	</div>
{/if}
