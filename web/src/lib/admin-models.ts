export type ModelKind = 'chat' | 'embedding' | 'image' | 'speech' | 'transcription' | 'ocr' | 'rerank' | 'system_one';
export type ModelFilter = 'all' | 'chat' | 'other' | 'alias' | 'configured';
export type PricingUnit = 'tokens' | 'images' | 'characters' | 'seconds';
export const MODEL_ADMIN_TABS = ['upstreams', 'catalog', 'defaults', 'routing'] as const;
export type ModelAdminTab = (typeof MODEL_ADMIN_TABS)[number];

export function selectedModelAdminTab(search: string): ModelAdminTab {
	const selected = new URLSearchParams(search).get('tab');
	return MODEL_ADMIN_TABS.find((tab) => tab === selected) ?? 'catalog';
}

export interface ModelCapabilities {
	vision?: boolean | null;
	audio_input?: boolean | null;
	pdf_input?: boolean | null;
	tools?: boolean | null;
	parallel_tools?: boolean | null;
	structured_output?: boolean | null;
	fallback_vision?: string | null;
	fallback_tools?: string | null;
}

export interface ModelDefaults {
	defaults_toml: string;
	reasoning_style: string | null;
	context_window: number | null;
	input_price: number | null;
	output_price: number | null;
	pricing_unit: PricingUnit;
	budget_standard: number | null;
	budget_deep: number | null;
	budget_max: number | null;
	effort_standard: string | null;
	effort_deep: string | null;
	effort_max: string | null;
	capabilities: ModelCapabilities;
}

export interface AdminModel {
	name: string;
	kind: ModelKind;
	alias_target: string | null;
	configured: boolean;
	resolved_reasoning_style: string;
	uses_token_budget: boolean;
	effort_levels: string[];
	/**
	 * The context window the serving backend reports, already capped by what
	 * it says it actually allocated. `null` when nothing serves this model, or
	 * when the server does not report one (Ollama, most hosted providers).
	 */
	detected_context_window: number | null;
	defaults: ModelDefaults | null;
}

/**
 * What to say beneath the context-window field.
 *
 * The value is discovered where the backend reports one, and the operator may
 * always override it. Lowering is legitimate — capping a model to save VRAM —
 * and passes without comment. Raising it above what the server actually serves
 * is the one case worth interrupting for: nothing here compacts the prompt,
 * and the server truncates it without an error, which is indistinguishable
 * from a model that has simply forgotten the start of the conversation.
 *
 * `null` means say nothing.
 */
export interface ContextHint {
	tone: 'info' | 'warning';
	key: 'admin-context-detected' | 'admin-context-unreported' | 'admin-context-exceeds-detected';
	window?: number;
}

/**
 * `entered` is deliberately not typed `string`.
 *
 * The field is `<input type="number">` with `bind:value`, and Svelte coerces
 * that binding to a number on every edit — so the state starts as the string
 * the row was seeded with and becomes `number | null` the moment anyone types.
 * Declaring `string` compiled, passed its tests (which only ever handed it
 * string literals) and threw `entered.trim is not a function` inside a
 * `$derived` on the first keystroke, taking the editor down with it.
 */
export function contextWindowHint(
	entered: string | number | null | undefined,
	detected: number | null
): ContextHint | null {
	// Where the number would come from if the operator supplied none. Also the
	// answer whenever their value is at or below it, and whenever there is
	// nothing to compare against.
	const provenance: ContextHint =
		detected === null
			? { tone: 'info', key: 'admin-context-unreported' }
			: { tone: 'info', key: 'admin-context-detected', window: detected };

	const typed = String(entered ?? '').trim();
	if (typed === '') return provenance;

	const value = Number(typed);
	// Not a usable window: say nothing rather than guess what was meant.
	if (!Number.isFinite(value) || value <= 0) return null;

	return detected !== null && value > detected
		? { tone: 'warning', key: 'admin-context-exceeds-detected', window: detected }
		: provenance;
}



export interface FeatureDefault {
	feature: string;
	model: string | null;
	available: string[];
}

export interface AdminModelsData {
	models: AdminModel[];
	all_models: string[];
	currency: string;
	feature_defaults: FeatureDefault[];
	search: { provider: string; searxng_url: string | null; brave_key_set: boolean; tavily_key_set: boolean; tavily_enabled: boolean; tavily_active: boolean };
}

export function matchesModelFilter(model: AdminModel, filter: ModelFilter, query: string): boolean {
	const group = model.alias_target !== null ? 'alias' : model.kind === 'chat' ? 'chat' : 'other';
	const matchesKind = filter === 'all' || filter === group || (filter === 'configured' && model.configured);
	return matchesKind && model.name.toLowerCase().includes(query.trim().toLowerCase());
}

export function configuredFacets(model: AdminModel): string[] {
	const defaults = model.defaults;
	if (!defaults) return [];
	const capabilities = defaults.capabilities;
	const facets: string[] = [];
	if (defaults.input_price !== null || defaults.output_price !== null) facets.push('price');
	if (defaults.context_window !== null) facets.push('context');
	if ([defaults.budget_standard, defaults.budget_deep, defaults.budget_max, defaults.effort_standard, defaults.effort_deep, defaults.effort_max].some((value) => value !== null)) facets.push('budget');
	if (Object.values(capabilities).some((value) => value !== null && value !== undefined && value !== '')) facets.push('capabilities');
	if (defaults.defaults_toml.trim()) facets.push('toml');
	return facets;
}

export function pricingUnitFor(model: AdminModel): PricingUnit {
	if (model.defaults) return model.defaults.pricing_unit;
	if (model.kind === 'image') return 'images';
	if (model.kind === 'speech') return 'characters';
	if (model.kind === 'transcription') return 'seconds';
	return 'tokens';
}
