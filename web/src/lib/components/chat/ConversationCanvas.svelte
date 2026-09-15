<script lang="ts">
	import { onMount } from 'svelte';
	import { api } from '$lib/api';
	import type { CanvasDocument, ChatAsset } from '$lib/api';
	import { canvasBounds, clampCanvasWidth } from '$lib/canvas-layout';
	import { canvasRefresh } from '$lib/canvas-refresh';
	import { n, t } from '$lib/i18n.svelte';
	import Markdown from './Markdown.svelte';
	import SearchableSelect from '$lib/components/SearchableSelect.svelte';

	let { id, documents, assets, isOwner, onclose, onerror }: {
		id: string;
		documents: CanvasDocument[];
		assets: ChatAsset[];
		isOwner: boolean;
		onclose: () => void;
		onerror: (message: string) => void;
	} = $props();
	let tab = $state<'document' | 'assets'>('document');
	let selectedId = $state('');
	let opened = $state<Awaited<ReturnType<typeof api.getChatDocument>> | null>(null);
	let draft = $state('');
	let editing = $state(false);
	let saving = $state(false);
	/** Set when a newer version landed while a hand edit was open. */
	let newerVersion = $state<number | null>(null);
	/**
	 * The read the panel is waiting on, as `id@version`. Deliberately NOT
	 * `$state`: the reconciling effect writes `selectedId`, which re-runs it
	 * before the fetch it just started can land, and a tracked guard would
	 * simply re-run it again. A plain variable stops the duplicate request.
	 */
	let inFlight: string | null = null;
	const initialBounds = canvasBounds(1200);
	let panel: HTMLElement;
	let bounds = $state(initialBounds);
	let canvasWidth = $state(initialBounds.preferred);
	const canvasWidthStorageKey = 'chat-canvas-width';
	let documentOptions = $derived(documents.map((document) => ({ value: document.id, label: document.title, description: `v${document.current_ver}` })));
	let versionOptions = $derived((opened?.history ?? []).map((revision) => ({
		value: String(revision.version),
		label: `v${revision.version}`,
		description: revision.author === 'user' ? t('render-canvas-version-by-you') : revision.summary
	})));

	onMount(() => {
		const container = panel.parentElement;
		if (!container) return;
		let initialized = false;
		const resize = () => {
			bounds = canvasBounds(container.clientWidth);
			if (!initialized) {
				const stored = Number(localStorage.getItem(canvasWidthStorageKey));
				canvasWidth = clampCanvasWidth(Number.isFinite(stored) && stored > 0 ? stored : bounds.preferred, bounds);
				localStorage.setItem(canvasWidthStorageKey, String(canvasWidth));
				initialized = true;
			} else {
				canvasWidth = clampCanvasWidth(canvasWidth, bounds);
			}
		};
		const observer = new ResizeObserver(resize);
		observer.observe(container);
		resize();
		return () => observer.disconnect();
	});

	// The document list is re-read whenever the conversation reports a change
	// (a `sidebar_changed` event, which every canvas tool call pushes). That is
	// also the only cue the panel gets that the *open* document grew, so it
	// reconciles what it is showing against the list here rather than waiting
	// for a page reload.
	$effect(() => {
		const next = canvasRefresh({
			documents,
			selectedId,
			opened: opened && {
				id: opened.document.id,
				shownVersion: opened.version.version,
				knownHead: opened.document.current_ver
			},
			editing
		});
		if (next.action === 'none') return;
		if (next.action === 'clear') {
			tab = 'assets';
			selectedId = '';
			opened = null;
			newerVersion = null;
			return;
		}
		if (next.action === 'notify') {
			newerVersion = next.version;
			return;
		}
		// Switching documents (or opening the first one) also brings the tab
		// forward; a document that merely grew must not yank someone off the
		// assets tab.
		if (opened?.document.id !== next.documentId) tab = 'document';
		const key = `${next.documentId}@${next.version ?? 'head'}`;
		if (inFlight === key) return;
		inFlight = key;
		selectedId = next.documentId;
		void openDocument(next.documentId, next.version).finally(() => {
			if (inFlight === key) inFlight = null;
		});
	});

	async function openDocument(documentId: string, version?: number) {
		selectedId = documentId;
		try {
			opened = await api.getChatDocument(id, documentId, version);
			draft = opened.version.content;
			editing = false;
			newerVersion = null;
		} catch (error) {
			onerror(String(error));
		}
	}

	async function save() {
		if (!opened) return;
		saving = true;
		try {
			await api.editChatDocument(id, opened.document.id, draft);
			await openDocument(opened.document.id);
		} catch (error) {
			onerror(String(error));
		} finally {
			saving = false;
		}
	}

	function setCanvasWidth(width: number) {
		canvasWidth = clampCanvasWidth(width, bounds);
		localStorage.setItem(canvasWidthStorageKey, String(canvasWidth));
	}

	function beginResize(event: PointerEvent) {
		const startingX = event.clientX;
		const startingWidth = canvasWidth;
		const move = (moveEvent: PointerEvent) => setCanvasWidth(startingWidth + startingX - moveEvent.clientX);
		const finish = () => {
			window.removeEventListener('pointermove', move);
			window.removeEventListener('pointerup', finish);
		};
		window.addEventListener('pointermove', move);
		window.addEventListener('pointerup', finish);
		event.preventDefault();
	}

	function resizeFromKeyboard(event: KeyboardEvent) {
		if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return;
		setCanvasWidth(canvasWidth + (event.key === 'ArrowLeft' ? 24 : -24));
		event.preventDefault();
	}
</script>

<aside bind:this={panel} aria-label={t('chat-render-canvas-toggle-label')} class="fixed inset-0 z-40 flex min-w-0 flex-col border-l border-base-300 bg-base-100 xl:relative xl:z-auto xl:w-[var(--canvas-width)] xl:shrink-0" style:--canvas-width={`${canvasWidth}px`}>
	<button
		type="button"
		role="slider"
		aria-orientation="vertical"
		aria-label={t('render-canvas-resize-aria')}
		aria-valuemin={bounds.minimum}
		aria-valuemax={bounds.maximum}
		aria-valuenow={canvasWidth}
		class="absolute inset-y-0 left-0 z-10 hidden w-2 -translate-x-1/2 cursor-col-resize focus:outline-2 focus:outline-primary xl:block"
		onpointerdown={beginResize}
		onkeydown={resizeFromKeyboard}
	></button>
	<div class="tabs tabs-border flex items-center border-b border-base-300 px-2">
		<button class="tab {tab === 'document' ? 'tab-active' : ''}" disabled={documents.length === 0} onclick={() => (tab = 'document')}>{t('chat-render-canvas-document-tab')}</button>
		<button class="tab {tab === 'assets' ? 'tab-active' : ''}" onclick={() => (tab = 'assets')}>{t('chat-render-canvas-assets-tab')} <span class="badge badge-sm ms-1">{assets.length}</span></button>
		<span class="flex-1"></span>
		<button class="btn btn-ghost btn-sm btn-circle" onclick={onclose} aria-label={t('chat-render-canvas-close-title')} title={t('chat-render-canvas-close-title')}>✕</button>
	</div>

	{#if tab === 'document'}
		<div class="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto p-4">
			{#if documents.length > 1}
				<SearchableSelect options={documentOptions} value={selectedId} onchange={openDocument} ariaLabel={t('chat-render-canvas-document-tab')} size="sm" class="w-full" />
			{/if}
			{#if opened}
				<div class="flex items-center gap-2">
					<h2 class="text-base font-semibold">{opened.document.title}</h2>
					<SearchableSelect options={versionOptions} value={String(opened.version.version)} onchange={(value) => openDocument(opened?.document.id ?? '', Number(value))} ariaLabel={t('render-canvas-version-aria')} size="xs" class="w-48" />
					<span class="text-xs opacity-60">{t('chat-render-revision-count', { count: opened.history.length })}</span>
					{#if opened.version.author === 'user'}<span class="badge badge-outline badge-sm">{t('render-canvas-hand-edited')}</span>{/if}
					<span class="flex-1"></span>
					{#if isOwner}
						{#if editing}
							<button class="btn btn-ghost btn-sm" onclick={() => { editing = false; newerVersion = null; draft = opened?.version.content ?? ''; }}>{t('render-canvas-cancel')}</button>
							<button class="btn btn-primary btn-sm" disabled={saving} onclick={save}>{saving ? t('chat-render-canvas-saving') : t('render-canvas-save')}</button>
						{:else}
							<button class="btn btn-ghost btn-sm" onclick={() => (editing = true)}>{t('render-canvas-edit-button')}</button>
						{/if}
					{/if}
				</div>
				{#if editing}
					{#if newerVersion !== null}
						<div class="alert alert-warning py-2 text-sm">
							<span>{t('render-canvas-newer-version', { version: newerVersion })}</span>
							<button class="btn btn-ghost btn-xs" onclick={() => { editing = false; newerVersion = null; void openDocument(selectedId); }}>{t('render-canvas-load-newer')}</button>
						</div>
					{/if}
					<p class="text-xs opacity-60">{t('render-canvas-edit-hint')}</p>
					<textarea class="textarea textarea-bordered min-h-96 w-full flex-1 font-mono text-sm" bind:value={draft}></textarea>
				{:else}
					<Markdown content={opened.version.content} class="prose prose-sm max-w-none overflow-x-auto" />
				{/if}
			{/if}
		</div>
	{:else}
		<div class="min-h-0 flex-1 overflow-y-auto p-4">
			<div class="mb-3 flex items-center gap-2"><h2 class="text-base font-semibold">{t('chat-render-canvas-assets-heading')}</h2><span class="text-xs opacity-60">{t('chat-render-canvas-assets-count', { count: assets.length })}</span></div>
			{#if assets.length === 0}
				<div class="alert"><span>{t('chat-render-canvas-assets-empty')}</span></div>
			{:else}
				<div class="grid grid-cols-[repeat(auto-fit,minmax(min(100%,18rem),1fr))] gap-3">
					{#each assets as asset (asset.id)}
						<div class="card border border-base-300 bg-base-100">
							{#if asset.mime.startsWith('image/')}<img src={asset.url} alt={asset.filename} class="max-h-56 w-full rounded-t-box object-contain" />{:else if asset.mime.startsWith('video/')}<!-- svelte-ignore a11y_media_has_caption --><video src={asset.url} controls class="max-h-56 w-full rounded-t-box"></video>{:else if asset.mime.startsWith('audio/')}<audio src={asset.url} controls class="mt-4 w-full px-3"></audio>{/if}
							<div class="card-body gap-1 p-3"><strong class="truncate text-sm" title={asset.filename}>{asset.filename}</strong><span class="text-xs opacity-60">{asset.mime} · {n(Math.ceil(asset.size / 1024))} KB</span><div class="card-actions justify-end"><a class="btn btn-ghost btn-sm" href={asset.url} download={asset.filename}>{t('chat-render-canvas-asset-download')}</a></div></div>
						</div>
					{/each}
				</div>
			{/if}
		</div>
	{/if}
</aside>
