<script lang="ts">
	import { dotCommunicationApi, type Dot, type DotCommunication, type DotCommunicationDirection, type DotCommunicationPeer } from '#lib/dots.js';
	import { ApiError } from '#lib/api.js';
	import { communicationChange, communicationDecision } from '#lib/dot-communication.js';
	import { searchCapabilities } from '#lib/capability-picker.js';
	import { me } from '#lib/session.svelte.js';
	import { t } from '#lib/i18n.svelte.js';

	let { dot }: { dot: Dot } = $props();
	let data = $state<DotCommunication | null>(null);
	let loading = $state(false);
	let saving = $state<string | null>(null);
	let query = $state('');
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let drafts = $state<Record<string, { peer: DotCommunicationPeer; enabled: boolean }>>({});
	let rowErrors = $state<Record<string, string>>({});
	const directions: DotCommunicationDirection[] = ['outbound', 'inbound'];
	const canManage = $derived(me.value?.can_manage_dots === true);
	const currentDot = $derived(data ? { ...data.dot, active: data.dot.active && dot.active } : dot);
	const peers = $derived(searchCapabilities((data?.peers ?? []).map((peer) => ({
		...peer, key: peer.dot.id, title: peer.dot.display || peer.dot.name,
		description: peer.dot.description, kind: 'dot', group: '', order: 0, icon: null
	})), query));
	const rowKey = (peer: DotCommunicationPeer, direction: DotCommunicationDirection) => `${direction}:${peer.dot.id}`;
	const enabled = (peer: DotCommunicationPeer, direction: DotCommunicationDirection) => drafts[rowKey(peer, direction)]?.enabled ?? communicationDecision(peer, direction).enabled;
	const pending = (peer: DotCommunicationPeer, direction: DotCommunicationDirection) => {
		const draft = drafts[rowKey(peer, direction)];
		return draft ? communicationChange(currentDot, draft.peer, direction, draft.enabled) : null;
	};

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

	function change(peer: DotCommunicationPeer, direction: DotCommunicationDirection, value: boolean) {
		const key = rowKey(peer, direction);
		drafts[key] = { peer: drafts[key]?.peer ?? peer, enabled: value };
		notice = null;
	}

	async function save(peer: DotCommunicationPeer, direction: DotCommunicationDirection) {
		const body = pending(peer, direction);
		if (!body || !canManage) return;
		const key = rowKey(peer, direction);
		saving = key;
		delete rowErrors[key];
		notice = null;
		try {
			data = await dotCommunicationApi.save(dot.id, body);
			delete drafts[key];
			notice = t('dot-communication-saved');
		} catch (caught) {
			rowErrors[key] = caught instanceof ApiError ? caught.serverMessage ?? caught.message : caught instanceof Error ? caught.message : t('dot-communication-error-save');
		} finally {
			saving = null;
		}
	}
</script>

<details class="collapse collapse-arrow border border-base-300 bg-base-100" ontoggle={onToggle}>
	<summary class="collapse-title min-h-0 py-3 text-sm font-medium">{t('dot-communication-heading')}</summary>
	<div class="collapse-content min-w-0 space-y-3 text-sm">
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
									<input type="checkbox" class="checkbox checkbox-sm mt-0.5 shrink-0" checked={enabled(peer, direction)} disabled={!canManage || saving !== null || (!enabled(peer, direction) && !decision.enabled && (!currentDot.active || !peer.dot.active))} aria-label={t(`dot-communication-${direction}-aria`, { name: peer.dot.display || peer.dot.name, id: peer.dot.id })} onchange={(event) => change(peer, direction, event.currentTarget.checked)} />
									<span class="min-w-0 break-words font-medium">{peer.dot.display || peer.dot.name}<code class="mt-1 block break-all font-mono text-xs font-normal text-base-content/50">{peer.dot.id}</code></span>
								</label>
								{#if peer.dot.description}<p class="mt-2 break-words text-xs text-base-content/60">{peer.dot.description}</p>{/if}
								<div class="mt-2 flex flex-wrap gap-2">
									<span class="badge badge-outline badge-sm">{peer.dot.active ? t('dots-status-active') : t('dots-status-inactive')}</span>
									{#if decision.enabled && decision.counterpartEnabled}<span class="badge badge-outline badge-sm">{t('dot-communication-both')}</span>{/if}
								</div>
								<p class="mt-2 text-xs text-base-content/60">{t(`dot-communication-${direction}-counterpart-${decision.counterpartEnabled ? 'enabled' : 'disabled'}`)}</p>
								{#if !currentDot.active || !peer.dot.active}<p class="mt-1 text-xs text-base-content/60">{t('dot-communication-inactive')}</p>{/if}
								{#if rowErrors[key]}<div class="alert alert-error mt-2 py-2" role="alert"><span class="break-words">{rowErrors[key]}</span></div>{/if}
								<div class="mt-2 flex justify-end"><button class="btn btn-sm" type="button" disabled={!canManage || saving !== null || pending(peer, direction) === null} onclick={() => void save(peer, direction)}>{saving === key ? t('dot-access-saving') : t('memory-save-button')}</button></div>
							</li>
						{:else}
							<li class="text-base-content/60">{data.peers.length ? t('searchable-select-no-results') : t('dot-communication-empty')}</li>
						{/each}
					</ul>
				</section>
			{/each}
		{/if}
		<div class="flex justify-end"><button class="btn btn-ghost btn-sm" type="button" disabled={loading || saving !== null} onclick={() => void refresh()}>{t('dot-communication-reload')}</button></div>
	</div>
</details>
