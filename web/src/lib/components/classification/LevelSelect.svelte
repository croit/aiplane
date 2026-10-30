<script lang="ts">
	import { sortedLevels, type Level } from '#lib/classification.js';
	import { t } from '#lib/i18n.svelte.js';

	/** One level out of the draft's, lowest first; `none` adds an empty choice (an unrated destination, an unmapped marking). */
	let { levels, value = $bindable(), ariaLabel, none = null, size = 'sm', onchange, disabledIds = [] }: {
		levels: readonly Level[];
		value: string | null;
		ariaLabel: string;
		none?: string | null;
		size?: 'xs' | 'sm' | 'md';
		onchange?: (value: string | null) => void;
		disabledIds?: readonly string[];
	} = $props();
</script>

<select
	class="select select-{size} w-full min-w-0"
	aria-label={ariaLabel}
	value={value ?? ''}
	onchange={(e) => {
		const next = e.currentTarget.value === '' ? null : e.currentTarget.value;
		value = next;
		onchange?.(next);
	}}
>
	{#if none !== null}<option value="">{none}</option>{/if}
	{#each sortedLevels(levels) as level (level.id)}<option value={level.id} disabled={disabledIds.includes(level.id)}>{level.name || t('cls-level-unnamed')}</option>{/each}
</select>
