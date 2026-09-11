<script lang="ts">
	import { onMount } from 'svelte';
	import { adminJson } from '$lib/admin-client';
	import ProfileForm from '$lib/components/rag/ProfileForm.svelte';
	import { t } from '$lib/i18n.svelte';
	import type { RagProfile } from '$lib/rag';

	let profiles = $state<RagProfile[]>([]);
	let editing = $state<string | null>(null);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);

	async function refresh() {
		try {
			profiles = (await adminJson<{ data: RagProfile[] }>('/api/v0/rag/profiles')).data ?? [];
			error = null;
		} catch (caught) {
			error = String(caught);
		}
	}

	async function saved(message: string) {
		notice = message;
		editing = null;
		await refresh();
	}

	onMount(refresh);
</script>

<div class="w-full space-y-6">
	<header>
		<a class="link text-sm" href="/rag">← {t('rag-heading')}</a>
		<h1 class="mt-2 text-2xl font-bold">{t('rag-profile-heading')}</h1>
		<p class="mt-2 text-sm text-base-content/60">{t('rag-profile-description')}</p>
	</header>
	{#if error}<div class="alert alert-error"><span>{error}</span></div>{/if}
	{#if notice}<div class="alert alert-info"><span>{notice}</span></div>{/if}

	<ProfileForm onsaved={saved} />

	<section class="card card-border bg-base-100">
		<div class="card-body">
			<h2 class="card-title">{t('rag-profile-list-heading')}</h2>
			<ul class="list">
				{#each profiles as profile (profile.id)}
					<li class="list-row border-b border-base-300 last:border-b-0">
						<div class="list-col-grow">
							<div class="flex flex-wrap items-center gap-2"><strong>{profile.name}</strong><span class="badge badge-ghost badge-sm">{t('rag-profile-version', { version: profile.version })}</span>{#if profile.builtin}<span class="badge badge-sm">{t('rag-profile-builtin')}</span>{/if}</div>
							{#if profile.description}<p class="text-sm text-base-content/60">{profile.description}</p>{/if}
							<p class="text-xs text-base-content/60">{t('rag-profile-summary', { count: profile.fields.length })}</p>
						</div>
						<button class="btn btn-sm" onclick={() => (editing = editing === profile.name ? null : profile.name)}>{editing === profile.name ? t('rag-button-cancel') : t('rag-button-edit')}</button>
						{#if editing === profile.name}<div class="list-col-wrap pt-3"><ProfileForm {profile} onsaved={saved} oncancel={() => (editing = null)} /></div>{/if}
					</li>
				{:else}
					<li class="list-row text-base-content/60">{t('rag-profile-empty')}</li>
				{/each}
			</ul>
		</div>
	</section>
</div>
