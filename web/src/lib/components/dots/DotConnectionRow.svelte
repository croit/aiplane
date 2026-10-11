<script lang="ts">
	import { dotConnectionsApi, type Dot, type DotConnection, type DotConnectionMode, type DotConnections } from '#lib/dots.js';
	import { connectionChange, connectionModes, selectedConnectionAvailable } from '#lib/dot-connections.js';
	import { ApiError } from '#lib/api.js';
	import { me } from '#lib/session.svelte.js';
	import { t } from '#lib/i18n.svelte.js';
	import ConnectorLogo from '../integrations/ConnectorLogo.svelte';
	import IntegrationCredentialActions from '../integrations/IntegrationCredentialActions.svelte';
	let { dot, row, oninventory, onchange }: { dot: Dot; row: DotConnection; oninventory: (data: DotConnections) => void; onchange: () => void } = $props();
	let draft = $state<{ source: DotConnection; mode: DotConnectionMode } | null>(null);
	let busy = $state(false);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	const mode = $derived(draft?.mode ?? row.selected.mode);
	const change = $derived(connectionChange(draft?.source ?? row, mode));
	const canManage = $derived(me.value?.can_manage_dots === true);
	const credential = $derived(mode === 'user' ? row.user_connection : row.principal_connection);
	const version = () => ({ connection_id: row.principal_connection?.id ?? null, connection_generation: row.principal_connection?.generation ?? null });
	const message = (caught: unknown, fallback: string) => caught instanceof ApiError ? caught.serverMessage ?? caught.message : caught instanceof Error ? caught.message : fallback;

	async function save() {
		if (!change || !canManage) return;
		busy = true; error = null; notice = null;
		try {
			oninventory(await dotConnectionsApi.save(dot.id, row.key, change));
			draft = null; notice = t('dot-connection-saved'); onchange();
		} catch (caught) { error = message(caught, t('dot-connection-error-save')); }
		finally { busy = false; }
	}
	async function connect() {
		if (!canManage || !row.can_connect) return;
		busy = true; error = null; notice = null;
		try { const result = await dotConnectionsApi.connect(dot.id, row.key, version()); window.location.assign(result.authorization_url); }
		catch (caught) { error = message(caught, t('dot-connection-error-connect')); }
		finally { busy = false; }
	}
	async function disconnect() {
		if (!canManage || !row.can_disconnect || !window.confirm(t('dot-connection-disconnect-confirm', { name: dot.display || dot.name, connector: row.connector?.title ?? row.key }))) return;
		busy = true; error = null; notice = null;
		try { oninventory(await dotConnectionsApi.disconnect(dot.id, row.key, version())); notice = t('dot-connection-disconnected'); onchange(); }
		catch (caught) { error = message(caught, t('dot-connection-error-disconnect')); }
		finally { busy = false; }
	}
</script>
<section class="card border border-base-300" aria-label={row.connector?.title ?? row.key}>
	<div class="card-body min-w-0 gap-3 p-3">
		<div class="flex items-start gap-2">
			{#if row.connector}<ConnectorLogo connectorKey={row.key} icon={row.connector.icon} />{/if}
			<div class="min-w-0"><h3 class="card-title break-words text-sm">{row.connector?.title ?? row.key}</h3><code class="block break-all text-xs text-base-content/50">{row.key}</code></div>
		</div>
		{#if row.connector?.description}<p class="break-words text-xs text-base-content/60">{row.connector.description}</p>{/if}
		<label class="flex flex-col gap-1"><span class="text-xs">{t('dot-connection-heading')}</span>
			<select class="select select-sm w-full" value={mode} disabled={!canManage || busy || row.supported_modes.length === 0} onchange={(event) => { draft = { source: draft?.source ?? row, mode: event.currentTarget.value as DotConnectionMode }; notice = null; }}>
				{#each connectionModes(row) as option}<option value={option}>{option === 'user' ? t('dot-connection-user') : t('dot-connection-principal', { name: dot.display || dot.name })}</option>{/each}
			</select>
		</label>
		{#if !row.connector && row.principal_connection}<p class="text-xs text-warning">{t('dot-connection-local-unavailable')}</p>
		{:else if mode === row.selected.mode && !selectedConnectionAvailable(row)}<p class="text-xs text-warning">{t('dot-connection-unavailable')}</p>{/if}
		{#if row.connector && credential?.authorizing}<span class="badge badge-outline badge-sm">{t('dot-connection-authorizing')}</span>
		{:else if row.connector && credential?.connected && !credential.errored && !credential.needs_reauth && (mode !== row.selected.mode || selectedConnectionAvailable(row))}<span class="badge badge-success badge-sm">{t('integrations-badge-connected')}</span>
		{:else if row.connector && mode !== row.selected.mode}<p class="text-xs text-warning">{t('dot-connection-unavailable')}</p>{/if}
		{#if row.connector && credential?.account_label}<p class="break-words text-xs">{credential.account_label}</p>{/if}
		<p class="text-xs text-base-content/60">{t('dot-connection-pending-warning')}</p>
		{#if row.principal_connection || (row.connector?.scope === 'per_user' && row.connector.auth_type === 'oauth2' && row.supported_modes.includes('principal'))}
			<p class="text-xs font-medium">{t('dot-connection-principal', { name: dot.display || dot.name })}</p>
			{#if row.connector}<p class="text-xs text-base-content/60">{t('dot-connection-provider-account')}</p>{/if}
			<IntegrationCredentialActions connectorKey={row.key} authType={row.connector?.auth_type ?? null} connected={row.principal_connection?.connected ?? false} needsSetup={row.connector?.needs_setup ?? false} disabled={!canManage || busy} canConnect={row.can_connect && row.connector?.scope === 'per_user' && row.connector.auth_type === 'oauth2' && row.supported_modes.includes('principal')} canDisconnect={row.can_disconnect} onconnect={connect} ondisconnect={disconnect} />
		{/if}
		{#if error}<div class="alert alert-error py-2" role="alert"><span class="break-words">{error}</span></div>{/if}
		{#if notice}<div class="alert alert-success py-2" role="status"><span>{notice}</span></div>{/if}
		<div class="flex justify-end"><button class="btn btn-sm" type="button" disabled={!canManage || busy || !change} onclick={save}>{busy ? t('dot-access-saving') : t('memory-save-button')}</button></div>
	</div>
</section>
