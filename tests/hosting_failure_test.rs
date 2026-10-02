mod common;

use common::MockCliRunner;
use gflow::hosting::devops::AzureDevOps;
use gflow::hosting::github::GitHub;
use gflow::hosting::{HostingPlatform, PrBody};

#[test]
fn github_open_pr_failure_keeps_the_authentication_remedy() {
    let runner = MockCliRunner::scripted(&[Err("HTTP 401")]);
    let error = GitHub::new(&runner)
        .open_pr_to("release/1.0.0", "main")
        .unwrap_err();
    assert!(error.contains("Could not check for an open PR: HTTP 401"));
    assert!(error.contains("gh auth login"));
}

#[test]
fn azure_create_rejects_failed_or_invalid_responses() {
    for responses in [
        vec![Ok("extension"), Err("list unavailable")],
        vec![Ok("extension"), Ok("None")],
        vec![Ok("extension"), Ok(""), Err("create unavailable")],
        vec![Ok("extension"), Ok(""), Ok("None")],
    ] {
        let runner = MockCliRunner::scripted(&responses);
        let hosting = AzureDevOps::new("org".into(), "project".into(), "repo".into(), &runner);
        let error = hosting
            .create_or_get_pr("feature/x", "develop", "feat: x", PrBody::Empty)
            .unwrap_err();
        let last = responses.last().unwrap();
        match last {
            Ok(_) => assert!(error.contains("Unexpected az pull request id: 'None'")),
            Err(reason) => {
                assert!(error.contains(reason));
                assert!(error.contains("az login"));
            }
        }
        assert_eq!(
            runner.calls().len(),
            responses.len(),
            "no follow-up write after failed lookup"
        );
    }
}

#[test]
fn azure_landed_pr_failure_does_not_become_an_unmerged_result() {
    let runner = MockCliRunner::scripted(&[Ok("extension"), Err("access denied")]);
    let hosting = AzureDevOps::new("org".into(), "project".into(), "repo".into(), &runner);
    let error = hosting.merged_pr_to("release/1.0.0", "main").unwrap_err();
    assert!(error.contains("access denied"));
    assert!(error.contains("az devops login"));
}

#[test]
fn azure_open_pr_rejects_failed_or_invalid_responses() {
    for response in [Err("access denied"), Ok("None")] {
        let runner = MockCliRunner::scripted(&[Ok("extension"), response]);
        let hosting = AzureDevOps::new("org".into(), "project".into(), "repo".into(), &runner);
        let error = hosting.open_pr_to("release/1.0.0", "main").unwrap_err();
        match response {
            Err(reason) => {
                assert!(error.contains(reason));
                assert!(error.contains("az login"));
            }
            Ok(_) => assert!(error.contains("Unexpected az pull request id: 'None'")),
        }
    }
}

#[test]
fn azure_rejects_malformed_merged_rows_instead_of_authorizing_cleanup() {
    for base in [None, Some("main")] {
        let runner = MockCliRunner::scripted(&[Ok("extension"), Ok("completed\tmissing-fields")]);
        let hosting = AzureDevOps::new("org".into(), "project".into(), "repo".into(), &runner);
        let error = match base {
            None => hosting.merged_pr("release/1.0.0").unwrap_err(),
            Some(base) => hosting.merged_pr_to("release/1.0.0", base).unwrap_err(),
        };
        assert!(error.contains("Unexpected merged-PR data from az:"));
        assert!(error.contains("missing-fields"));
    }
}
