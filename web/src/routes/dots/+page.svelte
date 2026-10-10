<script lang="ts">
	import { onMount } from 'svelte';
	import Modal from '#lib/components/ui/Modal.svelte';
	import StatusPill from '#lib/components/ui/StatusPill.svelte';
	import DotMemoryPanel from '#lib/components/dots/DotMemoryPanel.svelte';
	import { activeDotCount, canCreateDot, dotsApi, type Dot } from '#lib/dots.js';
	import { t } from '#lib/i18n.svelte.js';

	let dots = $state<Dot[]>([]);
	let activeLimit = $state(0);
	let loading = $state(true);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let creating = $state(false);
	let name = $state('');
	let display = $state('');
	let description = $state('');
	let createError = $state<string | null>(null);
	let busy = $state(false);
	let busyId = $state<string | null>(null);
	let active = $derived(activeDotCount(dots));
	let createAllowed = $derived(canCreateDot(active, activeLimit));

	async function reload() {
		error = null;
		try {
			const result = await dotsApi.list();
			dots = result.dots;
			activeLimit = result.active_limit;
		} catch {
			error = t('dots-error-load');
		} finally {
			loading = false;
		}
	}

	onMount(() => void reload());

	async function create() {
		busy = true;
		createError = null;
		try {
			await dotsApi.create({ name: name.trim(), display: display.trim() || undefined, description: description.trim() });
			creating = false;
			name = '';
			display = '';
			description = '';
			notice = t('dots-toast-created');
			await reload();
		} catch (caught) {
			createError = (caught as Error).message;
		} finally {
			busy = false;
		}
	}

	async function setActive(dot: Dot) {
		busyId = dot.id;
		error = null;
		try {
			await dotsApi.setActive(dot.id, !dot.active);
			notice = t('dots-toast-state');
			await reload();
		} catch (caught) {
			error = (caught as Error).message;
		} finally {
			busyId = null;
		}
	}
</script>

<div class="w-full space-y-6">
	<header class="flex flex-wrap items-start justify-between gap-3">
		<div>
			<h1 class="text-2xl font-bold">{t('dots-heading')}</h1>
			<p class="mt-2 max-w-3xl text-sm text-base-content/60">{t('dots-intro')}</p>
			{#if !loading}<p class="mt-2 text-sm font-medium">{t('dots-active-count', { active, limit: activeLimit })}</p>{/if}
		</div>
		<button class="btn btn-primary btn-sm" type="button" disabled={!createAllowed || loading} onclick={() => { createError = null; creating = true; }}>
			{t('dots-create')}
		</button>
	</header>

	{#if notice}<div class="alert alert-success" role="status"><span>{notice}</span><button class="btn btn-ghost btn-xs" type="button" aria-label={t('ui-close')} onclick={() => (notice = null)}>✕</button></div>{/if}
	{#if error}<div class="alert alert-error" role="alert"><span>{error}</span></div>{/if}
	{#if activeLimit > 0 && !createAllowed}<div class="alert alert-warning" role="status"><span>{t('dots-limit-reached')}</span></div>{/if}

	{#if loading}
		<div class="skeleton h-28 w-full"></div>
	{:else if !dots.length && !error}
		<div class="card card-border"><div class="card-body text-sm text-base-content/60">{t('dots-empty')}</div></div>
	{:else}
		<div class="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
			{#each dots as dot (dot.id)}
				<article class="card card-border">
					<div class="card-body gap-3 p-4">
						<div class="flex items-start justify-between gap-2">
							<div class="min-w-0">
								<h2 class="card-title text-base">{dot.display || dot.name}</h2>
								{#if dot.display && dot.display !== dot.name}<p class="mt-1 font-mono text-xs text-base-content/60">{dot.name}</p>{/if}
							</div>
							{#if dot.active}<StatusPill tone="ok" size="sm">{t('dots-status-active')}</StatusPill>{:else}<StatusPill size="sm">{t('dots-status-inactive')}</StatusPill>{/if}
						</div>
						{#if dot.description}<p class="line-clamp-3 text-sm text-base-content/70">{dot.description}</p>{/if}
						<DotMemoryPanel {dot} />
						<div class="card-actions justify-end">
							<button class="btn btn-sm" type="button" disabled={busyId === dot.id || (!dot.active && !createAllowed)} onclick={() => void setActive(dot)}>
								{dot.active ? t('dots-action-deactivate') : t('dots-action-activate')}
							</button>
						</div>
					</div>
				</article>
			{/each}
		</div>
	{/if}
</div>

{#snippet createActions()}
	<button class="btn btn-ghost" type="button" disabled={busy} onclick={() => (creating = false)}>{t('dots-create-cancel')}</button>
	<button class="btn btn-primary" type="submit" form="dot-create" disabled={busy || !name.trim() || !createAllowed}>{t('dots-create-submit')}</button>
{/snippet}

<Modal bind:open={creating} title={t('dots-create-dialog')} footer={createActions}>
	<form id="dot-create" class="flex flex-col gap-3" onsubmit={(event) => { event.preventDefault(); void create(); }}>
		{#if createError}<div class="alert alert-error text-sm" role="alert"><span>{createError}</span></div>{/if}
		<label class="flex flex-col gap-1">
			<span class="label-text">{t('dots-create-name')}</span>
			<input class="input w-full" bind:value={name} required maxlength="80" placeholder={t('dots-create-name-placeholder')} />
		</label>
		<label class="flex flex-col gap-1">
			<span class="label-text">{t('dots-create-display')}</span>
			<input class="input w-full" bind:value={display} maxlength="80" />
		</label>
		<label class="flex flex-col gap-1">
			<span class="label-text">{t('dots-create-description')}</span>
			<textarea class="textarea w-full" bind:value={description} maxlength="500" rows="3"></textarea>
		</label>
	</form>
</Modal>
