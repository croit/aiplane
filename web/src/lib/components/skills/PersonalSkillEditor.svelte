<script lang="ts">
	import { t } from '$lib/i18n.svelte';

	let { name, initial, isNew, onsave } = $props<{
		name: string;
		initial: string;
		isNew: boolean;
		onsave: (name: string, manifest: string) => void | Promise<void>;
	}>();
	let manifest = $state('');
	let saving = $state(false);
	$effect(() => { manifest = initial; });

	async function save(event: SubmitEvent) {
		event.preventDefault();
		saving = true;
		try { await onsave(name, manifest); } finally { saving = false; }
	}
</script>

<section class="min-w-0 flex-1">
	<h2 class="m-0 text-xl font-semibold">{t(isNew ? 'my-skills-new-heading' : 'my-skills-edit-heading')}</h2>
	<p class="mb-3 mt-1 text-sm text-base-content/60">{t('my-skills-editor-hint')}</p>
	<form class="flex flex-col gap-3" onsubmit={save}>
		<textarea name="content" rows="24" spellcheck="false" required class="textarea textarea-bordered w-full font-mono text-sm leading-relaxed" bind:value={manifest}></textarea>
		<div class="flex items-center justify-end gap-2"><a href={isNew ? '/skills' : `/skills?skill=${encodeURIComponent(name)}`} class="btn btn-ghost btn-sm">{t('my-skills-cancel-button')}</a><button type="submit" class="btn btn-primary btn-sm" disabled={saving}>{t('my-skills-save-button')}</button></div>
	</form>
</section>
