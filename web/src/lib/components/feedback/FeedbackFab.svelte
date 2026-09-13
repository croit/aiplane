<script lang="ts">
	/**
	 * The floating feedback button.
	 *
	 * `data-feedback-fab` marks it for exclusion from the screenshot — it is
	 * the one thing guaranteed to be on screen at the moment of capture, and a
	 * report whose screenshot features its own button is a report about the
	 * wrong thing.
	 *
	 * Pressing it captures the page *before* the dialog opens, which is why it
	 * shows a busy state instead of reacting instantly.
	 *
	 * Not rendered on a conversation page: the composer already owns the bottom
	 * -right corner (send, stop, mic, attach), and a floating button there sits
	 * on top of them. The composer carries its own feedback button instead.
	 */
	import { feedback, openDialog } from '$lib/feedback.svelte';
	import { t } from '$lib/i18n.svelte';

	let capturing = $derived(feedback.shotStatus === 'capturing' && !feedback.open);
</script>

<button
	type="button"
	data-feedback-fab
	class="btn btn-circle btn-primary fixed bottom-4 right-4 z-40 shadow-lg"
	title={t('feedback-fab-title')}
	aria-label={t('feedback-fab-aria')}
	disabled={capturing}
	onclick={() => void openDialog()}
>
	{#if capturing}
		<span class="loading loading-spinner loading-sm"></span>
	{:else}
		<svg
			xmlns="http://www.w3.org/2000/svg"
			viewBox="0 0 24 24"
			width="18"
			height="18"
			fill="none"
			stroke="currentColor"
			stroke-width="1.75"
			stroke-linecap="round"
			stroke-linejoin="round"
			aria-hidden="true"
		>
			<path d="m8 2 1.88 1.88M14.12 3.88 16 2" />
			<path d="M9 7.13V6a3 3 0 1 1 6 0v1.13" />
			<path
				d="M12 20c-3.3 0-6-2.7-6-6v-3a4 4 0 0 1 4-4h4a4 4 0 0 1 4 4v3c0 3.3-2.7 6-6 6Z"
			/>
			<path d="M6 13H2M6 17H3M6 9H3M18 13h4M18 17h3M18 9h3" />
		</svg>
	{/if}
</button>
