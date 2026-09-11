<script lang="ts">
	import { onMount } from 'svelte';
	import { api } from '$lib/api';
	import type { CanvasDocument, ChatAsset } from '$lib/api';
	import { n, t } from '$lib/i18n.svelte';
	import Markdown from './Markdown.svelte';

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
	let canvasWidth = $state(672);
	let canvasMaxWidth = $state(1024);
	const minimumCanvasWidth = 320;
	const canvasWidthStorageKey = 'chat-canvas-width';

	onMount(() => {
		canvasMaxWidth = Math.max(minimumCanvasWidth, Math.floor(window.innerWidth * 0.7));
		const stored = Number(localStorage.getItem(canvasWidthStorageKey));
		canvasWidth = clampCanvasWidth(Number.isFinite(stored) && stored > 0 ? stored : Math.min(672, Math.round(window.innerWidth * 0.42)));
	});

	$effect(() => {
		if (documents.length === 0) {
			tab = 'assets';
			selectedId = '';
			opened = null;
			return;
		}
		if (!documents.some((document: CanvasDocument) => document.id === selectedId)) {
			tab = 'document';
			selectedId = documents[0].id;
			void openDocument(selectedId);
		}
	});

	async function openDocument(documentId: string, version?: number) {
		selectedId = documentId;
		try {
			opened = await api.getChatDocument(id, documentId, version);
			draft = opened.version.content;
			editing = false;
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

	function clampCanvasWidth(width: number) {
		return Math.min(canvasMaxWidth, Math.max(minimumCanvasWidth, Math.round(width)));
	}

	function setCanvasWidth(width: number) {
		canvasWidth = clampCanvasWidth(width);
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

<aside aria-label={t('chat-render-canvas-toggle-label')} class="fixed inset-0 z-40 flex min-w-0 flex-col border-l border-base-300 bg-base-100 md:relative md:z-auto md:w-[var(--canvas-width)] md:shrink-0" style:--canvas-width={`${canvasWidth}px`}>
	<button
		type="button"
		role="slider"
		aria-orientation="vertical"
		aria-label={t('render-canvas-resize-aria')}
		aria-valuemin={minimumCanvasWidth}
		aria-valuemax={canvasMaxWidth}
		aria-valuenow={canvasWidth}
		class="absolute inset-y-0 left-0 z-10 hidden w-2 -translate-x-1/2 cursor-col-resize focus:outline-2 focus:outline-primary md:block"
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
				<select class="select select-bordered select-sm w-full" value={selectedId} onchange={(event) => openDocument((event.currentTarget as HTMLSelectElement).value)}>
					{#each documents as document (document.id)}<option value={document.id}>{document.title} · v{document.current_ver}</option>{/each}
				</select>
			{/if}
			{#if opened}
				<div class="flex items-center gap-2">
					<h2 class="text-base font-semibold">{opened.document.title}</h2>
					<select class="select select-ghost select-xs w-auto" aria-label={t('render-canvas-version-aria')} value={opened.version.version} onchange={(event) => openDocument(opened?.document.id ?? '', Number((event.currentTarget as HTMLSelectElement).value))}>
						{#each opened.history as revision (revision.version)}<option value={revision.version}>v{revision.version} · {revision.author === 'user' ? t('render-canvas-version-by-you') : revision.summary}</option>{/each}
					</select>
					<span class="text-xs opacity-60">{t('chat-render-revision-count', { count: opened.history.length })}</span>
					{#if opened.version.author === 'user'}<span class="badge badge-outline badge-sm">{t('render-canvas-hand-edited')}</span>{/if}
					<span class="flex-1"></span>
					{#if isOwner}
						{#if editing}
							<button class="btn btn-ghost btn-sm" onclick={() => { editing = false; draft = opened?.version.content ?? ''; }}>{t('render-canvas-cancel')}</button>
							<button class="btn btn-primary btn-sm" disabled={saving} onclick={save}>{saving ? t('chat-render-canvas-saving') : t('render-canvas-save')}</button>
						{:else}
							<button class="btn btn-ghost btn-sm" onclick={() => (editing = true)}>{t('render-canvas-edit-button')}</button>
						{/if}
					{/if}
				</div>
				{#if editing}
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
				<div class="grid grid-cols-1 gap-3 lg:grid-cols-2">
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
