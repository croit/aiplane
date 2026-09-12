<script lang="ts">
	import {
		deviceLabel,
		formatGiB,
		formatVram,
		type ComfyuiCatalog,
		type ComfyuiHealth
	} from '$lib/admin-comfyui';
	import { n, t } from '$lib/i18n.svelte';

	// The strip that answers the operator's first question ("is it up?")
	// before the catalog answers their second ("what can it do?"). `health`
	// is null while the probe is in flight — a third state, not a failure.
	let {
		catalog,
		health,
		reloading = false,
		onreload
	}: {
		catalog: ComfyuiCatalog;
		health: ComfyuiHealth | null;
		reloading?: boolean;
		onreload: () => void;
	} = $props();

	let worker = $derived(health?.worker ?? null);
	let dot = $derived(
		health === null ? 'bg-base-content/30' : health.reachable ? 'bg-success' : 'bg-error'
	);
	let label = $derived(
		health === null
			? t('admin-comfyui-worker-checking')
			: health.reachable
				? t('admin-comfyui-worker-reachable')
				: t('admin-comfyui-worker-unreachable')
	);
</script>

<section class="card mb-5 border border-base-300">
	<div class="card-body gap-4 p-4">
		<div class="flex flex-wrap items-start justify-between gap-3">
			<!-- `basis-0 flex-1` so a long unreachable-error line shrinks rather
			     than pushing the queue badge and Reload onto their own row. -->
			<div class="min-w-0 flex-1 basis-0">
				<h2 class="m-0 mb-1 text-xs font-medium uppercase tracking-wide text-base-content/50">{t('admin-comfyui-worker-status')}</h2>
				<div class="flex items-center gap-2">
					<span class="inline-block size-2 shrink-0 rounded-full {dot}" aria-hidden="true"></span>
					<span class="text-sm font-semibold">{label}</span>
					<span class="sr-only">{t('admin-comfyui-worker-url')}</span>
					<span class="truncate font-mono text-xs text-base-content/60">{catalog.base_url}</span>
				</div>
				{#if worker}
					<p class="m-0 mt-1 text-xs text-base-content/60">
						{t('admin-comfyui-worker-software', {
							version: worker.version ?? '—',
							python: (worker.python_version ?? '—').split(' ')[0],
							torch: worker.pytorch_version ?? '—'
						})}
					</p>
					{#each worker.devices as device (device.name)}
						<p class="m-0 text-xs text-base-content/60">
							<span class="font-medium text-base-content/80">{deviceLabel(device.name)}</span>
							{#if device.vram_total !== null}
								· {t('admin-comfyui-worker-vram', {
									free: formatGiB(device.vram_free),
									total: formatGiB(device.vram_total)
								})}
							{/if}
							<span class="sr-only">{formatVram(device)}</span>
						</p>
					{/each}
				{:else if health && !health.reachable && health.error}
					<p class="m-0 mt-1 break-all font-mono text-xs text-error/80">{health.error}</p>
				{/if}
			</div>
			<div class="flex shrink-0 items-center gap-2">
				{#if worker}
					<span class="badge badge-ghost badge-sm whitespace-nowrap">
						{t('admin-comfyui-worker-queue', {
							running: n(worker.queue_running),
							pending: n(worker.queue_pending)
						})}
					</span>
				{/if}
				<button type="button" class="btn btn-primary btn-sm" disabled={reloading} onclick={onreload}>
					{t('admin-comfyui-reload')}
				</button>
			</div>
		</div>

		<!-- Operator configuration: four facts, one line each, in a grid that
		     collapses to one column on a phone. -->
		<div class="border-t border-base-300 pt-3">
		<h2 class="m-0 mb-2 text-xs font-medium uppercase tracking-wide text-base-content/50">{t('admin-comfyui-operator-config')}</h2>
		<dl class="grid grid-cols-2 gap-x-4 gap-y-2 text-xs sm:grid-cols-4">
			<div>
				<dt class="text-base-content/50">{t('admin-comfyui-timeout')}</dt>
				<dd class="m-0 font-mono">{catalog.timeout_secs} s</dd>
			</div>
			<div>
				<dt class="text-base-content/50">{t('admin-comfyui-poll-interval')}</dt>
				<dd class="m-0 font-mono">{catalog.queue_poll_interval_ms} ms</dd>
			</div>
			<div>
				<dt class="text-base-content/50">{t('admin-comfyui-max-concurrent')}</dt>
				<dd class="m-0 font-mono">{catalog.max_concurrent_jobs}</dd>
			</div>
			<div class="col-span-2 min-w-0 sm:col-span-1">
				<dt class="text-base-content/50">{t('admin-comfyui-content-directory')}</dt>
				<dd class="m-0 truncate font-mono" title={catalog.content_dir}>{catalog.content_dir}</dd>
			</div>
		</dl>
		</div>
	</div>
</section>
