use super::*;

fn db(entries: &[(&str, &str)]) -> UniqueStatsDb {
    entries
        .iter()
        .map(|(n, s)| (n.to_string(), s.to_string()))
        .collect()
}

#[test]
fn appends_range_for_matching_line() {
    let db = db(&[(
        "Akara's Robe (1)",
        "+50 Defense\n+(6 to 10) to all Attributes\n+50 to Life\nElemental Resists +(11 to 15)%",
    )]);
    let actual = "+50 Defense\n+6 to all Attributes\n+50 to Life\nElemental Resists +13%";
    let result = annotate_with_roll_ranges(&db, "Akara's Robe (1)", None, actual);
    assert_eq!(
        result,
        "+50 Defense\n+6 to all Attributes (6-10)\n+50 to Life\nElemental Resists +13% (11-15)"
    );
}

#[test]
fn strips_kind_label_and_uses_tier_suffix() {
    let db = db(&[("Akara's Robe (Sacred)", "+(31 to 50) to all Attributes")]);
    let actual = "+40 to all Attributes";
    let result =
        annotate_with_roll_ranges(&db, "Akara's Robe SSSU", Some(ItemTier::Sacred), actual);
    assert_eq!(result, "+40 to all Attributes (31-50)");
}

#[test]
fn unknown_name_returns_input_unchanged() {
    let db = db(&[]);
    let actual = "+6 to all Attributes";
    assert_eq!(annotate_with_roll_ranges(&db, "Nope", None, actual), actual);
}

#[test]
fn flat_template_value_is_not_annotated() {
    let db = db(&[("X", "+50 Defense")]);
    let actual = "+50 Defense";
    assert_eq!(annotate_with_roll_ranges(&db, "X", None, actual), actual);
}

#[test]
fn line_with_no_matching_template_line_is_left_unchanged() {
    let db = db(&[("X", "+(6 to 10) to all Attributes")]);
    let actual = "Socketed (2)\n+6 to all Attributes";
    assert_eq!(
        annotate_with_roll_ranges(&db, "X", None, actual),
        "Socketed (2)\n+6 to all Attributes (6-10)"
    );
}
