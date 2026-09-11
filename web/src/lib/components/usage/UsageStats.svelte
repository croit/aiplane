<script lang="ts">
	import { t } from '$lib/i18n.svelte';
	import { usageCost, usageInteger } from '$lib/usage';
	import type { UsageSummary } from '$lib/usage-types';

	let { summary, allUsers, currency }: { summary: UsageSummary; allUsers: boolean; currency: string } = $props();
	let showCost = $derived(summary.total_cost > 0);
	let cards = $derived([
		{ title: t('usage-stat-requests-title'), value: usageInteger(summary.requests), description: t('usage-stat-requests-desc') },
		{ title: t('usage-stat-tokens-title'), value: usageInteger(summary.total_tokens), description: t('usage-stat-tokens-desc') },
		...(showCost ? [{ title: t('usage-stat-cost-title'), value: usageCost(summary.total_cost, currency), description: t('usage-stat-cost-desc') }] : []),
		...(allUsers ? [{ title: t('usage-stat-users-title'), value: usageInteger(summary.unique_users), description: t('usage-stat-users-desc') }] : []),
		{ title: t('usage-stat-errors-title'), value: usageInteger(summary.errors), description: t('usage-stat-errors-desc') }
	]);
</script>

<div class="stats stats-vertical w-full border border-base-300 bg-base-100 shadow sm:stats-horizontal">
	{#each cards as card (card.title)}
		<div class="stat">
			<div class="stat-title">{card.title}</div>
			<div class="stat-value text-2xl tabular-nums">{card.value}</div>
			<div class="stat-desc">{card.description}</div>
		</div>
	{/each}
</div>
