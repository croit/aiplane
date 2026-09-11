<script lang="ts">
	import { t } from '$lib/i18n.svelte';
	import type { IntegrationConnector, IntegrationMode } from '$lib/integrations';
	import ConnectorLogo from './ConnectorLogo.svelte';
	import IntegrationTools from './IntegrationTools.svelte';

	let { connector, ontoken, ondisconnect, onretry, onmode, onall } = $props<{
		connector: IntegrationConnector;
		ontoken: (token: string) => void | Promise<void>;
		ondisconnect: () => void | Promise<void>;
		onretry: () => void | Promise<void>;
		onmode: (tool: string, mode: IntegrationMode) => void | Promise<void>;
		onall: (mode: IntegrationMode) => void | Promise<void>;
	}>();
	let token = $state('');
	let saving = $state(false);

	async function connectToken(event: SubmitEvent) {
		event.preventDefault();
		if (!token.trim()) return;
		saving = true;
		try { await ontoken(token); token = ''; } finally { saving = false; }
	}
</script>

<section class="card border border-base-300">
	<div class="card-body gap-3">
		<div class="flex flex-wrap items-start gap-3">
			<span class="mt-0.5 shrink-0"><ConnectorLogo connectorKey={connector.key} icon={connector.icon} /></span>
			<div class="min-w-0 flex-1">
				<div class="flex flex-wrap items-center gap-2">
					<h2 class="card-title m-0 text-base">{connector.title}</h2>
					{#if connector.is_global}<span class="badge badge-info badge-sm">{t('integrations-badge-global')}</span>
					{:else if connector.connected && !connector.errored}<span class="badge badge-success badge-sm">{t('integrations-badge-connected')}</span>{/if}
					{#if !connector.is_global && connector.errored}<span class="badge badge-error badge-sm">{t('integrations-badge-needs-reconnect')}</span>{/if}
					{#if !connector.is_global && !connector.connected && connector.needs_setup}<span class="badge badge-ghost badge-sm">{t('integrations-badge-needs-admin-setup')}</span>{/if}
				</div>
				{#if connector.description}<p class="m-0 mt-1 text-sm text-base-content/60">{connector.description}</p>{/if}
			</div>
			<div class="ml-9 w-full shrink-0 sm:ml-0 sm:w-auto">
				{#if connector.is_global}
					<span class="text-xs text-base-content/50">{t('integrations-no-sign-in')}</span>
				{:else if connector.connected}
					<div class="flex items-center gap-2">
						{#if connector.auth_type === 'static_bearer'}
							<button type="button" class="btn btn-ghost btn-sm" title={t('integrations-reconnect-title')} onclick={onretry}>{t('integrations-reconnect-button')}</button>
						{:else}
							<form method="post" action={`/integrations/${encodeURIComponent(connector.key)}/connect`} class="m-0"><button type="submit" class="btn btn-ghost btn-sm" title={t('integrations-reconnect-title')}>{t('integrations-reconnect-button')}</button></form>
						{/if}
						<button type="button" class="btn btn-ghost btn-sm text-error" onclick={ondisconnect}>{t('integrations-disconnect-button')}</button>
					</div>
				{:else if connector.auth_type !== 'static_bearer'}
					<form method="post" action={`/integrations/${encodeURIComponent(connector.key)}/connect`} class="m-0"><button type="submit" class="btn btn-primary btn-sm" disabled={connector.needs_setup}>{t('integrations-connect-button')}</button></form>
				{/if}
			</div>
		</div>

		{#if connector.auth_type === 'static_bearer' && !connector.connected && !connector.is_global}
			<form class="flex flex-wrap items-end gap-2 border-t border-base-300 pt-3" onsubmit={connectToken}>
				<label class="flex min-w-48 flex-1 flex-col gap-1"><span class="label-text text-xs">{t('integrations-token-label')}</span><input class="input input-bordered input-sm w-full" type="password" required autocomplete="off" placeholder={t('integrations-token-placeholder')} bind:value={token} /></label>
				<button class="btn btn-primary btn-sm" type="submit" disabled={saving}>{t('integrations-connect-button')}</button>
			</form>
		{/if}

		{#if connector.is_global || connector.connected}
			<IntegrationTools {connector} {onmode} {onall} />
		{/if}
	</div>
</section>
