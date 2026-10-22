<script lang="ts">
	import type { Snippet } from 'svelte';
	import { dt, t } from '#lib/i18n.svelte.js';
	import { limitPercent, usageCost, usageInteger } from '#lib/usage.js';
	import type { UsageLimit } from '#lib/usage-types.js';

	let { limits, currency, timezone, action = null }: {
		limits: UsageLimit[];
		currency: string;
		timezone: string;
		action?: Snippet<[number]> | null;
	} = $props();
	const dimensionKeys: Record<string, string> = { requests: 'limits-dim-requests', tokens: 'limits-dim-tokens', cost: 'limits-dim-cost-short' };
	const windowKeys: Record<string, string> = { hour: 'limits-win-hour', day: 'limits-win-day', week: 'limits-win-week', month: 'limits-win-month' };

	function amount(limit: UsageLimit, value: number): string {
		return limit.dimension === 'cost'
			? usageCost(value, currency)
			: usageInteger(value);
	}
</script>

<div class="flex flex-col gap-3">
	{#each limits as limit, index (limit.dimension + limit.window + (limit.model ?? '') + index)}
		{@const percent = limitPercent(limit.used, limit.limit)}
		<div class="flex flex-col gap-1">
			<div class="flex items-baseline justify-between gap-2 text-sm">
				<span class="font-medium">{t(dimensionKeys[limit.dimension] ?? limit.dimension)} · {limit.model ?? t('limits-all-models')} · {t(windowKeys[limit.window] ?? limit.window)}</span>
				<span class="flex items-baseline gap-2"><span class="font-mono tabular-nums opacity-70">{amount(limit, limit.used)} / {amount(limit, limit.limit)}</span>{@render action?.(index)}</span>
			</div>
			<progress class="progress w-full {percent >= 100 ? 'progress-error' : percent >= 90 ? 'progress-warning' : 'progress-primary'}" value={percent} max="100"></progress>
			{#if limit.refreshes_at}
				<div class="flex items-baseline justify-between gap-2 text-xs opacity-60">
					<span>{t('usage-limit-used', { percent })}</span>
					<span>{t('usage-limit-refreshes', { time: dt(limit.refreshes_at, { timeZone: timezone, month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit', hour12: false }) })}</span>
				</div>
			{/if}
		</div>
	{/each}
</div>
