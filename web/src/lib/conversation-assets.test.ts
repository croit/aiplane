import assert from 'node:assert/strict';
import test from 'node:test';

import type { ChatAsset } from './api.ts';
import type { ChatAttachment } from './chat-protocol.ts';
import { canonicalAttachments } from './conversation-assets.ts';

test('stale duplicate markers resolve to the owning turn asset exactly once', () => {
	const attachments: ChatAttachment[] = [
		{ filename: 'result.png', mime: 'image/png', size: 10, url: '/chat/attachment/current/result.png' },
		{ filename: 'result.png', mime: 'image/png', size: 9, url: '/chat/attachment/stale/result.png' }
	];
	const assets: ChatAsset[] = [
		{ id: 'asset', turn_id: 'current', filename: 'result.png', mime: 'image/png', size: 10, url: '/chat/attachment/current/result.png' }
	];

	assert.deepEqual(canonicalAttachments(attachments, assets, 'current'), [attachments[0]]);
});

test('unrelated attachments are preserved when no canonical asset exists', () => {
	const attachment: ChatAttachment = { filename: 'notes.txt', mime: 'text/plain', size: 5, url: '/elsewhere/notes.txt' };
	assert.deepEqual(canonicalAttachments([attachment], [], 'turn'), [attachment]);
});
