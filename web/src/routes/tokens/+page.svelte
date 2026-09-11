<script lang="ts">
	import { onMount } from 'svelte';
	import { api } from '$lib/api';
	import { adminDelete, adminJson, adminPost, adminPut } from '$lib/admin-client';
	import ManagedTokenRow from '$lib/components/tokens/ManagedTokenRow.svelte';
	import PushNotificationsCard from '$lib/components/tokens/PushNotificationsCard.svelte';
	import TokenAccountCard from '$lib/components/tokens/TokenAccountCard.svelte';
	import { t } from '$lib/i18n.svelte';
	import type { ManagedToken, TokenManagementDetails } from '$lib/tokens';

	let details = $state<TokenManagementDetails | null>(null);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let busy = $state(false);
	let minted = $state<{ name: string; plaintext: string } | null>(null);
	let name = $state('');
	let ttlDays = $state(90);

	async function refresh() {
		try { details = await adminJson<TokenManagementDetails>('/api/v0/tokens/details'); error = null; }
		catch (caught) { error = String(caught); }
	}

	async function create() {
		const trimmed = name.trim();
		if (!trimmed || busy) return;
		busy = true;
		try {
			const response = await api.createToken({ name: trimmed, ttl_days: ttlDays, tools_enabled: false, disabled_tools: [] });
			minted = { name: response.token.name, plaintext: response.plaintext };
			name = '';
			await refresh();
		} catch (caught) { notice = String(caught); } finally { busy = false; }
	}

	async function mutate(action: () => Promise<void>) {
		notice = null;
		try { await action(); }
		catch (caught) { notice = String(caught); }
	}

	async function updateTools(token: ManagedToken, enabled: boolean, disabled: string[]) {
		await mutate(async () => { await api.updateTokenTools(token.id, { tools_enabled: enabled, disabled_tools: disabled }); await refresh(); });
	}
	async function updateModels(token: ManagedToken, restrict: boolean, models: string[]) {
		await mutate(async () => { await adminPut(`/api/v0/tokens/${token.id}/models`, { restrict, models });
			notice = restrict ? t('tokens-models-saved-toast', { count: models.length }) : t('tokens-models-cleared-toast'); await refresh(); });
	}
	async function addQuota(token: ManagedToken, dimension: string, window: string, value: number) {
		await mutate(async () => { await adminPost(`/api/v0/tokens/${token.id}/quota`, { dimension, window, value }); notice = t('tokens-limits-saved-toast'); await refresh(); });
	}
	async function removeQuota(token: ManagedToken, id: string) {
		await mutate(async () => { await adminDelete(`/api/v0/tokens/${token.id}/quota/${id}`); notice = t('tokens-limits-removed-toast'); await refresh(); });
	}
	async function setMcpPolicy(token: ManagedToken, allow: boolean) {
		await mutate(async () => { await adminPut(`/api/v0/tokens/${token.id}/mcp-policy`, { allow });
			notice = allow ? t('tokens-mcp-ask-enabled-toast') : t('tokens-mcp-ask-disabled-toast'); await refresh(); });
	}
	async function rotate(token: ManagedToken) {
		if (!confirm(t('tokens-rotate-confirm'))) return;
		await mutate(async () => { const response = await api.rotateToken(token.id); minted = { name: response.token.name, plaintext: response.plaintext }; await refresh(); window.scrollTo({ top: 0, behavior: 'smooth' }); });
	}
	async function revoke(token: ManagedToken) { if (confirm(t('tokens-revoke-confirm'))) await mutate(async () => { await api.revokeToken(token.id); await refresh(); }); }
	async function remove(token: ManagedToken) { if (confirm(t('tokens-remove-confirm'))) await mutate(async () => { await api.deleteToken(token.id); await refresh(); }); }

	onMount(refresh);
</script>

<div class="mx-auto w-full max-w-5xl px-4 pb-6 pt-14 sm:px-6 sm:pt-6">
	<h1 class="mb-2 text-2xl font-bold">{t('tokens-page-heading')}</h1>
	<p class="mb-6 text-sm text-base-content/60">{t('tokens-intro')}</p>
	{#if error}<div class="alert alert-error mb-4"><span>{error}</span></div>{/if}
	{#if notice}<div class="alert alert-info mb-4"><span>{notice}</span></div>{/if}

	{#if minted}
		<section class="card mb-6"><div class="card-body">
			<h2 class="card-title text-base"><span class="text-success">✓</span>{t('tokens-minted-heading')}</h2>
			<p class="text-sm text-base-content/70">{t('tokens-minted-copy-warning')}</p>
			<div class="relative"><pre class="m-0 w-full min-w-0 select-all whitespace-pre-wrap break-all rounded-md border border-base-300 bg-base-100 p-3 pr-12 font-mono text-xs">{minted.plaintext}</pre><button class="btn btn-ghost btn-sm btn-square absolute right-1.5 top-1.5" onclick={() => navigator.clipboard?.writeText(minted?.plaintext ?? '')} aria-label={t('tokens-copy-aria')}>{t('webhooks-copy')}</button></div>
			<p class="mb-0 mt-3 text-xs text-base-content/60">{t('tokens-minted-name', { name: minted.name })}</p>
		</div></section>
	{/if}

	{#if details?.push_enabled}<PushNotificationsCard />{/if}
	<form class="card mb-6 border border-base-300" onsubmit={(event) => { event.preventDefault(); void create(); }}><div class="card-body">
		<h2 class="card-title">{t('tokens-create-heading')}</h2><p class="text-base-content/70">{t('tokens-create-description')}</p>
		<label class="flex w-full flex-col gap-1"><span class="label-text">{t('tokens-name-label')}</span><input class="input input-bordered w-full" required placeholder={t('tokens-name-placeholder')} bind:value={name} /></label>
		<label class="flex w-32 flex-col gap-1"><span class="label-text">{t('tokens-ttl-label')}</span><input class="input input-bordered w-full" type="number" min="1" max="1825" bind:value={ttlDays} /></label>
		<div class="card-actions mt-2 justify-end"><button class="btn btn-primary" type="submit" disabled={busy}>{t('tokens-create-submit')}</button></div>
	</div></form>

	<section class="card border border-base-300"><div class="card-body">
		<h2 class="card-title">{t('tokens-list-heading')}</h2>
		{#if !details}<div class="skeleton h-24 w-full"></div>{:else if details.tokens.length === 0}<p class="text-sm text-base-content/60">{t('tokens-list-empty')}</p>{:else}
			<ul class="flex flex-col divide-y divide-base-300">{#each details.tokens as token (token.id)}
				<ManagedTokenRow {token} tools={details.tools} models={details.models} currency={details.currency} timezone={details.timezone} usageEnabled={details.usage_enabled}
					ontools={(enabled, disabled) => updateTools(token, enabled, disabled)} onmodels={(restrict, models) => updateModels(token, restrict, models)}
					onquota={(dimension, window, value) => addQuota(token, dimension, window, value)} onremovequota={(id) => removeQuota(token, id)}
					onmcp={(allow) => setMcpPolicy(token, allow)} onrotate={() => rotate(token)} onrevoke={() => revoke(token)} onremove={() => remove(token)} />
			{/each}</ul>
		{/if}
	</div></section>
	{#if details}<TokenAccountCard account={details.account} />{/if}
</div>
