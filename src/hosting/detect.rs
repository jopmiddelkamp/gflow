//! Hosting-provider detection.
//!
//! The provider is auto-detected from the `origin` remote URL. The git config
//! key `gflow.hosting.provider` (`github` | `devops`) overrides detection for
//! edge cases (e.g. GitHub Enterprise domains).

use crate::git::Git;
use super::Result;

const PROVIDER_KEY: &str = "gflow.hosting.provider";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Provider {
    GitHub,
    AzureDevOps { org: String, project: String, repo: String },
}

/// Detect the hosting provider from the `gflow.hosting.provider` override and
/// the `origin` remote URL.
pub fn detect(git: &dyn Git) -> Result<Provider> {
    let override_val = git.get_config(PROVIDER_KEY)?;
    let remote = git.remote_url().ok();
    resolve(remote.as_deref(), override_val.as_deref())
}

/// Pure resolution core (unit-tested without a git repo).
///
/// - Override `github` → GitHub unconditionally.
/// - Override `devops` → the remote must be an Azure DevOps URL (az needs the
///   org/project/repo coordinates) or an error is returned.
/// - No override → parse the remote; unrecognized hosts and missing remotes
///   fall back to GitHub (preserves pre-detection behavior).
fn resolve(remote_url: Option<&str>, override_val: Option<&str>) -> Result<Provider> {
    match override_val.map(str::trim) {
        Some("github") => Ok(Provider::GitHub),
        Some("devops") => match remote_url.and_then(parse_remote) {
            Some(p @ Provider::AzureDevOps { .. }) => Ok(p),
            _ => Err(format!(
                "{PROVIDER_KEY} is 'devops' but the Azure DevOps organization/project/repository could not be determined from the origin remote URL."
            )),
        },
        Some(other) => Err(format!(
            "Invalid {PROVIDER_KEY} value '{other}'. Valid values: github, devops."
        )),
        None => Ok(remote_url.and_then(parse_remote).unwrap_or(Provider::GitHub)),
    }
}

/// Parse a remote URL into a provider. Returns `None` for unrecognized hosts.
fn parse_remote(url: &str) -> Option<Provider> {
    let url = url.trim().trim_end_matches('/');
    let url = url.strip_suffix(".git").unwrap_or(url);

    // SSH scp-like syntax: git@host:path
    if let Some(rest) = url.strip_prefix("git@") {
        let (host, path) = rest.split_once(':')?;
        return parse_host_path(host, path);
    }

    // URL syntax: scheme://[user@]host/path
    let rest = url.split_once("://").map(|(_, r)| r)?;
    let rest = rest.rsplit_once('@').map_or(rest, |(_, r)| r);
    let (host, path) = rest.split_once('/')?;
    parse_host_path(host, path)
}

fn parse_host_path(host: &str, path: &str) -> Option<Provider> {
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    match host {
        "github.com" => Some(Provider::GitHub),
        // https://dev.azure.com/{org}/{project}/_git/{repo}
        "dev.azure.com" => match segments.as_slice() {
            [org, project, "_git", repo] => Some(azure(org, project, repo)),
            _ => None,
        },
        // git@ssh.dev.azure.com:v3/{org}/{project}/{repo}
        "ssh.dev.azure.com" => match segments.as_slice() {
            ["v3", org, project, repo] => Some(azure(org, project, repo)),
            _ => None,
        },
        // Legacy https://{org}.visualstudio.com/[DefaultCollection/]{project}/_git/{repo}
        _ => {
            let org = host.strip_suffix(".visualstudio.com")?;
            match segments.as_slice() {
                [project, "_git", repo] | ["DefaultCollection", project, "_git", repo] => {
                    Some(azure(org, project, repo))
                }
                _ => None,
            }
        }
    }
}

fn azure(org: &str, project: &str, repo: &str) -> Provider {
    Provider::AzureDevOps {
        org: org.to_string(),
        // https remotes percent-encode spaces in project names; az wants the
        // display name. Spaces are the only realistic case in ADO names.
        project: project.replace("%20", " "),
        repo: repo.replace("%20", " "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ado(org: &str, project: &str, repo: &str) -> Provider {
        Provider::AzureDevOps {
            org: org.to_string(),
            project: project.to_string(),
            repo: repo.to_string(),
        }
    }

    #[test]
    fn github_https() {
        assert_eq!(parse_remote("https://github.com/acme/repo.git"), Some(Provider::GitHub));
        assert_eq!(parse_remote("https://github.com/acme/repo"), Some(Provider::GitHub));
    }

    #[test]
    fn github_ssh() {
        assert_eq!(parse_remote("git@github.com:acme/repo.git"), Some(Provider::GitHub));
        assert_eq!(parse_remote("ssh://git@github.com/acme/repo.git"), Some(Provider::GitHub));
    }

    #[test]
    fn devops_https() {
        assert_eq!(
            parse_remote("https://dev.azure.com/beans/Shop/_git/backend"),
            Some(ado("beans", "Shop", "backend")),
        );
    }

    #[test]
    fn devops_https_with_user() {
        assert_eq!(
            parse_remote("https://beans@dev.azure.com/beans/Shop/_git/backend"),
            Some(ado("beans", "Shop", "backend")),
        );
    }

    #[test]
    fn devops_https_percent_encoded_project() {
        assert_eq!(
            parse_remote("https://dev.azure.com/beans/My%20Shop/_git/backend"),
            Some(ado("beans", "My Shop", "backend")),
        );
    }

    #[test]
    fn devops_ssh_v3() {
        assert_eq!(
            parse_remote("git@ssh.dev.azure.com:v3/beans/Shop/backend"),
            Some(ado("beans", "Shop", "backend")),
        );
        assert_eq!(
            parse_remote("ssh://git@ssh.dev.azure.com/v3/beans/Shop/backend"),
            Some(ado("beans", "Shop", "backend")),
        );
    }

    #[test]
    fn devops_legacy_visualstudio() {
        assert_eq!(
            parse_remote("https://beans.visualstudio.com/Shop/_git/backend"),
            Some(ado("beans", "Shop", "backend")),
        );
        assert_eq!(
            parse_remote("https://beans.visualstudio.com/DefaultCollection/Shop/_git/backend"),
            Some(ado("beans", "Shop", "backend")),
        );
    }

    #[test]
    fn trailing_slash_and_git_suffix_stripped() {
        assert_eq!(
            parse_remote("https://dev.azure.com/beans/Shop/_git/backend.git/"),
            Some(ado("beans", "Shop", "backend")),
        );
    }

    #[test]
    fn unrecognized_hosts_yield_none() {
        assert_eq!(parse_remote("https://gitlab.com/acme/repo.git"), None);
        assert_eq!(parse_remote("https://github.enterprise.corp/acme/repo"), None);
        assert_eq!(parse_remote("not-a-url"), None);
        assert_eq!(parse_remote("https://dev.azure.com/beans/backend"), None);
    }

    #[test]
    fn no_override_unrecognized_or_missing_remote_defaults_to_github() {
        assert_eq!(resolve(Some("https://gitlab.com/a/b"), None), Ok(Provider::GitHub));
        assert_eq!(resolve(None, None), Ok(Provider::GitHub));
    }

    #[test]
    fn no_override_ado_remote_detected() {
        assert_eq!(
            resolve(Some("https://dev.azure.com/beans/Shop/_git/backend"), None),
            Ok(ado("beans", "Shop", "backend")),
        );
    }

    #[test]
    fn override_github_wins_over_ado_remote() {
        assert_eq!(
            resolve(Some("https://dev.azure.com/beans/Shop/_git/backend"), Some("github")),
            Ok(Provider::GitHub),
        );
    }

    #[test]
    fn override_devops_requires_parsable_ado_remote() {
        let err = resolve(Some("https://github.com/acme/repo"), Some("devops")).unwrap_err();
        assert!(err.contains("could not be determined"), "{err}");
        let err = resolve(None, Some("devops")).unwrap_err();
        assert!(err.contains("could not be determined"), "{err}");
    }

    #[test]
    fn override_devops_with_a_parsable_ado_remote_is_accepted() {
        // The asymmetry that makes the error above safe: an explicit devops
        // override is honored (not silently downgraded to GitHub) whenever the
        // coordinates az needs are actually present.
        assert_eq!(
            resolve(Some("https://dev.azure.com/beans/Shop/_git/backend"), Some("devops")),
            Ok(ado("beans", "Shop", "backend")),
        );
        // Config values are trimmed before matching — `git config` round-trips
        // stray whitespace and an untrimmed value would fall through to GitHub.
        assert_eq!(
            resolve(Some("git@ssh.dev.azure.com:v3/beans/Shop/backend"), Some("  devops  ")),
            Ok(ado("beans", "Shop", "backend")),
        );
    }

    #[test]
    fn ado_urls_missing_coordinates_yield_none() {
        // Each ADO URL shape needs its full segment list; a partial one must not
        // produce Provider::AzureDevOps with empty org/project/repo.
        assert_eq!(parse_remote("git@ssh.dev.azure.com:v3/beans/Shop"), None, "ssh v3 without repo");
        assert_eq!(parse_remote("git@ssh.dev.azure.com:v4/beans/Shop/backend"), None, "wrong ssh version segment");
        assert_eq!(parse_remote("https://beans.visualstudio.com/Shop"), None, "legacy without _git/repo");
        assert_eq!(parse_remote("https://beans.visualstudio.com/Shop/_git"), None, "legacy without repo");
    }

    #[test]
    fn override_invalid_value_errors() {
        let err = resolve(Some("https://github.com/acme/repo"), Some("gitlab")).unwrap_err();
        assert!(err.contains("Valid values: github, devops"), "{err}");
    }
}
