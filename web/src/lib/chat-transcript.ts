import type { ToolCall, Turn } from './chat-protocol.js';

export const TOOL_GROUP_THRESHOLD = 3;
export const TOOL_PAYLOAD_LIMIT = 16 * 1024;

export function presentedToolCallStatus(status: ToolCall['status'], turnStatus: Turn['status']): ToolCall['status'] | 'waiting' {
	return status === 'running' && turnStatus === 'suspended' ? 'waiting' : status;
}

export function prettyToolPayload(raw: string, limit = TOOL_PAYLOAD_LIMIT): { text: string; truncated: boolean; bytes: number; chars: number } {
	let pretty = raw;
	try {
		pretty = JSON.stringify(JSON.parse(raw), null, 2);
	} catch {
		pretty = raw;
	}
	const bytes = new TextEncoder().encode(pretty).length;
	if (bytes <= limit) return { text: pretty, truncated: false, bytes, chars: pretty.length };
	let end = 0;
	let used = 0;
	for (const char of pretty) {
		const size = new TextEncoder().encode(char).length;
		if (used + size > limit) break;
		used += size;
		end += char.length;
	}
	return { text: pretty.slice(0, end), truncated: true, bytes, chars: end };
}

export function summarizeToolCalls(calls: { name: string }[], label: (name: string) => string = (name) => name): string {
	const counts = new Map<string, number>();
	for (const call of calls) counts.set(label(call.name), (counts.get(label(call.name)) ?? 0) + 1);
	return [...counts].map(([name, count]) => (count > 1 ? `${name} ×${count}` : name)).join(', ');
}

/**
 * The search engine a `search_web` call ran on, from its result JSON.
 *
 * The tool reports `provider` in every successful result, and the row is the
 * one place where "which engine answered" is a question: the same tool can
 * serve one turn from Tavily and the next from a fallback, and the expanded
 * output buries the field in a JSON block. An errored call has no result, so
 * nothing to read.
 */
export function searchProviderFromOutput(outputJson: string | null): string | null {
	if (outputJson === null) return null;
	try {
		const provider = (JSON.parse(outputJson) as { provider?: unknown }).provider;
		return typeof provider === 'string' && provider.length > 0 ? provider : null;
	} catch {
		return null;
	}
}
