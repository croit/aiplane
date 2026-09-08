<script lang="ts">
	import { onMount } from 'svelte';
	import { adminJson, adminPost, adminDelete } from '$lib/admin-client';

	interface Collection {
		id: number;
		name: string;
		description: string | null;
		git_url: string;
		git_ref: string;
		status: string;
		sync_hook_set: boolean;
		allowed_groups?: string[];
	}
	interface Ref {
		id: number;
		collection_id: number;
		git_ref: string;
		git_url: string | null;
		is_primary: boolean;
		status: string;
		last_indexed_at: string | null;
		last_indexed_commit: string | null;
		last_error: string | null;
	}

	let collections = $state<Collection[]>([]);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let expanded = $state<number | null>(null);
	let refs = $state<Record<number, Ref[]>>({});
	let secret = $state<string | null>(null);

	async function refresh() {
		try {
			const data = await adminJson<{ data: Collection[] }>('/api/v0/rag/collections');
			collections = data.data ?? [];
			error = null;
		} catch (err) {
			error = String(err);
		}
	}

	async function open(c: Collection) {
		if (expanded === c.id) {
			expanded = null;
			return;
		}
		expanded = c.id;
		if (!refs[c.id]) {
			try {
				const data = await adminJson<{ data: Ref[] }>(`/api/v0/rag/collections/${c.id}/refs`);
				refs[c.id] = data.data ?? [];
			} catch (err) {
				refs[c.id] = [];
				notice = String(err);
			}
		}
	}

	async function reindex(c: Collection) {
		try {
			await adminPost(`/api/v0/rag/collections/${c.id}/reindex`);
			notice = `Reindex requested for ${c.name}.`;
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	async function rotateToken(c: Collection) {
		if (!confirm('Mint a new sync token? The old hook URL stops working.')) return;
		try {
			const res = await adminPost<{ token: string; url_hint: string }>(
				`/api/v0/rag/collections/${c.id}/sync-token`
			);
			secret = `${location.origin}${res.url_hint}`;
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	async function remove(c: Collection) {
		if (!confirm(`Delete collection ${c.name} and its index?`)) return;
		try {
			await adminDelete(`/api/v0/rag/collections/${c.id}`);
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	async function rebuildRef(ref: Ref) {
		try {
			await adminPost(`/api/v0/rag/collections/${ref.collection_id}/refs/${ref.id}/rebuild`);
			notice = 'Full rebuild requested.';
		} catch (err) {
			notice = String(err);
		}
	}

	function statusBadge(status: string): string {
		return (
			{
				ready: 'badge-success',
				pending: 'badge-warning',
				indexing: 'badge-info',
				cloning: 'badge-info',
				error: 'badge-error'
			}[status] ?? 'badge-ghost'
		);
	}

	onMount(refresh);
</script>

<div class="flex items-center justify-between mb-4">
	<h1 class="text-2xl font-bold">RAG collections</h1>
</div>

{#if error}<div class="alert alert-error mb-4"><span>{error}</span></div>{/if}
{#if notice}<div class="alert alert-warning mb-4"><span>{notice}</span></div>{/if}

{#if secret}
	<div class="card border border-success mb-6">
		<div class="card-body">
			<h2 class="card-title text-base">Sync hook URL — shown once</h2>
			<pre class="bg-base-100 border border-base-300 rounded-md p-3 font-mono text-xs select-all break-all whitespace-pre-wrap">curl -X POST "{secret}"</pre>
		</div>
	</div>
{/if}

<ul class="flex flex-col gap-3">
	{#each collections as c (c.id)}
		<li class="card border border-base-300">
			<div class="card-body py-3">
				<div class="flex items-center gap-3 flex-wrap">
					<span class="badge {statusBadge(c.status)} badge-sm">{c.status}</span>
					<button class="font-medium link link-hover" onclick={() => open(c)}>{c.name}</button>
					<span class="font-mono text-xs text-base-content/50 truncate max-w-56">{c.git_url}</span>
					{#if c.sync_hook_set}<span class="badge badge-outline badge-sm">hook</span>{/if}
					<span class="flex-1"></span>
					<button class="btn btn-ghost btn-xs" onclick={() => reindex(c)}>Reindex</button>
					<button class="btn btn-ghost btn-xs" onclick={() => rotateToken(c)}>Sync token</button>
					<button class="btn btn-ghost btn-xs text-error" onclick={() => remove(c)}>Delete</button>
				</div>
				{#if c.description}<p class="text-xs text-base-content/60">{c.description}</p>{/if}
				{#if expanded === c.id}
					<ul class="mt-2 flex flex-col gap-1 border-t border-base-300 pt-2">
						{#each refs[c.id] ?? [] as ref (ref.id)}
							<li class="flex items-center gap-2 text-xs flex-wrap">
								<span class="badge badge-sm {statusBadge(ref.status)}">{ref.status}</span>
								{#if ref.is_primary}<span class="badge badge-outline badge-xs">primary</span>{/if}
								<span class="font-mono">{ref.git_ref}</span>
								{#if ref.last_indexed_at}
									<span class="text-base-content/50">indexed {new Date(ref.last_indexed_at).toLocaleString()}</span>
								{/if}
								{#if ref.last_error}<span class="text-error truncate max-w-64">{ref.last_error}</span>{/if}
								<span class="flex-1"></span>
								<button class="btn btn-ghost btn-xs" onclick={() => rebuildRef(ref)}>Rebuild</button>
							</li>
						{:else}
							<li class="text-xs text-base-content/50">No sources.</li>
						{/each}
					</ul>
				{/if}
			</div>
		</li>
	{/each}
</ul>
