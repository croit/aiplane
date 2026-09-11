<script lang="ts">
	import { onMount } from 'svelte';
	import { adminDelete, adminJson, adminPut } from '$lib/admin-client';
	import { matchesModelFilter } from '$lib/admin-models';
	import type { AdminModelsData, ModelFilter } from '$lib/admin-models';
	import AdminModelRow from '$lib/components/admin/AdminModelRow.svelte';
	import DefaultModelsCard from '$lib/components/admin/DefaultModelsCard.svelte';
	import SearchSettingsCard from '$lib/components/admin/SearchSettingsCard.svelte';
	import { t } from '$lib/i18n.svelte';

	let data = $state<AdminModelsData | null>(null);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let query = $state('');
	let filter = $state<ModelFilter>('all');

	async function refresh() {
		try {
			data = await adminJson<AdminModelsData>('/api/v0/admin/models');
			error = null;
		} catch (caught) {
			error = String(caught);
		}
	}

	async function saveModel(body: Record<string, string>) {
		await adminPut('/api/v0/admin/models', body);
		notice = t('admin-saved-model', { model: body.model_name });
		await refresh();
	}

	async function clearModel(name: string) {
		if (!confirm(t('admin-clear-overrides-confirm', { model: name }))) return;
		await adminDelete(`/api/v0/admin/models/${encodeURIComponent(name)}`);
		notice = t('admin-cleared-defaults', { model: name });
		await refresh();
	}

	async function saveDefault(feature: string, model: string) {
		await adminPut('/api/v0/admin/model-defaults', { feature, model });
		notice = model ? t('admin-defaults-saved', { model }) : t('admin-defaults-cleared');
		await refresh();
	}

	async function saveSearch(settings: { provider: string; searxng_url: string; brave_api_key: string; clear_brave_key: boolean }) {
		await adminPut('/api/v0/admin/search-settings', settings);
		notice = t('admin-search-saved');
		await refresh();
	}

	let visibleModels = $derived(data?.models.filter((model) => matchesModelFilter(model, filter, query)) ?? []);
	onMount(refresh);
</script>

<svelte:head><title>{t('admin-page-title')}</title></svelte:head>

<section class="mx-auto flex max-w-5xl flex-col gap-4 p-4 pt-14 sm:p-6">
	<header class="flex flex-col gap-1">
		<h1 class="text-2xl font-bold">{t('admin-heading')}</h1>
		<p class="max-w-2xl text-sm text-base-content/70">{t('admin-intro-prefix')} <strong>{t('admin-intro-every')}</strong> {t('admin-intro-middle')} <strong>{t('admin-intro-always-wins')}</strong>{t('admin-intro-suffix')}</p>
	</header>

	{#if error}<div class="alert alert-error"><span>{error}</span></div>{/if}
	{#if notice}<div class="alert alert-success"><span>{notice}</span></div>{/if}
	{#if data}
		<DefaultModelsCard defaults={data.feature_defaults} onsave={saveDefault} />
		{#key `${data.search.provider}:${data.search.searxng_url}:${data.search.brave_key_set}`}
			<SearchSettingsCard search={data.search} onsave={saveSearch} />
		{/key}

		{#if data.models.length === 0}
			<div class="alert"><span>{t('admin-no-models')}</span></div>
		{:else}
			<article class="card border border-base-300 bg-base-100">
				<div class="card-body gap-3 pb-0">
					<div class="flex flex-wrap items-center gap-2">
						<input type="search" class="input input-bordered input-sm w-60 max-w-full" bind:value={query} placeholder={t('admin-filter-placeholder')} aria-label={t('admin-filter-placeholder')} />
						{#each [['all', 'admin-filter-all'], ['chat', 'admin-filter-chat'], ['other', 'admin-filter-other'], ['alias', 'admin-filter-aliases'], ['configured', 'admin-filter-configured']] as option}
							<button type="button" class="btn btn-xs {filter === option[0] ? 'btn-active' : ''}" onclick={() => (filter = option[0] as ModelFilter)}>{t(option[1])}</button>
						{/each}
					</div>
				</div>
				<div class="card-body gap-0 overflow-x-auto pt-3">
					<div class="grid min-w-[710px] grid-cols-[minmax(170px,1.6fr)_82px_108px_84px_128px_minmax(110px,1.1fr)_18px] items-center gap-2.5 border-b border-base-300 pb-2 text-[11px] uppercase tracking-wide text-base-content/50">
						<span>{t('admin-col-model')}</span><span>{t('admin-col-kind')}</span><span>{t('admin-col-price')}</span><span>{t('admin-col-context')}</span><span>{t('admin-col-reasoning')}</span><span>{t('admin-col-configured')}</span><span></span>
					</div>
					{#each visibleModels as model (model.name)}
						{#key JSON.stringify(model)}
							<AdminModelRow {model} currency={data.currency} allModels={data.all_models} onsave={saveModel} onclear={clearModel} />
						{/key}
					{/each}
				</div>
			</article>
		{/if}
	{/if}
</section>
