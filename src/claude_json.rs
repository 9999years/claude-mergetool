use owo_colors::OwoColorize;
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt::Display;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::time::Duration;

pub struct ClaudeEventWriter {
    /// Temp directory prefixes to replace with `$TMPDIR`, longest first.
    temp_dirs: Vec<String>,
    /// Whether we've written any output yet (for stripping leading newlines).
    has_output: AtomicBool,
}

impl ClaudeEventWriter {
    pub fn new() -> miette::Result<Self> {
        let raw = std::env::temp_dir();
        let mut temp_dirs = Vec::new();

        // Add canonicalized path (e.g. /private/tmp on macOS).
        if let Ok(canonical) = raw.canonicalize() {
            let s = canonical.to_string_lossy().into_owned();
            temp_dirs.push(s);
        }

        // Add raw path if it differs from canonical.
        let raw_s = raw.to_string_lossy().into_owned();
        if !temp_dirs.contains(&raw_s) {
            temp_dirs.push(raw_s);
        }

        // Longest first so we don't partially replace a longer prefix.
        temp_dirs.sort_by_key(|b| std::cmp::Reverse(b.len()));

        Ok(Self {
            temp_dirs,
            has_output: AtomicBool::new(false),
        })
    }

    pub fn display<'a>(&'a self, event: &'a str) -> impl Display + 'a {
        RawClaudeEvent {
            event,
            temp_dirs: &self.temp_dirs,
            has_output: &self.has_output,
        }
    }
}

/// Wrapper which displays a raw Claude JSON line when formatted.
struct RawClaudeEvent<'a> {
    event: &'a str,
    temp_dirs: &'a [String],
    has_output: &'a AtomicBool,
}

impl<'a> Display for RawClaudeEvent<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match serde_json::from_str::<ClaudeEvent>(self.event) {
            Ok(event) => {
                let formatted = event.display(self.has_output).to_string();
                let result = self
                    .temp_dirs
                    .iter()
                    .fold(formatted, |s, dir| s.replace(dir.as_str(), "$TMPDIR"));
                write!(f, "{result}")
            }
            Err(_) => {
                tracing::debug!(event = %self.event, "Skipping Claude event");
                Ok(())
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClaudeEvent {
    Assistant {
        message: AssistantMessage,
    },
    Result {
        #[serde(flatten)]
        result: ClaudeResult,
    },
}

impl ClaudeEvent {
    fn display<'a>(&'a self, has_output: &'a AtomicBool) -> ClaudeEventDisplay<'a> {
        ClaudeEventDisplay {
            event: self,
            has_output,
        }
    }
}

struct ClaudeEventDisplay<'a> {
    event: &'a ClaudeEvent,
    has_output: &'a AtomicBool,
}

impl Display for ClaudeEventDisplay<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.event {
            ClaudeEvent::Assistant { message } => {
                for block in &message.content {
                    match block {
                        ContentBlock::Text { text } => {
                            let text = if self.has_output.load(Relaxed) {
                                text.as_str()
                            } else {
                                text.trim_start_matches('\n')
                            };
                            if !text.is_empty() {
                                write!(f, "{}", termimad::term_text(text))?;
                                self.has_output.store(true, Relaxed);
                            }
                        }
                        ContentBlock::ToolUse { name, input } => {
                            match name.as_str() {
                                "Read" | "Write" | "Edit" => {
                                    let path = input.file_path.as_deref().unwrap_or("?");
                                    writeln!(f, "{}", format!("> {name} {path}").dimmed())?;
                                }
                                _ => {
                                    writeln!(f, "> {name}")?;
                                }
                            }
                            self.has_output.store(true, Relaxed);
                        }
                        ContentBlock::Unknown => {}
                    }
                }
            }
            ClaudeEvent::Result {
                result: ClaudeResult::Success(success),
            } => {
                writeln!(f, "{success}")?;
                self.has_output.store(true, Relaxed);
            }
        }
        Ok(())
    }
}

#[derive(Deserialize)]
struct AssistantMessage {
    #[serde(default)]
    content: Vec<ContentBlock>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        name: String,
        #[serde(default)]
        input: ToolInput,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Default, Deserialize)]
struct ToolInput {
    file_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "subtype", rename_all = "snake_case")]
enum ClaudeResult {
    Success(ClaudeSuccess),
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct ClaudeSuccess {
    is_error: bool,
    #[serde(rename = "duration_ms", deserialize_with = "deserialize_millis")]
    duration: Duration,
    #[serde(rename = "duration_api_ms", deserialize_with = "deserialize_millis")]
    api_duration: Duration,
    num_turns: u64,
    result: String,
    total_cost_usd: f64,
    usage: ClaudeUsage,
    // Why does this One field have a different naming format.
    #[serde(rename = "modelUsage")]
    model_usage: HashMap<String, ClaudeModelUsage>,
}

fn deserialize_millis<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
    u64::deserialize(d).map(Duration::from_millis)
}

impl Display for ClaudeSuccess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            format!(
                "Finished in {} ({} API time). Total cost: {} (Salary: {}/yr)",
                HumanTime(self.duration),
                HumanTime(self.api_duration),
                Dollars(self.total_cost_usd),
                Dollars(
                    // expect: $28,654.08/yr
                    // (duration / 1hr)
                    // (cost / (duration / 1hr)) * (hours_per_year) -> dollars
                    (self.total_cost_usd
                        / (self.duration.div_duration_f64(Duration::from_hours(1))))
                        * {
                            const WORKING_HOURS_PER_WEEK: f64 = 40.0;
                            const WORKING_WEEKS_PER_YEAR: f64 = 50.0; // 2 weeks vacation!
                            WORKING_HOURS_PER_WEEK * WORKING_WEEKS_PER_YEAR
                        }
                ),
            )
            .green()
            .bold()
        )?;

        if !self.model_usage.is_empty() {
            write!(f, "{}", "\nUsage by model:".dimmed())?;
            for (name, usage) in &self.model_usage {
                write!(f, "{}", format!("\n    {name}: {usage}").dimmed())?;
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct ClaudeUsage {
    input_tokens: u64,
    cache_creation_input_tokens: u64,
    cache_read_input_tokens: u64,
    output_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeModelUsage {
    input_tokens: u64,
    output_tokens: u64,
    cache_read_input_tokens: u64,
    cache_creation_input_tokens: u64,
    web_search_requests: u64,
    #[serde(rename = "costUSD")]
    cost_usd: f64,
    context_window: u64,
    max_output_tokens: u64,
}

impl Display for ClaudeModelUsage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} input, {} output, {} cache read, {} cache write ({})",
            Tokens(self.input_tokens),
            Tokens(self.output_tokens),
            Tokens(self.cache_read_input_tokens),
            Tokens(self.cache_creation_input_tokens),
            Dollars(self.cost_usd),
        )
    }
}

struct Dollars(f64);

impl Display for Dollars {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.0 < 1_000.0 {
            write!(f, "${:.4}", self.0)
        } else {
            write!(f, "${:.1}k", self.0 / 1_000.0)
        }
    }
}

struct Tokens(u64);

impl Display for Tokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let tokens = self.0;
        if tokens < 1_000 {
            write!(f, "{tokens}")
        } else if tokens < 1_000_000 {
            // Thousands.
            write!(f, "{:.1}k", (tokens as f64) / 1_000.00)
        } else {
            // Millions !!!
            write!(f, "{:.3}m", (tokens as f64) / 1_000_000.00)
        }
    }
}

struct HumanTime(Duration);

impl Display for HumanTime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let duration = self.0;
        if duration < Duration::from_secs(1) {
            write!(f, "{}ms", duration.as_millis())
        } else if duration < Duration::from_mins(1) {
            write!(f, "{:.2}s", duration.as_secs_f32())
        } else {
            write!(f, "{}", humantime::format_duration(duration))
        }
    }
}
