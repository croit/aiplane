<script lang="ts">
	import { onMount } from 'svelte';
	import { adminJson } from '$lib/admin-client';
	import CollectionCard from '$lib/components/rag/CollectionCard.svelte';
	import CollectionForm from '$lib/components/rag/CollectionForm.svelte';
	import { t } from '$lib/i18n.svelte';
	import type { RagCollection, RagProfile, RagProvider } from '$lib/rag';

	let collections = $state<RagCollection[]>([]);
	let providers = $state<RagProvider[]>([]);
	let profiles = $state<RagProfile[]>([]);
	let models = $state<string[]>([]);
	let defaultEmbedding = $state<string | null>(null);
	let loading = $state(true);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);

	async function refreshCollections() {
		collections = (await adminJson<{ data: RagCollection[] }>('/api/v0/rag/collections')).data ?? [];
	}

	async function load() {
		loading = true;
		try {
			const [collectionData, providerData, profileData] = await Promise.all([
				adminJson<{ data: RagCollection[] }>('/api/v0/rag/collections'),
				adminJson<{ data: RagProvider[]; embedding_models: string[]; default_embedding: string | null }>('/api/v0/rag/providers'),
				adminJson<{ data: RagProfile[] }>('/api/v0/rag/profiles')
			]);
			collections = collectionData.data ?? [];
			providers = providerData.data ?? [];
			profiles = profileData.data ?? [];
			models = providerData.embedding_models;
			defaultEmbedding = providerData.default_embedding;
			error = null;
		} catch (caught) {
			error = String(caught);
		} finally {
			loading = false;
		}
	}

	onMount(load);
</script>

<div class="mx-auto w-full max-w-5xl space-y-6">
	<header>
		<h1 class="text-2xl font-bold">{t('rag-heading')}</h1>
		<p class="mt-2 text-sm text-base-content/60">{t('rag-description-prefix')} <code>rag_search</code> {t('rag-description-suffix')}</p>
		<a class="link mt-2 inline-block text-sm" href="/rag/profiles">{t('rag-profile-link')}</a>
	</header>

	{#if error}<div class="alert alert-error"><span>{error}</span></div>{/if}
	{#if notice}<div class="alert alert-info"><span>{notice}</span></div>{/if}

	<CollectionForm {providers} {profiles} {models} defaultModel={defaultEmbedding} onsaved={async (name, aggregate) => { notice = aggregate ? t('rag-toast-created-aggregate', { name }) : t('rag-toast-indexing-queued', { name, ref: 'main' }); await refreshCollections(); }} />

	<section class="space-y-3">
		<h2 class="text-xl font-semibold">{t('rag-collections-heading')}</h2>
		{#if loading}
			<div class="skeleton h-28 w-full"></div>
		{:else if collections.length === 0}
			<div class="card card-border"><div class="card-body text-sm text-base-content/60">{t('rag-empty-list')}</div></div>
		{:else}
			{#each collections as collection (collection.id)}
				<CollectionCard {collection} {providers} {profiles} {models} onchanged={refreshCollections} onnotice={(message) => (notice = message)} />
			{/each}
		{/if}
	</section>
</div>
