<script lang="ts">
	import { needsClearedModel, type Draft, type DetectorModel } from '#lib/classification.js';
	import LevelSelect from './LevelSelect.svelte';
	import DetectorModelSelect from './DetectorModelSelect.svelte';
	import { t } from '#lib/i18n.svelte.js';

	let { draft = $bindable(), models }: { draft: Draft; models: readonly DetectorModel[] } = $props();
	const needed = $derived(needsClearedModel(draft));
	const unclearedPicked = $derived(models.some((m) => m.id === draft.policy.detector_model && !m.cleared_for_highest));
</script>

<div class="space-y-5">
	<p class="text-sm text-base-content/70">{t('cls-policy-lead')}</p>
	<div class="grid gap-4 sm:grid-cols-2">
		<label class="flex flex-col gap-1">
			<span class="text-sm font-medium">{t('cls-policy-lowering')}</span>
			<select class="select select-sm w-full" bind:value={draft.policy.lowering}>
				<option value="never">{t('cls-policy-lowering-never')}</option>
				<option value="with_reason">{t('cls-policy-lowering-with-reason')}</option>
			</select>
			<span class="text-xs text-base-content/60">{t('cls-policy-lowering-help')}</span>
		</label>
		<label class="flex flex-col gap-1">
			<span class="text-sm font-medium">{t('cls-policy-gate')}</span>
			<select class="select select-sm w-full" bind:value={draft.policy.gate_on_match}>
				<option value="ask">{t('cls-policy-gate-ask')}</option>
				<option value="block">{t('cls-policy-gate-block')}</option>
			</select>
			<span class="text-xs text-base-content/60">{t('cls-policy-gate-help')}</span>
		</label>
		<label class="flex items-start gap-3">
			<input type="checkbox" class="toggle toggle-sm mt-0.5" bind:checked={draft.policy.user_override} />
			<span>
				<span class="block text-sm font-medium">{t('cls-policy-override')}</span>
				<span class="block text-xs text-base-content/60">{t('cls-policy-override-help')}</span>
			</span>
		</label>
		<label class="flex flex-col gap-1">
			<span class="text-sm font-medium">{t('cls-policy-link')}</span>
			<LevelSelect levels={draft.levels} bind:value={draft.policy.link_share_max_level} ariaLabel={t('cls-policy-link')} />
			<span class="text-xs text-base-content/60">{t('cls-policy-link-help')}</span>
		</label>
		<label class="flex flex-col gap-1">
			<span class="text-sm font-medium">{t('cls-policy-unknown')}</span>
			<LevelSelect levels={draft.levels} bind:value={draft.policy.unknown_label_level} ariaLabel={t('cls-policy-unknown')} />
			<span class="text-xs text-base-content/60">{t('cls-policy-unknown-help')}</span>
		</label>
		<label class="flex flex-col gap-1">
			<span class="text-sm font-medium">{t('cls-policy-unreadable')}</span>
			<LevelSelect levels={draft.levels} bind:value={draft.policy.unreadable_level} ariaLabel={t('cls-policy-unreadable')} />
			<span class="text-xs text-base-content/60">{t('cls-policy-unreadable-help')}</span>
		</label>
		<label class="flex flex-col gap-1">
			<span class="text-sm font-medium">{t('cls-policy-model')}</span>
			<DetectorModelSelect {models} bind:value={draft.policy.detector_model} cleared={(m) => m.cleared_for_highest} lockUncleared />
			<span class="text-xs text-base-content/60">{t('cls-policy-model-help')}</span>
			{#if needed && draft.policy.detector_model === ''}<span class="text-xs text-warning">{t('cls-policy-model-needed')}</span>{/if}
			{#if unclearedPicked}<span class="text-xs text-error">{t('cls-policy-model-uncleared-picked')}</span>{/if}
		</label>
	</div>
	<div class="space-y-1">
		<span class="text-sm font-medium">{t('cls-policy-mode')}</span>
		<div class="join">
			<input class="btn btn-sm join-item" type="radio" name="cls-mode" value="monitor" aria-label={t('cls-policy-mode-monitor')} bind:group={draft.policy.mode} />
			<input class="btn btn-sm join-item" type="radio" name="cls-mode" value="enforce" aria-label={t('cls-policy-mode-enforce')} bind:group={draft.policy.mode} />
		</div>
		<p class="m-0 text-xs text-base-content/60">{t('cls-policy-mode-help')}</p>
	</div>
</div>
