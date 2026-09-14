import assert from 'node:assert/strict';
import test from 'node:test';

import { filterSearchOptions } from './searchable-select.ts';
import { timezoneOffset, timezoneOptions } from './timezones.ts';

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
