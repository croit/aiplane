<script lang="ts">
	import { onMount } from 'svelte';
	import { adminJson, adminPost, adminDelete } from '$lib/admin-client';
	import { t, n } from '$lib/i18n.svelte';

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
		if (!confirm(t('limits-delete-confirm'))) return;
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
			return data?.tokens.find((tok) => tok.id === rule.subject_id)?.name ?? rule.subject_id;
		}
		return rule.subject_id || t('limits-subject-global');
	}

	/** The second field changes meaning with the subject kind, so its label does too. */
	function subjectIdLabel(kind: string): string {
		if (kind === 'user') return t('limits-subject-user');
		if (kind === 'role') return t('limits-subject-role');
		if (kind === 'token') return t('limits-subject-token');
		return '';
	}

	onMount(refresh);
</script>

{#if error}<div class="alert alert-error mb-4"><span>{error}</span></div>{/if}
{#if notice}<div class="alert alert-warning mb-4"><span>{notice}</span></div>{/if}

{#if data}
	<div class="card border border-base-300 mb-6">
		<div class="card-body">
			<h2 class="card-title text-base">{t('limits-add-heading')}</h2>
			<form
				class="flex flex-wrap gap-3 items-end"
				onsubmit={(e) => {
					e.preventDefault();
					void save();
				}}
			>
				<label class="flex flex-col gap-1 w-32">
					<span class="label-text">{t('limits-field-subject')}</span>
					<select class="select select-bordered select-sm" bind:value={subjectType}>
						<option value="global">{t('limits-subject-global')}</option>
						<option value="role">{t('limits-subject-role')}</option>
						<option value="user">{t('limits-subject-user')}</option>
						<option value="token">{t('limits-subject-token')}</option>
					</select>
				</label>
				<label class="flex flex-col gap-1 w-48">
					<span class="label-text">{subjectIdLabel(subjectType)}</span>
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
					<span class="label-text">{t('limits-field-model')}</span>
					<input
						class="input input-bordered input-sm"
						bind:value={model}
						list="limit-models"
						placeholder={t('limits-field-model-ph')}
					/>
					<datalist id="limit-models">
						{#each data.models as m (m)}<option value={m}></option>{/each}
					</datalist>
				</label>
				<label class="flex flex-col gap-1 w-32">
					<span class="label-text">{t('limits-field-dimension')}</span>
					<select class="select select-bordered select-sm" bind:value={dimension}>
						<option value="requests">{t('limits-dim-requests')}</option>
						<option value="tokens">{t('limits-dim-tokens')}</option>
						<option value="cost">{t('limits-dim-cost', { cur: data.currency })}</option>
					</select>
				</label>
				<label class="flex flex-col gap-1 w-28">
					<span class="label-text">{t('limits-field-window')}</span>
					<select class="select select-bordered select-sm" bind:value={window}>
						<option value="hour">{t('limits-win-hour')}</option>
						<option value="day">{t('limits-win-day')}</option>
						<option value="week">{t('limits-win-week')}</option>
						<option value="month">{t('limits-win-month')}</option>
					</select>
				</label>
				<label class="flex flex-col gap-1 w-28">
					<span class="label-text">{t('limits-field-value')}</span>
					<input class="input input-bordered input-sm" type="number" min="0" step="any" bind:value={value} />
				</label>
				<button class="btn btn-primary btn-sm" type="submit" disabled={!value || (subjectType !== 'global' && !subjectId.trim())}>
					{t('limits-add-submit')}
				</button>
			</form>
		</div>
	</div>

	<div class="card border border-base-300">
		<div class="card-body">
			<h2 class="card-title text-base">{t('limits-rules-heading')}</h2>
			<div class="overflow-x-auto">
				<table class="table table-sm">
					<thead>
						<tr>
							<th>{t('limits-col-subject')}</th>
							<th>{t('limits-col-scope')}</th>
							<th>{t('limits-col-limit')}</th>
							<th>{t('limits-col-window')}</th>
							<th>{t('limits-col-managed-by')}</th>
							<th></th>
						</tr>
					</thead>
					<tbody>
						{#each data.limits as rule (rule.id)}
							<tr>
								<td><span class="badge badge-outline badge-sm me-1">{rule.subject_type}</span>{subjectLabel(rule)}</td>
								<td class="font-mono text-xs">{rule.model ?? t('limits-all-models')}</td>
								<td>
									{n(rule.value)}
									{rule.dimension === 'cost' ? data.currency : t(`limits-dim-${rule.dimension}`)}
								</td>
								<td>{t(`limits-win-${rule.window}`)}</td>
								<td class="text-xs text-base-content/50">{rule.managed_by}</td>
								<td>
									<button class="btn btn-ghost btn-xs text-error" onclick={() => remove(rule.id)}>
										{t('limits-delete')}
									</button>
								</td>
							</tr>
						{:else}
							<tr><td colspan="6" class="text-base-content/50">{t('limits-none')}</td></tr>
						{/each}
					</tbody>
				</table>
			</div>
		</div>
	</div>
{/if}
