<script lang="ts">
	import { onMount } from 'svelte';
	import { base } from '$app/paths';
	import { goto } from '$app/navigation';
	import { adminJson } from '$lib/admin-client';
	import CollectionForm from './CollectionForm.svelte';
	import { t } from '$lib/i18n.svelte';
	import type { RagCollection, RagProfile, RagProvider } from '$lib/rag';

	/**
	 * The collection editor, on its own route.
	 *
	 * The same form used to sit permanently expanded above the collection list
	 * (create) and inside a card (edit) — twenty-odd fields across five
	 * sections either way, with the list pushed off the screen. It gets a page
	 * now, and `/rag` gets an Add button.
	 *
	 * `id = null` is the create form; everything else is identical, so both
	 * routes render this.
	 */
	let { id = null }: { id?: string | null } = $props();

	let collection = $state<RagCollection | null>(null);
	let providers = $state<RagProvider[]>([]);
	let profiles = $state<RagProfile[]>([]);
	let models = $state<string[]>([]);
	let groups = $state<string[]>([]);
	let defaultEmbedding = $state<string | null>(null);
	let loading = $state(true);
	let error = $state<string | null>(null);
	let missing = $state(false);

	async function done(name: string, aggregate: boolean) {
		const notice = id === null ? (aggregate ? 'created-aggregate' : 'queued') : 'saved';
		await goto(`${base}/rag?notice=${notice}&name=${encodeURIComponent(name)}`);
	}

	onMount(async () => {
		try {
			const [collectionData, providerData, profileData] = await Promise.all([
				adminJson<{ data: RagCollection[] }>('/api/v0/rag/collections'),
				adminJson<{ data: RagProvider[]; embedding_models: string[]; default_embedding: string | null; groups?: string[] }>('/api/v0/rag/providers'),
				adminJson<{ data: RagProfile[] }>('/api/v0/rag/profiles')
			]);
			providers = providerData.data ?? [];
			profiles = profileData.data ?? [];
			models = providerData.embedding_models;
			groups = providerData.groups ?? [];
			defaultEmbedding = providerData.default_embedding;
			if (id !== null) {
				collection = (collectionData.data ?? []).find((entry) => String(entry.id) === id) ?? null;
				missing = collection === null;
			}
			error = null;
		} catch (caught) {
			error = String(caught);
		} finally {
			loading = false;
		}
	});
</script>

<svelte:head><title>{id === null ? t('rag-new-page-title') : t('rag-edit-page-title')}</title></svelte:head>

<div class="w-full max-w-4xl">
	<a class="link link-hover text-sm text-base-content/60" href="{base}/rag">← {t('rag-back-to-collections')}</a>
	<h1 class="m-0 mt-2 text-2xl font-bold">
		{id === null ? t('rag-create-heading') : collection ? t('rag-edit-heading', { name: collection.name }) : t('rag-edit-page-title')}
	</h1>

	{#if error}<div class="alert alert-error mt-4 text-sm"><span>{error}</span></div>{/if}

	{#if missing}
		<div class="alert alert-warning mt-4 text-sm"><span>{t('rag-not-found')}</span></div>
	{:else if loading}
		<div class="skeleton mt-5 h-96 w-full"></div>
	{:else}
		<div class="mt-5">
			<CollectionForm
				{collection}
				{providers}
				{profiles}
				{models}
				{groups}
				defaultModel={defaultEmbedding}
				onsaved={done}
				oncancel={() => goto(`${base}/rag`)}
			/>
		</div>
	{/if}
</div>
