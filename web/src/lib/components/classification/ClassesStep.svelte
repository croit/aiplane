<script lang="ts">
	import { ACTIONS, blankClass, detectorsOf, PATTERN_IDS, togglePattern, type ClassificationState, type Draft } from '#lib/classification.js';
	import LevelBadge from './LevelBadge.svelte';
	import LevelSelect from './LevelSelect.svelte';
	import TermsUpload from './TermsUpload.svelte';
	import ClassTester from './ClassTester.svelte';
	import { t } from '#lib/i18n.svelte.js';

	let { draft = $bindable(), saved }: { draft: Draft; saved: ClassificationState } = $props();
	const isSaved = (id: string) => saved.classes.some((c) => c.id === id);
	const hasModel = $derived(draft.policy.detector_model !== '');
	let expanded = $state<Record<string, boolean>>({});
	const levelOf = (id: string) => draft.levels.find((l) => l.id === id);

	function addClass() {
		const cls = blankClass(draft);
		expanded[cls.id] = true;
		draft.classes = [...draft.classes, cls];
	}
</script>

<div class="space-y-4">
	<p class="text-sm text-base-content/70">{t('cls-classes-lead')}</p>
	{#each draft.classes as cls, index (cls.id)}
		{@const level = levelOf(cls.level_id)}
		<div class="collapse collapse-arrow border border-base-300 bg-base-100">
			<input type="checkbox" aria-label={cls.name || t('cls-level-unnamed')} bind:checked={expanded[cls.id]} />
			<div class="collapse-title flex flex-wrap items-center gap-x-3 gap-y-1 pe-10">
				<span class="font-medium">{cls.name || t('cls-level-unnamed')}</span>
				{#if level}<LevelBadge name={level.name} color={level.color} />{/if}
				<span class="text-xs text-base-content/60">{t(`cls-action-${cls.action}`)}</span>
				<span class="flex flex-wrap gap-1">
					{#each detectorsOf(cls, hasModel) as detector, i (i)}
						{#if detector.kind === 'pattern'}
							<span class="badge badge-sm badge-outline">{t(`cls-pattern-${detector.id}`)}</span>
						{:else if detector.kind === 'rule'}
							<span class={['badge badge-sm', detector.active ? 'badge-outline' : 'badge-ghost text-base-content/50']} title={detector.active ? undefined : t('cls-class-rule-inactive')}>{t('cls-detector-rule')}</span>
						{:else}
							<span class="badge badge-sm badge-outline">{t('cls-detector-terms', { count: detector.count })}</span>
						{/if}
					{/each}
				</span>
			</div>
			<div class="collapse-content">
			<div class="flex flex-col gap-3 pt-2">
				<div class="flex flex-wrap items-end gap-2">
					<label class="flex min-w-0 flex-1 flex-col gap-1">
						<span class="text-xs">{t('cls-class-name')}</span>
						<input class="input input-sm w-full" bind:value={cls.name} />
					</label>
					<label class="flex w-44 flex-col gap-1">
						<span class="text-xs">{t('cls-class-level')}</span>
						<LevelSelect levels={draft.levels} bind:value={cls.level_id} ariaLabel={t('cls-class-level')} />
					</label>
					<label class="flex w-36 flex-col gap-1">
						<span class="text-xs">{t('cls-class-action')}</span>
						<select class="select select-sm" bind:value={cls.action}>
							{#each ACTIONS as action (action)}<option value={action}>{t(`cls-action-${action}`)}</option>{/each}
						</select>
					</label>
					<button class="btn btn-ghost btn-sm" type="button" onclick={() => (draft.classes = draft.classes.filter((_, i) => i !== index))}>{t('cls-remove')}</button>
				</div>
				<label class="flex flex-col gap-1">
					<span class="text-xs">{t('cls-class-description')}</span>
					<input class="input input-sm w-full" bind:value={cls.description} />
				</label>

				<fieldset class="flex flex-wrap items-center gap-x-4 gap-y-1">
					<legend class="mb-1 text-xs">{t('cls-class-patterns')}</legend>
					{#each PATTERN_IDS as pattern (pattern)}
						<label class="flex items-center gap-2 text-sm">
							<input type="checkbox" class="checkbox checkbox-sm" checked={cls.patterns.includes(pattern)} onchange={() => (cls.patterns = togglePattern(cls.patterns, pattern))} />
							{t(`cls-pattern-${pattern}`)}
						</label>
					{/each}
				</fieldset>

				<div class="space-y-2 rounded-box border border-base-300 p-3">
					<div class="flex flex-wrap items-center gap-2">
						<h5 class="m-0 text-sm font-semibold">{t('cls-class-rule')}</h5>
						{#if !hasModel && cls.rule_question.trim() !== ''}<span class="badge badge-warning badge-sm">{t('cls-class-rule-inactive')}</span>{/if}
					</div>
					<p class="m-0 text-xs text-base-content/60">{t('cls-class-rule-help')}</p>
					<label class="flex flex-col gap-1">
						<span class="text-xs">{t('cls-class-question')}</span>
						<textarea class="textarea w-full" rows="2" bind:value={cls.rule_question}></textarea>
					</label>
					<div class="grid gap-3 sm:grid-cols-2">
						<label class="flex flex-col gap-1">
							<span class="text-xs">{t('cls-class-positive')}</span>
							<textarea class="textarea w-full" rows="2" bind:value={cls.rule_positive}></textarea>
						</label>
						<label class="flex flex-col gap-1">
							<span class="text-xs">{t('cls-class-negative')}</span>
							<textarea class="textarea w-full" rows="2" bind:value={cls.rule_negative}></textarea>
						</label>
					</div>
					<label class="flex flex-col gap-1">
						<span class="text-xs">{t('cls-class-threshold', { value: cls.rule_threshold.toFixed(2) })}</span>
						<input type="range" class="range range-sm" min="0" max="1" step="0.05" bind:value={cls.rule_threshold} />
					</label>
				</div>

				<div class="space-y-1">
					<h5 class="m-0 text-sm font-semibold">{t('cls-class-terms')}</h5>
					{#if isSaved(cls.id)}
						<TermsUpload {cls} onterms={(terms) => (draft.classes[index]!.terms = terms)} />
					{:else}
						<p class="m-0 text-sm text-base-content/60">{t('cls-terms-save-first')}</p>
					{/if}
				</div>
			</div>
			</div>
		</div>
	{:else}
		<p class="text-sm text-base-content/60">{t('cls-classes-empty')}</p>
	{/each}
	<button class="btn btn-sm" type="button" onclick={addClass}>{t('cls-class-add')}</button>
	<ClassTester levels={saved.levels} />
</div>
