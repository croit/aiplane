<script lang="ts">
	import type { Draft } from '#lib/classification.js';
	import { t } from '#lib/i18n.svelte.js';

	let { draft = $bindable() }: { draft: Draft } = $props();
	const switches = ['visible', 'metadata', 'filename'] as const;
</script>

<div class="space-y-4">
	<p class="text-sm text-base-content/70">{t('cls-output-lead')}</p>
	{#each switches as name (name)}
		<label class="flex items-start gap-3">
			<input type="checkbox" class="toggle toggle-sm mt-0.5" bind:checked={draft.policy.output[name]} />
			<span>
				<span class="block text-sm font-medium">{t(`cls-output-${name}`)}</span>
				<span class="block text-xs text-base-content/60">{t(`cls-output-${name}-help`)}</span>
			</span>
		</label>
	{/each}
	<label class="flex flex-col gap-1 sm:max-w-sm">
		<span class="text-sm font-medium">{t('cls-output-tenant')}</span>
		<input class="input input-sm w-full" bind:value={draft.policy.mip_tenant_id} />
		<span class="text-xs text-base-content/60">{t('cls-output-tenant-help')}</span>
	</label>
</div>
