<script lang="ts">
	import { renderMarkdown } from '$lib/markdown';
	import type { MarkdownImage } from '$lib/markdown';
	import { t } from '$lib/i18n.svelte';

	let { content, class: className = '', images = [], hiddenImageUrls }: {
		content: string | null | undefined;
		class?: string;
		images?: MarkdownImage[];
		hiddenImageUrls?: ReadonlySet<string>;
	} = $props();
	const html = $derived(renderMarkdown(content, { copy: t('render-code-copy'), copied: t('render-code-copied') }, { images, hiddenImageUrls }));

	async function copyCode(event: MouseEvent) {
		const button = (event.target as HTMLElement).closest<HTMLButtonElement>('[data-code-copy]');
		if (!button) return;
		const code = button.parentElement?.querySelector('code')?.textContent ?? '';
		await navigator.clipboard.writeText(code.replace(/\n$/, ''));
		const previous = button.textContent;
		button.textContent = button.dataset.copiedLabel ?? previous;
		window.setTimeout(() => (button.textContent = previous), 1500);
	}

	function copyControls(node: HTMLElement) {
		node.addEventListener('click', copyCode);
		return { destroy: () => node.removeEventListener('click', copyCode) };
	}
</script>

<!-- eslint-disable-next-line svelte/no-at-html-tags -->
<div class={className} use:copyControls>{@html html}</div>
