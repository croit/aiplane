import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { compile } from 'svelte/compiler';
import { render } from 'svelte/server';

const sourceUrl = new URL('./components/integrations/IntegrationCredentialActions.svelte', import.meta.url);
const source = readFileSync(sourceUrl, 'utf8');
const compiled = compile(source, { filename: sourceUrl.pathname, generate: 'server' }).js.code
	.replace(/import \{ t \} from ['"]#lib\/i18n.svelte.js['"];?/, 'const t = (key) => key;')
	.replace(/from ['"](svelte(?:\/[^'"]+)?)['"]/g, (_match, name) => `from ${JSON.stringify(import.meta.resolve(name))}`);
const { default: Actions } = await import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`);
const props = { connectorKey: 'actual-key', authType: 'oauth2', connected: true, needsSetup: false, ondisconnect: () => {} };

// Only translated copy is replaced: this tests the real compiled component's controls.
const html = (values: Record<string, unknown>) => render(Actions, { props: { ...props, ...values } }).body;

test('an unavailable own credential offers local disconnect without an invented OAuth form', () => {
	const output = html({ authType: null, canConnect: false, canDisconnect: true });
	assert.match(output, /integrations-disconnect-button/);
	assert.doesNotMatch(output, /<form|integrations-connect-button|integrations-reconnect-button/);
});

test('missing connector metadata never falls through to native connect', () => {
	assert.doesNotMatch(html({ authType: null, canDisconnect: false }), /<form|<button/);
});

test('needs setup prevents connecting without disabling local erasure', () => {
	const output = html({ needsSetup: true, canConnect: false, canDisconnect: true });
	assert.match(output, /integrations-disconnect-button/);
	assert.doesNotMatch(output, /disabled|<form/);
});

test('ordinary integrations retain native OAuth connect and static credential retry', () => {
	assert.match(html({ connected: false }), /<form[^>]+\/integrations\/actual-key\/connect/);
	assert.match(html({ authType: 'static_bearer', onretry: () => {} }), /integrations-reconnect-button/);
});

test('current management rights and busy state disable local disconnect', () => {
	assert.match(html({ authType: null, canConnect: false, canDisconnect: true, disabled: true }), /<button[^>]+disabled/);
});
