export interface ComfyuiParam {
	key: string;
	description: string;
	required: boolean;
}

export interface ComfyuiWorkflow {
	id: string;
	tool_id: string;
	title: string;
	description: string;
	output_kind: string;
	output_node_id: string;
	filename_prefix: string;
	params: ComfyuiParam[];
}

export interface ComfyuiJob {
	id: number;
	prompt_id: string;
	workflow_id: string;
	status: string;
	error_message: string | null;
	output_filename: string | null;
	created_at: string;
	completed_at: string | null;
}

export interface ComfyuiCatalog {
	configured: boolean;
	base_url: string | null;
	content_dir: string | null;
	timeout_secs: number | null;
	queue_poll_interval_ms: number | null;
	max_concurrent_jobs: number | null;
	workflows: ComfyuiWorkflow[];
	jobs: ComfyuiJob[];
}

export function comfyuiJobPresentation(status: string): { icon: string; badge: string } {
	switch (status) {
		case 'pending': return { icon: '⏳', badge: 'badge-warning' };
		case 'completed': return { icon: '✅', badge: 'badge-success' };
		case 'failed':
		case 'timeout': return { icon: '⚠️', badge: 'badge-error' };
		default: return { icon: '•', badge: 'badge-ghost' };
	}
}

export function shortPromptId(promptId: string): string {
	return promptId.length > 12 ? `${promptId.slice(0, 12)}…` : promptId;
}
