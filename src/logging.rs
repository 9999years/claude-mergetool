use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

/// Resolve the platform-appropriate log directory, creating it if needed.
fn log_dir() -> Option<PathBuf> {
    let dir = if cfg!(target_os = "macos") {
        dirs::home_dir()?.join("Library/Logs/claude-mergetool")
    } else {
        dirs::state_dir()?.join("claude-mergetool/logs")
    };

    if let Err(e) = fs::create_dir_all(&dir) {
        tracing::warn!("Failed to create log directory {}: {e}", dir.display());
        return None;
    }

    Some(dir)
}

fn format_timestamp() -> String {
    jiff::Zoned::now().strftime("%Y-%m-%dT%H-%M-%S").to_string()
}

fn sanitize_filepath(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '/' | '\\' | ' ' => '_',
            _ => c,
        })
        .collect()
}

pub struct MergeLogger {
    event_file: Option<File>,
    summary_path: Option<PathBuf>,
}

impl MergeLogger {
    pub fn new(filepath: Option<&str>) -> Self {
        let dir = match log_dir() {
            Some(d) => d,
            None => {
                return Self {
                    event_file: None,
                    summary_path: None,
                };
            }
        };

        let summary_path = Some(dir.join("summary.jsonl"));

        let sanitized = filepath.map_or_else(|| "unknown".to_string(), sanitize_filepath);
        let filename = format!("{}_{}.jsonl", format_timestamp(), sanitized);
        let event_file = match File::create(dir.join(&filename)) {
            Ok(f) => Some(f),
            Err(e) => {
                tracing::warn!("Failed to create event log {filename}: {e}");
                None
            }
        };

        Self {
            event_file,
            summary_path,
        }
    }

    pub fn log_event(&mut self, line: &str) {
        if let Some(f) = &mut self.event_file
            && let Err(e) = writeln!(f, "{line}")
        {
            tracing::warn!("Event log write failed, disabling: {e}");
            self.event_file = None;
        }
    }

    pub fn log_summary(&mut self, line: &str) {
        if let Some(path) = &self.summary_path {
            match OpenOptions::new().create(true).append(true).open(path) {
                Ok(mut f) => {
                    if let Err(e) = writeln!(f, "{line}") {
                        tracing::warn!("Summary log write failed: {e}");
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to open summary log: {e}");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_basic() {
        assert_eq!(sanitize_filepath("src/lib.rs"), "src_lib.rs");
    }

    #[test]
    fn sanitize_backslash_and_spaces() {
        assert_eq!(
            sanitize_filepath("path\\to my\\file.rs"),
            "path_to_my_file.rs"
        );
    }

    #[test]
    fn sanitize_already_clean() {
        assert_eq!(sanitize_filepath("README.md"), "README.md");
    }

    #[test]
    fn sanitize_empty() {
        assert_eq!(sanitize_filepath(""), "");
    }

    #[test]
    fn logger_writes_events_and_summary() {
        let dir = tempfile::tempdir().unwrap();
        let event_path = dir.path().join("events.jsonl");
        let summary_path = dir.path().join("summary.jsonl");

        let event_file = File::create(&event_path).unwrap();
        let mut logger = MergeLogger {
            event_file: Some(event_file),
            summary_path: Some(summary_path.clone()),
        };

        // Non-result event: only goes to event file.
        logger.log_event(r#"{"type":"assistant","message":{}}"#);
        // Result event: goes to both event file and summary.
        let result_line = r#"{"type":"result","subtype":"success","is_error":false,"duration_ms":100,"duration_api_ms":90,"num_turns":1,"result":"ok","total_cost_usd":0.01,"usage":{"input_tokens":1,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":1},"modelUsage":{}}"#;
        logger.log_event(result_line);
        logger.log_summary(result_line);

        // Flush by dropping.
        drop(logger);

        let events = fs::read_to_string(&event_path).unwrap();
        let lines: Vec<&str> = events.lines().collect();
        assert_eq!(lines.len(), 2);

        let summary = fs::read_to_string(&summary_path).unwrap();
        let summary_lines: Vec<&str> = summary.lines().collect();
        assert_eq!(summary_lines.len(), 1);
        assert!(summary_lines[0].contains("\"type\":\"result\""));
    }
}
