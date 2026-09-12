<script lang="ts">
	import { base } from '$app/paths';
	import type { AdminSettingsSection, SettingsCategory } from '$lib/admin-settings';
	import { categorySummary, SETTINGS_CATEGORIES } from '$lib/admin-settings';
	import { t } from '$lib/i18n.svelte';

	let { sections, selected }: { sections: AdminSettingsSection[]; selected: SettingsCategory } = $props();
</script>

<nav class="w-full shrink-0 sm:w-52" aria-label={t('settings-heading')}>
	<!-- daisyUI only styles menu items that sit in an `li`: without it the rows
	     lose their padding, radius and vertical centring (the count badge then
	     rides above the label). -->
	<ul class="menu menu-sm w-full rounded-box bg-base-200">
		{#each SETTINGS_CATEGORIES as category (category)}
			{@const summary = categorySummary(sections.filter((section) => section.category === category))}
			<li>
				<a class:menu-active={selected === category} class="flex items-center justify-between gap-2" href={`${base}/admin/settings?tab=${category}`}>
					<span>{t(`settings-tab-${category}`)}</span>
					{#if summary.switchable > 0}<span class="badge badge-ghost badge-xs">{summary.on}/{summary.switchable}</span>{/if}
				</a>
			</li>
		{/each}
	</ul>
</nav>
