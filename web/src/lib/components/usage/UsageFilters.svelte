<script lang="ts">
	import { t } from '$lib/i18n.svelte';
	import SearchableSelect from '$lib/components/SearchableSelect.svelte';
	import { USAGE_PERIODS, USAGE_SOURCES, type UsageFilters } from '$lib/usage';
	import type { UsageResponse } from '$lib/usage-types';

	let { filters, data, onchange }: {
		filters: UsageFilters;
		data: UsageResponse;
		onchange: (key: keyof UsageFilters, value: string) => void;
	} = $props();
	let selectedTokenOffered = $derived(!filters.token || filters.token === 'none' || data.tokens.some((token) => token.id === filters.token));
	let backendOptions = $derived([{ value: '', label: t('usage-backend-all') }, ...data.backends.map((backend) => ({ value: backend, label: backend }))]);
	let tokenOptions = $derived([
		{ value: '', label: t('usage-token-all') },
		{ value: 'none', label: t('usage-token-none') },
		...data.tokens.map((token) => ({ value: token.id, label: token.label, keywords: [token.id] })),
		...(!selectedTokenOffered ? [{ value: filters.token, label: filters.token }] : [])
	]);
</script>

<form class="flex flex-wrap items-end gap-3" onsubmit={(event) => event.preventDefault()}>
	<label class="flex flex-col gap-1">
		<span class="label-text text-xs text-base-content/60">{t('usage-filter-period')}</span>
		<select aria-label={t('usage-filter-period')} class="select select-bordered select-sm" value={filters.period} onchange={(event) => onchange('period', event.currentTarget.value)}>
			{#each USAGE_PERIODS as [value, label] (value)}<option {value}>{t(label)}</option>{/each}
		</select>
	</label>
	<label class="flex flex-col gap-1">
		<span class="label-text text-xs text-base-content/60">{t('usage-filter-source')}</span>
		<select aria-label={t('usage-filter-source')} class="select select-bordered select-sm" value={filters.source} onchange={(event) => onchange('source', event.currentTarget.value)}>
			{#each USAGE_SOURCES as [value, label] (value)}<option {value}>{t(label)}</option>{/each}
		</select>
	</label>
	<label class="flex flex-col gap-1">
		<span class="label-text text-xs text-base-content/60">{t('usage-filter-backend')}</span>
		<SearchableSelect options={backendOptions} value={filters.backend} onchange={(value) => onchange('backend', value)} ariaLabel={t('usage-filter-backend')} size="sm" class="w-56" />
	</label>
	<label class="flex flex-col gap-1">
		<span class="label-text text-xs text-base-content/60">{t('usage-filter-token')}</span>
		<SearchableSelect options={tokenOptions} value={filters.token} onchange={(value) => onchange('token', value)} ariaLabel={t('usage-filter-token')} size="sm" class="w-64" />
	</label>
</form>
