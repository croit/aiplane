<script lang="ts">
	import { egressSummary, modelCleared, sortedLevels, type Draft, type DetectorModel } from '#lib/classification.js';
	import LevelSelect from './LevelSelect.svelte';
	import DetectorModelSelect from './DetectorModelSelect.svelte';
	import LevelsStep from './LevelsStep.svelte';
	import { t } from '#lib/i18n.svelte.js';

	/**
	 * The assistant's second step: which model pool may see which level,
	 * prefilled from where its backends run, and the detector model that
	 * reads content. Tools, connectors and collections are rated on the
	 * Destinations tab afterwards.
	 */
	let { draft = $bindable(), models, model = $bindable(), customLevels }: {
		draft: Draft;
		models: readonly DetectorModel[];
		model: string;
		customLevels: boolean;
	} = $props();

	const pools = $derived(draft.destinations.filter((d) => d.kind === 'pool'));
	const egress = $derived(egressSummary(draft.destinations));
	const lowest = $derived(sortedLevels(draft.levels)[0]?.name ?? '');
	const unclearedPicked = $derived(model !== '' && models.some((m) => m.id === model && !modelCleared(draft, m)));
</script>

<div class="space-y-5">
	{#if customLevels}<LevelsStep bind:draft />{/if}
	<p class="text-sm text-base-content/70">{t('cls-assist-models-lead')}</p>
	<ul class="m-0 list-none space-y-2 p-0">
		{#each pools as d (d.id)}
			<li class="flex flex-wrap items-center gap-2 rounded-box border border-base-300 p-3">
				<div class="min-w-0 flex-1">
					<p class="m-0 break-all text-sm font-medium">{d.name}</p>
					<p class="m-0 text-xs text-base-content/60">{t(`cls-assist-hosting-${d.hosting ?? 'unknown'}`, { hosts: (d.hosts ?? []).join(', ') })}</p>
				</div>
				<div class="w-full sm:w-52"><LevelSelect levels={draft.levels} bind:value={d.level_id} none={t('cls-destination-unrated')} ariaLabel={t('cls-destination-level', { name: d.name })} /></div>
			</li>
		{:else}
			<li class="text-sm text-base-content/60">{t('cls-destinations-empty')}</li>
		{/each}
	</ul>
	{#if egress.count > 0}
		<p class="text-sm text-base-content/70">{t('cls-assist-egress', { count: egress.count, names: egress.names.join(', '), level: lowest })}</p>
	{/if}
	<label class="flex flex-col gap-1">
		<span class="text-sm font-medium">{t('cls-assist-model')}</span>
		<DetectorModelSelect {models} bind:value={model} cleared={(m) => modelCleared(draft, m)} class="sm:w-80" />
		<span class="text-xs text-base-content/60">{t('cls-assist-model-help')}</span>
		{#if model === ''}<span class="text-xs text-warning">{t('cls-assist-questions-inactive')}</span>{/if}
		{#if unclearedPicked}<span class="text-xs text-error">{t('cls-policy-model-uncleared-picked')}</span>{/if}
	</label>
</div>
