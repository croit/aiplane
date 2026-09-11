export interface PersonalSkill {
	name: string;
	title: string;
	description: string;
	files: string[];
}

export const NEW_SKILL_TEMPLATE = `---
name: my-skill
title: My Skill
description: One line describing when the assistant should use this skill.
---

# Instructions

Write what the assistant should do when this skill is loaded.
`;

export function selectedSkill(skills: PersonalSkill[], requested: string | null): PersonalSkill | null {
	return skills.find((skill) => skill.name === requested) ?? skills[0] ?? null;
}
