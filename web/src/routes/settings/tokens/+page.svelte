<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import { api } from '#lib/api.js';
	import { adminDelete, adminJson, adminPost, adminPut } from '#lib/admin-client.js';
	import EditModal from '#lib/components/EditModal.svelte';
	import ManagedTokenCard from '#lib/components/tokens/ManagedTokenCard.svelte';
	import PrioritySelect from '#lib/components/PrioritySelect.svelte';
	import { canChoosePriority, type Priority } from '#lib/priority.js';
	import TokenSetupGuides from '#lib/components/tokens/TokenSetupGuides.svelte';
	import { t } from '#lib/i18n.svelte.js';
	import { selectedTokenTab } from '#lib/tokens-tabs.js';
	import type { ManagedToken, NewQuota, TokenManagementDetails } from '#lib/tokens.js';

	let details = $state<TokenManagementDetails | null>(null);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let busy = $state(false);
	let minted = $state<{ name: string; plaintext: string } | null>(null);
	let name = $state('');
	let ttlDays = $state(90);
	let priority = $state<Priority | null>(null);
	let createError = $state<string | null>(null);
	let creating = $state(false);
	let selected = $derived(selectedTokenTab(page.url.search));

	function openCreate() {
		name = '';
		ttlDays = 90;
		priority = null;
		createError = null;
		creating = true;
	}

	async function refresh() {
		try { details = await adminJson<TokenManagementDetails>('/api/v0/tokens/details'); error = null; }
		catch (caught) { error = String(caught); }
	}

	async function create() {
		const trimmed = name.trim();
		if (!trimmed || busy || !details || !canChoosePriority(priority, details.max_priority)) return;
		busy = true;
		createError = null;
		try {
			const response = await api.createToken({ name: trimmed, ttl_days: ttlDays, priority, tools_enabled: false, tool_states: {} });
			minted = { name: response.token.name, plaintext: response.plaintext };
			creating = false;
			await refresh();
		} catch (caught) { createError = String(caught); } finally { busy = false; }
	}

	async function mutate(action: () => Promise<void>) {
		notice = null;
		try { await action(); }
		catch (caught) { notice = String(caught); }
	}

	/** A dialog's Save: one draft can take several requests, and any of them
	 *  may fail after the earlier ones landed. So the card is reloaded either
	 *  way, and a failure is rethrown so the dialog keeps the draft open. */
	async function commit(action: () => Promise<void>) {
		notice = null;
		try { await action(); }
		catch (caught) { notice = String(caught); throw caught; }
		finally { await refresh(); }
	}

	async function updateTools(token: ManagedToken, enabled: boolean, states: Record<string, 'on' | 'auto' | 'off'>, mcpAllow: boolean) {
		await commit(async () => {
			await api.updateTokenTools(token.id, { tools_enabled: enabled, tool_states: states });
			if (mcpAllow !== token.mcp_allow) await adminPut(`/api/v0/tokens/${token.id}/mcp-policy`, { allow: mcpAllow });
			notice = t('tokens-tools-saved-toast');
		});
	}
	async function updateModels(token: ManagedToken, restrict: boolean, models: string[]) {
		await commit(async () => {
			await adminPut(`/api/v0/tokens/${token.id}/models`, { restrict, models });
			notice = restrict ? t('tokens-models-saved-toast', { count: models.length }) : t('tokens-models-cleared-toast');
		});
	}
	async function updatePriority(token: ManagedToken, priority: Priority | null) {
		await commit(async () => {
			await adminPut(`/api/v0/tokens/${encodeURIComponent(token.id)}/priority`, { priority });
			notice = t('priority-saved');
		});
	}
	async function updateQuotas(token: ManagedToken, changes: { remove: string[]; add: NewQuota[] }) {
		await commit(async () => {
			for (const id of changes.remove) await adminDelete(`/api/v0/tokens/${token.id}/quota/${id}`);
			for (const quota of changes.add) await adminPost(`/api/v0/tokens/${token.id}/quota`, quota);
			notice = t('tokens-limits-saved-toast');
		});
	}
	async function rotate(token: ManagedToken) {
		if (!confirm(t('tokens-rotate-confirm'))) return;
		await mutate(async () => { const response = await api.rotateToken(token.id); minted = { name: response.token.name, plaintext: response.plaintext }; await refresh(); window.scrollTo({ top: 0, behavior: 'smooth' }); });
	}
	async function revoke(token: ManagedToken) { if (confirm(t('tokens-revoke-confirm'))) await mutate(async () => { await api.revokeToken(token.id); await refresh(); }); }
	async function remove(token: ManagedToken) { if (confirm(t('tokens-remove-confirm'))) await mutate(async () => { await api.deleteToken(token.id); await refresh(); }); }

	onMount(refresh);
</script>

<div class="flex w-full flex-col gap-4">
	<p class="text-sm text-base-content/60">{t('tokens-intro')}</p>
	<nav class="tabs tabs-border w-full overflow-x-auto" aria-label={t('tokens-page-heading')}>
		<a class:tab-active={selected === 'tokens'} class="tab whitespace-nowrap" href="/settings/tokens?tab=tokens" aria-current={selected === 'tokens' ? 'page' : undefined}>{t('tokens-tab-tokens')}</a>
		<a class:tab-active={selected === 'guides'} class="tab whitespace-nowrap" href="/settings/tokens?tab=guides" aria-current={selected === 'guides' ? 'page' : undefined}>{t('tokens-tab-guides')}</a>
	</nav>
	{#if error}<div class="alert alert-error mb-4"><span>{error}</span></div>{/if}
	{#if notice}<div class="alert alert-info mb-4"><span>{notice}</span></div>{/if}

	{#if minted}
		<section class="card mb-6"><div class="card-body">
			<div class="flex items-center justify-between gap-3">
				<h2 class="card-title text-base"><span class="text-success">✓</span>{t('tokens-minted-heading')}</h2>
				<button type="button" class="btn btn-ghost btn-sm btn-square" onclick={() => (minted = null)} aria-label={t('tokens-panel-close')} title={t('tokens-panel-close')}>×</button>
			</div>
			<p class="text-sm text-base-content/70">{t('tokens-minted-copy-warning')}</p>
			<div class="flex flex-col items-start gap-2 sm:flex-row sm:items-start">
				<pre class="m-0 w-full min-w-0 flex-1 select-all whitespace-pre-wrap break-all rounded-box border border-base-300 bg-base-100 p-3 font-mono text-xs">{minted.plaintext}</pre>
				<button type="button" class="btn btn-sm shrink-0" onclick={() => navigator.clipboard?.writeText(minted?.plaintext ?? '')} aria-label={t('tokens-copy-aria')}>{t('webhooks-copy')}</button>
			</div>
			<p class="mb-0 mt-3 text-xs text-base-content/60">{t('tokens-minted-name', { name: minted.name })}</p>
		</div></section>
	{/if}

	{#if selected === 'tokens'}
	<section class="flex flex-col gap-4">
		<div class="flex flex-wrap items-center justify-between gap-3">
			<h2 class="m-0 text-lg font-semibold">{t('tokens-list-heading')}</h2>
			<button class="btn btn-primary btn-sm" type="button" onclick={openCreate} disabled={!details}>{t('tokens-create-heading')}</button>
		</div>
		{#if !details}<div class="skeleton h-24 w-full"></div>{:else if details.tokens.length === 0}<p class="text-sm text-base-content/60">{t('tokens-list-empty')}</p>{:else}
			<ul class="flex flex-col gap-4">{#each details.tokens as token (token.id)}
				<ManagedTokenCard {token} capabilities={details.capabilities} models={details.models} ownerLimits={details.owner_limits} currency={details.currency} timezone={details.timezone} usageEnabled={details.usage_enabled}
					ownerPriority={details.owner_priority} maxPriority={details.max_priority} priorityEnabled={details.priority_enabled} onpriority={(priority) => updatePriority(token, priority)}
					ontools={(enabled, states, mcpAllow) => updateTools(token, enabled, states, mcpAllow)} onmodels={(restrict, models) => updateModels(token, restrict, models)}
					onquotas={(changes) => updateQuotas(token, changes)} onrotate={() => rotate(token)} onrevoke={() => revoke(token)} onremove={() => remove(token)} />
			{/each}</ul>
		{/if}
	</section>
	{:else if selected === 'guides'}
		<TokenSetupGuides />
		{/if}
</div>

<EditModal
	bind:open={creating}
	title={t('tokens-create-heading')}
	description={t('tokens-create-description')}
	cancellabel={t('admin-cancel')}
	savelabel={t('tokens-create-submit')}
	saving={busy || !details || !canChoosePriority(priority, details.max_priority)}
	onsave={create}
>
	<div class="flex flex-col gap-3">
		{#if createError}<div class="alert alert-error" role="alert"><span>{createError}</span></div>{/if}
		<fieldset class="fieldset">
			<legend class="fieldset-legend">{t('tokens-name-label')}</legend>
			<!-- svelte-ignore a11y_autofocus -->
			<input class="input w-full" required autofocus placeholder={t('tokens-name-placeholder')} bind:value={name} aria-label={t('tokens-name-label')} />
		</fieldset>
		<fieldset class="fieldset">
			<legend class="fieldset-legend">{t('tokens-ttl-label')}</legend>
			<input class="input w-32" type="number" min="1" max="1825" bind:value={ttlDays} aria-label={t('tokens-ttl-label')} />
		</fieldset>
		{#if details}
			<PrioritySelect bind:value={priority} maximum={details.max_priority} inheritLabel={t('priority-inherit-value', { priority: t(`priority-${details.owner_priority}`) })} />
			<p class="text-sm text-base-content/60">{t('priority-owner-help')}</p>
			{#if !details.priority_enabled}<div class="alert alert-info"><span>{t('priority-owner-disabled')}</span></div>{/if}
		{/if}
	</div>
</EditModal>
