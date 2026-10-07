use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use super::{resolve_body_file, CliRunner, HostingPlatform, LandedPr, MergedPr, PrBody, Result};

/// One az call costs seconds, so lookups are answered from cached lists: all
/// PRs into a target, or all PRs from a head. A full page into a target means
/// older PRs fell past the limit, so a head missing from it loads that head's
/// own list. One branch never reaches the limit, so a head's list is complete.
const PR_LIST_LIMIT: usize = 1000;

const AUTH_REMEDY: &str = "If you are not signed in, run 'az login' (or 'az devops login' with a PAT), then re-run gflow.";

pub struct AzureDevOps<'a> {
    org: String,
    project: String,
    repo: String,
    runner: &'a dyn CliRunner,
    extension_verified: Cell<bool>,
    prs_into: RefCell<HashMap<String, Rc<PrList>>>,
    prs_from: RefCell<HashMap<String, Rc<PrList>>>,
}

/// Newest first, as az lists them.
struct PrList {
    rows: Vec<PrRow>,
    complete: bool,
}

#[derive(Clone)]
struct PrRow {
    line: String,
    status: String,
    source: String,
    target: String,
    head_sha: String,
    merge_sha: String,
    id: String,
}

impl<'a> AzureDevOps<'a> {
    pub fn new(org: String, project: String, repo: String, runner: &'a dyn CliRunner) -> Self {
        Self {
            org, project, repo, runner,
            extension_verified: Cell::new(false),
            prs_into: RefCell::default(),
            prs_from: RefCell::default(),
        }
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
        self.verify_extension()?;
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        self.runner.run("az", &args).map_err(|e| format!("{e}\n{AUTH_REMEDY}"))
    }

    /// Without the extension, az meets a repos command with an interactive
    /// install prompt that the captured output hides — gflow would hang.
    fn verify_extension(&self) -> Result<()> {
        if self.extension_verified.get() {
            return Ok(());
        }
        self.runner.run("az", &["extension", "show", "--name", "azure-devops"])
            .map_err(|e| format!("{e}\nThe Azure DevOps CLI extension is required. Run 'az extension add --name azure-devops'."))?;
        self.extension_verified.set(true);
        Ok(())
    }

    fn prs_into(&self, base: &str) -> Result<Rc<PrList>> {
        self.cached_list(&self.prs_into, "--target-branch", base)
    }

    fn prs_from(&self, head: &str) -> Result<Rc<PrList>> {
        self.cached_list(&self.prs_from, "--source-branch", head)
    }

    fn cached_list(&self, cache: &RefCell<HashMap<String, Rc<PrList>>>, filter: &str, branch: &str) -> Result<Rc<PrList>> {
        if let Some(prs) = cache.borrow().get(branch) {
            return Ok(Rc::clone(prs));
        }
        let mut args: Vec<String> = vec!["repos".into(), "pr".into(), "list".into()];
        args.extend(self.repo_args());
        args.extend([
            filter.into(), branch.into(),
            "--status".into(), "all".into(),
            "--top".into(), PR_LIST_LIMIT.to_string(),
            "--query".into(), "[].[status, sourceRefName, targetRefName, lastMergeSourceCommit.commitId, lastMergeCommit.commitId, pullRequestId]".into(),
            "-o".into(), "tsv".into(),
        ]);
        let rows = self.run_az(&args)?
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(parse_pr_row)
            .collect::<Result<Vec<_>>>()?;
        let prs = Rc::new(PrList { complete: rows.len() < PR_LIST_LIMIT, rows });
        cache.borrow_mut().insert(branch.to_string(), Rc::clone(&prs));
        Ok(prs)
    }

    /// The newest `head`→`base` PR with `status`: from the head's own list
    /// when it is loaded, else from the target's list, else — past a full
    /// target page — from the head's own list.
    fn newest(&self, head: &str, base: &str, status: &str) -> Result<Option<PrRow>> {
        let find = |prs: &PrList| prs.rows.iter()
            .find(|row| row.source == head && row.target == base && row.status == status)
            .cloned();
        let own = self.prs_from.borrow().get(head).cloned();
        if let Some(own) = own {
            return Ok(find(&own));
        }
        let into = self.prs_into(base)?;
        match find(&into) {
            Some(row) => Ok(Some(row)),
            None if into.complete => Ok(None),
            None => Ok(find(&*self.prs_from(head)?)),
        }
    }

    fn repo_args(&self) -> Vec<String> {
        vec![
            "--organization".into(), self.org_url(),
            "--project".into(), self.project.clone(),
            "--repository".into(), self.repo.clone(),
        ]
    }

    /// A completed PR's row, with the commits a finish acts on. az renders
    /// nulls as empty tsv fields (or the literal "None").
    fn landed_pr(&self, row: &PrRow) -> Result<LandedPr> {
        if row.head_sha.is_empty() || row.head_sha == "None" {
            return Err(format!("Unexpected merge source commit from az: '{}'", row.line));
        }
        if row.merge_sha.is_empty() || row.merge_sha == "None" {
            return Err(format!("Unexpected merge commit from az: '{}'", row.line));
        }
        Ok(LandedPr {
            url: self.pr_url(validate_pr_id(&row.id)?),
            head_sha: row.head_sha.clone(),
            merge_commit_sha: row.merge_sha.clone(),
        })
    }
}

fn parse_pr_row(line: &str) -> Result<PrRow> {
    let fields: Vec<&str> = line.split('\t').collect();
    let [status, source, target, head_sha, merge_sha, id] = fields.as_slice() else {
        return Err(format!("Unexpected PR data from az: '{line}'"));
    };
    let branch = |name: &str| name.strip_prefix("refs/heads/").unwrap_or(name).to_string();
    Ok(PrRow {
        line: line.to_string(),
        status: status.to_string(),
        source: branch(source),
        target: branch(target),
        head_sha: head_sha.to_string(),
        merge_sha: merge_sha.to_string(),
        id: id.to_string(),
    })
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
        if let Some(url) = self.open_pr_to(head, base)? {
            return Ok(url);
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
        self.prs_into.borrow_mut().remove(base);
        self.prs_from.borrow_mut().remove(head);
        Ok(self.pr_url(validate_pr_id(&created)?))
    }

    /// The branch's newest PR decides, into any base: a newer active or
    /// abandoned PR means the branch is still in play. `completed` is ADO's
    /// merged status.
    fn merged_pr(&self, head: &str) -> Result<Option<MergedPr>> {
        let own = self.prs_from(head)?;
        let Some(row) = own.rows.first().filter(|row| row.status == "completed") else {
            return Ok(None);
        };
        let LandedPr { url, head_sha, merge_commit_sha } = self.landed_pr(row)?;
        Ok(Some(MergedPr { url, head_sha, merge_commit_sha, base: row.target.clone() }))
    }

    /// The newest *completed* head→base PR: a newer active or abandoned PR must
    /// not erase the fact that this leg already landed — unlike `merged_pr`,
    /// where a newer PR does mean the work branch is still in play.
    fn merged_pr_to(&self, head: &str, base: &str) -> Result<Option<LandedPr>> {
        self.newest(head, base, "completed")?.map(|row| self.landed_pr(&row)).transpose()
    }

    fn open_pr_to(&self, head: &str, base: &str) -> Result<Option<String>> {
        self.newest(head, base, "active")?
            .map(|row| validate_pr_id(&row.id).map(|id| self.pr_url(id)))
            .transpose()
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
}
