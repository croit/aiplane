# LLM Gateway Browser Control (Chrome extension)

Lets a conversation on your gateway act in **this** browser — the one with your
logins — through the `browser_control` tool.

## Why an extension at all

The gateway runs on a server. Your browser sits behind NAT with no reachable
address, and gateway users are not necessarily the operator's own staff, so a
tunnel or a local port is not an option. The only channel that always exists is
the one your browser already opened: the chat page's event stream out, an
authenticated POST back. This extension is the far end of that channel.

Everything else follows from it:

- It works **only while the conversation is open**. Close the tab and nothing
  can act.
- It is **off until you switch it on** from the toolbar icon. A click on that
  icon is the one signal no web page can produce.
- It talks to **one origin you paired**, and Chrome — not this code — enforces
  that.

## Version and packaging

`manifest.json` carries `0.0.0`, a placeholder — the same arrangement as the
Helm chart's `Chart.yaml`. The number that ships is stamped at packaging time
from `scripts/derive-version.sh`, the one resolver the container images and the
chart also use, so an extension build can never claim a version the rest of the
release does not have.

    mise run package-extension

writes `target/llm-gateway-browser-control-<version>.zip`, without the tests or
the icon script. Loading `extension/` unpacked during development shows 0.0.0,
which is accurate: an unpacked tree is not a release.

The toolbar icons are generated, not hand-drawn:

    python3 extension/icons/render.py

They are the gateway's own mark (`web/static/favicon.svg`) — a diamond with a
node at its centre — in the product's accent while it is on, and grey while it
is off. Only the colour changes, never the shape, so the icon is read once and
its state at a glance. Pillow is needed to run it, and is deliberately not in
the toolchain: the artwork changes roughly never.

## Turning it on

Open your gateway in a tab with this extension installed and paired, and it
asks by itself: the same popup you get from clicking the toolbar icon opens on
its own, and one click on **Switch on** is the whole thing.

It asks once per browser session per gateway, and only while that tab is the
one you are looking at — a chat loading in a background tab will not put
anything over what you are reading. You can always switch it on yourself from
the toolbar icon, which carries a badge while a question is waiting.

## After you reload the extension

Chrome throws away the registration that puts this extension on your gateway,
and reloading an unpacked extension counts as a reload. Everything else
survives — the gateway stays listed, the icon stays green — so it looks fine
and answers "no extension" to everything.

It repairs itself: on browser start, on reload, and every time you open the
popup. That covers the pages you open afterwards *and* the tab you are looking
at, so there is nothing for you to do.

## Install (development)

1. `chrome://extensions` → enable *Developer mode* → *Load unpacked* → pick this
   `extension/` directory.
2. Open the extension's *Settings* and pair your gateway's URL. Chrome will ask
   for permission on that origin; that dialog **is** the pairing.
3. Open your gateway, click the extension's toolbar icon, *Switch on*.

## Telling at a glance whether it is on

The toolbar icon is **green with an "on" badge** while the extension may act and
**grey** while it may not, and its tooltip names the gateway it is armed for.
Chrome's "is debugging this browser" bar over the assistant's window says the
same thing from the other side.

## Where it works

The assistant gets **its own window**, opened behind yours and never focused,
with its tabs in a labelled *Assistant* group. Nothing is pulled in front of you
mid-sentence, and you can still watch what is happening by bringing that window
up. Screenshots are scoped to that window, so they show the page the assistant
drove rather than whatever you were reading.

## What it will and will not do

**A site you grant is a site the assistant may work on** — read it, and click,
type and submit on it. There is no dialog per action: a prompt for every click
is one nobody reads by the third, and a consent clicked away reflexively
protects no one.

`list_tabs` is the one thing a per-site grant cannot cover, because it reports
every open tab and belongs to no site. It works under "any site" and is refused
under "only sites I approve".

Site access has two modes, in Settings:

| Mode | What happens |
| --- | --- |
| **All sites** (default) | One Chrome permission, asked for the first time you switch the extension on |
| Only sites I approve | You approve sites up front in Settings; nothing runs anywhere else |

Both are asked for by a click in this extension's own UI. Chrome only grants a
permission during a user gesture, so it cannot be requested while a conversation
is waiting — which is why approving a site is something you do beforehand.

The default is the broad one on purpose: per-site prompts are what make tools
like this unusable, and a setting nobody keeps switched on protects nobody.

What carries the weight instead is the switch: the extension does nothing until
you turn it on from the toolbar, that click is one no web page can produce, the
icon is green the whole time it is on, Chrome shows its own bar over the
assistant's window, and every step is listed in the popup and in the gateway's
audit trail. Turning it off releases everything at once.

Chrome's own site-access control (`chrome://extensions` → *Details* → *Site
access*) stays available and can narrow things further; host permissions are
declared `optional_host_permissions` precisely so Chrome does not grey it out.

## The bar Chrome shows

While switched on, Chrome displays "LLM Gateway Browser Control is debugging
this browser" over the assistant's window. That is the price of real input —
clicks and keystrokes the page cannot tell apart from yours, which is what makes
editors, drag targets and canvas apps work at all. It is left visible rather
than worked around: something *is* driving that window, and you should be able
to see it. Switching the extension off releases it immediately.

## The limits worth knowing

**A page the assistant reads can try to steer it.** "Open … and click …" in the
text of a page is the realistic attack, and with a site granted it is carried
out without asking. Grant what you would let an assistant work in, and switch it
off when you are done — that is the trade this design makes deliberately, in
exchange for not asking you to approve every click.

**Script on your gateway's origin** — an XSS there, a malicious dependency in
its frontend, a compromised server — can ask this extension for anything the
gateway could ask for. No check here can tell the difference; they are the same
sender. Chrome's per-site permission still bounds *where*, and the switch still
bounds *when*, but neither is a substitute for trusting the gateway you paired.

## Layout

| File | Role |
| --- | --- |
| `src/policy.js` | The rules. Pure, no `chrome.*`, unit-tested (`mise run test-extension`) |
| `src/cdp.js` | Real input through the DevTools protocol: mouse, keyboard, viewport emulation, screenshots |
| `src/background.js` | Service worker: pairing checks, arming, permissions, confirmation, execution |
| `src/content.js` | Wire between the gateway page and the service worker. Decides nothing |
| `src/options.*` | Pair gateways, choose site access, forget trusted sites |
| `src/popup.*` | Switch on/off, recent activity |
