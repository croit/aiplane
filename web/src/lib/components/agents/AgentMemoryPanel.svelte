<script lang="ts">
	import EditModal from '#lib/components/EditModal.svelte';
	import { agentMemoriesApi, agentMemoryAuthorityRevoked, agentMemorySections, type AgentMemory, type AgentMemoryList, type AgentMemoryScope } from '#lib/owned-agents.js';
	import { ApiError } from '#lib/api.js';
	import { MEMORY_KINDS, type MemoryKind } from '#lib/memory.js';
	import { t } from '#lib/i18n.svelte.js';
	import type { OwnedAgent } from '#lib/agent-state.js';

	let { agent, tabbed = false, active = false }: { agent: OwnedAgent; tabbed?: boolean; active?: boolean } = $props();
	let list = $state<AgentMemoryList | null>(null);
	let drafts = $state<Record<string, string>>({});
	let loading = $state(false);
	let loaded = $state(false);
	let authorityValid = $state(false);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let adding = $state(false);
	let kind = $state<MemoryKind>('preference');
	let scope = $state<AgentMemoryScope | null>(null);
	let content = $state('');
	let formError = $state<string | null>(null);
	let saving = $state(false);
	let sections = $derived(agentMemorySections(list));
	const writable = (selected: AgentMemoryScope | null) => loaded && !loading && authorityValid && sections.some((section) => section.scope === selected && section.writable);
	const message = (caught: unknown, key: string) => caught instanceof ApiError ? caught.serverMessage ?? caught.message : caught instanceof Error ? caught.message : t(key);
	const withinLimit = (value: string) => new TextEncoder().encode(value).length <= 2000;
	function clearIfAuthorityRevoked(caught: unknown) {
		if (!(caught instanceof ApiError) || !agentMemoryAuthorityRevoked(caught.status)) return;
		list = null;
		drafts = {};
		loaded = false;
		adding = false;
		content = '';
	}

	async function refresh() {
		loading = true;
		try {
			list = await agentMemoriesApi.list(agent.id);
			drafts = Object.fromEntries(list.memories.map((memory) => [memory.id, memory.content]));
			loaded = true;
			authorityValid = true;
			error = null;
		} catch (caught) {
			authorityValid = false;
			clearIfAuthorityRevoked(caught);
			error = message(caught, 'agent-memory-error-load');
		} finally {
			loading = false;
		}
	}

	function onToggle(event: Event) {
		const details = event.currentTarget;
		if (details instanceof HTMLDetailsElement && details.open && !loaded && !loading) void refresh();
	}

	$effect(() => {
		if (tabbed && active && !loaded && !loading && !error) void refresh();
	});

	function openAdd(memoryKind: MemoryKind, selected: AgentMemoryScope) {
		if (!writable(selected)) return;
		scope = selected;
		kind = memoryKind;
		content = '';
		formError = null;
		error = null;
		adding = true;
	}

	async function add() {
		if (!content.trim() || !scope || !writable(scope)) return;
		if (!withinLimit(content.trim())) { formError = t('agent-memory-error-byte-limit'); return; }
		saving = true;
		formError = null;
		try {
			await agentMemoriesApi.create(agent.id, { kind, content: content.trim(), scope });
			adding = false;
			content = '';
			notice = t('agent-memory-notice-added');
			await refresh();
		} catch (caught) {
			authorityValid = false;
			clearIfAuthorityRevoked(caught);
			formError = message(caught, 'agent-memory-error-save');
			error = formError;
		} finally {
			saving = false;
		}
	}

	async function save(memory: AgentMemory) {
		const value = drafts[memory.id]?.trim();
		if (!value || !writable(memory.scope)) return;
		if (!withinLimit(value)) { error = t('agent-memory-error-byte-limit'); return; }
		error = null;
		try {
			await agentMemoriesApi.update(agent.id, memory.id, { kind: memory.kind, content: value, scope: memory.scope });
			notice = t('agent-memory-notice-saved');
			await refresh();
		} catch (caught) {
			authorityValid = false;
			clearIfAuthorityRevoked(caught);
			error = message(caught, 'agent-memory-error-save');
		}
	}

	async function remove(memory: AgentMemory) {
		if (!writable(memory.scope)) return;
		if (!confirm(t('memory-delete-confirm'))) return;
		error = null;
		try {
			await agentMemoriesApi.delete(agent.id, memory.id, memory.scope);
			notice = t('agent-memory-notice-deleted');
			await refresh();
		} catch (caught) {
			authorityValid = false;
			clearIfAuthorityRevoked(caught);
			error = message(caught, 'agent-memory-error-delete');
		}
	}
</script>


{#snippet panelBody()}
	<div class="space-y-3 text-sm">
		{#if loaded}<button class="btn btn-ghost btn-xs" type="button" disabled={loading} onclick={() => void refresh()}>{t('agent-memory-reload')}</button>{/if}
		{#if notice}<div class="alert alert-success py-2" role="status"><span>{notice}</span></div>{/if}
		{#if error}<div class="alert alert-error py-2" role="alert"><span>{error}</span><button class="btn btn-ghost btn-xs" type="button" onclick={() => void refresh()}>{t('agent-memory-retry')}</button></div>{/if}
		{#if loading}
			<div class="skeleton h-10 w-full"></div>
		{:else if loaded}
			{#each sections as section (section.scope)}
			<div class="space-y-3" role="region" aria-label={section.name}>
				<h3 class="font-semibold">{section.name}</h3>
				<p class="text-base-content/60">{t(section.scope === 'agent' ? 'agent-memory-own-scope' : 'agent-memory-organization-scope')}</p>
			{#each MEMORY_KINDS as memoryKind}
				<section class="space-y-1">
					<div class="flex flex-wrap items-center justify-between gap-2">
						<h3 class="font-medium">{t(`memory-kind-${memoryKind}`)}</h3>
						{#if writable(section.scope)}<button class="btn btn-ghost btn-xs" type="button" onclick={() => openAdd(memoryKind, section.scope)}>{t('memory-add-short')}</button>{/if}
					</div>
					<ul class="divide-y divide-base-300">
						{#each section.grouped[memoryKind] as memory (memory.id)}
							<li class="flex items-center gap-2 py-2">
								{#if writable(section.scope)}
								<form class="m-0 flex min-w-0 flex-1 items-center gap-2" onsubmit={(event) => { event.preventDefault(); void save(memory); }}>
									<input class="input input-sm min-w-0 flex-1" bind:value={drafts[memory.id]} maxlength="2000" required aria-label={t('memory-content-label')} />
									<button class="btn btn-outline btn-xs" type="submit" disabled={!drafts[memory.id]?.trim()}>{t('memory-save-button')}</button>
								</form>
								<button class="btn btn-ghost btn-square btn-xs" type="button" onclick={() => void remove(memory)} aria-label={t('memory-delete-title')} title={t('memory-delete-title')}>
									<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 6h18M8 6V4h8v2M19 6l-1 14H6L5 6M10 11v5M14 11v5"/></svg>
								</button>
								{:else}<p class="min-w-0 whitespace-pre-wrap break-words">{memory.content}</p>{/if}
							</li>
						{:else}
							<li class="py-2 text-sm text-base-content/50">{t('agent-memory-empty')}</li>
						{/each}
					</ul>
				</section>
			{/each}
			</div>
			{/each}
		{/if}
	</div>
{/snippet}

{#if tabbed}
	<section hidden={!active} class="min-w-0 space-y-3" data-agent-panel="memory">{@render panelBody()}</section>
{:else}
	<details class="collapse collapse-arrow border border-base-300 bg-base-100" ontoggle={onToggle}>
		<summary class="collapse-title min-h-0 py-3 text-sm font-medium">{t('agent-memory-heading')}</summary>
		<div class="collapse-content space-y-3 text-sm">{@render panelBody()}</div>
	</details>
{/if}

<EditModal
	bind:open={adding}
	title={t('memory-add-heading')}
	description={t(`memory-kind-${kind}`)}
	cancellabel={t('admin-cancel')}
	savelabel={t('memory-add-button')}
	saving={saving || !content.trim() || !writable(scope)}
	onsave={add}
>
	<div class="flex flex-col gap-3">
		{#if formError}<div class="alert alert-error text-sm" role="alert"><span>{formError}</span></div>{/if}
		<fieldset class="fieldset">
			<legend class="fieldset-legend">{t('memory-kind-aria')}</legend>
			<select class="select w-full" bind:value={kind} aria-label={t('memory-kind-aria')}>
				{#each MEMORY_KINDS as option}<option value={option}>{t(`memory-kind-${option}`)}</option>{/each}
			</select>
		</fieldset>
		<fieldset class="fieldset">
			<legend class="fieldset-legend">{t('memory-content-label')}</legend>
			<textarea class="textarea min-h-24 w-full" bind:value={content} maxlength="2000" aria-label={t('memory-content-label')} placeholder={t('memory-content-placeholder')}></textarea>
		</fieldset>
	</div>
</EditModal>
