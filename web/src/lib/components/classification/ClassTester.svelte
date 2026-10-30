<script lang="ts">
	import { classificationApi, failure, type Level, type TestResult } from '#lib/classification.js';
	import LevelBadge from './LevelBadge.svelte';
	import { t } from '#lib/i18n.svelte.js';

	/** Run a sample file or text through the saved scheme and show the level it would get and why. */
	let { levels }: { levels: readonly Level[] } = $props();

	let text = $state('');
	let file = $state<File | null>(null);
	let result = $state<TestResult | null>(null);
	let error = $state<string | null>(null);
	let busy = $state(false);

	async function run() {
		busy = true;
		error = null;
		try {
			result = await classificationApi.test({ file: file ?? undefined, text: text.trim() || undefined });
		} catch (err) {
			result = null;
			error = failure(err);
		} finally {
			busy = false;
		}
	}
	const level = $derived(levels.find((l) => l.id === result?.level_id));
</script>

<div class="card card-border bg-base-100">
	<div class="card-body gap-3 p-4">
		<h4 class="card-title text-base">{t('cls-test-heading')}</h4>
		<p class="m-0 text-sm text-base-content/70">{t('cls-test-lead')}</p>
		<textarea class="textarea w-full" rows="2" aria-label={t('cls-test-text')} placeholder={t('cls-test-text')} bind:value={text}></textarea>
		<div class="flex flex-wrap items-center gap-2">
			<input type="file" class="file-input file-input-sm" aria-label={t('cls-test-file')} onchange={(e) => (file = e.currentTarget.files?.[0] ?? null)} />
			<button class="btn btn-sm" type="button" disabled={busy || (!file && text.trim() === '')} onclick={run}>{t('cls-test-run')}</button>
		</div>
		{#if error}<div class="alert alert-error text-sm" role="alert"><span>{error}</span></div>{/if}
		{#if result}
			<div class="space-y-1 text-sm" role="status">
				<p class="m-0 flex items-center gap-2">{t('cls-test-result')} {#if level}<LevelBadge name={level.name} color={level.color} />{:else}<span class="badge">{result.level_id}</span>{/if}</p>
				{#if result.classes.length > 0}<p class="m-0">{t('cls-test-classes', { classes: result.classes.join(', ') })}</p>{/if}
				{#if result.causes.length > 0}<ul class="m-0 list-disc ps-5 text-base-content/70">{#each result.causes as cause (cause)}<li>{cause}</li>{/each}</ul>{/if}
			</div>
		{/if}
	</div>
</div>
