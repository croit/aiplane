<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { untrack } from 'svelte';
	import Modal from '#lib/components/ui/Modal.svelte';
	import StatusPill from '#lib/components/ui/StatusPill.svelte';
	import AgentDetailsPanel from './AgentDetailsPanel.svelte';
	import { ownedAgentsApi, AGENT_DETAIL_TABS, type AgentDetailTab, type AgentProfile } from '#lib/owned-agents.js';
	import { organizationsApi, type OrganizationListRow } from '#lib/organizations.js';
	import { agentsApi } from '#lib/agents.js';
	import { t } from '#lib/i18n.svelte.js';
	import { me } from '#lib/session.svelte.js';

	let { initial }: { initial: AgentProfile } = $props();

	let profile = $state<AgentProfile>(untrack(() => ({ ...initial })));
	let error = $state<string | null>(null);
	let busy = $state(false);
	let teams = $state<OrganizationListRow[]>([]);
	let copying = $state(false);
	let copyTarget = $state('');
	let copyName = $state('');
	let copyError = $state<string | null>(null);

	const owner = $derived(me.value?.id === profile.owner.id);
	const agent = $derived({ id: profile.id, name: profile.name, display: profile.display, description: profile.description, active: profile.active });
	const requestedTab = $derived(page.url.searchParams.get('tab'));
	const initialTab = $derived<AgentDetailTab>(requestedTab && requestedTab in AGENT_DETAIL_TABS ? (requestedTab as AgentDetailTab) : 'task');
	const ownerName = $derived(profile.owner.name ?? profile.owner.email);

	let teamsAsked = false;
	$effect(() => {
		if (teamsAsked || !owner || !me.value?.can_manage_agents) return;
		teamsAsked = true;
		organizationsApi.list().then(
			(result) => (teams = result.organizations.filter((row) => row.can_create_agent)),
			() => (teams = [])
		);
	});

	async function setActive() {
		busy = true;
		error = null;
		try {
			const result = await ownedAgentsApi.setActive(profile.id, !profile.active);
			profile.active = result.active;
		} catch (caught) {
			error = (caught as Error).message;
		} finally {
			busy = false;
		}
	}

	async function remove() {
		if (!confirm(t('agents-delete-personal-confirm', { name: profile.display || profile.name }))) return;
		busy = true;
		error = null;
		try {
			await agentsApi.remove(profile.id);
			await goto('/agents');
		} catch (caught) {
			error = (caught as Error).message;
			busy = false;
		}
	}

	async function openChat() {
		busy = true;
		error = null;
		try {
			const result = await ownedAgentsApi.createSession(profile.id);
			await goto(`/chat/${result.session.id}`);
		} catch (caught) {
			error = (caught as Error).message;
			busy = false;
		}
	}

	function startCopy() {
		copyTarget = teams[0]?.id ?? '';
		copyName = profile.display || profile.name;
		copyError = null;
		copying = true;
	}

	async function copyToTeam() {
		busy = true;
		copyError = null;
		try {
			const created = await organizationsApi.createAgent(copyTarget, { name: copyName.trim(), description: '', copy_from: profile.id });
			copying = false;
			await goto(`/agents/${encodeURIComponent(created.agent.id)}`);
		} catch (caught) {
			copyError = (caught as Error).message;
		} finally {
			busy = false;
		}
	}
</script>

<div class="w-full space-y-4">
	<a class="link link-hover text-sm" href="/agents">← {t('agents-back')}</a>
	{#if me.loaded && !owner}
		<div class="alert alert-warning" role="status"><span>{t('agents-admin-editing', { name: ownerName })}</span></div>
	{/if}
	<header class="flex flex-wrap items-start justify-between gap-3">
		<div class="min-w-0 space-y-1">
			<div class="flex flex-wrap items-center gap-2">
				<h1 class="m-0 truncate text-2xl font-bold">{profile.display || profile.name}</h1>
				<span class="badge badge-outline badge-sm">{t('agents-kind-personal')}</span>
				<StatusPill size="sm" tone={profile.active ? 'ok' : 'warn'}>{t(profile.active ? 'agents-status-active' : 'agents-status-inactive')}</StatusPill>
			</div>
			{#if profile.description}<p class="text-sm text-base-content/70">{profile.description}</p>{/if}
		</div>
		<div class="flex flex-wrap gap-2">
			{#if owner && teams.length}
				<button class="btn btn-sm" type="button" disabled={busy} onclick={startCopy}>{t('agents-copy-to-team')}</button>
			{/if}
			<button class="btn btn-sm" type="button" disabled={busy} onclick={() => void setActive()}>{t(profile.active ? 'agents-action-deactivate' : 'agents-action-activate')}</button>
			{#if owner && profile.active}
				<button class="btn btn-primary btn-sm" type="button" disabled={busy} onclick={() => void openChat()}>{t('agent-action-chat')}</button>
			{/if}
			<button class="btn btn-ghost btn-sm text-error" type="button" disabled={busy} onclick={() => void remove()}>{t('agents-delete')}</button>
		</div>
	</header>
	{#if error}<div class="alert alert-error" role="alert"><span>{error}</span></div>{/if}

	{#if me.loaded}
		<AgentDetailsPanel {agent} {profile} {owner} initiallyOpenTab={initialTab} onprofile={(next) => (profile = next)} />
	{:else}
		<div class="skeleton h-64 w-full"></div>
	{/if}
</div>

{#snippet copyActions()}
	<button class="btn btn-ghost" type="button" disabled={busy} onclick={() => (copying = false)}>{t('admin-cancel')}</button>
	<button class="btn btn-primary" type="submit" form="agent-copy" disabled={busy || !copyTarget || !copyName.trim()}>{t('agents-copy-submit')}</button>
{/snippet}

<Modal bind:open={copying} title={t('agents-copy-to-team')} footer={copyActions}>
	<form id="agent-copy" class="flex flex-col gap-3" onsubmit={(event) => { event.preventDefault(); void copyToTeam(); }}>
		<p class="text-sm text-base-content/70">{t('agents-copy-help')}</p>
		{#if copyError}<div class="alert alert-error text-sm" role="alert"><span>{copyError}</span></div>{/if}
		<label class="flex flex-col gap-1">
			<span class="label-text">{t('agents-copy-team')}</span>
			<select id="agent-copy-team" class="select w-full" bind:value={copyTarget}>
				{#each teams as team (team.id)}<option value={team.id}>{team.name}</option>{/each}
			</select>
		</label>
		<label class="flex flex-col gap-1">
			<span class="label-text">{t('agents-create-name')}</span>
			<input id="agent-copy-name" class="input w-full" bind:value={copyName} required maxlength="80" />
		</label>
	</form>
</Modal>
