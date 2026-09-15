<script lang="ts">
	import { onMount } from 'svelte';
	import { base } from '$app/paths';
	import { goto } from '$app/navigation';
	import { api, ApiError } from '$lib/api';
	import type { CanvasDocument, ChatAsset, ChatCapability } from '$lib/api';
	import { createConversationController } from '$lib/chat.svelte';
	import { createVoiceController } from '$lib/voice.svelte';
	import { refreshSidebar, sidebar } from '$lib/sidebar.svelte';
	import { parseUserContent, replaceUserText } from '$lib/chat-protocol';
	import type { ChatSession } from '$lib/chat-protocol';
	import { canonicalAttachments } from '$lib/conversation-assets';
	import { DragDepth, carriesFiles, droppedOnlyDirectories, filesFrom } from '$lib/drop-files';
	import { me } from '$lib/session.svelte';
	import { t, time, n } from '$lib/i18n.svelte';
	import ConversationHeader from '$lib/components/chat/ConversationHeader.svelte';
	import DictationButton from '$lib/components/chat/DictationButton.svelte';
	import CapabilityPicker from '$lib/components/chat/CapabilityPicker.svelte';
	import Markdown from '$lib/components/chat/Markdown.svelte';
	import ToolCalls from '$lib/components/chat/ToolCalls.svelte';
	import ConversationCanvas from '$lib/components/chat/ConversationCanvas.svelte';
	import MessageAttachments from '$lib/components/chat/MessageAttachments.svelte';
	import { feedback, openDialog as openFeedback } from '$lib/feedback.svelte';
	import { clearPageTitleOverride, setPageTitleOverride } from '$lib/page-title';
	import { page } from '$app/state';

	// One conversation. The route keys this component on the id, so an
	// instance belongs to exactly one conversation for its whole life —
	// which is what lets every `$state` below simply be per-conversation
	// state, with no reset list to maintain.
	let { id } = $props<{ id: string }>();

	let controller = $state<ReturnType<typeof createConversationController> | null>(null);
	let session = $state<ChatSession | null>(null);
	let model = $state('');
	let models = $state<{ id: string; gdpr: boolean; nda: boolean; reasoning: boolean }[]>([]);
	let transcriptionModels = $state<string[]>([]);
	let transcriptionModel = $state('');
	let speechAvailable = $state(false);
	let speechVoices = $state<string[]>([]);
	let speechVoice = $state('');
	let draft = $state('');
	let files = $state<File[]>([]);
	let tools = $state<ChatCapability[]>([]);
	let effort = $state('standard');
	// Levels, not labels: the option text is looked up in the template so a
	// language switch re-renders the picker (same reason as the layout's nav).
	const EFFORTS = ['fast', 'standard', 'deep', 'max'] as const;
	let sending = $state(false);
	let notice = $state<string | null>(null);
	let compactedUpToSeq = $state<number | null>(null);
	let assets = $state<ChatAsset[]>([]);
	let clock = $state(Date.now());
	let editingTurn = $state<{ id: string; content: string } | null>(null);
	let editDraft = $state('');
	let editFiles = $state<File[]>([]);
	let promptChoices = $state<string[]>([]);

	let documents = $state<CanvasDocument[]>([]);
	let canvasOpen = $state(false);

	const turns = $derived(controller ? controller.state.turns : []);
	const streaming = $derived(controller !== null && controller.state.liveTurnId !== null);
	const prompt = $derived(controller?.state.prompt ?? null);
	const selectedModel = $derived(models.find((candidate) => candidate.id === model));
	/**
	 * Whether the effort control does anything for the selected model.
	 *
	 * The server resolves this exactly as the request path does. A model the
	 * gateway has no reasoning parameter for gets a disabled picker with a
	 * reason, rather than a working-looking select that changes nothing —
	 * which is the same lie as an effort parameter dropped in silence, just on
	 * the other side of the wire.
	 *
	 * Free-typed model names are not in the list, and there we do not know, so
	 * the control stays enabled rather than being wrongly greyed out.
	 */
	const effortApplies = $derived(selectedModel?.reasoning ?? true);
	const hasCanvas = $derived(documents.length > 0 || assets.length > 0);
	// The composer's feedback button captures the page before the dialog opens,
	// so it needs the same busy state the floating button has elsewhere.
	const feedbackCapturing = $derived(feedback.shotStatus === 'capturing' && !feedback.open);
	/** A conversation shared *with* you renders read-only, plus a Fork action. */
	const isOwner = $derived(
		session === null || me.value === null || session.user_id === me.value.id
	);
	let metaRequest = 0;

	$effect(() => {
		const pathname = page.url.pathname;
		setPageTitleOverride(
			pathname,
			t('page-title-branded', { title: session?.title || t('chat-default-title') })
		);
		return () => clearPageTitleOverride(pathname);
	});

	async function loadMeta() {
		const request = ++metaRequest;
		try {
			const snap = await api.getChatSession(id);
			if (request !== metaRequest) return;
			session = snap.session;
			compactedUpToSeq = snap.compacted_up_to_seq;
			assets = snap.assets;
			if (snap.assets.length > 0 && window.innerWidth >= 768) canvasOpen = true;
			// Prefill the model picker from the conversation's last assistant
			// turn — the "keep talking to what you were talking to" default.
			const lastModel = [...snap.turns]
				.reverse()
				.find((e) => e.turn.role === 'assistant' && e.turn.model)?.turn.model;
			// A conversation with no assistant turn to learn from falls back to
			// the first offered model, or the composer would open with an empty
			// picker and refuse to send.
			if (!model) model = lastModel ?? models[0]?.id ?? '';
		} catch (err) {
			if (request !== metaRequest) return;
			notice = String(err);
		}
	}

	async function refreshConversationMeta() {
		await Promise.all([loadMeta(), refreshSidebar()]);
		const sidebarSession = sidebar.sessions.find((candidate) => candidate.id === id);
		if (session && sidebarSession?.title) {
			session = { ...session, title: sidebarSession.title };
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

	async function loadVoiceConfig() {
		try {
			const config = await api.chatVoiceConfig();
			transcriptionModels = config.data;
			transcriptionModel = config.data[0] ?? '';
			speechAvailable = config.speech_available;
			speechVoices = config.speech_voices;
			speechVoice = config.speech_voice ?? '';
		} catch {
			transcriptionModels = [];
			speechAvailable = false;
			speechVoices = [];
		}
	}

	async function saveSpeechVoice() {
		try {
			await api.setSpeechVoice(speechVoice);
		} catch (caught) {
			notice = String(caught);
		}
	}

	async function setCapability(capability: ChatCapability, state: ChatCapability['state']) {
		try {
			await api.setChatCapability(id, capability.kind, capability.key, state);
			tools = tools.map((tool) =>
				tool.kind === capability.kind && tool.key === capability.key ? { ...tool, state } : tool
			);
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
		// `loadModels` was defined and never called, which left `models` empty
		// and the picker permanently in its free-text fallback.
		void loadModels();
		void loadTools();
		void loadVoiceConfig();
		const c = createConversationController(id);
		c.onSidebarChanged = () => {
			void refreshConversationMeta();
			// A turn that wrote to the canvas bumps the sidebar too, so this is
			// also the cue to re-read the document list.
			void loadDocuments();
		};
		c.onTurnFinalized = () => void refreshConversationMeta();
		c.attach();
		controller = c;
		void loadMeta();
		void loadDocuments();
		const timer = window.setInterval(() => (clock = Date.now()), 100);
		return () => {
			window.clearInterval(timer);
			c.destroy();
		};
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

	function appendTranscript(text: string) {
		draft = draft.trim() ? `${draft.trimEnd()} ${text}` : text;
	}

	function closeVoice() {
		voice?.close();
		voice = null;
		voiceOpen.open = false;
	}

	// Which turn the voice controller is currently narrating. A plain `let`,
	// deliberately NOT `$state`: the effect below both reads and writes it, and
	// making it reactive would feed the effect its own output.
	let narratingTurnId: string | null = null;

	// Feed the live reply into the voice controller whenever a voice-submitted
	// turn is streaming (state drives it, no DOM observers).
	//
	// The latch is what makes the *last* sentence audible. `turn_finalized`
	// sets the turn's status and clears `liveTurnId` in the same synchronous
	// call, and Svelte only runs effects after that work — so an effect that
	// bailed on `!liveTurnId` could never observe "finalized". It therefore
	// never passed `final: true`, and since `splitSentences` holds back the
	// trailing sentence (it needs whitespace after the terminator to know the
	// sentence ended), a short reply like "Sure, that's 42." was spoken
	// entirely never, and `stopFeeding` never ran so the phase machine hung.
	$effect(() => {
		if (!voice || !controller) return;
		const liveId = controller.state.liveTurnId;
		if (liveId) narratingTurnId = liveId;
		const tracked = narratingTurnId;
		if (!tracked) return;
		const entry = controller.state.turns.find((e) => e.turn.id === tracked);
		if (!entry) return;
		const content = entry.turn.content ?? '';
		const finalized = entry.turn.status !== 'in_progress';
		voice.feedReplyText(content, finalized);
		if (finalized) narratingTurnId = null;
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
					? t('chat-error-still-streaming')
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

	// Whole-conversation drop target. The composer input used to be
	// the only one, which made attaching by drag a game of darts — and an
	// unwinnable one while a turn streamed, because a `disabled` textarea
	// fires no drop events at all. Dropping anywhere else navigated the tab
	// away from the chat to render the file. Now the page takes the drop,
	// the overlay says so, and the files land in whichever composer is open.
	const dragDepth = new DragDepth();
	let dragging = $state(false);

	/// Files dropped while the edit dialog is open belong to *that* message,
	/// not to a new one.
	function stageFiles(dropped: File[]) {
		if (dropped.length === 0) return;
		if (editingTurn) editFiles = [...editFiles, ...dropped];
		else addFiles(dropped);
	}

	function onDragEnter(e: DragEvent) {
		if (!isOwner || !carriesFiles(e.dataTransfer)) return;
		e.preventDefault();
		dragging = dragDepth.enter();
	}

	function onDragLeave() {
		if (!dragging) return;
		dragging = dragDepth.leave();
	}

	function onDrop(e: DragEvent) {
		if (!isOwner || !carriesFiles(e.dataTransfer)) return;
		// Without this the browser opens the file in the tab, taking the
		// conversation with it.
		e.preventDefault();
		dragging = dragDepth.reset();
		if (droppedOnlyDirectories(e.dataTransfer)) {
			notice = t('render-drop-folders-unsupported');
			return;
		}
		stageFiles(filesFrom(e.dataTransfer));
	}

	function onDragOver(e: DragEvent) {
		if (!isOwner || !carriesFiles(e.dataTransfer)) return;
		// A drop only happens where dragover was prevented — on every frame,
		// not once on enter.
		e.preventDefault();
		if (e.dataTransfer) e.dataTransfer.dropEffect = 'copy';
	}

	/// POST a JSON body and fail loudly.
	///
	/// `fetch` resolves for a 4xx/5xx — only a network error rejects — so a
	/// bare `await fetch(...)` silently accepts "409 a turn is already
	/// streaming" and then re-attaches as if it had worked. Every action here
	/// funnels through this so a refusal reaches the notice bar.
	async function postJson(path: string, body: unknown): Promise<void> {
		const res = await fetch(path, {
			method: 'POST',
			credentials: 'same-origin',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify(body)
		});
		if (!res.ok) throw new Error((await res.text()).slice(0, 200) || res.statusText);
	}

	async function retry(turnId: string) {
		if (!model.trim() || streaming) return;
		if (!window.confirm(t('render-retry-confirm'))) return;
		try {
			await postJson(`/api/v0/chat/sessions/${id}/turns/${turnId}/retry`, {
				model: model.trim()
			});
			controller?.attach();
		} catch (err) {
			notice = String(err);
		}
	}

	function editTurn(turnId: string, current: string) {
		editingTurn = { id: turnId, content: current };
		editDraft = parseUserContent(current).text;
		editFiles = [];
	}

	async function saveTurnEdit() {
		if (!editingTurn || !editDraft.trim() || !model.trim() || streaming) return;
		if (!window.confirm(t('render-edit-confirm'))) return;
		try {
			const form = new FormData();
			form.append('model', model.trim());
			form.append('message', replaceUserText(editingTurn.content, editDraft));
			for (const file of editFiles) form.append('attachment', file);
			const response = await fetch(`/api/v0/chat/sessions/${id}/turns/${editingTurn.id}/edit`, {
				method: 'POST',
				credentials: 'same-origin',
				body: form
			});
			if (!response.ok) throw new Error((await response.text()).slice(0, 200) || response.statusText);
			editingTurn = null;
			editFiles = [];
			controller?.attach();
		} catch (err) {
			notice = String(err);
		}
	}

	async function toggleShare() {
		const shared = !(session?.shared ?? false);
		try {
			await postJson(`/api/v0/chat/sessions/${id}/share`, { shared });
		} catch (err) {
			notice = String(err);
			return;
		}
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
		if (!window.confirm(t('render-attachment-remove-confirm', { filename }))) return;
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
			if (documents.length > 0 && window.innerWidth >= 768) canvasOpen = true;
		} catch {
			// A conversation with no canvas is the normal case — stay quiet.
			documents = [];
		}
	}

	/// Answer the tool's question.
	///
	/// A clicked option goes in `choices`; only free text goes in `text`. The
	/// distinction is load-bearing, not cosmetic: `ask_user::confirm` treats a
	/// matching entry in `choices` as the ONLY form of consent, precisely so
	/// that "yes, but move it to 07:00" is read as a change request rather
	/// than approval. Sending every answer as `text` (which this did) made
	/// `Confirmation::Approved` unreachable, so a user clicking "Yes, schedule
	/// it" got a turn reporting that they had declined.
	async function answerPrompt(choice: string | null, freeText?: string, choices = choice === null ? [] : [choice]) {
		if (!prompt || prompt.action !== 'show') return;
		const payload =
			choice === null && freeText === undefined
				? { dismissed: true }
				: { choices, text: freeText ?? null };
		try {
			await postJson(`/api/v0/me/ask/feedback/${encodeURIComponent(prompt.turn_id)}`, payload);
		} catch (err) {
			notice = String(err);
		} finally {
			if (controller) controller.state.prompt = null;
		}
	}

	function togglePromptChoice(option: string) {
		promptChoices = promptChoices.includes(option)
			? promptChoices.filter((choice) => choice !== option)
			: [...promptChoices, option];
	}

	async function sharePromptLocation() {
		if (!prompt || prompt.action !== 'show' || prompt.kind !== 'location') return;
		try {
			const position = await new Promise<GeolocationPosition>((resolve, reject) =>
				navigator.geolocation.getCurrentPosition(resolve, reject, { enableHighAccuracy: true, timeout: 10_000 })
			);
			await postJson(`/api/v0/me/location/feedback/${encodeURIComponent(prompt.turn_id)}`, {
				lat: position.coords.latitude,
				lon: position.coords.longitude,
				accuracy: position.coords.accuracy
			});
			if (controller) controller.state.prompt = null;
		} catch (err) {
			notice = String(err);
		}
	}

	async function declinePromptLocation() {
		if (!prompt || prompt.action !== 'show' || prompt.kind !== 'location') return;
		try {
			await postJson(`/api/v0/me/location/feedback/${encodeURIComponent(prompt.turn_id)}`, { denied: true });
		} catch (err) {
			notice = String(err);
		} finally {
			if (controller) controller.state.prompt = null;
		}
	}

	let promptText = $state('');
	function submitPrompt() {
		const value = promptText.trim();
		// Free text: no `choices`, so a confirmation reads it as "not approved"
		// and the model gets the words rather than a yes.
		if (value || promptChoices.length > 0) void answerPrompt('', value || undefined, promptChoices);
		promptText = '';
		promptChoices = [];
	}

	function thinkingLabel(entry: (typeof turns)[number], live: boolean): string {
		const streaming = live && entry.turn.status === 'in_progress';
		const started = entry.turn.reasoning_started_at ? Date.parse(entry.turn.reasoning_started_at) : NaN;
		const elapsed = Number.isFinite(started) ? Math.max(0, clock - started) : 0;
		const secs = entry.turn.reasoning_elapsed_ms ?? elapsed;
		return streaming
			? t('render-thinking-in-progress', { secs: n(secs / 1000, { minimumFractionDigits: 1, maximumFractionDigits: 1 }) })
			: t('render-thinking-finalized', { secs: n(secs / 1000, { minimumFractionDigits: 1, maximumFractionDigits: 1 }) });
	}

	function onKeydown(event: KeyboardEvent) {
		if (event.key === 'Enter' && !event.shiftKey) {
			event.preventDefault();
			void send();
		}
	}
</script>

<!-- The drop target is the whole conversation, not just the composer box:
     a file dragged onto the transcript is meant for the next message, and
     landing it anywhere else would navigate the tab away from the chat. The
     handlers no-op for a read-only shared conversation and for drags that
     carry no files (selecting text inside a bubble fires the same events). -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
	class="relative flex h-full min-h-0 flex-col"
	data-chat-dropzone
	ondragenter={onDragEnter}
	ondragover={onDragOver}
	ondragleave={onDragLeave}
	ondrop={onDrop}
>
	<ConversationHeader
		{id}
		title={session?.title}
		{isOwner}
		shared={session?.shared ?? false}
		{models}
		bind:model
		{transcriptionModels}
		bind:transcriptionModel
		{speechAvailable}
		{speechVoices}
		bind:speechVoice
		{hasCanvas}
		oncanvas={() => (canvasOpen = !canvasOpen)}
		onshare={toggleShare}
		onfork={fork}
		onspeechvoice={saveSpeechVoice}
	/>

	{#if !isOwner}
		<div class="alert alert-info mb-4"><span>{t('chat-render-shared-readonly-banner')}</span></div>
	{/if}
	{#if selectedModel && !selectedModel.gdpr}
		<div class="alert alert-warning mb-4"><span>{t('chat-render-gdpr-banner')}</span></div>
	{/if}
	{#if selectedModel && !selectedModel.nda}
		<div class="alert alert-warning mb-4"><span>{t('chat-render-nda-banner')}</span></div>
	{/if}

	<div class="flex min-h-0 flex-1 gap-3" data-chat-workspace>
		<main data-chat-transcript class="min-h-0 min-w-0 flex-1 overflow-y-auto pe-1 xl:min-w-[35rem]">

{#if notice}
	<div class="alert alert-warning mb-4"><span>{notice}</span></div>
{/if}

{#if prompt?.action === 'show'}
	{@const shown = prompt}
	<div class="card border border-warning mb-4">
		<div class="card-body">
			<h2 class="card-title text-base">{shown.header ?? t('chat-prompt-heading')}</h2>
			<p>{shown.question}</p>
			{#if shown.kind === 'location'}
				<div class="mt-2 flex flex-wrap gap-2">
					<button class="btn btn-primary btn-sm" onclick={sharePromptLocation}>{t('tools-location-share-button')}</button>
					<button class="btn btn-ghost btn-sm" onclick={declinePromptLocation}>{t('chat-prompt-skip')}</button>
				</div>
			{:else if shown.options.length > 0}
				<div class="flex flex-wrap gap-2 mt-1">
					{#each shown.options as option (option)}
						<button
							class="btn btn-sm {shown.multi_select && promptChoices.includes(option) ? 'btn-primary' : 'btn-outline'}"
							onclick={() => shown.multi_select ? togglePromptChoice(option) : answerPrompt(option)}
						>
							{option}
						</button>
					{/each}
				</div>
			{/if}
			{#if shown.kind !== 'location'}
			<div class="join mt-2 w-full">
				<input
					class="input input-bordered input-sm join-item w-full"
					placeholder={t('chat-prompt-placeholder')}
					bind:value={promptText}
					onkeydown={(e) => e.key === 'Enter' && submitPrompt()}
				/>
				<button class="btn btn-primary btn-sm join-item" onclick={submitPrompt}>
					{t('chat-prompt-answer')}
				</button>
				<button class="btn btn-ghost btn-sm join-item" onclick={() => answerPrompt(null)}>
					{t('chat-prompt-skip')}
				</button>
			</div>
			{/if}
		</div>
	</div>
{/if}

<div class="mb-4 flex flex-1 flex-col gap-4">
	{#each turns as entry, index (entry.turn.id)}
		{#if index > 0 && compactedUpToSeq !== null && turns[index - 1].turn.seq <= compactedUpToSeq && entry.turn.seq > compactedUpToSeq}
			<div class="divider my-2 text-xs opacity-60" role="separator" aria-label={t('render-compaction-divider')}>
				<span aria-hidden="true">ⓘ</span> {t('render-compaction-divider')}
			</div>
		{/if}
		{#if entry.turn.role === 'user'}
			{@const parsed = parseUserContent(entry.turn.user_content)}
			{@const shownAttachments = canonicalAttachments(parsed.attachments, assets, entry.turn.id)}
			<div class="chat chat-end">
				<div class="chat-bubble border border-primary/20 bg-primary/10 text-base-content backdrop-blur-sm">
					{#if shownAttachments.length > 0}
						<MessageAttachments attachments={shownAttachments} removable={isOwner && !streaming} onremove={(filename) => removeAttachment(entry.turn.id, filename)} />
					{/if}
					{#if parsed.text}
						<div class="whitespace-pre-wrap">{parsed.text}</div>
					{/if}
				</div>
				{#if isOwner && !streaming}
					<div class="chat-footer opacity-60">
						<button class="btn btn-ghost btn-xs" onclick={() => editTurn(entry.turn.id, entry.turn.user_content ?? '')}>
							{t('render-edit-button')}
						</button>
					</div>
				{/if}
			</div>
		{:else}
			{@const parsed = parseUserContent(entry.turn.content)}
			{@const shownAttachments = canonicalAttachments(parsed.attachments, assets, entry.turn.id)}
			<div class="chat chat-start">
				<div class="chat-bubble w-full max-w-[min(90vw,48rem)] border border-base-300/60 bg-base-200/35 p-0 backdrop-blur-sm">
					<div class="p-3 flex min-w-0 flex-col gap-2">
						{#if entry.turn.reasoning}
							<details class="collapse collapse-arrow text-sm -ms-2">
								<summary class="collapse-title cursor-pointer text-base-content/60 py-1 min-h-0 h-7">
									{thinkingLabel(entry, true)}
								</summary>
								<div class="collapse-content whitespace-pre-wrap text-xs text-base-content/70 max-h-64 overflow-y-auto">
									{entry.turn.reasoning}
								</div>
							</details>
						{/if}

						<ToolCalls calls={entry.tool_calls} />

						{#if entry.turn.status === 'in_progress'}
							<div class="flex items-center gap-2 text-sm text-base-content/60">
								<span class="loading loading-dots loading-sm"></span>
								<span>{entry.turn.content ? t('render-still-working-spinner') : t('render-thinking-spinner')}</span>
							</div>
						{/if}

						{#if shownAttachments.length > 0}<MessageAttachments attachments={shownAttachments} removable={isOwner && !streaming} onremove={(filename) => removeAttachment(entry.turn.id, filename)} />{/if}
						<Markdown
							content={parsed.text}
							class="prose prose-sm chat-prose min-w-0 max-w-none"
							images={[...shownAttachments, ...assets]}
							hiddenImageUrls={new Set(shownAttachments.map((attachment) => attachment.url))}
						/>

						{#if entry.turn.status === 'errored'}
							<div class="alert alert-error py-2"><span>{entry.turn.error_message}</span></div>
						{:else if entry.turn.status === 'cancelled'}
							<div class="text-xs text-base-content/50">{t('chat-turn-stopped')}</div>
						{/if}
						<div class="text-xs opacity-50">{time(entry.turn.created_at)}</div>
						{#if isOwner && entry.turn.status !== 'in_progress' && !streaming}
							<div class="flex gap-1 mt-1">
								<button class="btn btn-ghost btn-xs" onclick={() => retry(entry.turn.id)}>
									{t('render-retry-button')}
								</button>
							</div>
						{/if}
					</div>
				</div>
			</div>
		{/if}
	{/each}
</div>

		</main>
		{#if canvasOpen && hasCanvas}
			<ConversationCanvas {id} {documents} {assets} {isOwner} onclose={() => (canvasOpen = false)} onerror={(message) => (notice = message)} />
		{/if}
	</div>

{#if isOwner}
<div data-chat-composer class="card mt-3 w-full shrink-0 border border-base-300 bg-base-100/85 backdrop-blur-sm">
	<div class="flex flex-col gap-1 p-2">
		<div class="flex flex-wrap items-center gap-2">
			<CapabilityPicker capabilities={tools} onset={setCapability} />
			<span class="flex-1"></span>
			<select
				class="select select-bordered select-xs w-auto max-w-48"
				aria-label={t('chat-render-effort-title')}
				title={effortApplies ? t('chat-render-effort-tooltip') : t('chat-render-effort-unsupported')}
				disabled={!effortApplies}
				bind:value={effort}
				onchange={saveEffort}
			>
				{#each EFFORTS as level (level)}
					<option value={level}>
						{t('chat-render-effort-label-prefix')} {t(`chat-render-effort-${level}`)}
					</option>
				{/each}
			</select>
			<!-- The feedback entry point for conversation pages. The floating
			     button is suppressed here (it would sit on top of send/stop),
			     so this is the only one — never width-gated.
			
			     Styled as `btn-primary`, exactly like the floating button it
			     stands in for: that is a filled circle (white on the dark
			     theme, near-black on the light one — see `--color-primary` in
			     app.css), and it is how people recognise this control. It had
			     been `btn-ghost btn-xs` here, which left a bare outline
			     smaller than every other control in the composer — findable
			     only if you already knew it was there. -->
			<button
				class="btn btn-circle btn-primary btn-sm shadow"
				data-feedback-fab
				onclick={() => void openFeedback()}
				disabled={feedbackCapturing}
				aria-label={t('feedback-fab-aria')}
				title={t('feedback-fab-aria')}
			>
				{#if feedbackCapturing}
					<span class="loading loading-spinner loading-xs"></span>
				{:else}
					<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m8 2 1.88 1.88M14.12 3.88 16 2"/><path d="M9 7.13V6a3 3 0 1 1 6 0v1.13"/><path d="M12 20c-3.3 0-6-2.7-6-6v-3a4 4 0 0 1 4-4h4a4 4 0 0 1 4 4v3c0 3.3-2.7 6-6 6Z"/><path d="M6 13H2M6 17H3M6 9H3M18 13h4M18 17h3M18 9h3"/></svg>
				{/if}
			</button>
		</div>
		<div class="flex items-end gap-1">
			<textarea
				class="textarea textarea-ghost min-h-11 max-h-48 flex-1 resize-none focus:outline-none"
				rows="1"
				placeholder={t('chat-render-composer-placeholder')}
				bind:value={draft}
				onkeydown={onKeydown}
				onpaste={onPaste}
				disabled={streaming}
			></textarea>
			<label
				class="btn btn-sm btn-circle btn-ghost"
				aria-label={t('render-composer-attach-aria')}
				title={t('render-composer-attach-title')}
			>
				<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m21.44 11.05-9.19 9.19a6 6 0 0 1-8.49-8.49l8.57-8.57A4 4 0 1 1 18 8.84l-8.59 8.57a2 2 0 0 1-2.83-2.83l8.49-8.48"/></svg>
				<input type="file" multiple class="hidden" onchange={(e) => { addFiles((e.currentTarget as HTMLInputElement).files); (e.currentTarget as HTMLInputElement).value = ''; }} />
			</label>
			{#if transcriptionModels.length > 0}
				<DictationButton model={transcriptionModel} ontranscript={appendTranscript} onerror={(message) => (notice = message)} />
			{/if}
			{#if speechAvailable && transcriptionModels.length > 0}
				<button
					class="btn btn-sm btn-circle btn-ghost"
					onclick={openVoice}
					aria-label={t('voice-toggle-title')}
					title={t('voice-toggle-title')}
				>
					<svg viewBox="0 0 24 24" width="17" height="17" fill="currentColor" aria-hidden="true"><path d="M3 10h2v4H3zm4-4h2v12H7zm4-3h2v18h-2zm4 5h2v8h-2zm4 2h2v4h-2z" /></svg>
				</button>
			{/if}
			{#if streaming}
				<button class="btn btn-sm btn-circle btn-error" onclick={stop} aria-label={t('render-composer-stop')} title={t('render-composer-stop')}>
					<svg viewBox="0 0 24 24" width="16" height="16" fill="currentColor" aria-hidden="true"><rect x="6" y="6" width="12" height="12" rx="1" /></svg>
				</button>
			{:else}
				<button class="btn btn-sm btn-circle btn-primary" onclick={send} disabled={(!draft.trim() && files.length === 0) || !model.trim() || sending} aria-label={t('render-composer-send')} title={t('render-composer-send')}>
					<svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="M12 19V5m0 0-6 6m6-6 6 6" /></svg>
				</button>
			{/if}
		</div>
		{#if files.length > 0}
			<div class="flex flex-wrap gap-1">
				{#each files as file, index (`${file.name}-${file.size}-${file.lastModified}`)}
					<span class="badge badge-outline gap-1">
						{file.name}
						<button type="button" aria-label={t('render-attachment-remove-title', { filename: file.name })} onclick={() => (files = files.filter((_, candidate) => candidate !== index))}>×</button>
					</span>
				{/each}
			</div>
		{/if}
	</div>
</div>
{/if}

	{#if dragging}
		<!-- `pointer-events-none` is load-bearing: an overlay that swallowed
		     pointer events would fire dragleave the instant it appeared, so the
		     highlight would strobe and the drop would land on nothing. -->
		<div
			data-chat-drop-overlay
			class="pointer-events-none absolute inset-0 z-30 flex flex-col items-center justify-center gap-1 rounded-box border-2 border-dashed border-primary bg-base-100/80 backdrop-blur-sm"
		>
			<span class="text-lg font-semibold">{t('render-drop-overlay')}</span>
			<span class="text-sm opacity-70">{t('render-drop-overlay-hint')}</span>
		</div>
	{/if}
</div>

{#if editingTurn}
	<dialog class="modal modal-open" aria-label={t('render-edit-prompt')}>
		<!-- The dialog is a sibling of the conversation, not a child, so the
		     page-wide dropzone above never sees a drop landing on it. Same
		     handlers, and `stageFiles` routes them to this message's files. -->
		<!-- svelte-ignore a11y_no_static_element_interactions -->
		<div
			class="modal-box {dragging ? 'outline outline-2 outline-dashed outline-primary' : ''}"
			ondragenter={onDragEnter}
			ondragover={onDragOver}
			ondragleave={onDragLeave}
			ondrop={onDrop}
		>
			<h2 class="text-lg font-semibold">{t('render-edit-prompt')}</h2>
			<textarea class="textarea textarea-bordered mt-3 min-h-36 w-full" bind:value={editDraft}></textarea>
			{#if editFiles.length > 0}
				<div class="mt-2 flex flex-wrap gap-1">
					{#each editFiles as file, index (`${file.name}-${file.size}-${file.lastModified}`)}
						<span class="badge badge-outline gap-1">{file.name}<button type="button" aria-label={t('render-attachment-remove-title', { filename: file.name })} onclick={() => (editFiles = editFiles.filter((_, candidate) => candidate !== index))}>×</button></span>
					{/each}
				</div>
			{/if}
			<div class="modal-action">
				<label class="btn btn-ghost btn-sm" aria-label={t('render-composer-attach-aria')} title={t('render-composer-attach-title')}>
					{t('render-composer-attach-aria')}
					<input type="file" multiple class="hidden" onchange={(event) => { editFiles = [...editFiles, ...Array.from((event.currentTarget as HTMLInputElement).files ?? [])]; (event.currentTarget as HTMLInputElement).value = ''; }} />
				</label>
				<button class="btn btn-ghost btn-sm" onclick={() => (editingTurn = null)}>{t('render-edit-cancel')}</button>
				<button class="btn btn-primary btn-sm" disabled={!editDraft.trim()} onclick={saveTurnEdit}>{t('render-edit-save')}</button>
			</div>
		</div>
		<form method="dialog" class="modal-backdrop"><button onclick={() => (editingTurn = null)}>{t('render-edit-cancel')}</button></form>
	</dialog>
{/if}

{#if voiceOpen.open && voice}
	<dialog class="modal modal-open" aria-label={t('voice-modal-title')}>
		<div class="modal-box max-w-sm">
			<div class="flex flex-col items-center gap-4 py-4">
				<button
					class="btn btn-circle btn-lg {voice.state.phase === 'listening'
						? 'btn-error animate-pulse'
						: voice.state.phase === 'speaking'
							? 'btn-primary'
							: 'btn-neutral'}"
					onclick={() => voice?.tap(transcriptionModel)}
					aria-label={voice.state.phase === 'listening'
						? t('voice-hint-tap-to-send')
						: t('voice-hint-tap-to-talk')}
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
						{t('voice-phase-listening')}
					{:else if voice.state.phase === 'working'}
						{t('voice-status-working')}
					{:else if voice.state.phase === 'speaking'}
						{t('voice-phase-speaking')}
					{:else}
						{t('voice-hint-tap-to-talk')}
					{/if}
				</p>
				{#if voice.state.captionUser}
					<p class="text-xs text-base-content/60 w-full text-left">
						<strong>{t('voice-caption-you')}:</strong>
						{voice.state.captionUser}
					</p>
				{/if}
				{#if voice.state.captionAi}
					<p class="text-xs text-base-content/60 w-full text-left">
						<strong>{t('voice-caption-ai')}:</strong>
						{voice.state.captionAi}
					</p>
				{/if}
				{#if voice.state.note}
					<div class="alert alert-warning py-2"><span>{voice.state.note}</span></div>
				{/if}
			</div>
			<div class="modal-action">
				<button class="btn btn-ghost btn-sm" onclick={closeVoice}>{t('chat-render-close')}</button>
			</div>
		</div>
		<form method="dialog" class="modal-backdrop">
			<button onclick={closeVoice}>{t('chat-render-close')}</button>
		</form>
	</dialog>
{/if}
