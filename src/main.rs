use clap::Parser;
use command_error::ChildExt;
use command_error::CommandExt;
use miette::IntoDiagnostic;
use std::collections::BTreeSet;
use std::io::Write;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

mod claude_json;

#[derive(Parser)]
#[command(
    name = "claude-mergetool",
    about = "AI-powered merge conflict resolution",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Resolve a merge conflict using Claude
    Merge(MergeArgs),
}

#[derive(clap::Args)]
struct MergeArgs {
    /// Git merge driver mode (writes result to `<left>` path)
    #[arg(long)]
    git: bool,

    /// Base version (common ancestor)
    base: PathBuf,
    /// Left version (ours / current branch)
    left: PathBuf,
    /// Right version (theirs / incoming)
    right: PathBuf,

    /// Output file path (jj mode)
    #[arg(short = 'o', long)]
    output: Option<PathBuf>,

    /// Ancestor conflict label
    #[arg(short = 's')]
    ancestor_label: Option<String>,
    /// Left/ours conflict label
    #[arg(short = 'x', default_value = "ours")]
    left_label: String,
    /// Right/theirs conflict label
    #[arg(short = 'y', default_value = "theirs")]
    right_label: String,

    /// Original file path
    #[arg(short = 'p', default_value = "unknown file")]
    filepath: String,

    /// Conflict marker size
    #[arg(short = 'l')]
    marker_size: Option<u32>,
}

impl MergeArgs {
    fn output_path(&self) -> miette::Result<&Path> {
        match (self.output.as_deref(), self.git) {
            (Some(path), _) => Ok(path),
            (None, true) => Ok(&self.left),
            (None, false) => Err(miette::miette!("either --git or -o <path> is required")),
        }
    }

    fn command(&self) -> miette::Result<Command> {
        let system_prompt = format!(
            "You are resolving a merge conflict in `{}`. \
             Your working directory is the root of the repository, so you can browse and edit \
             other files if needed (e.g. if code moved between files).\n\n\
             Three versions of the file are provided as temporary files: \
             the base (common ancestor), left ({}), and right ({}). \
             Read all three, understand what each side changed relative to the base, \
             and write a resolved version to the output path. \
             If changes are compatible, merge them cleanly. \
             If they genuinely conflict, use your best judgment and explain your reasoning.",
            self.filepath, self.left_label, self.right_label,
        );

        let user_prompt = format!(
            "Resolve the merge conflict in `{}`.\n\n\
             Read these three versions of the file:\n\
             - Base (common ancestor): {}\n\
             - Left ({}): {}\n\
             - Right ({}): {}\n\n\
             Write the resolved file to: {}",
            self.filepath,
            self.base.display(),
            self.left_label,
            self.left.display(),
            self.right_label,
            self.right.display(),
            self.output_path()?.display(),
        );

        // Collect unique parent dirs from all temp file paths and grant
        // Read/Write/Edit access so Claude can work with them without prompts.
        let temp_dirs: BTreeSet<_> = [
            self.base.as_path(),
            self.left.as_path(),
            self.right.as_path(),
            self.output_path()?,
        ]
        .iter()
        .filter_map(|p| p.parent().filter(|p| *p != ""))
        .collect();

        let mut command = Command::new("claude");

        command
            .arg("--print")
            .arg("--verbose")
            .arg("--output-format=stream-json")
            .arg("--permission-mode=acceptEdits")
            .arg("--append-system-prompt")
            .arg(&system_prompt)
            .arg(user_prompt)
            .stdin(Stdio::null())
            .stdout(Stdio::piped());

        for dir in &temp_dirs {
            let dir_display = dir.display();
            tracing::debug!("Granting access to {dir_display}");
            command.arg("--add-dir").arg(*dir);
        }

        Ok(command)
    }

    fn run(&self) -> miette::Result<()> {
        let mut child = self.command()?.spawn_checked()?;
        let stdout = child
            .child_mut()
            .stdout
            .take()
            .expect("claude piped stdout should have a stdout field");
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(line) => {
                    write!(
                        std::io::stderr().lock(),
                        "{}",
                        claude_json::RawClaudeEvent(&line)
                    )
                    .into_diagnostic()?;
                }
                Err(err) => {
                    tracing::debug!("{err}");
                }
            }
        }

        child.wait_checked()?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use command_error::Utf8ProgramAndArgs;
    use expect_test::expect;

    #[test]
    fn command_git_mode() {
        let args = MergeArgs {
            git: true,
            base: PathBuf::from("/tmp/base.txt"),
            left: PathBuf::from("/tmp/left.txt"),
            right: PathBuf::from("/tmp/right.txt"),
            output: None,
            ancestor_label: None,
            left_label: "ours".to_string(),
            right_label: "theirs".to_string(),
            filepath: "src/lib.rs".to_string(),
            marker_size: None,
        };
        let command = args.command().unwrap();
        let displayed: Utf8ProgramAndArgs = (&command).into();
        expect![[r#"
            claude --print --verbose '--output-format=stream-json' '--permission-mode=acceptEdits' --append-system-prompt 'You are resolving a merge conflict in `src/lib.rs`. Your working directory is the root of the repository, so you can browse and edit other files if needed (e.g. if code moved between files).

            Three versions of the file are provided as temporary files: the base (common ancestor), left (ours), and right (theirs). Read all three, understand what each side changed relative to the base, and write a resolved version to the output path. If changes are compatible, merge them cleanly. If they genuinely conflict, use your best judgment and explain your reasoning.' 'Resolve the merge conflict in `src/lib.rs`.

            Read these three versions of the file:
            - Base (common ancestor): /tmp/base.txt
            - Left (ours): /tmp/left.txt
            - Right (theirs): /tmp/right.txt

            Write the resolved file to: /tmp/left.txt' --add-dir /tmp"#]].assert_eq(&displayed.to_string());
    }

    #[test]
    fn command_output_mode() {
        let args = MergeArgs {
            git: false,
            base: PathBuf::from("/tmp/base.txt"),
            left: PathBuf::from("/tmp/left.txt"),
            right: PathBuf::from("/tmp/right.txt"),
            output: Some(PathBuf::from("/tmp/output.txt")),
            ancestor_label: Some("ancestor".to_string()),
            left_label: "current".to_string(),
            right_label: "incoming".to_string(),
            filepath: "README.md".to_string(),
            marker_size: Some(7),
        };
        let command = args.command().unwrap();
        let displayed: Utf8ProgramAndArgs = (&command).into();
        expect![[r#"
            claude --print --verbose '--output-format=stream-json' '--permission-mode=acceptEdits' --append-system-prompt 'You are resolving a merge conflict in `README.md`. Your working directory is the root of the repository, so you can browse and edit other files if needed (e.g. if code moved between files).

            Three versions of the file are provided as temporary files: the base (common ancestor), left (current), and right (incoming). Read all three, understand what each side changed relative to the base, and write a resolved version to the output path. If changes are compatible, merge them cleanly. If they genuinely conflict, use your best judgment and explain your reasoning.' 'Resolve the merge conflict in `README.md`.

            Read these three versions of the file:
            - Base (common ancestor): /tmp/base.txt
            - Left (current): /tmp/left.txt
            - Right (incoming): /tmp/right.txt

            Write the resolved file to: /tmp/output.txt' --add-dir /tmp"#]].assert_eq(&displayed.to_string());
    }
}

fn main() -> miette::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Merge(args) => args.run()?,
    }

    Ok(())
}
