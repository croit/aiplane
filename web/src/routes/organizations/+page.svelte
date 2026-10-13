<script lang="ts">
    import { onMount } from 'svelte';
    import { organizationsApi, type OrganizationListRow } from '#lib/organizations.js';
    import { ApiError } from '#lib/api.js';
    import { t } from '#lib/i18n.svelte.js';
    let rows = $state<OrganizationListRow[] | null>(null);
    let error = $state<string | null>(null);
    async function reload() {
        try { rows = (await organizationsApi.list()).organizations; error = null; }
        catch (caught) { error = caught instanceof ApiError ? caught.serverMessage ?? caught.message : String(caught); }
    }
    onMount(() => void reload());
</script>
<div class="flex min-w-0 flex-col gap-4">
    <header class="flex flex-wrap gap-3"><h1 class="text-2xl font-bold">{t('organizations-heading')}</h1><button class="btn btn-sm ml-auto" onclick={() => void reload()}>{t('organization-reload')}</button></header>
    {#if error}<div class="alert alert-error" role="alert"><span>{error}</span></div>{/if}
    {#if rows}
        {#each rows as row (row.id)}<section class="card card-border bg-base-100"><div class="card-body"><h2 class="card-title break-words"><a class="link link-hover" href="/organizations/{encodeURIComponent(row.id)}">{row.name}</a></h2>{#if row.description}<p class="whitespace-pre-wrap break-words">{row.description}</p>{/if}</div></section>
        {:else}<p>{t('organizations-empty')}</p>{/each}
    {:else if !error}<div class="skeleton h-64 w-full"></div>{/if}
</div>
