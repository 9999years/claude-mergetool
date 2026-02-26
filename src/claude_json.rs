use serde_json::Value;
use std::fmt::Display;

/// Wrapper which displays a raw Claude JSON line when formatted.
pub struct RawClaudeEvent<'a>(pub &'a str);

impl<'a> Display for RawClaudeEvent<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let event = self.0;
        let json: Value = match serde_json::from_str(event) {
            Ok(v) => v,
            Err(_) => {
                return Ok(());
            }
        };

        match json["type"].as_str() {
            Some("assistant") => {
                // Extract text from message.content[]
                if let Some(content) = json["message"]["content"].as_array() {
                    for block in content {
                        if block["type"].as_str() == Some("text")
                            && let Some(text) = block["text"].as_str()
                        {
                            write!(f, "{text}")?;
                        }
                    }
                }
            }
            Some("tool_use") => {
                let name = json["tool_name"].as_str().unwrap_or("?");
                // Show a short summary of the tool call
                match name {
                    "Read" | "Write" | "Edit" => {
                        let path = json["tool_input"]["file_path"].as_str().unwrap_or("?");
                        writeln!(f, "\n\x1b[2m> {name} {path}\x1b[0m")?;
                    }
                    _ => {
                        writeln!(f, "\n\x1b[2m> {name}\x1b[0m")?;
                    }
                }
            }
            _ => {
                tracing::debug!(%event, "Skipping Claude event");
            }
        }

        Ok(())
    }
}
