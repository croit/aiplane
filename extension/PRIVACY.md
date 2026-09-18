<!--
SPDX-License-Identifier: AGPL-3.0-only
Copyright (C) 2026 croit GmbH

This is the privacy policy the Chrome Web Store listing must link to. It has to
be reachable at a public URL — the store rejects a listing without one — so
publish it somewhere stable and put that URL in the dashboard's Privacy tab.

Keep it truthful and keep it current: the store's data disclosures are checked
against it, and a policy that promises less than the extension does is worse
than no policy at all.
-->

# Privacy policy — croit AIplane Browser Control

*Last updated: 2026-09-18*

## The short version

This extension sends your data to **one place: the AIplane you paired it
with yourself.** Whoever runs that AIplane holds your data.

The authors of this extension — croit GmbH — **receive nothing**. No page
content, no addresses, no screenshots, no usage statistics, no crash reports,
no identifiers. Not in aggregate, not anonymised, not at all. There is no
server belonging to us in this picture, because the extension never contacts
one. That is not a promise about how we behave with your data; it is a
statement that your data never reaches us in the first place.

AIplane is software you or your organisation host. If you did not set up
AIplane yourself, the person or company that did is the one who decides
what happens to what the assistant reads — ask them for their privacy policy,
not ours.

## What the extension sends, and when

Nothing at all until two things are true: you have **paired** an AIplane in the
extension's settings, and you have **switched the extension on** from its
toolbar icon. Switching it off, or closing Chrome, ends that immediately.

While it is on, and only for a conversation you are having in that AIplane,
the extension can send AIplane:

- **The content of pages the assistant works on** — their text and structure,
  the addresses they are at, the titles of the tabs, and screenshots when the
  assistant asks for one.
- **What the assistant did** — the steps it carried out and whether they
  worked.

This is what makes the feature work: an assistant that cannot see the page
cannot act on it. It is also the reason to switch the extension off when you
are done.

The assistant acts **with your logins**. It uses the browser you are already
signed in to, so anything you can reach while signed in, it can reach while it
is switched on.

## What stays on your computer

- **Which AIplane servers you paired**, in Chrome's extension storage, until you
  remove them.
- **Whether the extension is switched on**, and for which AIplane, until you
  switch it off or close Chrome.
- **A short activity log** — the last steps carried out, kept so you can see
  what happened in the popup. It never leaves your browser and is discarded
  when Chrome closes.

None of this is sent anywhere.

## What AIplane records

AIplane keeps an audit trail of what the assistant did in your browser: who
asked, which conversation, which action, and whether it succeeded. It
deliberately does **not** store the addresses visited, the text typed, or the
screenshots taken.

The conversation itself — including page content the assistant read, and any
screenshots — is stored by AIplane like the rest of your conversation, and
is passed to whichever AI model that AIplane is configured to use. Which model,
and under whose terms, is AIplane operator's decision. That is the part
worth asking them about.

## What this extension never does

- It does not collect analytics, telemetry or usage statistics.
- It does not send anything to the authors, or to any third party.
- It does not sell, rent or share your data with anyone.
- It does not read your browsing history, your bookmarks, or your saved
  passwords.
- It does not run code sent to it. AIplane sends a fixed set of named
  actions, which the extension checks against a list it ships with; anything
  it does not recognise is refused.
- It does not act while switched off.

## Limited use

The extension's use of information received through Google APIs adheres to the
[Chrome Web Store User Data Policy](https://developer.chrome.com/docs/webstore/program-policies/limited-use),
including the Limited Use requirements. Data is used only to provide the
feature described above — letting a conversation in *your* AIplane act in your
browser — and is transmitted only to the AIplane you paired.

## Permissions, and why each one exists

| Permission | Why |
|---|---|
| `debugger` | To operate pages the way you do: real mouse and keyboard events, device emulation, full-page screenshots. Chrome offers no other way to produce input a website treats as genuine. See the note below. |
| `scripting` | To place the small bridge script on AIplane's own page, and to read the structure of pages the assistant works on. |
| `tabs` | To open and find the tab the assistant works in, and to report open tabs when you ask it to. |
| `tabGroups` | To keep the assistant's tab in its own group, out of the way of your own tabs. |
| `storage` | To remember the paired AIplane, the on/off state and the activity log — all locally. |
| Access to websites (optional) | Granted by you when you switch the extension on, and only then. Without it the assistant can see nothing. |

**About `debugger`:** Chrome shows a banner while it is in use — *"… started
debugging this browser"* — and that banner is a feature, not a nuisance: it
tells you, in every tab, that an assistant can act right now. We use this
interface because the alternatives cannot do the job. Events created by an
ordinary extension are marked as untrusted, and browsers refuse them for file
uploads, rich text editors and anything else that requires a genuine user
action. It is the same interface used by comparable assistants.

## Contact

Questions about the extension itself: raise an issue in the project's public
repository. Questions about what happens to your data: ask whoever runs the
AIplane you paired — they hold it, and only they can answer.
