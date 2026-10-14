//! End-to-end checks of the command-line interface.

use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rule-safari")).args(args).output().expect("the binary runs")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn inspect_finds_the_glider_in_life() {
    let out = run(&["inspect", "B3/S23"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.starts_with("B3/S23 · Conway's Life"), "{text}");
    assert!(text.contains("c/4 diagonal"), "{text}");
}

#[test]
fn inspect_accepts_classic_notation() {
    let out = run(&["inspect", "23/36"]);
    assert!(out.status.success());
    assert!(stdout(&out).starts_with("B36/S23 · HighLife"));
}

#[test]
fn bad_rules_are_rejected_with_a_reason() {
    let out = run(&["inspect", "B9/S23"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("neighbour count"));
}

#[test]
fn watch_prints_one_plain_frame_when_piped() {
    let out = run(&["watch", "B3/S23", "--width", "20", "--height", "10", "--generations", "5"]);
    assert!(out.status.success());
    let text = stdout(&out);
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines[0].starts_with("B3/S23 · Conway's Life · generation 5"), "{text}");
    assert_eq!(lines.len(), 1 + 5, "a title and two cell rows per line");
    assert!(lines[1..].iter().all(|l| l.chars().count() == 20));
    assert!(!text.contains('\x1b'), "no escape codes when not a terminal");
}

#[test]
fn hunt_writes_a_gallery_and_results() {
    let dir = std::env::temp_dir().join(format!("rule-safari-cli-{}", std::process::id()));
    let out = run(&["hunt", "--sample", "64", "--finalists", "4", "--top", "6", "--out", dir.to_str().unwrap()]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(stdout(&out).contains("Screened 64 rules"));
    let page = std::fs::read_to_string(dir.join("gallery.html")).unwrap();
    assert!(page.contains("\"specimens\"") && !page.contains("__SAFARI_DATA__"));
    let results: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("results.json")).unwrap()).unwrap();
    assert_eq!(results["meta"]["screened"], 64);
    std::fs::remove_dir_all(&dir).unwrap();
}
