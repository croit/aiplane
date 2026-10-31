<script lang="ts">
	import type { Snippet } from 'svelte';
	import { t } from '#lib/i18n.svelte.js';
	import {
		answerField,
		decisionButtons,
		isContentConfirmation,
		minutesLeft,
		prettyArguments,
		slotLine,
		submitAnswer,
		type Answer,
		type DecisionKind,
		type Waiting
	} from '#lib/suspension.js';

	/**
	 * What a paused turn waits for, and the answer to it: one card for the
	 * inbox, the agent builder's test chat and a person's own chat. It offers
	 * exactly the decisions the server offered, names slots by their labels,
	 * and masks a value only the visitor may type.
	 */
	let {
		waiting,
		lead = null,
		note = null,
		busy = false,
		error = null,
		class: extra = '',
		id = undefined,
		onanswer,
		children
	}: {
		waiting: Waiting;
		lead?: string | null;
		note?: string | null;
		busy?: boolean;
		error?: string | null;
		class?: string;
		id?: string;
		/** Send the answer; resolves whether it went out, so a failed one keeps what was typed. */
		onanswer: (answer: Answer) => Promise<boolean>;
		children?: Snippet;
	} = $props();

	let text = $state('');
	let missing = $state(false);

	const field = $derived(answerField(waiting.kind));
	const buttons = $derived(decisionButtons(waiting));
	const left = $derived(minutesLeft(waiting.expires_at));

	async function decide(decision: DecisionKind) {
		({ text, missing } = await submitAnswer(decision, text, onanswer));
	}
</script>

<div {id} class="card card-border bg-base-100 {extra}">
	<div class="card-body gap-3 p-4 text-sm">
		{@render children?.()}
		{#if lead}<p class="m-0">{lead}</p>{/if}
		{#if waiting.detail}<p class="m-0 font-semibold break-words">{waiting.detail}</p>{/if}

		{#if waiting.question}
			<div>
				<p class="text-xs font-semibold text-base-content/60">{t('suspension-question')}</p>
				<p class="whitespace-pre-wrap break-words">{waiting.question}</p>
			</div>
		{/if}
		{#if waiting.context?.visitor_message}
			<div>
				<p class="text-xs font-semibold text-base-content/60">{t('suspension-visitor-message')}</p>
				<blockquote class="whitespace-pre-wrap break-words border-l-2 border-base-300 pl-3">{waiting.context.visitor_message}</blockquote>
			</div>
		{/if}
		{#if waiting.context?.slots?.length}
			<div>
				<p class="text-xs font-semibold text-base-content/60">{t('suspension-slots')}</p>
				<dl class="mt-1 grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5">
					{#each waiting.context.slots as slot (slot.slot)}
						{@const line = slotLine(slot, t)}
						<dt class="text-base-content/70">{line.label}</dt>
						<dd class="m-0 break-words">{line.value}</dd>
					{/each}
				</dl>
			</div>
		{/if}
		{#if waiting.context?.transcript?.length}
			<details class="collapse collapse-arrow border border-base-300">
				<summary class="collapse-title font-semibold">{t('suspension-transcript')}</summary>
				<div class="collapse-content space-y-2">
					{#each waiting.context.transcript as line, i (i)}
						<p class="whitespace-pre-wrap break-words">
							<span class="font-semibold">{t(line.role === 'user' ? 'suspension-transcript-visitor' : 'suspension-transcript-agent')}:</span>
							{line.text}
						</p>
					{/each}
				</div>
			</details>
		{/if}
		{#if waiting.kind === 'approval' && waiting.call && !isContentConfirmation(waiting)}
			{#each [waiting.call, ...(waiting.bundled ?? [])] as call, i (i)}
				<div>
					<p class="text-xs font-semibold text-base-content/60">{t('suspension-tool', { tool: call.name })}</p>
					{#if call.arguments}
						<pre class="mt-1 overflow-x-auto rounded-box bg-base-200 p-2 text-xs">{prettyArguments(call.arguments)}</pre>
					{/if}
				</div>
			{/each}
		{/if}
		{#if note}<p class="text-xs text-base-content/60">{note}</p>{/if}
		{#if left !== null}<p class="text-xs text-base-content/60">{t('suspension-expires-in', { minutes: left })}</p>{/if}

		{#if error}<div class="alert alert-error alert-soft" role="alert"><span>{error}</span></div>{/if}
		{#if missing}<div class="alert alert-warning alert-soft" role="alert"><span>{t('suspension-answer-required')}</span></div>{/if}

		{#if waiting.options.includes('value')}
			<form class="flex flex-col gap-2" onsubmit={(e) => { e.preventDefault(); void decide('value'); }}>
				{#if field.secret}
					<input class="input input-sm w-full" type="password" autocomplete="off" bind:value={text} aria-label={t(field.label)} placeholder={t(field.label)} disabled={busy} />
				{:else}
					<textarea class="textarea w-full" rows="3" bind:value={text} aria-label={t(field.label)} placeholder={t(field.label)} disabled={busy}></textarea>
				{/if}
				<button class="btn btn-primary btn-sm self-start" type="submit" disabled={busy}>{t(field.submit)}</button>
			</form>
		{/if}
		{#if buttons.length}
			<div class="flex flex-wrap gap-2">
				{#each buttons as button (button.decision)}
					<button class="btn btn-sm {button.primary ? 'btn-primary' : 'btn-ghost'}" type="button" disabled={busy} onclick={() => void decide(button.decision)}>{t(button.key, button.args)}</button>
				{/each}
			</div>
		{/if}
	</div>
</div>
