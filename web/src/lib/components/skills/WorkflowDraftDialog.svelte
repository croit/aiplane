<script lang="ts">
	import { api, type ChatModelChoice } from '#lib/api.js';
	import SearchableSelect from '#lib/components/SearchableSelect.svelte';
	import Modal from '#lib/components/ui/Modal.svelte';
	import { t } from '#lib/i18n.svelte.js';
	import { modelSelectOptions } from '#lib/model-option.js';

	let { open = $bindable(false), ongenerated }: {
		open?: boolean;
		ongenerated: (manifest: string) => void | Promise<void>;
	} = $props();

	let name = $state('');
	let goal = $state('');
	let steps = $state(['', '']);
	let stopConditions = $state('');
	let models = $state<ChatModelChoice[]>([]);
	let model = $state('');
	let loadingModels = $state(false);
	let generating = $state(false);
	let error = $state<string | null>(null);
	let modelOptions = $derived(modelSelectOptions(models, {
		gdpr: t('searchable-select-model-gdpr'),
		nda: t('searchable-select-model-nda')
	}));

	$effect(() => {
		if (open && models.length === 0 && !loadingModels) void loadModels();
	});

	async function loadModels() {
		loadingModels = true;
		try {
			const result = await api.listChatModels();
			models = result.models;
			model = result.models[0]?.id ?? '';
		} catch (caught) {
			error = String(caught);
		} finally {
			loadingModels = false;
		}
	}

	function addStep() {
		if (steps.length < 20) steps = [...steps, ''];
	}

	function removeStep(index: number) {
		if (steps.length > 1) steps = steps.filter((_, current) => current !== index);
	}

	async function generate(event: SubmitEvent) {
		event.preventDefault();
		error = null;
		generating = true;
		try {
			const result = await api.draftWorkflowSkill({
				name,
				goal,
				steps: steps.map((step) => step.trim()).filter(Boolean),
				stop_conditions: stopConditions,
				model
			});
			open = false;
			await ongenerated(result.manifest);
		} catch (caught) {
			error = String(caught);
		} finally {
			generating = false;
		}
	}
</script>

<Modal bind:open title={t('my-skills-draft-title')} description={t('my-skills-draft-intro')} size="lg">
	<form class="flex flex-col gap-4" onsubmit={generate}>
		<label class="flex flex-col gap-1">
			<span class="label-text">{t('my-skills-draft-name')}</span>
			<input class="input w-full" maxlength="160" required bind:value={name} placeholder={t('my-skills-draft-name-placeholder')} />
		</label>
		<label class="flex flex-col gap-1">
			<span class="label-text">{t('my-skills-draft-goal')}</span>
			<textarea class="textarea w-full" rows="2" maxlength="1200" required bind:value={goal} placeholder={t('my-skills-draft-goal-placeholder')}></textarea>
		</label>
		<fieldset class="flex flex-col gap-2">
			<legend class="label-text mb-1">{t('my-skills-draft-steps')}</legend>
			<p class="m-0 text-sm text-base-content/60">{t('my-skills-draft-steps-hint')}</p>
			{#each steps as step, index (index)}
				<div class="flex items-start gap-2">
					<span class="pt-3 text-sm text-base-content/60">{index + 1}.</span>
					<textarea class="textarea min-w-0 flex-1" rows="2" maxlength="1200" required bind:value={steps[index]} aria-label={t('my-skills-draft-step-label', { number: index + 1 })} placeholder={t('my-skills-draft-step-placeholder')}></textarea>
					{#if steps.length > 1}<button class="btn btn-ghost btn-sm" type="button" aria-label={t('my-skills-draft-remove-step')} onclick={() => removeStep(index)}>×</button>{/if}
				</div>
			{/each}
			<button class="btn btn-ghost btn-sm self-start" type="button" disabled={steps.length >= 20} onclick={addStep}>+ {t('my-skills-draft-add-step')}</button>
		</fieldset>
		<label class="flex flex-col gap-1">
			<span class="label-text">{t('my-skills-draft-stop')}</span>
			<textarea class="textarea w-full" rows="2" maxlength="1200" required bind:value={stopConditions} placeholder={t('my-skills-draft-stop-placeholder')}></textarea>
		</label>
		<label class="flex flex-col gap-1">
			<span class="label-text">{t('my-skills-draft-model')}</span>
			<SearchableSelect options={modelOptions} bind:value={model} ariaLabel={t('my-skills-draft-model')} placeholder={t('my-skills-draft-model-placeholder')} disabled={loadingModels || models.length === 0} />
		</label>
		{#if error}<div class="alert alert-error text-sm"><span>{error}</span></div>{/if}
		{#if models.length === 0 && !loadingModels}<p class="m-0 text-sm text-base-content/60">{t('my-skills-draft-no-models')}</p>{/if}
		<div class="modal-action mt-0">
			<button class="btn btn-ghost" type="button" disabled={generating} onclick={() => (open = false)}>{t('my-skills-cancel-button')}</button>
			<button class="btn btn-primary" type="submit" disabled={generating || loadingModels || models.length === 0 || !model}>{generating ? t('my-skills-draft-generating') : t('my-skills-draft-create')}</button>
		</div>
	</form>
</Modal>
