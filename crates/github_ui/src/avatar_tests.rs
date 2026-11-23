use crate::avatar::{initials, people_label, trusted_avatar_url};

#[test]
fn only_https_urls_on_githubs_avatar_host_are_loaded() {
    for allowed in [
        "https://avatars.githubusercontent.com/u/583231?v=4",
        "https://avatars.githubusercontent.com/u/583231?s=40&v=4",
        "https://AVATARS.githubusercontent.com/u/1",
        "https://avatars.githubusercontent.com:443/u/1",
    ] {
        assert!(trusted_avatar_url(allowed).is_some(), "{allowed}");
    }
    for refused in [
        "http://avatars.githubusercontent.com/u/1",
        "https://avatars.githubusercontent.com.evil.example/u/1",
        "https://evil.example/u/1?https://avatars.githubusercontent.com/u/1",
        "https://evil.example/avatars.githubusercontent.com/u/1",
        "https://user:secret@avatars.githubusercontent.com/u/1",
        "https://avatars.githubusercontent.com@evil.example/u/1",
        "https://avatars.githubusercontent.com:8443/u/1",
        "https://private-avatars.githubusercontent.com/u/1",
        "https://github.com/octocat.png",
        "//avatars.githubusercontent.com/u/1",
        "avatars.githubusercontent.com/u/1",
        "file:///etc/passwd",
        "data:image/png;base64,AAAA",
        "",
    ] {
        assert!(trusted_avatar_url(refused).is_none(), "{refused}");
    }
}

#[test]
fn initials_come_from_the_name_or_else_the_login() {
    assert_eq!(initials("octocat", Some("The Octocat")), "TO");
    assert_eq!(initials("octocat", Some("Mona")), "M");
    assert_eq!(initials("octocat", None), "O");
    assert_eq!(initials("octocat", Some("   ")), "O");
    assert_eq!(
        initials("\u{202e}evil", None),
        "E",
        "hidden characters are skipped"
    );
    assert_eq!(initials("-", None), "?");
}

#[test]
fn the_people_next_to_their_faces_are_named_by_login() {
    let logins = |names: &[&str]| {
        names
            .iter()
            .map(|name| name.to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(people_label(&logins(&[])), "");
    assert_eq!(people_label(&logins(&["octocat"])), "octocat");
    assert_eq!(
        people_label(&logins(&["octocat", "hubot"])),
        "octocat, hubot"
    );
    assert_eq!(
        people_label(&logins(&["octocat", "hubot", "monalisa"])),
        "octocat +2"
    );
}
