use super::{resolve_body_file, CliRunner, HostingPlatform, LandedPr, MergedPr, PrBody, Result};

pub struct AzureDevOps<'a> {
    org: String,
    project: String,
    repo: String,
    runner: &'a dyn CliRunner,
}

impl<'a> AzureDevOps<'a> {
    pub fn new(org: String, project: String, repo: String, runner: &'a dyn CliRunner) -> Self {
        Self { org, project, repo, runner }
    }

    fn org_url(&self) -> String {
        // Valid for legacy {org}.visualstudio.com organizations too.
        format!("https://dev.azure.com/{}", self.org)
    }

    /// Canonical PR web URL, built from the coordinates parsed out of the remote.
    /// az's `repository.webUrl` is unreliable (absent in `pr list` responses,
    /// legacy-format for visualstudio.com orgs), so it is never used.
    fn pr_url(&self, id: &str) -> String {
        format!(
            "{}/{}/_git/{}/pullrequest/{id}",
            self.org_url(),
            encode_segment(&self.project),
            encode_segment(&self.repo),
        )
    }

    fn run_az(&self, args: &[String]) -> Result<String> {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        self.runner.run("az", &args)
    }

    fn repo_args(&self) -> Vec<String> {
        vec![
            "--organization".into(), self.org_url(),
            "--project".into(), self.project.clone(),
            "--repository".into(), self.repo.clone(),
        ]
    }

    /// Parse the `status<TAB>headSha<TAB>mergeCommitSha<TAB>targetRefName<TAB>id`
    /// tsv row of the merged-PR query. Empty output is the normal "no PR" case; a
    /// row whose status isn't `completed` means the newest PR is open/abandoned →
    /// `None`.
    ///
    /// The queries feeding this select over a `[0:1]` slice, never `[0]`: az's tsv
    /// writer prints a flat list of scalars one value per line, and only a list of
    /// lists as a tab-separated row.
    fn parse_merged_pr_row(&self, row: &str) -> Result<Option<MergedPr>> {
        let row = row.trim();
        if row.is_empty() {
            return Ok(None);
        }
        let fields: Vec<&str> = row.split('\t').collect();
        let [status, sha, merge_sha, target, id] = fields.as_slice() else {
            return Err(format!("Unexpected merged-PR data from az: '{row}'"));
        };
        if *status != "completed" {
            return Ok(None);
        }
        // az renders nulls as empty tsv fields (or the literal "None").
        if sha.is_empty() || *sha == "None" {
            return Err(format!("Unexpected merge source commit from az: '{row}'"));
        }
        if merge_sha.is_empty() || *merge_sha == "None" {
            return Err(format!("Unexpected merge commit from az: '{row}'"));
        }
        let base = target.strip_prefix("refs/heads/").unwrap_or(target).to_string();
        Ok(Some(MergedPr {
            url: self.pr_url(validate_pr_id(id)?),
            head_sha: sha.to_string(),
            merge_commit_sha: merge_sha.to_string(),
            base,
        }))
    }

    /// Parse the `status<TAB>headSha<TAB>mergeCommitSha<TAB>id` tsv row of the
    /// base-filtered merged-PR query. Same empty/status/id rules as
    /// `parse_merged_pr_row`, plus the merge commit SHA gets the same
    /// null-guard as the head SHA.
    fn parse_landed_pr_row(&self, row: &str) -> Result<Option<LandedPr>> {
        let row = row.trim();
        if row.is_empty() {
            return Ok(None);
        }
        let fields: Vec<&str> = row.split('\t').collect();
        let [status, head_sha, merge_commit_sha, id] = fields.as_slice() else {
            return Err(format!("Unexpected merged-PR data from az: '{row}'"));
        };
        if *status != "completed" {
            return Ok(None);
        }
        if head_sha.is_empty() || *head_sha == "None" {
            return Err(format!("Unexpected merge source commit from az: '{row}'"));
        }
        if merge_commit_sha.is_empty() || *merge_commit_sha == "None" {
            return Err(format!("Unexpected merge commit from az: '{row}'"));
        }
        Ok(Some(LandedPr {
            url: self.pr_url(validate_pr_id(id)?),
            head_sha: head_sha.to_string(),
            merge_commit_sha: merge_commit_sha.to_string(),
        }))
    }
}

/// URL-encode a path segment. Spaces (decoded during remote-URL detection) are
/// the only realistic case in ADO project/repo names.
fn encode_segment(s: &str) -> String {
    s.replace(' ', "%20")
}

/// Validate a `--query pullRequestId -o tsv` result: a single line of digits.
/// Anything else (empty, az's "None" for null) is a hard error.
fn validate_pr_id(id: &str) -> Result<&str> {
    let id = id.trim();
    if !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()) {
        Ok(id)
    } else {
        Err(format!("Unexpected az pull request id: '{id}'"))
    }
}

/// az's `--description` is a list argument where each element becomes one line.
fn description_args(body: &str) -> Vec<String> {
    let mut args = vec!["--description".to_string()];
    args.extend(body.split('\n').map(|l| l.to_string()));
    args
}

impl HostingPlatform for AzureDevOps<'_> {
    fn create_or_get_pr(&self, head: &str, base: &str, title: &str, body: PrBody<'_>) -> Result<String> {
        // Return the existing active PR for this head/base if there is one.
        let mut list_args: Vec<String> = vec!["repos".into(), "pr".into(), "list".into()];
        list_args.extend(self.repo_args());
        list_args.extend([
            "--source-branch".into(), head.into(),
            "--target-branch".into(), base.into(),
            "--status".into(), "active".into(),
            "--query".into(), "[0].pullRequestId".into(),
            "-o".into(), "tsv".into(),
        ]);
        let existing = self.run_az(&list_args)?;
        if !existing.is_empty() {
            return Ok(self.pr_url(validate_pr_id(&existing)?));
        }

        let default_paths = [
            ".azuredevops/pull_request_template.md",
            ".azuredevops/PULL_REQUEST_TEMPLATE.md",
            ".vsts/pull_request_template.md",
            "pull_request_template.md",
            "PULL_REQUEST_TEMPLATE.md",
            "docs/pull_request_template.md",
        ];
        let body_file = resolve_body_file(body, &default_paths);
        let description = match body_file {
            Some(path) => std::fs::read_to_string(&path)
                .map_err(|e| format!("Failed to read PR template '{path}': {e}"))?,
            None => String::new(),
        };

        let mut create_args: Vec<String> = vec!["repos".into(), "pr".into(), "create".into()];
        create_args.extend(self.repo_args());
        create_args.extend([
            "--source-branch".into(), head.into(),
            "--target-branch".into(), base.into(),
            "--title".into(), title.into(),
        ]);
        create_args.extend(description_args(&description));
        create_args.extend([
            "--query".into(), "pullRequestId".into(),
            "-o".into(), "tsv".into(),
        ]);
        let created = self.run_az(&create_args)?;
        Ok(self.pr_url(validate_pr_id(&created)?))
    }

    fn merged_pr(&self, head: &str) -> Result<Option<MergedPr>> {
        // Newest PR for this source branch decides (az lists newest first);
        // `completed` is ADO's merged status.
        let mut args: Vec<String> = vec!["repos".into(), "pr".into(), "list".into()];
        args.extend(self.repo_args());
        args.extend([
            "--source-branch".into(), head.into(),
            "--status".into(), "all".into(),
            "--query".into(), "[0:1].[status, lastMergeSourceCommit.commitId, lastMergeCommit.commitId, targetRefName, pullRequestId]".into(),
            "-o".into(), "tsv".into(),
        ]);
        let row = self.run_az(&args)?;
        self.parse_merged_pr_row(&row)
    }

    fn merged_pr_to(&self, head: &str, base: &str) -> Result<Option<LandedPr>> {
        // --target-branch narrows to exactly this landing; az still lists
        // newest first, so [0] is the newest such PR. `completed`, not `all`:
        // this answers "has this leg landed", which a newer active or abandoned
        // PR must not erase — unlike `merged_pr`, where a newer PR does mean
        // the work branch is still in play.
        let mut args: Vec<String> = vec!["repos".into(), "pr".into(), "list".into()];
        args.extend(self.repo_args());
        args.extend([
            "--source-branch".into(), head.into(),
            "--target-branch".into(), base.into(),
            "--status".into(), "completed".into(),
            "--query".into(), "[0:1].[status, lastMergeSourceCommit.commitId, lastMergeCommit.commitId, pullRequestId]".into(),
            "-o".into(), "tsv".into(),
        ]);
        let row = self.run_az(&args)?;
        self.parse_landed_pr_row(&row)
    }

    fn open_pr_to(&self, head: &str, base: &str) -> Result<Option<String>> {
        let mut args: Vec<String> = vec!["repos".into(), "pr".into(), "list".into()];
        args.extend(self.repo_args());
        args.extend([
            "--source-branch".into(), head.into(),
            "--target-branch".into(), base.into(),
            "--status".into(), "active".into(),
            "--query".into(), "[0].pullRequestId".into(),
            "-o".into(), "tsv".into(),
        ]);
        let id = self.run_az(&args)?;
        let id = id.trim();
        Ok(if id.is_empty() { None } else { Some(self.pr_url(validate_pr_id(id)?)) })
    }

    fn check_auth(&self) -> Result<()> {
        // Explicit extension check first: it also prevents az's interactive
        // dynamic-install prompt from firing inside a non-tty command later.
        self.run_az(&["extension".into(), "show".into(), "--name".into(), "azure-devops".into()])
            .map_err(|e| format!("Azure DevOps CLI extension is missing. Run 'az extension add --name azure-devops'.\n{e}"))?;
        // Probe actual repo access rather than `az account show`: PAT auth via
        // `az devops login` (or AZURE_DEVOPS_EXT_PAT) works without an `az login`
        // session, which `account show` would wrongly report as unauthenticated.
        let mut args: Vec<String> = vec!["repos".into(), "show".into()];
        args.extend(self.repo_args());
        args.extend(["--query".into(), "id".into(), "-o".into(), "tsv".into()]);
        self.run_az(&args).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hosting::SystemCli;

    // These exercise the pure helpers (URL synthesis, tsv parsing); the runner is
    // never invoked. Provider policy that *does* run the CLI lives in
    // tests/hosting_test.rs against a scripted runner.

    #[test]
    fn pr_url_is_canonical_dev_azure_form() {
        // Legacy visualstudio.com orgs get the canonical URL too — it redirects.
        let ado = AzureDevOps::new("wilko".into(), "Shuttel".into(), "Shuttel".into(), &SystemCli);
        assert_eq!(ado.pr_url("2662"), "https://dev.azure.com/wilko/Shuttel/_git/Shuttel/pullrequest/2662");
    }

    #[test]
    fn pr_url_encodes_spaces_in_project_and_repo() {
        let ado = AzureDevOps::new("beans".into(), "My Shop".into(), "the repo".into(), &SystemCli);
        assert_eq!(ado.pr_url("7"), "https://dev.azure.com/beans/My%20Shop/_git/the%20repo/pullrequest/7");
    }

    #[test]
    fn validate_pr_id_accepts_digits_only() {
        assert_eq!(validate_pr_id("2662"), Ok("2662"));
        assert_eq!(validate_pr_id(" 42\n"), Ok("42"));
        assert!(validate_pr_id("").is_err());
        assert!(validate_pr_id("None").is_err());
        assert!(validate_pr_id("https://x\t42").is_err());
    }

    #[test]
    fn description_args_one_arg_per_line() {
        assert_eq!(
            description_args("line one\nline two\n\nline four"),
            vec!["--description", "line one", "line two", "", "line four"],
        );
    }

    #[test]
    fn description_args_empty_body_is_single_empty_line() {
        assert_eq!(description_args(""), vec!["--description", ""]);
    }

    fn ado() -> AzureDevOps<'static> {
        AzureDevOps::new("beans".into(), "Shop".into(), "shop".into(), &SystemCli)
    }

    #[test]
    fn merged_pr_row_empty_means_no_pr() {
        assert_eq!(ado().parse_merged_pr_row(""), Ok(None));
    }

    #[test]
    fn merged_pr_row_completed_parses_with_synthesized_url_and_short_base() {
        let pr = ado().parse_merged_pr_row("completed\tabc123\tdeadbeef\trefs/heads/develop\t49").unwrap().unwrap();
        assert_eq!(pr.url, "https://dev.azure.com/beans/Shop/_git/shop/pullrequest/49");
        assert_eq!(pr.head_sha, "abc123");
        assert_eq!(pr.merge_commit_sha, "deadbeef");
        assert_eq!(pr.base, "develop");
    }

    #[test]
    fn merged_pr_row_active_or_abandoned_is_none() {
        assert_eq!(ado().parse_merged_pr_row("active\tabc\tdeadbeef\trefs/heads/develop\t49"), Ok(None));
        assert_eq!(ado().parse_merged_pr_row("abandoned\tabc\tdeadbeef\trefs/heads/develop\t49"), Ok(None));
    }

    #[test]
    fn merged_pr_row_missing_commit_or_bad_id_is_a_hard_error() {
        assert!(ado().parse_merged_pr_row("completed\t\tdeadbeef\trefs/heads/develop\t49").is_err());
        assert!(ado().parse_merged_pr_row("completed\tNone\tdeadbeef\trefs/heads/develop\t49").is_err());
        assert!(ado().parse_merged_pr_row("completed\tabc\t\trefs/heads/develop\t49").is_err());
        assert!(ado().parse_merged_pr_row("completed\tabc\tNone\trefs/heads/develop\t49").is_err());
        assert!(ado().parse_merged_pr_row("completed\tabc\tdeadbeef\trefs/heads/develop\tNone").is_err());
    }

    #[test]
    fn landed_pr_row_empty_means_no_pr() {
        assert_eq!(ado().parse_landed_pr_row(""), Ok(None));
    }

    #[test]
    fn landed_pr_row_completed_parses_with_synthesized_url_and_merge_commit() {
        let pr = ado().parse_landed_pr_row("completed\tabc123\tdeadbeef\t49").unwrap().unwrap();
        assert_eq!(pr.url, "https://dev.azure.com/beans/Shop/_git/shop/pullrequest/49");
        assert_eq!(pr.head_sha, "abc123");
        assert_eq!(pr.merge_commit_sha, "deadbeef");
    }

    #[test]
    fn landed_pr_row_active_or_abandoned_is_none() {
        assert_eq!(ado().parse_landed_pr_row("active\tabc\tdeadbeef\t49"), Ok(None));
        assert_eq!(ado().parse_landed_pr_row("abandoned\tabc\tdeadbeef\t49"), Ok(None));
    }

    #[test]
    fn landed_pr_row_missing_shas_or_bad_id_is_a_hard_error() {
        assert!(ado().parse_landed_pr_row("completed\t\tdeadbeef\t49").is_err());
        assert!(ado().parse_landed_pr_row("completed\tNone\tdeadbeef\t49").is_err());
        assert!(ado().parse_landed_pr_row("completed\tabc\t\t49").is_err());
        assert!(ado().parse_landed_pr_row("completed\tabc\tNone\t49").is_err());
        assert!(ado().parse_landed_pr_row("completed\tabc\tdeadbeef\tNone").is_err());
    }
}
