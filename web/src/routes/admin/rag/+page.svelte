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

	interface ProviderField {
		key: string;
		label: string;
		help?: string | null;
		required: boolean;
		secret: boolean;
		default?: string | null;
	}
	interface Provider {
		kind: string;
		label: string;
		description: string;
		auth: { kind: string };
		fields: ProviderField[];
	}
	interface Profile {
		name: string;
		description: string | null;
		version: number;
		fields: { key: string; label: string }[];
	}

	let collections = $state<Collection[]>([]);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let expanded = $state<number | null>(null);
	let refs = $state<Record<number, Ref[]>>({});
	let secret = $state<string | null>(null);

	// Per-collection "add sources" box: one url per line, an optional
	// `@ref` after it — the ergonomics the legacy bulk form had, because
	// these collections routinely aggregate tens of repositories.
	let sourceDraft = $state<Record<number, string>>({});
	let addingTo = $state<number | null>(null);

	// New-collection form, driven by the provider descriptors so a source
	// kind this build has never heard of still renders its own fields.
	let providers = $state<Provider[]>([]);
	let profiles = $state<Profile[]>([]);
	let creating = $state(false);
	let form = $state({
		name: '',
		description: '',
		git_url: '',
		git_ref: 'main',
		embedding_model: '',
		profile: '',
		source_kind: 'git',
		chunk_size: 512,
		chunk_overlap: 64
	});
	let sourceConfig = $state<Record<string, string>>({});
	let testing = $state(false);
	const selectedProvider = $derived(providers.find((p) => p.kind === form.source_kind) ?? null);

	async function refresh() {
		try {
			const data = await adminJson<{ data: Collection[] }>('/api/v0/rag/collections');
			collections = data.data ?? [];
			error = null;
		} catch (err) {
			error = String(err);
		}
	}

	async function loadFormData() {
		try {
			providers = (await adminJson<{ data: Provider[] }>('/api/v0/rag/providers')).data ?? [];
			profiles = (await adminJson<{ data: Profile[] }>('/api/v0/rag/profiles')).data ?? [];
		} catch {
			// The list still works without the create form's vocabulary.
		}
	}

	async function createCollection() {
		try {
			await adminPost('/api/v0/rag/collections', {
				name: form.name.trim(),
				description: form.description.trim() || null,
				git_url: form.git_url.trim(),
				git_ref: form.git_ref.trim() || 'main',
				embedding_model: form.embedding_model.trim(),
				profile: form.profile || null,
				source_kind: form.source_kind,
				source_config: sourceConfig,
				chunk_size: form.chunk_size,
				chunk_overlap: form.chunk_overlap
			});
			creating = false;
			form.name = '';
			form.git_url = '';
			sourceConfig = {};
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	/** Probe the entered source before saving — a wrong host or credential
	 * should fail here, not silently on the indexing timeline. */
	async function testSource() {
		testing = true;
		try {
			const res = await adminPost<{
				ok: boolean;
				account?: string | null;
				root_entries?: number;
				server?: string | null;
				error?: string;
			}>('/api/v0/rag/test-source', {
				source_kind: form.source_kind,
				source_config: sourceConfig
			});
			notice = res.ok
				? `Connected${res.account ? ` as ${res.account}` : ''} — ${res.root_entries} entries at the root${res.server ? ` (${res.server})` : ''}.`
				: `Could not connect: ${res.error}`;
		} catch (err) {
			notice = String(err);
		} finally {
			testing = false;
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

	async function reloadRefs(collectionId: number) {
		try {
			const data = await adminJson<{ data: Ref[] }>(
				`/api/v0/rag/collections/${collectionId}/refs`
			);
			refs[collectionId] = data.data ?? [];
		} catch (err) {
			notice = String(err);
		}
	}

	/** One source per line: `<url> [@ref]`; `#` comments and blanks skipped. */
	async function addSources(collectionId: number) {
		const sources = (sourceDraft[collectionId] ?? '')
			.split('\n')
			.map((l) => l.trim())
			.filter((l) => l && !l.startsWith('#'))
			.map((line) => {
				const [url, ref] = line.split(/\s+/, 2);
				return { url, git_ref: ref ? ref.replace(/^@/, '') : null };
			});
		if (sources.length === 0) return;
		try {
			const res = await adminPost<{ added: unknown[]; skipped: number }>(
				`/api/v0/rag/collections/${collectionId}/refs`,
				{ sources }
			);
			notice =
				`Added ${res.added.length} source${res.added.length === 1 ? '' : 's'}` +
				(res.skipped > 0 ? ` (${res.skipped} already present).` : '.');
			sourceDraft[collectionId] = '';
			addingTo = null;
			await reloadRefs(collectionId);
		} catch (err) {
			notice = String(err);
		}
	}

	async function makePrimary(ref: Ref) {
		try {
			await adminPost(`/api/v0/rag/collections/${ref.collection_id}/refs/${ref.id}/primary`);
			await reloadRefs(ref.collection_id);
		} catch (err) {
			notice = String(err);
		}
	}

	async function removeRef(ref: Ref) {
		if (!confirm(`Remove source ${ref.git_url ?? ref.git_ref}?`)) return;
		try {
			await adminDelete(`/api/v0/rag/collections/${ref.collection_id}/refs/${ref.id}`);
			await reloadRefs(ref.collection_id);
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

	onMount(() => {
		void refresh();
		void loadFormData();
	});
</script>

<div class="flex items-center justify-between mb-4">
	<h1 class="text-2xl font-bold">RAG collections</h1>
	<button class="btn btn-primary btn-sm" onclick={() => (creating = !creating)}>
		{creating ? 'Cancel' : 'New collection'}
	</button>
</div>

{#if creating}
	<div class="card border border-base-300 bg-base-200 mb-6">
		<div class="card-body gap-3">
			<h2 class="card-title text-base">New collection</h2>
			<div class="grid gap-3 sm:grid-cols-2">
				<label class="form-control">
					<span class="label-text text-xs">Name</span>
					<input class="input input-bordered input-sm" bind:value={form.name} />
				</label>
				<label class="form-control">
					<span class="label-text text-xs">Embedding model</span>
					<input class="input input-bordered input-sm" bind:value={form.embedding_model} />
				</label>
				<label class="form-control sm:col-span-2">
					<span class="label-text text-xs">Description</span>
					<input class="input input-bordered input-sm" bind:value={form.description} />
				</label>
				<label class="form-control">
					<span class="label-text text-xs">Source kind</span>
					<select class="select select-bordered select-sm" bind:value={form.source_kind}>
						{#each providers as p (p.kind)}
							<option value={p.kind}>{p.label}</option>
						{/each}
					</select>
				</label>
				<label class="form-control">
					<span class="label-text text-xs">Extraction profile</span>
					<select class="select select-bordered select-sm" bind:value={form.profile}>
						<option value="">None</option>
						{#each profiles as p (p.name)}
							<option value={p.name}>{p.name}</option>
						{/each}
					</select>
				</label>

				{#if form.source_kind === 'git'}
					<label class="form-control">
						<span class="label-text text-xs">Repository URL</span>
						<input class="input input-bordered input-sm" bind:value={form.git_url} />
					</label>
					<label class="form-control">
						<span class="label-text text-xs">Ref</span>
						<input class="input input-bordered input-sm" bind:value={form.git_ref} />
					</label>
				{:else if selectedProvider}
					{#each selectedProvider.fields as f (f.key)}
						<label class="form-control">
							<span class="label-text text-xs">
								{f.label}{f.required ? ' *' : ''}
							</span>
							<input
								class="input input-bordered input-sm"
								type={f.secret ? 'password' : 'text'}
								placeholder={f.default ?? ''}
								value={sourceConfig[f.key] ?? ''}
								oninput={(e) => (sourceConfig[f.key] = e.currentTarget.value)}
							/>
							{#if f.help}<span class="label-text-alt text-xs opacity-60">{f.help}</span>{/if}
						</label>
					{/each}
				{/if}

				<label class="form-control">
					<span class="label-text text-xs">Chunk size</span>
					<input
						class="input input-bordered input-sm"
						type="number"
						bind:value={form.chunk_size}
					/>
				</label>
				<label class="form-control">
					<span class="label-text text-xs">Chunk overlap</span>
					<input
						class="input input-bordered input-sm"
						type="number"
						bind:value={form.chunk_overlap}
					/>
				</label>
			</div>
			{#if selectedProvider && selectedProvider.auth.kind === 'oauth2'}
				<p class="text-xs opacity-70">
					This provider authorises through a browser: save the collection first, then connect it.
				</p>
			{/if}
			<div class="flex gap-2 justify-end">
				{#if form.source_kind !== 'git'}
					<button class="btn btn-sm" onclick={testSource} disabled={testing}>
						{testing ? 'Testing…' : 'Test connection'}
					</button>
				{/if}
				<button
					class="btn btn-primary btn-sm"
					onclick={createCollection}
					disabled={!form.name.trim() || !form.embedding_model.trim()}
				>
					Create
				</button>
			</div>
		</div>
	</div>
{/if}

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
								{#if ref.git_url}
									<span class="font-mono text-base-content/50 truncate max-w-64">{ref.git_url}</span>
								{/if}
								{#if ref.last_indexed_at}
									<span class="text-base-content/50">indexed {new Date(ref.last_indexed_at).toLocaleString()}</span>
								{/if}
								{#if ref.last_error}<span class="text-error truncate max-w-64">{ref.last_error}</span>{/if}
								<span class="flex-1"></span>
								{#if !ref.is_primary}
									<button class="btn btn-ghost btn-xs" onclick={() => makePrimary(ref)}>
										Make primary
									</button>
								{/if}
								<button class="btn btn-ghost btn-xs" onclick={() => rebuildRef(ref)}>Rebuild</button>
								<button class="btn btn-ghost btn-xs text-error" onclick={() => removeRef(ref)}>
									Remove
								</button>
							</li>
						{:else}
							<li class="text-xs text-base-content/50">
								No sources — this collection indexes nothing until one is added.
							</li>
						{/each}

						<li class="mt-2">
							{#if addingTo === c.id}
								<textarea
									class="textarea textarea-bordered w-full font-mono text-xs"
									rows="4"
									placeholder={'https://example.com/repo.git\nhttps://example.com/other.git @develop'}
									value={sourceDraft[c.id] ?? ''}
									oninput={(e) => (sourceDraft[c.id] = e.currentTarget.value)}
								></textarea>
								<p class="text-xs opacity-60 mt-1">
									One source per line; add <code>@ref</code> to override this collection's
									<code>{c.git_ref}</code>.
								</p>
								<div class="flex gap-2 justify-end mt-1">
									<button class="btn btn-ghost btn-xs" onclick={() => (addingTo = null)}>
										Cancel
									</button>
									<button class="btn btn-primary btn-xs" onclick={() => addSources(c.id)}>
										Add sources
									</button>
								</div>
							{:else}
								<button class="btn btn-ghost btn-xs" onclick={() => (addingTo = c.id)}>
									+ Add sources
								</button>
							{/if}
						</li>
					</ul>
				{/if}
			</div>
		</li>
	{/each}
</ul>
