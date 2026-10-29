<script lang="ts">
    import { goto } from '$app/navigation';
    import { page } from '$app/state';
    import { organizationsApi, type OrganizationDetail } from '#lib/organizations.js';
    import { agentPublicName } from '#lib/agents.js';
    import { canCreateAgent } from '#lib/agent-state.js';
    import { ApiError } from '#lib/api.js';
    import { t } from '#lib/i18n.svelte.js';
    import Modal from '#lib/components/ui/Modal.svelte';
    let data = $state<OrganizationDetail | null>(null);
    let error = $state<string | null>(null);
    let busy = $state(false);
    let creating = $state(false);
    let createError = $state<string | null>(null);
    let loading = $state(false);
    let requestId = 0;
    let name = $state(''); let display = $state(''); let description = $state('');
    const id = $derived(page.params.id ?? '');
    async function reload(target = id) {
        const request = ++requestId;
        loading = true;
        try {
            const result = await organizationsApi.detail(target);
            if (request === requestId) { data = result; error = null; }
        }
        catch (caught) {
            if (request === requestId) error = caught instanceof ApiError ? caught.serverMessage ?? caught.message : String(caught);
        }
        finally { if (request === requestId) loading = false; }
    }
    async function create() {
        if (!data?.can_create_agent || !name.trim() || !canCreateAgent(data.active_count, data.active_limit)) return;
        busy = true; createError = null;
        try {
            const result = await organizationsApi.createAgent(id, { name, display, description });
            creating = false;
            name = ''; display = ''; description = '';
            await goto(`/agents/${encodeURIComponent(result.agent.id)}/setup/start`);
        }
        catch (caught) { createError = caught instanceof ApiError ? caught.serverMessage ?? caught.message : String(caught); }
        finally { busy = false; }
    }
    async function activate(agentId: string, active: boolean) {
        busy = true; error = null;
        try { await organizationsApi.setActive(id, agentId, active); await reload(); }
        catch (caught) { error = caught instanceof ApiError ? caught.serverMessage ?? caught.message : String(caught); }
        finally { busy = false; }
    }
    $effect(() => {
        const target = id;
        data = null;
        creating = false; createError = null;
        name = ''; display = ''; description = '';
        void reload(target);
    });
</script>
<div class="flex min-w-0 flex-col gap-4">
    <a class="link link-hover w-fit" href="/organizations">{t('organizations-heading')}</a>
    {#if error}<div class="alert alert-error" role="alert"><span>{error}</span></div>{/if}
    {#if data}
        <header class="flex flex-wrap items-center gap-3"><h1 class="break-words text-2xl font-bold">{data.organization.name}</h1><div class="ml-auto flex flex-wrap gap-2"><button class="btn btn-sm" disabled={busy || loading} onclick={() => void reload()}>{t('organization-reload')}</button>{#if data.can_create_agent}<button class="btn btn-primary btn-sm" disabled={busy || loading || !canCreateAgent(data.active_count, data.active_limit)} onclick={() => { createError = null; creating = true; }}>{t('owned-agents-create')}</button>{/if}</div></header>
        {#if data.organization.description}<p class="whitespace-pre-wrap break-words">{data.organization.description}</p>{/if}
        <div class="alert alert-info"><span>{t('organization-configuration-only')}</span></div>
        <p>{t('organization-active-count', { count: data.active_count, limit: data.active_limit })}</p>
        {#each data.agents as owned_agent (owned_agent.id)}
            <section class="card card-border bg-base-100"><div class="card-body"><h2 class="card-title break-words">{owned_agent.display || agentPublicName(owned_agent)}</h2><p>{agentPublicName(owned_agent)}</p>
                {#if owned_agent.description}<p class="whitespace-pre-wrap break-words">{owned_agent.description}</p>{/if}
                {#if owned_agent.ownership}<span class="badge badge-outline">{t(owned_agent.ownership.active ? 'agents-status-active' : 'agents-status-inactive')}</span>{/if}
                <div class="card-actions"><a class="btn btn-sm" href="/agents/{encodeURIComponent(owned_agent.id)}?tab=setup">{t('agents-setup-edit')}</a>
                    {#if owned_agent.access === 'write' && owned_agent.ownership}<button class="btn btn-sm" disabled={busy || loading || (!owned_agent.ownership.active && !canCreateAgent(data.active_count, data.active_limit))} onclick={() => void activate(owned_agent.id, !owned_agent.ownership!.active)}>{t(owned_agent.ownership.active ? 'agents-action-deactivate' : 'agents-action-activate')}</button>{/if}
                </div>
            </div></section>
        {:else}<p>{t('organization-agents-empty')}</p>{/each}
    {:else if !error}<div class="skeleton h-64 w-full"></div>{:else}<button class="btn w-fit" disabled={loading} onclick={() => void reload()}>{t('organization-reload')}</button>{/if}
</div>
{#snippet createActions()}
    <button class="btn btn-ghost" type="button" disabled={busy} onclick={() => (creating = false)}>{t('agents-create-cancel')}</button>
    <button class="btn btn-primary" type="submit" form="organization-agent-create" disabled={busy || loading || !name.trim() || !data?.can_create_agent || !canCreateAgent(data.active_count, data.active_limit)}>{t('owned-agents-create-submit')}</button>
{/snippet}
<Modal bind:open={creating} title={t('owned-agents-create')} footer={createActions}>
    <form id="organization-agent-create" class="fieldset min-w-0 gap-3" onsubmit={(event) => { event.preventDefault(); void create(); }}>
        {#if createError}<div class="alert alert-error" role="alert"><span>{createError}</span></div>{/if}
        <fieldset class="fieldset min-w-0 gap-3" disabled={busy || loading || !data?.can_create_agent}>
            <label class="fieldset"><span class="label">{t('agents-create-name')}</span><input class="input w-full" bind:value={name} maxlength="80" required /></label>
            <label class="fieldset"><span class="label">{t('agents-create-display')}</span><input class="input w-full" bind:value={display} maxlength="80" /></label>
            <label class="fieldset"><span class="label">{t('owned-agents-create-description')}</span><textarea class="textarea w-full" bind:value={description} maxlength="16384" placeholder={t('agents-create-description-placeholder')}></textarea><span class="label text-xs text-base-content/60">{t('owned-agents-create-description-help')}</span></label>
        </fieldset>
    </form>
</Modal>
