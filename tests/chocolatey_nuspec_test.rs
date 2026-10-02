const NUSPEC: &str = include_str!("../packaging/chocolatey/gflow.nuspec");

fn icon_url() -> &'static str {
    let start = NUSPEC.find("<iconUrl>").expect("nuspec has no <iconUrl>") + "<iconUrl>".len();
    let end = NUSPEC[start..].find("</iconUrl>").expect("unclosed <iconUrl>");
    &NUSPEC[start..start + end]
}

// Chocolatey moderation rejects raw.githubusercontent.com and unpinned URLs;
// CI replaces __TAG__ with the release tag, so the committed URL must keep it.
#[test]
fn icon_url_is_jsdelivr_pinned_to_the_tag_placeholder() {
    assert_eq!(
        icon_url(),
        "https://cdn.jsdelivr.net/gh/jopmiddelkamp/gflow@__TAG__/packaging/chocolatey/icon.png"
    );
}
