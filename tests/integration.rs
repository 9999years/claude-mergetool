use std::fs;
use std::process::Command;

use command_error::ChildExt;
use command_error::CommandExt;
use utf8_command::Utf8Output;

fn git_command(dir: &std::path::Path, args: &[&str]) -> Command {
    let mut command = Command::new("git");
    command
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@test.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@test.com");
    command
}

/// Run a git command in the given directory, panicking on failure.
fn git(dir: &std::path::Path, args: &[&str]) -> Utf8Output {
    git_command(dir, args)
        .output_checked_with_utf8(|_| Ok::<_, Option<String>>(()))
        .expect("failed to run git")
}

/// Like `git`, but panics if the command exits non-zero.
fn git_ok(dir: &std::path::Path, args: &[&str]) -> Utf8Output {
    git_command(dir, args).output_checked_utf8().unwrap()
}

const BASE_CONTENT: &str = r#"/// Adds two numbers.
fn add(a: i32, b: i32) -> i32 {
    a + b
}

/// Greets a user.
fn greet(name: &str) -> String {
    format!("Hello, {name}!")
}
"#;

const LEFT_CONTENT: &str = r#"/// Adds two numbers and prints the result.
fn add(a: i32, b: i32) -> i32 {
    let result = a + b;
    println!("{a} + {b} = {result}");
    result
}

/// Greets a user.
fn greet(name: &str) -> String {
    format!("Hello, {name}!")
}
"#;

const RIGHT_CONTENT: &str = r#"/// Adds two numbers and does nothing.
fn add(a: i32, b: i32) -> i32 {
    a + b
}

/// Greets a user warmly.
fn greet(name: &str) -> String {
    format!("Welcome, {name}! Great to see you.")
}
"#;

/// This is a pretty nasty test and it _will_ cost you real-world dollars, so it's disabled by
/// default, but it's there!
///
/// Run it with `cargo test -- --ignored --no-capture`.
#[test]
#[ignore]
fn resolve_merge_conflict() {
    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    let repo = tmp.path();

    // Initialize repo with a base commit.
    git_ok(repo, &["init", "-b", "main"]);
    let file = repo.join("lib.rs");

    fs::write(&file, BASE_CONTENT).unwrap();
    git_ok(repo, &["add", "lib.rs"]);
    git_ok(repo, &["commit", "-m", "base"]);

    // Create the `left` branch with left changes.
    git_ok(repo, &["checkout", "-b", "left"]);
    fs::write(&file, LEFT_CONTENT).unwrap();
    git_ok(repo, &["add", "lib.rs"]);
    git_ok(repo, &["commit", "-m", "left changes"]);

    // Go back to main and create the `right` branch with right changes.
    git_ok(repo, &["checkout", "main"]);
    git_ok(repo, &["checkout", "-b", "right"]);
    fs::write(&file, RIGHT_CONTENT).unwrap();
    git_ok(repo, &["add", "lib.rs"]);
    git_ok(repo, &["commit", "-m", "right changes"]);

    // Merge left into right — this should conflict on the overlapping lines.
    git_ok(repo, &["checkout", "left"]);
    let merge_output = git(repo, &["merge", "right", "--no-edit"]);
    assert!(
        !merge_output.status.success(),
        "expected merge to fail with a conflict, but it succeeded"
    );

    // Configure git to use our built binary as a mergetool.
    let bin = env!("CARGO_BIN_EXE_claude-mergetool");
    let tool_cmd = format!(r#"{bin} merge "$BASE" "$LOCAL" "$REMOTE" -o "$MERGED" -p "$MERGED""#);
    git_ok(repo, &["config", "mergetool.claude.cmd", &tool_cmd]);
    git_ok(repo, &["config", "mergetool.claude.trustExitCode", "true"]);

    // Run the mergetool — this calls `claude` under the hood.
    // May take several minutes while Claude processes the conflict.
    let mergetool_output = Command::new("git")
        .args(["mergetool", "-t", "claude", "--no-prompt"])
        .current_dir(repo)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@test.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@test.com")
        .spawn_checked()
        .expect("failed to launch git mergetool")
        .wait_checked()
        .expect("git mergetool failed");

    assert!(
        mergetool_output.success(),
        "git mergetool failed (exit {})",
        mergetool_output,
    );

    // Read the resolved file.
    let resolved = fs::read_to_string(&file).expect("failed to read resolved file");

    // It must not contain conflict markers.
    assert!(
        !resolved.contains("<<<<<<<"),
        "resolved file still contains <<<<<<< markers:\n{resolved}"
    );
    assert!(
        !resolved.contains("======="),
        "resolved file still contains ======= markers:\n{resolved}"
    );
    assert!(
        !resolved.contains(">>>>>>>"),
        "resolved file still contains >>>>>>> markers:\n{resolved}"
    );

    // It should contain key elements from both sides.
    // Left side added printing logic:
    assert!(
        resolved.contains("println!"),
        "resolved file is missing `println!` from the left side:\n{resolved}"
    );
    // Right side changed the greeting:
    assert!(
        resolved.contains("Welcome"),
        "resolved file is missing `Welcome` from the right side:\n{resolved}"
    );
}
