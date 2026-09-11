<script lang="ts">
	import { onMount } from 'svelte';
	import { disablePush, enablePush, push, refreshPushState } from '$lib/push.svelte';
	import { t } from '$lib/i18n.svelte';

	onMount(refreshPushState);
	let phase = $derived(push.ui.phase);
	let status = $derived(
		phase === 'on' ? t('tokens-push-on') :
		phase === 'denied' ? t('tokens-push-denied') :
		phase === 'unsupported' ? t('tokens-push-unsupported') : t('tokens-push-off')
	);
</script>

<section class="card mb-6 border border-base-300">
	<div class="card-body">
		<h2 class="card-title">{t('tokens-push-heading')}</h2>
		<p class="text-base-content/70">{t('tokens-push-description')}</p>
		<p class="text-sm text-base-content/60">{push.note ?? status}</p>
		<div class="card-actions justify-end">
			{#if phase === 'on'}
				<button class="btn btn-outline" onclick={disablePush}>{t('tokens-push-disable')}</button>
			{:else if phase !== 'unsupported'}
				<button class="btn btn-primary" onclick={enablePush} disabled={phase === 'busy' || phase === 'denied'}>{t('tokens-push-enable')}</button>
			{/if}
		</div>
	</div>
</section>
