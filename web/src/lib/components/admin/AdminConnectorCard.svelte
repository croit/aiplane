<script lang="ts">
	import { connectorBadges, type AdminConnector, type ConnectorFormValue } from '$lib/admin-connectors';
	import AdminConnectorForm from './AdminConnectorForm.svelte';
	import { t } from '$lib/i18n.svelte';

	let { connector, redirectUri, groups, onsave, ontoggle, ondelete } = $props<{
		connector: AdminConnector;
		redirectUri: string;
		groups: string[];
		onsave: (value: ConnectorFormValue) => void | Promise<void>;
		ontoggle: () => void | Promise<void>;
		ondelete: () => void | Promise<void>;
	}>();

	const badgeClass: Record<string, string> = { enabled: 'badge-success', disabled: 'badge-ghost', global: 'badge-info', audited: 'badge-warning', default: 'badge-outline', dcr: 'badge-outline', needs_setup: 'badge-warning' };
</script>

<article class="card border border-base-300" data-testid={`connector-${connector.key}`}>
	<div class="card-body gap-2">
		<div class="flex flex-wrap items-center gap-3">
			<span class="shrink-0 text-xl leading-none">{connector.icon ?? '🔌'}</span>
			<div class="min-w-0 flex-1">
				<div class="flex flex-wrap items-center gap-2"><h2 class="card-title m-0 text-base">{connector.title}</h2><code class="text-xs text-base-content/50">{connector.key}</code>{#each connectorBadges(connector) as badge}<span class="badge badge-sm {badgeClass[badge]}">{t(`connectors-badge-${badge.replace('_', '-')}`)}</span>{/each}</div>
				<p class="m-0 mt-0.5 break-all text-xs text-base-content/50">{connector.base_url}</p>
			</div>
			<div class="flex shrink-0 flex-wrap items-center gap-2">
				<button type="button" class="btn btn-ghost btn-xs {connector.enabled ? '' : 'btn-primary'}" disabled={!connector.enabled && connector.needs_setup} title={!connector.enabled && connector.needs_setup ? t('connectors-enable-disabled-title') : undefined} onclick={ontoggle}>{connector.enabled ? t('connectors-disable-button') : t('connectors-enable-button')}</button>
				{#if connector.audit}<a href={`/admin/connectors/${encodeURIComponent(connector.key)}/audit`} class="btn btn-ghost btn-xs">{t('connectors-audit-log-button')}</a>{/if}
				<button type="button" class="btn btn-ghost btn-xs text-error" onclick={ondelete}>{t('connectors-delete-button')}</button>
			</div>
		</div>
		<details><summary class="cursor-pointer text-sm text-base-content/70">{t('connectors-edit-summary')}</summary><div class="mt-2"><AdminConnectorForm {connector} {redirectUri} {groups} {onsave} /></div></details>
	</div>
</article>
