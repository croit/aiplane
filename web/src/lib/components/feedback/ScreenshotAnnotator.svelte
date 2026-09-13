<script lang="ts">
	/**
	 * The screenshot + its annotation toolbar.
	 *
	 * The drawing itself is `$lib/feedback-annotator` — a plain canvas
	 * controller, deliberately not reactive state: shapes, pointer capture and
	 * an undo stack are exactly the things that get worse when every stroke
	 * has to round-trip through the reactivity graph. This component owns the
	 * canvas and the scrolling box around it, mirrors just enough of the
	 * controller's state to keep the toolbar honest, and publishes the export
	 * function so `submit()` can read the annotated PNG back out.
	 *
	 * Zoom is a multiple of the *fit* size, so the baseline depends on how big
	 * the box currently is — which changes when the dialog enters annotate
	 * mode. A ResizeObserver feeds that back in; without it a zoomed view would
	 * keep the old baseline and jump.
	 */
	import { onDestroy } from 'svelte';
	import {
		ANNOTATOR_COLORS,
		ZOOM_FACTOR,
		createAnnotator,
		type Annotator,
		type AnnotatorTool,
		type ZoomMode
	} from '$lib/feedback-annotator';
	import { setShotExporter } from '$lib/feedback.svelte';
	import { t } from '$lib/i18n.svelte';

	let { dataUrl, tall = false }: { dataUrl: string; tall?: boolean } = $props();

	let canvas = $state<HTMLCanvasElement | null>(null);
	let viewport = $state<HTMLDivElement | null>(null);
	let annotator: Annotator | null = null;

	// Mirrored toolbar state. Read back from the controller on every change
	// rather than tracked separately, so the two cannot disagree.
	let tool = $state<AnnotatorTool>('rect');
	let color = $state(ANNOTATOR_COLORS[0]!);
	let canUndo = $state(false);
	let canRedo = $state(false);
	let zoom = $state(1);
	let zoomMode = $state<ZoomMode>('fit');

	const TOOLS: { id: AnnotatorTool; glyph: string; key: string }[] = [
		{ id: 'pan', glyph: '✥', key: 'feedback-tool-pan-title' },
		{ id: 'rect', glyph: '▭', key: 'feedback-tool-rect-title' },
		{ id: 'arrow', glyph: '↗', key: 'feedback-tool-arrow-title' },
		{ id: 'pen', glyph: '✎', key: 'feedback-tool-pen-title' },
		{ id: 'text', glyph: 'T', key: 'feedback-tool-text-title' },
		{ id: 'redact', glyph: '▮', key: 'feedback-tool-redact-title' }
	];

	function sync(): void {
		if (!annotator) return;
		tool = annotator.getTool();
		color = annotator.getColor();
		canUndo = annotator.canUndo();
		canRedo = annotator.canRedo();
		zoom = annotator.getZoom();
		zoomMode = annotator.zoomMode();
	}

	// ONE effect for the whole lifecycle — creation and image load together.
	//
	// Splitting them is the obvious shape and it is broken: `annotator` is a
	// plain `let`, so an effect that reads it does not depend on it. The
	// bind:this refs arrive after the first flush, so the loading effect can
	// run while `annotator` is still null, bail, and never be re-run — a blank
	// canvas at its raw capture size, with no zoom applied. Keying the whole
	// thing on canvas + viewport + dataUrl makes that impossible.
	//
	// Rebuilding on a new `dataUrl` is not a cost: a fresh capture resets the
	// shapes and history anyway.
	$effect(() => {
		const el = canvas;
		const box = viewport;
		const url = dataUrl;
		if (!el || !box) return;
		const instance = createAnnotator(el, box, () => window.prompt(t('feedback-tool-text-prompt')));
		annotator = instance;
		instance.onChange(sync);
		setShotExporter(() => instance.toDataUrl());
		const observer = new ResizeObserver(() => instance.refit());
		observer.observe(box);
		sync();
		if (url) {
			void instance
				.loadDataUrl(url)
				.then(sync)
				.catch(() => {});
		}
		return () => {
			observer.disconnect();
			instance.destroy();
			if (annotator === instance) annotator = null;
			setShotExporter(null);
		};
	});

	onDestroy(() => setShotExporter(null));
</script>

<div class="flex min-h-0 flex-col gap-2 {tall ? 'flex-1' : ''}">
	<div class="flex flex-wrap items-center gap-1 rounded-lg border border-base-300 bg-base-200 p-1">
		<div class="join">
			{#each TOOLS as item (item.id)}
				<button
					type="button"
					class="btn join-item btn-xs {tool === item.id ? 'btn-primary' : 'btn-ghost'}"
					title={t(item.key)}
					aria-label={t(item.key)}
					aria-pressed={tool === item.id}
					onclick={() => annotator?.setTool(item.id)}>{item.glyph}</button
				>
			{/each}
		</div>

		<div class="mx-1 h-5 w-px bg-base-300"></div>

		<div class="flex gap-1">
			{#each ANNOTATOR_COLORS as swatch (swatch)}
				<button
					type="button"
					class="h-5 w-5 rounded border-2 transition-transform hover:scale-110 {color === swatch
						? 'border-base-content'
						: 'border-base-300'}"
					style="background:{swatch}"
					title={t('feedback-color-aria')}
					aria-label={t('feedback-color-aria')}
					aria-pressed={color === swatch}
					onclick={() => annotator?.setColor(swatch)}
				></button>
			{/each}
		</div>

		<div class="mx-1 h-5 w-px bg-base-300"></div>

		<button
			type="button"
			class="btn btn-ghost btn-xs"
			disabled={!canUndo}
			title={t('feedback-undo-title')}
			aria-label={t('feedback-undo-title')}
			onclick={() => annotator?.undo()}>↶</button
		>
		<button
			type="button"
			class="btn btn-ghost btn-xs"
			disabled={!canRedo}
			title={t('feedback-redo-title')}
			aria-label={t('feedback-redo-title')}
			onclick={() => annotator?.redo()}>↷</button
		>
		<button
			type="button"
			class="btn btn-ghost btn-xs"
			title={t('feedback-clear-annot-title')}
			onclick={() => annotator?.clearAnnotations()}>{t('feedback-clear-annot-label')}</button
		>

		<div class="ms-auto flex items-center gap-1">
			<button
				type="button"
				class="btn btn-ghost btn-xs"
				title={t('feedback-zoom-out-title')}
				aria-label={t('feedback-zoom-out-title')}
				onclick={() => annotator?.zoomBy(1 / ZOOM_FACTOR)}>−</button
			>
			<!-- One button, two presets: click cycles fit → fill-width → fit, and
			     shows which one you are on (or the raw percentage when neither). -->
			<button
				type="button"
				class="btn btn-ghost btn-xs tabular-nums"
				title={t('feedback-zoom-preset-title')}
				onclick={() => annotator?.cycleZoomPreset()}
			>
				{zoomMode === 'fit'
					? t('feedback-zoom-fit-label')
					: zoomMode === 'width'
						? t('feedback-zoom-width-label')
						: `${Math.round(zoom * 100)}%`}
			</button>
			<button
				type="button"
				class="btn btn-ghost btn-xs"
				title={t('feedback-zoom-in-title')}
				aria-label={t('feedback-zoom-in-title')}
				onclick={() => annotator?.zoomBy(ZOOM_FACTOR)}>+</button
			>
		</div>
	</div>

	<!-- The scrolling box IS the annotator's viewport: zoom is measured against
	     it and panning moves its scroll offsets. `place-items-center` keeps a
	     zoomed-out capture centred instead of pinned to the corner. -->
	<div
		bind:this={viewport}
		class="grid place-items-center overflow-auto rounded-lg border border-base-300 bg-base-200 {tall
			? 'min-h-0 flex-1'
			: 'max-h-[38vh]'}"
	>
		<canvas bind:this={canvas} class="block h-auto max-w-none touch-none"></canvas>
	</div>

	{#if tall}
		<p class="m-0 text-xs text-base-content/60">{t('feedback-annotate-hint')}</p>
	{/if}
</div>
