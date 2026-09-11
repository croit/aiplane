<script lang="ts">
	import { tokenState, visibleTokenModels } from '$lib/admin-tokens';
	import type { AdminToken, AdminTokenLimit } from '$lib/admin-tokens';
	import { t } from '$lib/i18n.svelte';
	import { tokenDate } from '$lib/tokens';
	import { usageCost, usageInteger } from '$lib/usage';

	let { token, models, usageEnabled, currency, timezone, onsave }: {
		token: AdminToken;
		models: string[];
		usageEnabled: boolean;
		currency: string;
		timezone: string;
		onsave: (id: string, restrict: boolean, models: string[]) => Promise<void>;
	} = $props();
	let details: HTMLDetailsElement;
	let restrict = $state(false);
	let selected = $state<string[]>([]);
	let saving = $state(false);
	let error = $state<string | null>(null);

	function prepareEditor() {
		restrict = token.admin_models !== null;
		selected = token.admin_models ? [...token.admin_models] : [...models];
	}

	function setRestrict(enabled: boolean) {
		restrict = enabled;
		if (enabled && selected.length === 0) selected = [...models];
	}

	function toggleModel(model: string) {
		selected = selected.includes(model)
			? selected.filter((candidate) => candidate !== model)
			: [...selected, model];
	}

	async function save() {
		saving = true;
		error = null;
		try {
			await onsave(token.id, restrict, selected);
			details.open = false;
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
	<td><div class="font-medium">{token.name}</div><div class="break-all font-mono text-xs text-base-content/50">{token.id}</div></td>
	<td class="break-all">{token.owner_email}</td>
	<td>
		{#if credentialState === 'revoked'}<span class="badge badge-error">{t('tokens-badge-revoked')}</span>
		{:else if credentialState === 'expired'}<span class="badge badge-warning">{t('admin-tokens-badge-expired')}</span>
		{:else}<span class="badge badge-secondary">{t('tokens-badge-active')}</span>{/if}
	</td>
	<td class="text-xs text-base-content/70">
		{t('tokens-row-meta', {
			created: tokenDate(token.created_at, timezone),
			last_used: token.last_used_at ? tokenDate(token.last_used_at, timezone) : t('tokens-last-used-never'),
			expires: tokenDate(token.expires_at, timezone)
		})}
	</td>
	<td class="text-right tabular-nums">{usageEnabled ? usageInteger(token.usage_this_month.requests) : '—'}</td>
	<td class="text-right tabular-nums">{usageEnabled ? usageInteger(token.usage_this_month.total_tokens) : '—'}</td>
	<td class="text-right tabular-nums">{usageEnabled ? usageCost(token.usage_this_month.cost, currency) : '—'}</td>
	<td>
		<div class="flex min-w-64 flex-col gap-1">
			<div class="break-all text-xs {ownerModels === null ? 'text-base-content/50' : 'font-mono'}">{ownerModels?.join(', ') || t('limits-all-models')}</div>
			{#if token.limits.length > 0}<div class="text-xs text-base-content/70">{token.limits.map(limitLabel).join(' · ')}</div>{/if}
			<details bind:this={details} class="collapse collapse-arrow border border-base-300 bg-base-100">
				<summary class="collapse-title min-h-0 px-3 py-2 text-xs font-medium" onclick={prepareEditor}>{token.admin_models === null ? t('admin-tokens-models-summary-all') : t('admin-tokens-models-summary-restricted', { count: token.admin_models.length })}</summary>
				<div class="collapse-content flex flex-col gap-3 px-3 pb-3">
					{#if error}<div class="alert alert-error py-2 text-xs"><span>{error}</span></div>{/if}
					<p class="text-xs text-base-content/70">{t('admin-tokens-models-help')}</p>
					<label class="label cursor-pointer justify-start gap-2"><input type="checkbox" class="toggle toggle-primary toggle-sm" checked={restrict} onchange={(event) => setRestrict(event.currentTarget.checked)} /><span>{t('admin-tokens-models-restrict-label')}</span></label>
					<div class="flex flex-wrap gap-2">
						{#each models as model (model)}<label class="label cursor-pointer gap-1"><input type="checkbox" class="checkbox checkbox-sm" disabled={!restrict} checked={selected.includes(model)} onchange={() => toggleModel(model)} /><span class="font-mono text-xs">{model}</span></label>{/each}
					</div>
					<button class="btn btn-primary btn-sm self-end" disabled={saving || (restrict && selected.length === 0)} onclick={save}>{t('admin-tokens-models-save')}</button>
				</div>
			</details>
		</div>
	</td>
</tr>
