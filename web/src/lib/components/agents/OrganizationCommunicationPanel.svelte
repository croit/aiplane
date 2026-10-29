<script lang="ts">
	import { onMount } from 'svelte';
	import { agentCommunicationApi, type OwnedAgent, type AgentCommunication } from '#lib/owned-agents.js';
	import { ApiError } from '#lib/api.js';
	import { t } from '#lib/i18n.svelte.js';

	let { agent, writable }: { agent: OwnedAgent; writable: boolean } = $props();
	let data = $state<AgentCommunication | null>(null);
	let loading = $state(false);
	let saving = $state(false);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let rowErrors = $state<Record<string, string>>({});
	let drafts = $state<Record<string, boolean>>({});

	const enabled = (peerId: string, current: boolean) => drafts[peerId] ?? current;
	const changed = (peerId: string, current: boolean) => drafts[peerId] !== undefined && drafts[peerId] !== current;
	const pendingCallers = $derived(data ? data.callers.filter((caller) => changed(caller.agent.id, caller.incoming.inbound_enabled)) : []);

	async function reload() {
		loading = true;
		error = null;
		notice = null;
		try {
			data = await agentCommunicationApi.load(agent.id);
			drafts = {};
			rowErrors = {};
		} catch (caught) {
			error = caught instanceof ApiError ? caught.serverMessage ?? caught.message : caught instanceof Error ? caught.message : t('agent-communication-error-load');
		} finally {
			loading = false;
		}
	}

	async function saveAll() {
		if (!writable || pendingCallers.length === 0) return;
		saving = true;
		notice = null;
		rowErrors = {};
		let failed = false;
		for (const caller of pendingCallers) {
			const draft = drafts[caller.agent.id];
			if (draft === undefined) continue;
			const current = data?.callers.find((item) => item.agent.id === caller.agent.id);
			try {
				data = await agentCommunicationApi.save(agent.id, {
					peer_id: caller.agent.id,
					enabled: draft,
					expected_generation: (current ?? caller).incoming.generation
				});
				delete drafts[caller.agent.id];
			} catch (caught) {
				rowErrors[caller.agent.id] = caught instanceof ApiError ? caught.serverMessage ?? caught.message : caught instanceof Error ? caught.message : t('agent-communication-error-save');
				failed = true;
				break;
			}
		}
		if (!failed) notice = t('agent-communication-saved');
		saving = false;
	}

	onMount(() => void reload());
</script>

<section class="space-y-3" aria-label={t('agent-communication-heading')}>
	<h3 class="font-medium">{t('agent-communication-heading')}</h3>
	<p class="text-xs text-base-content/60">{t('agent-communication-scope')}</p>
	{#if notice}<div class="alert alert-success py-2" role="status"><span>{notice}</span></div>{/if}
	{#if error}<div class="alert alert-error py-2" role="alert"><span class="break-words">{error}</span></div>{/if}
	{#if loading}<div class="skeleton h-16 w-full"></div>
	{:else if data}
		{#if data.callers.length}
			<ul class="max-h-80 space-y-2 overflow-y-auto">
				{#each data.callers as caller (caller.agent.id)}
					<li class="rounded-box border border-base-300 p-3">
						<label class="flex items-start gap-2">
							<input type="checkbox" class="checkbox checkbox-sm mt-0.5 shrink-0" checked={enabled(caller.agent.id, caller.incoming.inbound_enabled)} disabled={!writable || saving} aria-label={t('agent-communication-inbound-aria', { name: caller.agent.name, id: caller.agent.id })} onchange={(event) => (drafts[caller.agent.id] = event.currentTarget.checked)} />
							<span class="min-w-0 break-words font-medium" title={caller.agent.id}>{caller.agent.name}</span>
						</label>
						{#if caller.owner.name}<p class="mt-2 text-xs text-base-content/60">{caller.owner.name}</p>{/if}
						{#if rowErrors[caller.agent.id]}<div class="alert alert-error mt-2 py-2" role="alert"><span class="break-words">{rowErrors[caller.agent.id]}</span></div>{/if}
					</li>
				{/each}
			</ul>
		{:else}
			<p class="text-sm text-base-content/60">{t('agent-communication-empty')}</p>
		{/if}
	{/if}
	<div class="flex flex-wrap justify-end gap-2">
		{#if writable && pendingCallers.length}<button class="btn btn-sm" type="button" disabled={saving || loading} onclick={() => void saveAll()}>{saving ? t('agent-access-saving') : t('agent-communication-save-changes')}</button>{/if}
		<button class="btn btn-ghost btn-sm" type="button" disabled={loading || saving} onclick={() => void reload()}>{t('agent-communication-reload')}</button>
	</div>
</section>
