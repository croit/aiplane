<script lang="ts">
	import { page } from '$app/state';
	import { onMount } from 'svelte';
	import { adminJson, adminPost, adminPut } from '$lib/admin-client';
	import type { AdminModelsData } from '$lib/admin-models';
	import type { AdminSettingsData, AdminSettingsSection as AdminSettingsSectionData } from '$lib/admin-settings';
	import { fieldDraft, selectedSettingsCategory } from '$lib/admin-settings';
	import AdminSettingsRail from '$lib/components/admin/AdminSettingsRail.svelte';
	import AdminSettingsSection from '$lib/components/admin/AdminSettingsSection.svelte';
	import SearchSettingsCard from '$lib/components/admin/SearchSettingsCard.svelte';
	import { t } from '$lib/i18n.svelte';

	let data = $state<AdminSettingsData | null>(null);
	let drafts = $state<Record<string, Record<string, string>>>({});
	let saving = $state<string | null>(null);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let search = $state<AdminModelsData['search'] | null>(null);
	let selected = $derived(selectedSettingsCategory(page.url.search));
	let visibleSections = $derived(data?.sections.filter((section) => section.category === selected) ?? []);

	async function refresh() {
		try {
			const [settings, models] = await Promise.all([
				adminJson<AdminSettingsData>('/api/v0/admin/settings'),
				adminJson<AdminModelsData>('/api/v0/admin/models')
			]);
			data = settings;
			search = models.search;
			const next: Record<string, Record<string, string>> = {};
			for (const section of data.sections) next[section.name] = Object.fromEntries(section.fields.map((field) => [field.key, fieldDraft(field)]));
			drafts = next;
			error = null;
		} catch (caught) { error = String(caught); }
	}

	async function saveSearch(settings: { provider: string; searxng_url: string; brave_api_key: string; clear_brave_key: boolean; tavily_api_key: string; clear_tavily_key: boolean; tavily_enabled: boolean }) {
		await adminPut('/api/v0/admin/search-settings', settings);
		notice = t('admin-search-saved');
		await refresh();
	}

	function change(section: string, key: string, value: string) {
		drafts[section][key] = value;
	}

	async function save(section: AdminSettingsSectionData) {
		saving = section.name;
		error = null;
		try { await adminPost('/api/v0/admin/settings', { section: section.name, values: drafts[section.name] ?? {} }); notice = section.fields.some((field) => field.restart) ? t('settings-saved-restart') : t('settings-saved'); await refresh(); }
		catch (caught) { error = `${t('settings-save-failed')} ${String(caught)}`; }
		finally { saving = null; }
	}

	async function clearField(key: string) {
		if (!confirm(t('settings-clear-confirm', { key }))) return;
		error = null;
		try { await adminPost('/api/v0/admin/settings/clear', { key }); notice = t('settings-cleared'); await refresh(); }
		catch (caught) { error = String(caught); }
	}

	onMount(refresh);
</script>

<div class="flex w-full flex-col gap-4">
	<div><h1 class="mb-2 text-2xl font-semibold">{t('settings-heading')}</h1><p class="m-0 text-sm text-base-content/60">{t('settings-intro')}</p></div>
	{#if error}<div class="alert alert-error"><span>{error}</span></div>{/if}
	{#if notice}<div class="alert alert-success"><span>{notice}</span></div>{/if}
	{#if data?.needs_backend}<div class="alert alert-warning"><div class="flex flex-col gap-1"><span class="font-medium">{t('settings-no-backend-heading')}</span><span class="text-sm">{t('settings-no-backend-body')}</span><a class="link link-neutral self-start text-sm" href="/admin/models?tab=upstreams">{t('settings-no-backend-cta')}</a></div></div>{/if}
	{#if data && data.restart_pending.length > 0}<div class="alert alert-warning"><div class="flex flex-col gap-1"><span class="font-medium">{t('settings-restart-pending-heading')}</span><span class="text-sm">{t('settings-restart-pending-body')}</span><code class="text-xs">{data.restart_pending.join(', ')}</code></div></div>{/if}
{#if data}<div class="flex flex-col items-start gap-4 sm:flex-row"><AdminSettingsRail sections={data.sections} {selected} /><div class="flex min-w-0 grow flex-col gap-4">{#if selected === 'web-search'}{#if search}{#key `${search.provider}:${search.searxng_url}:${search.brave_key_set}:${search.tavily_key_set}:${search.tavily_enabled}:${search.tavily_active}`}<SearchSettingsCard {search} onsave={saveSearch} />{/key}{/if}{:else}{#each visibleSections as section (section.name)}<AdminSettingsSection {section} drafts={drafts[section.name] ?? {}} onchange={(key, value) => change(section.name, key, value)} onsave={() => save(section)} onclear={clearField} saving={saving === section.name} />{/each}{/if}</div></div>
	{:else if !error}<div class="flex gap-4"><div class="skeleton h-48 w-52"></div><div class="skeleton h-96 grow"></div></div>{/if}
</div>
