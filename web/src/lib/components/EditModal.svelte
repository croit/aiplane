<script lang="ts">
	import type { Snippet } from 'svelte';

	/**
	 * The one editor dialog the admin rows share.
	 *
	 * Replaces the inline `<details>` editors: a row's Edit button opens this
	 * instead of pushing the row open and shoving the rest of the list down
	 * the page. Small forms only — anything multi-section gets its own route.
	 *
	 * `open` is bindable so the caller owns the state (and can reset its draft
	 * when the dialog opens); the effect below keeps the real `<dialog>` in
	 * step, and `onclose` catches the paths that bypass the buttons — Escape
	 * and the backdrop — so the caller never desyncs.
	 *
	 * Three footer shapes, because the editors are three shapes:
	 *   `save`  — Cancel + Save, for a draft the caller commits (`onsave`).
	 *   `close` — one Close button, for panels whose controls already apply
	 *             on change (a token's capability toggles).
	 *   `none`  — the child is a whole `<form>` with its own actions (the
	 *             upstream pool and backend editors).
	 *
	 * Nothing inside renders while closed — these sit inside list rows, and
	 * the `<details>` they replace mounted every row's editor whether or not
	 * anyone opened it. It also keeps a closed dialog's title out of the
	 * document, so a row's "Edit pool" button is the only thing by that name.
	 */
	let {
		open = $bindable(false),
		title,
		description = null,
		wide = false,
		saving = false,
		footer = 'save',
		savelabel = null,
		cancellabel,
		onsave = null,
		children
	}: {
		open?: boolean;
		title: string;
		description?: string | null;
		wide?: boolean;
		saving?: boolean;
		footer?: 'save' | 'close' | 'none';
		savelabel?: string | null;
		cancellabel: string;
		onsave?: (() => void | Promise<void>) | null;
		children: Snippet;
	} = $props();

	let dialog = $state<HTMLDialogElement | null>(null);

	$effect(() => {
		if (!dialog) return;
		if (open && !dialog.open) dialog.showModal();
		else if (!open && dialog.open) dialog.close();
	});
</script>

<dialog bind:this={dialog} class="modal" onclose={() => (open = false)}>
	{#if open}
		<div class="modal-box {wide ? 'max-w-3xl' : 'max-w-lg'}">
			<h3 class="m-0 text-base font-semibold">{title}</h3>
			{#if description}<p class="mb-0 mt-1 text-xs text-base-content/60">{description}</p>{/if}
			<div class="mt-4">{@render children()}</div>
			{#if footer === 'save' && onsave}
				<div class="modal-action">
					<button type="button" class="btn btn-ghost btn-sm" onclick={() => (open = false)}>{cancellabel}</button>
					<button type="button" class="btn btn-primary btn-sm" disabled={saving} onclick={onsave}>{savelabel}</button>
				</div>
			{:else if footer === 'close'}
				<div class="modal-action">
					<button type="button" class="btn btn-sm" onclick={() => (open = false)}>{cancellabel}</button>
				</div>
			{/if}
		</div>
	{/if}
	<!-- Clicking the backdrop closes, like every other dialog in the app. -->
	<form method="dialog" class="modal-backdrop"><button aria-label={cancellabel}></button></form>
</dialog>
