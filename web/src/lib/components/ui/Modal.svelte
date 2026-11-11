<script lang="ts">
	import type { Snippet } from 'svelte';
	import { t } from '#lib/i18n.svelte.js';

	/**
	 * The centred dialog every modal in the app builds on: a real `<dialog>`
	 * driven by the bindable `open`, closed by Escape, the backdrop or the
	 * header's close button, all of which flow back into `open` through
	 * `onclose` so the caller never desyncs. Nothing inside renders while
	 * closed, so a list of rows can each own one without mounting them all.
	 */
	let {
		open = $bindable(false),
		title,
		description = null,
		size = 'md',
		onclose = null,
		children,
		footer = null
	}: {
		open?: boolean;
		title: string;
		description?: string | null;
		size?: 'sm' | 'md' | 'lg' | 'xl';
		onclose?: (() => void) | null;
		children: Snippet;
		footer?: Snippet | null;
	} = $props();

	const WIDTH = { sm: 'max-w-sm', md: 'max-w-lg', lg: 'max-w-3xl', xl: 'max-w-5xl' } as const;
	const titleId = $props.id();

	let dialog = $state<HTMLDialogElement | null>(null);

	$effect(() => {
		if (!dialog) return;
		if (open && !dialog.open) {
			dialog.showModal();
			focusFirstField();
		} else if (!open && dialog.open) dialog.close();
	});

	/**
	 * Land the cursor where the operator can type: an explicitly marked field
	 * first (an editor opened to fix one thing), otherwise the first real
	 * control. The close button is in the header and would otherwise take the
	 * focus, leaving Save enabled and the keyboard pointed at "dismiss".
	 */
	function focusFirstField() {
		const scope = dialog?.querySelector<HTMLElement>('.modal-box');
		if (!scope) return;
		// An editor opened to fix one field says so; that mark outranks DOM
		// order, which would land on the name input above it.
		const marked = scope.querySelector<HTMLElement>('[data-autofocus]');
		if (marked && !marked.hasAttribute('disabled') && marked.offsetParent !== null) {
			marked.focus();
			return;
		}
		const candidates = scope.querySelectorAll<HTMLElement>(
			'input:not([type="hidden"]), select, textarea'
		);
		for (const element of candidates) {
			if (element.hasAttribute('disabled') || element.offsetParent === null) continue;
			element.focus();
			return;
		}
	}

	function closed() {
		open = false;
		onclose?.();
	}
</script>

<dialog bind:this={dialog} class="modal modal-middle" aria-labelledby={titleId} onclose={closed}>
	{#if open}
		<div class="modal-box flex max-h-[calc(100dvh-2rem)] w-[calc(100%-2rem)] flex-col gap-4 p-0 {WIDTH[size]}">
			<header class="flex items-start gap-3 px-5 pt-5">
				<div class="min-w-0 flex-1">
					<h3 id={titleId} class="m-0 text-lg font-semibold">{title}</h3>
					{#if description}<p class="mb-0 mt-1 text-sm text-base-content/60">{description}</p>{/if}
				</div>
				<button type="button" class="btn btn-circle btn-ghost btn-sm" aria-label={t('ui-close')} onclick={() => (open = false)}>✕</button>
			</header>
			<div class="min-h-0 flex-1 overflow-y-auto px-5 {footer ? '' : 'pb-5'}">{@render children()}</div>
			{#if footer}
				<footer class="flex flex-wrap items-center justify-end gap-2 border-t border-base-300 px-5 py-3">{@render footer()}</footer>
			{/if}
		</div>
	{/if}
	<form method="dialog" class="modal-backdrop"><button aria-label={t('ui-close')}></button></form>
</dialog>
