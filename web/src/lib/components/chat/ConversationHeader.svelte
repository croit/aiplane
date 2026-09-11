<script lang="ts">
	import { t } from '$lib/i18n.svelte';
	import SearchableSelect from '$lib/components/SearchableSelect.svelte';
	import { modelSelectOptions, type ChatModelOption } from '$lib/model-option';

	let {
		id,
		title,
		isOwner,
		shared,
		models,
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
		shared: boolean;
		models: ChatModelOption[];
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

	let modelOptions = $derived(modelSelectOptions(models, {
		gdpr: t('searchable-select-model-gdpr'),
		nda: t('searchable-select-model-nda')
	}));
	let transcriptionOptions = $derived(transcriptionModels.map((id) => ({ value: id, label: id })));
	let speechOptions = $derived([
		{ value: '', label: t('chat-render-tts-voice-default') },
		...speechVoices.map((voice) => ({ value: voice, label: voice }))
	]);
</script>

<header class="mb-4 flex min-w-0 items-center gap-2 border-b border-base-300 pb-3">
	<h1 class="hidden min-w-0 flex-1 truncate text-lg font-semibold sm:block">
		{title?.trim() || t('chat-render-new-conversation-fallback')}
	</h1>
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

		{#if isOwner}
			<div class="hidden min-w-0 items-center gap-2 sm:flex">
				{#if models.length > 0}
					<label class="flex min-w-0 items-center gap-1.5">
						<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true"><path d="M4 6h16M7 12h10M10 18h4M8 4v4m8 2v4m-4 2v4" /></svg>
						<SearchableSelect options={modelOptions} bind:value={model} ariaLabel={t('chat-render-model-aria')} size="sm" class="w-64" />
					</label>
				{:else}
					<input class="input input-bordered input-sm w-56" placeholder={t('chat-render-model-placeholder')} aria-label={t('chat-render-model-aria')} bind:value={model} />
				{/if}
				{#if transcriptionModels.length > 0}
					<label class="flex min-w-0 items-center gap-1.5">
						<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true"><rect x="9" y="2" width="6" height="11" rx="3" /><path d="M5 10v1a7 7 0 0 0 14 0v-1M12 18v3M8 22h8" /></svg>
						<SearchableSelect options={transcriptionOptions} bind:value={transcriptionModel} ariaLabel={t('chat-render-voice-model-aria')} size="sm" class="w-64" />
					</label>
				{/if}
				{#if speechAvailable && speechVoices.length >= 2}
					<SearchableSelect options={speechOptions} bind:value={speechVoice} ariaLabel={t('chat-render-tts-voice-aria')} size="sm" class="w-44" onchange={() => onspeechvoice()} />
				{/if}
			</div>
			{#if hasCanvas}<button class="btn btn-ghost btn-sm" onclick={oncanvas} aria-label={t('chat-render-canvas-toggle-title')} title={t('chat-render-canvas-toggle-title')}><span aria-hidden="true">▣</span><span class="hidden sm:inline">{t('chat-render-canvas-toggle-label')}</span></button>{/if}
			<button class="btn btn-ghost btn-sm" onclick={onshare} title={t('chat-render-share-tooltip')}>
				{shared ? t('chat-render-share-label-on') : t('chat-render-share-label-off')}
			</button>
		{:else}
			<button class="btn btn-ghost btn-sm" onclick={onfork} title={t('chat-render-fork-tooltip')}>{t('chat-render-fork-label')}</button>
		{/if}
	</div>
</header>
