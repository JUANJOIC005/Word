//! AI Assistant pane (Word fork): a chat with Claude Code or Codex that edit the open document
//! live. The app starts its control server on demand and runs the agent's command-line tool with
//! one MCP server, `word-cli mcp --connect 127.0.0.1:<port>`, which drives this window.
//!
//! The pane creates documents from scratch (a template plus a description), sends attached files
//! (the agent reads them; pictures can be inserted), runs one-click actions (summarize, proofread,
//! format, translate…) and limits the work to the selection or tracks every change on request.
//!
//! The agent runs as the user's own `claude` or `codex` install (their login, their plan); the
//! pane only shows its replies and the tools it calls. Kept out of upstream files so syncing with
//! WordCraft stays easy: lib.rs, ribbon.rs, keytips.rs and the app's main.rs only hook it in.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex};

use egui::{RichText, Stroke, Ui};
use serde_json::{Value, json};

use crate::theme::{Tokens, regular, semibold};
use crate::{ControlRequest, FileDialogAnswer, FileDialogRequest, WordApp};

/// Starts the control server when the pane first needs it: its port, the request channel and a
/// guard that removes the key file when dropped. Set by the desktop app (`main.rs`).
pub type ControlStarter = Box<dyn FnMut(&egui::Context) -> Option<(u16, Receiver<ControlRequest>, Box<dyn std::any::Any + Send>)> + Send>;

/// What the agent is told besides the user's message.
const INSTRUCTIONS: &str = "You are the AI assistant built into the Word word processor. The document the user has open \
is edited live through the `word` MCP tools: read it with get_text or inspect_document, change it with execute (any \
command; list_commands finds them), batch, select_text and type_text. Use real styles (Heading 1, Heading 2, Title, \
lists, tables) rather than manual formatting. Work on the document only through those tools; read attached files \
when the user lists them. Do not create, open, save or close documents unless the user asks. Keep replies short and \
answer in the user's language.";

/// File › New templates offered by Create with AI: (`file.new` template, label).
const TEMPLATES: &[(&str, &str)] = &[("blank", "Blank document"), ("letter", "Letter"), ("resume", "Résumé"), ("report", "Report")];

/// Claude models: (`--model` alias, label); empty is Claude Code's own default.
const CLAUDE_MODELS: &[(&str, &str)] = &[("", "Default"), ("opus", "Opus"), ("sonnet", "Sonnet"), ("haiku", "Haiku")];

/// One-click actions: (label, request).
const ACTIONS: &[(&str, &str)] = &[
    ("Summarize", "Write a short summary of the document and insert it at the beginning under a heading, in the document's language."),
    (
        "Proofread",
        "Proofread the document: fix spelling, grammar and punctuation and improve clarity without changing the meaning or the formatting.",
    ),
    (
        "Improve formatting",
        "Improve the formatting: consistent heading styles, paragraph spacing, and lists or tables where they help. Do not change the wording.",
    ),
    ("Table of contents", "Insert a table of contents at the beginning (references.toc), making sure the headings use heading styles."),
    ("Continue writing", "Continue writing from the end of the document, matching its tone and style, for about three paragraphs."),
    ("Translate to English", "Translate the document into English, keeping its formatting."),
    ("Translate to Spanish", "Translate the document into Spanish, keeping its formatting."),
];

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Agent {
    #[default]
    Claude,
    Codex,
}

impl Agent {
    fn program(self) -> &'static str {
        match self {
            Agent::Claude => "claude",
            Agent::Codex => "codex",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Agent::Claude => "Claude",
            Agent::Codex => "Codex",
        }
    }
}

enum Line {
    User(String),
    Agent(String),
    Tool(String),
    Info(String),
    Error(String),
}

#[derive(Debug, PartialEq)]
enum Event {
    Text(String),
    Tool(String),
    Session(String),
    Stats(String),
    Error(String),
    Done,
}

struct Running {
    rx: Receiver<Event>,
    child: Arc<Mutex<Option<Child>>>,
    started: f64,
}

impl Running {
    fn stop(&self) {
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

#[derive(Default)]
pub struct AiPane {
    pub open: bool,
    pub agent: Agent,
    /// The control server's port: from `--control`, or once [`Self::starter`] ran.
    pub port: Option<u16>,
    pub starter: Option<ControlStarter>,
    /// Held for its `Drop` (removes the control key file).
    #[allow(dead_code)]
    guard: Option<Box<dyn std::any::Any + Send>>,
    /// `--model` for Claude (empty: its default).
    model: String,
    /// File › New template for Create with AI.
    template: usize,
    track_changes: bool,
    selection_only: bool,
    attachments: Vec<PathBuf>,
    picking: Option<Receiver<FileDialogAnswer>>,
    input: String,
    log: Vec<Line>,
    /// Claude's session, resumed by the next message so the conversation continues.
    session: Option<String>,
    running: Option<Running>,
}

pub fn show(app: &mut WordApp, ui: &mut Ui) {
    poll(app);
    if !app.ai.open {
        return;
    }
    let t = Tokens::get(ui.ctx());
    egui::Panel::right("ai_pane")
        .default_size(360.0)
        .min_size(280.0)
        .resizable(true)
        .frame(egui::Frame::NONE.fill(t.panel).inner_margin(10).stroke(Stroke::new(1.0, t.border)))
        .show(ui, |ui| {
            if crate::panes::header(ui, "AI Assistant") {
                app.ai.open = false;
                return;
            }
            body(app, ui, &t);
        });
    if app.ai.running.is_some() || app.ai.picking.is_some() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
    }
}

fn body(app: &mut WordApp, ui: &mut Ui, t: &Tokens) {
    let busy = app.ai.running.is_some();
    settings(app, ui, busy);
    ui.separator();
    // Bottom up: the composer keeps its place and the conversation takes the rest.
    egui::Panel::bottom("ai_composer").frame(egui::Frame::NONE).resizable(false).show(ui, |ui| {
        ui.add_space(6.0);
        composer(app, ui, busy);
    });
    conversation(app, ui, t, busy);
}

/// Agent, model, scope, track changes, and Create with AI.
fn settings(app: &mut WordApp, ui: &mut Ui, busy: bool) {
    ui.add_enabled_ui(!busy, |ui| {
        ui.horizontal(|ui| {
            let before = app.ai.agent;
            for agent in [Agent::Claude, Agent::Codex] {
                ui.selectable_value(&mut app.ai.agent, agent, agent.name());
            }
            if app.ai.agent != before {
                app.ai.session = None;
            }
            if app.ai.agent == Agent::Claude {
                let current = CLAUDE_MODELS.iter().find(|m| m.0 == app.ai.model).map_or("Default", |m| m.1);
                egui::ComboBox::from_id_salt("ai_model").selected_text(tl!(current)).show_ui(ui, |ui| {
                    for (alias, label) in CLAUDE_MODELS {
                        ui.selectable_value(&mut app.ai.model, (*alias).to_string(), tl!(label));
                    }
                });
            }
        });
        egui::CollapsingHeader::new(tl!("Options")).id_salt("ai_options").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.radio_value(&mut app.ai.selection_only, false, tl!("Whole document"));
                ui.radio_value(&mut app.ai.selection_only, true, tl!("Selection only"));
            });
            ui.checkbox(&mut app.ai.track_changes, tl!("Track the agent's changes (review them in Review)"));
        });
        egui::CollapsingHeader::new(tl!("Create a document")).id_salt("ai_create").show(ui, |ui| {
            ui.label(RichText::new(tl!("Describe it in the message box, pick a starting point and create it.")).font(regular(12.0)).weak());
            ui.horizontal(|ui| {
                let (_, label) = TEMPLATES[app.ai.template.min(TEMPLATES.len() - 1)];
                egui::ComboBox::from_id_salt("ai_template").selected_text(tl!(label)).show_ui(ui, |ui| {
                    for (i, (_, label)) in TEMPLATES.iter().enumerate() {
                        ui.selectable_value(&mut app.ai.template, i, tl!(label));
                    }
                });
                if ui.add_enabled(!app.ai.input.trim().is_empty(), egui::Button::new(tl!("Create with AI"))).clicked() {
                    create_document(app, ui.ctx());
                }
            });
        });
        egui::CollapsingHeader::new(tl!("Quick actions")).id_salt("ai_actions").default_open(true).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for (label, request) in ACTIONS {
                    if ui.small_button(tl!(label)).clicked() {
                        send(app, ui.ctx(), request, tl!(label));
                    }
                }
            });
        });
    });
}

fn conversation(app: &mut WordApp, ui: &mut Ui, t: &Tokens, busy: bool) {
    let mut insert: Option<String> = None;
    let agent = app.ai.agent.name();
    egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
        if app.ai.log.is_empty() {
            ui.label(RichText::new(tl!("Ask Claude or Codex to write or edit the open document. Changes appear as they work.")).color(t.text));
        }
        for line in &app.ai.log {
            match line {
                Line::User(s) => {
                    ui.label(RichText::new(tl!("You")).font(semibold(13.0)));
                    ui.add(egui::Label::new(RichText::new(s).font(regular(13.0))).selectable(true));
                }
                Line::Agent(s) => {
                    ui.label(RichText::new(agent).font(semibold(13.0)).color(t.accent));
                    ui.add(egui::Label::new(RichText::new(s).font(regular(13.0))).selectable(true));
                    ui.horizontal(|ui| {
                        if ui.small_button(tl!("Copy")).clicked() {
                            ui.ctx().copy_text(s.clone());
                        }
                        if ui.small_button(tl!("Insert in document")).clicked() {
                            insert = Some(s.clone());
                        }
                    });
                }
                Line::Tool(s) => {
                    ui.label(RichText::new(format!("> {s}")).font(regular(12.0)).weak());
                }
                Line::Info(s) => {
                    ui.label(RichText::new(s).font(regular(11.0)).weak());
                }
                Line::Error(s) => {
                    ui.label(RichText::new(s).font(regular(12.0)).color(egui::Color32::from_rgb(0xD1, 0x3B, 0x3B)));
                }
            }
            ui.add_space(4.0);
        }
        if let Some(r) = &app.ai.running {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(format!("{} {:.0} s", tl!("Working…"), (crate::now_ms() - r.started) / 1000.0));
            });
        }
    });
    if let Some(text) = insert
        && !busy
    {
        insert_text(app, &text);
    }
}

/// Attachments, the message box and Send / Stop / New conversation.
fn composer(app: &mut WordApp, ui: &mut Ui, busy: bool) {
    if !app.ai.attachments.is_empty() {
        let mut remove = None;
        ui.horizontal_wrapped(|ui| {
            for (i, p) in app.ai.attachments.iter().enumerate() {
                let name = p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned());
                ui.label(RichText::new(name).font(regular(12.0))).on_hover_text(p.display().to_string());
                if crate::widgets::icon_button(ui, "close", tl!("Remove")).clicked() {
                    remove = Some(i);
                }
            }
        });
        if let Some(i) = remove {
            app.ai.attachments.remove(i);
        }
    }
    let edit = ui.add_enabled(
        !busy,
        egui::TextEdit::multiline(&mut app.ai.input).desired_rows(4).desired_width(f32::INFINITY).hint_text(tl!("Message (Ctrl+Enter to send)")),
    );
    let ctrl_enter = edit.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.command);
    let mut send_now = false;
    ui.horizontal(|ui| {
        if busy {
            if ui.button(tl!("Stop")).clicked()
                && let Some(r) = &app.ai.running
            {
                r.stop();
            }
        } else if ui.add_enabled(!app.ai.input.trim().is_empty(), egui::Button::new(tl!("Send"))).clicked() {
            send_now = true;
        }
        if ui.add_enabled(!busy && app.ai.picking.is_none(), egui::Button::new(tl!("Attach files…"))).clicked() {
            pick_attachments(app);
        }
        if ui.add_enabled(!busy && !app.ai.log.is_empty(), egui::Button::new(tl!("New conversation"))).clicked() {
            app.ai.log.clear();
            app.ai.session = None;
        }
    });
    if (send_now || ctrl_enter) && !busy {
        // The agent's key presses go through egui: leave the keyboard to the document.
        edit.surrender_focus();
        let text = app.ai.input.trim().to_string();
        app.ai.input.clear();
        send(app, ui.ctx(), &text, &text);
    }
}

fn poll(app: &mut WordApp) {
    if let Some(rx) = &app.ai.picking {
        match rx.try_recv() {
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => app.ai.picking = None,
            Ok(answer) => {
                app.ai.picking = None;
                match answer {
                    Ok(paths) => add_attachments(app, paths),
                    Err(e) => app.ai.log.push(Line::Error(format!("{} {e}", tl!("No file dialog could be shown:")))),
                }
            }
        }
    }
    let Some(r) = &app.ai.running else { return };
    let events: Vec<Event> = r.rx.try_iter().collect();
    for ev in events {
        match ev {
            Event::Text(s) => app.ai.log.push(Line::Agent(s)),
            Event::Tool(s) => app.ai.log.push(Line::Tool(s)),
            Event::Session(s) => app.ai.session = Some(s),
            Event::Stats(s) => app.ai.log.push(Line::Info(s)),
            Event::Error(s) => app.ai.log.push(Line::Error(s)),
            Event::Done => app.ai.running = None,
        }
    }
}

fn pick_attachments(app: &mut WordApp) {
    let req = FileDialogRequest::OpenMany { purpose: "attachments".into() };
    if let Some(show) = &app.services.file_dialog {
        app.ai.picking = Some(show(req));
    } else if let Some(pick) = &app.services.pick_open {
        let picked: Vec<String> = pick("attachments").into_iter().collect();
        add_attachments(app, picked);
    } else {
        app.ai.log.push(Line::Error(tl!("This version has no file dialog.").to_string()));
    }
}

fn add_attachments(app: &mut WordApp, paths: Vec<String>) {
    for p in paths.into_iter().map(PathBuf::from) {
        if !app.ai.attachments.contains(&p) {
            app.ai.attachments.push(p);
        }
    }
}

/// File › New from the chosen template, then the agent writes the description into it.
fn create_document(app: &mut WordApp, ctx: &egui::Context) {
    let description = app.ai.input.trim().to_string();
    let (template, _) = TEMPLATES[app.ai.template.min(TEMPLATES.len() - 1)];
    match app.run("file.new", json!({"template": template})) {
        // "Save changes?" is up: the user answers it, then presses Create again.
        Ok(v) if v.get("pending").is_some() || v.get("done") == Some(&Value::Bool(false)) => {
            app.ai.log.push(Line::Info(tl!("Save or discard the current document's changes, then press Create with AI again.").to_string()));
        }
        Err(e) => app.ai.log.push(Line::Error(e)),
        Ok(_) => {
            app.ai.input.clear();
            let request = format!(
                "A new document from the \"{template}\" template is open now. Write this document in it from scratch, \
                 replacing any template placeholder text, with a title, headings, lists and tables where useful: {description}"
            );
            send(app, ctx, &request, &description);
        }
    }
}

/// Text at the caret, one paragraph per line (an agent reply put into the document by hand).
fn insert_text(app: &mut WordApp, text: &str) {
    for (i, line) in text.lines().enumerate() {
        if i > 0 && app.run("text.newParagraph", json!({})).is_err() {
            return;
        }
        if !line.is_empty() && app.run("text.insert", json!({"text": line, "raw": true})).is_err() {
            return;
        }
    }
}

/// The request with the pane's options: scope, track changes, attached files.
fn build_prompt(app: &WordApp, request: &str) -> String {
    let mut p = request.to_string();
    if app.ai.selection_only {
        let sel = app.session.selected_text();
        if !sel.trim().is_empty() {
            p.push_str(&format!(
                "\n\nWork only on the current selection and leave the rest of the document unchanged. The selected text is:\n\"\"\"\n{sel}\n\"\"\""
            ));
        }
    }
    if app.ai.track_changes {
        p.push_str(
            "\n\nBefore editing, turn on Track Changes with execute review.trackChanges {\"value\": true} so the user can review every change.",
        );
    }
    if !app.ai.attachments.is_empty() {
        p.push_str("\n\nAttached files (read them for their content; insert a picture with execute insert.picture {\"path\": …}):");
        for a in &app.ai.attachments {
            p.push_str(&format!("\n- {}", a.display()));
        }
    }
    p
}

/// Run the agent on `request` (`shown` is what the conversation shows for it).
fn send(app: &mut WordApp, ctx: &egui::Context, request: &str, shown: &str) {
    if request.trim().is_empty() || app.ai.running.is_some() {
        return;
    }
    let Some(port) = ensure_control(app, ctx) else {
        app.ai.log.push(Line::Error(tl!("The control channel could not start, so the agent cannot reach the document.").to_string()));
        return;
    };
    let prompt = build_prompt(app, request);
    let mut shown = shown.to_string();
    if !app.ai.attachments.is_empty() {
        let names: Vec<String> = app.ai.attachments.iter().filter_map(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).collect();
        shown.push_str(&format!("\n[{}]", names.join(", ")));
    }
    app.ai.log.push(Line::User(shown));
    let job = Job {
        agent: app.ai.agent,
        model: app.ai.model.clone(),
        port,
        session: app.ai.session.clone(),
        dirs: app.ai.attachments.iter().filter_map(|p| p.parent().map(PathBuf::from)).collect(),
    };
    match spawn(&job, &prompt, ctx.clone()) {
        Ok(r) => app.ai.running = Some(r),
        Err(e) => app.ai.log.push(Line::Error(e)),
    }
}

/// The control server's port, starting it the first time when the app runs without `--control`.
fn ensure_control(app: &mut WordApp, ctx: &egui::Context) -> Option<u16> {
    if let Some(p) = app.ai.port {
        return Some(p);
    }
    let start = app.ai.starter.as_mut()?;
    let (port, rx, guard) = start(ctx)?;
    if app.control_rx.is_none() {
        app.control_rx = Some(rx);
    }
    app.ai.port = Some(port);
    app.ai.guard = Some(guard);
    Some(port)
}

/// `name` on `PATH`, else in the usual per-user install folders (a desktop launcher's `PATH` often
/// lacks `~/.local/bin`, where Claude Code installs itself).
fn find_program(name: &str) -> Option<PathBuf> {
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

/// One agent run.
struct Job {
    agent: Agent,
    model: String,
    port: u16,
    session: Option<String>,
    /// Folders of the attached files, which the agent may read.
    dirs: Vec<PathBuf>,
}

fn command(job: &Job, program: PathBuf, cli: &str) -> Command {
    let connect = format!("127.0.0.1:{}", job.port);
    let mut c = Command::new(program);
    match job.agent {
        Agent::Claude => {
            let mcp = json!({"mcpServers": {"word": {"command": cli, "args": ["mcp", "--connect", connect]}}}).to_string();
            // The prompt goes on standard input: --mcp-config, --add-dir and --allowedTools take
            // several values.
            c.args(["-p", "--output-format", "stream-json", "--verbose", "--strict-mcp-config"]);
            c.args(["--append-system-prompt", INSTRUCTIONS, "--mcp-config", mcp.as_str()]);
            if !job.model.is_empty() {
                c.args(["--model", job.model.as_str()]);
            }
            if let Some(s) = &job.session {
                c.args(["--resume", s.as_str()]);
            }
            if !job.dirs.is_empty() {
                c.arg("--add-dir").args(&job.dirs);
            }
            c.args(["--allowedTools", "mcp__word"]);
            if !job.dirs.is_empty() {
                c.args(["Read", "Glob", "Grep"]);
            }
        }
        Agent::Codex => {
            let toml_str = |s: &str| Value::String(s.to_string()).to_string();
            c.args(["exec", "--json", "--skip-git-repo-check"]);
            c.args(["-c".to_string(), format!("mcp_servers.word.command={}", toml_str(cli))]);
            c.args(["-c".to_string(), format!("mcp_servers.word.args=[\"mcp\", \"--connect\", {}]", toml_str(&connect))]);
            c.arg("-");
        }
    }
    c
}

fn spawn(job: &Job, prompt: &str, ctx: egui::Context) -> Result<Running, String> {
    let agent = job.agent;
    let Some(program) = find_program(agent.program()) else {
        return Err(match agent {
            Agent::Claude => tl!("Claude Code (`claude`) is not installed or not on PATH.").to_string(),
            Agent::Codex => tl!("Codex (`codex`) is not installed or not on PATH.").to_string(),
        });
    };
    let cli = cli_path();
    let mut cmd = command(job, program, &cli.to_string_lossy());
    // Away from the user's files: the agent reaches the document through the MCP bridge and
    // only the attached files' folders besides.
    cmd.current_dir(std::env::temp_dir()).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut cmd, 0x0800_0000); // CREATE_NO_WINDOW
    let mut child = cmd.spawn().map_err(|e| format!("{}: {e}", agent.program()))?;
    let stdin_text = match agent {
        Agent::Claude => prompt.to_string(),
        Agent::Codex => format!("{INSTRUCTIONS}\n\n{prompt}"),
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(stdin_text.as_bytes());
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
    let mut said_something = false;
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        for ev in parse(agent, &v) {
            said_something |= matches!(ev, Event::Text(_) | Event::Error(_));
            let _ = tx.send(ev);
        }
        ctx.request_repaint();
    }
    let status = child.lock().ok().and_then(|mut c| c.take()).and_then(|mut c| c.wait().ok());
    if let Some(status) = status
        && !status.success()
        && !said_something
    {
        // Give the reader of stderr a moment to finish.
        std::thread::sleep(std::time::Duration::from_millis(100));
        let err = errors.lock().map(|e| e.trim().to_string()).unwrap_or_default();
        let tail: String = err.chars().rev().take(600).collect::<Vec<_>>().into_iter().rev().collect();
        let _ = tx.send(Event::Error(if tail.is_empty() { format!("{} exited with {status}", agent.program()) } else { tail }));
    }
    let _ = tx.send(Event::Done);
    ctx.request_repaint();
}

/// One line of the agent's JSON output as pane events.
fn parse(agent: Agent, v: &Value) -> Vec<Event> {
    let s = |v: &Value| v.as_str().map(str::to_string);
    let tool = |name: &str| name.trim_start_matches("mcp__word__").to_string();
    let mut out = Vec::new();
    match agent {
        // `claude -p --output-format stream-json`: system/init, assistant messages, result.
        Agent::Claude => match v["type"].as_str() {
            Some("system") => out.extend(s(&v["session_id"]).map(Event::Session)),
            Some("assistant") => {
                for c in v["message"]["content"].as_array().into_iter().flatten() {
                    match c["type"].as_str() {
                        Some("text") => out.extend(s(&c["text"]).filter(|t| !t.trim().is_empty()).map(Event::Text)),
                        Some("tool_use") => out.extend(c["name"].as_str().map(|n| Event::Tool(tool(n)))),
                        _ => {}
                    }
                }
            }
            Some("result") => {
                out.extend(s(&v["session_id"]).map(Event::Session));
                if v["is_error"].as_bool() == Some(true) {
                    out.push(Event::Error(s(&v["result"]).unwrap_or_else(|| "error".into())));
                }
                let secs = v["duration_ms"].as_f64().map(|ms| format!("{:.1} s", ms / 1000.0));
                let cost = v["total_cost_usd"].as_f64().map(|c| format!("${c:.3}"));
                let stats: Vec<String> = secs.into_iter().chain(cost).collect();
                if !stats.is_empty() {
                    out.push(Event::Stats(stats.join(" · ")));
                }
            }
            _ => {}
        },
        // `codex exec --json`: thread and item events.
        Agent::Codex => match v["type"].as_str() {
            Some("item.completed") => match v["item"]["type"].as_str() {
                Some("agent_message") => out.extend(s(&v["item"]["text"]).map(Event::Text)),
                Some("mcp_tool_call") => out.extend(v["item"]["tool"].as_str().map(|n| Event::Tool(tool(n)))),
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
            {"type": "tool_use", "name": "mcp__word__execute", "input": {}}
        ]}});
        assert_eq!(parse(Agent::Claude, &msg), vec![Event::Text("Hecho.".into()), Event::Tool("execute".into())]);
        let done = json!({"type": "result", "is_error": false, "session_id": "abc", "duration_ms": 1500, "total_cost_usd": 0.0123});
        assert_eq!(parse(Agent::Claude, &done), vec![Event::Session("abc".into()), Event::Stats("1.5 s · $0.012".into())]);
        let failed = json!({"type": "result", "is_error": true, "result": "boom"});
        assert_eq!(parse(Agent::Claude, &failed), vec![Event::Error("boom".into())]);
    }

    #[test]
    fn codex_json_becomes_pane_events() {
        let msg = json!({"type": "item.completed", "item": {"type": "agent_message", "text": "Listo"}});
        assert_eq!(parse(Agent::Codex, &msg), vec![Event::Text("Listo".into())]);
        let call = json!({"type": "item.completed", "item": {"type": "mcp_tool_call", "server": "word", "tool": "type_text"}});
        assert_eq!(parse(Agent::Codex, &call), vec![Event::Tool("type_text".into())]);
    }

    #[test]
    fn the_agent_gets_one_mcp_server_that_connects_to_this_window() {
        let job =
            Job { agent: Agent::Claude, model: "sonnet".into(), port: 4242, session: Some("s1".into()), dirs: vec![PathBuf::from("/home/u/Docs")] };
        let a = args(&command(&job, "claude".into(), "/usr/bin/word-cli"));
        let at = a.iter().position(|x| x == "--mcp-config").unwrap();
        let cfg: Value = serde_json::from_str(&a[at + 1]).unwrap();
        assert_eq!(cfg["mcpServers"]["word"]["args"], json!(["mcp", "--connect", "127.0.0.1:4242"]));
        assert!(a.windows(2).any(|w| w == ["--resume", "s1"]));
        assert!(a.windows(2).any(|w| w == ["--model", "sonnet"]));
        assert!(a.windows(2).any(|w| w == ["--add-dir", "/home/u/Docs"]));
        // --allowedTools comes last, so its values end the command line.
        let tools = a.iter().position(|x| x == "--allowedTools").unwrap();
        assert_eq!(&a[tools + 1..], ["mcp__word", "Read", "Glob", "Grep"]);

        let job = Job { agent: Agent::Codex, model: String::new(), port: 4242, session: None, dirs: Vec::new() };
        let a = args(&command(&job, "codex".into(), "/usr/bin/word-cli"));
        assert!(a.contains(&"mcp_servers.word.args=[\"mcp\", \"--connect\", \"127.0.0.1:4242\"]".to_string()), "{a:?}");
    }
}
