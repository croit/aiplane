<script lang="ts">
	import { onMount } from 'svelte';
	import { base } from '$app/paths';
	import { api, ApiError } from '$lib/api';
	import { createConversationController } from '$lib/chat.svelte';
	import { renderMarkdown } from '$lib/markdown';
	import type { ChatSession } from '$lib/chat-protocol';

	let { data } = $props<{ data: { id: string } }>();
	const id = $derived(data.id);

	let controller = $state<ReturnType<typeof createConversationController> | null>(null);
	let session = $state<ChatSession | null>(null);
	let model = $state('');
	let models = $state<{ id: string; gdpr: boolean; nda: boolean }[]>([]);
	let draft = $state('');
	let sending = $state(false);
	let notice = $state<string | null>(null);

	const turns = $derived(controller ? controller.state.turns : []);
	const streaming = $derived(controller !== null && controller.state.liveTurnId !== null);
	const prompt = $derived(controller?.state.prompt ?? null);

	async function loadMeta() {
		try {
			const snap = await api.getChatSession(id);
			session = snap.session;
			// Prefill the model picker from the conversation's last assistant
			// turn — the "keep talking to what you were talking to" default.
			const lastModel = [...snap.turns]
				.reverse()
				.find((t) => t.turn.role === 'assistant' && t.turn.model)?.turn.model;
			if (!model && lastModel) model = lastModel;
		} catch (err) {
			notice = String(err);
		}
	}

	/** The picker prefers the offered models; free text stays as the
	 * fallback for gateways without pools (or models the grant filters out).
	 * The compliance flags ride along as the option's title tooltip. */
	async function loadModels() {
		try {
			const list = await api.listChatModels();
			models = list.models;
			if (!model && models.length > 0) model = models[0].id;
		} catch {
			// Picker stays free-text; submitting still works.
		}
	}

	onMount(() => {
		void loadModels();
		const c = createConversationController(id);
		c.onSidebarChanged = () => loadMeta();
		c.attach();
		controller = c;
		void loadMeta();
		return () => c.destroy();
	});

	async function send() {
		const text = draft.trim();
		if (!text || !model.trim() || sending) return;
		sending = true;
		notice = null;
		try {
			await api.sendChatMessage(id, { model: model.trim(), message: text });
			draft = '';
			// The reply arrives on a fresh stream (the idle one closed).
			controller?.attach();
		} catch (err) {
			notice =
				err instanceof ApiError && err.status === 409
					? 'The previous reply is still streaming — stop it first.'
					: String(err);
		} finally {
			sending = false;
		}
	}

	async function stop() {
		try {
			await api.cancelChatTurn(id);
		} catch (err) {
			notice = String(err);
		}
	}

	async function answerPrompt(text: string | null) {
		if (!prompt || prompt.action !== 'show') return;
		try {
			await fetch(`/api/v0/me/ask/feedback/${encodeURIComponent(prompt.turn_id)}`, {
				method: 'POST',
				credentials: 'same-origin',
				headers: { 'content-type': 'application/json' },
				body: JSON.stringify(text === null ? { dismissed: true } : { choices: [], text })
			});
		} finally {
			if (controller) controller.state.prompt = null;
		}
	}

	let promptText = $state('');
	function submitPrompt() {
		const value = promptText.trim();
		if (value) void answerPrompt(value);
		promptText = '';
	}

	function onKeydown(event: KeyboardEvent) {
		if (event.key === 'Enter' && !event.shiftKey) {
			event.preventDefault();
			void send();
		}
	}
</script>

<div class="flex items-center justify-between mb-4 gap-2">
	<h1 class="text-lg font-semibold truncate flex-1 min-w-0">
		{session?.title?.trim() || 'Untitled chat'}
	</h1>
	<a href="{base}/chat" class="btn btn-ghost btn-sm">All chats</a>
</div>

{#if notice}
	<div class="alert alert-warning mb-4"><span>{notice}</span></div>
{/if}

{#if prompt?.action === 'show'}
	{@const shown = prompt}
	<div class="card border border-warning mb-4">
		<div class="card-body">
			<h2 class="card-title text-base">The assistant asks</h2>
			<p>{shown.question}</p>
			{#if shown.options.length > 0}
				<div class="flex flex-wrap gap-2 mt-1">
					{#each shown.options as option (option)}
						<button class="btn btn-outline btn-sm" onclick={() => answerPrompt(option)}>
							{option}
						</button>
					{/each}
				</div>
			{/if}
			<div class="join mt-2">
				<input
					class="input input-bordered input-sm join-item w-full"
					placeholder="Type an answer…"
					bind:value={promptText}
					onkeydown={(e) => e.key === 'Enter' && submitPrompt()}
				/>
				<button class="btn btn-primary btn-sm join-item" onclick={submitPrompt}>Answer</button>
				<button class="btn btn-ghost btn-sm join-item" onclick={() => answerPrompt(null)}>
					Skip
				</button>
			</div>
		</div>
	</div>
{/if}

<div class="flex flex-col gap-4 mb-4">
	{#each turns as entry (entry.turn.id)}
		{#if entry.turn.role === 'user'}
			<div class="chat chat-end">
				<div class="chat-bubble chat-bubble-primary whitespace-pre-wrap">{entry.turn.user_content}</div>
			</div>
		{:else}
			<div class="chat chat-start">
				<div class="chat-bubble chat-bubble-ghost w-full max-w-[min(90vw,48rem)] p-0">
					<div class="p-3 flex flex-col gap-2">
						{#if entry.turn.reasoning}
							<details class="text-sm">
								<summary class="cursor-pointer text-base-content/60">Thinking…</summary>
								<div class="mt-2 whitespace-pre-wrap text-xs text-base-content/70 max-h-64 overflow-y-auto">
									{entry.turn.reasoning}
								</div>
							</details>
						{/if}

						{#each entry.tool_calls as call (call.id)}
							<details class="text-sm">
								<summary class="cursor-pointer">
									<span class="badge badge-sm badge-outline me-1">{call.status}</span>
									{call.name}
								</summary>
								<div class="mt-1 text-xs text-base-content/70">
									<div class="font-mono break-all">{call.arguments_json}</div>
									{#if call.output_json}
										<pre class="mt-1 font-mono whitespace-pre-wrap max-h-48 overflow-y-auto">{call.output_json}</pre>
									{/if}
								</div>
							</details>
						{/each}

						{#if entry.turn.status === 'in_progress' && !entry.turn.content}
							<span class="loading loading-dots loading-sm"></span>
						{/if}

						<!-- eslint-disable-next-line svelte/no-at-html-tags -- sanitised in renderMarkdown -->
						<div class="prose prose-sm max-w-none chat-prose">{@html renderMarkdown(entry.turn.content)}</div>

						{#if entry.turn.status === 'errored'}
							<div class="alert alert-error py-2"><span>{entry.turn.error_message}</span></div>
						{:else if entry.turn.status === 'cancelled'}
							<div class="text-xs text-base-content/50">stopped</div>
						{/if}
					</div>
				</div>
			</div>
		{/if}
	{/each}
</div>

<div class="card border border-base-300 sticky bottom-0">
	<div class="card-body p-3 gap-2">
		<div class="flex gap-2">
			{#if models.length > 0}
				<select
					class="select select-bordered select-sm w-56"
					aria-label="Model"
					bind:value={model}
					disabled={streaming || sending}
				>
					{#each models as m (m.id)}
						<option value={m.id} title="{m.gdpr ? 'GDPR region' : ''} {m.nda ? 'NDA-covered' : ''}">
							{m.id}{m.gdpr ? ' · gdpr' : ''}{m.nda ? ' · nda' : ''}
						</option>
					{/each}
				</select>
			{:else}
				<input
					class="input input-bordered input-sm w-56"
					placeholder="model (e.g. gpt-4o-mini)"
					aria-label="Model"
					bind:value={model}
					disabled={streaming || sending}
				/>
			{/if}
			<textarea
				class="textarea textarea-bordered flex-1 min-h-11 max-h-48"
				rows="1"
				placeholder="Message the model…"
				bind:value={draft}
				onkeydown={onKeydown}
				disabled={streaming}
			></textarea>
			{#if streaming}
				<button class="btn btn-error" onclick={stop}>Stop</button>
			{:else}
				<button class="btn btn-primary" onclick={send} disabled={!draft.trim() || !model.trim() || sending}>
					Send
				</button>
			{/if}
		</div>
	</div>
</div>
