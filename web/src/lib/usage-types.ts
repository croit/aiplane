/** Wire shapes for GET /api/v0/usage (mirrors `db::usage` + the handler's assembly). */
export interface UsageSummary {
	requests: number;
	total_tokens: number;
	total_cost: number;
	unique_users: number;
	errors: number;
}

export interface UsageGroup {
	key: string;
	label: string;
	requests: number;
	total_tokens: number;
	input_units: number;
	output_units: number;
	cost: number;
	errors: number;
}

export interface UsageLimit {
	model: string | null;
	dimension: string;
	window: string;
	limit: number;
	used: number;
	refreshes_at: string;
}

export interface UsageResponse {
	period: string;
	scope: 'all' | 'self';
	currency: string;
	summary: UsageSummary;
	by_user: UsageGroup[];
	by_token: UsageGroup[];
	by_backend: UsageGroup[];
	by_source: UsageGroup[];
	by_model: UsageGroup[];
	backends: string[];
	tokens: { id: string; label: string }[];
	limits: UsageLimit[];
	unpriced_models: string[];
}
