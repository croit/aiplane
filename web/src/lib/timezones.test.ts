import assert from 'node:assert/strict';
import test, { type TestContext } from 'node:test';

import { filterSearchOptions } from './searchable-select.ts';
import { localTimezone, reportLocalTimezone, timezoneOffset, timezoneOptions } from './timezones.ts';

// Fixed instants either side of the northern DST boundary, so the offsets
// asserted here are the ones a user would actually see.
const WINTER = new Date('2026-01-15T12:00:00Z');
const SUMMER = new Date('2026-07-15T12:00:00Z');

test('an offset follows daylight saving rather than a fixed table', () => {
	assert.equal(timezoneOffset('Europe/Berlin', WINTER), 'UTC+01:00');
	assert.equal(timezoneOffset('Europe/Berlin', SUMMER), 'UTC+02:00');
	// A zone that does not shift, and one west of Greenwich.
	assert.equal(timezoneOffset('Asia/Tokyo', SUMMER), 'UTC+09:00');
	assert.equal(timezoneOffset('America/New_York', WINTER), 'UTC-05:00');
});

test('zero offset reads as UTC+00:00, not the bare GMT the platform emits', () => {
	assert.equal(timezoneOffset('UTC', WINTER), 'UTC+00:00');
});

test('an unresolvable zone has no offset instead of throwing', () => {
	assert.equal(timezoneOffset('Mars/Olympus_Mons', WINTER), null);
});

test('the option list is alphabetical and labels drop the underscore', () => {
	const options = timezoneOptions('UTC', SUMMER);
	assert.ok(options.length > 100, `expected the platform tz list, got ${options.length}`);
	const values = options.map((option) => option.value);
	assert.deepEqual(values, [...values].sort((a, b) => a.localeCompare(b)));
	const newYork = options.find((option) => option.value === 'America/New_York');
	assert.equal(newYork?.label, 'America/New York');
	assert.equal(newYork?.description, 'UTC-04:00');
});

test('a stored zone the platform does not list stays selectable', () => {
	// Opening the form must not silently move a schedule off a zone the
	// browser has never heard of.
	const options = timezoneOptions('Mars/Olympus_Mons', SUMMER);
	const kept = options.find((option) => option.value === 'Mars/Olympus_Mons');
	assert.ok(kept, 'the current value was dropped from the list');
	assert.equal(kept.description, undefined);
	assert.equal(options.filter((option) => option.value === 'UTC').length, 1, 'no duplicates');
});

test('a zone is findable by city, by underscore id, and by offset', () => {
	const options = timezoneOptions('UTC', SUMMER);
	const found = (query: string) => filterSearchOptions(options, query).map((option) => option.value);
	assert.ok(found('tokyo').includes('Asia/Tokyo'));
	assert.ok(found('new_york').includes('America/New_York'));
	assert.ok(found('new york').includes('America/New_York'));
	assert.ok(found('+09:00').includes('Asia/Tokyo'));
});

// --- reporting the browser's zone to the gateway ----------------------------

function browserZone(context: TestContext, zone: string | null) {
	const original = globalThis.Intl;
	context.after(() => {
		globalThis.Intl = original;
	});
	globalThis.Intl = Object.assign(Object.create(original), {
		DateTimeFormat: () => ({ resolvedOptions: () => (zone ? { timeZone: zone } : {}) })
	});
}

function tabSessionStorage(context: TestContext, initial: Record<string, string> = {}) {
	const store = new Map<string, string>(Object.entries(initial));
	const storage = {
		getItem: (key: string) => store.get(key) ?? null,
		setItem: (key: string, value: string) => {
			store.set(key, value);
		},
		removeItem: (key: string) => {
			store.delete(key);
		},
		clear: () => store.clear()
	};
	const descriptor = Object.getOwnPropertyDescriptor(globalThis, 'sessionStorage');
	context.after(() => {
		if (descriptor) Object.defineProperty(globalThis, 'sessionStorage', descriptor);
		else Reflect.deleteProperty(globalThis, 'sessionStorage');
	});
	Object.defineProperty(globalThis, 'sessionStorage', { configurable: true, value: storage });
	return storage;
}

function gatewayFetch(context: TestContext, status: number = 200) {
	const posted: { path: string; body: { timezone: string } }[] = [];
	const originalFetch = globalThis.fetch;
	context.after(() => {
		globalThis.fetch = originalFetch;
	});
	globalThis.fetch = async (path: string | URL | Request, init?: RequestInit) => {
		posted.push({ path: String(path), body: JSON.parse(String(init?.body)) });
		return new Response(JSON.stringify({ ok: true, timezone: posted.at(-1)?.body.timezone ?? '' }), { status });
	};
	return posted;
}

test('the browser zone is reported to the gateway once per tab session', async (context) => {
	browserZone(context, 'Europe/Berlin');
	const storage = tabSessionStorage(context);
	const posted = gatewayFetch(context);
	reportLocalTimezone();
	await new Promise((resolve) => setTimeout(resolve, 0));
	assert.equal(posted.length, 1);
	assert.equal(posted[0].path, '/api/v0/me/timezone');
	assert.deepEqual(posted[0].body, { timezone: 'Europe/Berlin' });
	assert.equal(storage.getItem('tz-reported'), '1');
});

test('a tab session that already reported does not report again', async (context) => {
	browserZone(context, 'Europe/Berlin');
	tabSessionStorage(context, { 'tz-reported': '1' });
	const posted = gatewayFetch(context);
	reportLocalTimezone();
	await new Promise((resolve) => setTimeout(resolve, 0));
	assert.equal(posted.length, 0);
});

test('a browser that will not name its zone reports nothing', async (context) => {
	browserZone(context, null);
	const storage = tabSessionStorage(context);
	const posted = gatewayFetch(context);
	reportLocalTimezone();
	await new Promise((resolve) => setTimeout(resolve, 0));
	assert.equal(posted.length, 0);
	assert.equal(storage.getItem('tz-reported'), null);
});

test('a refused report leaves the tab session unmarked so the next load retries', async (context) => {
	browserZone(context, 'Europe/Berlin');
	const storage = tabSessionStorage(context);
	gatewayFetch(context, 401);
	reportLocalTimezone();
	await new Promise((resolve) => setTimeout(resolve, 0));
	assert.equal(storage.getItem('tz-reported'), null);
});

test('localTimezone answers null when the browser will not say', (context) => {
	browserZone(context, null);
	assert.equal(localTimezone(), null);
});
