import assert from 'node:assert/strict';
import test from 'node:test';
import { loginPageUrl, safeReturnTo } from './auth.ts';

test('safeReturnTo keeps same-origin paths including their query', () => {
	assert.equal(safeReturnTo('/admin/settings?tab=tools'), '/admin/settings?tab=tools');
});

test('safeReturnTo rejects absolute, protocol-relative, and login-loop targets', () => {
	for (const target of ['https://evil.example', '//evil.example', 'admin/settings', '/login']) {
		assert.equal(safeReturnTo(target), null);
	}
});

test('loginPageUrl preserves the complete same-origin destination', () => {
	assert.equal(loginPageUrl('/admin/settings?tab=tools'), '/login?return_to=%2Fadmin%2Fsettings%3Ftab%3Dtools');
});
