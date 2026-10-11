export const LOCALES = ['en', 'de', 'fr', 'es', 'ru', 'zh'] as const;
export type Locale = (typeof LOCALES)[number];
export const LANG_COOKIE = 'lang';

function isLocale(v: string | undefined | null): v is Locale {
	return !!v && (LOCALES as readonly string[]).includes(v);
}

export function detect(): Locale {
	if (typeof document === 'undefined') return 'en';
	const cookie = document.cookie.match(/(?:^|;\s*)lang=([a-zA-Z-]+)/)?.[1];
	if (isLocale(cookie)) return cookie;
	for (const tag of navigator.languages ?? []) {
		const primary = tag.split('-')[0]?.toLowerCase();
		if (isLocale(primary)) return primary;
	}
	return 'en';
}
