<script lang="ts">
	import { renderMarkdown } from '$lib/markdown';
	import { t } from '$lib/i18n.svelte';
	import { effectiveSkillGroups, type AdminSkill } from '$lib/admin-skills';

	let { skill, groups, ondelete, onsavegrants } = $props<{
		skill: AdminSkill;
		groups: string[];
		ondelete: () => void | Promise<void>;
		onsavegrants: (groups: string[]) => void | Promise<void>;
	}>();
	let dialog: HTMLDialogElement;
	let selectedGroups = $state<string[]>([]);
	let saving = $state(false);
	let directGroups = $derived(skill.granted_groups.filter((group: string) => !skill.all_skills_groups.includes(group)));

	$effect(() => {
		selectedGroups = [...skill.granted_groups];
	});

	function toggle(group: string, enabled: boolean) {
		selectedGroups = enabled
			? [...new Set([...selectedGroups, group])]
			: selectedGroups.filter((candidate) => candidate !== group);
	}

	async function save() {
		saving = true;
		try {
			await onsavegrants(selectedGroups);
			dialog.close();
		} finally {
			saving = false;
		}
	}
</script>

<section class="min-w-0 flex-1">
	<div class="flex flex-wrap items-start justify-between gap-3">
		<h2 class="m-0 break-all font-mono text-xl font-semibold">{skill.name}</h2>
		<div class="flex shrink-0 flex-wrap items-center gap-1">
			<a href={`/api/v0/admin/skills/${encodeURIComponent(skill.name)}/archive`} download={`${skill.name}.skill`} class="btn btn-ghost btn-sm" title={t('skills-download-title')}>{t('skills-download-button')}</a>
			<button type="button" class="btn btn-ghost btn-sm text-error" title={t('skills-delete-title')} onclick={ondelete}>{t('skills-delete-button')}</button>
		</div>
	</div>
	<div class="mt-2 flex flex-wrap items-start gap-x-8 gap-y-2">
		<div>
			<div class="mb-1 text-xs uppercase tracking-wide text-base-content/50">{t('skills-granted-to-heading')}</div>
			<div class="flex flex-wrap items-center gap-1">
				{#each skill.all_skills_groups as group}<span class="badge badge-outline badge-sm" title={t('skills-granted-config-title')}>{group}</span>{/each}
				{#each directGroups as group}<span class="badge badge-secondary badge-sm">{group}</span>{/each}
				{#if effectiveSkillGroups(skill).length === 0}
					<button type="button" class="badge badge-warning badge-outline badge-sm cursor-pointer" title={t('skills-choose-access-title')} onclick={() => dialog.showModal()}>{t('skills-no-grants-warning')}</button>
				{:else}
					<button type="button" class="btn btn-ghost btn-xs" title={t('skills-edit-access-title')} onclick={() => dialog.showModal()}>{t('skills-edit-access-button')}</button>
				{/if}
			</div>
		</div>
		<div><div class="mb-1 text-xs uppercase tracking-wide text-base-content/50">{t('skills-files-heading')}</div><div class="text-sm text-base-content/80">{t('skills-files-count', { count: skill.files.length })}</div></div>
	</div>
	<div class="mt-4"><div class="mb-1 text-xs uppercase tracking-wide text-base-content/50">{t('skills-description-heading')}</div><p class="m-0 text-sm text-base-content/80">{skill.description}</p></div>
	<div class="card mt-5 border border-base-300"><div class="card-body prose max-w-none overflow-x-auto"><!-- eslint-disable-next-line svelte/no-at-html-tags -- sanitised in renderMarkdown -->{@html renderMarkdown(skill.body)}</div></div>

	<dialog bind:this={dialog} class="modal">
		<div class="modal-box max-w-lg">
			<h3 class="text-base font-semibold">{t('skills-grant-dialog-heading')}</h3>
			<p class="mt-1 text-xs text-base-content/60">{t('skills-grant-dialog-desc-part1')} <span class="font-mono">{skill.name}</span>{t('skills-grant-dialog-desc-part2')}</p>
			<div class="mt-3 flex flex-col">
				{#if groups.length === 0}<p class="text-sm text-base-content/60">{t('skills-grant-dialog-no-roles-part1')} <code class="font-mono text-xs">/admin/groups</code> {t('skills-grant-dialog-no-roles-part2')}</p>{/if}
				{#each groups as group}
					{@const inherited = skill.all_skills_groups.includes(group)}
					<label class="flex min-h-11 cursor-pointer items-center gap-2 py-1.5">
						<input type="checkbox" class="checkbox checkbox-sm" checked={inherited || selectedGroups.includes(group)} disabled={inherited} onchange={(event) => toggle(group, event.currentTarget.checked)} />
						<span class="font-mono text-sm">{group}</span>
						{#if inherited}<span class="badge badge-ghost badge-xs">{t('skills-from-config-badge')}</span>{/if}
					</label>
				{/each}
			</div>
			<div class="modal-action">
				<button type="button" class="btn btn-ghost btn-sm" onclick={() => dialog.close()}>{t('skills-cancel-button')}</button>
				<button type="button" class="btn btn-primary btn-sm" disabled={saving} onclick={save}>{t('skills-save-access-button')}</button>
			</div>
		</div>
		<form method="dialog" class="modal-backdrop"><button>{t('skills-cancel-button')}</button></form>
	</dialog>
</section>
