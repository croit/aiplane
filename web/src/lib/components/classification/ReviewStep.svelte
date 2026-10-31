<script lang="ts">
	import { diffIsEmpty, diffSummary, draftIssues, unratedCount, type ClassificationState, type Draft } from '#lib/classification.js';
	import { t } from '#lib/i18n.svelte.js';

	let { before, draft, error }: { before: ClassificationState; draft: Draft; error: string | null } = $props();
	const diff = $derived(diffSummary(before, draft));
	const issues = $derived(draftIssues(draft));
</script>

<div class="space-y-4">
	<p class="text-sm text-base-content/70">{t('cls-review-lead')}</p>
	{#if error}<div class="alert alert-error text-sm" role="alert"><span>{error}</span></div>{/if}
	{#each issues as issue (issue)}<div class="alert alert-warning text-sm" role="status"><span>{t(issue)}</span></div>{/each}
	{#if unratedCount(draft.destinations) > 0}
		<div class="alert alert-warning text-sm" role="status"><span>{t('cls-destinations-unrated', { count: unratedCount(draft.destinations) })}</span></div>
	{/if}
	{#if draft.policy.mode === 'enforce' && !before.active}
		<div class="alert alert-info text-sm" role="status"><span>{t('cls-review-monitor-first')}</span></div>
	{/if}

	<div class="card card-border bg-base-100">
		<div class="card-body gap-2 p-4 text-sm">
			<h4 class="card-title text-base">{t('cls-review-scheme')}</h4>
			<p class="m-0">{t('cls-review-levels', { names: draft.levels.map((l) => l.name).join(' < ') })}</p>
			<p class="m-0">{t('cls-review-counts', { classes: draft.classes.length, markings: draft.markings.length, destinations: draft.destinations.length - diff.unrated })}</p>
			<p class="m-0">{t('cls-review-mode', { mode: t(`cls-policy-mode-${draft.policy.mode}`) })}</p>
		</div>
	</div>

	<div class="card card-border bg-base-100">
		<div class="card-body gap-1 p-4 text-sm">
			<h4 class="card-title text-base">{t('cls-review-changes')}</h4>
			<ul class="m-0 list-disc space-y-1 ps-5">
				{#if diff.levelsAdded.length}<li>{t('cls-diff-levels-added', { names: diff.levelsAdded.join(', ') })}</li>{/if}
				{#if diff.levelsRemoved.length}<li>{t('cls-diff-levels-removed', { names: diff.levelsRemoved.join(', ') })}</li>{/if}
				{#if diff.levelsRenamed}<li>{t('cls-diff-levels-edited', { count: diff.levelsRenamed })}</li>{/if}
				{#if diff.levelsReordered}<li>{t('cls-diff-levels-reordered')}</li>{/if}
				{#if diff.classesAdded.length}<li>{t('cls-diff-classes-added', { names: diff.classesAdded.join(', ') })}</li>{/if}
				{#if diff.classesRemoved.length}<li>{t('cls-diff-classes-removed', { names: diff.classesRemoved.join(', ') })}</li>{/if}
				{#if diff.classesChanged.length}<li>{t('cls-diff-classes-changed', { names: diff.classesChanged.join(', ') })}</li>{/if}
				{#if diff.markingsBefore !== diff.markingsAfter}<li>{t('cls-diff-markings', { before: diff.markingsBefore, after: diff.markingsAfter })}</li>{/if}
				{#if diff.destinationsRated}<li>{t('cls-diff-destinations-rated', { count: diff.destinationsRated })}</li>{/if}
				{#if diff.destinationsChanged}<li>{t('cls-diff-destinations-changed', { count: diff.destinationsChanged })}</li>{/if}
				{#if diff.policyChanged.length}<li>{t('cls-diff-policy', { names: diff.policyChanged.map((k) => t(`cls-policy-field-${k}`)).join(', ') })}</li>{/if}
				{#if diffIsEmpty(diff)}
					<li class="list-none text-base-content/60">{t('cls-diff-none')}</li>
				{/if}
			</ul>
		</div>
	</div>
</div>
