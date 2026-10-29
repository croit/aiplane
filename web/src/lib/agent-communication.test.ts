import assert from 'node:assert/strict';
import test from 'node:test';
import { communicationDecision, communicationChange } from './agent-communication.ts';
import type { OwnedAgent, OwnedAgentCommunicationPeer } from './owned-agents.ts';

const agent: OwnedAgent = { id: 'harald', name: 'Harald', display: '', description: '', active: true };
const peer: OwnedAgentCommunicationPeer = {
	agent: { ...agent, id: 'clara', name: 'Clara' },
	outgoing: { outbound_enabled: false, inbound_enabled: true, generation: 'outgoing-generation' },
	incoming: { outbound_enabled: true, inbound_enabled: false, generation: null },
	cross_organization: false
};

test('outbound edits the caller consent while inbound edits the receiver consent', () => {
	assert.deepEqual(communicationDecision(peer, 'outbound'), { enabled: false, counterpartEnabled: true, generation: 'outgoing-generation' });
	assert.deepEqual(communicationDecision(peer, 'inbound'), { enabled: false, counterpartEnabled: true, generation: null });
	assert.deepEqual(communicationChange(agent, peer, 'outbound', true), { peer_id: 'clara', direction: 'outbound', enabled: true, expected_generation: 'outgoing-generation' });
	assert.deepEqual(communicationChange(agent, peer, 'inbound', true), { peer_id: 'clara', direction: 'inbound', enabled: true, expected_generation: null });
	assert.equal(peer.outgoing.outbound_enabled, false);
	assert.equal(peer.incoming.inbound_enabled, false);
});

test('unchanged decisions produce no write and no new consent generation', () => {
	assert.equal(communicationChange(agent, peer, 'outbound', false), null);
});

test('inactive peers or callers cannot gain new consent but existing consent can be revoked', () => {
	assert.equal(communicationChange(agent, { ...peer, agent: { ...peer.agent, active: false } }, 'outbound', true), null);
	assert.equal(communicationChange({ ...agent, active: false }, peer, 'inbound', true), null);
	const enabled = { ...peer, agent: { ...peer.agent, active: false }, outgoing: { ...peer.outgoing, outbound_enabled: true } };
	assert.deepEqual(communicationChange({ ...agent, active: false }, enabled, 'outbound', false), { peer_id: 'clara', direction: 'outbound', enabled: false, expected_generation: 'outgoing-generation' });
});

test('the cross-organization peer marker carries its own outbound consent', () => {
	const orgPeer = { ...peer, cross_organization: true };
	assert.equal(orgPeer.cross_organization, true);
	assert.deepEqual(communicationChange(agent, orgPeer, 'outbound', true), { peer_id: 'clara', direction: 'outbound', enabled: true, expected_generation: 'outgoing-generation' });
});
