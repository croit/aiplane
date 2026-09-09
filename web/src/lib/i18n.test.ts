// The i18n contract: catalogs stay in step, and lookup degrades gracefully.
//
// The server-rendered stack had a Rust test asserting the Fluent catalogs
// agreed with each other. That guard died with it, and a catalog silently
// missing keys is exactly the kind of regression nobody notices in the
// language they read — the app looks fine in English while a German user gets
// raw keys. This is its replacement, on the generated TypeScript side.

import test from 'node:test';
import assert from 'node:assert';

import { en } from './locales/en.ts';
import { de } from './locales/de.ts';
import { fr } from './locales/fr.ts';
import { es } from './locales/es.ts';
import { ru } from './locales/ru.ts';
import { zh } from './locales/zh.ts';

const CATALOGS = { de, fr, es, ru, zh };

test('every catalog carries exactly the English key set', () => {
	const base = Object.keys(en).sort();
	for (const [lang, catalog] of Object.entries(CATALOGS)) {
		const keys = Object.keys(catalog).sort();
		const missing = base.filter((k) => !(k in catalog));
		const extra = keys.filter((k) => !(k in en));
		assert.deepEqual(
			missing,
			[],
			`${lang} is missing ${missing.length} key(s) — a reader of that language ` +
				`would see raw keys where English readers see text: ${missing.slice(0, 8).join(', ')}`
		);
		assert.deepEqual(
			extra,
			[],
			`${lang} has ${extra.length} key(s) English does not — either a typo, or a ` +
				`string that was never added to the source language: ${extra.slice(0, 8).join(', ')}`
		);
	}
});

test('a message never renders empty', () => {
	for (const [lang, catalog] of Object.entries({ en, ...CATALOGS })) {
		for (const [key, value] of Object.entries(catalog)) {
			if (typeof value === 'string') {
				assert.notEqual(value.trim(), '', `${lang}/${key} is blank`);
			} else {
				assert.ok(
					Object.keys(value).length > 0,
					`${lang}/${key} has no variants`
				);
				for (const [variant, text] of Object.entries(value)) {
					assert.notEqual(text.trim(), '', `${lang}/${key}[${variant}] is blank`);
				}
			}
		}
	}
});

test('placeholders match across languages', () => {
	// A translation that drops `{count}` silently renders a sentence with a
	// hole in it; one that invents a placeholder renders the literal braces.
	const names = (msg: string) => [...msg.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();
	const flatten = (v: string | Record<string, string>) =>
		typeof v === 'string' ? [v] : Object.values(v);

	for (const [lang, catalog] of Object.entries(CATALOGS)) {
		for (const [key, value] of Object.entries(en)) {
			const translated = catalog[key];
			if (translated === undefined) continue; // covered by the key-set test
			const expected = new Set(flatten(value).flatMap(names));
			const actual = new Set(flatten(translated).flatMap(names));
			for (const name of actual) {
				assert.ok(
					expected.has(name),
					`${lang}/${key} uses {${name}}, which the English message does not provide — ` +
						`it would render as literal braces`
				);
			}
		}
	}
});
