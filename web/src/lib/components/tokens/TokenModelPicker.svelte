<script lang="ts">
	import { n, t } from '#lib/i18n.svelte.js';
	import { filterTokenModels, groupTokenModels, isCompliant, selectionSummary, type TokenModel } from '#lib/token-models.js';
	import { formatModelPrice } from '#lib/model-price.js';

	/**
	 * The model allowlist of one API token: a switch between "follows my
	 * access" and "only these", and — when restricted — the models grouped by
	 * kind, each with where its data goes and what it costs. Shared by the
	 * owner's and the admin's token editors, which differ only in wording.
	 */
	let { models, currency, help, restrictLabel, allLabel, restrict = $bindable(), selected = $bindable() }: {
		models: TokenModel[];
		currency: string;
		help: string;
		restrictLabel: string;
		allLabel: string;
		restrict: boolean;
		selected: string[];
	} = $props();

	let query = $state('');
	let gdpr = $state(false);
	let nda = $state(false);
	let free = $state(false);

	const shown = $derived(filterTokenModels(models, { query, gdpr, nda, free }));
	const groups = $derived(groupTokenModels(shown));
	const summary = $derived(selectionSummary(models, restrict ? selected : null));

	function money(value: number): string {
		return `${n(value, { maximumFractionDigits: 4 })} ${currency}`;
	}

	function price(model: TokenModel): string {
		// The token catalog omits the price object for unpriced models, so an
		// omitted price reads as "free" here — unlike the chat picker, where a
		// `null` price marks a route that has no single price and shows none.
		return formatModelPrice(model.price ?? { input: null, output: null, unit: 'tokens' }, currency, t, n) ?? '';
	}

	function toggle(id: string, checked: boolean) {
		selected = checked ? [...selected, id] : selected.filter((entry) => entry !== id);
	}

	function selectCompliant() {
		selected = models.filter(isCompliant).map((model) => model.id);
	}
</script>

<div class="flex flex-col gap-3">
	<p class="m-0 text-sm text-base-content/70">{help}</p>
	<label class="flex items-center gap-3 text-sm font-medium"><input type="checkbox" class="toggle toggle-primary" bind:checked={restrict} />{restrictLabel}</label>

	{#if restrict}
		<div class="flex flex-wrap items-center gap-2">
			<label class="input input-sm min-w-48 flex-1"><span aria-hidden="true">⌕</span><input bind:value={query} placeholder={t('tokens-models-search')} aria-label={t('tokens-models-search')} /></label>
			<input type="checkbox" class="btn btn-sm" aria-label={t('tokens-models-filter-gdpr')} bind:checked={gdpr} />
			<input type="checkbox" class="btn btn-sm" aria-label={t('tokens-models-filter-nda')} bind:checked={nda} />
			<input type="checkbox" class="btn btn-sm" aria-label={t('tokens-models-filter-free')} bind:checked={free} />
		</div>
		<div class="flex flex-wrap gap-2">
			<button type="button" class="btn btn-outline btn-xs" onclick={selectCompliant}>{t('tokens-models-select-compliant')}</button>
			<button type="button" class="btn btn-ghost btn-xs" onclick={() => (selected = [])}>{t('tokens-models-select-none')}</button>
		</div>

		<div class="max-h-[45vh] overflow-y-auto rounded-box border border-base-300">
			{#each groups as group (group.kind)}
				<div class="sticky top-0 z-10 bg-base-200 px-3 py-1 text-xs font-semibold uppercase tracking-wide text-base-content/60">{t(`tokens-models-kind-${group.kind}`)}</div>
				<ul class="list">
					{#each group.models as model (model.id)}
						<li class="list-row items-center py-2">
							<input type="checkbox" class="checkbox checkbox-sm" checked={selected.includes(model.id)} onchange={(event) => toggle(model.id, event.currentTarget.checked)} aria-label={model.id} />
							<div class="min-w-0">
								<div class="break-all font-mono text-sm">{model.id}</div>
								{#if model.alias_of}<div class="text-xs text-base-content/60">{t('tokens-models-alias', { target: model.alias_of })}</div>{/if}
							</div>
							<div class="flex flex-wrap items-center justify-end gap-1">
								<span class="badge badge-sm {model.gdpr ? 'badge-success badge-soft' : 'badge-error'}" title={model.gdpr ? t('tokens-models-gdpr-ok') : t('chat-render-gdpr-banner')}>{model.gdpr ? '✓' : '✕'} {t('searchable-select-model-gdpr')}</span>
								<span class="badge badge-sm {model.nda ? 'badge-success badge-soft' : 'badge-error'}" title={model.nda ? t('tokens-models-nda-ok') : t('chat-render-nda-banner')}>{model.nda ? '✓' : '✕'} {t('searchable-select-model-nda')}</span>
								<span class="w-full text-right font-mono text-xs tabular-nums text-base-content/60 sm:w-auto sm:min-w-40">{price(model)}</span>
							</div>
						</li>
					{/each}
				</ul>
			{:else}
				<p class="m-0 p-4 text-center text-sm text-base-content/60">{t('tokens-models-empty')}</p>
			{/each}
		</div>
	{/if}

	<div class="flex flex-wrap items-center gap-x-3 gap-y-1 text-sm">
		<span class="font-medium">{restrict ? t('tokens-models-summary-restricted', { count: summary.count }) : allLabel}</span>
		{#if summary.maxOutputPerMillion !== null}<span class="text-base-content/60">{t('tokens-models-max-price', { price: money(summary.maxOutputPerMillion) })}</span>{/if}
	</div>
	{#if summary.nonCompliant > 0}
		<div role="alert" class="alert alert-warning alert-soft text-sm"><span>{t('tokens-models-noncompliant-warning', { count: summary.nonCompliant })}</span></div>
	{/if}
	{#if restrict && selected.length === 0}<p class="m-0 text-sm text-error">{t('tokens-models-none-picked')}</p>{/if}
</div>
