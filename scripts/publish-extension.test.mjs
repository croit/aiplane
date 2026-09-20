// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { test } from 'node:test';

const script = new URL('./publish-extension.mjs', import.meta.url);

function run(args, env) {
	return spawnSync(process.execPath, [script.pathname, ...args], {
		encoding: 'utf8',
		env
	});
}

test('malformed credential JSON never exposes parser input', () => {
	const secretFragment = 'PRIVATE_KEY_FRAGMENT_MUST_NOT_LEAK';
	const result = run(['--check'], {
		CWS_SERVICE_ACCOUNT: `{"private_key":"${secretFragment}`
	});
	const output = `${result.stdout}${result.stderr}`;

	assert.equal(result.status, 1);
	assert.match(result.stderr, /CWS_SERVICE_ACCOUNT is not valid JSON/);
	assert.equal(output.includes(secretFragment), false);
});

test('missing variable diagnostics name variables without their values', () => {
	const result = run(['extension.zip'], {
		CWS_SERVICE_ACCOUNT: '{}'
	});

	assert.equal(result.status, 1);
	assert.equal(result.stderr, 'missing CWS_PUBLISHER_ID, CWS_EXTENSION_ID\n');
});
