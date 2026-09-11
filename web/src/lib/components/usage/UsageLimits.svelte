<script lang="ts">
	import { dt, t } from '$lib/i18n.svelte';
	import { limitPercent, usageCost, usageInteger } from '$lib/usage';
	import type { UsageLimit } from '$lib/usage-types';

	let { limits, currency, timezone }: { limits: UsageLimit[]; currency: string; timezone: string } = $props();
	const dimensionKeys: Record<string, string> = { requests: 'limits-dim-requests', tokens: 'limits-dim-tokens', cost: 'limits-dim-cost-short' };
	const windowKeys: Record<string, string> = { hour: 'limits-win-hour', day: 'limits-win-day', week: 'limits-win-week', month: 'limits-win-month' };

	function amount(limit: UsageLimit, value: number): string {
		return limit.dimension === 'cost'
			? usageCost(value, currency)
			: usageInteger(value);
	}
</script>

<section class="card border border-base-300 bg-base-100">
	<div class="card-body gap-3 p-4">
		<h2 class="card-title text-base">{t('usage-limits-heading')}</h2>
		<div class="flex flex-col gap-3">
			{#each limits as limit (limit.dimension + limit.window + (limit.model ?? ''))}
				{@const percent = limitPercent(limit.used, limit.limit)}
				<div class="flex flex-col gap-1">
					<div class="flex items-baseline justify-between gap-2 text-sm">
						<span class="font-medium">{t(dimensionKeys[limit.dimension] ?? limit.dimension)} · {limit.model ?? t('limits-all-models')} · {t(windowKeys[limit.window] ?? limit.window)}</span>
						<span class="tabular-nums opacity-70">{amount(limit, limit.used)} / {amount(limit, limit.limit)}</span>
					</div>
					<progress class="progress w-full {percent >= 100 ? 'progress-error' : percent >= 90 ? 'progress-warning' : 'progress-primary'}" value={percent} max="100"></progress>
					<div class="flex items-baseline justify-between gap-2 text-xs opacity-60">
						<span>{t('usage-limit-used', { percent })}</span>
						<span>{t('usage-limit-refreshes', { time: dt(limit.refreshes_at, { timeZone: timezone, month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit', hour12: false }) })}</span>
					</div>
				</div>
			{/each}
		</div>
	</div>
</section>
