<script lang="ts">
	import StepIndicator from '#lib/components/ui/StepIndicator.svelte';
	import ChoiceCard from '#lib/components/ui/ChoiceCard.svelte';
	import { applyTemplate, classificationApi, draftFrom, failure, buildBody, finalizeDraft, modelBlocksActivation, pickSystemOneModel, prefillPools, type ClassificationState, type Draft } from '#lib/classification.js';
	import ModelsStep from './ModelsStep.svelte';
	import ReviewStep from './ReviewStep.svelte';
	import { untrack } from 'svelte';
	import { t } from '#lib/i18n.svelte.js';

	/**
	 * The set-up assistant: the standard, the model pools with their System One
	 * model, then Activate. Everything else comes from the template and is
	 * edited on the overview's tabs. The draft lives client-side and is saved
	 * once at the end; leaving the assistant discards it.
	 */
	let { saved, onactivated, oncancel }: {
		saved: ClassificationState;
		onactivated: (state: ClassificationState) => void;
		oncancel: () => void;
	} = $props();

	const STEPS = ['standard', 'models', 'activate'] as const;
	let draft = $state<Draft>(untrack(() => prefillPools(draftFrom(saved))));
	let step = $state(0);
	let chosen = $state<string>('custom');
	let chosenModel = $state<string | null>(untrack(() => saved.policy.system_one_model || null));
	let visited = $state<number[]>([]);
	let error = $state<string | null>(null);
	let busy = $state(false);

	const model = $derived(chosenModel ?? pickSystemOneModel(draft, saved.system_one_models));
	const final = $derived(finalizeDraft(draft, model));
	const blocked = $derived(modelBlocksActivation(draft, saved.system_one_models, model));

	function go(next: number) {
		if (!visited.includes(step)) visited = [...visited, step];
		step = next;
		error = null;
	}

	function pick(id: string) {
		chosen = id;
		chosenModel = id === 'custom' ? saved.policy.system_one_model || null : null;
		const template = saved.templates.find((tpl) => tpl.id === id);
		const base = draftFrom(saved);
		draft = prefillPools(template ? applyTemplate(base, template) : base);
	}

	async function activate() {
		busy = true;
		error = null;
		try {
			onactivated(await classificationApi.save(buildBody(final)));
		} catch (err) {
			error = failure(err);
		} finally {
			busy = false;
		}
	}
	const labels = $derived(STEPS.map((s) => t(`cls-assist-step-${s}`)));
</script>

<div class="space-y-4">
	<StepIndicator steps={labels} current={step} completed={visited} onselect={(i) => go(i)} />
	<div class="min-h-64">
		{#if STEPS[step] === 'standard'}
			<div class="space-y-3">
				<p class="text-sm text-base-content/70">{t('cls-path-lead')}</p>
				<div class="grid gap-2.5 sm:grid-cols-2" role="radiogroup" aria-label={t('cls-assist-step-standard')}>
					{#each saved.templates as tpl (tpl.id)}
						<ChoiceCard title={tpl.name} description={tpl.levels.map((l) => l.name).join(' < ')} selected={chosen === tpl.id} onselect={() => pick(tpl.id)} />
					{/each}
					<ChoiceCard title={t('cls-path-custom')} description={saved.active ? t('cls-path-custom-current') : t('cls-path-custom-blank')} selected={chosen === 'custom'} onselect={() => pick('custom')} />
				</div>
			</div>
		{:else if STEPS[step] === 'models'}
			<ModelsStep bind:draft models={saved.system_one_models} bind:model={() => model, (v) => (chosenModel = v)} customLevels={chosen === 'custom'} />
		{:else}
			<ReviewStep before={saved} draft={final} {error} />
		{/if}
	</div>
	<div class="flex flex-wrap items-center justify-between gap-2 border-t border-base-300 pt-3">
		<button class="btn btn-ghost" type="button" onclick={oncancel}>{t('admin-cancel')}</button>
		<div class="flex gap-2">
			<button class="btn" type="button" disabled={step === 0} onclick={() => go(step - 1)}>{t('agents-setup-back')}</button>
			{#if step < STEPS.length - 1}
				<button class="btn btn-primary" type="button" onclick={() => go(step + 1)}>{t('agents-setup-next')}</button>
			{:else}
				<button class="btn btn-primary" type="button" disabled={busy || blocked} onclick={activate}>{t('cls-activate')}</button>
			{/if}
		</div>
	</div>
</div>
