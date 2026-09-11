<script lang="ts">
	import { onMount } from 'svelte';
	import type { AdminToken } from '$lib/admin-tokens';
	import { adminJson, adminPut } from '$lib/admin-client';
	import AdminTokenRow from '$lib/components/admin/AdminTokenRow.svelte';
	import { t } from '$lib/i18n.svelte';

	interface TokensData {
		tokens: AdminToken[];
		models: string[];
		usage_enabled: boolean;
		currency: string;
		timezone: string;
	}

	let data = $state<TokensData | null>(null);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);

	async function refresh() {
		try {
			data = await adminJson<TokensData>('/api/v0/admin/tokens');
			error = null;
		} catch (caught) {
			error = String(caught);
		}
	}

	async function saveModels(id: string, restrict: boolean, models: string[]) {
		try {
			await adminPut(`/api/v0/admin/tokens/${encodeURIComponent(id)}/models`, { restrict, models });
			notice = restrict
				? t('admin-tokens-models-saved-toast', { count: models.length })
				: t('admin-tokens-models-cleared-toast');
			await refresh();
		} catch (caught) {
			notice = null;
			throw caught;
		}
	}

	onMount(refresh);
</script>

<section class="flex w-full flex-col gap-4">
	<header class="flex flex-col gap-2">
		<h1 class="text-2xl font-bold">{t('admin-tokens-heading')}</h1>
		<p class="text-sm text-base-content/70">{t('admin-tokens-blurb')}</p>
	</header>

	{#if error}<div class="alert alert-error"><span>{error}</span></div>{/if}
	{#if notice}<div class="alert alert-success"><span>{notice}</span></div>{/if}

	{#if data}
		<article class="card border border-base-300 bg-base-100">
			<div class="card-body gap-2 p-4">
				{#if data.tokens.length === 0}
					<p class="text-sm text-base-content/60">{t('admin-tokens-none')}</p>
				{:else}
					<div class="overflow-x-auto"><table class="table table-sm min-w-[84rem]">
						<thead><tr>
							<th class="w-64">{t('admin-tokens-col-name')}</th><th class="w-56">{t('admin-tokens-col-owner')}</th><th class="w-28">{t('admin-tokens-col-state')}</th><th class="w-64">{t('admin-tokens-col-dates')}</th>
							<th class="w-24 text-right">{t('usage-col-requests')}</th><th class="w-24 text-right">{t('usage-col-tokens')}</th><th class="w-28 text-right">{t('usage-col-cost')}</th><th>{t('admin-tokens-col-scope')}</th>
						</tr></thead>
						<tbody>{#each data.tokens as token (token.id)}<AdminTokenRow {token} models={data.models} usageEnabled={data.usage_enabled} currency={data.currency} timezone={data.timezone} onsave={saveModels} />{/each}</tbody>
					</table></div>
					<p class="text-xs text-base-content/60">{t('admin-tokens-count', { count: data.tokens.length })}</p>
				{/if}
			</div>
		</article>
	{/if}
</section>
