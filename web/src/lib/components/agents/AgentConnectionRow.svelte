<script lang="ts">
	import { agentConnectionsApi, type OwnedAgent, type AgentConnection, type AgentConnectionMode, type AgentConnections } from '#lib/owned-agents.js';
	import { connectionChange, connectionModes, selectedConnectionAvailable } from '#lib/agent-connections.js';
	import { ApiError } from '#lib/api.js';
	import { t } from '#lib/i18n.svelte.js';
	import ConnectorLogo from '../integrations/ConnectorLogo.svelte';
	import IntegrationCredentialActions from '../integrations/IntegrationCredentialActions.svelte';
	let { agent, row, oninventory, onchange }: { agent: OwnedAgent; row: AgentConnection; oninventory: (data: AgentConnections) => void; onchange: () => void } = $props();
	let draft = $state<{ source: AgentConnection; mode: AgentConnectionMode } | null>(null);
	let busy = $state(false);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	const mode = $derived(draft?.mode ?? row.selected.mode);
	const change = $derived(row.can_select ? connectionChange(draft?.source ?? row, mode) : null);
	const credential = $derived(mode === 'user' ? row.user_connection : row.principal_connection);
	const version = () => ({ connection_id: row.principal_connection?.id ?? null, connection_generation: row.principal_connection?.generation ?? null });
	const message = (caught: unknown, fallback: string) => caught instanceof ApiError ? caught.serverMessage ?? caught.message : caught instanceof Error ? caught.message : fallback;

	async function save() {
		if (!change || !row.can_select) return;
		busy = true; error = null; notice = null;
		try {
			oninventory(await agentConnectionsApi.save(agent.id, row.key, change));
			draft = null; notice = t('agent-connection-saved'); onchange();
		} catch (caught) { error = message(caught, t('agent-connection-error-save')); }
		finally { busy = false; }
	}
	async function connect() {
		if (!row.can_connect) return;
		busy = true; error = null; notice = null;
		try { const result = await agentConnectionsApi.connect(agent.id, row.key, version()); window.location.assign(result.authorization_url); }
		catch (caught) { error = message(caught, t('agent-connection-error-connect')); }
		finally { busy = false; }
	}
	async function disconnect() {
		if (!row.can_disconnect || !window.confirm(t('agent-connection-disconnect-confirm', { name: agent.display || agent.name, connector: row.connector?.title ?? row.key }))) return;
		busy = true; error = null; notice = null;
		try { oninventory(await agentConnectionsApi.disconnect(agent.id, row.key, version())); notice = t('agent-connection-disconnected'); onchange(); }
		catch (caught) { error = message(caught, t('agent-connection-error-disconnect')); }
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
		<label class="flex flex-col gap-1"><span class="text-xs">{t('agent-connection-heading')}</span>
			<select class="select select-sm w-full" value={mode} disabled={!row.can_select || busy || row.supported_modes.length === 0} onchange={(event) => { draft = { source: draft?.source ?? row, mode: event.currentTarget.value as AgentConnectionMode }; notice = null; }}>
				{#each connectionModes(row) as option}<option value={option}>{option === 'user' ? t('agent-connection-user') : t('agent-connection-principal', { name: agent.display || agent.name })}</option>{/each}
			</select>
		</label>
		{#if !row.connector && row.principal_connection}<p class="text-xs text-warning">{t('agent-connection-local-unavailable')}</p>
		{:else if mode === row.selected.mode && !selectedConnectionAvailable(row)}<p class="text-xs text-warning">{t('agent-connection-unavailable')}</p>{/if}
		{#if row.connector && credential?.authorizing}<span class="badge badge-outline badge-sm">{t('agent-connection-authorizing')}</span>
		{:else if row.connector && credential?.connected && !credential.errored && !credential.needs_reauth && (mode !== row.selected.mode || selectedConnectionAvailable(row))}<span class="badge badge-success badge-sm">{t('integrations-badge-connected')}</span>
		{:else if row.connector && mode !== row.selected.mode}<p class="text-xs text-warning">{t('agent-connection-unavailable')}</p>{/if}
		{#if row.connector && credential?.account_label}<p class="break-words text-xs">{credential.account_label}</p>{/if}
		<p class="text-xs text-base-content/60">{t('agent-connection-pending-warning')}</p>
		{#if row.principal_connection || (row.connector?.scope === 'per_user' && row.connector.auth_type === 'oauth2' && row.supported_modes.includes('principal'))}
			<p class="text-xs font-medium">{t('agent-connection-principal', { name: agent.display || agent.name })}</p>
			{#if row.connector}<p class="text-xs text-base-content/60">{t('agent-connection-provider-account')}</p>{/if}
			<IntegrationCredentialActions connectorKey={row.key} authType={row.connector?.auth_type ?? null} connected={row.principal_connection?.connected ?? false} needsSetup={row.connector?.needs_setup ?? false} disabled={busy} canConnect={row.can_connect && row.connector?.scope === 'per_user' && row.connector.auth_type === 'oauth2' && row.supported_modes.includes('principal')} canDisconnect={row.can_disconnect} onconnect={connect} ondisconnect={disconnect} />
		{/if}
		{#if error}<div class="alert alert-error py-2" role="alert"><span class="break-words">{error}</span></div>{/if}
		{#if notice}<div class="alert alert-success py-2" role="status"><span>{notice}</span></div>{/if}
		<div class="flex justify-end"><button class="btn btn-sm" type="button" disabled={!row.can_select || busy || !change} onclick={save}>{busy ? t('agent-access-saving') : t('memory-save-button')}</button></div>
	</div>
</section>
