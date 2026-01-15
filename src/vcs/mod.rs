//! VCS abstraction layer for supporting multiple version control systems.
//!
//! Currently supports:
//! - Git (always enabled)
//! - Mercurial (optional, via `hg` feature flag)
//! - Jujutsu (optional, via `jj` feature flag)
//!
//! ## Detection Order
//!
//! When auto-detecting the VCS type, Jujutsu is tried first (if enabled)
//! because jj repos are Git-backed and contain a `.git` directory. If jj
//! detection fails, Git is tried next, then Mercurial (if enabled).

pub mod git;
#[cfg(feature = "hg")]
mod hg;
#[cfg(feature = "jj")]
mod jj;
mod traits;

pub use git::GitBackend;
#[cfg(feature = "hg")]
pub use hg::HgBackend;
#[cfg(feature = "jj")]
pub use jj::JjBackend;
pub use traits::{CommitInfo, VcsBackend, VcsInfo};

use crate::error::{Result, TuicrError};

/// Detect the VCS type and return the appropriate backend.
///
/// Detection order: Jujutsu (if enabled) → Git → Mercurial (if enabled).
/// Jujutsu is tried first because jj repos are Git-backed.
pub fn detect_vcs() -> Result<Box<dyn VcsBackend>> {
    // Try jj first since jj repos are Git-backed
    #[cfg(feature = "jj")]
    if let Ok(backend) = JjBackend::discover() {
        return Ok(Box::new(backend));
    }

    // Try git
    if let Ok(backend) = GitBackend::discover() {
        return Ok(Box::new(backend));
    }

    // Try hg if feature is enabled
    #[cfg(feature = "hg")]
    if let Ok(backend) = HgBackend::discover() {
        return Ok(Box::new(backend));
    }

    Err(TuicrError::NotARepository)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vcs::traits::VcsType;
    use std::path::PathBuf;

    #[test]
    fn exports_are_accessible() {
        // Verify that public types are properly exported
        let _: fn() -> Result<Box<dyn VcsBackend>> = detect_vcs;

        // VcsInfo can be constructed
        let info = VcsInfo {
            root_path: PathBuf::from("/test"),
            head_commit: "abc".to_string(),
            branch_name: None,
            vcs_type: VcsType::Git,
        };
        assert_eq!(info.head_commit, "abc");

        // CommitInfo can be constructed
        let commit = CommitInfo {
            id: "abc".to_string(),
            short_id: "abc".to_string(),
            summary: "test".to_string(),
            author: "author".to_string(),
            time: chrono::Utc::now(),
        };
        assert_eq!(commit.id, "abc");
    }

    #[test]
    fn detect_vcs_outside_repo_returns_error() {
        // When run outside any VCS repo, should return NotARepository
        // Note: This test may pass or fail depending on where tests are run
        // In CI or outside a repo, it should fail with NotARepository
        // Inside the tuicr repo (which is git), it will succeed
        let result = detect_vcs();

        // We just verify the function runs without panic
        // The actual result depends on the environment
        match result {
            Ok(backend) => {
                // If we're in a repo, we should get valid info
                let info = backend.info();
                assert!(!info.head_commit.is_empty());
            }
            Err(TuicrError::NotARepository) => {
                // Expected when outside a repo
            }
            Err(e) => {
                panic!("Unexpected error: {:?}", e);
            }
        }
    }

    #[cfg(feature = "hg")]
    #[test]
    fn detect_vcs_finds_hg_repo() {
        use std::fs;
        use std::process::Command;

        // Check if hg is available
        let hg_available = Command::new("hg")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);

        if !hg_available {
            eprintln!("Skipping test: hg command not available");
            return;
        }

        // Create a temp hg-only repo (no git)
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let root = temp_dir.path();

        // Initialize hg repo
        Command::new("hg")
            .args(["init"])
            .current_dir(root)
            .output()
            .expect("Failed to init hg repo");

        // Create and commit a file
        fs::write(root.join("test.txt"), "test\n").expect("Failed to write file");
        Command::new("hg")
            .args(["add", "test.txt"])
            .current_dir(root)
            .output()
            .expect("Failed to add file");
        Command::new("hg")
            .args(["commit", "-m", "Initial commit"])
            .current_dir(root)
            .output()
            .expect("Failed to commit");

        // Change to the temp dir and detect VCS
        std::env::set_current_dir(root).unwrap();

        let backend = detect_vcs().expect("Should detect hg repo");
        let info = backend.info();

        assert_eq!(info.vcs_type, VcsType::Mercurial);
        assert_eq!(info.root_path, root);
    }

    #[cfg(feature = "jj")]
    #[test]
    fn detect_vcs_finds_jj_repo() {
        use std::fs;
        use std::process::Command;

        // Check if jj is available
        let jj_available = Command::new("jj")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);

        if !jj_available {
            eprintln!("Skipping test: jj command not available");
            return;
        }

        // Create a temp jj repo
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let root = temp_dir.path();

        // Initialize jj repo (git-backed)
        let output = Command::new("jj")
            .args(["git", "init"])
            .current_dir(root)
            .output()
            .expect("Failed to init jj repo");

        if !output.status.success() {
            eprintln!(
                "Skipping test: jj git init failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }

        // Create and commit a file
        fs::write(root.join("test.txt"), "test\n").expect("Failed to write file");
        Command::new("jj")
            .args(["commit", "-m", "Initial commit"])
            .current_dir(root)
            .output()
            .expect("Failed to commit");

        // Change to the temp dir and detect VCS
        std::env::set_current_dir(root).unwrap();

        let backend = detect_vcs().expect("Should detect jj repo");
        let info = backend.info();

        // jj should be detected before git since jj repos are git-backed
        assert_eq!(info.vcs_type, VcsType::Jujutsu);
        assert_eq!(info.root_path, root);
    }
}
