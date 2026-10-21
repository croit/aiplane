<script lang="ts">
	import { t } from '#lib/i18n.svelte.js';
	import type { IntegrationMode } from '#lib/integrations.js';

	let { current, onchange, disabled = false }: {
		current?: IntegrationMode;
		onchange: (mode: IntegrationMode) => void | Promise<void>;
		disabled?: boolean;
	} = $props();
	const modes: IntegrationMode[] = ['always', 'ask', 'off'];
</script>

<div class="join shrink-0">
	{#each modes as mode}
		<button type="button" class="btn btn-xs join-item {current === mode ? 'btn-primary' : 'btn-ghost'}" {disabled} onclick={() => onchange(mode)}>{t(`integrations-mode-${mode}`)}</button>
	{/each}
</div>
