<script lang="ts">
	import { choosesOver, groupDestinations, unratedCount, type Destination, type Draft } from '#lib/classification.js';
	import LevelSelect from './LevelSelect.svelte';
	import PoolGuidance from './PoolGuidance.svelte';
	import { t } from '#lib/i18n.svelte.js';
	import { untrack } from 'svelte';

	/**
	 * Every place data can go, one kind at a time, with the level the operator
	 * declares it is fit for. The order (unrated first) is fixed when the step
	 * opens so a row does not jump away under the cursor once it is rated.
	 */
	let { draft = $bindable() }: { draft: Draft } = $props();

	const key = (d: Pick<Destination, 'kind' | 'id'>) => `${d.kind}/${d.id}`;
	const order = untrack(() => groupDestinations(draft.destinations).map((g) => ({ kind: g.kind, keys: g.items.map(key) })));
	const current = $derived(new Map(draft.destinations.map((d) => [key(d), d] as const)));
	let guided = $state<string | null>(null);

	const rated = (kind: Destination['kind']) => draft.destinations.filter((d) => d.kind === kind && d.level_id !== null).length;
	const total = (kind: Destination['kind']) => draft.destinations.filter((d) => d.kind === kind).length;
</script>

<div class="space-y-6">
	<p class="text-sm text-base-content/70">{t('cls-destinations-lead')}</p>
	{#if unratedCount(draft.destinations) > 0}
		<div class="alert alert-warning text-sm" role="status"><span>{t('cls-destinations-unrated', { count: unratedCount(draft.destinations) })}</span></div>
	{/if}
	{#each order as group (group.kind)}
		<section class="space-y-2">
			<div class="flex items-center gap-3">
				<h4 class="m-0 text-base font-semibold">{t(`cls-destination-kind-${group.kind}`)}</h4>
				<progress class="progress progress-primary w-40" value={rated(group.kind)} max={total(group.kind)} aria-label={t('cls-destinations-progress', { rated: rated(group.kind), total: total(group.kind) })}></progress>
				<span class="text-xs text-base-content/60">{rated(group.kind)}/{total(group.kind)}</span>
			</div>
			<ul class="m-0 list-none space-y-2 p-0">
				{#each group.keys as k (k)}
					{@const d = current.get(k)}
					{#if d}
						<li class="rounded-box border border-base-300 p-3">
							<div class="flex flex-wrap items-center gap-2">
								<div class="min-w-0 flex-1">
									<p class="m-0 break-all text-sm font-medium">{d.name}</p>
									{#if d.description}<p class="m-0 text-xs text-base-content/60">{d.description}</p>{:else}<p class="m-0 text-xs text-base-content/40">{t('cls-destination-no-description')}</p>{/if}
								</div>
								<div class="w-44"><LevelSelect levels={draft.levels} bind:value={d.level_id} none={t('cls-destination-unrated')} ariaLabel={t('cls-destination-level', { name: d.name })} /></div>
								{#if choosesOver(d)}
									<select class="select select-sm w-36" aria-label={t('cls-destination-over', { name: d.name })} title={t('cls-destination-over-help')} bind:value={d.over}>
										<option value="hide">{t('cls-over-hide')}</option>
										<option value="gate">{t('cls-over-gate')}</option>
									</select>
								{/if}
								{#if d.kind === 'pool'}
									<button class="btn btn-ghost btn-xs" type="button" aria-expanded={guided === k} onclick={() => (guided = guided === k ? null : k)}>{t('cls-pool-guidance-title')}</button>
								{/if}
							</div>
							{#if d.kind === 'pool' && guided === k}
								<div class="mt-2"><PoolGuidance levels={draft.levels} onuse={(id) => (d.level_id = id)} /></div>
							{/if}
						</li>
					{/if}
				{/each}
			</ul>
		</section>
	{:else}
		<p class="text-sm text-base-content/60">{t('cls-destinations-empty')}</p>
	{/each}
</div>
