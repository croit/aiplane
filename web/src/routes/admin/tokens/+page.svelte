<script lang="ts">
	import { onMount } from 'svelte';
	import { adminJson, adminPut } from '$lib/admin-client';
	import { t, n } from '$lib/i18n.svelte';

	interface AdminToken {
		id: string;
		name: string;
		owner_id: string;
		owner_email: string;
		revoked: boolean;
		tools_enabled: boolean;
		owner_models: string[] | null;
		admin_models: string[] | null;
		limits: unknown[];
		usage_this_month: { requests: number; total_tokens: number; cost: number };
	}
	interface TokensData {
		tokens: AdminToken[];
		models: string[];
		usage_enabled: boolean;
		currency: string;
	}

	let data = $state<TokensData | null>(null);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let editing = $state<string | null>(null);
	let editRestrict = $state(false);
	let editModels = $state<string[]>([]);

	async function refresh() {
		try {
			data = await adminJson<TokensData>('/api/v0/admin/tokens');
			error = null;
		} catch (err) {
			error = String(err);
		}
	}

	function openEdit(token: AdminToken) {
		editing = token.id;
		editRestrict = (token.admin_models?.length ?? 0) > 0;
		editModels = token.admin_models ? [...token.admin_models] : [];
	}

	async function saveModels() {
		if (!editing) return;
		try {
			await adminPut(`/api/v0/admin/tokens/${encodeURIComponent(editing)}/models`, {
				restrict: editRestrict,
				models: editModels
			});
			editing = null;
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	function toggleModel(model: string) {
		const idx = editModels.indexOf(model);
		if (idx >= 0) editModels.splice(idx, 1);
		else editModels.push(model);
		editModels = editModels;
	}

	onMount(refresh);
</script>

{#if error}<div class="alert alert-error mb-4"><span>{error}</span></div>{/if}
{#if notice}<div class="alert alert-warning mb-4"><span>{notice}</span></div>{/if}

{#if data}
	{#if editing}
		<div class="card border border-base-300 mb-6">
			<div class="card-body">
				<h2 class="card-title text-base">{t('admin-tokens-allowlist-heading')}</h2>
				<p class="text-sm text-base-content/60">{t('admin-tokens-models-help')}</p>
				<label class="label cursor-pointer gap-2 my-1">
					<input type="checkbox" class="toggle toggle-primary" bind:checked={editRestrict} />
					<span class="label-text">{t('admin-tokens-models-restrict-label')}</span>
				</label>
				{#if editRestrict}
					<div class="flex flex-wrap gap-2">
						{#each data.models as model (model)}
							<label class="label cursor-pointer gap-1">
								<input
									type="checkbox"
									class="checkbox checkbox-sm"
									checked={editModels.includes(model)}
									onchange={() => toggleModel(model)}
								/>
								<span class="label-text text-xs font-mono">{model}</span>
							</label>
						{/each}
					</div>
				{/if}
				<div class="card-actions justify-end mt-2">
					<button class="btn btn-ghost btn-sm" onclick={() => (editing = null)}>{t('admin-cancel')}</button>
					<button class="btn btn-primary btn-sm" onclick={saveModels}>{t('groups-save')}</button>
				</div>
			</div>
		</div>
	{/if}

	<div class="card border border-base-300">
		<div class="card-body">
			<h2 class="card-title text-base">{t('admin-tokens-heading')}</h2>
			<div class="overflow-x-auto">
				<table class="table table-sm">
					<thead>
						<tr>
							<th>{t('admin-tokens-col-name')}</th>
							<th>{t('admin-tokens-col-owner')}</th>
							<th>{t('admin-tokens-col-state')}</th>
							<th>{t('admin-tokens-models-button')}</th>
							<th>{t('admin-cap-tools')}</th>
							<th>{t('admin-tokens-col-this-month')}</th>
							<th></th>
						</tr>
					</thead>
					<tbody>
						{#each data.tokens as token (token.id)}
							<tr>
								<td>{token.name}</td>
								<td class="text-xs">{token.owner_email}</td>
								<td>
									{#if token.revoked}
										<span class="badge badge-error badge-sm">{t('tokens-badge-revoked')}</span>
									{:else}
										<span class="badge badge-secondary badge-sm">{t('tokens-badge-active')}</span>
									{/if}
								</td>
								<td class="text-xs">
									{#if (token.admin_models?.length ?? 0) > 0}
										{t('admin-tokens-models-summary-restricted', { count: token.admin_models!.length })}
									{:else}
										<span class="text-base-content/50">{t('admin-tokens-models-summary-all')}</span>
									{/if}
								</td>
								<td>{token.tools_enabled ? t('admin-tokens-tools-on') : t('admin-tokens-tools-off')}</td>
								<td class="text-xs">
									{#if data.usage_enabled}
										{t('tokens-usage-line', {
											requests: n(token.usage_this_month.requests),
											tokens: n(token.usage_this_month.total_tokens),
											cost: `${n(token.usage_this_month.cost, {
												minimumFractionDigits: 2,
												maximumFractionDigits: 2
											})} ${data.currency}`
										})}
									{:else}
										<span class="text-base-content/50">{t('admin-tokens-usage-off')}</span>
									{/if}
								</td>
								<td>
									<button class="btn btn-ghost btn-xs" onclick={() => openEdit(token)}>
										{t('admin-tokens-models-button')}
									</button>
								</td>
							</tr>
						{/each}
					</tbody>
				</table>
			</div>
		</div>
	</div>
{/if}
