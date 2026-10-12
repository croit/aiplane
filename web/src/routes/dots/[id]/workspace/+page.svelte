<script lang="ts">
	import { page } from '$app/state';
	import { onMount, onDestroy } from 'svelte';
	import { ApiError } from '#lib/api.js';
	import { dotWorkspaceApi, type DotWorkspaceList, type DotWorkspaceEntry } from '#lib/dots.js';
	import Modal from '#lib/components/ui/Modal.svelte';
	import { dt, n, t } from '#lib/i18n.svelte.js';

	const dotId = page.params.id ?? '';
	const requestedPath = page.url.searchParams.get('path') ?? '';
	let data = $state<DotWorkspaceList | null>(null);
	let loading = $state(true);
	let saving = $state(false);
	let authorityValid = $state(false);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let file = $state<File | null>(null);
	let fileInput = $state<HTMLInputElement | null>(null);
	let creatingFolder = $state(false);
	let folderName = $state('');
	let formError = $state<string | null>(null);
	let epoch = 0;
	let disposed = false;
	const writable = $derived(authorityValid && data?.available === true && data.busy === false && !loading && !saving);
	const currentPath = $derived(data?.path ?? requestedPath);
	const crumbs = $derived(currentPath.split('/').filter(Boolean).map((name, index, segments) => ({ name, path: segments.slice(0, index + 1).join('/') })));
	const entryPath = (name: string) => currentPath ? `${currentPath}/${name}` : name;
	const directoryUrl = (path: string) => `/dots/${encodeURIComponent(dotId)}/workspace${path ? `?${new URLSearchParams({ path })}` : ''}`;
	const message = (caught: unknown, key: string) => caught instanceof ApiError && caught.serverMessage ? caught.serverMessage : t(key);

	function clearPrivateData() {
		data = null;
		file = null;
		if (fileInput) fileInput.value = '';
		creatingFolder = false;
		folderName = '';
		formError = null;
		notice = null;
	}

	async function reload(more = false) {
		const mine = ++epoch;
		const cursor = more ? data?.next_cursor : null;
		loading = true;
		authorityValid = false;
		try {
			const result = await dotWorkspaceApi.list(dotId, requestedPath, cursor);
			if (disposed || mine !== epoch) return;
			data = more && data?.entries && result.entries ? { ...result, entries: [...data.entries, ...result.entries] } : result;
			authorityValid = true;
			error = null;
		} catch (caught) {
			if (disposed || mine !== epoch) return;
			if (caught instanceof ApiError && [401, 403, 404].includes(caught.status)) clearPrivateData();
			error = message(caught, 'dot-workspace-error-load');
		} finally {
			if (!disposed && mine === epoch) loading = false;
		}
	}

	async function change(operation: () => Promise<void>, succeeded?: () => void) {
		if (!writable) return;
		const mine = epoch;
		saving = true;
		error = null;
		formError = null;
		try {
			await operation();
			if (disposed || mine !== epoch) return;
			succeeded?.();
			notice = t('dot-workspace-notice-saved');
			await reload();
		} catch (caught) {
			if (disposed || mine !== epoch) return;
			authorityValid = false;
			if (caught instanceof ApiError && [401, 403].includes(caught.status)) clearPrivateData();
			error = message(caught, 'dot-workspace-error-save');
			formError = error;
		} finally {
			if (!disposed) saving = false;
		}
	}

	async function upload() {
		if (!file || !writable) return;
		if (file.size > 50 * 1024 * 1024) { formError = t('dot-workspace-upload-limit'); return; }
		const selected = file;
		await change(() => dotWorkspaceApi.upload(dotId, entryPath(selected.name), selected), () => {
			file = null;
			if (fileInput) fileInput.value = '';
		});
	}

	async function createFolder() {
		const name = folderName.trim();
		if (!name || !writable) return;
		await change(() => dotWorkspaceApi.createFolder(dotId, entryPath(name)), () => {
			creatingFolder = false;
			folderName = '';
		});
	}

	async function remove(entry: DotWorkspaceEntry) {
		if (!writable || !window.confirm(t('dot-workspace-delete-confirm', { name: entry.name }))) return;
		await change(() => dotWorkspaceApi.remove(dotId, entryPath(entry.name)));
	}

	onMount(() => void reload());
	onDestroy(() => { disposed = true; epoch += 1; });
</script>

<div class="w-full min-w-0 space-y-5">
	<header class="flex flex-wrap items-start justify-between gap-3">
		<div class="min-w-0">
			<a class="link link-hover text-sm" href="/dots">{t('dots-heading')}</a>
			<h1 class="mt-2 text-2xl font-bold">{t('dot-workspace-heading')}</h1>
			{#if data}<p class="mt-1 break-all font-medium">{data.dot_name}</p>{/if}
			<p class="mt-2 max-w-3xl text-sm text-base-content/60">{t('dot-workspace-intro')}</p>
		</div>
		<button class="btn btn-sm" disabled={loading || saving} onclick={() => void reload()}>{t('dot-workspace-reload')}</button>
	</header>
	{#if error}<div class="alert alert-error" role="alert"><span>{error}</span></div>{/if}
	{#if notice}<div class="alert alert-success" role="status"><span>{notice}</span><button class="btn btn-ghost btn-xs" aria-label={t('ui-close')} onclick={() => notice = null}>✕</button></div>{/if}
	{#if loading && !data}
		<div class="skeleton h-28 w-full"></div>
	{:else if data}
		{#if !data.available}
			<div class="alert alert-warning" role="status"><span>{t('dot-workspace-unavailable')}</span></div>
		{:else}
			{#if data.busy}<div class="alert alert-warning" role="status"><span>{t('dot-workspace-busy')}</span></div>{/if}
			<nav class="breadcrumbs max-w-full text-sm" aria-label={t('dot-workspace-heading')}>
				<ul><li><a href={directoryUrl('')}>{t('dot-workspace-root')}</a></li>{#each crumbs as crumb}<li><a href={directoryUrl(crumb.path)}>{crumb.name}</a></li>{/each}</ul>
			</nav>
			<div class="card card-border"><div class="card-body gap-3 p-4">
				<form class="flex min-w-0 flex-wrap items-end gap-3" onsubmit={(event) => { event.preventDefault(); void upload(); }}>
					<label class="flex min-w-0 flex-1 flex-col gap-1"><span class="label-text">{t('dot-workspace-file')}</span><input bind:this={fileInput} type="file" class="file-input file-input-sm w-full" disabled={!writable} onchange={(event) => { file = event.currentTarget.files?.[0] ?? null; formError = null; }} /></label>
					<button class="btn btn-sm" type="submit" disabled={!writable || !file}>{t('dot-workspace-upload')}</button>
					<button class="btn btn-sm" type="button" disabled={!writable} onclick={() => { formError = null; creatingFolder = true; }}>{t('dot-workspace-create-folder')}</button>
				</form>
				{#if formError && !creatingFolder}<p class="text-sm text-error" role="alert">{formError}</p>{/if}
			</div></div>
			{#if data.entries !== null}
				{#if data.entries.length === 0}<div class="card card-border"><div class="card-body text-sm text-base-content/60">{t('dot-workspace-empty')}</div></div>{:else}
					<ul class="grid min-w-0 gap-3 sm:grid-cols-2 xl:grid-cols-3">
						{#each data.entries as entry (entry.name)}
							<li class="card card-border min-w-0"><div class="card-body min-w-0 gap-2 p-4">
								{#if entry.kind === 'directory'}<a class="link link-hover break-all text-sm font-semibold" href={directoryUrl(entryPath(entry.name))}>{entry.name}</a>{:else}<strong class="break-all text-sm">{entry.name}</strong>{/if}
								<p class="text-xs text-base-content/60">{entry.kind === 'directory' ? t('dot-workspace-folder') : entry.size_bytes === null ? t('dot-workspace-size-missing') : `${n(entry.size_bytes)} B`}</p>
								{#if entry.modified_at !== null}<p class="text-xs text-base-content/60">{t('dot-workspace-modified', { when: dt(entry.modified_at) })}</p>{/if}
								<div class="card-actions flex-wrap justify-end">
									{#if entry.kind === 'file'}<a class="btn btn-ghost btn-sm" href={dotWorkspaceApi.downloadUrl(dotId, entryPath(entry.name))} download={entry.name}>{t('chat-render-canvas-asset-download')}</a>{/if}
									<button class="btn btn-ghost btn-sm" disabled={!writable} onclick={() => void remove(entry)}>{t('dot-workspace-delete')}</button>
								</div>
							</div></li>
						{/each}
					</ul>
				{/if}
			{/if}
			{#if data.next_cursor !== null}<button class="btn btn-sm" disabled={loading || saving} onclick={() => void reload(true)}>{t('dot-workspace-more')}</button>{/if}
		{/if}
	{/if}
</div>

{#snippet folderActions()}
	<button class="btn btn-ghost" type="button" disabled={saving} onclick={() => creatingFolder = false}>{t('dots-create-cancel')}</button>
	<button class="btn" type="submit" form="workspace-create-folder" disabled={!writable || !folderName.trim()}>{t('dot-workspace-create-folder')}</button>
{/snippet}

<Modal bind:open={creatingFolder} title={t('dot-workspace-create-folder')} footer={folderActions}>
	<form id="workspace-create-folder" class="space-y-3" onsubmit={(event) => { event.preventDefault(); void createFolder(); }}>
		{#if formError}<div class="alert alert-error" role="alert"><span>{formError}</span></div>{/if}
		<label class="flex flex-col gap-1"><span class="label-text">{t('dot-workspace-folder-name')}</span><input class="input w-full" bind:value={folderName} disabled={saving} required /></label>
	</form>
</Modal>
