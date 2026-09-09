<script lang="ts">
	import { onMount } from 'svelte';
	import { base } from '$app/paths';
	import { goto } from '$app/navigation';
	import { api, ApiError } from '$lib/api';
	import type { CanvasDocument } from '$lib/api';
	import { createConversationController } from '$lib/chat.svelte';
	import { createVoiceController } from '$lib/voice.svelte';
	import { refreshSidebar } from '$lib/sidebar.svelte';
	import { renderMarkdown } from '$lib/markdown';
	import { parseUserContent } from '$lib/chat-protocol';
	import type { ChatSession } from '$lib/chat-protocol';
	import { me } from '$lib/session.svelte';

	let { data } = $props<{ data: { id: string } }>();
	const id = $derived(data.id);

	let controller = $state<ReturnType<typeof createConversationController> | null>(null);
	let session = $state<ChatSession | null>(null);
	let model = $state('');
	let models = $state<{ id: string; gdpr: boolean; nda: boolean }[]>([]);
	let draft = $state('');
	let files = $state<File[]>([]);
	let tools = $state<{ key: string; title: string; enabled: boolean }[]>([]);
	let toolsOpen = $state(false);
	let effort = $state('standard');
	const EFFORTS: [string, string][] = [
		['fast', 'Thinking: Fast'],
		['standard', 'Thinking: Standard'],
		['deep', 'Thinking: Deep'],
		['max', 'Thinking: Max']
	];
	let sending = $state(false);
	let notice = $state<string | null>(null);

	// Canvas documents (the panel the document tools write into).
	let documents = $state<CanvasDocument[]>([]);
	let openDoc = $state<{
		document: CanvasDocument;
		content: string;
		history: { version: number; created_at: string; chars: number; author: string }[];
	} | null>(null);
	let docDraft = $state('');
	let docEditing = $state(false);
	let docSaving = $state(false);

	const turns = $derived(controller ? controller.state.turns : []);
	const streaming = $derived(controller !== null && controller.state.liveTurnId !== null);
	const prompt = $derived(controller?.state.prompt ?? null);
	/** A conversation shared *with* you renders read-only, plus a Fork action. */
	const isOwner = $derived(
		session === null || me.value === null || session.user_id === me.value.id
	);

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

	async function loadTools() {
		try {
			tools = (await api.listChatCapabilities(id)).tools;
		} catch {
			tools = [];
		}
	}

	async function toggleCapability(key: string, current: boolean) {
		try {
			await api.setChatCapability(id, key, !current);
			tools = tools.map((t) => (t.key === key ? { ...t, enabled: !current } : t));
		} catch (err) {
			notice = String(err);
		}
	}

	async function saveEffort() {
		try {
			await api.setChatEffort(id, effort);
		} catch {
			/* non-fatal */
		}
	}

	onMount(() => {
		void loadTools();
		const c = createConversationController(id);
		c.onSidebarChanged = () => {
			void loadMeta();
			void refreshSidebar();
			// A turn that wrote to the canvas bumps the sidebar too, so this is
			// also the cue to re-read the document list.
			void loadDocuments();
		};
		c.attach();
		controller = c;
		void loadMeta();
		void loadDocuments();
		return () => c.destroy();
	});

	const voiceOpen = $state({ open: false });
	let voice = $state<ReturnType<typeof createVoiceController> | null>(null);

	/** Voice turns submit exactly like typed ones — plus the flag. */
	async function submitVoiceTurn(text: string) {
		if (!model.trim() || sending) return;
		sending = true;
		try {
			await api.sendChatMessage(id, { model: model.trim(), message: text, voice: true });
			controller?.attach();
			void refreshSidebar();
		} catch (err) {
			notice = String(err);
		} finally {
			sending = false;
		}
	}

	function openVoice() {
		voice = createVoiceController(submitVoiceTurn);
		voiceOpen.open = true;
	}

	function closeVoice() {
		voice?.close();
		voice = null;
		voiceOpen.open = false;
	}

	// Feed the live reply into the voice controller whenever a
	// voice-submitted turn is streaming (state drives it, no DOM observers).
	$effect(() => {
		if (!voice || !controller) return;
		const liveId = controller.state.liveTurnId;
		if (!liveId) return;
		const live = controller.state.turns.find((t) => t.turn.id === liveId);
		const content = live?.turn.content ?? '';
		const finalized = live?.turn.status !== 'in_progress';
		if (finalized && content === '') return;
		voice.feedReplyText(content, finalized);
	});

	async function send() {
		const text = draft.trim();
		if ((!text && files.length === 0) || !model.trim() || sending) return;
		sending = true;
		notice = null;
		try {
			if (files.length > 0) {
				// Attachments ride as multipart (same shape as the legacy
				// composer) so they land under this turn's storage prefix.
				const fd = new FormData();
				fd.append('model', model.trim());
				fd.append('message', text);
				for (const f of files) fd.append('attachment', f);
				const res = await fetch(`/api/v0/chat/sessions/${id}/messages`, {
					method: 'POST',
					credentials: 'same-origin',
					body: fd
				});
				if (!res.ok) throw new Error((await res.text()).slice(0, 200));
			} else {
				await api.sendChatMessage(id, { model: model.trim(), message: text });
			}
			draft = '';
			files = [];
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

	function addFiles(list: FileList | File[] | null) {
		if (!list) return;
		files = [...files, ...Array.from(list)];
	}

	function onPaste(e: ClipboardEvent) {
		const images = Array.from(e.clipboardData?.files ?? []).filter((f) =>
			f.type.startsWith('image/')
		);
		if (images.length > 0) {
			e.preventDefault();
			addFiles(images);
		}
	}

	function onDrop(e: DragEvent) {
		e.preventDefault();
		addFiles(e.dataTransfer?.files ?? null);
	}

	function onDragOver(e: DragEvent) {
		e.preventDefault();
	}

	async function retry(turnId: string) {
		if (!model.trim() || streaming) return;
		try {
			await fetch(`/api/v0/chat/sessions/${id}/turns/${turnId}/retry`, {
				method: 'POST',
				credentials: 'same-origin',
				headers: { 'content-type': 'application/json' },
				body: JSON.stringify({ model: model.trim() })
			});
			controller?.attach();
		} catch (err) {
			notice = String(err);
		}
	}

	async function editTurn(turnId: string, current: string) {
		const text = window.prompt('Edit your message:', current)?.trim();
		if (!text || !model.trim() || streaming || text === current) return;
		try {
			await fetch(`/api/v0/chat/sessions/${id}/turns/${turnId}/edit`, {
				method: 'POST',
				credentials: 'same-origin',
				headers: { 'content-type': 'application/json' },
				body: JSON.stringify({ model: model.trim(), message: text })
			});
			controller?.attach();
		} catch (err) {
			notice = String(err);
		}
	}

	async function toggleShare() {
		const shared = !(session?.shared ?? false);
		await fetch(`/api/v0/chat/sessions/${id}/share`, {
			method: 'POST',
			credentials: 'same-origin',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ shared })
		});
		await loadMeta();
	}

	async function stop() {
		try {
			await api.cancelChatTurn(id);
		} catch (err) {
			notice = String(err);
		}
	}

	/** Take a conversation shared with you into your own chats, then open it. */
	async function fork() {
		try {
			const forked = await api.forkChatSession(id);
			await refreshSidebar();
			await goto(`${base}/chat/${forked.id}`);
		} catch (err) {
			notice = String(err);
		}
	}

	async function removeAttachment(turnId: string, filename: string) {
		try {
			await api.removeChatAttachment(id, turnId, filename);
			// The turn's markers changed in the DB. Re-attaching replays a
			// snapshot (which replaces the turns wholesale), so the bubble shows
			// what is stored rather than a locally patched copy.
			controller?.attach();
		} catch (err) {
			notice = String(err);
		}
	}

	async function loadDocuments() {
		try {
			documents = (await api.listChatDocuments(id)).documents;
		} catch {
			// A conversation with no canvas is the normal case — stay quiet.
			documents = [];
		}
	}

	async function openDocument(docId: string) {
		try {
			const got = await api.getChatDocument(id, docId);
			openDoc = { document: got.document, content: got.version.content, history: got.history };
			docDraft = got.version.content;
			docEditing = false;
		} catch (err) {
			notice = String(err);
		}
	}

	async function saveDocument() {
		if (!openDoc) return;
		docSaving = true;
		try {
			await api.editChatDocument(id, openDoc.document.id, docDraft);
			await openDocument(openDoc.document.id);
			await loadDocuments();
		} catch (err) {
			notice = String(err);
		} finally {
			docSaving = false;
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

	function ts(iso: string | null): string {
		return iso ? new Date(iso).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }) : '';
	}

	function thinkingLabel(entry: (typeof turns)[number], live: boolean): string {
		const streaming = live && entry.turn.status === 'in_progress';
		const secs = entry.turn.reasoning_elapsed_ms ?? 0;
		return streaming ? 'Thinking…' : `Thought for ${(secs / 1000).toFixed(1)}s`;
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
	{#if isOwner}
		<button class="btn btn-ghost btn-sm" onclick={toggleShare}>
			{session?.shared ? 'Unshare' : 'Share'}
		</button>
	{:else}
		<!-- Shared *with* you: the copy is how you keep (and can edit) it. -->
		<button class="btn btn-ghost btn-sm" onclick={fork}>Save a copy</button>
	{/if}
	<div class="dropdown dropdown-end">
		<button class="btn btn-ghost btn-sm" popovertarget="export-menu" style="anchor-name:--export">
			Export
		</button>
		<ul
			class="dropdown-content menu rounded-box bg-base-200 p-2 shadow z-10 w-40"
			popover
			id="export-menu"
			style="position-anchor:--export"
		>
			<li><a href="/api/v0/chat/sessions/{id}/export.md" download>Markdown</a></li>
			<li><a href="/api/v0/chat/sessions/{id}/export.pdf" download>PDF</a></li>
		</ul>
	</div>
</div>

{#if documents.length > 0}
	<div class="card border border-base-300 bg-base-200 mb-4">
		<div class="card-body p-3 gap-2">
			<div class="flex items-center gap-2 flex-wrap">
				<span class="text-sm font-medium">Documents</span>
				{#each documents as doc (doc.id)}
					<button
						class="btn btn-xs {openDoc?.document.id === doc.id ? 'btn-primary' : 'btn-ghost'}"
						onclick={() => (openDoc?.document.id === doc.id ? (openDoc = null) : openDocument(doc.id))}
					>
						{doc.title}
						<span class="badge badge-ghost badge-xs">v{doc.current_ver}</span>
					</button>
				{/each}
			</div>

			{#if openDoc}
				{@const shown = openDoc}
				<div class="flex items-center gap-2">
					<span class="text-xs opacity-60">
						v{shown.document.current_ver} · {shown.history.length} revision{shown.history
							.length === 1
							? ''
							: 's'}
					</span>
					<div class="flex-1"></div>
					{#if isOwner}
						{#if docEditing}
							<button class="btn btn-xs" onclick={() => { docEditing = false; docDraft = shown.content; }}>
								Cancel
							</button>
							<button class="btn btn-xs btn-primary" onclick={saveDocument} disabled={docSaving}>
								{docSaving ? 'Saving…' : 'Save'}
							</button>
						{:else}
							<button class="btn btn-xs" onclick={() => (docEditing = true)}>Edit</button>
						{/if}
					{/if}
				</div>
				{#if docEditing}
					<textarea
						class="textarea textarea-bordered w-full font-mono text-sm"
						rows="14"
						bind:value={docDraft}
					></textarea>
				{:else}
					<div class="prose prose-sm max-w-none overflow-x-auto">
						<!-- eslint-disable-next-line svelte/no-at-html-tags -->
						{@html renderMarkdown(shown.content)}
					</div>
				{/if}
			{/if}
		</div>
	</div>
{/if}

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
			{@const parsed = parseUserContent(entry.turn.user_content)}
			<div class="chat chat-end">
				<div class="chat-bubble chat-bubble-primary">
					{#if parsed.attachments.length > 0}
						<div class="flex flex-wrap gap-2 mb-2 justify-end">
							{#each parsed.attachments as att (att.url)}
								<div class="relative group">
									{#if att.mime.startsWith('image/')}
										<a href={att.url} target="_blank" rel="noopener">
											<img src={att.url} alt={att.filename} class="rounded-lg max-h-48" />
										</a>
									{:else}
										<a href={att.url} class="btn btn-sm" download={att.filename}>
											{att.filename} ({Math.round(att.size / 1024)} KB)
										</a>
									{/if}
									{#if isOwner && !streaming}
										<button
											class="btn btn-xs btn-circle btn-error absolute -top-2 -right-2 opacity-0 group-hover:opacity-100 focus:opacity-100"
											aria-label="Remove {att.filename}"
											title="Remove {att.filename}"
											onclick={() => removeAttachment(entry.turn.id, att.filename)}
										>
											✕
										</button>
									{/if}
								</div>
							{/each}
						</div>
					{/if}
					{#if parsed.text}
						<div class="whitespace-pre-wrap">{parsed.text}</div>
					{/if}
				</div>
				{#if isOwner && !streaming}
					<div class="chat-footer opacity-60">
						<button class="btn btn-ghost btn-xs" onclick={() => editTurn(entry.turn.id, entry.turn.user_content ?? '')}>Edit</button>
					</div>
				{/if}
			</div>
		{:else}
			<div class="chat chat-start">
				<div class="chat-bubble chat-bubble-ghost w-full max-w-[min(90vw,48rem)] p-0">
					<div class="p-3 flex flex-col gap-2">
						{#if entry.turn.reasoning}
							<details class="collapse collapse-arrow text-sm -ms-2">
								<summary class="collapse-title cursor-pointer text-base-content/60 py-1 min-h-0 h-7">
									{thinkingLabel(entry, false)}
								</summary>
								<div class="collapse-content whitespace-pre-wrap text-xs text-base-content/70 max-h-64 overflow-y-auto">
									{entry.turn.reasoning}
								</div>
							</details>
						{/if}

						{#each entry.tool_calls as call (call.id)}
							<details class="collapse collapse-arrow bg-base-200/60 rounded-lg mb-1">
								<summary class="collapse-title text-sm py-1.5 min-h-0 h-8 flex items-center gap-2">
									{#if call.status === 'completed'}
										<span class="text-success">✓</span>
									{:else if call.status === 'errored'}
										<span class="text-error">✗</span>
									{:else}
										<span class="loading loading-spinner loading-xs"></span>
									{/if}
									<span class="text-base-content/60">Used</span>
									<span class="font-medium">{call.name}</span>
								</summary>
								<div class="collapse-content text-xs text-base-content/70">
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
						<div class="text-xs opacity-50">{ts(entry.turn.created_at)}</div>
						{#if isOwner && entry.turn.status !== 'in_progress' && !streaming}
							<div class="flex gap-1 mt-1">
								<button class="btn btn-ghost btn-xs" onclick={() => retry(entry.turn.id)}>Retry</button>
							</div>
						{/if}
					</div>
				</div>
			</div>
		{/if}
	{/each}
</div>

<div class="card border border-base-300 sticky bottom-0">
	<div class="card-body p-3 gap-2">
		<div class="flex flex-wrap gap-2 items-center mb-2">
			<button class="btn btn-ghost btn-xs gap-1" onclick={() => (toolsOpen = !toolsOpen)}>
				+ Tools
			</button>
			{#each tools.filter((t) => t.enabled) as t (t.key)}
				<span class="badge badge-outline badge-sm gap-1">
					{t.title}
					<button class="text-error" aria-label="Disable {t.title}" onclick={() => toggleCapability(t.key, true)}>✕</button>
				</span>
			{/each}
			<span class="flex-1"></span>
			<select class="select select-bordered select-xs" aria-label="Reasoning effort" bind:value={effort} onchange={saveEffort}>
				{#each EFFORTS as [value, label] (value)}
					<option value={value}>{label}</option>
				{/each}
			</select>
		</div>
		{#if toolsOpen}
			<div class="flex flex-wrap gap-2 mb-2 border border-base-300 rounded-lg p-2">
				{#each tools as t (t.key)}
					<label class="label cursor-pointer gap-1">
						<input
							type="checkbox"
							class="checkbox checkbox-xs"
							checked={t.enabled}
							onchange={() => toggleCapability(t.key, t.enabled)}
						/>
						<span class="label-text text-xs">{t.title}</span>
					</label>
				{/each}
			</div>
		{/if}
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
				onpaste={onPaste}
				ondragover={onDragOver}
				ondrop={onDrop}
				disabled={streaming}
			></textarea>
			<label class="btn btn-ghost btn-square" aria-label="Attach files" title="Attach files">
				<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m21.44 11.05-9.19 9.19a6 6 0 0 1-8.49-8.49l8.57-8.57A4 4 0 1 1 18 8.84l-8.59 8.57a2 2 0 0 1-2.83-2.83l8.49-8.48"/></svg>
				<input type="file" multiple class="hidden" onchange={(e) => { addFiles((e.currentTarget as HTMLInputElement).files); (e.currentTarget as HTMLInputElement).value = ''; }} />
			</label>
			{#if streaming}
				<button class="btn btn-error" onclick={stop}>Stop</button>
			{:else}
				<button class="btn btn-primary" onclick={send} disabled={(!draft.trim() && files.length === 0) || !model.trim() || sending}>
					Send
				</button>
			{/if}
			<button class="btn btn-ghost btn-square" onclick={openVoice} aria-label="Voice mode" title="Voice mode">
				<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" class="inline-block align-text-bottom" aria-hidden="true">
					<rect x="9" y="2" width="6" height="11" rx="3" />
					<path d="M5 10v1a7 7 0 0 0 14 0v-1" />
					<path d="M12 18v3" />
					<path d="M8 22h8" />
				</svg>
			</button>
		</div>
	</div>
</div>

{#if voiceOpen.open && voice}
	<dialog class="modal modal-open" aria-label="Voice mode">
		<div class="modal-box max-w-sm">
			<div class="flex flex-col items-center gap-4 py-4">
				<button
					class="btn btn-circle btn-lg {voice.state.phase === 'listening'
						? 'btn-error animate-pulse'
						: voice.state.phase === 'speaking'
							? 'btn-primary'
							: 'btn-neutral'}"
					onclick={() => voice?.tap(model)}
					aria-label={voice.state.phase === 'listening' ? 'Stop and send' : 'Talk'}
				>
					<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="28" height="28" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
						<rect x="9" y="2" width="6" height="11" rx="3" />
						<path d="M5 10v1a7 7 0 0 0 14 0v-1" />
						<path d="M12 18v3" />
						<path d="M8 22h8" />
					</svg>
				</button>
				<p class="text-sm font-medium">
					{#if voice.state.phase === 'listening'}
						Listening — tap to send
					{:else if voice.state.phase === 'working'}
						Working…
					{:else if voice.state.phase === 'speaking'}
						Speaking — tap to interrupt
					{:else}
						Tap to talk
					{/if}
				</p>
				{#if voice.state.captionUser}
					<p class="text-xs text-base-content/60 w-full text-left"><strong>You:</strong> {voice.state.captionUser}</p>
				{/if}
				{#if voice.state.captionAi}
					<p class="text-xs text-base-content/60 w-full text-left"><strong>AI:</strong> {voice.state.captionAi}</p>
				{/if}
				{#if voice.state.note}
					<div class="alert alert-warning py-2"><span>{voice.state.note}</span></div>
				{/if}
			</div>
			<div class="modal-action">
				<button class="btn btn-ghost btn-sm" onclick={closeVoice}>Close</button>
			</div>
		</div>
		<form method="dialog" class="modal-backdrop"><button onclick={closeVoice}>close</button></form>
	</dialog>
{/if}
