use super::{resolve_body_file, CliRunner, PrBody, HostingPlatform, LandedPr, MergedPr, Result};

const AUTH_REMEDY: &str = "If authentication expired, run 'gh auth login', then re-run 'gflow finish'.";

pub struct GitHub<'a> {
    runner: &'a dyn CliRunner,
}

impl<'a> GitHub<'a> {
    pub fn new(runner: &'a dyn CliRunner) -> Self {
        Self { runner }
    }

    fn run_gh(&self, args: &[&str]) -> Result<String> {
        self.runner.run("gh", args)
    }
}

impl HostingPlatform for GitHub<'_> {
    fn create_or_get_pr(&self, head: &str, base: &str, title: &str, body: PrBody<'_>) -> Result<String> {
        // The probe must filter by base: a fan-out source branch (a protected
        // hotfix landing on both main and a release branch) can have open PRs
        // to more than one base at once, and a head-only lookup would return
        // an arbitrary one. ADO's create_or_get_pr already filters by both
        // --source-branch and --target-branch.
        let existing = self
            .run_gh(&["pr", "list", "--head", head, "--base", base, "--state", "open", "--limit", "1", "--json", "url", "--jq", ".[0].url // empty"])
            .map_err(|e| format!("Could not check for an existing PR: {e}\n{AUTH_REMEDY}"))?;
        if !existing.is_empty() {
            return Ok(existing);
        }

        let git_default_paths = [
            ".github/PULL_REQUEST_TEMPLATE.md",
            ".github/pull_request_template.md",
            "PULL_REQUEST_TEMPLATE.md",
            "pull_request_template.md",
            "docs/pull_request_template.md",
        ];
        let body_file = resolve_body_file(body, &git_default_paths);

        if let Some(path) = body_file {
            self.run_gh(&["pr", "create", "--head", head, "--base", base, "--title", title, "--body-file", &path])
        } else {
            self.run_gh(&["pr", "create", "--head", head, "--base", base, "--title", title, "--body", ""])
        }
    }

    fn merged_pr(&self, head: &str) -> Result<Option<MergedPr>> {
        // The branch's most recent PR decides: `gh pr list` returns newest first,
        // and the jq filter drops anything not MERGED (open PR → the branch is
        // still in play; closed-unmerged → a fresh PR is the right move).
        let line = self
            .run_gh(&[
                "pr", "list", "--head", head, "--state", "all", "--limit", "1",
                "--json", "url,state,headRefOid,mergeCommit,baseRefName",
                "--jq", r#".[0] | select(.state == "MERGED") | [.url, .headRefOid, .mergeCommit.oid, .baseRefName] | @tsv"#,
            ])
            .map_err(|e| format!("Could not check for a merged PR: {e}\n{AUTH_REMEDY}"))?;
        parse_merged_pr(&line)
    }

    fn merged_pr_to(&self, head: &str, base: &str) -> Result<Option<LandedPr>> {
        // Filtering by --base narrows to exactly this landing; --limit 1 still
        // gives the newest such PR. `merged`, not `all`: this answers "has this
        // leg landed", which a newer open or abandoned PR must not erase —
        // unlike `merged_pr`, where a newer PR does mean the work branch is
        // still in play.
        let line = self
            .run_gh(&[
                "pr", "list", "--head", head, "--base", base, "--state", "merged", "--limit", "1",
                "--json", "url,state,headRefOid,mergeCommit",
                "--jq", r#".[0] | select(.state == "MERGED") | [.url, .headRefOid, .mergeCommit.oid] | @tsv"#,
            ])
            .map_err(|e| format!("Could not check for a merged PR: {e}\n{AUTH_REMEDY}"))?;
        parse_landed_pr(&line)
    }

    fn open_pr_to(&self, head: &str, base: &str) -> Result<Option<String>> {
        let url = self
            .run_gh(&["pr", "list", "--head", head, "--base", base, "--state", "open", "--limit", "1", "--json", "url", "--jq", ".[0].url // empty"])
            .map_err(|e| format!("Could not check for an open PR: {e}\n{AUTH_REMEDY}"))?;
        let url = url.trim();
        Ok(if url.is_empty() { None } else { Some(url.to_string()) })
    }

    fn check_auth(&self) -> Result<()> {
        self.run_gh(&["auth", "status"]).map(|_| ())
    }
}

/// Parse the `url<TAB>headRefOid<TAB>mergeCommit.oid<TAB>baseRefName` line the
/// merged-PR jq filter emits. Empty output is the normal "no merged PR" case;
/// anything else that isn't four non-empty fields is a hard error (never guess
/// about cleanup).
fn parse_merged_pr(line: &str) -> Result<Option<MergedPr>> {
    let line = line.trim();
    if line.is_empty() {
        return Ok(None);
    }
    match line.split('\t').collect::<Vec<_>>().as_slice() {
        [url, sha, merge_sha, base] if !url.is_empty() && !sha.is_empty() && !merge_sha.is_empty() && !base.is_empty() => {
            Ok(Some(MergedPr {
                url: url.to_string(),
                head_sha: sha.to_string(),
                merge_commit_sha: merge_sha.to_string(),
                base: base.to_string(),
            }))
        }
        _ => Err(format!("Unexpected merged-PR data from gh: '{line}'")),
    }
}

/// Parse the `url<TAB>headRefOid<TAB>mergeCommit.oid` line the `merged_pr_to`
/// jq filter emits. Same empty/malformed rules as `parse_merged_pr`.
fn parse_landed_pr(line: &str) -> Result<Option<LandedPr>> {
    let line = line.trim();
    if line.is_empty() {
        return Ok(None);
    }
    match line.split('\t').collect::<Vec<_>>().as_slice() {
        [url, head_sha, merge_commit_sha] if !url.is_empty() && !head_sha.is_empty() && !merge_commit_sha.is_empty() => {
            Ok(Some(LandedPr {
                url: url.to_string(),
                head_sha: head_sha.to_string(),
                merge_commit_sha: merge_commit_sha.to_string(),
            }))
        }
        _ => Err(format!("Unexpected merged-PR data from gh: '{line}'")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_output_means_no_merged_pr() {
        assert_eq!(parse_merged_pr(""), Ok(None));
        assert_eq!(parse_merged_pr("  \n"), Ok(None));
    }

    #[test]
    fn four_fields_parse_into_merged_pr() {
        let pr = parse_merged_pr("https://github.com/o/r/pull/49\tabc123\tdeadbeef\tdevelop").unwrap().unwrap();
        assert_eq!(pr.url, "https://github.com/o/r/pull/49");
        assert_eq!(pr.head_sha, "abc123");
        assert_eq!(pr.merge_commit_sha, "deadbeef");
        assert_eq!(pr.base, "develop");
    }

    #[test]
    fn malformed_output_is_a_hard_error() {
        assert!(parse_merged_pr("only-a-url").is_err());
        assert!(parse_merged_pr("url\tabc123\tdevelop").is_err());
        assert!(parse_merged_pr("url\tabc123\t\tdevelop").is_err());
    }

    #[test]
    fn empty_output_means_no_landed_pr() {
        assert_eq!(parse_landed_pr(""), Ok(None));
        assert_eq!(parse_landed_pr("  \n"), Ok(None));
    }

    #[test]
    fn three_fields_parse_into_landed_pr() {
        let pr = parse_landed_pr("https://github.com/o/r/pull/49\tabc123\tdeadbeef").unwrap().unwrap();
        assert_eq!(pr.url, "https://github.com/o/r/pull/49");
        assert_eq!(pr.head_sha, "abc123");
        assert_eq!(pr.merge_commit_sha, "deadbeef");
    }

    #[test]
    fn malformed_landed_pr_output_is_a_hard_error() {
        assert!(parse_landed_pr("only-a-url").is_err());
        assert!(parse_landed_pr("url\t\tdeadbeef").is_err());
    }
}
