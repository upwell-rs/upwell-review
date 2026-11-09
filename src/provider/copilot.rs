//! `copilot --output-format json`: JSONL events whose message and reasoning deltas are reassembled
//! into lines, so a draft is saved the moment its line completes.

use std::io::{self, BufRead, Write};
use std::process::{Command, Stdio};

use serde::Deserialize;
use serde::de::IgnoredAny;

use super::{Invocation, Outcome, Provider, ProviderKind, Transcript, prompt_text};
use crate::stream::{DraftSink, LineBuffers, StreamError, read_lines};

/// The only tools a copilot session may use: reading files and the read-only code-graph tools.
pub const TOOLS: [&str; 11] = [
    "view",
    "glob",
    "rg",
    "blast-radius-blast_radius",
    "blast-radius-find_symbol",
    "blast-radius-symbol_info",
    "blast-radius-call_slice",
    "blast-radius-module_graph",
    "blast-radius-dead_code",
    "blast-radius-diff_blast",
    "codegraph-codegraph_explore",
];

/// Tool groups denied outright, on top of the allow-list.
pub const DENIED: [&str; 4] = ["shell", "write", "memory", "url"];

/// Configured MCP servers a review session must not reach.
pub const DISABLED_MCP_SERVERS: [&str; 3] = ["MCP_DOCKER", "playwright", "railway"];

/// The `copilot` CLI.
#[derive(Debug)]
pub struct Copilot;

/// One copilot JSONL event; unknown event types are ignored.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum Event {
    #[serde(rename = "assistant.message_delta")]
    MessageDelta { data: MessageDelta },
    #[serde(rename = "assistant.reasoning_delta")]
    ReasoningDelta { data: ReasoningDelta },
    #[serde(rename = "assistant.message")]
    Message { data: Message },
    #[serde(rename = "assistant.reasoning")]
    Reasoning { data: Reasoning },
    #[serde(rename = "result")]
    Result {
        #[serde(rename = "exitCode")]
        exit_code: Option<i64>,
    },
    #[serde(other)]
    Other,
}

/// A piece of an assistant message.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MessageDelta {
    message_id: Option<String>,
    delta_content: Option<String>,
}

/// A piece of a reasoning block.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReasoningDelta {
    reasoning_id: Option<String>,
    delta_content: Option<String>,
}

/// A complete assistant message, possibly with tool requests and its reasoning text.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Message {
    message_id: Option<String>,
    content: Option<String>,
    tool_requests: Option<Vec<IgnoredAny>>,
    reasoning_text: Option<String>,
}

/// A complete reasoning block.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Reasoning {
    reasoning_id: Option<String>,
    content: Option<String>,
}

/// Which streamed text a delta belongs to.
#[derive(Debug, PartialEq, Eq, Hash)]
enum Key {
    Message(String),
    Reasoning(String),
}

/// The parser state of one copilot stream.
#[derive(Debug)]
struct State<'s> {
    sink: &'s mut DraftSink,
    buffers: LineBuffers<Key>,
    plain: String,
    response: String,
    exit_code: Option<Option<i64>>,
}

impl State<'_> {
    fn line(&mut self, line: &str) -> io::Result<()> {
        let Ok(event) = serde_json::from_str::<Event>(line) else {
            self.plain.push_str(line);
            self.plain.push('\n');

            return self.sink.scan(line);
        };

        match event {
            Event::MessageDelta { data } => {
                let key = Key::Message(data.message_id.unwrap_or_default());

                self.buffers
                    .push(key, &data.delta_content.unwrap_or_default(), self.sink)
            }
            Event::ReasoningDelta { data } => {
                let key = Key::Reasoning(data.reasoning_id.unwrap_or_default());

                self.buffers
                    .push(key, &data.delta_content.unwrap_or_default(), self.sink)
            }
            Event::Message { data } => self.message(data),
            Event::Reasoning { data } => {
                self.buffers.finish(
                    &Key::Reasoning(data.reasoning_id.unwrap_or_default()),
                    self.sink,
                )?;
                self.sink.scan(&data.content.unwrap_or_default())
            }
            Event::Result { exit_code } => {
                self.exit_code = Some(exit_code);

                Ok(())
            }
            Event::Other => Ok(()),
        }
    }

    /// Flushes the message's open line, scans the whole message and its reasoning, and keeps the last
    /// message without tool requests as the response.
    fn message(&mut self, data: Message) -> io::Result<()> {
        let content = data.content.unwrap_or_default();
        let final_text = data
            .tool_requests
            .is_none_or(|requests| requests.is_empty());

        self.buffers.finish(
            &Key::Message(data.message_id.unwrap_or_default()),
            self.sink,
        )?;
        self.sink.scan(&content)?;
        self.sink.scan(&data.reasoning_text.unwrap_or_default())?;

        if final_text && !content.is_empty() {
            self.response = content;
        }

        Ok(())
    }

    fn transcript(mut self) -> io::Result<Transcript> {
        let outcome = match self.exit_code {
            None => Outcome::Missing,
            Some(Some(0)) => Outcome::Completed,
            Some(Some(code)) => Outcome::Failed(format!("exit code {code}")),
            Some(None) => Outcome::Failed("no exit code".to_owned()),
        };

        self.buffers.finish_all(self.sink)?;
        self.sink.scan(&self.plain)?;

        Ok(Transcript {
            response: self.response,
            structured: None,
            outcome,
        })
    }
}

impl Provider for Copilot {
    const KIND: ProviderKind = ProviderKind::Copilot;

    fn command(&self, call: &Invocation<'_>) -> io::Result<Command> {
        let mut command = Command::new(Self::KIND.cli());

        command
            .current_dir(call.worktree)
            .arg("-p")
            .arg(prompt_text(call)?)
            .args(["--model", call.model, "--reasoning-effort", call.effort])
            .args(["--output-format", "json", "--no-ask-user"])
            .args([
                "--disable-builtin-mcps",
                "--disable-client-telemetry",
                "--no-auto-update",
            ])
            .arg("-C")
            .arg(call.worktree)
            .arg("--add-dir")
            .arg(call.run)
            .arg("--available-tools")
            .args(TOOLS)
            .args(DENIED.map(|tool| format!("--deny-tool={tool}")))
            .stdin(Stdio::null());

        for server in DISABLED_MCP_SERVERS {
            command.args(["--disable-mcp-server", server]);
        }

        Ok(command)
    }

    fn parse<R: BufRead, L: Write>(
        &self,
        input: R,
        log: &mut L,
        sink: &mut DraftSink,
    ) -> Result<Transcript, StreamError> {
        let mut state = State {
            sink,
            buffers: LineBuffers::new(),
            plain: String::new(),
            response: String::new(),
            exit_code: None,
        };

        read_lines(input, log, |line| Ok(state.line(line)?))?;

        Ok(state.transcript()?)
    }
}
