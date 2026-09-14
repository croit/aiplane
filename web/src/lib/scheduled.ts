export type ScheduleMode = 'hourly' | 'daily' | 'weekly' | 'monthly' | 'advanced';

export interface ScheduleFields {
	mode: ScheduleMode;
	minute: number;
	hour: number;
	dayOfMonth: number;
	weekdays: number[];
	advanced: string;
}

export interface ScheduledAction {
	id: string;
	name: string;
	prompt: string;
	model: string;
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
	last_status: string | null;
	last_error: string | null;
	/** Recorded fires. */
	run_count: number;
	/** Distinct chats those fires opened — one, for a reusing schedule. */
	chat_count: number;
}

/** One recorded fire, as `/api/v0/scheduled/{id}/runs` returns it. */
export interface ScheduledRun {
	id: string;
	fired_at: string;
	/** `null` only while a run is still in flight. */
	status: string | null;
	session_id: string | null;
	error: string | null;
}

/** Where a schedule's row sends someone who wants to read what it produced. */
export interface ScheduleLinks {
	/** The single conversation, when the schedule has exactly one. */
	chat: string | null;
	/** The run history, when there is more there than the `chat` link shows. */
	runs: string | null;
}

/**
 * The links a schedule's row offers.
 *
 * A schedule that reuses one conversation has exactly one chat however often
 * it has fired, so the row goes straight into it — a history of identical
 * links would be a detour. A schedule that opens a fresh chat each time has a
 * list, and that list is the history page. Runs that produced no chat at all
 * (the owner was over quota, the model never answered) are still runs worth
 * seeing, so the history stays reachable whenever it holds anything the chat
 * link does not already show.
 */
export function scheduleLinks(action: ScheduledAction): ScheduleLinks {
	const chat = action.chat_count <= 1 && action.last_session_id ? `/chat/${action.last_session_id}` : null;
	const everythingIsInTheChatLink = chat !== null && action.run_count <= 1;
	return {
		chat,
		runs: action.run_count > 0 && !everythingIsInTheChatLink ? `/scheduled/${action.id}/runs` : null
	};
}

export interface ScheduledData {
	actions: ScheduledAction[];
	models: ChatModelOption[];
	default_timezone: string;
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
