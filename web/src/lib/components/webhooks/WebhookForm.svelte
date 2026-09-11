<script lang="ts">
	import { untrack } from 'svelte';
	import { adminPost, adminPut } from '$lib/admin-client';
	import { t } from '$lib/i18n.svelte';
	import type { ChatModelOption } from '$lib/model-option';
	import type { Webhook } from '$lib/webhooks';

	let { webhook = null, models, onsaved, oncancel, onsecret } = $props<{
		webhook?: Webhook | null;
		models: ChatModelOption[];
		onsaved: () => void | Promise<void>;
		oncancel?: () => void;
		onsecret?: (secret: string) => void;
	}>();
	const initial = untrack(() => webhook);
	let name = $state(initial?.name ?? '');
	let model = $state(untrack(() => initial?.model ?? models[0]?.id ?? ''));
	let prompt = $state(initial?.prompt ?? '');
	let synchronous = $state(initial?.synchronous ?? false);
	let toolsEnabled = $state(initial?.tools_enabled ?? false);
	let reuseConversation = $state(initial?.reuse_conversation ?? false);
	let reuseRounds = $state(initial?.reuse_rounds ?? 5);
	let error = $state<string | null>(null);
	let busy = $state(false);
	let selectedModel = $derived(models.find((candidate: ChatModelOption) => candidate.id === model));

	function modelLabel(candidate: ChatModelOption): string {
		if (!candidate.gdpr && !candidate.nda) return t('webhooks-model-non-gdpr-nda-restricted', { model: candidate.id });
		if (!candidate.gdpr) return t('webhooks-model-non-gdpr', { model: candidate.id });
		if (!candidate.nda) return t('webhooks-model-nda-restricted', { model: candidate.id });
		return candidate.id;
	}

	async function save() {
		busy = true;
		error = null;
		const body = {
			name, prompt, model, tools_enabled: toolsEnabled, synchronous,
			reuse_conversation: reuseConversation, reuse_rounds: reuseRounds
		};
		try {
			if (webhook) await adminPut(`/api/v0/webhooks/${webhook.id}`, body);
			else {
				const created = await adminPost<{ secret: string }>('/api/v0/webhooks', body);
				onsecret?.(created.secret);
				name = '';
				prompt = '';
				synchronous = false;
				toolsEnabled = false;
				reuseConversation = false;
				reuseRounds = 5;
			}
			await onsaved();
		} catch (caught) {
			error = String(caught);
		} finally {
			busy = false;
		}
	}
</script>

<form class="card mb-6 min-w-0 border border-base-300" onsubmit={(event) => { event.preventDefault(); void save(); }}>
	<div class="card-body min-w-0 gap-4">
		{#if error}<div class="alert alert-error"><span>{error}</span></div>{/if}
		<label class="flex min-w-0 w-full flex-col gap-1"><div class="label"><span class="label-text">{t('webhooks-name-label')}</span></div><input class="input min-w-0 w-full" bind:value={name} required maxlength="128" aria-label={t('webhooks-name-label')} placeholder={t('webhooks-name-placeholder')} /></label>
		<label class="flex min-w-0 w-full flex-col gap-1">
			<div class="label"><span class="label-text">{t('webhooks-model-label')}</span></div>
			{#if models.length}<select class="select min-w-0 w-full" bind:value={model} aria-label={t('webhooks-model-label')}>{#each models as candidate (candidate.id)}<option value={candidate.id}>{modelLabel(candidate)}</option>{/each}</select>
			{:else}<input class="input min-w-0 w-full" bind:value={model} required aria-label={t('webhooks-model-label')} placeholder={t('webhooks-model-placeholder')} />{/if}
		</label>
		{#if selectedModel && !selectedModel.gdpr}<div class="alert alert-error"><span>{t('webhooks-gdpr-warning')}</span></div>{/if}
		{#if selectedModel && !selectedModel.nda}<div class="alert alert-error"><span>{t('webhooks-nda-warning')}</span></div>{/if}
		<label class="flex min-w-0 w-full flex-col gap-1"><div class="label"><span class="label-text">{t('webhooks-prompt-label')}</span></div><textarea class="textarea min-h-28 min-w-0 w-full" bind:value={prompt} required maxlength="8000" aria-label={t('webhooks-prompt-label')} placeholder={t('webhooks-prompt-placeholder')}></textarea></label>

		<label class="label cursor-pointer justify-start gap-3 whitespace-normal"><input class="checkbox checkbox-sm" type="checkbox" bind:checked={synchronous} /><span>{t('webhooks-sync-toggle-label')}</span></label>
		<div class="flex flex-col gap-1">
			<label class="label cursor-pointer justify-start gap-3 whitespace-normal"><input class="checkbox checkbox-sm" type="checkbox" bind:checked={toolsEnabled} /><span>{t('webhooks-tools-toggle-label')}</span></label>
			{#if toolsEnabled}<div class="alert alert-warning"><span>{t('webhooks-tools-warning')}</span></div>{/if}
		</div>
		<div class="flex flex-wrap items-center gap-3">
			<label class="label cursor-pointer justify-start gap-3 whitespace-normal"><input class="checkbox checkbox-sm" type="checkbox" bind:checked={reuseConversation} /><span>{t('webhooks-reuse-toggle-label')}</span></label>
			{#if reuseConversation}<label class="flex items-center gap-2 text-sm"><span class="opacity-70">{t('webhooks-reuse-rounds-prefix')}</span><input class="input input-sm w-20" type="number" min="1" max="50" bind:value={reuseRounds} aria-label={t('webhooks-reuse-rounds-aria')} /><span class="opacity-70">{t('webhooks-reuse-rounds-suffix')}</span></label>{/if}
		</div>
		<div class="card-actions justify-end">{#if oncancel}<button class="btn" type="button" onclick={oncancel}>{t('admin-cancel')}</button>{/if}<button class="btn btn-primary" type="submit" disabled={busy || !name.trim() || !prompt.trim() || !model.trim()}>{webhook ? t('webhooks-save-submit') : t('webhooks-create-submit')}</button></div>
	</div>
</form>
