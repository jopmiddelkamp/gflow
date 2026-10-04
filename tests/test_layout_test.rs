use std::fs;
use std::path::Path;

const NOT_A_MODULE: &[&str] = &[
    "chocolatey_nuspec", // packaging metadata, no src module
    "coverage_gate",     // the .claude/hooks/tdd-gate.sh Stop hook
    "process",           // re-runs its own binary as a child-process fixture
    "terminal",          // re-runs its own binary inside an isolated terminal
    "test_layout",
];

fn collect_module_names(dir: &Path, names: &mut Vec<String>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            names.push(path.file_name().unwrap().to_string_lossy().into_owned());
            collect_module_names(&path, names);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
            if stem != "mod" && stem != "lib" {
                names.push(stem);
            }
        }
    }
}

#[test]
fn every_test_file_is_named_after_the_src_module_it_tests() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut modules = Vec::new();
    collect_module_names(&root.join("src"), &mut modules);

    let mut misnamed: Vec<String> = fs::read_dir(root.join("tests"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .filter(|file| {
            let subject = file.strip_suffix("_test.rs");
            !subject.is_some_and(|s| modules.iter().any(|m| m == s) || NOT_A_MODULE.contains(&s))
        })
        .collect();
    misnamed.sort();

    assert!(
        misnamed.is_empty(),
        "Name each tests/ file <module>_test.rs after the src module whose behavior it tests, \
         and keep that module's failure paths in the same file. Misnamed: {misnamed:?}"
    );
}
