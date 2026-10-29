<script lang="ts">
	import { onMount } from 'svelte';
	import AgentMemoryPanel from './AgentMemoryPanel.svelte';
	import AgentActivityPanel from './AgentActivityPanel.svelte';
	import AgentAccessPanel from './AgentAccessPanel.svelte';
	import OwnedAgentCommunicationPanel from './OwnedAgentCommunicationPanel.svelte';
	import AgentProfilePanel from './AgentProfilePanel.svelte';
	import type { OwnedAgent } from '#lib/agent-state.js';
	import { AGENT_DETAIL_TABS, agentDetailTabs, type AgentDetailTab, type AgentProfile } from '#lib/owned-agents.js';
	import { t } from '#lib/i18n.svelte.js';

	let {
		agent,
		profile,
		owner,
		initiallyOpenTab = 'task',
		onprofile
	}: {
		agent: OwnedAgent;
		profile: AgentProfile;
		/** The owner sees every tab; an admin changing another user's agent only its settings. */
		owner: boolean;
		initiallyOpenTab?: AgentDetailTab;
		onprofile: (profile: AgentProfile) => void;
	} = $props();
	let selected = $state<AgentDetailTab>('task');
	const tabs = $derived(agentDetailTabs(owner).map((id) => ({ id, label: AGENT_DETAIL_TABS[id] })));
	const panelId = (tab: AgentDetailTab) => `agent-${agent.id}-panel-${tab}`;
	const tabId = (tab: AgentDetailTab) => `agent-${agent.id}-tab-${tab}`;
	onMount(() => selectTab(agentDetailTabs(owner).includes(initiallyOpenTab) ? initiallyOpenTab : 'task'));

	function selectTab(tab: AgentDetailTab) {
		selected = tab;
	}

	function moveTab(event: KeyboardEvent, current: AgentDetailTab) {
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

<section class="min-w-0 space-y-4 rounded-box border border-base-300 bg-base-100 p-3 sm:p-4" aria-label={t('agents-details-heading')}>
	<div role="tablist" class="tabs tabs-border w-full overflow-x-auto" aria-label={t('agents-details-tablist')}>
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

	<div id={panelId('task')} role="tabpanel" aria-labelledby={tabId('task')} tabindex="0" hidden={selected !== 'task'}>
		<AgentProfilePanel {profile} onsaved={onprofile} />
	</div>
	<div id={panelId('tools')} role="tabpanel" aria-labelledby={tabId('tools')} tabindex="0" hidden={selected !== 'tools'}>
		<AgentAccessPanel {agent} tabbed active={selected === 'tools'} />
	</div>
	{#if owner}
	<div id={panelId('memory')} role="tabpanel" aria-labelledby={tabId('memory')} tabindex="0" hidden={selected !== 'memory'}>
		<AgentMemoryPanel {agent} tabbed active={selected === 'memory'} />
	</div>
	<div id={panelId('activity')} role="tabpanel" aria-labelledby={tabId('activity')} tabindex="0" hidden={selected !== 'activity'}>
		<AgentActivityPanel {agent} tabbed active={selected === 'activity'} />
	</div>
	<div id={panelId('communication')} role="tabpanel" aria-labelledby={tabId('communication')} tabindex="0" hidden={selected !== 'communication'}>
		<OwnedAgentCommunicationPanel {agent} tabbed active={selected === 'communication'} />
	</div>
	<div id={panelId('workspace')} role="tabpanel" aria-labelledby={tabId('workspace')} tabindex="0" hidden={selected !== 'workspace'}>
		<div class="card card-border">
			<div class="card-body gap-2 p-4">
				<h3 class="card-title text-base">{t('agent-workspace-heading')}</h3>
				<p class="text-sm text-base-content/70">{t('agents-workspace-tab-help')}</p>
				<div class="card-actions justify-end">
					<a class="btn btn-sm" href={`/agents/${encodeURIComponent(agent.id)}/workspace`}>{t('agents-workspace-open')}</a>
				</div>
			</div>
		</div>
	</div>
	<div id={panelId('automation')} role="tabpanel" aria-labelledby={tabId('automation')} tabindex="0" hidden={selected !== 'automation'}>
		<div class="grid min-w-0 gap-3 md:grid-cols-2">
			<article class="card card-border">
				<div class="card-body gap-2 p-4">
					<h3 class="card-title text-base">{t('scheduled-heading')}</h3>
					<p class="text-sm text-base-content/70">{t('agents-scheduled-tab-help')}</p>
					<div class="card-actions justify-end">
						<a class="btn btn-sm" href={`/scheduled?principal_id=${encodeURIComponent(agent.id)}`}>{t('agents-automation-manage')}</a>
						<a class="btn btn-sm btn-primary" href={`/scheduled/new?personal_agent_id=${encodeURIComponent(agent.id)}`}>{t('agent-background-add-schedule')}</a>
					</div>
				</div>
			</article>
			<article class="card card-border">
				<div class="card-body gap-2 p-4">
					<h3 class="card-title text-base">{t('webhooks-heading')}</h3>
					<p class="text-sm text-base-content/70">{t('agents-webhooks-tab-help')}</p>
					<div class="card-actions justify-end">
						<a class="btn btn-sm" href={`/webhooks?principal_id=${encodeURIComponent(agent.id)}`}>{t('agents-automation-manage')}</a>
						<a class="btn btn-sm btn-primary" href={`/webhooks/new?personal_agent_id=${encodeURIComponent(agent.id)}`}>{t('agent-background-add-webhook')}</a>
					</div>
				</div>
			</article>
		</div>
	</div>
	{/if}
</section>
