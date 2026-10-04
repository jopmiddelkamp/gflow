const NUSPEC: &str = include_str!("../packaging/chocolatey/gflow.nuspec");

fn element(name: &str) -> &'static str {
    let open = format!("<{name}>");
    let start = NUSPEC.find(&open).unwrap_or_else(|| panic!("nuspec has no {open}")) + open.len();
    let end = NUSPEC[start..].find(&format!("</{name}>")).unwrap_or_else(|| panic!("unclosed {open}"));
    &NUSPEC[start..start + end]
}

// Chocolatey moderation rejects raw.githubusercontent.com and unpinned URLs;
// CI replaces __TAG__ with the release tag, so the committed URL must keep it.
#[test]
fn icon_url_is_jsdelivr_pinned_to_the_tag_placeholder() {
    assert_eq!(
        element("iconUrl"),
        "https://cdn.jsdelivr.net/gh/jopmiddelkamp/gflow@__TAG__/packaging/chocolatey/icon.png"
    );
}

// Chocolatey moderation requires a copyright notice on every new version.
#[test]
fn copyright_names_the_author() {
    assert_eq!(element("copyright"), "Copyright 2026 Jop Middelkamp");
}
