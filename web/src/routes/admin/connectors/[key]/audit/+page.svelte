<script lang="ts">
	import { page } from '$app/state';
	import { onMount } from 'svelte';
	import { adminJson } from '$lib/admin-client';
	import { dt, t } from '$lib/i18n.svelte';
	import { clearPageTitleOverride, setPageTitleOverride } from '$lib/page-title';

	interface AuditEvent { id: string; user_id: string; user_email: string; tool_id: string; arguments: string | null; outcome: string; error: string | null; session_id: string | null; created_at: string; }
	interface AuditData { connector: { key: string; title: string }; events: AuditEvent[]; }
	let data = $state<AuditData | null>(null);
	let error = $state<string | null>(null);

	onMount(async () => {
		try { data = await adminJson<AuditData>(`/api/v0/admin/connectors/${encodeURIComponent(page.params.key ?? '')}/audit`); }
		catch (caught) { error = String(caught); }
	});

	$effect(() => {
		if (!data) return;
		const pathname = page.url.pathname;
		setPageTitleOverride(pathname, t('connectors-audit-page-title', { name: data.connector.title }));
		return () => clearPageTitleOverride(pathname);
	});
</script>

<div class="w-full">
	<a href="/admin/connectors" class="text-sm text-base-content/60 hover:underline">{t('connectors-audit-back')}</a>
	{#if error}<div class="alert alert-error mt-4"><span>{error}</span></div>{/if}
	{#if data}
		<div class="mb-1 mt-2 flex flex-wrap items-center gap-2"><h1 class="m-0 text-2xl font-bold">{data.connector.title}</h1><span class="badge badge-warning badge-sm">{t('connectors-badge-audited')}</span></div>
		<p class="mb-6 text-sm text-base-content/60">{t('connectors-audit-intro', { key: data.connector.key })}</p>
		{#if data.events.length === 0}<p class="m-0 text-sm text-base-content/50">{t('connectors-audit-empty')}</p>{:else}<div class="overflow-x-auto"><table class="table table-sm"><thead><tr><th>{t('connectors-audit-when')}</th><th>{t('connectors-audit-user')}</th><th>{t('connectors-audit-tool')}</th><th>{t('connectors-audit-outcome')}</th><th>{t('connectors-audit-detail')}</th></tr></thead><tbody>{#each data.events as event}<tr><td class="whitespace-nowrap text-xs text-base-content/60">{dt(event.created_at)}</td><td class="text-xs">{event.user_email || event.user_id}</td><td class="text-xs"><code>{event.tool_id}</code></td><td><span class="badge badge-xs {event.outcome === 'ok' ? 'badge-success' : 'badge-error'}">{event.outcome}</span></td><td class="max-w-md truncate text-xs text-base-content/50">{event.error || event.arguments || ''}</td></tr>{/each}</tbody></table></div>{/if}
	{:else if !error}<div class="skeleton mt-4 h-40 w-full"></div>{/if}
</div>
