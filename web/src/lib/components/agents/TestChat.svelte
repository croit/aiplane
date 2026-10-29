<script lang="ts">
	import { onDestroy, untrack } from 'svelte';
	import { agentsApi, readFailure, settleAnswered, testTurnLabel, turnsToRead, type AgentError, type PrivateChatSource, type Spec, type TestDebug, type TestTurnView } from '#lib/agents.js';
	import { createConversationController, type ConversationController } from '#lib/chat.svelte.js';
	import { waitingFrom, waitingLead, type Answer, type SuspensionView } from '#lib/suspension.js';
	import { t } from '#lib/i18n.svelte.js';
	import StreamedChat from '#lib/components/chat/StreamedChat.svelte';
	import SuspensionCard from '#lib/components/SuspensionCard.svelte';
	import DebugPanel from './DebugPanel.svelte';

	/**
	 * Private agent conversations use the saved draft or the organization's
	 * published version. The server pins the source for each conversation.
	 */
	let { agentId, spec = {}, dirty, onsave, onturn, organization = false, sources = ['draft'], liveVersion = null, initialSessionId = null, onsession }: {
		agentId: string;
		spec?: Spec;
		dirty: boolean;
		onsave: () => Promise<void>;
		onturn?: (debug: TestDebug | null) => void;
		organization?: boolean;
		sources?: PrivateChatSource[];
		liveVersion?: number | null;
		initialSessionId?: string | null;
		onsession?: (sessionId: string | null) => void;
	} = $props();

	let controller = $state<ConversationController | null>(null);
	let source = $state<PrivateChatSource>('draft');
	let conversation = $state<{ source: PrivateChatSource; agent_version: number } | null>(null);
	let views = $state<Record<string, TestTurnView>>({});
	let selected = $state<string | null>(null);
	let error = $state<string | null>(null);
	let sending = $state(false);
	let stopping = $state(false);
	let historyLoading = $state(false);
	let lastExternalSession: string | null | undefined;
	let conversationEpoch = 0;
	let answering = $state<string | null>(null);
	// Pauses just answered, by the request answered: their old
	// debug view is gone, and a new one is read once the stream shows the
	// turn moved on and stopped again.
	let answered: Record<string, string> = {};
	let reading = new Set<string>();

	const turns = $derived(controller?.state.turns ?? []);
	const stoppable = $derived(turns.findLast((live) => live.turn.role === 'assistant' && (live.turn.status === 'in_progress' || live.turn.status === 'suspended'))?.turn.id ?? null);
	const working = $derived(controller?.state.liveTurnId != null);
	const busy = $derived(working || sending || stopping || historyLoading || answering !== null);
	const offeredSources = $derived(conversation?.source === 'published' && !sources.includes('published') ? [...sources, 'published' as const] : sources);
	const publishedVersion = $derived(conversation?.source === 'published' ? conversation.agent_version : liveVersion);
	const latest = $derived(turns.findLast((live) => views[live.turn.id])?.turn.id ?? null);
	const shown = $derived(views[selected ?? latest ?? '']?.debug ?? null);

	$effect(() => {
		const sessionId = initialSessionId;
		if (lastExternalSession === sessionId) return;
		lastExternalSession = sessionId;
		untrack(() => {
			if (controller?.id === sessionId) return;
			clearConversation();
			if (sessionId) {
				historyLoading = true;
				open(sessionId).attach();
			}
		});
	});

	$effect(() => {
		const debug = latest ? views[latest]?.debug : null;
		if (debug) onturn?.(debug);
	});

	// Whenever nothing runs, every stopped turn gets its view: after a stream
	// ended, and after an attach that found the turn already over.
	$effect(() => {
		if (!controller || working) return;
		void turns.map((live) => live.turn.status).join();
		void readStopped();
	});

	/** Read the debug view of every turn that stopped and has none yet, then catch the transcript up. */
	async function readStopped() {
		const c = controller;
		if (!c) return;
		const currentReading = reading;
		answered = settleAnswered(c.state.turns, answered);
		const ids = turnsToRead(c.state.turns, views, answered).filter((id) => !currentReading.has(id));
		if (!ids.length) return;
		for (const id of ids) currentReading.add(id);
		try {
			for (const id of ids) {
				const view = await agentsApi.testTurnView(agentId, c.id, id);
				if (controller !== c) return;
				views[id] = view;
				conversation = { source: view.source, agent_version: view.agent_version };
				source = view.source;
				historyLoading = false;
			}
			// The output filter may have rewritten the answer after the turn's
			// row was final; a fresh snapshot shows what a visitor would get.
			if (controller === c) c.attach();
		} catch (err) {
			const failure = readFailure(err as AgentError);
			// Not settled yet: the next attach ends when it is, and the stream's
			// end reads it again.
			if (failure === 'retry') {
				if (controller === c) c.attach();
			} else if (controller === c) error = failure;
		} finally {
			for (const id of ids) currentReading.delete(id);
		}
	}

	function open(sessionId: string) {
		const c = createConversationController(sessionId, agentsApi.testEventsUrl(agentId, sessionId));
		c.onTurnFinalized = () => void readStopped();
		controller = c;
		return c;
	}

	async function send(message: string): Promise<boolean> {
		if (busy) return false;
		const epoch = conversationEpoch;
		sending = true;
		error = null;
		selected = null;
		try {
			const sent = await agentsApi.sendTestMessage(agentId, message, controller?.id ?? null, conversation?.source ?? source);
			if (conversationEpoch !== epoch) return false;
			conversation = { source: sent.source, agent_version: sent.agent_version };
			source = sent.source;
			(controller?.id === sent.session_id ? controller : open(sent.session_id)).attach();
			onsession?.(sent.session_id);
			return true;
		} catch (err) {
			if (conversationEpoch === epoch) error = (err as AgentError).message;
			return false;
		} finally {
			if (conversationEpoch === epoch) sending = false;
		}
	}

	async function answer(turnId: string, waiting: SuspensionView, decision: Answer): Promise<boolean> {
		const c = controller;
		if (!c) return false;
		const epoch = conversationEpoch;
		answering = turnId;
		error = null;
		try {
			await agentsApi.resumeTurn(agentId, c.id, turnId, waiting.request_id, decision);
			if (conversationEpoch !== epoch) return false;
			answered[turnId] = waiting.request_id;
			delete views[turnId];
			c.attach();
			return true;
		} catch (err) {
			if (conversationEpoch === epoch) error = (err as AgentError).message;
			return false;
		} finally {
			if (conversationEpoch === epoch) answering = null;
		}
	}

	function clearConversation() {
		conversationEpoch += 1;
		sending = false;
		answering = null;
		stopping = false;
		onturn?.(null);
		controller?.destroy();
		controller = null;
		conversation = null;
		source = 'draft';
		views = {};
		answered = {};
		selected = null;
		error = null;
		historyLoading = false;
		reading = new Set();
	}

	function reset() {
		if (busy) return;
		clearConversation();
		lastExternalSession = null;
		onsession?.(null);
	}

	async function stop() {
		const c = controller;
		const turnId = stoppable;
		if (!c || !turnId || stopping) return;
		const epoch = conversationEpoch;
		stopping = true;
		try {
			await agentsApi.cancelTestConversation(agentId, c.id, turnId);
			if (conversationEpoch === epoch) c.attach();
		} catch (err) {
			if (conversationEpoch === epoch) error = (err as AgentError).message;
		} finally { if (conversationEpoch === epoch) stopping = false; }
	}

	onDestroy(() => { conversationEpoch += 1; controller?.destroy(); });
</script>

<div data-test-chat class="flex min-h-0 flex-1 flex-col gap-3">
	<div class="flex flex-wrap items-center gap-2">
		<p class="text-sm text-base-content/70">{t(organization ? 'agents-test-organization-intro' : 'agents-test-intro')}</p>
		<button class="btn btn-ghost btn-sm ml-auto" type="button" onclick={reset} disabled={!turns.length || busy}>{t('agents-test-new')}</button>
	</div>
	{#if organization && !historyLoading}
		<label class="fieldset w-fit max-w-full">
			<span class="fieldset-legend">{t('agents-test-source')}</span>
			<select class="select select-sm max-w-full" value={source} disabled={busy} onchange={(event) => { if (busy) return; const next = event.currentTarget.value as PrivateChatSource; reset(); source = next; }}>
				{#each offeredSources as choice}
					<option value={choice}>{choice === 'draft' ? t('agents-test-source-draft') : t('agents-test-source-published', { version: publishedVersion! })}</option>
				{/each}
			</select>
		</label>
		{#if conversation}<p class="text-sm text-base-content/70">{conversation.source === 'draft' ? t('agents-test-conversation-draft') : t('agents-test-conversation-published', { version: conversation.agent_version })}</p>{/if}
	{/if}
	{#if historyLoading}<p class="flex items-center gap-2 text-sm text-base-content/70"><span class="loading loading-spinner loading-sm"></span>{t('agent-activity-loading-more')}</p>{/if}
	{#if dirty && !historyLoading && source === 'draft'}
		<div class="alert alert-warning alert-soft text-sm">
			<span>{t('agents-test-unsaved')}</span>
			<button class="btn btn-sm" type="button" onclick={() => void onsave()}>{t('agents-save')}</button>
		</div>
	{/if}
	{#if error}<div class="alert alert-error text-sm" role="alert"><span>{error}</span></div>{/if}

	<div class="grid min-h-0 flex-1 gap-4 grid-rows-[minmax(0,3fr)_minmax(0,2fr)] lg:grid-cols-[3fr_2fr] lg:grid-rows-1">
		<div class="flex min-h-0 flex-col">
			<StreamedChat
				{turns}
				{working}
				disabled={sending || stopping || historyLoading || answering !== null}
				empty={t(organization ? 'agents-test-organization-empty' : 'agents-test-empty')}
				placeholder={t(organization ? 'embed-input-label' : 'agents-test-placeholder')}
				sendLabel={t('agents-test-send')}
				userLabel={t(organization ? 'embed-speaker-you' : 'agents-test-visitor')}
				assistantLabel={t('agents-test-agent')}
				highlighted={selected}
				onsend={send}
			>
				{#snippet composer()}
					{#if stoppable}<button class="btn btn-error" type="button" onclick={stop} disabled={stopping || answering !== null}>{t('render-composer-stop')}</button>{/if}
				{/snippet}
				{#snippet below(live)}
					{#if live.turn.status !== 'in_progress'}
						<div class="chat-footer flex items-center gap-2 text-xs opacity-70">
							{#if live.turn.status !== 'completed'}<span>{t(testTurnLabel(live.turn.status))}</span>{/if}
							{#if views[live.turn.id]}
								<button class="btn btn-ghost btn-xs" type="button" aria-pressed={selected === live.turn.id} onclick={() => (selected = live.turn.id)}>
									{t('agents-test-show-debug')}
								</button>
							{/if}
						</div>
					{/if}
					{#if live.turn.status === 'suspended' && live.suspension}
						{@const waiting = live.suspension}
						<SuspensionCard
							class="col-start-2 mt-1 w-full max-w-md"
							waiting={waitingFrom(waiting, live.tool_calls, views[live.turn.id]?.handoff ?? null)}
							lead={t(waitingLead(waiting.kind, 'test'), { tool: waiting.tool ?? '' })}
								note={!historyLoading && waiting.kind === 'human_answer' ? t(organization ? (source === 'published' ? 'agents-test-organization-published-handoff' : 'agents-test-organization-draft-handoff') : 'agents-test-handoff-inbox-hint') : null}
							busy={answering === live.turn.id}
							onanswer={(decision) => answer(live.turn.id, waiting, decision)}
						/>
					{/if}
				{/snippet}
			</StreamedChat>
		</div>

		<div class="min-h-0 overflow-y-auto rounded-box border border-base-300 p-3">
			<h3 class="mb-3 font-semibold">{t('agents-debug-heading')}</h3>
			{#if shown}
				<DebugPanel debug={shown} {spec} />
			{:else}
				<p class="text-sm text-base-content/60">{t('agents-debug-empty')}</p>
			{/if}
		</div>
	</div>
</div>
