<script lang="ts">
	import { onMount } from 'svelte';
	import { api } from '$lib/api';
	import UsageFiltersView from '$lib/components/usage/UsageFilters.svelte';
	import UsageLimits from '$lib/components/usage/UsageLimits.svelte';
	import UsageStats from '$lib/components/usage/UsageStats.svelte';
	import UsageTable from '$lib/components/usage/UsageTable.svelte';
	import { t } from '$lib/i18n.svelte';
	import { usageSearch, type UsageFilters } from '$lib/usage';
	import type { UsageResponse } from '$lib/usage-types';

	let data = $state<UsageResponse | null>(null);
	let error = $state<string | null>(null);
	let request = 0;
	let filters = $state<UsageFilters>({ period: 'today', scope: 'self', source: '', backend: '', token: '' });

	async function refresh(replaceUrl = true) {
		const current = ++request;
		error = null;
		if (replaceUrl) history.replaceState(history.state, '', `/usage${usageSearch(filters)}`);
		try {
			const next = await api.usage(filters);
			if (current !== request) return;
			data = next;
			if (filters.scope !== next.scope) {
				filters.scope = next.scope;
				history.replaceState(history.state, '', `/usage${usageSearch(filters)}`);
			}
		} catch (caught) {
			if (current === request) error = String(caught);
		}
	}

	function change(key: keyof UsageFilters, value: string) {
		filters = { ...filters, [key]: value } as UsageFilters;
		void refresh();
	}

	function readUrl() {
		const params = new URLSearchParams(location.search);
		filters = {
			period: params.get('period') || 'today',
			scope: params.get('scope') === 'all' ? 'all' : 'self',
			source: params.get('source') || '',
			backend: params.get('backend') || '',
			token: params.get('token') || ''
		};
	}

	onMount(() => {
		readUrl();
		void refresh(false);
		const restore = () => { readUrl(); void refresh(false); };
		addEventListener('popstate', restore);
		return () => removeEventListener('popstate', restore);
	});
</script>

<svelte:head><title>{t(data?.scope === 'all' ? 'usage-title-all' : 'usage-title-mine')}</title></svelte:head>

<section class="flex w-full flex-col gap-4">
	<header class="flex flex-col gap-2">
		<div class="flex flex-wrap items-start justify-between gap-3">
			<h1 class="text-2xl font-bold">{t(data?.scope === 'all' ? 'usage-heading-all' : 'usage-heading-mine')}</h1>
			{#if data?.can_view_all}
				<div class="join">
					<button class="btn join-item btn-sm" class:btn-active={filters.scope === 'self'} class:btn-primary={filters.scope === 'self'} onclick={() => change('scope', 'self')}>{t('usage-toggle-mine')}</button>
					<button class="btn join-item btn-sm" class:btn-active={filters.scope === 'all'} class:btn-primary={filters.scope === 'all'} onclick={() => change('scope', 'all')}>{t('usage-toggle-all')}</button>
				</div>
			{/if}
		</div>
		<p class="text-sm text-base-content/70">{t(data?.scope === 'all' ? 'usage-blurb-all' : 'usage-blurb-mine')}</p>
	</header>

	{#if error}<div class="alert alert-error"><span>{error}</span></div>{/if}
	{#if data}
		{#if !data.usage_enabled}
			<div class="alert alert-warning"><span>{t('usage-metrics-disabled-prefix')}<code class="text-xs">[usage].enabled = false</code>{t('usage-metrics-disabled-suffix')}</span></div>
		{/if}
		{#if data.unpriced_models.length > 0}
			<div class="alert alert-warning"><span>{t('usage-unpriced-warning', { models: data.unpriced_models.join(', ') })}</span></div>
		{/if}
		{#if data.scope === 'self' && data.limits.length > 0}<UsageLimits limits={data.limits} currency={data.currency} timezone={data.timezone} />{/if}
		<UsageFiltersView {filters} {data} onchange={change} />
		<UsageStats summary={data.summary} allUsers={data.scope === 'all'} currency={data.currency} />
		<div class="grid grid-cols-1 gap-4 lg:grid-cols-2">
			{#if data.scope === 'all'}<UsageTable titleKey="usage-table-by-user" keyKey="usage-key-user" rows={data.by_user} showCost={data.summary.total_cost > 0} currency={data.currency} />{/if}
			<UsageTable titleKey="usage-table-by-backend" keyKey="usage-key-backend" rows={data.by_backend} showCost={data.summary.total_cost > 0} currency={data.currency} />
			<UsageTable titleKey="usage-table-by-source" keyKey="usage-key-source" rows={data.by_source} showCost={data.summary.total_cost > 0} currency={data.currency} />
			<UsageTable titleKey="usage-table-by-model" keyKey="usage-key-model" rows={data.by_model} showCost={data.summary.total_cost > 0} currency={data.currency} />
			<UsageTable titleKey="usage-table-by-token" keyKey="usage-key-token" rows={data.by_token} showCost={data.summary.total_cost > 0} currency={data.currency} emptyLabel={t('usage-token-none')} />
		</div>
	{:else if !error}
		<div class="skeleton h-48 w-full"></div>
	{/if}
</section>
