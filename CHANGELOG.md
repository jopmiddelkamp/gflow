# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [4.2.0] - 2026-09-10

### Added
- gflow now ships as a Claude Code plugin, so Claude can drive gflow's branch, release and PR commands for you. Install it with `/plugin marketplace add jopmiddelkamp/gflow`, then `/plugin install gflow@gflow`. The bundled skill checks `gflow --version` on first use and tells you to update if your binary is older than the syntax it documents.

### Changed
- No CLI behaviour changed in this release. The binaries are functionally identical to 4.1.0 — the release exists to publish the Claude Code plugin.

## [4.1.0] - 2026-09-08

### Added
- Configuration is now layered across three files, later overriding earlier key by key: `~/.gflow/config` (you, every repository) → `<repo>/.gflow/config` (this repository, committed) → `<repo>/.gflow/config.local` (you, this repository, gitignored). A layer that does not mention a key never undoes a lower one.
- New `--repo` flag on `gflow worktree`, writing the repository's committed `.gflow/config`. `--local` now writes `.gflow/config.local`, which is yours alone and never committed. No flag still writes `~/.gflow/config`. `--repo` and `--local` conflict.
- `~/.gflow/config` can supply the landing policy, so a throwaway repository no longer needs its own committed `.gflow/config` or a `gflow init` run. A repo with no config file in any layer is still "not initialised".
- The worktree settings are now file keys — `worktree`, `editor`, `path` — readable and editable by hand. Layers 1 and 2 take every key; `config.local` takes only these three. `mode`, `keep-release-branches`, and `bump-strategy` found there are reported and ignored: a private opt-out of `mode=protected` would defeat the guarantee the committed file makes.

### Changed
- `gflow worktree enable|disable|editor|path` and the interactive wizard write config files instead of `gflow.worktree.*` git config. Existing commands and the `--local` flag keep working; each message now names the file it wrote.
- gflow writes and maintains `<repo>/.gflow/.gitignore` itself so the private layer is never committed. It ignores itself too, so gflow's bookkeeping never appears as untracked noise, and your repository's own `.gitignore` is never touched.

### Fixed
- The name prompt no longer gets overwritten by the next message when it lands on the terminal's bottom row. The prompt ended with a cursor-move escape, which cannot scroll the screen, so `Fetching latest...` printed on top of it and left the tail of the prompt visible.
- Azure DevOps: finishing or resuming a leg no longer fails with `Unexpected merged-PR data from az`. The merged-PR queries asked az for a flat list of values, which its TSV writer prints one per line instead of as a single tab-separated row.

### Migration
- Automatic and silent. On the first run, any `gflow.worktree.*` git config moves into the new files in the scope it was set in — global keys to `~/.gflow/config`, `--local` keys to that repository's `.gflow/config.local` — and the git keys are unset. Local git config was never committed, so it stays uncommitted; nothing appears in `git status`. A value the file already states wins; the stale git key is still cleaned up.
- `gflow.branch.main` and `gflow.hosting.provider` stay in git config and are untouched: they are detection caches gflow writes back itself, not settings you chose.

## [4.0.0] - 2026-09-04

### Changed
- **BREAKING:** The project is renamed from bflow to gflow. The binary is now `gflow`, the crate is `gflow`, and the packages published to Homebrew and Chocolatey are `gflow`.
- **BREAKING:** Repo configuration moved from `.bflow/config` to `.gflow/config`, and the version script from `.bflow/set-version.{sh,cmd}` to `.gflow/set-version.{sh,cmd}`.
- **BREAKING:** Git config keys are now `gflow.*` (was `bflow.*`) — `gflow.branch.main`, `gflow.hosting.provider`, and every `gflow.worktree.*` key.
- **BREAKING:** PR templates are now read from `.github/pr-templates/gflow-<key>.md` (was `bflow-<key>.md`).
- The project home is now `jopmiddelkamp/gflow`; every release, tap, and documentation URL points there instead of `Beans-BV/beans-gitflow`.
- Logo and art assets are rebranded to gflow.

### Migration
- Rename `.bflow/` to `.gflow/` in every repo that was initialised with bflow, and commit the move.
- Rename `.github/pr-templates/bflow-*.md` to `gflow-*.md`.
- Re-set your local git config keys under the `gflow.` prefix, for example `git config gflow.branch.main master`.
- Reinstall the CLI as `gflow`; the old `bflow` binary is not updated.

## [3.5.0] - 2026-09-02

### Added
- Every PR announcement (work, fix, landing, and version PRs) now prints a bare, copy-ready block: the PR title in bold on one line, the URL on the next — ready to paste into Slack. The block is also copied to your clipboard automatically (best-effort and announced; silently skipped on machines without a clipboard tool). Uses pbcopy on macOS, clip on Windows, and wl-copy/xclip/xsel on Linux.
- Terminal styling now degrades cleanly: bold is dropped when output is piped, when NO_COLOR is set, or when TERM=dumb.

### Changed
- Re-running `gflow finish` (or `gflow sync`) from a `finish/*` branch after resolving a landing-merge conflict now switches the worktree back to the source branch automatically and continues — the manual `git switch` step is gone.
- The merge-conflict message now announces loudly that gflow switched the current worktree to the finish branch, and names the exact recovery commands.
- Completed operations print a uniform check mark (release/hotfix complete, develop synced, branch created and pushed).

## [3.4.0] - 2026-09-01

### Added
- PR completion-type enforcement: every PR gflow creates or re-surfaces prints a hard-to-miss banner naming the required completion type — Squash for work, fix, and version PRs; Merge commit for protected finish/* landing PRs. On the next run gflow verifies the merged PR via the merge commit's parent count and hard-stops with undo instructions when the wrong type was used, before any cleanup or tagging.
- New `--accept-merge-type` flag on `gflow finish` and `gflow sync` to accept a wrongly completed PR and continue.

## [3.3.0] - 2026-09-01

### Added
- Worktree mode: `gflow start hotfix-fix` now opens a worktree for the `hotfix/{v}` container branch as well (or announces the worktree that already holds it), opened before the fix branch's own worktree so the fix keeps editor focus. The container is where `gflow finish` runs later, and no other command would ever hand it a worktree.
- Every PR gflow creates or re-surfaces now opens in your default browser right after its URL is printed — protected-mode landing PRs and version PRs included, matching what work and fix PRs already did.

### Changed
- Protected-mode landing PRs (the merges of a hotfix or release into `main`, `develop`, and open `release/*` branches) now get an empty description: the title says everything, so the repository's native PR template no longer decorates them. An explicit `.github/pr-templates/gflow-release.md`, `gflow-hotfix.md`, or `gflow-default.md` still applies.

### Fixed
- A failed browser open (headless CI, no `xdg-open`) is now a warning instead of an error — previously a work-branch finish failed on it even though its PR had already been created.

## [3.2.0] - 2026-08-26

### Added
- `gflow init` — repositories are now initialised explicitly. Repo-wide policy lives in a committed `.gflow/config` (landing mode, `keep-release-branches`, bump strategy), written by a three-question wizard. Commit the file so every clone and CI job shares the same policy.
- Protected landing mode (`mode=protected`): on repos where `main`/`develop` require pull requests, `gflow finish`, `bump`, and `sync` open a PR for every landing instead of merging directly, print its URL, and exit 0 — gflow never merges a PR. Re-running the same command after a human merges it continues from there; one PR lands per run, and only the last one deletes the source branch (unless its tip isn't part of any landed PR, in which case gflow keeps it and prints how to remove it by hand). Progress is re-derived from PR and tag state on every run — nothing is stored on disk.
- Every protected landing PR's head is a throwaway `finish/<source>-into-<target>` branch, so the release/hotfix branch is never a PR head and can never be touched by a conflict resolution or the platform's auto-delete-head-branches setting. Landing PRs are born mergeable: gflow merges the target into the finish branch on every run, so a conflict surfaces immediately in your terminal with exact recovery steps, and a PR that turns conflicted later (the target moved) is healed by simply re-running. Strict develop/release legs re-open with a refreshed finish branch when a landing predates newer source commits, so no commit can silently miss a target.
- Repo-owned version script (`.gflow/set-version.sh` / `.gflow/set-version.cmd`, picked by platform): an opt-in file gflow runs with the clean `X.Y.Z` version when a release or hotfix branch is cut, when develop is bumped after a release, and on `gflow bump`, committing whatever it changes as `chore: set version {v}`. A repo with neither file behaves exactly as before.
- `bump-strategy` config key (`.gflow/config`): `rc` (default) keeps today's SemVer pre-release tags (`vX.Y.0-rc.N` for staging, clean `vX.Y.0` for production); `patch` gives every staged build a real incrementing version (`vX.Y.1`, `vX.Y.2`, …) for projects that cannot consume pre-release tags — finish then only merges (the last bump tag is already final), and hotfix versions derive from shipped tags only, never from an open release's staging numbers.
- `keep-release-branches` config key (`.gflow/config`): skips gflow's own deletion of `release/*`/`hotfix/*` branches once a finish or bump is done with them; work branches are unaffected. Kept branches whose clean tag exists are treated as shipped, not open, so `gflow start`'s reuse checks, `release-fix` discovery, and hotfix fan-out skip them.
- `release-chore/{v}/set-version` branch type: gflow-created branches carrying a version-script commit that can't land directly on a protected release branch; finishes like `release-fix` if a human needs to intervene.
- Release and hotfix branches join the worktree flow: `gflow start release` opens the new release in its own worktree (the current checkout returns to `develop`; `--no-worktree` skips it), and a release/hotfix finish works from any worktree — a merge target checked out in another worktree is merged there in place (that tree must be clean), a mainline held by another worktree is detached around instead of checked out, and the finish removes its own worktree when it ran inside one.
- `worktrees.json` setup commands: after creating a worktree, gflow runs the repo's `.cursor/worktrees.json` / `worktrees.json` setup commands (Cursor / worktree-cli format) in the new tree, with `$ROOT_WORKTREE_PATH` pointing at the main checkout. Command failures are reported and never fail the start; an unparsable file warns (naming the file and byte offset) and is skipped instead of blocking.
- `master` mainline support: on first run gflow detects whether the repository uses `main` or `master`, saves the answer to `gflow.branch.main` (repo-local git config), and every flow, menu, and message follows the resolved name. Only those two names are accepted.

### Changed
- A repository without `.gflow/config` is **not initialised**: the interactive menu offers the init wizard and subcommands refuse with `run 'gflow init'`. Existing repos need `gflow init` once — that is the whole migration.
- Pending-landing announcements are now a readable block — blank-line separated, with the PR title (bold on a terminal), the URL, what happens next, and the conflict hint — instead of a wall of text.

### Fixed
- The interactive menu on a hotfix branch now offers "start hotfix fix".
- GitHub's open-PR probe (`create_or_get_pr`) now filters by base branch instead of returning a branch's newest PR regardless of target, which could reuse the wrong PR when a hotfix fans out into several release branches.
- PR templates are now resolved from the working tree the command runs in, instead of the main working tree — running a finish from a worktree no longer applies another branch's template.

## [3.1.0] - 2026-08-03

### Changed
- PR titles for work branches now convert dashes in the branch name to spaces, matching what `release-fix` and `hotfix-fix` already did (e.g. `feature/foo-bar` → `feat: foo bar`, breaking: `feat!: foo bar`). All flows share one title formatter, so squash-merged history reads as prose everywhere ([#17](https://github.com/jopmiddelkamp/gflow/pull/17)).

### Fixed
- Parent-branch detection no longer drops `develop` from the PR target candidates when it is ahead of the branch being finished. A busy `develop` — one carrying teammates' merges since you branched — was misread as a child work branch and filtered out, leaving the wrong target or none at all ([#16](https://github.com/jopmiddelkamp/gflow/pull/16)).

## [3.0.0] - 2026-07-29

### Added
- Azure DevOps support — the hosting provider is auto-detected from the origin remote URL (`dev.azure.com` / `*.visualstudio.com` → Azure DevOps via the `az` CLI, anything else → GitHub via `gh`), overridable with `git config gflow.hosting.provider github|devops`. PRs are created with `az repos pr create` and open at the canonical `dev.azure.com` URL; preflight verifies the `azure-devops` extension and repository access ([#13](https://github.com/jopmiddelkamp/gflow/pull/13)).
- `gflow finish` completes an already-merged branch instead of opening a new PR. When a work branch's (or release-fix/hotfix-fix branch's) most recent PR is merged and the local tip is exactly the merged commit, finish deletes the remote branch (if the platform didn't already), deletes the local branch, and — when the branch lives in its own worktree — removes the worktree. New commits after the merge, or a PR closed without merging, still get a fresh PR; cleanup never runs on them ([#15](https://github.com/jopmiddelkamp/gflow/pull/15)).
- Optional git worktree flow for `gflow start`. When `gflow.worktree.enabled` is set (git config), work-branch starts (feature/fix/chore/docs/refactor, plus release-fix/hotfix-fix) create the branch in a native git worktree and open it in your editor instead of switching the current checkout. Configurable via `gflow.worktree.editor` (default `code`; `none` to skip opening) and `gflow.worktree.path` (base directory, default the repo's parent). Worktree folders are named `<repo-name>-<branch-with-slashes-as-dashes>`. Off by default; `--no-worktree` skips it for a single command.
- `gflow worktree` command to configure the worktree flow without editing git config by hand: an interactive setup wizard (`gflow worktree`) plus non-interactive subcommands `enable`, `disable`, `editor <cmd>`, `path <dir>`, and `status`. Writes global git config by default; `--local` scopes to the current repository.
- `gflow finish --base <branch>` on work branches sets the PR target explicitly, skipping parent-branch detection and its selection menu — closes the last non-interactive gap for AI agents and CI. The branch must exist on origin (PR targets live on the remote) and differ from the branch being finished; branch types with a fixed target (release, hotfix, release-fix, hotfix-fix) reject the flag. Additionally, when detection finds exactly one candidate parent it is now used directly instead of showing a one-item menu.

## [2.4.0] - 2026-06-17

### Changed
- `gflow finish` resume state is now scoped per source branch. An interrupted release/hotfix finish is tracked in its own file under `.git/gflow-finish/` (e.g. `hotfix-2.5.2.state`) and only resumes when you are standing on that source branch. From `develop`, `main`, or any `feature/*` branch, gflow behaves normally — a stalled finish no longer hijacks every command. Two finishes can be in progress at once without colliding, and `gflow finish --abort` is likewise branch-scoped ([#10](https://github.com/jopmiddelkamp/gflow/pull/10)).
- Every merge step in the release and hotfix finish flows now appends recovery guidance on conflict, naming the exact source branch to `git switch` back to before re-running `gflow finish`.

### Added
- Pre-2.4 global `.git/gflow-finish.state` files are migrated automatically into the per-branch folder on first run.

## [2.3.0] - 2026-06-17

### Fixed
- `release-fix` and `hotfix-fix` PR titles now convert dashes in the branch name to spaces (e.g. `hotfix-fix/2.1.0/null-crash` → `fix: null crash`), keeping squash-merged history readable ([#9](https://github.com/jopmiddelkamp/gflow/pull/9)).

## [2.2.0] - 2026-06-01

### Added
- Branch-aware PR templates — `gflow finish` selects the PR body by branch type, resolving most-specific first: branch-specific (`.github/pr-templates/gflow-<type>.md`) → group → `gflow-default.md` → the repo's existing git default. The fix family (`fix`, `release-fix`, `hotfix-fix`) shares `gflow-fix.md`, with optional per-branch overrides like `gflow-release-fix.md`. Opt-in: with no `.github/pr-templates/` directory, behavior is unchanged ([#8](https://github.com/jopmiddelkamp/gflow/pull/8)).

## [2.1.0] - 2026-05-26

### Added
- Idempotent `gflow finish` for release and hotfix flows — re-running after a merge conflict resumes from the first incomplete step. Steps already completed (merges, tags, pushes, branch deletion) are detected from real git state and skipped.
- `gflow finish --abort` discards an in-progress finish state.
- Hotfix propagation into open `release/*` branches — when a hotfix is finished while a release branch exists, the fix is merged into the release too so the upcoming release ships it; the operator runs `gflow bump` afterward to cut a new RC.
- `gflow start release --major` / `--minor` flags to skip the interactive prompt in non-interactive contexts (CI, AI agents, scripts).

### Fixed
- `gflow finish` on a release branch dispatches correctly after a develop-merge conflict — state file consultation now precedes the branch-eligibility check so resume works from any branch.
- Release/hotfix cleanup switches off the source branch before deletion when HEAD is still on it (resume paths that skipped the develop merge).
- Edge cases in the prior release finish flow ([#7](https://github.com/jopmiddelkamp/gflow/pull/7)).

## [2.0.0] - 2026-04-14

### Changed
- **BREAKING:** Tags now use `v` prefix (e.g., `v1.2.0` instead of `1.2.0`)
- **BREAKING:** Release start creates RC tags (`v1.2.0-rc.1`) instead of clean tags
- **BREAKING:** `gflow bump` increments RC number (`v1.2.0-rc.2`) instead of patch version
- Release finish creates clean production tag (`v1.2.0`) from latest RC
- SemVer parsing supports pre-release identifiers (`-rc.N`)

### Added
- RC-head guard on `gflow finish`: rejects a release branch when HEAD has commits past the latest RC tag, enforcing that every commit merged to `main` was validated on staging via an RC deploy
- Smart major/minor release selection — `gflow start release` scans commits for breaking changes and preselects accordingly
- `gflow finish` now asks about breaking changes on feature/fix/refactor branches and adds `!` to the PR title when confirmed (e.g., `feat!: name`)
- `gflow finish --breaking[=true|false]` flag for non-interactive control over the breaking-change marker
- CI Integration guide in README with GitHub Actions examples for staging and production
- Mobile app (Flutter) CI guidance with version extraction and build number patterns
- Chocolatey package icon

## [1.2.1] - 2026-04-09

### Changed
- Added `iconUrl`, `packageSourceUrl`, `summary`, and `releaseNotes` fields to Chocolatey nuspec for faster package review approval

## [1.2.0] - 2026-04-09

### Added
- `pull` method on `Git` trait using `--ff-only` for silent fast-forward syncs

### Changed
- Remote sync ("pull latest") operations now fast-forward silently instead of creating merge commits
- Merge commit messages use consistent format: `chore: merge <branch> into <target>` for both main and develop
- Gracefully skip pull when remote branch does not exist (local-only branches)

## [1.1.1] - 2026-04-01

### Fixed
- Windows keyboard input handling for interactive prompts

## [1.1.0] - 2026-03-30

### Added
- `--no-checkout` flag for all `gflow start` subcommands (except `release`) — creates and pushes the branch without switching to it, designed for git worktree workflows
- Auto-stash/restore of uncommitted changes during `gflow start` — no more "working tree is not clean" errors
- `--no-checkout` on `release-fix` and `hotfix-fix` skips branch-type validation and discovers the target branch automatically

### Fixed
- Orphaned stash when early return errors occurred during start flows
- Improved error message when base branch does not exist

## [1.0.0] - 2026-03-26

### Changed
- Release process moved from CLI command to skill-based automation
- Version bump to 1.0.0 (stable release)

## [0.2.1] - 2026-03-26

### Fixed
- Patch version bump fix

## [0.2.0] - 2026-03-26

### Added
- Custom terminal UI with crossterm (select menus with number-key instant selection, text input with live space-to-hyphen conversion)
- Parent branch detection via merge-base for accurate PR targeting
- Child work branch support with retarget-and-merge workflow
- Base branch selection (`--base`) when starting work branches
- Auto-bump version before finishing release if commits exist since last tag
- Homebrew and Chocolatey package distribution
- Unit tests with mock Git/Hosting implementations

### Changed
- Split release and release-fix into separate menu actions
- Improved menu rendering and terminal artifact cleanup

## [0.1.0] - 2026-03-25

### Added
- Initial release of gflow CLI
- Core Git and HostingPlatform traits with GitHub implementation via `gh` CLI
- SemVer parsing, branch name generation, and branch type detection
- Interactive menu system for all branch types
- Start flows: feature, fix, chore, docs, refactor, release, release-fix, hotfix-fix
- Finish flows: work branches (PR creation), release (merge + tag), hotfix (merge + tag)
- Release management: bump version, sync with develop
- Cross-platform CI/CD with GitHub Actions (macOS x86_64, macOS ARM64, Windows)
- README with mermaid diagrams documenting the branch model and workflows

[4.2.0]: https://github.com/jopmiddelkamp/gflow/compare/v4.1.0...v4.2.0
[4.1.0]: https://github.com/jopmiddelkamp/gflow/compare/v4.0.0...v4.1.0
[4.0.0]: https://github.com/jopmiddelkamp/gflow/compare/v3.5.0...v4.0.0
[3.5.0]: https://github.com/jopmiddelkamp/gflow/compare/v3.4.0...v3.5.0
[3.4.0]: https://github.com/jopmiddelkamp/gflow/compare/v3.3.0...v3.4.0
[3.3.0]: https://github.com/jopmiddelkamp/gflow/compare/v3.2.0...v3.3.0
[3.2.0]: https://github.com/jopmiddelkamp/gflow/compare/v3.1.0...v3.2.0
[3.1.0]: https://github.com/jopmiddelkamp/gflow/compare/v3.0.0...v3.1.0
[3.0.0]: https://github.com/jopmiddelkamp/gflow/compare/v2.4.0...v3.0.0
[2.4.0]: https://github.com/jopmiddelkamp/gflow/compare/v2.3.0...v2.4.0
[2.3.0]: https://github.com/jopmiddelkamp/gflow/compare/v2.2.0...v2.3.0
[2.2.0]: https://github.com/jopmiddelkamp/gflow/compare/v2.1.0...v2.2.0
[2.1.0]: https://github.com/jopmiddelkamp/gflow/compare/v2.0.0...v2.1.0
[2.0.0]: https://github.com/jopmiddelkamp/gflow/compare/v1.2.1...v2.0.0
[1.2.1]: https://github.com/jopmiddelkamp/gflow/compare/v1.2.0...v1.2.1
[1.2.0]: https://github.com/jopmiddelkamp/gflow/compare/v1.1.1...v1.2.0
[1.1.1]: https://github.com/jopmiddelkamp/gflow/compare/v1.1.0...v1.1.1
[1.1.0]: https://github.com/jopmiddelkamp/gflow/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/jopmiddelkamp/gflow/compare/v0.2.1...v1.0.0
[0.2.1]: https://github.com/jopmiddelkamp/gflow/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/jopmiddelkamp/gflow/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/jopmiddelkamp/gflow/releases/tag/v0.1.0
