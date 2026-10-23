import assert from 'node:assert/strict';
import test from 'node:test';
import { NEW_SKILL_TEMPLATE, selectedSkill, skillArchiveFilename, type PersonalSkill } from './skills.ts';

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

test('the archive filename is the sanitized title with a .skill extension', () => {
	assert.equal(skillArchiveFilename({ name: '6f1e2c3d-aaaa-bbbb-cccc-000011112222', title: 'Invoice Export' }), 'Invoice-Export.skill');
	assert.equal(skillArchiveFilename({ name: 'release-helper', title: 'Release Helper' }), 'Release-Helper.skill');
});

test('the archive filename falls back to the name when the title has no usable content', () => {
	assert.equal(skillArchiveFilename({ name: 'uuid-name', title: '!!!' }), 'uuid-name.skill');
	assert.equal(skillArchiveFilename({ name: 'uuid-name', title: 'Überprüfung' }), 'uuid-name.skill');
});
