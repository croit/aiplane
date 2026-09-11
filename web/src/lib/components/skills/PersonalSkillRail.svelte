<script lang="ts">
	import NavIcon from '$lib/components/NavIcon.svelte';
	import { t } from '$lib/i18n.svelte';
	import type { PersonalSkill } from '$lib/skills';

	let { skills, selected, mode, enabled, onupload } = $props<{
		skills: PersonalSkill[];
		selected: PersonalSkill | null;
		mode: 'view' | 'edit' | 'new';
		enabled: boolean;
		onupload: (file: File) => void | Promise<void>;
	}>();
	let file = $state<File | null>(null);
	let uploading = $state(false);

	async function upload(event: SubmitEvent) {
		event.preventDefault();
		if (!file) return;
		uploading = true;
		try { await onupload(file); file = null; } finally { uploading = false; }
	}
</script>

<aside class="flex w-full shrink-0 flex-col gap-3 sm:sticky sm:top-6 sm:w-60">
	{#if enabled}
		<a href="/skills?new=1" class="btn btn-sm w-full gap-1 {mode === 'new' ? 'btn-primary' : 'btn-outline'}"><NavIcon name="sparkles" size={14} /> {t('my-skills-new-button')}</a>
		<form class="card border border-base-300" onsubmit={upload}>
			<div class="card-body gap-2 p-3">
				<div class="text-xs uppercase tracking-wide text-base-content/50">{t('my-skills-upload-heading')}</div>
				<input type="file" accept=".skill,.zip" required class="file-input file-input-bordered file-input-sm w-full" onchange={(event) => (file = event.currentTarget.files?.[0] ?? null)} />
				<button type="submit" class="btn btn-primary btn-sm w-full" disabled={!file || uploading}>{t('my-skills-upload-button')}</button>
			</div>
		</form>
	{/if}
	<div class="card border border-base-300">
		<div class="card-body p-2">
			<div class="px-2 py-1 text-xs uppercase tracking-wide text-base-content/50">{t('my-skills-loaded-heading')}</div>
			{#if skills.length === 0}<div class="px-2 py-1 text-sm text-base-content/50">{t('my-skills-none-yet')}</div>{/if}
			<ul class="flex flex-col">
				{#each skills as skill}
					<li><a href={`/skills?skill=${encodeURIComponent(skill.name)}`} class="block cursor-pointer rounded px-2 py-1.5 text-sm transition-colors {mode !== 'new' && selected?.name === skill.name ? 'bg-base-300 font-semibold text-base-content' : 'text-base-content/70 hover:bg-base-200'}">{skill.title}</a></li>
				{/each}
			</ul>
		</div>
	</div>
</aside>
