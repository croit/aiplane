<script lang="ts">
	import { onMount } from 'svelte';
	import DotMemoryPanel from './DotMemoryPanel.svelte';
	import DotActivityPanel from './DotActivityPanel.svelte';
	import DotAccessPanel from './DotAccessPanel.svelte';
	import DotCommunicationPanel from './DotCommunicationPanel.svelte';
	import type { Dot } from '#lib/dot-state.js';
	import { t } from '#lib/i18n.svelte.js';

	let { dot, initiallyOpenTab = 'memory' }: { dot: Dot; initiallyOpenTab?: DotDetailTab } = $props();
	type DotDetailTab = 'memory' | 'activity' | 'access' | 'communication' | 'workspace' | 'automation';
	let selected = $state<DotDetailTab>('memory');
	const tabs: { id: DotDetailTab; label: string }[] = [
		{ id: 'memory', label: 'dots-tab-memory' },
		{ id: 'activity', label: 'dots-tab-activity' },
		{ id: 'access', label: 'dots-tab-access' },
		{ id: 'communication', label: 'dots-tab-communication' },
		{ id: 'workspace', label: 'dots-tab-workspace' },
		{ id: 'automation', label: 'dots-tab-automation' },
	];
	const panelId = (tab: DotDetailTab) => `dot-${dot.id}-panel-${tab}`;
	const tabId = (tab: DotDetailTab) => `dot-${dot.id}-tab-${tab}`;
	onMount(() => selectTab(initiallyOpenTab));

	function selectTab(tab: DotDetailTab) {
		selected = tab;
	}

	function moveTab(event: KeyboardEvent, current: DotDetailTab) {
		const index = tabs.findIndex((tab) => tab.id === current);
		let next = index;
		if (event.key === 'ArrowRight') next = (index + 1) % tabs.length;
		else if (event.key === 'ArrowLeft') next = (index + tabs.length - 1) % tabs.length;
		else if (event.key === 'Home') next = 0;
		else if (event.key === 'End') next = tabs.length - 1;
		else return;
		event.preventDefault();
		const nextTab = tabs[next];
		if (!nextTab) return;
		selectTab(nextTab.id);
		requestAnimationFrame(() => document.getElementById(tabId(nextTab.id))?.focus());
	}
</script>

<section class="min-w-0 space-y-4 rounded-box border border-base-300 bg-base-100 p-3 sm:p-4" aria-label={t('dots-details-heading')}>
	<div role="tablist" class="tabs tabs-border w-full overflow-x-auto" aria-label={t('dots-details-tablist')}>
		{#each tabs as tab (tab.id)}
			<button
				id={tabId(tab.id)}
				role="tab"
				type="button"
				class="tab whitespace-nowrap"
				class:tab-active={selected === tab.id}
				aria-selected={selected === tab.id}
				aria-controls={panelId(tab.id)}
				tabindex={selected === tab.id ? 0 : -1}
				onclick={() => selectTab(tab.id)}
				onkeydown={(event) => moveTab(event, tab.id)}
			>{t(tab.label)}</button>
		{/each}
	</div>

	<div id={panelId('memory')} role="tabpanel" aria-labelledby={tabId('memory')} tabindex="0" hidden={selected !== 'memory'}>
		<DotMemoryPanel {dot} tabbed active={selected === 'memory'} />
	</div>
	<div id={panelId('activity')} role="tabpanel" aria-labelledby={tabId('activity')} tabindex="0" hidden={selected !== 'activity'}>
		<DotActivityPanel {dot} tabbed active={selected === 'activity'} />
	</div>
	<div id={panelId('access')} role="tabpanel" aria-labelledby={tabId('access')} tabindex="0" hidden={selected !== 'access'}>
		<DotAccessPanel {dot} tabbed active={selected === 'access'} />
	</div>
	<div id={panelId('communication')} role="tabpanel" aria-labelledby={tabId('communication')} tabindex="0" hidden={selected !== 'communication'}>
		<DotCommunicationPanel {dot} tabbed active={selected === 'communication'} />
	</div>
	<div id={panelId('workspace')} role="tabpanel" aria-labelledby={tabId('workspace')} tabindex="0" hidden={selected !== 'workspace'}>
		<div class="card card-border">
			<div class="card-body gap-2 p-4">
				<h3 class="card-title text-base">{t('dot-workspace-heading')}</h3>
				<p class="text-sm text-base-content/70">{t('dots-workspace-tab-help')}</p>
				<div class="card-actions justify-end">
					<a class="btn btn-sm" href={`/dots/${encodeURIComponent(dot.id)}/workspace`}>{t('dots-workspace-open')}</a>
				</div>
			</div>
		</div>
	</div>
	<div id={panelId('automation')} role="tabpanel" aria-labelledby={tabId('automation')} tabindex="0" hidden={selected !== 'automation'}>
		<div class="grid min-w-0 gap-3 md:grid-cols-2">
			<article class="card card-border">
				<div class="card-body gap-2 p-4">
					<h3 class="card-title text-base">{t('scheduled-heading')}</h3>
					<p class="text-sm text-base-content/70">{t('dots-scheduled-tab-help')}</p>
					<div class="card-actions justify-end">
						<a class="btn btn-sm" href={`/scheduled?principal_id=${encodeURIComponent(dot.id)}`}>{t('dots-automation-manage')}</a>
						<a class="btn btn-sm btn-primary" href={`/scheduled/new?dot_id=${encodeURIComponent(dot.id)}`}>{t('dot-background-add-schedule')}</a>
					</div>
				</div>
			</article>
			<article class="card card-border">
				<div class="card-body gap-2 p-4">
					<h3 class="card-title text-base">{t('webhooks-heading')}</h3>
					<p class="text-sm text-base-content/70">{t('dots-webhooks-tab-help')}</p>
					<div class="card-actions justify-end">
						<a class="btn btn-sm" href={`/webhooks?principal_id=${encodeURIComponent(dot.id)}`}>{t('dots-automation-manage')}</a>
						<a class="btn btn-sm btn-primary" href={`/webhooks/new?dot_id=${encodeURIComponent(dot.id)}`}>{t('dot-background-add-webhook')}</a>
					</div>
				</div>
			</article>
		</div>
	</div>
</section>
