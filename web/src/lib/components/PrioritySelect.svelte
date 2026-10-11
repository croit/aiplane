<script lang="ts">
	import { canChoosePriority, type Priority } from '#lib/priority.js';
	import { t } from '#lib/i18n.svelte.js';
	let { value = $bindable(null), inheritLabel, disabled = false, maximum = 'high', onchange }: {
		value: Priority | null;
		inheritLabel: string;
		disabled?: boolean;
		maximum?: Priority;
		onchange?: (value: Priority | null) => void;
	} = $props();
</script>

<label class="fieldset">
	<span class="fieldset-legend">{t('priority-label')}</span>
	<select aria-label={t('priority-label')} class="select select-sm w-full" {disabled} value={value ?? ''} onchange={(event) => {
		value = (event.currentTarget.value || null) as Priority | null;
		onchange?.(value);
	}}>
		<option value="">{inheritLabel}</option>
		{#each ['low', 'medium', 'high'] as priority}
			<option value={priority} disabled={!canChoosePriority(priority as Priority, maximum)}>{t(`priority-${priority}`)}{!canChoosePriority(priority as Priority, maximum) ? ` — ${t('priority-admin-only')}` : ''}</option>
		{/each}
	</select>
</label>
