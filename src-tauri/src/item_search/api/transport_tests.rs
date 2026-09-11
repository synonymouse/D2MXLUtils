use super::*;

#[test]
fn percent_encodes_spaces_and_apostrophes() {
    assert_eq!(
        percent_encode_query("Red Vex' Curse"),
        "Red%20Vex%27%20Curse"
    );
}
