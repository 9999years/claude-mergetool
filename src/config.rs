use std::path::{Path, PathBuf};

use miette::IntoDiagnostic;
use miette::miette;
use serde::Deserialize;

const DEFAULT_CONFIG_TEMPLATE: &str = include_str!("../config.toml");

#[derive(Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Override the `--permission-mode` flag (default: "acceptEdits").
    pub permission_mode: Option<String>,
    /// Additional CLI arguments passed to `claude`.
    pub extra_args: Option<Vec<String>>,
    /// Text appended to the default system prompt.
    pub extra_system_prompt: Option<String>,
}

impl Config {
    /// Returns the permission mode, defaulting to `"acceptEdits"`.
    pub fn permission_mode(&self) -> &str {
        self.permission_mode.as_deref().unwrap_or("acceptEdits")
    }

    /// Returns extra CLI args, defaulting to an empty slice.
    pub fn extra_args(&self) -> &[String] {
        self.extra_args.as_deref().unwrap_or_default()
    }

    /// Appends the extra system prompt (if any) to `prompt` with a `\n\n` separator.
    pub fn append_system_prompt(&self, prompt: &mut String) {
        if let Some(extra) = &self.extra_system_prompt {
            prompt.push_str("\n\n");
            prompt.push_str(extra);
        }
    }
}

/// Returns the platform-appropriate default config path.
pub fn default_config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("claude-mergetool").join("config.toml"))
}

/// Load config from the given path, or the default path if `None`.
///
/// - Explicit path: error if missing or malformed.
/// - Default path: returns defaults if missing, errors if malformed.
pub fn load_config(path: Option<&Path>) -> miette::Result<Config> {
    match path {
        Some(path) => {
            let contents = std::fs::read_to_string(path)
                .into_diagnostic()
                .map_err(|e| {
                    miette::miette!("failed to read config file {}: {e}", path.display())
                })?;
            toml::from_str(&contents)
                .into_diagnostic()
                .map_err(|e| miette::miette!("failed to parse config file {}: {e}", path.display()))
        }
        None => {
            let Some(path) = default_config_path() else {
                return Ok(Config::default());
            };
            match std::fs::read_to_string(&path) {
                Ok(contents) => toml::from_str(&contents).into_diagnostic().map_err(|e| {
                    miette::miette!("failed to parse config file {}: {e}", path.display())
                }),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
                Err(e) => Err(miette::miette!(
                    "failed to read config file {}: {e}",
                    path.display()
                )),
            }
        }
    }
}

#[derive(clap::Args, Debug)]
pub struct GenerateConfigArgs {
    /// Write to this path instead of the default config location.
    #[arg(long)]
    output: Option<PathBuf>,
    /// Overwrite an existing config file.
    #[arg(long)]
    force: bool,
}

impl GenerateConfigArgs {
    pub fn run(&self) -> miette::Result<()> {
        let path = match &self.output {
            Some(p) => p.clone(),
            None => default_config_path()
                .ok_or_else(|| miette!("could not determine default config directory"))?,
        };

        if path.exists() && !self.force {
            return Err(miette!(
                "config file already exists at {}\nUse --force to overwrite.",
                path.display()
            ));
        }

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .into_diagnostic()
                .map_err(|e| miette!("failed to create directory {}: {e}", parent.display()))?;
        }

        std::fs::write(&path, DEFAULT_CONFIG_TEMPLATE)
            .into_diagnostic()
            .map_err(|e| miette!("failed to write config file {}: {e}", path.display()))?;

        eprintln!("Wrote default config to {}", path.display());

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty() {
        let config: Config = toml::from_str("").unwrap();
        assert_eq!(config, Config::default());
    }

    #[test]
    fn parse_full() {
        let config: Config = toml::from_str(
            r#"
            permission_mode = "plan"
            extra_args = ["--model", "opus"]
            extra_system_prompt = "Be concise."
            "#,
        )
        .unwrap();
        assert_eq!(
            config,
            Config {
                permission_mode: Some("plan".to_string()),
                extra_args: Some(vec!["--model".to_string(), "opus".to_string()]),
                extra_system_prompt: Some("Be concise.".to_string()),
            }
        );
    }

    #[test]
    fn parse_partial() {
        let config: Config = toml::from_str(
            r#"
            permission_mode = "plan"
            "#,
        )
        .unwrap();
        assert_eq!(
            config,
            Config {
                permission_mode: Some("plan".to_string()),
                extra_args: None,
                extra_system_prompt: None,
            }
        );
    }

    #[test]
    fn unknown_field_rejected() {
        let result: Result<Config, _> = toml::from_str(
            r#"
            permision_mode = "plan"
            "#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn permission_mode_default() {
        let config = Config::default();
        assert_eq!(config.permission_mode(), "acceptEdits");
    }

    #[test]
    fn permission_mode_override() {
        let config = Config {
            permission_mode: Some("plan".to_string()),
            ..Config::default()
        };
        assert_eq!(config.permission_mode(), "plan");
    }

    #[test]
    fn extra_args_default() {
        let config = Config::default();
        assert_eq!(config.extra_args(), &[] as &[String]);
    }

    #[test]
    fn extra_args_override() {
        let config = Config {
            extra_args: Some(vec!["--model".to_string(), "opus".to_string()]),
            ..Config::default()
        };
        assert_eq!(config.extra_args(), &["--model", "opus"]);
    }

    #[test]
    fn append_system_prompt_none() {
        let config = Config::default();
        let mut prompt = "base prompt".to_string();
        config.append_system_prompt(&mut prompt);
        assert_eq!(prompt, "base prompt");
    }

    #[test]
    fn append_system_prompt_some() {
        let config = Config {
            extra_system_prompt: Some("Be concise.".to_string()),
            ..Config::default()
        };
        let mut prompt = "base prompt".to_string();
        config.append_system_prompt(&mut prompt);
        assert_eq!(prompt, "base prompt\n\nBe concise.");
    }

    #[test]
    fn load_missing_explicit_path_errors() {
        let result = load_config(Some(Path::new("/nonexistent/config.toml")));
        assert!(result.is_err());
    }

    #[test]
    fn load_valid_explicit_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "permission_mode = \"plan\"").unwrap();
        let config = load_config(Some(&path)).unwrap();
        assert_eq!(config.permission_mode.as_deref(), Some("plan"));
    }

    #[test]
    fn load_malformed_explicit_path_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "not valid toml [[[").unwrap();
        let result = load_config(Some(&path));
        assert!(result.is_err());
    }

    #[test]
    fn generate_config_writes_template() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let args = GenerateConfigArgs {
            output: Some(path.clone()),
            force: false,
        };
        args.run().unwrap();
        let contents = std::fs::read_to_string(&path).unwrap();
        assert_eq!(contents, DEFAULT_CONFIG_TEMPLATE);
    }

    #[test]
    fn generate_config_errors_if_exists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "existing").unwrap();
        let args = GenerateConfigArgs {
            output: Some(path),
            force: false,
        };
        let result = args.run();
        assert!(result.is_err());
    }

    #[test]
    fn generate_config_force_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "existing").unwrap();
        let args = GenerateConfigArgs {
            output: Some(path.clone()),
            force: true,
        };
        args.run().unwrap();
        let contents = std::fs::read_to_string(&path).unwrap();
        assert_eq!(contents, DEFAULT_CONFIG_TEMPLATE);
    }
}
