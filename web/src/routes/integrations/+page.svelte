<script lang="ts">
	import { onMount } from 'svelte';
	import { adminJson, adminPost } from '$lib/admin-client';
	import IntegrationConnectorCard from '$lib/components/integrations/IntegrationConnectorCard.svelte';
	import { t } from '$lib/i18n.svelte';
	import { withAllToolModes, withToolMode, type IntegrationConnector, type IntegrationMode } from '$lib/integrations';

	let connectors = $state<IntegrationConnector[]>([]);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);

	async function refresh() {
		try {
			connectors = (await adminJson<{ connectors: IntegrationConnector[] }>('/api/v0/integrations')).connectors;
			error = null;
		} catch (err) {
			error = String(err);
		}
	}

	async function connect(key: string, token: string) {
		try {
			await adminPost(`/api/v0/integrations/${encodeURIComponent(key)}/token`, { token });
			notice = t('integrations-toast-connected', { name: key });
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	async function retry(key: string) {
		try {
			await adminPost(`/api/v0/integrations/${encodeURIComponent(key)}/retry`);
			await refresh();
		} catch (err) { notice = String(err); }
	}

	async function setMode(key: string, tool: string, mode: IntegrationMode) {
		try {
			await adminPost(`/api/v0/integrations/${encodeURIComponent(key)}/tools/mode`, { tool, mode });
			connectors = connectors.map((connector) => connector.key === key ? withToolMode(connector, tool, mode) : connector);
		} catch (err) { notice = String(err); }
	}

	async function setAll(key: string, mode: IntegrationMode) {
		try {
			await adminPost(`/api/v0/integrations/${encodeURIComponent(key)}/tools/all`, { mode });
			connectors = connectors.map((connector) => connector.key === key ? withAllToolModes(connector, mode) : connector);
		} catch (err) { notice = String(err); }
	}

	async function disconnect(key: string) {
		if (!confirm(t('integrations-disconnect-confirm'))) return;
		try {
			await adminPost(`/api/v0/integrations/${encodeURIComponent(key)}/disconnect`);
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	onMount(refresh);
</script>

<div class="mx-auto w-full max-w-5xl px-4 pb-6 pt-14 sm:px-6 sm:pt-6">
	<h1 class="mb-2 text-2xl font-bold">{t('integrations-heading')}</h1>
	<p class="mb-6 text-sm text-base-content/60">{t('integrations-intro')}</p>
	{#if error}<div class="alert alert-error mb-4"><span>{error}</span></div>{/if}
	{#if notice}<div class="alert alert-warning mb-4"><span>{notice}</span></div>{/if}
	{#if connectors.length === 0}
		<div class="card border border-base-300"><div class="card-body"><p class="m-0 text-sm text-base-content/60">{t('integrations-empty')}</p></div></div>
	{:else}
		<div class="flex flex-col gap-4">
			{#each connectors as connector (connector.key)}
				<IntegrationConnectorCard {connector} ontoken={(token) => connect(connector.key, token)} ondisconnect={() => disconnect(connector.key)} onretry={() => retry(connector.key)} onmode={(tool, mode) => setMode(connector.key, tool, mode)} onall={(mode) => setAll(connector.key, mode)} />
			{/each}
		</div>
	{/if}
</div>
