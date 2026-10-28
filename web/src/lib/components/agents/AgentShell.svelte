<script lang="ts">
	import StatusPill from '#lib/components/ui/StatusPill.svelte';
	import { onMount, type Snippet } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { boundedViewport } from '#lib/viewport.js';
	import { agentsApi, agentPublicName, organizationOwner } from '#lib/agents.js';
	import { checklist, publishState } from '#lib/agent-setup.js';
	import { AgentWorkspace, provideWorkspace } from '#lib/agent-workspace.svelte.js';
	import { t } from '#lib/i18n.svelte.js';

	/**
	 * What every page of one agent shares: the workspace (the spec buffer and
	 * everything loaded with it), the header with the agent's state and its
	 * Save / Publish / Delete, and the save and publish answers.
	 */
	let { id, children }: { id: string; children: Snippet } = $props();
	// The layout keys the shell by the agent id, so a new id is a new shell.
	// svelte-ignore state_referenced_locally
	const ws = provideWorkspace(new AgentWorkspace(id));
	// The test chat fills the window the shell bounds: the page passes the
	// height on down a flex column instead of growing past it.
	const bounded = $derived(boundedViewport(page.url));

	onMount(() => void ws.load());
	$effect(() => ws.remember());

	const todos = $derived(checklist(ws.spec, ws.dirty ? [] : (ws.detail?.publish_issues ?? []), ws.resources?.defaults, ws.detail));
	const owner = $derived(organizationOwner(ws.detail));
	const isDot = $derived(ws.detail?.dot !== null && ws.detail?.dot !== undefined);
	const personalDot = $derived(ws.detail?.kind === 'personal');
	const returnPath = $derived(owner ? `/organizations/${encodeURIComponent(owner.id)}` : '/agents');
	const open = $derived(todos.filter((x) => x.blocking).length);
	const state = $derived(publishState(todos));

	async function publish() {
		try {
			await ws.publish();
		} catch {
			if (ws.issues.length) await goto(`/agents/${id}?tab=setup`);
		}
	}

	async function remove() {
		if (!ws.detail || !confirm(t('agents-delete-confirm', { name: agentPublicName(ws.detail) }))) return;
		try {
			await agentsApi.remove(id);
			await goto(returnPath);
		} catch (err) {
			ws.fail(err);
		}
	}
</script>

<div data-agent-page class="flex w-full flex-col gap-4 {bounded ? 'h-full min-h-0' : ''}">
	<a class="link link-hover text-sm text-base-content/60" href={returnPath}>← {owner?.name ?? t('agents-back')}</a>

	{#if ws.loadError}
		<div class="alert alert-error"><span>{ws.loadError}</span></div>
	{:else if ws.loading || !ws.detail}
		<div class="skeleton h-96 w-full"></div>
	{:else}
		{#if owner}<div class="alert alert-info"><span>{t('organization-owned-by', { name: owner.name })} {t('organization-configuration-only')}</span></div>{/if}
		<header class="flex flex-wrap items-center gap-3">
			<div class="min-w-0">
				<h1 class="m-0 truncate text-2xl font-bold">{ws.spec.profile?.display || ws.detail.display || agentPublicName(ws.detail)}</h1>
				{#if isDot}
					<span class="badge badge-outline badge-sm max-w-full whitespace-normal">{owner ? t('agents-kind-team', { name: owner.name }) : t('agents-kind-personal')}</span>
				{:else}
					<p class="m-0 font-mono text-xs text-base-content/60">{agentPublicName(ws.detail)}</p>
				{/if}
			</div>
			{#if personalDot}
				<StatusPill size="sm" tone={ws.detail.dot!.active ? 'ok' : 'warn'}>{t(ws.detail.dot!.active ? 'dots-status-active' : 'dots-status-inactive')}</StatusPill>
			{:else if ws.detail.live_version === null}
				<StatusPill>{t('agents-never-published')}</StatusPill>
			{:else}
				<StatusPill tone="ok">{t('agents-live-badge', { version: ws.detail.live_version })}</StatusPill>
			{/if}
			{#if isDot && owner}<StatusPill size="sm" tone={ws.detail.dot!.active ? 'ok' : 'warn'}>{t(ws.detail.dot!.active ? 'dots-status-active' : 'dots-status-inactive')}</StatusPill>{/if}
			{#if ws.writable}
				{#if state === 'blocked'}
					<StatusPill tone="warn">{t('agents-setup-open-count', { count: open })}</StatusPill>
				{:else if state === 'recommended'}
					<StatusPill tone="ok">{t('agents-setup-ready-recommended-pill')}</StatusPill>
				{:else}
					<StatusPill tone="ok">{t('agents-setup-ready-pill')}</StatusPill>
				{/if}
			{/if}
			{#if ws.dirty}<StatusPill tone="warn">{t('agents-unsaved')}</StatusPill>{/if}
			{#if !ws.writable}<StatusPill>{t('agents-read-only')}</StatusPill>{/if}
			{#if ws.writable}
				<div class="ml-auto flex flex-wrap gap-2">
					<button class="btn btn-sm" type="button" disabled={ws.busy || !ws.dirty} onclick={() => void ws.save()}>{t('agents-save')}</button>
					<button class="btn btn-primary btn-sm" type="button" disabled={ws.busy || open > 0} title={open ? t('agents-setup-publish-blocked') : undefined} onclick={() => void publish()}>{t('agents-publish-action')}</button>
					<button class="btn btn-ghost btn-sm text-error" type="button" onclick={() => void remove()}>{t('agents-delete')}</button>
				</div>
			{/if}
		</header>

		{#if ws.error && !ws.staleRefusal}
			<div class="alert alert-error text-sm" role="alert">
				<div>
					<p class="m-0">{ws.error}</p>
					{#if ws.shownIssues.length > 1}
						<ul class="m-0 mt-1 list-inside list-disc">
							{#each ws.shownIssues as issue (issue.path + issue.message)}
								<li><span class="font-mono">{issue.path || t('agents-issue-root')}</span>: {issue.message}</li>
							{/each}
						</ul>
					{/if}
				</div>
			</div>
		{/if}
		{#if ws.notice}
			<div class="toast toast-top toast-center z-50"><div class="alert alert-success text-sm" role="status"><span>{t(ws.notice.key, ws.notice.args)}</span></div></div>
		{/if}

		{@render children()}
	{/if}
</div>
