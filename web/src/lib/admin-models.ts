export type ModelKind = 'chat' | 'embedding' | 'image' | 'speech' | 'transcription' | 'ocr' | 'rerank';
export type ModelFilter = 'all' | 'chat' | 'other' | 'alias' | 'configured';
export type PricingUnit = 'tokens' | 'images' | 'characters' | 'seconds';

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
	defaults: ModelDefaults | null;
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
	search: { provider: string; searxng_url: string | null; brave_key_set: boolean };
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
