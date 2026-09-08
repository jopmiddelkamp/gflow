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
fn the_probe_filter_never_lets_a_missing_pr_become_a_url() {
    // `.[0].url` over an empty list is jq null. Suppressing a null result is
    // gh's output formatting, not a promised contract, and a "null" reaching
    // the caller would be returned as the PR's URL — the landing PR silently
    // never created. `// empty` makes emptiness the filter's own guarantee.
    let runner = MockCliRunner::scripted(&[Ok(""), Ok("https://github.com/o/r/pull/12")]);

    gh(&runner).create_or_get_pr("feature/x", "develop", "feat: x", PrBody::NativeDefault).unwrap();

    let probe = &runner.calls()[0];
    assert!(probe.ends_with("--jq .[0].url // empty"), "probe must coerce null to empty; got: {probe}");
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
    assert_eq!(runner.calls()[1],
        "gh pr create --head feature/x --base develop --title feat: x --body ");
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
fn a_closed_or_merged_pr_leads_to_a_fresh_one() {
    // --state open excludes a closed/merged PR from the probe: the branch has
    // new work, so a new PR is correct.
    let runner = MockCliRunner::scripted(&[Ok(""), Ok("https://github.com/o/r/pull/9")]);

    let url = gh(&runner).create_or_get_pr("feature/x", "develop", "feat: x", PrBody::NativeDefault).unwrap();

    assert_eq!(url, "https://github.com/o/r/pull/9");
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

// --- GitHub: merged_pr and check_auth ---

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

#[test]
fn github_auth_check_runs_gh_auth_status() {
    let runner = MockCliRunner::scripted(&[Ok("Logged in to github.com")]);

    gh(&runner).check_auth().unwrap();

    assert_eq!(runner.calls(), vec!["gh auth status"]);
}

// --- Azure DevOps ---

#[test]
fn an_active_ado_pr_is_reused_and_its_url_synthesized() {
    // az's webUrl is unreliable, so the URL is built from the parsed coordinates.
    let runner = MockCliRunner::scripted(&[Ok("2662")]);

    let url = ado(&runner).create_or_get_pr("feature/x", "develop", "feat: x", PrBody::NativeDefault).unwrap();

    assert_eq!(url, "https://dev.azure.com/beans/Shop/_git/shop/pullrequest/2662");
    assert_eq!(runner.calls().len(), 1, "no create call may follow");
    let call = &runner.calls()[0];
    assert!(call.contains("--status active"), "only an open PR may be reused; got: {call}");
    assert!(call.contains("--organization https://dev.azure.com/beans --project Shop --repository shop"),
        "every az call carries the repo coordinates; got: {call}");
}

#[test]
fn no_active_ado_pr_creates_one() {
    let runner = MockCliRunner::scripted(&[Ok(""), Ok("2663")]);

    let url = ado(&runner).create_or_get_pr("feature/x", "develop", "feat: x", PrBody::NativeDefault).unwrap();

    assert_eq!(url, "https://dev.azure.com/beans/Shop/_git/shop/pullrequest/2663");
    let call = &runner.calls()[1];
    assert!(call.starts_with("az repos pr create"), "got: {call}");
    assert!(call.contains("--description"), "az always receives a description; got: {call}");
}

#[test]
fn an_unreadable_pr_template_is_a_hard_error_naming_the_path() {
    let runner = MockCliRunner::scripted(&[Ok("")]);

    let err = ado(&runner)
        .create_or_get_pr("feature/x", "develop", "feat: x", PrBody::File("/definitely/not/here.md"))
        .unwrap_err();

    assert!(err.contains("Failed to read PR template"), "got: {err}");
    assert!(err.contains("/definitely/not/here.md"), "must name the path; got: {err}");
}

#[test]
fn ado_merged_pr_queries_the_newest_pr_of_any_status() {
    let runner = MockCliRunner::scripted(&[Ok("completed\tabc123\tdeadbeef\trefs/heads/develop\t49")]);

    let pr = ado(&runner).merged_pr("feature/x").unwrap().unwrap();

    assert_eq!(pr.url, "https://dev.azure.com/beans/Shop/_git/shop/pullrequest/49");
    assert_eq!(pr.head_sha, "abc123");
    assert_eq!(pr.merge_commit_sha, "deadbeef");
    let call = &runner.calls()[0];
    assert!(call.contains("--status all"), "got: {call}");
    // The `[0:1]` slice, not `[0]`: a multiselect on a plain index is a flat list
    // of scalars, which az's tsv writer prints one value per line. Only a list of
    // lists becomes a single tab-separated row, which is what the parser reads.
    assert!(call.contains("[0:1].[status, lastMergeSourceCommit.commitId, lastMergeCommit.commitId, targetRefName, pullRequestId]"),
        "the tsv row parser depends on this exact projection and order; got: {call}");
}

#[test]
fn ado_merged_pr_to_filters_by_source_and_target_branch() {
    let runner = MockCliRunner::scripted(&[Ok("completed\tabc123\tdeadbeef\t49")]);

    let pr = ado(&runner).merged_pr_to("feature/x", "develop").unwrap().unwrap();

    assert_eq!(pr.url, "https://dev.azure.com/beans/Shop/_git/shop/pullrequest/49");
    assert_eq!(pr.head_sha, "abc123");
    assert_eq!(pr.merge_commit_sha, "deadbeef");
    // `--status completed`, not `all`: a newer active or abandoned PR must not
    // erase the fact that this leg already landed.
    assert_eq!(
        runner.calls()[0],
        "az repos pr list --organization https://dev.azure.com/beans --project Shop --repository shop \
--source-branch feature/x --target-branch develop --status completed \
--query [0:1].[status, lastMergeSourceCommit.commitId, lastMergeCommit.commitId, pullRequestId] -o tsv"
    );
}

#[test]
fn ado_auth_check_verifies_the_extension_before_probing_the_repo() {
    // The extension check comes first: it also prevents az's interactive
    // dynamic-install prompt from firing inside a later non-tty command.
    let runner = MockCliRunner::scripted(&[Ok("azure-devops 1.0.0"), Ok("repo-id")]);

    ado(&runner).check_auth().unwrap();

    assert_eq!(runner.calls()[0], "az extension show --name azure-devops");
    assert!(runner.calls()[1].starts_with("az repos show"),
        "repo access is probed directly, not via `az account show`; got: {}", runner.calls()[1]);
}

#[test]
fn a_missing_ado_extension_names_the_install_command() {
    let runner = MockCliRunner::scripted(&[Err("az extension show failed: not installed")]);

    let err = ado(&runner).check_auth().unwrap_err();

    assert!(err.contains("az extension add --name azure-devops"), "got: {err}");
    assert_eq!(runner.calls().len(), 1, "the repo probe must not run without the extension");
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
    let runner = MockCliRunner::scripted(&[Ok("61")]);

    let url = ado(&runner).open_pr_to("release/1.2.0", "develop").unwrap().unwrap();

    assert!(url.ends_with("/pullrequest/61"), "got: {url}");
    let call = &runner.calls()[0];
    assert!(call.contains("--source-branch release/1.2.0") && call.contains("--target-branch develop")
        && call.contains("--status active") && call.contains("[0].pullRequestId"), "got: {call}");
}

#[test]
fn az_open_pr_to_empty_result_is_none() {
    let runner = MockCliRunner::scripted(&[Ok("")]);
    assert_eq!(ado(&runner).open_pr_to("release/1.2.0", "develop").unwrap(), None);
}

#[test]
fn az_malformed_merged_pr_row_is_an_error_not_a_silent_none() {
    // A row that isn't the expected 5-field tsv means the az query changed
    // shape — surfacing it beats guessing at merge state.
    let runner = MockCliRunner::scripted(&[Ok("garbage-without-tabs")]);
    let err = ado(&runner).merged_pr("feature/x").unwrap_err();
    assert!(err.contains("Unexpected merged-PR data"), "got: {err}");
}

#[test]
fn az_malformed_merged_pr_to_row_is_an_error_not_a_silent_none() {
    let runner = MockCliRunner::scripted(&[Ok("completed\tonly-two")]);
    let err = ado(&runner).merged_pr_to("feature/x", "develop").unwrap_err();
    assert!(err.contains("Unexpected merged-PR data"), "got: {err}");
}
