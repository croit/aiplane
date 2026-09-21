<script lang="ts">
	/**
	 * Grants as a grid: rows are the things that can be granted, columns are the
	 * groups, cells toggle.
	 *
	 * The group cards answer "set this group up". This answers the other
	 * question — "who can call this?" — which otherwise means opening every card
	 * in turn. Both write through the same save, so neither is a second source of
	 * truth.
	 */
	import { coverageOf, matchesGrantFilter, type GrantFilter, type GrantRow } from '$lib/admin-groups';
	import { toggleValue } from '$lib/multi-select';
	import { t } from '$lib/i18n.svelte';
	import type { AdminGroup } from './AdminGroupForm.svelte';

	let {
		rows,
		groups,
		held,
		onsave
	}: {
		rows: GrantRow[];
		groups: AdminGroup[];
		/** Which of the group's lists this matrix edits. */
		held: (group: AdminGroup) => string[];
		onsave: (group: AdminGroup, next: string[]) => Promise<void>;
	} = $props();

	let query = $state('');
	let filter = $state<GrantFilter>('all');
	let saving = $state<string | null>(null);

	let visible = $derived(rows.filter((row) => matchesGrantFilter(row, filter, query, groups, held)));

	const FILTERS: [GrantFilter, string][] = [
		['all', 'groups-matrix-filter-all'],
		['families', 'groups-matrix-filter-families'],
		['granted', 'groups-matrix-filter-granted'],
		['ungranted', 'groups-matrix-filter-ungranted']
	];

	async function toggle(group: AdminGroup, row: GrantRow) {
		const cell = `${group.name}\u0000${row.value}`;
		saving = cell;
		try {
			await onsave(group, toggleValue(held(group), row.value));
		} finally {
			saving = null;
		}
	}
</script>

{#if groups.length === 0}
	<div class="alert"><span>{t('groups-matrix-no-groups')}</span></div>
{:else}
	<article class="card border border-base-300 bg-base-100">
		<div class="card-body gap-3 pb-0">
			<div class="flex flex-wrap items-center gap-2">
				<input type="search" class="input input-bordered input-sm w-60 max-w-full" bind:value={query} placeholder={t('groups-matrix-filter-placeholder')} aria-label={t('groups-matrix-filter-placeholder')} />
				{#each FILTERS as [value, label] (value)}
					<button type="button" class="btn btn-xs {filter === value ? 'btn-active' : ''}" onclick={() => (filter = value)}>{t(label)}</button>
				{/each}
			</div>
		</div>
		<div class="card-body gap-0 overflow-x-auto pt-3">
			<table class="table table-sm">
				<thead>
					<tr>
						<th class="sticky left-0 bg-base-100">{t('groups-matrix-col-grant')}</th>
						{#each groups as group (group.name)}
							<th class="text-center font-mono text-xs font-normal">{group.name}</th>
						{/each}
					</tr>
				</thead>
				<tbody>
					{#each visible as row (row.value)}
						<tr class={row.kind === 'tool' ? '' : 'bg-base-200/40'}>
							<td class="sticky left-0 bg-base-100">
								<span class="block {row.kind === 'tool' ? 'font-mono text-xs' : 'text-sm font-medium'}">{row.label}</span>
								{#if row.description && row.description !== row.label}
									<span class="block font-mono text-[11px] text-base-content/50">{row.description}</span>
								{/if}
							</td>
							{#each groups as group (group.name)}
								{@const state = coverageOf(held(group), row.value)}
								<td class="text-center">
									<input
										type="checkbox"
										class="checkbox checkbox-sm"
										checked={state !== 'none'}
										indeterminate={state === 'covered'}
										disabled={state === 'covered' || saving !== null}
										title={state === 'covered' ? t('groups-matrix-covered-title') : undefined}
										aria-label="{row.label} — {group.name}"
										onchange={() => toggle(group, row)}
									/>
								</td>
							{/each}
						</tr>
					{/each}
				</tbody>
			</table>
			{#if visible.length === 0}
				<p class="py-6 text-center text-sm text-base-content/60">{t('groups-matrix-empty')}</p>
			{/if}
		</div>
	</article>
{/if}
