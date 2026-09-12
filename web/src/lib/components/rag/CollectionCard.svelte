<script lang="ts">
	import { onMount, untrack } from 'svelte';
	import { adminDelete, adminJson, adminPatch, adminPost } from '$lib/admin-client';
	import { dt, t } from '$lib/i18n.svelte';
	import { parseSources, sourceLabel, type RagCollection, type RagLogEntry, type RagRef } from '$lib/rag';
	import { base } from '$app/paths';
	import EditModal from '$lib/components/EditModal.svelte';

	let { collection, onchanged, onnotice } = $props<{
		collection: RagCollection;
		onchanged: () => void | Promise<void>;
		onnotice: (message: string) => void;
	}>();

	let refs = $state<RagRef[]>([]);
	let addingSource = $state(false);
	let sourceDraft = $state('');
	let sourceUrl = $state('');
	let sourceRef = $state(untrack(() => collection.git_ref));
	let editingRef = $state<number | null>(null);
	let sourceEditorOpen = $state(false);
	let refUrl = $state('');
	let refName = $state('');
	let openLog = $state<number | null>(null);
	let logs = $state<Record<number, RagLogEntry[]>>({});
	let syncUrl = $state<string | null>(null);

	function badge(status: string): string {
		return ({ ready: 'badge-success', pending: 'badge-warning', indexing: 'badge-info', cloning: 'badge-info', error: 'badge-error' } as Record<string, string>)[status] ?? 'badge-ghost';
	}

	function label(status: string): string {
		const key = `rag-status-${status}`;
		const translated = t(key);
		return translated === key ? status : translated;
	}

	async function loadRefs() {
		try {
			refs = (await adminJson<{ data: RagRef[] }>(`/api/v0/rag/collections/${collection.id}/refs`)).data ?? [];
		} catch (error) {
			onnotice(String(error));
		}
	}

	async function post(path: string, notice?: string) {
		try {
			await adminPost(path);
			if (notice) onnotice(notice);
			await loadRefs();
			await onchanged();
		} catch (error) {
			onnotice(String(error));
		}
	}

	async function addSources() {
		const sources = parseSources(sourceDraft);
		if (!sources.length) return;
		try {
			const result = await adminPost<{ added: unknown[]; skipped: number }>(`/api/v0/rag/collections/${collection.id}/refs`, { sources });
			onnotice(result.skipped ? t('rag-toast-bulk-queued-skipped', { added: result.added.length, skipped: result.skipped }) : t('rag-toast-bulk-queued', { added: result.added.length }));
			sourceDraft = '';
			await loadRefs();
		} catch (error) {
			onnotice(String(error));
		}
	}

	async function addSource() {
		try {
			await adminPost(`/api/v0/rag/collections/${collection.id}/refs`, {
				sources: [{ url: collection.search_mode === 'aggregate' ? sourceUrl : '', git_ref: sourceRef }]
			});
			sourceUrl = '';
			sourceRef = collection.git_ref;
			await loadRefs();
		} catch (error) {
			onnotice(String(error));
		}
	}

	function editSource(source: RagRef) {
		editingRef = source.id;
		refUrl = source.git_url ?? '';
		refName = source.git_ref;
		sourceEditorOpen = true;
	}

	/** The source dialog lives outside the row loop; `editingRef` is the row
	 *  it was opened on. */
	async function saveEditedSource() {
		const source = refs.find((candidate) => candidate.id === editingRef);
		if (source) await saveSource(source);
	}

	async function saveSource(source: RagRef) {
		try {
			await adminPatch(`/api/v0/rag/collections/${collection.id}/refs/${source.id}`, { git_url: refUrl.trim() || null, git_ref: refName.trim() });
			editingRef = null;
			sourceEditorOpen = false;
			onnotice(t('rag-toast-source-updated'));
			await loadRefs();
		} catch (error) {
			onnotice(String(error));
		}
	}

	async function toggleLog(source: RagRef) {
		if (openLog === source.id) {
			openLog = null;
			return;
		}
		try {
			logs[source.id] = (await adminJson<{ data: RagLogEntry[] }>(`/api/v0/rag/collections/${collection.id}/refs/${source.id}/log`)).data ?? [];
			openLog = source.id;
		} catch (error) {
			onnotice(String(error));
		}
	}

	async function removeSource(source: RagRef) {
		if (!confirm(t('rag-remove-source-confirm', { source: source.git_url ?? source.git_ref }))) return;
		try {
			await adminDelete(`/api/v0/rag/collections/${collection.id}/refs/${source.id}`);
			await loadRefs();
		} catch (error) {
			onnotice(String(error));
		}
	}

	async function rotateSync() {
		if (!confirm(t('rag-sync-token-confirm'))) return;
		try {
			const result = await adminPost<{ url_hint: string }>(`/api/v0/rag/collections/${collection.id}/sync-token`);
			syncUrl = `${location.origin}${result.url_hint}`;
			await onchanged();
		} catch (error) {
			onnotice(String(error));
		}
	}

	async function clearSync() {
		try {
			await adminPost(`/api/v0/rag/collections/${collection.id}/sync-token/clear`);
			syncUrl = null;
			onnotice(t('rag-toast-sync-token-cleared'));
			await onchanged();
		} catch (error) {
			onnotice(String(error));
		}
	}

	async function removeCollection() {
		if (!confirm(t('rag-delete-collection-confirm', { name: collection.name }))) return;
		try {
			await adminDelete(`/api/v0/rag/collections/${collection.id}`);
			await onchanged();
		} catch (error) {
			onnotice(String(error));
		}
	}

	onMount(loadRefs);
</script>

<article class="card card-border bg-base-100">
	<div class="card-body gap-4">
		<div class="flex flex-col gap-3 sm:flex-row sm:items-start">
			<div class="min-w-0 flex-1">
				<div class="flex flex-wrap items-center gap-2">
					<h3 class="card-title text-base">{collection.name}</h3>
					<span class="badge badge-sm {badge(collection.status)}">{label(collection.status)}</span>
					{#if collection.search_mode === 'aggregate'}<span class="badge badge-secondary badge-sm">{t('rag-badge-aggregate')}</span>{/if}
					{#if collection.sync_hook_set}<span class="badge badge-ghost badge-sm">{t('rag-badge-sync-hook')}</span>{/if}
				</div>
				{#if collection.description}<p class="mt-1 text-sm text-base-content/70">{collection.description}</p>{/if}
				<p class="mt-1 break-all font-mono text-xs text-base-content/60">{collection.search_mode === 'aggregate' ? t('rag-meta-aggregate', { count: refs.length, hint: collection.pat_set ? t('rag-pat-set') : t('rag-pat-none') }) : t('rag-meta-versioned', { url: collection.git_url, hint: collection.git_ref })}</p>
				<div class="mt-2 flex flex-wrap gap-2 text-xs text-base-content/60">
					<span>{t('rag-embed-prefix')} {collection.embedding_model}</span>
					<span>·</span><span>{collection.pat_set ? t('rag-pat-set') : t('rag-pat-none')}</span>
					{#if collection.allowed_groups.length}<span>·</span><span>{t('rag-label-allowed-groups')}: {collection.allowed_groups.join(', ')}</span>{/if}
				</div>
			</div>
			<div class="flex flex-wrap gap-2">
				<a class="btn btn-sm" href="{base}/rag/{collection.id}/edit">{t('rag-button-edit')}</a>
				<button class="btn btn-sm" onclick={rotateSync}>{collection.sync_hook_set ? t('rag-button-sync-token-rotate') : t('rag-button-sync-token')}</button>
				{#if collection.sync_hook_set}<button class="btn btn-ghost btn-sm" onclick={clearSync}>{t('rag-button-sync-token-clear')}</button>{/if}
				<button class="btn btn-outline btn-error btn-sm" onclick={removeCollection}>{t('rag-button-delete-collection')}</button>
			</div>
		</div>

		{#if syncUrl}
			<div class="alert alert-success"><span class="min-w-0"><strong>{t('rag-sync-url-heading')}</strong><code class="mt-1 block break-all">curl -X POST "{syncUrl}"</code></span></div>
		{/if}
		{#if collection.connected_account}<div class="alert alert-info"><span>{t('rag-source-consent-connected')}: {collection.connected_account}</span></div>{/if}
		<ul class="list rounded-box border border-base-300">
			{#each refs as source (source.id)}
				{@const statusSource = collection.search_mode === 'aggregate' ? (refs.find((item) => item.is_primary) ?? source) : source}
				<li class="list-row border-b border-base-300 last:border-b-0">
					<div class="list-col-grow min-w-0">
						<div class="flex flex-wrap items-center gap-2">
							<span class="badge badge-sm {badge(statusSource.status)}">{label(statusSource.status)}</span>
							{#if source.is_primary && collection.search_mode !== 'aggregate'}<span class="badge badge-sm">{t('rag-badge-primary')}</span>{/if}
							<strong class="font-mono text-sm">{collection.search_mode === 'aggregate' && source.git_url ? `${sourceLabel(source.git_url)} @ ${source.git_ref}` : source.git_ref}</strong>
							<span class="badge badge-ghost badge-sm">{source.document_count ? t('rag-ref-files', { files: source.document_count }) : t('rag-badge-no-files')}</span>
						</div>
						{#if source.git_url}<p class="mt-1 break-all font-mono text-xs text-base-content/60">{source.git_url}</p>{/if}
						<p class="mt-1 text-xs text-base-content/60">{statusSource.last_indexed_at ? t('rag-ref-indexed-line', { date: dt(statusSource.last_indexed_at), commit: statusSource.last_indexed_commit?.slice(0, 12) ?? '—' }) : t('rag-never')}</p>
						{#if statusSource.last_error}<p class="mt-1 text-sm text-error">{statusSource.last_error}</p>{/if}
					</div>
					<div class="flex flex-wrap items-center justify-end gap-1">
						<button class="btn btn-ghost btn-xs" onclick={() => toggleLog(source)}>{t('rag-button-log')}</button>
						<button class="btn btn-ghost btn-xs" onclick={() => editSource(source)}>{t('rag-button-edit')}</button>
						<button class="btn btn-xs" onclick={() => post(`/api/v0/rag/collections/${collection.id}/refs/${source.id}/rebuild`, t('rag-toast-rebuild-queued'))}>{t('rag-button-reindex')}</button>
						{#if !source.is_primary && collection.search_mode !== 'aggregate'}<button class="btn btn-ghost btn-xs" onclick={() => post(`/api/v0/rag/collections/${collection.id}/refs/${source.id}/primary`)}>{t('rag-button-set-primary')}</button>{/if}
						<button class="btn btn-ghost btn-error btn-xs" onclick={() => removeSource(source)}>{t('rag-button-remove')}</button>
					</div>
					{#if openLog === source.id}
						<div class="list-col-wrap border-t border-base-300 pt-3">
							<h4 class="font-medium">{t('rag-log-heading')}</h4>
							{#if !(logs[source.id]?.length)}<p class="mt-2 text-sm text-base-content/60">{t('rag-log-empty')}</p>{:else}<ul class="mt-2 space-y-2">{#each logs[source.id] as entry (entry.id)}<li class="flex gap-2 text-xs"><span class="badge badge-xs {entry.level === 'error' ? 'badge-error' : entry.level === 'warn' ? 'badge-warning' : 'badge-ghost'}">{t(`rag-log-${entry.level}`)}</span><time class="shrink-0 text-base-content/50">{dt(entry.created_at)}</time><span>{entry.message}</span></li>{/each}</ul>{/if}
						</div>
					{/if}
				</li>
			{:else}
				<li class="list-row text-sm text-base-content/60">{t('rag-no-sources')}</li>
			{/each}
		</ul>

		<div class="flex justify-end">
			<button class="btn btn-sm" type="button" onclick={() => (addingSource = true)}>
				{collection.search_mode === 'aggregate' ? t('rag-button-add-source') : t('rag-button-add-ref')}
			</button>
		</div>
	</div>

	<!-- Editing one source is two fields; adding one is two more. Both were
	     inline forms that grew the card — they are dialogs now, like every
	     other short editor in the app. -->
	<EditModal
		bind:open={sourceEditorOpen}
		title={t('rag-edit-source-heading')}
		description={collection.name}
		cancellabel={t('rag-button-cancel')}
		savelabel={t('rag-button-save-source')}
		onsave={saveEditedSource}
	>
		<div class="grid grid-cols-1 gap-2 sm:grid-cols-2">
			<fieldset class="fieldset"><legend class="fieldset-legend">{collection.search_mode === 'aggregate' ? t('rag-label-git-url-source') : t('rag-label-git-url-inherit')}</legend><input class="input w-full font-mono" bind:value={refUrl} /></fieldset>
			<fieldset class="fieldset"><legend class="fieldset-legend">{t('rag-label-branch-tag')}</legend><input class="input w-full font-mono" bind:value={refName} /></fieldset>
		</div>
	</EditModal>

	<EditModal
		bind:open={addingSource}
		wide={collection.search_mode === 'aggregate'}
		title={t('rag-add-source-heading')}
		description={collection.name}
		cancellabel={t('rag-button-cancel')}
		footer="close"
	>
		<div class="flex flex-col gap-4">
			<div class="flex flex-wrap items-end gap-2">
				{#if collection.search_mode === 'aggregate'}<fieldset class="fieldset min-w-64 flex-1"><legend class="fieldset-legend">{t('rag-label-git-url-source')}</legend><input class="input input-sm w-full font-mono" bind:value={sourceUrl} placeholder={t('rag-placeholder-source-git-url')} /></fieldset>{/if}
				<fieldset class="fieldset min-w-48 flex-1"><legend class="fieldset-legend">{t('rag-label-branch-tag')}</legend><input class="input input-sm w-full font-mono" bind:value={sourceRef} placeholder={t('rag-placeholder-branch-tag-commit')} /></fieldset>
				<button class="btn btn-primary btn-sm" onclick={addSource} disabled={!sourceRef.trim() || (collection.search_mode === 'aggregate' && !sourceUrl.trim())}>{collection.search_mode === 'aggregate' ? t('rag-button-add-source') : t('rag-button-add-ref')}</button>
			</div>
			{#if collection.search_mode === 'aggregate'}
				<div class="flex flex-col gap-2 border-t border-base-300 pt-3 sm:flex-row sm:items-end"><fieldset class="fieldset flex-1"><legend class="fieldset-legend">{t('rag-button-add-bulk')}</legend><textarea class="textarea min-h-28 w-full font-mono text-xs" bind:value={sourceDraft} placeholder={t('rag-placeholder-bulk-sources')}></textarea><p class="label">{t('rag-add-sources-hint', { at: '@ref', ref: collection.git_ref })}</p></fieldset><button class="btn btn-sm" onclick={addSources} disabled={!sourceDraft.trim()}>{t('rag-button-add-bulk')}</button></div>
			{/if}
		</div>
	</EditModal>
</article>
