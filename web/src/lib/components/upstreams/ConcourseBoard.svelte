<script lang="ts">
	import { untrack } from 'svelte';
	import { t, n } from '#lib/i18n.svelte.js';
	import FlapValue from './FlapValue.svelte';
	import { boardTone, type BoardTone } from './board-state.js';
	import {
		activityCounts,
		boardGroups,
		boardPoolMatches,
		boardRowMatches,
		liveForPool,
		naturalSort,
		parallelismMismatch,
		poolCoverage,
		poolProblems,
		type Backend,
		type Coverage,
		type LiveBackend,
		type Pool,
		type PoolProblem
	} from '#lib/upstreams.js';

	interface Props {
		backends: Backend[];
		pools: Pool[];
		usage: Record<string, number[]>;
		coverage: Record<string, Coverage[]>;
		/** A model name from a catalog deep link, applied to the search field. */
		initialQuery?: string;
		onEdit: (backend: Backend) => void;
		onEditPool: (pool: Pool) => void;
		onAddBackend: (pool: Pool) => void;
		onFixModels: (backend: Backend) => void;
		onToggleEnabled: (backend: Backend) => void;
		onOpenRouterAccount: () => void;
		toggling: string | null;
	}

	let {
		backends,
		pools,
		usage,
		coverage,
		initialQuery = '',
		onEdit,
		onEditPool,
		onAddBackend,
		onFixModels,
		onToggleEnabled,
		onOpenRouterAccount,
		toggling
	}: Props = $props();

	let expanded = $state<string | null>(null);
	// Two lenses over the same list: free text over name, endpoint, model and
	// alias, and a lamp that keeps only the rows in that state. They stack, so
	// "the red rows serving qwen" is one query and one click.
	let query = $state(untrack(() => initialQuery));
	let toneFilter = $state<BoardTone | null>(null);

	function clearFilter() {
		query = '';
		toneFilter = null;
	}

	interface RowState {
		tone: BoardTone;
		key: string;
		detail: string;
	}

	/** The lamp words: same tone order as `boardTone`, named for the board. */
	function rowState(backend: Backend, live: LiveBackend | null): RowState {
		const tone = boardTone(backend, live);
		if (!live) return { tone, key: 'board-no-probe', detail: t('board-no-probe-detail') };
		if (live.auth_failed) return { tone, key: 'board-auth', detail: t('board-auth-detail') };
		if (!live.enabled) return { tone, key: 'board-maintenance', detail: t('board-maintenance-detail') };
		if (!live.healthy) return { tone, key: 'board-down', detail: t('board-down-detail') };
		const max = live.max_inflight || backend.max_inflight;
		if (max > 0 && live.inflight / max >= 0.85) {
			return { tone, key: 'board-saturated', detail: t('board-saturated-detail', { inflight: live.inflight, max }) };
		}
		return { tone, key: 'board-on-time', detail: t('board-on-time-detail') };
	}

	interface Row {
		key: string;
		backend: Backend;
		live: LiveBackend | null;
		state: RowState;
		requests: number;
		segments: { total: number; on: number; hot: boolean };
		parallel: number | null;
	}

	interface Band {
		key: string;
		pool: Pool | null;
		rows: Row[];
		tone: BoardTone;
		up: number;
		models: { serving: number; total: number };
		problems: PoolProblem[];
		openRouter: boolean;
	}

	function inflightSegments(live: LiveBackend | null, backend: Backend): { total: number; on: number; hot: boolean } {
		const max = live?.max_inflight || backend.max_inflight || 0;
		const value = live?.inflight ?? 0;
		const total = 8;
		const on = max > 0 ? Math.round((value / max) * total) : 0;
		return { total, on, hot: max > 0 && value / max >= 0.85 };
	}

	const bands = $derived.by<Band[]>(() =>
		boardGroups(pools, backends).map((group) => {
			const pool = group.pool;
			const coverageItems = pool ? coverage[pool.name] ?? poolCoverage(pool, backends) : [];
			const rows = group.backends.map((backend) => {
				const live = pool ? liveForPool(pool, backend) : backend.live;
				return {
					key: `${pool?.name ?? '__unassigned'}:${backend.name}`,
					backend,
					live,
					state: rowState(backend, live),
					requests: (usage[backend.name] ?? []).reduce((sum, value) => sum + value, 0),
					segments: inflightSegments(live, backend),
					parallel: live
						? parallelismMismatch(live.detected_max_parallel, live.max_inflight || backend.max_inflight)
						: null
				};
			});
			const up = rows.filter((row) => row.live?.healthy && row.live.enabled).length;
			const tone: BoardTone = !rows.length ? 'gray' : up === rows.length ? 'green' : up === 0 ? 'red' : 'amber';
			return {
				key: pool?.name ?? '__unassigned',
				pool,
				rows,
				tone,
				up,
				models: {
					serving: coverageItems.filter((item) => item.serving > 0).length,
					total: coverageItems.length
				},
				problems: pool ? poolProblems(pool, backends, coverageItems) : [],
				openRouter:
					!!pool &&
					pool.backends.length > 0 &&
					pool.backends.every(
						(name) => backends.find((candidate) => candidate.name === name)?.provider === 'openrouter'
					)
			};
		})
	);

	const summary = $derived.by(() => {
		const counts: Record<BoardTone, number> = { green: 0, amber: 0, red: 0, gray: 0 };
		for (const backend of backends) counts[boardTone(backend)] += 1;
		return counts;
	});

	const summaryItems: { tone: BoardTone; key: string }[] = [
		{ tone: 'green', key: 'board-summary-on-time' },
		{ tone: 'amber', key: 'board-summary-degraded' },
		{ tone: 'red', key: 'board-summary-down' },
		{ tone: 'gray', key: 'board-summary-maintenance' }
	];

	const visibleBands = $derived.by<Band[]>(() => {
		const searching = query.trim() !== '';
		if (!searching && toneFilter === null) return bands;
		return bands.flatMap((band) => {
			const poolMatches = boardPoolMatches(band.pool?.name ?? null, band.rows, query);
			const rows = band.rows.filter(
				(row) =>
					(poolMatches || boardRowMatches(row, query)) &&
					(toneFilter === null || row.state.tone === toneFilter)
			);
			if (rows.length) return [{ ...band, rows }];
			// An empty pool is a topology row that may still need its first
			// backend — it stays while the query names the pool, never under a
			// lamp it cannot carry.
			if (!band.rows.length && poolMatches && toneFilter === null) return [band];
			return [];
		});
	});

	function keySource(backend: Backend): string {
		if (backend.has_stored_key) {
			return t('backends-field-api-key-stored', { hint: backend.api_key_hint ?? '—' });
		}
		if (backend.api_key_env) {
			return backend.api_key_env_set
				? t('backends-key-env-badge', { var: backend.api_key_env })
				: t('backends-key-env-unset-badge', { var: backend.api_key_env });
		}
		return t('board-key-none');
	}

	function aliasChip(
		backend: Backend,
		live: LiveBackend | null,
		alias: Backend['aliases'][number]
	): { label: string; title: string; broken: boolean } {
		if (alias.target) {
			const served = live?.models.includes(alias.target) ?? true;
			return served
				? { label: `${alias.alias} → ${alias.target}`, title: t('backends-alias-target-title', { target: alias.target }), broken: false }
				: {
						label: `${alias.alias} → ${alias.target}`,
						title: t('backends-alias-missing-title', { target: alias.target }),
						broken: true
					};
		}
		if (live?.models.length === 1) {
			return {
				label: `${alias.alias} → ${live.models[0]}`,
				title: t('backends-alias-target-title', { target: live.models[0] }),
				broken: false
			};
		}
		return {
			label: t('backends-alias-disabled-label', { name: alias.alias }),
			title: t('backends-alias-disabled-title'),
			broken: true
		};
	}
</script>

<section class="board-wrap" data-testid="concourse-board">
	<header class="board-summary">
		<div class="summary-lamps">
			{#each summaryItems as item (item.tone)}
				{#if item.tone !== 'gray' || summary.gray > 0 || toneFilter === 'gray'}
					<button
						type="button"
						class="summary-item"
						aria-pressed={toneFilter === item.tone}
						disabled={summary[item.tone] === 0 && toneFilter !== item.tone}
						onclick={() => (toneFilter = toneFilter === item.tone ? null : item.tone)}
					>
						<span class="lamp lamp-{item.tone}"></span>{summary[item.tone]}&nbsp;{t(item.key)}
					</button>
				{/if}
			{/each}
		</div>
		<div class="board-filter">
			<input
				type="search"
				class="input input-sm"
				bind:value={query}
				placeholder={t('board-filter-placeholder')}
				aria-label={t('board-filter-placeholder')}
			/>
			{#if query || toneFilter}
				<button class="btn btn-ghost btn-xs" type="button" onclick={clearFilter}>{t('board-filter-clear')}</button>
			{/if}
		</div>
	</header>

	<div class="board">
		<div class="board-head">
			<div class="cell cell-name">{t('board-col-name')}</div>
			<div class="cell">{t('board-col-state')}</div>
			<div class="cell">{t('board-col-load')}</div>
			<div class="cell cell-requests">{t('board-col-requests')}</div>
			<div class="cell cell-actions"></div>
		</div>

		{#each visibleBands as band (band.key)}
			<section
				class="pool-band"
				class:unassigned={!band.pool}
				aria-labelledby={`band-${band.key}`}
				data-testid={`upstream-pool-${band.pool?.name ?? 'unassigned'}`}
			>
				<header class="pool-head">
					<div class="pool-identity">
						<h3 class="pool-title" id={`band-${band.key}`}>{band.pool?.name ?? t('upstreams-unassigned-heading')}</h3>
						{#if band.pool}
							<span class="badge badge-secondary badge-sm">{band.pool.kind}</span>
							<span class="pool-strategy" title={t('pools-field-strategy-hint')}>{band.pool.strategy}</span>
							{#if band.pool.fallback_offline}
								<span class="badge badge-warning badge-outline badge-sm font-mono" title={t('backends-fallback-offline-title')}>{t('backends-fallback-offline-badge', { model: band.pool.fallback_offline })}</span>
							{/if}
							{#if !band.pool.enforce_limits}
								<span class="pool-limits-off" title={t('pools-field-enforce-limits')}>✗ {t('upstreams-comp-limits')}</span>
							{/if}
						{:else}
							<span class="pool-strategy">{t('upstreams-unassigned-description')}</span>
						{/if}
					</div>
					{#if band.pool}
						<div class="pool-meta">
							<span class="pool-stat"><span class="lamp lamp-{band.tone}"></span>{band.up}/{band.rows.length} {t('backends-status-up')}</span>
							{#if band.models.total}
								<span class="pool-stat" class:text-warning={band.models.serving < band.models.total}>{band.models.serving}/{band.models.total} {t('board-models-unit')}</span>
							{/if}
							{#if !band.openRouter}
								<button class="btn btn-ghost btn-xs" type="button" onclick={() => { if (band.pool) onAddBackend(band.pool); }}>+ {t('upstreams-add-backend')}</button>
							{/if}
							{#if band.openRouter}
								<button class="btn btn-ghost btn-xs" type="button" onclick={onOpenRouterAccount} title={t('board-pool-managed')}>{t('board-pool-account')}</button>
							{:else}
								<button class="btn btn-ghost btn-xs" type="button" onclick={() => { if (band.pool) onEditPool(band.pool); }}>{t('upstreams-edit-pool')}</button>
							{/if}
						</div>
					{/if}
				</header>

				{#if band.problems.length}
					<div class="pool-problems" role="status">
						<span class="pool-problems-title">{t('upstreams-problems-heading')}</span>
						<ul>{#each band.problems as problem (problem.key)}{@const message = t(problem.key, problem.params)}<li>{message}</li>{/each}</ul>
					</div>
				{/if}

				{#each band.rows as row (row.key)}
					{@const models = naturalSort(row.live?.models ?? [])}
					{@const shown = models.slice(0, 12)}
					{@const more = models.length - shown.length}
					{@const activity = activityCounts(usage[row.backend.name] ?? [])}
					{@const open = expanded === row.key}
					<div class="board-row" class:open data-testid={`upstream-backend-${row.backend.name}`}>
						<div class="cell cell-name">
							<button class="row-name" type="button" aria-expanded={open} onclick={() => (expanded = open ? null : row.key)}>
								<span class="chevron" aria-hidden="true">{open ? '▾' : '▸'}</span>
								<span class="name-stack">
									<span class="name-cell">{row.backend.name}</span>
									{#if row.backend.provider === 'openrouter'}
										<span class="profile-tag">openrouter</span>
									{:else if row.live?.detected_version}
										<span class="profile-tag">{row.live.profile} · {row.live.detected_version}</span>
									{/if}
								</span>
							</button>
						</div>
						<div class="cell cell-state" title={row.state.detail}>
							<span class="lamp lamp-{row.state.tone}"></span>
							<span class="state-word state-{row.state.tone}">{t(row.state.key)}</span>
							{#if row.live && row.live.models.length === 0}
								<button class="badge badge-warning badge-sm" type="button" title={t('backends-no-models-title')} onclick={() => (row.backend.provider === 'openrouter' ? onOpenRouterAccount() : onFixModels(row.backend))}>{t('backends-no-models')}</button>
							{/if}
						</div>
						<div class="cell cell-load">
							<span class="models-count">{row.live ? row.live.inflight : 0}<span class="unit">/{row.live?.max_inflight ?? row.backend.max_inflight}</span></span>
							{#if row.parallel !== null}
								<span class="parallel-warning" title={t('backends-parallel-mismatch', { parallel: row.parallel })}>▲</span>
							{/if}
							<div class="inflight-bar">
								{#each Array(row.segments.total) as _, i (i)}
									<i class={i < row.segments.on ? (row.segments.hot ? 'seg-hot' : 'seg-on') : ''}></i>
								{/each}
							</div>
						</div>
						<div class="cell cell-requests"><FlapValue value={n(row.requests)} /></div>
						<div class="cell cell-actions">
							<label class="serve" title={t('backends-enabled-hint')}>
								<input
									class="toggle toggle-sm"
									type="checkbox"
									checked={row.backend.enabled}
									disabled={toggling === row.backend.name}
									aria-label={t('backends-enabled-label', { name: row.backend.name })}
									onchange={() => onToggleEnabled(row.backend)}
								/>
								<span>{row.backend.enabled ? t('board-enabled') : t('board-drained')}</span>
							</label>
							<button class="btn btn-ghost btn-sm" type="button" aria-label={t('board-edit-named', { name: row.backend.name })} onclick={() => onEdit(row.backend)}>{t('board-edit')}</button>
						</div>
					</div>
					{#if open}
						<div class="row-detail">
							<p class="detail-sentence">{row.state.detail}</p>
							<div class="detail-block">
								<span class="detail-label">{t('board-detail-endpoint')}</span>
								<span class="detail-value">{row.backend.base_url}</span>
								<span class="detail-value detail-dim">{keySource(row.backend)}</span>
							</div>
							<div class="detail-block">
								<span class="detail-label">{t('board-detail-models')}</span>
								<div class="chips">
									{#each shown as model (model)}
										<a class="chip chip-link" href={`/admin/models?tab=catalog&q=${encodeURIComponent(model)}`} title={t('board-model-open-catalog')}>{model}</a>
									{/each}
									{#if more > 0}<span class="chip chip-dim">{t('board-models-more', { count: more })}</span>{/if}
									{#if models.length === 0}<span class="detail-dim">{t('board-models-none')}</span>{/if}
								</div>
								{#if row.backend.aliases.length}
									<span class="detail-label">{t('board-detail-aliases')}</span>
									<div class="chips">
										{#each row.backend.aliases as alias (alias.alias)}
											{@const chip = aliasChip(row.backend, row.live, alias)}
											<span class="chip" class:chip-broken={chip.broken} title={chip.title}>{chip.label}</span>
										{/each}
									</div>
								{/if}
							</div>
							<div class="detail-block">
								<span class="detail-label">{t('board-detail-activity')}</span>
								<span class="detail-value">{t('backends-activity-summary', { m15: n(activity.m15), m30: n(activity.m30), m60: n(activity.m60) })}</span>
								{#if row.parallel !== null}
									<span class="detail-warning">{t('backends-parallel-mismatch', { parallel: row.parallel })}</span>
								{/if}
								{#if (usage[row.backend.name] ?? []).length > 1}
									{@const buckets = usage[row.backend.name] ?? []}
									{@const maximum = Math.max(1, ...buckets)}
									<svg class="sparkline" viewBox="0 0 88 20" aria-hidden="true"><polyline fill="none" stroke="currentColor" stroke-width="1.5" points={buckets.map((value, index) => `${index * 8},${20 - (value / maximum) * 20}`).join(' ')}></polyline></svg>
								{/if}
							</div>
						</div>
					{/if}
				{:else}
					<div class="pool-empty">
						<span>{t('backends-pool-empty')}</span>
						{#if band.pool}
							<button class="btn btn-ghost btn-xs" type="button" onclick={() => { if (band.pool) onAddBackend(band.pool); }}>+ {t('upstreams-add-backend')}</button>
						{/if}
					</div>
				{/each}
			</section>
		{:else}
			<div class="board-empty">
				<span>{t('board-filter-empty')}</span>
				<button class="btn btn-ghost btn-xs" type="button" onclick={clearFilter}>{t('board-filter-clear')}</button>
			</div>
		{/each}
	</div>

	<div class="board-legend" aria-hidden="true">
		<span class="board-legend-item"><span class="lamp lamp-green"></span>{t('board-legend-on-time')}</span>
		<span class="board-legend-item"><span class="lamp lamp-amber"></span>{t('board-legend-degraded')}</span>
		<span class="board-legend-item"><span class="lamp lamp-red"></span>{t('board-legend-down')}</span>
		<span class="board-legend-item"><span class="lamp lamp-gray"></span>{t('board-legend-maintenance')}</span>
	</div>
</section>

<style>
	.board-wrap {
		container-type: inline-size;
	}
	.board-summary {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 1.25rem;
		padding: 0.75rem 1rem;
		border: 1px solid var(--color-base-300);
		border-bottom: none;
		border-radius: var(--radius-box) var(--radius-box) 0 0;
		background-color: color-mix(in oklab, var(--color-base-content) 4%, var(--color-base-200));
		font-size: 0.8125rem;
		font-weight: 600;
		letter-spacing: 0.03em;
		text-transform: uppercase;
		color: color-mix(in oklab, var(--color-base-content) 70%, transparent);
	}
	.summary-lamps {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 1.25rem;
	}
	.summary-item {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		padding: 0;
		border: none;
		background: none;
		font: inherit;
		letter-spacing: inherit;
		text-transform: inherit;
		color: inherit;
		cursor: pointer;
	}
	.summary-item[aria-pressed='true'] {
		color: var(--color-base-content);
		box-shadow: inset 0 -2px 0 var(--color-primary);
	}
	.summary-item:disabled {
		cursor: default;
	}
	.board-filter {
		display: flex;
		align-items: center;
		gap: 0.375rem;
		margin-left: auto;
	}
	.board-filter .input {
		width: 18rem;
		max-width: 45vw;
	}
	.board {
		border: 1px solid var(--color-base-300);
		border-radius: 0 0 var(--radius-box) var(--radius-box);
		overflow: hidden;
		background-color: var(--color-base-200);
	}
	.board-head,
	.board-row {
		display: grid;
		grid-template-columns: minmax(220px, 2fr) minmax(150px, 0.9fr) minmax(150px, 0.8fr) 90px minmax(190px, auto);
		align-items: center;
	}
	.board-head {
		background-color: color-mix(in oklab, var(--color-base-content) 4%, var(--color-base-200));
		border-bottom: 1px solid var(--color-base-300);
	}
	.board-head .cell {
		font-size: 0.6875rem;
		font-weight: 600;
		letter-spacing: 0.06em;
		text-transform: uppercase;
		color: color-mix(in oklab, var(--color-base-content) 55%, transparent);
	}
	.pool-band + .pool-band {
		border-top: 1px solid var(--color-base-300);
	}
	.pool-head {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 0.5rem 1rem;
		padding: 0.5rem 0.875rem;
		background-color: color-mix(in oklab, var(--color-base-content) 4%, var(--color-base-200));
	}
	.pool-identity {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 0.5rem;
		min-width: 0;
	}
	.pool-title {
		margin: 0;
		font-family: var(--font-mono, monospace);
		font-size: 0.875rem;
		font-weight: 600;
		letter-spacing: 0.01em;
	}
	.pool-strategy {
		font-family: var(--font-mono, monospace);
		font-size: 0.6875rem;
		color: color-mix(in oklab, var(--color-base-content) 55%, transparent);
	}
	.unassigned .pool-head {
		background-color: transparent;
	}
	.pool-limits-off {
		font-size: 0.6875rem;
		color: var(--color-error);
	}
	.pool-meta {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 0.25rem 0.875rem;
		margin-left: auto;
		font-size: 0.75rem;
		color: color-mix(in oklab, var(--color-base-content) 65%, transparent);
	}
	.pool-stat {
		display: flex;
		align-items: center;
		gap: 0.375rem;
		white-space: nowrap;
		font-variant-numeric: tabular-nums;
	}
	.pool-problems {
		display: flex;
		align-items: flex-start;
		gap: 0.5rem;
		padding: 0.5rem 0.875rem;
		border-top: 1px dashed var(--color-base-300);
		background-color: color-mix(in oklab, var(--color-warning) 8%, var(--color-base-200));
		font-size: 0.75rem;
		line-height: 1.4;
	}
	.pool-problems::before {
		content: '▲';
		flex: none;
		color: var(--color-warning);
		font-size: 0.625rem;
		line-height: 1.6;
	}
	.pool-problems-title {
		font-weight: 600;
		white-space: nowrap;
	}
	.pool-problems ul {
		margin: 0;
		padding: 0;
		list-style: none;
	}
	.board-row {
		border-top: 1px solid var(--color-base-300);
		min-height: 50px;
		transition: background-color 140ms;
	}
	.board-row:hover,
	.board-row.open {
		background-color: color-mix(in oklab, var(--color-base-content) 3%, transparent);
	}
	.cell {
		padding: 0.5rem 0.875rem;
		display: flex;
		align-items: center;
		gap: 0.5rem;
		min-width: 0;
		overflow: hidden;
	}
	.cell-requests {
		justify-content: flex-end;
		font-variant-numeric: tabular-nums;
	}
	.cell-actions {
		justify-content: flex-end;
		gap: 0.5rem;
		overflow: visible;
	}
	.row-name {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		min-width: 0;
		width: 100%;
		padding: 0;
		text-align: start;
		background: none;
		border: none;
		color: inherit;
		cursor: pointer;
	}
	.chevron {
		flex: none;
		width: 0.75rem;
		font-size: 0.6875rem;
		color: color-mix(in oklab, var(--color-base-content) 45%, transparent);
	}
	.name-stack {
		display: flex;
		flex-direction: column;
		gap: 0.0625rem;
		min-width: 0;
	}
	.name-cell {
		font-weight: 600;
		font-size: 0.9375rem;
		letter-spacing: 0.04em;
		text-transform: uppercase;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.profile-tag {
		font-size: 0.625rem;
		color: color-mix(in oklab, var(--color-base-content) 45%, transparent);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.models-count {
		font-family: var(--font-mono, monospace);
		font-size: 0.875rem;
		font-weight: 500;
		white-space: nowrap;
	}
	.models-count .unit {
		color: color-mix(in oklab, var(--color-base-content) 40%, transparent);
		font-size: 0.6875rem;
		margin-left: 0.25rem;
	}
	/* Lamps: printed marks, no radius, no glow — the board's state vocabulary. */
	.lamp {
		width: 0.5rem;
		height: 0.5rem;
		flex: none;
		display: inline-block;
	}
	.lamp-green {
		background-color: var(--color-success);
	}
	.lamp-amber {
		background-color: var(--color-warning);
	}
	.lamp-red {
		background-color: var(--color-error);
	}
	.lamp-gray {
		background-color: color-mix(in oklab, var(--color-base-content) 35%, transparent);
	}
	.state-word {
		font-size: 0.75rem;
		font-weight: 600;
		letter-spacing: 0.05em;
		text-transform: uppercase;
		white-space: nowrap;
	}
	.state-green {
		color: color-mix(in oklab, var(--color-base-content) 70%, transparent);
	}
	.state-amber {
		color: var(--color-warning);
	}
	.state-red {
		color: var(--color-error);
	}
	.state-gray {
		color: color-mix(in oklab, var(--color-base-content) 50%, transparent);
	}
	.inflight-bar {
		display: flex;
		gap: 2px;
		margin-top: 0.25rem;
	}
	.inflight-bar i {
		width: 0.5rem;
		height: 0.25rem;
		background-color: var(--color-base-300);
		display: block;
	}
	.inflight-bar i.seg-on {
		background-color: color-mix(in oklab, var(--color-base-content) 60%, transparent);
	}
	.inflight-bar i.seg-hot {
		background-color: var(--color-warning);
	}
	.serve {
		display: flex;
		align-items: center;
		gap: 0.375rem;
		font-size: 0.75rem;
		white-space: nowrap;
		color: color-mix(in oklab, var(--color-base-content) 70%, transparent);
	}
	.row-detail {
		display: grid;
		grid-template-columns: minmax(0, 1.15fr) minmax(0, 1.2fr) minmax(0, 1fr);
		gap: 1.25rem;
		padding: 0.75rem 0.875rem 0.875rem 2.125rem;
		border-top: 1px dashed var(--color-base-300);
		background-color: color-mix(in oklab, var(--color-base-content) 2%, transparent);
	}
	.detail-block {
		display: flex;
		flex-direction: column;
		gap: 0.25rem;
		min-width: 0;
	}
	.detail-sentence {
		grid-column: 1 / -1;
		margin: 0;
		font-size: 0.8125rem;
		color: color-mix(in oklab, var(--color-base-content) 70%, transparent);
	}
	.detail-label {
		font-size: 0.625rem;
		font-weight: 600;
		letter-spacing: 0.06em;
		text-transform: uppercase;
		color: color-mix(in oklab, var(--color-base-content) 45%, transparent);
	}
	.detail-value {
		font-family: var(--font-mono, monospace);
		font-size: 0.75rem;
		overflow-wrap: anywhere;
	}
	.detail-dim {
		color: color-mix(in oklab, var(--color-base-content) 50%, transparent);
	}
	.detail-warning {
		font-size: 0.75rem;
		color: var(--color-warning);
	}
	.parallel-warning {
		font-size: 0.625rem;
		line-height: 1;
		color: var(--color-warning);
		cursor: help;
	}
	.chips {
		display: flex;
		flex-wrap: wrap;
		gap: 0.25rem;
	}
	.chip {
		display: inline-flex;
		align-items: center;
		max-width: 100%;
		border: 1px solid var(--color-base-300);
		border-radius: calc(var(--radius-box) / 3);
		padding: 0.0625rem 0.375rem;
		font-family: var(--font-mono, monospace);
		font-size: 0.6875rem;
		overflow-wrap: anywhere;
	}
	.chip-dim {
		border-style: dashed;
		color: color-mix(in oklab, var(--color-base-content) 55%, transparent);
	}
	.chip-broken {
		border-color: color-mix(in oklab, var(--color-warning) 60%, var(--color-base-300));
		color: var(--color-warning);
	}
	.chip-link:hover,
	.chip-link:focus-visible {
		border-color: var(--color-primary);
		color: var(--color-primary);
	}
	.sparkline {
		width: 88px;
		height: 20px;
		color: var(--color-primary);
	}
	.pool-empty {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 0.75rem;
		margin: 0;
		padding: 0.75rem 0.875rem;
		border-top: 1px solid var(--color-base-300);
		font-size: 0.8125rem;
		color: color-mix(in oklab, var(--color-base-content) 55%, transparent);
	}
	.board-empty {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: center;
		gap: 0.75rem;
		padding: 1.5rem 0.875rem;
		border-top: 1px solid var(--color-base-300);
		font-size: 0.8125rem;
		color: color-mix(in oklab, var(--color-base-content) 55%, transparent);
	}
	.board-legend {
		display: flex;
		flex-wrap: wrap;
		gap: 1.25rem;
		padding: 0.625rem 0.25rem;
		font-size: 0.75rem;
		color: color-mix(in oklab, var(--color-base-content) 55%, transparent);
	}
	.board-legend-item {
		display: flex;
		align-items: center;
		gap: 0.5rem;
	}

	@container (max-width: 860px) {
		.board-head {
			display: none;
		}
		.board-row {
			grid-template-columns: minmax(0, 1fr) auto;
			gap: 0.25rem 0.75rem;
			padding: 0.5rem 0.875rem;
		}
		.cell {
			padding: 0;
			overflow: visible;
		}
		.cell-name {
			grid-column: 1;
			grid-row: 1;
		}
		.cell-state {
			grid-column: 2;
			grid-row: 1;
			justify-content: flex-end;
		}
		.cell-load {
			grid-column: 1;
			grid-row: 2;
		}
		.cell-requests {
			display: none;
		}
		.cell-actions {
			grid-column: 2;
			grid-row: 2;
		}
		.row-detail {
			grid-template-columns: 1fr;
			padding-left: 0.875rem;
		}
	}
	@container (max-width: 520px) {
		.board-row {
			grid-template-columns: 1fr;
		}
		.cell-state {
			grid-column: 1;
			grid-row: 2;
			justify-content: flex-start;
		}
		.cell-load {
			grid-row: 3;
		}
		.cell-actions {
			grid-column: 1;
			grid-row: 4;
			justify-content: flex-start;
		}
	}
</style>
