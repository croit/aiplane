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
