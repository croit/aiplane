export interface RagCollection {
	id: number;
	name: string;
	description: string | null;
	git_url: string;
	git_ref: string;
	pat_set: boolean;
	pat_hint: string | null;
	source_kind: string;
	source_config: Record<string, string>;
	source_secrets_set: boolean;
	sync_hook_set: boolean;
	connected_account: string | null;
	connected_by: string | null;
	connected_at: string | null;
	profile_id: number | null;
	extraction_model: string | null;
	embedding_model: string;
	include_globs: string[];
	exclude_globs: string[];
	chunk_size: number;
	chunk_overlap: number;
	search_mode: 'versioned' | 'aggregate';
	/** Minutes between automatic re-syncs; 0 = only on demand. */
	refresh_interval_mins: number;
	allowed_groups: string[];
	status: string;
	last_indexed_at: string | null;
	last_indexed_commit: string | null;
	last_error: string | null;
}

export interface RagRef {
	id: number;
	collection_id: number;
	git_ref: string;
	git_url: string | null;
	is_primary: boolean;
	status: string;
	last_indexed_at: string | null;
	last_indexed_commit: string | null;
	last_error: string | null;
	chunk_count: number;
	document_count: number;
	source_status: string | null;
}

export interface RagLogEntry {
	id: number;
	created_at: string;
	level: 'info' | 'warn' | 'error';
	phase: string;
	message: string;
	commit_sha: string | null;
	files: number | null;
	chunks: number | null;
	duration_ms: number | null;
}

export interface RagProviderField {
	key: string;
	label: string;
	help?: string | null;
	required: boolean;
	secret: boolean;
	default?: string | null;
}

export interface RagProvider {
	kind: string;
	label: string;
	description: string;
	auth: { kind: 'fields' | 'oauth2'; connect_path?: string };
	fields: RagProviderField[];
}

export interface RagProfileField {
	key: string;
	label: string;
	type: 'text' | 'number' | 'date' | 'enum';
	description?: string;
	values?: string[];
	filterable?: boolean;
	sortable?: boolean;
}

export interface RagProfile {
	id: number;
	name: string;
	description: string | null;
	prompt: string;
	version: number;
	builtin: boolean;
	fields: RagProfileField[];
}

export function parseList(value: string): string[] {
	return value
		.split(/[\n,]/)
		.map((entry) => entry.trim())
		.filter(Boolean);
}

export function parseSources(value: string): { url: string; git_ref: string | null }[] {
	return value
		.split('\n')
		.map((line) => line.trim())
		.filter((line) => line && !line.startsWith('#'))
		.map((line) => {
			const [url, ref] = line.split(/\s+/, 2);
			return { url, git_ref: ref ? ref.replace(/^@/, '') : null };
		});
}

export function profileFieldsJson(fields: RagProfileField[]): string {
	return JSON.stringify(fields, null, 2);
}

export function sourceLabel(url: string): string {
	const trimmed = url.replace(/\/$/, '');
	return (trimmed.split(/[/:]/).pop() || trimmed).replace(/\.git$/, '');
}

/** The confirmation the collection list shows after the editor redirects back to it. */
/** The auto re-sync presets the editor offers, in minutes. */
export const REFRESH_PRESETS = [0, 60, 1440, 10080];

/**
 * How often a collection re-syncs, as a catalog key. A value stored by
 * someone scripting the API (say 720) is named in minutes rather than
 * rounded to the nearest preset.
 */
export function refreshIntervalLabel(mins: number): { key: string; args: Record<string, string> } {
	switch (mins) {
		case 0:
			return { key: 'rag-refresh-never', args: {} };
		case 60:
			return { key: 'rag-refresh-hourly', args: {} };
		case 1440:
			return { key: 'rag-refresh-daily', args: {} };
		case 10080:
			return { key: 'rag-refresh-weekly', args: {} };
		default:
			return { key: 'rag-refresh-custom', args: { mins: String(mins) } };
	}
}

export function listNotice(params: Pick<URLSearchParams, 'get'>): { key: string; args: Record<string, string> } | null {
	const name = params.get('name') ?? '';
	const ref = params.get('ref');
	switch (params.get('notice')) {
		case 'created-aggregate':
			return { key: 'rag-toast-created-aggregate', args: { name } };
		case 'queued':
			return ref ? { key: 'rag-toast-indexing-queued', args: { name, ref } } : { key: 'rag-toast-source-indexing-queued', args: { name } };
		case 'saved':
			return { key: 'rag-toast-collection-saved', args: { name } };
		default:
			return null;
	}
}
