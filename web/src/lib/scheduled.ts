export type ScheduleMode = 'hourly' | 'daily' | 'weekly' | 'monthly' | 'advanced';

export interface ScheduleFields {
	mode: ScheduleMode;
	minute: number;
	hour: number;
	dayOfMonth: number;
	weekdays: number[];
	advanced: string;
}

import type { Dot } from './dot-state.ts';
import type { DotBinding } from './dot-background.ts';
import type { AgentVersion } from './agents.ts';
import { adminJson, adminPost, adminPut, adminDelete } from './admin-client.ts';

export interface ScheduledAction extends DotBinding {
	id: string;
	name: string;
	prompt: string;
	model: string | null;
	principal_id: string | null;
	agent_version: number | null;
	configuration_generation: string;
	cron: string;
	schedule_summary: string;
	timezone: string;
	tools_enabled: boolean;
	reuse_conversation: boolean;
	reuse_rounds: number;
	enabled: boolean;
	next_run_at: string | null;
	last_run_at: string | null;
	last_session_id: string | null;
	/** That chat has since been deleted; list responses only. */
	last_chat_deleted?: boolean;
	last_status: string | null;
	last_error: string | null;
	/** Recorded fires. */
	run_count: number;
	/** Distinct chats those fires opened — one, for a reusing schedule. */
	chat_count: number;
}

/** One recorded fire, as `/api/v0/scheduled/{id}/runs` returns it. */
export interface ScheduledRun {
	agent_version: number | null;
	configuration_generation: string | null;
	id: string;
	fired_at: string;
	/** `null` only while a run is still in flight. */
	status: string | null;
	session_id: string | null;
	assistant_turn_id: string | null;
	/** The run opened a chat the user has since deleted. */
	chat_deleted: boolean;
	error: string | null;
}

export interface ScheduledData {
	actions: ScheduledAction[];
	dots: Dot[];
	models: ChatModelOption[];
	default_timezone: string | null;
	principal_id: string | null;
	can_write: boolean;
	versions: AgentVersion[];
}

export interface ScheduledBody {
	name: string;
	prompt: string;
	cron: string;
	timezone: string;
	tools_enabled: boolean;
	reuse_conversation: boolean;
	reuse_rounds: number;
	model?: string;
	dot_id?: string | null;
	principal_id?: string;
	agent_version?: number;
	linked_session_id?: string | null;
}

export const scheduledApi = {
	list: (principalId?: string | null) => adminJson<ScheduledData>(`/api/v0/scheduled${principalId ? `?principal_id=${encodeURIComponent(principalId)}` : ''}`),
	create: (body: ScheduledBody) => adminPost('/api/v0/scheduled', body),
	update: (id: string, body: ScheduledBody & { expected_configuration_generation: string }) => adminPut(`/api/v0/scheduled/${encodeURIComponent(id)}`, body),
	toggle: (id: string, enabled: boolean, generation: string) => adminPost<{ enabled: boolean; configuration_generation: string }>(`/api/v0/scheduled/${encodeURIComponent(id)}/toggle`, { enabled, expected_configuration_generation: generation }),
	remove: (id: string) => adminDelete(`/api/v0/scheduled/${encodeURIComponent(id)}`),
	runs: (id: string) => adminJson<{ action: ScheduledAction; runs: ScheduledRun[] }>(`/api/v0/scheduled/${encodeURIComponent(id)}/runs`),
	preview: (cron: string, timezone: string) => adminPost<{ summary: string; upcoming: string[] }>('/api/v0/scheduled/preview', { cron, timezone })
};

export function scheduledVersionModel(versions: AgentVersion[], version: number | null): string | null {
	const model = versions.find((candidate) => candidate.version === version)?.spec.main?.model;
	return typeof model === 'string' && model.trim() ? model : null;
}

export const defaultSchedule = (): ScheduleFields => ({
	mode: 'daily', minute: 0, hour: 9, dayOfMonth: 1, weekdays: [], advanced: ''
});

export function cronFromSchedule(schedule: ScheduleFields): string | null {
	const minute = Math.min(59, Math.max(0, schedule.minute));
	const hour = Math.min(23, Math.max(0, schedule.hour));
	if (schedule.mode === 'hourly') return `${minute} * * * *`;
	if (schedule.mode === 'daily') return `${minute} ${hour} * * *`;
	if (schedule.mode === 'weekly') return schedule.weekdays.length ? `${minute} ${hour} * * ${schedule.weekdays.join(',')}` : null;
	if (schedule.mode === 'monthly') return `${minute} ${hour} ${Math.min(31, Math.max(1, schedule.dayOfMonth))} * *`;
	return schedule.advanced.trim() || null;
}

export function scheduleFromCron(cron: string): ScheduleFields {
	const fields = cron.trim().split(/\s+/);
	if (fields.length !== 5) return { ...defaultSchedule(), mode: 'advanced', advanced: cron };
	const [minute, hour, dayOfMonth, month, dayOfWeek] = fields;
	const parsedMinute = Number(minute);
	const parsedHour = Number(hour);
	if (Number.isInteger(parsedMinute) && hour === '*' && dayOfMonth === '*' && month === '*' && dayOfWeek === '*') {
		return { ...defaultSchedule(), mode: 'hourly', minute: parsedMinute };
	}
	if (Number.isInteger(parsedMinute) && Number.isInteger(parsedHour) && month === '*') {
		if (dayOfMonth === '*' && dayOfWeek === '*') return { ...defaultSchedule(), mode: 'daily', minute: parsedMinute, hour: parsedHour };
		if (dayOfMonth === '*' && dayOfWeek !== '*') return { ...defaultSchedule(), mode: 'weekly', minute: parsedMinute, hour: parsedHour, weekdays: dayOfWeek.split(',').map(Number) };
		const parsedDay = Number(dayOfMonth);
		if (Number.isInteger(parsedDay) && dayOfWeek === '*') return { ...defaultSchedule(), mode: 'monthly', minute: parsedMinute, hour: parsedHour, dayOfMonth: parsedDay };
	}
	return { ...defaultSchedule(), mode: 'advanced', advanced: cron };
}

export function formatScheduledRun(value: string, locale: string, timezone: string): string {
	const date = new Date(value);
	if (Number.isNaN(date.getTime())) return '';
	const parts = new Intl.DateTimeFormat(locale, {
		weekday: 'short', month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit',
		hourCycle: 'h23', timeZone: timezone
	}).formatToParts(date);
	const part = (type: Intl.DateTimeFormatPartTypes) => parts.find((candidate) => candidate.type === type)?.value ?? '';
	return `${part('weekday')} ${part('month')} ${part('day')}, ${part('hour')}:${part('minute')}`;
}
import type { ChatModelOption } from './model-option';
