#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::process::Command;

fn tokf() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tokf"))
}

#[test]
fn completions_bash_produces_output() {
    let output = tokf().args(["completions", "bash"]).output().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.is_empty(), "expected non-empty output");
    assert!(
        stdout.contains("tokf"),
        "expected 'tokf' in bash completions:\n{stdout}"
    );
}

#[test]
fn completions_zsh_produces_output() {
    let output = tokf().args(["completions", "zsh"]).output().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.is_empty(), "expected non-empty output");
    assert!(
        stdout.contains("tokf"),
        "expected 'tokf' in zsh completions:\n{stdout}"
    );
}

#[test]
fn completions_fish_produces_output() {
    let output = tokf().args(["completions", "fish"]).output().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.is_empty(), "expected non-empty output");
    assert!(
        stdout.contains("tokf"),
        "expected 'tokf' in fish completions:\n{stdout}"
    );
}

#[test]
fn completions_nushell_produces_output() {
    let output = tokf().args(["completions", "nushell"]).output().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.is_empty(), "expected non-empty output");
    assert!(
        stdout.contains("tokf"),
        "expected 'tokf' in nushell completions:\n{stdout}"
    );
}

#[test]
fn completions_powershell_produces_output() {
    let output = tokf().args(["completions", "powershell"]).output().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.is_empty(), "expected non-empty output");
    assert!(
        stdout.contains("tokf"),
        "expected 'tokf' in powershell completions:\n{stdout}"
    );
}

#[test]
fn completions_elvish_produces_output() {
    let output = tokf().args(["completions", "elvish"]).output().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.is_empty(), "expected non-empty output");
    assert!(
        stdout.contains("tokf"),
        "expected 'tokf' in elvish completions:\n{stdout}"
    );
}

#[test]
fn completions_invalid_shell_fails() {
    let output = tokf()
        .args(["completions", "invalidshell"])
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "expected failure for invalid shell name"
    );
}

#[test]
fn completions_missing_shell_arg_fails() {
    let output = tokf().arg("completions").output().unwrap();
    assert!(
        !output.status.success(),
        "expected failure when shell arg is missing"
    );
}
