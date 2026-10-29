import type { OwnedAgent } from './agent-state.ts';
import type { SearchOption } from './searchable-select.ts';

export interface AgentBinding {
	personal_agent_id: string | null;
	agent: Pick<OwnedAgent, 'id' | 'name' | 'display' | 'active'> | null;
}

export function agentRunOptions(
	agents: readonly OwnedAgent[],
	initialId: string | null,
	labels: { person: string; inactive: string; unavailable: (id: string) => string }
): SearchOption[] {
	const options: SearchOption[] = [{ value: '', label: labels.person }];
	for (const agent of agents) {
		options.push({
			value: agent.id,
			label: agent.display || agent.name,
			description: agent.active ? agent.description : labels.inactive,
			keywords: [agent.name, agent.id],
			disabled: !agent.active && agent.id !== initialId
		});
	}
	if (initialId && !agents.some((agent) => agent.id === initialId)) {
		options.push({ value: initialId, label: labels.unavailable(initialId), disabled: false });
	}
	return options;
}
