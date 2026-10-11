<script lang="ts">
    import { onMount } from 'svelte';
    import OrganizationForm from '#lib/components/organizations/OrganizationForm.svelte';
    import Modal from '#lib/components/ui/Modal.svelte';
    import { adminOrganizationsApi, type AdminOrganization, type OrganizationBody, type OrganizationEdit } from '#lib/organizations.js';
    import { ApiError } from '#lib/api.js';
    import { t } from '#lib/i18n.svelte.js';
    let data = $state<{ organizations: AdminOrganization[]; groups: string[] } | null>(null);
    let error = $state<string | null>(null);
    let saved = $state(false);
    let creating = $state(false);
    async function reload() {
        try { data = await adminOrganizationsApi.list(); error = null; }
        catch (caught) { error = caught instanceof ApiError ? caught.serverMessage ?? caught.message : String(caught); }
    }
    async function save(row: AdminOrganization | null, body: OrganizationBody | OrganizationEdit) {
        if (row) await adminOrganizationsApi.save(row.id, body as OrganizationEdit);
        else { await adminOrganizationsApi.create(body); creating = false; }
        saved = true; await reload();
    }
    onMount(() => void reload());
</script>
<div class="flex min-w-0 flex-col gap-4">
    <header class="flex flex-wrap items-center gap-3"><h2 class="text-xl font-semibold">{t('organizations-heading')}</h2><div class="ml-auto flex flex-wrap gap-2"><button class="btn btn-sm" onclick={() => void reload()}>{t('organization-reload')}</button><button class="btn btn-primary btn-sm" disabled={!data} onclick={() => (creating = true)}>{t('organization-create')}</button></div></header>
    {#if error}<div class="alert alert-error" role="alert"><span>{error}</span></div>{/if}
    {#if saved}<div class="alert alert-success" role="status"><span>{t('organization-saved')}</span></div>{/if}
    {#if data}
        {#each data.organizations as row (row.id)}
            <section class="card card-border bg-base-100"><div class="card-body"><h3 class="card-title break-words">{row.name}</h3>
                {#if !row.member_groups.length}<p>{t('organization-no-members')}</p>{/if}
                {#key row.revision}<OrganizationForm {row} groups={data.groups} onsave={(body) => save(row, body)} />{/key}
            </div></section>
        {/each}
    {:else if !error}<div class="skeleton h-64 w-full"></div>{/if}
</div>
<Modal bind:open={creating} title={t('organization-create')}>
    {#if data}<OrganizationForm groups={data.groups} onsave={(body) => save(null, body)} />{/if}
</Modal>
