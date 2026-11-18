//! What it takes to make one file's row visible in the Projects dock.
//!
//! The dock hides a file when its project is folded or any folder above it is
//! folded, and when the whole dock is collapsed. The folds are host-owned flags
//! (the host flips them and mirrors the rows back), so revealing a file is a
//! plan the host carries out: this module computes it from the mirrored rows,
//! without touching them.

use crate::project_tree::{in_folder_tree, ProjectView};

/// The folds to open so the row of one file shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevealPlan {
    /// Root of the project that holds the file.
    pub project_root: String,
    /// The project is folded and must be unfolded.
    pub unfold_project: bool,
    /// Folded folders (relative to the project root) that are the file's
    /// folder or sit above it: all of them must be unfolded.
    pub unfold_folders: Vec<String>,
}

/// The plan to reveal the file at `path`, or `None` when no project lists it
/// (the file is gone, or the rows have not caught up yet).
pub fn reveal_plan(projects: &[ProjectView], path: &str) -> Option<RevealPlan> {
    projects.iter().find_map(|project| {
        let file = project.files.iter().find(|file| file.path == path)?;
        let mut unfold_folders: Vec<String> = project
            .collapsed_folders
            .iter()
            .filter(|folded| in_folder_tree(&file.folder, Some(folded.as_str())))
            .cloned()
            .collect();
        unfold_folders.sort();
        Some(RevealPlan {
            project_root: project.root.clone(),
            unfold_project: !project.expanded,
            unfold_folders,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_tabs::DocumentTabStatus;
    use crate::project_tree::{ProjectFileKind, ProjectFileView};

    fn file(path: &str, folder: &str) -> ProjectFileView {
        ProjectFileView {
            path: path.to_string(),
            label: path.rsplit('/').next().unwrap_or(path).to_string(),
            open_id: None,
            dirty: false,
            status: DocumentTabStatus::Idle,
            active: false,
            folder: folder.to_string(),
            kind: ProjectFileKind::Design,
            missing: false,
            removable: false,
        }
    }

    fn project(expanded: bool, folded: &[&str]) -> ProjectView {
        ProjectView {
            root: "/work/shop".to_string(),
            name: "shop".to_string(),
            files: vec![
                file("/work/shop/a.op", ""),
                file("/work/shop/landing/v2/hero.op", "landing/v2"),
            ],
            expanded,
            collapsed_folders: folded.iter().map(|folder| folder.to_string()).collect(),
            ..ProjectView::default()
        }
    }

    #[test]
    fn nothing_folded_needs_nothing_unfolded() {
        let plan = reveal_plan(&[project(true, &[])], "/work/shop/landing/v2/hero.op").unwrap();
        assert!(!plan.unfold_project);
        assert!(plan.unfold_folders.is_empty());
        assert_eq!(plan.project_root, "/work/shop");
    }

    #[test]
    fn a_folded_project_is_unfolded() {
        let plan = reveal_plan(&[project(false, &[])], "/work/shop/a.op").unwrap();
        assert!(plan.unfold_project);
    }

    #[test]
    fn the_folder_and_every_folded_ancestor_are_unfolded_but_not_siblings() {
        let plan = reveal_plan(
            &[project(
                true,
                &["landing", "landing/v2", "other", "landing/v3"],
            )],
            "/work/shop/landing/v2/hero.op",
        )
        .unwrap();
        assert_eq!(plan.unfold_folders, ["landing", "landing/v2"]);
    }

    #[test]
    fn a_file_in_the_root_is_not_hidden_by_a_folded_folder() {
        let plan = reveal_plan(&[project(true, &["landing"])], "/work/shop/a.op").unwrap();
        assert!(plan.unfold_folders.is_empty());
    }

    #[test]
    fn a_file_no_project_lists_has_no_plan() {
        assert_eq!(
            reveal_plan(&[project(true, &[])], "/work/shop/gone.op"),
            None
        );
        assert_eq!(reveal_plan(&[], "/work/shop/a.op"), None);
    }
}
