<script lang="ts">
	import { onMount } from 'svelte';
	import { adminJson, adminPut } from '$lib/admin-client';

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
				<h2 class="card-title text-base">Admin model allowlist</h2>
				<p class="text-sm text-base-content/60">
					Restricting limits this token to the ticked models (intersected with its owner's access and any owner-set list).
				</p>
				<label class="label cursor-pointer gap-2 my-1">
					<input type="checkbox" class="toggle toggle-primary" bind:checked={editRestrict} />
					<span class="label-text">Restrict to specific models</span>
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
					<button class="btn btn-ghost btn-sm" onclick={() => (editing = null)}>Cancel</button>
					<button class="btn btn-primary btn-sm" onclick={saveModels}>Save</button>
				</div>
			</div>
		</div>
	{/if}

	<div class="card border border-base-300">
		<div class="card-body">
			<h2 class="card-title text-base">Token register</h2>
			<div class="overflow-x-auto">
				<table class="table table-sm">
					<thead>
						<tr><th>Name</th><th>Owner</th><th>Status</th><th>Models</th><th>Tools</th><th>This month</th><th></th></tr>
					</thead>
					<tbody>
						{#each data.tokens as token (token.id)}
							<tr>
								<td>{token.name}</td>
								<td class="text-xs">{token.owner_email}</td>
								<td>
									{#if token.revoked}<span class="badge badge-error badge-sm">revoked</span>
									{:else}<span class="badge badge-secondary badge-sm">active</span>{/if}
								</td>
								<td class="text-xs">
									{#if (token.admin_models?.length ?? 0) > 0}
										{token.admin_models!.length} restricted
									{:else}
										<span class="text-base-content/50">all (admin)</span>
									{/if}
								</td>
								<td>{token.tools_enabled ? 'on' : 'off'}</td>
								<td class="text-xs">
									{#if data.usage_enabled}
										{token.usage_this_month.requests.toLocaleString()} req ·
										{token.usage_this_month.cost.toFixed(2)} {data.currency}
									{:else}
										<span class="text-base-content/50">usage off</span>
									{/if}
								</td>
								<td><button class="btn btn-ghost btn-xs" onclick={() => openEdit(token)}>Models</button></td>
							</tr>
						{/each}
					</tbody>
				</table>
			</div>
		</div>
	</div>
{/if}
