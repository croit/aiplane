<script lang="ts">
	import { buildBody, classificationApi, draftFrom, failure, type ClassificationState, type Draft } from '#lib/classification.js';
	import LevelsStep from './LevelsStep.svelte';
	import ClassesStep from './ClassesStep.svelte';
	import MarkingsStep from './MarkingsStep.svelte';
	import DestinationsStep from './DestinationsStep.svelte';
	import OutputStep from './OutputStep.svelte';
	import PolicyStep from './PolicyStep.svelte';
	import { untrack } from 'svelte';
	import { page } from '$app/state';
	import { t } from '#lib/i18n.svelte.js';

	/** The active scheme, every section of the assistant on its own tab over one draft; one Save replaces the scheme. */
	let { saved, onsaved, onassistant }: {
		saved: ClassificationState;
		onsaved: (state: ClassificationState) => void;
		onassistant: () => void;
	} = $props();

	let draft = $state<Draft>(untrack(() => draftFrom(saved)));
	let error = $state<string | null>(null);
	let done = $state(false);
	let busy = $state(false);
	const sections = ['levels', 'classes', 'markings', 'destinations', 'output', 'policy'] as const;
	const selected = $derived(sections.find((s) => s === page.url.searchParams.get('tab')) ?? sections[0]);

	async function save() {
		busy = true;
		error = null;
		done = false;
		try {
			const next = await classificationApi.save(buildBody(draft));
			onsaved(next);
			draft = draftFrom(next);
			done = true;
		} catch (err) {
			error = failure(err);
		} finally {
			busy = false;
		}
	}
</script>

<div class="space-y-4">
	<nav class="tabs tabs-border w-full overflow-x-auto" aria-label={t('cls-heading')}>
		{#each sections as section (section)}
			<a class:tab-active={selected === section} class="tab whitespace-nowrap" href="/admin/classification?tab={section}">{t(`cls-step-${section}`)}</a>
		{/each}
	</nav>
	<div class="min-h-64">
		{#if selected === 'levels'}<LevelsStep bind:draft />
		{:else if selected === 'classes'}<ClassesStep bind:draft {saved} />
		{:else if selected === 'markings'}<MarkingsStep bind:draft />
		{:else if selected === 'destinations'}<DestinationsStep bind:draft />
		{:else if selected === 'output'}<OutputStep bind:draft />
		{:else}<PolicyStep bind:draft models={saved.detector_models} />{/if}
	</div>
	{#if error}<div class="alert alert-error text-sm" role="alert"><span>{error}</span></div>{/if}
	{#if done}<div class="alert alert-success text-sm" role="status"><span>{t('cls-saved')}</span></div>{/if}
	<div class="flex flex-wrap items-center justify-between gap-2">
		<button class="btn" type="button" onclick={onassistant}>{t('cls-run-assistant')}</button>
		<button class="btn btn-primary" type="button" disabled={busy} onclick={save}>{t('cls-save')}</button>
	</div>
</div>
