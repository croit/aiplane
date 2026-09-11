export interface RagCollection {
	id: number;
	name: string;
	description: string | null;
	git_url: string;
	git_ref: string;
	pat_set: boolean;
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
