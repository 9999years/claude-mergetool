use clap::Parser;
use clap::ValueEnum;
use command_error::ChildExt;
use command_error::CommandExt;
use command_error::Utf8ProgramAndArgs;
use miette::Context;
use miette::IntoDiagnostic;
use miette::miette;
use owo_colors::OwoColorize;
use std::collections::BTreeSet;
use std::fmt::Display;
use std::io::Write;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tracing::level_filters::LevelFilter;

mod claude_json;

#[derive(Parser, Debug)]
#[command(
    name = "claude-mergetool",
    about = "AI-powered merge conflict resolution",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand, Debug)]
enum Commands {
    /// Resolve a merge conflict using Claude
    Merge(MergeArgs),
    /// Install `claude-mergetool` as a merge tool for Git or jj.
    Install(InstallArgs),
}

#[derive(clap::Args, Debug)]
struct InstallArgs {
    /// Programs to configure `claude-mergetool` for. Defaults to `git` and `jj` (if available).
    #[arg()]
    programs: Vec<InstallProgram>,
}

impl InstallArgs {
    pub fn run(mut self) -> miette::Result<()> {
        if self.programs.is_empty() {
            self.programs = InstallProgram::default_values();
            if self.programs.is_empty() {
                return Err(miette!("Neither `git` nor `jj` is available"));
            }
        }

        tracing::debug!(programs = ?self.programs, "Determined programs to configure");

        for program in self.programs {
            tracing::info!("Configuring `claude-mergetool` for {program}");
            program.install().wrap_err_with(|| {
                format!("Failed to configure `claude-mergetool` for `{program}`")
            })?;
        }

        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum InstallProgram {
    /// Install `claude-mergetool` as a merge tool for Git.
    Git,

    /// Install `claude-mergetool` as a merge tool for jj.
    Jj,
}

impl Display for InstallProgram {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.program())
    }
}

impl InstallProgram {
    pub fn program(&self) -> &'static str {
        match self {
            InstallProgram::Git => "git",
            InstallProgram::Jj => "jj",
        }
    }

    pub fn is_available(&self) -> bool {
        Command::new(self.program())
            .arg("--version")
            .output_checked()
            .is_ok()
    }

    pub fn default_values() -> Vec<Self> {
        Self::value_variants()
            .iter()
            .copied()
            .filter(|program| program.is_available())
            .collect()
    }

    fn config_set_command(&self, name: &str, value: &str) -> Command {
        let mut command = Command::new(self.program());
        command.arg("config");
        command.arg("set");
        match self {
            InstallProgram::Git => {
                command.arg("--global");
            }
            InstallProgram::Jj => {
                command.arg("--user");
            }
        }
        command.arg(name);
        command.arg(value);
        command
    }

    fn config_set(&self, name: &str, value: &str) -> miette::Result<()> {
        let mut command = self.config_set_command(name, value);
        tracing::info!("$ {}", Utf8ProgramAndArgs::from(&command));

        let output = command.output_checked_utf8()?;
        tracing::info!("{output:#?}");

        Ok(())
    }

    pub fn install(&self) -> miette::Result<()> {
        match self {
            InstallProgram::Git => {
                self.config_set(
                    "mergetool.claude.cmd",
                    r#"claude-mergetool merge "$BASE" "$LOCAL" "$REMOTE" -o "$MERGED""#,
                )?;

                self.config_set("mergetool.claude.trustExitCode", "true")?;

                self.config_set("mergetool.claude.trustExitCode", "true")?;
            }
            InstallProgram::Jj => {
                self.config_set("merge-tools.claude.program", "claude-mergetool")?;

                self.config_set(
                    "merge-tools.claude.merge-args",
                    r#"["merge", "$base", "$left", "$right", "-o", "$output", "-p", "$path"]"#,
                )?;
            }
        }

        Ok(())
    }
}

#[derive(clap::Args, Debug)]
struct MergeArgs {
    /// Git merge driver mode (writes result to `<left>` path)
    #[arg(long)]
    git_merge_driver: bool,

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
    #[arg(short = 'p')]
    filepath: Option<String>,

    /// Conflict marker size
    #[arg(short = 'l')]
    marker_size: Option<u32>,
}

impl MergeArgs {
    fn output_path(&self) -> miette::Result<&Path> {
        match (self.output.as_deref(), self.git_merge_driver) {
            (Some(path), _) => Ok(path),
            (None, true) => Ok(&self.left),
            (None, false) => Err(miette::miette!(
                "either --git-merge-driver or -o <path> is required"
            )),
        }
    }

    fn filepath(&self) -> &str {
        self.filepath.as_deref().unwrap_or("unknown file")
    }

    fn command(&self) -> miette::Result<Command> {
        if let Some(filepath) = &self.filepath {
            eprintln!(
                "{}",
                format!("Resolving merge conflict in {}", filepath.underline())
                    .bold()
                    .green()
            );
        }

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
            self.filepath(),
            self.left_label,
            self.right_label,
        );

        let user_prompt = format!(
            "Resolve the merge conflict in `{}`.\n\n\
             Read these three versions of the file:\n\
             - Base (common ancestor): {}\n\
             - Left ({}): {}\n\
             - Right ({}): {}\n\n\
             Write the resolved file to: {}",
            self.filepath(),
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

        tracing::debug!("Claude command: {}", Utf8ProgramAndArgs::from(&command));

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

        let writer = claude_json::ClaudeEventWriter::new()?;

        for line in reader.lines() {
            match line {
                Ok(line) => {
                    write!(std::io::stderr().lock(), "{}", writer.display(&line))
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
            git_merge_driver: true,
            base: PathBuf::from("/tmp/base.txt"),
            left: PathBuf::from("/tmp/left.txt"),
            right: PathBuf::from("/tmp/right.txt"),
            output: None,
            ancestor_label: None,
            left_label: "ours".to_string(),
            right_label: "theirs".to_string(),
            filepath: Some("src/lib.rs".to_string()),
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
            git_merge_driver: false,
            base: PathBuf::from("/tmp/base.txt"),
            left: PathBuf::from("/tmp/left.txt"),
            right: PathBuf::from("/tmp/right.txt"),
            output: Some(PathBuf::from("/tmp/output.txt")),
            ancestor_label: Some("ancestor".to_string()),
            left_label: "current".to_string(),
            right_label: "incoming".to_string(),
            filepath: Some("README.md".to_string()),
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

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .without_time()
        .with_writer(std::io::stderr)
        .init();

    tracing::debug!("Parsed arguments:{cli:#?}");

    match cli.command {
        Commands::Merge(args) => args.run()?,
        Commands::Install(install) => install.run()?,
    }

    Ok(())
}
