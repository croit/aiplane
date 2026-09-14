import type { SearchOption } from './searchable-select';

/**
 * The IANA timezone list, as picker options.
 *
 * Typing a timezone by hand is a guess at an exact spelling the platform
 * already knows: `Europe/Berlin` works, `Europe/Berlín`, `CEST` and `Berlin`
 * do not, and the schedule only reports that when the server rejects it. The
 * list comes from `Intl.supportedValuesOf('timeZone')` rather than a table in
 * this repo — it is the browser's own tzdata, so it stays current without
 * anyone maintaining it, and it is the same set the server's `jiff` resolves
 * against.
 *
 * Each option carries its current UTC offset, which is what people actually
 * recognise when the zone name is unfamiliar, and the offset is searchable
 * ("+02") alongside the city.
 */

/** Zone ids the platform knows, or a minimal set when it cannot say. */
export function supportedTimezones(): string[] {
	// `supportedValuesOf` is ES2022; a browser without it still gets a working
	// picker, just a short one — better than a 400-row table going stale here.
	const intl = Intl as typeof Intl & { supportedValuesOf?: (key: string) => string[] };
	try {
		const zones = intl.supportedValuesOf?.('timeZone');
		// The platform list is canonical zone ids only — it has no plain `UTC`
		// (just `Etc/…` aliases, and not even those on some engines), yet `UTC`
		// is what the gateway stores for a user who has no zone of their own.
		// Offer it, or that default would be the one value the picker cannot show.
		if (zones?.length) return ['UTC', ...zones];
	} catch {
		/* fall through to the minimal list */
	}
	const local = localTimezone();
	return [...new Set(['UTC', ...(local ? [local] : [])])];
}

/** The viewer's own zone, or `null` when the browser will not say. */
export function localTimezone(): string | null {
	try {
		return Intl.DateTimeFormat().resolvedOptions().timeZone || null;
	} catch {
		return null;
	}
}

/**
 * `zone`'s current UTC offset as `UTC+02:00`, or `null` if the zone is not
 * one the platform resolves. DST-correct, because it is computed for `at`
 * rather than read from a fixed table.
 */
export function timezoneOffset(zone: string, at: Date = new Date()): string | null {
	try {
		const parts = new Intl.DateTimeFormat('en-US', {
			timeZone: zone,
			timeZoneName: 'longOffset'
		}).formatToParts(at);
		const name = parts.find((part) => part.type === 'timeZoneName')?.value;
		if (!name) return null;
		// `longOffset` renders as `GMT+02:00`, and exactly `GMT` at zero.
		return name === 'GMT' ? 'UTC+00:00' : name.replace('GMT', 'UTC');
	} catch {
		return null;
	}
}

/**
 * Picker options for every known zone, alphabetical.
 *
 * `current` is always present even when the platform does not list it — a
 * stored value the browser has never heard of (an old alias like
 * `Asia/Calcutta`, or a zone added after this browser shipped) must stay
 * selectable, or opening the form would silently change the schedule.
 */
export function timezoneOptions(current: string, at: Date = new Date()): SearchOption[] {
	const zones = new Set(supportedTimezones());
	const trimmed = current.trim();
	if (trimmed) zones.add(trimmed);
	return [...zones]
		.sort((a, b) => a.localeCompare(b))
		.map((zone) => {
			const offset = timezoneOffset(zone, at);
			return {
				value: zone,
				label: zone.replaceAll('_', ' '),
				...(offset ? { description: offset } : {}),
				// The raw id keeps `America/New_York` findable by typing the
				// underscore form, and the offset digits make "+02" a query.
				keywords: [zone, ...(offset ? [offset, offset.replace('UTC', '')] : [])]
			};
		});
}
