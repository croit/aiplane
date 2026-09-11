<script lang="ts">
	import { onMount } from 'svelte';
	import { adminDelete, adminJson, adminPost, adminPut } from '$lib/admin-client';
	import { t } from '$lib/i18n.svelte';
	import { groupMemories, MEMORY_KINDS, type Memory, type MemoryKind } from '$lib/memory';

	let memories = $state<Memory[]>([]);
	let drafts = $state<Record<string, string>>({});
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let kind = $state<MemoryKind>('preference');
	let content = $state('');
	let grouped = $derived(groupMemories(memories));

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
		notice = null;
		try {
			await adminPost('/api/v0/memories', { kind, content });
			content = '';
			await refresh();
		} catch (caught) {
			notice = String(caught);
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

<div class="mx-auto w-full max-w-5xl">
	<h1 class="mb-2 text-2xl font-bold">{t('memory-heading')}</h1>
	<p class="mb-6 text-sm text-base-content/60">{t('memory-description')}</p>

	{#if error}<div class="alert alert-error mb-4"><span>{error}</span></div>{/if}
	{#if notice}<div class="alert alert-warning mb-4"><span>{notice}</span></div>{/if}

	<form class="card card-border mb-6 bg-base-100" onsubmit={(event) => { event.preventDefault(); void add(); }}>
		<div class="card-body gap-3">
			<h2 class="card-title text-base">{t('memory-add-heading')}</h2>
			<div class="flex flex-col gap-2 sm:flex-row">
				<select class="select select-bordered sm:w-48" bind:value={kind} aria-label={t('memory-kind-aria')}>
					{#each MEMORY_KINDS as memoryKind}<option value={memoryKind}>{t(`memory-kind-${memoryKind}`)}</option>{/each}
				</select>
				<input class="input input-bordered min-w-0 flex-1" bind:value={content} maxlength="2000" required placeholder={t('memory-content-placeholder')} />
				<button class="btn btn-primary" type="submit" disabled={!content.trim()}>{t('memory-add-button')}</button>
			</div>
		</div>
	</form>

	{#each MEMORY_KINDS as memoryKind}
		<section class="card card-border mb-6 bg-base-100">
			<div class="card-body">
				<h2 class="card-title text-base">{t(`memory-kind-${memoryKind}`)}</h2>
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
