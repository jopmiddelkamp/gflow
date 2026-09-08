mod common;

use std::fs;

use common::MockPrompter;
use gflow::init::{ensure, run, wizard};
use gflow::repo_config::{self, BumpStrategy, Mode, RepoConfig, NOT_INITIALISED};

fn root() -> common::TempDir {
    common::tmp_dir("gflow-init-test")
}

#[test]
fn wizard_asks_the_three_policy_questions_and_writes_the_answers() {
    let root = root();
    let prompter = MockPrompter::scripted(&[1, 1, 1]);

    let cfg = wizard(&prompter, &root).unwrap();

    assert_eq!(cfg, RepoConfig { mode: Mode::Protected, keep_release_branches: true, bump_strategy: BumpStrategy::Patch });
    assert_eq!(prompter.calls(), vec![
        "select:Landing mode:[free — merge and push directly, protected — every landing goes through a PR]",
        "select:Release branches after finish:[delete (default), keep]",
        "select:Bump strategy:[rc — pre-release tags, one clean tag at finish (default), patch — real patch version on every bump]",
    ]);
    assert_eq!(fs::read_to_string(root.join(".gflow").join("config")).unwrap(),
        "mode=protected\nkeep-release-branches=true\nbump-strategy=patch\n");
}

#[test]
fn wizard_defaults_are_the_first_item_of_every_question() {
    let root = root();
    let cfg = wizard(&MockPrompter::scripted(&[0, 0, 0]), &root).unwrap();
    assert_eq!(cfg, RepoConfig::default());
}

#[test]
fn wizard_abort_writes_nothing() {
    let root = root();
    let prompter = MockPrompter::aborting();
    assert_eq!(wizard(&prompter, &root).unwrap_err(), "Aborted");
    assert_eq!(prompter.calls().len(), 1, "the first question was asked and the abort stopped there");
    assert!(!repo_config::exists(&root));
}

#[test]
fn ensure_loads_an_existing_config_without_prompting() {
    let root = root();
    let cfg = RepoConfig { mode: Mode::Protected, ..RepoConfig::default() };
    repo_config::write(&root, &cfg).unwrap();
    let prompter = MockPrompter::new();

    assert_eq!(ensure(&prompter, None, &root, true).unwrap().settings.resolve(), cfg);
    assert!(prompter.calls().is_empty());
}

#[test]
fn ensure_runs_the_wizard_when_missing_and_interactive() {
    let root = root();
    let cfg = ensure(&MockPrompter::scripted(&[0, 1, 0]), None, &root, true).unwrap().settings.resolve();
    assert!(cfg.keep_release_branches);
    assert!(repo_config::exists(&root));
}

#[test]
fn ensure_refuses_when_missing_and_non_interactive() {
    let root = root();
    let prompter = MockPrompter::new();
    assert_eq!(ensure(&prompter, None, &root, false).unwrap_err(), NOT_INITIALISED);
    assert!(prompter.calls().is_empty());
    assert!(!repo_config::exists(&root));
}

#[test]
fn run_refuses_when_already_initialised() {
    let root = root();
    repo_config::write(&root, &RepoConfig::default()).unwrap();
    let err = run(&MockPrompter::new(), &root).unwrap_err();
    assert_eq!(err, "Already initialised: edit .gflow/config directly (mode, keep-release-branches, bump-strategy).");
}

#[test]
fn run_initialises_a_fresh_repo() {
    let root = root();
    run(&MockPrompter::scripted(&[0, 0, 1]), &root).unwrap();
    assert_eq!(repo_config::load(&root).unwrap().bump_strategy, BumpStrategy::Patch);
}

#[test]
fn a_repo_with_no_config_is_initialised_by_the_global_file() {
    // The playground case: `~/.gflow/config` supplies the policy, so a throwaway
    // repo needs no `gflow init` and no committed file.
    let home = root();
    let repo = root();
    fs::create_dir_all(home.join(".gflow")).unwrap();
    fs::write(home.join(".gflow").join("config"), "mode=protected\nworktree=true\n").unwrap();
    let prompter = MockPrompter::aborting();

    let layers = ensure(&prompter, Some(&home), &repo, false).unwrap();

    assert!(layers.initialised);
    assert_eq!(layers.settings.worktree, Some(true));
    assert_eq!(layers.settings.clone().resolve().mode, Mode::Protected);
    assert!(prompter.calls().is_empty(), "no wizard: the policy is already stated");
}

#[test]
fn the_committed_repo_file_still_overrides_the_global_one() {
    let home = root();
    let repo = root();
    fs::create_dir_all(home.join(".gflow")).unwrap();
    fs::write(home.join(".gflow").join("config"), "mode=protected\n").unwrap();
    repo_config::write(&repo, &RepoConfig { mode: Mode::Free, keep_release_branches: false, bump_strategy: BumpStrategy::Rc }).unwrap();

    let layers = ensure(&MockPrompter::aborting(), Some(&home), &repo, false).unwrap();

    assert_eq!(layers.settings.resolve().mode, Mode::Free);
}
