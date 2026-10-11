import type { Dot, DotCommunicationChange, DotCommunicationDirection, DotCommunicationPeer } from './dots.ts';

export function communicationDecision(peer: DotCommunicationPeer, direction: DotCommunicationDirection) {
	const edge = direction === 'outbound' ? peer.outgoing : peer.incoming;
	return {
		enabled: direction === 'outbound' ? edge.outbound_enabled : edge.inbound_enabled,
		counterpartEnabled: direction === 'outbound' ? edge.inbound_enabled : edge.outbound_enabled,
		generation: edge.generation
	};
}

export function communicationChange(dot: Dot, peer: DotCommunicationPeer, direction: DotCommunicationDirection, enabled: boolean): DotCommunicationChange | null {
	const current = communicationDecision(peer, direction);
	if (enabled === current.enabled || (enabled && (!dot.active || !peer.dot.active))) return null;
	return { peer_id: peer.dot.id, direction, enabled, expected_generation: current.generation };
}
