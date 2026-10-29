export interface OwnedAgent {
	id: string;
	name: string;
	display: string;
	description: string;
	active: boolean;
}

export function activeAgentCount(agents: readonly OwnedAgent[]): number {
	return agents.reduce((count, agent) => count + Number(agent.active), 0);
}

export function canCreateAgent(active: number, limit: number): boolean {
	return active < limit;
}
