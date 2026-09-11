import test from 'node:test';
import assert from 'node:assert/strict';
import { effectiveSkillGroups, selectedAdminSkill, skillFileTree } from './admin-skills.ts';
import type { AdminSkill } from './admin-skills.ts';

const skill: AdminSkill = {
	name: 'release-helper',
	title: 'Release Helper',
	description: 'Prepares a release.',
	files: ['template.txt', 'references/example.md', 'references/checklist.md'],
	body: '# Steps',
	all_skills_groups: ['admin'],
	granted_groups: ['user', 'admin']
};

test('admin skill selection falls back to the first loaded skill', () => {
	assert.equal(selectedAdminSkill([skill], 'missing'), skill);
	assert.equal(selectedAdminSkill([], null), null);
});

test('effective grants combine all-skill and direct grants without duplicates', () => {
	assert.deepEqual(effectiveSkillGroups(skill), ['admin', 'user']);
});

test('skill files are grouped into the production rail tree', () => {
	assert.deepEqual(skillFileTree(skill.files), [
		{ directory: null, files: ['template.txt'] },
		{ directory: 'references', files: ['example.md', 'checklist.md'] }
	]);
});
