# Dot workspace and unattended browser

## Status and scope

Adopted implementation proposal under the maintainer's delegated Dot product decisions. This document specifies the next vertical slice; it does not describe an implemented feature. The background automation slice remains separate in [dots-background.md](dots-background.md).

A personal Dot needs files that survive turns, schedules, webhook runs and container expiry, plus its own server-side browser state. This slice extends the existing sandbox runner and `browse_page` implementation. The Dot uses the existing model, tool permissions, pending approvals, metering and owner identity. It does not require a connected browser extension. Organization ownership and human remote desktop control are outside this slice.

## Verified existing mechanisms

| Mechanism | Source and existing behavior |
|---|---|
| Internal runner protocol | `crates/shared/src/sandbox.rs`: code, staged `/work` files, time/network limits, container reuse and keep-alive; no Dot scope currently. |
| Host storage | `crates/sandbox-runner/src/config.rs` and `backend.rs`: optional `SANDBOX_WORK_ROOT`, private runner root and per-container scratch bind mounts. Without it, scratch is tmpfs. |
| Container lifecycle | `crates/sandbox-runner/src/pool.rs`: warm containers, global concurrency gate, leases, idle expiry, release and startup cleanup. |
| Isolation | `backend.rs`: existing gVisor/runtime choice, read-only root filesystem, capabilities dropped, no-new-privileges, memory/CPU/PID limits and network policy. |
| Browser | `crates/aiplane-runtime/src/server/tools/sandbox/browse.rs`: Playwright Chromium, explicit proxy settings, a per-turn leased browser container and ephemeral browser context. |
| Tool context | Runtime tool runner and Dot session resolution: authenticated owner and verified Dot association; access rules and active state are checked before execution. |
| File presentation | `web/src/lib/components/chat/ConversationCanvas.svelte` and `conversation-assets.ts`: actual filenames, media types, sizes and download controls for conversation assets. These are attachment assets, not a directory browser. |

The runner's HTTP service currently relies on an internal network boundary; it does not authenticate `/run`. Deployments must keep it private and use their existing transport protection, including mTLS where required. A new owner field in the protocol is a trusted gateway claim, not authentication against an exposed runner.

## Shared mechanisms

Reuse `ToolContext`, the Dot persona/session loader, `GrantedToolSource`, existing approval suspensions, `SandboxClient`, `SandboxLease`, the pool's concurrency/TTL machinery and the current backend container options. Reuse bounded HTTP body/response readers and the SPA API error envelope. Keep agent ownership accessors in `aiplane-agents`, state-dependent cleanup orchestration in runtime, routes in `aiplane-api` and router wiring in gateway.

Add one concrete shared runner scope type, with real callers in the sandbox and browser tools and workspace routes. Add one descriptor-relative filesystem access implementation used by runner file operations and cleanup. Extend the existing container teardown mechanism to report confirmed success and retry failures. These primitives and their state-based tests land before the UI or tools depending on them.

Do not add a second filesystem database, scheduler, capability list, model picker or approval mechanism. JSON, TOML and SQLite project state are ordinary files written through the existing sandbox tool.

## Storage and trusted scope

Use this runner-owned layout when `SANDBOX_WORK_ROOT` is configured:

```text
<SANDBOX_WORK_ROOT>/
  sbx-<container>/work/            existing scratch
  sbx-<container>/tmp/             existing scratch
  dots/<canonical-dot-uuid>/
    owner.json                    private runner metadata
    files/                        mounted at /workspace
    browserprofile/               browser-only mount at /dot-browser
```

The root and control metadata are never mounted into a container. Generic Dot code mounts only `files`; only the Dot browser container mounts its profile. Workspace APIs expose only descendants of `files`, never cookies or profile files. Host paths and owner metadata are not returned to the model or browser.

Add an optional typed scope to the existing `RunRequest`: `owner_user_id`, `dot_id`, `turn_id`, and a mount purpose enum `workspace` or `browser`. Ordinary requests omit it. The gateway derives every field from its verified execution context, including the persisted assistant turn ID; no field is accepted from model arguments. Browser purpose is chosen by the browser wrapper, not by generated code. Management requests contain owner and Dot identity but no fabricated execution turn.

The runner validates canonical Dot UUID syntax before deriving a directory. The owner ID is opaque metadata, never a path component. On first use, create the private root and owner manifest atomically; on subsequent use, compare the manifest with the trusted owner claim. A mismatch is a hard error, never an opportunity to overwrite ownership. Missing or malformed metadata on an existing nonempty root also fails closed.

Persistent support requires a writable persistent work root. Extend existing runner health with an explicit availability flag derived from configuration and storage validation. If unavailable, return an actionable error and show unavailability; do not silently substitute temporary storage. A configured path without a persistent host volume cannot guarantee survival of runner replacement.

## Execution, leases and concurrency

Dot containers are created with their mounts before starting. They cannot consume a generic warm container or reuse another Dot's lease. Store the scope with each lease and check it before executing a supplied container ID. A mismatched lease is rejected. A genuinely expired matching lease may create a fresh container for the same scope, preserving the mounted data and the existing network restrictions.

Adopt one active computer execution turn per Dot as a storage consistency default. A runner-owned lock admits the verified turn and permits its cooperating code and browser containers; another turn receives a retryable busy result before executing code. This is independent of active Dot count and the existing global concurrency limit. Hold the lock until every participating container is confirmed stopped. File uploads, deletes and folder creation require the same exclusive ownership with no live execution containers; browsing metadata and downloading a bounded file may proceed, with the UI stating that live files can change. A download is a bounded read, not a coherent backup of a running SQLite database.

Hold an exclusive browser profile lock for the browser container's whole lifetime, including idle lease time. Use the existing runner's single-process ownership assumption and a filesystem lock on private metadata to guard accidental duplicate access. One runner process owns each work root. Supporting independent runner replicas sharing one root requires additional coordination and is not claimed here.

Current `ContainerBackend::destroy` returns `()` and Podman teardown logs failures; the pool can drop a lease before asynchronous teardown completes. That is insufficient for persistent profile reuse. Change the shared teardown path to return confirmed absence or an error. On failure retain the scope's occupied state, retry through the existing sweep/lifecycle machinery and deny a second browser mount. Startup reaping of runner-labelled stale containers must complete successfully before reopening affected scopes. Do not release a lock solely because the gateway disconnected or the TTL expired.

Container timeouts, egress, gVisor, CPU/memory/PID limits and TTL remain the existing operator controls. `/work` and `/tmp` remain scratch and normal artifact collection remains limited to its existing outputs. Do not enumerate and upload `/workspace` on each tool call. A Dot explicitly copies a chosen output into `/work` when it wants to produce a conversation attachment.

## Browser behavior

For Dot browser requests, replace ephemeral `launch` plus `new_context` with Playwright `launch_persistent_context(user_data_dir="/dot-browser", ...)`, preserving the current explicit launch/proxy options and viewport. Ordinary browser requests retain their existing behavior. Generated tool inputs cannot select a profile directory.

Navigation, page reading, clicking, filling and screenshots use the existing `browse_page` actions and approval rules. Cookies and local storage created by real use survive later containers. Processes, open tabs and a live desktop are not promised to survive expiry or restart. Existing MCP OAuth credentials are not automatically browser cookies; establishing a browser login remains an actual browser operation and must use the supported authentication flow.

Profile contents are sensitive credentials. They are excluded from workspace listings, attachments, memories and diagnostic logs. An explicit owner action can reset the browser profile after confirmed container teardown; it preserves workspace files. The server browser is independent of the user's Chrome extension connection.

## Filesystem safety

`backend.rs::read_sandbox_file` already uses `O_NOFOLLOW` for a single flat artifact name, but it is not a nested workspace implementation and its read is not independently bounded. Extend the principle through a shared descriptor-relative walker, using the runner's existing libc dependency or equivalent standard library primitives.

- Reject absolute paths, parent/current segments, empty segments, NULs and overlong paths. Do not use a canonicalize-then-open sequence.
- Open each directory relative to the previous directory descriptor with no-follow semantics. Check final objects through the opened descriptor. Never traverse symbolic links or device/socket/FIFO nodes.
- Bound request bytes, response bytes, listing entry counts and path depth. Paginate directory results with an opaque cursor; report additional entries rather than presenting an incomplete listing as complete.
- Use exclusive temporary files and atomic rename for uploads. Reject replacement of nonregular objects. Operate on the validated parent descriptor for delete and folder creation.
- Initially allow deletion of regular files and empty folders only. A recursive user deletion API is unnecessary for this slice.
- Cleanup of a whole Dot uses the same protected traversal and private root identity checks; a malicious workspace symlink must not redirect deletion outside the root.

Do not execute user file contents in the web origin. Download with attachment disposition and `nosniff`; any later previews must reuse the existing safe asset renderer's actual supported media handling.

## API and web walkthrough

Extend `/api/v0/dots/{id}` with owner-scoped workspace operations, using existing Dot authentication/access checks. Proposed concrete routes:

| Route | Result |
|---|---|
| `GET /workspace?path=&cursor=` | Availability/busy state, actual relative path and bounded entries `{name, kind, size_bytes, modified_at}`; nullable metadata is shown as missing. |
| `GET /workspace/file?path=` | Bounded attachment download of a regular file. |
| `PUT /workspace/file?path=` | Capped raw upload, atomically replacing a regular file; conflict if the computer is in use. |
| `POST /workspace/folder` | Body with relative path; create an actual directory. |
| `DELETE /workspace/entry?path=` | Remove a regular file or empty directory with explicit confirmation. |
| `POST /workspace/browser-reset` | Explicit confirmed profile reset, preserving files; conflict while teardown cannot be confirmed. |

These routes forward only validated owner/Dot claims to internal runner operations. The runner repeats path and ownership checks. No runner URL or host directory is selected by the user. Existing disabled Dot ownership still allows inspection and file management; execution additionally requires active state and current granted tools.

Walkthrough with actual resource data:

1. Open a named Dot card and choose **Workspace**. The link targets `/dots/{id}/workspace`; the page header uses the returned Dot name.
2. Show the runner's actual availability and the root listing. An empty directory has an explicit empty state, not invented project folders. Breadcrumb labels use actual path segments.
3. Reuse the existing asset row treatment for filename, kind, size and download. Opening a directory navigates to its actual path. Do not create a duplicate attachment/document collection.
4. Upload a chosen file or create a folder using the existing form/save/error patterns. Show progress and the server's outcome, then reload the listing. Preserve the current path after failure.
5. Delete a selected file or empty folder after showing its actual name. A busy conflict explains that the current Dot run must finish or be stopped; it does not bypass that run's approvals.
6. A separate **Reset browser login** confirmation explains that the Dot's saved browser logins will be removed and files preserved. It does not display browser credentials.
7. On mobile, keep row names and action buttons within the viewport, use existing loading/error components and accessible translated controls. No new library is needed.

All visible strings use the six Fluent catalogs. Required groups cover workspace navigation/empty/loading/unavailable/busy, entry actions, upload outcome, browser reset confirmation/outcome and actionable storage/path/ownership errors. Resource names and descriptions are API data, not translation defaults. The coordinator verifies desktop/mobile UI in the real seeded gateway before completion.

## Retention, deletion and retries

Deactivation preserves files and browser profile. It prevents new execution through the existing Dot active-state checks; existing containers are released through the lifecycle path. Re-enabling accesses the same validated storage.

Dot deletion and account deletion enqueue a durable cleanup record containing the owner and Dot ID in the same database transaction as identity deletion. The record must survive both deletions, so it has no cascading foreign key to either. Amend the existing unreleased Dot migration and schema fixture; do not add one migration per package. Accessors belong in `aiplane-agents`; the runtime worker consumes the queue and calls the existing runner client.

Before removing storage, the runner seals the scope against new mounts, confirms all its containers are absent and validates its manifest. It atomically quarantines the private root and removes the protected tree. Retrying an already absent root succeeds. A failed stop or filesystem operation retains the durable cleanup record, reports a diagnostic without secrets and retries with bounded backoff. A crash after runner removal but before queue acknowledgement is handled by idempotence. A failed cleanup never reassigns storage to another Dot.

Existing startup scratch pruning targets `sbx-*`; it must continue to preserve `dots/`. The runner must not guess which stored Dots were deleted from directory age. The gateway cleanup queue is authoritative.

## Operator implications

Document the persistent runner volume in README, sandbox deployment and Kubernetes documentation in the implementation commit. Backups must cover the runner volume separately from gateway SQLite and encryption keys. Snapshotting app-created SQLite files requires writers stopped or an application-consistent backup; copying live files is insufficient.

`work_size` limits tmpfs scratch; it does not impose a per-Dot quota on bind-mounted files. Use the existing documented dedicated filesystem/volume capacity limit and disk monitoring. Do not advertise individual hard quotas until a filesystem-backed enforcement mechanism exists. Disk-full errors must preserve old files and return an actionable upload/tool failure. Browser profiles also consume this operator-managed capacity.

## Disjoint implementation packages and evidence

| Owner package | Files and responsibility |
|---|---|
| Coordinator: contract/schema/docs | `shared/src/sandbox.rs`, one Dot migration plus fixtures/checksum, architecture rules, docs index/README/deployment/API guides. Settle the typed scope and health fields before dependent edits. |
| Runner | `sandbox-runner/src/{config,backend,pool,server}.rs` and one filesystem module: mounts, owner proof, bounded paths, exclusive scope/profile locks, checked teardown and internal file/reset/delete operations. |
| Runtime | `aiplane-runtime/src/server/tools/sandbox/{mod,browse}.rs`, tool context callers and state-dependent cleanup worker; `aiplane-agents` cleanup accessors if leased explicitly. No UI/catalog edits. |
| API/UI | `aiplane-api` Dot workspace handlers and gateway routing; web workspace page/components/client, six catalogs/generated TS and new targeted browser scenario. Backend/API and UI files can be leased separately after the contract is fixed. |

Schema, shared protocol, catalogs/generated files and shared docs have one writer each. Packages do not compile Rust concurrently against the shared target; the coordinator grants the build lock. No package claims completion before its tests, documentation and real caller are integrated.

Tests are written red before implementation and assert observable state using real temporary files and existing fake backend/in-memory database collaborators:

- UUID/owner manifest creation and mismatch; traversal, symlink replacement races, special files, listing/read/upload bounds, atomic failed uploads and disk failures.
- Correct mount arguments; ordinary warm containers remain unscoped; foreign lease IDs rejected; expired same-scope lease preserves files; purpose cannot widen mounts or egress.
- Different turns contend; cooperating same-turn containers share files; profile survives browser recreation; teardown failure keeps the profile busy until confirmed recovery; startup stale-container failure blocks reuse.
- Persistent storage absent gives an explicit unavailable result; TTL/turn release removes containers while keeping files/profile.
- Dot/account deletion queue survives identity deletion; cleanup idempotence, retry after failure and hostile symlinks; deactivate/reactivate preserves data.
- Owner-authenticated workspace routes reject another user's Dot, overlarge bodies and profile paths; inactive inspection works while execution is denied.
- Browser daemon tests exercise persistent versus ordinary context setup and existing proxy settings; a real runner smoke scenario verifies a file and browser state across two containers.
- UI fixture uses actual listed names, unknown metadata, empty/unavailable/busy/error states, upload/download/delete, browser reset confirmation, keyboard access and mobile layout.

## Constraints to verify during implementation

The proposal requires confirmed teardown and consistent gateway assistant-turn context in both sandbox wrappers. These are shared prerequisites, not claims about existing behavior. The current workload image creates and runs as the `sandbox` user with UID 1001 (`sandbox-image/Containerfile`). Rootless Podman volume UID mappings, owner access to files created with restrictive permissions, browser-profile write access and cleanup permissions must be verified against that image before selecting directory modes. Ordinary scratch directories use mode 1777; this alone does not prove that the runner can inspect or remove nested private browser files. A persistent browser authentication experience cannot be inferred from MCP OAuth; document only the browser actions actually supported. The current single-runner-per-root topology is the supported deployment target.

The provided Linux installer targets rootful Podman (`deploy/sandbox/setup-sandbox.sh`), and its host service does not select a non-root user (`deploy/sandbox/sandbox-runner.service`). This is a concrete first deployment target for restrictive workload-owned mount leaves and runner-owned control directories, subject to real write/read/delete and browser-profile probes. The Docker Compose runner currently mounts only the Docker socket; it does not configure persistent work storage or establish identical bind-source paths inside the runner and on the daemon host (`deploy/compose.example.yml`). Neither rootless mapping nor remote-daemon path equivalence is an established capability. Persistent availability must remain unavailable where the selected deployment cannot prove those requirements.
