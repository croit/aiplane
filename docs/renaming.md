<!--
SPDX-License-Identifier: AGPL-3.0-only
Copyright (C) 2026 croit GmbH
-->

# croit LLM Gateway is now croit AIplane

Same software, same repository, same database. The product grew past the
category its old name described: a gateway is one of the things AIplane does,
not what it is.

Nothing in an existing deployment stops working because of the rename. This
page is the complete list of what changed, what still answers to its old name,
and the one place — Helm — where an upgrade wants a deliberate decision from
you.

**If you only read one section, read [Upgrading](#upgrading).**

## What changed

| | Was | Is |
|---|---|---|
| Product name | croit LLM Gateway | **croit AIplane** |
| Repository | `croit/llm-gateway` | **`croit/aiplane`** |
| Container image | `ghcr.io/croit/llm-gateway` | **`ghcr.io/croit/aiplane`** |
| Sandbox images | `…/llm-gateway-sandbox`, `…-sandbox-runner` | **`…/aiplane-sandbox`, `…/aiplane-sandbox-runner`** |
| OCR sidecar image | `…/llm-gateway-ocr-sidecar` | **`…/aiplane-ocr-sidecar`** |
| Helm chart | `oci://ghcr.io/croit/charts/llm-gateway` | **`oci://ghcr.io/croit/charts/aiplane`** |
| Environment variables | `GATEWAY_*` | **`AIPLANE_*`** |
| Executable + its crate | `gateway` | **`aiplane`** |
| Quadlet units | `gateway.container`, `gateway.volume` | **`aiplane.container`, `aiplane.volume`** |
| Compose project + service | `llm-gateway` / `gateway` | **`aiplane` / `aiplane`** |
| Chrome extension | LLM Gateway Browser Control | **croit AIplane Browser Control** |
| Sandbox container label | `app=llm-gateway-sandbox` | **`app=aiplane-sandbox`** |
| Host config directory | `/etc/gateway/` | **`/etc/aiplane/`** |
| Rust crates | `gateway-core`, `gateway-api`, … | **`aiplane-core`, `aiplane-api`, …** |
| At-rest key label | `croit-llm-gateway/at-rest-encryption/v1` | **`croit-aiplane/at-rest-encryption/v1`** |
| `owned_by` in `GET /v1/models` | `llm-gateway` | **`aiplane`** |
| Outbound `User-Agent` | `llm-gateway/<version>` | **`aiplane/<version>`** |

## Upgrading

### Containers (Compose, Quadlet, plain `docker run`)

Nothing to do. The old image names are still published on every build, tag for
tag, from the same commit — an install pinned to `ghcr.io/croit/llm-gateway:production`
keeps updating exactly as before.

Point at `ghcr.io/croit/aiplane` when it suits you. The legacy names are
deprecated and will stop being published in a future deliberate breaking
release; that release will say so in its notes.

Your existing unit and service names (`gateway.service`, a compose service
called `gateway`, `/etc/gateway/gateway.env`) keep working untouched — they are
names *you* installed, and nothing in an image reaches back to rename them. The
templates in `deploy/` now use `aiplane` for fresh installs, so the commands in
the docs assume the new names. Rename yours if you like the tidiness; it buys
nothing else.

### Environment variables

Every variable is read under both spellings. `AIPLANE_SESSION_KEY` wins if set,
`GATEWAY_SESSION_KEY` still works and logs one deprecation warning per variable
per process. Rename them at your leisure.

**One thing to avoid:** do not set the *old* spelling of a variable the image
already bakes a default for — `AIPLANE_DATA_DIR` and `AIPLANE_STATIC_DIR`. The
canonical name wins, so the image's value would quietly beat yours, and for a
data directory that looks exactly like a fresh, empty install. AIplane refuses
to start rather than choose, naming both values. If you override the data
directory, override it under `AIPLANE_DATA_DIR`.

### Helm

This is the one that needs a decision, because Helm derives every object name
from the chart name. Installing the `aiplane` chart over a release of the
`llm-gateway` chart would rename the StatefulSet, the Service and the
PersistentVolumeClaim that holds the database.

Pick one:

**A. Keep your object names (recommended).** Move to the new chart and pin the
old name:

```bash
helm upgrade <release> oci://ghcr.io/croit/charts/aiplane \
  -n <namespace> --reuse-values --set nameOverride=llm-gateway
```

Every object keeps the name it has today. Only the chart you pull changes.

**B. Stay on the legacy chart for now.** The same chart is still published
under `oci://ghcr.io/croit/charts/llm-gateway` from every build; only its
`name:` differs. `helm upgrade <release> oci://ghcr.io/croit/charts/llm-gateway`
behaves exactly as it always has. Deprecated, and it goes away in a future
breaking release.

**C. Move to the new names deliberately.** Only worth it if you are prepared to
migrate the PVC: back up the database, uninstall, install fresh as `aiplane`,
restore. Do not do this as a side effect of a routine upgrade.

The session-key Secret is handled for you either way: the chart reads a key
stored under `GATEWAY_SESSION_KEY` as well as `AIPLANE_SESSION_KEY`, and the
Secret it renders carries both. If you brought *your own* Secret and it holds
the key under the old name, say so rather than rewriting it:

```yaml
sessionKey:
  existingSecret: my-secret
  existingSecretKey: GATEWAY_SESSION_KEY
```

Missing this would mint a fresh key and leave every secret in the database
undecryptable, which is why the chart looks for both spellings on its own.

### The at-rest encryption key

Nothing to do, and nothing to re-enter.

The key that seals every stored secret is derived from the session secret with
a domain-separation label, and that label now spells AIplane. The old labels are
kept in `RETIRED_LABELS` and still tried on every read, so a database sealed by
any earlier release opens exactly as before. On the first boot after the
upgrade, `db::reseal` rewrites each of those values under the current key and
logs how many it moved; it is idempotent, so every later boot finds nothing and
moves on.

If you set `AIPLANE_ENCRYPTION_KEY` explicitly, none of this applies to you: an
explicit key is used verbatim and no label was ever involved.

The retired labels are removed only in a deliberate breaking release. Until
then, restoring a years-old backup into a current binary still works.

### Git remote

GitHub redirects the old URL, so an existing clone keeps working. To update it
anyway:

```bash
git remote set-url origin git@github.com:croit/aiplane.git
```

## What deliberately kept its old name

Not oversights. Each of these is load-bearing, and renaming it would break
something real for no benefit.

| Identifier | Why it stays |
|---|---|
| `/var/lib/gateway`, `gateway.sqlite`, the `gateway` uid 1000 | Paths and ownership inside the image and on mounted volumes. Every existing deployment mounts its volume at `/var/lib/gateway`; moving it would orphan the database behind a path the operator has to edit by hand. They are invisible to anyone not reading a `docker inspect`. |
| `VolumeName=systemd-gateway` | The podman volume that already exists on every Quadlet host. Renaming it starts a second, empty one beside the database. |
| `RETIRED_LABELS` in `crypto` | Read, never written. They are how a database sealed by an older release still opens; see [The at-rest encryption key](#the-at-rest-encryption-key). |
| `app=llm-gateway-sandbox` (swept, never written) | Sandbox containers left behind by a pre-rename runner. The runner and the systemd unit sweep both labels so an upgrade does not leak containers. |
| `gateway-browser-request` … (extension ↔ page messages) and the `gateways` storage key | A protocol between two independently updated components. The published extension and the server it talks to are not upgraded together, so renaming these would break every user whose extension has not updated yet — and the storage key holds their paired list. |
| `x-gateway-affinity` request header | A wire contract clients already send (Claude Code sets it per launch). `x-aiplane-affinity` is accepted as well; neither is going away without notice. |
| `X-Gateway-Backend`, `x-gateway-tool-rounds` response headers | Clients and dashboards already read them. |
| `[gateway]` settings section and its TOML keys | Shown in the admin UI as "the key this setting replaces" — a deliberate reference to what an older config file spelled. |
| `llmgw-` ComfyUI bundle prefix | Baked into every manifest's `output_filename_prefix` and into directory names operators have already copied into their own content dir. Renaming it renames produced files for nothing. |
| "the gateway" as a common noun — in prose, in UI strings like "restart the gateway", and in code that names the proxy layer | Still the right word for the subsystem, and every locale translated it as a common noun (*pasarela*, *passerelle*, *шлюз*, *网关*). AIplane *has* a gateway; it is not one. The labels that named the **product** — "Gateway roles", "Gateway groups" — did move to AIplane. |

## For operators of the public deployment

Nothing to do. The UI says AIplane, the PWA installs as AIplane, and existing
sessions, tokens and conversations are untouched — the rename changed no
database schema and no stored value.

Users who installed the PWA before the rename keep the old name on their home
screen until they reinstall it; that is a browser-side cache, not something the
server can change for them.
