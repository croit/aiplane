<script lang="ts">
	import SectionTabs from '#lib/components/SectionTabs.svelte';
	import { sectionTabs } from '#lib/section-tabs.js';
	import { t } from '#lib/i18n.svelte.js';

	let { children }: { children: import('svelte').Snippet } = $props();
</script>

<div class="flex w-full flex-col gap-4">
	<h1 class="text-2xl font-bold">{t('tools-heading')}</h1>
	<SectionTabs tabs={sectionTabs.tools} label="tools-heading" />
	{@render children()}
</div>
