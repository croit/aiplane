import test from 'node:test';
import assert from 'node:assert/strict';
import { navItemActive } from './nav.ts';

test('a nav entry stays lit on its own sub-pages', () => {
	assert.equal(navItemActive('/admin/comfyui', '', '/admin/comfyui'), true);
	assert.equal(navItemActive('/admin/comfyui/jobs', '', '/admin/comfyui'), true);
	assert.equal(navItemActive('/rag/profiles', '', '/rag'), true);
	assert.equal(navItemActive('/chat/abc-123', '', '/chat'), true);
});

test('a sibling route that merely shares a string prefix does not light it', () => {
	// The bug a bare startsWith would introduce: two entries lit at once.
	assert.equal(navItemActive('/admin/tokens', '', '/tokens'), false);
	assert.equal(navItemActive('/admin/skills', '', '/skills'), false);
	assert.equal(navItemActive('/tokens-archive', '', '/tokens'), false);
});

test('the root entry matches only itself', () => {
	assert.equal(navItemActive('/', '', '/'), true);
	assert.equal(navItemActive('/admin/users', '', '/'), false);
});

test('a deployment served under a base path resolves the same way', () => {
	assert.equal(navItemActive('/gw/admin/comfyui/jobs', '/gw', '/admin/comfyui'), true);
	assert.equal(navItemActive('/gw/admin/comfyui', '/gw', '/admin/comfyui'), true);
	assert.equal(navItemActive('/gw/admin/users', '/gw', '/admin/comfyui'), false);
});
