//! Save-conflict prompt and autosave reason keys across every locale table.

#[test]
fn save_conflict_keys_exist_in_every_locale_and_name_the_file() {
    const KEYS: [&str; 4] = [
        "dialog.saveConflictTitle",
        "dialog.saveConflictChangedBody",
        "dialog.saveConflictUnknownBody",
        "dialog.autosaveChangedOnDisk",
    ];
    // The title is shared by both prompts and names no file.
    const NAMES_THE_FILE: [&str; 3] = [
        "dialog.saveConflictChangedBody",
        "dialog.saveConflictUnknownBody",
        "dialog.autosaveChangedOnDisk",
    ];
    type Lookup = fn(&str) -> Option<&'static str>;
    let tables: [(&str, Lookup); 15] = [
        ("en", super::en::lookup),
        ("zh_cn", super::zh_cn::lookup),
        ("zh_tw", super::zh_tw::lookup),
        ("ja", super::ja::lookup),
        ("ko", super::ko::lookup),
        ("fr", super::fr::lookup),
        ("es", super::es::lookup),
        ("de", super::de::lookup),
        ("pt", super::pt::lookup),
        ("ru", super::ru::lookup),
        ("hi", super::hi::lookup),
        ("tr", super::tr::lookup),
        ("th", super::th::lookup),
        ("vi", super::vi::lookup),
        ("id", super::id::lookup),
    ];
    for (name, lookup) in tables {
        for key in KEYS {
            let text = lookup(key).unwrap_or_else(|| panic!("`{name}` is missing `{key}`"));
            assert!(!text.trim().is_empty(), "`{name}` has an empty `{key}`");
        }
        for key in NAMES_THE_FILE {
            assert!(
                lookup(key).is_some_and(|text| text.contains("{{name}}")),
                "`{name}` / `{key}` must carry {{{{name}}}}"
            );
        }
    }
}
