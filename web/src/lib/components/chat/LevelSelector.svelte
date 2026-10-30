<script lang="ts">
	import Modal from '#lib/components/ui/Modal.svelte';
	import { chatClassificationApi, failure, levelOptions, type ChatClassification } from '#lib/classification.js';
	import { t } from '#lib/i18n.svelte.js';

	/**
	 * Move the conversation to another level. Higher levels are always offered;
	 * lower ones only when the operator allows lowering with a reason, which
	 * this asks for before sending. The server decides what is accepted.
	 */
	let { sessionId, classification, onchanged, onerror }: {
		sessionId: string;
		classification: ChatClassification;
		onchanged: () => void | Promise<void>;
		onerror: (message: string) => void;
	} = $props();

	const options = $derived(levelOptions(classification));
	let pending = $state<string | null>(null);
	let reason = $state('');
	let busy = $state(false);

	async function apply(level: string, why?: string) {
		busy = true;
		try {
			await chatClassificationApi.setLevel(sessionId, level, why);
			pending = null;
			reason = '';
			await onchanged();
		} catch (err) {
			onerror(failure(err));
		} finally {
			busy = false;
		}
	}

	function choose(event: Event & { currentTarget: HTMLSelectElement }) {
		const level = event.currentTarget.value;
		event.currentTarget.value = classification.level?.id ?? '';
		const option = options.find((o) => o.level.id === level);
		if (!option) return;
		if (option.needsReason) pending = level;
		else void apply(level);
	}
</script>

<select class="select select-xs w-auto max-w-48" aria-label={t('cls-chat-level-aria')} title={t('cls-chat-level-title')} disabled={busy || options.length === 0} onchange={choose}>
	<option value={classification.level?.id ?? ''} selected>{t('cls-chat-level-prefix')} {classification.level?.name}</option>
	{#each options as option (option.level.id)}<option value={option.level.id}>{option.level.name}</option>{/each}
</select>

<Modal open={pending !== null} title={t('cls-chat-reason-title')} description={t('cls-chat-reason-lead')} size="sm" onclose={() => (pending = null)}>
	<textarea class="textarea w-full" rows="3" aria-label={t('cls-chat-reason-label')} bind:value={reason}></textarea>
	{#snippet footer()}
		<button class="btn btn-ghost" type="button" onclick={() => (pending = null)}>{t('admin-cancel')}</button>
		<button class="btn btn-primary" type="button" disabled={busy || reason.trim() === ''} onclick={() => pending && apply(pending, reason.trim())}>{t('cls-chat-reason-apply')}</button>
	{/snippet}
</Modal>
