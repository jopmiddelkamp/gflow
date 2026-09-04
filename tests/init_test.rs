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

    assert_eq!(ensure(&prompter, &root, true).unwrap(), cfg);
    assert!(prompter.calls().is_empty());
}

#[test]
fn ensure_runs_the_wizard_when_missing_and_interactive() {
    let root = root();
    let cfg = ensure(&MockPrompter::scripted(&[0, 1, 0]), &root, true).unwrap();
    assert!(cfg.keep_release_branches);
    assert!(repo_config::exists(&root));
}

#[test]
fn ensure_refuses_when_missing_and_non_interactive() {
    let root = root();
    let prompter = MockPrompter::new();
    assert_eq!(ensure(&prompter, &root, false).unwrap_err(), NOT_INITIALISED);
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
