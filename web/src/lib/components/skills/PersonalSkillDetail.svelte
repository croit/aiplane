<script lang="ts">
	import { renderMarkdown } from '$lib/markdown';
	import { t } from '$lib/i18n.svelte';
	import type { PersonalSkill } from '$lib/skills';

	let { skill, body, ondelete } = $props<{ skill: PersonalSkill; body: string; ondelete: () => void | Promise<void> }>();
</script>

<section class="min-w-0 flex-1">
	<div class="flex flex-wrap items-start justify-between gap-3">
		<h2 class="m-0 text-xl font-semibold">{skill.title}</h2>
		<div class="flex shrink-0 flex-wrap items-center gap-1">
			<a href={`/skills?skill=${encodeURIComponent(skill.name)}&edit=1`} class="btn btn-ghost btn-sm">{t('my-skills-edit-button')}</a>
			<a href={`/api/v0/skills/${encodeURIComponent(skill.name)}/archive`} download={`${skill.name}.skill`} class="btn btn-ghost btn-sm" title={t('my-skills-download-title')}>{t('my-skills-download-button')}</a>
			<button type="button" class="btn btn-ghost btn-sm text-error" title={t('my-skills-delete-title')} onclick={ondelete}>{t('my-skills-delete-button')}</button>
		</div>
	</div>
	<div class="mt-1 flex flex-wrap items-center gap-x-6 gap-y-1 text-xs text-base-content/50"><span class="font-mono">{skill.name}</span><span>{t('my-skills-files-count', { count: skill.files.length })}</span></div>
	<div class="mt-4"><div class="mb-1 text-xs uppercase tracking-wide text-base-content/50">{t('my-skills-description-heading')}</div><p class="m-0 text-sm text-base-content/80">{skill.description}</p></div>
	<div class="card mt-5 border border-base-300"><div class="card-body prose prose-sm max-w-none"><!-- eslint-disable-next-line svelte/no-at-html-tags -- sanitised in renderMarkdown -->{@html renderMarkdown(body)}</div></div>
</section>
