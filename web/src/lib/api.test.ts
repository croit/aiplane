import assert from 'node:assert/strict';
import test, { type TestContext } from 'node:test';

import { ApiError, request } from './api.ts';

function browserLanguage(context: TestContext, cookie: string, languages: string[]) {
	const documentDescriptor = Object.getOwnPropertyDescriptor(globalThis, 'document');
	const navigatorDescriptor = Object.getOwnPropertyDescriptor(globalThis, 'navigator');
	context.after(() => {
		if (documentDescriptor) Object.defineProperty(globalThis, 'document', documentDescriptor);
		else Reflect.deleteProperty(globalThis, 'document');
		if (navigatorDescriptor) Object.defineProperty(globalThis, 'navigator', navigatorDescriptor);
		else Reflect.deleteProperty(globalThis, 'navigator');
	});
	const browserDocument = { cookie };
	Object.defineProperty(globalThis, 'document', { configurable: true, value: browserDocument });
	Object.defineProperty(globalThis, 'navigator', { configurable: true, value: { languages } });
	return browserDocument;
}

test('JSON requests send the selected interface language before browser preferences', async (context) => {
	browserLanguage(context, 'lang=en', ['de-DE']);
	const originalFetch = globalThis.fetch;
	context.after(() => { globalThis.fetch = originalFetch; });
	let sent: RequestInit | undefined;
	globalThis.fetch = async (_path, init) => { sent = init; return new Response(null, { status: 204 }); };
	await request('/language', { headers: { 'x-request-id': 'language-test' } });
	const headers = new Headers(sent?.headers);
	assert.equal(headers.get('accept-language'), 'en');
	assert.equal(headers.get('accept'), 'application/json');
	assert.equal(headers.get('x-request-id'), 'language-test');
	assert.equal(sent?.credentials, 'same-origin');
});

test('JSON request language follows a changed language cookie without reloading', async (context) => {
	const browserDocument = browserLanguage(context, 'lang=en', ['de']);
	const originalFetch = globalThis.fetch;
	context.after(() => { globalThis.fetch = originalFetch; });
	const languages: (string | null)[] = [];
	globalThis.fetch = async (_path, init) => {
		languages.push(new Headers(init?.headers).get('accept-language'));
		return new Response(null, { status: 204 });
	};
	await request('/first');
	browserDocument.cookie = 'lang=fr';
	await request('/second');
	assert.deepEqual(languages, ['en', 'fr']);
});

test('JSON requests preserve explicit Headers values without changing the caller object', async (context) => {
	browserLanguage(context, 'lang=en', ['de']);
	const originalFetch = globalThis.fetch;
	context.after(() => { globalThis.fetch = originalFetch; });
	const headers = new Headers({ 'Accept-Language': 'es', Accept: 'application/vnd.test+json', 'X-Request-ID': 'explicit-test' });
	let sent: Headers | undefined;
	globalThis.fetch = async (_path, init) => { sent = new Headers(init?.headers); return new Response(null, { status: 204 }); };
	await request('/explicit', { headers });
	assert.equal(sent?.get('accept-language'), 'es');
	assert.equal(sent?.get('accept'), 'application/vnd.test+json');
	assert.equal(sent?.get('x-request-id'), 'explicit-test');
	assert.equal(headers.get('accept-language'), 'es');
	assert.equal(headers.get('accept'), 'application/vnd.test+json');
});

test('JSON requests preserve explicit record and tuple language headers', async (context) => {
	browserLanguage(context, 'lang=en', ['de']);
	const originalFetch = globalThis.fetch;
	context.after(() => { globalThis.fetch = originalFetch; });
	const languages: (string | null)[] = [];
	globalThis.fetch = async (_path, init) => {
		languages.push(new Headers(init?.headers).get('accept-language'));
		return new Response(null, { status: 204 });
	};
	await request('/record', { headers: { 'Accept-Language': 'ru' } });
	await request('/tuple', { headers: [['Accept-Language', 'zh']] });
	assert.deepEqual(languages, ['ru', 'zh']);
});

test('JSON requests use existing browser language fallback and remain stable without document', async (context) => {
	browserLanguage(context, 'lang=unsupported', ['de-AT', 'en']);
	const originalFetch = globalThis.fetch;
	context.after(() => { globalThis.fetch = originalFetch; });
	const languages: (string | null)[] = [];
	globalThis.fetch = async (_path, init) => {
		languages.push(new Headers(init?.headers).get('accept-language'));
		return new Response(null, { status: 204 });
	};
	await request('/browser');
	Reflect.deleteProperty(globalThis, 'document');
	await request('/node');
	assert.deepEqual(languages, ['de', 'en']);
});

test('successful empty responses complete without JSON parsing', async (context) => {
	const originalFetch = globalThis.fetch;
	context.after(() => { globalThis.fetch = originalFetch; });
	globalThis.fetch = async () => new Response(null, { status: 204 });
	assert.equal(await request<void>('/empty'), undefined);
});

test('a refusal carries the envelope: code, the server sentence and its issues', async (context) => {
	const originalFetch = globalThis.fetch;
	context.after(() => { globalThis.fetch = originalFetch; });
	const body = JSON.stringify({
		error: {
			message: 'cannot save the test case: at `expect`, empty',
			code: 'invalid_test_case',
			issues: [{ path: 'expect', message: 'empty' }]
		}
	});
	globalThis.fetch = async () => new Response(body, { status: 422 });

	const err = await request('/cases').catch((e: unknown) => e);

	assert.ok(err instanceof ApiError);
	assert.equal(err.status, 422);
	assert.equal(err.code, 'invalid_test_case');
	assert.equal(err.serverMessage, 'cannot save the test case: at `expect`, empty');
	assert.deepEqual(err.issues, [{ path: 'expect', message: 'empty' }]);
	assert.equal(err.detail, body);
});

test('a body that is no envelope leaves only the status and the raw text', async (context) => {
	const originalFetch = globalThis.fetch;
	context.after(() => { globalThis.fetch = originalFetch; });
	globalThis.fetch = async () => new Response('upstream timeout', { status: 502 });

	const err = await request('/x').catch((e: unknown) => e);

	assert.ok(err instanceof ApiError);
	assert.equal(err.code, undefined);
	assert.equal(err.serverMessage, undefined);
	assert.deepEqual(err.issues, []);
	assert.equal(err.detail, 'upstream timeout');
});
