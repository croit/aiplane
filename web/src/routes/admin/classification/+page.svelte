<script lang="ts">
	import { onMount } from 'svelte';
	import { classificationApi, failure, type ClassificationState } from '#lib/classification.js';
	import ClassificationAssistant from '#lib/components/classification/ClassificationAssistant.svelte';
	import ClassificationOverview from '#lib/components/classification/ClassificationOverview.svelte';
	import { t } from '#lib/i18n.svelte.js';

	let current = $state<ClassificationState | null>(null);
	let error = $state<string | null>(null);
	let assisting = $state(false);

	onMount(async () => {
		try {
			current = await classificationApi.get();
		} catch (err) {
			error = failure(err);
		}
	});
</script>

<div class="space-y-4">
	<h1 class="text-2xl font-semibold">{t('cls-heading')}</h1>
	{#if error}<div class="alert alert-error" role="alert"><span>{error}</span></div>{/if}
	{#if !current && !error}
		<span class="loading loading-spinner"></span>
	{:else if current}
		{#if assisting}
			<ClassificationAssistant saved={current} oncancel={() => (assisting = false)} onactivated={(next) => { current = next; assisting = false; }} />
		{:else if current.active}
			
				<ClassificationOverview saved={current} onsaved={(next) => (current = next)} onassistant={() => (assisting = true)} />
			
		{:else}
			<div class="card card-border bg-base-100">
				<div class="card-body gap-3">
					<h2 class="card-title">{t('cls-off-title')}</h2>
					<p class="m-0 text-sm text-base-content/70">{t('cls-off-lead')}</p>
					<div class="card-actions"><button class="btn btn-primary" type="button" onclick={() => (assisting = true)}>{t('cls-setup')}</button></div>
				</div>
			</div>
		{/if}
	{/if}
</div>
