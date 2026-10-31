<script lang="ts">
	import type { DetectorModel } from '#lib/classification.js';
	import { t } from '#lib/i18n.svelte.js';

	/**
	 * The detector model picker: System One models first (recommended), chat
	 * models as the labelled fallback. `cleared` says whether a model may see
	 * the highest level; `lockUncleared` greys the others out.
	 */
	let { models, value = $bindable(), cleared, lockUncleared = false, class: cls = '' }: {
		models: readonly DetectorModel[];
		value: string;
		cleared: (model: DetectorModel) => boolean;
		lockUncleared?: boolean;
		class?: string;
	} = $props();

	const groups = $derived([
		{ kind: 'system_one' as const, label: t('cls-model-group-system-one') },
		{ kind: 'chat' as const, label: t('cls-model-group-chat') }
	]);
	const chatPicked = $derived(models.some((m) => m.id === value && m.kind === 'chat'));
</script>

<select class="select select-sm w-full {cls}" bind:value>
	<option value="">{t('cls-policy-model-none')}</option>
	{#each groups as group (group.kind)}
		{@const inGroup = models.filter((m) => m.kind === group.kind)}
		{#if inGroup.length > 0}
			<optgroup label={group.label}>
				{#each inGroup as m (m.id)}
					<option value={m.id} disabled={lockUncleared && !cleared(m)}>{cleared(m) ? m.id : t('cls-policy-model-not-cleared', { model: m.id })}</option>
				{/each}
			</optgroup>
		{/if}
	{/each}
</select>
{#if models.length === 0}
	<div class="alert alert-warning alert-soft text-xs" role="status">
		<span>{t('cls-model-none-available')} <a class="link" href="/admin/models">{t('cls-model-none-available-link')}</a></span>
	</div>
{/if}
{#if chatPicked}
	<div class="alert alert-warning alert-soft text-xs" role="status"><span>{t('cls-model-chat-warning')}</span></div>
{/if}
