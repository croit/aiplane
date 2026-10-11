<script lang="ts">
	import { issuesUnder, type agentUiCapabilities, type AgentSummary, type Granted, type Spec, type SpecIssue } from '#lib/agents.js';
	import { t } from '#lib/i18n.svelte.js';
	import MainAgentForm from './MainAgentForm.svelte';
	import RoutesForm from './RoutesForm.svelte';
	import SettingsForm from './SettingsForm.svelte';
	import StateSlotsForm from './StateSlotsForm.svelte';

	/**
	 * The form-based builder: the same spec the JSON tab shows, as sections.
	 * A section's badge counts the validator's issues under it, so a failed
	 * save points at where to look even with the section folded.
	 */
	let { spec = $bindable(), issues, granted, agents, ongrants, capabilities }: {
		spec: Spec;
		issues: SpecIssue[];
		granted: Granted;
		agents: AgentSummary[];
		ongrants: () => void;
		capabilities: ReturnType<typeof agentUiCapabilities>;
	} = $props();

	const count = (...prefixes: string[]) => prefixes.reduce((n, p) => n + issuesUnder(issues, p).length, 0);
	const sections = $derived([
		{ id: 'main', title: 'agents-section-main', hint: 'agents-section-main-hint', n: count('main') },
		{ id: 'state', title: 'agents-section-state', hint: 'agents-section-state-hint', n: count('state') },
		{ id: 'routes', title: 'agents-section-routes', hint: 'agents-section-routes-hint', n: count('routes', 'router') },
		{ id: 'settings', title: 'agents-section-settings', hint: 'agents-section-settings-hint', n: count('profile', 'verifiers', 'finish', 'publish') }
	]);
</script>

<div class="space-y-3">
	{#each sections as section (section.id)}
		<div class="collapse collapse-arrow rounded-box border border-base-300 bg-base-200">
			<input type="checkbox" checked={section.id === 'main'} aria-label={t(section.title)} />
			<div class="collapse-title flex items-center gap-2">
				<span class="font-semibold">{t(section.title)}</span>
				{#if section.n}<span class="badge badge-error badge-sm">{section.n}</span>{/if}
				<span class="hidden text-sm font-normal text-base-content/60 sm:inline">{t(section.hint)}</span>
			</div>
			<div class="collapse-content">
				<div class="pt-2">
					{#if section.id === 'main'}
						<MainAgentForm bind:spec {issues} {granted} {ongrants} />
					{:else if section.id === 'state'}
						<StateSlotsForm bind:spec {issues} />
					{:else if section.id === 'routes'}
						<RoutesForm bind:spec {issues} {agents} models={granted.models} />
					{:else}
						<SettingsForm bind:spec {issues} {capabilities} />
					{/if}
				</div>
			</div>
		</div>
	{/each}
</div>
