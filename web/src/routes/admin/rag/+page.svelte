<script lang="ts">
	import { onMount } from 'svelte';
	import { adminJson, adminPost, adminDelete } from '$lib/admin-client';
	import { t, dt } from '$lib/i18n.svelte';

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
			if (!res.ok) {
				notice = t('rag-source-test-failed', { error: res.error ?? '' });
			} else {
				const entries = res.root_entries ?? 0;
				const head = res.account
					? t('rag-source-test-ok', { account: res.account, entries })
					: t('rag-source-test-ok-plain', { entries });
				notice = res.server ? `${head} ${t('rag-source-detected', { server: res.server })}` : head;
			}
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
			notice = t('rag-toast-reindex-queued-ref', { ref: c.name });
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	async function rotateToken(c: Collection) {
		if (!confirm(t('rag-sync-token-confirm'))) return;
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
		if (!confirm(t('rag-delete-collection-confirm', { name: c.name }))) return;
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
			notice = t('rag-toast-rebuild-queued');
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
				res.skipped > 0
					? t('rag-toast-bulk-queued-skipped', { added: res.added.length, skipped: res.skipped })
					: t('rag-toast-bulk-queued', { added: res.added.length });
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
		if (!confirm(t('rag-remove-source-confirm', { source: ref.git_url ?? ref.git_ref }))) return;
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

	/** The wire values match `rag-status-*`; anything unknown shows verbatim. */
	function statusLabel(status: string): string {
		const key = `rag-status-${status}`;
		const value = t(key);
		return value === key ? status : value;
	}

	onMount(() => {
		void refresh();
		void loadFormData();
	});
</script>

<div class="flex items-center justify-between mb-4">
	<h1 class="text-2xl font-bold">{t('rag-heading')}</h1>
	<button class="btn btn-primary btn-sm" onclick={() => (creating = !creating)}>
		{creating ? t('rag-button-cancel') : t('rag-button-new-collection')}
	</button>
</div>

{#if creating}
	<div class="card border border-base-300 bg-base-200 mb-6">
		<div class="card-body gap-3">
			<h2 class="card-title text-base">{t('rag-button-new-collection')}</h2>
			<div class="grid gap-3 sm:grid-cols-2">
				<label class="flex flex-col gap-1">
					<span class="text-xs">{t('rag-label-name')}</span>
					<input class="input input-bordered input-sm" bind:value={form.name} />
				</label>
				<label class="flex flex-col gap-1">
					<span class="text-xs">{t('rag-label-embedding-model')}</span>
					<input class="input input-bordered input-sm" bind:value={form.embedding_model} />
				</label>
				<label class="flex flex-col gap-1 sm:col-span-2">
					<span class="text-xs">{t('rag-label-description')}</span>
					<input class="input input-bordered input-sm" bind:value={form.description} />
				</label>
				<label class="flex flex-col gap-1">
					<span class="text-xs">{t('rag-label-source-kind')}</span>
					<select class="select select-bordered select-sm" bind:value={form.source_kind}>
						{#each providers as p (p.kind)}
							<option value={p.kind}>{p.label}</option>
						{/each}
					</select>
				</label>
				<label class="flex flex-col gap-1">
					<span class="text-xs">{t('rag-label-profile')}</span>
					<select class="select select-bordered select-sm" bind:value={form.profile}>
						<option value="">{t('rag-option-profile-none')}</option>
						{#each profiles as p (p.name)}
							<option value={p.name}>{p.name}</option>
						{/each}
					</select>
				</label>

				{#if form.source_kind === 'git'}
					<label class="flex flex-col gap-1">
						<span class="text-xs">{t('rag-label-git-url')}</span>
						<input class="input input-bordered input-sm" bind:value={form.git_url} />
					</label>
					<label class="flex flex-col gap-1">
						<span class="text-xs">{t('rag-label-branch-tag')}</span>
						<input class="input input-bordered input-sm" bind:value={form.git_ref} />
					</label>
				{:else if selectedProvider}
					{#each selectedProvider.fields as f (f.key)}
						<label class="flex flex-col gap-1">
							<span class="text-xs">
								{f.label}{f.required ? ' *' : ''}
							</span>
							<input
								class="input input-bordered input-sm"
								type={f.secret ? 'password' : 'text'}
								placeholder={f.default ?? ''}
								value={sourceConfig[f.key] ?? ''}
								oninput={(e) => (sourceConfig[f.key] = e.currentTarget.value)}
							/>
							{#if f.help}<span class="text-xs opacity-60">{f.help}</span>{/if}
						</label>
					{/each}
				{/if}

				<label class="flex flex-col gap-1">
					<span class="text-xs">{t('rag-label-chunk-size')}</span>
					<input
						class="input input-bordered input-sm"
						type="number"
						bind:value={form.chunk_size}
					/>
				</label>
				<label class="flex flex-col gap-1">
					<span class="text-xs">{t('rag-label-chunk-overlap')}</span>
					<input
						class="input input-bordered input-sm"
						type="number"
						bind:value={form.chunk_overlap}
					/>
				</label>
			</div>
			{#if selectedProvider && selectedProvider.auth.kind === 'oauth2'}
				<p class="text-xs opacity-70">{t('rag-source-consent-save-first')}</p>
			{/if}
			<div class="flex gap-2 justify-end">
				{#if form.source_kind !== 'git'}
					<button class="btn btn-sm" onclick={testSource} disabled={testing}>
						{testing ? t('rag-source-testing') : t('rag-source-test-button')}
					</button>
				{/if}
				<button
					class="btn btn-primary btn-sm"
					onclick={createCollection}
					disabled={!form.name.trim() || !form.embedding_model.trim()}
				>
					{t('rag-button-create')}
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
			<h2 class="card-title text-base">{t('rag-sync-url-heading')}</h2>
			<pre class="bg-base-100 border border-base-300 rounded-md p-3 font-mono text-xs select-all break-all whitespace-pre-wrap">curl -X POST "{secret}"</pre>
		</div>
	</div>
{/if}

<ul class="flex flex-col gap-3">
	{#each collections as c (c.id)}
		<li class="card border border-base-300">
			<div class="card-body py-3">
				<div class="flex items-center gap-3 flex-wrap">
					<span class="badge {statusBadge(c.status)} badge-sm">{statusLabel(c.status)}</span>
					<button class="font-medium link link-hover" onclick={() => open(c)}>{c.name}</button>
					<span class="font-mono text-xs text-base-content/50 truncate max-w-56">{c.git_url}</span>
					{#if c.sync_hook_set}<span class="badge badge-outline badge-sm">{t('rag-badge-sync-hook')}</span>{/if}
					<span class="flex-1"></span>
					<button class="btn btn-ghost btn-xs" onclick={() => reindex(c)}>{t('rag-button-reindex')}</button>
					<button class="btn btn-ghost btn-xs" onclick={() => rotateToken(c)}>{t('rag-button-sync-token')}</button>
					<button class="btn btn-ghost btn-xs text-error" onclick={() => remove(c)}>{t('groups-delete')}</button>
				</div>
				{#if c.description}<p class="text-xs text-base-content/60">{c.description}</p>{/if}
				{#if expanded === c.id}
					<ul class="mt-2 flex flex-col gap-1 border-t border-base-300 pt-2">
						{#each refs[c.id] ?? [] as ref (ref.id)}
							<li class="flex items-center gap-2 text-xs flex-wrap">
								<span class="badge badge-sm {statusBadge(ref.status)}">{statusLabel(ref.status)}</span>
								{#if ref.is_primary}<span class="badge badge-outline badge-xs">{t('rag-badge-primary')}</span>{/if}
								<span class="font-mono">{ref.git_ref}</span>
								{#if ref.git_url}
									<span class="font-mono text-base-content/50 truncate max-w-64">{ref.git_url}</span>
								{/if}
								{#if ref.last_indexed_at}
									<span class="text-base-content/50">
										{t('rag-ref-indexed-at', { date: dt(ref.last_indexed_at) })}
									</span>
								{/if}
								{#if ref.last_error}<span class="text-error truncate max-w-64">{ref.last_error}</span>{/if}
								<span class="flex-1"></span>
								{#if !ref.is_primary}
									<button class="btn btn-ghost btn-xs" onclick={() => makePrimary(ref)}>
										{t('rag-button-set-primary')}
									</button>
								{/if}
								<button class="btn btn-ghost btn-xs" onclick={() => rebuildRef(ref)}>
									{t('rag-button-rebuild')}
								</button>
								<button class="btn btn-ghost btn-xs text-error" onclick={() => removeRef(ref)}>
									{t('rag-button-remove')}
								</button>
							</li>
						{:else}
							<li class="text-xs text-base-content/50">{t('rag-no-sources')}</li>
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
									{t('rag-add-sources-hint', { at: '@ref', ref: c.git_ref })}
								</p>
								<div class="flex gap-2 justify-end mt-1">
									<button class="btn btn-ghost btn-xs" onclick={() => (addingTo = null)}>
										{t('rag-button-cancel')}
									</button>
									<button class="btn btn-primary btn-xs" onclick={() => addSources(c.id)}>
										{t('rag-button-add-bulk')}
									</button>
								</div>
							{:else}
								<button class="btn btn-ghost btn-xs" onclick={() => (addingTo = c.id)}>
									+ {t('rag-button-add-source')}
								</button>
							{/if}
						</li>
					</ul>
				{/if}
			</div>
		</li>
	{/each}
</ul>
