#!/usr/bin/env node
// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH
//
// Upload and publish the Chrome extension to the Chrome Web Store.
//
//   node scripts/publish-extension.mjs <path-to-zip>
//   node scripts/publish-extension.mjs --check    (credentials only)
//
// `--check` authenticates and stops. It exists because "the secret is set" and
// "the secret works" are different statements, and only the first one can be
// read off GitHub: a service-account key pasted by hand loses the newlines in
// its private key often enough that discovering it during a release is a
// question of when. This answers it in three seconds, at any time.
//
// Authenticates as a **service account**, which is the reason this is written
// out rather than pulled from an action off the shelf: the widely used ones
// still authenticate with a client id, client secret and refresh token, and a
// refresh token in CI is a credential somebody has to notice has expired. A
// service account is added once in the Web Store dashboard under Account and
// then keeps working.
//
// It targets **API v2**. v1 is switched off on 2026-10-15, and v2 is where
// service accounts exist at all.
//
// Environment:
//   CWS_SERVICE_ACCOUNT  the service-account JSON key, verbatim (a secret)
//   CWS_PUBLISHER_ID     from the dashboard, Publisher → Settings (a variable:
//                        it is an identifier, and masking it in the log turns a
//                        wrong one into an unreadable 404)
//   CWS_EXTENSION_ID     the 32-character item id, which is in the extension's
//                        public store URL (also a variable)
//
// No dependencies: Node signs the JWT with its own crypto and `fetch` is
// built in. Adding a package here would mean an npm install in a job that
// otherwise needs nothing.

import { createSign } from 'node:crypto';
import { readFile } from 'node:fs/promises';

const API = 'https://chromewebstore.googleapis.com';
const SCOPE = 'https://www.googleapis.com/auth/chromewebstore';

/** base64url, which JWTs use and Node's base64 is one replace away from. */
const b64url = (input) =>
	Buffer.from(input).toString('base64').replaceAll('+', '-').replaceAll('/', '_').replace(/=+$/, '');

/**
 * Exchange a service-account key for an access token.
 *
 * The self-signed-JWT flow: sign an assertion with the account's private key,
 * hand it to Google, get a token back. No user, no consent screen, nothing to
 * re-authorise in six months.
 */
async function accessToken(key) {
	const now = Math.floor(Date.now() / 1000);
	const header = b64url(JSON.stringify({ alg: 'RS256', typ: 'JWT' }));
	const claims = b64url(
		JSON.stringify({
			iss: key.client_email,
			scope: SCOPE,
			aud: key.token_uri ?? 'https://oauth2.googleapis.com/token',
			iat: now,
			exp: now + 3600
		})
	);
	const sign = createSign('RSA-SHA256');
	sign.update(`${header}.${claims}`);
	const assertion = `${header}.${claims}.${sign.sign(key.private_key, 'base64url')}`;

	const response = await fetch(key.token_uri ?? 'https://oauth2.googleapis.com/token', {
		method: 'POST',
		headers: { 'content-type': 'application/x-www-form-urlencoded' },
		body: new URLSearchParams({
			grant_type: 'urn:ietf:params:oauth:grant-type:jwt-bearer',
			assertion
		})
	});
	const body = await response.json().catch(() => ({}));
	if (!response.ok) {
		// Google's error here is genuinely informative (wrong audience, clock
		// skew, key revoked), so pass it through rather than paraphrasing.
		throw new Error(
			`could not get a token for ${key.client_email}: ${response.status} ${JSON.stringify(body)}`
		);
	}
	return body.access_token;
}

/** A store call, with the failure spelled out — these are hard to debug blind. */
async function call(url, token, init = {}) {
	const response = await fetch(url, {
		...init,
		headers: { authorization: `Bearer ${token}`, ...(init.headers ?? {}) }
	});
	const text = await response.text();
	if (!response.ok) {
		throw new Error(`${init.method ?? 'GET'} ${url} → ${response.status}\n${text}`);
	}
	return text ? JSON.parse(text) : {};
}

async function main() {
	const zip = process.argv[2];
	if (!zip) throw new Error('usage: publish-extension.mjs <path-to-zip> | --check');
	const checkOnly = zip === '--check';

	const { CWS_SERVICE_ACCOUNT, CWS_PUBLISHER_ID, CWS_EXTENSION_ID } = process.env;
	// The item id is not needed to prove the key works, and it does not exist
	// until the first upload has been made by hand.
	const required = checkOnly
		? ['CWS_SERVICE_ACCOUNT']
		: ['CWS_SERVICE_ACCOUNT', 'CWS_PUBLISHER_ID', 'CWS_EXTENSION_ID'];
	const missing = required.filter((name) => !process.env[name]);
	if (missing.length > 0) throw new Error(`missing ${missing.join(', ')}`);

	let key;
	try {
		key = JSON.parse(CWS_SERVICE_ACCOUNT);
	} catch (err) {
		// By far the most likely failure, and the least obvious one downstream.
		throw new Error(
			`CWS_SERVICE_ACCOUNT is not valid JSON (${err.message}). It must be the ` +
				`whole downloaded key file, unedited.`
		);
	}
	if (!key.private_key?.includes('BEGIN PRIVATE KEY')) {
		throw new Error('CWS_SERVICE_ACCOUNT has no usable private_key — was it pasted whole?');
	}

	const token = await accessToken(key);
	if (checkOnly) {
		console.log(`credentials work: ${key.client_email} authenticated against the store API`);
		return;
	}
	const item = `publishers/${CWS_PUBLISHER_ID}/items/${CWS_EXTENSION_ID}`;

	// The store rejects a version that is not greater than the published one,
	// so a re-run of the same tag fails here rather than halfway through.
	const payload = await readFile(zip);
	console.log(`uploading ${zip} (${payload.length} bytes)`);
	await call(`${API}/upload/v2/${item}:upload?uploadType=media`, token, {
		method: 'POST',
		headers: { 'content-type': 'application/zip' },
		body: payload
	});

	console.log('publishing');
	await call(`${API}/v2/${item}:publish`, token, {
		method: 'POST',
		headers: { 'content-type': 'application/json' },
		// The default: submit for review and go live once it passes. A staged
		// rollout would need someone watching it, which a tag build is not.
		body: JSON.stringify({ publishType: 'DEFAULT_PUBLISH' })
	});

	// Say what actually happened. "Published" is misleading on its own: the
	// item is in review, which for this extension (debugger permission, broad
	// hosts) takes days rather than minutes.
	const status = await call(`${API}/v2/${item}:fetchStatus`, token);
	console.log(`submitted — status: ${JSON.stringify(status)}`);
}

main().catch((err) => {
	console.error(String(err?.message ?? err));
	process.exit(1);
});
