<script lang="ts">
	import { n, t } from '#lib/i18n.svelte.js';
	import SearchableSelect from '#lib/components/SearchableSelect.svelte';
	import { modelSelectOptions, type ChatModelOption } from '#lib/model-option.js';
	import { formatModelPrice } from '#lib/model-price.js';
	import { causeMessage, levelBadgeClass, type ChatClassification } from '#lib/classification.js';

	let {
		id,
		title,
		isOwner,
		canWrite,
		canFork,
		shared,
		linkCopied,
		classification,
		models,
		currency,
		model = $bindable(),
		transcriptionModels,
		transcriptionModel = $bindable(),
		speechAvailable,
		speechVoices,
		speechVoice = $bindable(),
		hasCanvas,
		oncanvas,
		onshare,
		onfork,
		onspeechvoice
	}: {
		id: string;
		title: string | null | undefined;
		isOwner: boolean;
		canWrite: boolean;
		canFork: boolean;
		shared: boolean;
		linkCopied: boolean;
		classification: ChatClassification | null;
		models: ChatModelOption[];
		currency: string;
		model: string;
		transcriptionModels: string[];
		transcriptionModel: string;
		speechAvailable: boolean;
		speechVoices: string[];
		speechVoice: string;
		hasCanvas: boolean;
		oncanvas: () => void;
		onshare: () => void;
		onfork: () => void;
		onspeechvoice: () => void;
	} = $props();

	const cause = $derived(causeMessage(classification?.cause));
	let modelOptions = $derived(modelSelectOptions(
			models,
			(price) => formatModelPrice(price, currency, t, n),
			(levelId) => classification?.levels.find((l) => l.id === levelId)?.name ?? null
		));
	let transcriptionOptions = $derived(transcriptionModels.map((id) => ({ value: id, label: id })));
	let speechOptions = $derived([
		{ value: '', label: t('chat-render-tts-voice-default') },
		...speechVoices.map((voice) => ({ value: voice, label: voice }))
	]);
</script>

<header class="mb-4 flex min-w-0 flex-wrap items-center gap-2 border-b border-base-300 pb-3">
	<h1 class="hidden min-w-0 flex-1 truncate text-lg font-semibold sm:block">
		{title?.trim() || t('chat-render-new-conversation-fallback')}
	</h1>
	{#if classification?.active && classification.level}
		<span class={levelBadgeClass(classification.level.color)} title={cause ? t(cause.key, cause.args) : undefined} data-classification-badge>{classification.level.name}</span>
	{/if}
	{#if canWrite}
		<div class="order-last flex w-full min-w-0 items-center gap-2 sm:order-none sm:w-auto">
			{#if models.length > 0}
				<label class="flex min-w-0 flex-1 items-center gap-1.5 sm:flex-none">
					<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true"><path d="M4 6h16M7 12h10M10 18h4M8 4v4m8 2v4m-4 2v4" /></svg>
					<SearchableSelect options={modelOptions} bind:value={model} ariaLabel={t('chat-render-model-aria')} size="sm" class="w-full sm:w-64" />
				</label>
			{:else}
				<input class="input input-sm w-full sm:w-56" placeholder={t('chat-render-model-placeholder')} aria-label={t('chat-render-model-aria')} bind:value={model} />
			{/if}
			{#if transcriptionModels.length > 0}
				<label class="flex min-w-0 flex-1 items-center gap-1.5 sm:flex-none">
					<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true"><rect x="9" y="2" width="6" height="11" rx="3" /><path d="M5 10v1a7 7 0 0 0 14 0v-1M12 18v3M8 22h8" /></svg>
					<SearchableSelect options={transcriptionOptions} bind:value={transcriptionModel} ariaLabel={t('chat-render-voice-model-aria')} size="sm" class="w-full sm:w-64" />
				</label>
			{/if}
			{#if speechAvailable && speechVoices.length >= 2}
				<SearchableSelect options={speechOptions} bind:value={speechVoice} ariaLabel={t('chat-render-tts-voice-aria')} size="sm" class="min-w-0 flex-1 sm:w-44 sm:flex-none" onchange={() => onspeechvoice()} />
			{/if}
		</div>
	{/if}
	<div class="ml-auto flex min-w-0 items-center gap-2">
		<div class="dropdown dropdown-end">
			<button
				class="btn btn-ghost btn-sm gap-1"
				popovertarget="export-menu"
				style="anchor-name:--export"
				aria-label={t('chat-render-export-aria')}
				title={t('chat-render-export-tooltip')}
			>
				<svg viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true"><path d="M12 3v12m0 0 4-4m-4 4-4-4M5 15v4h14v-4" /></svg>
				<span class="hidden sm:inline">{t('chat-render-export-label')}</span>
			</button>
			<ul class="dropdown-content menu z-10 w-40 rounded-box bg-base-200 p-2 shadow" popover id="export-menu" style="position-anchor:--export">
				<li><a href="/api/v0/chat/sessions/{id}/export.md" download>{t('chat-render-export-md')}</a></li>
				<li><a href="/api/v0/chat/sessions/{id}/export.pdf" download>{t('chat-render-export-pdf')}</a></li>
			</ul>
		</div>

		{#if isOwner && hasCanvas}<button class="btn btn-ghost btn-sm" onclick={oncanvas} aria-label={t('chat-render-canvas-toggle-title')} title={t('chat-render-canvas-toggle-title')}><span aria-hidden="true">▣</span><span class="hidden sm:inline">{t('chat-render-canvas-toggle-label')}</span></button>{/if}
		{#if canWrite}
			<button class="btn btn-ghost btn-sm" onclick={onshare} title={t('chat-render-share-tooltip')}>
				{linkCopied ? t('chat-render-share-link-copied') : shared ? t('chat-render-share-label-on') : t('chat-render-share-label-off')}
			</button>
		{:else if canFork}
			<button class="btn btn-ghost btn-sm" onclick={onfork} title={t('chat-render-fork-tooltip')}>{t('chat-render-fork-label')}</button>
		{/if}
	</div>
</header>
