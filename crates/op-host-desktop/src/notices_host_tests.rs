use super::*;

fn only(existing: &'static str) -> impl Fn(&Path) -> bool {
    move |path| path == Path::new(existing)
}

#[test]
fn each_package_layout_resolves_to_its_staged_file() {
    // Release archive and Windows installer: next to the program.
    assert_eq!(
        resolve_notices_target(
            Path::new("/opt/openpencil/openpencil-desktop"),
            only("/opt/openpencil/notices/THIRD_PARTY_NOTICES.md")
        ),
        NoticesTarget::File(PathBuf::from(
            "/opt/openpencil/notices/THIRD_PARTY_NOTICES.md"
        ))
    );
    // macOS bundle.
    assert_eq!(
        resolve_notices_target(
            Path::new("/Applications/OpenPencil.app/Contents/MacOS/openpencil-desktop"),
            only("/Applications/OpenPencil.app/Contents/Resources/notices/THIRD_PARTY_NOTICES.md")
        ),
        NoticesTarget::File(PathBuf::from(
            "/Applications/OpenPencil.app/Contents/Resources/notices/THIRD_PARTY_NOTICES.md"
        ))
    );
    // Linux package and AppImage (`usr/bin` next to `usr/share`).
    assert_eq!(
        resolve_notices_target(
            Path::new("/usr/bin/openpencil-desktop"),
            only("/usr/share/doc/openpencil/notices/THIRD_PARTY_NOTICES.md")
        ),
        NoticesTarget::File(PathBuf::from(
            "/usr/share/doc/openpencil/notices/THIRD_PARTY_NOTICES.md"
        ))
    );
}

#[test]
fn a_build_without_staged_notices_falls_back_to_the_repository_file() {
    let target =
        resolve_notices_target(Path::new("/work/target/release/openpencil-desktop"), |_| {
            false
        });
    match target {
        NoticesTarget::Url(url) => {
            assert!(url.starts_with("https://github.com/"));
            assert!(url.ends_with("/packaging/shared/THIRD_PARTY_NOTICES.md"));
        }
        other => panic!("expected the URL fallback, got {other:?}"),
    }
    // An executable path with no parent cannot hold notices either.
    assert!(matches!(
        resolve_notices_target(Path::new("/"), |_| true),
        NoticesTarget::Url(_)
    ));
}

#[test]
fn the_resolved_path_has_no_parent_components() {
    let target = resolve_notices_target(
        Path::new("/App/Contents/MacOS/openpencil-desktop"),
        |path| path.ends_with("Resources/notices/THIRD_PARTY_NOTICES.md"),
    );
    let NoticesTarget::File(path) = target else {
        panic!("expected a file");
    };
    assert!(!path.to_string_lossy().contains(".."), "{path:?}");
}

#[test]
fn the_first_existing_candidate_wins() {
    let target = resolve_notices_target(Path::new("/x/bin/openpencil-desktop"), |_| true);
    assert_eq!(
        target,
        NoticesTarget::File(PathBuf::from("/x/bin/notices/THIRD_PARTY_NOTICES.md"))
    );
}
