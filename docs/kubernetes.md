# Kubernetes

How to run the gateway on Kubernetes with the Helm chart in
[`deploy/helm/llm-gateway/`](../deploy/helm/llm-gateway/).

The deployment is deliberately small: **one pod, one volume, one secret.**
Everything else — upstream pools, models, groups, the OIDC provider, RAG, tool
permissions — is configured in the web UI after the first start and stored in
the gateway's own database. There is no config file and no ConfigMap.

| | |
|---|---|
| **Objects a default install creates** | ServiceAccount, Secret, Service, StatefulSet (4) |
| **Required values** | none |
| **Required secrets** | one, and the chart can generate it |
| **Scaling** | exactly one replica — see [Why one replica](#why-one-replica) |

---

## Before you start

- A cluster on **1.25 or newer**, and `kubectl` pointed at it.
- **Helm 3**.
- A **StorageClass** that can provision a `ReadWriteOnce` volume. Snapshot
  support (a `VolumeSnapshotClass`) makes backups much nicer, but is optional.
- An **ingress controller** with TLS, or your own way of routing to a Service.
  The gateway speaks plain HTTP and expects TLS to be terminated in front of it.
- An **OIDC provider** (Keycloak, Entra ID, Authentik, Google, …) where you can
  register a redirect URI. You do this *during* the setup wizard, not before —
  the wizard shows you the exact URI to paste.
- At least one **LLM backend** reachable from the cluster (an OpenAI-compatible
  URL plus an API key). Entered in the UI later, not here.

---

## Step 1 — Create a namespace

```bash
kubectl create namespace llm-gateway
```

## Step 2 — Decide how the session key is managed

`GATEWAY_SESSION_KEY` is the single secret a deployment must have. It signs
sessions **and** derives the key that seals every secret in the database:
backend API keys, the OIDC client secret, per-user OAuth tokens. Lose it and
the volume is unreadable — back the two up together.

It must be **64 hex characters** (32 bytes). The gateway refuses to boot on
anything else.

**Option A — bring your own Secret (recommended, and required for GitOps):**

```bash
kubectl -n llm-gateway create secret generic llm-gateway-session \
  --from-literal=GATEWAY_SESSION_KEY="$(openssl rand -hex 32)"
```

and then install with `--set sessionKey.existingSecret=llm-gateway-session`.

**Option B — let the chart generate it.** This is the default. On every
`helm upgrade` the chart reads the key back out of the cluster, so it stays
stable, and the Secret carries `helm.sh/resource-policy: keep` so
`helm uninstall` cannot take it with it.

> **GitOps warning.** `helm template` runs without a cluster, so the lookup that
> preserves the key finds nothing and renders a **fresh** key every time. If you
> render manifests instead of letting Helm talk to the cluster (Argo CD in
> manifest mode, `helm template | kubectl apply`), use Option A and set
> `sessionKey.autoGenerate=false` so a mistake fails loudly instead of quietly
> rotating your encryption key.

## Step 3 — Install

The chart is published as an OCI artifact next to the images, so no checkout is
needed — and with no `--version`, Helm resolves the **newest official
release**:

```bash
helm install llm-gateway oci://ghcr.io/croit/charts/llm-gateway \
  --namespace llm-gateway \
  --set sessionKey.existingSecret=llm-gateway-session \
  --set ingress.enabled=true \
  --set ingress.host=gateway.example.com \
  --set ingress.className=nginx
```

That is the recommended way to install: you stay current, and the gateway keeps
itself current afterwards (see [Updates](#updates)). Builds from `main` are
published too, as SemVer prereleases, which Helm skips unless you ask for them
with `--devel` — so "latest" never lands on an untagged build by accident.

Check what you are about to get, or pin a specific release:

```bash
helm show chart oci://ghcr.io/croit/charts/llm-gateway            # newest release
helm install … oci://ghcr.io/croit/charts/llm-gateway --version 2609.1.0
```

To try an unreleased state, install from a git checkout instead — that chart
has `appVersion: latest` and therefore follows the `:latest` images:

```bash
helm install llm-gateway ./deploy/helm/llm-gateway -n llm-gateway
```

The version scheme and how releases are cut: [`releases.md`](releases.md).

Watch it come up:

```bash
kubectl -n llm-gateway rollout status statefulset/llm-gateway
```

Without an ingress, reach it directly instead:

```bash
kubectl -n llm-gateway port-forward svc/llm-gateway 8080:8080
# → http://localhost:8080
```

### Ingress settings that matter

Chat responses stream over Server-Sent Events, and a model can think for a long
time before the first token. Two controller settings decide whether that works:
**response buffering must be off**, and **read timeouts must be generous**. The
chart ships nginx annotations that do this; translate them for your controller:

```yaml
ingress:
  annotations:
    nginx.ingress.kubernetes.io/proxy-buffering: "off"
    nginx.ingress.kubernetes.io/proxy-read-timeout: "3600"
    nginx.ingress.kubernetes.io/proxy-send-timeout: "3600"
    nginx.ingress.kubernetes.io/proxy-body-size: "100m"   # uploads
```

## Step 4 — Run the setup wizard

Open the gateway. A fresh install lands on **`/setup`**.

1. The wizard shows the **redirect URI** to register with your identity
   provider — `https://<your host>/auth/callback`. Register it there first.
2. Enter the provider's discovery URL, client ID and client secret.
3. Name the IdP group that should become gateway admins.
4. The wizard **proves the login with a real sign-in** before it commits
   anything, then hands you an admin account.

If the gateway is behind a proxy that rewrites the scheme, set the public URL
explicitly: `--set publicUrl=https://gateway.example.com`. With
`ingress.enabled=true` the chart already derives it from the ingress host.

## Step 5 — Add a backend and a model

Signed in as admin:

1. **`/admin/upstreams`** — add a backend (base URL + API key) and a pool, then
   list the models it serves.
2. **`/admin/groups`** — map IdP groups to gateway roles and decide which
   models and tools each role may use.
3. **`/chat`** — send a first message to confirm the whole path works.

## Step 6 — Back up the session key today

```bash
kubectl -n llm-gateway get secret llm-gateway-session \
  -o jsonpath='{.data.GATEWAY_SESSION_KEY}' | base64 -d
```

Put it wherever your other credentials live. See [Backups](#backups-and-restore).

---

## What the chart creates

| Object | Why |
|---|---|
| `StatefulSet` (1 replica) | The gateway pod, plus any sidecars you enabled. A StatefulSet rather than a Deployment because its `volumeClaimTemplate` keeps the database on uninstall and its update strategy replaces the pod instead of briefly running two. |
| `PersistentVolumeClaim` (`data-<name>-0`) | `/var/lib/gateway` — the SQLite database and the RAG index store. The image points `GATEWAY_DATA_DIR` here, so this one mount is all the persistence there is. |
| `Secret` (`<name>-session`) | `GATEWAY_SESSION_KEY`, unless you brought your own. |
| `Service` | Port 8080, plus port 8000 when the Google Workspace sidecar is on. |
| `ServiceAccount` | Identity only. Its token is **not** mounted — nothing in the pod talks to the Kubernetes API. |
| `PersistentVolumeClaim` (`<name>-gworkspace-oauth`) | Only with the Google Workspace sidecar: its OAuth store, which must survive a container restart. |
| `Ingress` (optional) | One for the gateway, one more for the Google Workspace sidecar if it needs a public hostname. |
| `CronJob` + `ServiceAccount`/`Role`/`RoleBinding` | Automatic updates (on by default): a nightly `kubectl rollout restart` of this StatefulSet, and nothing else. |

The pod runs as uid 1000, non-root, with a read-only root filesystem, all
capabilities dropped and `seccompProfile: RuntimeDefault`. `fsGroup: 1000` is
what makes a freshly provisioned volume writable.

---

## Day-2 operations

### Updates

**By default the gateway updates itself.** The images follow `:production` —
the moving pointer at the newest official release — and a small CronJob
restarts the StatefulSet nightly at 04:00 so a moved tag is actually picked up.
Kubernetes does not notice a tag moving on its own; something has to restart the
pod, and that CronJob is it. Its ServiceAccount may patch exactly one object:
this StatefulSet.

Two things to be aware of, because this happens unattended:

- the deployment is a single replica, so the restart is a brief interruption —
  seconds, and open chats reconnect and resume, but pick an hour that suits you;
- a release can carry **database migrations**, which then apply with nobody
  watching. Keep backups (below).

Change the window, or switch it off entirely:

```yaml
autoUpdate:
  enabled: true
  schedule: "0 4 * * *"
  timeZone: Europe/Berlin
```

With `autoUpdate.enabled=false` the images pin to the exact build the chart was
released with (`v2609.1.0`), and updating is yours to trigger.

**Chart-level changes** — new settings, new templates — arrive with an upgrade,
not with the image. Run this when you want them; with no `--version` it takes
the newest released chart:

```bash
helm upgrade llm-gateway oci://ghcr.io/croit/charts/llm-gateway -n llm-gateway --reuse-values
```

**Roll back:**

```bash
helm rollback llm-gateway                       # previous release of the chart
helm upgrade … --version 2609.1.0 --set autoUpdate.enabled=false   # pin an older build
```

Pinning matters for a rollback: with auto-updates on, the next nightly restart
would pull `:production` again and undo it.

**Apply a setting that says it needs a restart.** A few settings in
`/admin/settings` are marked restart-only. The UI records that a restart is
pending; deliver it with:

```bash
kubectl -n llm-gateway rollout restart statefulset/llm-gateway
```

**Logs:**

```bash
kubectl -n llm-gateway logs statefulset/llm-gateway -c gateway -f
```

**Locked out** (the IdP moved, or no group maps to admin any more):

```bash
kubectl -n llm-gateway exec -it llm-gateway-0 -c gateway -- restore-setup
```

It prints a one-time URL valid for 30 minutes and reopens the wizard pre-filled
with the current provider. The gateway keeps serving the whole time — nobody is
logged out and nothing is deleted.

**Uninstall.** `helm uninstall` deliberately leaves the PVC and the session
Secret behind, so the data survives a reinstall. To really delete everything:

```bash
helm uninstall llm-gateway -n llm-gateway
kubectl -n llm-gateway delete pvc data-llm-gateway-0
kubectl -n llm-gateway delete secret llm-gateway-session   # point of no return
```

---

## Backups and restore

Two things must be backed up, and they are worthless apart:

1. the **session key** Secret, and
2. the **PVC** (database + RAG index).

With the Google Workspace sidecar there is a third, independent one: its
`<name>-gworkspace-oauth` PVC. Losing it costs no gateway data, but every user
has to reconnect their Google account.

The image ships no `sqlite3` binary, so take the volume, not the file.

**With volume snapshots** (preferred — no downtime):

```bash
cat <<'EOF' | kubectl -n llm-gateway apply -f -
apiVersion: snapshot.storage.k8s.io/v1
kind: VolumeSnapshot
metadata:
  name: llm-gateway-$(date +%Y%m%d)
spec:
  volumeSnapshotClassName: csi-snapclass    # yours
  source:
    persistentVolumeClaimName: data-llm-gateway-0
EOF
```

**Without snapshots**, stop the pod first so the database is at rest — SQLite in
WAL mode has a `-wal` sidecar file, and copying the three files from underneath
a running writer is not a backup:

```bash
kubectl -n llm-gateway scale statefulset llm-gateway --replicas=0
# … copy the volume with whatever your storage offers, or run a throwaway pod
#   that mounts the PVC and tars /var/lib/gateway …
kubectl -n llm-gateway scale statefulset llm-gateway --replicas=1
```

**Restore** is the reverse: restore the volume, re-create the Secret with the
*same* key, install the chart pointing at both.

---

## Optional sidecars

They run as extra containers in the gateway pod. With one replica that is
simply simpler than separate Deployments: the connector URL becomes
`http://localhost:<port>/mcp`, there is no extra Service, no cross-pod DNS and
no Host-header allowlist, and one restart covers everything. A crashing sidecar
is restarted on its own by the kubelet and does not take the gateway down.

Prefer to run one elsewhere? Deploy it however you like and paste its URL into
`/admin/connectors`. The gateway only ever knows a URL, so the chart does not
need to model that case.

Background on the connectors themselves: [`connectors.md`](connectors.md).

### Google Workspace (Gmail, Calendar, Drive, Docs)

The only sidecar that must be reachable **from the browser** — Google's consent
screen redirects through it — so it gets its own hostname.

1. In Google Cloud, create an OAuth client (web application) and allow
   `https://gworkspace-mcp.example.com/oauth2callback` as its redirect URI.
2. Create the Secret:

   ```bash
   kubectl -n llm-gateway create secret generic gworkspace-oauth \
     --from-literal=GOOGLE_OAUTH_CLIENT_ID=… \
     --from-literal=GOOGLE_OAUTH_CLIENT_SECRET=… \
     --from-literal=FASTMCP_SERVER_AUTH_GOOGLE_JWT_SIGNING_KEY="$(openssl rand -hex 32)"
   ```

3. Enable it:

   ```yaml
   mcp:
     googleWorkspace:
       enabled: true
       existingSecret: gworkspace-oauth
       externalUrl: https://gworkspace-mcp.example.com
       allowedClientRedirectUris: https://gateway.example.com/integrations/callback
       ingress:
         enabled: true
         className: nginx
         host: gworkspace-mcp.example.com
   ```

4. In the gateway: `/admin/connectors` → **Google Workspace** → URL
   `http://localhost:8000/mcp` (no trailing slash) → Save → Enable. Users then
   connect their own account at `/integrations`.

Its OAuth store gets a small PVC of its own (`<name>-gworkspace-oauth`). That
is not a detail: the store holds the gateway's registered client, the
authorization codes, the refresh tokens the server issues *and* the upstream
Google tokens. If it is ever lost, every user is disconnected with
`invalid_client: Invalid client_id` within about half an hour — so the chart
keeps it on `helm uninstall`, and you should include it in backups.

Note that `mcp.googleapis.com`-hosted Google MCP endpoints are a Workspace
Developer Preview feature; this self-hosted bridge is the supported path.

### GitLab (self-managed / CE)

Each request carries the calling user's own GitLab token, which the bridge
forwards — so there is no shared credential and no OAuth, and it needs no
public URL.

```yaml
mcp:
  gitlab:
    enabled: true
    apiUrl: https://gitlab.example.com/api/v4
    permissionMode: readonly     # or modify (no deletes) / full
```

Connector URL: `http://localhost:3002/mcp`.

### Discord

One shared bot for the whole gateway, so this connector is global rather than
per user. Member search additionally needs the **Server Members Intent**
enabled in the Discord developer portal, or the roster never loads.

```bash
kubectl -n llm-gateway create secret generic discord-bot \
  --from-literal=DISCORD_TOKEN=…
```

```yaml
mcp:
  discord:
    enabled: true
    existingSecret: discord-bot
```

Connector URL: `http://localhost:8085/mcp`.

### Document OCR

The sidecar adapts the gateway's internal `/ocr` contract to an Unlimited-OCR
vLLM server, which this chart does **not** deploy — it wants a GPU and a
lifecycle of its own. The sidecar image itself is published alongside the
gateway (`ghcr.io/croit/llm-gateway-ocr-sidecar`) and versioned with it, so
enabling it needs no image of your own.

OCR needs all three of these before it does anything:

1. `ocr.enabled=true` and `ocr.vllmBaseUrl` pointing at the vLLM server,
2. an `ocr` pool and backend at `/admin/upstreams` pointing at
   `http://localhost:9100` with `baidu/Unlimited-OCR` in its model list,
3. `chat.ocr.enabled` turned on at `/admin/settings`.

Details and tuning: [`ocr.md`](ocr.md).

---

## Sandbox

The code sandbox (`run_in_sandbox`, `generate_document`, `capture_webpage`) is
**not part of this chart**, on purpose.

The runner executes model-written code, so it needs a kernel-level boundary —
gVisor or Kata — which a generic cluster does not have by default. And the
gateway reaches it over plain HTTP at a URL stored in the database. That makes
it a *setting*, not a Kubernetes object.

Three ways to get there, simplest first.

### Option A — run the runner outside the cluster (recommended)

Keep the runner on a host that already has gVisor, exactly as
[`docs/sandbox.md`](sandbox.md) and [`deploy/quadlet/`](../deploy/quadlet/)
describe it. Then point the gateway at it:

`/admin/settings` → **Code sandbox** → `sandbox.enabled` on,
`sandbox.runner_url` = `http://sandbox-host.example.com:9000` → restart the pod.

No new code, no RuntimeClass in the cluster, no privileged pod, and none of the
image-pull pain that the (large) sandbox image causes on a generic node pool.
The sandbox host is a specialist machine anyway; forcing it into a generic
cluster buys little.

Make sure that URL is only reachable from the gateway — the runner executes
untrusted code and has no authentication of its own.

### Option B — pod-per-job inside the cluster

The cluster-native shape: the runner creates one Pod per job with
`runtimeClassName: gvisor`, `/work` and `/tmp` as `emptyDir`, a default-deny
`NetworkPolicy` instead of a Podman network, and a ServiceAccount scoped to
`pods` + `pods/exec` in a dedicated sandbox namespace.

**This is not implemented yet** — the runner currently drives a `podman` or
`docker` CLI. It is a contained piece of work (the backend is a four-method
trait), but until it exists, Option A is the way to run the sandbox alongside a
Kubernetes gateway.

### Option C — leave it off

The chart's default, and the product's. Everything except the three sandbox
tools works without it.

### What not to do

Do not mount a container socket into a pod, and do not run the runner
privileged, to get the sandbox into the cluster. That trades a bounded piece of
work for host-root-equivalent access on behalf of model-generated code.

---

## Why one replica

`replicas` is not a value in this chart. Two reasons, both structural:

1. **SQLite runs in WAL mode.** WAL keeps its index in shared memory, so
   [SQLite requires every process using the database to be on the same host](https://www.sqlite.org/wal.html).
   A second pod cannot share that, and a network filesystem does not help.
2. **A running chat turn lives in the pod's memory.** The worker driving it, the
   flag the stop button flips, and the fan-out that `/chat/<id>/tail` subscribes
   to are all in-process. A second replica would not share that work; it would
   duplicate turns and stream into the void, and the stop button would land on
   the wrong pod.

So this is a vertical-scaling deployment: give the pod more CPU and memory. The
gateway spends nearly all of its time waiting on upstream models, and one pod
carries a lot of concurrent conversations.

Lifting the limit is a real project, not a flag: move the database to Postgres,
move turn ownership and the update fan-out into the database plus a pub/sub
channel, make the scheduled-actions claim atomic, and decide where the RAG
index lives (it is per-collection SQLite plus a vector file on local disk).
Until then, availability comes from fast rescheduling — which is what the
StatefulSet, the probes and a 45-second grace period are for.

---

## Troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| Pod stuck `Pending` | No StorageClass, or none that provisions RWO | `kubectl get sc`; set `persistence.storageClass` |
| `CrashLoopBackOff`, log says the session key is missing or malformed | Not 64 hex chars | `openssl rand -hex 32`; recreate the Secret |
| Log shows a permission error opening the database | Storage driver ignores `fsGroup` (some NFS provisioners do) | Use a CSI driver that honours it, or pre-chown the volume to uid 1000 |
| Browser reaches `/setup` again after it was completed | The PVC was replaced — a new, empty database | Check that the PVC bound and was not deleted between installs |
| "Setup is closed on this gateway" | Expected once configured | `kubectl exec … -- restore-setup` |
| Chat answers appear all at once at the end, or time out | Ingress buffers responses or times them out | `proxy-buffering: "off"`, raise `proxy-read-timeout` |
| Uploads fail at a certain size | Ingress body-size cap | Raise `proxy-body-size` |
| `invalid_client: Invalid client_id` on a Google connector | The MCP OAuth store was lost | Confirm the `gworkspace-mcp` subPath is on the volume; users must reconnect once |
| Sandbox tools missing from the tool list | `sandbox.enabled` off, or no runner reachable | See [Sandbox](#sandbox) |
| The gateway restarted overnight on its own | The auto-update CronJob — working as intended | Move it with `autoUpdate.schedule`, or set `autoUpdate.enabled=false` |
| A pinned older version came back after a night | Auto-updates re-pulled `:production` | Pin *and* set `autoUpdate.enabled=false` |
| `helm upgrade` fails with "updates to statefulset spec … are forbidden" after changing `persistence.size` | A StatefulSet's volumeClaimTemplates are immutable | Resize the PVC directly (`kubectl -n … patch pvc data-llm-gateway-0 …`) if the StorageClass allows expansion, and set the value to match |

---

## Related

- [`../deploy/README.md`](../deploy/README.md) — the Docker Compose and
  systemd/Podman deployments, and what every image is for
- [`auth.md`](auth.md) — OIDC, sessions, the setup wizard and recovery
- [`connectors.md`](connectors.md) — per-user MCP connectors end to end
- [`sandbox.md`](sandbox.md) — the sandbox runner, isolation, and installing a
  sandbox runtime
- [`architecture.md`](architecture.md) — what runs inside the one binary
- [`releases.md`](releases.md) — the version scheme behind `--version`

Changing the chart? `mise run helm-lint` renders it twice — the default install
and `ci/full-values.yaml` with every sidecar on — and validates both against the
real Kubernetes schemas.
