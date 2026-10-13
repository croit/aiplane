# Deploy AIplane

Choose a deployment method that fits your host. All methods run the same
application and include the version-matched manual at `/docs/`.

| Method | Use it when | Configuration source |
|---|---|---|
| Container | You want to evaluate with an existing identity provider and model server | [Get started](../getting-started.md) |
| Docker Compose | You operate Docker and optionally want connector/OCR/sandbox services | [`compose.example.yml`](../../deploy/compose.example.yml) |
| Podman with systemd | You manage a Linux host with systemd and rootful Podman | [`aiplane.container`](../../deploy/quadlet/aiplane.container) |
| Helm | You run Kubernetes with persistent storage | [`chart values`](../../deploy/helm/aiplane/values.yaml) |
| Source build | You develop AIplane or package your own build | `mise run build` |

## Production prerequisites

Provide a reachable OpenID Connect provider and model API. Terminate HTTPS at
a reverse proxy or ingress; register the public `/auth/callback` URL at your
identity provider. Keep the session key and persistent data volume together
in your backup plan. Optional capabilities have their own prerequisites:
attachment storage, transcription/speech models, embedding models, OCR,
ComfyUI, connectors, and sandbox execution are configured separately.

The gateway uses SQLite and process-local chat workers. Run one application
instance against its state volume. The Helm chart fixes its StatefulSet at
one replica. Model servers can have multiple backends and scale separately.

## Docker Compose

From the repository root, create `deploy/aiplane.env` with restricted
permissions and this line, using your securely stored key:

```text
AIPLANE_SESSION_KEY=<your-64-character-hex-key>
```

Optional services are behind Compose profiles. Starting only `aiplane` requires
just `deploy/aiplane.env`; prerequisites for OCR, sandbox and Google Workspace
are checked when their profiles are enabled. To use Google Workspace, prepare
its environment file and start with `--profile google-workspace`. For OCR, set
`OCR_VLLM_BASE_URL` before enabling `--profile ocr`. Enable `--profile sandbox`
only after configuring the runner and its host requirements.

Once the example configuration validates, start AIplane:

```bash
docker compose -f deploy/compose.example.yml up -d aiplane
```

The example binds `127.0.0.1:8080`, mounts the persistent `aiplane-data` volume,
uses a read-only root filesystem and drops Linux capabilities. Open the
application through the host or reverse proxy and complete setup.

The Compose example uses `:latest`. For production, choose an explicitly
reviewed version or digest in your deployment configuration. The README's
evaluation command uses `:production`; these tags are different deployment
choices, not immutable version pins.

### Optional services

- **Google Workspace MCP:** configure its example environment file and start
  the `google-workspace-mcp` service with `--profile google-workspace`. The
  browser and gateway must both reach the external URL used in its OAuth flow.
- **OCR:** the `ocr` profile starts the PDF OCR adapter. Set
  `OCR_VLLM_BASE_URL` to your Unlimited-OCR server's `/v1` URL, then configure
  the OCR endpoint in AIplane's settings.
- **Sandbox:** the `sandbox` profile starts the runner and egress proxy. The
  runner needs a host with gVisor (`runsc`) for that configuration. The
  `local-unsafe` mode does not provide that isolation and is a development
  choice, not a production equivalent.
  The runner refuses startup if it cannot confirm removal of stale labelled
  containers. Keep access to the configured container runtime available;
  failed teardown remains tracked and retried before scratch data is removed.
  Personal Dot persistence additionally requires the verified local rootful
  Linux host runner with `runsc` and an existing private `SANDBOX_WORK_ROOT`.
  The sample Compose configuration does not provision that host storage layout.
  Use the [host runner setup](../sandbox.md#personal-dot-workspaces); a socket
  mount or volume setting alone does not prove persistent file access.

```bash
docker compose -f deploy/compose.example.yml --profile ocr up -d
docker compose -f deploy/compose.example.yml --profile sandbox up -d
```

Enabling a service does not grant users its tools. Configure the corresponding
AIplane setting and access grants as well. Review the current Compose file
for each service's required environment and volume declarations.

## Podman and systemd

Install the repository's Quadlet container and volume units into
`/etc/containers/systemd/`. Put the environment file at
`/etc/aiplane/aiplane.env`, restricted to the operator, and supply
`AIPLANE_SESSION_KEY`. The units are named `aiplane.container` and
`aiplane.volume`; the service name is `aiplane.service`.

```bash
sudo systemctl daemon-reload
sudo systemctl start aiplane.service
sudo systemctl status aiplane.service
```

The unit's `[Install]` section makes it start at boot. Quadlet generates the
service during boot and `daemon-reload`; generated units use `start`, not
`systemctl enable`. See the [Podman Quadlet documentation](https://docs.podman.io/en/latest/markdown/podman-systemd.unit.5.html#enabling-unit-files).

The supplied container unit binds only loopback. Place your HTTPS reverse
proxy in front of it. Review `Image=` in the unit and pin the version or
digest you intend to deploy.

## Kubernetes

Use the OCI Helm chart. Substitute your hostname and the release version you
have selected:

```bash
helm install aiplane oci://ghcr.io/croit/charts/aiplane \
  --namespace aiplane --create-namespace \
  --version <release-version> \
  --set ingress.enabled=true \
  --set ingress.host=aiplane.example.com
```

Configure TLS using the chart's ingress values and your cluster's certificate
arrangement. Persistent storage defaults to a 20 GiB ReadWriteOnce claim.
The chart can generate the session key Secret or use `sessionKey.existingSecret`;
retain the key across replacements and restores. Examine the chart values for
the required key name and optional sidecar settings.

Google Workspace, GitLab and Discord MCP bridges and the OCR adapter are
optional containers controlled by chart values. Google Workspace and Discord
require configured Secrets when enabled. The sandbox runner is external to
this chart; set its URL in AIplane after deploying its isolated host.

Check the rollout and probes:

```bash
kubectl -n aiplane rollout status statefulset/aiplane
kubectl -n aiplane logs aiplane-0 -c gateway
```

`/healthz` returns success when the process is alive. `/readyz` returns `503`
with `setup_required` until setup completes, and `200` afterwards. Readiness
does not prove that every upstream model server is healthy.
The Helm chart uses `/healthz` for its readiness probe so that the Service
continues routing requests to the initial setup wizard.
For Prometheus metrics, see [monitoring](monitoring.md).

## Verify images

CI signs every image and Helm chart it publishes with
[Sigstore cosign](https://docs.sigstore.dev/cosign/verifying/verify/), keyless.
There is no public key to distribute: the signature is bound to the GitHub
workflow that built the artifact, recorded in Sigstore's public transparency
log, and stored in GHCR next to the image. Verifying proves that an image was
built by `croit/aiplane`'s CI from `main` or a release tag and has not been
changed since.

Install [cosign](https://docs.sigstore.dev/cosign/system_config/installation/),
then verify the reference you deploy:

```bash
cosign verify ghcr.io/croit/aiplane:production \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  --certificate-identity-regexp '^https://github\.com/croit/aiplane/\.github/workflows/ci\.yml@refs/(heads/main|tags/v.+)$'
```

The same two flags verify `ghcr.io/croit/aiplane-sandbox`,
`ghcr.io/croit/aiplane-sandbox-runner`, `ghcr.io/croit/aiplane-ocr-sidecar` and
the chart (`ghcr.io/croit/charts/aiplane:<chart version>`). To accept only
release builds, narrow the identity to `@refs/tags/v.+$`. A verification
succeeds only when the certificate names that workflow and ref; any other
signer, or no signature, fails with a non-zero exit code. Artifacts published
before the pipeline signed them have no signature and fail verification; deploy
a signed version instead.

To enforce this in Kubernetes, use an admission policy that checks Sigstore
signatures (for example the Sigstore policy-controller or Kyverno's
`verifyImages`) with the issuer and identity above.

## Build from source

Install the toolchain with `mise install`. `mise run build` builds the release
binary and its required runtime artifacts, including the frontend/manual.
For frontend-only work, use `mise run build-web`. Its output contains the
SPA and the manual under `target/frontend/build/docs`.

When packaging the binary yourself, ship that frontend directory too and set
`AIPLANE_STATIC_DIR` to its installed location. A standalone binary without
its static artifact does not contain the website or manual. The repository
Dockerfile copies the full frontend artifact into the runtime image.

Use `mise run dev` for development rather than rebuilding release binaries.
Use a separate `mise run dev-ui` fixture for browser checks; it uses mock
upstreams and synthetic accounts rather than production data.

## Network and filesystem boundaries

The data directory holds writable state; the frontend/manual is a read-only
build artifact. Settings edited in the UI describe paths as seen by the
application/container, not paths on your laptop.

URLs selected by users, models or agent owners use the outbound network
guard. Its default denies private networks; changing
`AIPLANE_ALLOW_PRIVATE_NETWORKS` changes that policy. These guarded requests
go directly to checked addresses rather than through environment HTTP
proxies. Operator-configured model backends use the operator HTTP client.

## Persistent Dot workspace storage

The runner checks UID/GID 1001 private writes, host reads, persistence through
another fresh isolated container and confirmed teardown before reporting
`workspace: true`. Rootless, remote and `local-unsafe` runners do not qualify
for this persistent storage deployment. Failed readiness leaves ordinary
scratch execution available and refuses scoped persistence without fallback.
The gateway refreshes the health proof for each scoped execution, file
management and cleanup request.

Use one runner per persistent root. Monitor the backing volume's free space;
`SANDBOX_WORK_SIZE` is not a persistent storage quota. Include the runner's
`dots/` and permanent `.dot-retired/` tombstones in backups alongside the
separate gateway SQLite database. Quiesce gateway writes and runner executions
for a consistent restore point. Restore the matching database, files and
tombstones together; the gateway's durable cleanup queue retries retirement
when runner access returns. The [sandbox guide](../sandbox.md#deletion-recovery-and-backups)
describes bounded cleanup and retry behavior.

See [environment reference](../reference/environment.md),
[backup and recovery](backup-recovery.md), and
[troubleshooting](troubleshooting.md).

## Enable request prioritization

Enable the gateway switch in **Administration → Settings → Access → Gateway**, configure group and application-token priorities under Access, and apply any backend priority mapping. Model-server priority scheduling must also be enabled in its container command: vLLM `--scheduling-policy priority`; SGLang `--enable-priority-scheduling --schedule-policy fcfs`. Automatic forwarding depends on reported flags; otherwise configure a manual mapping. The admission queue is per AIplane process, so multiple gateway replicas do not share queue order. Review backend concurrency limits and [Request priority](../request-priority.md) before assessing request latency.
