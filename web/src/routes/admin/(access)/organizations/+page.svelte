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
    let editing = $state<AdminOrganization | null>(null);
    let editOpen = $state(false);
    async function reload() {
        try { data = await adminOrganizationsApi.list(); error = null; }
        catch (caught) { error = caught instanceof ApiError ? caught.serverMessage ?? caught.message : String(caught); }
    }
    async function save(row: AdminOrganization | null, body: OrganizationBody | OrganizationEdit) {
        if (row) await adminOrganizationsApi.save(row.id, body as OrganizationEdit);
        else { await adminOrganizationsApi.create(body); creating = false; }
        if (row) { editing = null; editOpen = false; }
        saved = true; await reload();
    }
    onMount(() => void reload());
</script>
<div class="flex min-w-0 flex-col gap-4">
    <header class="flex flex-wrap items-center gap-3"><h2 class="text-xl font-semibold">{t('organizations-heading')}</h2><div class="ml-auto flex flex-wrap gap-2"><button class="btn btn-sm" onclick={() => void reload()}>{t('organization-reload')}</button><button class="btn btn-primary btn-sm" disabled={!data} onclick={() => (creating = true)}>{t('organization-create')}</button></div></header>
    {#if error}<div class="alert alert-error" role="alert"><span>{error}</span></div>{/if}
    {#if saved}<div class="alert alert-success" role="status"><span>{t('organization-saved')}</span></div>{/if}
    {#if data}
        {#if data.organizations.length}
            <div class="grid min-w-0 gap-3 md:grid-cols-2">
                {#each data.organizations as row (row.id)}
                    <article class="card card-border bg-base-100">
                        <div class="card-body gap-3 p-4">
                            <div class="min-w-0">
                                <h3 class="card-title break-words">{row.name}</h3>
                                {#if row.description}<p class="mt-2 whitespace-pre-wrap break-words text-sm text-base-content/70">{row.description}</p>{/if}
                            </div>
                            <div class="space-y-2">
                                <h4 class="text-sm font-medium">{t('organization-member-groups')}</h4>
                                {#if row.member_groups.length}
                                    <div class="flex flex-wrap gap-2">{#each row.member_groups as group (group)}<span class="badge badge-outline">{group}</span>{/each}</div>
                                {:else}<p class="text-sm text-base-content/60">{t('organization-no-members')}</p>{/if}
                            </div>
                            <div class="card-actions justify-end">
                                <button class="btn btn-sm" type="button" onclick={() => { editing = row; editOpen = true; }}>{t('organization-edit')}</button>
                            </div>
                        </div>
                    </article>
                {/each}
            </div>
        {:else}<p>{t('organizations-empty')}</p>{/if}
    {:else if !error}<div class="skeleton h-64 w-full"></div>{/if}
</div>
<Modal bind:open={creating} title={t('organization-create')}>
    {#if data}<OrganizationForm groups={data.groups} onsave={(body) => save(null, body)} />{/if}
</Modal>
<Modal bind:open={editOpen} title={t('organization-edit')}>
    {#if data && editing}
        {#key `${editing.id}:${editing.revision}`}<OrganizationForm row={editing} groups={data.groups} onsave={(body) => save(editing, body)} />{/key}
    {/if}
</Modal>
