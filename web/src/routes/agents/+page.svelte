<script lang="ts">
	import Modal from '#lib/components/ui/Modal.svelte';
	import ArchitectModal from '#lib/components/agents/ArchitectModal.svelte';
	import ArchitectChat from '#lib/components/agents/ArchitectChat.svelte';
	import { dotsApi } from '#lib/dots.js';
	import { organizationsApi, type OrganizationListRow } from '#lib/organizations.js';
	import { creatableKinds } from '#lib/agents.js';
	import StatusPill from '#lib/components/ui/StatusPill.svelte';
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { agentIdFromName, agentsApi, ownerLabel, type AgentError, type AgentKind, type AgentSummary } from '#lib/agents.js';
	import { locale, t } from '#lib/i18n.svelte.js';
	import { me } from '#lib/session.svelte.js';

	let agents = $state<AgentSummary[]>([]);
	let canViewAll = $state(false);
	/** What the server answered, not what the URL asked: it ignores a non-admin's `scope=all`. */
	let scope = $state<'self' | 'all'>('self');
	const requestedScope = () => (page.url.searchParams.get('scope') === 'all' ? 'all' : 'self');
	const groups = $derived.by(() => {
		if (scope === 'all') {
			const sorted = [...agents].sort((a, b) => ownerLabel(a.owner).localeCompare(ownerLabel(b.owner), locale.current) || (a.display || a.name).localeCompare(b.display || b.name, locale.current));
			return [{ key: 'all', title: null, agents: sorted }];
		}
		const mine = (a: AgentSummary) => a.kind !== 'team' && a.owner.kind === 'user' && a.owner.id === me.value?.id;
		return [
			{ key: 'mine', title: t('agents-group-mine'), agents: agents.filter(mine) },
			{ key: 'team', title: t('agents-group-team'), agents: agents.filter((a) => a.kind === 'team') },
			{ key: 'shared', title: t('agents-group-shared'), agents: agents.filter((a) => a.kind !== 'team' && !mine(a)) }
		].filter((group) => group.agents.length > 0);
	});
	let loading = $state(true);
	let error = $state<string | null>(null);

	let creating = $state(false);
	let teams = $state<OrganizationListRow[]>([]);
	const kinds = $derived(creatableKinds(me.value, teams));
	let kind = $state<AgentKind>('personal');
	let team = $state('');
	let planningPersonal = $state(false);
	let display = $state('');
	let customId = $state<string | null>(null);
	let description = $state('');
	const id = $derived(customId ?? agentIdFromName(display));
	let createError = $state<string | null>(null);
	let busy = $state(false);
	let planning = $state(false);

	const when = (iso: string) => new Date(iso).toLocaleDateString(locale.current);

	async function reload() {
		try {
			const list = await agentsApi.list(requestedScope());
			agents = list.agents;
			canViewAll = list.can_view_all;
			scope = list.scope;
		} catch (err) {
			error = (err as AgentError).message;
		} finally {
			loading = false;
		}
	}

	onMount(reload);

	let teamsAsked = false;
	$effect(() => {
		if (teamsAsked || !me.value?.can_manage_agents) return;
		teamsAsked = true;
		organizationsApi.list().then((result) => (teams = result.organizations.filter((row) => row.can_create_dot)), () => (teams = []));
	});

	function openCreate() {
		kind = kinds[0] ?? 'public';
		team = teams[0]?.id ?? '';
		createError = null;
		creating = true;
	}

	async function changeScope(next: 'self' | 'all') {
		if (next === scope) return;
		loading = true;
		await goto(next === 'all' ? '/agents?scope=all' : '/agents', { replace: true, reset: false });
		await reload();
	}

	function kindLabel(agent: AgentSummary): string {
		if (agent.kind === 'team' && agent.owner.kind === 'organization') return t('agents-kind-team', { name: agent.owner.name });
		return t(agent.kind === 'personal' ? 'agents-kind-personal' : 'agents-kind-public');
	}

	function planInstead() {
		creating = false;
		planning = true;
	}

	async function create() {
		busy = true;
		createError = null;
		try {
			if (kind === 'personal') {
				const created = await dotsApi.create({ name: display.trim(), description: description.trim() });
				await goto(`/agents/${encodeURIComponent(created.dot.id)}`);
			} else if (kind === 'team') {
				const created = await organizationsApi.createDot(team, { name: display.trim(), description: description.trim() });
				await goto(`/agents/${encodeURIComponent(created.agent.id)}/setup/start`);
			} else {
				const agent = await agentsApi.create({ name: id, display: display.trim(), description });
				await goto(`/agents/${agent.id}/setup/start`);
			}
		} catch (err) {
			createError = (err as AgentError).message;
		} finally {
			busy = false;
		}
	}
</script>

<div class="w-full space-y-6">
	<header class="flex flex-wrap items-start justify-between gap-3">
		<div>
			<h1 class="text-2xl font-bold">{t('agents-heading')}</h1>
			<p class="mt-2 max-w-3xl text-sm text-base-content/60">{t(scope === 'all' ? 'agents-intro-all' : 'agents-intro')}</p>
		</div>
		<div class="flex flex-wrap items-center gap-2">
			{#if canViewAll}
				<div class="join">
					<button class="btn join-item btn-sm" class:btn-active={scope === 'self'} class:btn-primary={scope === 'self'} onclick={() => void changeScope('self')}>{t('usage-toggle-mine')}</button>
					<button class="btn join-item btn-sm" class:btn-active={scope === 'all'} class:btn-primary={scope === 'all'} onclick={() => void changeScope('all')}>{t('usage-toggle-all')}</button>
				</div>
			{/if}
			<button class="btn btn-sm" type="button" onclick={() => (planning = true)}>🎙 {t('architect-open')}</button>
			{#if kinds.length}<button class="btn btn-primary btn-sm" type="button" onclick={openCreate}>{t('agents-create')}</button>{/if}
		</div>
	</header>

	{#if error}<div class="alert alert-error"><span>{error}</span></div>{/if}

	{#if loading}
		<div class="skeleton h-28 w-full"></div>
	{:else if !agents.length && !error}
		<div class="card card-border"><div class="card-body text-sm text-base-content/60">{t('agents-list-empty')}</div></div>
	{:else}
		{#each groups as group (group.key)}
			<section class="space-y-3">
				{#if group.title}<h2 class="text-sm font-semibold uppercase tracking-wide text-base-content/60">{group.title}</h2>{/if}
				<div class="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
					{#each group.agents as agent (agent.id)}
						{@render card(agent)}
					{/each}
				</div>
			</section>
		{/each}
	{/if}
</div>

{#snippet card(agent: AgentSummary)}
	<a class="card card-border transition-colors hover:border-primary" href="/agents/{agent.id}">
		<div class="card-body gap-2 p-4">
			<div class="flex items-start justify-between gap-2">
				<h3 class="card-title text-base">{agent.display || agent.name}</h3>
				<span class="badge badge-outline badge-sm shrink-0">{kindLabel(agent)}</span>
			</div>
			<div class="flex flex-wrap items-center gap-2">
				{#if agent.dot}
					<StatusPill size="sm" tone={agent.dot.active ? 'ok' : 'warn'}>{t(agent.dot.active ? 'dots-status-active' : 'dots-status-inactive')}</StatusPill>
				{/if}
				{#if agent.kind !== 'personal'}
					{#if agent.live_version === null}
						<StatusPill size="sm">{t('agents-never-published')}</StatusPill>
					{:else}
						<StatusPill tone="ok" size="sm">{t('agents-live-badge', { version: agent.live_version })}</StatusPill>
					{/if}
				{/if}
			</div>
			{#if agent.description}<p class="line-clamp-2 text-sm text-base-content/70">{agent.description}</p>{/if}
			{#if scope === 'all' || (agent.kind !== 'team' && agent.owner.kind === 'user' && agent.owner.id !== me.value?.id)}
				<p class="text-xs text-base-content/60">{t('agents-owner', { name: ownerLabel(agent.owner) })}</p>
			{/if}
			{#if agent.shared_by}
				<p class="text-xs text-base-content/60">{t('agents-shared-by', { name: ownerLabel(agent.shared_by.by), date: when(agent.shared_by.at) })}</p>
			{/if}
			<div class="mt-1 flex items-center gap-2 text-xs text-base-content/50">
				{#if agent.access === 'read'}<span class="badge badge-outline badge-xs">{t('agents-read-only')}</span>{/if}
				<span>{t('agents-updated', { date: when(agent.updated_at) })}</span>
				{#if agent.disabled_at}<span class="badge badge-error badge-xs">{t('agents-disabled')}</span>{/if}
			</div>
		</div>
	</a>
{/snippet}

{#snippet createActions()}
	<button class="btn btn-ghost" type="button" onclick={() => (creating = false)}>{t('admin-cancel')}</button>
	<button class="btn btn-primary" type="submit" form="agent-create" disabled={busy || !display.trim() || (kind === 'public' && !id) || (kind === 'team' && !team)}>{t('agents-create-submit')}</button>
{/snippet}

<Modal bind:open={creating} title={t('agents-create')} footer={createActions}>
	<form id="agent-create" class="flex flex-col gap-3" onsubmit={(e) => { e.preventDefault(); void create(); }}>
		{#if createError}<div class="alert alert-error text-sm" role="alert"><span>{createError}</span></div>{/if}
		{#if kinds.length > 1}
			<fieldset class="flex flex-col gap-2">
				<legend class="label-text mb-1">{t('agents-create-for-whom')}</legend>
				{#each kinds as option (option)}
					<label class="flex cursor-pointer items-start gap-3 rounded-box border border-base-300 p-3" class:border-primary={kind === option}>
						<input id={`agent-kind-${option}`} class="radio radio-sm mt-1" type="radio" name="agent-kind" value={option} bind:group={kind} />
						<span class="flex flex-col gap-1">
							<span class="font-medium">{t(`agents-create-kind-${option}`)}</span>
							<span class="text-xs text-base-content/60">{t(`agents-create-kind-${option}-help`)}</span>
						</span>
					</label>
				{/each}
			</fieldset>
		{/if}
		{#if kind === 'team'}
			<label class="flex flex-col gap-1">
				<span class="label-text">{t('agents-copy-team')}</span>
				<select id="agent-create-team" class="select w-full" bind:value={team}>
					{#each teams as row (row.id)}<option value={row.id}>{row.name}</option>{/each}
				</select>
			</label>
		{/if}
		<label class="flex flex-col gap-1">
			<span class="label-text">{t('agents-create-name')}</span>
			<!-- svelte-ignore a11y_autofocus -->
			<input class="input w-full" bind:value={display} required maxlength="80" placeholder={t('agents-create-name-placeholder')} autofocus />
			{#if kind === 'public'}<span class="text-xs text-base-content/60">{t('agents-create-name-help')}</span>{/if}
		</label>
		{#if kind === 'public' && customId !== null}
			<label class="flex flex-col gap-1">
				<span class="label-text">{t('agents-create-id')}</span>
				<input class="input input-sm w-full font-mono" bind:value={customId} required maxlength="48" pattern="[a-z0-9]+(-[a-z0-9]+)*" />
				<span class="text-xs text-base-content/60">{t('agents-create-name-hint')}</span>
			</label>
		{:else if kind === 'public' && display.trim()}
			<p class="flex flex-wrap items-center gap-2 text-xs text-base-content/60">
				<span>{t('agents-create-id')}</span><code class="font-mono">{id || '…'}</code>
				<button class="btn btn-ghost btn-xs" type="button" onclick={() => (customId = id)}>{t('agents-create-id-change')}</button>
			</p>
		{/if}
		<label class="flex flex-col gap-1">
			<span class="label-text">{t('agents-create-description')}</span>
			<textarea class="textarea w-full" bind:value={description}></textarea>
			<span class="text-xs text-base-content/60">{t(kind === 'public' ? 'agents-create-description-help' : 'dots-create-description-help')}</span>
		</label>
		{#if kind === 'personal'}
			<button class="btn btn-ghost btn-sm self-start" type="button" onclick={() => { creating = false; planningPersonal = true; }}>🎙 {t('dot-setup-title')}</button>
		{:else if kind === 'public'}
			<button class="btn btn-ghost btn-sm self-start" type="button" onclick={planInstead}>🎙 {t('architect-create-option')}</button>
		{/if}
	</form>
</Modal>

<Modal bind:open={planningPersonal} title={t('dot-setup-title')} size="lg">
	<ArchitectChat dotOwner="personal" oncreated={(created) => { planningPersonal = false; void goto(`/agents/${encodeURIComponent(created)}`); }} />
</Modal>

<ArchitectModal bind:open={planning} onchanged={() => void reload()} />
