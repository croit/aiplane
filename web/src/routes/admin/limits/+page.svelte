<script lang="ts">
	import { onMount } from 'svelte';
	import { adminJson, adminPost, adminDelete } from '$lib/admin-client';

	interface Rule {
		id: string;
		subject_type: string;
		subject_id: string;
		model: string | null;
		dimension: string;
		window: string;
		value: number;
		managed_by: string;
	}
	interface LimitsData {
		limits: Rule[];
		users: { id: string; email: string }[];
		tokens: { id: string; name: string; owner: string }[];
		roles: string[];
		models: string[];
		currency: string;
	}

	let data = $state<LimitsData | null>(null);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);

	let subjectType = $state('user');
	let subjectId = $state('');
	let model = $state('');
	let dimension = $state('requests');
	let window = $state('day');
	let value = $state('100');

	async function refresh() {
		try {
			data = await adminJson<LimitsData>('/api/v0/admin/limits');
			error = null;
		} catch (err) {
			error = String(err);
		}
	}

	async function save() {
		notice = null;
		try {
			await adminPost('/api/v0/admin/limits', {
				subject_type: subjectType,
				subject_id: subjectId,
				model,
				dimension,
				window,
				value: parseFloat(value)
			});
			subjectId = '';
			model = '';
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	async function remove(id: string) {
		if (!confirm('Delete this rule?')) return;
		try {
			await adminDelete(`/api/v0/admin/limits/${encodeURIComponent(id)}`);
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	function subjectLabel(rule: Rule): string {
		if (rule.subject_type === 'user') {
			return data?.users.find((u) => u.id === rule.subject_id)?.email ?? rule.subject_id;
		}
		if (rule.subject_type === 'token') {
			return data?.tokens.find((t) => t.id === rule.subject_id)?.name ?? rule.subject_id;
		}
		return rule.subject_id || '(global)';
	}

	onMount(refresh);
</script>

{#if error}<div class="alert alert-error mb-4"><span>{error}</span></div>{/if}
{#if notice}<div class="alert alert-warning mb-4"><span>{notice}</span></div>{/if}

{#if data}
	<div class="card border border-base-300 mb-6">
		<div class="card-body">
			<h2 class="card-title text-base">Add / update a rule</h2>
			<form
				class="flex flex-wrap gap-3 items-end"
				onsubmit={(e) => {
					e.preventDefault();
					void save();
				}}
			>
				<label class="flex flex-col gap-1 w-32">
					<span class="label-text">Subject</span>
					<select class="select select-bordered select-sm" bind:value={subjectType}>
						<option value="global">Global</option>
						<option value="role">Role</option>
						<option value="user">User</option>
						<option value="token">Token</option>
					</select>
				</label>
				<label class="flex flex-col gap-1 w-48">
					<span class="label-text">{subjectType === 'user' ? 'User (id or email)' : subjectType === 'role' ? 'Role' : subjectType === 'token' ? 'Token id' : ''}</span>
					{#if subjectType === 'user'}
						<input class="input input-bordered input-sm" bind:value={subjectId} list="limit-users" />
						<datalist id="limit-users">
							{#each data.users as u (u.id)}<option value={u.email}></option>{/each}
						</datalist>
					{:else if subjectType === 'role'}
						<select class="select select-bordered select-sm" bind:value={subjectId}>
							<option value="" disabled selected>…</option>
							{#each data.roles as r (r)}<option value={r}>{r}</option>{/each}
						</select>
					{:else}
						<input class="input input-bordered input-sm" bind:value={subjectId} disabled={subjectType === 'global'} />
					{/if}
				</label>
				<label class="flex flex-col gap-1 w-44">
					<span class="label-text">Model</span>
					<input class="input input-bordered input-sm" bind:value={model} list="limit-models" placeholder="all" />
					<datalist id="limit-models">
						{#each data.models as m (m)}<option value={m}></option>{/each}
					</datalist>
				</label>
				<label class="flex flex-col gap-1 w-32">
					<span class="label-text">Dimension</span>
					<select class="select select-bordered select-sm" bind:value={dimension}>
						<option value="requests">Requests</option>
						<option value="tokens">Tokens</option>
						<option value="cost">Cost ({data.currency})</option>
					</select>
				</label>
				<label class="flex flex-col gap-1 w-28">
					<span class="label-text">Window</span>
					<select class="select select-bordered select-sm" bind:value={window}>
						<option value="hour">Hour</option>
						<option value="day">Day</option>
						<option value="week">Week</option>
						<option value="month">Month</option>
					</select>
				</label>
				<label class="flex flex-col gap-1 w-28">
					<span class="label-text">Value</span>
					<input class="input input-bordered input-sm" type="number" min="0" step="any" bind:value={value} />
				</label>
				<button class="btn btn-primary btn-sm" type="submit" disabled={!value || (subjectType !== 'global' && !subjectId.trim())}>
					Save
				</button>
			</form>
		</div>
	</div>

	<div class="card border border-base-300">
		<div class="card-body">
			<h2 class="card-title text-base">Rules</h2>
			<div class="overflow-x-auto">
				<table class="table table-sm">
					<thead><tr><th>Subject</th><th>Model</th><th>Limit</th><th>Window</th><th>By</th><th></th></tr></thead>
					<tbody>
						{#each data.limits as rule (rule.id)}
							<tr>
								<td><span class="badge badge-outline badge-sm me-1">{rule.subject_type}</span>{subjectLabel(rule)}</td>
								<td class="font-mono text-xs">{rule.model ?? 'all'}</td>
								<td>{rule.value.toLocaleString()} {rule.dimension === 'cost' ? data.currency : rule.dimension}</td>
								<td>{rule.window}</td>
								<td class="text-xs text-base-content/50">{rule.managed_by}</td>
								<td><button class="btn btn-ghost btn-xs text-error" onclick={() => remove(rule.id)}>Delete</button></td>
							</tr>
						{:else}
							<tr><td colspan="6" class="text-base-content/50">No rules.</td></tr>
						{/each}
					</tbody>
				</table>
			</div>
		</div>
	</div>
{/if}
