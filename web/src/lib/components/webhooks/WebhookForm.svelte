<script lang="ts">
	import { onDestroy, untrack } from 'svelte';
	import { t } from '#lib/i18n.svelte.js';
	import SearchableSelect from '#lib/components/SearchableSelect.svelte';
	import LinkedChatPicker from '#lib/components/LinkedChatPicker.svelte';
	import AgentRunPicker from '#lib/components/agents/AgentRunPicker.svelte';
	import type { OwnedAgent } from '#lib/agent-state.js';
	import { initialLinkedSession, linkedSessionField } from '#lib/linked-chat.js';
	import { modelSelectOptions, type ChatModelOption } from '#lib/model-option.js';
	import { webhooksApi, type Webhook, type WebhookBody } from '#lib/webhooks.js';
	import type { AgentVersion } from '#lib/agents.js';
	import { scheduledVersionModel } from '#lib/scheduled.js';

	let { webhook = null, models, agents, presetAgentId = null, principalId = null, versions = [], canWrite = false, onsaved, oncancel, onsecret }: {
		webhook?: Webhook | null;
		models: ChatModelOption[];
		agents: OwnedAgent[];
		presetAgentId?: string | null;
		principalId?: string | null;
		versions?: AgentVersion[];
		canWrite?: boolean;
		onsaved: () => void | Promise<void>;
		oncancel?: () => void;
		onsecret?: (secret: string) => void;
	} = $props();
	const initial = untrack(() => webhook);
	const initialAgentId = untrack(() => initial?.personal_agent_id ?? presetAgentId);
	let agentId = $state(initialAgentId ?? '');
	let name = $state(initial?.name ?? '');
	let model = $state(untrack(() => initial?.model ?? models[0]?.id ?? ''));
	let prompt = $state(initial?.prompt ?? '');
	let synchronous = $state(initial?.synchronous ?? false);
	let toolsEnabled = $state(initial?.tools_enabled ?? false);
	let reuseConversation = $state(initial?.reuse_conversation ?? false);
	let reuseRounds = $state(initial?.reuse_rounds ?? 5);
	const initialLinked = initialLinkedSession(initial);
	let linkedSession = $state(initialLinked);
	let error = $state<string | null>(null);
	let busy = $state(false);
	let disposed = false;
	onDestroy(() => { disposed = true; });
	let agentVersion = $state<number | null>(initial?.agent_version ?? null);
	const authoredModel = $derived(scheduledVersionModel(versions, agentVersion));
	const target = $derived(agents.find((candidate) => candidate.id === principalId));

	let modelOptions = $derived(modelSelectOptions(models));

	async function save() {
		if (disposed || !canWrite || busy) return;
		if (principalId && !versions.some((version) => version.version === agentVersion)) {
			error = t('scheduled-organization-version-required');
			return;
		}
		busy = true;
		error = null;
		const body: WebhookBody = {
			name, prompt, tools_enabled: toolsEnabled,
			synchronous: principalId ? false : synchronous,
			reuse_conversation: principalId ? false : reuseConversation, reuse_rounds: reuseRounds,
			...(principalId ? { principal_id: principalId, agent_version: agentVersion! } : {
				model, personal_agent_id: agentId || null, ...linkedSessionField(reuseConversation, initialLinked, linkedSession)
			})
		};
		try {
			if (webhook) await webhooksApi.update(webhook.id, { ...body, expected_configuration_generation: webhook.configuration_generation });
			else {
				// The trigger URL is shown exactly once. The editor route swaps
				// this form for the reveal, so there is no form left to clear —
				// it used to live above the list and had to reset itself.
				const created = await webhooksApi.create(body);
				if (disposed) return;
				onsecret?.(created.secret);
			}
			if (disposed) return;
			await onsaved();
		} catch (caught) {
			if (!disposed) error = String(caught);
		} finally {
			if (!disposed) busy = false;
		}
	}
</script>

<form class="card mb-6 min-w-0 border border-base-300" onsubmit={(event) => { event.preventDefault(); void save(); }}>
	<div class="card-body min-w-0 gap-4">
		{#if error}<div class="alert alert-error"><span>{error}</span></div>{/if}
		<label class="flex min-w-0 w-full flex-col gap-1"><div class="label"><span class="label-text">{t('webhooks-name-label')}</span></div><input class="input min-w-0 w-full" bind:value={name} required maxlength="128" aria-label={t('webhooks-name-label')} placeholder={t('webhooks-name-placeholder')} /></label>
		{#if principalId}
			{#if target}<p class="text-sm">{target.display || target.name}</p>{/if}
			<div class="alert alert-info text-sm"><span>{t('webhooks-organization-fresh')}</span></div>
			<label class="flex min-w-0 w-full flex-col gap-1">
				<div class="label"><span class="label-text">{t('scheduled-organization-version')}</span></div>
				<select class="select w-full" bind:value={agentVersion} required aria-label={t('scheduled-organization-version')}>
					<option value={null} disabled>{t('scheduled-organization-version-required')}</option>
					{#if agentVersion !== null && !versions.some((version) => version.version === agentVersion)}<option value={agentVersion} disabled>{agentVersion}</option>{/if}
					{#each versions as version (version.version)}<option value={version.version}>{t('agents-test-source-published', { version: version.version })}</option>{/each}
				</select>
			</label>
			<p class="inline-flex gap-1 text-sm"><span class="text-base-content/70">{t('webhooks-model-label')}:</span><span>{authoredModel ?? t('scheduled-organization-model-missing')}</span></p>
		{:else}
		<label class="flex min-w-0 w-full flex-col gap-1">
			<div class="label"><span class="label-text">{t('webhooks-model-label')}</span></div>
			{#if models.length}<SearchableSelect options={modelOptions} bind:value={model} ariaLabel={t('webhooks-model-label')} class="w-full" />
			{:else}<input class="input min-w-0 w-full" bind:value={model} required aria-label={t('webhooks-model-label')} placeholder={t('webhooks-model-placeholder')} />{/if}
		</label>
		<AgentRunPicker {agents} initialId={initial?.personal_agent_id ?? null} bind:value={agentId} onchange={() => (linkedSession = '')} />
		{/if}
		<label class="flex min-w-0 w-full flex-col gap-1"><div class="label"><span class="label-text">{t('webhooks-prompt-label')}</span></div><textarea class="textarea min-h-28 min-w-0 w-full" bind:value={prompt} required maxlength="8000" aria-label={t('webhooks-prompt-label')} placeholder={t('webhooks-prompt-placeholder')}></textarea></label>

		{#if !principalId}<label class="label cursor-pointer justify-start gap-3 whitespace-normal"><input class="checkbox checkbox-sm" type="checkbox" bind:checked={synchronous} /><span>{t('webhooks-sync-toggle-label')}</span></label>{/if}
		<div class="flex flex-col gap-1">
			<label class="label cursor-pointer justify-start gap-3 whitespace-normal"><input class="checkbox checkbox-sm" type="checkbox" bind:checked={toolsEnabled} /><span>{t(principalId ? 'webhooks-organization-tools-toggle-label' : 'webhooks-tools-toggle-label')}</span></label>
			{#if toolsEnabled}<div class="alert alert-warning"><span>{t(principalId ? 'webhooks-organization-tools-warning' : 'webhooks-tools-warning')}</span></div>{/if}
		</div>
		{#if !principalId}
		<div class="flex flex-wrap items-center gap-3">
			<label class="label cursor-pointer justify-start gap-3 whitespace-normal"><input class="checkbox checkbox-sm" type="checkbox" bind:checked={reuseConversation} /><span>{t('webhooks-reuse-toggle-label')}</span></label>
			{#if reuseConversation}<label class="flex items-center gap-2 text-sm"><span class="opacity-70">{t('webhooks-reuse-rounds-prefix')}</span><input class="input input-sm w-20" type="number" min="1" max="50" bind:value={reuseRounds} aria-label={t('webhooks-reuse-rounds-aria')} /><span class="opacity-70">{t('webhooks-reuse-rounds-suffix')}</span></label>{/if}
		</div>
		{#if reuseConversation}<LinkedChatPicker bind:value={linkedSession} agentId={agentId || null} />{/if}
		{/if}
		<div class="card-actions justify-end">{#if oncancel}<button class="btn" type="button" onclick={oncancel}>{t('admin-cancel')}</button>{/if}<button class="btn btn-primary" type="submit" disabled={!canWrite || busy || !name.trim() || !prompt.trim() || (principalId ? !versions.some((version) => version.version === agentVersion) : !model.trim())}>{webhook ? t('webhooks-save-submit') : t('webhooks-create-submit')}</button></div>
	</div>
</form>
