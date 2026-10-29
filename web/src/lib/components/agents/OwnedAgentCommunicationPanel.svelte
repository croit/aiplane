<script lang="ts">
	import { ownedAgentCommunicationApi, type OwnedAgent, type OwnedAgentCommunication, type OwnedAgentCommunicationDirection, type OwnedAgentCommunicationPeer } from '#lib/owned-agents.js';
	import { ApiError } from '#lib/api.js';
	import { communicationChange, communicationDecision } from '#lib/agent-communication.js';
	import { searchCapabilities } from '#lib/capability-picker.js';
	import { me } from '#lib/session.svelte.js';
	import { t } from '#lib/i18n.svelte.js';

	let { agent, tabbed = false, active = false }: { agent: OwnedAgent; tabbed?: boolean; active?: boolean } = $props();
	let data = $state<OwnedAgentCommunication | null>(null);
	let loading = $state(false);
	let saving = $state(false);
	let query = $state('');
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let drafts = $state<Record<string, boolean>>({});
	let rowErrors = $state<Record<string, string>>({});
	const directions: OwnedAgentCommunicationDirection[] = ['outbound', 'inbound'];
	const canManage = $derived(me.value?.can_manage_personal_agents === true);
	const currentAgent = $derived(data ? { ...data.agent, active: data.agent.active && agent.active } : agent);
	const peers = $derived(searchCapabilities((data?.peers ?? []).map((peer) => ({
		...peer, key: peer.agent.id, title: peer.agent.display || peer.agent.name,
		description: peer.agent.description, kind: 'agent', group: '', order: 0, icon: null
	})), query));
	const rowKey = (peer: OwnedAgentCommunicationPeer, direction: OwnedAgentCommunicationDirection) => `${direction}:${peer.agent.id}`;
	/** The organization's side of a cross-organization edge: read-only here. */
	const organizationControl = (peer: OwnedAgentCommunicationPeer, direction: OwnedAgentCommunicationDirection) => peer.cross_organization && direction === 'inbound';
	const enabled = (peer: OwnedAgentCommunicationPeer, direction: OwnedAgentCommunicationDirection) => drafts[rowKey(peer, direction)] ?? communicationDecision(peer, direction).enabled;
	const pendingBody = (peer: OwnedAgentCommunicationPeer, direction: OwnedAgentCommunicationDirection) => {
		const draft = drafts[rowKey(peer, direction)];
		return draft === undefined ? null : communicationChange(currentAgent, peer, direction, draft);
	};
	const pendingChanges = $derived(
		(data?.peers ?? []).flatMap((peer) =>
			directions
				.filter((direction) => !organizationControl(peer, direction))
				.map((direction) => ({ peer, direction, key: rowKey(peer, direction) }))
				.filter(({ key }) => drafts[key] !== undefined)
		)
	);

	async function refresh() {
		loading = true;
		error = null;
		notice = null;
		try {
			data = await ownedAgentCommunicationApi.load(agent.id);
			drafts = {};
			rowErrors = {};
		} catch (caught) {
			error = caught instanceof ApiError ? caught.serverMessage ?? caught.message : caught instanceof Error ? caught.message : t('agent-communication-error-load');
		} finally {
			loading = false;
		}
	}

	function onToggle(event: Event) {
		const details = event.currentTarget;
		if (details instanceof HTMLDetailsElement && details.open && !data && !loading) void refresh();
	}

	$effect(() => {
		if (tabbed && active && !data && !loading && !error) void refresh();
	});

	function change(peer: OwnedAgentCommunicationPeer, direction: OwnedAgentCommunicationDirection, value: boolean) {
		const key = rowKey(peer, direction);
		if (value === communicationDecision(peer, direction).enabled) delete drafts[key];
		else drafts[key] = value;
		notice = null;
	}

	async function saveAll() {
		if (!canManage || pendingChanges.length === 0) return;
		saving = true;
		notice = null;
		rowErrors = {};
		let failed = false;
		for (const { key, peer, direction } of pendingChanges) {
			const current = data?.peers.find((item) => item.agent.id === peer.agent.id);
			const body = current ? pendingBody(current, direction) : null;
			if (!body) { delete drafts[key]; continue; }
			try {
				data = await ownedAgentCommunicationApi.save(agent.id, body);
				delete drafts[key];
			} catch (caught) {
				rowErrors[key] = caught instanceof ApiError ? caught.serverMessage ?? caught.message : caught instanceof Error ? caught.message : t('agent-communication-error-save');
				failed = true;
				break;
			}
		}
		if (!failed) notice = t('personal-agent-communication-saved');
		saving = false;
	}
</script>

{#snippet panelBody()}
	<div class="min-w-0 space-y-3 text-sm">
		<p class="text-base-content/60">{t('personal-agent-communication-scope')}</p>
		<p class="text-xs text-base-content/60">{t('agent-communication-tool-policy')}</p>
		{#if notice}<div class="alert alert-success py-2" role="status"><span>{notice}</span></div>{/if}
		{#if error}<div class="alert alert-error py-2" role="alert"><span class="break-words">{error}</span></div>{/if}
		{#if loading}
			<div class="skeleton h-16 w-full"></div>
		{:else if data}
			{#if !canManage}<p class="text-xs text-base-content/60">{t('agent-error-permission-required')}</p>{/if}
			<label class="input input-sm w-full"><input bind:value={query} aria-label={t('agent-communication-search')} placeholder={t('agent-communication-search')} /></label>
			{#each directions as direction}
				<section class="space-y-2" aria-label={t(`agent-communication-${direction}`)}>
					<h3 class="font-medium">{t(`agent-communication-${direction}`)}</h3>
					<ul class="max-h-80 space-y-2 overflow-y-auto">
						{#each peers as peer (peer.agent.id)}
							{@const decision = communicationDecision(peer, direction)}
							{@const key = rowKey(peer, direction)}
							<li class="rounded-box border border-base-300 p-3">
								<label class="flex items-start gap-2">
									<input type="checkbox" class="checkbox checkbox-sm mt-0.5 shrink-0" checked={enabled(peer, direction)} disabled={!canManage || saving || organizationControl(peer, direction) || (!enabled(peer, direction) && !decision.enabled && (!currentAgent.active || !peer.agent.active))} aria-label={t(`agent-communication-${direction}-aria`, { name: peer.agent.display || peer.agent.name, id: peer.agent.id })} onchange={(event) => change(peer, direction, event.currentTarget.checked)} />
									<span class="min-w-0 break-words font-medium" title={peer.agent.id}>{peer.agent.display || peer.agent.name}</span>
								</label>
								{#if peer.agent.description}<p class="mt-2 break-words text-xs text-base-content/60">{peer.agent.description}</p>{/if}
								<div class="mt-2 flex flex-wrap gap-2">
									<span class="badge badge-outline badge-sm">{peer.agent.active ? t('agents-status-active') : t('agents-status-inactive')}</span>
									{#if peer.cross_organization}<span class="badge badge-outline badge-sm">{t('agent-communication-cross-organization')}</span>{/if}
									{#if decision.enabled && decision.counterpartEnabled}<span class="badge badge-outline badge-sm">{t('agent-communication-both')}</span>{/if}
								</div>
								{#if organizationControl(peer, direction)}<p class="mt-2 text-xs text-base-content/60">{t('agent-communication-inbound-organization')}</p>
								{:else}<p class="mt-2 text-xs text-base-content/60">{t(`agent-communication-${direction}-counterpart-${decision.counterpartEnabled ? 'enabled' : 'disabled'}`)}{#if !decision.counterpartEnabled && !peer.cross_organization}<a class="link ml-1" href={`/agents/${encodeURIComponent(peer.agent.id)}?tab=communication`}>{t('agent-communication-open-peer', { name: peer.agent.display || peer.agent.name })}</a>{/if}</p>{/if}
								{#if !currentAgent.active || !peer.agent.active}<p class="mt-1 text-xs text-base-content/60">{t('agent-communication-inactive')}</p>{/if}
								{#if rowErrors[key]}<div class="alert alert-error mt-2 py-2" role="alert"><span class="break-words">{rowErrors[key]}</span></div>{/if}
							</li>
						{:else}
							<li class="text-base-content/60">{data.peers.length ? t('searchable-select-no-results') : t('personal-agent-communication-empty')}</li>
						{/each}
					</ul>
				</section>
			{/each}
		{/if}
		<div class="flex flex-wrap justify-end gap-2">
			{#if canManage && pendingChanges.length}<button class="btn btn-sm" type="button" disabled={saving || loading} onclick={() => void saveAll()}>{saving ? t('agent-access-saving') : t('agent-communication-save-changes')}</button>{/if}
			<button class="btn btn-ghost btn-sm" type="button" disabled={loading || saving} onclick={() => void refresh()}>{t('agent-communication-reload')}</button>
		</div>
	</div>
{/snippet}

{#if tabbed}
	<section hidden={!active} class="min-w-0" data-agent-panel="communication">{@render panelBody()}</section>
{:else}
	<details class="collapse collapse-arrow border border-base-300 bg-base-100" data-panel="communication" ontoggle={onToggle}>
		<summary class="collapse-title min-h-0 py-3 text-sm font-medium">{t('personal-agent-communication-heading')}</summary>
		<div class="collapse-content">{@render panelBody()}</div>
	</details>
{/if}
