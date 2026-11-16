//! The `designs/` scan: what it lists, what it refuses to follow and what it
//! reports instead of silently showing an empty project.

use super::*;
use std::sync::mpsc;
use std::time::Duration;

fn temp_root(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "openpencil-scan-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir.canonicalize().unwrap()
}

fn touch(dir: &Path, name: &str) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, "{}").unwrap();
    path
}

fn names(listing: &DesignListing) -> Vec<String> {
    listing
        .files
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect()
}

/// Run `scan_design_dir` on another thread so a hang fails the test instead of
/// freezing it.
fn scan_with_deadline(root: &Path) -> DesignListing {
    let (sender, receiver) = mpsc::channel();
    let root = root.to_path_buf();
    std::thread::spawn(move || {
        let _ = sender.send(scan_design_dir(&root));
    });
    receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("the scan finished")
}

#[test]
fn only_visible_op_files_are_listed_in_sorted_order() {
    let root = temp_root("kinds");
    let designs = root.join("designs");
    touch(&designs, "b.op");
    touch(&designs, "a.op");
    touch(&designs, ".hidden.op");
    touch(&designs, "notes.txt");
    std::fs::create_dir_all(designs.join("folder.op")).unwrap();
    let listing = scan_design_dir(&root);
    assert_eq!(names(&listing), ["a.op", "b.op"]);
    assert_eq!(listing.status, ScanStatus::Ok);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_missing_designs_folder_is_an_empty_ok_listing() {
    let root = temp_root("missing");
    let listing = scan_design_dir(&root);
    assert!(listing.files.is_empty());
    assert_eq!(listing.status, ScanStatus::Ok);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn listing_is_capped_and_reports_truncation() {
    let root = temp_root("cap");
    let designs = root.join("designs");
    let total = MAX_LISTED_FILES + 20;
    for number in 0..total {
        touch(&designs, &format!("f{number:04}.op"));
    }
    let listing = scan_design_dir(&root);
    assert_eq!(listing.files.len(), MAX_LISTED_FILES);
    assert_eq!(listing.status, ScanStatus::Truncated(20));
    let listed = names(&listing);
    assert_eq!(listed.first().map(String::as_str), Some("f0000.op"));
    assert_eq!(
        listed.last().map(String::as_str),
        Some("f0499.op"),
        "the cap keeps the first names in sorted order"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
mod links {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn symlinked_designs_dir_outside_the_project_is_ignored() {
        let root = temp_root("out-project");
        let elsewhere = temp_root("out-elsewhere");
        touch(&elsewhere, "secret.op");
        symlink(&elsewhere, root.join("designs")).unwrap();
        assert_eq!(inspect_designs_dir(&root), DesignsDir::Outside);
        let listing = scan_design_dir(&root);
        assert!(listing.files.is_empty());
        assert_eq!(listing.status, ScanStatus::Ok);
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(elsewhere);
    }

    #[test]
    fn symlinked_designs_dir_inside_the_project_is_listed() {
        let root = temp_root("in-project");
        touch(&root.join("real"), "a.op");
        symlink(root.join("real"), root.join("designs")).unwrap();
        assert_eq!(names(&scan_design_dir(&root)), ["a.op"]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn symlinked_files_are_kept_only_when_they_resolve_inside_the_project() {
        let root = temp_root("file-links");
        let outside = temp_root("file-links-out");
        let designs = root.join("designs");
        let inside_target = touch(&root.join("shared"), "kept-target.op");
        let outside_target = touch(&outside, "leak.op");
        std::fs::create_dir_all(&designs).unwrap();
        symlink(&inside_target, designs.join("inside.op")).unwrap();
        symlink(&outside_target, designs.join("outside.op")).unwrap();
        symlink(root.join("shared"), designs.join("dir-link.op")).unwrap();
        assert_eq!(names(&scan_design_dir(&root)), ["inside.op"]);
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(outside);
    }

    #[test]
    fn symlink_loop_does_not_hang() {
        let root = temp_root("loop-files");
        let designs = root.join("designs");
        std::fs::create_dir_all(&designs).unwrap();
        symlink(designs.join("self.op"), designs.join("self.op")).unwrap();
        symlink(designs.join("b.op"), designs.join("a.op")).unwrap();
        symlink(designs.join("a.op"), designs.join("b.op")).unwrap();
        touch(&designs, "real.op");
        let listing = scan_with_deadline(&root);
        assert_eq!(names(&listing), ["real.op"]);

        // `designs` itself part of a loop.
        let looped = temp_root("loop-dir");
        symlink(looped.join("other"), looped.join("designs")).unwrap();
        symlink(looped.join("designs"), looped.join("other")).unwrap();
        let listing = scan_with_deadline(&looped);
        assert!(listing.files.is_empty());
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(looped);
    }

    #[test]
    fn unreadable_designs_dir_is_reported_not_empty() {
        use std::os::unix::fs::PermissionsExt;
        let root = temp_root("unreadable");
        let designs = root.join("designs");
        touch(&designs, "a.op");
        std::fs::set_permissions(&designs, std::fs::Permissions::from_mode(0o000)).unwrap();
        let listing = scan_design_dir(&root);
        let readable_anyway = std::fs::read_dir(&designs).is_ok();
        std::fs::set_permissions(&designs, std::fs::Permissions::from_mode(0o755)).unwrap();
        if readable_anyway {
            // Running as root: permissions do not apply, nothing to check.
            let _ = std::fs::remove_dir_all(root);
            return;
        }
        assert!(listing.files.is_empty());
        assert_eq!(
            listing.status,
            ScanStatus::Unreadable(ErrorKind::PermissionDenied)
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
