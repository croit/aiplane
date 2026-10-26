/**
 * The operator's welcome text with the signed-in person's data filled in.
 *
 * Supported placeholders:
 *   {name}        display name (the address's local part when there is none)
 *   {first_name}  first word of the display name
 *   {email}       the full address
 *   {language}    the interface language code (en, de, fr, es, ru, zh)
 *
 * One pass, so a substituted value can never be expanded again, and every
 * value is Markdown-escaped: a name is data, not markup. Unknown placeholders
 * are left verbatim so a typo stays visible instead of silently disappearing.
 */
export interface WelcomeUser {
	name: string | null | undefined;
	email: string | null | undefined;
	language: string;
}

const PLACEHOLDER = /\{(name|first_name|email|language)\}/g;

export function renderWelcome(text: string, user: WelcomeUser): string {
	const email = (user.email ?? '').trim();
	const display = (user.name ?? '').trim() || email.split('@')[0]?.trim() || '';
	const first = display.split(/[\s._-]+/).find(Boolean) ?? '';
	const values: Record<string, string> = {
		name: display,
		first_name: first,
		email,
		language: user.language
	};
	return text.replace(PLACEHOLDER, (whole, key: string) =>
		key in values ? escapeMarkdown(values[key]) : whole
	);
}

/** CommonMark's escapable punctuation, so a substituted value stays text. */
function escapeMarkdown(value: string): string {
	return value.replace(/[\\`*_{}[\]()#+\-.!|>~]/g, (character) => `\\${character}`);
}
