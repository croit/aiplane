<script lang="ts">
	import NavIcon from '$lib/components/NavIcon.svelte';
	import { t } from '$lib/i18n.svelte';
	import { skillFileTree, type AdminSkill } from '$lib/admin-skills';

	let { skills, selected, source, configured, onupload } = $props<{
		skills: AdminSkill[];
		selected: AdminSkill | null;
		source: string | null;
		configured: boolean;
		onupload: (file: File) => void | Promise<void>;
	}>();
	let file = $state<File | null>(null);
	let uploading = $state(false);

	async function upload(event: SubmitEvent) {
		event.preventDefault();
		if (!file) return;
		uploading = true;
		try {
			await onupload(file);
			file = null;
		} finally {
			uploading = false;
		}
	}
</script>

<aside class="flex w-full shrink-0 flex-col gap-3 sm:sticky sm:top-6 sm:w-60">
	{#if configured}
		<form class="card border border-base-300" onsubmit={upload}>
			<div class="card-body gap-2 p-3">
				<div class="text-xs uppercase tracking-wide text-base-content/50">{t('skills-upload-heading')}</div>
				<input type="file" accept=".skill,.zip" required class="file-input file-input-bordered file-input-sm w-full" onchange={(event) => (file = event.currentTarget.files?.[0] ?? null)} />
				<button type="submit" class="btn btn-primary btn-sm w-full" disabled={!file || uploading}>{t('skills-upload-button')}</button>
			</div>
		</form>
	{/if}
	<div class="card border border-base-300">
		<div class="card-body p-2">
			<div class="px-2 py-1 text-xs uppercase tracking-wide text-base-content/50">{t('skills-loaded-heading')}</div>
			{#if skills.length === 0}<div class="px-2 py-1 text-sm text-base-content/50">{t('skills-none-yet')}</div>{/if}
			<ul class="flex flex-col">
				{#each skills as skill}
					<li>
						<a href={`/admin/skills?skill=${encodeURIComponent(skill.name)}`} class="block cursor-pointer rounded px-2 py-1.5 font-mono text-sm transition-colors {selected?.name === skill.name ? 'bg-base-300 font-semibold text-base-content' : 'text-base-content/70 hover:bg-base-200'}">{skill.name}</a>
						{#if selected?.name === skill.name}
							<div class="pb-1 pl-3 pr-1 text-xs text-base-content/60">
								<div class="flex items-center gap-1 py-0.5"><NavIcon name="folder" size={12} /><span class="font-mono">SKILL.md</span></div>
								{#each skillFileTree(skill.files) as group}
									{#if group.directory}<div class="flex items-center gap-1 py-0.5 text-base-content/50"><NavIcon name="folder" size={12} /><span class="font-mono">{group.directory}/</span></div>{/if}
									{#each group.files as name}<div class="truncate py-0.5 font-mono {group.directory ? 'pl-4' : ''}">{name}</div>{/each}
								{/each}
							</div>
						{/if}
					</li>
				{/each}
			</ul>
			{#if source}<div class="mt-1 break-all border-t border-base-300 px-2 pt-2 text-xs text-base-content/40">{t('skills-source-prefix')} {source}</div>{/if}
		</div>
	</div>
</aside>
