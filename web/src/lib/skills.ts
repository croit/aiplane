export interface PersonalSkill {
	name: string;
	title: string;
	description: string;
	files: string[];
}

/**
 * The download name of a skill's archive: its title made filename-safe,
 * falling back to its name when the title is non-ASCII (a header-safe
 * filename cannot spell it) or has no usable content. Mirrors the server's
 * `Skill::archive_filename`, so the browser-suggested name matches the
 * Content-Disposition in every browser.
 */
export function skillArchiveFilename(skill: { name: string; title: string }): string {
	const fromTitle = /^[\x00-\x7F]*$/.test(skill.title) ? sanitizeFilename(skill.title) : null;
	const base = fromTitle ?? sanitizeFilename(skill.name) ?? skill.name;
	return `${base}.skill`;
}

function sanitizeFilename(text: string): string | null {
	const collapsed = text
		.replace(/[^A-Za-z0-9.-]+/g, '-')
		.replace(/^[-.]+|[-.]+$/g, '')
		.slice(0, 100);
	return collapsed.length > 0 ? collapsed : null;
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
