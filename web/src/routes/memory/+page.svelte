<script lang="ts">
	import { onMount } from 'svelte';
	import { adminJson, adminPost, adminPut, adminDelete } from '$lib/admin-client';

	interface Memory {
		id: string;
		kind: string;
		content: string;
		created_at: string;
	}

	let memories = $state<Memory[]>([]);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let kind = $state('preference');
	let content = $state('');
	let editing = $state<string | null>(null);

	async function refresh() {
		try {
			const data = await adminJson<{ memories: Memory[] }>('/api/v0/memories');
			memories = data.memories;
			error = null;
		} catch (err) {
			error = String(err);
		}
	}

	async function save() {
		notice = null;
		try {
			if (editing) {
				await adminPut(`/api/v0/memories/${editing}`, { kind, content });
			} else {
				await adminPost('/api/v0/memories', { kind, content });
			}
			editing = null;
			content = '';
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	async function remove(id: string) {
		if (!confirm('Delete this memory?')) return;
		try {
			await adminDelete(`/api/v0/memories/${id}`);
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	onMount(refresh);
</script>

<div class="flex items-center justify-between mb-4">
	<h1 class="text-2xl font-bold">Memory</h1>
</div>

<p class="text-base-content/60 text-sm mb-6">
	What the remember/recall tools keep about you. The model sees these as context in every conversation.
</p>

{#if error}<div class="alert alert-error mb-4"><span>{error}</span></div>{/if}
{#if notice}<div class="alert alert-warning mb-4"><span>{notice}</span></div>{/if}

<div class="card border border-base-300 mb-6">
	<div class="card-body">
		<h2 class="card-title text-base">{editing ? 'Edit memory' : 'Add a memory'}</h2>
		<div class="flex flex-wrap gap-3 items-end">
			<label class="flex flex-col gap-1 w-44">
				<span class="label-text">Kind</span>
				<select class="select select-bordered select-sm" bind:value={kind}>
					<option value="preference">Preference</option>
					<option value="project">Project</option>
					<option value="fact">Fact</option>
				</select>
			</label>
			<label class="flex flex-col gap-1 flex-1 min-w-64">
				<span class="label-text">Content</span>
				<input class="input input-bordered input-sm" bind:value={content} />
			</label>
			<button class="btn btn-primary btn-sm" onclick={save} disabled={!content.trim()}>
				{editing ? 'Save' : 'Add'}
			</button>
			{#if editing}
				<button class="btn btn-ghost btn-sm" onclick={() => { editing = null; content = ''; }}>Cancel</button>
			{/if}
		</div>
	</div>
</div>

<ul class="flex flex-col gap-2">
	{#each memories as m (m.id)}
		<li class="card border border-base-300">
			<div class="card-body py-2 flex-row items-center gap-3 flex-wrap">
				<span class="badge badge-outline badge-sm">{m.kind}</span>
				<span class="flex-1 min-w-48 text-sm">{m.content}</span>
				<button class="btn btn-ghost btn-xs" onclick={() => { editing = m.id; kind = m.kind; content = m.content; }}>Edit</button>
				<button class="btn btn-ghost btn-xs text-error" onclick={() => remove(m.id)}>Delete</button>
			</div>
		</li>
	{/each}
</ul>
