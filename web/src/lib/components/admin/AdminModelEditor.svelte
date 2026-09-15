<script lang="ts">
	import SearchableSelect from '$lib/components/SearchableSelect.svelte';
	import { untrack } from 'svelte';
	import { contextWindowHint, pricingUnitFor } from '$lib/admin-models';
	import type { AdminModel } from '$lib/admin-models';
	import { n, t } from '$lib/i18n.svelte';

	/**
	 * The per-model override form.
	 *
	 * Lifted out of `AdminModelRow` when the editor moved off the list: it used
	 * to open inside the row as a `<details>`, which pushed the rest of a
	 * hundred-model table down the screen and gave a five-section form the
	 * width of a table cell. It now has its own route.
	 */
	let { model, currency, allModels, onsave, onclear, oncancel }: {
		model: AdminModel;
		currency: string;
		allModels: string[];
		onsave: (body: Record<string, string>) => Promise<void>;
		onclear: (name: string) => Promise<void>;
		oncancel: () => void;
	} = $props();

	const defaults = untrack(() => model.defaults);
	let inputPrice = $state(defaults?.input_price?.toString() ?? '');
	let outputPrice = $state(defaults?.output_price?.toString() ?? '');
	let contextWindow = $state(defaults?.context_window?.toString() ?? '');
	let reasoningStyle = $state(defaults?.reasoning_style ?? '');
	let budgetStandard = $state(defaults?.budget_standard?.toString() ?? '');
	let budgetDeep = $state(defaults?.budget_deep?.toString() ?? '');
	let budgetMax = $state(defaults?.budget_max?.toString() ?? '');
	let effortStandard = $state(defaults?.effort_standard ?? '');
	let effortDeep = $state(defaults?.effort_deep ?? '');
	let effortMax = $state(defaults?.effort_max ?? '');
	let capVision = $state(triValue(defaults?.capabilities.vision));
	let capTools = $state(triValue(defaults?.capabilities.tools));
	let capStructured = $state(triValue(defaults?.capabilities.structured_output));
	let capAudio = $state(triValue(defaults?.capabilities.audio_input));
	let capPdf = $state(triValue(defaults?.capabilities.pdf_input));
	let capParallel = $state(triValue(defaults?.capabilities.parallel_tools));
	let fallbackVision = $state(defaults?.capabilities.fallback_vision ?? '');
	let fallbackTools = $state(defaults?.capabilities.fallback_tools ?? '');
	let defaultsToml = $state(defaults?.defaults_toml ?? '');
	let saving = $state(false);
	let error = $state<string | null>(null);

	function triValue(value: boolean | null | undefined) {
		return value === true ? 'true' : value === false ? 'false' : '';
	}

	function unitLabel(unit: string) {
		return t(`admin-price-unit-${unit}`);
	}

	async function save(priceOnly = false) {
		saving = true;
		error = null;
		try {
			await onsave({
				model_name: model.name,
				input_price: inputPrice,
				output_price: outputPrice,
				pricing_unit: pricingUnitFor(model),
				price_only: priceOnly ? '1' : '',
				context_window: contextWindow,
				reasoning_style: reasoningStyle,
				budget_standard: budgetStandard,
				budget_deep: budgetDeep,
				budget_max: budgetMax,
				effort_standard: effortStandard,
				effort_deep: effortDeep,
				effort_max: effortMax,
				cap_vision: capVision,
				cap_tools: capTools,
				cap_structured_output: capStructured,
				cap_audio_input: capAudio,
				cap_pdf_input: capPdf,
				cap_parallel_tools: capParallel,
				fallback_vision: fallbackVision,
				fallback_tools: fallbackTools,
				defaults_toml: defaultsToml
			});
		} catch (caught) {
			error = String(caught);
		} finally {
			saving = false;
		}
	}

	const isChat = $derived(model.kind === 'chat');
	// What to say beneath the context field: where the number came from, or —
	// the case that matters — that the one being typed is larger than what the
	// server will actually hold, and so will be truncated there rather than
	// compacted here.
	const contextHint = $derived(contextWindowHint(contextWindow, model.detected_context_window));

	const priceLabel = $derived(t('admin-price-label', { cur: currency, unit: unitLabel(pricingUnitFor(model)) }));
</script>

<div class="flex flex-col gap-3">
	{#if error}<div class="alert alert-error py-2 text-sm"><span>{error}</span></div>{/if}
	<form class="m-0 flex flex-col gap-3" onsubmit={(event) => { event.preventDefault(); save(!isChat); }}>
		<div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
			<label class="flex flex-col gap-1"><span class="text-xs opacity-70">{t('admin-price-in-label')} ({priceLabel})</span><input type="number" min="0" step="any" class="input input-bordered input-sm" bind:value={inputPrice} placeholder={t('admin-price-in-placeholder')} /></label>
			<label class="flex flex-col gap-1"><span class="text-xs opacity-70">{t('admin-price-out-label')} ({priceLabel})</span><input type="number" min="0" step="any" class="input input-bordered input-sm" bind:value={outputPrice} placeholder={t('admin-price-out-placeholder')} /></label>
			{#if isChat}
				<label class="flex flex-col gap-1">
					<span class="text-xs opacity-70">{t('admin-context-window-full-label')}</span>
					<input type="number" min="1" class="input input-bordered input-sm" class:input-warning={contextHint?.tone === 'warning'} bind:value={contextWindow} placeholder={t('admin-context-window-placeholder')} aria-describedby={contextHint ? 'context-window-hint' : undefined} />
					{#if contextHint}
						<span id="context-window-hint" class="text-xs {contextHint.tone === 'warning' ? 'text-warning' : 'opacity-60'}">{contextHint.tone === 'warning' ? '⚠ ' : ''}{t(contextHint.key, contextHint.window === undefined ? undefined : { window: n(contextHint.window) })}</span>
					{/if}
				</label>
				<label class="flex flex-col gap-1"><span class="text-xs opacity-70">{t('admin-reasoning-style-label')}</span><select class="select select-bordered select-sm" aria-label={t('admin-reasoning-style-aria')} bind:value={reasoningStyle}><option value="">{t('admin-reasoning-auto')}</option><option value="none">{t('admin-reasoning-none')}</option><option value="qwen">{t('admin-reasoning-qwen')}</option><option value="openai">{t('admin-reasoning-openai')}</option><option value="glm">{t('admin-reasoning-glm')}</option><option value="anthropic">{t('admin-reasoning-anthropic')}</option><option value="ollama">{t('admin-reasoning-ollama')}</option></select></label>
			{/if}
		</div>
		{#if isChat && model.uses_token_budget}
			<div class="flex flex-col gap-2 border-t border-base-300 pt-3"><span class="text-xs text-base-content/60">{t('admin-budget-hint')}</span><div class="grid grid-cols-1 gap-3 sm:grid-cols-3">
				<label class="flex flex-col gap-1"><span class="text-xs opacity-70">{t('admin-effort-standard')}</span><input type="number" min="1" class="input input-bordered input-sm" bind:value={budgetStandard} placeholder={t('admin-budget-placeholder')} /></label>
				<label class="flex flex-col gap-1"><span class="text-xs opacity-70">{t('admin-effort-deep')}</span><input type="number" min="1" class="input input-bordered input-sm" bind:value={budgetDeep} placeholder={t('admin-budget-placeholder')} /></label>
				<label class="flex flex-col gap-1"><span class="text-xs opacity-70">{t('admin-effort-max')}</span><input type="number" min="1" class="input input-bordered input-sm" bind:value={budgetMax} placeholder={t('admin-budget-placeholder')} /></label>
			</div></div>
		{:else if isChat && model.effort_levels.length > 0}
			<div class="flex flex-col gap-2 border-t border-base-300 pt-3"><span class="text-xs text-base-content/60">{t('admin-effort-hint')}</span><div class="grid grid-cols-1 gap-3 sm:grid-cols-3">
				{#each [['admin-effort-standard', effortStandard], ['admin-effort-deep', effortDeep], ['admin-effort-max', effortMax]] as control, index}
					<label class="flex flex-col gap-1"><span class="text-xs opacity-70">{t(control[0])}</span><select class="select select-bordered select-sm" value={control[1]} onchange={(event) => { if (index === 0) effortStandard = event.currentTarget.value; else if (index === 1) effortDeep = event.currentTarget.value; else effortMax = event.currentTarget.value; }}><option value="">{t('admin-effort-default-option')}</option>{#each model.effort_levels as level}<option value={level}>{level}</option>{/each}</select></label>
				{/each}
			</div></div>
		{/if}
		{#if isChat}
			<div class="flex flex-col gap-2 border-t border-base-300 pt-3">
				<span class="text-xs text-base-content/60">{t('admin-capabilities-heading')}</span>
				<div class="grid grid-cols-1 gap-2 sm:grid-cols-3">
					{#each [['admin-cap-vision', capVision], ['admin-cap-tools', capTools], ['admin-cap-structured-output', capStructured], ['admin-cap-audio-input', capAudio], ['admin-cap-pdf-input', capPdf], ['admin-cap-parallel-tools', capParallel]] as control, index}
						<label class="flex flex-col gap-1"><span class="text-xs opacity-70">{t(control[0])}</span><select class="select select-bordered select-sm" aria-label={t(control[0])} value={control[1]} onchange={(event) => { const value = event.currentTarget.value; if (index === 0) capVision = value; else if (index === 1) capTools = value; else if (index === 2) capStructured = value; else if (index === 3) capAudio = value; else if (index === 4) capPdf = value; else capParallel = value; }}><option value="">{t('admin-cap-unknown')}</option><option value="true">{t('admin-cap-enabled')}</option><option value="false">{t('admin-cap-disabled')}</option></select></label>
					{/each}
				</div>
				<div class="grid grid-cols-1 gap-2 sm:grid-cols-2">
					<label class="flex flex-col gap-1"><span class="text-xs opacity-70">{t('admin-cap-fallback-vision')}</span><SearchableSelect options={[{ value: '', label: t('admin-cap-no-fallback') }, ...allModels.map((name) => ({ value: name, label: name }))]} bind:value={fallbackVision} ariaLabel={t('admin-cap-fallback-vision')} size="sm" class="w-full" /></label>
					<label class="flex flex-col gap-1"><span class="text-xs opacity-70">{t('admin-cap-fallback-tools')}</span><SearchableSelect options={[{ value: '', label: t('admin-cap-no-fallback') }, ...allModels.map((name) => ({ value: name, label: name }))]} bind:value={fallbackTools} ariaLabel={t('admin-cap-fallback-tools')} size="sm" class="w-full" /></label>
				</div>
			</div>
			<label class="flex flex-col gap-1"><span class="text-xs opacity-70">{t('admin-toml-defaults-label')}</span><textarea class="textarea textarea-bordered w-full font-mono text-sm leading-relaxed" rows="6" spellcheck="false" bind:value={defaultsToml} placeholder={t('admin-toml-placeholder-header')}></textarea></label>
		{:else}
			<p class="m-0 text-xs text-base-content/60">{t('admin-other-price-note')}</p>
		{/if}
		<div class="flex items-center gap-2">
			{#if isChat}<button type="button" class="btn btn-ghost btn-xs" onclick={() => onclear(model.name)}>{t('admin-clear-overrides')}</button>{/if}
			<span class="flex-1"></span>
			<button type="button" class="btn btn-ghost btn-sm" onclick={oncancel}>{t('admin-cancel')}</button>
			<button type="submit" class="btn btn-primary btn-sm" disabled={saving}>{t('admin-save-model')}</button>
		</div>
	</form>
</div>
