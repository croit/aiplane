<script lang="ts">
	import type { ChatAttachment } from '$lib/chat-protocol';
	import { n, t } from '$lib/i18n.svelte';

	let { attachments, removable = false, onremove }: {
		attachments: ChatAttachment[];
		removable?: boolean;
		onremove: (filename: string) => void;
	} = $props();
	let unavailable = $state<string[]>([]);
	const media = $derived(attachments.filter((attachment: ChatAttachment) => /^(image|video|audio)\//.test(attachment.mime)));

	function mediaKind(attachment: ChatAttachment): 'image' | 'video' | 'audio' | 'other' {
		if (attachment.mime.startsWith('image/')) return 'image';
		if (attachment.mime.startsWith('video/')) return 'video';
		if (attachment.mime.startsWith('audio/')) return 'audio';
		return 'other';
	}

	function mediaNumber(attachment: ChatAttachment): number {
		return media.filter((candidate: ChatAttachment) => mediaKind(candidate) === mediaKind(attachment)).indexOf(attachment) + 1;
	}

	function sizeLabel(size: number): string {
		return `${n(Math.max(1, Math.ceil(size / 1024)))} KB`;
	}

	function failed(attachment: ChatAttachment) {
		if (!unavailable.includes(attachment.url)) unavailable = [...unavailable, attachment.url];
	}
</script>

<div class="flex flex-wrap gap-2">
	{#each attachments as attachment (attachment.url)}
		<div class="group relative min-w-0">
			{#if unavailable.includes(attachment.url)}
				<div class="alert alert-warning py-2" title={t('render-attachment-unavailable-title')}><span>{attachment.filename} · {t('render-attachment-unavailable-meta')}</span></div>
			{:else if attachment.mime.startsWith('image/')}
				<a href={attachment.link ?? attachment.url} target="_blank" rel="noopener" title={t('render-attachment-open-title', { filename: attachment.filename, mime: attachment.mime, size: sizeLabel(attachment.size) })}>
					<img src={attachment.url} alt={attachment.filename} class="max-h-64 rounded-box object-contain" onerror={() => failed(attachment)} />
					{#if media.length > 1}<span class="mt-1 block text-center text-xs opacity-60">{t('render-media-label', { kind: mediaKind(attachment), n: mediaNumber(attachment) })}</span>{/if}
				</a>
			{:else if attachment.mime.startsWith('video/')}
				<!-- svelte-ignore a11y_media_has_caption -->
				<video src={attachment.url} controls class="max-h-64 max-w-full rounded-box" onerror={() => failed(attachment)}></video>
				{#if media.length > 1}<span class="mt-1 block text-center text-xs opacity-60">{t('render-media-label', { kind: mediaKind(attachment), n: mediaNumber(attachment) })}</span>{/if}
			{:else if attachment.mime.startsWith('audio/')}
				<audio src={attachment.url} controls class="max-w-full" onerror={() => failed(attachment)}></audio>
				{#if media.length > 1}<span class="mt-1 block text-center text-xs opacity-60">{t('render-media-label', { kind: mediaKind(attachment), n: mediaNumber(attachment) })}</span>{/if}
			{:else}
				<a href={attachment.url} class="btn btn-sm max-w-full" download={attachment.filename} title={t('render-attachment-title', { filename: attachment.filename, mime: attachment.mime, size: sizeLabel(attachment.size) })}><span class="truncate">{attachment.filename}</span><span class="opacity-60">{sizeLabel(attachment.size)}</span></a>
			{/if}
			{#if removable}
				<button class="btn btn-error btn-xs btn-circle absolute -right-2 -top-2 opacity-0 group-hover:opacity-100 focus:opacity-100" aria-label={t('render-attachment-remove-aria')} title={t('render-attachment-remove-title', { filename: attachment.filename })} onclick={() => onremove(attachment.filename)}>✕</button>
			{/if}
		</div>
	{/each}
</div>
