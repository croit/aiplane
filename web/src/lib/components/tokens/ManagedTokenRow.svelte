<script lang="ts">
	import { n, t } from '$lib/i18n.svelte';
	import { tokenDate, type ManagedToken } from '$lib/tokens';
	import type { ToolEntry } from '$lib/tools';

	let { token, tools, models, currency, timezone, usageEnabled, ontools, onmodels, onquota, onremovequota, onmcp, onrotate, onrevoke, onremove }: {
		token: ManagedToken;
		tools: Omit<ToolEntry, 'enabled'>[];
		models: string[];
		currency: string;
		timezone: string;
		usageEnabled: boolean;
		ontools: (enabled: boolean, disabled: string[]) => Promise<void>;
		onmodels: (restrict: boolean, models: string[]) => Promise<void>;
		onquota: (dimension: string, window: string, value: number) => Promise<void>;
		onremovequota: (id: string) => Promise<void>;
		onmcp: (allow: boolean) => Promise<void>;
		onrotate: () => Promise<void>;
		onrevoke: () => Promise<void>;
		onremove: () => Promise<void>;
	} = $props();

	let restrict = $state(false);
	let selectedModels = $state<string[]>([]);
	let dimension = $state('requests');
	let windowKind = $state('day');
	let quotaValue = $state<number | null>(null);
	let busy = $state(false);

	$effect(() => {
		restrict = token.owner_models !== null;
		selectedModels = token.owner_models ?? [];
	});

	let modelsSummary = $derived(token.owner_models === null
		? t('tokens-models-summary-all')
		: t('tokens-models-summary-restricted', { count: token.owner_models.length }));
	let quotaSummary = $derived(token.quotas.length
		? t('tokens-limits-summary-some', { count: token.quotas.length })
		: t('tokens-limits-summary-none'));

	async function run(action: () => Promise<void>) {
		if (busy) return;
		busy = true;
		try { await action(); } finally { busy = false; }
	}

	function modelChanged(model: string, checked: boolean) {
		selectedModels = checked ? [...selectedModels, model] : selectedModels.filter((entry) => entry !== model);
	}

	function toolChanged(key: string, checked: boolean) {
		const disabled = checked
			? token.disabled_tools.filter((entry) => entry !== key)
			: [...new Set([...token.disabled_tools, key])];
		void run(() => ontools(true, disabled));
	}
</script>

<li class="py-3">
	<div class="flex flex-wrap items-center gap-3 sm:gap-4">
		<div class="min-w-48 flex-1">
			<div class="text-sm font-medium text-base-content">{token.name}</div>
			<div class="text-xs text-base-content/60">
				{t('tokens-row-meta', {
					created: tokenDate(token.created_at, timezone),
					last_used: token.last_used_at ? tokenDate(token.last_used_at, timezone) : t('tokens-last-used-never'),
					expires: tokenDate(token.expires_at, timezone)
				})}
			</div>
			{#if usageEnabled}
				<div class="text-xs text-base-content/60">{t('tokens-usage-line', {
					requests: token.usage?.requests ?? 0,
					tokens: token.usage?.tokens ?? 0,
					cost: `${n(token.usage?.cost ?? 0, { minimumFractionDigits: 2, maximumFractionDigits: 2 })} ${currency}`
				})}</div>
			{/if}
		</div>
		{#if token.revoked}
			<span class="badge badge-error">{t('tokens-badge-revoked')}</span>
			<button class="btn btn-outline btn-sm" onclick={() => run(onremove)} disabled={busy}>{t('tokens-remove-button')}</button>
		{:else}
			<span class="badge badge-secondary">{t('tokens-badge-active')}</span>
			<button class="btn btn-outline btn-sm" title={t('tokens-rotate-title')} onclick={() => run(onrotate)} disabled={busy}>{t('tokens-rotate-button')}</button>
			<button class="btn btn-error btn-sm" onclick={() => run(onrevoke)} disabled={busy}>{t('tokens-revoke-button')}</button>
		{/if}
	</div>

	{#if !token.revoked}
		<label class="mt-3 flex items-center gap-3 text-sm">
			<input type="checkbox" class="toggle toggle-primary toggle-sm" checked={token.tools_enabled} onchange={(event) => run(() => ontools(event.currentTarget.checked, token.disabled_tools))} disabled={busy} aria-label={t('tokens-tool-use-aria')} />
			<span><span class="font-medium">{t('tokens-tool-use-label')}</span><span class="ml-2 text-xs text-base-content/60">{t('tokens-tool-use-description')}</span></span>
		</label>

		{#if token.tools_enabled}
			<details class="collapse collapse-arrow mt-2 border-t border-base-300">
				<summary class="collapse-title min-h-0 px-0 py-3 text-sm font-medium">{t('tokens-capabilities-summary')}</summary>
				<div class="collapse-content px-0">
					<div class="divide-y divide-base-300">
						{#each tools as tool (tool.key)}
							<label class="flex items-center gap-4 py-2">
								<span class="min-w-0 flex-1"><span class="text-sm">{tool.title}</span> <code class="text-xs text-base-content/50">{tool.tech}</code><span class="block text-xs text-base-content/60">{tool.description}</span></span>
								<input type="checkbox" class="toggle toggle-primary toggle-sm" checked={!token.disabled_tools.includes(tool.key)} onchange={(event) => toolChanged(tool.key, event.currentTarget.checked)} disabled={busy} aria-label={t('tools-toggle-aria', { name: tool.title })} />
							</label>
						{/each}
					</div>
				</div>
			</details>
			<label class="mt-3 flex items-start gap-3">
				<input type="checkbox" class="checkbox checkbox-sm" checked={token.mcp_allow} onchange={(event) => run(() => onmcp(event.currentTarget.checked))} disabled={busy} aria-label={t('tokens-mcp-allow-aria')} />
				<span><span class="text-sm font-medium">{t('tokens-mcp-allow-label')}</span><span class="block text-xs text-base-content/60">{t('tokens-mcp-allow-description')}</span></span>
			</label>
		{/if}

		<details class="collapse collapse-arrow border-t border-base-300">
			<summary class="collapse-title min-h-0 px-0 py-3 text-sm font-medium">{modelsSummary}</summary>
			<div class="collapse-content px-0">
				<p class="text-xs text-base-content/60">{t('tokens-models-help')}</p>
				<label class="my-3 flex items-center gap-3 text-sm"><input type="checkbox" class="toggle toggle-primary toggle-sm" bind:checked={restrict} />{t('tokens-models-restrict-label')}</label>
				{#if token.admin_models}<div class="alert mb-3 text-xs"><span>{t('tokens-models-admin-set', { models: token.admin_models.join(', ') })}</span></div>{/if}
				{#if restrict}<div class="grid gap-2 sm:grid-cols-2">{#each models as model}<label class="flex items-center gap-2 text-sm"><input type="checkbox" class="checkbox checkbox-sm" checked={selectedModels.includes(model)} onchange={(event) => modelChanged(model, event.currentTarget.checked)} /> <code class="break-all text-xs">{model}</code></label>{/each}</div>{/if}
				<div class="mt-3 flex items-center justify-end gap-3">{#if restrict && selectedModels.length === 0}<span class="text-xs text-error">{t('tokens-models-none-picked')}</span>{/if}<button class="btn btn-primary btn-sm" disabled={busy || (restrict && selectedModels.length === 0)} onclick={() => run(() => onmodels(restrict, selectedModels))}>{t('tokens-models-save')}</button></div>
			</div>
		</details>

		<details class="collapse collapse-arrow border-t border-base-300">
			<summary class="collapse-title min-h-0 px-0 py-3 text-sm font-medium">{quotaSummary}</summary>
			<div class="collapse-content px-0">
				<p class="text-xs text-base-content/60">{t('tokens-limits-help')}</p>
				{#each token.quotas as quota (quota.id)}<div class="flex flex-wrap items-center gap-2 border-b border-base-300 py-2 text-sm"><code>{quota.value} {quota.dimension} / {quota.window}</code>{#if quota.model}<span class="badge badge-outline">{quota.model}</span>{/if}{#if quota.managed_by === 'admin'}<span class="badge badge-secondary">{t('tokens-limits-admin-badge')}</span>{:else}<button class="btn btn-ghost btn-xs ml-auto" onclick={() => run(() => onremovequota(quota.id))}>{t('tokens-limits-remove')}</button>{/if}</div>{/each}
				<div class="mt-3 flex flex-wrap items-end gap-2">
					<select class="select select-bordered select-sm" bind:value={dimension}><option value="requests">{t('limits-dim-requests')}</option><option value="tokens">{t('limits-dim-tokens')}</option><option value="cost">{t('limits-dim-cost')}</option></select>
					<select class="select select-bordered select-sm" bind:value={windowKind}><option value="hour">{t('limits-win-hour')}</option><option value="day">{t('limits-win-day')}</option><option value="week">{t('limits-win-week')}</option><option value="month">{t('limits-win-month')}</option></select>
					<input class="input input-bordered input-sm w-32" type="number" min="0" step="any" bind:value={quotaValue} placeholder={t('tokens-quota-max-placeholder')} aria-label={t('tokens-quota-max-placeholder')} />
					<button class="btn btn-primary btn-sm" disabled={busy || quotaValue === null || quotaValue < 0} onclick={() => run(() => onquota(dimension, windowKind, quotaValue ?? 0))}>{t('tokens-limits-add')}</button>
				</div>
			</div>
		</details>
	{/if}
</li>
