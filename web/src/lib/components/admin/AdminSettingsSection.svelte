<script lang="ts">
	import type { AdminSettingsSection } from '$lib/admin-settings';
	import { settingsCatalogKey } from '$lib/admin-settings';
	import AdminSettingsField from './AdminSettingsField.svelte';
	import { t } from '$lib/i18n.svelte';

	let { section, drafts, onchange, onsave, onclear, saving }: { section: AdminSettingsSection; drafts: Record<string, string>; onchange: (key: string, value: string) => void; onsave: () => Promise<void>; onclear: (key: string) => Promise<void>; saving: boolean } = $props();
	let master = $derived(section.fields[0]?.key === `${section.name}.enabled` && section.fields[0]?.kind === 'bool' ? section.fields[0] : null);
	let fields = $derived(master ? section.fields.slice(1) : section.fields);
	let titleKey = $derived(settingsCatalogKey('settings-s-', section.name));
	let blurbKey = $derived(`${titleKey}-blurb`);
</script>

<form data-testid={`settings-section-${section.name.replaceAll('.', '-')}`} class="card border border-base-300" onsubmit={(event) => { event.preventDefault(); void onsave(); }}>
	<div class="card-body gap-3">
		<div><h2 class="card-title text-base">{t(titleKey)}</h2><p class="m-0 text-sm text-base-content/70">{t(blurbKey)}</p></div>
		<div class="flex flex-col gap-3">
			{#if master}<AdminSettingsField field={master} draft={drafts[master.key] ?? ''} {onchange} {onclear} />{/if}
			{#if section.enabled === false && fields.length > 0}
				<details><summary class="link link-hover cursor-pointer text-sm">{t('settings-show-fields', { count: fields.length })}</summary><div class="grid grid-cols-1 gap-x-4 gap-y-3 pt-3 sm:grid-cols-2">{#each fields as field (field.key)}<AdminSettingsField {field} draft={drafts[field.key] ?? ''} {onchange} {onclear} />{/each}</div></details>
			{:else}<div class="grid grid-cols-1 gap-x-4 gap-y-3 sm:grid-cols-2">{#each fields as field (field.key)}<AdminSettingsField {field} draft={drafts[field.key] ?? ''} {onchange} {onclear} />{/each}</div>{/if}
		</div>
		{#if section.fields.some((field) => field.restart)}<p class="m-0 text-xs text-warning">{t('settings-restart-note')}</p>{/if}
		<div class="card-actions justify-end"><button type="submit" class="btn btn-primary btn-sm" disabled={saving}>{t('settings-save')}</button></div>
	</div>
</form>
