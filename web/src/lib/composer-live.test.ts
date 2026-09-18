// Drift guard for the composer staying usable while a turn runs.
//
// Every contract below is one a rendered-DOM test cannot reach without a
// streaming server, and every one of them fails silently: the composer keeps
// looking right and quietly drops what the user typed.
//
//   1. The textarea is never `disabled`. That single attribute is what the
//      whole change exists to remove — re-adding it restores the old
//      swallowed-keystroke behaviour with nothing else breaking.
//   2. Enter during a turn queues instead of sending, and Ctrl/Cmd+Enter
//      interjects. Wire them to the wrong handler and messages either vanish
//      or land in the wrong place.
//   3. The queue is drained when a turn finalises AND when the page loads
//      with entries restored from storage — the second one no event
//      announces, so a queue restored into an idle conversation would sit
//      there forever.
//   4. Interjections render with their outcome. A note shown without saying
//      whether the model read it is the one failure mode the three-way status
//      exists to prevent.

import test from 'node:test';
import assert from 'node:assert';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

const ROOT = new URL('../..', import.meta.url).pathname;
const read = (rel: string): string => readFileSync(join(ROOT, rel), 'utf8');
const conversation = read('src/lib/Conversation.svelte');

/** The composer's textarea element, start tag only. */
function composerTextarea(source: string): string {
	const start = source.indexOf('<textarea');
	assert.ok(start >= 0, 'the composer has a textarea');
	const end = source.indexOf('></textarea>', start);
	assert.ok(end > start, 'the textarea tag is closed');
	return source.slice(start, end);
}

test('the composer textarea is never disabled', () => {
	const tag = composerTextarea(conversation);
	assert.ok(tag.includes('bind:value={draft}'), 'sanity: found the composer textarea');
	assert.ok(
		!/\bdisabled=/.test(tag),
		'the composer must stay usable while a turn streams — that is the whole point'
	);
});

test('Enter submits through the branch that queues during a turn', () => {
	assert.ok(
		/function onKeydown[\s\S]*?submitComposer\(\)/.test(conversation),
		'Enter goes through submitComposer, which queues while streaming'
	);
	assert.ok(
		/function submitComposer[\s\S]*?if \(streaming\)[\s\S]*?enqueueDraft\(\)/.test(conversation),
		'submitComposer queues rather than sending while a turn is live'
	);
});

test('Ctrl/Cmd+Enter during a turn interjects instead of queueing', () => {
	assert.ok(
		/if \(streaming && \(event\.metaKey \|\| event\.ctrlKey\)\)[\s\S]*?interject\(\)/.test(
			conversation
		),
		'the modifier is what tells "to the running answer" from "to the next one"'
	);
});

test('the interject action posts to the steer endpoint, not the message endpoint', () => {
	assert.ok(
		/async function interject\(\)[\s\S]*?api\.steerChatTurn\(id,/.test(conversation),
		'interjecting goes to steerChatTurn'
	);
	const api = read('src/lib/api.ts');
	assert.ok(api.includes('/steer`'), 'the client knows the steer endpoint');
});

test('interrupting re-aims: it queues the draft before stopping the turn', () => {
	const fn = /async function interruptAndReaim\(\)[\s\S]*?\n\t\}/.exec(conversation)?.[0] ?? '';
	assert.ok(fn.includes('enqueueDraft()'), 'the typed text is kept');
	assert.ok(fn.includes('await stop()'), 'and the running answer is stopped');
	assert.ok(
		fn.indexOf('enqueueDraft()') < fn.indexOf('await stop()'),
		'queue first: stopping fires the finalize hook that drains the queue'
	);
});

test('the queue is drained both on finalize and on a restored page', () => {
	assert.ok(
		/onTurnFinalized = \(\) => \{[\s\S]*?flushQueue\(\)/.test(conversation),
		'a finished turn sends what is waiting'
	);
	assert.ok(
		/\$effect\(\(\) => \{\s*if \(!streaming && model\.trim\(\) && queue\.length > 0\) void flushQueue\(\)/.test(
			conversation
		),
		'a queue restored from storage into an idle conversation must go out too'
	);
});

// `model` is empty until loadModels() resolves, and an idle conversation fires
// no turn_finalized to try again — so reading `model` inside the effect is what
// makes the model arriving re-trigger the drain. Without it a restored queue
// sat visible and unsent forever.
test('the drain depends on the model, not just on the queue', () => {
	const effect =
		/\$effect\(\(\) => \{\s*if \(!streaming[\s\S]*?\}\);/.exec(conversation)?.[0] ?? '';
	assert.ok(effect.includes('model.trim()'), 'the effect reads model, so it re-runs when it lands');
});

// `at_capacity` means another of this user's chats holds the last slot: this
// conversation is idle, will never fire turn_finalized, and nothing else would
// ever retry the entry.
test('a capacity refusal schedules its own retry', () => {
	assert.ok(
		/if \(code === 'at_capacity'\) retryLater\(\)/.test(conversation),
		'at_capacity arms a timer'
	);
	assert.ok(
		/function retryLater\(\)[\s\S]*?setTimeout\([\s\S]*?flushQueue\(\)/.test(conversation),
		'and the timer drains again'
	);
	assert.ok(
		/if \(capacityTimer !== null\) clearTimeout\(capacityTimer\)/.test(conversation),
		'the timer dies with the component'
	);
});

// Discarding a returned interjection has to settle the row too: forgetting it
// locally leaves it `pending`, and the next finalize re-queues the dismissed
// sentence — which then sends itself.
test('discarding a returned interjection settles it on the server', () => {
	const fn = /async function discardQueued\([\s\S]*?\n\t\}/.exec(conversation)?.[0] ?? '';
	assert.ok(fn.includes('api.discardSteer(id, steerId)'), 'the row is settled, not just forgotten');
	assert.ok(
		/onclick=\{\(\) => void discardQueued\(entry\)\}/.test(conversation),
		'the discard button goes through it'
	);
});

// Editing used to drop the entry's text whenever the composer already held a
// draft, and to drop its interjection claim always — one silent loss and one
// duplicate send.
test('editing a queued entry keeps both the text and the claim', () => {
	const fn = /function editQueued\([\s\S]*?\n\t\}/.exec(conversation)?.[0] ?? '';
	assert.ok(fn.includes('draft.trim() ?'), 'an existing draft is kept');
	assert.ok(!/draft = draft\.trim\(\) \? draft :/.test(fn), 'and the entry text is not thrown away');
	assert.ok(fn.includes('draftRedeem'), 'the interjection claim travels with the text');
	const send = /async function send\(\)[\s\S]*?\n\t\}/.exec(conversation)?.[0] ?? '';
	assert.ok(
		send.includes('entry.redeemSteers = draftRedeem'),
		'and is carried by the entry that draft is sent as'
	);
});

// Two submit paths meant every refusal and every new field had to be
// remembered twice, and they had already drifted — only the queued path knew
// about at_capacity.
test('the composer and the queue submit through one function', () => {
	const send = /async function send\(\)[\s\S]*?\n\t\}/.exec(conversation)?.[0] ?? '';
	assert.ok(send.includes('sendQueued(entry)'), 'send() goes through the queued path');
	assert.ok(
		!send.includes('api.sendChatMessage') && !send.includes('FormData'),
		'send() does not submit on its own — that is what drifted before'
	);
	const sendQueued = /async function sendQueued\([\s\S]*?\n\t\}/.exec(conversation)?.[0] ?? '';
	assert.ok(
		!sendQueued.includes('FormData') && !sendQueued.includes('fetch('),
		'the submit path builds no request body of its own — api.ts owns that, and with it the typed errors'
	);
	assert.ok(
		sendQueued.includes('api.sendChatMessageWithFiles') && sendQueued.includes('api.sendChatMessage('),
		'both shapes go through the API client'
	);
});

// The multipart body has to be able to express a redeemed interjection, or
// editing one and attaching a file silently drops its claim and the note is
// queued again by the next finalize.
test('the multipart submit can carry a redeemed interjection', () => {
	const api = read('src/lib/api.ts');
	assert.ok(
		/fd\.append\('redeem_steer', steerId\)/.test(api),
		'the claim rides along with attachments'
	);
	// A hand-rolled fetch throws a plain Error, which silently disables every
	// `err.code` branch the caller has.
	assert.ok(
		/sendChatMessageWithFiles[\s\S]*?return request</.test(api),
		'and goes through request(), so a refusal arrives as ApiError with its code'
	);
});

// A restored entry whose attachment could not be stored must not send itself:
// "summarise the attached report" without the report is a guaranteed wrong
// answer.
// Four findings from one review, all the same shape: a claim that outlives
// the text it belonged to. The row then reads "re-sent" while the sentence it
// stood for was never sent anywhere.
test('a claim never outlives the text it belongs to', () => {
	const interject = /async function interject\(\)[\s\S]*?\n\t\}/.exec(conversation)?.[0] ?? '';
	assert.ok(
		interject.includes('api.steerChatTurn(id, text, redeem)'),
		'interjecting carries the claim to the server'
	);
	assert.ok(
		interject.includes("draftRedeem = []"),
		'and drops it only once the server has taken it'
	);
	assert.ok(
		/entry\.redeemSteers = redeem/.test(interject),
		'a 409 keeps the claim with the queued text rather than dropping it'
	);
	assert.ok(
		/\$effect\(\(\) => \{\s*if \(!draft\.trim\(\) && files\.length === 0 && draftRedeem\.length > 0\)/.test(
			conversation
		),
		'an emptied composer holds no claim'
	);
});

// `steer_already_settled` means the text is accounted for elsewhere. Treating
// it like a retryable refusal made send() put the entry back, the drain fire
// on the length change, and the message vanish from composer and queue alike.
test('the four send outcomes are distinguished, not collapsed to a boolean', () => {
	assert.ok(
		/type SendOutcome = 'sent' \| 'settled' \| 'wait' \| 'failed'/.test(conversation),
		'the outcomes are named'
	);
	const send = /async function send\(\)[\s\S]*?\n\t\}/.exec(conversation)?.[0] ?? '';
	assert.ok(
		/outcome === 'wait' \|\| outcome === 'failed'/.test(send),
		'only a retryable outcome puts the text back in the queue'
	);
	const flush = /async function flushQueue\(\)[\s\S]*?\n\t\}/.exec(conversation)?.[0] ?? '';
	assert.ok(
		/outcome === 'sent' \|\| outcome === 'settled'/.test(flush),
		'and both terminal outcomes leave the queue'
	);
});

// The drain reacts to the queue changing, so an entry that failed and was put
// back would immediately be retried — against a server that may well have
// accepted the request that timed out. That is how one message becomes two.
test('a failed send is held instead of retried in a loop', () => {
	const sendQueued = /async function sendQueued\([\s\S]*?\n\t\}/.exec(conversation)?.[0] ?? '';
	assert.ok(sendQueued.includes('entry.held = true'), 'a real failure holds the entry');
	assert.ok(sendQueued.includes('entry.error = String(err)'), 'and says why');
});

// An interjection cannot carry a file, so a composer holding one has nothing
// to interject with — the sentence would reach a model that never sees what it
// refers to, and the attachment would silently stay behind.
test('interjecting is refused while the composer holds attachments', () => {
	const interject = /async function interject\(\)[\s\S]*?\n\t\}/.exec(conversation)?.[0] ?? '';
	assert.ok(interject.includes('if (files.length > 0) return'), 'the keystroke declines');
	assert.ok(
		/disabled=\{!draft\.trim\(\) \|\| files\.length > 0 \|\| sending\}/.test(conversation),
		'and so does the button'
	);
});

// A note left pending by a tab that was closed mid-turn, or by a turn swept to
// errored at startup, is found on load — not only when some later turn in that
// conversation happens to finish.
test('undelivered interjections are recovered on load, not only on finalize', () => {
	assert.ok(
		/\$effect\(\(\) => \{\s*if \(!controller \|\| streaming\) return;\s*queue = withUndeliveredSteers\(queue, controller\.state\.turns\)/.test(
			conversation
		),
		'an idle conversation folds its stranded notes back into the queue'
	);
});

test('an entry that lost its attachment is held back, not sent', () => {
	assert.ok(conversation.includes('nextSendable(queue)'), 'the drain picks the first sendable entry');
	assert.ok(conversation.includes('chat-queue-held-hint'), 'and the page says why one is waiting');
});

// Found in a real browser, and it silently ate every queued message on
// reload: the effect that writes the queue back is created before `onMount`,
// so it ran first, saw the empty initial value and cleared storage before the
// restore could read it. The restore has to happen where the state is
// declared.
test('the queue is restored at initialisation, not from onMount', () => {
	assert.ok(
		/let queue = \$state<QueuedMessage\[\]>\(restoreQueue\(\)\)/.test(conversation),
		'the initial value is the restored queue'
	);
	assert.ok(
		!/onMount\(\(\) => \{[\s\S]*?restoreQueue\(\)/.test(conversation),
		'restoring in onMount races the persistence effect and loses the queue'
	);
	assert.ok(
		/function restoreQueue\(\)[\s\S]*?try \{[\s\S]*?catch/.test(conversation),
		'storage can throw (private windows, blocked site data) and must not break the page'
	);
});

// Two tabs of one conversation each restore the same stored queue and both
// drain when it goes idle, so without following the other tab's writes the
// second one re-sends a message the first already sent.
test('the queue follows what another tab does to it', () => {
	assert.ok(
		/addEventListener\('storage', onStorage\)/.test(conversation),
		'other tabs writing the queue are observed'
	);
	assert.ok(
		/removeEventListener\('storage', onStorage\)/.test(conversation),
		'and the listener dies with the component'
	);
});

// Also found in a real browser: clicking a composer button moves focus to it,
// so the next thing typed went to the button and vanished. Anything that
// empties the composer hands the cursor back.
test('the actions that empty the composer return focus to it', () => {
	assert.ok(
		/bind:this=\{composerInput\}/.test(conversation),
		'the textarea is reachable for focus()'
	);
	for (const fn of ['enqueueDraft', 'interject']) {
		const body = new RegExp(`function ${fn}\\(\\)[\\s\\S]*?\n\t\\}`).exec(conversation)?.[0] ?? '';
		assert.ok(body.includes('focusComposer()'), `${fn} must hand focus back`);
	}
});

test('an undelivered interjection is folded back into the queue when the turn ends', () => {
	assert.ok(
		/onTurnFinalized = \(\) => \{[\s\S]*?withUndeliveredSteers\(queue, c\.state\.turns\)/.test(
			conversation
		),
		'the fallback the honest "pending" status exists for'
	);
});

test('an interjection renders with what became of it', () => {
	for (const key of ['chat-steer-delivered', 'chat-steer-resent', 'chat-steer-pending']) {
		assert.ok(conversation.includes(key), `${key} is rendered`);
	}
	const en = read('src/lib/locales/en.ts');
	for (const key of ['chat-steer-delivered', 'chat-steer-resent', 'chat-steer-pending']) {
		assert.ok(en.includes(key), `${key} is translated`);
	}
});

test('the transcript draws interjections from the turn they belong to', () => {
	assert.ok(
		/\{#each entry\.steers as note \(note\.id\)\}/.test(conversation),
		'keyed on the row id so a status change re-labels rather than re-adds'
	);
});
