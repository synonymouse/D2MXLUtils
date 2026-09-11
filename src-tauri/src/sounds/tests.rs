use super::*;

#[test]
fn extension_from_lowercases_and_strips_dot() {
    assert_eq!(extension_from("foo.MP3").as_deref(), Some("mp3"));
    assert_eq!(extension_from("with.spaces.ogg").as_deref(), Some("ogg"));
    assert_eq!(extension_from("no_ext"), None);
}
