//! The agent process: Claude Code (`claude -p`) or Codex (`codex exec`) with one MCP server,
//! `word-cli mcp --connect 127.0.0.1:<port>`, that drives this window. Its JSON output becomes
//! [`Event`]s for the pane.
//!
//! Tokens are spent on the work, not on the set-up: Claude gets a short system prompt of its own
//! instead of Claude Code's long default one, no built-in tools (only Read when files are
//! attached) and only the document tools it needs; a conversation resumes its session rather than
//! resending what was said.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

/// What the agent is told besides the user's message.
pub const INSTRUCTIONS: &str = "You are the writing assistant built into the Word word processor. You edit the document \
the user has open, live, through the `word` tools.

Work efficiently:
- Look only as much as you need: inspect_document for structure, get_text for wording. Don't re-read the document after \
every change.
- Write new or rewritten content in one call per section: execute edit.paste {\"html\": \"<h1>…</h1><p>…</p><ul><li>…</li></ul><table>…</table>\"}. \
Headings become real Heading styles. Run several commands at once with batch. Avoid many small type_text calls.
- To change a passage, select_text it, then type_text or edit.paste over the selection.
- Useful commands: insert.coverPage {title, subtitle, author, style: classic|band|minimal|modern, accent: RRGGBB}, \
references.toc {levels} and references.updateToc, design.theme {name: Craft|Studio|Gallery|Atelier|Darkroom|Sketchbook}, \
design.paragraphSpacing {value}, para.style {style}, insert.pageNumber {position, align}, insert.header / insert.footer {text}, \
caret.docStart, text.pageBreak. list_commands {query} finds any other.
- Don't create, open, save or close documents unless asked; with no document open, start one with new_document.
- On a long task, finish it section by section so the work done stays in the document if you are interrupted.

Reply to the user in their language, in one to three short sentences: what is now in the document, or what you need from them. \
Never mention tools, commands, HTML or technical steps.";

/// Asked when a stopped or failed task is picked up again.
pub const CONTINUE: &str = "Continue the task you were doing when you were interrupted, from where it stopped. \
Check the document's current state first and don't redo the parts already done.";

/// Document tools the agent may use; the rest (screenshots, clicks, keys, files) stay out of its
/// context.
const TOOLS: [&str; 8] = ["execute", "batch", "type_text", "select_text", "get_text", "inspect_document", "list_commands", "new_document"];
const OFF_TOOLS: [&str; 8] = ["screenshot", "click", "key", "ui_inspect", "parity", "render_page", "open_document", "save_document"];

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Agent {
    #[default]
    Claude,
    Codex,
}

impl Agent {
    pub fn program(self) -> &'static str {
        match self {
            Agent::Claude => "claude",
            Agent::Codex => "codex",
        }
    }

    pub fn id(self) -> &'static str {
        self.program()
    }

    pub fn from_id(id: &str) -> Option<Agent> {
        match id {
            "claude" => Some(Agent::Claude),
            "codex" => Some(Agent::Codex),
            _ => None,
        }
    }
}

/// Claude models. Auto is Sonnet: good writing at a fraction of Opus's cost.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Model {
    #[default]
    Auto,
    Opus,
    Sonnet,
    Haiku,
}

impl Model {
    pub const ALL: [Model; 4] = [Model::Auto, Model::Opus, Model::Sonnet, Model::Haiku];

    fn alias(self) -> &'static str {
        match self {
            Model::Auto | Model::Sonnet => "sonnet",
            Model::Opus => "opus",
            Model::Haiku => "haiku",
        }
    }
}

/// What the pane learns from the agent's output.
#[derive(Debug, PartialEq)]
pub enum Event {
    /// A reply for the user.
    Text(String),
    /// A tool call: the tool and, for `execute`, the command id (for the activity line).
    Tool(String, Option<String>),
    /// The session (Claude) or thread (Codex) to resume next time.
    Session(String),
    Error(String),
    /// The run ended: `ok` when the agent finished its turn.
    Done {
        ok: bool,
    },
}

pub struct Running {
    pub rx: Receiver<Event>,
    child: Arc<Mutex<Option<Child>>>,
    pub started: f64,
}

impl Running {
    pub fn stop(&self) {
        if let Ok(mut c) = self.child.lock()
            && let Some(c) = c.as_mut()
        {
            let _ = c.kill();
        }
    }
}

impl Drop for Running {
    /// Closing the window (or the pane's state) ends the agent too.
    fn drop(&mut self) {
        self.stop();
    }
}

/// One agent run.
pub struct Job {
    pub agent: Agent,
    pub model: Model,
    pub port: u16,
    pub session: Option<String>,
    /// Folders of the attached files, which the agent may read.
    pub dirs: Vec<PathBuf>,
}

/// `name` on `PATH`, else in the usual per-user install folders (a desktop launcher's `PATH` often
/// lacks `~/.local/bin`, where Claude Code installs itself).
pub fn find_program(name: &str) -> Option<PathBuf> {
    let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from) {
        dirs.extend([".local/bin", ".npm-global/bin", ".claude/local", ".cargo/bin"].map(|d| home.join(d)));
        dirs.push(home.join("AppData/Roaming/npm"));
    }
    dirs.into_iter().map(|d| d.join(&file)).find(|p| p.is_file())
}

/// The command-line tool next to this executable (`word-cli`, or `wordcraft-cli` in a build tree).
fn cli_path() -> PathBuf {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        for name in ["word-cli", "wordcraft-cli"] {
            let p = dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
            if p.is_file() {
                return p;
            }
        }
    }
    find_program("word-cli").unwrap_or_else(|| "word-cli".into())
}

pub fn command(job: &Job, program: PathBuf, cli: &str) -> Command {
    let connect = format!("127.0.0.1:{}", job.port);
    let mut c = Command::new(program);
    match job.agent {
        Agent::Claude => {
            let mcp = json!({"mcpServers": {"word": {"command": cli, "args": ["mcp", "--connect", connect]}}}).to_string();
            // The prompt goes on standard input. --tools, --add-dir, --disallowedTools and
            // --allowedTools take several values, so each is followed by another flag or ends the line.
            c.args(["-p", "--output-format", "stream-json", "--verbose", "--strict-mcp-config"]);
            c.args(["--tools", if job.dirs.is_empty() { "" } else { "Read" }]);
            c.args(["--system-prompt", INSTRUCTIONS, "--mcp-config", mcp.as_str(), "--model", job.model.alias()]);
            if let Some(s) = &job.session {
                c.args(["--resume", s.as_str()]);
            }
            if !job.dirs.is_empty() {
                c.arg("--add-dir").args(&job.dirs);
            }
            c.arg("--disallowedTools").args(OFF_TOOLS.map(|t| format!("mcp__word__{t}")));
            c.arg("--allowedTools").args(TOOLS.map(|t| format!("mcp__word__{t}")));
            if !job.dirs.is_empty() {
                c.arg("Read");
            }
        }
        Agent::Codex => {
            let toml_str = |s: &str| Value::String(s.to_string()).to_string();
            let tools: Vec<String> = TOOLS.iter().map(|t| toml_str(t)).collect();
            c.args(["exec", "--json", "--skip-git-repo-check"]);
            c.args(["-c".to_string(), format!("mcp_servers.word.command={}", toml_str(cli))]);
            c.args(["-c".to_string(), format!("mcp_servers.word.args=[\"mcp\", \"--connect\", {}]", toml_str(&connect))]);
            c.args(["-c".to_string(), format!("mcp_servers.word.enabled_tools=[{}]", tools.join(", "))]);
            if let Some(s) = &job.session {
                c.args(["resume", s.as_str()]);
            }
            c.arg("-");
        }
    }
    c
}

/// What goes on the agent's standard input: Codex has no system prompt flag, so its first turn
/// carries the instructions (a resumed thread already has them).
fn stdin_text(job: &Job, prompt: &str) -> String {
    match job.agent {
        Agent::Codex if job.session.is_none() => format!("{INSTRUCTIONS}\n\n{prompt}"),
        _ => prompt.to_string(),
    }
}

/// Start the agent; `missing` is the message for a missing program.
pub fn spawn(job: &Job, prompt: &str, ctx: egui::Context, missing: &str) -> Result<Running, String> {
    let agent = job.agent;
    let Some(program) = find_program(agent.program()) else {
        return Err(missing.to_string());
    };
    let cli = cli_path();
    let mut cmd = command(job, program, &cli.to_string_lossy());
    // Away from the user's files: the agent reaches the document through the MCP bridge and
    // only the attached files' folders besides.
    cmd.current_dir(std::env::temp_dir()).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut cmd, 0x0800_0000); // CREATE_NO_WINDOW
    let mut child = cmd.spawn().map_err(|e| format!("{}: {e}", agent.program()))?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(stdin_text(job, prompt).as_bytes());
    }
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let stderr = child.stderr.take();
    let child = Arc::new(Mutex::new(Some(child)));
    let (tx, rx) = channel();
    let errors = Arc::new(Mutex::new(String::new()));
    if let Some(mut stderr) = stderr {
        let errors = errors.clone();
        std::thread::spawn(move || {
            let mut s = String::new();
            let _ = stderr.read_to_string(&mut s);
            if let Ok(mut e) = errors.lock() {
                *e = s;
            }
        });
    }
    let waiter = child.clone();
    std::thread::spawn(move || read_events(agent, stdout, tx, waiter, errors, ctx));
    Ok(Running { rx, child, started: crate::now_ms() })
}

fn read_events(agent: Agent, stdout: impl Read, tx: Sender<Event>, child: Arc<Mutex<Option<Child>>>, errors: Arc<Mutex<String>>, ctx: egui::Context) {
    let (mut finished, mut explained) = (false, false);
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        finished |= is_turn_end(agent, &v);
        for ev in parse(agent, &v) {
            explained |= matches!(ev, Event::Error(_));
            let _ = tx.send(ev);
        }
        ctx.request_repaint();
    }
    let status = child.lock().ok().and_then(|mut c| c.take()).and_then(|mut c| c.wait().ok());
    let ok = finished && status.is_some_and(|s| s.success());
    if !finished && !explained {
        // Give the reader of stderr a moment to finish; its tail explains the failure.
        std::thread::sleep(std::time::Duration::from_millis(100));
        let err = errors.lock().map(|e| e.trim().to_string()).unwrap_or_default();
        let tail: String = err.chars().rev().take(400).collect::<Vec<_>>().into_iter().rev().collect();
        let why = if tail.is_empty() { status.map(|s| s.to_string()).unwrap_or_default() } else { tail };
        let _ = tx.send(Event::Error(why));
    }
    let _ = tx.send(Event::Done { ok });
    ctx.request_repaint();
}

/// Whether this output line ends the agent's turn (it finished rather than being cut off).
fn is_turn_end(agent: Agent, v: &Value) -> bool {
    match agent {
        Agent::Claude => v["type"] == "result" && v["is_error"] != true,
        Agent::Codex => v["type"] == "turn.completed",
    }
}

/// One line of the agent's JSON output as pane events.
pub fn parse(agent: Agent, v: &Value) -> Vec<Event> {
    let s = |v: &Value| v.as_str().map(str::to_string);
    let tool = |name: &str, input: &Value| {
        let name = name.trim_start_matches("mcp__word__").to_string();
        let command = input["command"].as_str().map(str::to_string);
        Event::Tool(name, command)
    };
    let mut out = Vec::new();
    match agent {
        // `claude -p --output-format stream-json`: system/init, assistant messages, result.
        Agent::Claude => match v["type"].as_str() {
            Some("system") => out.extend(s(&v["session_id"]).map(Event::Session)),
            Some("assistant") => {
                for c in v["message"]["content"].as_array().into_iter().flatten() {
                    match c["type"].as_str() {
                        Some("text") => out.extend(s(&c["text"]).filter(|t| !t.trim().is_empty()).map(Event::Text)),
                        Some("tool_use") => out.extend(c["name"].as_str().map(|n| tool(n, &c["input"]))),
                        _ => {}
                    }
                }
            }
            Some("result") => {
                out.extend(s(&v["session_id"]).map(Event::Session));
                if v["is_error"].as_bool() == Some(true) {
                    out.push(Event::Error(s(&v["result"]).unwrap_or_else(|| "error".into())));
                }
            }
            _ => {}
        },
        // `codex exec --json`: thread and item events.
        Agent::Codex => match v["type"].as_str() {
            Some("thread.started") => out.extend(s(&v["thread_id"]).map(Event::Session)),
            Some("item.completed") => match v["item"]["type"].as_str() {
                Some("agent_message") => out.extend(s(&v["item"]["text"]).map(Event::Text)),
                Some("mcp_tool_call") => out.extend(v["item"]["tool"].as_str().map(|n| tool(n, &v["item"]["arguments"]))),
                _ => {}
            },
            Some("error") => out.extend(s(&v["message"]).map(Event::Error)),
            Some("turn.failed") => out.extend(s(&v["error"]["message"]).map(Event::Error)),
            _ => {}
        },
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(c: &Command) -> Vec<String> {
        c.get_args().map(|a| a.to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn claude_stream_json_becomes_pane_events() {
        let init = json!({"type": "system", "subtype": "init", "session_id": "abc"});
        assert_eq!(parse(Agent::Claude, &init), vec![Event::Session("abc".into())]);
        let msg = json!({"type": "assistant", "message": {"content": [
            {"type": "text", "text": "Hecho."},
            {"type": "tool_use", "name": "mcp__word__execute", "input": {"command": "insert.coverPage"}}
        ]}});
        assert_eq!(parse(Agent::Claude, &msg), vec![Event::Text("Hecho.".into()), Event::Tool("execute".into(), Some("insert.coverPage".into()))]);
        let done = json!({"type": "result", "is_error": false, "session_id": "abc"});
        assert_eq!(parse(Agent::Claude, &done), vec![Event::Session("abc".into())]);
        assert!(is_turn_end(Agent::Claude, &done));
        let failed = json!({"type": "result", "is_error": true, "result": "boom"});
        assert_eq!(parse(Agent::Claude, &failed), vec![Event::Error("boom".into())]);
        assert!(!is_turn_end(Agent::Claude, &failed));
    }

    #[test]
    fn codex_json_becomes_pane_events() {
        let start = json!({"type": "thread.started", "thread_id": "t1"});
        assert_eq!(parse(Agent::Codex, &start), vec![Event::Session("t1".into())]);
        let msg = json!({"type": "item.completed", "item": {"type": "agent_message", "text": "Listo"}});
        assert_eq!(parse(Agent::Codex, &msg), vec![Event::Text("Listo".into())]);
        let call = json!({"type": "item.completed", "item": {"type": "mcp_tool_call", "server": "word", "tool": "type_text"}});
        assert_eq!(parse(Agent::Codex, &call), vec![Event::Tool("type_text".into(), None)]);
        assert!(is_turn_end(Agent::Codex, &json!({"type": "turn.completed"})));
    }

    #[test]
    fn claude_runs_lean_with_only_the_document_tools() {
        let job = Job { agent: Agent::Claude, model: Model::Auto, port: 4242, session: Some("s1".into()), dirs: vec![PathBuf::from("/home/u/Docs")] };
        let a = args(&command(&job, "claude".into(), "/usr/bin/word-cli"));
        let at = a.iter().position(|x| x == "--mcp-config").unwrap();
        let cfg: Value = serde_json::from_str(&a[at + 1]).unwrap();
        assert_eq!(cfg["mcpServers"]["word"]["args"], json!(["mcp", "--connect", "127.0.0.1:4242"]));
        assert!(a.windows(2).any(|w| w == ["--resume", "s1"]));
        assert!(a.windows(2).any(|w| w == ["--model", "sonnet"]), "Auto is Sonnet");
        assert!(a.windows(2).any(|w| w == ["--add-dir", "/home/u/Docs"]));
        assert!(a.windows(2).any(|w| w == ["--tools", "Read"]), "attachments: Read only");
        assert!(a.windows(2).any(|w| w[0] == "--system-prompt" && w[1] == INSTRUCTIONS), "its own short system prompt");
        assert!(a.contains(&"mcp__word__screenshot".to_string()), "screenshots stay out");
        // --allowedTools comes last, so its values end the command line.
        let tools = a.iter().position(|x| x == "--allowedTools").unwrap();
        assert_eq!(a[tools + 1..].len(), TOOLS.len() + 1);
        assert_eq!(a.last().map(String::as_str), Some("Read"));

        let job = Job { agent: Agent::Claude, model: Model::Opus, port: 1, session: None, dirs: Vec::new() };
        let a = args(&command(&job, "claude".into(), "word-cli"));
        assert!(a.windows(2).any(|w| w == ["--tools", ""]), "no built-in tools without attachments");
        assert!(a.windows(2).any(|w| w == ["--model", "opus"]));
        assert!(!a.contains(&"--resume".to_string()));
    }

    #[test]
    fn codex_resumes_its_thread_and_gets_the_instructions_once() {
        let mut job = Job { agent: Agent::Codex, model: Model::Auto, port: 4242, session: None, dirs: Vec::new() };
        let a = args(&command(&job, "codex".into(), "/usr/bin/word-cli"));
        assert!(a.contains(&"mcp_servers.word.args=[\"mcp\", \"--connect\", \"127.0.0.1:4242\"]".to_string()), "{a:?}");
        assert!(stdin_text(&job, "hola").starts_with(INSTRUCTIONS));
        job.session = Some("t1".into());
        let a = args(&command(&job, "codex".into(), "/usr/bin/word-cli"));
        assert!(a.windows(2).any(|w| w == ["resume", "t1"]));
        assert_eq!(a.last().map(String::as_str), Some("-"));
        assert_eq!(stdin_text(&job, "hola"), "hola");
    }
}
