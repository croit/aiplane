export const SETTINGS_CATEGORIES = ['chat', 'tools', 'data', 'access', 'notifications', 'web-search'] as const;
export type SettingsCategory = (typeof SETTINGS_CATEGORIES)[number];
export type SettingsFieldKind =
	| 'bool'
	| 'int'
	| 'float'
	| 'text'
	| 'path'
	| 'model'
	| 'secret'
	| 'list'
	| 'choice';

export interface AdminSettingsField {
	key: string;
	kind: SettingsFieldKind;
	span: 'full' | 'half';
	restart: boolean;
	value: string | null;
	secret_set: boolean;
	models: string[];
	model_options: Array<{ id: string; gdpr: boolean; nda: boolean }>;
	/**
	 * The closed option set of a `choice` field; empty for every other kind.
	 * Only the identifiers travel — each option's label is a catalog key the
	 * SPA derives (`settings-f-<key>-opt-<value>`), so a stored value stays
	 * stable while its wording is translated.
	 */
	choices: string[];
}

export interface AdminSettingsSection {
	name: string;
	category: SettingsCategory;
	enabled: boolean | null;
	fields: AdminSettingsField[];
}

export interface AdminSettingsData {
	sections: AdminSettingsSection[];
	restart_pending: string[];
	needs_backend: boolean;
}

export function selectedSettingsCategory(search: string): SettingsCategory {
	const selected = new URLSearchParams(search).get('tab');
	return SETTINGS_CATEGORIES.find((category) => category === selected) ?? 'chat';
}

export function categorySummary(sections: Pick<AdminSettingsSection, 'enabled'>[]): { on: number; switchable: number } {
	const switchable = sections.filter((section) => section.enabled !== null);
	return { on: switchable.filter((section) => section.enabled).length, switchable: switchable.length };
}

export function fieldDraft(field: Pick<AdminSettingsField, 'kind' | 'value'>): string {
	if (field.kind === 'bool') return field.value === 'true' ? 'true' : 'false';
	if (field.kind === 'list' && field.value) {
		try {
			const items: unknown = JSON.parse(field.value);
			if (Array.isArray(items) && items.every((item) => typeof item === 'string')) return items.join(', ');
		} catch {}
	}
	return field.value ?? '';
}

export function settingsCatalogKey(prefix: string, path: string): string {
	return `${prefix}${path.replaceAll('.', '-')}`;
}
