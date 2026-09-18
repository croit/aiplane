<!--
SPDX-License-Identifier: AGPL-3.0-only
Copyright (C) 2026 croit GmbH

Everything the Chrome Web Store dashboard asks for, written out so it can be
pasted rather than improvised at submission time — and so it can be reviewed
here, in the repository, where it is visible next to the code it describes.

The justifications below are the part that decides how the review goes. Two of
them (`debugger`, remote code) are the ones a reviewer will actually stop at;
they are written to be checkable against the source rather than persuasive.

Keep this in step with `manifest.json` and `PRIVACY.md`. A permission added to
the manifest without a justification here is a permission that will be
questioned, and a disclosure that contradicts the privacy policy is a rejection.
-->

# Chrome Web Store listing

## Name

    LLM Gateway Browser Control

75 characters is the limit; this is 27.

## Summary (132 characters max)

    Let a conversation in your own LLM Gateway read and operate pages in this browser — only while you switch it on.

111 characters.

## Category

Productivity / Workflow & Planning.

## Detailed description

    LLM Gateway Browser Control connects a conversation in your own LLM Gateway
    to the browser you are already using — with the sessions you are already
    signed in to.

    Ask the assistant to look something up in an internal tool, fill in a form,
    walk through a checkout, compare two pages, or take a screenshot of what it
    found. It works in a tab of its own, in its own tab group, so it stays out
    of the way of whatever you are doing.

    WHOSE GATEWAY

    LLM Gateway is open-source software that you or your organisation host. This
    extension talks to the gateway you pair it with and to nothing else. The
    authors of this extension receive no data from it at all — no page content,
    no addresses, no statistics. Whoever runs your gateway holds your data.

    IT IS OFF UNTIL YOU SWITCH IT ON

    Nothing happens until you pair a gateway and switch the extension on from
    its toolbar icon. The icon is coloured while it is on and grey while it is
    off, Chrome shows its own banner across every tab, and every step is listed
    in the popup. Switching it off releases everything at once, and so does
    closing Chrome.

    WHAT IT CAN DO

    Navigate, read a page, find things on it, click, hover, drag, type, press
    keys, scroll, take screenshots, emulate a phone or a desktop viewport, wait
    for something to appear, and list open tabs.

    WHAT IT CANNOT DO

    Run code sent from anywhere. The gateway sends a fixed set of named actions
    that this extension checks against a list it ships with; anything else is
    refused. It also cannot switch itself on — that takes a click on the
    extension's own button, which no web page can produce.

    WORTH KNOWING

    A page the assistant reads can contain text aimed at the assistant. Grant
    the sites you would let an assistant work in, and switch it off when you are
    done.

    Source code and documentation: see the project repository.

## Single purpose

    Let a conversation in a user's own LLM Gateway read and operate web pages in
    the user's browser, on the user's behalf and only while the user has
    switched the extension on.

## Permission justifications

One per declared permission. The dashboard asks for each separately.

### `debugger`

    The extension operates web pages on the user's behalf: clicking, typing,
    scrolling, emulating a mobile or desktop viewport, and capturing full-page
    screenshots. The Chrome DevTools Protocol is the only interface that can do
    this.

    Events created with chrome.scripting are marked isTrusted=false. Chrome and
    websites reject them for exactly the cases this feature exists to cover: a
    file input refuses to open ("File chooser dialog can only be shown with a
    user activation"), rich text editors ignore synthetic key events, and every
    API behind user activation — clipboard, fullscreen, pickers — is unavailable.
    Device metrics emulation (setDeviceMetricsOverride) has no chrome.* API at
    all, and chrome.tabs.captureVisibleTab captures only the visible part of the
    active tab, which cannot produce the full-page screenshot the assistant
    needs to describe a page.

    The debugger is attached only while the user has switched the extension on,
    only to the tab the assistant works in, and is detached the moment the user
    switches it off. Chrome's own "started debugging this browser" banner is
    left in place deliberately, as a visible signal that the assistant can act.

### `scripting`

    Two uses. First, registering a small bridge script on the origins of
    gateways the user paired, which is how the gateway's page reaches the
    extension; these origins are not known at publish time because a gateway is
    self-hosted on the user's own domain, so externally_connectable cannot be
    used. Second, reading the structure of a page the assistant is working on
    (read_page, find), which is done in the page rather than over the debugger.

### `tabs`

    To open the tab the assistant works in and find it again across steps, to
    re-attach the bridge script to gateway tabs that were already open after the
    extension updates, and to report the titles and addresses of open tabs when
    the user asks the assistant to list them.

### `tabGroups`

    To place the assistant's working tab in its own named tab group, so it is
    visibly separate from the user's own tabs and does not interleave with them.

### `storage`

    To remember which gateways the user paired, whether the extension is
    currently switched on and for which gateway, and a short local log of the
    steps carried out, shown in the popup. All of it stays in the browser; none
    of it is transmitted.

### Host permissions (`https://*/*`, `http://*/*`, optional)

    Declared as optional_host_permissions and requested at the moment the user
    switches the extension on, not at install time. The assistant cannot know in
    advance which sites a user will ask it to work on, and a gateway is
    self-hosted on an address only the user knows, so the set cannot be
    enumerated at publish time. Users who prefer a narrower grant can switch the
    extension to "only sites I approve" in its settings and approve individual
    sites.

## Remote code

Answer: **No, the extension does not execute remote code.**

    All logic is contained in the extension package. The gateway sends data, not
    code: a JSON list of named actions drawn from a fixed set of fourteen
    (navigate, read_page, find, click, hover, drag, type_text, press_key, scroll,
    screenshot, set_viewport, wait_for, go_back, list_tabs). Every action is
    checked against KNOWN_ACTIONS in src/policy.js before anything runs, and an
    action the extension does not recognise is refused and treated as a write
    attempt. No string received from the gateway is ever evaluated, injected as
    script, or passed to a function constructor. No script is loaded from a
    remote origin; the extension loads no external resources of any kind.

## Data disclosures

Tick these, and only these — they must match `PRIVACY.md` exactly:

| Disclosure | Answer |
|---|---|
| Personally identifiable information | No |
| Health information | No |
| Financial and payment information | No |
| Authentication information | No |
| Personal communications | No |
| Location | No |
| Web history | **Yes** — page addresses and titles are sent to the user's own gateway while the extension is switched on |
| User activity | **Yes** — the actions the assistant performs are sent to the user's own gateway and recorded in its audit trail |
| Website content | **Yes** — text, structure and screenshots of pages the assistant works on are sent to the user's own gateway |

Certifications (all three must be true, and are):

- I do not sell or transfer user data to third parties, outside of the approved use cases
- I do not use or transfer user data for purposes that are unrelated to my item's single purpose
- I do not use or transfer user data to determine creditworthiness or for lending purposes

## Privacy policy URL

`PRIVACY.md` has to be served from a public, stable URL, and that URL goes in
the dashboard's Privacy tab. The store rejects a listing without one.

## Assets

Generated by `python3 extension/icons/promo.py`:

| Asset | Size | File |
|---|---|---|
| Store icon | 128×128 | `icons/on-128.png` |
| Small promo tile | 440×280 | `icons/promo-440x280.png` |
| Marquee promo tile | 1400×560 | `icons/promo-1400x560.png` |

Screenshots (at least one, at most five, **1280×800**, square corners, full
bleed) have to be taken from a running install — a generated mock-up would
misrepresent the product. Worth showing, in this order:

1. A conversation where the assistant reads a page and answers about it.
2. The popup while switched on, with the activity log visible.
3. The settings page, showing the site-access choice.
