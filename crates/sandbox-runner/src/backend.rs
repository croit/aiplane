// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Container backend: the thin seam between the pool/HTTP logic and
//! `podman`. The real [`PodmanBackend`] shells out to drive single-use,
//! gVisor-isolated containers; the test-only `FakeBackend` lets the pool
//! logic be unit-tested without a container runtime present.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use shared::sandbox::{RunRequest, RunResponse};
use thiserror::Error;
use tokio::io::AsyncWriteExt;

use crate::config::Config;

#[derive(Debug, Error)]
pub enum BackendError {
    #[error("spawning podman failed: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("podman {op} exited {code}: {stderr}")]
    Command {
        op: &'static str,
        code: String,
        stderr: String,
    },
    #[error("sandbox-agent protocol error: {0}")]
    Protocol(String),
    #[error("sandbox control command failed: {0}")]
    Control(#[from] crate::process::ProcessError),
}

fn validate_podman_id(id: &str) -> Result<(), BackendError> {
    let hex = |part: &str| {
        !part.is_empty()
            && part
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    };
    let full_id = id.len() == 64 && hex(id);
    let name = id.strip_prefix("sbx-").is_some_and(|suffix| {
        let parts: Vec<_> = suffix.split('-').collect();
        parts.len() == 3
            && !parts[0].is_empty()
            && parts[0].len() <= 10
            && parts[0].bytes().all(|byte| byte.is_ascii_digit())
            && parts[1].len() <= 32
            && parts[2].len() <= 16
            && hex(parts[1])
            && hex(parts[2])
    });
    if full_id || name {
        Ok(())
    } else {
        Err(BackendError::Protocol(
            "container reference is not a full ID or generated sandbox name".into(),
        ))
    }
}

fn parse_container_ids(bytes: &[u8]) -> Result<Vec<String>, BackendError> {
    let text = std::str::from_utf8(bytes).map_err(|_| {
        BackendError::Protocol("container enumeration returned invalid UTF-8".into())
    })?;
    text.lines()
        .map(|line| {
            let id = line.trim();
            validate_podman_id(id)?;
            if id.len() != 64 {
                return Err(BackendError::Protocol(
                    "container enumeration requires full IDs".into(),
                ));
            }
            Ok(id.to_owned())
        })
        .collect()
}

fn listed_container_exists(bytes: &[u8], expected: &str) -> Result<bool, BackendError> {
    let text = std::str::from_utf8(bytes).map_err(|_| {
        BackendError::Protocol("container enumeration returned invalid UTF-8".into())
    })?;
    let mut found = false;
    for line in text.lines() {
        let (id, name) = line.split_once('\t').ok_or_else(|| {
            BackendError::Protocol("container enumeration omitted its ID or name".into())
        })?;
        validate_podman_id(id)?;
        if id.len() != 64 {
            return Err(BackendError::Protocol(
                "container enumeration returned a truncated ID".into(),
            ));
        }
        let mut name_matches = false;
        for name in name.split(',').map(str::trim) {
            if name.is_empty()
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
            {
                return Err(BackendError::Protocol(
                    "container enumeration returned an invalid name".into(),
                ));
            }
            name_matches |= name == expected;
        }
        found |= id == expected || name_matches;
    }
    Ok(found)
}

/// Label every sandbox container this runner creates carries, so a restart
/// (or an operator, or the unit's ExecStopPost) can find them all again.
pub const LABEL: &str = "app=aiplane-sandbox";

/// Network posture a sandbox container is created with. Pooled (warm)
/// containers are always [`Network::None`]; a call that requests and is
/// granted egress gets a fresh [`Network::Egress`] container instead, so
/// the default-deny pool is never reused for a networked job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Network {
    None,
    Egress,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct GuestProof {
    uid: u32,
    gid: u32,
    kernel: String,
    directory_uid: u32,
    directory_gid: u32,
    directory_mode: u32,
    file_uid: u32,
    file_gid: u32,
    file_mode: u32,
    nonce: String,
}

fn validate_guest_proof(
    proof: &GuestProof,
    host_kernel: &str,
    nonce: &str,
) -> Result<(), BackendError> {
    if proof.uid != 1001
        || proof.gid != 1001
        || proof.directory_uid != 1001
        || proof.directory_gid != 1001
        || proof.directory_mode != 0o700
        || proof.file_uid != 1001
        || proof.file_gid != 1001
        || proof.file_mode != 0o600
        || proof.nonce != nonce
        || crate::isolation::verdict(host_kernel, &proof.kernel)
            != crate::isolation::Verdict::Isolated
    {
        return Err(BackendError::Protocol("workspace probe did not establish workload identity, private permissions, persistent data and isolation".into()));
    }
    Ok(())
}

const CONTROL_DEADLINE: Duration = Duration::from_secs(30);

pub struct WorkspaceMount {
    pub name: String,
    pub files: Arc<crate::workspace::Workspace>,
    pub path: std::path::PathBuf,
    pub browser_profile_path: Option<std::path::PathBuf>,
}

/// Abstracts "boot a sandbox", "run a job in it", "destroy it". Object-
/// safe via `async_trait` so the pool can hold `Arc<dyn ContainerBackend>`
/// and tests can swap in a fake.
#[async_trait::async_trait]
pub trait ContainerBackend: Send + Sync + 'static {
    fn validate_container_id(&self, id: &str) -> Result<(), BackendError>;
    /// Boot one fresh, idle sandbox container and return its id.
    async fn create(
        &self,
        network: Network,
        workspace: Option<&WorkspaceMount>,
    ) -> Result<String, BackendError>;

    fn workspace_available(&self) -> bool {
        false
    }

    /// Run one job inside an existing container. `timeout` is the hard
    /// wall-clock stop; on overrun the returned response has
    /// `timed_out = true` (the caller destroys the container regardless).
    async fn exec(
        &self,
        id: &str,
        req: &RunRequest,
        timeout: Duration,
    ) -> Result<RunResponse, BackendError>;

    async fn browser_control(
        &self,
        _id: &str,
        _operation: &shared::sandbox::BrowserControlOperation,
    ) -> Result<shared::sandbox::BrowserControlResponse, BackendError> {
        Err(BackendError::Protocol(
            "browser handover is unavailable on this backend".into(),
        ))
    }

    async fn destroy(&self, id: &str) -> Result<(), BackendError>;

    /// Content id of the configured workload image as it resolves *right
    /// now*. The pool snapshots this and re-checks periodically; a change
    /// means the image was rebuilt or re-tagged, so warm containers booted
    /// from the old id are stale and get recycled. The default returns a
    /// fixed sentinel for backends with no real image (e.g. the local dev
    /// backend), which simply never reports a change.
    async fn image_id(&self) -> Result<String, BackendError> {
        Ok("static".to_string())
    }
}

/// Drives `podman` to run each job under the configured OCI runtime
/// (`runsc` by default). Every container is locked down: read-only rootfs,
/// all capabilities dropped, no-new-privileges, tmpfs `/work`, resource
/// Monotonic part of a container name, so two containers created in the same
/// nanosecond still differ.
static CONTAINER_SEQ: AtomicU64 = AtomicU64::new(0);

/// Name for a fresh sandbox container. The runner names containers itself
/// (rather than using the id podman prints) because the host directories that
/// back `/work` and `/tmp` have to be created — and passed as bind mounts —
/// *before* `podman run` returns an id. Deriving the paths from the name keeps
/// the mapping stateless: `destroy` and `read_spill` recompute it.
pub(crate) fn new_container_name() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = CONTAINER_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("sbx-{}-{nanos:x}-{seq:x}", std::process::id())
}

/// Host directories backing one container's `/work` and `/tmp`.
fn host_dirs(root: &Path, name: &str) -> (PathBuf, PathBuf) {
    let base = root.join(name);
    (base.join("work"), base.join("tmp"))
}

/// Create the per-container scratch directories. The shared root is kept
/// `0700` so only the runner can walk it from the host; each container's own
/// directories are `1777` to mirror the `mode=1777` the tmpfs mounts used, so
/// whichever uid the image runs as can write there. The container reaches them
/// through the bind mount, not by traversing the root, so the tight root
/// permission costs nothing.
fn prepare_host_dirs(root: &Path, work: &Path, tmp: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(root)?;
    std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))?;
    for d in [work, tmp] {
        std::fs::create_dir_all(d)?;
        std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o1777))?;
    }
    Ok(())
}

/// Read a file the sandbox produced, out of a host bind mount.
///
/// Everything in `dir` was written by untrusted code, so this is deliberately
/// narrow:
///
/// * `name` must be a single path component — no separators, no `..`. The
///   agent writes its spill file and artifacts flat, so nothing legitimate
///   needs a subdirectory, and forbidding them removes the whole class of
///   attacks where an intermediate directory is a symlink (`O_NOFOLLOW` only
///   protects the final component).
/// * the open uses `O_NOFOLLOW`, so a symlink planted under a plausible name
///   (`out.mp4` → `/etc/shadow`) fails instead of being read.
/// * the opened descriptor must be a regular file — checked via the handle,
///   not the path, so there is no time-of-check/time-of-use window.
pub fn read_sandbox_file(dir: &Path, name: &str) -> Result<Vec<u8>, BackendError> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;

    if name.is_empty() || Path::new(name).components().count() != 1 || name.contains('/') {
        return Err(BackendError::Protocol(format!(
            "refusing to read {name:?} from a sandbox scratch dir: not a plain file name"
        )));
    }
    let path = dir.join(name);
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&path)
        .map_err(|e| {
            BackendError::Protocol(format!(
                "opening {} from sandbox scratch: {e}",
                path.display()
            ))
        })?;
    let md = f.metadata().map_err(|e| {
        BackendError::Protocol(format!("stat {} from sandbox scratch: {e}", path.display()))
    })?;
    if !md.is_file() {
        return Err(BackendError::Protocol(format!(
            "{} is not a regular file",
            path.display()
        )));
    }
    let mut buf = Vec::with_capacity(md.len() as usize);
    f.read_to_end(&mut buf).map_err(|e| {
        BackendError::Protocol(format!(
            "reading {} from sandbox scratch: {e}",
            path.display()
        ))
    })?;
    Ok(buf)
}

/// Split a container-side absolute path into (which mount, file name), for
/// translating an agent-reported path to its host location. Returns `None` for
/// anything that isn't directly inside `/work` or `/tmp`.
fn split_mount_path(path: &str) -> Option<(Mount, &str)> {
    for (prefix, mount) in [("/work/", Mount::Work), ("/tmp/", Mount::Tmp)] {
        if let Some(rest) = path.strip_prefix(prefix) {
            return Some((mount, rest));
        }
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mount {
    Work,
    Tmp,
}

/// caps, and no network unless [`Network::Egress`] is requested.
pub struct PodmanBackend {
    cfg: Arc<Config>,
    workspace_ready: std::sync::atomic::AtomicBool,
}

impl PodmanBackend {
    pub fn new(cfg: Arc<Config>) -> Self {
        Self {
            cfg,
            workspace_ready: std::sync::atomic::AtomicBool::new(false),
        }
    }

    fn command(&self, local: bool) -> tokio::process::Command {
        let mut command = tokio::process::Command::new(&self.cfg.podman);
        if local || self.workspace_available() {
            command.arg("--remote=false");
        }
        command
    }

    async fn control(
        &self,
        args: &[String],
        local: bool,
    ) -> Result<std::process::Output, BackendError> {
        let mut command = self.command(local);
        command.args(args);
        Ok(crate::process::run_control(command, CONTROL_DEADLINE).await?)
    }

    async fn destroy_control(&self, id: &str, local: bool) -> Result<(), BackendError> {
        self.validate_container_id(id)?;
        let removed = self
            .control(&["rm".into(), "-f".into(), id.into()], local)
            .await;
        if !matches!(&removed, Ok(output) if output.status.success()) {
            let listed = self
                .control(
                    &[
                        "ps".into(),
                        "-a".into(),
                        "--no-trunc".into(),
                        "--format".into(),
                        "{{.ID}}\t{{.Names}}".into(),
                    ],
                    local,
                )
                .await?;
            if !listed.status.success() {
                return Err(BackendError::Command {
                    op: "ps",
                    code: listed.status.to_string(),
                    stderr: String::from_utf8_lossy(&listed.stderr).into_owned(),
                });
            }
            if listed_container_exists(&listed.stdout, id)? {
                return Err(BackendError::Protocol(
                    "container removal failed and the container remains present; retry teardown"
                        .into(),
                ));
            }
        }
        self.remove_host_dirs(id);
        Ok(())
    }

    pub async fn verify_workspace_readiness(&self) -> Result<(), BackendError> {
        self.workspace_ready
            .store(false, std::sync::atomic::Ordering::Release);
        if !cfg!(target_os = "linux")
            || unsafe { libc::geteuid() } != 0
            || self.cfg.runtime != "runsc"
        {
            return Err(BackendError::Protocol(
                "persistent workspaces require the verified local rootful Linux gVisor deployment"
                    .into(),
            ));
        }
        if ["CONTAINER_HOST", "CONTAINER_CONNECTION"]
            .iter()
            .any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()))
        {
            return Err(BackendError::Protocol(
                "remote Podman configuration cannot establish local persistent workspace readiness"
                    .into(),
            ));
        }
        let root = self.cfg.work_root.as_ref().ok_or_else(|| {
            BackendError::Protocol("persistent workspace storage is not configured".into())
        })?;
        let rootless = self
            .control(
                &[
                    "info".into(),
                    "--format".into(),
                    "{{.Host.Security.Rootless}}".into(),
                ],
                true,
            )
            .await?;
        if !rootless.status.success()
            || std::str::from_utf8(&rootless.stdout).ok().map(str::trim) != Some("false")
        {
            return Err(BackendError::Protocol(
                "Podman did not verify local rootful execution".into(),
            ));
        }
        let host_kernel = std::fs::read_to_string("/proc/sys/kernel/osrelease")?;
        if host_kernel.trim().is_empty() {
            return Err(BackendError::Protocol(
                "host kernel identity is unavailable".into(),
            ));
        }
        let probe = crate::workspace::WorkspaceProbe::create(root)
            .map_err(|error| BackendError::Protocol(error.to_string()))?;
        let prepared = probe
            .prepare_workload_owner(1001, 1001)
            .map_err(|error| BackendError::Protocol(error.to_string()));
        if let Err(error) = prepared {
            let _ = probe.cleanup();
            return Err(error);
        }
        let path = probe
            .mount_path()
            .map_err(|error| BackendError::Protocol(error.to_string()))?;
        if path.to_str().is_none_or(|path| path.contains(':')) {
            probe
                .cleanup()
                .map_err(|error| BackendError::Protocol(error.to_string()))?;
            return Err(BackendError::Protocol(
                "workspace storage path cannot be used as a Podman bind mount".into(),
            ));
        }
        use std::io::Read;
        let mut random = [0u8; 32];
        std::fs::File::open("/dev/urandom")?.read_exact(&mut random)?;
        let nonce: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
        for write in [true, false] {
            let name = new_container_name();
            let outcome = self
                .probe_container(&probe, &path, &name, &nonce, &host_kernel, write)
                .await;
            // A failed CLI may have created the named container before reporting failure.
            self.destroy_control(&name, true).await?;
            if let Err(error) = outcome {
                probe
                    .cleanup()
                    .map_err(|cleanup| BackendError::Protocol(cleanup.to_string()))?;
                return Err(error);
            }
        }
        probe
            .cleanup()
            .map_err(|error| BackendError::Protocol(error.to_string()))?;
        self.workspace_ready
            .store(true, std::sync::atomic::Ordering::Release);
        Ok(())
    }

    async fn probe_container(
        &self,
        probe: &crate::workspace::WorkspaceProbe,
        path: &Path,
        name: &str,
        nonce: &str,
        host_kernel: &str,
        write: bool,
    ) -> Result<(), BackendError> {
        use std::os::unix::fs::MetadataExt;
        self.validate_container_id(name)?;
        probe
            .validate_mount_path(path)
            .map_err(|error| BackendError::Protocol(error.to_string()))?;
        let root = self
            .cfg
            .work_root
            .as_ref()
            .ok_or_else(|| BackendError::Protocol("workspace storage unavailable".into()))?;
        let (work, tmp) = host_dirs(root, name);
        prepare_host_dirs(root, &work, &tmp)?;
        let started = self
            .control(
                &self.run_args_with_workspace_path(Network::None, name, Some(path)),
                true,
            )
            .await?;
        if !started.status.success() || parse_container_ids(&started.stdout)?.len() != 1 {
            return Err(BackendError::Protocol(
                "workspace probe container could not be started".into(),
            ));
        }
        let script = format!(
            r#"import os,json,stat,platform
os.umask(0o077)
assert os.getuid()==1001 and os.getgid()==1001
if {write}:
    os.mkdir('/workspace/private',0o700)
    fd=os.open('/workspace/private/probe.bin',os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600)
    with os.fdopen(fd,'wb') as f:
        f.write(b'{nonce}')
        f.flush()
        os.fsync(f.fileno())
d=os.stat('/workspace/private',follow_symlinks=False)
f=os.stat('/workspace/private/probe.bin',follow_symlinks=False)
with open('/workspace/private/probe.bin','rb') as handle:
    data=handle.read(65).decode('ascii')
print(json.dumps(dict(uid=os.getuid(),gid=os.getgid(),kernel=platform.release(),directory_uid=d.st_uid,directory_gid=d.st_gid,directory_mode=stat.S_IMODE(d.st_mode),file_uid=f.st_uid,file_gid=f.st_gid,file_mode=stat.S_IMODE(f.st_mode),nonce=data)))
"#,
            write = if write { "True" } else { "False" }
        );
        let output = self
            .control(
                &[
                    "exec".into(),
                    name.into(),
                    "python3".into(),
                    "-c".into(),
                    script,
                ],
                true,
            )
            .await?;
        if !output.status.success() {
            return Err(BackendError::Protocol(
                "workspace probe execution failed".into(),
            ));
        }
        let proof: GuestProof = serde_json::from_slice(&output.stdout).map_err(|_| {
            BackendError::Protocol("workspace probe returned an invalid proof".into())
        })?;
        validate_guest_proof(&proof, host_kernel, nonce)?;
        let bytes = probe
            .read("private/probe.bin")
            .map_err(|error| BackendError::Protocol(error.to_string()))?;
        let directory = probe
            .metadata("private")
            .map_err(|error| BackendError::Protocol(error.to_string()))?;
        let file = probe
            .metadata("private/probe.bin")
            .map_err(|error| BackendError::Protocol(error.to_string()))?;
        if bytes != nonce.as_bytes()
            || !directory.is_dir()
            || directory.uid() != 1001
            || directory.gid() != 1001
            || directory.mode() & 0o7777 != 0o700
            || !file.is_file()
            || file.uid() != 1001
            || file.gid() != 1001
            || file.mode() & 0o7777 != 0o600
            || file.len() != 64
        {
            return Err(BackendError::Protocol(
                "host filesystem did not confirm the private workload data".into(),
            ));
        }
        Ok(())
    }

    /// Best-effort removal of one container's scratch directories, keyed by
    /// the container name `create` handed out. A crafted name must not be able
    /// to walk out of the root, hence the single-component check.
    fn remove_host_dirs(&self, name: &str) {
        let Some(root) = &self.cfg.work_root else {
            return;
        };
        if name.is_empty() || Path::new(name).components().count() != 1 {
            tracing::warn!(
                container = name,
                "refusing to remove sandbox scratch for a suspicious container name"
            );
            return;
        }
        let dir = root.join(name);
        match std::fs::remove_dir_all(&dir) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => tracing::warn!(
                dir = %dir.display(), error = %e,
                "removing sandbox scratch failed; disk may leak"
            ),
            _ => {}
        }
    }

    /// Remove sandbox containers left over from a previous runner process.
    ///
    /// A restart empties the pool, so every container carrying our label is
    /// unusable afterwards: the new process tracks none of them, and a gateway
    /// still holding a lease id just falls back to a fresh single-use sandbox.
    /// Without this they linger until someone notices (and, with host-backed
    /// scratch, keep their directories pinned too). Assumes one runner per
    /// host, which the fixed bind address already implies.
    pub async fn reap_stale_containers(&self) -> Result<(), BackendError> {
        let out = self
            .command(false)
            .args([
                "ps",
                "-aq",
                "--no-trunc",
                "--filter",
                &format!("label={LABEL}"),
            ])
            .stdin(Stdio::null())
            .output()
            .await?;
        if !out.status.success() {
            return Err(BackendError::Command {
                op: "ps",
                code: out.status.to_string(),
                stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            });
        }
        let ids = parse_container_ids(&out.stdout)?;
        if ids.is_empty() {
            return Ok(());
        }
        tracing::info!(
            count = ids.len(),
            "reaping sandbox containers from a previous run"
        );
        for id in ids {
            self.destroy(&id).await?;
        }
        Ok(())
    }

    /// Read artifacts the agent deliberately left on disk.
    ///
    /// With host-backed scratch the agent reports metadata with an empty
    /// `content_b64` (see `SANDBOX_ARTIFACTS_INPLACE`), so a large produced
    /// file is never base64-encoded inside the sandbox nor squeezed through its
    /// response JSON — that is what removes the agent's 64 MiB cap. The bytes
    /// are read here through the same guarded path as the spill file; the hop
    /// to the gateway keeps its existing base64 encoding.
    ///
    /// An artifact whose file cannot be read is dropped rather than delivered
    /// empty, so the gateway never stores a 0-byte file under a real name.
    fn hydrate_inplace_artifacts(&self, container: &str, resp: &mut RunResponse) {
        let Some(root) = &self.cfg.work_root else {
            return;
        };
        let (work, _tmp) = host_dirs(root, container);
        resp.artifacts.retain_mut(|a| {
            if !a.content_b64.is_empty() || a.size == 0 {
                return true;
            }
            match read_sandbox_file(&work, &a.name) {
                Ok(bytes) => {
                    a.content_b64 = shared::b64::encode(&bytes);
                    true
                }
                Err(e) => {
                    tracing::warn!(
                        container, artifact = %a.name, error = %e,
                        "dropping artifact: could not read it from the sandbox scratch dir"
                    );
                    false
                }
            }
        });
    }

    /// Drop scratch directories whose container is gone — the runner crashing
    /// mid-job would otherwise leave the files (and their disk) behind forever.
    /// Called at startup after [`Self::reap_stale_containers`], so in practice
    /// nothing is live and everything stale goes; normal teardown goes through
    /// [`Self::remove_host_dirs`].
    pub async fn prune_orphan_scratch(&self) {
        let Some(root) = &self.cfg.work_root else {
            return;
        };
        let out = self
            .command(false)
            .args([
                "ps",
                "-a",
                "--filter",
                &format!("label={LABEL}"),
                "--format",
                "{{.Names}}",
            ])
            .stdin(Stdio::null())
            .output()
            .await;
        // Without a reliable list of live containers, removing nothing is the
        // safe error: a stale directory wastes disk, a wrongly removed one
        // breaks a running job.
        let Ok(out) = out else {
            tracing::warn!("listing sandbox containers failed; skipping scratch prune");
            return;
        };
        if !out.status.success() {
            tracing::warn!("listing sandbox containers failed; skipping scratch prune");
            return;
        }
        let live: std::collections::HashSet<String> = String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
        let Ok(entries) = std::fs::read_dir(root) else {
            return;
        };
        let mut pruned = 0usize;
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with("sbx-") && !live.contains(&name) {
                self.remove_host_dirs(&name);
                pruned += 1;
            }
        }
        if pruned > 0 {
            tracing::info!(pruned, "pruned orphaned sandbox scratch directories");
        }
    }

    /// Hardening + lifecycle flags shared by every `podman run`.
    ///
    /// `name` is ours (see [`new_container_name`]) so the scratch bind mounts
    /// can be derived from it later without extra bookkeeping.
    fn run_args(
        &self,
        network: Network,
        name: &str,
        workspace: Option<&WorkspaceMount>,
    ) -> Vec<String> {
        self.run_args_with_mount_paths(
            network,
            name,
            workspace.map(|mount| mount.path.as_path()),
            workspace.and_then(|mount| mount.browser_profile_path.as_deref()),
        )
    }

    fn run_args_with_workspace_path(
        &self,
        network: Network,
        name: &str,
        workspace_path: Option<&Path>,
    ) -> Vec<String> {
        self.run_args_with_mount_paths(network, name, workspace_path, None)
    }

    fn run_args_with_mount_paths(
        &self,
        network: Network,
        name: &str,
        workspace_path: Option<&Path>,
        browser_profile_path: Option<&Path>,
    ) -> Vec<String> {
        let c = &self.cfg;
        let mut a: Vec<String> = vec![
            "run".into(),
            "-d".into(),
            "--name".into(),
            name.to_string(),
            "--runtime".into(),
            c.runtime.clone(),
            "--read-only".into(),
        ];
        // Writable scratch for the job; rootfs stays read-only. `/work` is the
        // job's CWD; `/tmp` is exec-mounted because chromium and LibreOffice
        // drop helper binaries there.
        //
        // With `work_root` set these are host bind mounts, so produced files
        // are readable straight from the host instead of having to be shipped
        // back through 64 KiB `podman exec` reads (see `Config::work_root`).
        // Bind mounts carry no `size=`, so the backing filesystem is the quota.
        if let Some(root) = &c.work_root {
            let (work, tmp) = host_dirs(root, name);
            a.push("-v".into());
            a.push(format!("{}:/work:rw", work.display()));
            a.push("-v".into());
            a.push(format!("{}:/tmp:rw,exec", tmp.display()));
        } else {
            a.push("--tmpfs".into());
            a.push(format!("/work:rw,size={},mode=1777", c.work_size));
            a.push("--tmpfs".into());
            a.push(format!("/tmp:rw,exec,size={},mode=1777", c.tmp_size));
        }
        if let Some(path) = workspace_path {
            a.push("-v".into());
            a.push(format!("{}:/workspace:rw", path.display()));
        }
        if let Some(path) = browser_profile_path {
            a.push("-v".into());
            a.push(format!("{}:/dot-browser:rw", path.display()));
        }
        a.extend::<Vec<String>>(vec![
            "--cap-drop=ALL".into(),
            "--security-opt".into(),
            "no-new-privileges".into(),
            "--memory".into(),
            c.memory.clone(),
            // Pin memory+swap to the same value so the guest can't use swap to
            // exceed --memory. Without this podman defaults --memory-swap to
            // 2x --memory, doubling the effective cap and letting a memory bomb
            // run far past it. Equal values = swap disabled, hard cap at --memory.
            "--memory-swap".into(),
            c.memory.clone(),
            "--cpus".into(),
            c.cpus.clone(),
            "--pids-limit".into(),
            c.pids_limit.to_string(),
            // Make a runaway sandbox the first thing the host OOM-killer reaps.
            // Even with the cgroup cap above, gVisor's sentry holds host memory
            // proportional to guest use; under host pressure this ensures the
            // sandbox dies, not the runner (which sets OOMScoreAdjust=-800).
            "--oom-score-adj".into(),
            "1000".into(),
            "--label".into(),
            LABEL.into(),
        ]);
        match network {
            Network::None => {
                a.push("--network".into());
                a.push("none".into());
            }
            Network::Egress => {
                a.push("--network".into());
                a.push(c.egress_network.clone());
                if !c.egress_proxy.is_empty() {
                    for var in ["HTTP_PROXY", "HTTPS_PROXY", "http_proxy", "https_proxy"] {
                        a.push("--env".into());
                        a.push(format!("{var}={}", c.egress_proxy));
                    }
                }
            }
        }
        a.push(c.image.clone());
        // Keep the container alive and idle until we `exec` a job into it.
        a.push("sleep".into());
        a.push("infinity".into());
        a
    }
}

#[async_trait::async_trait]
impl ContainerBackend for PodmanBackend {
    fn validate_container_id(&self, id: &str) -> Result<(), BackendError> {
        validate_podman_id(id)
    }
    fn workspace_available(&self) -> bool {
        self.workspace_ready
            .load(std::sync::atomic::Ordering::Acquire)
    }
    async fn create(
        &self,
        network: Network,
        workspace: Option<&WorkspaceMount>,
    ) -> Result<String, BackendError> {
        if workspace.is_some() && !self.workspace_available() {
            return Err(BackendError::Protocol(
                "persistent workspace mount readiness has not been verified".into(),
            ));
        }
        if let Some(mount) = workspace {
            mount
                .files
                .validate_mount_path(&mount.path)
                .map_err(|error| BackendError::Protocol(error.to_string()))?;
            if let Some(path) = &mount.browser_profile_path {
                mount
                    .files
                    .validate_browser_profile_path(path)
                    .map_err(|error| BackendError::Protocol(error.to_string()))?;
            }
            mount
                .files
                .prepare_workload_owner(1001, 1001)
                .map_err(|error| BackendError::Protocol(error.to_string()))?;
        }
        let name = workspace
            .map(|mount| mount.name.clone())
            .unwrap_or_else(new_container_name);
        self.validate_container_id(&name)?;
        if let Some(root) = &self.cfg.work_root {
            let (work, tmp) = host_dirs(root, &name);
            prepare_host_dirs(root, &work, &tmp).map_err(|e| {
                BackendError::Protocol(format!(
                    "preparing sandbox scratch under {}: {e}",
                    root.display()
                ))
            })?;
        }
        let args = self.run_args(network, &name, workspace);
        let out = self.control(&args, false).await?;
        if !out.status.success() {
            if workspace.is_none() {
                self.remove_host_dirs(&name);
            }
            return Err(BackendError::Command {
                op: "run",
                code: out.status.code().map(|c| c.to_string()).unwrap_or_default(),
                stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
            });
        }
        let ids = parse_container_ids(&out.stdout)?;
        if ids.len() != 1 {
            return Err(BackendError::Protocol(
                "podman run did not return exactly one full ID".into(),
            ));
        }
        // Hand back the name, not the id podman printed: podman accepts either
        // wherever a container is named, and the name is what the scratch
        // directories are derived from.
        Ok(name)
    }

    async fn exec(
        &self,
        id: &str,
        req: &RunRequest,
        timeout: Duration,
    ) -> Result<RunResponse, BackendError> {
        // The job marshalling lives inside the image: pipe the RunRequest to
        // `sandbox-agent` on stdin, read a RunResponse back on stdout.
        let mut cmd = self.command(false);
        cmd.arg("exec").arg("-i");
        if self.cfg.work_root.is_some() {
            // Host-backed scratch: tell the agent to leave produced files on
            // disk instead of base64-ing them into its response. We read them
            // below, which is what lifts its 64 MiB artifact cap.
            cmd.arg("-e").arg("SANDBOX_ARTIFACTS_INPLACE=1");
        }
        let child = cmd
            .arg(id)
            .arg("/usr/local/bin/sandbox-agent")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut resp = drive_agent(child, id, req, timeout, self).await?;
        self.hydrate_inplace_artifacts(id, &mut resp);
        Ok(resp)
    }

    async fn browser_control(
        &self,
        id: &str,
        operation: &shared::sandbox::BrowserControlOperation,
    ) -> Result<shared::sandbox::BrowserControlResponse, BackendError> {
        self.validate_container_id(id)?;
        let payload = browser_daemon_payload(operation)?;
        let mut command = self.command(true);
        command.args([
            "exec",
            "-i",
            "--user",
            "1001:1001",
            id,
            "python",
            "-c",
            BROWSER_CONTROL_CLIENT,
        ]);
        let output = crate::process::run_control_with_input(
            command,
            &payload,
            Duration::from_secs(15),
            3 * 1024 * 1024,
        )
        .await?;
        if !output.status.success() {
            return Err(BackendError::Protocol(
                "the browser handover command did not complete".into(),
            ));
        }
        decode_browser_control_response(&output.stdout)
    }

    async fn image_id(&self) -> Result<String, BackendError> {
        let out = self
            .command(false)
            .args(["image", "inspect", &self.cfg.image, "--format", "{{.Id}}"])
            .stdin(Stdio::null())
            .output()
            .await?;
        if !out.status.success() {
            return Err(BackendError::Command {
                op: "image inspect",
                code: out.status.code().map(|c| c.to_string()).unwrap_or_default(),
                stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
            });
        }
        let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if id.is_empty() {
            return Err(BackendError::Protocol("image inspect printed no id".into()));
        }
        Ok(id)
    }

    async fn destroy(&self, id: &str) -> Result<(), BackendError> {
        self.destroy_control(id, false).await
    }
}

const BROWSER_CONTROL_CLIENT: &str = r#"import socket,sys
s=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM)
s.settimeout(10)
s.connect('/tmp/gwbp.sock')
data=sys.stdin.buffer.read(16385)
if len(data)>16384: raise SystemExit(2)
s.sendall(data+b'\n')
out=b''
while not out.endswith(b'\n'):
    part=s.recv(65536)
    if not part: break
    out+=part
    if len(out)>3145728: raise SystemExit(3)
s.close()
sys.stdout.buffer.write(out)
"#;

fn browser_daemon_payload(
    operation: &shared::sandbox::BrowserControlOperation,
) -> Result<Vec<u8>, BackendError> {
    let command = match operation {
        shared::sandbox::BrowserControlOperation::Begin => {
            serde_json::json!({ "action": "human_begin" })
        }
        shared::sandbox::BrowserControlOperation::Claim { .. } => {
            return Err(BackendError::Protocol(
                "browser controller claims are handled by the runner".into(),
            ));
        }
        shared::sandbox::BrowserControlOperation::Frame { .. } => {
            serde_json::json!({ "action": "human_frame" })
        }
        shared::sandbox::BrowserControlOperation::Input { input, .. } => serde_json::json!({
            "action": "human_input",
            "input": input,
        }),
        shared::sandbox::BrowserControlOperation::Finish { .. } => {
            serde_json::json!({ "action": "human_finish" })
        }
    };
    serde_json::to_vec(&command)
        .map(|mut bytes| {
            bytes.push(b'\n');
            bytes
        })
        .map_err(|_| {
            BackendError::Protocol("the browser handover request could not be encoded".into())
        })
}

fn decode_browser_control_response(
    bytes: &[u8],
) -> Result<shared::sandbox::BrowserControlResponse, BackendError> {
    let response: shared::sandbox::BrowserControlResponse =
        serde_json::from_slice(bytes).map_err(|_| {
            BackendError::Protocol("the browser returned an invalid handover response".into())
        })?;
    if !response.ok {
        return Err(BackendError::Protocol(
            "the browser could not complete the handover operation".into(),
        ));
    }
    Ok(response)
}

#[async_trait::async_trait]
impl SpillReader for PodmanBackend {
    /// Read the spilled file back in sub-cap chunks: one `dd` block per
    /// `podman exec`, each ≤ `CHUNK` bytes so it clears gVisor's 64 KiB
    /// exec-stdout cap intact. The container is still alive (it ran `sleep
    /// infinity` and we only exec'd the agent into it), so these follow-up
    /// execs land before the pool destroys it.
    async fn read_spill(
        &self,
        container: &str,
        path: &str,
        len: u64,
    ) -> Result<Vec<u8>, BackendError> {
        // Fast path: with a host-backed scratch mount the spill file is an
        // ordinary file on this host, so one `read` replaces the thousands of
        // 60 KiB execs below (426 KiB/s measured). See `Config::work_root`.
        if let Some(root) = &self.cfg.work_root {
            if let Some((mount, name)) = split_mount_path(path) {
                let (work, tmp) = host_dirs(root, container);
                let dir = match mount {
                    Mount::Work => work,
                    Mount::Tmp => tmp,
                };
                let buf = read_sandbox_file(&dir, name)?;
                if buf.len() as u64 != len {
                    return Err(BackendError::Protocol(format!(
                        "spilled response size mismatch: {} bytes on disk, agent announced {len}",
                        buf.len()
                    )));
                }
                return Ok(buf);
            }
            tracing::debug!(
                path,
                "spill path is not inside a scratch mount; falling back to chunked exec reads"
            );
        }
        const CHUNK: u64 = 60 * 1024;
        let len = len as usize;
        let mut buf: Vec<u8> = Vec::with_capacity(len);
        let mut block: u64 = 0;
        while buf.len() < len {
            let out = self
                .command(false)
                .arg("exec")
                .arg(container)
                .arg("dd")
                .arg(format!("if={path}"))
                .arg(format!("bs={CHUNK}"))
                .arg(format!("skip={block}"))
                .arg("count=1")
                .arg("status=none")
                .stdin(Stdio::null())
                .output()
                .await?;
            if !out.status.success() {
                return Err(BackendError::Command {
                    op: "exec dd",
                    code: out.status.code().map(|c| c.to_string()).unwrap_or_default(),
                    stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
                });
            }
            if out.stdout.is_empty() {
                break; // unexpected early EOF; the length check below catches it
            }
            buf.extend_from_slice(&out.stdout);
            block += 1;
        }
        if buf.len() != len {
            return Err(BackendError::Protocol(format!(
                "spilled response short read: got {} of {len} bytes from {path}",
                buf.len()
            )));
        }
        Ok(buf)
    }
}

/// Header the agent prints on stdout INSTEAD of an inline RunResponse when the
/// response is too large for the gVisor-capped `podman exec` stdout (>64 KiB):
/// it names a temp file the agent spilled the full RunResponse JSON to, which
/// [`SpillReader`] reads back. See `sandbox-image/sandbox-agent`.
#[derive(serde::Deserialize)]
struct SpillHeader {
    sandbox_response_file: String,
    sandbox_response_bytes: u64,
}

/// Reads a spilled response file back out of a sandbox. Container backends pull
/// it through `podman exec` in sub-64 KiB chunks (gVisor silently truncates a
/// single exec's stdout at exactly 64 KiB); the dev [`LocalBackend`] reads the
/// host file directly. Kept off [`ContainerBackend`] since only [`drive_agent`]
/// needs it.
#[async_trait::async_trait]
trait SpillReader: Send + Sync {
    async fn read_spill(
        &self,
        container: &str,
        path: &str,
        len: u64,
    ) -> Result<Vec<u8>, BackendError>;
}

/// Decode the agent's stdout into a [`RunResponse`]. A small response arrives
/// inline (this also stays compatible with an agent predating the spill
/// protocol); a large one arrives as a [`SpillHeader`] naming a file we read
/// back via `spill`.
async fn decode_agent_output(
    stdout: &[u8],
    stderr: &[u8],
    container: &str,
    spill: &dyn SpillReader,
) -> Result<RunResponse, BackendError> {
    // Inline RunResponse first — a header lacks RunResponse's required fields
    // (and vice versa), so the two shapes never collide.
    let inline_err = match serde_json::from_slice::<RunResponse>(stdout) {
        Ok(resp) => return Ok(resp),
        Err(e) => e,
    };
    if let Ok(hdr) = serde_json::from_slice::<SpillHeader>(stdout) {
        let bytes = spill
            .read_spill(
                container,
                &hdr.sandbox_response_file,
                hdr.sandbox_response_bytes,
            )
            .await?;
        return serde_json::from_slice::<RunResponse>(&bytes).map_err(|e| {
            BackendError::Protocol(format!("spilled response not a RunResponse: {e}"))
        });
    }
    Err(BackendError::Protocol(format!(
        "agent output not a RunResponse: {inline_err}; stderr={}",
        String::from_utf8_lossy(stderr).trim()
    )))
}

/// Pipe a job to a spawned `sandbox-agent` process, enforce the wall-clock
/// timeout, and decode its RunResponse. Shared by every backend so the
/// agent protocol lives in exactly one place. `container` is the id `spill`
/// reads a large (spilled) response from; ignored for small inline responses.
async fn drive_agent(
    mut child: tokio::process::Child,
    container: &str,
    req: &RunRequest,
    timeout: Duration,
    spill: &dyn SpillReader,
) -> Result<RunResponse, BackendError> {
    let job =
        serde_json::to_vec(req).map_err(|e| BackendError::Protocol(format!("encode job: {e}")))?;
    if let Some(mut stdin) = child.stdin.take() {
        // A write error usually means the agent already exited; fall through
        // and let wait_with_output surface its stderr.
        let _ = stdin.write_all(&job).await;
        let _ = stdin.shutdown().await;
    }
    match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(out)) => {
            if !out.status.success() {
                return Err(BackendError::Command {
                    op: "exec",
                    code: out.status.code().map(|c| c.to_string()).unwrap_or_default(),
                    stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
                });
            }
            decode_agent_output(&out.stdout, &out.stderr, container, spill).await
        }
        // Outer timeout: report it; the caller destroys the sandbox, which
        // kills the in-flight process.
        Ok(Err(e)) => Err(BackendError::Spawn(e)),
        Err(_elapsed) => Ok(RunResponse {
            exit_code: -1,
            stdout: String::new(),
            stderr: format!("sandbox timed out after {}s", timeout.as_secs()),
            artifacts: Vec::new(),
            duration_ms: timeout.as_millis() as u64,
            timed_out: true,
            output_truncated: false,
            artifacts_truncated: false,
            container_id: None,
        }),
    }
}

#[cfg(test)]
mod teardown_validation_tests {
    use super::*;

    #[test]
    fn checked_teardown_validates_actual_generated_names_and_strict_full_ids() {
        for name in [new_container_name(), "a".repeat(64)] {
            validate_podman_id(&name).unwrap();
        }
        for invalid in [
            "",
            "-f",
            "sbx-",
            "sbx-1-x-2",
            "sbx-1-a-2-extra",
            "abcdef",
            "../sbx-1-a-2",
        ] {
            assert!(validate_podman_id(invalid).is_err(), "{invalid}");
        }
        assert!(parse_container_ids(&[0xff]).is_err());
        assert!(parse_container_ids(b"\n").is_err());
        assert!(parse_container_ids(b"sbx-1-a-2\n").is_err());
        let id = "a".repeat(64);
        assert_eq!(
            parse_container_ids(format!("{id}\n").as_bytes()).unwrap(),
            vec![id]
        );
        assert!(parse_container_ids(b"").unwrap().is_empty());
    }

    #[test]
    fn checked_teardown_enumeration_requires_exact_name_or_id_and_all_valid_rows() {
        let id = "a".repeat(64);
        let listing = format!("{id}\tsbx-1-ab-2\n");
        assert!(listed_container_exists(listing.as_bytes(), "sbx-1-ab-2").unwrap());
        assert!(listed_container_exists(listing.as_bytes(), &id).unwrap());
        assert!(!listed_container_exists(listing.as_bytes(), "sbx-1-ab-20").unwrap());
        assert!(!listed_container_exists(listing.as_bytes(), "aaaaaaaaaaaa").unwrap());
        assert!(!listed_container_exists(b"", "sbx-1-ab-2").unwrap());
        for invalid in [
            format!("{listing}bad\tother\n"),
            format!("{id}\n"),
            format!("{id}\t\n"),
        ] {
            assert!(listed_container_exists(invalid.as_bytes(), "sbx-1-ab-2").is_err());
        }
        assert!(listed_container_exists(&[0xff], "sbx-1-ab-2").is_err());
    }

    #[test]
    fn checked_teardown_local_validation_rejects_foreign_and_parent_paths() {
        let backend = LocalBackend {
            agent: PathBuf::new(),
        };
        let base = std::env::temp_dir().join("llm-sandbox-local");
        backend
            .validate_container_id(base.join("work-1-2").to_str().unwrap())
            .unwrap();
        for path in [
            base.clone(),
            base.join(".."),
            base.join("work-1-2/child"),
            base.join("sandbox-agent-1-2.py"),
            base.join("work-x-2"),
        ] {
            assert!(
                backend
                    .validate_container_id(path.to_str().unwrap())
                    .is_err(),
                "{}",
                path.display()
            );
        }
    }
}

#[cfg(test)]
mod podman_args_tests {
    use super::*;

    fn cfg() -> Arc<Config> {
        Arc::new(Config {
            bind: "127.0.0.1:9000".into(),
            image: "img".into(),
            runtime: "runsc".into(),
            podman: "podman".into(),
            pool_size: 3,
            max_concurrent: 6,
            image_check_secs: 0,
            default_timeout_secs: 60,
            max_timeout_secs: 300,
            memory: "1024m".into(),
            cpus: "2".into(),
            pids_limit: 256,
            work_size: "512m".into(),
            tmp_size: "512m".into(),
            work_root: None,
            max_output_bytes: 131_072,
            egress_network: String::new(),
            egress_proxy: String::new(),
            lease_ttl_secs: 600,
            max_leases: 6,
        })
    }

    fn has_pair(args: &[String], flag: &str, val: &str) -> bool {
        args.windows(2).any(|w| w[0] == flag && w[1] == val)
    }

    /// Pins the host-cgroup resource caps so a refactor can't silently drop the
    /// DoS-hardening flags (memory bomb regression: an unbounded guest crashed
    /// the runner before these were enforced).
    #[test]
    fn run_args_enforce_resource_caps() {
        let a = PodmanBackend::new(cfg()).run_args(Network::None, "sbx-test", None);
        assert!(has_pair(&a, "--memory", "1024m"));
        // swap pinned to memory → guest can't escape the cap via swap
        assert!(has_pair(&a, "--memory-swap", "1024m"));
        assert!(has_pair(&a, "--cpus", "2"));
        assert!(has_pair(&a, "--pids-limit", "256"));
        // a memory bomb is reaped before the runner (which is OOMScoreAdjust=-800)
        assert!(has_pair(&a, "--oom-score-adj", "1000"));
        // core lockdown still in place
        assert!(a.iter().any(|s| s == "--read-only"));
        assert!(a.iter().any(|s| s == "--cap-drop=ALL"));
        assert!(has_pair(&a, "--network", "none"));
        // scratch tmpfs sizes come from config (operator-tunable for large jobs)
        assert!(a.iter().any(|s| s == "/work:rw,size=512m,mode=1777"));
        assert!(a.iter().any(|s| s == "/tmp:rw,exec,size=512m,mode=1777"));
    }
}

/// The in-image agent source, embedded so the [`LocalBackend`] runs the
/// exact same marshaller as the container image (single source of truth).
const AGENT_SRC: &str = include_str!("../../../sandbox-image/sandbox-agent");

/// **DEV-ONLY, NO ISOLATION.** Runs the agent directly on the host (a temp
/// dir per job), so the full HTTP→runner→agent path is exercisable on a
/// machine without podman (e.g. macOS). Code runs with the runner's
/// own privileges — never select this in production. Activated by
/// `SANDBOX_RUNTIME=local-unsafe`.
pub struct LocalBackend {
    agent: std::path::PathBuf,
}

/// Process-wide counter for unique agent + workdir paths, so two
/// `LocalBackend` instances in one process (e.g. parallel tests) never
/// collide on the same file or working directory.
static LOCAL_SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn local_seq() -> usize {
    LOCAL_SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
}

impl LocalBackend {
    pub fn new() -> std::io::Result<Self> {
        let base = std::env::temp_dir().join("llm-sandbox-local");
        std::fs::create_dir_all(&base)?;
        let agent = base.join(format!(
            "sandbox-agent-{}-{}.py",
            std::process::id(),
            local_seq()
        ));
        std::fs::write(&agent, AGENT_SRC)?;
        Ok(Self { agent })
    }
}

#[async_trait::async_trait]
impl ContainerBackend for LocalBackend {
    fn validate_container_id(&self, id: &str) -> Result<(), BackendError> {
        let path = Path::new(id);
        let base = std::env::temp_dir().join("llm-sandbox-local");
        let valid = path.parent() == Some(base.as_path())
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_prefix("work-"))
                .is_some_and(|suffix| {
                    let parts: Vec<_> = suffix.split('-').collect();
                    parts.len() == 2
                        && parts.iter().all(|part| {
                            !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit())
                        })
                });
        if valid {
            Ok(())
        } else {
            Err(BackendError::Protocol(
                "local container reference is not a generated work directory".into(),
            ))
        }
    }
    async fn create(
        &self,
        _network: Network,
        workspace: Option<&WorkspaceMount>,
    ) -> Result<String, BackendError> {
        if workspace.is_some() {
            return Err(BackendError::Protocol(
                "local backend does not support persistent workspace mounts".into(),
            ));
        }
        let dir = std::env::temp_dir().join("llm-sandbox-local").join(format!(
            "work-{}-{}",
            std::process::id(),
            local_seq()
        ));
        std::fs::create_dir_all(&dir)?;
        Ok(dir.to_string_lossy().into_owned())
    }

    async fn exec(
        &self,
        id: &str,
        req: &RunRequest,
        timeout: Duration,
    ) -> Result<RunResponse, BackendError> {
        let child = tokio::process::Command::new("python3")
            .arg(&self.agent)
            .env("SANDBOX_AGENT_WORK", id)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        drive_agent(child, id, req, timeout, self).await
    }

    async fn destroy(&self, id: &str) -> Result<(), BackendError> {
        self.validate_container_id(id)?;
        match tokio::fs::remove_dir_all(id).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

#[async_trait::async_trait]
impl SpillReader for LocalBackend {
    /// Runs on the host (no gVisor cap), so read the whole spill file directly
    /// and then remove it — unlike the container case, nothing else reaps it.
    async fn read_spill(
        &self,
        _container: &str,
        path: &str,
        len: u64,
    ) -> Result<Vec<u8>, BackendError> {
        let bytes = tokio::fs::read(path).await?;
        let _ = tokio::fs::remove_file(path).await;
        if bytes.len() as u64 != len {
            return Err(BackendError::Protocol(format!(
                "spilled response size mismatch: file has {} bytes, header said {len}",
                bytes.len()
            )));
        }
        Ok(bytes)
    }
}

#[cfg(test)]
pub(crate) mod fake {
    //! In-memory backend for the pool unit tests. Records create/destroy
    //! so tests can assert warm-pool refill and single-use teardown.

    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use shared::sandbox::Language;

    #[derive(Default)]
    pub struct FakeBackend {
        next: AtomicUsize,
        pub created: Mutex<Vec<(String, Network)>>,
        pub destroyed: Mutex<Vec<String>>,
        pub execs: AtomicUsize,
        pub fail_destroy: std::sync::atomic::AtomicBool,
        pub exec_entered: tokio::sync::Notify,
        pub exec_release: tokio::sync::Notify,
        pub browser_controls: Mutex<Vec<shared::sandbox::BrowserControlOperation>>,
        pub fail_browser_begin_once: std::sync::atomic::AtomicBool,
        pub hold_create: std::sync::atomic::AtomicBool,
        pub create_entered: tokio::sync::Notify,
        pub create_release: tokio::sync::Notify,
        /// Simulated workload-image id; `set_image` mutates it so pool tests
        /// can exercise the auto-recycle path.
        image: Mutex<String>,
    }

    impl FakeBackend {
        pub fn new() -> Self {
            let b = Self::default();
            *b.image.lock().unwrap() = "img-v1".to_string();
            b
        }
        pub fn live_count(&self) -> usize {
            self.created.lock().unwrap().len() - self.destroyed.lock().unwrap().len()
        }
        /// Swap the reported image id, simulating a rebuild / re-tag.
        pub fn set_image(&self, id: &str) {
            *self.image.lock().unwrap() = id.to_string();
        }
    }

    #[async_trait::async_trait]
    impl ContainerBackend for FakeBackend {
        fn validate_container_id(&self, id: &str) -> Result<(), BackendError> {
            if validate_podman_id(id).is_ok()
                || id.strip_prefix("fake-").is_some_and(|suffix| {
                    !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
                })
            {
                Ok(())
            } else {
                Err(BackendError::Protocol(
                    "invalid fake container reference".into(),
                ))
            }
        }
        fn workspace_available(&self) -> bool {
            true
        }
        async fn create(
            &self,
            network: Network,
            workspace: Option<&WorkspaceMount>,
        ) -> Result<String, BackendError> {
            let n = self.next.fetch_add(1, Ordering::SeqCst);
            let id = workspace
                .map(|mount| mount.name.clone())
                .unwrap_or_else(|| format!("fake-{n}"));
            self.created.lock().unwrap().push((id.clone(), network));
            if self.hold_create.load(Ordering::SeqCst) {
                self.create_entered.notify_one();
                self.create_release.notified().await;
            }
            Ok(id)
        }

        async fn image_id(&self) -> Result<String, BackendError> {
            Ok(self.image.lock().unwrap().clone())
        }

        async fn exec(
            &self,
            id: &str,
            req: &RunRequest,
            _timeout: Duration,
        ) -> Result<RunResponse, BackendError> {
            self.execs.fetch_add(1, Ordering::SeqCst);
            if req.code == "hold-exec" {
                self.exec_entered.notify_one();
                self.exec_release.notified().await;
            }
            Ok(RunResponse {
                exit_code: 0,
                stdout: format!("ran {} in {id}", req.language.as_str()),
                stderr: String::new(),
                artifacts: Vec::new(),
                duration_ms: 1,
                timed_out: false,
                output_truncated: false,
                artifacts_truncated: false,
                container_id: None,
            })
        }

        async fn browser_control(
            &self,
            _id: &str,
            operation: &shared::sandbox::BrowserControlOperation,
        ) -> Result<shared::sandbox::BrowserControlResponse, BackendError> {
            self.browser_controls
                .lock()
                .unwrap()
                .push(operation.clone());
            if matches!(operation, shared::sandbox::BrowserControlOperation::Begin)
                && self.fail_browser_begin_once.swap(false, Ordering::SeqCst)
            {
                return Err(BackendError::Protocol("browser control unavailable".into()));
            }
            Ok(shared::sandbox::BrowserControlResponse {
                ok: true,
                generation: 1,
                expires_at: "2026-10-12T10:00:00Z".into(),
                frame_b64: None,
                width: None,
                height: None,
            })
        }

        async fn destroy(&self, id: &str) -> Result<(), BackendError> {
            if self.fail_destroy.load(Ordering::SeqCst) {
                return Err(BackendError::Protocol("teardown unavailable".into()));
            }
            let mut destroyed = self.destroyed.lock().unwrap();
            if !destroyed.iter().any(|previous| previous == id) {
                destroyed.push(id.to_string());
            }
            Ok(())
        }
    }

    pub fn req() -> RunRequest {
        RunRequest {
            language: Language::Python,
            code: "print(1)".into(),
            files: Vec::new(),
            timeout_secs: None,
            network: false,
            container_id: None,
            keep_alive: false,
            workspace: None,
        }
    }
}

#[cfg(test)]
mod local_tests {
    //! End-to-end test of the dev `LocalBackend` against a real `python3`.
    //! Exercises the full agent contract (file inputs, stdout, artifact
    //! collection) on any host with python3 — notably macOS, where
    //! podman / gVisor aren't available.

    use super::*;
    use shared::sandbox::{InputFile, Language, RunRequest};

    fn python3_available() -> bool {
        std::process::Command::new("python3")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    #[tokio::test]
    async fn runs_python_collects_artifact_and_reads_input() {
        if !python3_available() {
            eprintln!("skipping local_backend test: python3 not on PATH");
            return;
        }
        let be = LocalBackend::new().unwrap();
        let id = be.create(Network::None, None).await.unwrap();
        let req = RunRequest {
            language: Language::Python,
            // Reads an input file, writes an output artifact, prints to stdout.
            code: "print('in=' + open('data.txt').read()); open('out.txt','w').write('hi')".into(),
            files: vec![InputFile {
                name: "data.txt".into(),
                content_b64: "Zm9v".into(), // "foo"
            }],
            timeout_secs: None,
            network: false,
            container_id: None,
            keep_alive: false,
            workspace: None,
        };
        let resp = be.exec(&id, &req, Duration::from_secs(30)).await.unwrap();
        be.destroy(&id).await.unwrap();

        assert_eq!(resp.exit_code, 0, "stderr={}", resp.stderr);
        assert!(resp.stdout.contains("in=foo"), "stdout={}", resp.stdout);
        let art = resp
            .artifacts
            .iter()
            .find(|a| a.name == "out.txt")
            .expect("out.txt artifact");
        assert_eq!(art.content_b64, "aGk="); // "hi"
        // The input file must NOT be reported as a produced artifact.
        assert!(!resp.artifacts.iter().any(|a| a.name == "data.txt"));
    }

    #[tokio::test]
    async fn large_stdout_is_preserved_as_an_attachment() {
        if !python3_available() {
            eprintln!("skipping local_backend test: python3 not on PATH");
            return;
        }
        let be = LocalBackend::new().unwrap();
        let id = be.create(Network::None, None).await.unwrap();
        // Print ~300 KB (> the agent's 32 KiB preserve threshold).
        let req = RunRequest {
            language: Language::Python,
            code: "print('X' * 300000)".into(),
            files: vec![],
            timeout_secs: None,
            network: false,
            container_id: None,
            keep_alive: false,
            workspace: None,
        };
        let resp = be.exec(&id, &req, Duration::from_secs(30)).await.unwrap();
        be.destroy(&id).await.unwrap();
        let art = resp.artifacts.iter().find(|a| a.name == "stdout.txt");
        assert!(
            art.is_some(),
            "no stdout.txt; exit={} stdout_len={} timed_out={} artifacts={:?} stderr={:.200}",
            resp.exit_code,
            resp.stdout.len(),
            resp.timed_out,
            resp.artifacts
                .iter()
                .map(|a| (a.name.clone(), a.size))
                .collect::<Vec<_>>(),
            resp.stderr,
        );
        assert!(art.unwrap().size >= 300_000, "preserved full stream");
    }

    /// The regression this whole change exists for.
    ///
    /// Asked for a documentation set, a model writes `docs/backend.md` — the
    /// tool told it to write output files to the working directory, and a
    /// subdirectory is in the working directory. Collection used to be a
    /// top-level `os.listdir`, so the run came back `exit_code: 0` with an
    /// EMPTY artifact list: the one result shape a model cannot tell apart
    /// from its own mistake. In the conversation that prompted this fix it
    /// then spent six turns insisting the files existed and the interface was
    /// broken, forging attachment stubs to prove it.
    #[tokio::test]
    async fn collects_files_produced_in_subdirectories() {
        if !python3_available() {
            eprintln!("skipping local_backend test: python3 not on PATH");
            return;
        }
        let be = LocalBackend::new().unwrap();
        let id = be.create(Network::None, None).await.unwrap();
        let req = RunRequest {
            language: Language::Python,
            code: "import os\n\
                   os.makedirs('docs/deep', exist_ok=True)\n\
                   open('CLAUDE.md','w').write('root')\n\
                   open('docs/backend.md','w').write('backend')\n\
                   open('docs/deep/api.md','w').write('api')\n"
                .into(),
            files: vec![],
            timeout_secs: None,
            network: false,
            container_id: None,
            keep_alive: false,
            workspace: None,
        };
        let resp = be.exec(&id, &req, Duration::from_secs(30)).await.unwrap();
        be.destroy(&id).await.unwrap();
        assert_eq!(resp.exit_code, 0, "stderr={}", resp.stderr);

        let by_name = |n: &str| resp.artifacts.iter().find(|a| a.name == n).cloned();
        // A top-level file keeps its exact name — introducing recursion must
        // not rename anything that already worked.
        let root = by_name("CLAUDE.md").expect("top-level file still collected");
        assert_eq!(root.path, None, "a top-level file reports no sandbox path");

        // Nested files arrive under a flattened name, and say where they came
        // from so the model can match them to the paths it wrote.
        let nested = by_name("docs-backend.md").expect("docs/backend.md collected");
        assert_eq!(nested.path.as_deref(), Some("docs/backend.md"));
        assert_eq!(nested.content_b64, shared::b64::encode(b"backend"));
        let deep = by_name("docs-deep-api.md").expect("docs/deep/api.md collected");
        assert_eq!(deep.path.as_deref(), Some("docs/deep/api.md"));
        assert_eq!(deep.content_b64, shared::b64::encode(b"api"));
    }

    /// A flattened name must never silently overwrite a real top-level file:
    /// `docs/backend.md` and a literal `docs-backend.md` both want the same
    /// delivered name, and the top-level one owns it.
    #[tokio::test]
    async fn flattened_names_yield_to_a_real_top_level_file() {
        if !python3_available() {
            eprintln!("skipping local_backend test: python3 not on PATH");
            return;
        }
        let be = LocalBackend::new().unwrap();
        let id = be.create(Network::None, None).await.unwrap();
        let req = RunRequest {
            language: Language::Python,
            code: "import os\n\
                   os.makedirs('docs', exist_ok=True)\n\
                   open('docs-backend.md','w').write('flat')\n\
                   open('docs/backend.md','w').write('nested')\n"
                .into(),
            files: vec![],
            timeout_secs: None,
            network: false,
            container_id: None,
            keep_alive: false,
            workspace: None,
        };
        let resp = be.exec(&id, &req, Duration::from_secs(30)).await.unwrap();
        be.destroy(&id).await.unwrap();
        assert_eq!(resp.exit_code, 0, "stderr={}", resp.stderr);

        let flat = resp
            .artifacts
            .iter()
            .find(|a| a.name == "docs-backend.md")
            .expect("the real top-level file keeps the name");
        assert_eq!(flat.path, None);
        assert_eq!(flat.content_b64, shared::b64::encode(b"flat"));
        let nested = resp
            .artifacts
            .iter()
            .find(|a| a.path.as_deref() == Some("docs/backend.md"))
            .expect("the nested file is still delivered");
        assert_eq!(nested.name, "docs-backend-2.md");
        assert_eq!(nested.content_b64, shared::b64::encode(b"nested"));
    }

    /// `HOME` is the working directory, so anything that wants a profile or
    /// cache dir drops one next to the job's real output. Recursion must not
    /// turn those into attachments — nor descend into a vendored tree.
    #[tokio::test]
    async fn cache_and_vendor_directories_are_not_collected() {
        if !python3_available() {
            eprintln!("skipping local_backend test: python3 not on PATH");
            return;
        }
        let be = LocalBackend::new().unwrap();
        let id = be.create(Network::None, None).await.unwrap();
        let req = RunRequest {
            language: Language::Python,
            code: "import os\n\
                   for d in ('.cache/fontconfig', '.config', 'node_modules/left-pad', \
                   '__pycache__'):\n\
                   \x20   os.makedirs(d, exist_ok=True)\n\
                   \x20   open(os.path.join(d, 'junk.txt'), 'w').write('noise')\n\
                   open('report.md','w').write('the real output')\n"
                .into(),
            files: vec![],
            timeout_secs: None,
            network: false,
            container_id: None,
            keep_alive: false,
            workspace: None,
        };
        let resp = be.exec(&id, &req, Duration::from_secs(30)).await.unwrap();
        be.destroy(&id).await.unwrap();
        assert_eq!(resp.exit_code, 0, "stderr={}", resp.stderr);
        let names: Vec<&str> = resp.artifacts.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["report.md"],
            "only the job's own output should be delivered"
        );
    }

    /// Untrusted code must not be able to hand the user a file from outside
    /// its own output by planting a symlink under a plausible name. Held at
    /// the agent, so it also holds for the base64 path where the runner's
    /// `O_NOFOLLOW` read never runs.
    #[tokio::test]
    async fn symlinked_output_is_never_collected() {
        if !python3_available() {
            eprintln!("skipping local_backend test: python3 not on PATH");
            return;
        }
        let be = LocalBackend::new().unwrap();
        let id = be.create(Network::None, None).await.unwrap();
        let req = RunRequest {
            language: Language::Python,
            code: "import os\n\
                   os.makedirs('out', exist_ok=True)\n\
                   os.symlink('/etc/hosts', 'out/hosts.txt')\n\
                   os.symlink('/etc', 'escape')\n\
                   open('out/real.txt','w').write('mine')\n"
                .into(),
            files: vec![],
            timeout_secs: None,
            network: false,
            container_id: None,
            keep_alive: false,
            workspace: None,
        };
        let resp = be.exec(&id, &req, Duration::from_secs(30)).await.unwrap();
        be.destroy(&id).await.unwrap();
        assert_eq!(resp.exit_code, 0, "stderr={}", resp.stderr);
        let names: Vec<&str> = resp.artifacts.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["out-real.txt"],
            "symlinks (file and directory) must be skipped, not followed: {names:?}"
        );
    }

    #[tokio::test]
    async fn reused_workdir_collects_only_this_calls_output() {
        // A kept-alive container is exec'd into more than once in a turn. The
        // agent must return only the files THIS call produced — not re-collect
        // earlier calls' outputs (which would flood the user with duplicate
        // attachments). Same container id = same /work across both execs.
        if !python3_available() {
            eprintln!("skipping local_backend test: python3 not on PATH");
            return;
        }
        let be = LocalBackend::new().unwrap();
        let id = be.create(Network::None, None).await.unwrap();
        let mk = |code: &str| RunRequest {
            language: Language::Python,
            code: code.into(),
            files: vec![],
            timeout_secs: None,
            network: false,
            container_id: None,
            keep_alive: false,
            workspace: None,
        };

        // Call 1 writes a.txt.
        let r1 = be
            .exec(
                &id,
                &mk("open('a.txt','w').write('one')"),
                Duration::from_secs(30),
            )
            .await
            .unwrap();
        assert_eq!(r1.exit_code, 0, "stderr={}", r1.stderr);
        assert!(r1.artifacts.iter().any(|a| a.name == "a.txt"));

        // Call 2 (same container) writes b.txt while a.txt still sits in /work.
        let r2 = be
            .exec(
                &id,
                &mk("open('b.txt','w').write('two')"),
                Duration::from_secs(30),
            )
            .await
            .unwrap();
        be.destroy(&id).await.unwrap();
        assert_eq!(r2.exit_code, 0, "stderr={}", r2.stderr);
        let names: Vec<&str> = r2.artifacts.iter().map(|a| a.name.as_str()).collect();
        assert!(names.contains(&"b.txt"), "new file collected: {names:?}");
        assert!(
            !names.contains(&"a.txt"),
            "the untouched prior-call output must NOT be re-collected: {names:?}"
        );
    }

    #[tokio::test]
    async fn large_artifact_round_trips_via_spill() {
        if !python3_available() {
            eprintln!("skipping local_backend test: python3 not on PATH");
            return;
        }
        let be = LocalBackend::new().unwrap();
        let id = be.create(Network::None, None).await.unwrap();
        // A ~500 KB artifact makes the RunResponse far exceed the agent's
        // 60 KB inline cap, so it is spilled to a file and read back via
        // `SpillReader` rather than arriving inline on stdout — exercising the
        // whole spill path (the gVisor exec-stdout truncation it works around
        // can only be hit with a real runsc container, verified separately).
        let req = RunRequest {
            language: Language::Python,
            code: "open('big.bin','wb').write(b'Z'*500000)".into(),
            files: vec![],
            timeout_secs: None,
            network: false,
            container_id: None,
            keep_alive: false,
            workspace: None,
        };
        let resp = be.exec(&id, &req, Duration::from_secs(30)).await.unwrap();
        be.destroy(&id).await.unwrap();

        assert_eq!(resp.exit_code, 0, "stderr={}", resp.stderr);
        let art = resp
            .artifacts
            .iter()
            .find(|a| a.name == "big.bin")
            .expect("big.bin artifact");
        assert_eq!(art.size, 500_000);
        // Standard base64 of 500_000 bytes is ceil(500000/3)*4 chars; an exact
        // match proves the payload survived the spill+reassembly uncorrupted.
        assert_eq!(art.content_b64.len(), 500_000usize.div_ceil(3) * 4);
    }
}

#[cfg(test)]
mod scratch_tests {
    //! Host-backed scratch (`Config::work_root`): path handling and the
    //! symlink defence. Everything in a scratch directory was written by
    //! untrusted code, so these are the tests that matter most in this file.

    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "sbx-scratch-test-{}-{tag}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        base
    }

    #[test]
    fn reads_a_regular_file() {
        let d = tmpdir("plain");
        std::fs::write(d.join("out.mp4"), b"video-bytes").unwrap();
        assert_eq!(read_sandbox_file(&d, "out.mp4").unwrap(), b"video-bytes");
    }

    #[test]
    fn refuses_a_symlink_planted_by_the_sandbox() {
        // The attack this design has to survive: untrusted code puts a symlink
        // where the runner expects its artifact, aiming to have a host file
        // read out and handed to the model.
        let d = tmpdir("symlink");
        let secret = d.join("secret.txt");
        std::fs::write(&secret, b"host-only").unwrap();
        std::os::unix::fs::symlink(&secret, d.join("out.mp4")).unwrap();

        let err = read_sandbox_file(&d, "out.mp4").expect_err("symlink must be refused");
        assert!(
            !format!("{err}").contains("host-only"),
            "error must not leak content"
        );
        // And nothing was read: the file behind the link stays untouched.
        assert_eq!(std::fs::read(&secret).unwrap(), b"host-only");
    }

    #[test]
    fn refuses_path_traversal_and_separators() {
        let d = tmpdir("traversal");
        std::fs::create_dir_all(d.join("sub")).unwrap();
        std::fs::write(d.join("sub").join("f"), b"x").unwrap();
        for name in ["../etc/passwd", "sub/f", "/etc/passwd", "", "."] {
            assert!(
                read_sandbox_file(&d, name).is_err(),
                "{name:?} must be refused"
            );
        }
    }

    #[test]
    fn refuses_a_directory() {
        let d = tmpdir("dir");
        std::fs::create_dir_all(d.join("adir")).unwrap();
        assert!(read_sandbox_file(&d, "adir").is_err());
    }

    #[test]
    fn maps_container_paths_to_their_mount() {
        assert_eq!(
            split_mount_path("/work/out.mp4"),
            Some((Mount::Work, "out.mp4"))
        );
        assert_eq!(
            split_mount_path("/tmp/tmpab12"),
            Some((Mount::Tmp, "tmpab12"))
        );
        // Not in a scratch mount → caller must fall back to exec reads.
        assert_eq!(split_mount_path("/etc/passwd"), None);
        assert_eq!(split_mount_path("/workspace/x"), None);
    }

    #[test]
    fn host_dirs_are_per_container() {
        let (w1, t1) = host_dirs(Path::new("/srv/scratch"), "sbx-a");
        let (w2, _) = host_dirs(Path::new("/srv/scratch"), "sbx-b");
        assert_eq!(w1, PathBuf::from("/srv/scratch/sbx-a/work"));
        assert_eq!(t1, PathBuf::from("/srv/scratch/sbx-a/tmp"));
        assert_ne!(w1, w2, "two containers must not share scratch");
    }

    #[test]
    fn container_names_are_unique() {
        let a = new_container_name();
        let b = new_container_name();
        assert_ne!(a, b);
        assert!(a.starts_with("sbx-"));
        // Single path component, so it can never widen the scratch root.
        assert_eq!(Path::new(&a).components().count(), 1);
    }

    #[test]
    fn prepare_host_dirs_locks_down_the_root_and_opens_the_leaves() {
        use std::os::unix::fs::PermissionsExt;
        let root = tmpdir("modes");
        let (work, tmp) = host_dirs(&root, "sbx-x");
        prepare_host_dirs(&root, &work, &tmp).unwrap();
        // Root walkable by the runner only …
        let rmode = std::fs::metadata(&root).unwrap().permissions().mode() & 0o777;
        assert_eq!(rmode, 0o700, "scratch root must not be world-traversable");
        // … while the container's own dirs stay writable for whatever uid the
        // image runs as (mirrors the tmpfs `mode=1777` this replaces).
        for d in [&work, &tmp] {
            let m = std::fs::metadata(d).unwrap().permissions().mode() & 0o7777;
            assert_eq!(m, 0o1777, "{} should be sticky+world-writable", d.display());
        }
    }
}

#[cfg(test)]
mod scratch_mount_tests {
    //! `run_args` has to switch between the two scratch strategies: internal
    //! tmpfs (default) and host bind mounts (`work_root` set).

    use super::*;

    fn cfg_with_root(root: Option<&str>) -> Arc<Config> {
        let mut c = crate::config::Config::for_test();
        c.work_root = root.map(PathBuf::from);
        Arc::new(c)
    }

    #[test]
    fn defaults_to_internal_tmpfs() {
        let a = PodmanBackend::new(cfg_with_root(None)).run_args(Network::None, "sbx-1", None);
        let joined = a.join(" ");
        assert!(
            joined.contains("--tmpfs /work:rw,size=512m,mode=1777"),
            "{joined}"
        );
        assert!(
            !joined.contains("-v "),
            "no bind mounts without work_root: {joined}"
        );
        assert!(joined.contains("--name sbx-1"));
    }

    #[test]
    fn bind_mounts_scratch_when_work_root_is_set() {
        let a = PodmanBackend::new(cfg_with_root(Some("/srv/scratch"))).run_args(
            Network::None,
            "sbx-2",
            None,
        );
        let joined = a.join(" ");
        // Per-container paths, derived from the name, so two sandboxes can
        // never see each other's files.
        assert!(
            joined.contains("/srv/scratch/sbx-2/work:/work:rw"),
            "{joined}"
        );
        // `/tmp` keeps exec: chromium and LibreOffice drop helper binaries there.
        assert!(
            joined.contains("/srv/scratch/sbx-2/tmp:/tmp:rw,exec"),
            "{joined}"
        );
        assert!(
            !joined.contains("--tmpfs"),
            "tmpfs must be replaced: {joined}"
        );
    }

    #[test]
    fn mounts_only_the_workspace_files_directory_read_write() {
        let a = PodmanBackend::new(cfg_with_root(Some("/srv/scratch")))
            .run_args_with_workspace_path(
                Network::None,
                "sbx-3",
                Some(Path::new("/srv/dots/abc/files")),
            );
        let joined = a.join(" ");
        assert!(
            joined.contains("-v /srv/dots/abc/files:/workspace:rw"),
            "{joined}"
        );
        // Only `files` is mounted; private owner metadata and retired roots never are.
        assert!(!joined.contains("owner.json"), "{joined}");
        assert!(!joined.contains(".dot-retired"), "{joined}");
    }

    #[test]
    fn omits_the_workspace_mount_without_a_scope() {
        let a = PodmanBackend::new(cfg_with_root(Some("/srv/scratch")))
            .run_args_with_workspace_path(Network::None, "sbx-4", None);
        let joined = a.join(" ");
        assert!(
            !joined.contains("/workspace"),
            "no workspace mount without a scope: {joined}"
        );
    }
}

#[cfg(test)]
mod inplace_artifact_tests {
    //! With host-backed scratch the agent hands back artifact metadata only and
    //! the runner reads the bytes off the bind mount. That is what removes the
    //! agent's 64 MiB cap, so it has to be exact about which artifacts survive.

    use super::*;
    use shared::sandbox::Artifact;

    fn scratch(tag: &str) -> (Arc<Config>, PathBuf, String) {
        let root = std::env::temp_dir().join(format!(
            "sbx-inplace-{}-{tag}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let name = "sbx-test".to_string();
        let (work, tmp) = host_dirs(&root, &name);
        prepare_host_dirs(&root, &work, &tmp).unwrap();
        let mut c = crate::config::Config::for_test();
        c.work_root = Some(root.clone());
        (Arc::new(c), work, name)
    }

    fn resp_with(artifacts: Vec<Artifact>) -> RunResponse {
        RunResponse {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
            artifacts,
            duration_ms: 1,
            timed_out: false,
            output_truncated: false,
            artifacts_truncated: false,
            container_id: None,
        }
    }

    fn art(name: &str, size: u64, content: &str) -> Artifact {
        Artifact {
            name: name.into(),
            path: None,
            size,
            mime: "application/octet-stream".into(),
            content_b64: content.into(),
        }
    }

    #[test]
    fn reads_the_bytes_the_agent_left_on_disk() {
        let (cfg, work, name) = scratch("read");
        std::fs::write(work.join("out.mp4"), b"foobar").unwrap();
        let mut r = resp_with(vec![art("out.mp4", 6, "")]);

        PodmanBackend::new(cfg).hydrate_inplace_artifacts(&name, &mut r);

        assert_eq!(r.artifacts.len(), 1);
        assert_eq!(r.artifacts[0].content_b64, "Zm9vYmFy");
    }

    #[test]
    fn leaves_inline_artifacts_untouched() {
        // The agent still inlines when scratch isn't host-backed, and a mixed
        // response must not be re-read (or clobbered) here.
        let (cfg, _work, name) = scratch("inline");
        let mut r = resp_with(vec![art("small.txt", 3, "Zm9v")]);

        PodmanBackend::new(cfg).hydrate_inplace_artifacts(&name, &mut r);

        assert_eq!(r.artifacts[0].content_b64, "Zm9v");
    }

    #[test]
    fn drops_an_artifact_that_is_a_symlink() {
        // Untrusted code naming a symlink as its output must not get the target
        // delivered to the user — and must not yield an empty file either.
        let (cfg, work, name) = scratch("symlink");
        let secret = work.join("secret");
        std::fs::write(&secret, b"host-only").unwrap();
        std::os::unix::fs::symlink(&secret, work.join("out.mp4")).unwrap();
        let mut r = resp_with(vec![art("out.mp4", 9, "")]);

        PodmanBackend::new(cfg).hydrate_inplace_artifacts(&name, &mut r);

        assert!(r.artifacts.is_empty(), "symlinked artifact must be dropped");
    }

    #[test]
    fn drops_an_artifact_whose_file_vanished() {
        let (cfg, _work, name) = scratch("missing");
        let mut r = resp_with(vec![art("gone.bin", 10, "")]);

        PodmanBackend::new(cfg).hydrate_inplace_artifacts(&name, &mut r);

        assert!(r.artifacts.is_empty());
    }

    #[test]
    fn is_a_no_op_without_host_backed_scratch() {
        // Plain tmpfs deployments keep the inline protocol untouched.
        let cfg = Arc::new(crate::config::Config::for_test()); // work_root: None
        let mut r = resp_with(vec![art("out.mp4", 6, "")]);

        PodmanBackend::new(cfg).hydrate_inplace_artifacts("sbx-x", &mut r);

        assert_eq!(r.artifacts.len(), 1, "must not touch the response");
        assert_eq!(r.artifacts[0].content_b64, "");
    }
}

#[cfg(test)]
mod workspace_readiness_tests {
    use super::*;

    fn proof() -> GuestProof {
        GuestProof {
            uid: 1001,
            gid: 1001,
            kernel: "gvisor-kernel".into(),
            directory_uid: 1001,
            directory_gid: 1001,
            directory_mode: 0o700,
            file_uid: 1001,
            file_gid: 1001,
            file_mode: 0o600,
            nonce: "ab".repeat(32),
        }
    }

    #[test]
    fn workspace_readiness_guest_proof_requires_exact_identity_modes_nonce_and_isolation() {
        let nonce = "ab".repeat(32);
        validate_guest_proof(&proof(), "host-kernel", &nonce).unwrap();
        for field in 0..10 {
            let mut value = serde_json::to_value(proof()).unwrap();
            let (key, replacement) = match field {
                0 => ("uid", serde_json::json!(0)),
                1 => ("gid", serde_json::json!(0)),
                2 => ("kernel", serde_json::json!("host-kernel")),
                3 => ("directory_uid", serde_json::json!(0)),
                4 => ("directory_gid", serde_json::json!(0)),
                5 => ("directory_mode", serde_json::json!(0o777)),
                6 => ("file_uid", serde_json::json!(0)),
                7 => ("file_gid", serde_json::json!(0)),
                8 => ("file_mode", serde_json::json!(0o644)),
                _ => ("nonce", serde_json::json!("cd".repeat(32))),
            };
            value[key] = replacement;
            let changed: GuestProof = serde_json::from_value(value).unwrap();
            assert!(
                validate_guest_proof(&changed, "host-kernel", &nonce).is_err(),
                "{key}"
            );
        }
        let mut empty = proof();
        empty.kernel.clear();
        assert!(validate_guest_proof(&empty, "host-kernel", &nonce).is_err());
        assert!(validate_guest_proof(&proof(), "", &nonce).is_err());
        let mut malformed = serde_json::to_value(proof()).unwrap();
        malformed["extra"] = serde_json::json!(true);
        assert!(serde_json::from_value::<GuestProof>(malformed).is_err());
    }

    #[tokio::test]
    async fn workspace_readiness_missing_storage_never_advertises_or_invokes_container_daemon() {
        let mut config = Config::for_test();
        config.work_root = None;
        config.podman = "/nonexistent-workspace-proof-podman".into();
        let backend = PodmanBackend::new(Arc::new(config));
        assert!(backend.verify_workspace_readiness().await.is_err());
        assert!(!backend.workspace_available());
    }

    #[tokio::test]
    async fn workspace_readiness_unproven_host_remains_unavailable_after_failure() {
        let mut config = Config::for_test();
        config.work_root = Some(std::env::temp_dir());
        config.podman = "/nonexistent-workspace-proof-podman".into();
        let backend = PodmanBackend::new(Arc::new(config));
        assert!(backend.verify_workspace_readiness().await.is_err());
        assert!(!backend.workspace_available());
    }
}

#[cfg(test)]
mod browser_control_response_tests {
    use super::*;

    #[test]
    fn failed_browser_cleanup_cannot_be_reported_as_a_completed_handover() {
        let response = br#"{"ok":false,"generation":1,"expires_at":"2026-10-12T10:00:00Z"}"#;
        assert!(matches!(
            decode_browser_control_response(response),
            Err(BackendError::Protocol(_))
        ));
    }
}
