<script lang="ts">
	import { onMount } from 'svelte';
	import { adminDelete, adminJson, adminPost, adminPut } from '$lib/admin-client';
	import EditModal from '$lib/components/EditModal.svelte';
	import { t } from '$lib/i18n.svelte';
	import { groupMemories, MEMORY_KINDS, type Memory, type MemoryKind } from '$lib/memory';

	let memories = $state<Memory[]>([]);
	let drafts = $state<Record<string, string>>({});
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let grouped = $derived(groupMemories(memories));

	// The add form used to sit above the three category cards with its own
	// kind picker, so writing a preference meant scrolling past the section
	// already labelled Preferences and then telling the form which section it
	// was. Each card carries its own Add button now, and it opens the dialog
	// with that card's kind already chosen.
	let adding = $state(false);
	let kind = $state<MemoryKind>('preference');
	let content = $state('');
	let saving = $state(false);

	function openAdd(memoryKind: MemoryKind) {
		kind = memoryKind;
		content = '';
		adding = true;
	}

	async function refresh() {
		try {
			const data = await adminJson<{ memories: Memory[] }>('/api/v0/memories');
			memories = data.memories;
			drafts = Object.fromEntries(memories.map((memory) => [memory.id, memory.content]));
			error = null;
		} catch (caught) {
			error = String(caught);
		}
	}

	async function add() {
		if (!content.trim()) return;
		notice = null;
		saving = true;
		try {
			await adminPost('/api/v0/memories', { kind, content });
			content = '';
			adding = false;
			await refresh();
		} catch (caught) {
			notice = String(caught);
		} finally {
			saving = false;
		}
	}

	async function save(memory: Memory) {
		notice = null;
		try {
			await adminPut(`/api/v0/memories/${memory.id}`, { kind: memory.kind, content: drafts[memory.id] });
			await refresh();
		} catch (caught) {
			notice = String(caught);
		}
	}

	async function remove(id: string) {
		if (!confirm(t('memory-delete-confirm'))) return;
		try {
			await adminDelete(`/api/v0/memories/${id}`);
			await refresh();
		} catch (caught) {
			notice = String(caught);
		}
	}

	onMount(refresh);
</script>

<div class="w-full">
	<h1 class="mb-2 text-2xl font-bold">{t('memory-heading')}</h1>
	<p class="mb-6 text-sm text-base-content/60">{t('memory-description')}</p>

	{#if error}<div class="alert alert-error mb-4"><span>{error}</span></div>{/if}
	{#if notice}<div class="alert alert-warning mb-4"><span>{notice}</span></div>{/if}

	{#each MEMORY_KINDS as memoryKind}
		<section class="card card-border mb-6 bg-base-100">
			<div class="card-body">
				<div class="flex flex-wrap items-center justify-between gap-2">
					<h2 class="card-title text-base">{t(`memory-kind-${memoryKind}`)}</h2>
					<button class="btn btn-sm" type="button" onclick={() => openAdd(memoryKind)}>{t('memory-add-short')}</button>
				</div>
				<!-- The kinds are not just labels: a preference is sent with every
				     conversation, while project notes and facts wait to be looked
				     up. That decides which card something belongs in, so it is
				     said here rather than left to be discovered. -->
				<p class="text-sm text-base-content/60">{t(`memory-kind-${memoryKind}-hint`)}</p>
				<ul class="flex flex-col divide-y divide-base-300">
					{#each grouped[memoryKind] as memory (memory.id)}
						<li class="flex items-center gap-2 py-2">
							<form class="m-0 flex min-w-0 flex-1 items-center gap-2" onsubmit={(event) => { event.preventDefault(); void save(memory); }}>
								<input class="input input-bordered input-sm min-w-0 flex-1" bind:value={drafts[memory.id]} maxlength="2000" required aria-label={t('memory-content-label')} />
								<button class="btn btn-outline btn-sm" type="submit" disabled={!drafts[memory.id]?.trim()}>{t('memory-save-button')}</button>
							</form>
							<button class="btn btn-ghost btn-square btn-sm" type="button" onclick={() => remove(memory.id)} title={t('memory-delete-title')} aria-label={t('memory-delete-title')}>
								<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 6h18M8 6V4h8v2M19 6l-1 14H6L5 6M10 11v5M14 11v5"/></svg>
							</button>
						</li>
					{:else}
						<li class="py-2 text-sm text-base-content/50">{t('memory-empty')}</li>
					{/each}
				</ul>
			</div>
		</section>
	{/each}
</div>

<!-- One dialog for all three cards: the button that opened it has already
     chosen the kind, and the picker stays so a mis-click is a correction
     rather than a delete-and-retype. -->
<EditModal
	bind:open={adding}
	title={t('memory-add-heading')}
	description={t(`memory-kind-${kind}`)}
	cancellabel={t('admin-cancel')}
	savelabel={t('memory-add-button')}
	{saving}
	onsave={add}
>
	<div class="flex flex-col gap-3">
		<fieldset class="fieldset">
			<legend class="fieldset-legend">{t('memory-kind-aria')}</legend>
			<select class="select w-full" bind:value={kind} aria-label={t('memory-kind-aria')}>
				{#each MEMORY_KINDS as option}<option value={option}>{t(`memory-kind-${option}`)}</option>{/each}
			</select>
		</fieldset>
		<fieldset class="fieldset">
			<legend class="fieldset-legend">{t('memory-content-label')}</legend>
			<!-- svelte-ignore a11y_autofocus -->
			<textarea class="textarea min-h-24 w-full" bind:value={content} maxlength="2000" autofocus aria-label={t('memory-content-label')} placeholder={t('memory-content-placeholder')}></textarea>
		</fieldset>
	</div>
</EditModal>
