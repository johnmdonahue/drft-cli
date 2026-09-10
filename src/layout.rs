//! Project-owned file layout and legacy-layout validation.

use anyhow::{Context, Result, bail};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

pub const STATE_DIR: &str = ".drft";
pub const CONFIG_FILE: &str = ".drft/config.toml";
pub const LOCK_FILE: &str = ".drft/lock.toml";
pub const LEGACY_CONFIG_FILE: &str = "drft.toml";
pub const LEGACY_LOCK_FILE: &str = "drft.lock";

pub fn config_path(root: &Path) -> PathBuf {
    root.join(CONFIG_FILE)
}

pub fn lock_path(root: &Path) -> PathBuf {
    root.join(LOCK_FILE)
}

pub fn state_dir(root: &Path) -> PathBuf {
    root.join(STATE_DIR)
}

pub fn has_config_marker(root: &Path) -> Result<bool> {
    Ok(current_config_exists(root)? || occupied(&root.join(LEGACY_CONFIG_FILE))?)
}

pub fn current_config_exists(root: &Path) -> Result<bool> {
    validate_state_dir(root)?;
    occupied(&config_path(root))
}

pub fn current_lock_exists(root: &Path) -> Result<bool> {
    validate_state_dir(root)?;
    occupied(&lock_path(root))
}

/// Require project state to live in a real directory beneath the graph root.
/// Following a `.drft` symlink would let a checkout redirect config reads and
/// lock writes outside the selected project.
pub fn validate_state_dir(root: &Path) -> Result<()> {
    let path = state_dir(root);
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_dir() => Ok(()),
        Ok(_) => bail!(
            "unsafe drft state path: `{STATE_DIR}` must be a directory within the project, not a symlink or other file"
        ),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("failed to inspect {}", path.display())),
    }
}

/// Reject every legacy or conflicting layout before a command reads or writes
/// project state. Paths count as occupied even when they are directories or
/// dangling symlinks, so migration guidance never treats an unsafe destination
/// as available.
pub fn reject_legacy(root: &Path) -> Result<()> {
    validate_state_dir(root)?;
    let current_config = occupied(&config_path(root))?;
    let current_lock = occupied(&lock_path(root))?;
    let legacy_config = occupied(&root.join(LEGACY_CONFIG_FILE))?;
    let legacy_lock = occupied(&root.join(LEGACY_LOCK_FILE))?;

    if !legacy_config && !legacy_lock {
        return Ok(());
    }

    let mut actions = Vec::new();
    if legacy_config {
        if current_config {
            actions.push(format!(
                "both `{LEGACY_CONFIG_FILE}` and `{CONFIG_FILE}` exist; reconcile them manually because drft will not choose or overwrite either"
            ));
        } else {
            actions.push(format!(
                "move `{LEGACY_CONFIG_FILE}` to `{CONFIG_FILE}` (create `{STATE_DIR}` first)"
            ));
        }
    }
    if legacy_lock {
        if current_lock {
            actions.push(format!(
                "both `{LEGACY_LOCK_FILE}` and `{LOCK_FILE}` exist; reconcile them manually because drft will not choose or overwrite either"
            ));
        } else {
            actions.push(format!(
                "move `{LEGACY_LOCK_FILE}` to `{LOCK_FILE}` (create `{STATE_DIR}` first)"
            ));
        }
    }

    bail!(
        "legacy drft project files require manual migration:\n- {}\ndrft did not modify any files; rerun the command after resolving every item",
        actions.join("\n- ")
    )
}

fn occupied(path: &Path) -> Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| format!("failed to inspect {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create(path: &Path) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, "sentinel").unwrap();
    }

    #[test]
    fn current_layout_is_accepted() {
        let dir = TempDir::new().unwrap();
        create(&config_path(dir.path()));
        create(&lock_path(dir.path()));
        reject_legacy(dir.path()).unwrap();
    }

    #[test]
    fn legacy_files_are_all_named_in_one_migration_error() {
        let dir = TempDir::new().unwrap();
        create(&dir.path().join(LEGACY_CONFIG_FILE));
        create(&dir.path().join(LEGACY_LOCK_FILE));

        let error = reject_legacy(dir.path()).unwrap_err().to_string();
        assert!(error.contains("`drft.toml` to `.drft/config.toml`"));
        assert!(error.contains("`drft.lock` to `.drft/lock.toml`"));
        assert!(error.contains("did not modify"));
    }

    #[test]
    fn coexistence_reports_every_conflict() {
        let dir = TempDir::new().unwrap();
        for path in [
            config_path(dir.path()),
            lock_path(dir.path()),
            dir.path().join(LEGACY_CONFIG_FILE),
            dir.path().join(LEGACY_LOCK_FILE),
        ] {
            create(&path);
        }

        let error = reject_legacy(dir.path()).unwrap_err().to_string();
        assert!(error.contains("both `drft.toml` and `.drft/config.toml` exist"));
        assert!(error.contains("both `drft.lock` and `.drft/lock.toml` exist"));
    }

    #[cfg(unix)]
    #[test]
    fn dangling_legacy_symlink_is_not_treated_as_absent() {
        let dir = TempDir::new().unwrap();
        std::os::unix::fs::symlink("missing", dir.path().join(LEGACY_LOCK_FILE)).unwrap();
        let error = reject_legacy(dir.path()).unwrap_err().to_string();
        assert!(error.contains("`drft.lock` to `.drft/lock.toml`"));
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_cannot_stand_in_for_the_state_directory() {
        let dir = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        std::os::unix::fs::symlink(outside.path(), state_dir(dir.path())).unwrap();

        let error = reject_legacy(dir.path()).unwrap_err().to_string();
        assert!(error.contains("`.drft` must be a directory within the project"));
    }
}
