use super::*;

fn entry(family: &str, resolved: bool) -> MissingFontEntry {
    MissingFontEntry {
        family: family.into(),
        run_count: 1,
        mismatch_note: None,
        resolved,
    }
}

fn ui(supported: bool) -> FontDownloadUi {
    FontDownloadUi {
        supported,
        ..FontDownloadUi::default()
    }
}

#[test]
fn unsupported_hosts_never_offer_a_download() {
    let ui = ui(false);
    assert_eq!(ui.row_action(&entry("Pacifico", false)), None);
    assert_eq!(ui.downloadable_rows(&[entry("Pacifico", false)]), 0);
}

#[test]
fn row_action_follows_the_status() {
    let mut ui = ui(true);
    let row = entry("Pacifico", false);
    assert_eq!(ui.row_action(&row), Some(RowDownloadAction::Download));
    ui.set_status("PACIFICO ", FontDownloadStatus::Ready);
    assert_eq!(ui.row_action(&row), Some(RowDownloadAction::Download));
    ui.set_status("Pacifico", FontDownloadStatus::Downloading);
    assert_eq!(ui.row_action(&row), None);
    assert!(ui.any_downloading());
    ui.set_status("Pacifico", FontDownloadStatus::Downloaded);
    assert_eq!(ui.row_action(&row), None);
    ui.set_status(
        "Pacifico",
        FontDownloadStatus::Failed(FontDownloadFailure::Offline),
    );
    assert_eq!(ui.row_action(&row), Some(RowDownloadAction::Retry));
    for failure in [
        FontDownloadFailure::UnknownFont,
        FontDownloadFailure::LicenseNotAllowed,
        FontDownloadFailure::NotFound,
        FontDownloadFailure::TooLarge,
        FontDownloadFailure::BudgetExceeded,
    ] {
        ui.set_status("Pacifico", FontDownloadStatus::Failed(failure));
        assert_eq!(ui.row_action(&row), None, "{failure:?}");
    }
    // A resolved row has nothing left to download.
    ui.set_status("Pacifico", FontDownloadStatus::Ready);
    assert_eq!(ui.row_action(&entry("Pacifico", true)), None);
}

#[test]
fn every_failure_has_its_own_reason_key() {
    let keys: std::collections::BTreeSet<_> = [
        FontDownloadFailure::UnknownFont,
        FontDownloadFailure::LicenseNotAllowed,
        FontDownloadFailure::Offline,
        FontDownloadFailure::Timeout,
        FontDownloadFailure::NotFound,
        FontDownloadFailure::TooLarge,
        FontDownloadFailure::Invalid,
        FontDownloadFailure::BudgetExceeded,
        FontDownloadFailure::Other,
    ]
    .into_iter()
    .map(FontDownloadFailure::reason_key)
    .collect();
    assert_eq!(keys.len(), 9);
}
