---
name: verify
description: Drive the real gflow binary end-to-end against a fake Azure DevOps remote (fake ssh + fake az) to verify a change at the terminal.
---

# Verifying gflow at its surface

Build `cargo build --release`, copy `target/release/gflow` into a scratch `bin/`
that comes first on `PATH`. Never point it at a real repo or remote.

## Harness (all in one scratch dir `$V`)

- `origin.git`: `git init --bare`. Clone it as `git@ssh.dev.azure.com:v3/org/proj/repo`
  so detection picks Azure DevOps; `GIT_SSH_COMMAND=$V/bin/fake-ssh`.
- `fake-ssh`: skip `-o X`/`-flag` args (note `-o BatchMode=yes`), drop the host,
  rewrite `'v3/org/proj/repo'` to `$V/origin.git`, `exec sh -c "$cmd"`. Knobs:
  a delay (latency) and "fail when BatchMode" (passphrase key without agent).
- `fake-az`: log calls; PR state in a tsv (`status source head merge id target`).
  Serve `extension show`, the per-target list (`--target-branch` only, newest
  first via `tail -r`), the per-head list (`merged_pr`), the per-pair fallback,
  and `create`. A `complete-pr src tgt` helper merges `--no-ff` on origin and
  marks the row completed with head + merge SHAs.
- Logging `git` wrapper (`exec /usr/bin/git`); optional "reject ahead-behind"
  knob to simulate git < 2.41.
- `.gflow/config`: `mode=protected`, `bump-strategy=patch`, committed on master.

## Gotchas

- Isolate `HOME`: the real `~/.gflow/config` may enable worktrees + an editor.
- Shadow `open` and `pbcopy` in `$V/bin`: a new PR copies to the clipboard and
  opens the browser.
- Menus need a TTY: `(sleep N; printf '6\r') | script -q /dev/null gflow`.
  Send keys only after the menu is drawn (slow paths need N≈4).
- The background fetch outlives gflow and the pty (own process group): wait
  ~2× the fake ssh delay before checking `FETCH_HEAD`.
- git runs `ssh -G host` first to detect the ssh variant; a fake delay hits it too.
- Patch strategy: commits on a release branch need `gflow bump` before finish.

## Flows worth driving

Menu abort during a slow fetch (exit time, fetch finishes detached); fetch that
needs a prompt (foreground retry); protected release finish over three runs
(az list calls per run, `push --porcelain` tag skip, one `push --delete` for all
finish branches); work-branch finish with several candidates, with and without
ahead-behind; a full 1000-row PR list (direct lookup, then reuse).
