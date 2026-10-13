// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use std::collections::BTreeMap;
use std::ffi::{CStr, CString};
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use shared::sandbox::{
    DotWorkspace, STAGE_TOTAL_MAX_BYTES, WorkspaceEntry, WorkspaceEntryKind, WorkspaceListing,
};
use thiserror::Error;

const MAX_PATH_BYTES: usize = 4096;
const MAX_COMPONENTS: usize = 32;
const MAX_MANIFEST_BYTES: usize = 16 * 1024;
const PAGE_SIZE: usize = 100;
const SCAN_LIMIT: usize = 10_000;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("workspace path is invalid: {0}; use a relative UTF-8 path within the workspace")]
    InvalidPath(&'static str),
    #[error(
        "workspace identity does not match its private owner manifest; check the Dot owner and workspace storage"
    )]
    OwnerMismatch,
    #[error("workspace is permanently retired; use a different active Dot")]
    Retired,
    #[error(
        "retired workspace exceeds the cleanup depth limit; ask the operator to inspect the protected storage"
    )]
    CleanupDepth,
    #[error(
        "workspace contains an unsafe filesystem entry; remove symlinks or special files through the operator"
    )]
    UnsafeEntry,
    #[error("workspace file exceeds the staging byte budget; use a smaller file")]
    FileTooLarge,
    #[error(
        "workspace workload ownership requires a rootful Linux runner; configure a supported sandbox host"
    )]
    UnsupportedOwnership,
    #[error(
        "workspace directory exceeds the listing scan budget; split its entries into smaller directories"
    )]
    DirectoryTooLarge,
    #[error("workspace filesystem operation failed: {0}; check storage access and availability")]
    Io(#[from] std::io::Error),
    #[error(
        "workspace owner manifest is invalid: {0}; check workspace storage without replacing its ownership"
    )]
    Manifest(#[from] serde_json::Error),
}

pub struct Workspace {
    files: File,
    browser_profile: File,
    work_root: PathBuf,
    files_path: PathBuf,
    browser_profile_path: PathBuf,
    identity: DotWorkspace,
}

impl Workspace {
    pub fn open(work_root: &Path, identity: &DotWorkspace) -> Result<Self, WorkspaceError> {
        validate_identity(identity)?;
        let work_root = if work_root.is_absolute() {
            work_root.to_path_buf()
        } else {
            std::env::current_dir()?.join(work_root)
        };
        let root = open_root(&work_root)?;
        require_private(&root)?;
        check_not_retired(&root, identity)?;
        let dots = directory(&root, "dots", true)?;
        require_private(&dots)?;
        let dot = directory(&dots, &identity.dot_id, true)?;
        require_private(&dot)?;
        match open_regular(&dot, "owner.json") {
            Ok(manifest) => {
                drop(manifest);
                check_manifest(&dot, identity)?;
            }
            Err(WorkspaceError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                if unclaimed_nonempty(&dot)? {
                    return Err(WorkspaceError::OwnerMismatch);
                }
                publish_manifest(&dot, identity)?;
            }
            Err(error) => return Err(error),
        }
        let files_path = work_root.join("dots").join(&identity.dot_id).join("files");
        let browser_profile_path = work_root
            .join("dots")
            .join(&identity.dot_id)
            .join("browser-profile");
        Ok(Self {
            files: directory(&dot, "files", true)?,
            browser_profile: directory(&dot, "browser-profile", true)?,
            work_root,
            files_path,
            browser_profile_path,
            identity: identity.clone(),
        })
    }

    pub fn seal_retirement(
        work_root: &Path,
        identity: &DotWorkspace,
    ) -> Result<(), WorkspaceError> {
        validate_identity(identity)?;
        let root = open_root(work_root)?;
        require_private(&root)?;
        if let Some(dots) = optional_directory(&root, "dots")? {
            require_private(&dots)?;
            if let Some(dot) = optional_directory(&dots, &identity.dot_id)? {
                require_private(&dot)?;
                check_manifest(&dot, identity)?;
            }
        }
        let retired = directory(&root, ".dot-retired", true)?;
        require_private(&retired)?;
        let dot = directory(&retired, &identity.dot_id, true)?;
        require_private(&dot)?;
        match open_regular(&dot, "owner.json") {
            Ok(manifest) => {
                drop(manifest);
                check_manifest(&dot, identity)?;
            }
            Err(WorkspaceError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                if unclaimed_nonempty(&dot)? {
                    return Err(WorkspaceError::OwnerMismatch);
                }
                publish_manifest(&dot, identity)?;
            }
            Err(error) => return Err(error),
        }
        dot.sync_all()?;
        retired.sync_all()?;
        root.sync_all()?;
        Ok(())
    }

    pub fn cleanup_retired(
        work_root: &Path,
        identity: &DotWorkspace,
    ) -> Result<bool, WorkspaceError> {
        validate_identity(identity)?;
        let root = open_root(work_root)?;
        require_private(&root)?;
        let retired = directory(&root, ".dot-retired", false)?;
        require_private(&retired)?;
        let dot = directory(&retired, &identity.dot_id, false)?;
        require_private(&dot)?;
        check_manifest(&dot, identity)?;
        let dots = optional_directory(&root, "dots")?;
        let original = if let Some(dots) = &dots {
            require_private(dots)?;
            optional_directory(dots, &identity.dot_id)?
        } else {
            None
        };
        let payload = optional_directory(&dot, "payload")?;
        if let Some(original) = &original {
            require_private(original)?;
            check_manifest(original, identity)?;
            if payload.is_some() {
                return Err(WorkspaceError::UnsafeEntry);
            }
            let dots = dots.as_ref().ok_or(WorkspaceError::UnsafeEntry)?;
            let original_name = cstring(&identity.dot_id)?;
            let linked = directory(dots, &identity.dot_id, false)?;
            same_directory(&linked, original)?;
            if unsafe {
                libc::renameat(
                    dots.as_raw_fd(),
                    original_name.as_ptr(),
                    dot.as_raw_fd(),
                    c"payload".as_ptr(),
                )
            } != 0
            {
                return Err(std::io::Error::last_os_error().into());
            }
            let moved = directory(&dot, "payload", false)?;
            same_directory(&moved, original)?;
            dots.sync_all()?;
            dot.sync_all()?;
        } else if let Some(payload) = &payload {
            require_private(payload)?;
            // The retained retirement owner authorizes partially removed payloads after a crash.
        } else {
            return Ok(true);
        }
        let mut remaining = 1000;
        let complete = remove_retired_node(&dot, c"payload", 0, &mut remaining)?;
        dot.sync_all()?;
        Ok(complete)
    }

    pub fn mount_path(&self) -> Result<PathBuf, WorkspaceError> {
        self.validate_mount_path(&self.files_path)?;
        Ok(self.files_path.clone())
    }

    pub fn validate_mount_path(&self, path: &Path) -> Result<(), WorkspaceError> {
        self.validate_path(path)
    }

    pub fn prepare_workload_owner(&self, uid: u32, gid: u32) -> Result<(), WorkspaceError> {
        self.validate_path(&self.files_path)?;
        self.validate_browser_profile_path(&self.browser_profile_path)?;
        prepare_owner(&self.files, uid, gid)?;
        prepare_owner(&self.browser_profile, uid, gid)
    }

    pub fn browser_profile_path(&self) -> Result<PathBuf, WorkspaceError> {
        self.validate_browser_profile_path(&self.browser_profile_path)?;
        Ok(self.browser_profile_path.clone())
    }

    pub fn validate_browser_profile_path(&self, path: &Path) -> Result<(), WorkspaceError> {
        if path != self.browser_profile_path {
            return Err(WorkspaceError::UnsafeEntry);
        }
        let root = open_root(&self.work_root)?;
        require_private(&root)?;
        check_not_retired(&root, &self.identity)?;
        let dots = directory(&root, "dots", false)?;
        require_private(&dots)?;
        let dot = directory(&dots, &self.identity.dot_id, false)?;
        require_private(&dot)?;
        let mut manifest = open_regular(&dot, "owner.json")?;
        require_private(&manifest)?;
        let saved: DotWorkspace =
            serde_json::from_slice(&read_bounded(&mut manifest, MAX_MANIFEST_BYTES)?)?;
        if saved != self.identity {
            return Err(WorkspaceError::OwnerMismatch);
        }
        let browser_profile = directory(&dot, "browser-profile", false)?;
        same_directory(&browser_profile, &self.browser_profile)
    }

    fn validate_path(&self, path: &Path) -> Result<(), WorkspaceError> {
        if path != self.files_path {
            return Err(WorkspaceError::UnsafeEntry);
        }
        let root = open_root(&self.work_root)?;
        require_private(&root)?;
        check_not_retired(&root, &self.identity)?;
        let dots = directory(&root, "dots", false)?;
        require_private(&dots)?;
        let dot = directory(&dots, &self.identity.dot_id, false)?;
        require_private(&dot)?;
        let mut manifest = open_regular(&dot, "owner.json")?;
        require_private(&manifest)?;
        let saved: DotWorkspace =
            serde_json::from_slice(&read_bounded(&mut manifest, MAX_MANIFEST_BYTES)?)?;
        if saved != self.identity {
            return Err(WorkspaceError::OwnerMismatch);
        }
        let files = directory(&dot, "files", false)?;
        same_directory(&files, &self.files)
    }

    pub fn read(&self, path: &str) -> Result<Vec<u8>, WorkspaceError> {
        read_from(&self.files, path)
    }

    pub fn write(&self, path: &str, bytes: &[u8]) -> Result<(), WorkspaceError> {
        if bytes.len() > STAGE_TOTAL_MAX_BYTES {
            return Err(WorkspaceError::FileTooLarge);
        }
        let (parent, name) = self.parent(path)?;
        match stat_at(&parent, name) {
            Ok(stat) if file_kind(&stat) == libc::S_IFREG => {}
            Ok(_) => return Err(WorkspaceError::UnsafeEntry),
            Err(WorkspaceError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let mut temp = TemporaryFile::new(&parent)?;
        temp.file.write_all(bytes)?;
        temp.file.sync_all()?;
        let destination = cstring(name)?;
        if unsafe {
            libc::renameat(
                parent.as_raw_fd(),
                temp.name.as_ptr(),
                parent.as_raw_fd(),
                destination.as_ptr(),
            )
        } != 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        parent.sync_all()?;
        Ok(())
    }

    pub fn mkdir(&self, path: &str) -> Result<(), WorkspaceError> {
        let (parent, name) = self.parent(path)?;
        let name = cstring(name)?;
        if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let created = open_fd(
            parent.as_raw_fd(),
            &name,
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
            0,
        )?;
        let owner = parent.metadata()?;
        // Management holds the workspace mutation reservation, so no workload can enter before ownership is set.
        set_owner(&created, owner.uid(), owner.gid())?;
        let linked = open_fd(
            parent.as_raw_fd(),
            &name,
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
            0,
        )?
        .metadata()?;
        let held = created.metadata()?;
        if linked.dev() != held.dev() || linked.ino() != held.ino() {
            return Err(WorkspaceError::UnsafeEntry);
        }
        parent.sync_all()?;
        Ok(())
    }

    pub fn remove(&self, path: &str) -> Result<(), WorkspaceError> {
        let (parent, name) = self.parent(path)?;
        let stat = stat_at(&parent, name)?;
        let flags = match file_kind(&stat) {
            libc::S_IFREG => 0,
            libc::S_IFDIR => libc::AT_REMOVEDIR,
            _ => return Err(WorkspaceError::UnsafeEntry),
        };
        let name = cstring(name)?;
        if unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), flags) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        parent.sync_all()?;
        Ok(())
    }

    pub fn list(
        &self,
        path: &str,
        after: Option<&str>,
    ) -> Result<WorkspaceListing, WorkspaceError> {
        if let Some(after) = after
            && components(after)?.len() != 1
        {
            return Err(WorkspaceError::InvalidPath("cursor is not a filename"));
        }
        let directory = self.open_directory(path)?;
        let mut page = BTreeMap::new();
        scan_names(&directory, SCAN_LIMIT, |name| {
            let stat = stat_at(&directory, &name)?;
            let (kind, size_bytes) = match file_kind(&stat) {
                libc::S_IFREG => (
                    WorkspaceEntryKind::File,
                    Some(
                        stat.st_size
                            .try_into()
                            .map_err(|_| WorkspaceError::UnsafeEntry)?,
                    ),
                ),
                libc::S_IFDIR => (WorkspaceEntryKind::Directory, None),
                _ => return Err(WorkspaceError::UnsafeEntry),
            };
            if after.is_none_or(|after| name.as_str() > after) {
                let modified_at = modified_at(&stat);
                page.insert(
                    name.clone(),
                    WorkspaceEntry {
                        name,
                        kind,
                        size_bytes,
                        modified_at,
                    },
                );
                if page.len() > PAGE_SIZE + 1 {
                    page.pop_last();
                }
            }
            Ok(())
        })?;
        let has_more = page.len() > PAGE_SIZE;
        if has_more {
            page.pop_last();
        }
        let next_cursor = if has_more {
            page.last_key_value().map(|(name, _)| name.clone())
        } else {
            None
        };
        Ok(WorkspaceListing {
            entries: page.into_values().collect(),
            next_cursor,
        })
    }

    fn parent<'a>(&self, path: &'a str) -> Result<(File, &'a str), WorkspaceError> {
        parent_from(&self.files, path)
    }

    fn open_directory(&self, path: &str) -> Result<File, WorkspaceError> {
        let mut current = self.files.try_clone()?;
        if !path.is_empty() {
            for part in components(path)? {
                current = directory(&current, part, false)?;
            }
        }
        Ok(current)
    }
}

pub struct WorkspaceProbe {
    files: File,
    probe: File,
    work_root: PathBuf,
    probe_name: String,
    files_path: PathBuf,
}

impl WorkspaceProbe {
    pub fn create(work_root: &Path) -> Result<Self, WorkspaceError> {
        let work_root = if work_root.is_absolute() {
            work_root.to_path_buf()
        } else {
            std::env::current_dir()?.join(work_root)
        };
        let root = open_root(&work_root)?;
        require_private(&root)?;
        let probes = directory(&root, ".workspace-probes", true)?;
        require_private(&probes)?;
        for _ in 0..32 {
            let probe_name = format!(
                "probe-{}-{}",
                std::process::id(),
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
            );
            let name = cstring(&probe_name)?;
            if unsafe { libc::mkdirat(probes.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    continue;
                }
                return Err(error.into());
            }
            let probe = directory(&probes, &probe_name, false)?;
            require_private(&probe)?;
            let files = directory(&probe, "files", true)?;
            let files_path = work_root
                .join(".workspace-probes")
                .join(&probe_name)
                .join("files");
            return Ok(Self {
                files,
                probe,
                work_root,
                probe_name,
                files_path,
            });
        }
        Err(WorkspaceError::Io(std::io::Error::from(
            std::io::ErrorKind::AlreadyExists,
        )))
    }

    pub fn mount_path(&self) -> Result<PathBuf, WorkspaceError> {
        self.validate_mount_path(&self.files_path)?;
        Ok(self.files_path.clone())
    }

    pub fn validate_mount_path(&self, path: &Path) -> Result<(), WorkspaceError> {
        if path != self.files_path {
            return Err(WorkspaceError::UnsafeEntry);
        }
        let root = open_root(&self.work_root)?;
        require_private(&root)?;
        let probes = directory(&root, ".workspace-probes", false)?;
        require_private(&probes)?;
        let probe = directory(&probes, &self.probe_name, false)?;
        require_private(&probe)?;
        same_directory(&probe, &self.probe)?;
        let files = directory(&probe, "files", false)?;
        same_directory(&files, &self.files)
    }

    pub fn prepare_workload_owner(&self, uid: u32, gid: u32) -> Result<(), WorkspaceError> {
        self.validate_mount_path(&self.files_path)?;
        prepare_owner(&self.files, uid, gid)
    }

    pub fn read(&self, path: &str) -> Result<Vec<u8>, WorkspaceError> {
        read_from(&self.files, path)
    }

    pub fn metadata(&self, path: &str) -> Result<std::fs::Metadata, WorkspaceError> {
        let (parent, name) = parent_from(&self.files, path)?;
        let stat = stat_at(&parent, name)?;
        match file_kind(&stat) {
            libc::S_IFREG => Ok(open_regular(&parent, name)?.metadata()?),
            libc::S_IFDIR => Ok(directory(&parent, name, false)?.metadata()?),
            _ => Err(WorkspaceError::UnsafeEntry),
        }
    }

    pub fn cleanup(&self) -> Result<(), WorkspaceError> {
        self.validate_mount_path(&self.files_path)?;
        if names(&self.probe, 2)? != ["files"] {
            return Err(WorkspaceError::UnsafeEntry);
        }
        let entries = names(&self.files, 2)?;
        if entries.iter().any(|name| name != "private") {
            return Err(WorkspaceError::UnsafeEntry);
        }
        let private = if entries.is_empty() {
            None
        } else {
            let private = directory(&self.files, "private", false)?;
            let entries = names(&private, 2)?;
            if entries.iter().any(|name| name != "probe.bin") {
                return Err(WorkspaceError::UnsafeEntry);
            }
            if !entries.is_empty() {
                drop(open_regular(&private, "probe.bin")?);
            }
            Some((private, !entries.is_empty()))
        };
        if let Some((private, has_file)) = private {
            if has_file {
                unlink(&private, "probe.bin", false)?;
            }
            unlink(&self.files, "private", true)?;
        }
        unlink(&self.probe, "files", true)?;
        let root = open_root(&self.work_root)?;
        let probes = directory(&root, ".workspace-probes", false)?;
        unlink(&probes, &self.probe_name, true)?;
        probes.sync_all()?;
        Ok(())
    }
}

fn same_directory(actual: &File, held: &File) -> Result<(), WorkspaceError> {
    let actual = actual.metadata()?;
    let held = held.metadata()?;
    if actual.dev() != held.dev() || actual.ino() != held.ino() {
        return Err(WorkspaceError::UnsafeEntry);
    }
    Ok(())
}

fn prepare_owner(files: &File, uid: u32, gid: u32) -> Result<(), WorkspaceError> {
    if !cfg!(target_os = "linux") || unsafe { libc::geteuid() } != 0 {
        return Err(WorkspaceError::UnsupportedOwnership);
    }
    set_owner(files, uid, gid)?;
    if unsafe { libc::fchmod(files.as_raw_fd(), 0o700) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

fn parent_from<'a>(files: &File, path: &'a str) -> Result<(File, &'a str), WorkspaceError> {
    let parts = components(path)?;
    let mut parent = files.try_clone()?;
    for part in &parts[..parts.len() - 1] {
        parent = directory(&parent, part, false)?;
    }
    Ok((parent, parts[parts.len() - 1]))
}

fn read_from(files: &File, path: &str) -> Result<Vec<u8>, WorkspaceError> {
    let (parent, name) = parent_from(files, path)?;
    let mut file = open_regular(&parent, name)?;
    read_bounded(&mut file, STAGE_TOTAL_MAX_BYTES)
}

fn unlink(parent: &File, name: &str, directory: bool) -> Result<(), WorkspaceError> {
    let name = cstring(name)?;
    if unsafe {
        libc::unlinkat(
            parent.as_raw_fd(),
            name.as_ptr(),
            if directory { libc::AT_REMOVEDIR } else { 0 },
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

use std::os::unix::fs::{MetadataExt, PermissionsExt};

fn validate_identity(identity: &DotWorkspace) -> Result<(), WorkspaceError> {
    let uuid = identity.dot_id.as_bytes();
    if identity.owner_user_id.is_empty()
        || uuid.len() != 36
        || !uuid.iter().enumerate().all(|(index, byte)| {
            if [8, 13, 18, 23].contains(&index) {
                *byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(byte)
            }
        })
    {
        return Err(WorkspaceError::InvalidPath("Dot identity is not canonical"));
    }
    Ok(())
}

fn components(path: &str) -> Result<Vec<&str>, WorkspaceError> {
    if path.is_empty() || path.len() > MAX_PATH_BYTES || path.contains('\0') {
        return Err(WorkspaceError::InvalidPath(
            "empty, NUL-containing or overlong path",
        ));
    }
    let parts: Vec<_> = path.split('/').collect();
    if parts.len() > MAX_COMPONENTS
        || parts
            .iter()
            .any(|part| part.is_empty() || *part == "." || *part == "..")
    {
        return Err(WorkspaceError::InvalidPath(
            "invalid path component or excessive depth",
        ));
    }
    Ok(parts)
}

fn cstring(value: &str) -> Result<CString, WorkspaceError> {
    CString::new(value).map_err(|_| WorkspaceError::InvalidPath("NUL-containing filename"))
}

fn open_root(path: &Path) -> Result<File, WorkspaceError> {
    let initial = cstring(if path.is_absolute() { "/" } else { "." })?;
    let mut root = open_fd(
        libc::AT_FDCWD,
        &initial,
        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
        0,
    )?;
    for part in path.components() {
        match part {
            Component::RootDir => {}
            Component::Normal(name) => {
                let name = name
                    .to_str()
                    .ok_or(WorkspaceError::InvalidPath("non-UTF-8 storage root"))?;
                root = directory(&root, name, false)?;
            }
            _ => return Err(WorkspaceError::InvalidPath("noncanonical storage root")),
        }
    }
    Ok(root)
}

fn open_fd(
    parent: libc::c_int,
    name: &CStr,
    flags: libc::c_int,
    mode: libc::mode_t,
) -> Result<File, WorkspaceError> {
    let fd = unsafe {
        libc::openat(
            parent,
            name.as_ptr(),
            flags | libc::O_CLOEXEC,
            mode as libc::c_uint,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn directory(parent: &File, name: &str, create: bool) -> Result<File, WorkspaceError> {
    let name = cstring(name)?;
    if create && unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::AlreadyExists {
            return Err(error.into());
        }
    }
    open_fd(
        parent.as_raw_fd(),
        &name,
        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
        0,
    )
}

fn require_private(directory: &File) -> Result<(), WorkspaceError> {
    let metadata = directory.metadata()?;
    if metadata.permissions().mode() & 0o077 != 0 || metadata.uid() != unsafe { libc::geteuid() } {
        return Err(WorkspaceError::UnsafeEntry);
    }
    Ok(())
}

fn set_owner(file: &File, uid: u32, gid: u32) -> Result<(), WorkspaceError> {
    let current = file.metadata()?;
    if current.uid() == uid && current.gid() == gid {
        return Ok(());
    }
    if unsafe { libc::fchown(file.as_raw_fd(), uid, gid) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

fn stat_at(parent: &File, name: &str) -> Result<libc::stat, WorkspaceError> {
    let name = cstring(name)?;
    stat_raw(parent, &name)
}

fn file_kind(stat: &libc::stat) -> libc::mode_t {
    stat.st_mode & libc::S_IFMT
}

fn open_regular(parent: &File, name: &str) -> Result<File, WorkspaceError> {
    let name = cstring(name)?;
    let file = open_fd(
        parent.as_raw_fd(),
        &name,
        libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK,
        0,
    )?;
    if !file.metadata()?.is_file() {
        return Err(WorkspaceError::UnsafeEntry);
    }
    Ok(file)
}

fn read_bounded(file: &mut File, maximum: usize) -> Result<Vec<u8>, WorkspaceError> {
    if file.metadata()?.len() > maximum as u64 {
        return Err(WorkspaceError::FileTooLarge);
    }
    let mut bytes = Vec::new();
    file.take(maximum as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > maximum {
        return Err(WorkspaceError::FileTooLarge);
    }
    Ok(bytes)
}

struct TemporaryFile<'a> {
    parent: &'a File,
    name: CString,
    file: File,
}

impl<'a> TemporaryFile<'a> {
    fn new(parent: &'a File) -> Result<Self, WorkspaceError> {
        for _ in 0..32 {
            let name = cstring(&format!(
                ".workspace-{}-{}",
                std::process::id(),
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
            ))?;
            match open_fd(
                parent.as_raw_fd(),
                &name,
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW,
                0o600,
            ) {
                Ok(file) => {
                    let owner = parent.metadata()?;
                    if let Err(error) = set_owner(&file, owner.uid(), owner.gid()) {
                        unsafe {
                            libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), 0);
                        }
                        return Err(error);
                    }
                    return Ok(Self { parent, name, file });
                }
                Err(WorkspaceError::Io(error))
                    if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        Err(WorkspaceError::Io(std::io::Error::from(
            std::io::ErrorKind::AlreadyExists,
        )))
    }
}

impl Drop for TemporaryFile<'_> {
    fn drop(&mut self) {
        unsafe {
            libc::unlinkat(self.parent.as_raw_fd(), self.name.as_ptr(), 0);
        }
    }
}

struct DirectoryStream(*mut libc::DIR);
impl Drop for DirectoryStream {
    fn drop(&mut self) {
        unsafe {
            libc::closedir(self.0);
        }
    }
}

#[cfg(target_os = "linux")]
fn errno() -> *mut libc::c_int {
    unsafe { libc::__errno_location() }
}
#[cfg(target_os = "macos")]
fn errno() -> *mut libc::c_int {
    unsafe { libc::__error() }
}

fn scan_names(
    directory: &File,
    limit: usize,
    mut visit: impl FnMut(String) -> Result<(), WorkspaceError>,
) -> Result<(), WorkspaceError> {
    let stream = open_stream(directory)?;
    let mut count = 0;
    while let Some(name) = next_name(&stream)? {
        count += 1;
        if count > limit {
            return Err(WorkspaceError::DirectoryTooLarge);
        }
        let name = name
            .to_str()
            .map_err(|_| WorkspaceError::InvalidPath("non-UTF-8 directory entry"))?;
        visit(name.to_owned())?;
    }
    Ok(())
}

fn names(directory: &File, limit: usize) -> Result<Vec<String>, WorkspaceError> {
    let mut names = Vec::new();
    scan_names(directory, limit, |name| {
        names.push(name);
        Ok(())
    })?;
    Ok(names)
}

/// Whether a private root holds any entry. A directory past the scan limit is
/// also nonempty; only genuine I/O errors propagate.
fn unclaimed_nonempty(directory: &File) -> Result<bool, WorkspaceError> {
    match names(directory, 2) {
        Ok(names) => Ok(!names.is_empty()),
        Err(WorkspaceError::DirectoryTooLarge) => Ok(true),
        Err(error) => Err(error),
    }
}

fn optional_directory(parent: &File, name: &str) -> Result<Option<File>, WorkspaceError> {
    match directory(parent, name, false) {
        Ok(directory) => Ok(Some(directory)),
        Err(WorkspaceError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn check_manifest(directory: &File, identity: &DotWorkspace) -> Result<(), WorkspaceError> {
    let mut manifest = open_regular(directory, "owner.json")?;
    require_private(&manifest)?;
    let saved: DotWorkspace =
        serde_json::from_slice(&read_bounded(&mut manifest, MAX_MANIFEST_BYTES)?)?;
    if saved != *identity {
        return Err(WorkspaceError::OwnerMismatch);
    }
    Ok(())
}

fn check_not_retired(root: &File, identity: &DotWorkspace) -> Result<(), WorkspaceError> {
    if let Some(retired) = optional_directory(root, ".dot-retired")? {
        require_private(&retired)?;
        if let Some(dot) = optional_directory(&retired, &identity.dot_id)? {
            require_private(&dot)?;
            // An interrupted seal is also closed until retirement is repaired, never a fresh claim.
            check_manifest(&dot, identity)?;
            return Err(WorkspaceError::Retired);
        }
    }
    Ok(())
}

fn publish_manifest(directory: &File, identity: &DotWorkspace) -> Result<(), WorkspaceError> {
    let bytes = serde_json::to_vec(identity)?;
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(WorkspaceError::OwnerMismatch);
    }
    let mut temp = TemporaryFile::new(directory)?;
    temp.file.write_all(&bytes)?;
    temp.file.sync_all()?;
    let destination = cstring("owner.json")?;
    if unsafe {
        libc::linkat(
            directory.as_raw_fd(),
            temp.name.as_ptr(),
            directory.as_raw_fd(),
            destination.as_ptr(),
            0,
        )
    } != 0
    {
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::AlreadyExists {
            return Err(error.into());
        }
        check_manifest(directory, identity)?;
    }
    drop(temp);
    directory.sync_all()?;
    Ok(())
}

fn stat_raw(parent: &File, name: &CStr) -> Result<libc::stat, WorkspaceError> {
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    if unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(unsafe { stat.assume_init() })
}

fn held_stat(file: &File) -> Result<libc::stat, WorkspaceError> {
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    if unsafe { libc::fstat(file.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(unsafe { stat.assume_init() })
}

fn open_stream(directory: &File) -> Result<DirectoryStream, WorkspaceError> {
    let dot = c".";
    let fd = open_fd(
        directory.as_raw_fd(),
        dot,
        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
        0,
    )?
    .into_raw_fd();
    let stream = unsafe { libc::fdopendir(fd) };
    if stream.is_null() {
        let error = std::io::Error::last_os_error();
        unsafe {
            libc::close(fd);
        }
        return Err(error.into());
    }
    Ok(DirectoryStream(stream))
}

fn next_name(stream: &DirectoryStream) -> Result<Option<CString>, WorkspaceError> {
    loop {
        unsafe {
            *errno() = 0;
        }
        let entry = unsafe { libc::readdir(stream.0) };
        if entry.is_null() {
            let error = unsafe { *errno() };
            return if error == 0 {
                Ok(None)
            } else {
                Err(std::io::Error::from_raw_os_error(error).into())
            };
        }
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
        if name.to_bytes() == b"." || name.to_bytes() == b".." {
            continue;
        }
        return Ok(Some(name.to_owned()));
    }
}

fn remove_retired_node(
    parent: &File,
    name: &CStr,
    depth: usize,
    remaining: &mut usize,
) -> Result<bool, WorkspaceError> {
    if depth > 128 {
        return Err(WorkspaceError::CleanupDepth);
    }
    if *remaining == 0 {
        return Ok(false);
    }
    *remaining -= 1;
    let stat = match stat_raw(parent, name) {
        Ok(stat) => stat,
        Err(WorkspaceError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(true);
        }
        Err(error) => return Err(error),
    };
    let is_directory = file_kind(&stat) == libc::S_IFDIR;
    if is_directory {
        let directory = open_fd(
            parent.as_raw_fd(),
            name,
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
            0,
        )?;
        let held = held_stat(&directory)?;
        if held.st_dev != stat.st_dev || held.st_ino != stat.st_ino {
            return Err(WorkspaceError::UnsafeEntry);
        }
        let stream = open_stream(&directory)?;
        while let Some(child) = next_name(&stream)? {
            if !remove_retired_node(&directory, &child, depth + 1, remaining)? {
                directory.sync_all()?;
                return Ok(false);
            }
        }
        let current = stat_raw(parent, name)?;
        if current.st_dev != held.st_dev || current.st_ino != held.st_ino {
            return Err(WorkspaceError::UnsafeEntry);
        }
        directory.sync_all()?;
    }
    let flags = if is_directory { libc::AT_REMOVEDIR } else { 0 };
    if unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), flags) } != 0 {
        let error = std::io::Error::last_os_error();
        if is_directory && error.raw_os_error() == Some(libc::ENOTEMPTY) {
            return Ok(false);
        }
        if error.kind() != std::io::ErrorKind::NotFound {
            return Err(error.into());
        }
    }
    parent.sync_all()?;
    Ok(true)
}

fn modified_at(stat: &libc::stat) -> Option<String> {
    let seconds = stat.st_mtime;
    let mut utc = std::mem::MaybeUninit::<libc::tm>::uninit();
    if unsafe { libc::gmtime_r(&seconds, utc.as_mut_ptr()) }.is_null() {
        return None;
    }
    let utc = unsafe { utc.assume_init() };
    let mut bytes = [0u8; 32];
    let format = c"%Y-%m-%dT%H:%M:%SZ";
    let len = unsafe {
        libc::strftime(
            bytes.as_mut_ptr().cast(),
            bytes.len(),
            format.as_ptr(),
            &utc,
        )
    };
    if len == 0 {
        None
    } else {
        std::str::from_utf8(&bytes[..len]).ok().map(str::to_owned)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use shared::sandbox::{DotWorkspace, STAGE_TOTAL_MAX_BYTES};

    use super::*;

    static NEXT: AtomicU64 = AtomicU64::new(0);
    const DOT: &str = "12345678-1234-4234-8234-123456789abc";

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let root = fs::canonicalize("/tmp").unwrap().join(format!(
                "aw-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
            Self(fs::canonicalize(root).unwrap())
        }

        fn identity(&self) -> DotWorkspace {
            DotWorkspace {
                owner_user_id: "opaque/owner:ß".into(),
                dot_id: DOT.into(),
            }
        }

        fn dot_root(&self) -> PathBuf {
            self.0.join("dots").join(DOT)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn workspace_files_persist_and_keep_control_metadata_private() {
        let fixture = Fixture::new();
        let identity = fixture.identity();
        let files = Workspace::open(&fixture.0, &identity).unwrap();
        files.mkdir("notes").unwrap();
        files.write("notes/result.txt", b"first").unwrap();
        files.write("notes/result.txt", b"replacement").unwrap();
        assert_eq!(files.read("notes/result.txt").unwrap(), b"replacement");
        assert!(files.read("../owner.json").is_err());
        assert!(files.read("owner.json").is_err());
        drop(files);
        let files = Workspace::open(&fixture.0, &identity).unwrap();
        assert_eq!(files.read("notes/result.txt").unwrap(), b"replacement");
        assert_eq!(
            fs::read_dir(fixture.dot_root().join("files/notes"))
                .unwrap()
                .count(),
            1
        );
        assert_eq!(
            fs::metadata(fixture.dot_root().join("owner.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(fixture.dot_root())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }

    #[test]
    fn workspace_files_refuse_foreign_or_malformed_owner_manifest() {
        let fixture = Fixture::new();
        let identity = fixture.identity();
        drop(Workspace::open(&fixture.0, &identity).unwrap());
        let original = fs::read(fixture.dot_root().join("owner.json")).unwrap();
        let mut foreign = identity.clone();
        foreign.owner_user_id = "another-owner".into();
        assert!(Workspace::open(&fixture.0, &foreign).is_err());
        assert_eq!(
            fs::read(fixture.dot_root().join("owner.json")).unwrap(),
            original
        );
        fs::write(fixture.dot_root().join("owner.json"), b"{malformed").unwrap();
        assert!(Workspace::open(&fixture.0, &identity).is_err());
        assert_eq!(
            fs::read(fixture.dot_root().join("owner.json")).unwrap(),
            b"{malformed"
        );
    }

    #[test]
    fn workspace_files_refuse_unclaimed_nonempty_dot_directory() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.dot_root().join("files")).unwrap();
        fs::write(fixture.dot_root().join("files/existing"), b"unclaimed").unwrap();
        assert!(Workspace::open(&fixture.0, &fixture.identity()).is_err());
        assert!(!fixture.dot_root().join("owner.json").exists());
    }

    #[test]
    fn workspace_files_require_canonical_dot_uuid_and_safe_relative_paths() {
        let fixture = Fixture::new();
        for dot_id in [
            "../escape",
            "12345678-1234-4234-8234-123456789ABC",
            "not-a-uuid",
            "",
        ] {
            let mut identity = fixture.identity();
            identity.dot_id = dot_id.into();
            assert!(Workspace::open(&fixture.0, &identity).is_err(), "{dot_id}");
        }
        let files = Workspace::open(&fixture.0, &fixture.identity()).unwrap();
        for path in [
            "",
            "/absolute",
            ".",
            "..",
            "a/../b",
            "a/./b",
            "a//b",
            "a/",
            "a\0b",
        ] {
            assert!(files.write(path, b"refused").is_err(), "{path:?}");
            assert!(files.mkdir(path).is_err(), "{path:?}");
            assert!(files.remove(path).is_err(), "{path:?}");
        }
        assert!(files.write(&"x".repeat(4097), b"refused").is_err());
        assert!(files.write(&vec!["a"; 33].join("/"), b"refused").is_err());
        files.write("UTF8-ß.txt", b"allowed").unwrap();
        assert_eq!(files.read("UTF8-ß.txt").unwrap(), b"allowed");
    }

    #[test]
    fn workspace_files_refuse_symlinks_at_every_directory_boundary() {
        let fixture = Fixture::new();
        let outside = Fixture::new();
        fs::write(outside.0.join("secret"), b"outside").unwrap();
        let files = Workspace::open(&fixture.0, &fixture.identity()).unwrap();
        let root = fixture.dot_root().join("files");
        symlink(&outside.0, root.join("linked")).unwrap();
        symlink(outside.0.join("secret"), root.join("leaf")).unwrap();
        for path in ["linked/secret", "leaf"] {
            assert!(files.read(path).is_err());
            assert!(files.write(path, b"overwrite").is_err());
            assert!(files.remove(path).is_err());
        }
        assert!(files.mkdir("linked/new").is_err());
        assert_eq!(fs::read(outside.0.join("secret")).unwrap(), b"outside");
        drop(files);
        fs::remove_dir_all(&root).unwrap();
        symlink(&outside.0, &root).unwrap();
        assert!(Workspace::open(&fixture.0, &fixture.identity()).is_err());
    }

    #[test]
    fn workspace_files_refuse_symlink_store_and_manifest() {
        let fixture = Fixture::new();
        let outside = Fixture::new();
        let linked = fixture.0.join("root-link");
        symlink(&outside.0, &linked).unwrap();
        assert!(Workspace::open(&linked, &fixture.identity()).is_err());
        drop(Workspace::open(&fixture.0, &fixture.identity()).unwrap());
        fs::remove_file(fixture.dot_root().join("owner.json")).unwrap();
        fs::write(outside.0.join("owner"), b"outside").unwrap();
        symlink(
            outside.0.join("owner"),
            fixture.dot_root().join("owner.json"),
        )
        .unwrap();
        assert!(Workspace::open(&fixture.0, &fixture.identity()).is_err());
    }

    #[test]
    fn workspace_files_reject_special_files_without_waiting_for_a_writer() {
        let fixture = Fixture::new();
        let files = Workspace::open(&fixture.0, &fixture.identity()).unwrap();
        let fifo = std::ffi::CString::new(
            fixture
                .dot_root()
                .join("files/fifo")
                .as_os_str()
                .as_encoded_bytes(),
        )
        .unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        assert!(files.read("fifo").is_err());
        assert!(files.write("fifo", b"refused").is_err());
        assert!(files.remove("fifo").is_err());
        let socket =
            std::os::unix::net::UnixListener::bind(fixture.dot_root().join("files/socket"))
                .unwrap();
        assert!(files.read("socket").is_err());
        assert!(files.write("socket", b"refused").is_err());
        assert!(files.remove("socket").is_err());
        drop(socket);
    }

    #[test]
    fn workspace_files_enforce_existing_staging_budget_before_read_or_write() {
        let fixture = Fixture::new();
        let files = Workspace::open(&fixture.0, &fixture.identity()).unwrap();
        let oversized = fs::File::create(fixture.dot_root().join("files/large")).unwrap();
        oversized.set_len(STAGE_TOTAL_MAX_BYTES as u64 + 1).unwrap();
        assert!(files.read("large").is_err());
        files.write("keep", b"original").unwrap();
        assert!(
            files
                .write("keep", &vec![0; STAGE_TOTAL_MAX_BYTES + 1])
                .is_err()
        );
        assert_eq!(files.read("keep").unwrap(), b"original");
    }

    #[test]
    fn workspace_files_delete_only_regular_files_and_empty_directories() {
        let fixture = Fixture::new();
        let files = Workspace::open(&fixture.0, &fixture.identity()).unwrap();
        files.mkdir("directory").unwrap();
        files.write("directory/file", b"retained").unwrap();
        assert!(files.remove("directory").is_err());
        assert_eq!(files.read("directory/file").unwrap(), b"retained");
        files.remove("directory/file").unwrap();
        files.remove("directory").unwrap();
        assert!(!fixture.dot_root().join("files/directory").exists());
    }
    #[test]
    fn workspace_files_list_bounded_sorted_pages_without_control_metadata() {
        let fixture = Fixture::new();
        let files = Workspace::open(&fixture.0, &fixture.identity()).unwrap();
        for index in (0..101).rev() {
            files.write(&format!("file-{index:03}"), b"value").unwrap();
        }
        let page = files.list("", None).unwrap();
        assert_eq!(page.entries.len(), 100);
        assert_eq!(page.entries.first().unwrap().name, "file-000");
        assert_eq!(page.entries.last().unwrap().name, "file-099");
        assert_eq!(page.next_cursor.as_deref(), Some("file-099"));
        assert!(page.entries.iter().all(|entry| entry.size_bytes == Some(5)));
        let next = files.list("", page.next_cursor.as_deref()).unwrap();
        assert_eq!(next.entries.len(), 1);
        assert_eq!(next.entries[0].name, "file-100");
        assert!(next.next_cursor.is_none());
        assert!(files.list("", Some("../escape")).is_err());
        assert!(files.list("../", None).is_err());
        assert!(!page.entries.iter().any(|entry| entry.name == "owner.json"));
    }

    #[test]
    fn workspace_files_listing_refuses_special_entries() {
        let fixture = Fixture::new();
        let outside = Fixture::new();
        let files = Workspace::open(&fixture.0, &fixture.identity()).unwrap();
        let root = fixture.dot_root().join("files");
        symlink(&outside.0, root.join("linked")).unwrap();
        assert!(files.list("", None).is_err());
        fs::remove_file(root.join("linked")).unwrap();
        let socket = std::os::unix::net::UnixListener::bind(root.join("socket")).unwrap();
        assert!(files.list("", None).is_err());
        drop(socket);
    }

    // The macOS test filesystem rejects non-UTF-8 names before listing; Linux is the supported production target.
    #[cfg(target_os = "linux")]
    #[test]
    fn workspace_files_listing_refuses_non_utf8_names() {
        use std::os::unix::ffi::OsStringExt;
        let fixture = Fixture::new();
        let files = Workspace::open(&fixture.0, &fixture.identity()).unwrap();
        let root = fixture.dot_root().join("files");
        fs::write(
            root.join(std::ffi::OsString::from_vec(vec![0xff])),
            b"value",
        )
        .unwrap();
        assert!(files.list("", None).is_err());
    }

    #[test]
    fn workspace_files_listing_refuses_over_scan_budget() {
        let fixture = Fixture::new();
        let files = Workspace::open(&fixture.0, &fixture.identity()).unwrap();
        let root = fixture.dot_root().join("files");
        for index in 0..10001 {
            fs::File::create(root.join(format!("file-{index:05}"))).unwrap();
        }
        assert!(files.list("", None).is_err());
    }
    #[test]
    fn workspace_files_concurrent_same_owner_claims_can_retry_without_replacement() {
        let fixture = Fixture::new();
        let identity = fixture.identity();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(4));
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let barrier = barrier.clone();
                let root = fixture.0.clone();
                let identity = identity.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    Workspace::open(&root, &identity).is_ok()
                })
            })
            .collect();
        let admitted = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .filter(|admitted| *admitted)
            .count();
        assert!(admitted >= 1);
        let original = fs::read(fixture.dot_root().join("owner.json")).unwrap();
        for _ in 0..4 {
            let files = Workspace::open(&fixture.0, &identity).unwrap();
            files.write("retained", b"same owner").unwrap();
        }
        assert_eq!(
            fs::read(fixture.dot_root().join("owner.json")).unwrap(),
            original
        );
    }

    #[test]
    fn workspace_files_competing_owners_cannot_replace_the_winning_manifest() {
        let fixture = Fixture::new();
        let mut other = fixture.identity();
        other.owner_user_id = "competing-owner".into();
        let identities = [fixture.identity(), other];
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let handles: Vec<_> = identities
            .iter()
            .cloned()
            .map(|identity| {
                let barrier = barrier.clone();
                let root = fixture.0.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    Workspace::open(&root, &identity).is_ok()
                })
            })
            .collect();
        let outcomes: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        assert_eq!(outcomes.iter().filter(|admitted| **admitted).count(), 1);
        let original = fs::read(fixture.dot_root().join("owner.json")).unwrap();
        for (identity, admitted) in identities.iter().zip(outcomes) {
            assert_eq!(Workspace::open(&fixture.0, identity).is_ok(), admitted);
        }
        assert_eq!(
            fs::read(fixture.dot_root().join("owner.json")).unwrap(),
            original
        );
    }

    #[test]
    fn workspace_files_api_writes_inherit_opened_parent_ownership_and_private_modes() {
        use std::os::unix::fs::MetadataExt;
        let fixture = Fixture::new();
        let files = Workspace::open(&fixture.0, &fixture.identity()).unwrap();
        files.mkdir("nested").unwrap();
        files.write("nested/file", b"value").unwrap();
        let parent = fs::metadata(fixture.dot_root().join("files")).unwrap();
        for relative in ["nested", "nested/file"] {
            let metadata = fs::metadata(fixture.dot_root().join("files").join(relative)).unwrap();
            assert_eq!(
                (metadata.uid(), metadata.gid()),
                (parent.uid(), parent.gid())
            );
            let expected = if metadata.is_dir() { 0o700 } else { 0o600 };
            assert_eq!(metadata.permissions().mode() & 0o777, expected);
        }
    }

    #[test]
    fn workspace_files_mount_revalidation_rejects_path_substitution_and_public_control_parent() {
        let fixture = Fixture::new();
        let files = Workspace::open(&fixture.0, &fixture.identity()).unwrap();
        let original = fixture.dot_root().join("files");
        files.validate_path(&original).unwrap();
        fs::rename(&original, fixture.dot_root().join("old-files")).unwrap();
        fs::create_dir(&original).unwrap();
        assert!(files.validate_path(&original).is_err());
        fs::remove_dir(&original).unwrap();
        fs::rename(fixture.dot_root().join("old-files"), &original).unwrap();
        assert!(files.validate_path(&original).is_ok());
        fs::set_permissions(fixture.dot_root(), fs::Permissions::from_mode(0o755)).unwrap();
        assert!(files.validate_path(&original).is_err());
    }

    #[test]
    fn workspace_files_workload_preparation_requires_supported_actual_process_authority() {
        use std::os::unix::fs::MetadataExt;
        let fixture = Fixture::new();
        let files = Workspace::open(&fixture.0, &fixture.identity()).unwrap();
        let root = fixture.dot_root().join("files");
        let before = fs::metadata(&root).unwrap();
        if cfg!(target_os = "linux") && unsafe { libc::geteuid() } == 0 {
            files.prepare_workload_owner(1001, 1001).unwrap();
            assert_eq!(
                (
                    fs::metadata(&root).unwrap().uid(),
                    fs::metadata(&root).unwrap().gid()
                ),
                (1001, 1001)
            );
            files.mkdir("nested").unwrap();
            files.write("nested/value", b"owned").unwrap();
            for relative in ["nested", "nested/value"] {
                let metadata = fs::metadata(root.join(relative)).unwrap();
                assert_eq!((metadata.uid(), metadata.gid()), (1001, 1001));
            }
            assert_eq!(fs::metadata(fixture.dot_root()).unwrap().uid(), 0);
            assert_eq!(files.mount_path().unwrap(), root);
            files.validate_mount_path(&root).unwrap();
        } else {
            assert!(files.prepare_workload_owner(1001, 1001).is_err());
            assert_eq!(files.mount_path().unwrap(), root);
            files.validate_mount_path(&root).unwrap();
            let after = fs::metadata(&root).unwrap();
            assert_eq!((before.uid(), before.gid()), (after.uid(), after.gid()));
        }
    }

    #[test]
    fn workspace_probe_uses_private_non_dot_storage_and_cleans_only_known_entries() {
        let fixture = Fixture::new();
        let probe = WorkspaceProbe::create(&fixture.0).unwrap();
        let files = probe.mount_path().unwrap();
        assert!(files.starts_with(fixture.0.join(".workspace-probes")));
        assert!(!fixture.0.join("dots").exists());
        assert_eq!(
            fs::metadata(files.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        fs::create_dir(files.join("private")).unwrap();
        fs::write(files.join("private/probe.bin"), b"nonce").unwrap();
        assert_eq!(probe.read("private/probe.bin").unwrap(), b"nonce");
        assert!(probe.metadata("private").unwrap().is_dir());
        assert!(probe.metadata("private/probe.bin").unwrap().is_file());
        assert_eq!(probe.metadata("private/probe.bin").unwrap().len(), 5);
        probe.cleanup().unwrap();
        assert!(!files.parent().unwrap().exists());
    }

    #[test]
    fn workspace_probe_retains_unknown_entries_and_rejects_substituted_mount_paths() {
        let fixture = Fixture::new();
        let probe = WorkspaceProbe::create(&fixture.0).unwrap();
        let files = probe.mount_path().unwrap();
        fs::write(files.join("unexpected"), b"retained").unwrap();
        assert!(probe.cleanup().is_err());
        assert_eq!(fs::read(files.join("unexpected")).unwrap(), b"retained");
        let original = files.with_file_name("old-files");
        fs::rename(&files, &original).unwrap();
        fs::create_dir(&files).unwrap();
        assert!(probe.validate_mount_path(&files).is_err());
        assert!(probe.mount_path().is_err());
        assert!(probe.cleanup().is_err());
        assert_eq!(fs::read(original.join("unexpected")).unwrap(), b"retained");
    }

    #[test]
    fn workspace_probe_reuses_bounded_nofollow_reads_and_preserves_external_files() {
        let fixture = Fixture::new();
        let outside = Fixture::new();
        let probe = WorkspaceProbe::create(&fixture.0).unwrap();
        let files = probe.mount_path().unwrap();
        fs::write(outside.0.join("probe.bin"), b"outside").unwrap();
        symlink(&outside.0, files.join("private")).unwrap();
        assert!(probe.read("private/probe.bin").is_err());
        assert!(probe.metadata("private").is_err());
        assert!(probe.cleanup().is_err());
        assert_eq!(fs::read(outside.0.join("probe.bin")).unwrap(), b"outside");
        fs::remove_file(files.join("private")).unwrap();
        let large = fs::File::create(files.join("large")).unwrap();
        large.set_len(STAGE_TOTAL_MAX_BYTES as u64 + 1).unwrap();
        assert!(probe.read("large").is_err());
        assert!(probe.read("../owner.json").is_err());
        assert!(probe.metadata("../owner.json").is_err());
    }

    #[test]
    fn workspace_probe_ownership_preparation_uses_actual_supported_process_authority() {
        use std::os::unix::fs::MetadataExt;
        let fixture = Fixture::new();
        let probe = WorkspaceProbe::create(&fixture.0).unwrap();
        let files = probe.mount_path().unwrap();
        if cfg!(target_os = "linux") && unsafe { libc::geteuid() } == 0 {
            probe.prepare_workload_owner(1001, 1001).unwrap();
            assert_eq!(
                (
                    fs::metadata(&files).unwrap().uid(),
                    fs::metadata(&files).unwrap().gid()
                ),
                (1001, 1001)
            );
            assert_eq!(fs::metadata(files.parent().unwrap()).unwrap().uid(), 0);
        } else {
            let owner = fs::metadata(&files).unwrap().uid();
            assert!(probe.prepare_workload_owner(1001, 1001).is_err());
            assert_eq!(fs::metadata(&files).unwrap().uid(), owner);
        }
        probe.cleanup().unwrap();
    }

    #[test]
    fn workspace_retirement_persists_owner_and_refuses_delayed_or_held_mounts() {
        let fixture = Fixture::new();
        let identity = fixture.identity();
        let files = Workspace::open(&fixture.0, &identity).unwrap();
        files.write("retained", b"payload").unwrap();
        let path = files.mount_path().unwrap();
        Workspace::seal_retirement(&fixture.0, &identity).unwrap();
        assert!(Workspace::open(&fixture.0, &identity).is_err());
        assert!(files.mount_path().is_err());
        assert!(files.validate_mount_path(&path).is_err());
        assert!(Workspace::cleanup_retired(&fixture.0, &identity).unwrap());
        assert!(!fixture.dot_root().exists());
        let manifest = fixture.0.join(".dot-retired").join(DOT).join("owner.json");
        let saved: DotWorkspace = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
        assert_eq!(saved, identity);
        assert!(!manifest.parent().unwrap().join("payload").exists());
        assert_eq!(
            fs::metadata(&manifest).unwrap().permissions().mode() & 0o777,
            0o600
        );
        Workspace::seal_retirement(&fixture.0, &identity).unwrap();
        assert!(Workspace::cleanup_retired(&fixture.0, &identity).unwrap());
        assert!(Workspace::open(&fixture.0, &identity).is_err());
    }

    #[test]
    fn workspace_retirement_of_absent_storage_never_creates_a_workspace() {
        let fixture = Fixture::new();
        let identity = fixture.identity();
        Workspace::seal_retirement(&fixture.0, &identity).unwrap();
        assert!(!fixture.0.join("dots").exists());
        assert!(Workspace::cleanup_retired(&fixture.0, &identity).unwrap());
        assert!(!fixture.0.join("dots").exists());
        assert!(Workspace::open(&fixture.0, &identity).is_err());
    }

    #[test]
    fn workspace_retirement_rejects_foreign_owners_without_mutation() {
        let fixture = Fixture::new();
        let identity = fixture.identity();
        let files = Workspace::open(&fixture.0, &identity).unwrap();
        files.write("retained", b"original").unwrap();
        let mut foreign = identity.clone();
        foreign.owner_user_id = "foreign".into();
        assert!(Workspace::seal_retirement(&fixture.0, &foreign).is_err());
        assert_eq!(files.read("retained").unwrap(), b"original");
        Workspace::seal_retirement(&fixture.0, &identity).unwrap();
        let manifest = fixture.0.join(".dot-retired").join(DOT).join("owner.json");
        let original = fs::read(&manifest).unwrap();
        assert!(Workspace::seal_retirement(&fixture.0, &foreign).is_err());
        assert!(Workspace::cleanup_retired(&fixture.0, &foreign).is_err());
        assert_eq!(fs::read(&manifest).unwrap(), original);
        assert_eq!(
            fs::read(fixture.dot_root().join("files/retained")).unwrap(),
            b"original"
        );
    }

    #[test]
    fn workspace_retirement_unlinks_hostile_entries_without_following_them() {
        let fixture = Fixture::new();
        let outside = Fixture::new();
        let identity = fixture.identity();
        drop(Workspace::open(&fixture.0, &identity).unwrap());
        let root = fixture.dot_root().join("files");
        fs::write(outside.0.join("secret"), b"outside").unwrap();
        symlink(&outside.0, root.join("linked-directory")).unwrap();
        symlink(outside.0.join("secret"), root.join("linked-file")).unwrap();
        let fifo =
            std::ffi::CString::new(root.join("fifo").as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        let socket = std::os::unix::net::UnixListener::bind(root.join("socket")).unwrap();
        Workspace::seal_retirement(&fixture.0, &identity).unwrap();
        assert!(Workspace::cleanup_retired(&fixture.0, &identity).unwrap());
        assert_eq!(fs::read(outside.0.join("secret")).unwrap(), b"outside");
        drop(socket);
    }

    #[test]
    fn workspace_retirement_large_tree_makes_bounded_restartable_progress() {
        let fixture = Fixture::new();
        let identity = fixture.identity();
        drop(Workspace::open(&fixture.0, &identity).unwrap());
        let root = fixture.dot_root().join("files");
        for index in 0..1500 {
            fs::write(root.join(format!("entry-{index}")), b"data").unwrap();
        }
        Workspace::seal_retirement(&fixture.0, &identity).unwrap();
        assert!(!Workspace::cleanup_retired(&fixture.0, &identity).unwrap());
        let payload = fixture.0.join(".dot-retired").join(DOT).join("payload");
        let remaining = fs::read_dir(payload.join("files")).unwrap().count();
        assert!(remaining > 0 && remaining < 1500);
        assert!(1500 - remaining <= 1000);
        assert!(Workspace::cleanup_retired(&fixture.0, &identity).unwrap());
        assert!(Workspace::cleanup_retired(&fixture.0, &identity).unwrap());
    }

    #[test]
    fn workspace_retirement_rejects_malformed_manifest_and_excessive_depth() {
        let fixture = Fixture::new();
        let identity = fixture.identity();
        drop(Workspace::open(&fixture.0, &identity).unwrap());
        fs::write(fixture.dot_root().join("owner.json"), b"malformed").unwrap();
        assert!(Workspace::seal_retirement(&fixture.0, &identity).is_err());
        fs::write(
            fixture.dot_root().join("owner.json"),
            serde_json::to_vec(&identity).unwrap(),
        )
        .unwrap();
        let mut directory = fixture.dot_root().join("files");
        for _ in 0..130 {
            directory = directory.join("d");
            fs::create_dir(&directory).unwrap();
        }
        Workspace::seal_retirement(&fixture.0, &identity).unwrap();
        assert!(Workspace::cleanup_retired(&fixture.0, &identity).is_err());
        assert!(
            fixture
                .0
                .join(".dot-retired")
                .join(DOT)
                .join("owner.json")
                .exists()
        );
        assert!(Workspace::open(&fixture.0, &identity).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn workspace_retirement_removes_raw_non_utf8_names() {
        use std::os::unix::ffi::OsStringExt;
        let fixture = Fixture::new();
        let identity = fixture.identity();
        drop(Workspace::open(&fixture.0, &identity).unwrap());
        fs::write(
            fixture
                .dot_root()
                .join("files")
                .join(std::ffi::OsString::from_vec(vec![0xff])),
            b"data",
        )
        .unwrap();
        Workspace::seal_retirement(&fixture.0, &identity).unwrap();
        assert!(Workspace::cleanup_retired(&fixture.0, &identity).unwrap());
    }
}
