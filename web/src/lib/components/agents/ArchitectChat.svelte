<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import { api } from '#lib/api.js';
	import { agentsApi, type AgentError } from '#lib/agents.js';
	import { conversationTitle, dotPlanCanApply, groupedDotTools, setupPath, toolLabel, undoTarget, type DotSetupPlan, type Phase } from '#lib/architect.js';
	import { createConversationController, type ConversationController } from '#lib/chat.svelte.js';
	import type { ToolCall } from '#lib/chat-protocol.js';
	import { t } from '#lib/i18n.svelte.js';
	import DictationButton from '#lib/components/chat/DictationButton.svelte';
	import StreamedChat from '#lib/components/chat/StreamedChat.svelte';

	/**
	 * The architect's conversation, mounted while its window is open: it opens
	 * (or reopens) the person's architect chat about `agentId`, streams it
	 * like any chat, and sends what is typed or dictated. Every tool call shows
	 * with its input and output; a draft change offers Undo.
	 */
	let {
		agentId = null,
		agentName = null,
		onchanged = null,
		dotOwner = null,
		oncreated = null
	}: {
		agentId?: string | null;
		agentName?: string | null;
		onchanged?: (() => void) | null;
		dotOwner?: 'personal' | null;
		oncreated?: ((id: string) => void) | null;
	} = $props();

	let phase = $state<Phase>('starting');
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let model = '';
	let controller = $state<ConversationController | null>(null);
	let transcriptionModel = $state('');
	let undone = $state<Record<string, boolean>>({});
	let proposal = $state<DotSetupPlan | null>(null);
	let dotId = $state<string | null>(null);
	let applying = $state(false);
	let planLoading = $state(false);
	let planRequest = 0;

	const turns = $derived(controller?.state.turns ?? []);
	const working = $derived(controller?.state.liveTurnId != null);
	const canApply = $derived(dotPlanCanApply(proposal, working || planLoading, applying) && dotId === null);

	async function refreshPlan() {
		if (!dotOwner || !controller) return;
		const sessionId = controller.id;
		const requestId = ++planRequest;
		planLoading = true;
		try {
			const result = await agentsApi.dotPlan(sessionId);
			if (controller?.id !== sessionId || requestId !== planRequest) return;
			proposal = result.proposal;
			dotId = result.dot_id;
		} catch (err) {
			if (controller?.id === sessionId && requestId === planRequest) {
				proposal = null;
				error = (err as Error).message;
			}
		} finally {
			if (controller?.id === sessionId && requestId === planRequest) planLoading = false;
		}
	}

	async function applyPlan() {
		if (!canApply || !controller || !proposal) return;
		applying = true;
		error = null;
		try {
			const result = await agentsApi.applyDotPlan(controller.id, proposal.revision);
			dotId = result.dot.id;
			oncreated?.(result.dot.id);
		} catch (err) {
			error = (err as Error).message;
			await refreshPlan();
		} finally {
			applying = false;
		}
	}

	async function start(fresh: boolean) {
		phase = 'starting';
		error = null;
		controller?.destroy();
		controller = null;
		proposal = null;
		dotId = null;
		planLoading = false;
		planRequest++;
		try {
			const started = await agentsApi.startArchitect({
				agent_id: agentId ?? undefined,
				title: dotOwner ? t('dot-setup-title') : conversationTitle(t, agentName),
				dot_owner: dotOwner ?? undefined,
				fresh
			});
			model = started.model;
			const c = createConversationController(started.session_id);
			c.onTurnFinalized = () => { onchanged?.(); void refreshPlan(); };
			c.attach();
			controller = c;
			phase = 'ready';
			await refreshPlan();
		} catch (err) {
			error = (err as AgentError).message;
			phase = 'failed';
		}
	}

	async function send(message: string): Promise<boolean> {
		if (!controller) return false;
		error = null;
		try {
			await api.sendChatMessage(controller.id, { model, message });
			if (dotOwner) { proposal = null; planLoading = false; planRequest++; }
			controller.attach();
			return true;
		} catch (err) {
			error = (err as Error).message;
			return false;
		}
	}

	async function undo(call: ToolCall) {
		const target = undoTarget(call);
		if (!target) return;
		try {
			await agentsApi.restoreDraft(target.agentId, target.revision);
			undone = { ...undone, [call.id]: true };
			onchanged?.();
		} catch (err) {
			error = (err as AgentError).message;
		}
	}

	onMount(async () => {
		void start(false);
		try {
			transcriptionModel = (await api.chatVoiceConfig()).data[0] ?? '';
		} catch {
			transcriptionModel = '';
		}
	});

	onDestroy(() => controller?.destroy());
</script>

<div class={`flex flex-col gap-3 ${dotOwner && proposal ? 'h-[min(35dvh,24rem)] min-h-64' : 'h-[min(70dvh,44rem)] min-h-80'}`}>
	<div class="flex items-center gap-2">
		<p class="m-0 flex-1 text-sm text-base-content/60">{t(dotOwner ? 'dot-setup-intro' : 'architect-intro')}</p>
		<button class="btn btn-ghost btn-sm" type="button" disabled={phase === 'starting' || working || applying} onclick={() => void start(true)}>{t('architect-new-conversation')}</button>
	</div>

	{#if error}<div class="alert alert-error text-sm" role="alert"><span>{error}</span></div>{/if}
	{#if notice}<div class="alert alert-warning text-sm" role="status"><span>{notice}</span></div>{/if}

	<StreamedChat
		{turns}
		{working}
		disabled={phase !== 'ready' || applying || dotId !== null}
		empty={phase === 'starting' ? '' : t(dotOwner ? 'dot-setup-empty' : 'architect-empty')}
		placeholder={t(dotOwner ? 'dot-setup-placeholder' : 'architect-placeholder')}
		sendLabel={t('architect-send')}
		toolLabel={toolLabel(t)}
		onsend={send}
	>
		{#snippet before()}
			{#if phase === 'starting'}
				<p class="m-0 flex items-center gap-2 text-sm text-base-content/60"><span class="loading loading-spinner loading-sm"></span>{t('architect-starting')}</p>
			{/if}
		{/snippet}
		{#snippet inside(live)}
			{@const path = agentId ? null : live.tool_calls.map(setupPath).findLast((p) => p !== null)}
			{#if live.tool_calls.some((call) => undoTarget(call)) || path}
				<div class="flex flex-wrap gap-2">
					{#each live.tool_calls as call (call.id)}
						{#if undoTarget(call)}
							{#if undone[call.id]}
								<span class="badge badge-outline">{t('architect-undone')}</span>
							{:else}
								<button class="btn btn-xs" type="button" onclick={() => void undo(call)}>↶ {t('architect-undo')}</button>
							{/if}
						{/if}
					{/each}
					{#if path}<a class="btn btn-xs btn-ghost" href={path}>{t('architect-open-setup')}</a>{/if}
				</div>
			{/if}
		{/snippet}
		{#snippet composer(append)}
			{#if transcriptionModel}
				<DictationButton model={transcriptionModel} ontranscript={append} onerror={(message) => (notice = message)} />
			{/if}
		{/snippet}
	</StreamedChat>
</div>

{#if dotOwner && proposal}
	{@const unresolved = proposal.unresolved}
	<section class="card card-border mt-4" aria-label={t('dot-setup-preview')}>
		<div class="card-body gap-3 p-4">
			<h4 class="card-title text-base">{t('dot-setup-preview')}</h4>
			<dl class="grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-2 text-sm">
				<dt class="text-base-content/60">{t('dots-create-name')}</dt><dd class="break-words font-medium">{proposal.display}</dd>
				<dt class="text-base-content/60">{t('dots-purpose-label')}</dt><dd class="whitespace-pre-wrap break-words">{proposal.description}</dd>
				<dt class="text-base-content/60">{t('dot-setup-identity')}</dt><dd>{t('dot-setup-personal-identity')}</dd>
				{#if proposal.knowledge}<dt class="text-base-content/60">{t('agents-setup-step-abilities-short')}</dt><dd class="break-words">{proposal.knowledge.name}</dd>{/if}
			</dl>
			{#if proposal.tools.length}
				<ul class="list text-sm">
					{#each groupedDotTools(proposal.tools) as group (group.key)}
						<li class="list-row gap-2 px-0"><span class="break-words">{group.label}{#if group.tools.length > 1} <span class="badge badge-ghost badge-sm">{t('dot-setup-tool-count', { count: group.tools.length })}</span>{/if}</span><span class="text-base-content/60">{t(group.permission === 'always_allow' ? 'dot-access-allow' : 'dot-access-ask')}</span></li>
					{/each}
				</ul>
			{/if}
			<details class="collapse collapse-arrow border border-base-300">
				<summary class="collapse-title min-h-0 py-3 text-sm font-medium">{t('dot-setup-details')}</summary>
				<div class="collapse-content space-y-3 whitespace-pre-wrap break-words text-sm">
					<p>{proposal.draft_spec.main?.instructions?.orchestration || ''}</p>
					{#if proposal.draft_spec.main?.instructions?.response}<div><h5 class="font-medium">{t('agents-setup-tone')}</h5><p>{proposal.draft_spec.main.instructions.response}</p></div>{/if}
					{#if proposal.draft_spec.scope?.topics?.length}<div><h5 class="font-medium">{t('agents-setup-step-scope')}</h5><p>{proposal.draft_spec.scope.topics.join(', ')}</p></div>{/if}
					{#if proposal.tools.length}<ul class="list text-xs">{#each proposal.tools as tool (tool.id)}<li class="list-row gap-2 px-0"><code class="break-all">{tool.id}</code><span>{t(tool.permission === 'always_allow' ? 'dot-access-allow' : 'dot-access-ask')}</span></li>{/each}</ul>{/if}
				</div>
			</details>
			{#if proposal.unresolved.length || proposal.dropped.length}
				<div class="alert alert-warning text-sm" role="status"><div><p class="font-medium">{t('dot-setup-unresolved')}</p><ul class="mt-2 list-disc pl-5">{#each [...new Set(unresolved)] as item}<li>{item}</li>{/each}{#each proposal.dropped.filter((item) => !unresolved.includes(item.reason)) as item}<li>{item.item ? `${item.item}: ` : ''}{item.reason}</li>{/each}</ul></div></div>
			{/if}
			<div class="card-actions justify-end">
				{#if dotId}<a class="btn btn-primary" href={`/dots?dot_id=${encodeURIComponent(dotId)}`}>{t('dots-details-open')}</a>{:else}<button class="btn btn-primary" type="button" disabled={!canApply} onclick={() => void applyPlan()}>{#if applying}<span class="loading loading-spinner loading-sm"></span>{/if}{t('dot-setup-apply')}</button>{/if}
			</div>
		</div>
	</section>
{/if}
