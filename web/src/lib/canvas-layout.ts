export interface CanvasBounds {
	minimum: number;
	maximum: number;
	preferred: number;
}

const MINIMUM_CANVAS_WIDTH = 320;
const MINIMUM_CHAT_WIDTH = 560;
const SPLIT_GAP = 12;
const MAXIMUM_CANVAS_WIDTH = 768;
const PREFERRED_CANVAS_WIDTH = 600;

export function canvasBounds(containerWidth: number): CanvasBounds {
	const maximum = Math.max(
		MINIMUM_CANVAS_WIDTH,
		Math.min(
			MAXIMUM_CANVAS_WIDTH,
			Math.floor(containerWidth * 0.46),
			Math.floor(containerWidth - MINIMUM_CHAT_WIDTH - SPLIT_GAP)
		)
	);
	const preferred = Math.min(PREFERRED_CANVAS_WIDTH, Math.round(containerWidth * 0.38));
	return {
		minimum: MINIMUM_CANVAS_WIDTH,
		maximum,
		preferred: Math.min(maximum, Math.max(MINIMUM_CANVAS_WIDTH, preferred))
	};
}

export function clampCanvasWidth(width: number, bounds: CanvasBounds): number {
	return Math.min(bounds.maximum, Math.max(bounds.minimum, Math.round(width)));
}
