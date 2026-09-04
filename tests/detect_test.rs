mod common;

use common::MockGit;
use gflow::hosting::detect::{detect, Provider};

// `detect` is the thin shell over the pure `resolve`/`parse_remote` core (unit
// tested in-module): it reads the override key and the origin remote, in that
// order. These tests pin the two git reads and the missing-remote path — the
// only behavior the pure core cannot express.

fn ado(org: &str, project: &str, repo: &str) -> Provider {
    Provider::AzureDevOps {
        org: org.to_string(),
        project: project.to_string(),
        repo: repo.to_string(),
    }
}

#[test]
fn reads_the_override_key_then_the_origin_remote() {
    let mut git = MockGit::new();
    git.remote_url = "https://dev.azure.com/beans/Shop/_git/backend".to_string();

    assert_eq!(detect(&git).unwrap(), ado("beans", "Shop", "backend"));
    assert_eq!(git.calls(), vec!["get_config:gflow.hosting.provider", "remote_url"]);
}

#[test]
fn the_configured_override_wins_over_the_remote() {
    let mut git = MockGit::new();
    git.remote_url = "https://dev.azure.com/beans/Shop/_git/backend".to_string();
    git.config.insert("gflow.hosting.provider".to_string(), "github".to_string());

    assert_eq!(detect(&git).unwrap(), Provider::GitHub);
}

#[test]
fn a_repo_without_an_origin_remote_falls_back_to_github() {
    // `remote_url` errors when there is no origin (a fresh `git init`). Detection
    // must not turn that into a hard failure — GitHub is the documented default.
    let mut git = MockGit::new();
    git.fail_remote_url = true;

    assert_eq!(detect(&git).unwrap(), Provider::GitHub);
}

#[test]
fn a_devops_override_without_an_origin_remote_is_a_hard_error() {
    // Asymmetric on purpose: az cannot work without org/project/repo, so an
    // explicit devops override with nothing to parse must fail loudly rather
    // than silently fall back to GitHub.
    let mut git = MockGit::new();
    git.fail_remote_url = true;
    git.config.insert("gflow.hosting.provider".to_string(), "devops".to_string());

    let err = detect(&git).unwrap_err();

    assert!(err.contains("could not be determined"), "got: {err}");
}
