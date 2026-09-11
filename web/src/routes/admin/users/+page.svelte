<script lang="ts">
	import { onMount } from 'svelte';
	import { adminJson, adminPost } from '$lib/admin-client';
	import NavIcon from '$lib/components/NavIcon.svelte';
	import UserIdentity from '$lib/components/admin/UserIdentity.svelte';
	import { dt, t } from '$lib/i18n.svelte';

	interface AdminUser {
		id: string;
		email: string;
		name: string | null;
		oidc_groups: string[];
		gateway_roles: string[];
		created_at: string;
	}

	interface ImpersonationEvent {
		id: string;
		action: string;
		actor_email: string;
		target_email: string;
		created_at: string;
	}

	interface UsersData {
		users: AdminUser[];
		audit: ImpersonationEvent[];
		current_user_id: string;
		allow_impersonation: boolean;
	}

	let data = $state<UsersData | null>(null);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);

	async function refresh() {
		try {
			data = await adminJson<UsersData>('/api/v0/admin/users');
			error = null;
		} catch (caught) {
			error = String(caught);
		}
	}

	async function impersonate(id: string) {
		if (!confirm(t('admin-users-impersonate-confirm'))) return;
		try {
			await adminPost(`/api/v0/admin/users/${encodeURIComponent(id)}/impersonate`);
			location.assign('/');
		} catch (caught) {
			notice = String(caught);
		}
	}

	onMount(refresh);
</script>

<section class="flex w-full flex-col gap-4">
	<header class="flex flex-col gap-1">
		<h1 class="text-2xl font-bold">{t('admin-users-heading')}</h1>
		{#if data?.allow_impersonation}
			<p class="text-sm text-base-content/70">
				{t('admin-users-desc-allowed-prefix')} <strong>{t('admin-users-impersonate-button')}</strong> {t('admin-users-desc-allowed-suffix')}
			</p>
		{:else if data}
			<p class="text-sm text-base-content/70">
				{t('admin-users-desc-disabled-prefix')} <strong>{t('admin-users-disabled-label')}</strong> {t('admin-users-desc-disabled-suffix')}
			</p>
		{/if}
	</header>

	{#if error}<div class="alert alert-error"><span>{error}</span></div>{/if}
	{#if notice}<div class="alert alert-warning"><span>{notice}</span></div>{/if}

	{#if data}
		<div class="card overflow-x-auto border border-base-300 bg-base-100">
			<table class="table table-sm">
				<thead><tr>
					<th>{t('admin-users-col-user')}</th>
					<th>{t('admin-users-col-oidc-groups')}</th>
					<th>{t('admin-users-col-gateway-roles')}</th>
					<th>{t('admin-users-col-joined')}</th>
					<th class="text-right">{t('admin-users-col-action')}</th>
				</tr></thead>
				<tbody>
					{#each data.users as user (user.id)}
						<tr>
							<td><UserIdentity email={user.email} name={user.name} id={user.id} /></td>
							<td class="text-sm">{user.oidc_groups.join(', ') || t('admin-users-no-oidc-groups')}</td>
							<td class="text-sm">{user.gateway_roles.join(', ') || t('admin-users-no-gateway-roles')}</td>
							<td class="whitespace-nowrap text-sm">{dt(user.created_at, { year: 'numeric', month: '2-digit', day: '2-digit' })}</td>
							<td class="text-right">
								{#if user.id === data.current_user_id}
									<span class="badge badge-ghost">{t('admin-users-you-badge')}</span>
								{:else if data.allow_impersonation}
									<button class="btn btn-outline btn-sm" onclick={() => impersonate(user.id)}><NavIcon name="users" size={14} /> {t('admin-users-impersonate-button')}</button>
								{:else}
									<span class="text-base-content/40">—</span>
								{/if}
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>

		<section class="card mt-2 border border-base-300 bg-base-100">
			<div class="card-body gap-2">
				<h2 class="card-title text-base">{t('admin-users-audit-heading')}</h2>
				{#if data.audit.length === 0}
					<p class="text-sm text-base-content/60">{t('admin-users-audit-empty')}</p>
				{:else}
					<div class="overflow-x-auto"><table class="table table-sm">
						<thead><tr><th>{t('admin-users-audit-col-when')}</th><th>{t('admin-users-audit-col-action')}</th><th>{t('admin-users-audit-col-admin')}</th><th>{t('admin-users-audit-col-target')}</th></tr></thead>
						<tbody>{#each data.audit as event (event.id)}<tr>
							<td class="whitespace-nowrap text-sm">{dt(event.created_at, { dateStyle: 'short', timeStyle: 'short' })}</td>
							<td><span class="badge {event.action === 'start' ? 'badge-warning' : 'badge-ghost'}">{event.action}</span></td>
							<td class="break-all text-sm">{event.actor_email}</td><td class="break-all text-sm">{event.target_email}</td>
						</tr>{/each}</tbody>
					</table></div>
				{/if}
			</div>
		</section>
	{/if}
</section>
