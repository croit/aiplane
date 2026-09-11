import assert from 'node:assert/strict';
import test from 'node:test';
import { NEW_SKILL_TEMPLATE, selectedSkill, type PersonalSkill } from './skills.ts';

const skills: PersonalSkill[] = [
	{ name: 'alpha', title: 'Alpha', description: '', files: [] },
	{ name: 'beta', title: 'Beta', description: '', files: [] }
];

test('selectedSkill honors a valid query and otherwise selects the first skill', () => {
	assert.equal(selectedSkill(skills, 'beta')?.name, 'beta');
	assert.equal(selectedSkill(skills, 'missing')?.name, 'alpha');
	assert.equal(selectedSkill([], null), null);
});

test('the new-skill template is immediately valid and editable', () => {
	assert.match(NEW_SKILL_TEMPLATE, /^---\nname: my-skill\n/);
	assert.match(NEW_SKILL_TEMPLATE, /description: .+\n---/);
});
