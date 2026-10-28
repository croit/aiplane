/**
 * One agent being edited, shared by every page under `/agents/[id]`: the
 * overview and tabs, the setup assistant at `/agents/[id]/setup/[step]`, and
 * the advanced editor. The layout creates it once and hands it down through
 * context, so moving between those pages keeps the unsaved buffer, and a
 * reload simply loads the saved draft again.
 *
 * The spec being edited is a buffer; `save()` sends it, and the server's
 * validation answer is kept as `issues` beside the fields it is about. The
 * test chat and Publish act on what is saved, so both save first when the
 * buffer is ahead.
 */
import { getContext, setContext } from 'svelte';
import {
	agentsApi,
	cleanSpec,
	ensureShape,
	grantable,
	liveIssues,
	toolTitle,
	type AgentDetail,
	type AgentError,
	type AgentResources,
	type AgentSummary,
	type AgentVersion,
	type AssistSuggestion,
	type Granted,
	type GrantKind,
	type Spec,
	type SpecIssue,
	type TestDebug
} from './agents.ts';
import { setupErrorMessage } from './agent-setup.ts';
import { t } from './i18n.svelte';
import { emptyPlan, isEmpty, plannedGrants, stageGrant, stageRevoke, type GrantPlan } from './agent-grant-plan.ts';
import { readProposal, writeProposal } from './proposal-memory.ts';

const NOTICE_MS = 4000;

export class AgentWorkspace {
	readonly id: string;
	detail = $state<AgentDetail | null>(null);
	versions = $state<AgentVersion[]>([]);
	resources = $state<AgentResources | null>(null);
	/** Other agents, the hand-off targets. */
	agents = $state<AgentSummary[]>([]);
	spec = $state<Spec>(ensureShape({}));
	savedJson = $state('');
	/** Bumped whenever the buffer is replaced from outside a form, so forms re-read it. */
	formKey = $state(0);
	issues = $state<SpecIssue[]>([]);
	loading = $state(true);
	loadError = $state<string | null>(null);
	error = $state<string | null>(null);
	/** A catalog key and its arguments: the page translates it, so a language switch follows. */
	notice = $state<{ key: string; args?: Record<string, string | number> } | null>(null);
	busy = $state(false);
	lastDebug = $state<TestDebug | null>(null);
	/** What the person told the prompt assistant; kept with the proposal across a reload. */
	scenario = $state('');
	/** The prompt assistant's latest proposal (#117), offered step by step until applied or dismissed. */
	suggestion = $state<AssistSuggestion | null>(null);
	/** Proposal parts already applied or dismissed (`task`, `tone`, `scope`, …). */
	handled = $state<string[]>([]);
	/** Grant changes the steps staged; carried out by `save`, dropped by `discardGrants`. */
	plan = $state<GrantPlan>(emptyPlan());

	readonly writable = $derived(this.detail?.access === 'write');
	readonly dirty = $derived(JSON.stringify(cleanSpec(this.spec)) !== this.savedJson || !isEmpty(this.plan));
	/** The agent's grants as they will be once the staged plan is saved: what the steps show. */
	readonly grants = $derived(plannedGrants(this.detail?.grants ?? [], this.plan));
	/** Only what still points into the draft: removing the route a refusal was about clears it. */
	readonly shownIssues = $derived(liveIssues(this.issues, this.spec));
	readonly staleRefusal = $derived(this.issues.length > 0 && this.shownIssues.length === 0);
	readonly granted = $derived.by((): Granted => {
		const grants = this.detail?.grants ?? [];
		const connectorTools = grants
			.filter((g) => g.kind === 'connector')
			.flatMap((g) => grantable(this.resources, 'connector').find((c) => c.grant.refs.includes(g.ref))?.tools ?? []);
		return {
			models: grants.filter((g) => g.kind === 'model').map((g) => g.ref),
			defaults: this.resources?.defaults ?? null,
			tools: [...grants.filter((g) => g.kind === 'tool').map((g) => g.ref), ...connectorTools].map((tid) => ({ id: tid, name: toolTitle(this.resources, tid) })),
			skills: grants.filter((g) => g.kind === 'skill').map((g) => g.ref)
		};
	});

	/** Whether the proposal still offers `part`. */
	offers(part: keyof AssistSuggestion['steps']): boolean {
		const value = this.suggestion?.steps[part];
		const present = Array.isArray(value) ? value.length > 0 : !!value;
		return present && !this.handled.includes(part);
	}

	propose(suggestion: AssistSuggestion) {
		this.suggestion = suggestion;
		this.handled = [];
	}

	settle(part: string) {
		if (!this.handled.includes(part)) this.handled = [...this.handled, part];
	}

	constructor(id: string) {
		this.id = id;
		const kept = typeof sessionStorage === 'undefined' ? null : readProposal(sessionStorage, id);
		if (kept) {
			this.scenario = kept.scenario;
			this.suggestion = kept.suggestion;
			this.handled = kept.handled;
		}
	}

	/** Keeps the proposal for this tab, so a reload or a step opened by its URL still offers it. */
	remember() {
		if (typeof sessionStorage === 'undefined') return;
		writeProposal(sessionStorage, this.id, { scenario: this.scenario, suggestion: this.suggestion, handled: this.handled });
	}

	private adopt(next: AgentDetail) {
		this.detail = next;
		const shaped = ensureShape(next.draft_spec);
		this.spec = shaped;
		this.savedJson = JSON.stringify(cleanSpec(shaped));
		this.formKey++;
	}

	async refresh(keepBuffer = false) {
		const [next, vs] = await Promise.all([agentsApi.get(this.id), agentsApi.versions(this.id)]);
		this.versions = vs.versions;
		if (keepBuffer) this.detail = next;
		else this.adopt(next);
	}

	async load() {
		try {
			await this.refresh();
			const [r, a] = await Promise.all([agentsApi.resources(this.id), agentsApi.list()]);
			this.resources = r;
			this.agents = a.agents.filter((x) => x.id !== this.id);
		} catch (err) {
			this.loadError = setupErrorMessage(err as AgentError, t);
		} finally {
			this.loading = false;
		}
	}

	fail(err: unknown) {
		const e = err as AgentError;
		this.error = setupErrorMessage(e, t);
		this.issues = e.issues ?? [];
	}

	/**
	 * Saves the draft with the staged grants: grants first (the validator
	 * checks the draft against them), revocations after (the saved draft no
	 * longer uses them). A refused grant stops before the draft is sent, with
	 * the server's reason; what was already granted stays and leaves the plan.
	 */
	async save(): Promise<boolean> {
		this.busy = true;
		this.error = null;
		this.notice = null;
		try {
			for (const r of [...this.plan.grant]) {
				await agentsApi.grant(this.id, r.kind, r.ref);
				this.plan = { ...this.plan, grant: this.plan.grant.filter((x) => x !== r) };
			}
			const clean = cleanSpec(this.spec);
			await agentsApi.saveDraft(this.id, clean);
			this.savedJson = JSON.stringify(clean);
			for (const r of [...this.plan.revoke]) {
				await agentsApi.revokeGrant(this.id, r.kind, r.ref);
				this.plan = { ...this.plan, revoke: this.plan.revoke.filter((x) => x !== r) };
			}
			this.issues = [];
			await this.refresh(true);
			this.flash({ key: 'agents-saved' });
			return true;
		} catch (err) {
			this.fail(err);
			await this.refresh(true).catch(() => {});
			return false;
		} finally {
			this.busy = false;
		}
	}

	private noticeTimer: ReturnType<typeof setTimeout> | undefined;

	/** A success notice that clears itself, so it does not stay above every later step. */
	private flash(notice: NonNullable<AgentWorkspace['notice']>) {
		clearTimeout(this.noticeTimer);
		this.notice = notice;
		this.noticeTimer = setTimeout(() => (this.notice = null), NOTICE_MS);
	}

	/** Drops the staged grant changes (a cancelled edit). */
	discardGrants() {
		this.plan = emptyPlan();
	}

	/** Replaces the buffer (an applied template, the JSON tab, a modal's Apply) and re-reads the forms. */
	replace(next: Spec) {
		this.spec = ensureShape(next);
		this.formKey++;
	}

	async publish(): Promise<number> {
		if (this.dirty && !(await this.save())) throw new Error('save failed');
		this.busy = true;
		this.error = null;
		this.notice = null;
		try {
			const result = await agentsApi.publish(this.id);
			this.issues = [];
			await this.refresh(true);
			this.flash({ key: 'agents-published', args: { version: result.version } });
			return result.version;
		} catch (err) {
			this.fail(err);
			throw err;
		} finally {
			this.busy = false;
		}
	}

	async madeLive(version: number) {
		this.error = null;
		this.flash({ key: 'agents-live-is', args: { version } });
		await this.refresh(true);
	}

	/** Stages granting `ref` to the agent; nothing happens until `save`. */
	stageGrant(kind: GrantKind, ref: string) {
		this.plan = stageGrant(this.plan, this.detail?.grants ?? [], { kind, ref });
	}

	/**
	 * Stages giving `ref` up: withdraws a staged grant, or revokes a held one
	 * on `save` unless the published version uses it. Returns whether the
	 * grant is kept for the live version.
	 */
	stageRevoke(kind: GrantKind, ref: string): boolean {
		const { plan, kept } = stageRevoke(this.plan, this.detail?.grants ?? [], this.detail?.live_spec ?? null, { kind, ref }, this.resources?.defaults);
		this.plan = plan;
		return kept;
	}
}

const KEY = Symbol('agent-workspace');

export function provideWorkspace(ws: AgentWorkspace): AgentWorkspace {
	return setContext(KEY, ws);
}

export function useWorkspace(): AgentWorkspace {
	return getContext<AgentWorkspace>(KEY);
}
