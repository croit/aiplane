<script lang="ts">
	import { dotCommunicationApi, type Dot, type DotCommunication, type DotCommunicationDirection, type DotCommunicationPeer } from '#lib/dots.js';
	import { ApiError } from '#lib/api.js';
	import { communicationChange, communicationDecision } from '#lib/dot-communication.js';
	import { searchCapabilities } from '#lib/capability-picker.js';
	import { me } from '#lib/session.svelte.js';
	import { t } from '#lib/i18n.svelte.js';

	let { dot, tabbed = false, active = false }: { dot: Dot; tabbed?: boolean; active?: boolean } = $props();
	let data = $state<DotCommunication | null>(null);
	let loading = $state(false);
	let saving = $state(false);
	let query = $state('');
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let drafts = $state<Record<string, boolean>>({});
	let rowErrors = $state<Record<string, string>>({});
	const directions: DotCommunicationDirection[] = ['outbound', 'inbound'];
	const canManage = $derived(me.value?.can_manage_dots === true);
	const currentDot = $derived(data ? { ...data.dot, active: data.dot.active && dot.active } : dot);
	const peers = $derived(searchCapabilities((data?.peers ?? []).map((peer) => ({
		...peer, key: peer.dot.id, title: peer.dot.display || peer.dot.name,
		description: peer.dot.description, kind: 'dot', group: '', order: 0, icon: null
	})), query));
	const rowKey = (peer: DotCommunicationPeer, direction: DotCommunicationDirection) => `${direction}:${peer.dot.id}`;
	/** The organization's side of a cross-organization edge: read-only here. */
	const organizationControl = (peer: DotCommunicationPeer, direction: DotCommunicationDirection) => peer.cross_organization && direction === 'inbound';
	const enabled = (peer: DotCommunicationPeer, direction: DotCommunicationDirection) => drafts[rowKey(peer, direction)] ?? communicationDecision(peer, direction).enabled;
	const pendingBody = (peer: DotCommunicationPeer, direction: DotCommunicationDirection) => {
		const draft = drafts[rowKey(peer, direction)];
		return draft === undefined ? null : communicationChange(currentDot, peer, direction, draft);
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
			data = await dotCommunicationApi.load(dot.id);
			drafts = {};
			rowErrors = {};
		} catch (caught) {
			error = caught instanceof ApiError ? caught.serverMessage ?? caught.message : caught instanceof Error ? caught.message : t('dot-communication-error-load');
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

	function change(peer: DotCommunicationPeer, direction: DotCommunicationDirection, value: boolean) {
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
			const current = data?.peers.find((item) => item.dot.id === peer.dot.id);
			const body = current ? pendingBody(current, direction) : null;
			if (!body) { delete drafts[key]; continue; }
			try {
				data = await dotCommunicationApi.save(dot.id, body);
				delete drafts[key];
			} catch (caught) {
				rowErrors[key] = caught instanceof ApiError ? caught.serverMessage ?? caught.message : caught instanceof Error ? caught.message : t('dot-communication-error-save');
				failed = true;
				break;
			}
		}
		if (!failed) notice = t('dot-communication-saved');
		saving = false;
	}
</script>

{#snippet panelBody()}
	<div class="min-w-0 space-y-3 text-sm">
		<p class="text-base-content/60">{t('dot-communication-scope')}</p>
		<p class="text-xs text-base-content/60">{t('dot-communication-tool-policy')}</p>
		{#if notice}<div class="alert alert-success py-2" role="status"><span>{notice}</span></div>{/if}
		{#if error}<div class="alert alert-error py-2" role="alert"><span class="break-words">{error}</span></div>{/if}
		{#if loading}
			<div class="skeleton h-16 w-full"></div>
		{:else if data}
			{#if !canManage}<p class="text-xs text-base-content/60">{t('dot-error-permission-required')}</p>{/if}
			<label class="input input-sm w-full"><input bind:value={query} aria-label={t('dot-communication-search')} placeholder={t('dot-communication-search')} /></label>
			{#each directions as direction}
				<section class="space-y-2" aria-label={t(`dot-communication-${direction}`)}>
					<h3 class="font-medium">{t(`dot-communication-${direction}`)}</h3>
					<ul class="max-h-80 space-y-2 overflow-y-auto">
						{#each peers as peer (peer.dot.id)}
							{@const decision = communicationDecision(peer, direction)}
							{@const key = rowKey(peer, direction)}
							<li class="rounded-box border border-base-300 p-3">
								<label class="flex items-start gap-2">
									<input type="checkbox" class="checkbox checkbox-sm mt-0.5 shrink-0" checked={enabled(peer, direction)} disabled={!canManage || saving || organizationControl(peer, direction) || (!enabled(peer, direction) && !decision.enabled && (!currentDot.active || !peer.dot.active))} aria-label={t(`dot-communication-${direction}-aria`, { name: peer.dot.display || peer.dot.name, id: peer.dot.id })} onchange={(event) => change(peer, direction, event.currentTarget.checked)} />
									<span class="min-w-0 break-words font-medium" title={peer.dot.id}>{peer.dot.display || peer.dot.name}</span>
								</label>
								{#if peer.dot.description}<p class="mt-2 break-words text-xs text-base-content/60">{peer.dot.description}</p>{/if}
								<div class="mt-2 flex flex-wrap gap-2">
									<span class="badge badge-outline badge-sm">{peer.dot.active ? t('dots-status-active') : t('dots-status-inactive')}</span>
									{#if peer.cross_organization}<span class="badge badge-outline badge-sm">{t('dot-communication-cross-organization')}</span>{/if}
									{#if decision.enabled && decision.counterpartEnabled}<span class="badge badge-outline badge-sm">{t('dot-communication-both')}</span>{/if}
								</div>
								{#if organizationControl(peer, direction)}<p class="mt-2 text-xs text-base-content/60">{t('dot-communication-inbound-organization')}</p>
								{:else}<p class="mt-2 text-xs text-base-content/60">{t(`dot-communication-${direction}-counterpart-${decision.counterpartEnabled ? 'enabled' : 'disabled'}`)}{#if !decision.counterpartEnabled && !peer.cross_organization}<a class="link ml-1" href={`/agents/${encodeURIComponent(peer.dot.id)}?tab=communication`}>{t('dot-communication-open-peer', { name: peer.dot.display || peer.dot.name })}</a>{/if}</p>{/if}
								{#if !currentDot.active || !peer.dot.active}<p class="mt-1 text-xs text-base-content/60">{t('dot-communication-inactive')}</p>{/if}
								{#if rowErrors[key]}<div class="alert alert-error mt-2 py-2" role="alert"><span class="break-words">{rowErrors[key]}</span></div>{/if}
							</li>
						{:else}
							<li class="text-base-content/60">{data.peers.length ? t('searchable-select-no-results') : t('dot-communication-empty')}</li>
						{/each}
					</ul>
				</section>
			{/each}
		{/if}
		<div class="flex flex-wrap justify-end gap-2">
			{#if canManage && pendingChanges.length}<button class="btn btn-sm" type="button" disabled={saving || loading} onclick={() => void saveAll()}>{saving ? t('dot-access-saving') : t('dot-communication-save-changes')}</button>{/if}
			<button class="btn btn-ghost btn-sm" type="button" disabled={loading || saving} onclick={() => void refresh()}>{t('dot-communication-reload')}</button>
		</div>
	</div>
{/snippet}

{#if tabbed}
	<section hidden={!active} class="min-w-0" data-dot-panel="communication">{@render panelBody()}</section>
{:else}
	<details class="collapse collapse-arrow border border-base-300 bg-base-100" data-panel="communication" ontoggle={onToggle}>
		<summary class="collapse-title min-h-0 py-3 text-sm font-medium">{t('dot-communication-heading')}</summary>
		<div class="collapse-content">{@render panelBody()}</div>
	</details>
{/if}
