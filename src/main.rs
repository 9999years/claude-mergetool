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
    about = "AI-powered merge conflict resolution"
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

    fn run(&self) -> miette::Result<()> {
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
        .filter_map(|p| p.parent())
        .collect();

        let mut cmd = Command::new("claude");

        cmd.arg("--print")
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
            cmd.arg("--add-dir").arg(*dir);
            for tool in ["Read", "Edit", "Write"] {
                cmd.arg("--allowedTools")
                    .arg(format!("{tool}(//{dir_display}/**)"));
            }
        }

        let mut child = cmd.spawn_checked()?;
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

fn main() -> miette::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Merge(args) => args.run()?,
    }

    Ok(())
}
