import assert from 'node:assert/strict';
import test from 'node:test';

import { cronFromSchedule, formatScheduledRun, scheduleFromCron } from './scheduled.ts';

test('the friendly schedule builder covers hourly, daily, weekly, and monthly cron', () => {
	assert.equal(cronFromSchedule({ mode: 'hourly', minute: 15, hour: 9, dayOfMonth: 1, weekdays: [], advanced: '' }), '15 * * * *');
	assert.equal(cronFromSchedule({ mode: 'daily', minute: 0, hour: 9, dayOfMonth: 1, weekdays: [], advanced: '' }), '0 9 * * *');
	assert.equal(cronFromSchedule({ mode: 'weekly', minute: 30, hour: 8, dayOfMonth: 1, weekdays: [1, 3, 0], advanced: '' }), '30 8 * * 1,3,0');
	assert.equal(cronFromSchedule({ mode: 'monthly', minute: 0, hour: 7, dayOfMonth: 31, weekdays: [], advanced: '' }), '0 7 31 * *');
});

test('stored cron expressions repopulate the friendly editor without losing Sunday', () => {
	assert.deepEqual(scheduleFromCron('30 8 * * 1,3,0'), {
		mode: 'weekly', minute: 30, hour: 8, dayOfMonth: 1, weekdays: [1, 3, 0], advanced: ''
	});
	assert.equal(scheduleFromCron('5 4 * 2 *').mode, 'advanced');
});

test('scheduled run times retain the original compact 24-hour layout', () => {
	assert.equal(formatScheduledRun('2026-09-10T00:00:00Z', 'en', 'Asia/Tokyo'), 'Thu Sep 10, 09:00');
	assert.equal(formatScheduledRun('invalid', 'en', 'UTC'), '');
});
