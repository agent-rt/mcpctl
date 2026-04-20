//! End-to-end tests against `@modelcontextprotocol/server-everything` via npx.
//!
//! These tests require network + npx on PATH. They are `#[ignore]`d by default so
//! `cargo test` stays hermetic and fast. Run them with:
//!
//!     cargo test --test e2e_stdio -- --ignored --nocapture

use std::fs;
use std::process::Command;

use predicates::prelude::*;
use tempfile::TempDir;

fn npx_available() -> bool {
    Command::new("npx")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn setup_config() -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = serde_json::json!({
        "mcpServers": {
            "everything": {
                "command": "npx",
                "args": ["-y", "@modelcontextprotocol/server-everything"]
            }
        }
    });
    fs::write(
        dir.path().join(".claude.json"),
        serde_json::to_vec_pretty(&cfg).unwrap(),
    )
    .unwrap();
    dir
}

fn cmcp(dir: &TempDir) -> assert_cmd::Command {
    let mut cmd = assert_cmd::Command::cargo_bin("cmcp").unwrap();
    cmd.env("CMCP_CONFIG_DIR", dir.path());
    cmd
}

#[test]
fn own_config_roundtrip_add_list_remove() {
    let dir = tempfile::tempdir().expect("tempdir");

    let run = |cmd: &mut assert_cmd::Command| {
        cmd.env("CMCP_CONFIG_DIR", dir.path());
    };

    // add
    let mut c = assert_cmd::Command::cargo_bin("cmcp").unwrap();
    run(&mut c);
    c.args(["server", "add", "demo", "--command", "echo", "--arg", "hi"])
        .assert()
        .success();

    // list shows it with source=cmcp
    let mut c = assert_cmd::Command::cargo_bin("cmcp").unwrap();
    run(&mut c);
    c.args(["server", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("demo"))
        .stdout(predicate::str::contains("cmcp"));

    // duplicate without --force errors
    let mut c = assert_cmd::Command::cargo_bin("cmcp").unwrap();
    run(&mut c);
    c.args(["server", "add", "demo", "--command", "echo"])
        .assert()
        .failure();

    // remove
    let mut c = assert_cmd::Command::cargo_bin("cmcp").unwrap();
    run(&mut c);
    c.args(["server", "remove", "demo"]).assert().success();

    // list empty
    let mut c = assert_cmd::Command::cargo_bin("cmcp").unwrap();
    run(&mut c);
    c.args(["server", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("no MCP servers"));
}

#[test]
fn own_config_wins_over_claude_code() {
    let dir = tempfile::tempdir().expect("tempdir");
    // Claude Code side says command=from-claude
    fs::write(
        dir.path().join(".claude.json"),
        serde_json::to_vec(&serde_json::json!({
            "mcpServers": {"shared": {"command": "from-claude"}}
        }))
        .unwrap(),
    )
    .unwrap();
    // cmcp side overrides with command=from-cmcp
    fs::write(
        dir.path().join("mcp.json"),
        serde_json::to_vec(&serde_json::json!({
            "mcpServers": {"shared": {"command": "from-cmcp"}}
        }))
        .unwrap(),
    )
    .unwrap();

    let mut c = assert_cmd::Command::cargo_bin("cmcp").unwrap();
    c.env("CMCP_CONFIG_DIR", dir.path())
        .args(["server", "show", "shared"])
        .assert()
        .success()
        .stdout(predicate::str::contains("from-cmcp"))
        .stdout(predicate::str::contains("source:      cmcp"));
}

#[test]
#[ignore = "requires npx and network; run with --ignored"]
fn server_list_finds_everything() {
    if !npx_available() {
        eprintln!("skip: npx not available");
        return;
    }
    let dir = setup_config();
    cmcp(&dir)
        .args(["server", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("everything"));
}

#[test]
#[ignore = "requires npx and network; run with --ignored"]
fn tool_list_returns_tools() {
    if !npx_available() {
        eprintln!("skip: npx not available");
        return;
    }
    let dir = setup_config();
    cmcp(&dir)
        .args(["tool", "list", "everything", "--timeout", "90"])
        .assert()
        .success()
        // server-everything exposes an `echo` tool at minimum
        .stdout(predicate::str::contains("echo"));
}

#[test]
#[ignore = "requires npx and network; run with --ignored"]
fn call_echo_roundtrip() {
    if !npx_available() {
        eprintln!("skip: npx not available");
        return;
    }
    let dir = setup_config();
    cmcp(&dir)
        .args([
            "mcp://everything/echo",
            "--arg",
            "message=hi-from-cmcp",
            "--timeout",
            "90",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("hi-from-cmcp"));
}
