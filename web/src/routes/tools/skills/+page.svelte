<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { onMount } from 'svelte';
	import { adminDelete, adminJson, adminPost } from '#lib/admin-client.js';
	import NavIcon from '#lib/components/NavIcon.svelte';
	import PersonalSkillDetail from '#lib/components/skills/PersonalSkillDetail.svelte';
	import PersonalSkillEditor from '#lib/components/skills/PersonalSkillEditor.svelte';
	import PersonalSkillRail from '#lib/components/skills/PersonalSkillRail.svelte';
	import WorkflowDraftDialog from '#lib/components/skills/WorkflowDraftDialog.svelte';
	import { t } from '#lib/i18n.svelte.js';
	import { NEW_SKILL_TEMPLATE, selectedSkill, type PersonalSkill } from '#lib/skills.js';

	interface SkillDetail {
		body: string;
		manifest: string;
	}

	let skills = $state<PersonalSkill[]>([]);
	let enabled = $state(false);
	let details = $state<Record<string, SkillDetail>>({});
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let workflowDraftOpen = $state(false);
	let generatedManifest = $state('');
	let requested = $derived(page.url.searchParams.get('skill'));
	let mode = $derived<'view' | 'edit' | 'new'>(page.url.searchParams.has('new') ? 'new' : page.url.searchParams.has('edit') ? 'edit' : 'view');
	let selected = $derived(selectedSkill(skills, requested));

	$effect(() => {
		if (mode !== 'new') generatedManifest = '';
	});

	async function refresh() {
		try {
			const data = await adminJson<{ skills: PersonalSkill[]; user_skills_enabled: boolean }>('/api/v0/skills');
			skills = data.skills;
			enabled = data.user_skills_enabled;
			error = null;
		} catch (caught) { error = String(caught); }
	}

	async function loadDetail(name: string) {
		if (details[name]) return;
		try {
			const detail = await adminJson<SkillDetail>(`/api/v0/skills/${encodeURIComponent(name)}/body`);
			details = { ...details, [name]: detail };
		} catch (caught) { notice = String(caught); }
	}

	$effect(() => {
		const name = selected?.name;
		if (name && mode !== 'new') void loadDetail(name);
	});

	async function upload(file: File) {
		const body = new FormData();
		body.append('file', file);
		try {
			const installed = await adminJson<{ name: string }>('/api/v0/skills', { method: 'POST', body });
			notice = t('my-skills-toast-installed', { name: installed.name });
			await refresh();
			await goto(`/tools/skills?skill=${encodeURIComponent(installed.name)}`);
		} catch (caught) { notice = String(caught); }
	}

	async function save(name: string, manifest: string, overwrite: boolean) {
		try {
			const saved = await adminPost<{ name: string }>('/api/v0/skills', { name, manifest, overwrite });
			details = {};
			await refresh();
			await goto(`/tools/skills?skill=${encodeURIComponent(saved.name)}`);
		} catch (caught) { notice = String(caught); }
	}

	async function remove() {
		if (!selected || !confirm(t('my-skills-delete-confirm', { name: selected.name }))) return;
		try {
			await adminDelete(`/api/v0/skills/${encodeURIComponent(selected.name)}`);
			details = {};
			await refresh();
			await goto('/tools/skills');
		} catch (caught) { notice = String(caught); }
	}

	async function useGeneratedDraft(manifest: string) {
		await goto('/tools/skills?new=1');
		generatedManifest = manifest;
	}

	onMount(refresh);
</script>

<div class="w-full">
	<div class="flex items-center gap-2"><NavIcon name="sparkles" size={20} /><h2 class="m-0 text-xl font-bold">{t('my-skills-heading')}</h2></div>
	<p class="mb-4 mt-1 text-sm text-base-content/60">{t('my-skills-intro')}</p>
	{#if error}<div class="alert alert-error mb-4 text-sm"><span>{error}</span></div>{/if}
	{#if notice}<div class="alert alert-warning mb-4 text-sm"><span>{notice}</span></div>{/if}
	<div class="flex flex-col items-start gap-6 sm:flex-row">
		<PersonalSkillRail {skills} {selected} {mode} {enabled} onupload={upload} onworkflowdraft={() => (workflowDraftOpen = true)} />
		{#if !enabled}
			<section class="min-w-0 flex-1 pt-2 text-sm text-base-content/60">{t('my-skills-empty-not-configured')}</section>
		{:else if mode === 'new'}
			<PersonalSkillEditor name="" initial={generatedManifest || NEW_SKILL_TEMPLATE} isNew={true} onsave={save} />
		{:else if selected && details[selected.name] && mode === 'edit'}
			<PersonalSkillEditor name={selected.name} initial={details[selected.name].manifest} isNew={false} onsave={save} />
		{:else if selected && details[selected.name]}
			<PersonalSkillDetail skill={selected} body={details[selected.name].body} ondelete={remove} />
		{:else if selected}
			<section class="min-w-0 flex-1"><div class="skeleton h-6 w-48"></div><div class="skeleton mt-5 h-48 w-full"></div></section>
		{:else}
			<section class="min-w-0 flex-1 pt-2 text-sm text-base-content/60">{t('my-skills-empty-loaded')}</section>
		{/if}
	</div>
</div>

<WorkflowDraftDialog bind:open={workflowDraftOpen} ongenerated={useGeneratedDraft} />
