import assert from 'node:assert/strict';
import test from 'node:test';
import { canChoosePriority } from './priority.ts';

test('token owners can inherit and choose only priorities up to their ceiling', () => {
    assert.equal(canChoosePriority(null, 'low'), true);
    assert.equal(canChoosePriority('low', 'low'), true);
    assert.equal(canChoosePriority('medium', 'low'), false);
    assert.equal(canChoosePriority('high', 'medium'), false);
    assert.equal(canChoosePriority('medium', 'medium'), true);
    assert.equal(canChoosePriority('high', 'high'), true);
});
