<script lang="ts">
	import { onMount } from 'svelte';
	import { adminJson, adminDelete } from '$lib/admin-client';
	import { t } from '$lib/i18n.svelte';

	interface Skill {
		name: string;
		title: string;
		description: string;
		files: string[];
		body?: string | null;
	}
	let data = $state<{ skills: Skill[]; user_skills_enabled: boolean } | null>(null);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let expanded = $state<string | null>(null);
	let bodies = $state<Record<string, string>>({});

	async function refresh() {
		try {
			data = await adminJson('/api/v0/skills');
			error = null;
		} catch (err) {
			error = String(err);
		}
	}

	async function toggle(name: string) {
		if (expanded === name) {
			expanded = null;
			return;
		}
		expanded = name;
		if (!(name in bodies)) {
			try {
				const res = await adminJson<{ body: string }>(`/api/v0/skills/${encodeURIComponent(name)}/body`);
				bodies[name] = res.body;
			} catch (err) {
				bodies[name] = String(err);
			}
		}
	}

	async function upload(e: Event) {
		const input = e.currentTarget as HTMLInputElement;
		const file = input.files?.[0];
		if (!file) return;
		const fd = new FormData();
		fd.append('file', file);
		try {
			const res = await fetch('/api/v0/skills', { method: 'POST', body: fd });
			if (!res.ok) throw new Error((await res.text()).slice(0, 200));
			notice = t('my-skills-toast-installed', { name: file.name });
			await refresh();
		} catch (err) {
			notice = String(err);
		} finally {
			input.value = '';
		}
	}

	async function remove(name: string) {
		if (!confirm(t('my-skills-delete-confirm', { name }))) return;
		try {
			await adminDelete(`/api/v0/skills/${encodeURIComponent(name)}`);
			await refresh();
		} catch (err) {
			notice = String(err);
		}
	}

	onMount(refresh);
</script>

<div class="flex items-center justify-between mb-4">
	<h1 class="text-2xl font-bold">{t('my-skills-heading')}</h1>
	{#if data?.user_skills_enabled}
		<label class="btn btn-primary btn-sm cursor-pointer">
			{t('my-skills-upload-button')}
			<input type="file" accept=".skill,.zip" class="hidden" onchange={upload} />
		</label>
	{/if}
</div>

<p class="text-base-content/60 text-sm mb-6">
	{t('my-skills-intro')}
</p>

{#if error}<div class="alert alert-error mb-4"><span>{error}</span></div>{/if}
{#if notice}<div class="alert alert-warning mb-4"><span>{notice}</span></div>{/if}

<ul class="flex flex-col gap-2">
	{#each data?.skills ?? [] as skill (skill.name)}
		<li class="card border border-base-300">
			<div class="card-body py-2">
				<div class="flex items-center gap-3 flex-wrap">
					<button class="font-medium link link-hover" onclick={() => toggle(skill.name)}>
						{skill.title}
					</button>
					<span class="font-mono text-xs text-base-content/50">{skill.name}</span>
					<span class="flex-1"></span>
					<button class="btn btn-ghost btn-xs" onclick={() => toggle(skill.name)}>
						{expanded === skill.name ? t('my-skills-hide-button') : t('my-skills-view-button')}
					</button>
					<a
						class="btn btn-ghost btn-xs"
						href="/api/v0/skills/{encodeURIComponent(skill.name)}/archive"
						download="{skill.name}.skill"
						title={t('my-skills-download-title')}
					>
						{t('my-skills-download-button')}
					</a>
					{#if data?.user_skills_enabled}
						<button
							class="btn btn-ghost btn-xs text-error"
							onclick={() => remove(skill.name)}
							title={t('my-skills-delete-title')}
						>
							{t('my-skills-delete-button')}
						</button>
					{/if}
				</div>
				<p class="text-xs text-base-content/60">{skill.description}</p>
				{#if expanded === skill.name}
					<pre class="mt-2 bg-base-200 rounded-md p-3 text-xs whitespace-pre-wrap max-h-96 overflow-y-auto">{bodies[skill.name] ?? '…'}</pre>
				{/if}
			</div>
		</li>
	{:else}
		<li class="text-sm text-base-content/50">{t('my-skills-empty-loaded')}</li>
	{/each}
</ul>
