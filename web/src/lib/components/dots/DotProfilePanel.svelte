<script lang="ts">
	import { dotsApi, parseTopics, type DotProfile } from '#lib/dots.js';
	import { t } from '#lib/i18n.svelte.js';

	let { profile, onsaved }: { profile: DotProfile; onsaved: (profile: DotProfile) => void } = $props();

	let name = $state('');
	let description = $state('');
	let task = $state('');
	let tone = $state('');
	let topics = $state('');
	let busy = $state(false);
	let error = $state<string | null>(null);
	let saved = $state(false);

	$effect.pre(() => {
		name = profile.display;
		description = profile.description;
		task = profile.task;
		tone = profile.tone;
		topics = profile.topics.join('\n');
	});

	const dirty = $derived(
		name !== profile.display ||
			description !== profile.description ||
			task !== profile.task ||
			tone !== profile.tone ||
			parseTopics(topics).join('\n') !== profile.topics.join('\n')
	);

	async function save() {
		busy = true;
		error = null;
		saved = false;
		try {
			const next = await dotsApi.saveProfile(profile.id, {
				revision: profile.revision,
				display: name.trim(),
				description: description.trim(),
				task: task.trim(),
				tone: tone.trim(),
				topics: parseTopics(topics)
			});
			saved = true;
			onsaved(next);
		} catch (caught) {
			error = (caught as Error).message;
		} finally {
			busy = false;
		}
	}
</script>

<form class="flex max-w-3xl flex-col gap-4" onsubmit={(event) => { event.preventDefault(); void save(); }}>
	<p class="text-sm text-base-content/70">{t('agents-profile-intro')}</p>
	{#if error}<div class="alert alert-error text-sm" role="alert"><span>{error}</span></div>{/if}
	{#if saved && !dirty}<div class="alert alert-success text-sm" role="status"><span>{t('agents-profile-saved')}</span></div>{/if}
	<label class="flex flex-col gap-1">
		<span class="label-text">{t('agents-create-name')}</span>
		<input id="agent-profile-name" class="input w-full" bind:value={name} required maxlength="80" />
	</label>
	<label class="flex flex-col gap-1">
		<span class="label-text">{t('agents-create-description')}</span>
		<textarea id="agent-profile-description" class="textarea w-full" rows="2" maxlength="500" bind:value={description}></textarea>
		<span class="text-xs text-base-content/60">{t('dots-create-description-help')}</span>
	</label>
	<label class="flex flex-col gap-1">
		<span class="label-text">{t('agents-profile-task')}</span>
		<textarea id="agent-profile-task" class="textarea w-full" rows="8" maxlength="20000" bind:value={task} placeholder={t('agents-profile-task-placeholder')}></textarea>
		<span class="text-xs text-base-content/60">{t('agents-profile-task-help')}</span>
	</label>
	<label class="flex flex-col gap-1">
		<span class="label-text">{t('agents-profile-tone')}</span>
		<textarea id="agent-profile-tone" class="textarea w-full" rows="3" maxlength="20000" bind:value={tone} placeholder={t('agents-profile-tone-placeholder')}></textarea>
	</label>
	<label class="flex flex-col gap-1">
		<span class="label-text">{t('agents-profile-topics')}</span>
		<textarea id="agent-profile-topics" class="textarea w-full" rows="4" bind:value={topics}></textarea>
		<span class="text-xs text-base-content/60">{t('agents-profile-topics-help')}</span>
	</label>
	<div>
		<button class="btn btn-primary btn-sm" type="submit" disabled={busy || !dirty || !name.trim()}>
			{#if busy}<span class="loading loading-spinner loading-xs"></span>{/if}{t('agents-profile-save')}
		</button>
	</div>
</form>
