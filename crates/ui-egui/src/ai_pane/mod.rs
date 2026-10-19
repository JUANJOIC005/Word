//! AI (Word fork): a chat with Claude Code or Codex that edit the open document live, and the
//! ribbon's AI tab that chooses the agent and opens Document Design.
//!
//! The app starts its control server on demand and runs the agent's command-line tool with one
//! MCP server, `word-cli mcp --connect 127.0.0.1:<port>`, which drives this window. The agent runs
//! as the user's own `claude` or `codex` install (their login, their plan).
//!
//! The chat shows what the user needs: their messages, the agent's answers and, while it works, one
//! live line saying what it is doing in plain words — never the tools it calls. Each document keeps
//! its conversation and the agent's session (`store`), so the next message continues it and a task
//! that was stopped, failed or cut short by closing the app can be picked up where it stopped.
//!
//! Kept out of upstream files so syncing with WordCraft stays easy: lib.rs, ribbon.rs, icons.rs and
//! the app's main.rs only hook it in.

mod agent;
mod chat;
mod markdown;
mod store;
mod studio;

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError};

use serde_json::{Value, json};

pub use agent::{Agent, Model};
use store::{Conversation, Msg};

use crate::{ControlRequest, FileDialogAnswer, FileDialogRequest, WordApp};

/// Starts the control server when the pane first needs it: its port, the request channel and a
/// guard that removes the key file when dropped. Set by the desktop app (`main.rs`).
pub type ControlStarter = Box<dyn FnMut(&egui::Context) -> Option<(u16, Receiver<ControlRequest>, Box<dyn std::any::Any + Send>)> + Send>;

/// The fork's own text in English or Spanish. It stays out of the language catalogs on purpose:
/// they must be complete in every language, and this is the fork's, not upstream's.
pub(crate) fn tx(en: &'static str, es: &'static str) -> &'static str {
    note(if crate::i18n::current().code() == "es" { es } else { en })
}

/// [`tx`] with `{name}` filled in.
pub(crate) fn txf(en: &'static str, es: &'static str, name: &str) -> String {
    noted(tx(en, es).replace("{name}", name))
}

#[cfg(test)]
thread_local! {
    /// Every text the fork drew on this thread, so the untranslated-text audit (`i18n::audit`)
    /// knows these are its own and not missing `tl!`.
    static DRAWN: std::cell::RefCell<std::collections::HashSet<String>> = Default::default();
}

/// `s`, remembered as the fork's own text in tests.
fn note(s: &'static str) -> &'static str {
    #[cfg(test)]
    DRAWN.with_borrow_mut(|d| {
        d.insert(s.to_string());
    });
    s
}

/// A composed text, remembered as the fork's own in tests.
pub(crate) fn noted(s: String) -> String {
    #[cfg(test)]
    DRAWN.with_borrow_mut(|d| {
        d.insert(s.clone());
    });
    s
}

/// Whether the fork drew `text` (the audit exempts it from `tl!`).
#[cfg(test)]
pub(crate) fn drew(text: &str) -> bool {
    DRAWN.with_borrow(|d| d.contains(text))
}

/// What the agent is doing, in the user's words.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Thinking,
    Reading,
    Files,
    Writing,
    Formatting,
    Cover,
    Contents,
}

impl Phase {
    /// The phase a tool call starts (`command` is `execute`'s command id).
    fn of(tool: &str, command: Option<&str>) -> Phase {
        match (tool, command.unwrap_or("")) {
            ("get_text" | "inspect_document" | "list_commands", _) => Phase::Reading,
            ("Read" | "Glob" | "Grep", _) => Phase::Files,
            (_, "insert.coverPage") => Phase::Cover,
            (_, c) if c.starts_with("references.") => Phase::Contents,
            (_, c) if ["design.", "styles.", "para.", "format.", "table.", "page.", "layout."].iter().any(|p| c.starts_with(p)) => Phase::Formatting,
            ("batch", _) => Phase::Formatting,
            _ => Phase::Writing,
        }
    }

    /// Whether the call changes the document.
    fn edits(self) -> bool {
        !matches!(self, Phase::Thinking | Phase::Reading | Phase::Files)
    }

    pub fn text(self) -> &'static str {
        match self {
            Phase::Thinking => tx("Thinking", "Pensando"),
            Phase::Reading => tx("Reading the document", "Leyendo el documento"),
            Phase::Files => tx("Reading your files", "Leyendo tus archivos"),
            Phase::Writing => tx("Writing", "Escribiendo"),
            Phase::Formatting => tx("Formatting", "Dando formato"),
            Phase::Cover => tx("Designing the cover", "Diseñando la portada"),
            Phase::Contents => tx("Building the contents", "Creando el índice"),
        }
    }
}

/// The turn the agent is working on.
pub struct Turn {
    pub phase: Phase,
    pub edits: u32,
    /// The agent's latest words: only the last ones of a turn are shown.
    reply: Option<String>,
    error: Option<String>,
    stopped: bool,
}

#[derive(Default)]
pub struct AiPane {
    pub open: bool,
    /// The control server's port: from `--control`, or once [`Self::starter`] ran.
    pub port: Option<u16>,
    pub starter: Option<ControlStarter>,
    /// Where conversations are kept (`<settings>/ai`); none keeps them for this run only.
    pub store_dir: Option<PathBuf>,
    /// Held for its `Drop` (removes the control key file).
    #[allow(dead_code)]
    guard: Option<Box<dyn std::any::Any + Send>>,
    model: Model,
    track_changes: bool,
    selection_only: bool,
    attachments: Vec<PathBuf>,
    picking: Option<Receiver<FileDialogAnswer>>,
    input: String,
    conv: Conversation,
    /// The document whose conversation is loaded.
    key: Option<String>,
    running: Option<agent::Running>,
    turn: Option<Turn>,
    studio: Option<studio::Studio>,
    /// Put the keyboard in the message box on the next frame.
    focus_input: bool,
}

impl AiPane {
    pub fn agent(&self) -> Agent {
        self.conv.agent
    }

    pub fn busy(&self) -> bool {
        self.running.is_some()
    }
}

/// `ui.aiPane` (`value`: show or hide, default toggle, or show when an `agent` is given; `agent`:
/// `claude` or `codex`) and `ui.aiStudio` (Document Design).
pub fn command(app: &mut WordApp, id: &str, p: &Value) -> Result<Value, String> {
    sync_document(app);
    match id {
        "ui.aiStudio" => {
            if !app.has_document() {
                return Err("no document is open".into());
            }
            app.ai.studio = Some(studio::Studio::new(app));
            Ok(json!({"open": true}))
        }
        _ => {
            let agent = p.get("agent").map(|v| v.as_str().and_then(Agent::from_id).ok_or("agent: claude or codex")).transpose()?;
            if let Some(a) = agent {
                set_agent(app, a);
            }
            let value = p.get("value").map(|v| v.as_bool().ok_or("value: true or false")).transpose()?;
            app.ai.open = value.unwrap_or(agent.is_some() || !app.ai.open);
            if app.ai.open {
                app.ai.focus_input = true;
            }
            Ok(json!({"open": app.ai.open, "agent": app.ai.agent().id()}))
        }
    }
}

/// Talk to `agent` from now on; its own session starts with the next message.
fn set_agent(app: &mut WordApp, agent: Agent) {
    if app.ai.conv.agent == agent || app.ai.busy() {
        return;
    }
    app.ai.conv.agent = agent;
    app.ai.conv.session = None;
    save(app);
}

/// Load the open document's conversation when the document changes (not mid-turn).
fn sync_document(app: &mut WordApp) {
    if app.ai.busy() {
        return;
    }
    let key = store::key(app.session.path.as_deref());
    if app.ai.key.as_deref() == Some(key.as_str()) {
        return;
    }
    let agent = app.ai.conv.agent;
    app.ai.conv = store::load(app.ai.store_dir.as_deref(), &key);
    if app.ai.conv.messages.is_empty() && app.ai.conv.pending.is_none() {
        // A fresh conversation keeps the agent chosen last.
        app.ai.conv.agent = agent;
    }
    app.ai.key = Some(key);
}

fn save(app: &WordApp) {
    if let Some(key) = &app.ai.key {
        store::save(app.ai.store_dir.as_deref(), key, &app.ai.conv);
    }
}

pub fn show(app: &mut WordApp, ui: &mut egui::Ui) {
    sync_document(app);
    poll(app);
    if let Some(prompt) = studio::show(app, ui.ctx()) {
        app.ai.open = true;
        send(app, ui.ctx(), prompt, tx("Tidy the headings for the contents", "Ordena los títulos para el índice"), None);
    }
    if !app.ai.open {
        return;
    }
    chat::show(app, ui);
    if app.ai.busy() || app.ai.picking.is_some() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(60));
    }
}

/// The AI ribbon tab: the two agents, Document Design and how the agent works.
pub fn ribbon(app: &mut WordApp, ui: &mut egui::Ui) {
    use crate::widgets::{big, big_toggle_with, group, small};
    let (open, agent) = (app.ai.open, app.ai.agent());
    group(ui, tx("Assistant", "Asistente"), None, app, |ui, app| {
        big_toggle_with(ui, app, "aiClaude", "Claude", "ui.aiPane", json!({"agent": "claude"}), open && agent == Agent::Claude);
        big_toggle_with(ui, app, "aiCodex", "Codex", "ui.aiPane", json!({"agent": "codex"}), open && agent == Agent::Codex);
    });
    group(ui, tx("Document", "Documento"), None, app, |ui, app| {
        big(ui, app, "aiDesign", tx("Document\nDesign", "Diseño del\ndocumento"), "ui.aiStudio", json!({}), false);
    });
    group(ui, tx("How it works", "Cómo trabaja"), None, app, |ui, app| {
        ui.vertical(|ui| {
            let (sel, track) = (app.ai.selection_only, app.ai.track_changes);
            if small(
                ui,
                app,
                "aiCheck",
                Some(tx("Selection only", "Solo la selección")),
                tx("Selection only", "Solo la selección"),
                "ui.aiPane",
                json!({"value": open}),
                sel,
            )
            .clicked()
            {
                app.ai.selection_only = !sel;
            }
            if small(
                ui,
                app,
                "trackChanges",
                Some(tx("Track its changes", "Marcar sus cambios")),
                tx("Track its changes", "Marcar sus cambios"),
                "ui.aiPane",
                json!({"value": open}),
                track,
            )
            .clicked()
            {
                app.ai.track_changes = !track;
            }
        });
    });
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
                    Err(e) => note_msg(app, format!("{} {e}", tx("No file dialog could be shown:", "No se pudo mostrar el diálogo de archivos:"))),
                }
            }
        }
    }
    let Some(r) = &app.ai.running else { return };
    let started = r.started;
    let events: Vec<agent::Event> = r.rx.try_iter().collect();
    for ev in events {
        let Some(turn) = app.ai.turn.as_mut() else { break };
        match ev {
            agent::Event::Text(s) => turn.reply = Some(s),
            agent::Event::Tool(name, command) => {
                turn.phase = Phase::of(&name, command.as_deref());
                if turn.phase.edits() {
                    turn.edits = turn.edits.saturating_add(1);
                }
            }
            agent::Event::Session(s) => app.ai.conv.session = Some(s),
            agent::Event::Error(e) => turn.error = Some(e),
            agent::Event::Done { ok } => {
                let secs = ((crate::now_ms() - started) / 1000.0).max(0.0) as u32;
                finish(app, ok, secs);
                break;
            }
        }
    }
}

/// The turn ended: its answer and a line of what it did, or a card to continue it.
fn finish(app: &mut WordApp, ok: bool, secs: u32) {
    app.ai.running = None;
    let Some(turn) = app.ai.turn.take() else { return };
    if let Some(text) = turn.reply {
        app.ai.conv.push(Msg::Assistant { text });
    }
    if ok {
        app.ai.conv.push(Msg::Work { edits: turn.edits, secs });
        app.ai.conv.pending = None;
    } else {
        let detail = if turn.stopped { String::new() } else { turn.error.unwrap_or_default() };
        app.ai.conv.push(Msg::Stopped { by_user: turn.stopped, detail });
    }
    save(app);
}

/// A message from the pane itself.
fn note_msg(app: &mut WordApp, text: String) {
    app.ai.conv.push(Msg::Note { text });
}

fn pick_attachments(app: &mut WordApp) {
    let req = FileDialogRequest::OpenMany { purpose: "attachments".into() };
    if let Some(show) = &app.services.file_dialog {
        app.ai.picking = Some(show(req));
    } else if let Some(pick) = &app.services.pick_open {
        let picked: Vec<String> = pick("attachments").into_iter().collect();
        add_attachments(app, picked);
    } else {
        note_msg(app, tx("This version has no file dialog.", "Esta versión no tiene diálogo de archivos.").to_string());
    }
}

fn add_attachments(app: &mut WordApp, paths: Vec<String>) {
    for p in paths.into_iter().map(PathBuf::from) {
        if !app.ai.attachments.contains(&p) {
            app.ai.attachments.push(p);
        }
    }
}

/// An answer put into the document at the caret, formatted (headings, lists, bold).
fn insert_reply(app: &mut WordApp, text: &str) {
    let _ = app.run("edit.paste", json!({"html": markdown::to_html(text)}));
}

/// The request with the user's options: scope, track changes, attached files.
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

/// Run the agent on `request`; `shown` is what the conversation shows for it. `pending` keeps the
/// original request of a task being continued.
fn send(app: &mut WordApp, ctx: &egui::Context, request: &str, shown: &str, pending: Option<String>) {
    if request.trim().is_empty() || app.ai.busy() {
        return;
    }
    let Some(port) = ensure_control(app, ctx) else {
        note_msg(
            app,
            tx(
                "The control channel could not start, so the assistant cannot reach the document.",
                "No se pudo iniciar el canal de control, así que el asistente no puede acceder al documento.",
            )
            .to_string(),
        );
        return;
    };
    let prompt = build_prompt(app, request);
    let files = app.ai.attachments.iter().filter_map(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).collect();
    app.ai.conv.push(Msg::User { text: shown.to_string(), files });
    app.ai.conv.pending = Some(pending.unwrap_or_else(|| request.to_string()));
    let job = agent::Job {
        agent: app.ai.agent(),
        model: app.ai.model,
        port,
        session: app.ai.conv.session.clone(),
        dirs: app.ai.attachments.iter().filter_map(|p| p.parent().map(PathBuf::from)).collect(),
    };
    let missing = match job.agent {
        Agent::Claude => tx(
            "Claude Code isn't installed. Install it from claude.com/claude-code and sign in, then try again.",
            "Claude Code no está instalado. Instálalo desde claude.com/claude-code, inicia sesión y vuelve a intentarlo.",
        ),
        Agent::Codex => tx(
            "Codex isn't installed. Install the Codex command-line tool and sign in, then try again.",
            "Codex no está instalado. Instala la herramienta de línea de órdenes de Codex, inicia sesión y vuelve a intentarlo.",
        ),
    };
    match agent::spawn(&job, &prompt, ctx.clone(), missing) {
        Ok(r) => {
            app.ai.running = Some(r);
            app.ai.turn = Some(Turn { phase: Phase::Thinking, edits: 0, reply: None, error: None, stopped: false });
            app.ai.attachments.clear();
        }
        Err(e) => {
            app.ai.conv.pending = None;
            note_msg(app, e);
        }
    }
    save(app);
}

/// Pick up the interrupted task where it stopped.
fn continue_task(app: &mut WordApp, ctx: &egui::Context) {
    let Some(task) = app.ai.conv.pending.clone() else { return };
    // A resumed session remembers the task; a new one is told what it was.
    let request = if app.ai.conv.session.is_some() { agent::CONTINUE.to_string() } else { format!("{}\n\nThe task was: {task}", agent::CONTINUE) };
    send(app, ctx, &request, tx("Continue where you left off", "Continúa donde lo dejaste"), Some(task));
}

fn stop(app: &mut WordApp) {
    if let Some(turn) = app.ai.turn.as_mut() {
        turn.stopped = true;
    }
    if let Some(r) = &app.ai.running {
        r.stop();
    }
}

fn new_conversation(app: &mut WordApp) {
    if app.ai.busy() {
        return;
    }
    app.ai.conv = Conversation { agent: app.ai.agent(), ..Default::default() };
    app.ai.attachments.clear();
    save(app);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> WordApp {
        let mut a = WordApp::new(wordcraft_engine::Session::new(wordcraft_engine::sample::sample_document()), crate::Services::default());
        a.ai.store_dir = None;
        a
    }

    #[test]
    fn the_ai_tab_buttons_choose_the_agent_and_open_the_chat() {
        let mut a = app();
        assert_eq!(a.run("ui.aiPane", json!({"agent": "codex"})).unwrap(), json!({"open": true, "agent": "codex"}));
        assert_eq!(a.ai.agent(), Agent::Codex);
        assert_eq!(a.run("ui.aiPane", json!({})).unwrap()["open"], false, "no agent: toggles");
        assert!(a.run("ui.aiPane", json!({"agent": "gpt"})).is_err());
        assert!(a.run("ui.aiPane", json!({"value": "yes"})).is_err());
        a.run("ui.aiStudio", json!({})).unwrap();
        assert!(a.ai.studio.is_some());
    }

    #[test]
    fn the_activity_line_speaks_plainly_and_counts_changes() {
        assert_eq!(Phase::of("inspect_document", None), Phase::Reading);
        assert_eq!(Phase::of("execute", Some("insert.coverPage")), Phase::Cover);
        assert_eq!(Phase::of("execute", Some("references.toc")), Phase::Contents);
        assert_eq!(Phase::of("execute", Some("design.theme")), Phase::Formatting);
        assert_eq!(Phase::of("execute", Some("edit.paste")), Phase::Writing);
        assert_eq!(Phase::of("type_text", None), Phase::Writing);
        assert!(!Phase::Reading.edits() && Phase::Writing.edits());
    }

    #[test]
    fn a_stopped_task_is_kept_to_continue_and_a_finished_one_is_not() {
        let mut a = app();
        a.ai.key = Some("t".into());
        a.ai.conv.pending = Some("Escribe un informe".into());
        a.ai.turn = Some(Turn { phase: Phase::Writing, edits: 3, reply: Some("Voy por la mitad".into()), error: None, stopped: true });
        finish(&mut a, false, 12);
        assert_eq!(a.ai.conv.pending.as_deref(), Some("Escribe un informe"));
        assert_eq!(a.ai.conv.messages.last(), Some(&Msg::Stopped { by_user: true, detail: String::new() }));
        a.ai.turn = Some(Turn { phase: Phase::Writing, edits: 2, reply: Some("Listo.".into()), error: None, stopped: false });
        finish(&mut a, true, 30);
        assert_eq!(a.ai.conv.pending, None);
        let n = a.ai.conv.messages.len();
        assert_eq!(a.ai.conv.messages[n - 2..], [Msg::Assistant { text: "Listo.".into() }, Msg::Work { edits: 2, secs: 30 }]);
    }
}
