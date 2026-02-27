use clap::ValueEnum;
use command_error::CommandExt;
use command_error::Utf8ProgramAndArgs;
use miette::Context;
use miette::miette;
use std::fmt::Display;
use std::process::Command;

#[derive(clap::Args, Debug)]
pub struct InstallArgs {
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
