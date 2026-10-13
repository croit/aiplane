/**
 * The pure half of the agent architect's window (#118, docs/ui.md →
 * "Agent architect"): what a tool call offers (Undo, a link to the setup),
 * and the conversation's title.
 */
import type { ToolCall } from './chat-protocol.ts';
import type { Spec } from './agents.ts';

export interface DotSetupPlan {
	revision: string;
	name: string;
	display: string;
	description: string;
	draft_spec: Spec;
	knowledge: { id: string; name: string; description?: string | null } | null;
	tools: { id: string; capability_key: string; label?: string | null; description?: string | null; permission: string }[];
	dropped: { step: string; item?: string | null; reason: string }[];
	unresolved: string[];
}

export function dotPlanCanApply(plan: DotSetupPlan | null, working: boolean, applying: boolean): boolean {
	return plan !== null && !working && !applying && plan.dropped.length === 0 && plan.unresolved.length === 0;
}

export function groupedDotTools(tools: DotSetupPlan['tools']) {
	const groups = new Map<string, { key: string; label: string; permission: string; tools: DotSetupPlan['tools'] }>();
	for (const tool of tools) {
		const key = JSON.stringify([tool.capability_key, tool.permission]);
		const group = groups.get(key);
		if (group) group.tools.push(tool);
		else groups.set(key, { key, label: tool.label || tool.capability_key, permission: tool.permission, tools: [tool] });
	}
	return [...groups.values()];
}

/** The architect's tools that change an agent's draft. */
const DRAFT_WRITERS = new Set(['update_agent_draft']);
/** The architect's tools whose answer names an agent's setup page. */
const SETUP_LINKERS = new Set(['create_agent_draft', 'update_agent_draft', 'read_agent']);
const TOOLS = new Set(['list_agents', 'read_agent', 'list_grantable', 'propose_setup', 'create_agent_draft', 'update_agent_draft', 'run_test_turn', 'propose_dot_setup']);

function output(call: ToolCall): Record<string, unknown> | null {
	if (call.status !== 'completed' || !call.output_json) return null;
	try {
		const parsed: unknown = JSON.parse(call.output_json);
		return parsed && typeof parsed === 'object' ? (parsed as Record<string, unknown>) : null;
	} catch {
		return null;
	}
}

/** The earlier draft a completed draft change can be undone to. */
export function undoTarget(call: ToolCall): { agentId: string; revision: number } | null {
	if (!DRAFT_WRITERS.has(call.name)) return null;
	const out = output(call);
	const agentId = out?.agent_id;
	const revision = out?.revision;
	if (typeof agentId !== 'string' || typeof revision !== 'number') return null;
	return { agentId, revision };
}

/** The setup page a completed call points at, as an app path. */
export function setupPath(call: ToolCall): string | null {
	if (!SETUP_LINKERS.has(call.name)) return null;
	const url = output(call)?.setup_url;
	return typeof url === 'string' && url.startsWith('/agents/') ? url : null;
}

/** Whether a finished call changed an agent, so a page showing it reloads. */
export function changesAgent(call: ToolCall): boolean {
	return (call.name === 'create_agent_draft' || DRAFT_WRITERS.has(call.name)) && call.status === 'completed';
}

export type Phase = 'starting' | 'ready' | 'failed';

/** The conversation's title in the person's chat history. */
export function conversationTitle(tr: (key: string, args?: Record<string, string | number>) => string, name: string | null | undefined): string {
	const shown = name?.trim();
	return shown ? tr('architect-conversation-title', { name: shown }) : tr('architect-conversation-new');
}

/** A call's name as the person reads it: what the architect did, not the tool's id. */
export function toolLabel(tr: (key: string) => string): (name: string) => string {
	return (name) => (TOOLS.has(name) ? tr(`architect-tool-${name.replaceAll('_', '-')}`) : name);
}
