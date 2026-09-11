<script lang="ts">
	import { t } from '$lib/i18n.svelte';

	type Model = { id: string; gdpr: boolean; nda: boolean };

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
		models: Model[];
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

	function modelLabel(option: Model): string {
		if (!option.gdpr && !option.nda) {
			return t('chat-render-model-non-gdpr-confidential', { id: option.id });
		}
		if (!option.gdpr) return t('chat-render-model-non-gdpr', { id: option.id });
		if (!option.nda) return t('chat-render-model-confidential', { id: option.id });
		return option.id;
	}
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
						<select class="select select-bordered select-sm w-56" aria-label={t('chat-render-model-aria')} bind:value={model}>
							{#each models as option (option.id)}<option value={option.id}>{modelLabel(option)}</option>{/each}
						</select>
					</label>
				{:else}
					<input class="input input-bordered input-sm w-56" placeholder={t('chat-render-model-placeholder')} aria-label={t('chat-render-model-aria')} bind:value={model} />
				{/if}
				{#if transcriptionModels.length > 0}
					<label class="flex min-w-0 items-center gap-1.5">
						<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true"><rect x="9" y="2" width="6" height="11" rx="3" /><path d="M5 10v1a7 7 0 0 0 14 0v-1M12 18v3M8 22h8" /></svg>
						<select class="select select-bordered select-sm w-56" aria-label={t('chat-render-voice-model-aria')} bind:value={transcriptionModel}>
							{#each transcriptionModels as option (option)}<option value={option}>{option}</option>{/each}
						</select>
					</label>
				{/if}
				{#if speechAvailable && speechVoices.length >= 2}
					<select class="select select-bordered select-sm w-44" aria-label={t('chat-render-tts-voice-aria')} bind:value={speechVoice} onchange={onspeechvoice}>
						<option value="">{t('chat-render-tts-voice-default')}</option>
						{#each speechVoices as option (option)}<option value={option}>{option}</option>{/each}
					</select>
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
