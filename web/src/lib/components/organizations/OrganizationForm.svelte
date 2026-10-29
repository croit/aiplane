<script lang="ts">
    import GroupSelect from '#lib/components/GroupSelect.svelte';
    import { ApiError } from '#lib/api.js';
    import { organizationDraft, organizationChange, type AdminOrganization, type OrganizationBody, type OrganizationEdit } from '#lib/organizations.js';
    import { t } from '#lib/i18n.svelte.js';
    let { row, groups, onsave }: { row?: AdminOrganization; groups: string[]; onsave: (body: OrganizationBody | OrganizationEdit) => Promise<void> } = $props();
    // svelte-ignore state_referenced_locally
    let draft = $state(organizationDraft(row));
    let busy = $state(false);
    let error = $state<string | null>(null);
    const change = $derived(row ? organizationChange(row, draft) : draft);
    const valid = $derived(!!draft.name.trim() && draft.member_groups.every((group) => groups.includes(group)));
    async function save() {
        if (!change || !valid) return;
        busy = true; error = null;
        try { await onsave(change); if (!row) draft = organizationDraft(); }
        catch (caught) { error = caught instanceof ApiError ? caught.serverMessage ?? caught.message : String(caught); }
        finally { busy = false; }
    }
</script>
<form class="fieldset min-w-0 gap-3" onsubmit={(event) => { event.preventDefault(); void save(); }}>
    <fieldset class="fieldset min-w-0 gap-3" disabled={busy}>
        <label class="fieldset"><span class="label">{t('agents-create-name')}</span><input class="input w-full" bind:value={draft.name} maxlength="80" required /></label>
        <label class="fieldset"><span class="label">{t('owned-agents-create-description')}</span><textarea class="textarea w-full" bind:value={draft.description} maxlength="16384"></textarea></label>
        <div class="fieldset min-w-0"><span class="label">{t('organization-member-groups')}</span><GroupSelect {groups} bind:values={draft.member_groups} label={t('organization-member-groups')} /></div>
        <p class="text-sm text-base-content/70">{t('organization-membership-explanation')}</p>
        <button class="btn btn-primary w-fit" type="submit" disabled={!valid || !change}>{t(row ? 'groups-save' : 'organization-create')}</button>
    </fieldset>
    {#if error}<div class="alert alert-error" role="alert"><span>{error}</span></div>{/if}
</form>
