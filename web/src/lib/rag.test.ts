import test from 'node:test';
import assert from 'node:assert';
import { listNotice, parseList, refreshIntervalLabel, parseSources, profileFieldsJson, sourceLabel } from './rag.ts';

test('RAG lists parse comma and newline separated values without empty entries', () => {
	assert.deepEqual(parseList('*.rs, *.md\ntarget/\n'), ['*.rs', '*.md', 'target/']);
});

test('RAG bulk sources support optional refs and comments', () => {
	assert.deepEqual(
		parseSources(`
# platform repositories
https://example.test/one.git
https://example.test/two.git @stable
`)
		,
		[
			{ url: 'https://example.test/one.git', git_ref: null },
			{ url: 'https://example.test/two.git', git_ref: 'stable' }
		]
	);
});

test('RAG profile fields pretty print for an editable JSON round trip', () => {
	assert.equal(
		profileFieldsJson([{ key: 'date', label: 'Date', type: 'date' }]),
		'[\n  {\n    "key": "date",\n    "label": "Date",\n    "type": "date"\n  }\n]'
	);
});

test('aggregate sources use a compact repository label', () => {
	assert.equal(sourceLabel('https://github.com/proxmox/qemu-server.git'), 'qemu-server');
});

const params = (query: string) => new URLSearchParams(query);

test('queued notice names the ref the collection was saved with', () => {
	assert.deepEqual(listNotice(params('notice=queued&name=docs&ref=release')), {
		key: 'rag-toast-indexing-queued',
		args: { name: 'docs', ref: 'release' }
	});
});

test('queued notice without a ref omits it instead of assuming main', () => {
	assert.deepEqual(listNotice(params('notice=queued&name=wiki')), {
		key: 'rag-toast-source-indexing-queued',
		args: { name: 'wiki' }
	});
});

test('created-aggregate and saved notices carry the name', () => {
	assert.deepEqual(listNotice(params('notice=created-aggregate&name=all')), { key: 'rag-toast-created-aggregate', args: { name: 'all' } });
	assert.deepEqual(listNotice(params('notice=saved&name=docs')), { key: 'rag-toast-collection-saved', args: { name: 'docs' } });
});

test('no or unknown notice yields nothing', () => {
	assert.strictEqual(listNotice(params('')), null);
	assert.strictEqual(listNotice(params('notice=bogus&name=x')), null);
});

test('RAG refresh intervals name their preset and keep a scripted value as minutes', () => {
	assert.deepEqual(refreshIntervalLabel(0), { key: 'rag-refresh-never', args: {} });
	assert.deepEqual(refreshIntervalLabel(60), { key: 'rag-refresh-hourly', args: {} });
	assert.deepEqual(refreshIntervalLabel(1440), { key: 'rag-refresh-daily', args: {} });
	assert.deepEqual(refreshIntervalLabel(10080), { key: 'rag-refresh-weekly', args: {} });
	assert.deepEqual(refreshIntervalLabel(720), { key: 'rag-refresh-custom', args: { mins: '720' } });
});
