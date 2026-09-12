<script lang="ts">
	import EditModal from '$lib/components/EditModal.svelte';
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
	let editing = $state(false);
	let restrict = $state(false);
	let selected = $state<string[]>([]);
	let saving = $state(false);
	let error = $state<string | null>(null);

	/** Fresh draft every time the dialog opens, so a cancelled edit never
	 *  leaks into the next one. */
	function openEditor() {
		restrict = token.admin_models !== null;
		selected = token.admin_models ? [...token.admin_models] : [...models];
		error = null;
		editing = true;
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
		{#if credentialState === 'revoked'}<span class="badge badge-error">{t('tokens-badge-revoked')}</span>
		{:else if credentialState === 'expired'}<span class="badge badge-warning">{t('admin-tokens-badge-expired')}</span>
		{:else}<span class="badge badge-secondary">{t('tokens-badge-active')}</span>{/if}
	</td>
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
				title={t('admin-tokens-models-edit')}
				description={token.name}
				cancellabel={t('admin-tokens-models-cancel')}
				savelabel={t('admin-tokens-models-save')}
				saving={saving || (restrict && selected.length === 0)}
				onsave={save}
			>
				<div class="flex flex-col gap-3">
					{#if error}<div class="alert alert-error py-2 text-xs"><span>{error}</span></div>{/if}
					<p class="m-0 text-xs text-base-content/70">{t('admin-tokens-models-help')}</p>
					<label class="label cursor-pointer justify-start gap-2"><input type="checkbox" class="toggle toggle-primary toggle-sm" checked={restrict} onchange={(event) => setRestrict(event.currentTarget.checked)} /><span>{t('admin-tokens-models-restrict-label')}</span></label>
					<div class="flex max-h-64 flex-wrap gap-2 overflow-y-auto">
						{#each models as model (model)}<label class="label cursor-pointer gap-1"><input type="checkbox" class="checkbox checkbox-sm" disabled={!restrict} checked={selected.includes(model)} onchange={() => toggleModel(model)} /><span class="font-mono text-xs">{model}</span></label>{/each}
					</div>
				</div>
			</EditModal>
		</div>
	</td>
</tr>
