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

const AZ_PR_LIST: &str = "az repos pr list --organization https://dev.azure.com/beans --project Shop --repository shop";
const AZ_ROWS: &str = "--status all --top 1000 \
--query [].[status, sourceRefName, targetRefName, lastMergeSourceCommit.commitId, lastMergeCommit.commitId, pullRequestId] -o tsv";

fn az_prs_into(base: &str) -> String {
    format!("{AZ_PR_LIST} --target-branch {base} {AZ_ROWS}")
}

fn az_prs_from(head: &str) -> String {
    format!("{AZ_PR_LIST} --source-branch {head} {AZ_ROWS}")
}

#[test]
fn an_active_ado_pr_is_reused_and_its_url_synthesized() {
    // az's webUrl is unreliable, so the URL is built from the parsed coordinates.
    let runner = az_scripted(&[Ok("abandoned\trefs/heads/feature/x\trefs/heads/develop\tabc\t\t2661\n\
active\trefs/heads/feature/x\trefs/heads/develop\tabc\t\t2662")]);

    let url = ado(&runner).create_or_get_pr("feature/x", "develop", "feat: x", PrBody::NativeDefault).unwrap();

    assert_eq!(url, "https://dev.azure.com/beans/Shop/_git/shop/pullrequest/2662", "only an open PR may be reused");
    assert_eq!(az_calls(&runner), vec![az_prs_into("develop")], "no create call may follow");
}

#[test]
fn no_active_ado_pr_creates_one() {
    let runner = az_scripted(&[Ok("completed\trefs/heads/feature/x\trefs/heads/develop\tabc\tdef\t2600"), Ok("2663")]);

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
fn ado_merged_pr_is_the_branchs_newest_pr_only_when_completed() {
    for (case, rows, expected) in [
        ("no PR at all", "", None),
        ("a newer active PR keeps the branch in play",
            "active\trefs/heads/feature/x\trefs/heads/develop\th2\tm2\t50\n\
completed\trefs/heads/feature/x\trefs/heads/develop\th1\tm1\t49", None),
        ("a newer abandoned PR keeps the branch in play",
            "abandoned\trefs/heads/feature/x\trefs/heads/develop\th2\t\t50\n\
completed\trefs/heads/feature/x\trefs/heads/develop\th1\tm1\t49", None),
        ("the newest PR is merged",
            "completed\trefs/heads/feature/x\trefs/heads/develop\tabc123\tdeadbeef\t49",
            Some(("https://dev.azure.com/beans/Shop/_git/shop/pullrequest/49", "abc123", "deadbeef", "develop"))),
    ] {
        let runner = az_scripted(&[Ok(rows)]);

        let pr = ado(&runner).merged_pr("feature/x").unwrap();

        let pr = pr.as_ref().map(|pr| (pr.url.as_str(), pr.head_sha.as_str(), pr.merge_commit_sha.as_str(), pr.base.as_str()));
        assert_eq!(pr, expected, "{case}");
        assert_eq!(az_calls(&runner), vec![az_prs_from("feature/x")], "{case}");
    }
}

#[test]
fn ado_a_completed_pr_without_its_commits_or_id_is_an_error() {
    for (row, expected) in [
        ("completed\trefs/heads/x\trefs/heads/main\t\tdeadbeef\t49", "Unexpected merge source commit from az:"),
        ("completed\trefs/heads/x\trefs/heads/main\tNone\tdeadbeef\t49", "Unexpected merge source commit from az:"),
        ("completed\trefs/heads/x\trefs/heads/main\tabc\t\t49", "Unexpected merge commit from az:"),
        ("completed\trefs/heads/x\trefs/heads/main\tabc\tNone\t49", "Unexpected merge commit from az:"),
        ("completed\trefs/heads/x\trefs/heads/main\tabc\tdeadbeef\tNone", "Unexpected az pull request id: 'None'"),
    ] {
        let runner = az_scripted(&[Ok(row)]);
        let err = ado(&runner).merged_pr("x").unwrap_err();
        assert!(err.contains(expected), "merged_pr {row:?}: {err}");

        let runner = az_scripted(&[Ok(row)]);
        let err = ado(&runner).merged_pr_to("x", "main").unwrap_err();
        assert!(err.contains(expected), "merged_pr_to {row:?}: {err}");
    }
}

#[test]
fn ado_a_work_finish_asks_az_once_for_its_prs() {
    // merged_pr loads the branch's own PRs; the open-PR check before creating
    // one reads the same list instead of listing every PR into the base.
    for (case, rows, creates) in [
        ("first finish", "", true),
        ("re-run with the PR open", "active\trefs/heads/feature/x\trefs/heads/develop\th\t\t62", false),
    ] {
        let runner = az_scripted(&[Ok(rows), Ok("63")]);
        let ado = ado(&runner);

        assert_eq!(ado.merged_pr("feature/x").unwrap(), None, "{case}");
        ado.create_or_get_pr("feature/x", "develop", "feat: x", PrBody::Empty).unwrap();

        let calls = az_calls(&runner);
        assert_eq!(calls[0], az_prs_from("feature/x"), "{case}");
        assert_eq!(calls.len(), if creates { 2 } else { 1 }, "{case}: {calls:?}");
    }
}

/// A full page of PRs into a target, none from the heads the tests ask about.
fn truncated_list(base: &str) -> String {
    (0..1000)
        .map(|i| format!("completed\trefs/heads/feature/other-{i}\trefs/heads/{base}\th{i}\tm{i}\t{i}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn ado_lookups_into_one_target_share_one_list_query() {
    // One az call costs seconds; a protected landing asks about the same
    // target up to five times per run.
    let runner = az_scripted(&[Ok("active\trefs/heads/release/1.0.0\trefs/heads/main\taaa\t\t61\n\
completed\trefs/heads/finish/release-1.0.0-into-main\trefs/heads/main\tbbb\tccc\t60")]);
    let ado = ado(&runner);

    let open = ado.open_pr_to("release/1.0.0", "main").unwrap().unwrap();
    let landed = ado.merged_pr_to("finish/release-1.0.0-into-main", "main").unwrap().unwrap();
    let legacy = ado.merged_pr_to("release/1.0.0", "main").unwrap();

    assert!(open.ends_with("/pullrequest/61"), "got: {open}");
    assert!(landed.url.ends_with("/pullrequest/60"), "got: {}", landed.url);
    assert_eq!((landed.head_sha.as_str(), landed.merge_commit_sha.as_str()), ("bbb", "ccc"));
    assert_eq!(legacy, None, "a complete list without a completed PR means none exists");
    assert_eq!(az_calls(&runner), vec![az_prs_into("main")]);
}

#[test]
fn ado_the_newest_pr_of_each_status_decides() {
    let runner = az_scripted(&[Ok("active\trefs/heads/x\trefs/heads/main\th3\t\t9\n\
completed\trefs/heads/x\trefs/heads/main\th2\tm2\t7\n\
completed\trefs/heads/x\trefs/heads/main\th1\tm1\t5")]);
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
    let list = format!("completed\trefs/heads/x\trefs/heads/main\th\tm\t1001\n{}", truncated_list("main"));
    let runner = az_scripted(&[Ok(&list)]);

    let landed = ado(&runner).merged_pr_to("x", "main").unwrap().unwrap();

    assert!(landed.url.ends_with("/pullrequest/1001"), "got: {}", landed.url);
    assert_eq!(az_calls(&runner).len(), 1);
}

#[test]
fn ado_a_head_missing_from_a_truncated_list_loads_its_own_list_once() {
    // develop gathers more than 1000 PRs: a head absent from that page may
    // still have older PRs, and only its own list can tell.
    let list = truncated_list("develop");
    let own = "completed\trefs/heads/feature/x\trefs/heads/main\th9\tm9\t70\n\
active\trefs/heads/feature/x\trefs/heads/develop\th8\t\t61\n\
completed\trefs/heads/feature/x\trefs/heads/develop\tabc123\tdeadbeef\t49";
    let runner = az_scripted(&[Ok(&list), Ok(own)]);
    let ado = ado(&runner);

    let pr = ado.merged_pr_to("feature/x", "develop").unwrap().unwrap();
    let open = ado.open_pr_to("feature/x", "develop").unwrap().unwrap();

    assert_eq!(pr.url, "https://dev.azure.com/beans/Shop/_git/shop/pullrequest/49", "a landing into another base must not count");
    assert_eq!((pr.head_sha.as_str(), pr.merge_commit_sha.as_str()), ("abc123", "deadbeef"));
    assert!(open.ends_with("/pullrequest/61"), "got: {open}");
    assert_eq!(az_calls(&runner), vec![az_prs_into("develop"), az_prs_from("feature/x")]);
}

#[test]
fn ado_a_heads_own_list_keeps_the_failure_rules() {
    let list = truncated_list("main");

    let runner = az_scripted(&[Ok(&list), Err("access denied")]);
    let err = ado(&runner).merged_pr_to("x", "main").unwrap_err();
    assert!(err.contains("access denied") && err.contains("az login"), "got: {err}");

    let runner = az_scripted(&[Ok(&list), Err("access denied")]);
    let err = ado(&runner).open_pr_to("x", "main").unwrap_err();
    assert!(err.contains("access denied") && err.contains("az login"), "got: {err}");

    let runner = az_scripted(&[Ok(&list), Ok("")]);
    assert_eq!(ado(&runner).open_pr_to("x", "main").unwrap(), None);

    let runner = az_scripted(&[Ok(&list), Ok("active\trefs/heads/x\trefs/heads/main\th\t\tNone")]);
    let err = ado(&runner).open_pr_to("x", "main").unwrap_err();
    assert!(err.contains("Unexpected az pull request id: 'None'"), "got: {err}");
}

#[test]
fn ado_creating_a_pr_refreshes_the_lists_that_answered_for_it() {
    let new_pr = "active\trefs/heads/feature/x\trefs/heads/develop\th\t\t62";
    for (case, read_own_list_first, reload) in [
        ("answered from the target's list", false, az_prs_into("develop")),
        ("answered from the branch's own list", true, az_prs_into("develop")),
    ] {
        let runner = az_scripted(&[Ok(""), Ok("62"), Ok(new_pr)]);
        let ado = ado(&runner);
        if read_own_list_first {
            ado.merged_pr("feature/x").unwrap();
        }

        ado.create_or_get_pr("feature/x", "develop", "feat: x", PrBody::Empty).unwrap();
        let open = ado.open_pr_to("feature/x", "develop").unwrap().unwrap();

        assert!(open.ends_with("/pullrequest/62"), "{case}: the new PR must be visible; got: {open}");
        let calls = az_calls(&runner);
        assert_eq!(calls.last(), Some(&reload), "{case}: {calls:?}");
        assert_eq!(calls.len(), 3, "{case}: {calls:?}");
    }
}

#[test]
fn ado_a_malformed_list_row_is_an_error() {
    for (case, lookup) in [("target list", false), ("branch list", true)] {
        let runner = az_scripted(&[Ok("completed\trefs/heads/x\tmissing-fields")]);

        let err = if lookup { ado(&runner).merged_pr("x").map(|_| ()) } else { ado(&runner).open_pr_to("x", "main").map(|_| ()) }.unwrap_err();

        assert!(err.contains("Unexpected PR data from az:") && err.contains("missing-fields"), "{case}: {err}");
    }
}

#[test]
fn ado_prefetched_prs_answer_the_branchs_later_lookups() {
    let runner = az_scripted(&[Ok("completed\trefs/heads/feature/x\trefs/heads/develop\tabc\tdef\t49")]);
    let ado = ado(&runner);

    ado.prefetch_prs("feature/x").unwrap();
    assert_eq!(az_calls(&runner), vec![az_prs_from("feature/x")]);
    let pr = ado.merged_pr("feature/x").unwrap().unwrap();
    let open = ado.open_pr_to("feature/x", "develop").unwrap();

    assert_eq!((pr.base.as_str(), open), ("develop", None));
    assert_eq!(az_calls(&runner).len(), 1, "the lookups reuse the prefetched list");
}

#[test]
fn ado_a_failed_prefetch_names_the_login_commands() {
    let runner = az_scripted(&[Err("az repos pr list failed: TF400813")]);

    let err = ado(&runner).prefetch_prs("feature/x").unwrap_err();

    assert!(err.contains("TF400813") && err.contains("az login"), "got: {err}");
}

#[test]
fn gh_prefetch_asks_nothing() {
    // gh lookups are per pair and cheap; nothing is worth loading early.
    let runner = MockCliRunner::scripted(&[]);

    gh(&runner).prefetch_prs("feature/x").unwrap();

    assert!(runner.calls().is_empty());
}

#[test]
fn the_ado_extension_is_verified_once_before_the_first_az_call() {
    // Without the extension, az answers a repos command with an interactive
    // install prompt, hidden behind the captured output — gflow would hang.
    let runner = MockCliRunner::scripted(&[Ok("azure-devops 1.0.0"), Ok(""), Ok("")]);
    let ado = ado(&runner);

    ado.merged_pr("feature/x").unwrap();
    ado.open_pr_to("release/1.0.0", "develop").unwrap();

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
    let runner = az_scripted(&[Ok("active\trefs/heads/release/1.2.0\trefs/heads/develop\th\t\t61")]);

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
        vec![Ok("extension"), Ok("active\trefs/heads/feature/x\trefs/heads/develop\th\t\tNone")],
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
    for response in [Err("access denied"), Ok("active\trefs/heads/release/1.0.0\trefs/heads/main\th\t\tNone")] {
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
        assert!(error.contains("Unexpected PR data from az:"), "got: {error}");
        assert!(error.contains("missing-fields"));
    }
}
