import test from 'node:test';
import assert from 'node:assert/strict';
import {
	deviceLabel,
	filterJobs,
	filterWorkflows,
	formatDuration,
	formatGiB,
	groupWorkflowsByKind,
	inlineCodeSpans,
	jobDurationMs,
	jobFilterCounts,
	jobTone,
	relativeTime,
	requiredParams,
	selectedWorkflow,
	workflowStats,
	type ComfyuiJob,
	type ComfyuiWorkflow
} from './admin-comfyui.ts';

function workflow(id: string, kind: string, params: [string, boolean][] = []): ComfyuiWorkflow {
	return {
		id,
		tool_id: `comfyui_${id}`,
		title: id.replaceAll('_', ' '),
		description: `Does ${id}.`,
		output_kind: kind,
		output_node_id: '9',
		filename_prefix: `llmgw-${id}`,
		params: params.map(([key, required]) => ({ key, required, description: `The ${key}.` }))
	};
}

function job(id: number, overrides: Partial<ComfyuiJob> = {}): ComfyuiJob {
	return {
		id,
		prompt_id: `prompt-${id}`,
		session_id: 'session-1',
		turn_id: 'turn-1',
		user_id: 'dev',
		workflow_id: 'text_to_image',
		output_kind: 'image',
		status: 'completed',
		error_message: null,
		output_filename: `text_to_image-${id}.png`,
		output_mime: 'image/png',
		created_at: '2026-07-30T05:12:17.529109363Z',
		completed_at: '2026-07-30T05:12:20.391930217Z',
		...overrides
	};
}

test('the catalog groups by output kind in a fixed order and keeps unknown kinds last', () => {
	const groups = groupWorkflowsByKind([
		workflow('text_to_music', 'audio'),
		workflow('hologram', 'lightfield'),
		workflow('text_to_image', 'image'),
		workflow('image_to_video', 'video'),
		workflow('edit_image', 'image')
	]);
	assert.deepEqual(
		groups.map((group) => group.kind),
		['image', 'video', 'audio', 'lightfield']
	);
	assert.deepEqual(
		groups[0].workflows.map((entry) => entry.id),
		['edit_image', 'text_to_image']
	);
});

test('catalog search also matches parameter keys, not just titles', () => {
	const workflows = [
		workflow('clone_voice', 'audio', [['reference_audio', true]]),
		workflow('text_to_image', 'image', [['prompt', true]])
	];
	assert.deepEqual(
		filterWorkflows(workflows, 'reference_audio').map((entry) => entry.id),
		['clone_voice']
	);
	assert.deepEqual(
		filterWorkflows(workflows, 'IMAGE').map((entry) => entry.id),
		['text_to_image']
	);
	assert.equal(filterWorkflows(workflows, '   ').length, 2);
});

test('the detail pane falls back to the first workflow when the query names none', () => {
	const workflows = [workflow('edit_image', 'image'), workflow('text_to_image', 'image')];
	assert.equal(selectedWorkflow(workflows, 'text_to_image')?.id, 'text_to_image');
	assert.equal(selectedWorkflow(workflows, 'deleted_since')?.id, 'edit_image');
	assert.equal(selectedWorkflow(workflows, null)?.id, 'edit_image');
	assert.equal(selectedWorkflow([], 'anything'), null);
});

test('required parameters are what the detail header counts', () => {
	const entry = workflow('edit_image', 'image', [
		['image', true],
		['prompt', true],
		['seed', false]
	]);
	assert.deepEqual(
		requiredParams(entry).map((param) => param.key),
		['image', 'prompt']
	);
});

test('a timeout reads as a failure and a pending job does not', () => {
	assert.equal(jobTone('completed'), 'success');
	assert.equal(jobTone('timeout'), 'error');
	assert.equal(jobTone('failed'), 'error');
	assert.equal(jobTone('pending'), 'warning');
	assert.equal(jobTone('something-new'), 'neutral');
});

test('the failed filter covers both ComfyUI failures and gateway timeouts', () => {
	const jobs = [
		job(1),
		job(2, { status: 'timeout', completed_at: null, error_message: 'Timed out after 900 seconds' }),
		job(3, { status: 'failed', error_message: 'node 9 produced no output' }),
		job(4, { status: 'pending', completed_at: null })
	];
	assert.deepEqual(
		filterJobs(jobs, 'failed').map((entry) => entry.id),
		[2, 3]
	);
	assert.deepEqual(filterJobs(jobs, 'all').length, 4);
	assert.deepEqual(jobFilterCounts(jobs), { all: 4, completed: 1, pending: 1, failed: 2 });
});

test('job duration comes from the timestamp pair and is absent while running', () => {
	assert.equal(jobDurationMs(job(1)), 2862);
	assert.equal(jobDurationMs(job(2, { status: 'pending', completed_at: null })), null);
	assert.equal(jobDurationMs(job(3, { created_at: 'not a date' })), null);
});

test('durations stay legible from milliseconds to hours', () => {
	assert.equal(formatDuration(null), '—');
	assert.equal(formatDuration(420), '420 ms');
	assert.equal(formatDuration(2862), '2.9 s');
	assert.equal(formatDuration(59_940), '59.9 s');
	assert.equal(formatDuration(834_000), '13m 54s');
	assert.equal(formatDuration(901_000), '15m 01s');
	assert.equal(formatDuration(3_726_000), '1h 02m');
});

test('relative time reads in the viewer language and survives a bad timestamp', () => {
	const now = Date.parse('2026-07-30T06:00:00Z');
	assert.equal(relativeTime('2026-07-30T05:46:00Z', 'en', now), '14 minutes ago');
	assert.equal(relativeTime('2026-07-30T05:59:50Z', 'en', now), '10 seconds ago');
	assert.equal(relativeTime('2026-07-28T06:00:00Z', 'en', now), '2 days ago');
	assert.equal(relativeTime('2026-07-30T05:46:00Z', 'de', now), 'vor 14 Minuten');
	assert.equal(relativeTime('nonsense', 'en', now), '');
});

test('workflow stats rank the broken first and take a median, not a mean', () => {
	const jobs = [
		job(1, { workflow_id: 'text_to_image' }),
		job(2, { workflow_id: 'text_to_image' }),
		job(3, {
			workflow_id: 'image_to_video',
			created_at: '2026-07-30T04:00:00Z',
			completed_at: '2026-07-30T04:00:10Z'
		}),
		job(4, {
			workflow_id: 'image_to_video',
			created_at: '2026-07-30T04:10:00Z',
			completed_at: '2026-07-30T04:10:20Z'
		}),
		job(5, {
			workflow_id: 'image_to_video',
			status: 'timeout',
			created_at: '2026-07-30T04:20:00Z',
			completed_at: '2026-07-30T04:35:00Z'
		})
	];
	const stats = workflowStats(jobs);
	assert.deepEqual(
		stats.map((entry) => entry.workflow_id),
		['image_to_video', 'text_to_image']
	);
	assert.equal(stats[0].runs, 3);
	assert.equal(stats[0].failed, 1);
	// 10 s, 20 s, 900 s → 20 s. A mean would claim 310 s, which no run took.
	assert.equal(stats[0].median_ms, 20_000);
	assert.equal(stats[1].failed, 0);
});

test('worker readouts drop ComfyUI device noise and report free VRAM first', () => {
	assert.equal(
		deviceLabel('cuda:0 NVIDIA RTX PRO 6000 Blackwell Max-Q Workstation Edition : cudaMallocAsync'),
		'NVIDIA RTX PRO 6000 Blackwell Max-Q Workstation Edition'
	);
	assert.equal(deviceLabel('cpu'), 'cpu');
	assert.equal(formatGiB(102_014_189_568), '95.0 GiB');
	assert.equal(formatGiB(null), '—');
});

test('the default selection is the first workflow the rail shows, not the first the API sent', () => {
	// API order is manifest-directory order; the rail groups image → video →
	// audio. Landing on an audio workflow while the list opens on image reads
	// as a bug, so the fallback follows display order.
	const workflows = [
		workflow('clone_voice', 'audio'),
		workflow('text_to_image', 'image'),
		workflow('edit_image', 'image')
	];
	assert.equal(selectedWorkflow(workflows, null)?.id, 'edit_image');
	assert.equal(selectedWorkflow(workflows, 'clone_voice')?.id, 'clone_voice');
});

test('backticked identifiers in model-facing prose become code spans', () => {
	assert.deepEqual(inlineCodeSpans('Pass the id in `reference_audio` exactly.'), [
		{ text: 'Pass the id in ', code: false },
		{ text: 'reference_audio', code: true },
		{ text: ' exactly.', code: false }
	]);
	assert.deepEqual(inlineCodeSpans('no markup here'), [{ text: 'no markup here', code: false }]);
	// An unbalanced backtick must stay literal rather than eat the remainder.
	assert.deepEqual(inlineCodeSpans('a ` dangling tick'), [{ text: 'a ` dangling tick', code: false }]);
	assert.deepEqual(inlineCodeSpans('`leading` and `trailing`'), [
		{ text: 'leading', code: true },
		{ text: ' and ', code: false },
		{ text: 'trailing', code: true }
	]);
});
