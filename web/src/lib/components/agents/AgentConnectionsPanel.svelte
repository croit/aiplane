<script lang="ts">
	import { onMount } from 'svelte';
	import { agentConnectionsApi, type OwnedAgent, type AgentConnections } from '#lib/owned-agents.js';
	import { ApiError } from '#lib/api.js';
	import { t } from '#lib/i18n.svelte.js';
	import AgentConnectionRow from './AgentConnectionRow.svelte';
	let { agent, onchange }: { agent: OwnedAgent; onchange: () => void } = $props();
	let data = $state<AgentConnections | null>(null);
	let loading = $state(false);
	let error = $state<string | null>(null);
	let revision = $state(0);
	async function reload() {
		loading = true; error = null;
		try { data = await agentConnectionsApi.load(agent.id); revision += 1; }
		catch (caught) { error = caught instanceof ApiError ? caught.serverMessage ?? caught.message : caught instanceof Error ? caught.message : t('agent-connection-error-load'); }
		finally { loading = false; }
	}
	onMount(() => void reload());
</script>
<section class="space-y-3" aria-label={t('agent-connection-heading')}>
	<h3 class="font-medium">{t('agent-connection-heading')}</h3>
	{#if error}<div class="alert alert-error py-2" role="alert"><span class="break-words">{error}</span></div>{/if}
	{#if loading}<div class="skeleton h-16 w-full"></div>{/if}
	{#if data}{#key revision}{#each data.connections as row (row.key)}<AgentConnectionRow {agent} {row} {onchange} oninventory={(next) => data = next} />{/each}{/key}{/if}
	<div class="flex justify-end"><button class="btn btn-ghost btn-sm" type="button" disabled={loading} onclick={reload}>{t('agent-connection-reload')}</button></div>
</section>
