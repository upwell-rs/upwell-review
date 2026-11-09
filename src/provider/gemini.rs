//! `agy --output-format stream-json`: step updates whose text and thinking deltas are reassembled
//! into lines, then a final result event with the structured answer.

use std::io::{self, BufRead, Write};
use std::process::{Command, Stdio};

use serde::Deserialize;
use serde_json::Value;

use super::{Invocation, Outcome, Provider, ProviderKind, Transcript, prompt_text};
use crate::stream::{DraftSink, LineBuffers, StreamError, read_lines};

/// The agy permission settings: only the read-only code-graph MCP tools are allowed.
pub const SETTINGS: &str = r#"{"permissions":{"allow":["mcp(blast-radius/open_project)","mcp(blast-radius/find_symbol)","mcp(blast-radius/symbol_info)","mcp(blast-radius/blast_radius)","mcp(blast-radius/diff_blast)","mcp(blast-radius/call_slice)","mcp(blast-radius/module_graph)","mcp(blast-radius/dead_code)","mcp(codegraph/codegraph_explore)"]}}"#;

/// The status of a successful agy result.
const SUCCESS: &str = "SUCCESS";

/// The `agy` CLI, which runs the gemini models.
#[derive(Debug)]
pub struct Gemini;

/// One agy stream-json event; unknown events are ignored.
#[derive(Debug, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
enum Event {
    StepUpdate {
        step_update: StepUpdate,
    },
    Result {
        result: Final,
    },
    #[serde(other)]
    Other,
}

/// A step's text and thinking deltas and its state.
#[derive(Debug, Deserialize)]
struct StepUpdate {
    step_index: Option<Step>,
    text_delta: Option<String>,
    thinking_delta: Option<String>,
    state: Option<String>,
}

/// A step index as agy writes it.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Deserialize)]
#[serde(untagged)]
enum Step {
    Number(i64),
    Text(String),
}

/// The final result: its status, the full response and the structured answer.
#[derive(Debug, Deserialize)]
struct Final {
    status: Option<String>,
    response: Option<String>,
    structured_output: Option<Value>,
}

/// Which streamed text of a step a delta belongs to.
#[derive(Debug, PartialEq, Eq, Hash)]
enum Key {
    Text(Option<Step>),
    Thinking(Option<Step>),
}

/// The parser state of one agy stream.
#[derive(Debug)]
struct State<'s> {
    sink: &'s mut DraftSink,
    buffers: LineBuffers<Key>,
    plain: String,
    result: Option<Final>,
}

impl State<'_> {
    fn line(&mut self, line: &str) -> io::Result<()> {
        let Ok(event) = serde_json::from_str::<Event>(line) else {
            self.plain.push_str(line);
            self.plain.push('\n');

            return self.sink.scan(line);
        };

        match event {
            Event::StepUpdate { step_update } => self.step(step_update),
            Event::Result { result } => {
                self.result = Some(result);

                Ok(())
            }
            Event::Other => Ok(()),
        }
    }

    fn step(&mut self, update: StepUpdate) -> io::Result<()> {
        let step = update.step_index;
        let done = update.state.as_deref() == Some("DONE");

        if let Some(delta) = update.text_delta {
            self.buffers
                .push(Key::Text(step.clone()), &delta, self.sink)?;
        }

        if let Some(delta) = update.thinking_delta {
            self.buffers
                .push(Key::Thinking(step.clone()), &delta, self.sink)?;
        }

        if done {
            self.buffers.finish(&Key::Text(step.clone()), self.sink)?;
            self.buffers.finish(&Key::Thinking(step), self.sink)?;
        }

        Ok(())
    }

    fn transcript(mut self) -> io::Result<Transcript> {
        self.buffers.finish_all(self.sink)?;
        self.sink.scan(&self.plain)?;

        let Some(result) = self.result else {
            return Ok(Transcript {
                response: String::new(),
                structured: None,
                outcome: Outcome::Missing,
            });
        };
        let outcome = match result.status.as_deref() {
            Some(SUCCESS) => Outcome::Completed,
            status => Outcome::Failed(format!("status {}", status.unwrap_or("None"))),
        };

        Ok(Transcript {
            response: result.response.unwrap_or_default(),
            structured: result.structured_output,
            outcome,
        })
    }
}

impl Provider for Gemini {
    const KIND: ProviderKind = ProviderKind::Gemini;

    fn command(&self, call: &Invocation<'_>) -> io::Result<Command> {
        let mut command = Command::new(Self::KIND.cli());

        command
            .current_dir(call.worktree)
            .args(["--model", call.model, "--mode", "plan", "--sandbox"])
            .args([
                "--output-format",
                "stream-json",
                "--settings",
                SETTINGS,
                "--json-schema",
            ])
            .arg(call.schema)
            .arg("--add-dir")
            .arg(call.worktree)
            .arg("--add-dir")
            .arg(call.run)
            .arg("-p")
            .arg(prompt_text(call)?)
            .stdin(Stdio::null());

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
            result: None,
        };

        read_lines(input, log, |line| Ok(state.line(line)?))?;

        Ok(state.transcript()?)
    }
}
