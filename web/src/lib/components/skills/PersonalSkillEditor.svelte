<script lang="ts">
	import { t } from '#lib/i18n.svelte.js';

	let { name, initial, isNew, onsave }: {
		name: string;
		initial: string;
		isNew: boolean;
		onsave: (name: string, manifest: string, overwrite: boolean) => void | Promise<void>;
	} = $props();
	let manifest = $state('');
	let saving = $state(false);
	$effect(() => { manifest = initial; });

	async function save(event: SubmitEvent) {
		event.preventDefault();
		saving = true;
		try { await onsave(name, manifest, !isNew); } finally { saving = false; }
	}
</script>

<section class="min-w-0 flex-1">
	<h2 class="m-0 text-xl font-semibold">{t(isNew ? 'my-skills-new-heading' : 'my-skills-edit-heading')}</h2>
	<p class="mb-3 mt-1 text-sm text-base-content/60">{t('my-skills-editor-hint')}</p>
	<form class="flex flex-col gap-3" onsubmit={save}>
		<textarea name="content" rows="24" spellcheck="false" required class="textarea w-full font-mono text-sm leading-relaxed" bind:value={manifest}></textarea>
		<div class="flex items-center justify-end gap-2"><a href={isNew ? '/tools/skills' : `/tools/skills?skill=${encodeURIComponent(name)}`} class="btn btn-ghost btn-sm">{t('my-skills-cancel-button')}</a><button type="submit" class="btn btn-primary btn-sm" disabled={saving}>{t('my-skills-save-button')}</button></div>
	</form>
</section>
