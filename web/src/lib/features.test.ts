import assert from 'node:assert/strict';
import test from 'node:test';

import { featureEnabled, featureForRoute, visibleNavLinks } from './features.ts';

test('a feature page and its sub-pages resolve to the same feature', () => {
	assert.equal(featureForRoute('/admin/comfyui'), 'comfyui');
	assert.equal(featureForRoute('/admin/comfyui/jobs'), 'comfyui');
	assert.equal(featureForRoute('/rag'), 'rag');
	assert.equal(featureForRoute('/rag/new'), 'rag');
	assert.equal(featureForRoute('/rag/42/edit'), 'rag');
	assert.equal(featureForRoute('/rag/profiles'), 'rag');
	assert.equal(featureForRoute('/usage'), 'usage');
	assert.equal(featureForRoute('/admin/limits'), 'limits');
});

test('both skills pages hang off the one skills switch', () => {
	assert.equal(featureForRoute('/skills'), 'skills');
	assert.equal(featureForRoute('/admin/skills'), 'skills');
});

test('an ungated route needs no feature', () => {
	for (const path of ['/', '/chat', '/chat/abc', '/memory', '/scheduled', '/tokens', '/admin/users']) {
		assert.equal(featureForRoute(path), null, path);
	}
});

test('a prefix only matches whole path segments', () => {
	// `/usage` must not claim `/usages-report`, and a longer prefix wins over
	// a shorter one that also matches.
	assert.equal(featureForRoute('/usagex'), null);
	assert.equal(featureForRoute('/ragtime'), null);
});

test('the base path is stripped before matching', () => {
	assert.equal(featureForRoute('/gw/admin/comfyui', '/gw'), 'comfyui');
	assert.equal(featureForRoute('/gw/chat', '/gw'), null);
});

test('a gateway that reports no feature list gates nothing', () => {
	// Serving an SPA newer than the gateway must show too much, never hide a
	// page that works.
	assert.equal(featureEnabled(undefined, 'comfyui'), true);
	assert.equal(featureEnabled([], 'comfyui'), false);
	assert.equal(featureEnabled(['rag', 'comfyui'], 'comfyui'), true);
});

test('nav entries for a disabled feature are dropped, ungated ones stay', () => {
	const links = [
		['nav-memory', '/memory', 'folder'],
		['nav-usage', '/usage', 'chart'],
		['nav-comfyui', '/admin/comfyui', 'sparkles'],
		['nav-rag', '/rag', 'database']
	] as const;
	assert.deepEqual(
		visibleNavLinks(links, ['rag', 'push']).map(([, path]) => path),
		['/memory', '/rag']
	);
	assert.deepEqual(visibleNavLinks(links, undefined).length, 4);
});
