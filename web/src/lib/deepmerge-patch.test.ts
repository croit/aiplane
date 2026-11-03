// The vendored deepmerge (web/vendor/deepmerge) exists because upstream 4.3.1
// has no fixed release for GHSA-gcqx-w48m-w2h4. This is the behaviour the
// patch adds: a `__proto__` (or `constructor`/`prototype`) key is never merged,
// even when the target has no Object.prototype to make the old guard skip it.
import test from 'node:test';
import assert from 'node:assert';
import deepmerge from 'deepmerge';

test('the vendored deepmerge refuses prototype keys', () => {
	const source = JSON.parse(
		'{"__proto__":{"polluted":true},"constructor":{"prototype":{"polluted":true}},"safe":1}'
	);
	const merged = deepmerge(Object.create(null) as Record<string, unknown>, source);
	assert.equal(merged.safe, 1);
	assert.equal(Object.getPrototypeOf(merged), Object.prototype);
	assert.equal(({} as { polluted?: boolean }).polluted, undefined);
	assert.equal((merged as { polluted?: boolean }).polluted, undefined);
});
