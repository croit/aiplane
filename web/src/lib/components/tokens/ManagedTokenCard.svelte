<script lang="ts">
	import type { Snippet } from 'svelte';
	import PrioritySelect from '#lib/components/PrioritySelect.svelte';
	import { canChoosePriority, type Priority } from '#lib/priority.js';
	import StatusPill from '#lib/components/ui/StatusPill.svelte';
	import EditModal from '#lib/components/EditModal.svelte';
	import CapabilityPicker from '#lib/components/chat/CapabilityPicker.svelte';
	import LimitBars from '#lib/components/usage/LimitBars.svelte';
	import TokenModelPicker from './TokenModelPicker.svelte';
	import { n, t } from '#lib/i18n.svelte.js';
	import { daysUntilExpiry, quotaChanges, quotaUsed, tokenDate, type ManagedToken, type NewQuota } from '#lib/tokens.js';
	import { selectionSummary, type TokenModel } from '#lib/token-models.js';
	import { limitPercent, usageCost, usageInteger } from '#lib/usage.js';
	import type { UsageLimit } from '#lib/usage-types.js';
	import type { ChatCapability } from '#lib/api.js';

	type ToolState = 'on' | 'auto' | 'off';


	let { token, capabilities, models, ownerLimits, currency, timezone, usageEnabled, ownerPriority, maxPriority, priorityEnabled, onpriority, ontools, onmodels, onquotas, onrotate, onrevoke, onremove }: {
		token: ManagedToken;
		capabilities: Omit<ChatCapability, 'state'>[];
		models: TokenModel[];
		ownerLimits: UsageLimit[];
		currency: string;
		timezone: string;
		usageEnabled: boolean;
		ownerPriority: Priority;
		maxPriority: Priority;
		priorityEnabled: boolean;
		onpriority: (priority: Priority | null) => Promise<void>;
		ontools: (enabled: boolean, states: Record<string, ToolState>, mcpAllow: boolean) => Promise<void>;
		onmodels: (restrict: boolean, models: string[]) => Promise<void>;
		onquotas: (changes: { remove: string[]; add: NewQuota[] }) => Promise<void>;
		onrotate: () => Promise<void>;
		onrevoke: () => Promise<void>;
		onremove: () => Promise<void>;
	} = $props();

	let busy = $state(false);
	let picker = $state<CapabilityPicker | null>(null);
	let editingModels = $state(false);
	let editingTools = $state(false);
	let editingBudget = $state(false);
	let editingPriority = $state(false);
	let priorityDraft = $state<Priority | null>(null);
	let priorityError = $state<string | null>(null);

	let restrict = $state(false);
	let selectedModels = $state<string[]>([]);
	let toolsEnabled = $state(false);
	let toolStates = $state<Record<string, ToolState>>({});
	let mcpAllow = $state(false);
	let keptQuotas = $state<string[]>([]);
	let addedQuotas = $state<NewQuota[]>([]);
	let dimension = $state<NewQuota['dimension']>('cost');
	let windowKind = $state<NewQuota['window']>('month');
	let quotaValue = $state<number | null>(null);

	const menuId = $derived(`token-menu-${token.id}`);
	const expiresIn = $derived(daysUntilExpiry(token.expires_at, Date.now()));
	const modelSummary = $derived(selectionSummary(models, token.owner_models));
	const pinned = $derived(Object.values(token.tool_states).filter((state) => state === 'on').length);
	const tightest = $derived([...token.quota_status].sort((a, b) => limitPercent(b.used, b.limit) - limitPercent(a.used, a.limit))[0] ?? null);
	const draftLimits = $derived<UsageLimit[]>([
		...token.quotas.filter((quota) => keptQuotas.includes(quota.id) || quota.managed_by === 'admin').map((quota) => ({
			model: quota.model, dimension: quota.dimension, window: quota.window, limit: quota.value,
			used: quotaUsed(quota, token.quota_status) ?? 0, refreshes_at: ''
		})),
		...addedQuotas.map((quota) => ({ model: null, dimension: quota.dimension, window: quota.window, limit: quota.value, used: 0, refreshes_at: '' }))
	]);
	const visibleQuotas = $derived(token.quotas.filter((quota) => keptQuotas.includes(quota.id) || quota.managed_by === 'admin'));
	const pickerCapabilities = $derived(capabilities.map((capability): ChatCapability => ({
		...capability,
		state: toolStates[stateKey(capability)] ?? (capability.kind === 'skill' || capability.group === 'integrations' ? 'off' : 'auto')
	})));

	function stateKey(capability: Pick<ChatCapability, 'kind' | 'key'>): string {
		return capability.kind === 'skill' ? `skill:${capability.key}` : capability.key;
	}

	function open(which: 'models' | 'tools' | 'budget' | 'priority') {
		restrict = token.owner_models !== null;
		selectedModels = [...(token.owner_models ?? [])];
		toolsEnabled = token.tools_enabled;
		toolStates = { ...token.tool_states };
		mcpAllow = token.mcp_allow;
		keptQuotas = token.quotas.filter((quota) => quota.managed_by === 'owner').map((quota) => quota.id);
		addedQuotas = [];
		quotaValue = null;
		priorityDraft = token.priority;
		priorityError = null;
		editingPriority = which === 'priority';
		editingModels = which === 'models';
		editingTools = which === 'tools';
		editingBudget = which === 'budget';
	}

	async function save(action: () => Promise<void>) {
		if (busy) return;
		busy = true;
		try {
			await action();
			editingModels = editingTools = editingBudget = editingPriority = false;
		} catch {
			// The page has already said what failed; the draft stays open so it can be retried.
		} finally { busy = false; }
	}

	async function savePriority() {
		priorityError = null;
		await save(async () => {
			try { await onpriority(priorityDraft); }
			catch (caught) { priorityError = String(caught); throw caught; }
		});
	}

	async function act(action: () => Promise<void>) {
		document.getElementById(menuId)?.hidePopover();
		if (busy) return;
		busy = true;
		try { await action(); } finally { busy = false; }
	}

	async function setCapability(capability: ChatCapability, state: ChatCapability['state']) {
		const states = { ...toolStates };
		if (state === 'auto' && capability.kind === 'tool' && capability.group !== 'integrations') delete states[stateKey(capability)];
		else states[stateKey(capability)] = state;
		toolStates = states;
	}

	function addQuota() {
		if (quotaValue === null || quotaValue < 0) return;
		addedQuotas = [...addedQuotas, { dimension, window: windowKind, value: quotaValue }];
		quotaValue = null;
	}

	function removeDraftLimit(index: number) {
		const owned = visibleQuotas[index];
		if (owned) keptQuotas = keptQuotas.filter((id) => id !== owned.id);
		else addedQuotas = addedQuotas.filter((_, position) => position !== index - visibleQuotas.length);
	}

	function amount(limit: UsageLimit, value: number): string {
		return limit.dimension === 'cost' ? usageCost(value, currency) : usageInteger(value);
	}
</script>

{#snippet tile(title: string, icon: string, which: 'models' | 'tools' | 'budget' | 'priority', body: Snippet)}
	<button type="button" class="card card-border bg-base-100 text-left transition-colors hover:border-primary focus-visible:border-primary" onclick={() => open(which)} aria-label={`${title} · ${t('tokens-edit-button')}`}>
		<div class="card-body gap-1 p-4">
			<div class="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-base-content/60"><span aria-hidden="true">{icon}</span>{title}<span class="ml-auto text-base normal-case" aria-hidden="true">›</span></div>
			{@render body()}
		</div>
	</button>
{/snippet}

{#snippet modelsBody()}
	<div class="font-medium">{token.owner_models === null ? t('tokens-models-tile-all', { count: modelSummary.count }) : t('tokens-models-tile-some', { count: modelSummary.count, total: models.length })}</div>
	{#if modelSummary.nonCompliant > 0}
		<div class="text-sm text-warning">⚠ {t('tokens-models-tile-noncompliant', { count: modelSummary.nonCompliant })}</div>
	{:else if modelSummary.count > 0}
		<div class="text-sm text-success">✓ {t('tokens-models-tile-compliant')}</div>
	{/if}
	{#if modelSummary.maxOutputPerMillion !== null}<div class="text-xs text-base-content/60">{t('tokens-models-max-price', { price: `${n(modelSummary.maxOutputPerMillion, { maximumFractionDigits: 4 })} ${currency}` })}</div>{/if}
	{#if token.admin_models}<div class="text-xs text-base-content/60">{t('tokens-models-tile-admin', { count: token.admin_models.length })}</div>{/if}
{/snippet}

{#snippet toolsBody()}
	<div class="font-medium">{token.tools_enabled ? t('tokens-tools-tile-on') : t('tokens-tools-tile-off')}</div>
	{#if token.tools_enabled}
		{#if pinned > 0}<div class="text-sm text-base-content/70">{t('tokens-tools-tile-pinned', { count: pinned })}</div>{/if}
		<div class="text-xs text-base-content/60">{token.mcp_allow ? t('tokens-tools-tile-mcp-allowed') : t('tokens-tools-tile-mcp-blocked')}</div>
	{:else}
		<div class="text-xs text-base-content/60">{t('tokens-tool-use-description')}</div>
	{/if}
{/snippet}

{#snippet budgetBody()}
	{#if tightest}
		{@const percent = limitPercent(tightest.used, tightest.limit)}
		<div class="font-medium tabular-nums">{amount(tightest, tightest.used)} / {amount(tightest, tightest.limit)}</div>
		<progress class="progress w-full {percent >= 100 ? 'progress-error' : percent >= 90 ? 'progress-warning' : 'progress-primary'}" value={percent} max="100"></progress>
		<div class="text-xs text-base-content/60">{t(`limits-win-${tightest.window}`)}{token.quota_status.length > 1 ? ` · ${t('tokens-budget-tile-more', { count: token.quota_status.length - 1 })}` : ''}</div>
	{:else}
		<div class="font-medium">{t('tokens-budget-tile-none')}</div>
	{/if}
	{#if usageEnabled}<div class="text-xs text-base-content/60">{t('tokens-usage-line', { requests: usageInteger(token.usage?.requests ?? 0), tokens: usageInteger(token.usage?.tokens ?? 0), cost: usageCost(token.usage?.cost ?? 0, currency) })}</div>{/if}
	{#if ownerLimits.length > 0}<div class="text-xs text-base-content/60">{t('tokens-budget-owner-applies')}</div>{/if}
{/snippet}

{#snippet priorityBody()}
	<div class="font-medium">{token.priority === null ? t('priority-inherit-value', { priority: t(`priority-${ownerPriority}`) }) : t(`priority-${token.priority}`)}</div>
	{#if !priorityEnabled}<div class="text-xs text-base-content/60">{t('priority-owner-disabled')}</div>{/if}
{/snippet}

{#if token.revoked}
	<li class="card border border-dashed border-base-300 bg-base-200/40 opacity-70">
		<div class="card-body flex-row flex-wrap items-center gap-3 p-4">
			<div class="min-w-48 flex-1">
				<div class="font-medium">{token.name}</div>
				<div class="text-xs text-base-content/60">{t('tokens-row-meta', { created: tokenDate(token.created_at, timezone), last_used: token.last_used_at ? tokenDate(token.last_used_at, timezone) : t('tokens-last-used-never'), expires: tokenDate(token.expires_at, timezone) })}</div>
			</div>
			<StatusPill tone="bad">{t('tokens-badge-revoked')}</StatusPill>
			<button type="button" class="btn btn-sm" onclick={() => act(onremove)} disabled={busy}>{t('tokens-remove-button')}</button>
		</div>
	</li>
{:else}
	<li class="card card-border bg-base-200/40">
		<div class="card-body gap-4 p-4 sm:p-5">
			<div class="flex flex-wrap items-start gap-3">
				<div class="min-w-48 flex-1">
					<h3 class="m-0 text-base font-semibold">{token.name}</h3>
					<div class="text-xs text-base-content/60">{t('tokens-row-meta', { created: tokenDate(token.created_at, timezone), last_used: token.last_used_at ? tokenDate(token.last_used_at, timezone) : t('tokens-last-used-never'), expires: tokenDate(token.expires_at, timezone) })}</div>
				</div>
				<div class="flex items-center gap-2">
					{#if expiresIn === 'expired'}
						<StatusPill tone="warn">{t('admin-tokens-badge-expired')}</StatusPill>
					{:else}
						{#if expiresIn !== null}<span class="badge badge-warning badge-soft">{expiresIn === 0 ? t('tokens-expires-today') : t('tokens-expires-soon', { days: expiresIn })}</span>{/if}
						<StatusPill tone="ok">{t('tokens-badge-active')}</StatusPill>
					{/if}
					<button type="button" class="btn btn-ghost btn-sm btn-square" popovertarget={menuId} style="anchor-name:--{menuId}" aria-label={t('tokens-menu-aria')} title={t('tokens-menu-aria')}>⋯</button>
					<ul class="dropdown dropdown-end menu z-10 w-56 rounded-box bg-base-200 p-2 shadow" popover id={menuId} style="position-anchor:--{menuId}">
						<li><button type="button" onclick={() => act(onrotate)} title={t('tokens-rotate-title')}>{t('tokens-rotate-button')}</button></li>
						<li><button type="button" class="text-error" onclick={() => act(onrevoke)}>{t('tokens-revoke-button')}</button></li>
					</ul>
				</div>
			</div>

			<div class="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
				{@render tile(t('tokens-tile-models'), '◇', 'models', modelsBody)}
				{@render tile(t('tokens-tile-tools'), '⚒', 'tools', toolsBody)}
				{@render tile(t('tokens-tile-budget'), '◔', 'budget', budgetBody)}
				{@render tile(t('priority-label'), '↥', 'priority', priorityBody)}
			</div>
		</div>

		<EditModal bind:open={editingPriority} title={t('priority-label')} description={token.name}
			cancellabel={t('admin-cancel')} savelabel={t('tokens-save')}
			saving={busy || !canChoosePriority(priorityDraft, maxPriority)} onsave={savePriority}>
			<div class="flex flex-col gap-3">
				{#if priorityError}<div class="alert alert-error" role="alert"><span>{priorityError}</span></div>{/if}
				<PrioritySelect bind:value={priorityDraft} maximum={maxPriority} disabled={busy} inheritLabel={t('priority-inherit-value', { priority: t(`priority-${ownerPriority}`) })} />
				<p class="text-sm text-base-content/60">{t('priority-owner-inherited', { priority: t(`priority-${ownerPriority}`) })}</p>
				<p class="text-sm text-base-content/60">{t('priority-owner-help')}</p>
				{#if !priorityEnabled}<div class="alert alert-info"><span>{t('priority-owner-disabled')}</span></div>{/if}
			</div>
		</EditModal>

		<EditModal
			bind:open={editingModels}
			wide
			title={t('tokens-tile-models')}
			description={token.name}
			cancellabel={t('admin-cancel')}
			savelabel={t('tokens-save')}
			saving={busy || (restrict && selectedModels.length === 0)}
			onsave={() => save(() => onmodels(restrict, selectedModels))}
		>
			{#if token.admin_models}<div role="alert" class="alert alert-info alert-soft mb-3 text-sm"><span>{t('tokens-models-admin-set', { models: token.admin_models.join(', ') })}</span></div>{/if}
			<TokenModelPicker {models} {currency} help={t('tokens-models-help')} restrictLabel={t('tokens-models-restrict-label')} allLabel={t('tokens-models-tile-all', { count: models.length })} bind:restrict bind:selected={selectedModels} />
		</EditModal>

		<EditModal
			bind:open={editingTools}
			title={t('tokens-tile-tools')}
			description={token.name}
			cancellabel={t('admin-cancel')}
			savelabel={t('tokens-save')}
			saving={busy}
			onsave={() => save(() => ontools(toolsEnabled, toolStates, mcpAllow))}
		>
			<div class="flex flex-col gap-4">
				<label class="flex items-start gap-3">
					<input type="checkbox" class="toggle toggle-primary mt-0.5" bind:checked={toolsEnabled} />
					<span><span class="block text-sm font-medium">{t('tokens-tool-use-label')}</span><span class="block text-sm text-base-content/60">{t('tokens-tool-use-description')}</span></span>
				</label>
				{#if toolsEnabled}
					<div class="flex items-center justify-between gap-3 rounded-box border border-base-300 p-3">
						<span class="text-sm">{t('tokens-tools-capabilities-help')}</span>
						<button type="button" class="btn btn-outline btn-sm" onclick={() => picker?.show()}>{t('tokens-capabilities-summary')}</button>
					</div>
					<label class="flex items-start gap-3">
						<input type="checkbox" class="toggle toggle-primary mt-0.5" bind:checked={mcpAllow} />
						<span><span class="block text-sm font-medium">{t('tokens-mcp-allow-label')}</span><span class="block text-sm text-base-content/60">{t('tokens-mcp-allow-description')}</span></span>
					</label>
				{/if}
			</div>
		</EditModal>
		<!-- Outside the dialog on purpose: nested in its box, the full-screen picker would be clipped and restyled by it. -->
		{#if editingTools && toolsEnabled}
			<CapabilityPicker bind:this={picker} capabilities={pickerCapabilities} onset={setCapability} dialogId={`token-tool-selector-${token.id}`} showActive={false} showTrigger={false} />
		{/if}

		<EditModal
			bind:open={editingBudget}
			wide
			title={t('tokens-tile-budget')}
			description={token.name}
			cancellabel={t('admin-cancel')}
			savelabel={t('tokens-save')}
			saving={busy}
			onsave={() => save(() => onquotas(quotaChanges(token.quotas, keptQuotas, addedQuotas)))}
		>
			<div class="flex flex-col gap-5">
				<section class="flex flex-col gap-3">
					<h4 class="m-0 text-sm font-semibold">{t('tokens-budget-token-heading')}</h4>
					<p class="m-0 text-sm text-base-content/60">{t('tokens-limits-help')}</p>
					{#if draftLimits.length > 0}
						<LimitBars limits={draftLimits} {currency} {timezone}>
							{#snippet action(index)}
								{#if index < visibleQuotas.length && visibleQuotas[index].managed_by === 'admin'}
									<span class="badge badge-sm badge-secondary">{t('tokens-limits-admin-badge')}</span>
								{:else}
									<button type="button" class="btn btn-ghost btn-xs" onclick={() => removeDraftLimit(index)}>{t('tokens-limits-remove')}</button>
								{/if}
							{/snippet}
						</LimitBars>
					{:else}
						<p class="m-0 text-sm">{t('tokens-budget-tile-none')}</p>
					{/if}
					<div class="flex flex-wrap items-end gap-2">
						<select class="select select-sm w-auto" bind:value={dimension} aria-label={t('tokens-budget-dimension')}><option value="cost">{t('limits-dim-cost', { cur: currency })}</option><option value="requests">{t('limits-dim-requests')}</option><option value="tokens">{t('limits-dim-tokens')}</option></select>
						<select class="select select-sm w-auto" bind:value={windowKind} aria-label={t('tokens-budget-window')}><option value="hour">{t('limits-win-hour')}</option><option value="day">{t('limits-win-day')}</option><option value="week">{t('limits-win-week')}</option><option value="month">{t('limits-win-month')}</option></select>
						<input class="input input-sm w-32" type="number" min="0" step="any" bind:value={quotaValue} placeholder={t('tokens-quota-max-placeholder')} aria-label={t('tokens-quota-max-placeholder')} />
						<button type="button" class="btn btn-outline btn-sm" disabled={quotaValue === null || quotaValue < 0} onclick={addQuota}>{t('tokens-limits-add')}</button>
					</div>
				</section>
				<section class="flex flex-col gap-3 border-t border-base-300 pt-4">
					<h4 class="m-0 text-sm font-semibold">{t('tokens-budget-owner-heading')}</h4>
					{#if ownerLimits.length > 0}
						<LimitBars limits={ownerLimits} {currency} {timezone} />
					{:else}
						<p class="m-0 text-sm text-base-content/60">{t('tokens-budget-owner-none')}</p>
					{/if}
				</section>
			</div>
		</EditModal>
	</li>
{/if}
