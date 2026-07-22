#![allow(clippy::unwrap_used, clippy::expect_used, clippy::missing_const_for_fn)]

use std::process::Command;

fn tokf() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tokf"))
}

// --- tokf run ---

#[test]
fn run_echo_hello() {
    let output = tokf().args(["run", "echo", "hello"]).output().unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "hello");
}

#[test]
fn run_no_filter_passes_through() {
    let output = tokf()
        .args(["run", "--no-filter", "echo", "hello"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "hello");
}

#[test]
fn run_timing_shows_duration() {
    let output = tokf()
        .args(["run", "--timing", "echo", "hello"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.is_empty() || stderr.contains("[tokf]"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn run_false_propagates_exit_code() {
    let output = tokf()
        .args(["run", "--no-mask-exit-code", "false"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_ne!(output.status.code(), Some(0));
}

#[test]
fn run_exit_code_42() {
    let output = tokf()
        .args(["run", "--no-mask-exit-code", "sh", "-c", "exit 42"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(42));
}

#[test]
fn run_verbose_shows_resolution_details() {
    let output = tokf()
        .args(["run", "--verbose", "echo", "hello"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("[tokf]"),
        "expected verbose output on stderr, got: {stderr}"
    );
}

#[test]
fn run_nonexistent_command_exits_with_error() {
    let output = tokf()
        .args(["run", "nonexistent_cmd_xyz_99"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("[tokf] error"),
        "expected error on stderr, got: {stderr}"
    );
    assert!(
        stderr.contains("program not found: nonexistent_cmd_xyz_99"),
        "expected missing program name on stderr, got: {stderr}"
    );
}

#[test]
fn run_builtin_git_status_filter_succeeds() {
    let dir = tempfile::TempDir::new().unwrap();

    let init = Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        init.status.success(),
        "git init failed: {}",
        String::from_utf8_lossy(&init.stderr)
    );

    let output = tokf()
        .args(["run", "--verbose", "git", "status"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("matched git/status.toml"),
        "expected built-in git status filter to match, got: {stderr}"
    );
    assert!(
        !stderr.contains("program not found"),
        "built-in git status filter should not depend on sh, got: {stderr}"
    );
}

#[test]
fn run_git_status_preserves_global_c_dir_with_run_override() {
    let cwd_repo = tempfile::TempDir::new().unwrap();
    let target_repo = tempfile::TempDir::new().unwrap();

    for dir in [cwd_repo.path(), target_repo.path()] {
        let init = Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            init.status.success(),
            "git init failed: {}",
            String::from_utf8_lossy(&init.stderr)
        );
    }

    std::fs::write(cwd_repo.path().join("wrong-repo.txt"), "wrong").unwrap();
    std::fs::write(target_repo.path().join("right-repo.txt"), "right").unwrap();

    let output = tokf()
        .args([
            "run",
            "git",
            "-C",
            target_repo.path().to_str().unwrap(),
            "status",
        ])
        .current_dir(cwd_repo.path())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("right-repo.txt"),
        "expected target repo status, got: {stdout}"
    );
    assert!(
        !stdout.contains("wrong-repo.txt"),
        "status came from current working repo instead of -C target: {stdout}"
    );
}

#[test]
fn run_no_filter_preserves_failing_exit_code() {
    let output = tokf()
        .args([
            "run",
            "--no-mask-exit-code",
            "--no-filter",
            "sh",
            "-c",
            "exit 7",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(7));
}

#[test]
fn run_timing_with_matched_filter() {
    let dir = tempfile::TempDir::new().unwrap();
    let filters_dir = dir.path().join(".tokf/filters");
    std::fs::create_dir_all(&filters_dir).unwrap();
    std::fs::write(
        filters_dir.join("echo.toml"),
        "command = \"echo\"\n[on_success]\noutput = \"filtered\"",
    )
    .unwrap();

    let output = tokf()
        .args(["run", "--timing", "echo", "hello"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("[tokf] filter took"),
        "expected timing output when filter matched, got: {stderr}"
    );
}

#[test]
fn run_embedded_filter_from_empty_dir() {
    // From a directory with no local .tokf/filters, the embedded stdlib should still be active.
    // Use `--verbose` to confirm the built-in filter was matched.
    let dir = tempfile::TempDir::new().unwrap();
    let output = tokf()
        .args(["--verbose", "run", "git", "status"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    // git status may succeed or fail depending on whether dir is a git repo; either is fine.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("built-in") || stderr.contains("git/status"),
        "expected verbose output indicating built-in filter was matched, got: {stderr}"
    );
}

// --- exit-code masking (default on): always exit 0, prepend error line (claude-code#27621) ---

#[test]
fn mask_exit_code_returns_zero_on_failure() {
    let output = tokf()
        .args(["run", "sh", "-c", "echo failure-msg; exit 1"])
        .output()
        .unwrap();
    assert!(output.status.success(), "should exit 0 by default");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("failure-msg"),
        "output should contain command output, got: {stdout}"
    );
    assert!(
        stdout.starts_with("Error: Exit code 1\n"),
        "error line should be prepended, got: {stdout}"
    );
}

#[test]
fn mask_exit_code_no_error_line_on_success() {
    let output = tokf()
        .args(["run", "echo", "success-msg"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("success-msg"),
        "successful command should print to stdout, got: {stdout}"
    );
    assert!(
        !stdout.contains("Error: Exit code"),
        "successful command should not have error line, got: {stdout}"
    );
}

#[test]
fn mask_exit_code_filtered_failure() {
    let dir = tempfile::TempDir::new().unwrap();
    let filters_dir = dir.path().join(".tokf/filters");
    std::fs::create_dir_all(&filters_dir).unwrap();
    std::fs::write(
        filters_dir.join("sh.toml"),
        "command = \"sh\"\n[on_failure]\noutput = \"FILTERED_FAIL\"",
    )
    .unwrap();

    let output = tokf()
        .args(["run", "sh", "-c", "echo raw; exit 1"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "should exit 0 by default");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("FILTERED_FAIL"),
        "filtered output should be present, got: {stdout}"
    );
    assert!(
        stdout.starts_with("Error: Exit code 1\n"),
        "error line should be prepended, got: {stdout}"
    );
}

#[test]
fn mask_exit_code_signal_exit_code() {
    let output = tokf()
        .args(["run", "sh", "-c", "echo signal-msg; exit 130"])
        .output()
        .unwrap();
    assert!(output.status.success(), "should exit 0 by default");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("signal-msg"),
        "output should contain command output, got: {stdout}"
    );
    assert!(
        stdout.starts_with("Error: Exit code 130\n"),
        "should prepend signal exit code, got: {stdout}"
    );
}

#[test]
fn mask_exit_code_empty_output_on_failure() {
    let output = tokf().args(["run", "sh", "-c", "exit 1"]).output().unwrap();
    assert!(output.status.success(), "should exit 0 by default");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.trim(),
        "Error: Exit code 1",
        "empty failure should only show exit code line, got: {stdout}"
    );
}

#[test]
fn passthrough_args_skips_run_override() {
    let dir = tempfile::TempDir::new().unwrap();
    let filters_dir = dir.path().join(".tokf/filters");
    std::fs::create_dir_all(&filters_dir).unwrap();
    // Filter with `run` override that would change the output, plus passthrough_args
    std::fs::write(
        filters_dir.join("echo.toml"),
        r#"command = "echo"
run = "echo FILTERED"
passthrough_args = ["--skip"]

[on_success]
output = "FILTERED_OUTPUT"
"#,
    )
    .unwrap();

    // Without passthrough arg: filter applies, we get "FILTERED_OUTPUT"
    let output = tokf()
        .args(["run", "echo", "hello"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("FILTERED_OUTPUT"),
        "expected filter to apply without passthrough arg, got: {stdout}"
    );

    // With passthrough arg: filter skipped, original command runs
    let output = tokf()
        .args(["run", "echo", "--skip", "hello"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("--skip hello"),
        "expected original command output with passthrough, got: {stdout}"
    );
    assert!(
        !stdout.contains("FILTERED"),
        "filter should be skipped with passthrough arg, got: {stdout}"
    );
}

#[test]
fn passthrough_args_verbose_shows_message() {
    let dir = tempfile::TempDir::new().unwrap();
    let filters_dir = dir.path().join(".tokf/filters");
    std::fs::create_dir_all(&filters_dir).unwrap();
    std::fs::write(
        filters_dir.join("echo.toml"),
        "command = \"echo\"\npassthrough_args = [\"--skip\"]\n[on_success]\noutput = \"FILTERED\"",
    )
    .unwrap();

    let output = tokf()
        .args(["run", "--verbose", "echo", "--skip"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("passthrough"),
        "expected passthrough message in verbose output, got: {stderr}"
    );
}

#[test]
fn no_mask_exit_code_propagates_exit_code() {
    let output = tokf()
        .args([
            "run",
            "--no-mask-exit-code",
            "sh",
            "-c",
            "echo hello; exit 42",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(42));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("hello"),
        "output should contain command output, got: {stdout}"
    );
    assert!(
        !stdout.contains("Error: Exit code"),
        "should not have error line with --no-mask-exit-code, got: {stdout}"
    );
}

// --- args-pattern variant routing ---

#[test]
fn args_pattern_variant_routes_to_child_filter() {
    // A parent filter with passthrough_args would normally skip filtering
    // for --special. With an args-pattern variant, the child filter takes
    // over instead of passing through.
    let dir = tempfile::TempDir::new().unwrap();
    let filters_dir = dir.path().join(".tokf/filters");
    std::fs::create_dir_all(&filters_dir).unwrap();

    // Parent filter: forces "run" override, passes through --special
    std::fs::write(
        filters_dir.join("echo.toml"),
        r#"command = "echo"
run = "echo PARENT_OVERRIDE"

passthrough_args = ["--other"]

[on_success]
output = "PARENT_FILTERED"

[[variant]]
name = "special"
detect.args_pattern = "--special"
filter = "echo-special"
"#,
    )
    .unwrap();

    // Child filter: synthetic command that won't match any real invocation.
    // Variant child filters are only loaded via lookup_filter_by_name, never
    // by command matching, so the command field is just a placeholder.
    std::fs::write(
        filters_dir.join("echo-special.toml"),
        r#"command = "echo-special"

[on_success]
output = "CHILD_FILTERED"
"#,
    )
    .unwrap();

    // With --special: args-pattern variant fires, child filter applies
    let output = tokf()
        .args(["run", "echo", "--special", "hello"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("CHILD_FILTERED"),
        "expected child filter to apply via args-pattern variant, got: {stdout}"
    );
    assert!(
        !stdout.contains("PARENT"),
        "parent filter should not apply when args variant matched, got: {stdout}"
    );

    // Without --special: parent filter applies normally
    let output = tokf()
        .args(["run", "echo", "hello"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("PARENT_FILTERED"),
        "expected parent filter when no args variant matches, got: {stdout}"
    );
}

// Regression test for https://github.com/nicholasgasior/tokf/issues/381:
// gh filters with a `run` override that injects --json must passthrough when
// the user already supplies their own --json / -q / --jq, otherwise the
// template renders an empty stub.
#[test]
fn gh_json_flag_triggers_passthrough() {
    let dir = tempfile::TempDir::new().unwrap();
    let filters_dir = dir.path().join(".tokf/filters/gh/pr");
    std::fs::create_dir_all(&filters_dir).unwrap();
    // Mimic the bundled gh/pr/view filter (command + run + json extract + template)
    std::fs::write(
        filters_dir.join("view.toml"),
        r#"command = "gh pr view *"
run = "echo INJECTED_JSON_SCHEMA"
passthrough_args = ["--web", "-w", "--json", "-q", "--jq"]

[on_success]
output = "FILTERED_TEMPLATE"
"#,
    )
    .unwrap();

    // Without --json: filter applies, we get the template
    let output = tokf()
        .args(["run", "gh", "pr", "view", "1"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("FILTERED_TEMPLATE"),
        "expected filter template without --json, got: {stdout}"
    );

    // With --json: passthrough — original args forwarded, no template rendered
    let output = tokf()
        .args([
            "run", "gh", "pr", "view", "1", "--json", "isDraft", "-q", ".isDraft",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("FILTERED_TEMPLATE"),
        "filter template must not appear when --json is passed, got: {stdout}"
    );
    assert!(
        !stdout.contains("INJECTED_JSON_SCHEMA"),
        "run override must not execute when --json is passed, got: {stdout}"
    );

    // With --jq: same passthrough behaviour
    let output = tokf()
        .args([
            "run", "gh", "pr", "view", "1", "--json", "isDraft", "--jq", ".isDraft",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("FILTERED_TEMPLATE"),
        "filter template must not appear when --jq is passed, got: {stdout}"
    );
}

// --- local environment wrappers (nix develop -c) ---

/// Write a fake `nix` executable into `dir` that skips its own args until it
/// sees `-c`/`--command`, then execs everything after. This lets us exercise
/// the local-wrapper unwrapping end-to-end without a real Nix install.
#[cfg(unix)]
fn write_fake_nix(dir: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;

    let script = "#!/bin/sh\n\
        while [ $# -gt 0 ]; do\n\
        \x20 case \"$1\" in\n\
        \x20   -c|--command) shift; exec \"$@\" ;;\n\
        \x20   *) shift ;;\n\
        \x20 esac\n\
        done\n\
        exit 0\n";
    let nix_path = dir.join("nix");
    std::fs::write(&nix_path, script).unwrap();
    std::fs::set_permissions(&nix_path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// PATH with `bindir` prepended so the fake `nix` shadows any real one while
/// `git` etc. remain resolvable.
#[cfg(unix)]
fn path_with(bindir: &std::path::Path) -> String {
    let existing = std::env::var("PATH").unwrap_or_default();
    format!("{}:{}", bindir.display(), existing)
}

#[cfg(unix)]
#[test]
fn run_nix_develop_unwraps_git_status() {
    let bindir = tempfile::TempDir::new().unwrap();
    write_fake_nix(bindir.path());

    let repo = tempfile::TempDir::new().unwrap();
    let init = Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    assert!(init.status.success());
    std::fs::write(repo.path().join("a-new-file.txt"), "hi").unwrap();

    let output = tokf()
        .args(["run", "--verbose", "nix", "develop", "-c", "git", "status"])
        .env("PATH", path_with(bindir.path()))
        .current_dir(repo.path())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("matched git/status.toml"),
        "expected the git status filter to match through the nix wrapper, got: {stderr}"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("a-new-file.txt"),
        "expected filtered git status to list the new file, got: {stdout}"
    );
}

#[cfg(unix)]
#[test]
fn run_nix_develop_attr_and_flags_unwrap() {
    // `nix develop .#agent --impure -c git status` — attrs/flags before -c.
    let bindir = tempfile::TempDir::new().unwrap();
    write_fake_nix(bindir.path());

    let repo = tempfile::TempDir::new().unwrap();
    let init = Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    assert!(init.status.success());

    let output = tokf()
        .args([
            "run",
            "--verbose",
            "nix",
            "develop",
            ".#agent",
            "--impure",
            "-c",
            "git",
            "status",
        ])
        .env("PATH", path_with(bindir.path()))
        .current_dir(repo.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("matched git/status.toml"),
        "expected match through nix develop with attr + flags, got: {stderr}"
    );
}

/// Real-nix smoke test — opt-in, ignored by default. Runs only when a real
/// `nix` is on PATH. Creates a minimal flake with an empty devShell and checks
/// that `tokf run nix develop -c git status` filters correctly.
///
/// This can download nixpkgs on first run, so it is `#[ignore]`d and only runs
/// via `cargo test -- --ignored`.
#[cfg(unix)]
#[test]
#[ignore = "requires a real nix install; may download nixpkgs"]
fn run_nix_develop_real_nix() {
    let has_nix = Command::new("nix")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !has_nix {
        eprintln!("skipping: no real `nix` on PATH");
        return;
    }

    let repo = tempfile::TempDir::new().unwrap();
    let init = Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    assert!(init.status.success());
    std::fs::write(repo.path().join("real-nix-file.txt"), "hi").unwrap();

    // Arch-agnostic: define an empty devShell for every common system so
    // `nix develop` resolves on x86_64/aarch64 Linux or macOS alike.
    let flake = r#"{
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAll = f: nixpkgs.lib.genAttrs systems (s: f nixpkgs.legacyPackages.${s});
    in {
      devShells = forAll (pkgs: { default = pkgs.mkShellNoCC { }; });
    };
}
"#;
    std::fs::write(repo.path().join("flake.nix"), flake).unwrap();
    // Stage the flake so `nix develop` (which requires a git tree) sees it.
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo.path())
        .output()
        .unwrap();

    let output = tokf()
        .args([
            "run",
            "--verbose",
            "nix",
            "develop",
            "--extra-experimental-features",
            "nix-command flakes",
            "-c",
            "git",
            "status",
        ])
        .current_dir(repo.path())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("matched git/status.toml"),
        "expected the git status filter to match through real nix, got: {stderr}"
    );
}
