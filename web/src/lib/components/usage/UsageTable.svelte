<script lang="ts">
	import { t } from '$lib/i18n.svelte';
	import { usageCost, usageInteger } from '$lib/usage';
	import type { UsageGroup } from '$lib/usage-types';

	let { titleKey, keyKey, rows, showCost, currency, emptyLabel = '—' }: {
		titleKey: string;
		keyKey: string;
		rows: UsageGroup[];
		showCost: boolean;
		currency: string;
		emptyLabel?: string;
	} = $props();
</script>

<section class="card border border-base-300 bg-base-100">
	<div class="card-body gap-2 p-4">
		<h2 class="card-title text-base">{t(titleKey)}</h2>
		{#if rows.length === 0}
			<p class="text-sm text-base-content/60">{t('usage-no-activity')}</p>
		{:else}
			<div class="overflow-x-auto">
				<table class="table table-sm">
					<thead><tr>
						<th>{t(keyKey)}</th><th class="text-right">{t('usage-col-requests')}</th><th class="text-right">{t('usage-col-tokens')}</th>
						{#if showCost}<th class="text-right">{t('usage-col-cost')}</th>{/if}<th class="text-right">{t('usage-col-errors')}</th>
					</tr></thead>
					<tbody>{#each rows as row (row.key)}<tr>
						<td class="max-w-xs break-all font-mono">{row.label || emptyLabel}</td>
						<td class="text-right tabular-nums">{usageInteger(row.requests)}</td><td class="text-right tabular-nums">{usageInteger(row.total_tokens)}</td>
						{#if showCost}<td class="text-right tabular-nums">{usageCost(row.cost, currency)}</td>{/if}
						<td class:text-error={row.errors > 0} class="text-right tabular-nums">{usageInteger(row.errors)}</td>
					</tr>{/each}</tbody>
				</table>
			</div>
		{/if}
	</div>
</section>
