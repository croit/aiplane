import test from 'node:test';
import assert from 'node:assert/strict';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';

function svelteFiles(dir: string, out: string[] = []): string[] {
	for (const entry of readdirSync(dir)) {
		const path = join(dir, entry);
		if (statSync(path).isDirectory()) svelteFiles(path, out);
		else if (entry.endsWith('.svelte')) out.push(path);
	}
	return out;
}

test('application pages use the full width supplied by the shared shell', () => {
	const root = new URL('../..', import.meta.url).pathname;
	const offenders = svelteFiles(join(root, 'src', 'routes')).flatMap((file) => {
		const body = readFileSync(file, 'utf8');
		return body.split('\n').flatMap((line, index) =>
			/max-w-(5xl|6xl)/.test(line)
				? [`${file.slice(root.length)}:${index + 1}`]
				: []
		);
	});

	assert.deepEqual(
		offenders,
		[],
		`Page-sized max-width constraints waste the application shell's available width:\n${offenders.join('\n')}`
	);
});
