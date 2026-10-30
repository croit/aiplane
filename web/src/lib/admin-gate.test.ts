// Who is an administrator is the server's `is_admin` flag (`Me.is_admin`,
// `account.is_admin`), never a group name: the setup wizard names its group
// `admins`, operators name theirs freely, and the break-glass group is
// `__bootstrap_admin__`. The dev-UI seed happens to call its group `admin`,
// so a name check passes locally and hides the admin UI everywhere else.

import test from 'node:test';
import assert from 'node:assert';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';

function sources(dir: string, out: string[] = []): string[] {
	for (const entry of readdirSync(dir)) {
		if (entry === 'node_modules') continue;
		const path = join(dir, entry);
		if (statSync(path).isDirectory()) sources(path, out);
		else if (/\.(svelte|ts|js)$/.test(entry) && !entry.endsWith('.test.ts')) out.push(path);
	}
	return out;
}

// A group-name literal anywhere near the identity's groups: catches
// `.includes('admin')`, `.some((g) => g === 'admins')`, `indexOf("admin")`,
// without failing a membership check against a real group id.
const ADMIN_NAME = /(['"`])(admins?|__bootstrap_admin__)\1/;
const GROUPS = /\b(role_ids|rbac_roles|roles)\b/;

test('no web code decides admin rights by a group name', () => {
	const root = new URL('../..', import.meta.url).pathname;
	const offenders: string[] = [];
	for (const tree of ['src', 'shared', 'embed']) {
		for (const file of sources(join(root, tree))) {
			const lines = readFileSync(file, 'utf8').split('\n');
			lines.forEach((line, i) => {
				const window = lines.slice(Math.max(0, i - 2), i + 3).join('\n');
				if (ADMIN_NAME.test(line) && GROUPS.test(window)) offenders.push(`${file.slice(root.length)}:${i + 1}`);
			});
		}
	}
	assert.deepEqual(offenders, [], `read is_admin from the server instead:\n` + offenders.join('\n'));
});
