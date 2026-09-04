use gflow::git::branch::BranchType;

#[test]
fn parse_main() {
    assert_eq!(BranchType::parse("main"), BranchType::Main);
    assert_eq!(BranchType::parse("master"), BranchType::Main);
}

#[test]
fn parse_develop() {
    assert_eq!(BranchType::parse("develop"), BranchType::Develop);
}

#[test]
fn parse_feature() {
    assert_eq!(BranchType::parse("feature/passkey-login"), BranchType::Feature { name: "passkey-login".to_string() });
}

#[test]
fn parse_fix() {
    assert_eq!(BranchType::parse("fix/profile-loading"), BranchType::Fix { name: "profile-loading".to_string() });
}

#[test]
fn parse_chore() {
    assert_eq!(BranchType::parse("chore/update-ci"), BranchType::Chore { name: "update-ci".to_string() });
}

#[test]
fn parse_docs() {
    assert_eq!(BranchType::parse("docs/api-reference"), BranchType::Docs { name: "api-reference".to_string() });
}

#[test]
fn parse_refactor() {
    assert_eq!(BranchType::parse("refactor/auth-module"), BranchType::Refactor { name: "auth-module".to_string() });
}

#[test]
fn parse_release() {
    assert_eq!(BranchType::parse("release/2.6.0"), BranchType::Release { major: 2, minor: 6, patch: 0 });
}

#[test]
fn parse_release_fix() {
    assert_eq!(BranchType::parse("release-fix/2.6.0/payment-error"), BranchType::ReleaseFix { major: 2, minor: 6, patch: 0, name: "payment-error".to_string() });
}

#[test]
fn parse_hotfix() {
    assert_eq!(BranchType::parse("hotfix/2.6.1"), BranchType::Hotfix { major: 2, minor: 6, patch: 1 });
}

#[test]
fn parse_hotfix_fix() {
    assert_eq!(BranchType::parse("hotfix-fix/2.6.1/incorrect-dto"), BranchType::HotfixFix { major: 2, minor: 6, patch: 1, name: "incorrect-dto".to_string() });
}

#[test]
fn parse_other() {
    assert_eq!(BranchType::parse("some-random-branch"), BranchType::Other);
}

// --- Malformed branches degrade to Other, never to a wrong type ---
//
// Everything downstream gates on BranchType: a `release/` branch whose version
// does not parse must NOT become a Release (a finish would then invent a version
// and tag main). `Other` is the safe answer — gflow refuses instead of guessing.

#[test]
fn work_branch_without_a_name_is_not_a_work_branch() {
    assert_eq!(BranchType::parse("feature/"), BranchType::Other);
    assert_eq!(BranchType::parse("fix/"), BranchType::Other);
    assert_eq!(BranchType::parse("chore/"), BranchType::Other);
    assert_eq!(BranchType::parse("docs/"), BranchType::Other);
    assert_eq!(BranchType::parse("refactor/"), BranchType::Other);
}

#[test]
fn work_branch_prefix_without_a_slash_is_not_a_work_branch() {
    assert_eq!(BranchType::parse("feature-flags"), BranchType::Other);
    assert_eq!(BranchType::parse("feature"), BranchType::Other);
}

#[test]
fn release_branch_with_an_unparseable_version_is_not_a_release() {
    assert_eq!(BranchType::parse("release/next"), BranchType::Other);
    assert_eq!(BranchType::parse("release/2.6"), BranchType::Other);
    assert_eq!(BranchType::parse("release/"), BranchType::Other);
}

#[test]
fn hotfix_branch_with_an_unparseable_version_is_not_a_hotfix() {
    assert_eq!(BranchType::parse("hotfix/urgent"), BranchType::Other);
    assert_eq!(BranchType::parse("hotfix/2.6"), BranchType::Other);
}

#[test]
fn release_fix_needs_both_a_valid_version_and_a_name() {
    assert_eq!(BranchType::parse("release-fix/2.6.0"), BranchType::Other, "no name segment");
    assert_eq!(BranchType::parse("release-fix/2.6.0/"), BranchType::Other, "empty name");
    assert_eq!(BranchType::parse("release-fix/next/payment"), BranchType::Other, "bad version");
}

#[test]
fn hotfix_fix_needs_both_a_valid_version_and_a_name() {
    assert_eq!(BranchType::parse("hotfix-fix/2.6.1"), BranchType::Other, "no name segment");
    assert_eq!(BranchType::parse("hotfix-fix/2.6.1/"), BranchType::Other, "empty name");
    assert_eq!(BranchType::parse("hotfix-fix/urgent/dto"), BranchType::Other, "bad version");
}

#[test]
fn only_branches_carrying_a_name_expose_one() {
    // `name()` feeds PR titles; the versioned branches have no slug to expose.
    assert_eq!(BranchType::parse("feature/passkey-login").name(), Some("passkey-login"));
    assert_eq!(BranchType::parse("release-fix/2.6.0/payment").name(), Some("payment"));
    assert_eq!(BranchType::parse("hotfix-fix/2.6.1/dto").name(), Some("dto"));
    assert_eq!(BranchType::parse("release/2.6.0").name(), None);
    assert_eq!(BranchType::parse("hotfix/2.6.1").name(), None);
    assert_eq!(BranchType::parse("main").name(), None);
    assert_eq!(BranchType::parse("develop").name(), None);
    assert_eq!(BranchType::parse("whatever").name(), None);
}

#[test]
fn parse_release_chore() {
    assert_eq!(BranchType::parse("release-chore/1.1.0/set-version"), BranchType::ReleaseChore { major: 1, minor: 1, patch: 0, name: "set-version".to_string() });
}

#[test]
fn release_chore_needs_both_a_valid_version_and_a_name() {
    assert_eq!(BranchType::parse("release-chore/1.1.0"), BranchType::Other, "no name segment");
    assert_eq!(BranchType::parse("release-chore/1.1.0/"), BranchType::Other, "empty name");
    assert_eq!(BranchType::parse("release-chore/next/set-version"), BranchType::Other, "bad version");
}

#[test]
fn release_chore_exposes_its_name() {
    assert_eq!(BranchType::parse("release-chore/1.1.0/set-version").name(), Some("set-version"));
}

#[test]
fn release_chore_has_a_fixed_finish_target() {
    assert!(BranchType::parse("release-chore/1.1.0/set-version").has_fixed_finish_target());
}

#[test]
fn release_chore_is_not_a_work_branch() {
    assert!(!BranchType::parse("release-chore/1.1.0/set-version").is_work_branch());
    assert_eq!(BranchType::parse("release-chore/1.1.0/set-version").work_kind(), None);
}

#[test]
fn release_chore_pr_template_keys() {
    assert_eq!(BranchType::parse("release-chore/1.1.0/set-version").pr_template_keys(), Some(("release-chore", "chore")));
}

#[test]
fn finish_branches_parse_as_other() {
    assert_eq!(BranchType::parse("finish/release-1.2.0-into-main"), BranchType::Other);
    assert_eq!(BranchType::parse("finish/hotfix-1.1.1-into-release-1.2.0"), BranchType::Other);
}
