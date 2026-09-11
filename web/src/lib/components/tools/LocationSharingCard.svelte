<script lang="ts">
	import { t } from '$lib/i18n.svelte';
	import type { LocationSharingState } from '$lib/tools';

	let { location, busy, onshare, onforget }: {
		location: LocationSharingState;
		busy: boolean;
		onshare: () => void;
		onforget: () => void;
	} = $props();

	let status = $derived(
		location.shared
			? location.accuracy === null
				? t('tools-location-shared')
				: t('tools-location-shared-accuracy', { accuracy: Math.round(location.accuracy) })
			: t('tools-location-not-shared')
	);
</script>

<section class="card mb-6 border border-base-300">
	<div class="card-body">
		<h2 class="card-title text-base">{t('tools-location-heading')}</h2>
		<p class="m-0 text-sm text-base-content/60">{t('tools-location-description')}</p>
		<div class="mt-3 flex flex-wrap items-center gap-3">
			<button type="button" class="btn btn-primary btn-sm" onclick={onshare} disabled={busy}>
				{t('tools-location-share-button')}
			</button>
			<button type="button" class="btn btn-ghost btn-sm" onclick={onforget} disabled={busy || !location.shared}>
				{t('tools-location-stop-button')}
			</button>
			<span class="text-xs text-base-content/60">{status}</span>
		</div>
	</div>
</section>
