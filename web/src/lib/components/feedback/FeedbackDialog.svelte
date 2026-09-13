<script lang="ts">
	/**
	 * The feedback dialog: form, voice intake, annotated screenshot, pasted
	 * images, diagnostics consent, and the public-tracker confirmation.
	 *
	 * Two `<dialog>`s, not one: modal dialogs share the browser's top layer, so
	 * the confirmation stacks cleanly over the form without any z-index
	 * juggling, and dismissing it leaves the form exactly as it was.
	 *
	 * `data-feedback-dialog` keeps both out of the screenshot.
	 */
	import { onDestroy } from 'svelte';
	import {
		addAttachment,
		cancelConfirm,
		captureExact,
		closeDialog,
		collectSystemInfo,
		feedback,
		isChatPage,
		recapture,
		removeAttachment,
		removeShot,
		requestSubmit,
		resetDialog,
		setAnnotating,
		stopVoice,
		submit,
		toggleVoice
	} from '$lib/feedback.svelte';
	import { t } from '$lib/i18n.svelte';
	import ScreenshotAnnotator from './ScreenshotAnnotator.svelte';

	let dialog = $state<HTMLDialogElement | null>(null);
	let confirmDialog = $state<HTMLDialogElement | null>(null);
	let dragging = $state(false);

	// Snapshotted when the dialog opens so the <details> preview shows exactly
	// what a submit right now would attach — recomputing it on every keystroke
	// would serialise a 100-entry log into the DOM for no reason.
	let systemInfoPreview = $state('');
	let showChatConsent = $state(false);

	$effect(() => {
		if (!dialog) return;
		if (feedback.open && !dialog.open) dialog.showModal();
		else if (!feedback.open && dialog.open) dialog.close();
	});

	$effect(() => {
		if (!confirmDialog) return;
		if (feedback.confirming && !confirmDialog.open) confirmDialog.showModal();
		else if (!feedback.confirming && confirmDialog.open) confirmDialog.close();
	});

	$effect(() => {
		if (!feedback.open) return;
		showChatConsent = isChatPage();
		systemInfoPreview = JSON.stringify(collectSystemInfo(), null, 2);
	});

	function dismiss(): void {
		stopVoice();
		closeDialog();
		resetDialog();
	}

	/** Escape / the backdrop bypass the buttons; keep the store in step. */
	function onDialogClose(): void {
		if (feedback.open) dismiss();
	}

	/**
	 * Escape in annotate mode returns to the form. Letting the browser close
	 * the dialog there would discard the report *and* the annotations, which is
	 * never what Escape is meant to do from a sub-mode.
	 */
	function onDialogCancel(event: Event): void {
		if (!feedback.annotating) return;
		event.preventDefault();
		setAnnotating(false);
	}

	async function onPaste(event: ClipboardEvent): Promise<void> {
		const items = event.clipboardData?.items;
		if (!items) return;
		for (const item of items) {
			if (item.kind !== 'file' || !item.type.startsWith('image/')) continue;
			const file = item.getAsFile();
			if (!file) continue;
			event.preventDefault();
			await addAttachment(file);
		}
	}

	async function onDrop(event: DragEvent): Promise<void> {
		dragging = false;
		const files = event.dataTransfer?.files;
		if (!files?.length) return;
		event.preventDefault();
		for (const file of files) await addAttachment(file);
	}

	let voiceLabel = $derived(
		feedback.voicePhase === 'recording'
			? t('feedback-voice-stop-label')
			: feedback.voicePhase === 'working'
				? t('feedback-voice-working-label')
				: t('feedback-voice-button-label')
	);

	let shotStatusLabel = $derived(
		feedback.shotStatus === 'capturing'
			? t('feedback-shot-status-capturing')
			: feedback.shotStatus === 'attached'
				? t('feedback-shot-status-attached')
				: feedback.shotStatus === 'failed'
					? t('feedback-shot-status-failed')
					: t('feedback-shot-status-none')
	);

	onDestroy(stopVoice);
</script>

<dialog
	bind:this={dialog}
	data-feedback-dialog
	class="modal modal-bottom sm:modal-middle"
	onclose={onDialogClose}
	oncancel={onDialogCancel}
>
	{#if feedback.open}
		<div
			class="modal-box flex max-h-[92vh] flex-col p-0 {feedback.annotating
				? 'h-[92vh] w-[96vw] max-w-none'
				: 'max-w-3xl'}"
		>
			<div class="flex items-center gap-2 border-b border-base-300 px-5 py-3">
				<h2 class="m-0 flex-1 text-base font-semibold">
					{feedback.annotating ? t('feedback-annotate-heading') : t('feedback-dialog-heading')}
				</h2>
				{#if feedback.voiceEnabled && !feedback.filed && !feedback.annotating}
					<button
						type="button"
						class="btn btn-sm gap-2 {feedback.voicePhase === 'recording'
							? 'btn-error'
							: 'btn-ghost'}"
						title={t('feedback-voice-button-title')}
						disabled={feedback.voicePhase === 'working'}
						onclick={() => void toggleVoice()}
					>
						{#if feedback.voicePhase === 'working'}
							<span class="loading loading-spinner loading-xs"></span>
						{:else}
							<svg
								xmlns="http://www.w3.org/2000/svg"
								viewBox="0 0 24 24"
								width="16"
								height="16"
								fill="none"
								stroke="currentColor"
								stroke-width="1.75"
								stroke-linecap="round"
								stroke-linejoin="round"
								aria-hidden="true"
							>
								<rect x="9" y="2" width="6" height="12" rx="3" />
								<path d="M5 10a7 7 0 0 0 14 0M12 17v5" />
							</svg>
						{/if}
						<span class="hidden sm:inline">{voiceLabel}</span>
					</button>
				{/if}
				<!-- While annotating this steps back to the form rather than
				     throwing the report away with the drawing on it. -->
				<button
					type="button"
					class="btn btn-square btn-ghost btn-sm"
					aria-label={feedback.annotating ? t('feedback-annotate-done') : t('feedback-close-aria')}
					onclick={() => (feedback.annotating ? setAnnotating(false) : dismiss())}
				>
					✕
				</button>
			</div>

			{#if feedback.filed}
				<div class="flex flex-col gap-3 px-5 py-6">
					<h3 class="m-0 text-base font-semibold">{t('feedback-thanks-heading')}</h3>
					<p class="m-0 text-sm text-base-content/70">
						{feedback.filed.number
							? t('feedback-thanks-issue', { number: feedback.filed.number })
							: t('feedback-thanks-body')}
					</p>
					<div class="modal-action mt-2">
						{#if feedback.filed.url}
							<a
								class="btn btn-ghost btn-sm"
								href={feedback.filed.url}
								target="_blank"
								rel="noreferrer noopener">{t('feedback-thanks-open')}</a
							>
						{/if}
						<button type="button" class="btn btn-primary btn-sm" onclick={dismiss}
							>{t('feedback-done-button')}</button
						>
					</div>
				</div>
			{:else if feedback.annotating}
				<!-- Annotate mode: the capture gets the whole dialog. The form is
				     only hidden, not unmounted — its values live in the store, so
				     coming back lands on exactly what was typed. -->
				<div class="flex min-h-0 flex-1 flex-col gap-2 px-5 py-4">
					{#if feedback.shotDataUrl}
						<ScreenshotAnnotator dataUrl={feedback.shotDataUrl} tall />
					{/if}
				</div>
				<div class="flex items-center justify-end gap-2 border-t border-base-300 px-5 py-3">
					<button type="button" class="btn btn-ghost btn-sm" onclick={() => void recapture()}>
						{t('feedback-shot-recapture')}
					</button>
					<button type="button" class="btn btn-primary btn-sm" onclick={() => setAnnotating(false)}>
						{t('feedback-annotate-done')}
					</button>
				</div>
			{:else}
				<!-- svelte-ignore a11y_no_static_element_interactions -->
				<div
					class="relative flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto px-5 py-4"
					onpaste={(event) => void onPaste(event)}
					ondragover={(event) => {
						event.preventDefault();
						dragging = true;
					}}
					ondragleave={() => (dragging = false)}
					ondrop={(event) => {
						event.preventDefault();
						void onDrop(event);
					}}
				>
					<p class="m-0 text-xs text-base-content/60">{t('feedback-dialog-blurb')}</p>

					<div class="grid gap-3 sm:grid-cols-3">
						<label class="flex flex-col gap-1 sm:col-span-2">
							<span class="text-sm font-medium">{t('feedback-title-label')}</span>
							<input
								class="input input-sm input-bordered w-full"
								maxlength="120"
								placeholder={t('feedback-title-placeholder')}
								bind:value={feedback.title}
							/>
						</label>
						<label class="flex flex-col gap-1">
							<span class="text-sm font-medium">{t('feedback-priority-label')}</span>
							<select class="select select-sm select-bordered w-full" bind:value={feedback.priority}>
								<option value="low">{t('feedback-priority-low')}</option>
								<option value="medium">{t('feedback-priority-medium')}</option>
								<option value="high">{t('feedback-priority-high')}</option>
							</select>
						</label>
					</div>

					<label class="flex flex-col gap-1">
						<span class="text-sm font-medium">{t('feedback-description-label')}</span>
						<textarea
							class="textarea textarea-bordered w-full text-sm"
							rows="4"
							maxlength="2000"
							placeholder={t('feedback-description-placeholder')}
							bind:value={feedback.description}
						></textarea>
					</label>

					<label class="flex flex-col gap-1">
						<span class="text-sm font-medium">{t('feedback-business-label')}</span>
						<textarea
							class="textarea textarea-bordered w-full text-sm"
							rows="2"
							maxlength="2000"
							placeholder={t('feedback-business-placeholder')}
							bind:value={feedback.business}
						></textarea>
					</label>

					<label class="flex flex-col gap-1">
						<span class="text-sm font-medium">{t('feedback-acceptance-label')}</span>
						<textarea
							class="textarea textarea-bordered w-full text-sm"
							rows="2"
							maxlength="2000"
							placeholder={t('feedback-acceptance-placeholder')}
							bind:value={feedback.acceptance}
						></textarea>
					</label>

					<!-- Screenshot -->
					<div class="flex flex-col gap-2">
						<div class="flex flex-wrap items-center gap-2">
							<span class="text-sm font-medium">{t('feedback-shot-label')}</span>
							<span class="text-xs text-base-content/60">{shotStatusLabel}</span>
							<div class="ms-auto flex flex-wrap gap-1">
								{#if feedback.shotDataUrl}
									<button
										type="button"
										class="btn btn-primary btn-xs"
										onclick={() => setAnnotating(true)}
									>
										{t('feedback-shot-annotate')}
									</button>
								{/if}
								<button
									type="button"
									class="btn btn-ghost btn-xs"
									disabled={feedback.shotStatus === 'capturing' || feedback.capturingExact}
									onclick={() => void recapture()}
								>
									{feedback.shotDataUrl
										? t('feedback-shot-recapture')
										: t('feedback-shot-capture')}
								</button>
								{#if feedback.exactSupported}
									<button
										type="button"
										class="btn btn-ghost btn-xs"
										title={t('feedback-shot-exact-title')}
										disabled={feedback.shotStatus === 'capturing' || feedback.capturingExact}
										onclick={() => void captureExact()}
									>
										{t('feedback-shot-exact')}
									</button>
								{/if}
								{#if feedback.shotDataUrl}
									<button type="button" class="btn btn-ghost btn-xs" onclick={removeShot}>
										{t('feedback-shot-remove')}
									</button>
								{/if}
							</div>
						</div>
						{#if feedback.shotDataUrl}
							<ScreenshotAnnotator dataUrl={feedback.shotDataUrl} />
						{/if}
					</div>

					<!-- Pasted / dropped images -->
					<div class="flex flex-col gap-2">
						<div class="flex items-center gap-2">
							<span class="text-sm font-medium">{t('feedback-attachments-label')}</span>
							<span class="text-xs text-base-content/60"
								>{t('feedback-attachments-count', {
									count: feedback.attachments.length,
									max: feedback.maxAttachments
								})}</span
							>
						</div>
						{#if feedback.attachments.length}
							<div class="flex flex-wrap gap-2">
								{#each feedback.attachments as attachment, index (attachment.url)}
									<div class="relative h-20 w-20">
										<img
											src={attachment.url}
											alt=""
											class="h-full w-full rounded border border-base-300 object-cover"
										/>
										<button
											type="button"
											class="btn btn-circle btn-error btn-xs absolute -right-2 -top-2"
											aria-label={t('feedback-attachments-remove')}
											onclick={() => removeAttachment(index)}>✕</button
										>
									</div>
								{/each}
							</div>
						{/if}
						<p class="m-0 text-xs text-base-content/60">{t('feedback-attachments-hint')}</p>
					</div>

					<!-- Diagnostics consent -->
					<div class="flex flex-col gap-1 border-t border-base-300 pt-3">
						<label class="flex cursor-pointer items-center gap-2">
							<input type="checkbox" class="checkbox checkbox-sm" bind:checked={feedback.includeBrowserLog} />
							<span class="text-sm">{t('feedback-log-browser-label')}</span>
						</label>
						{#if showChatConsent}
							<label class="flex cursor-pointer items-center gap-2">
								<input type="checkbox" class="checkbox checkbox-sm" bind:checked={feedback.includeChatLog} />
								<span class="text-sm">{t('feedback-log-chat-label')}</span>
							</label>
						{/if}
						<details class="text-xs text-base-content/60">
							<summary class="cursor-pointer hover:text-base-content"
								>{t('feedback-diagnostics-label')}</summary
							>
							<pre
								class="mt-2 max-h-48 overflow-auto whitespace-pre-wrap break-words rounded bg-base-200 p-2 text-[10px]">{systemInfoPreview}</pre>
						</details>
					</div>

					{#if feedback.notice}
						<div class="alert alert-info py-2 text-sm"><span>{feedback.notice}</span></div>
					{/if}
					{#if feedback.error}
						<div class="alert alert-error py-2 text-sm"><span>{feedback.error}</span></div>
					{/if}

					{#if dragging}
						<div
							class="pointer-events-none absolute inset-0 z-10 flex items-center justify-center rounded-lg bg-primary/10 text-sm font-medium text-primary"
						>
							{t('feedback-attachments-drop')}
						</div>
					{/if}
				</div>

				<div class="flex items-center justify-end gap-2 border-t border-base-300 px-5 py-3">
					<button type="button" class="btn btn-ghost btn-sm" disabled={feedback.busy} onclick={dismiss}
						>{t('feedback-cancel-button')}</button
					>
					<button
						type="button"
						class="btn btn-primary btn-sm"
						disabled={feedback.busy}
						onclick={requestSubmit}
					>
						{feedback.busy ? t('feedback-sending') : t('feedback-submit-button')}
					</button>
				</div>
			{/if}
		</div>
	{/if}
	<form method="dialog" class="modal-backdrop">
		<button aria-label={t('feedback-close-aria')}></button>
	</form>
</dialog>

<dialog
	bind:this={confirmDialog}
	data-feedback-dialog
	class="modal"
	onclose={() => (feedback.confirming = false)}
>
	<div class="modal-box max-w-md">
		<h3 class="m-0 text-base font-semibold">{t('feedback-confirm-heading')}</h3>
		<div class="mt-3 flex flex-col gap-2 text-sm">
			<p class="m-0">
				{t('feedback-confirm-public-p1-prefix')}
				<strong>{t('feedback-confirm-public-p1-strong')}</strong>
				{t('feedback-confirm-public-p1-suffix')}
			</p>
			<p class="m-0">
				{t('feedback-confirm-private-p2-prefix')}
				<strong>{t('feedback-confirm-private-p2-strong')}</strong>
				{t('feedback-confirm-private-p2-suffix')}
			</p>
		</div>
		<div class="modal-action">
			<button type="button" class="btn btn-ghost btn-sm" onclick={cancelConfirm}
				>{t('feedback-confirm-cancel-button')}</button
			>
			<button type="button" class="btn btn-primary btn-sm" onclick={() => void submit()}
				>{t('feedback-confirm-ok-button')}</button
			>
		</div>
	</div>
	<form method="dialog" class="modal-backdrop">
		<button aria-label={t('feedback-confirm-cancel-button')}></button>
	</form>
</dialog>
