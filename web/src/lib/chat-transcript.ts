export const TOOL_GROUP_THRESHOLD = 3;
export const TOOL_PAYLOAD_LIMIT = 16 * 1024;

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

export function summarizeToolCalls(calls: { name: string }[]): string {
	const counts = new Map<string, number>();
	for (const call of calls) counts.set(call.name, (counts.get(call.name) ?? 0) + 1);
	return [...counts].map(([name, count]) => (count > 1 ? `${name} ×${count}` : name)).join(', ');
}
