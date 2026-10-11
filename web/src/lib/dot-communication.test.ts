import assert from 'node:assert/strict';
import test from 'node:test';
import { communicationDecision, communicationChange } from './dot-communication.ts';
import type { Dot, DotCommunicationPeer } from './dots.ts';

const dot: Dot = { id: 'harald', name: 'Harald', display: '', description: '', active: true };
const peer: DotCommunicationPeer = {
	dot: { ...dot, id: 'clara', name: 'Clara' },
	outgoing: { outbound_enabled: false, inbound_enabled: true, generation: 'outgoing-generation' },
	incoming: { outbound_enabled: true, inbound_enabled: false, generation: null }
};

test('outbound edits the caller consent while inbound edits the receiver consent', () => {
	assert.deepEqual(communicationDecision(peer, 'outbound'), { enabled: false, counterpartEnabled: true, generation: 'outgoing-generation' });
	assert.deepEqual(communicationDecision(peer, 'inbound'), { enabled: false, counterpartEnabled: true, generation: null });
	assert.deepEqual(communicationChange(dot, peer, 'outbound', true), { peer_id: 'clara', direction: 'outbound', enabled: true, expected_generation: 'outgoing-generation' });
	assert.deepEqual(communicationChange(dot, peer, 'inbound', true), { peer_id: 'clara', direction: 'inbound', enabled: true, expected_generation: null });
	assert.equal(peer.outgoing.outbound_enabled, false);
	assert.equal(peer.incoming.inbound_enabled, false);
});

test('unchanged decisions produce no write and no new consent generation', () => {
	assert.equal(communicationChange(dot, peer, 'outbound', false), null);
});

test('inactive peers or callers cannot gain new consent but existing consent can be revoked', () => {
	assert.equal(communicationChange(dot, { ...peer, dot: { ...peer.dot, active: false } }, 'outbound', true), null);
	assert.equal(communicationChange({ ...dot, active: false }, peer, 'inbound', true), null);
	const enabled = { ...peer, dot: { ...peer.dot, active: false }, outgoing: { ...peer.outgoing, outbound_enabled: true } };
	assert.deepEqual(communicationChange({ ...dot, active: false }, enabled, 'outbound', false), { peer_id: 'clara', direction: 'outbound', enabled: false, expected_generation: 'outgoing-generation' });
});
