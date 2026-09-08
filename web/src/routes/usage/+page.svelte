<script lang="ts">
	import { onMount } from 'svelte';
	import { api } from '$lib/api';
	import type { UsageResponse, UsageGroup } from '$lib/usage-types';

	let data = $state<UsageResponse | null>(null);
	let error = $state<string | null>(null);
	let period = $state('today');
	let allUsers = $state(false);

	const PERIODS: [string, string][] = [
		['today', 'Today'],
		['24h', 'Last 24 h'],
		['this_week', 'This week'],
		['last_week', 'Last week'],
		['this_month', 'This month'],
		['last_month', 'Last month']
	];

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
	<h1 class="text-2xl font-bold">Usage</h1>
	<div class="flex gap-2 items-center">
		{#if data?.scope === 'all'}
			<span class="badge badge-outline badge-sm">all users</span>
		{/if}
		<label class="label cursor-pointer gap-2 text-sm">
			<span class="label-text">All users</span>
			<input type="checkbox" class="toggle toggle-sm" bind:checked={allUsers} />
		</label>
		<select class="select select-bordered select-sm" bind:value={period} aria-label="Period">
			{#each PERIODS as [value, label] (value)}
				<option {value}>{label}</option>
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
				Models with traffic but no configured price (spend under-counted):
				{data.unpriced_models.join(', ')}
			</span>
		</div>
	{/if}

	{@const cards = [
			{ label: 'Requests', value: data.summary.requests.toLocaleString() },
			{ label: 'Tokens', value: data.summary.total_tokens.toLocaleString() },
			...(data.summary.total_cost > 0
				? [{ label: `Cost (${data.currency})`, value: data.summary.total_cost.toFixed(2) }]
				: []),
			{ label: 'Errors', value: data.summary.errors.toLocaleString() },
			...(data.scope === 'all'
				? [{ label: 'Users', value: data.summary.unique_users.toLocaleString() }]
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
				<h2 class="card-title text-base">Your limits</h2>
				{#each data.limits as limit (limit.dimension + limit.window + (limit.model ?? ''))}
					<div class="mb-2">
						<div class="flex justify-between text-xs mb-1">
							<span>
								{limit.dimension} / {limit.window}{limit.model ? ` · ${limit.model}` : ''}
							</span>
							<span class="text-base-content/60">
								{limit.used.toFixed(0)} / {limit.limit.toFixed(0)}
							</span>
						</div>
						<progress class="progress {pct(limit.used, limit.limit) >= 90 ? 'progress-error' : 'progress-primary'} w-full" value={pct(limit.used, limit.limit)} max={100}></progress>
					</div>
				{/each}
			</div>
		</div>
	{/if}

	{@const sections = [
			{ title: 'By model', groups: data.by_model },
			{ title: 'By source', groups: data.by_source },
			{ title: 'By backend', groups: data.by_backend },
			{ title: 'By token', groups: data.by_token }
		]}
	{#each sections as section (section.title)}
		{#if section.groups.length > 0}
			<div class="card border border-base-300 mb-4">
				<div class="card-body">
					<h2 class="card-title text-base">{section.title}</h2>
					<div class="overflow-x-auto">
						<table class="table table-sm">
							<thead>
								<tr><th>Name</th><th>Requests</th><th>Tokens</th><th>Errors</th>{#if data.summary.total_cost > 0}<th>Cost</th>{/if}</tr>
							</thead>
							<tbody>
								{#each section.groups as g (g.key)}
									<tr>
										<td>{g.label || g.key || '(no token)'}</td>
										<td>{g.requests.toLocaleString()}</td>
										<td>{g.total_tokens.toLocaleString()}</td>
										<td>{g.errors.toLocaleString()}</td>
										{#if data.summary.total_cost > 0}<td>{g.cost.toFixed(2)}</td>{/if}
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
				<p class="text-base-content/60">No usage in this period.</p>
			</div>
		</div>
	{/if}
{/if}
