<script lang="ts">
	import { onMount } from 'svelte';
	import { adminDelete, adminJson, adminPost, adminPut } from '$lib/admin-client';
	import { splitConnectorValues, type AdminConnectorsData, type ConnectorFormValue } from '$lib/admin-connectors';
	import AdminConnectorCard from '$lib/components/admin/AdminConnectorCard.svelte';
	import AdminConnectorForm from '$lib/components/admin/AdminConnectorForm.svelte';
	import { t } from '$lib/i18n.svelte';

	let data = $state<AdminConnectorsData | null>(null);
	let error = $state<string | null>(null);

	async function refresh() {
		try { data = await adminJson<AdminConnectorsData>('/api/v0/admin/connectors'); error = null; }
		catch (caught) { error = String(caught); }
	}

	async function save(value: ConnectorFormValue) {
		try {
			await adminPut('/api/v0/admin/connectors', {
				...value,
				scopes: splitConnectorValues(value.scopes),
				groups: splitConnectorValues(value.groups)
			});
			await refresh();
		} catch (caught) { error = String(caught); }
	}

	async function toggle(key: string, enabled: boolean) {
		try { await adminPost(`/api/v0/admin/connectors/${encodeURIComponent(key)}/toggle`, { enabled }); await refresh(); }
		catch (caught) { error = String(caught); }
	}

	async function remove(key: string) {
		if (!confirm(t('connectors-delete-confirm'))) return;
		try { await adminDelete(`/api/v0/admin/connectors/${encodeURIComponent(key)}`); await refresh(); }
		catch (caught) { error = String(caught); }
	}

	async function restore() {
		if (!confirm(t('connectors-restore-defaults-confirm'))) return;
		try { await adminPost('/api/v0/admin/connectors/restore-defaults'); await refresh(); }
		catch (caught) { error = String(caught); }
	}

	onMount(refresh);
</script>

<svelte:head><title>{t('connectors-page-title')}</title></svelte:head>

<div class="w-full">
	<div class="mb-2 flex flex-wrap items-center justify-between gap-3"><h1 class="m-0 text-2xl font-bold">{t('connectors-heading')}</h1><button type="button" class="btn btn-ghost btn-sm" onclick={restore}>{t('connectors-restore-defaults-button')}</button></div>
	<p class="mb-6 text-sm text-base-content/60">{t('connectors-catalog-intro')}</p>
	{#if error}<div class="alert alert-error mb-4 text-sm"><span>{error}</span></div>{/if}
	{#if data}
		<details class="card mb-2 border border-base-300"><summary class="card-body cursor-pointer py-3 text-sm font-medium">{t('connectors-add-summary')}</summary><div class="card-body pt-0"><AdminConnectorForm redirectUri={data.redirect_uri} groups={data.groups} onsave={save} /></div></details>
		{#if data.connectors.length === 0}<div class="card border border-base-300"><div class="card-body"><p class="m-0 text-sm text-base-content/60">{t('connectors-empty-state')}</p></div></div>{/if}
		<div class="mt-4 flex flex-col gap-3">
			{#each data.connectors as connector (JSON.stringify(connector))}
				<AdminConnectorCard {connector} redirectUri={data.redirect_uri} groups={data.groups} onsave={save} ontoggle={() => toggle(connector.key, !connector.enabled)} ondelete={() => remove(connector.key)} />
			{/each}
		</div>
	{:else if !error}<div class="flex flex-col gap-3">{#each [1, 2, 3] as _}<div class="skeleton h-24 w-full"></div>{/each}</div>{/if}
</div>
