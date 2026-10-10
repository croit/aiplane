<script lang="ts">
	import type { Priority } from '#lib/priority.js';
	import { t } from '#lib/i18n.svelte.js';
	let { value = $bindable(null), inheritLabel, disabled = false, onchange }: {
		value: Priority | null;
		inheritLabel: string;
		disabled?: boolean;
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
		<option value="low">{t('priority-low')}</option>
		<option value="medium">{t('priority-medium')}</option>
		<option value="high">{t('priority-high')}</option>
	</select>
</label>
