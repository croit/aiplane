/**
 * The feedback widget state (issue #22 P5): the SPA port of the legacy
 * feedback-capture TS. The endpoints are already JSON
 * (`GET /feedback/config`, `POST /feedback/extract`, `POST /feedback`) and
 * stay where they are — outside `/api/v0` by design (the drift spec scopes
 * to `/api/v0` only).
 */
export const feedback = $state({
	open: false,
	enabled: false,
	title: '',
	description: '',
	business: '',
	acceptance: '',
	priority: 'medium',
	busy: false,
	submitted: false,
	error: null as string | null
});

export async function loadConfig() {
	try {
		const res = await fetch('/api/v0/feedback/config', { headers: { accept: 'application/json' } });
		if (!res.ok) return;
		const cfg = (await res.json()) as { enabled: boolean };
		feedback.enabled = cfg.enabled;
	} catch {
		/* widget stays hidden */
	}
}

export function openDialog() {
	if (!feedback.enabled) return;
	feedback.open = true;
	feedback.submitted = false;
	feedback.error = null;
}

export async function submit() {
	if (feedback.busy || !feedback.title.trim() || !feedback.description.trim()) return;
	feedback.busy = true;
	feedback.error = null;
	try {
		const res = await fetch('/api/v0/feedback', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({
				title: feedback.title,
				description: feedback.description,
				business_value: feedback.business,
				acceptance_criteria: feedback.acceptance,
				priority: feedback.priority
			})
		});
		if (!res.ok) throw new Error((await res.text()).slice(0, 200));
		feedback.submitted = true;
	} catch (err) {
		feedback.error = String(err);
	} finally {
		feedback.busy = false;
	}
}
