<script lang="ts">
	import { addLevel, LEVEL_COLORS, moveLevel, removeLevel, type Draft } from '#lib/classification.js';
	import LevelBadge from './LevelBadge.svelte';
	import LevelSelect from './LevelSelect.svelte';
	import { t } from '#lib/i18n.svelte.js';

	let { draft = $bindable() }: { draft: Draft } = $props();
</script>

<div class="space-y-4">
	<p class="text-sm text-base-content/70">{t('cls-levels-lead')}</p>
	{#each draft.levels as level, index (level.id)}
		<div class="card card-border bg-base-100">
			<div class="card-body gap-3 p-4">
				<div class="flex flex-wrap items-center gap-2">
					<span class="badge badge-ghost">{index + 1}</span>
					<input class="input input-sm min-w-0 flex-1" aria-label={t('cls-level-name')} placeholder={t('cls-level-name')} bind:value={level.name} />
					<select class="select select-sm w-32" aria-label={t('cls-level-color')} bind:value={level.color}>
						{#each LEVEL_COLORS as color (color)}<option value={color}>{t(`cls-color-${color}`)}</option>{/each}
					</select>
					<LevelBadge name={level.name || t('cls-level-unnamed')} color={level.color} />
					<div class="join">
						<button class="btn btn-sm join-item" type="button" disabled={index === 0} aria-label={t('cls-level-up')} onclick={() => (draft.levels = moveLevel(draft.levels, index, -1))}>↑</button>
						<button class="btn btn-sm join-item" type="button" disabled={index === draft.levels.length - 1} aria-label={t('cls-level-down')} onclick={() => (draft.levels = moveLevel(draft.levels, index, 1))}>↓</button>
					</div>
					<button class="btn btn-ghost btn-sm" type="button" disabled={draft.levels.length <= 1} onclick={() => (draft = removeLevel(draft, level.id))}>{t('cls-remove')}</button>
				</div>
				<label class="flex flex-col gap-1">
					<span class="text-xs">{t('cls-level-description')}</span>
					<input class="input input-sm w-full" bind:value={level.description} />
				</label>
				<div class="grid gap-3 sm:grid-cols-3">
					<label class="flex flex-col gap-1">
						<span class="text-xs">{t('cls-level-marking')}</span>
						<input class="input input-sm w-full" bind:value={level.marking_text} />
					</label>
					<label class="flex flex-col gap-1">
						<span class="text-xs">{t('cls-level-filename')}</span>
						<input class="input input-sm w-full" bind:value={level.filename_tag} />
					</label>
					<label class="flex flex-col gap-1">
						<span class="text-xs">{t('cls-level-mip')}</span>
						<input class="input input-sm w-full" bind:value={level.mip_label_id} />
					</label>
				</div>
			</div>
		</div>
	{/each}
	<button class="btn btn-sm" type="button" onclick={() => (draft = addLevel(draft))}>{t('cls-level-add')}</button>
	<label class="flex flex-col gap-1 sm:max-w-xs">
		<span class="text-sm font-medium">{t('cls-default-level')}</span>
		<LevelSelect levels={draft.levels} bind:value={draft.policy.default_level} ariaLabel={t('cls-default-level')} />
		<span class="text-xs text-base-content/60">{t('cls-default-level-help')}</span>
	</label>
</div>
