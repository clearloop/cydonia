//! Slash command parsing, dispatch, and tab-completion for the REPL.

use crate::repl::runner::Runner;
use anyhow::Result;
use compact_str::CompactString;
use futures_util::StreamExt;
use protocol::message::DownloadEvent;
use rustyline::{
    Context,
    completion::{Completer, Pair},
};
use std::io::Write;

pub const SLASH_COMMANDS: &[&str] = &["/help", "/agent", "/memory", "/switch", "/download"];

/// Rustyline helper providing tab-completion for slash commands.
#[derive(rustyline::Helper, rustyline::Hinter, rustyline::Highlighter, rustyline::Validator)]
pub struct ReplHelper;

impl Completer for ReplHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        let prefix = &line[..pos];
        if !prefix.starts_with('/') {
            return Ok((0, vec![]));
        }
        let candidates = SLASH_COMMANDS
            .iter()
            .filter(|cmd| cmd.starts_with(prefix))
            .map(|cmd| Pair {
                display: cmd.to_string(),
                replacement: cmd.to_string(),
            })
            .collect();
        Ok((0, candidates))
    }
}

/// Dispatch a slash command. Returns `true` if the line was handled.
pub async fn handle_slash(
    runner: &mut Runner,
    agent: &mut CompactString,
    line: &str,
) -> Result<bool> {
    if !line.starts_with('/') {
        return Ok(false);
    }
    let rest = &line[1..];
    let (cmd, arg) = match rest.find(' ') {
        Some(pos) => (&rest[..pos], Some(rest[pos + 1..].trim())),
        None => (rest, None),
    };
    match cmd {
        "help" => {
            println!("Available commands:");
            println!("  /help              — show this help");
            println!("  /agent             — list registered agents");
            println!("  /agent <name>      — show agent details");
            println!("  /memory            — list memory entries");
            println!("  /memory <key>      — get a memory entry");
            println!("  /switch <name>     — switch active agent");
            println!("  /download <model>  — download a model from HuggingFace");
        }
        "agent" => match arg {
            None => {
                let agents = runner.list_agents().await?;
                if agents.is_empty() {
                    println!("No agents registered.");
                } else {
                    for a in agents {
                        let desc = if a.description.is_empty() {
                            "(no description)"
                        } else {
                            a.description.as_str()
                        };
                        println!("  {} — {}", a.name, desc);
                    }
                }
            }
            Some(name) => {
                let d = runner.agent_info(name).await?;
                println!("Name:        {}", d.name);
                println!("Description: {}", d.description);
                let tools = if d.tools.is_empty() {
                    "(none)".to_owned()
                } else {
                    d.tools
                        .iter()
                        .map(|t| t.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                let tags = if d.skill_tags.is_empty() {
                    "(none)".to_owned()
                } else {
                    d.skill_tags
                        .iter()
                        .map(|t| t.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                println!("Tools:       {tools}");
                println!("Skill tags:  {tags}");
                if !d.system_prompt.is_empty() {
                    println!("\nSystem prompt:\n{}", d.system_prompt);
                }
            }
        },
        "memory" => match arg {
            None => {
                let entries = runner.list_memory().await?;
                if entries.is_empty() {
                    println!("No memory entries.");
                } else {
                    for (key, value) in &entries {
                        let preview = if value.len() > 80 {
                            let end = value
                                .char_indices()
                                .nth(77)
                                .map(|(i, _)| i)
                                .unwrap_or(value.len());
                            format!("{}...", &value[..end])
                        } else {
                            value.clone()
                        };
                        println!("  {key}: {preview}");
                    }
                }
            }
            Some(key) => match runner.get_memory(key).await? {
                Some(value) => println!("{value}"),
                None => println!("No entry for key '{key}'."),
            },
        },
        "switch" => match arg {
            Some(name) if !name.is_empty() => {
                *agent = CompactString::from(name);
                println!("Switched to agent '{name}'.");
            }
            _ => println!("Usage: /switch <agent-name>"),
        },
        "download" => match arg {
            Some(model) if !model.is_empty() => {
                let stream = runner.download_stream(model);
                futures_util::pin_mut!(stream);
                let mut current_size: u64 = 0;
                let mut downloaded: u64 = 0;
                let mut current_file = String::new();
                while let Some(result) = stream.next().await {
                    match result? {
                        DownloadEvent::Start { model } => println!("Downloading {model}..."),
                        DownloadEvent::FileStart { filename, size } => {
                            current_file = filename;
                            current_size = size;
                            downloaded = 0;
                        }
                        DownloadEvent::Progress { bytes } => {
                            downloaded += bytes;
                            let pct = if current_size > 0 {
                                downloaded * 100 / current_size
                            } else {
                                0
                            };
                            eprint!(
                                "\r  {} {}% ({} / {})",
                                current_file,
                                pct,
                                format_bytes(downloaded),
                                format_bytes(current_size),
                            );
                            std::io::stderr().flush().ok();
                        }
                        DownloadEvent::FileEnd { filename } => {
                            eprintln!("\r  {filename} done{:30}", "");
                        }
                        DownloadEvent::End { model } => println!("Download complete: {model}"),
                    }
                }
            }
            _ => println!("Usage: /download <model>"),
        },
        _ => println!("Unknown command '{cmd}'. Type /help for available commands."),
    }
    Ok(true)
}

/// Format a byte count as a human-readable string.
fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}
