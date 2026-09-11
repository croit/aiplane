<script lang="ts">
	import { connectorForm, connectorUsesOAuth, type AdminConnector, type ConnectorFormValue } from '$lib/admin-connectors';
	import ConnectorOAuthHelp from './ConnectorOAuthHelp.svelte';
	import { t } from '$lib/i18n.svelte';

	let { connector, redirectUri, groups, onsave } = $props<{
		connector?: AdminConnector;
		redirectUri: string;
		groups: string[];
		onsave: (value: ConnectorFormValue) => void | Promise<void>;
	}>();
	let form = $state<ConnectorFormValue>(connectorForm());
	let saving = $state(false);
	const clientJsonPlaceholder = '{"web":{"client_id":"…","client_secret":"…"}}';

	$effect(() => {
		form = connectorForm(connector);
	});

	async function save(event: SubmitEvent) {
		event.preventDefault();
		saving = true;
		try { await onsave(form); } finally { saving = false; }
	}
</script>

<form class="flex flex-col gap-2" onsubmit={save}>
	<div class="grid grid-cols-1 gap-2 sm:grid-cols-2">
		<label class="flex w-full flex-col gap-1"><span class="text-xs">{connector ? t('connectors-field-key-readonly-label') : t('connectors-field-key-label')}</span><input class="input input-bordered input-sm w-full" bind:value={form.key} readonly={!!connector} placeholder={t('connectors-field-key-placeholder')} required /></label>
		<label class="flex w-full flex-col gap-1"><span class="text-xs">{t('connectors-field-name-label')}</span><input class="input input-bordered input-sm w-full" bind:value={form.title} placeholder={t('connectors-field-name-placeholder')} required /></label>
		<label class="flex w-full flex-col gap-1"><span class="text-xs">{t('connectors-field-icon-label')}</span><input class="input input-bordered input-sm w-full" bind:value={form.icon} placeholder="📧" /></label>
		<label class="flex w-full flex-col gap-1"><span class="text-xs">{t('connectors-field-category-label')}</span><input class="input input-bordered input-sm w-full" bind:value={form.category} placeholder={t('connectors-field-category-placeholder')} /></label>
	</div>
	<label class="flex w-full flex-col gap-1"><span class="text-xs">{t('connectors-field-description-label')}</span><input class="input input-bordered input-sm w-full" bind:value={form.description} placeholder={t('connectors-field-description-placeholder')} /></label>
	<label class="flex w-full flex-col gap-1"><span class="text-xs">{t('connectors-field-url-label')}</span><input class="input input-bordered input-sm w-full" bind:value={form.base_url} placeholder={t('connectors-field-url-placeholder')} required /></label>
	<label class="flex w-full flex-col gap-1"><span class="text-xs">{t('connectors-field-scope-label')}</span><select class="select select-bordered select-sm w-full" bind:value={form.scope} aria-label={t('connectors-field-scope-label')}><option value="per_user">{t('connectors-scope-per-user')}</option><option value="global">{t('connectors-scope-global')}</option></select><span class="text-xs text-base-content/50">{t('connectors-field-scope-help')}</span></label>
	<label class="flex w-full flex-col gap-1"><span class="text-xs">{t('connectors-field-auth-label')}</span><select class="select select-bordered select-sm w-full" bind:value={form.auth_type} aria-label={t('connectors-field-auth-label')}><option value="oauth2">{t('connectors-auth-option-oauth')}</option><option value="static_bearer">{t('connectors-auth-option-token')}</option><option value="none">{t('connectors-auth-option-none')}</option></select></label>

	{#if connectorUsesOAuth(form.auth_type)}
		<ConnectorOAuthHelp {form} {redirectUri} />
		<label class="flex w-full flex-col gap-1"><span class="text-xs">{t('connectors-field-client-json-label')}</span><textarea class="textarea textarea-bordered textarea-sm w-full font-mono text-xs" rows="3" bind:value={form.client_json} placeholder={clientJsonPlaceholder}></textarea><span class="text-xs text-base-content/50">{t('connectors-field-client-json-help')}</span></label>
		<div class="grid grid-cols-1 gap-2 sm:grid-cols-2">
			<label class="flex w-full flex-col gap-1"><span class="text-xs">{t('connectors-field-client-id-label')}</span><input class="input input-bordered input-sm w-full" bind:value={form.client_id} placeholder={t('connectors-field-client-id-placeholder')} /><span class="text-xs text-base-content/50">{t('connectors-field-client-id-help')}</span></label>
			<label class="flex w-full flex-col gap-1"><span class="text-xs">{t('connectors-field-client-secret-label')}</span><input class="input input-bordered input-sm w-full" type="password" bind:value={form.client_secret} placeholder={connector?.has_secret ? t('connectors-secret-placeholder-existing') : t('connectors-secret-placeholder-new')} /><span class="text-xs text-base-content/50">{t('connectors-field-client-secret-help')}</span></label>
		</div>
		<label class="flex min-h-11 cursor-pointer items-center gap-2"><input type="checkbox" class="checkbox checkbox-sm" bind:checked={form.use_dcr} /><span class="text-xs">{t('connectors-field-use-dcr-label')}</span></label>
		<label class="flex w-full flex-col gap-1"><span class="text-xs">{t('connectors-field-scopes-label')}</span><input class="input input-bordered input-sm w-full" bind:value={form.scopes} placeholder={t('connectors-field-scopes-placeholder')} /></label>
		<details><summary class="cursor-pointer text-xs text-base-content/60">{t('connectors-advanced-summary')}</summary><div class="mt-2 grid grid-cols-1 gap-2">
			<label class="flex flex-col gap-1"><span class="text-xs">{t('connectors-field-authorize-url-label')}</span><input class="input input-bordered input-sm" bind:value={form.authorize_url} placeholder={t('connectors-placeholder-optional-override')} /></label>
			<label class="flex flex-col gap-1"><span class="text-xs">{t('connectors-field-token-url-label')}</span><input class="input input-bordered input-sm" bind:value={form.token_url} placeholder={t('connectors-placeholder-optional-override')} /></label>
			<label class="flex flex-col gap-1"><span class="text-xs">{t('connectors-field-registration-url-label')}</span><input class="input input-bordered input-sm" bind:value={form.registration_url} placeholder={t('connectors-placeholder-optional-override')} /></label>
		</div></details>
	{:else if form.auth_type === 'static_bearer'}
		<div class="rounded-md border border-info/30 bg-info/5 p-3 text-xs">{t(form.scope === 'global' ? 'connectors-token-help-global' : 'connectors-token-help-user')}</div>
		{#if form.scope === 'global'}<label class="flex w-full flex-col gap-1"><span class="text-xs">{t('connectors-field-shared-token-label')}</span><input class="input input-bordered input-sm w-full" type="password" bind:value={form.client_secret} placeholder={connector?.has_secret ? t('connectors-secret-placeholder-existing') : t('connectors-secret-placeholder-new')} /><span class="text-xs text-base-content/50">{t('connectors-field-shared-token-help')}</span></label>{/if}
	{:else}
		<div class="rounded-md border border-info/30 bg-info/5 p-3 text-xs">{t(form.scope === 'global' ? 'connectors-none-help-global' : 'connectors-none-help-user')}</div>
	{/if}

	<label class="flex w-full flex-col gap-1"><span class="text-xs">{t('connectors-field-allowed-groups-label')}</span><input class="input input-bordered input-sm w-full" bind:value={form.groups} list={`connector-groups-${form.key || 'new'}`} placeholder={t('connectors-placeholder-optional')} /><datalist id={`connector-groups-${form.key || 'new'}`}>{#each groups as group}<option value={group}></option>{/each}</datalist></label>
	<label class="flex min-h-11 cursor-pointer items-center gap-2"><input type="checkbox" class="checkbox checkbox-sm" bind:checked={form.audit} /><span class="text-xs">{t('connectors-field-audit-label')}</span></label>
	<div><button type="submit" class="btn btn-primary btn-sm" disabled={saving}>{connector ? t('connectors-save-changes-button') : t('connectors-add-connector-button')}</button></div>
</form>
