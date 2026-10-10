<script lang="ts">
	import PrioritySelect from '#lib/components/PrioritySelect.svelte';
	import type { Priority } from '#lib/priority.js';
	import StatusPill from '#lib/components/ui/StatusPill.svelte';
	import EditModal from '#lib/components/EditModal.svelte';
	import { tokenState, visibleTokenModels } from '#lib/admin-tokens.js';
	import type { AdminToken, AdminTokenLimit } from '#lib/admin-tokens.js';
	import TokenModelPicker from '#lib/components/tokens/TokenModelPicker.svelte';
	import type { TokenModel } from '#lib/token-models.js';
	import { t } from '#lib/i18n.svelte.js';
	import { tokenDate } from '#lib/tokens.js';
	import { usageCost, usageInteger } from '#lib/usage.js';

	let { token, models, usageEnabled, currency, timezone, onsave, onpriority }: {
		token: AdminToken;
		models: TokenModel[];
		usageEnabled: boolean;
		currency: string;
		timezone: string;
		onpriority: (id: string, priority: Priority | null) => Promise<void>;
		onsave: (id: string, restrict: boolean, models: string[]) => Promise<void>;
	} = $props();
	let priorityDraft = $state<Priority | null>(null);
	$effect(() => { priorityDraft = token.priority; });
	let prioritySaving = $state(false);
	let priorityError = $state<string | null>(null);
	async function savePriority(priority: Priority | null) {
		prioritySaving = true; priorityError = null;
		try { await onpriority(token.id, priority); }
		catch (caught) { priorityError = String(caught); priorityDraft = token.priority; }
		finally { prioritySaving = false; }
	}
	let editing = $state(false);
	let restrict = $state(false);
	let selected = $state<string[]>([]);
	let saving = $state(false);
	let error = $state<string | null>(null);

	/** Fresh draft every time the dialog opens, so a cancelled edit never
	 *  leaks into the next one. */
	function openEditor() {
		restrict = token.admin_models !== null;
		selected = token.admin_models ? [...token.admin_models] : models.map((model) => model.id);
		error = null;
		editing = true;
	}

	async function save() {
		saving = true;
		error = null;
		try {
			await onsave(token.id, restrict, selected);
			editing = false;
		} catch (caught) {
			error = String(caught);
		} finally {
			saving = false;
		}
	}

	function limitLabel(limit: AdminTokenLimit) {
		const value = limit.dimension === 'cost'
			? usageCost(limit.value, currency)
			: usageInteger(limit.value);
		const dimension = limit.dimension === 'requests'
			? t('limits-dim-requests')
			: limit.dimension === 'tokens'
				? t('limits-dim-tokens')
				: t('limits-dim-cost-short');
		const window = limit.window === 'hour'
			? t('limits-win-hour')
			: limit.window === 'day'
				? t('limits-win-day')
				: limit.window === 'week'
					? t('limits-win-week')
					: t('limits-win-month');
		return `${value} ${dimension} / ${window}${limit.model ? ` · ${limit.model}` : ''}`;
	}

	const credentialState = $derived(tokenState(token));
	const ownerModels = $derived(visibleTokenModels(token.owner_models));
</script>

<tr>
	<td class="w-64"><div class="font-medium">{token.name}</div><div class="max-w-56 truncate font-mono text-xs text-base-content/50" title={token.id}>{token.id}</div></td>
	<td class="w-56"><div class="max-w-52 truncate" title={token.owner_email}>{token.owner_email}</div></td>
	<td class="w-28">
		{#if credentialState === 'revoked'}<StatusPill tone="bad">{t('tokens-badge-revoked')}</StatusPill>
		{:else if credentialState === 'expired'}<StatusPill tone="warn">{t('admin-tokens-badge-expired')}</StatusPill>
		{:else}<StatusPill tone="ok">{t('tokens-badge-active')}</StatusPill>{/if}
	</td>
	<td class="min-w-48"><PrioritySelect bind:value={priorityDraft} inheritLabel={t('priority-inherit')} disabled={prioritySaving} onchange={(value) => void savePriority(value)} />{#if priorityError}<p class="text-error text-xs" role="alert">{priorityError}</p>{/if}</td>
	<td class="w-64 text-xs leading-relaxed text-base-content/70">
		{t('tokens-row-meta', {
			created: tokenDate(token.created_at, timezone),
			last_used: token.last_used_at ? tokenDate(token.last_used_at, timezone) : t('tokens-last-used-never'),
			expires: tokenDate(token.expires_at, timezone)
		})}
	</td>
	<td class="w-24 whitespace-nowrap text-right tabular-nums">{usageEnabled ? usageInteger(token.usage_this_month.requests) : '—'}</td>
	<td class="w-24 whitespace-nowrap text-right tabular-nums">{usageEnabled ? usageInteger(token.usage_this_month.total_tokens) : '—'}</td>
	<td class="w-28 whitespace-nowrap text-right tabular-nums">{usageEnabled ? usageCost(token.usage_this_month.cost, currency) : '—'}</td>
	<td>
		<div class="flex min-w-72 flex-col gap-1">
			<div class="break-all text-xs {ownerModels === null ? 'text-base-content/50' : 'font-mono'}">{ownerModels?.join(', ') || t('limits-all-models')}</div>
			{#if token.limits.length > 0}<div class="text-xs text-base-content/70">{token.limits.map(limitLabel).join(' · ')}</div>{/if}
			<div class="flex items-center gap-2">
				<span class="text-xs text-base-content/70">{token.admin_models === null ? t('admin-tokens-models-summary-all') : t('admin-tokens-models-summary-restricted', { count: token.admin_models.length })}</span>
				<button type="button" class="btn btn-ghost btn-xs" onclick={openEditor}>{t('admin-tokens-models-edit')}</button>
			</div>
			<EditModal
				bind:open={editing}
				wide
				title={t('admin-tokens-models-edit')}
				description={token.name}
				cancellabel={t('admin-tokens-models-cancel')}
				savelabel={t('admin-tokens-models-save')}
				saving={saving || (restrict && selected.length === 0)}
				onsave={save}
			>
				<div class="flex flex-col gap-3">
					{#if error}<div class="alert alert-error py-2 text-xs"><span>{error}</span></div>{/if}
					<TokenModelPicker {models} {currency} help={t('admin-tokens-models-help')} restrictLabel={t('admin-tokens-models-restrict-label')} allLabel={t('admin-tokens-models-summary-all')} bind:restrict bind:selected />
				</div>
			</EditModal>
		</div>
	</td>
</tr>
