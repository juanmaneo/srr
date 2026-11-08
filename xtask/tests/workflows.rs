//! The CI workflows build in the same environment as `cargo xtask` on a laptop.
//!
//! `xtask::PINNED_ENV` is set on every gate command; the workflows of `.github/workflows` set
//! the same variables in their top-level `env:` so the steps that do not go through `xtask`
//! (the `mode3d` tests, the model audit, ...) build the same way. These tests fail when one
//! side changes without the other.

use std::fs;
use std::path::{Path, PathBuf};

use xtask::PINNED_ENV;

fn workflows() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../.github/workflows");
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "yml"))
        .collect();
    files.sort();
    files
}

/// The `name: value` pairs of the top-level `env:` block, quotes removed.
fn top_level_env(text: &str) -> Vec<(String, String)> {
    text.lines()
        .skip_while(|line| *line != "env:")
        .skip(1)
        .take_while(|line| line.starts_with("  "))
        .filter_map(|line| line.trim().split_once(':'))
        .map(|(name, value)| (name.to_string(), value.trim().trim_matches('"').to_string()))
        .collect()
}

/// The text of each job under `jobs:` (a job starts at a line indented by exactly two spaces).
fn jobs(text: &str) -> Vec<String> {
    let mut jobs: Vec<String> = Vec::new();
    for line in text.lines().skip_while(|line| *line != "jobs:").skip(1) {
        let starts_job = line.starts_with("  ") && !line.starts_with("   ");
        match jobs.last_mut() {
            Some(job) if !starts_job => {
                job.push_str(line);
                job.push('\n');
            }
            _ => jobs.push(format!("{line}\n")),
        }
    }
    jobs
}

#[test]
fn both_workflows_are_checked() {
    let names: Vec<String> = workflows()
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["ci.yml", "test.yml"]);
}

#[test]
fn every_workflow_sets_the_pinned_environment() {
    for path in workflows() {
        let env = top_level_env(&fs::read_to_string(&path).unwrap());
        for (name, value) in PINNED_ENV {
            assert!(
                env.contains(&(name.to_string(), value.to_string())),
                "{}: missing `{name}: \"{value}\"` in the top-level env (has {env:?})",
                path.display()
            );
        }
    }
}

#[test]
fn every_workflow_installs_the_coverage_tool_where_it_runs_the_gate() {
    // `dev-check` and `coverage` end with `cargo llvm-cov`, which the runners do not ship.
    for path in workflows() {
        let text = fs::read_to_string(&path).unwrap();
        for job in jobs(&text).iter().filter(|job| {
            job.contains("cargo xtask dev-check") || job.contains("cargo xtask coverage")
        }) {
            assert!(
                job.contains("cargo install --locked cargo-llvm-cov"),
                "{}: a job runs the coverage gate without installing cargo-llvm-cov",
                path.display()
            );
        }
    }
}

#[test]
fn the_job_splitter_keeps_each_job_with_its_steps() {
    let text = "env:\n  A: 1\njobs:\n  one:\n    steps:\n      - run: a\n  two:\n    steps:\n      - run: b\n";
    assert_eq!(
        jobs(text),
        [
            "  one:\n    steps:\n      - run: a\n",
            "  two:\n    steps:\n      - run: b\n"
        ]
    );
}

#[test]
fn the_env_parser_reads_only_the_top_level_block() {
    let text =
        "on: push\nenv:\n  A: \"1\"\n  B: \"\"\n  C: plain\njobs:\n  x:\n    env:\n      D: 2\n";
    assert_eq!(
        top_level_env(text),
        [("A", "1"), ("B", ""), ("C", "plain")].map(|(n, v)| (n.to_string(), v.to_string()))
    );
}
