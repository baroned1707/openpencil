use super::top_bar_info_text::elide_middle;

/// 10 px per character, so the arithmetic in the assertions stays readable.
fn ten_per_char(text: &str) -> f32 {
    text.chars().count() as f32 * 10.0
}

#[test]
fn text_that_fits_is_returned_unchanged() {
    assert_eq!(
        elide_middle("storefront", 100.0, ten_per_char),
        "storefront"
    );
}

#[test]
fn both_ends_survive_when_the_text_is_shortened() {
    let shortened = elide_middle("storefront-redesign-v2", 90.0, ten_per_char);
    assert!(shortened.contains('…'), "{shortened}");
    assert!(shortened.starts_with("stor"), "head kept: {shortened}");
    assert!(shortened.ends_with("n-v2"), "tail kept: {shortened}");
    assert!(ten_per_char(&shortened) <= 90.0, "{shortened}");
}

#[test]
fn it_takes_the_widest_candidate_that_fits() {
    // 90 px = 9 characters: 8 letters plus the ellipsis.
    let shortened = elide_middle("abcdefghijklmnop", 90.0, ten_per_char);
    assert_eq!(shortened.chars().count(), 9, "{shortened}");
    assert_eq!(shortened, "abcd…mnop");
}

#[test]
fn nothing_fits_returns_an_empty_string() {
    assert_eq!(elide_middle("storefront", 5.0, ten_per_char), "");
    assert_eq!(elide_middle("storefront", 0.0, ten_per_char), "");
    assert_eq!(elide_middle("", 100.0, ten_per_char), "");
}

#[test]
fn only_the_ellipsis_fits() {
    assert_eq!(elide_middle("storefront", 10.0, ten_per_char), "…");
}

#[test]
fn wide_characters_are_counted_by_the_metric_not_by_length() {
    let wide = |text: &str| {
        text.chars()
            .map(|c| if c.is_ascii() { 8.0 } else { 16.0 })
            .sum()
    };
    let shortened = elide_middle("商店首页设计稿最终版本", 80.0, wide);
    assert!(shortened.contains('…'));
    assert!(wide(&shortened) <= 80.0, "{shortened}");
    assert!(
        shortened.starts_with('商') && shortened.ends_with('本'),
        "{shortened}"
    );
}
