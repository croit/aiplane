<script lang="ts">
	import { t } from '#lib/i18n.svelte.js';
	let { connectorKey, authType, connected, needsSetup, disabled = false, canDisconnect = connected, onconnect, ondisconnect, onretry }: {
		connectorKey: string;
		authType: 'oauth2' | 'none' | 'static_bearer';
		connected: boolean;
		needsSetup: boolean;
		disabled?: boolean;
		canDisconnect?: boolean;
		onconnect?: () => void | Promise<void>;
		ondisconnect: () => void | Promise<void>;
		onretry?: () => void | Promise<void>;
	} = $props();
</script>
<div class="flex flex-wrap items-center gap-2">
	{#if authType === 'static_bearer'}
		{#if connected}<button type="button" class="btn btn-ghost btn-sm" disabled={disabled} title={t('integrations-reconnect-title')} onclick={onretry}>{t('integrations-reconnect-button')}</button>{/if}
	{:else if onconnect}
		<button type="button" class:btn-primary={!connected} class="btn btn-sm" disabled={disabled || needsSetup} onclick={onconnect}>{connected ? t('integrations-reconnect-button') : t('integrations-connect-button')}</button>
	{:else}
		<form method="post" action={`/integrations/${encodeURIComponent(connectorKey)}/connect`} class="m-0"><button type="submit" class="btn btn-sm" class:btn-ghost={connected} class:btn-primary={!connected} title={connected ? t('integrations-reconnect-title') : undefined} disabled={disabled || (!connected && needsSetup)}>{connected ? t('integrations-reconnect-button') : t('integrations-connect-button')}</button></form>
	{/if}
	{#if canDisconnect}<button type="button" class="btn btn-ghost btn-sm text-error" disabled={disabled} onclick={ondisconnect}>{t('integrations-disconnect-button')}</button>{/if}
</div>
