<script lang="ts">
	import Modal from '#lib/components/ui/Modal.svelte';
	import { causeMessage, chatClassificationApi, failure, levelBadgeClass, levelOptions, type ChatClassification } from '#lib/classification.js';
	import { t } from '#lib/i18n.svelte.js';

	/**
	 * The conversation's level as its coloured badge, opening the levels it
	 * can move to. Higher levels are always offered; lower ones only when the
	 * operator allows lowering with a reason, which this asks for before
	 * sending. The server decides what is accepted.
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

	const cause = $derived(causeMessage(classification.cause));
	let menu = $state<HTMLElement | null>(null);

	function choose(level: string, needsReason: boolean) {
		menu?.hidePopover();
		if (needsReason) pending = level;
		else void apply(level);
	}
</script>

{#if classification.level}
	{@const current = classification.level}
	{#if options.length === 0}
		<span class={levelBadgeClass(current.color)} title={cause ? t(cause.key, cause.args) : undefined} data-classification-badge>{current.name}</span>
	{:else}
		<button
			type="button"
			class="{levelBadgeClass(current.color)} cursor-pointer gap-1"
			popovertarget="level-menu"
			style="anchor-name:--level"
			disabled={busy}
			aria-label="{t('cls-chat-level-aria')}: {current.name}"
			title={cause ? `${t(cause.key, cause.args)} · ${t('cls-chat-level-title')}` : t('cls-chat-level-title')}
			data-classification-badge
		>
			{current.name}<svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="m6 9 6 6 6-6" /></svg>
		</button>
		<ul class="dropdown menu z-10 w-56 rounded-box bg-base-200 p-2 shadow" popover id="level-menu" style="position-anchor:--level" bind:this={menu}>
			{#each options as option (option.level.id)}
				<li><button type="button" onclick={() => choose(option.level.id, option.needsReason)}><span class={levelBadgeClass(option.level.color)}>{option.level.name}</span></button></li>
			{/each}
		</ul>
	{/if}
{/if}

<Modal open={pending !== null} title={t('cls-chat-reason-title')} description={t('cls-chat-reason-lead')} size="sm" onclose={() => (pending = null)}>
	<textarea class="textarea w-full" rows="3" aria-label={t('cls-chat-reason-label')} bind:value={reason}></textarea>
	{#snippet footer()}
		<button class="btn btn-ghost" type="button" onclick={() => (pending = null)}>{t('admin-cancel')}</button>
		<button class="btn btn-primary" type="button" disabled={busy || reason.trim() === ''} onclick={() => pending && apply(pending, reason.trim())}>{t('cls-chat-reason-apply')}</button>
	{/snippet}
</Modal>
