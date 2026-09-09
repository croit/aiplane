<script lang="ts">
	import { onMount } from 'svelte';
	import { api } from '$lib/api';
	import type { UsageResponse } from '$lib/usage-types';
	import { t, n } from '$lib/i18n.svelte';

	let data = $state<UsageResponse | null>(null);
	let error = $state<string | null>(null);
	let period = $state('today');
	let allUsers = $state(false);

	// Keys, not labels: the picker re-renders on a language switch because the
	// lookup happens in the template.
	const PERIODS: [string, string][] = [
		['today', 'usage-period-today'],
		['24h', 'usage-period-24h'],
		['this_week', 'usage-period-this-week'],
		['last_week', 'usage-period-last-week'],
		['this_month', 'usage-period-this-month'],
		['last_month', 'usage-period-last-month']
	];

	/** Money: always two decimals, but with the locale's own separators. */
	function money(value: number): string {
		return n(value, { minimumFractionDigits: 2, maximumFractionDigits: 2 });
	}

	async function refresh() {
		error = null;
		try {
			data = await api.usage({
				period,
				scope: allUsers ? 'all' : 'self'
			});
			// A non-admin's ?scope=all is clamped server-side; reflect reality.
			if (allUsers && data.scope === 'self') allUsers = false;
		} catch (err) {
			error = String(err);
		}
	}

	function pct(used: number, limit: number): number {
		if (limit <= 0) return 100;
		return Math.min(100, Math.round((used / limit) * 100));
	}

	$effect(() => {
		void period;
		void allUsers;
		void refresh();
	});

	onMount(refresh);
</script>

<div class="flex items-center justify-between mb-4 gap-2 flex-wrap">
	<h1 class="text-2xl font-bold">{t('nav-usage')}</h1>
	<div class="flex gap-2 items-center">
		{#if data?.scope === 'all'}
			<span class="badge badge-outline badge-sm">{t('usage-toggle-all')}</span>
		{/if}
		<label class="label cursor-pointer gap-2 text-sm">
			<span class="label-text">{t('usage-toggle-all')}</span>
			<input type="checkbox" class="toggle toggle-sm" bind:checked={allUsers} />
		</label>
		<select
			class="select select-bordered select-sm"
			bind:value={period}
			aria-label={t('usage-filter-period')}
		>
			{#each PERIODS as [value, label] (value)}
				<option {value}>{t(label)}</option>
			{/each}
		</select>
	</div>
</div>

{#if error}
	<div class="alert alert-error mb-4"><span>{error}</span></div>
{/if}

{#if data}
	{#if data.unpriced_models.length > 0}
		<div class="alert alert-warning mb-4 text-sm">
			<span>
				{t('usage-unpriced-warning', { models: data.unpriced_models.join(', ') })}
			</span>
		</div>
	{/if}

	{@const cards = [
			{ label: t('usage-stat-requests-title'), value: n(data.summary.requests) },
			{ label: t('usage-stat-tokens-title'), value: n(data.summary.total_tokens) },
			...(data.summary.total_cost > 0
				? [
						{
							label: t('limits-dim-cost', { cur: data.currency }),
							value: money(data.summary.total_cost)
						}
					]
				: []),
			{ label: t('usage-stat-errors-title'), value: n(data.summary.errors) },
			...(data.scope === 'all'
				? [{ label: t('usage-stat-users-title'), value: n(data.summary.unique_users) }]
				: [])
		]}
	<div class="grid grid-cols-2 sm:grid-cols-5 gap-3 mb-6">
		{#each cards as card (card.label)}
			<div class="card border border-base-300">
				<div class="card-body p-3">
					<div class="text-xs text-base-content/60">{card.label}</div>
					<div class="text-lg font-semibold">{card.value}</div>
				</div>
			</div>
		{/each}
	</div>

	{#if data.limits.length > 0}
		<div class="card border border-base-300 mb-6">
			<div class="card-body">
				<h2 class="card-title text-base">{t('usage-limits-heading')}</h2>
				{#each data.limits as limit (limit.dimension + limit.window + (limit.model ?? ''))}
					<div class="mb-2">
						<div class="flex justify-between text-xs mb-1">
							<span>
								{limit.dimension} / {limit.window}{limit.model ? ` · ${limit.model}` : ''}
							</span>
							<span class="text-base-content/60">
								{n(limit.used, { maximumFractionDigits: 0 })} / {n(limit.limit, {
									maximumFractionDigits: 0
								})}
							</span>
						</div>
						<progress class="progress {pct(limit.used, limit.limit) >= 90 ? 'progress-error' : 'progress-primary'} w-full" value={pct(limit.used, limit.limit)} max={100}></progress>
					</div>
				{/each}
			</div>
		</div>
	{/if}

	{@const sections = [
			{ title: t('usage-table-by-model'), groups: data.by_model },
			{ title: t('usage-table-by-source'), groups: data.by_source },
			{ title: t('usage-table-by-backend'), groups: data.by_backend },
			{ title: t('usage-table-by-token'), groups: data.by_token }
		]}
	{#each sections as section (section.title)}
		{#if section.groups.length > 0}
			<div class="card border border-base-300 mb-4">
				<div class="card-body">
					<h2 class="card-title text-base">{section.title}</h2>
					<div class="overflow-x-auto">
						<table class="table table-sm">
							<thead>
								<tr><th>{t('usage-col-name')}</th><th>{t('usage-col-requests')}</th><th>{t('usage-col-tokens')}</th><th>{t('usage-col-errors')}</th>{#if data.summary.total_cost > 0}<th>{t('usage-col-cost')}</th>{/if}</tr>
							</thead>
							<tbody>
								{#each section.groups as g (g.key)}
									<tr>
										<td>{g.label || g.key || t('usage-token-none')}</td>
										<td>{n(g.requests)}</td>
										<td>{n(g.total_tokens)}</td>
										<td>{n(g.errors)}</td>
										{#if data.summary.total_cost > 0}<td>{money(g.cost)}</td>{/if}
									</tr>
								{/each}
							</tbody>
						</table>
					</div>
				</div>
			</div>
		{/if}
	{/each}

	{#if data.summary.requests === 0}
		<div class="card border border-base-300">
			<div class="card-body">
				<p class="text-base-content/60">{t('usage-no-activity')}</p>
			</div>
		</div>
	{/if}
{/if}
