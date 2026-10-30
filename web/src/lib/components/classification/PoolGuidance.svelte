<script lang="ts">
	import { suggestPoolLevel, type Level, type PoolAnswers } from '#lib/classification.js';
	import { t } from '#lib/i18n.svelte.js';

	/**
	 * The four questions an operator can answer about a pool, and the level they
	 * point at. The rating is the operator's own declaration about where the
	 * data goes; this only suggests, and `onuse` applies the suggestion on request.
	 */
	let { levels, onuse }: { levels: readonly Level[]; onuse: (levelId: string) => void } = $props();

	let answers = $state<PoolAnswers>({ runs: 'own', dpa: false, transferBasis: false, nda: false });
	let touched = $state(false);
	const suggestion = $derived(touched ? suggestPoolLevel(levels, answers) : null);
	const suggested = $derived(levels.find((l) => l.id === suggestion));
</script>

<div class="space-y-2 rounded-box bg-base-200 p-3 text-sm" onchange={() => (touched = true)} role="group" aria-label={t('cls-pool-guidance-title')}>
	<p class="m-0 text-xs text-base-content/60">{t('cls-pool-guidance-lead')}</p>
	<label class="flex flex-wrap items-center gap-2">
		<span>{t('cls-pool-q-runs')}</span>
		<select class="select select-xs" bind:value={answers.runs}>
			<option value="own">{t('cls-pool-runs-own')}</option>
			<option value="eu">{t('cls-pool-runs-eu')}</option>
			<option value="other">{t('cls-pool-runs-other')}</option>
		</select>
	</label>
	{#if answers.runs !== 'own'}
		<label class="flex items-center gap-2"><input type="checkbox" class="checkbox checkbox-xs" bind:checked={answers.dpa} />{t('cls-pool-q-dpa')}</label>
		{#if answers.runs === 'other'}
			<label class="flex items-center gap-2"><input type="checkbox" class="checkbox checkbox-xs" bind:checked={answers.transferBasis} />{t('cls-pool-q-transfer')}</label>
		{/if}
		<label class="flex items-center gap-2"><input type="checkbox" class="checkbox checkbox-xs" bind:checked={answers.nda} />{t('cls-pool-q-nda')}</label>
	{/if}
	{#if suggested}
		<p class="m-0 flex flex-wrap items-center gap-2">
			{t('cls-pool-suggestion', { level: suggested.name })}
			<button class="btn btn-xs" type="button" onclick={() => onuse(suggested.id)}>{t('cls-pool-use')}</button>
		</p>
	{/if}
</div>
