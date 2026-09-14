/**
 * Optional features, and the routes that only exist when they are on.
 *
 * An operator who turns ComfyUI off at `/admin/settings` should not keep a
 * ComfyUI entry in the sidebar, and someone who follows an old link to it
 * should be told the feature is off — not shown an empty page or an API
 * error, which is indistinguishable from a broken deployment.
 *
 * Names are the `/admin/settings` section names, so the switch an operator
 * flipped and the thing that disappeared have the same name. `GET
 * /api/v0/me` carries the enabled list (`Me.features`).
 */

/** Route prefix → the feature that has to be on for it to exist. */
export const FEATURE_ROUTES: readonly (readonly [string, string])[] = [
	['/admin/comfyui', 'comfyui'],
	['/admin/limits', 'limits'],
	['/admin/skills', 'skills'],
	['/rag', 'rag'],
	['/skills', 'skills'],
	['/usage', 'usage']
] as const;

/**
 * The feature `pathname` needs, or `null` when it needs none.
 *
 * Longest prefix wins, and a prefix only matches on a whole path segment —
 * `/skills` must not claim `/admin/skills` (they happen to share a feature,
 * but `/tokens` and `/admin/tokens` are the shape this rule exists for).
 */
export function featureForRoute(pathname: string, base = ''): string | null {
	const path = base && pathname.startsWith(base) ? pathname.slice(base.length) || '/' : pathname;
	let best: { prefix: string; feature: string } | null = null;
	for (const [prefix, feature] of FEATURE_ROUTES) {
		if (path !== prefix && !path.startsWith(`${prefix}/`)) continue;
		if (!best || prefix.length > best.prefix.length) best = { prefix, feature };
	}
	return best?.feature ?? null;
}

/**
 * Is `feature` on?
 *
 * `undefined` — a gateway too old to report the list — means "no gating", so
 * an upgrade mismatch shows too much rather than hiding a page that works.
 */
export function featureEnabled(features: string[] | undefined, feature: string): boolean {
	return features === undefined || features.includes(feature);
}

/** Filter nav entries down to the ones whose feature is on. */
export function visibleNavLinks<T extends readonly [string, string, ...unknown[]]>(
	links: readonly T[],
	features: string[] | undefined
): T[] {
	return links.filter((link) => {
		const feature = featureForRoute(link[1]);
		return feature === null || featureEnabled(features, feature);
	});
}
