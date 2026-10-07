mod common;

use common::MockCliRunner;
use gflow::hosting::devops::AzureDevOps;
use gflow::hosting::github::GitHub;
use gflow::hosting::{HostingPlatform, PrBody};

// The provider adapters carry real policy on top of `gh`/`az`: when an existing
// PR is reused vs. a new one created, which CLI failures are normal and which
// are fatal, and the exact query flags that decide what the CLI returns. None of
// that was reachable while the subprocess call was hard-wired; it runs here
// against a scripted CLI. The subprocess itself (`SystemCli`) stays untested by
// design — tests never touch installed CLIs.

fn gh(runner: &MockCliRunner) -> GitHub<'_> {
    GitHub::new(runner)
}

fn ado(runner: &MockCliRunner) -> AzureDevOps<'_> {
    AzureDevOps::new("beans".to_string(), "Shop".to_string(), "shop".to_string(), runner)
}

/// A scripted az whose one-time extension check passes, so a test scripts only
/// the calls it is about.
fn az_scripted(responses: &[Result<&str, &str>]) -> MockCliRunner {
    let mut all = vec![Ok("azure-devops 1.0.0")];
    all.extend_from_slice(responses);
    MockCliRunner::scripted(&all)
}

/// The az calls after the extension check.
fn az_calls(runner: &MockCliRunner) -> Vec<String> {
    runner.calls().split_off(1)
}

// --- GitHub: create_or_get_pr ---

#[test]
fn an_open_pr_is_reused_instead_of_creating_a_second_one() {
    // "PR already open" is a normal resume outcome, not an error.
    let runner = MockCliRunner::scripted(&[Ok("https://github.com/o/r/pull/7")]);

    let url = gh(&runner).create_or_get_pr("feature/x", "develop", "feat: x", PrBody::NativeDefault).unwrap();

    assert_eq!(url, "https://github.com/o/r/pull/7");
    assert_eq!(runner.calls(), vec![
        "gh pr list --head feature/x --base develop --state open --limit 1 --json url --jq .[0].url // empty"
    ]);
}

#[test]
fn no_existing_pr_creates_one_with_an_empty_body() {
    // gh pr list exits 0 with an empty result when the branch has no open PR
    // at all — that is the normal first-finish path.
    let runner = MockCliRunner::scripted(&[
        Ok(""),
        Ok("https://github.com/o/r/pull/8"),
    ]);

    let url = gh(&runner).create_or_get_pr("feature/x", "develop", "feat: x", PrBody::NativeDefault).unwrap();

    assert_eq!(url, "https://github.com/o/r/pull/8");
    assert_eq!(runner.calls(), vec![
        "gh pr list --head feature/x --base develop --state open --limit 1 --json url --jq .[0].url // empty",
        "gh pr create --head feature/x --base develop --title feat: x --body ",
    ]);
}

#[test]
fn an_empty_body_pr_never_carries_a_body_file() {
    // Landing legs pass PrBody::Empty: gh must get an explicit empty --body
    // (no flag at all would drop gh into its interactive editor).
    let runner = MockCliRunner::scripted(&[Ok(""), Ok("https://github.com/o/r/pull/8")]);

    gh(&runner).create_or_get_pr("finish/hotfix-1.1.1-into-main", "main", "chore: merge hotfix 1.1.1 into main", PrBody::Empty).unwrap();

    assert_eq!(runner.calls()[1],
        "gh pr create --head finish/hotfix-1.1.1-into-main --base main --title chore: merge hotfix 1.1.1 into main --body ");
}

#[test]
fn a_real_gh_failure_is_fatal_and_names_the_auth_fix() {
    // Every probe failure is fatal now that gh pr list exits 0 for "no PRs" —
    // there is nothing left to swallow. An expired token must not silently
    // become "create a new PR".
    let runner = MockCliRunner::scripted(&[Err("gh pr list failed: HTTP 401: Bad credentials")]);

    let err = gh(&runner).create_or_get_pr("feature/x", "develop", "feat: x", PrBody::NativeDefault).unwrap_err();

    assert!(err.contains("gh auth login"), "must name the next command; got: {err}");
    assert_eq!(runner.calls().len(), 1, "nothing may be created after a real failure");
}

#[test]
fn a_resolved_template_is_passed_as_a_body_file() {
    let runner = MockCliRunner::scripted(&[Ok(""), Ok("https://github.com/o/r/pull/10")]);

    gh(&runner).create_or_get_pr("feature/x", "develop", "feat: x", PrBody::File(".github/pr-templates/gflow-feature.md")).unwrap();

    assert_eq!(runner.calls()[1],
        "gh pr create --head feature/x --base develop --title feat: x --body-file .github/pr-templates/gflow-feature.md");
}

#[test]
fn an_open_pr_to_a_different_base_is_not_reused() {
    // A fan-out source branch (a protected hotfix landing on both main and a
    // release branch) can have an open PR to one base while this call targets
    // another. The probe must be base-filtered so it returns empty here, not
    // the branch's other open PR.
    let runner = MockCliRunner::scripted(&[Ok(""), Ok("https://github.com/o/r/pull/11")]);

    let url = gh(&runner)
        .create_or_get_pr("hotfix/1.2.4", "release/1.2.0", "chore: merge hotfix 1.2.4 into release/1.2.0", PrBody::NativeDefault)
        .unwrap();

    assert_eq!(url, "https://github.com/o/r/pull/11");
    assert_eq!(runner.calls()[0],
        "gh pr list --head hotfix/1.2.4 --base release/1.2.0 --state open --limit 1 --json url --jq .[0].url // empty");
}

// --- GitHub: merged_pr ---

#[test]
fn merged_pr_asks_only_for_the_newest_pr_of_the_branch() {
    let runner = MockCliRunner::scripted(&[Ok("https://github.com/o/r/pull/49\tabc123\tdeadbeef\tdevelop")]);

    let pr = gh(&runner).merged_pr("feature/x").unwrap().unwrap();

    assert_eq!(pr.head_sha, "abc123");
    assert_eq!(pr.merge_commit_sha, "deadbeef");
    assert_eq!(pr.base, "develop");
    let call = &runner.calls()[0];
    assert!(call.contains("--state all"), "closed PRs must be visible too; got: {call}");
    assert!(call.contains("--limit 1"), "only the newest PR decides; got: {call}");
    assert!(call.contains("mergeCommit"), "the completion-type check needs the merge commit; got: {call}");
}

#[test]
fn a_merged_pr_lookup_failure_names_the_auth_fix() {
    let runner = MockCliRunner::scripted(&[Err("gh pr list failed: HTTP 401")]);

    let err = gh(&runner).merged_pr("feature/x").unwrap_err();

    assert!(err.contains("gh auth login"), "got: {err}");
}

#[test]
fn merged_pr_to_filters_by_exact_head_and_base() {
    let runner = MockCliRunner::scripted(&[Ok("https://github.com/o/r/pull/49\tabc123\tdeadbeef")]);

    let pr = gh(&runner).merged_pr_to("feature/x", "develop").unwrap().unwrap();

    assert_eq!(pr.head_sha, "abc123");
    assert_eq!(pr.merge_commit_sha, "deadbeef");
    // `--state merged`, not `all`: this answers "has this leg landed", which a
    // newer open or abandoned PR must not erase. `merged_pr` above deliberately
    // keeps `all` — for a work branch, a newer PR means the branch is in play.
    assert_eq!(
        runner.calls()[0],
        r#"gh pr list --head feature/x --base develop --state merged --limit 1 --json url,state,headRefOid,mergeCommit --jq .[0] | select(.state == "MERGED") | [.url, .headRefOid, .mergeCommit.oid] | @tsv"#
    );
}

#[test]
fn a_merged_pr_to_lookup_failure_names_the_auth_fix() {
    let runner = MockCliRunner::scripted(&[Err("gh pr list failed: HTTP 401")]);

    let err = gh(&runner).merged_pr_to("feature/x", "develop").unwrap_err();

    assert!(err.contains("gh auth login"), "got: {err}");
}

// --- Azure DevOps ---

#[test]
fn an_active_ado_pr_is_reused_and_its_url_synthesized() {
    // az's webUrl is unreliable, so the URL is built from the parsed coordinates.
    let runner = az_scripted(&[Ok("abandoned\trefs/heads/feature/x\tabc\t\t2661\nactive\trefs/heads/feature/x\tabc\t\t2662")]);

    let url = ado(&runner).create_or_get_pr("feature/x", "develop", "feat: x", PrBody::NativeDefault).unwrap();

    assert_eq!(url, "https://dev.azure.com/beans/Shop/_git/shop/pullrequest/2662", "only an open PR may be reused");
    assert_eq!(az_calls(&runner), vec!["az repos pr list --organization https://dev.azure.com/beans --project Shop --repository shop \
--target-branch develop --status all --top 1000 \
--query [].[status, sourceRefName, lastMergeSourceCommit.commitId, lastMergeCommit.commitId, pullRequestId] -o tsv"], "no create call may follow");
}

#[test]
fn no_active_ado_pr_creates_one() {
    let runner = az_scripted(&[Ok("completed\trefs/heads/feature/x\tabc\tdef\t2600"), Ok("2663")]);

    let url = ado(&runner).create_or_get_pr("feature/x", "develop", "feat: x", PrBody::NativeDefault).unwrap();

    assert_eq!(url, "https://dev.azure.com/beans/Shop/_git/shop/pullrequest/2663");
    let call = &az_calls(&runner)[1];
    assert!(call.starts_with("az repos pr create"), "got: {call}");
    assert!(call.contains("--description"), "az always receives a description; got: {call}");
}

#[test]
fn an_unreadable_pr_template_is_a_hard_error_naming_the_path() {
    let runner = az_scripted(&[Ok("")]);

    let err = ado(&runner)
        .create_or_get_pr("feature/x", "develop", "feat: x", PrBody::File("/definitely/not/here.md"))
        .unwrap_err();

    assert!(err.contains("Failed to read PR template"), "got: {err}");
    assert!(err.contains("/definitely/not/here.md"), "must name the path; got: {err}");
}

#[test]
fn ado_merged_pr_queries_the_newest_pr_of_any_status() {
    let runner = az_scripted(&[Ok("completed\tabc123\tdeadbeef\trefs/heads/develop\t49")]);

    let pr = ado(&runner).merged_pr("feature/x").unwrap().unwrap();

    assert_eq!(pr.url, "https://dev.azure.com/beans/Shop/_git/shop/pullrequest/49");
    assert_eq!(pr.head_sha, "abc123");
    assert_eq!(pr.merge_commit_sha, "deadbeef");
    let call = &az_calls(&runner)[0];
    assert!(call.contains("--status all"), "got: {call}");
    // The `[0:1]` slice, not `[0]`: a multiselect on a plain index is a flat list
    // of scalars, which az's tsv writer prints one value per line. Only a list of
    // lists becomes a single tab-separated row, which is what the parser reads.
    assert!(call.contains("[0:1].[status, lastMergeSourceCommit.commitId, lastMergeCommit.commitId, targetRefName, pullRequestId]"),
        "the tsv row parser depends on this exact projection and order; got: {call}");
}

/// A full page of PRs into a target, none from the heads the tests ask about.
fn truncated_list() -> String {
    (0..1000).map(|i| format!("completed\trefs/heads/feature/other-{i}\th{i}\tm{i}\t{i}")).collect::<Vec<_>>().join("\n")
}

#[test]
fn ado_lookups_into_one_target_share_one_list_query() {
    // One az call costs seconds; a protected landing asks about the same
    // target up to five times per run.
    let runner = az_scripted(&[Ok("active\trefs/heads/release/1.0.0\taaa\t\t61\n\
completed\trefs/heads/finish/release-1.0.0-into-main\tbbb\tccc\t60")]);
    let ado = ado(&runner);

    let open = ado.open_pr_to("release/1.0.0", "main").unwrap().unwrap();
    let landed = ado.merged_pr_to("finish/release-1.0.0-into-main", "main").unwrap().unwrap();
    let legacy = ado.merged_pr_to("release/1.0.0", "main").unwrap();

    assert!(open.ends_with("/pullrequest/61"), "got: {open}");
    assert!(landed.url.ends_with("/pullrequest/60"), "got: {}", landed.url);
    assert_eq!((landed.head_sha.as_str(), landed.merge_commit_sha.as_str()), ("bbb", "ccc"));
    assert_eq!(legacy, None, "a complete list without a completed PR means none exists");
    assert_eq!(az_calls(&runner), vec!["az repos pr list --organization https://dev.azure.com/beans --project Shop --repository shop \
--target-branch main --status all --top 1000 \
--query [].[status, sourceRefName, lastMergeSourceCommit.commitId, lastMergeCommit.commitId, pullRequestId] -o tsv"]);
}

#[test]
fn ado_the_newest_pr_of_each_status_decides() {
    let runner = az_scripted(&[Ok("active\trefs/heads/x\th3\t\t9\n\
completed\trefs/heads/x\th2\tm2\t7\n\
completed\trefs/heads/x\th1\tm1\t5")]);
    let ado = ado(&runner);

    // A newer active PR must not erase the fact that this leg already landed.
    let landed = ado.merged_pr_to("x", "main").unwrap().unwrap();
    let open = ado.open_pr_to("x", "main").unwrap().unwrap();

    assert_eq!((landed.head_sha.as_str(), landed.merge_commit_sha.as_str()), ("h2", "m2"));
    assert!(open.ends_with("/pullrequest/9"), "got: {open}");
}

#[test]
fn ado_a_head_found_in_a_truncated_list_needs_no_second_query() {
    // The list is newest first, so a head's first row is its newest PR even
    // when older PRs fell past the limit.
    let list = format!("completed\trefs/heads/x\th\tm\t1001\n{}", truncated_list());
    let runner = az_scripted(&[Ok(&list)]);

    let landed = ado(&runner).merged_pr_to("x", "main").unwrap().unwrap();

    assert!(landed.url.ends_with("/pullrequest/1001"), "got: {}", landed.url);
    assert_eq!(az_calls(&runner).len(), 1);
}

#[test]
fn ado_a_head_missing_from_a_truncated_list_is_asked_for_directly() {
    let list = truncated_list();
    let runner = az_scripted(&[Ok(&list), Ok("completed\tabc123\tdeadbeef\t49"), Ok("61")]);
    let ado = ado(&runner);

    let pr = ado.merged_pr_to("feature/x", "develop").unwrap().unwrap();
    let open = ado.open_pr_to("feature/x", "develop").unwrap().unwrap();

    assert_eq!(pr.url, "https://dev.azure.com/beans/Shop/_git/shop/pullrequest/49");
    assert_eq!(pr.head_sha, "abc123");
    assert_eq!(pr.merge_commit_sha, "deadbeef");
    assert!(open.ends_with("/pullrequest/61"), "got: {open}");
    let calls = az_calls(&runner);
    assert_eq!(calls.len(), 3, "the list is fetched once; got: {calls:?}");
    // `--status completed`, not `all`: a newer active or abandoned PR must not
    // erase the fact that this leg already landed.
    assert_eq!(
        calls[1],
        "az repos pr list --organization https://dev.azure.com/beans --project Shop --repository shop \
--source-branch feature/x --target-branch develop --status completed \
--query [0:1].[status, lastMergeSourceCommit.commitId, lastMergeCommit.commitId, pullRequestId] -o tsv"
    );
    assert_eq!(
        calls[2],
        "az repos pr list --organization https://dev.azure.com/beans --project Shop --repository shop \
--source-branch feature/x --target-branch develop --status active --query [0].pullRequestId -o tsv"
    );
}

#[test]
fn ado_direct_lookups_past_a_truncated_list_keep_their_failure_rules() {
    let list = truncated_list();

    let runner = az_scripted(&[Ok(&list), Err("access denied")]);
    let err = ado(&runner).merged_pr_to("x", "main").unwrap_err();
    assert!(err.contains("access denied") && err.contains("az login"), "got: {err}");

    let runner = az_scripted(&[Ok(&list), Err("access denied")]);
    let err = ado(&runner).open_pr_to("x", "main").unwrap_err();
    assert!(err.contains("access denied") && err.contains("az login"), "got: {err}");

    let runner = az_scripted(&[Ok(&list), Ok("")]);
    assert_eq!(ado(&runner).open_pr_to("x", "main").unwrap(), None);

    let runner = az_scripted(&[Ok(&list), Ok("None")]);
    let err = ado(&runner).open_pr_to("x", "main").unwrap_err();
    assert!(err.contains("Unexpected az pull request id: 'None'"), "got: {err}");
}

#[test]
fn ado_creating_a_pr_refreshes_that_targets_list() {
    let runner = az_scripted(&[Ok(""), Ok("62"), Ok("active\trefs/heads/feature/x\th\t\t62")]);
    let ado = ado(&runner);

    ado.create_or_get_pr("feature/x", "develop", "feat: x", PrBody::Empty).unwrap();
    let open = ado.open_pr_to("feature/x", "develop").unwrap().unwrap();

    assert!(open.ends_with("/pullrequest/62"), "the new PR must be visible; got: {open}");
    assert_eq!(az_calls(&runner).len(), 3);
}

#[test]
fn ado_a_malformed_list_row_is_an_error() {
    let runner = az_scripted(&[Ok("completed\trefs/heads/x\tmissing-fields")]);

    let err = ado(&runner).open_pr_to("x", "main").unwrap_err();

    assert!(err.contains("Unexpected PR data from az:") && err.contains("missing-fields"), "got: {err}");
}

#[test]
fn ado_a_landed_row_without_a_merge_commit_is_an_error() {
    let runner = az_scripted(&[Ok("completed\trefs/heads/x\tabc\tNone\t49")]);

    let err = ado(&runner).merged_pr_to("x", "main").unwrap_err();

    assert!(err.contains("Unexpected merge commit from az:"), "got: {err}");
}

#[test]
fn the_ado_extension_is_verified_once_before_the_first_az_call() {
    // Without the extension, az answers a repos command with an interactive
    // install prompt, hidden behind the captured output — gflow would hang.
    let runner = MockCliRunner::scripted(&[Ok("azure-devops 1.0.0"), Ok(""), Ok("")]);
    let ado = ado(&runner);

    ado.merged_pr("feature/x").unwrap();
    ado.open_pr_to("feature/x", "develop").unwrap();

    let calls = runner.calls();
    assert_eq!(calls[0], "az extension show --name azure-devops");
    assert_eq!(calls.len(), 3, "the extension is checked once per run; got: {calls:?}");
    assert!(calls[1].starts_with("az repos pr list") && calls[2].starts_with("az repos pr list"), "got: {calls:?}");
}

#[test]
fn a_missing_ado_extension_stops_before_any_repo_call() {
    let runner = MockCliRunner::scripted(&[Err("az extension show failed: not installed")]);

    let err = ado(&runner).merged_pr("feature/x").unwrap_err();

    assert!(err.contains("az extension add --name azure-devops"), "got: {err}");
    assert_eq!(runner.calls().len(), 1, "no repo call may run without the extension");
}

#[test]
fn a_failed_az_call_names_the_login_commands() {
    let runner = MockCliRunner::scripted(&[Ok("azure-devops 1.0.0"), Err("az repos pr list failed: TF400813: not authorized")]);

    let err = ado(&runner).merged_pr("feature/x").unwrap_err();

    assert!(err.contains("TF400813"), "the az reason stays visible; got: {err}");
    assert!(err.contains("az login") && err.contains("az devops login"), "must name the next command; got: {err}");
}

// --- open_pr_to (legacy-PR detection for finish-branch migration) ---

#[test]
fn gh_open_pr_to_filters_by_exact_head_base_and_open_state() {
    let runner = MockCliRunner::scripted(&[Ok("https://github.com/o/r/pull/61")]);

    let url = gh(&runner).open_pr_to("release/1.2.0", "develop").unwrap().unwrap();

    assert_eq!(url, "https://github.com/o/r/pull/61");
    assert_eq!(
        runner.calls()[0],
        "gh pr list --head release/1.2.0 --base develop --state open --limit 1 --json url --jq .[0].url // empty"
    );
}

#[test]
fn gh_open_pr_to_empty_result_is_none() {
    let runner = MockCliRunner::scripted(&[Ok("")]);
    assert_eq!(gh(&runner).open_pr_to("release/1.2.0", "develop").unwrap(), None);
}

#[test]
fn az_open_pr_to_lists_active_prs_and_synthesizes_the_url() {
    let runner = az_scripted(&[Ok("active\trefs/heads/release/1.2.0\th\t\t61")]);

    let url = ado(&runner).open_pr_to("release/1.2.0", "develop").unwrap().unwrap();

    assert_eq!(url, "https://dev.azure.com/beans/Shop/_git/shop/pullrequest/61");
}

#[test]
fn az_open_pr_to_empty_result_is_none() {
    let runner = az_scripted(&[Ok("")]);
    assert_eq!(ado(&runner).open_pr_to("release/1.2.0", "develop").unwrap(), None);
}

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
        vec![Ok("extension"), Ok("active\trefs/heads/feature/x\th\t\tNone")],
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
    for response in [Err("access denied"), Ok("active\trefs/heads/release/1.0.0\th\t\tNone")] {
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
        let expected = if base.is_some() { "Unexpected PR data from az:" } else { "Unexpected merged-PR data from az:" };
        assert!(error.contains(expected), "got: {error}");
        assert!(error.contains("missing-fields"));
    }
}
