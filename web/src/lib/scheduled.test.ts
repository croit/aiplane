import assert from 'node:assert/strict';
import test from 'node:test';

import { cronFromSchedule, formatScheduledRun, scheduleFromCron, scheduleLinks, type ScheduledAction } from './scheduled.ts';

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

const action = (fields: Partial<ScheduledAction>): ScheduledAction => ({
	id: 'act-1', name: 'Daily digest', prompt: 'Summarise', model: 'qwen', cron: '0 9 * * *',
	schedule_summary: 'every day at 09:00', timezone: 'Europe/Berlin', tools_enabled: true,
	reuse_conversation: false, reuse_rounds: 5, enabled: true, next_run_at: null, last_run_at: null,
	last_session_id: null, last_status: null, last_error: null, run_count: 0, chat_count: 0, ...fields
});

test('a schedule that has never fired offers nothing to open', () => {
	assert.deepEqual(scheduleLinks(action({})), { chat: null, runs: null });
});

test('one run that opened one chat links straight to it, with no history detour', () => {
	assert.deepEqual(scheduleLinks(action({ run_count: 1, chat_count: 1, last_session_id: 'sess-1' })), {
		chat: '/chat/sess-1',
		runs: null
	});
});

test('a reusing schedule keeps the direct chat link however often it has fired', () => {
	// Many runs, one conversation: the run list would be the same link twelve
	// times, so the chat stays one click away and the history stays available
	// for *when* each fire happened.
	assert.deepEqual(
		scheduleLinks(action({ reuse_conversation: true, run_count: 12, chat_count: 1, last_session_id: 'sess-1' })),
		{ chat: '/chat/sess-1', runs: '/scheduled/act-1/runs' }
	);
});

test('a schedule that opens a fresh chat each time links to the list, not to one of them', () => {
	assert.deepEqual(scheduleLinks(action({ run_count: 4, chat_count: 4, last_session_id: 'sess-4' })), {
		chat: null,
		runs: '/scheduled/act-1/runs'
	});
});

test('runs that produced no chat are still reachable', () => {
	// Over quota, or the model never answered: no session, but the failure is
	// the thing the owner came to find.
	assert.deepEqual(scheduleLinks(action({ run_count: 2, chat_count: 0 })), {
		chat: null,
		runs: '/scheduled/act-1/runs'
	});
});
