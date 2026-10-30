<script lang="ts">
	import SearchableSelect from '#lib/components/SearchableSelect.svelte';
	import { modelPickerOptions } from '#lib/agent-setup.js';
	import type { AgentResources, ModelKind } from '#lib/agents.js';
	import { t } from '#lib/i18n.svelte.js';

	/**
	 * One model key of an agent's spec, picked like the chat picks its model:
	 * `''` (labelled `emptyLabel`, left out when `null`) leaves the key unset,
	 * then what the manager may grant of `kind`, then what the agent holds.
	 */
	let {
		kind,
		value,
		held,
		resources,
		emptyLabel,
		ariaLabel,
		disabled = false,
		class: className = 'w-full max-w-sm',
		onchange
	}: {
		kind: ModelKind;
		value: string;
		held: string[];
		resources: AgentResources | null | undefined;
		emptyLabel: string | null;
		ariaLabel: string;
		disabled?: boolean;
		class?: string;
		onchange: (model: string) => void;
	} = $props();

	const options = $derived(
		modelPickerOptions(kind, value, held, resources, {
			empty: emptyLabel
		})
	);
</script>

<SearchableSelect {options} {value} {ariaLabel} {disabled} placeholder={t('agents-pick')} class={className} onchange={(model) => onchange(model)} />
