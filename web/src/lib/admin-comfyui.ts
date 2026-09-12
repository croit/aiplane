/**
 * Data model + pure presentation logic for the two ComfyUI operator pages
 * (`/admin/comfyui` — the workflow catalog, `/admin/comfyui/jobs` — the run
 * history).
 *
 * Everything here is framework-free so it can be unit-tested without a DOM:
 * the Svelte components below only bind these results to markup.
 */

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

/**
 * One row of `comfyui_jobs`. `session_id`/`turn_id`/`user_id` have always
 * been in the API response; the page uses them to link a run back to the
 * conversation that asked for it.
 */
export interface ComfyuiJob {
	id: number;
	prompt_id: string;
	session_id: string;
	turn_id: string;
	user_id: string;
	workflow_id: string;
	output_kind: string;
	status: string;
	error_message: string | null;
	output_filename: string | null;
	output_mime: string | null;
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

export interface ComfyuiDevice {
	name: string;
	type: string;
	vram_total: number | null;
	vram_free: number | null;
}

export interface ComfyuiWorker {
	version: string | null;
	python_version: string | null;
	pytorch_version: string | null;
	ram_total: number | null;
	ram_free: number | null;
	devices: ComfyuiDevice[];
	queue_running: number;
	queue_pending: number;
}

/** `GET /api/v0/comfyui/health`. `reachable: false` is a normal answer. */
export interface ComfyuiHealth {
	reachable: boolean;
	base_url: string;
	error: string | null;
	worker: ComfyuiWorker | null;
}

/**
 * Catalog order. ComfyUI's own `output_kind` vocabulary is open-ended, so
 * anything unrecognised sorts after the three we know rather than being
 * dropped — an operator who adds a new kind still sees their workflows.
 */
export const COMFYUI_KINDS = ['image', 'video', 'audio'] as const;

export interface ComfyuiKindGroup {
	kind: string;
	workflows: ComfyuiWorkflow[];
}

export function groupWorkflowsByKind(workflows: ComfyuiWorkflow[]): ComfyuiKindGroup[] {
	const byKind = new Map<string, ComfyuiWorkflow[]>();
	for (const workflow of workflows) {
		const bucket = byKind.get(workflow.output_kind);
		if (bucket) bucket.push(workflow);
		else byKind.set(workflow.output_kind, [workflow]);
	}
	const rank = (kind: string) => {
		const known = (COMFYUI_KINDS as readonly string[]).indexOf(kind);
		return known === -1 ? COMFYUI_KINDS.length : known;
	};
	return [...byKind.entries()]
		.sort(([a], [b]) => rank(a) - rank(b) || a.localeCompare(b))
		.map(([kind, entries]) => ({
			kind,
			workflows: [...entries].sort((a, b) => a.id.localeCompare(b.id))
		}));
}

/**
 * Catalog search. Matches the tool id, title, description and parameter
 * keys — an operator debugging a model call usually arrives with a parameter
 * name ("which workflow takes `reference_audio`?"), not a title.
 */
export function filterWorkflows(workflows: ComfyuiWorkflow[], query: string): ComfyuiWorkflow[] {
	const needle = query.trim().toLowerCase();
	if (!needle) return workflows;
	return workflows.filter((workflow) =>
		[
			workflow.id,
			workflow.tool_id,
			workflow.title,
			workflow.description,
			...workflow.params.map((param) => param.key)
		].some((field) => field.toLowerCase().includes(needle))
	);
}

/**
 * The workflow a `?workflow=` query selects, else the first one *as the rail
 * lists it* — landing on an entry the eye cannot find in the list (catalog
 * order is not display order) reads as a bug.
 */
export function selectedWorkflow(
	workflows: ComfyuiWorkflow[],
	requested: string | null
): ComfyuiWorkflow | null {
	if (workflows.length === 0) return null;
	return (
		workflows.find((workflow) => workflow.id === requested) ??
		groupWorkflowsByKind(workflows)[0].workflows[0]
	);
}

export interface TextSpan {
	text: string;
	code: boolean;
}

/**
 * Parameter descriptions are written for the model and use markdown-style
 * backticks (`` `reference_audio` ``). Rendering them raw litters the table
 * with stray backticks; a full markdown parser would be far too much for one
 * inline form, so split on balanced pairs and let the caller style the spans.
 * An unbalanced backtick stays literal text — never swallow the rest.
 */
export function inlineCodeSpans(text: string): TextSpan[] {
	const spans: TextSpan[] = [];
	let rest = text;
	for (;;) {
		const open = rest.indexOf('`');
		const close = open === -1 ? -1 : rest.indexOf('`', open + 1);
		if (open === -1 || close === -1) break;
		if (open > 0) spans.push({ text: rest.slice(0, open), code: false });
		spans.push({ text: rest.slice(open + 1, close), code: true });
		rest = rest.slice(close + 1);
	}
	if (rest) spans.push({ text: rest, code: false });
	return spans;
}

export function requiredParams(workflow: ComfyuiWorkflow): ComfyuiParam[] {
	return workflow.params.filter((param) => param.required);
}

export type ComfyuiJobTone = 'success' | 'error' | 'warning' | 'neutral';

/**
 * One tone per job state, used for both the status chip and the row accent.
 * `pending` is a warning rather than an error: it is the normal state of a
 * running job, and only looks wrong once it outlives the timeout.
 */
export function jobTone(status: string): ComfyuiJobTone {
	switch (status) {
		case 'completed':
			return 'success';
		case 'failed':
		case 'timeout':
			return 'error';
		case 'pending':
			return 'warning';
		default:
			return 'neutral';
	}
}

export const JOB_FILTERS = ['all', 'completed', 'pending', 'failed'] as const;
export type ComfyuiJobFilter = (typeof JOB_FILTERS)[number];

/** `failed` is the operator's word for "did not produce an asset" — both
 *  ComfyUI failures and gateway timeouts belong under it. */
export function filterJobs(jobs: ComfyuiJob[], filter: ComfyuiJobFilter): ComfyuiJob[] {
	if (filter === 'all') return jobs;
	if (filter === 'failed') return jobs.filter((job) => jobTone(job.status) === 'error');
	return jobs.filter((job) => job.status === filter);
}

export function jobFilterCounts(jobs: ComfyuiJob[]): Record<ComfyuiJobFilter, number> {
	return {
		all: jobs.length,
		completed: filterJobs(jobs, 'completed').length,
		pending: filterJobs(jobs, 'pending').length,
		failed: filterJobs(jobs, 'failed').length
	};
}

/** Wall-clock milliseconds a job took, or null while it is still running. */
export function jobDurationMs(job: ComfyuiJob): number | null {
	if (!job.completed_at) return null;
	const started = Date.parse(job.created_at);
	const finished = Date.parse(job.completed_at);
	if (Number.isNaN(started) || Number.isNaN(finished)) return null;
	const elapsed = finished - started;
	return elapsed >= 0 ? elapsed : null;
}

/**
 * A duration an operator can compare at a glance: sub-minute work keeps one
 * decimal (`2.9 s` vs `5.8 s` is the difference between two runs), anything
 * longer drops to whole units because a GPU minute never needs milliseconds.
 * Deliberately unit-suffixed rather than localised — these sit in a
 * monospace column and must stay the same width in every language.
 */
export function formatDuration(ms: number | null): string {
	if (ms === null || !Number.isFinite(ms) || ms < 0) return '—';
	if (ms < 1000) return `${Math.round(ms)} ms`;
	const seconds = ms / 1000;
	if (seconds < 60) return `${seconds.toFixed(1)} s`;
	const whole = Math.round(seconds);
	const minutes = Math.floor(whole / 60);
	if (minutes < 60) return `${minutes}m ${String(whole % 60).padStart(2, '0')}s`;
	return `${Math.floor(minutes / 60)}h ${String(minutes % 60).padStart(2, '0')}m`;
}

/**
 * "14 min ago", in the viewer's language. `Intl.RelativeTimeFormat` owns the
 * wording so the six catalogs don't each need a plural rule for every unit.
 */
export function relativeTime(value: string, locale: string, now = Date.now()): string {
	const at = Date.parse(value);
	if (Number.isNaN(at)) return '';
	const seconds = Math.round((at - now) / 1000);
	const format = new Intl.RelativeTimeFormat(locale, { numeric: 'auto' });
	const units: [Intl.RelativeTimeFormatUnit, number][] = [
		['second', 60],
		['minute', 60],
		['hour', 24],
		['day', 7],
		['week', 4.348],
		['month', 12]
	];
	let scaled = seconds;
	for (const [unit, step] of units) {
		if (Math.abs(scaled) < step) return format.format(scaled, unit);
		scaled = Math.round(scaled / step);
	}
	return format.format(scaled, 'year');
}

export interface ComfyuiWorkflowStats {
	workflow_id: string;
	runs: number;
	failed: number;
	median_ms: number | null;
}

/**
 * Per-workflow reliability over the jobs on hand, worst first — the ordering
 * that answers "what is broken?" without reading the run list. Median rather
 * than mean: one 900-second timeout would drag an average somewhere no run
 * actually is.
 */
export function workflowStats(jobs: ComfyuiJob[]): ComfyuiWorkflowStats[] {
	const byWorkflow = new Map<string, ComfyuiJob[]>();
	for (const job of jobs) {
		const bucket = byWorkflow.get(job.workflow_id);
		if (bucket) bucket.push(job);
		else byWorkflow.set(job.workflow_id, [job]);
	}
	return [...byWorkflow.entries()]
		.map(([workflow_id, entries]) => {
			const durations = entries
				.map(jobDurationMs)
				.filter((ms): ms is number => ms !== null)
				.sort((a, b) => a - b);
			return {
				workflow_id,
				runs: entries.length,
				failed: entries.filter((job) => jobTone(job.status) === 'error').length,
				median_ms: durations.length ? median(durations) : null
			};
		})
		.sort((a, b) => b.failed - a.failed || b.runs - a.runs || a.workflow_id.localeCompare(b.workflow_id));
}

function median(sorted: number[]): number {
	const middle = Math.floor(sorted.length / 2);
	return sorted.length % 2 ? sorted[middle] : Math.round((sorted[middle - 1] + sorted[middle]) / 2);
}

/** Bytes as GiB, for the worker's VRAM and RAM readouts. */
export function formatGiB(bytes: number | null): string {
	if (bytes === null || !Number.isFinite(bytes) || bytes < 0) return '—';
	return `${(bytes / 1024 ** 3).toFixed(1)} GiB`;
}

/** "8.3 / 95.0 GiB free" — free first, because that is the number that
 *  decides whether the next job fits. */
export function formatVram(device: ComfyuiDevice): string {
	if (device.vram_total === null) return '';
	return `${formatGiB(device.vram_free)} / ${formatGiB(device.vram_total)}`;
}

/** ComfyUI reports `cuda:0 NVIDIA RTX PRO 6000 … : cudaMallocAsync`; the
 *  allocator suffix and index prefix are noise in a one-line readout. */
export function deviceLabel(name: string): string {
	return name.replace(/^\w+:\d+\s*/, '').split(' : ')[0].trim() || name;
}
