<script lang="ts">
	import { connectorOAuthHelpProvider, type ConnectorFormValue } from '$lib/admin-connectors';
	import { t } from '$lib/i18n.svelte';

	let { form, redirectUri } = $props<{ form: ConnectorFormValue; redirectUri: string }>();
	let provider = $derived(connectorOAuthHelpProvider(form));
</script>

<div class="rounded-md border border-info/30 bg-info/5 p-3 text-xs leading-relaxed">
	<p class="m-0 font-medium">{t(form.use_dcr ? 'connectors-oauth-help-dcr-heading' : 'connectors-oauth-help-generic-heading')}</p>
	<p class="mb-0 mt-1">{t(form.use_dcr ? 'connectors-oauth-help-dcr-body' : 'connectors-oauth-help-generic-intro')}</p>
	{#if provider === 'google_workspace'}
		<p class="mb-0 mt-2">
			{t('connectors-oauth-help-gws-1')} <strong>{t('connectors-oauth-help-gws-self-hosted')}</strong>
			{t('connectors-oauth-help-gws-2')} <a class="link" target="_blank" rel="noopener noreferrer" href="https://github.com/taylorwilsdon/google_workspace_mcp">taylorwilsdon/google_workspace_mcp</a>
			{t('connectors-oauth-help-gws-3')} <code>/mcp/</code>{t('connectors-oauth-help-gws-4')}
			<strong>{t('connectors-oauth-help-gws-ga-apis')}</strong> {t('connectors-oauth-help-gws-5')}
			<code>WORKSPACE_MCP_ALLOWED_CLIENT_REDIRECT_URIS</code>:
		</p>
		<code class="mt-1 block select-all break-all rounded bg-base-300/60 p-1.5">{redirectUri}</code>
		<p class="mb-0 mt-2 text-base-content/60">{t('connectors-oauth-help-gws-footer')}</p>
	{:else if provider !== 'dcr'}
		<code class="mb-2 mt-1 block select-all break-all rounded bg-base-300/60 p-1.5">{redirectUri}</code>
		<p class="m-0">
			{#if provider === 'google'}
				{t('connectors-oauth-help-google-1')} <a class="link" target="_blank" rel="noopener noreferrer" href="https://console.cloud.google.com/apis/credentials">{t('connectors-oauth-help-google-link')}</a> {t('connectors-oauth-help-google-2')}
			{:else if provider === 'github'}
				{t('connectors-oauth-help-github-1')} <a class="link" target="_blank" rel="noopener noreferrer" href="https://github.com/settings/developers">{t('connectors-oauth-help-github-link')}</a> {t('connectors-oauth-help-github-2')}
			{:else if provider === 'slack'}
				{t('connectors-oauth-help-slack-1')} <a class="link" target="_blank" rel="noopener noreferrer" href="https://api.slack.com/apps">api.slack.com/apps</a>{t('connectors-oauth-help-slack-2')}
			{:else}
				{t('connectors-oauth-help-fallback')}
			{/if}
		</p>
		<p class="mb-0 mt-2 text-base-content/60">{t('connectors-oauth-why-1')} <strong>{t('connectors-term-this-gateway')}</strong> {t('connectors-oauth-why-2')} <strong>{t('connectors-oauth-why-no-app')}</strong> {t('connectors-oauth-why-3')}</p>
	{/if}
</div>
