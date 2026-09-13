/**
 * Canvas screenshot annotator for the feedback widget.
 *
 * The captured screenshot is drawn into a `<canvas>` at its intrinsic
 * (full-resolution) pixel size and displayed CSS-scaled; the user draws
 * rectangle / arrow / freehand / text / redaction shapes over it. Everything
 * is redrawn from a flat list of shapes each frame, with undo/redo history.
 *
 * Coordinate mapping is the load-bearing bit:
 *
 *     x = (clientX - rect.left) * canvas.width / rect.width
 *
 * `canvas.width` is the screenshot's natural pixel width; `rect.width` is the
 * displayed CSS width. The ratio absorbs both the device-pixel-ratio
 * supersampling and the CSS zoom, so stored points are always in intrinsic
 * image-pixel space and the export is 1:1.
 *
 * Framework-free on purpose: it owns a canvas and a pile of pointer state,
 * which is exactly the kind of thing that gets worse when it is rebuilt as
 * reactive state. The Svelte component hands it a canvas and calls methods.
 */

export type AnnotatorTool = 'pan' | 'rect' | 'arrow' | 'pen' | 'text' | 'redact';

interface Point {
	x: number;
	y: number;
}

interface Shape {
	tool: AnnotatorTool;
	color: string;
	width: number;
	points?: Point[];
	start?: Point;
	end?: Point;
	text?: string;
	font?: number;
}

export interface Annotator {
	loadDataUrl(dataUrl: string): Promise<void>;
	hasImage(): boolean;
	setTool(t: AnnotatorTool): void;
	getTool(): AnnotatorTool;
	setColor(c: string): void;
	getColor(): string;
	undo(): void;
	redo(): void;
	canUndo(): boolean;
	canRedo(): boolean;
	/** True once at least one shape has been drawn on the current image. */
	hasAnnotations(): boolean;
	clearAnnotations(): void;
	setZoom(z: number): void;
	/** Multiply the current zoom — what the +/− buttons and ctrl+wheel use. */
	zoomBy(factor: number): void;
	getZoom(): number;
	/** Recompute the fit baseline after the viewport box changed size. */
	refit(): void;
	/**
	 * Toggle between the two useful presets: the whole capture visible, and
	 * the capture filling the box's width (taller than the box, so you read it
	 * by scrolling — which is what you want on a long page).
	 */
	cycleZoomPreset(): void;
	/** Which preset, if either, the current zoom is sitting on. */
	zoomMode(): ZoomMode;
	reset(): void;
	/** PNG data URL of the image + annotations, or null when no image. */
	toDataUrl(): string | null;
	/** Fired after any state change (shape added, undo, tool, colour, zoom). */
	onChange(cb: () => void): void;
	/** Detach the pointer listeners. Call when the canvas goes away. */
	destroy(): void;
}

export const ANNOTATOR_COLORS = ['#ef4444', '#3b82f6', '#10b981', '#f59e0b', '#ffffff'];

/**
 * Zoom is a multiple of the *fit* size — the scale at which the whole capture
 * is visible in the viewport box — not a percentage of the container width.
 * That is what makes 100% mean "I can see everything" on a portrait phone
 * capture as well as a wide desktop one.
 *
 * The ceiling is high because the fit size of a tall screenshot inside a short
 * box is small, and redacting a single line of text in it needs real
 * magnification. Steps are multiplicative for the same reason: +0.25 at a time
 * would be a dozen clicks to get anywhere.
 */
export const ZOOM_MIN = 0.25;
export const ZOOM_MAX = 8;
export const ZOOM_FACTOR = 1.25;

/**
 * How the text tool asks for its content. A parameter rather than a direct
 * `window.prompt` call so the component supplies the *translated* wording
 * (and so the module stays testable without a DOM prompt).
 */
export type TextPrompt = () => string | null;

/** `fit` = whole capture visible, `width` = as wide as the box, else `custom`. */
export type ZoomMode = 'fit' | 'width' | 'custom';

/**
 * @param canvas   the drawing surface; its CSS width is owned by this module.
 * @param viewport the scrolling box the canvas sits in. Zoom is measured
 *                 against it, and panning moves its scroll offsets — which is
 *                 why it is a required parameter rather than
 *                 `canvas.parentElement`: a zoomed annotator you cannot pan is
 *                 no better than no zoom at all.
 */
export function createAnnotator(
	canvas: HTMLCanvasElement,
	viewport: HTMLElement,
	promptForText: TextPrompt = () => window.prompt('Annotation text')
): Annotator {
	const ctx = canvas.getContext('2d');
	let img: HTMLImageElement | null = null;
	let tool: AnnotatorTool = 'rect';
	let color = ANNOTATOR_COLORS[0]!;
	let shapes: Shape[] = [];
	let history: Shape[][] = [[]];
	let historyIndex = 0;
	let current: Shape | null = null;
	let drawing = false;
	let zoom = 1;
	/** Scroll offsets + pointer origin of an in-flight pan drag. */
	let panning: { x: number; y: number; left: number; top: number } | null = null;
	let changeCb: (() => void) | null = null;

	const notify = (): void => {
		if (changeCb) changeCb();
	};

	const clampZoom = (z: number): number => Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, z));

	/**
	 * Displayed width at which the whole capture fits inside the viewport —
	 * limited by the box's height as well as its width, so a tall portrait
	 * capture is fully visible rather than cropped to a strip.
	 */
	const fitWidth = (): number => {
		if (!img || !img.naturalHeight) return viewport.clientWidth;
		const byWidth = viewport.clientWidth;
		const byHeight = (viewport.clientHeight * img.naturalWidth) / img.naturalHeight;
		return Math.max(1, Math.min(byWidth, byHeight));
	};

	// The canvas keeps its aspect ratio from the width/height attributes, so
	// setting the CSS width alone scales it correctly in both axes.
	const applyZoom = (): void => {
		if (!img) return;
		canvas.style.width = `${Math.round(fitWidth() * zoom)}px`;
	};

	/**
	 * Zoom about the middle of the viewport. Without this the browser keeps
	 * the scroll offsets, so zooming in walks towards the top-left corner and
	 * whatever you were looking at is gone.
	 */
	const zoomTo = (z: number): void => {
		const next = clampZoom(z);
		if (next === zoom) return;
		const prevW = canvas.clientWidth || 1;
		const prevH = canvas.clientHeight || 1;
		const cx = (viewport.scrollLeft + viewport.clientWidth / 2) / prevW;
		const cy = (viewport.scrollTop + viewport.clientHeight / 2) / prevH;
		zoom = next;
		applyZoom();
		const w = canvas.clientWidth || 1;
		const h = canvas.clientHeight || 1;
		viewport.scrollLeft = cx * w - viewport.clientWidth / 2;
		viewport.scrollTop = cy * h - viewport.clientHeight / 2;
		notify();
	};

	/** The zoom at which the capture is exactly as wide as the viewport. */
	const widthZoom = (): number => {
		const fit = fitWidth();
		return fit > 0 ? viewport.clientWidth / fit : 1;
	};

	// A hair of tolerance: these are floating-point ratios of live element
	// sizes, and an exact comparison would flicker the button's label.
	const near = (a: number, b: number): boolean => Math.abs(a - b) < 0.01;

	const applyCursor = (): void => {
		canvas.style.cursor = tool === 'pan' ? 'grab' : 'crosshair';
	};

	// Stroke width / font scaled to the capture resolution, so annotations stay
	// visible on the CSS-downscaled preview instead of rendering hairline-thin
	// on a DPR-2/3 capture.
	const strokeWidth = (): number => Math.max(2, Math.round((canvas.width || 320) / 320));
	const fontSize = (): number => Math.max(14, Math.round((canvas.width || 600) / 45));

	// The arrowhead sits at the START point (where the pointer went down): you
	// press on the thing you are pointing at, then drag the tail away to where
	// there is room. Better than the head landing wherever you released.
	const drawArrow = (c: CanvasRenderingContext2D, s: Point, e: Point): void => {
		const head = Math.max(10, strokeWidth() * 4);
		const angle = Math.atan2(s.y - e.y, s.x - e.x);
		c.beginPath();
		c.moveTo(s.x, s.y);
		c.lineTo(e.x, e.y);
		c.stroke();
		c.beginPath();
		c.moveTo(s.x, s.y);
		c.lineTo(s.x - head * Math.cos(angle - Math.PI / 6), s.y - head * Math.sin(angle - Math.PI / 6));
		c.moveTo(s.x, s.y);
		c.lineTo(s.x - head * Math.cos(angle + Math.PI / 6), s.y - head * Math.sin(angle + Math.PI / 6));
		c.stroke();
	};

	const drawShape = (c: CanvasRenderingContext2D, s: Shape): void => {
		c.strokeStyle = s.color;
		c.fillStyle = s.color;
		c.lineWidth = s.width;
		c.lineCap = 'round';
		c.lineJoin = 'round';
		if (s.tool === 'redact' && s.start && s.end) {
			// Opaque fill so it actually censors — solid black, independent of
			// the stroke colour a lighter shade would leave readable.
			c.fillStyle = '#000000';
			c.fillRect(
				Math.min(s.start.x, s.end.x),
				Math.min(s.start.y, s.end.y),
				Math.abs(s.end.x - s.start.x),
				Math.abs(s.end.y - s.start.y)
			);
		} else if (s.tool === 'rect' && s.start && s.end) {
			c.strokeRect(s.start.x, s.start.y, s.end.x - s.start.x, s.end.y - s.start.y);
		} else if (s.tool === 'arrow' && s.start && s.end) {
			drawArrow(c, s.start, s.end);
		} else if (s.tool === 'pen' && s.points && s.points.length > 1) {
			c.beginPath();
			c.moveTo(s.points[0]!.x, s.points[0]!.y);
			for (let i = 1; i < s.points.length; i++) c.lineTo(s.points[i]!.x, s.points[i]!.y);
			c.stroke();
		} else if (s.tool === 'text' && s.start && s.text) {
			c.font = `${s.font ?? fontSize()}px sans-serif`;
			c.textBaseline = 'top';
			c.fillText(s.text, s.start.x, s.start.y);
		}
	};

	const redraw = (): void => {
		if (!ctx || !img) return;
		ctx.clearRect(0, 0, canvas.width, canvas.height);
		ctx.drawImage(img, 0, 0, canvas.width, canvas.height);
		for (const s of shapes) drawShape(ctx, s);
		if (current) drawShape(ctx, current);
	};

	const pushHistory = (next: Shape[]): void => {
		history = history.slice(0, historyIndex + 1);
		history.push(next);
		historyIndex = history.length - 1;
	};

	/**
	 * Keep tracking the stroke when the pointer drifts off the canvas — and
	 * never let a failure abort the stroke. `setPointerCapture` throws for a
	 * pointer id the browser does not consider active, which is exactly what a
	 * synthetic event (an automated test, an assistive tool) produces, and
	 * losing the whole drawing to that is not a trade worth making.
	 */
	const capturePointer = (pointerId: number): void => {
		try {
			canvas.setPointerCapture(pointerId);
		} catch {
			/* best-effort */
		}
	};

	const pos = (e: PointerEvent): Point => {
		const rect = canvas.getBoundingClientRect();
		return {
			x: ((e.clientX - rect.left) * canvas.width) / rect.width,
			y: ((e.clientY - rect.top) * canvas.height) / rect.height
		};
	};

	const onDown = (e: PointerEvent): void => {
		if (!img) return;
		e.preventDefault();
		// Pan, either with the hand tool or — whatever tool is selected — with
		// the middle button, which is what people reach for without thinking.
		if (tool === 'pan' || e.button === 1) {
			panning = {
				x: e.clientX,
				y: e.clientY,
				left: viewport.scrollLeft,
				top: viewport.scrollTop
			};
			capturePointer(e.pointerId);
			canvas.style.cursor = 'grabbing';
			return;
		}
		const p = pos(e);
		if (tool === 'text') {
			const text = promptForText();
			if (text) {
				shapes = [
					...shapes,
					{ tool: 'text', color, width: strokeWidth(), start: p, text, font: fontSize() }
				];
				pushHistory(shapes);
				redraw();
				notify();
			}
			return;
		}
		drawing = true;
		capturePointer(e.pointerId);
		current =
			tool === 'pen'
				? { tool, color, width: strokeWidth(), points: [p] }
				: { tool, color, width: strokeWidth(), start: p, end: p };
	};

	const onMove = (e: PointerEvent): void => {
		if (panning) {
			viewport.scrollLeft = panning.left - (e.clientX - panning.x);
			viewport.scrollTop = panning.top - (e.clientY - panning.y);
			return;
		}
		if (!drawing || !current) return;
		const p = pos(e);
		if (current.tool === 'pen') current.points!.push(p);
		else current.end = p;
		redraw();
	};

	const onUp = (): void => {
		if (panning) {
			panning = null;
			applyCursor();
			return;
		}
		if (!drawing || !current) return;
		shapes = [...shapes, current];
		pushHistory(shapes);
		current = null;
		drawing = false;
		redraw();
		notify();
	};

	// Ctrl/⌘ + wheel zooms, the gesture every image editor has trained people
	// to try. A bare wheel is left alone so the box still scrolls normally.
	const onWheel = (e: WheelEvent): void => {
		if (!img || !(e.ctrlKey || e.metaKey)) return;
		e.preventDefault();
		zoomTo(e.deltaY < 0 ? zoom * ZOOM_FACTOR : zoom / ZOOM_FACTOR);
	};

	canvas.addEventListener('pointerdown', onDown);
	canvas.addEventListener('pointermove', onMove);
	canvas.addEventListener('pointerup', onUp);
	canvas.addEventListener('pointercancel', onUp);
	canvas.addEventListener('wheel', onWheel, { passive: false });

	return {
		loadDataUrl(dataUrl: string): Promise<void> {
			return new Promise((resolve, reject) => {
				const image = new Image();
				image.onload = () => {
					img = image;
					canvas.width = image.naturalWidth;
					canvas.height = image.naturalHeight;
					shapes = [];
					history = [[]];
					historyIndex = 0;
					current = null;
					// A fresh capture always starts fully visible. Without this
					// the canvas renders at its raw capture size — three times
					// the viewport on a HiDPI screen — and the box just scrolls.
					zoom = 1;
					applyZoom();
					applyCursor();
					redraw();
					notify();
					resolve();
				};
				image.onerror = () => reject(new Error('image load failed'));
				image.src = dataUrl;
			});
		},
		hasImage: () => img !== null,
		setTool(t) {
			tool = t;
			applyCursor();
			notify();
		},
		getTool: () => tool,
		setColor(c) {
			color = c;
			notify();
		},
		getColor: () => color,
		undo() {
			if (historyIndex <= 0) return;
			historyIndex -= 1;
			shapes = [...(history[historyIndex] ?? [])];
			redraw();
			notify();
		},
		redo() {
			if (historyIndex >= history.length - 1) return;
			historyIndex += 1;
			shapes = [...(history[historyIndex] ?? [])];
			redraw();
			notify();
		},
		canUndo: () => historyIndex > 0,
		canRedo: () => historyIndex < history.length - 1,
		hasAnnotations: () => shapes.length > 0,
		clearAnnotations() {
			shapes = [];
			pushHistory([]);
			redraw();
			notify();
		},
		setZoom(z) {
			zoomTo(z);
		},
		zoomBy(factor) {
			zoomTo(zoom * factor);
		},
		getZoom: () => zoom,
		refit() {
			// The viewport changed size (the dialog grew into annotate mode, or
			// the window was resized): the fit baseline moved with it.
			applyZoom();
			notify();
		},
		zoomMode() {
			if (near(zoom, 1)) return 'fit';
			if (near(zoom, widthZoom())) return 'width';
			return 'custom';
		},
		cycleZoomPreset() {
			// From the fit view, fill the width; from anywhere else, back to fit.
			// Two clicks always return you to a known view.
			zoomTo(near(zoom, 1) ? widthZoom() : 1);
		},
		reset() {
			img = null;
			shapes = [];
			history = [[]];
			historyIndex = 0;
			current = null;
			zoom = 1;
			panning = null;
			// Back to "no explicit size" rather than 100%: the next image sets
			// its own fit width, and a stale one would make it flash wrong.
			canvas.style.width = '';
			if (ctx) ctx.clearRect(0, 0, canvas.width, canvas.height);
			notify();
		},
		toDataUrl() {
			if (!img) return null;
			// Make sure the latest state is rendered before exporting.
			redraw();
			return canvas.toDataURL('image/png');
		},
		onChange(cb) {
			changeCb = cb;
		},
		destroy() {
			canvas.removeEventListener('pointerdown', onDown);
			canvas.removeEventListener('pointermove', onMove);
			canvas.removeEventListener('pointerup', onUp);
			canvas.removeEventListener('pointercancel', onUp);
			canvas.removeEventListener('wheel', onWheel);
			changeCb = null;
		}
	};
}
