import type { OwnedAgent, OwnedAgentCommunicationChange, OwnedAgentCommunicationDirection, OwnedAgentCommunicationPeer } from './owned-agents.ts';

export function communicationDecision(peer: OwnedAgentCommunicationPeer, direction: OwnedAgentCommunicationDirection) {
	const edge = direction === 'outbound' ? peer.outgoing : peer.incoming;
	return {
		enabled: direction === 'outbound' ? edge.outbound_enabled : edge.inbound_enabled,
		counterpartEnabled: direction === 'outbound' ? edge.inbound_enabled : edge.outbound_enabled,
		generation: edge.generation
	};
}

export function communicationChange(agent: OwnedAgent, peer: OwnedAgentCommunicationPeer, direction: OwnedAgentCommunicationDirection, enabled: boolean): OwnedAgentCommunicationChange | null {
	const current = communicationDecision(peer, direction);
	if (enabled === current.enabled || (enabled && (!agent.active || !peer.agent.active))) return null;
	return { peer_id: peer.agent.id, direction, enabled, expected_generation: current.generation };
}
