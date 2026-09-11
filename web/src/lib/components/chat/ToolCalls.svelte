<script lang="ts">
	import type { ToolCall } from '$lib/chat-protocol';
	import { prettyToolPayload, summarizeToolCalls, TOOL_GROUP_THRESHOLD } from '$lib/chat-transcript';
	import { n, t } from '$lib/i18n.svelte';

	let { calls }: { calls: ToolCall[] } = $props();
	const anyRunning = $derived(calls.some((call: ToolCall) => call.status === 'running'));
	const anyErrored = $derived(calls.some((call: ToolCall) => call.status === 'errored'));

	function statusLabel(call: ToolCall): string {
		if (call.status === 'errored') return t('render-tool-status-error');
		if (call.status === 'running') return call.name.startsWith('comfyui_') ? t('render-tools-running') : t('render-tool-status-calling');
		return t('render-tool-status-used');
	}

	function groupLabel(): string {
		if (anyRunning) return t('render-tools-running');
		if (anyErrored) return t('render-tools-errored');
		return t('render-tools-used');
	}
</script>

{#snippet callRow(call: ToolCall)}
	{@const input = prettyToolPayload(call.arguments_json)}
	{@const output = call.output_json ? prettyToolPayload(call.output_json) : null}
	<details class="collapse collapse-arrow rounded-lg bg-base-200/60">
		<summary class="collapse-title flex min-h-8 items-center gap-2 py-1.5 text-sm">
			{#if call.status === 'completed'}
				<span class="text-success">✓</span>
			{:else if call.status === 'errored'}
				<span class="text-error">✗</span>
			{:else}
				<span class="loading loading-spinner loading-xs"></span>
			{/if}
			<span class="text-base-content/60">{statusLabel(call)}</span>
			<span class="font-medium">{call.name}</span>
		</summary>
		<div class="collapse-content flex flex-col gap-2 text-xs text-base-content/70">
			<div>
				<div class="mb-1 font-medium">{t('render-tool-input-label')}</div>
				<pre class="max-h-48 overflow-auto whitespace-pre-wrap break-all font-mono">{input.text}</pre>
				{#if input.truncated}<p class="opacity-70">{t('render-tool-output-truncated', { bytes: n(input.bytes), chars: n(input.chars) })}</p>{/if}
			</div>
			{#if output}
				<div>
					<div class="mb-1 font-medium">{t('render-tool-output-label')}</div>
					<pre class="max-h-48 overflow-auto whitespace-pre-wrap break-all font-mono">{output.text}</pre>
					{#if output.truncated}<p class="opacity-70">{t('render-tool-output-truncated', { bytes: n(output.bytes), chars: n(output.chars) })}</p>{/if}
				</div>
			{/if}
		</div>
	</details>
{/snippet}

{#if calls.length > TOOL_GROUP_THRESHOLD}
	<details class="collapse collapse-arrow rounded-lg bg-base-200/60">
		<summary class="collapse-title flex min-h-8 items-center gap-2 py-1.5 text-sm">
			{#if anyRunning}<span class="loading loading-spinner loading-xs"></span>{:else}<span class={anyErrored ? 'text-error' : 'text-success'}>{anyErrored ? '✗' : '✓'}</span>{/if}
			<span class="text-base-content/60">{groupLabel()}</span>
			<span class="font-medium">{t('render-tools-summary', { count: calls.length, breakdown: summarizeToolCalls(calls) })}</span>
		</summary>
		<div class="collapse-content flex flex-col gap-1">
			{#each calls as call (call.id)}{@render callRow(call)}{/each}
		</div>
	</details>
{:else}
	{#each calls as call (call.id)}{@render callRow(call)}{/each}
{/if}
