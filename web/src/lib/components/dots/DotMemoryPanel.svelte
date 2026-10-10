<script lang="ts">
	import EditModal from '#lib/components/EditModal.svelte';
	import { dotMemoriesApi } from '#lib/dots.js';
	import { groupMemories, MEMORY_KINDS, type Memory, type MemoryKind } from '#lib/memory.js';
	import { t } from '#lib/i18n.svelte.js';
	import type { Dot } from '#lib/dot-state.js';

	let { dot }: { dot: Dot } = $props();
	let memories = $state<Memory[]>([]);
	let drafts = $state<Record<string, string>>({});
	let loading = $state(false);
	let loaded = $state(false);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let adding = $state(false);
	let kind = $state<MemoryKind>('preference');
	let content = $state('');
	let formError = $state<string | null>(null);
	let saving = $state(false);
	let grouped = $derived(groupMemories(memories));

	async function refresh() {
		loading = true;
		try {
			memories = await dotMemoriesApi.list(dot.id);
			drafts = Object.fromEntries(memories.map((memory) => [memory.id, memory.content]));
			loaded = true;
			error = null;
		} catch (caught) {
			error = caught instanceof Error ? caught.message : t('dot-memory-error-load');
		} finally {
			loading = false;
		}
	}

	function onToggle(event: Event) {
		const details = event.currentTarget;
		if (details instanceof HTMLDetailsElement && details.open && !loaded && !loading) void refresh();
	}

	function openAdd(memoryKind: MemoryKind) {
		kind = memoryKind;
		content = '';
		formError = null;
		error = null;
		adding = true;
	}

	async function add() {
		if (!content.trim()) return;
		saving = true;
		formError = null;
		try {
			await dotMemoriesApi.create(dot.id, { kind, content: content.trim() });
			adding = false;
			content = '';
			notice = t('dot-memory-notice-added');
			await refresh();
		} catch (caught) {
			formError = caught instanceof Error ? caught.message : t('dot-memory-error-save');
		} finally {
			saving = false;
		}
	}

	async function save(memory: Memory) {
		const value = drafts[memory.id]?.trim();
		if (!value) return;
		error = null;
		try {
			await dotMemoriesApi.update(dot.id, memory.id, { kind: memory.kind, content: value });
			notice = t('dot-memory-notice-saved');
			await refresh();
		} catch (caught) {
			error = caught instanceof Error ? caught.message : t('dot-memory-error-save');
		}
	}

	async function remove(memory: Memory) {
		if (!confirm(t('memory-delete-confirm'))) return;
		error = null;
		try {
			await dotMemoriesApi.delete(dot.id, memory.id);
			notice = t('dot-memory-notice-deleted');
			await refresh();
		} catch (caught) {
			error = caught instanceof Error ? caught.message : t('dot-memory-error-delete');
		}
	}
</script>

<details class="collapse collapse-arrow border border-base-300 bg-base-100" ontoggle={onToggle}>
	<summary class="collapse-title min-h-0 py-3 text-sm font-medium">{t('dot-memory-heading')}</summary>
	<div class="collapse-content space-y-3 text-sm">
		<p class="text-base-content/60">{t('dot-memory-scope')}</p>
		{#if notice}<div class="alert alert-success py-2" role="status"><span>{notice}</span></div>{/if}
		{#if error}<div class="alert alert-error py-2" role="alert"><span>{error}</span><button class="btn btn-ghost btn-xs" type="button" onclick={() => void refresh()}>{t('dot-memory-retry')}</button></div>{/if}
		{#if loading}
			<div class="skeleton h-10 w-full"></div>
		{:else if loaded}
			{#each MEMORY_KINDS as memoryKind}
				<section class="space-y-1">
					<div class="flex flex-wrap items-center justify-between gap-2">
						<h3 class="font-medium">{t(`memory-kind-${memoryKind}`)}</h3>
						<button class="btn btn-ghost btn-xs" type="button" onclick={() => openAdd(memoryKind)}>{t('memory-add-short')}</button>
					</div>
					<ul class="divide-y divide-base-300">
						{#each grouped[memoryKind] as memory (memory.id)}
							<li class="flex items-center gap-2 py-2">
								<form class="m-0 flex min-w-0 flex-1 items-center gap-2" onsubmit={(event) => { event.preventDefault(); void save(memory); }}>
									<input class="input input-sm min-w-0 flex-1" bind:value={drafts[memory.id]} maxlength="2000" required aria-label={t('memory-content-label')} />
									<button class="btn btn-outline btn-xs" type="submit" disabled={!drafts[memory.id]?.trim()}>{t('memory-save-button')}</button>
								</form>
								<button class="btn btn-ghost btn-square btn-xs" type="button" onclick={() => void remove(memory)} aria-label={t('memory-delete-title')} title={t('memory-delete-title')}>
									<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 6h18M8 6V4h8v2M19 6l-1 14H6L5 6M10 11v5M14 11v5"/></svg>
								</button>
							</li>
						{:else}
							<li class="py-2 text-sm text-base-content/50">{t('dot-memory-empty')}</li>
						{/each}
					</ul>
				</section>
			{/each}
		{/if}
	</div>
</details>

<EditModal
	bind:open={adding}
	title={t('memory-add-heading')}
	description={t(`memory-kind-${kind}`)}
	cancellabel={t('admin-cancel')}
	savelabel={t('memory-add-button')}
	saving={saving || !content.trim()}
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
