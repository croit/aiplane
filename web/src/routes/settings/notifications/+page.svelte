<script lang="ts">
	import { onMount } from 'svelte';
	import { adminJson } from '#lib/admin-client.js';
	import PushNotificationsCard from '#lib/components/tokens/PushNotificationsCard.svelte';
	import { t } from '#lib/i18n.svelte.js';
	import type { TokenManagementDetails } from '#lib/tokens.js';

	let details = $state<TokenManagementDetails | null>(null);
	let error = $state<string | null>(null);

	onMount(async () => {
		try { details = await adminJson<TokenManagementDetails>('/api/v0/tokens/details'); }
		catch (caught) { error = String(caught); }
	});
</script>

{#if error}<div class="alert alert-error"><span>{error}</span></div>{/if}
{#if details?.push_enabled}
	<PushNotificationsCard />
{:else if details}
	<div role="alert" class="alert alert-warning">
		<span>{t('notifications-unavailable')}</span>
		{#if details.account.is_admin}
			<a class="link" href="/admin/settings">{t('notifications-admin-settings-link')}</a>
		{/if}
	</div>
{:else if !error}
	<div role="status" class="flex items-center gap-2 text-sm text-base-content/60">
		<span class="loading loading-spinner loading-sm" aria-hidden="true"></span>
		<span>{t('notifications-loading')}</span>
	</div>
{/if}
