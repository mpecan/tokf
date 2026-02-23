#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::process::Command;

use tempfile::TempDir;

fn tokf() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tokf"))
}

/// Write a TOML filter to a temp file; return (`TempDir`, `filter_path`).
fn write_filter(content: &str) -> (TempDir, std::path::PathBuf) {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("filter.toml");
    fs::write(&path, content).unwrap();
    (tmp, path)
}

/// Write a fixture to a temp file; return (`TempDir`, `fixture_path`).
fn write_fixture(content: &str) -> (TempDir, std::path::PathBuf) {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("fixture.txt");
    fs::write(&path, content).unwrap();
    (tmp, path)
}

#[test]
fn lua_filter_inline_source_replaces_output() {
    let (_ftmp, filter) = write_filter(
        r#"
command = "test"
[lua_script]
lang = "luau"
source = 'return "ok"'
"#,
    );
    let (_xtmp, fixture) = write_fixture("anything here");

    let output = tokf()
        .args([
            "test",
            filter.to_str().unwrap(),
            fixture.to_str().unwrap(),
            "--exit-code",
            "0",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "ok");
}

#[test]
fn lua_filter_nil_passthrough() {
    let (_ftmp, filter) = write_filter(
        r#"
command = "test"
[lua_script]
lang = "luau"
source = "return nil"

[on_success]
output = "branch output"
"#,
    );
    let (_xtmp, fixture) = write_fixture("ignored");

    let output = tokf()
        .args([
            "test",
            filter.to_str().unwrap(),
            fixture.to_str().unwrap(),
            "--exit-code",
            "0",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "branch output"
    );
}

#[test]
fn lua_filter_exit_code_branching() {
    let (_ftmp, filter) = write_filter(
        r#"
command = "test"
[lua_script]
lang = "luau"
source = '''
if exit_code == 0 then
    return "success path"
else
    return "failure path"
end
'''
"#,
    );
    let (_xtmp, fixture) = write_fixture("some output");

    let success = tokf()
        .args([
            "test",
            filter.to_str().unwrap(),
            fixture.to_str().unwrap(),
            "--exit-code",
            "0",
        ])
        .output()
        .unwrap();

    assert_eq!(
        String::from_utf8_lossy(&success.stdout).trim(),
        "success path"
    );

    let failure = tokf()
        .args([
            "test",
            filter.to_str().unwrap(),
            fixture.to_str().unwrap(),
            "--exit-code",
            "1",
        ])
        .output()
        .unwrap();

    assert_eq!(
        String::from_utf8_lossy(&failure.stdout).trim(),
        "failure path"
    );
}

#[test]
fn lua_filter_args_accessible() {
    // Create a project dir with a .tokf/filters/ structure so tokf run picks it up
    let tmp = TempDir::new().unwrap();
    let filters_dir = tmp.path().join(".tokf/filters");
    fs::create_dir_all(&filters_dir).unwrap();

    // Filter matches "tokf-lua-args-test" with a Lua script that returns args[1].
    // run = "true" so the underlying command actually succeeds (no real binary needed).
    fs::write(
        filters_dir.join("tokf-lua-args-test.toml"),
        r#"
command = "tokf-lua-args-test"
run = "true"
[lua_script]
lang = "luau"
source = "return args[1] or 'no-args'"
"#,
    )
    .unwrap();

    // Run tokf from that project dir so the filter is discovered
    // The command "tokf-lua-args-test hello" — "hello" becomes remaining_args[0]
    let output = tokf()
        .current_dir(tmp.path())
        .args(["run", "--no-cache", "tokf-lua-args-test", "hello"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "hello");
}

#[test]
fn lua_filter_file_script() {
    // Write the Lua script to a separate file
    let script_tmp = TempDir::new().unwrap();
    let script_path = script_tmp.path().join("filter.luau");
    fs::write(&script_path, r#"return "from file: " .. output"#).unwrap();

    let filter_content = format!(
        r#"
command = "test"
[lua_script]
lang = "luau"
file = "{}"
"#,
        script_path.display()
    );
    let (_ftmp, filter) = write_filter(&filter_content);
    let (_xtmp, fixture) = write_fixture("hello");

    let output = tokf()
        .args([
            "test",
            filter.to_str().unwrap(),
            fixture.to_str().unwrap(),
            "--exit-code",
            "0",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "from file: hello"
    );
}
