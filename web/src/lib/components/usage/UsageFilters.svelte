<script lang="ts">
	import { t } from '$lib/i18n.svelte';
	import { USAGE_PERIODS, USAGE_SOURCES, type UsageFilters } from '$lib/usage';
	import type { UsageResponse } from '$lib/usage-types';

	let { filters, data, onchange }: {
		filters: UsageFilters;
		data: UsageResponse;
		onchange: (key: keyof UsageFilters, value: string) => void;
	} = $props();
	let selectedTokenOffered = $derived(!filters.token || filters.token === 'none' || data.tokens.some((token) => token.id === filters.token));
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
		<select aria-label={t('usage-filter-backend')} class="select select-bordered select-sm" value={filters.backend} onchange={(event) => onchange('backend', event.currentTarget.value)}>
			<option value="">{t('usage-backend-all')}</option>
			{#each data.backends as backend (backend)}<option value={backend}>{backend}</option>{/each}
		</select>
	</label>
	<label class="flex flex-col gap-1">
		<span class="label-text text-xs text-base-content/60">{t('usage-filter-token')}</span>
		<select aria-label={t('usage-filter-token')} class="select select-bordered select-sm" value={filters.token} onchange={(event) => onchange('token', event.currentTarget.value)}>
			<option value="">{t('usage-token-all')}</option>
			<option value="none">{t('usage-token-none')}</option>
			{#each data.tokens as token (token.id)}<option value={token.id}>{token.label}</option>{/each}
			{#if !selectedTokenOffered}<option value={filters.token}>{filters.token}</option>{/if}
		</select>
	</label>
</form>
