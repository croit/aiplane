<script lang="ts">
	import { classificationApi, failure, mergeMarkings, suggestMarkingLevel, type Draft, type InspectResult, type MarkingKind } from '#lib/classification.js';
	import LevelSelect from './LevelSelect.svelte';
	import { t } from '#lib/i18n.svelte.js';

	/**
	 * Which existing markings mean which level. An already-labelled sample file
	 * is read for the markings it carries; each gets the level whose name it
	 * matches, for the operator to confirm. Stamp texts are typed in.
	 */
	let { draft = $bindable() }: { draft: Draft } = $props();

	interface Found {
		kind: MarkingKind;
		value: string;
		name?: string;
		level_id: string | null;
	}
	let found = $state<Found[]>([]);
	let unreadable = $state(false);
	let error = $state<string | null>(null);
	let busy = $state(false);
	let inspected = $state(false);

	async function inspect(file: File | null) {
		if (!file) return;
		busy = true;
		error = null;
		try {
			const result: InspectResult = await classificationApi.inspect(file);
			unreadable = result.unreadable;
			found = result.markings.map((m) => ({ ...m, level_id: suggestMarkingLevel(draft.levels, m) || null }));
			inspected = true;
		} catch (err) {
			error = failure(err);
		} finally {
			busy = false;
		}
	}

	function adopt() {
		draft.markings = mergeMarkings(
			draft.markings,
			found.flatMap((f) => (f.level_id ? [{ kind: f.kind, value: f.value, level_id: f.level_id }] : []))
		);
		found = [];
		inspected = false;
	}

	const stamps = $derived(draft.markings.map((m, index) => ({ m, index })).filter(({ m }) => m.kind === 'stamp'));
	const others = $derived(draft.markings.map((m, index) => ({ m, index })).filter(({ m }) => m.kind !== 'stamp'));
</script>

<div class="space-y-4">
	<p class="text-sm text-base-content/70">{t('cls-markings-lead')}</p>

	<div class="card card-border bg-base-100">
		<div class="card-body gap-3 p-4">
			<h4 class="card-title text-base">{t('cls-markings-inspect-heading')}</h4>
			<p class="m-0 text-sm text-base-content/70">{t('cls-markings-inspect-lead')}</p>
			<input type="file" class="file-input file-input-sm" aria-label={t('cls-markings-inspect-file')} disabled={busy} onchange={(e) => void inspect(e.currentTarget.files?.[0] ?? null)} />
			{#if error}<div class="alert alert-error text-sm" role="alert"><span>{error}</span></div>{/if}
			{#if inspected}
				{#if unreadable}<div class="alert alert-warning text-sm" role="status"><span>{t('cls-markings-unreadable')}</span></div>{/if}
				{#if found.length === 0}
					<p class="m-0 text-sm text-base-content/60">{t('cls-markings-none-found')}</p>
				{:else}
					<ul class="m-0 list-none space-y-2 p-0">
						{#each found as f (f.kind + f.value)}
							<li class="flex flex-wrap items-center gap-2">
								<span class="badge badge-outline">{t(`cls-marking-kind-${f.kind}`)}</span>
								<span class="min-w-0 flex-1 break-all text-sm">{f.name ?? f.value}</span>
								<div class="w-44"><LevelSelect levels={draft.levels} bind:value={f.level_id} none={t('cls-marking-unmapped')} ariaLabel={t('cls-marking-level')} /></div>
							</li>
						{/each}
					</ul>
					<button class="btn btn-sm btn-primary self-start" type="button" onclick={adopt}>{t('cls-markings-adopt')}</button>
				{/if}
			{/if}
		</div>
	</div>

	{#if others.length > 0}
		<ul class="m-0 list-none space-y-2 p-0">
			{#each others as { m, index } (index)}
				<li class="flex flex-wrap items-center gap-2">
					<span class="badge badge-outline">{t(`cls-marking-kind-${m.kind}`)}</span>
					<span class="min-w-0 flex-1 break-all text-sm">{m.value}</span>
					<div class="w-44"><LevelSelect levels={draft.levels} bind:value={m.level_id} ariaLabel={t('cls-marking-level')} /></div>
					<button class="btn btn-ghost btn-xs" type="button" onclick={() => (draft.markings = draft.markings.filter((_, i) => i !== index))}>{t('cls-remove')}</button>
				</li>
			{/each}
		</ul>
	{/if}

	<div class="space-y-2">
		<h4 class="m-0 text-base font-semibold">{t('cls-markings-stamps-heading')}</h4>
		<p class="m-0 text-sm text-base-content/70">{t('cls-markings-stamps-lead')}</p>
		{#each stamps as { m, index } (index)}
			<div class="flex flex-wrap items-center gap-2">
				<input class="input input-sm min-w-0 flex-1" aria-label={t('cls-marking-stamp')} bind:value={m.value} />
				<div class="w-44"><LevelSelect levels={draft.levels} bind:value={m.level_id} ariaLabel={t('cls-marking-level')} /></div>
				<button class="btn btn-ghost btn-xs" type="button" onclick={() => (draft.markings = draft.markings.filter((_, i) => i !== index))}>{t('cls-remove')}</button>
			</div>
		{/each}
		<button class="btn btn-sm" type="button" onclick={() => (draft.markings = [...draft.markings, { kind: 'stamp', value: '', level_id: draft.levels[0]!.id }])}>{t('cls-marking-stamp-add')}</button>
	</div>
</div>
