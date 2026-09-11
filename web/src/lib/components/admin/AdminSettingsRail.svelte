<script lang="ts">
	import { base } from '$app/paths';
	import type { AdminSettingsSection, SettingsCategory } from '$lib/admin-settings';
	import { categorySummary, SETTINGS_CATEGORIES } from '$lib/admin-settings';
	import { t } from '$lib/i18n.svelte';

	let { sections, selected }: { sections: AdminSettingsSection[]; selected: SettingsCategory } = $props();
</script>

<nav class="menu menu-sm w-full shrink-0 rounded-box bg-base-200 sm:w-52" aria-label={t('settings-heading')}>
	{#each SETTINGS_CATEGORIES as category (category)}
		{@const summary = categorySummary(sections.filter((section) => section.category === category))}
		<a class:menu-active={selected === category} class="flex justify-between gap-2" href={`${base}/admin/settings?tab=${category}`}><span>{t(`settings-tab-${category}`)}</span>{#if summary.switchable > 0}<span class="badge badge-ghost badge-xs">{summary.on}/{summary.switchable}</span>{/if}</a>
	{/each}
</nav>
