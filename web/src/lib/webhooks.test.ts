import assert from 'node:assert/strict';
import test from 'node:test';

import { formatWebhookFire, formatWebhookRun } from './webhooks.ts';

test('webhook timestamps retain the original compact 24-hour layout', () => {
	assert.equal(formatWebhookFire('2026-09-10T09:08:07Z', 'en'), 'Sep 10, 09:08');
	assert.equal(formatWebhookRun('2026-09-10T09:08:07Z', 'en'), 'Sep 10, 09:08:07');
});
