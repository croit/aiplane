import test from 'node:test';
import assert from 'node:assert/strict';
import { renderWelcome } from './welcome.ts';

const user = { name: 'Martin Verges', email: 'martin@example.com', language: 'de' };

test('fills the display name', () => {
	assert.equal(renderWelcome('Hello {name}!', user), 'Hello Martin Verges!');
});

test('fills the first name', () => {
	assert.equal(renderWelcome('Hello {first_name}!', user), 'Hello Martin!');
});

test('fills the address and the interface language', () => {
	assert.equal(renderWelcome('{email} / {language}', user), 'martin@example\\.com / de');
});

test('falls back to the local part of the address when there is no display name', () => {
	assert.equal(
		renderWelcome('Hello {name} ({first_name})!', { ...user, name: null }),
		'Hello martin (martin)!'
	);
});

test('replaces every occurrence', () => {
	assert.equal(renderWelcome('{name}, welcome. {name}.', user), 'Martin Verges, welcome. Martin Verges.');
});

test('escapes Markdown in the substituted value', () => {
	assert.equal(
		renderWelcome('Hello {name}!', { ...user, name: 'A*B_[C]' }),
		'Hello A\\*B\\_\\[C\\]!'
	);
});

test('does not expand a placeholder inside a substituted value', () => {
	assert.equal(renderWelcome('{name}', { ...user, name: '{email}' }), '\\{email\\}');
});

test('leaves unknown placeholders and text without placeholders untouched', () => {
	assert.equal(renderWelcome('Hi {nickname}.', user), 'Hi {nickname}.');
	assert.equal(renderWelcome('Hello there.', user), 'Hello there.');
});
