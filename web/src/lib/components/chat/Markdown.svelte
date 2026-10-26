<script lang="ts">
	import { renderMarkdown } from '#lib/markdown.js';
	import type { MarkdownImage } from '#lib/markdown.js';
	import { t } from '#lib/i18n.svelte.js';

	let { content, class: className = '', images = [], hiddenImageUrls, prompts = false, onprompt }: {
		content: string | null | undefined;
		class?: string;
		images?: MarkdownImage[];
		hiddenImageUrls?: ReadonlySet<string>;
		prompts?: boolean;
		onprompt?: (text: string) => void;
	} = $props();
	const html = $derived(renderMarkdown(content, { copy: t('render-code-copy'), copied: t('render-code-copied') }, { images, hiddenImageUrls, prompts }));

	async function copyCode(button: HTMLButtonElement) {
		const code = button.parentElement?.querySelector('code')?.textContent ?? '';
		await navigator.clipboard.writeText(code.replace(/\n$/, ''));
		const previous = button.textContent;
		button.textContent = button.dataset.copiedLabel ?? previous;
		window.setTimeout(() => (button.textContent = previous), 1500);
	}

	function clickControls(node: HTMLElement) {
		function onclick(event: MouseEvent) {
			const chip = (event.target as HTMLElement).closest<HTMLButtonElement>('button[data-prompt]');
			if (chip) {
				const text = chip.dataset.prompt ?? '';
				if (onprompt && text.trim()) onprompt(text);
				return;
			}
			const button = (event.target as HTMLElement).closest<HTMLButtonElement>('[data-code-copy]');
			if (button) void copyCode(button);
		}
		node.addEventListener('click', onclick);
		return { destroy: () => node.removeEventListener('click', onclick) };
	}
</script>

<!-- eslint-disable-next-line svelte/no-at-html-tags -->
<div class={className} use:clickControls>{@html html}</div>
