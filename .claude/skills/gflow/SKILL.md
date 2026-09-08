---
name: gflow
description: ALWAYS load this skill if you interact with GIT branches! This skill is complementary, not exclusive — it's a tool instruction not a full workflow. Always keep checking for and invoke other applicable skills alongside gflow that follow full workflows like e.g. superpowers.
---

# Branch Management via gflow

## Hard Rule

**NEVER** use `git branch`, `git merge`, `git tag`, `gh pr create`, `az repos pr create`, or any other raw git/gh/az command for branch lifecycle operations. **ALL** branch creation, merging, tagging, PR creation, and version bumping MUST go through `gflow`.

**ONLY EXCEPTION:** The user explicitly asks you to bypass gflow.

## Allowed Without gflow

- `git switch` / `git checkout` — switching between existing branches
- `git branch -d` / `git branch -D` — deleting a local branch that's no longer needed

Everything else branch-related → use `gflow`.

## Command Reference

### Start a branch

```bash
gflow start feature --name <name> [--base <branch>] [--no-checkout] [--no-worktree]
gflow start fix --name <name> [--base <branch>] [--no-checkout] [--no-worktree]
gflow start chore --name <name> [--base <branch>] [--no-checkout] [--no-worktree]
gflow start docs --name <name> [--base <branch>] [--no-checkout] [--no-worktree]
gflow start refactor --name <name> [--base <branch>] [--no-checkout] [--no-worktree]
gflow start release [--major | --minor] [--no-worktree] # from develop, prompts if no flag given
gflow start release-fix --name <name> [--no-checkout] [--no-worktree]
gflow start hotfix-fix --name <name> [--no-checkout] [--no-worktree]  # from the mainline or a hotfix branch
```

#### Mainline branch (`main` or `master`)

Either name works. On its first run in a repo gflow detects which exists and
saves it to `gflow.branch.main` (**local** scope — the mainline belongs to the
repo, unlike the worktree settings), announcing the save. Set it yourself with
`git config gflow.branch.main master`. Only `main` and `master` are accepted;
anything else is a hard error. Everything below that says "main" means this
resolved branch.

#### Worktree integration (optional)

When `worktree=true`, every `start` creates the branch in a native git worktree and opens it in an editor instead of switching the current checkout (`start release` creates and tags in the current checkout first, returns it to `develop`, then opens the release worktree; a release already held by a worktree is only announced). `start hotfix-fix` also opens a worktree for the `hotfix/{v}` container branch — before the fix branch's own worktree, so the fix keeps editor focus; a hotfix already held by a worktree is only announced. After creating a worktree and before opening the editor, gflow runs the repo's `.cursor/worktrees.json` / `worktrees.json` setup commands if present (Cursor / worktree-cli format: each entry a shell command in the new worktree, `$ROOT_WORKTREE_PATH` = main checkout, failures reported and the rest still run, never fails the start). Strict JSON only; an unparsable file warns (naming file + byte offset, trailing commas called out) and is skipped — it never blocks a command, and with the flow disabled it is not read. No prompt, no config — the committed file is the opt-in. Config keys (in the layered config files, not git config): `worktree` (bool, default false), `editor` (default `code`; `none` skips opening), `path` (base dir, default repo's parent, `~` expanded). Folder name: `<repo-name>-<branch-with-slashes-as-dashes>`. `--no-worktree` skips it for one command. Like `--no-checkout`, active worktree mode relaxes the branch-type check for `release-fix`/`hotfix-fix` (target branch is discovered automatically) — but standing on a `release/{v}`/`hotfix/{v}` branch still wins over discovery, and discovery takes the newest open version.

`finish` (release/hotfix) works from any worktree: a merge target checked out in another worktree is merged there in place (`git -C`); that tree must be clean or gflow refuses, naming the path.

Configure it with the `gflow worktree` command (writes `~/.gflow/config`; `--repo` writes the repo's committed `<repo>/.gflow/config`, `--local` writes the gitignored `<repo>/.gflow/config.local`; `--repo` and `--local` conflict):

```bash
gflow worktree                     # interactive setup (enable / editor / location)
gflow worktree enable | disable
gflow worktree editor <cmd>        # code | cursor | windsurf | zed | pycharm | none | any command
gflow worktree path <dir>
gflow worktree status
```

### Finish current branch

```bash
gflow finish [--breaking] [--base <branch>] [--accept-merge-type]  # infers action from current branch type
gflow finish --abort       # discard an in-progress release/hotfix finish
```

**PR completion types are enforced.** Work/fix/version PRs must be completed with **Squash**; protected `finish/*` landing PRs with a **Merge commit**. gflow prints a banner next to each PR URL saying which, and verifies on the next run via the merge commit's parent count (2 = merge, 1 = squash). A wrong completion **hard-stops** the flow (no cleanup, no tag) and prints undo steps — wrongly merged work PR: revert/reset the target, `git commit --amend --no-edit` on the branch, re-run; wrongly squashed landing PR: revert it on the platform. To accept the mistake and continue: re-run with `--accept-merge-type` (on `finish` and `sync`).

Every PR gflow creates or re-surfaces (work/fix PRs, protected landing PRs, version PRs) also opens in the default browser; a failed open is a warning, never an error. Protected landing PRs (hotfix/release → main/develop/release) get an empty description — the repo's native PR template never applies to them; a `gflow-release.md`/`gflow-hotfix.md`/`gflow-default.md` template still does.

- **Work branches** (feature/fix/chore/docs/refactor) → asks about breaking changes (feat/fix/refactor only), creates PR to base branch. PR title converts dashes to spaces (e.g. `feature/foo-bar` → `feat: foo bar`). If breaking, PR title gets `!` (e.g., `feat!: foo bar`). Use `--breaking` flag in non-interactive mode to skip the prompt.
  - PR target: detected from branch topology; a single candidate is used directly, multiple candidates show a menu. `--base <branch>` sets the target explicitly and skips both — **AI agents/CI must pass `--base` (plus `--breaking`) so finish never needs a TTY** (e.g. `gflow finish --base develop --breaking=false`). The branch must exist on origin (push or fetch first) and differ from the current branch. Not valid on release/hotfix/release-fix/hotfix-fix (fixed target).
  - **PR already merged** → re-running `gflow finish` completes the finish instead of opening a new PR: deletes remote + local branch and, when the branch is in its own worktree, removes the worktree (close the editor window afterwards). Only when the local tip equals the merged commit — new commits since the merge get a fresh PR instead. Applies to work branches and release-fix/hotfix-fix.
- **Release-fix / hotfix-fix** → creates PR to parent release/hotfix branch, title `fix: {name}` (dashes → spaces, e.g. `null-crash` → `fix: null crash`)
- **Release** → merges to main + develop, tags, cleans up (removes its own worktree when run inside one)
- **Hotfix** → merges to main + develop + every open `release/*`, tags, cleans up (removes its own worktree when run inside one). If a release branch already exists, the hotfix is propagated into it so the upcoming release ships the fix; the operator must then run `gflow bump` to cut a new RC for staging validation.

### Resuming after a merge conflict

`gflow finish` for **release** and **hotfix** branches is **idempotent**: re-running it after a merge conflict resumes from the first incomplete step. Recovery procedure:

1. Resolve the conflict in your editor.
2. `git add` the resolved files and `git commit` to complete the merge.
3. **Switch back to the source branch** (`git switch <release|hotfix branch>`) — a conflict usually leaves HEAD on the target branch (e.g. develop). The conflict message names the branch.
4. Re-run `gflow finish` — already-done steps (merges, tags, pushes, branch deletion) are detected and skipped.

**Resume is branch-scoped.** Each in-progress finish is tracked in its own file under `.git/gflow-finish/` (e.g. `hotfix-2.5.2.state`), keyed by the source branch. gflow resumes **only when you are on that source branch** — from develop/main/feature it behaves normally, so a stalled finish never blocks other work. Two finishes (release + hotfix) can be in progress at once. Run `gflow finish --abort` from the source branch to discard its state. Legacy pre-2.4 `.git/gflow-finish.state` files are migrated automatically.

### PR templates

PR bodies resolve from `.github/pr-templates/gflow-<key>.md`, most-specific first:

1. Branch-specific: `gflow-<type>.md` (e.g. `gflow-release-fix.md`)
2. Group: the fix family (`fix`, `release-fix`, `hotfix-fix`) shares `gflow-fix.md`; other types' group == their own name
3. `gflow-default.md`
4. Repo's git default (`.github/PULL_REQUEST_TEMPLATE.md` etc. on GitHub; `.azuredevops/pull_request_template.md` etc. on Azure DevOps), else empty body

Opt-in: with no `.github/pr-templates/`, behavior is unchanged.

### Initialise a repository

```bash
gflow init   # one-time; writes .gflow/config via three questions — commit the file
```

A repo with **no config file in any layer** is not initialised: the interactive menu offers the wizard, subcommands fail with `run 'gflow init'`. A `~/.gflow/config` that states a policy is enough — a throwaway repo then needs no committed file.

### Release-only commands

```bash
gflow bump # create next RC tag (on release/* only)
gflow sync [--accept-merge-type] # merge release into develop (on release/* only)
```

### Landing modes & version script

Config is three layers, later overriding earlier per key: `~/.gflow/config` (you, all repos; written by `gflow worktree ...`) → `<repo>/.gflow/config` (this repo, committed; `--repo`) → `<repo>/.gflow/config.local` (you, this repo, gitignored; `--local`). A silent layer never undoes a lower one. Layers 1-2 take every key; layer 3 takes only `worktree`/`editor`/`path` — team keys there are warned about and ignored, so nobody can privately opt out of `mode=protected`. gflow maintains `<repo>/.gflow/.gitignore` itself (self-ignoring, so it is never untracked noise) and never edits the repo's own `.gitignore`. 4.0.x `gflow.worktree.*` git config migrates automatically on first run, per scope (global → `~/.gflow/config`, `--local` → `config.local`, which stays uncommitted as it always was); only the detection caches `gflow.branch.main` and `gflow.hosting.provider` stay in git config.

`.gflow/config` (committed file, not git config — repo policy, not per-clone): `mode=free|protected` (default `free` = today's behavior), `keep-release-branches=true|false` (default `false`; skips deleting `release/*`/`hotfix/*` on finish, work branches unaffected), `bump-strategy=rc|patch` (default `rc`; see Tag Strategy).

**`mode=protected`** — `main`/`develop` require PRs. `finish`/`bump`/`sync` open (or reuse) a PR for every landing instead of merging directly, print a bare title + URL block (bold title on a TTY; also copied to the clipboard, silently skipped without a clipboard tool), and **exit 0** — gflow never merges a PR. **Re-run the same command after a human merges it.** Every landing PR's head is a throwaway `finish/<source>-into-<target>` branch gflow cuts from the source and deletes at completion. gflow merges the target into the finish branch on every run, so landing PRs are born mergeable: a conflict surfaces mid-run and **leaves the current worktree ON the finish branch**, mid-merge (gflow says so) — resolve there, `git add . && git commit --no-edit`, re-run (the re-run switches the worktree back to the source branch itself and pushes; `git merge --abort && git switch <source>` backs out). A PR that conflicts later (target moved) heals by re-running. Never rebase a finish branch; the release/hotfix branch is never touched. One PR per run, in order (main → develop → each open release branch for hotfixes). Develop/release legs are strict: a landing that predates newer source commits re-opens with a refreshed finish branch, so commits cannot silently miss a target (this also covers a mid-release `sync` followed by more release fixes). Only the last landing deletes the source branch plus all finish branches. Nothing is stored on disk to resume — progress is re-derived from PR/tag state each run. Migration: an OPEN landing PR with the release/hotfix branch as head (older gflow) is a hard error — merge or close it, then re-run. Don't add new, unrelated work to a release branch once its `main` PR has merged (the clean tag is already placed) — ship fixes as a hotfix instead.

`bump` may print a version-PR title + URL block and defer the RC tag when a version-script commit needs its own PR on an already-pushed release branch — re-run `gflow bump` after that PR merges; it tags the **PR's merge commit**, it does not re-run the script.

**Version script** (`.gflow/set-version.sh` / `.gflow/set-version.cmd`, platform-picked; both missing = feature off): runs with the clean `X.Y.Z` at release/hotfix branch creation, the post-release develop bump, and `bump`; commits `chore: set version {v}` only if it changed files. Requires a clean tree first. Under `bump-strategy=patch` each `bump` passes the **newly incremented** `X.Y.Z` (so the script commits every bump); under `rc` it gets the constant `X.Y.0`.

`chore/set-version-{v}` and `release-chore/{v}/set-version` are **gflow-created** — merge their PR, never commit to them. `chore/set-version-*` is also excluded from `finish`'s PR-target candidates (it gets merged and deleted out from under a PR that targeted it). A `release-chore/*` branch finishes like `release-fix` (PR into its release branch, no `--base`).

**Papercut**: version files can conflict on merge (hotfix vs. develop, major release vs. develop) — resolve manually, gflow does not auto-resolve. In protected mode it surfaces while gflow merges the target into the `finish/*` branch: resolve there mid-run, never on the source branch.

`--no-checkout` hotfix creation skips the script (HEAD isn't on the new branch) and warns with the manual recovery steps.

### Tag Strategy

Selected per repo via `bump-strategy` in `.gflow/config`. gflow writes `v`-prefixed tags; a plain `X.Y.Z` tag is read as the same version (latest lookup, shipped detection).

**`rc` (default)** — SemVer pre-release tags for CI integration:

- **Release start** → `v{X}.{Y}.0-rc.1` (RC tag — triggers staging deploy)
- **Bump version** → `v{X}.{Y}.0-rc.{N+1}` (next RC — triggers staging deploy)
- **Finish release** → `v{X}.{Y}.0` (clean tag — triggers production deploy)

CI systems filter on `v*-rc.*` for staging and clean `v*` (no hyphen) for production.

**`patch`** — every staged build carries a real, incrementing version (for projects that can't consume pre-release tags):

- **Release start** → `v{X}.{Y}.0` (clean tag)
- **Bump version** → next patch, e.g. `v{X}.{Y}.1`, `v{X}.{Y}.2` (clean tags)
- **Finish release** → merge only — the last bump tag is already final (finish re-pushes it if origin lacks it)
- **Hotfix version** derives from *shipped* tags only — an open release's staging tags are skipped

No tag-shape staging/production split exists under `patch`: CI must gate production on something else (branch, merge to main, manual promotion).

**Both strategies:**

- **Finish hotfix** → `v{X}.{Y}.{Z}` (clean tag at finish, both strategies)
- **Guard:** `gflow finish` rejects a release branch if HEAD is past the latest RC/patch tag — prevents promoting unstaged commits to production. Fix by running `gflow bump` and waiting for staging.

### When to use `--base`

All work branch types (feature/fix/chore/docs/refactor) default to branching from `develop`. Use `--base <branch>` only when the work depends on changes that are not yet in `develop`:

- **Stacking on another work branch** — e.g. `feature/login` depends on `feature/auth` which hasn't been merged yet
- **Branching from a release branch** — e.g. work that should target a specific release

```bash
gflow start feature --name login --base feature/auth
```

`--base` is **not available** for `release`, `release-fix`, or `hotfix-fix` — those have fixed base branches.

### When to use `--no-checkout`

Creates and pushes the branch without switching to it. Designed for git worktree workflows where the branch will be opened in a separate worktree. With `--no-checkout`: stash/merge of current branch is skipped, and branch-type validation is relaxed for `release-fix` and `hotfix-fix` (the target branch is discovered automatically — newest open version — unless you already stand on a `release/{v}`/`hotfix/{v}` branch, which then wins).

Not available for `start release`.

## Branch Model Quick Reference

| Branch | Created from | Merges into |
|--------|-------------|-------------|
| `feature/{name}` | develop | develop (PR) |
| `fix/{name}` | develop | develop (PR) |
| `chore/{name}` | develop | develop (PR) |
| `docs/{name}` | develop | develop (PR) |
| `refactor/{name}` | develop | develop (PR) |
| `release/{major}.{minor}.{patch}` | develop | main + develop |
| `release-fix/{v}/{name}` | release/{v} | release/{v} (PR) |
| `release-chore/{v}/set-version` | release/{v} (gflow-created) | release/{v} (PR) |
| `hotfix/{major}.{minor}.{patch}` | main | main + develop + open `release/*` |
| `hotfix-fix/{v}/{name}` | hotfix/{v} | hotfix/{v} (PR) |

## Non-Interactive Environments (AI agents, CI, scripts)

`gflow` without arguments is **interactive** and requires a TTY for its menu prompts. However, when all required arguments are provided (as shown in the Command Reference above), `gflow` runs **non-interactively** and is safe to use from AI agents, CI pipelines, and scripts.

**Rule:** Always provide all required arguments so the command runs non-interactively. Never run bare `gflow` without arguments from a non-interactive context.

**The repo must be initialised** (`.gflow/config` committed, or a `~/.gflow/config` supplying the policy) — subcommands refuse otherwise; run `gflow init` once interactively.

## Prerequisites

gflow runs preflight checks automatically:
- `git` must be installed
- The hosting provider is auto-detected from the origin remote URL (`dev.azure.com` / `*.visualstudio.com` → Azure DevOps, else GitHub); override with `git config gflow.hosting.provider github|devops`
- GitHub repos: `gh` installed + `gh auth login` completed
- Azure DevOps repos: `az` installed + `az extension add --name azure-devops` + authenticated (`az login`, or a PAT via `az devops login`)
- Uncommitted changes are auto-stashed and restored after the operation
