<script lang="ts">
	import StepIndicator from '#lib/components/ui/StepIndicator.svelte';
	import ChoiceCard from '#lib/components/ui/ChoiceCard.svelte';
	import { applyTemplate, classificationApi, draftFrom, failure, buildBody, type ClassificationState, type Draft } from '#lib/classification.js';
	import LevelsStep from './LevelsStep.svelte';
	import ClassesStep from './ClassesStep.svelte';
	import MarkingsStep from './MarkingsStep.svelte';
	import DestinationsStep from './DestinationsStep.svelte';
	import OutputStep from './OutputStep.svelte';
	import PolicyStep from './PolicyStep.svelte';
	import ReviewStep from './ReviewStep.svelte';
	import { untrack } from 'svelte';
	import { t } from '#lib/i18n.svelte.js';

	/**
	 * The set-up assistant: eight steps over one client-side draft, saved once
	 * at the end with a review of what changes. Nothing reaches the server
	 * before Activate, so leaving the assistant discards the draft.
	 */
	let { saved, onactivated, oncancel }: {
		saved: ClassificationState;
		onactivated: (state: ClassificationState) => void;
		oncancel: () => void;
	} = $props();

	const STEPS = ['path', 'levels', 'classes', 'markings', 'destinations', 'output', 'policy', 'review'] as const;
	let draft = $state<Draft>(untrack(() => draftFrom(saved)));
	let step = $state(0);
	let chosen = $state<string>('custom');
	let visited = $state<number[]>([]);
	let error = $state<string | null>(null);
	let busy = $state(false);

	function go(next: number) {
		if (!visited.includes(step)) visited = [...visited, step];
		step = next;
		error = null;
	}

	function pick(id: string) {
		chosen = id;
		const template = saved.templates.find((tpl) => tpl.id === id);
		draft = template ? applyTemplate(draftFrom(saved), template) : draftFrom(saved);
	}

	async function activate() {
		busy = true;
		error = null;
		try {
			onactivated(await classificationApi.save(buildBody(draft)));
		} catch (err) {
			error = failure(err);
		} finally {
			busy = false;
		}
	}
	const labels = $derived(STEPS.map((s) => t(`cls-step-${s}`)));
</script>

<div class="space-y-4">
	<StepIndicator steps={labels} current={step} completed={visited} onselect={(i) => go(i)} />
	<div class="min-h-64">
		{#if STEPS[step] === 'path'}
			<div class="space-y-3">
				<p class="text-sm text-base-content/70">{t('cls-path-lead')}</p>
				<div class="grid gap-2.5 sm:grid-cols-2" role="radiogroup" aria-label={t('cls-step-path')}>
					{#each saved.templates as tpl (tpl.id)}
						<ChoiceCard title={tpl.name} description={tpl.levels.map((l) => l.name).join(' < ')} selected={chosen === tpl.id} onselect={() => pick(tpl.id)} />
					{/each}
					<ChoiceCard title={t('cls-path-custom')} description={saved.active ? t('cls-path-custom-current') : t('cls-path-custom-blank')} selected={chosen === 'custom'} onselect={() => pick('custom')} />
				</div>
			</div>
		{:else if STEPS[step] === 'levels'}
			<LevelsStep bind:draft />
		{:else if STEPS[step] === 'classes'}
			<ClassesStep bind:draft {saved} />
		{:else if STEPS[step] === 'markings'}
			<MarkingsStep bind:draft />
		{:else if STEPS[step] === 'destinations'}
			<DestinationsStep bind:draft />
		{:else if STEPS[step] === 'output'}
			<OutputStep bind:draft />
		{:else if STEPS[step] === 'policy'}
			<PolicyStep bind:draft models={saved.system_one_models} />
		{:else}
			<ReviewStep before={saved} {draft} {error} />
		{/if}
	</div>
	<div class="flex flex-wrap items-center justify-between gap-2 border-t border-base-300 pt-3">
		<button class="btn btn-ghost" type="button" onclick={oncancel}>{t('admin-cancel')}</button>
		<div class="flex gap-2">
			<button class="btn" type="button" disabled={step === 0} onclick={() => go(step - 1)}>{t('agents-setup-back')}</button>
			{#if step < STEPS.length - 1}
				<button class="btn btn-primary" type="button" onclick={() => go(step + 1)}>{t('agents-setup-next')}</button>
			{:else}
				<button class="btn btn-primary" type="button" disabled={busy} onclick={activate}>{t('cls-activate')}</button>
			{/if}
		</div>
	</div>
</div>
