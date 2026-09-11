import test from 'node:test';
import assert from 'node:assert';
import { parseList, parseSources, profileFieldsJson, sourceLabel } from './rag.ts';

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
