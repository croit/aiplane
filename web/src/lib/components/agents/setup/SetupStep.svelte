<script lang="ts">
	import { agentUiCapabilities, type Spec } from '#lib/agents.js';
	import { useWorkspace } from '#lib/agent-workspace.svelte.js';
	import { t } from '#lib/i18n.svelte.js';
	import type { StepKey } from '#lib/agent-setup.js';
	import AbilitiesStep from './AbilitiesStep.svelte';
	import BasicsStep from './BasicsStep.svelte';
	import IdentityStep from './IdentityStep.svelte';
	import ReviewStep from './ReviewStep.svelte';
	import RoutesStep from './RoutesStep.svelte';
	import ScopeStep from './ScopeStep.svelte';
	import SiteStep from './SiteStep.svelte';
	import SlotsStep from './SlotsStep.svelte';
	import StartStep from './StartStep.svelte';

	/**
	 * One step of the setup, whichever host shows it: the assistant page binds
	 * the agent's buffer, the overview's modal binds a copy it applies on
	 * Apply. The step components themselves never know which.
	 */
	let { step, spec = $bindable(), onfix }: { step: StepKey; spec: Spec; onfix: (step: StepKey | null) => void } = $props();
	const ws = useWorkspace();
	const capabilities = $derived(agentUiCapabilities(ws.detail));
</script>

{#if step === 'start'}
	<StartStep bind:spec />
{:else if step === 'basics'}
	<BasicsStep bind:spec />
{:else if step === 'scope'}
	<ScopeStep bind:spec />
{:else if step === 'abilities'}
	<AbilitiesStep bind:spec />
{:else if step === 'slots'}
	<SlotsStep bind:spec />
{:else if step === 'identity'}
	<IdentityStep bind:spec />
{:else if step === 'routes'}
	<RoutesStep bind:spec />
{:else if step === 'site' && capabilities.site}
	<SiteStep bind:spec />
{:else if step === 'site'}
	<div class="alert alert-info"><span>{t('organization-configuration-only')}</span></div>
{:else}
	<ReviewStep bind:spec {onfix} />
{/if}
