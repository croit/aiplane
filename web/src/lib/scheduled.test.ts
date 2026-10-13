import assert from 'node:assert/strict';
import test from 'node:test';

import { cronFromSchedule, formatScheduledRun, scheduleFromCron } from './scheduled.ts';

test('schedule inventory retains authoritative principal scope, versions and explicit timezone', async (context) => {
	const { scheduledApi } = await import('./scheduled.ts');
	const original = globalThis.fetch;
	const paths: string[] = [];
	const actual = { actions: [], dots: [{ id: 'principal/id', name: 'Harald', active: true }], models: [], default_timezone: null, principal_id: 'principal/id', can_write: true, versions: [{ version: 3, spec: { main: { model: 'demo-model' } }, published_by: 'writer', published_at: '2026-10-11T00:00:00Z' }] };
	globalThis.fetch = async (input) => {
		paths.push(String(input));
		return new Response(JSON.stringify(actual), { status: 200 });
	};
	context.after(() => globalThis.fetch = original);
	assert.deepEqual(await scheduledApi.list('principal/id'), actual);
	await scheduledApi.list();
	assert.deepEqual(paths, ['/api/v0/scheduled?principal_id=principal%2Fid', '/api/v0/scheduled']);
});

test('organization schedules send their pinned version and compare the original configuration generation', async (context) => {
	const { scheduledApi } = await import('./scheduled.ts');
	const original = globalThis.fetch;
	const calls: { path: string; method: string | undefined; body: unknown }[] = [];
	globalThis.fetch = async (input, init) => {
		calls.push({ path: String(input), method: init?.method, body: init?.body ? JSON.parse(String(init.body)) : null });
		return new Response(JSON.stringify({ enabled: false, configuration_generation: 'next' }), { status: 200 });
	};
	context.after(() => globalThis.fetch = original);
	const body = { principal_id: 'principal', agent_version: 3, name: 'Check support', prompt: 'Review the queue', cron: '0 9 * * *', timezone: 'Europe/Berlin', tools_enabled: true, reuse_conversation: false, reuse_rounds: 5 };
	await scheduledApi.create(body);
	await scheduledApi.update('schedule/id', { ...body, expected_configuration_generation: 'original' });
	assert.deepEqual(await scheduledApi.toggle('schedule/id', false, 'original'), { enabled: false, configuration_generation: 'next' });
	assert.deepEqual(calls, [
		{ path: '/api/v0/scheduled', method: 'POST', body },
		{ path: '/api/v0/scheduled/schedule%2Fid', method: 'PUT', body: { ...body, expected_configuration_generation: 'original' } },
		{ path: '/api/v0/scheduled/schedule%2Fid/toggle', method: 'POST', body: { enabled: false, expected_configuration_generation: 'original' } }
	]);
});

test('schedule model projection uses only the selected actual published spec', async () => {
	const { scheduledVersionModel } = await import('./scheduled.ts');
	const versions = [
		{ version: 3, spec: { main: { model: 'demo-model' } }, published_by: 'writer', published_at: '2026-10-11T00:00:00Z' },
		{ version: 4, spec: { main: {} }, published_by: 'writer', published_at: '2026-10-11T00:00:00Z' }
	];
	assert.equal(scheduledVersionModel(versions, 3), 'demo-model');
	assert.equal(scheduledVersionModel(versions, 4), null);
	assert.equal(scheduledVersionModel(versions, 9), null);
	assert.equal(scheduledVersionModel(versions, null), null);
});

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
