<script lang="ts">
	import { onMount } from 'svelte';
	import { adminJson, adminPost } from '$lib/admin-client';
	import { t, dt } from '$lib/i18n.svelte';

	interface AdminUser {
		id: string;
		email: string;
		name: string | null;
		roles: string[];
		created_at: string;
	}

	let users = $state<AdminUser[]>([]);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);

	async function refresh() {
		try {
			const data = await adminJson<{ users: AdminUser[] }>('/api/v0/admin/users');
			users = data.users;
			error = null;
		} catch (err) {
			error = String(err);
		}
	}

	async function impersonate(id: string) {
		if (!confirm(t('admin-users-impersonate-confirm'))) return;
		try {
			await adminPost(`/api/v0/admin/users/${encodeURIComponent(id)}/impersonate`);
			location.reload();
		} catch (err) {
			notice = String(err);
		}
	}

	onMount(refresh);
</script>

{#if error}<div class="alert alert-error mb-4"><span>{error}</span></div>{/if}
{#if notice}<div class="alert alert-warning mb-4"><span>{notice}</span></div>{/if}

<div class="card border border-base-300">
	<div class="card-body">
		<h2 class="card-title text-base">{t('admin-users-heading')}</h2>
		<div class="overflow-x-auto">
			<table class="table table-sm">
				<thead>
					<tr>
						<th>{t('admin-users-col-email')}</th>
						<th>{t('tokens-account-user-id-label')}</th>
						<th>{t('admin-users-col-gateway-roles')}</th>
						<th>{t('admin-users-col-joined')}</th>
						<th></th>
					</tr>
				</thead>
				<tbody>
					{#each users as u (u.id)}
						<tr>
							<td>{u.email}</td>
							<td class="font-mono text-xs">{u.id}</td>
							<td>
								{#each u.roles as r (r)}<span class="badge badge-outline badge-sm me-1">{r}</span>{/each}
							</td>
							<td class="text-xs">{dt(u.created_at, { dateStyle: 'medium' })}</td>
							<td>
								<button class="btn btn-ghost btn-xs" onclick={() => impersonate(u.id)}>
									{t('admin-users-impersonate-button')}
								</button>
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	</div>
</div>
