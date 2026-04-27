#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::process::Command;

use tempfile::TempDir;

fn tokf() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tokf"))
}

/// `--print` is the deterministic, gh-free path: it always emits the markdown
/// to stdout, regardless of `gh` availability or the user's home directory.
#[test]
fn issue_print_outputs_markdown() {
    let tmp = TempDir::new().unwrap();
    let output = tokf()
        .current_dir(tmp.path())
        .env("TOKF_HOME", tmp.path())
        .args([
            "issue",
            "--title",
            "smoke",
            "--body",
            "test body",
            "--print",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "exit={:?} stderr={stderr}",
        output.status.code()
    );
    assert!(stdout.contains("## Summary"), "no summary:\n{stdout}");
    assert!(stdout.contains("test body"), "body missing:\n{stdout}");
    assert!(stdout.contains("**tokf**:"), "no version:\n{stdout}");
    assert!(
        stdout.contains("Excluded for privacy"),
        "no privacy footer:\n{stdout}"
    );
}

/// PII boundary: home prefix must not appear in the output. We set
/// `TOKF_HOME` inside a `TempDir` whose path is below the real `$HOME`, so
/// any leaked path will contain the user's home segment.
#[test]
fn issue_print_redacts_home_paths() {
    let tmp = TempDir::new().unwrap();
    let real_home = dirs::home_dir().expect("home dir resolvable");
    let output = tokf()
        .current_dir(tmp.path())
        .env("TOKF_HOME", tmp.path())
        .args(["issue", "--title", "t", "--body", "b", "--print"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let home_str = real_home.display().to_string();
    assert!(
        !stdout.contains(&home_str),
        "home prefix `{home_str}` leaked into output:\n{stdout}"
    );
}

/// `--body` and `--body-from` are mutually exclusive (clap-level conflict).
#[test]
fn issue_body_flags_conflict() {
    let tmp = TempDir::new().unwrap();
    let body_file = tmp.path().join("body.txt");
    std::fs::write(&body_file, "hello").unwrap();
    let output = tokf()
        .current_dir(tmp.path())
        .env("TOKF_HOME", tmp.path())
        .args([
            "issue",
            "--title",
            "t",
            "--body",
            "x",
            "--body-from",
            body_file.to_str().unwrap(),
            "--print",
        ])
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "expected non-zero exit for conflicting flags"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cannot be used with") || stderr.contains("conflicts"),
        "expected clap conflict message in stderr:\n{stderr}"
    );
}

/// `--body-from <path>` reads the body from a file.
#[test]
fn issue_body_from_file() {
    let tmp = TempDir::new().unwrap();
    let body_file = tmp.path().join("body.txt");
    std::fs::write(&body_file, "from-file-marker-xyz").unwrap();
    let output = tokf()
        .current_dir(tmp.path())
        .env("TOKF_HOME", tmp.path())
        .args([
            "issue",
            "--title",
            "t",
            "--body-from",
            body_file.to_str().unwrap(),
            "--print",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("from-file-marker-xyz"),
        "body not pulled from file:\n{stdout}"
    );
}
