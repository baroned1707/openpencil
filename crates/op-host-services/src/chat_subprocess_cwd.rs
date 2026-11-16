//! Working-directory policy for subprocess chat providers.
//!
//! A Finder-launched desktop app has cwd `/`, so a CLI agent would not find the
//! open document's project (`AGENTS.md`, relative paths). Codex is therefore
//! started in the document's project root. Every other CLI keeps the directory
//! it already had: Antigravity and Grok Build run in a private per-turn temp
//! dir ON PURPOSE (containment, see `chat_subprocess_safety`), Kimi in its
//! fixed empty generation workspace, and the rest inherit the process cwd.

use std::path::{Path, PathBuf};

use op_ai::chat_provider::CliName;

use super::SubprocessProvider;

impl SubprocessProvider {
    /// Start this provider's CLI in `cwd` (the document's project root) when
    /// the CLI is allowed to leave the process cwd (Codex only). `None` keeps
    /// today's behaviour.
    pub fn with_project_cwd(mut self, cwd: Option<PathBuf>) -> Self {
        self.project_cwd = cwd;
        self
    }
}

/// The directory to run the child in, or `None` to inherit the process's.
///
/// Precedence is containment first: an isolated per-turn dir always wins, then
/// Kimi's generation workspace, and only then the project root, which is
/// honoured for Codex alone.
pub(super) fn child_cwd(
    cli: Option<CliName>,
    isolated_cwd: Option<&Path>,
    project_cwd: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(dir) = isolated_cwd {
        return Some(dir.to_path_buf());
    }
    match cli {
        Some(CliName::Kimi) => Some(crate::chat_subprocess_kimi::workspace_dir()),
        Some(CliName::Codex) => project_cwd.map(Path::to_path_buf),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat_subprocess_safety::IsolatedTurn;

    const PROJECT: &str = "/projects/demo";

    #[test]
    fn codex_runs_in_the_project_root_only_when_given_one() {
        let project = Path::new(PROJECT);
        assert_eq!(
            child_cwd(Some(CliName::Codex), None, Some(project)),
            Some(project.to_path_buf())
        );
        assert_eq!(child_cwd(Some(CliName::Codex), None, None), None);
    }

    #[test]
    fn isolated_providers_keep_their_temp_dir() {
        let project = Path::new(PROJECT);
        // Real isolation: Grok prepares a private per-turn directory.
        let turn = IsolatedTurn::prepare(Some(CliName::GrokBuild), "hello", &[])
            .unwrap()
            .expect("Grok runs in an isolated turn");
        assert_eq!(
            child_cwd(Some(CliName::GrokBuild), Some(turn.cwd()), Some(project)),
            Some(turn.cwd().to_path_buf())
        );
        assert_ne!(turn.cwd(), project);
        // Antigravity is isolated the same way; the policy only sees the dir.
        let temp = std::env::temp_dir().join("antigravity-turn");
        assert_eq!(
            child_cwd(Some(CliName::Antigravity), Some(&temp), Some(project)),
            Some(temp)
        );
    }

    #[test]
    fn non_codex_clis_ignore_the_project_root() {
        let project = Path::new(PROJECT);
        for cli in [
            None,
            Some(CliName::Dsh),
            Some(CliName::OpenCode),
            Some(CliName::Copilot),
            Some(CliName::ClaudeCode),
        ] {
            assert_eq!(child_cwd(cli, None, Some(project)), None, "{cli:?}");
        }
        assert_eq!(
            child_cwd(Some(CliName::Kimi), None, Some(project)),
            Some(crate::chat_subprocess_kimi::workspace_dir())
        );
    }

    #[test]
    fn with_project_cwd_stores_the_directory() {
        let project = PathBuf::from(PROJECT);
        let provider = SubprocessProvider::for_cli(CliName::Codex)
            .expect("codex provider")
            .with_project_cwd(Some(project.clone()));
        assert_eq!(provider.project_cwd, Some(project));
    }
}
