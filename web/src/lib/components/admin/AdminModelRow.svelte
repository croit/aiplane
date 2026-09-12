<script lang="ts">
	import { base } from '$app/paths';
	import { untrack } from 'svelte';
	import { configuredFacets, pricingUnitFor } from '$lib/admin-models';
	import type { AdminModel } from '$lib/admin-models';
	import { t } from '$lib/i18n.svelte';

	let { model, currency }: {
		model: AdminModel;
		currency: string;
	} = $props();

	const defaults = untrack(() => model.defaults);

	function unitLabel(unit: string) {
		return t(`admin-price-unit-${unit}`);
	}

	function priceSummary() {
		if (!defaults || (defaults.input_price === null && defaults.output_price === null)) return '—';
		return `${defaults?.input_price ?? '—'} / ${defaults?.output_price ?? '—'} ${unitLabel(pricingUnitFor(model))}`;
	}

	function contextSummary() {
		const value = defaults?.context_window;
		if (value === null || value === undefined) return t('admin-value-default');
		return value >= 1000 ? `${Math.floor(value / 1000)}k` : value.toString();
	}

	function reasoningSummary() {
		if (defaults?.reasoning_style) {
			return model.resolved_reasoning_style === 'qwen' ? 'Qwen' : model.resolved_reasoning_style === 'openai' ? 'OpenAI' : model.resolved_reasoning_style === 'glm' ? 'GLM' : model.resolved_reasoning_style === 'anthropic' ? 'Anthropic' : 'none';
		}
		const style = model.resolved_reasoning_style === 'qwen' ? 'Qwen' : model.resolved_reasoning_style === 'openai' ? 'OpenAI' : model.resolved_reasoning_style === 'glm' ? 'GLM' : model.resolved_reasoning_style === 'anthropic' ? 'Anthropic' : 'none';
		return t('admin-reasoning-auto-resolved', { style });
	}

	const facets = $derived(configuredFacets(model));
	const isChat = $derived(model.kind === 'chat');
</script>

{#if model.alias_target}
	<div class="border-b border-base-300 py-2 opacity-70" data-testid={`model-row-${model.name}`}>
		<div class="grid min-w-[710px] grid-cols-[minmax(170px,1.6fr)_82px_108px_84px_128px_minmax(110px,1.1fr)_64px] items-center gap-2.5">
			<span class="break-all font-mono text-sm text-base-content/70">{model.name} <span class="text-base-content/40">→ {model.alias_target}</span></span>
			<span><span class="badge badge-info badge-sm">{t('admin-alias-chip')}</span></span>
			<span class="text-xs text-base-content/40">{model.kind}</span><span></span><span></span>
			<span class="text-xs text-base-content/40">{t('admin-alias-inherits')}</span><span></span>
		</div>
	</div>
{:else}
	<div class="border-b border-base-300" data-testid={`model-row-${model.name}`}>
		<div class="py-2">
			<div class="grid min-w-[710px] grid-cols-[minmax(170px,1.6fr)_82px_108px_84px_128px_minmax(110px,1.1fr)_64px] items-center gap-2.5">
				<span class="break-all font-mono text-sm font-semibold">{model.name}</span>
				<span><span class="badge badge-secondary badge-sm">{model.kind}</span></span>
				<span class="text-xs tabular-nums {priceSummary() === '—' ? 'text-base-content/40' : ''}">{priceSummary()}</span>
				<span class="text-xs tabular-nums {isChat && defaults?.context_window != null ? '' : 'text-base-content/40'}">{isChat ? contextSummary() : t('admin-value-na')}</span>
				<span class="text-xs {isChat ? '' : 'text-base-content/40'}">{isChat ? reasoningSummary() : t('admin-value-na')}</span>
				<span class="flex flex-wrap gap-1">
					{#if facets.length === 0}<span class="text-xs text-base-content/40">{t('admin-not-configured')}</span>{/if}
					{#each facets as facet}<span class="badge badge-ghost badge-sm">{t(`admin-badge-${facet === 'context' ? 'ctx' : facet === 'capabilities' ? 'caps' : facet}`)}</span>{/each}
				</span>
				<a class="btn btn-ghost btn-xs justify-self-end" href="{base}/admin/models/edit?model={encodeURIComponent(model.name)}">{t('admin-edit-model')}</a>
			</div>
		</div>
	</div>
{/if}
