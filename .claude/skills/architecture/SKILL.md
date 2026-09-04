---
name: architecture
description: "Use when writing, reviewing, or refactoring Rust code in this repo — deciding where new code goes, adding git/hosting/editor operations, adding commands or flows, error handling, side-effect ordering, or writing tests"
---

# gflow Architecture

Broad, global rules only. Implementation-level decisions, extension recipes, and testing conventions live in `decisions.md` in this skill folder — read it before changing behavior; many "odd" spots are load-bearing.

**Related:** `solid-principles` (DIP is the backbone here), `kiss-principles`, `dry-principles`.

## Principles

1. **Ports & adapters.** Business logic lives in `flows/` and depends only on traits (`Git`, `HostingPlatform`, `Editor`, `Prompter`). Concrete adapters are wired in `main.rs` — the only composition root. No subprocess calls outside adapter impls and the composition root.
2. **Plug-and-play integrations.** Hosting providers are auto-detected from the remote and swappable; adding a provider or editor never touches flows.
3. **Integrate through the user's own CLIs** (`git`, `gh`, `az`) — never link libraries or call APIs directly. gflow inherits the user's auth, config, hooks, and signing.
4. **Minimal dependency budget.** Two crates today. A new dependency needs a very good reason; prefer hand-rolling small things.
5. **Git is the single source of truth.** Versions come from tags, config from git config, state lives under `.git/`. No parallel version files or config stores.
6. **Crash-safe by design.** Every multi-step mutating flow: validate before mutating, persist intent before the first side effect, make each step idempotent, support resume and abort.
7. **Errors are terminal and user-facing.** `Result<T, String>`; every message names the exact next command to run. No error taxonomy, no panics (only documented `unreachable!` for enforced invariants).
8. **One behavior, two interfaces.** Interactive menu and CLI subcommands resolve to the same `Action` — a feature exists in both or neither, and nothing downstream knows which interface ran.
9. **Fully testable without side effects.** Every flow runs against mocks; tests never touch real git, the network, or installed CLIs.
10. **Process is enforced in code, not documentation.** If a workflow rule matters (e.g. only RC-validated commits reach main), gflow refuses — it does not trust the operator.

## Layer Map

| Module | Responsibility |
|---|---|
| `main.rs` | Composition root: builds adapters, preflight, hands off to `lifecycle::run` |
| `lifecycle.rs` | Cross-cutting lifecycle (stash/state/resume ordering), dispatches `Action` once |
| `cli.rs` / `menu.rs` | The two interfaces; both resolve to `Action` (`action.rs`) |
| `flows/` | Business logic per workflow |
| `git/`, `hosting/`, `editor.rs`, `prompt.rs` | Ports (traits) + adapters (CLI impls); `hosting/detect.rs` picks the provider. The *policy-bearing* adapters (`GitCli`, `GitHub`, `AzureDevOps`) spawn only through `CommandRunner`/`CliRunner`, so their own logic is mockable. The zero-policy shells — `CommandEditor::open`, `open_in_browser` — spawn directly; there is no decision in them to test |
| `state.rs`, `worktree.rs`, `version.rs` | Finish state, worktree feature, SemVer |
