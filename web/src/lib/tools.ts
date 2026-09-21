import { t } from './i18n.svelte';

export interface ToolEntry {
	key: string;
	title: string;
	tech: string;
	description: string;
	category: string;
	enabled: boolean;
}

export interface LocationSharingState {
	shared: boolean;
	accuracy: number | null;
}

export interface ToolsResponse {
	tools: ToolEntry[];
	location: LocationSharingState | null;
}

export interface BrowserLocation {
	lat: number;
	lon: number;
	accuracy: number | null;
}

export function currentBrowserLocation(): Promise<BrowserLocation> {
	return new Promise((resolve, reject) => {
		if (!navigator.geolocation) {
			reject(new Error('geolocation unavailable'));
			return;
		}
		navigator.geolocation.getCurrentPosition(
			(position) => resolve({
				lat: position.coords.latitude,
				lon: position.coords.longitude,
				accuracy: Number.isFinite(position.coords.accuracy) ? position.coords.accuracy : null
			}),
			reject,
			{ enableHighAccuracy: true, timeout: 15_000, maximumAge: 0 }
		);
	});
}

/**
 * Heading for a tool-catalog section, from the stable slug the server sends.
 *
 * Every surface that groups by category resolves it here — `/tools`, the
 * per-token capability panel, the chat capability picker and the admin grant
 * matrix — so they cannot drift apart or fall back to English one at a time.
 * The slug is `Category::key()`; an unknown one renders as itself rather than
 * blank, which is what a newly added category looks like before its key lands.
 */
export function toolCategoryLabel(key: string): string {
	const label = t(`tool-category-${key}`);
	return label === `tool-category-${key}` ? key : label;
}
