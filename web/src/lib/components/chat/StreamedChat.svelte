<script lang="ts">
	import { onMount, type Snippet } from 'svelte';
	import type { LiveTurn } from '#lib/chat-protocol.js';
	import { endScrollTop, nextFollow } from '#lib/chat-autoscroll.js';
	import { t } from '#lib/i18n.svelte.js';
	import Markdown from './Markdown.svelte';
	import ToolCalls from './ToolCalls.svelte';

	/**
	 * A compact conversation over a `ConversationController`'s turns: the
	 * transcript that follows its end while the reader is there, and a
	 * composer below it that stays in view. It fills the height its parent
	 * gives it. The agent architect and the agent's test chat both use it;
	 * what differs is passed in as snippets.
	 */
	let {
		turns,
		working,
		disabled = false,
		empty,
		placeholder,
		sendLabel,
		userLabel = null,
		assistantLabel = null,
		toolLabel = undefined,
		highlighted = null,
		onsend,
		inside,
		below,
		before,
		composer
	}: {
		turns: LiveTurn[];
		working: boolean;
		disabled?: boolean;
		empty: string;
		placeholder: string;
		sendLabel: string;
		userLabel?: string | null;
		assistantLabel?: string | null;
		toolLabel?: (name: string) => string;
		/** The assistant turn drawn as selected. */
		highlighted?: string | null;
		/** Send `text`; resolves true once it was accepted, which clears the field. */
		onsend: (text: string) => Promise<boolean>;
		/** Extra content at the end of an assistant bubble. */
		inside?: Snippet<[LiveTurn]>;
		/** Content under an assistant bubble. */
		below?: Snippet<[LiveTurn]>;
		/** Content above the transcript's first turn (a starting spinner). */
		before?: Snippet;
		/** Controls beside the send button; `append` adds text to the field. */
		composer?: Snippet<[(text: string) => void]>;
	} = $props();

	let text = $state('');
	let sending = $state(false);
	let transcript = $state<HTMLElement | null>(null);
	let body = $state<HTMLElement | null>(null);
	let following = true;
	let lastScrollTop = 0;

	function onScroll() {
		if (!transcript) return;
		following = nextFollow(following, lastScrollTop, transcript);
		lastScrollTop = transcript.scrollTop;
	}

	function scrollToEnd() {
		if (!transcript) return;
		transcript.scrollTop = endScrollTop(transcript);
		lastScrollTop = transcript.scrollTop;
	}

	const append = (heard: string) => (text = text.trim() ? `${text.trimEnd()} ${heard}` : heard);
	const canSend = $derived(!disabled && !sending && !working && text.trim().length > 0);

	async function send() {
		if (!canSend) return;
		sending = true;
		try {
			if (await onsend(text.trim())) {
				text = '';
				following = true;
				scrollToEnd();
			}
		} finally {
			sending = false;
		}
	}

	function keydown(event: KeyboardEvent) {
		if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) {
			event.preventDefault();
			void send();
		}
	}

	onMount(() => {
		const observer = new ResizeObserver(() => {
			if (following) scrollToEnd();
		});
		if (transcript) observer.observe(transcript);
		if (body) observer.observe(body);
		return () => observer.disconnect();
	});
</script>

<div class="flex min-h-0 flex-1 flex-col gap-2">
	<div bind:this={transcript} onscroll={onScroll} data-chat-transcript class="min-h-0 flex-1 overflow-y-auto rounded-box border border-base-300 bg-base-100 p-3" aria-live="polite">
		<div bind:this={body} class="flex flex-col gap-2">
			{@render before?.()}
			{#each turns as live (live.turn.id)}
				{#if live.turn.role === 'user'}
					<div class="chat chat-end">
						{#if userLabel}<div class="chat-header text-xs opacity-60">{userLabel}</div>{/if}
						<div class="chat-bubble border border-primary/20 bg-primary/10 text-base-content whitespace-pre-wrap">{live.turn.user_content}</div>
					</div>
				{:else}
					<div class="chat chat-start">
						{#if assistantLabel}<div class="chat-header text-xs opacity-60">{assistantLabel}</div>{/if}
						<div class="chat-bubble flex w-full max-w-full flex-col gap-2 border bg-base-200 text-base-content {highlighted === live.turn.id ? 'border-primary' : 'border-base-300'}">
							{#if live.tool_calls.length}<ToolCalls calls={live.tool_calls} label={toolLabel} />{/if}
							{#if live.turn.content}<Markdown content={live.turn.content} class="prose prose-sm max-w-none text-inherit" />{/if}
							{#if live.turn.status === 'in_progress'}
								<span class="flex items-center gap-2 text-sm text-base-content/60"><span class="loading loading-dots loading-sm"></span>{t(live.turn.content ? 'render-still-working-spinner' : 'render-thinking-spinner')}</span>
							{/if}
							{#if live.turn.error_code === 'loop_exhausted'}<p class="m-0 text-sm text-warning whitespace-pre-wrap">{t('chat-loop-exhausted', { attempts: live.attempts.length })}</p>
							{:else if live.turn.error_message}<p class="m-0 text-sm text-error whitespace-pre-wrap">{live.turn.error_message}</p>{/if}
							{@render inside?.(live)}
						</div>
						{@render below?.(live)}
					</div>
				{/if}
			{:else}
				<p class="m-0 text-sm text-base-content/60">{empty}</p>
			{/each}
		</div>
	</div>

	<form data-chat-composer class="flex w-full shrink-0 items-end gap-2" onsubmit={(e) => { e.preventDefault(); void send(); }}>
		<textarea class="textarea min-h-11 flex-1 resize-none" rows="2" bind:value={text} onkeydown={keydown} {placeholder} aria-label={placeholder} {disabled}></textarea>
		{@render composer?.(append)}
		<button class="btn btn-primary" type="submit" disabled={!canSend}>{sendLabel}</button>
	</form>
</div>
