use super::*;

fn tier(query: &str, text: &str) -> Option<MatchTier> {
    score(query, text).map(|score| score.tier)
}

#[test]
fn ranking_order_is_exact_then_prefix_then_word_start_then_subsequence() {
    assert_eq!(
        tier("cart", "cart.op"),
        Some(MatchTier::Exact),
        "extension aside"
    );
    assert_eq!(tier("car", "cart.op"), Some(MatchTier::Prefix));
    assert_eq!(tier("card", "product-card.op"), Some(MatchTier::WordStart));
    assert_eq!(
        tier("card", "productCard.op"),
        Some(MatchTier::WordStart),
        "camelCase"
    );
    assert_eq!(tier("chk", "checkout.op"), Some(MatchTier::Subsequence));
    assert!(MatchTier::Exact < MatchTier::Prefix);
    assert!(MatchTier::Prefix < MatchTier::WordStart);
    assert!(MatchTier::WordStart < MatchTier::Subsequence);
}

#[test]
fn a_prefix_beats_a_word_start_beats_a_scattered_match() {
    let query = "pa";
    let prefix = score(query, "pay-now.op").unwrap();
    let word = score(query, "checkout-pay.op").unwrap();
    let scattered = score(query, "pricing-table.op").unwrap();
    assert!(
        prefix < word && word < scattered,
        "{prefix:?} {word:?} {scattered:?}"
    );
}

#[test]
fn a_tighter_subsequence_beats_a_looser_one() {
    let tight = score("ct", "cart.op").unwrap();
    let loose = score("ct", "c-a-r-t-t.op").unwrap();
    assert!(tight.gaps < loose.gaps && tight < loose);
}

#[test]
fn case_and_whitespace_in_the_query_do_not_matter() {
    assert_eq!(tier("CARD", "product-card.op"), Some(MatchTier::WordStart));
    assert_eq!(
        tier("  Card ", "product-card.op"),
        Some(MatchTier::WordStart)
    );
    assert_eq!(
        tier("prod card", "product-card.op"),
        Some(MatchTier::Subsequence)
    );
}

#[test]
fn letters_out_of_order_or_missing_do_not_match() {
    assert_eq!(score("tc", "cart.op"), None);
    assert_eq!(score("xyz", "cart.op"), None);
    assert_eq!(score("carts", "cart.op"), None);
}

#[test]
fn the_extension_is_not_part_of_the_match() {
    assert_eq!(score("op", "cart.op"), None);
    assert_eq!(tier("op", "op-ui.op"), Some(MatchTier::Prefix));
    assert_eq!(
        tier("cart.op", "cart.op"),
        Some(MatchTier::Exact),
        "a typed extension is dropped"
    );
    assert_eq!(tier("car.op", "cart.op"), Some(MatchTier::Prefix));
}

#[test]
fn an_empty_query_matches_everything_equally() {
    let a = score("", "zebra.op").unwrap();
    let b = score("  ", "apple.op").unwrap();
    assert_eq!(a, b);
}

#[test]
fn non_ascii_names_match_by_character() {
    assert_eq!(tier("设计", "设计稿-首页.op"), Some(MatchTier::Prefix));
    assert_eq!(tier("首页", "设计稿-首页.op"), Some(MatchTier::WordStart));
    assert_eq!(tier("ÉCRAN", "écran-1.op"), Some(MatchTier::Prefix));
}

#[test]
fn a_project_name_match_ranks_after_every_name_match() {
    let by_name = score_in_project("shop", "shop-card.op", "web").unwrap();
    let by_scatter = score_in_project("shp", "s-h-p.op", "web").unwrap();
    let by_project = score_in_project("shop", "checkout.op", "shop").unwrap();
    assert_eq!(by_project.tier, MatchTier::Project);
    assert!(by_name < by_project && by_scatter < by_project);
    assert_eq!(score_in_project("shop", "checkout.op", "blog"), None);
    assert_eq!(
        score_in_project("shop", "checkout.op", ""),
        None,
        "no project, no match"
    );
}
