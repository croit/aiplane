<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import { adminJson, adminPut } from '#lib/admin-client.js';
	import { catalogSections, matchesModelFilter, selectedModelAdminTab } from '#lib/admin-models.js';
	import type { AdminModel, AdminModelsData, CatalogSections, ModelFilter } from '#lib/admin-models.js';
	import AdminModelRow from '#lib/components/admin/AdminModelRow.svelte';
	import DefaultModelsCard from '#lib/components/admin/DefaultModelsCard.svelte';
	import AutomaticRoutesCard from '#lib/components/admin/AutomaticRoutesCard.svelte';
	import UpstreamsPanel from '#lib/components/admin/UpstreamsPanel.svelte';
	import { t } from '#lib/i18n.svelte.js';

	let data = $state<AdminModelsData | null>(null);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);

	// The per-model editor lives on its own route and reports back through the
	// URL, so its confirmation shows up on the list the operator returns to.
	$effect(() => {
		const kind = page.url.searchParams.get('notice');
		const model = page.url.searchParams.get('model') ?? '';
		if (kind === 'saved') notice = t('admin-saved-model', { model });
		else if (kind === 'cleared') notice = t('admin-cleared-defaults', { model });
	});

	let query = $state('');
	// The catalog opens on the models that are actually served; aliases and
	// dead overrides are one click away.
	let filter = $state<ModelFilter>('active');

	async function refresh() {
		try {
			data = await adminJson<AdminModelsData>('/api/v0/admin/models');
			error = null;
		} catch (caught) {
			error = String(caught);
		}
	}

	async function saveDefault(feature: string, model: string) {
		await adminPut('/api/v0/admin/model-defaults', { feature, model });
		notice = model ? t('admin-defaults-saved', { model }) : t('admin-defaults-cleared');
		await refresh();
	}

	// The catalog's three sections in operator order — what is running, what
	// points at it, what nothing serves any more. The filter applies within
	// the sections; a section whose rows all fall out is hidden.
	let visibleSections = $derived(
		data
			? (Object.entries(catalogSections(data.models)) as [keyof CatalogSections, AdminModel[]][])
					.map(([key, models]) => ({ key, models: models.filter((model) => matchesModelFilter(model, filter, query)) }))
					.filter((section) => section.models.length > 0)
			: []
	);
	let selected = $derived(selectedModelAdminTab(page.url.search));
	onMount(refresh);
</script>

<svelte:head><title>{t('admin-page-title')}</title></svelte:head>

<section class="flex w-full flex-col gap-4">
	<header class="flex flex-col gap-3">
		<div>
			<h1 class="text-2xl font-bold">{t('admin-models-routing-heading')}</h1>
			<p class="max-w-2xl text-sm text-base-content/70">{t('admin-models-routing-intro')}</p>
		</div>
		<nav class="tabs tabs-border w-full overflow-x-auto" aria-label={t('admin-models-routing-heading')}>
			<a class:tab-active={selected === 'upstreams'} class="tab whitespace-nowrap" href="/admin/models?tab=upstreams">{t('admin-models-tab-upstreams')}</a>
			<a class:tab-active={selected === 'catalog'} class="tab whitespace-nowrap" href="/admin/models?tab=catalog">{t('admin-models-tab-catalog')}</a>
			<a class:tab-active={selected === 'defaults'} class="tab whitespace-nowrap" href="/admin/models?tab=defaults">{t('admin-models-tab-defaults')}</a>
			<a class:tab-active={selected === 'routing'} class="tab whitespace-nowrap" href="/admin/models?tab=routing">{t('admin-models-tab-routing')}</a>
		</nav>
	</header>

	{#if error}<div class="alert alert-error"><span>{error}</span></div>{/if}
	{#if notice}<div class="alert alert-success"><span>{notice}</span></div>{/if}
	{#if selected === 'upstreams'}
		<UpstreamsPanel />
	{:else if data}
		{#if selected === 'defaults'}
			<DefaultModelsCard defaults={data.feature_defaults} onsave={saveDefault} />
		{:else if selected === 'routing'}
			<AutomaticRoutesCard />
		{:else}
		{#if data.models.length === 0}
			<div class="alert"><span>{t('admin-no-models')}</span></div>
		{:else}
			<article class="card">
				<div class="card-body gap-3 pb-0">
					<div class="flex flex-wrap items-center gap-2">
						<input type="search" class="input input-sm w-60 max-w-full" bind:value={query} placeholder={t('admin-filter-placeholder')} aria-label={t('admin-filter-placeholder')} />
						{#each [['active', 'admin-filter-active'], ['all', 'admin-filter-all'], ['alias', 'admin-filter-aliases'], ['inactive', 'admin-filter-inactive'], ['chat', 'admin-filter-chat'], ['other', 'admin-filter-other'], ['configured', 'admin-filter-configured']] as option}
							<button type="button" class="btn btn-xs {filter === option[0] ? 'btn-primary' : 'btn-ghost'}" onclick={() => (filter = option[0] as ModelFilter)}>{t(option[1])}</button>
						{/each}
					</div>
				</div>
				<div class="card-body gap-0 overflow-x-auto pt-3">
					<div class="grid min-w-[752px] grid-cols-[minmax(170px,1.6fr)_82px_150px_84px_128px_minmax(110px,1.1fr)_64px] items-center gap-2.5 border-b border-base-300 pb-2 text-[11px] uppercase tracking-wide text-base-content/50">
						<span>{t('admin-col-model')}</span><span>{t('admin-col-kind')}</span><span>{t('admin-col-price')}</span><span>{t('admin-col-context')}</span><span>{t('admin-col-reasoning')}</span><span>{t('admin-col-configured')}</span><span></span>
					</div>
					{#if visibleSections.length === 0}
						<div class="py-8 text-center text-sm text-base-content/50">{t('admin-no-models-match')}</div>
					{:else}
						{#each visibleSections as section (section.key)}
							<div class="border-b border-base-300 bg-base-200/50 px-3 py-1.5 text-[11px] font-semibold uppercase tracking-wide text-base-content/60">{t(`admin-filter-${section.key}`)}</div>
							{#each section.models as model (model.name)}
								{#key JSON.stringify(model)}
									<AdminModelRow {model} currency={data.currency} />
								{/key}
							{/each}
						{/each}
					{/if}
				</div>
			</article>
		{/if}
		{/if}
	{/if}
</section>
