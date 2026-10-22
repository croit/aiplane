<script lang="ts">
	import { untrack } from 'svelte';
	import { agentsApi, type AgentError, type AgentVersion } from '#lib/agents.js';
	import {
		RANGE_DAYS,
		METRICS,
		analyticsQuery,
		bars,
		countRows,
		type AgentAnalytics,
		type Metric
	} from '#lib/agent-analytics.js';
	import { dt, n, t } from '#lib/i18n.svelte.js';
	import { usageCost, usageInteger } from '#lib/usage.js';
	import SegmentedControl from '#lib/components/ui/SegmentedControl.svelte';

	/**
	 * What the agent did over a period, from `GET …/analytics`: counts only,
	 * never what a visitor wrote. Builder test chats are not counted.
	 */
	let { agentId, versions }: { agentId: string; versions: AgentVersion[] } = $props();

	const W = 600;
	const H = 120;

	let days = $state<number>(30);
	let version = $state<number | null>(null);
	let metric = $state<Metric>('conversations');
	let data = $state<AgentAnalytics | null>(null);
	let error = $state<string | null>(null);
	let loading = $state(true);
	let requested = 0;

	async function load() {
		const mine = ++requested;
		loading = true;
		try {
			const next = await agentsApi.analytics(agentId, analyticsQuery(days, version, new Date()));
			if (mine !== requested) return;
			data = next;
			error = null;
		} catch (err) {
			if (mine === requested) error = (err as AgentError).message;
		} finally {
			if (mine === requested) loading = false;
		}
	}

	$effect(() => {
		void days;
		void version;
		untrack(() => void load());
	});

	const money = (v: number) => (data ? usageCost(v, data.currency) : String(v));
	const metricLabel = (m: Metric) => t(`agents-an-${m}`);
	const format = (m: Metric, v: number) => (m === 'cost' ? money(v) : usageInteger(v));

	const tiles = $derived(
		data
			? [
					{ title: t('agents-an-conversations'), value: usageInteger(data.conversations) },
					{ title: t('agents-an-turns'), value: usageInteger(data.turns) },
					{
						title: t('agents-an-subagents'),
						value: usageInteger(data.sub_agents.dispatched),
						desc: t('agents-an-subagents-desc', {
							finished: data.sub_agents.finished,
							incomplete: data.sub_agents.incomplete
						})
					},
					{ title: t('agents-an-gate-refusals'), value: usageInteger(data.gate_refusals.total), desc: t('agents-an-gate-refusals-desc') },
					{ title: t('agents-an-output-blocks'), value: usageInteger(data.output_blocks.total), desc: t('agents-an-output-blocks-desc') },
					{ title: t('agents-an-limit-refusals'), value: usageInteger(data.limit_refusals.total), desc: t('agents-an-limit-refusals-desc') },
					{ title: t('agents-an-handoffs'), value: usageInteger(data.human_handoffs), desc: t('agents-an-handoffs-desc') },
					{ title: t('agents-an-requests'), value: usageInteger(data.usage.requests), desc: t('agents-an-requests-desc') },
					{ title: t('agents-an-tokens'), value: usageInteger(data.usage.tokens) },
					...(data.usage.cost > 0 ? [{ title: t('agents-an-cost'), value: money(data.usage.cost) }] : [])
				]
			: []
	);

	const chart = $derived(data ? bars(data.daily, metric, W, H) : []);
	const peak = $derived(Math.max(0, ...chart.map((b) => b.value)));
	const tables = $derived(
		data
			? [
					{ title: t('agents-an-routes-chosen'), rows: countRows(data.routes_chosen) },
					{ title: t('agents-an-gate-by-route'), rows: countRows(data.gate_refusals.by_route) },
					{
						title: t('agents-an-gate-by-slot'),
						rows: data.gate_refusals.by_missing_slot.map((s): [string, number] => [`${s.route} · ${s.slot}`, s.count])
					},
					{ title: t('agents-an-incomplete-reasons'), rows: countRows(data.sub_agents.incomplete_by_reason) },
					{ title: t('agents-an-output-actions'), rows: countRows(data.output_blocks.by_action) },
					{ title: t('agents-an-limit-kinds'), rows: countRows(data.limit_refusals.by_kind) }
				]
			: []
	);
</script>

<div class="space-y-4">
	<div class="flex flex-wrap items-end gap-3">
		<label class="fieldset">
			<span class="fieldset-legend text-xs">{t('agents-an-range')}</span>
			<select class="select select-sm" bind:value={days}>
				{#each RANGE_DAYS as d (d)}<option value={d}>{t('agents-an-range-days', { count: d })}</option>{/each}
			</select>
		</label>
		<label class="fieldset">
			<span class="fieldset-legend text-xs">{t('agents-an-version')}</span>
			<select class="select select-sm" bind:value={version}>
				<option value={null}>{t('agents-an-version-all')}</option>
				{#each versions as v (v.version)}<option value={v.version}>{t('agents-an-version-option', { version: v.version })}</option>{/each}
			</select>
		</label>
		{#if loading}<span class="loading loading-spinner loading-sm" aria-hidden="true"></span>{/if}
	</div>

	{#if error}<div class="alert alert-error text-sm" role="alert"><span>{error}</span></div>{/if}

	{#if data}
		{#if version !== null}<div class="alert alert-info text-sm"><span>{t('agents-an-unversioned-note')}</span></div>{/if}

		<div class="grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-5">
			{#each tiles as tile (tile.title)}
				<div class="stats border border-base-300 bg-base-100 shadow">
					<div class="stat p-4">
						<div class="stat-title">{tile.title}</div>
						<div class="stat-value text-2xl font-mono tabular-nums">{tile.value}</div>
						{#if tile.desc}<div class="stat-desc whitespace-normal">{tile.desc}</div>{/if}
					</div>
				</div>
			{/each}
		</div>

		<div class="card card-border">
			<div class="card-body gap-3 p-4">
				<div class="flex flex-wrap items-center justify-between gap-2">
					<h3 class="card-title text-base">{t('agents-an-chart-title')}</h3>
					<SegmentedControl options={METRICS.map((m) => ({ value: m, label: metricLabel(m) }))} bind:value={metric} label={t('agents-an-metric')} size="sm" />
				</div>
				{#if peak === 0}
					<p class="text-sm text-base-content/60">{t('agents-an-chart-empty')}</p>
				{:else}
					<svg viewBox={`0 0 ${W} ${H}`} class="h-40 w-full" role="img" aria-label={`${t('agents-an-chart-title')}: ${metricLabel(metric)}`} preserveAspectRatio="none">
						{#each chart as b (b.day)}
							<rect x={b.x} y={b.y} width={b.width} height={b.height} class="fill-primary" rx="1">
								<title>{dt(b.day, { dateStyle: 'medium', timeZone: 'UTC' })}: {format(metric, b.value)}</title>
							</rect>
						{/each}
					</svg>
					<div class="flex justify-between text-xs font-mono text-base-content/60 tabular-nums">
						<span>{dt(chart[0].day, { dateStyle: 'medium', timeZone: 'UTC' })}</span>
						<span>{format(metric, peak)}</span>
						<span>{dt(chart[chart.length - 1].day, { dateStyle: 'medium', timeZone: 'UTC' })}</span>
					</div>
				{/if}
			</div>
		</div>

		<div class="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
			{#each tables as table (table.title)}
				<div class="card card-border">
					<div class="card-body gap-2 p-4">
						<h3 class="card-title text-sm">{table.title}</h3>
						{#if table.rows.length === 0}
							<p class="text-sm text-base-content/60">{t('agents-an-none')}</p>
						{:else}
							<table class="table table-sm">
								<tbody>
									{#each table.rows as [name, count] (name)}
										<tr><td class="break-all font-mono">{name}</td><td class="text-right font-mono tabular-nums">{n(count)}</td></tr>
									{/each}
								</tbody>
							</table>
						{/if}
					</div>
				</div>
			{/each}
		</div>
	{/if}
</div>
