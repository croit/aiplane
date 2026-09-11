<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { onMount } from 'svelte';
	import { adminDelete, adminJson, adminPut } from '$lib/admin-client';
	import { selectedAdminSkill, type AdminSkillsData } from '$lib/admin-skills';
	import AdminSkillDetail from '$lib/components/admin/AdminSkillDetail.svelte';
	import AdminSkillRail from '$lib/components/admin/AdminSkillRail.svelte';
	import NavIcon from '$lib/components/NavIcon.svelte';
	import { t } from '$lib/i18n.svelte';

	let data = $state<AdminSkillsData | null>(null);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let requested = $derived(page.url.searchParams.get('skill'));
	let selected = $derived(selectedAdminSkill(data?.skills ?? [], requested));

	async function refresh() {
		try {
			data = await adminJson<AdminSkillsData>('/api/v0/admin/skills');
			error = null;
		} catch (caught) {
			error = String(caught);
		}
	}

	async function upload(file: File) {
		const body = new FormData();
		body.append('file', file);
		try {
			const installed = await adminJson<{ name: string }>('/api/v0/admin/skills', { method: 'POST', body });
			notice = t('skills-installed', { name: installed.name });
			await refresh();
			await goto(`/admin/skills?skill=${encodeURIComponent(installed.name)}`);
		} catch (caught) {
			error = String(caught);
		}
	}

	async function saveGrants(groups: string[]) {
		if (!selected) return;
		try {
			await adminPut('/api/v0/admin/skills/grants', { skill: selected.name, roles: groups });
			await refresh();
		} catch (caught) {
			error = String(caught);
		}
	}

	async function remove() {
		if (!selected || !confirm(t('skills-delete-confirm', { name: selected.name }))) return;
		try {
			await adminDelete(`/api/v0/admin/skills/${encodeURIComponent(selected.name)}`);
			await refresh();
			await goto('/admin/skills');
		} catch (caught) {
			error = String(caught);
		}
	}

	onMount(refresh);
</script>

<div class="w-full">
	<div class="flex items-center gap-2"><NavIcon name="sliders" size={20} /><h1 class="m-0 text-2xl font-bold">{t('skills-heading')}</h1></div>
	<p class="mb-4 mt-1 text-sm text-base-content/60">{t('skills-intro-part1')} <code class="font-mono text-xs">read_skill</code> {t('skills-intro-part2')} <code class="font-mono text-xs">.skill</code> {t('skills-intro-part3')}</p>
	{#if data && !data.directory_accessible}<div class="alert alert-warning mb-4 text-sm"><span>{t('skills-error-no-dir-access')} {data.source ?? ''}</span></div>{/if}
	{#if error}<div class="alert alert-error mb-4 text-sm"><span>{error}</span></div>{/if}
	{#if notice}<div class="alert alert-success mb-4 text-sm"><span>{notice}</span></div>{/if}
	{#if data}
		<div class="flex flex-col items-start gap-6 sm:flex-row">
			<AdminSkillRail skills={data.skills} {selected} source={data.source} configured={data.configured} onupload={upload} />
			{#if !data.configured}
				<section class="min-w-0 flex-1 pt-2 text-sm text-base-content/60">{t('skills-empty-not-configured')}</section>
			{:else if selected}
				{#key selected.name}<AdminSkillDetail skill={selected} groups={data.groups} ondelete={remove} onsavegrants={saveGrants} />{/key}
			{:else}
				<section class="min-w-0 flex-1 pt-2 text-sm text-base-content/60">{t('skills-empty-loaded')}</section>
			{/if}
		</div>
	{:else if !error}
		<div class="flex gap-6"><div class="skeleton h-72 w-60"></div><div class="skeleton h-72 flex-1"></div></div>
	{/if}
</div>
