/**
 * The SPA's translation layer.
 *
 * The product is multilingual and always has been — six languages, carried
 * over from the server-rendered stack's Fluent catalogs (which still serve the
 * strings the *server* generates: tool prompts, OAuth error pages, proxy
 * errors). This module is the client half.
 *
 * # Why only two catalogs load
 *
 * The corpus is ~1770 messages, which is 33–48 KB gzipped per language. All
 * six at once is ~214 KB of JavaScript on the critical path, and five of them
 * would be text nobody in the tab can read — so each language is its own
 * chunk, and a session loads English (the fallback `t()` needs anyway) plus
 * the one the reader actually chose.
 *
 * They are still chunks of the build rather than fetched data: the service
 * worker precaches them with everything else, so a switch works offline, and
 * `+layout.ts` awaits the active one before the first route renders. That
 * await is the point — resolving it lazily would paint the app in English and
 * then swap it, which is worse than a few milliseconds of nothing.
 *
 * # Which language
 *
 * The `lang` cookie first, then `navigator.languages`, then English. The
 * cookie is the same one the Rust side reads (`session_core::i18n::LANG_COOKIE`),
 * so a server-rendered error page and the app agree on the language — the two
 * halves must not disagree in front of the user.
 *
 * # Missing keys
 *
 * `t()` falls back to English, then to the key itself. It never throws and
 * never renders empty: a missing translation should degrade to a readable
 * English string, not to a blank button. The `i18n-drift` test keeps the
 * catalogs in step so this stays a safety net rather than a strategy.
 */
import { en } from './locales/en';

/**
 * A catalog entry: a message with `{name}` placeholders, or a set of variants.
 *
 * Variants cover the two Fluent selectors this corpus uses. For a count, the
 * keys are `Intl.PluralRules` categories and the right one is chosen from
 * `args.count` — which is why Russian keeps `one`/`few`/`many` rather than
 * being flattened to singular/plural, a distinction English does not have and
 * a naive `count === 1` would destroy. For anything else the variant is chosen
 * by `args.select`.
 */
export type Message = string | Record<string, string>;

/** A flat catalog: Fluent-style key → message. */
export type Catalog = Record<string, Message>;

export const LOCALES = ['en', 'de', 'fr', 'es', 'ru', 'zh'] as const;
export type Locale = (typeof LOCALES)[number];

/** Native names, for the switcher — a language is listed as its speakers write it. */
export const LOCALE_NAMES: Record<Locale, string> = {
	en: 'English',
	de: 'Deutsch',
	fr: 'Français',
	es: 'Español',
	ru: 'Русский',
	zh: '中文'
};

/**
 * How to fetch each catalog. Written out one literal `import()` per language
 * rather than an `import(`./locales/${l}`)` template, because the bundler can
 * only split what it can see statically — a computed specifier makes it give
 * up and inline all six again, which is the thing this exists to avoid.
 */
const LOADERS: Record<Locale, () => Promise<{ [k: string]: Catalog }>> = {
	en: async () => ({ en }),
	de: () => import('./locales/de'),
	fr: () => import('./locales/fr'),
	es: () => import('./locales/es'),
	ru: () => import('./locales/ru'),
	zh: () => import('./locales/zh')
};

/** English is always here: it is the fallback every other language needs. */
const CATALOGS: Partial<Record<Locale, Catalog>> = { en };

/**
 * Load a catalog, once. Awaited by `+layout.ts` for the initial language and
 * by `setLocale` for a switch, so `t()` itself can stay synchronous — a
 * translation function that returned a promise would infect every call site
 * in the app.
 */
export async function loadCatalog(which: Locale): Promise<void> {
	if (CATALOGS[which]) return;
	try {
		CATALOGS[which] = (await LOADERS[which]())[which];
	} catch {
		// A chunk that will not load (offline before the service worker
		// primed, a bad deploy) must not take the app down with it: English
		// is already in memory, and `t()` falls back to it.
	}
}

export const LANG_COOKIE = 'lang';

function isLocale(v: string | undefined | null): v is Locale {
	return !!v && (LOCALES as readonly string[]).includes(v);
}

/** The cookie, then the browser's preference list, then English. */
export function detect(): Locale {
	if (typeof document === 'undefined') return 'en';
	const cookie = document.cookie.match(/(?:^|;\s*)lang=([a-zA-Z-]+)/)?.[1];
	if (isLocale(cookie)) return cookie;
	for (const tag of navigator.languages ?? []) {
		// `de-AT` and `de` are the same catalog to us; match the primary subtag.
		const primary = tag.split('-')[0]?.toLowerCase();
		if (isLocale(primary)) return primary;
	}
	return 'en';
}

export const locale = $state<{ current: Locale }>({ current: detect() });

/**
 * Switch language and remember it.
 *
 * A cookie rather than `localStorage` because the server reads it too, and
 * `document.documentElement.lang` is updated so screen readers and the
 * browser's own spellcheck follow along.
 *
 * The catalog is awaited before `locale.current` moves, so the app never
 * renders a half-translated frame — every `t()` in the tree re-runs the
 * instant that assignment lands, and the messages have to be there by then.
 */
export async function setLocale(next: Locale): Promise<void> {
	await loadCatalog(next);
	locale.current = next;
	document.cookie = `${LANG_COOKIE}=${next}; path=/; max-age=31536000; samesite=lax`;
	document.documentElement.lang = next;
}

/**
 * Translate `key`, interpolating `{name}` placeholders from `args`.
 *
 * Reading `locale.current` inside makes every call site reactive: a language
 * switch re-renders the whole app with no per-component subscription.
 */
export function t(key: string, args?: Record<string, string | number>): string {
	const entry = CATALOGS[locale.current]?.[key] ?? en[key];
	if (entry === undefined) return key;

	const message = typeof entry === 'string' ? entry : selectVariant(entry, args);
	if (!args) return message;
	return message.replace(/\{(\w+)\}/g, (whole, name: string) =>
		name in args ? String(args[name]) : whole
	);
}

/**
 * Pick a variant: by plural category when there is a count, by `select`
 * otherwise. Falls back to `other` — Fluent's default variant — and then to
 * any variant at all, so a catalog missing the exact category still renders
 * words rather than nothing.
 */
function selectVariant(variants: Record<string, string>, args?: Record<string, string | number>): string {
	if (args && 'count' in args) {
		const category = new Intl.PluralRules(locale.current).select(Number(args.count));
		if (variants[category]) return variants[category];
	}
	const selector = args?.select;
	if (typeof selector === 'string' && variants[selector]) return variants[selector];
	return variants.other ?? Object.values(variants)[0] ?? '';
}

/**
 * A number formatted for the active locale.
 *
 * Kept here so pages do not each reach for `toLocaleString` with a different
 * locale argument — the point of the switcher is that *everything* follows it,
 * including the thousands separator.
 */
export function n(value: number, options?: Intl.NumberFormatOptions): string {
	return new Intl.NumberFormat(locale.current, options).format(value);
}

/** A timestamp formatted for the active locale. Accepts what the API returns. */
export function dt(value: string | number | Date, options?: Intl.DateTimeFormatOptions): string {
	const date = value instanceof Date ? value : new Date(value);
	if (Number.isNaN(date.getTime())) return '';
	return new Intl.DateTimeFormat(locale.current, options ?? {
		dateStyle: 'medium',
		timeStyle: 'short'
	}).format(date);
}

/** Just the clock part — chat turns are stamped with the time alone. */
export function time(value: string | null | undefined): string {
	if (!value) return '';
	const date = new Date(value);
	if (Number.isNaN(date.getTime())) return '';
	return new Intl.DateTimeFormat(locale.current, {
		hour: '2-digit',
		minute: '2-digit'
	}).format(date);
}
