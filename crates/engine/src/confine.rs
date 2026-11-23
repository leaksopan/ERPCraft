//! Confined file access for headless automation and MCP (Craft RFC-0015).
//!
//! When confinement is active, every filesystem read must resolve inside an allowed read or write
//! root, and every filesystem write must resolve inside an allowed write root. Paths are resolved
//! through symlinks, junctions, `..` segments and 8.3 short names before comparing path components
//! so sibling-prefix paths (e.g. `/sandbox-other` vs `/sandbox`) and traversal escapes are refused.

use std::path::{Component, Path, PathBuf};

use crate::error::{EngineError, Result};

/// Allowed read and write directories for a confined session.
#[derive(Clone, Debug, Default)]
pub struct Confine {
    read_roots: Vec<PathBuf>,
    write_roots: Vec<PathBuf>,
}

impl Confine {
    /// Builds a confinement policy from raw directory paths. Every root must already exist and be
    /// a directory; missing or non-directory roots fail immediately at startup.
    pub fn new(read_roots: &[PathBuf], write_roots: &[PathBuf]) -> Result<Self> {
        let mut reads = Vec::with_capacity(read_roots.len());
        for p in read_roots {
            reads.push(validate_root("--automation-read-root", p)?);
        }
        let mut writes = Vec::with_capacity(write_roots.len());
        for p in write_roots {
            writes.push(validate_root("--automation-write-root", p)?);
        }
        Ok(Self { read_roots: reads, write_roots: writes })
    }

    /// Requires at least one read root and at least one write root (mandatory for agent entry
    /// points such as `erpcraft-cli mcp`).
    pub fn require_both(read_roots: &[PathBuf], write_roots: &[PathBuf]) -> Result<Self> {
        if read_roots.is_empty() {
            return Err(EngineError::refused("confined mode requires at least one --automation-read-root"));
        }
        if write_roots.is_empty() {
            return Err(EngineError::refused("confined mode requires at least one --automation-write-root"));
        }
        Self::new(read_roots, write_roots)
    }

    pub fn read_roots(&self) -> &[PathBuf] {
        &self.read_roots
    }

    pub fn write_roots(&self) -> &[PathBuf] {
        &self.write_roots
    }

    /// Verifies that `path` may be read (inside a read root or a write root) and returns the
    /// resolved canonical path.
    pub fn check_read(&self, path: &Path) -> Result<PathBuf> {
        if self.read_roots.is_empty() && self.write_roots.is_empty() {
            return Err(EngineError::refused(format!("reading '{}' is refused: no read root is configured", path.display())));
        }
        let resolved = resolve_existing(path)?;
        if self.read_roots.iter().chain(self.write_roots.iter()).any(|root| inside_root(&resolved, root)) {
            Ok(resolved)
        } else {
            Err(EngineError::refused(format!("access denied: '{}' is outside the allowed read roots", path.display())))
        }
    }

    /// Verifies that `path` may be written (inside a write root) and returns the resolved path.
    /// The file itself does not have to exist yet; its nearest existing ancestor is canonicalized.
    pub fn check_write(&self, path: &Path) -> Result<PathBuf> {
        if self.write_roots.is_empty() {
            return Err(EngineError::refused(format!("writing '{}' is refused: no write root is configured", path.display())));
        }
        let resolved = resolve_for_write(path)?;
        if self.write_roots.iter().any(|root| inside_root(&resolved, root)) {
            Ok(resolved)
        } else {
            Err(EngineError::refused(format!("access denied: '{}' is outside the allowed write roots", path.display())))
        }
    }
}

fn validate_root(flag: &str, path: &Path) -> Result<PathBuf> {
    let meta = std::fs::metadata(path).map_err(|e| EngineError::invalid(format!("{flag} '{}': {e}", path.display())))?;
    if !meta.is_dir() {
        return Err(EngineError::invalid(format!("{flag} '{}': not a directory", path.display())));
    }
    std::fs::canonicalize(path).map_err(|e| EngineError::invalid(format!("{flag} '{}': {e}", path.display())))
}

fn resolve_existing(path: &Path) -> Result<PathBuf> {
    if path.as_os_str().is_empty() {
        return Err(EngineError::invalid("file path must not be empty"));
    }
    std::fs::canonicalize(path).map_err(|e| EngineError::refused(format!("cannot access '{}': {e}", path.display())))
}

fn resolve_for_write(path: &Path) -> Result<PathBuf> {
    if path.as_os_str().is_empty() {
        return Err(EngineError::invalid("file path must not be empty"));
    }
    // Make relative paths absolute against the current working directory first so ancestor lookup
    // always reaches a real directory.
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map_err(|e| EngineError::io(format!("current_dir: {e}")))?.join(path)
    };
    // If the target already exists (even as a symlink), canonicalize it directly so symlinks
    // pointing outside a write root are caught.
    if std::fs::symlink_metadata(&abs).is_ok() {
        return std::fs::canonicalize(&abs).map_err(|e| EngineError::refused(format!("cannot resolve '{}': {e}", path.display())));
    }
    // Walk up until an existing ancestor is found, collecting the non-existent tail components.
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    let mut cur = abs.as_path();
    loop {
        if cur.exists() {
            let mut base = std::fs::canonicalize(cur).map_err(|e| EngineError::refused(format!("cannot resolve '{}': {e}", path.display())))?;
            for part in tail.into_iter().rev() {
                base.push(part);
            }
            return Ok(base);
        }
        let Some(name) = cur.file_name() else {
            return Err(EngineError::refused(format!("cannot resolve ancestor of '{}'", path.display())));
        };
        if name == ".." {
            // Normalize lexical `..` before hitting non-existent segments by rejecting traversal
            // across non-existent directories.
            return Err(EngineError::refused(format!("access denied: '{}' contains parent traversal in a missing path", path.display())));
        }
        if name != "." {
            tail.push(name.to_os_string());
        }
        cur = cur.parent().ok_or_else(|| EngineError::refused(format!("cannot resolve parent of '{}'", path.display())))?;
    }
}

/// Component-wise containment check (case-insensitive on Windows/macOS).
fn inside_root(resolved: &Path, root: &Path) -> bool {
    let mut r_it = root.components();
    let mut p_it = resolved.components();
    loop {
        match (r_it.next(), p_it.next()) {
            (None, _) => return true,
            (Some(_), None) => return false,
            (Some(rc), Some(pc)) => {
                if !component_eq(rc, pc) {
                    return false;
                }
            }
        }
    }
}

fn component_eq(a: Component<'_>, b: Component<'_>) -> bool {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        a.as_os_str().to_string_lossy().eq_ignore_ascii_case(&b.as_os_str().to_string_lossy())
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        a == b
    }
}
