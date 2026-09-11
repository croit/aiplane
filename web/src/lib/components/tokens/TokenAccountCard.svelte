<script lang="ts">
	import { t } from '$lib/i18n.svelte';
	import type { TokenManagementDetails } from '$lib/tokens';
	let { account }: { account: TokenManagementDetails['account'] } = $props();
	let oidc = $derived(account.oidc_roles.length ? account.oidc_roles.join(', ') : t('tokens-roles-none'));
	let rbac = $derived(account.rbac_roles.length ? account.rbac_roles.join(', ') : t('tokens-roles-none-granted'));
</script>

<section class="card mt-6 border border-base-300">
	<div class="card-body">
		<h2 class="card-title text-base">{t('tokens-account-heading')}</h2>
		<p class="text-sm text-base-content/60">{t('tokens-signed-in-as', { email: account.email })}</p>
		<dl class="mt-2 grid grid-cols-[8rem_minmax(0,1fr)] gap-x-4 gap-y-2 text-sm">
			<dt class="text-base-content/60">{t('tokens-account-user-id-label')}</dt><dd class="min-w-0 break-all font-mono text-xs">{account.user_id}</dd>
			<dt class="text-base-content/60">{t('tokens-account-oidc-label')}</dt><dd class="min-w-0 break-words">{oidc}</dd>
			<dt class="text-base-content/60">{t('tokens-account-rbac-label')}</dt><dd class="min-w-0 break-words">{rbac}</dd>
		</dl>
	</div>
</section>
