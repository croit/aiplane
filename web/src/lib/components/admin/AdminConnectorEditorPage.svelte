<script lang="ts">
	import { onMount } from 'svelte';
	import { base } from '$app/paths';
	import { goto } from '$app/navigation';
	import { adminJson, adminPut } from '$lib/admin-client';
	import {
		splitConnectorValues,
		type AdminConnector,
		type AdminConnectorsData,
		type ConnectorFormValue
	} from '$lib/admin-connectors';
	import AdminConnectorForm from './AdminConnectorForm.svelte';
	import { t } from '$lib/i18n.svelte';

	/**
	 * The connector editor, on its own route.
	 *
	 * It used to be a `<details>` inside the connector's list row, which put a
	 * dozen fields inline under a one-line summary and pushed every other
	 * connector down the page. The form is multi-section (URL, auth mode, DCR,
	 * scopes, groups), so it gets a page rather than a dialog — and a URL the
	 * operator can link to.
	 *
	 * `key = null` is the create form; the page is otherwise identical, which
	 * is the point — one form, one layout, one save path.
	 */
	let { key = null }: { key?: string | null } = $props();

	let data = $state<AdminConnectorsData | null>(null);
	let error = $state<string | null>(null);
	let connector = $derived<AdminConnector | null>(
		key === null ? null : (data?.connectors.find((entry) => entry.key === key) ?? null)
	);
	let missing = $derived(key !== null && data !== null && connector === null);

	async function save(value: ConnectorFormValue) {
		try {
			await adminPut('/api/v0/admin/connectors', {
				...value,
				scopes: splitConnectorValues(value.scopes),
				groups: value.groups
			});
			await goto(`${base}/admin/connectors`);
		} catch (caught) {
			error = String(caught);
		}
	}

	onMount(async () => {
		try {
			data = await adminJson<AdminConnectorsData>('/api/v0/admin/connectors');
		} catch (caught) {
			error = String(caught);
		}
	});
</script>

<svelte:head><title>{key === null ? t('connectors-add-page-title') : t('connectors-edit-page-title')}</title></svelte:head>

<div class="w-full max-w-3xl">
	<a class="link link-hover text-sm text-base-content/60" href="{base}/admin/connectors">← {t('connectors-heading')}</a>
	<h1 class="m-0 mt-2 text-2xl font-bold">
		{key === null ? t('connectors-add-summary') : t('connectors-edit-heading', { name: connector?.title ?? key })}
	</h1>
	{#if key !== null}<p class="mb-0 mt-1 font-mono text-xs text-base-content/50">{key}</p>{/if}

	{#if error}<div class="alert alert-error mt-4 text-sm"><span>{error}</span></div>{/if}

	{#if missing}
		<div class="alert alert-warning mt-4 text-sm"><span>{t('connectors-not-found')}</span></div>
	{:else if data}
		<div class="card mt-5 border border-base-300">
			<div class="card-body">
				<AdminConnectorForm connector={connector ?? undefined} redirectUri={data.redirect_uri} groups={data.groups} onsave={save} />
			</div>
		</div>
	{:else if !error}
		<div class="skeleton mt-5 h-96 w-full"></div>
	{/if}
</div>
