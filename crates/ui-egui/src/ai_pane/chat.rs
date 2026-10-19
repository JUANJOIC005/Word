//! The chat pane: a header with the agent and its model, the conversation (a welcome screen with
//! suggestions while it is empty, or Colours and style) and the message box with its slash
//! commands.

use egui::{Color32, CornerRadius, Key, KeyboardShortcut, Margin, Modifiers, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

use super::store::Msg;
use super::{Agent, Model, Phase, markdown, tx, txf};
use crate::WordApp;
use crate::icons;
use crate::theme::{Tokens, medium, regular, semibold};

/// Requests behind the welcome screen's suggestions.
const PROOFREAD: &str = "Proofread the document (or only the selected text, if there is one) in place: fix spelling, grammar and \
punctuation and make unclear sentences clearer, without changing the meaning, the tone or the formatting.";
const SUMMARIZE: &str = "Summarize the document (or only the selected text, if there is one) here in the chat, in its language: \
the key points as short bullets, then one line with the conclusion. Don't change the document.";

/// The slash commands: what they do, their names (the first is shown) and a line about them.
const SLASH: [(&str, [&str; 2], &str, &str); 4] = [
    ("clear", ["/clear", "/limpiar"], "Start a new conversation (uses no tokens)", "Empieza una conversación nueva (no gasta tokens)"),
    (
        "compact",
        ["/compact", "/compactar"],
        "Summarize the chat so far to keep going with fewer tokens",
        "Resume lo hablado para seguir gastando menos tokens",
    ),
    ("design", ["/design", "/diseño"], "A new, different design for the document", "Un diseño nuevo y distinto para el documento"),
    ("colors", ["/colors", "/colores"], "The colours and type the AI uses", "Los colores y la tipografía que usa la IA"),
];

/// Something the user asked for while the pane was drawn, done after drawing.
enum Act {
    Fill(&'static str),
    Send(&'static str, &'static str),
    Design,
    Continue,
    Insert(String),
    Attach,
    NewChat,
    Stop,
    Agent(Agent),
    Submit(String),
    SkipSelection(String),
}

pub(super) fn mix(a: Color32, b: Color32, k: f32) -> Color32 {
    let l = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * k).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

fn name(a: Agent) -> &'static str {
    super::note(match a {
        Agent::Claude => "Claude",
        Agent::Codex => "Codex",
    })
}

/// The agent's mark on a soft tile.
pub fn avatar(p: &egui::Painter, r: Rect, agent: Agent, t: &Tokens) {
    let tile = match (agent, t.dark) {
        (Agent::Claude, false) => Color32::from_rgb(0xF7, 0xE9, 0xE3),
        (Agent::Claude, true) => Color32::from_rgb(0x4A, 0x30, 0x28),
        (Agent::Codex, _) => t.hover,
    };
    p.rect_filled(r, CornerRadius::same((r.width() * 0.28) as u8), tile);
    let icon = match agent {
        Agent::Claude => "aiClaude",
        Agent::Codex => "aiCodex",
    };
    icons::paint(p, r.shrink(r.width() * 0.18), icon, t.text, t.accent);
}

fn model_label(m: Model) -> &'static str {
    match m {
        Model::Auto => tx("Automatic", "Automático"),
        Model::Opus => super::note("Opus"),
        Model::Sonnet => super::note("Sonnet"),
        Model::Haiku => super::note("Haiku"),
    }
}

fn model_hint(m: Model) -> &'static str {
    match m {
        Model::Auto => tx("Best balance of quality and cost", "El mejor equilibrio entre calidad y coste"),
        Model::Opus => tx("Highest quality for long documents", "Máxima calidad para documentos largos"),
        Model::Sonnet => tx("Fast and precise", "Rápido y preciso"),
        Model::Haiku => tx("Quickest and lightest", "El más rápido y ligero"),
    }
}

/// A menu row: icon or avatar, title and a line of description; `on` draws a check.
fn menu_row(ui: &mut Ui, title: &str, hint: &str, on: bool, t: &Tokens, lead: impl FnOnce(&egui::Painter, Rect)) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(260.0, 44.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(r, CornerRadius::same(8), t.hover);
    }
    lead(ui.painter(), Rect::from_center_size(pos2(r.min.x + 22.0, r.center().y), vec2(26.0, 26.0)));
    ui.painter().text(pos2(r.min.x + 44.0, r.min.y + 8.0), egui::Align2::LEFT_TOP, title, medium(12.5), t.text);
    ui.painter().text(pos2(r.min.x + 44.0, r.min.y + 25.0), egui::Align2::LEFT_TOP, hint, regular(11.0), t.text_dim);
    if on {
        icons::paint(ui.painter(), Rect::from_center_size(pos2(r.max.x - 16.0, r.center().y), vec2(14.0, 14.0)), "aiCheck", t.accent, t.accent);
    }
    resp
}

/// A borderless round icon button.
fn round_button(ui: &mut Ui, icon: &str, tip: &str, size: f32, t: &Tokens) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    if resp.hovered() {
        ui.painter().circle_filled(r.center(), size / 2.0, t.hover);
    }
    icons::paint(ui.painter(), Rect::from_center_size(r.center(), vec2(size * 0.55, size * 0.55)), icon, t.icon, t.accent);
    resp.on_hover_text(tip)
}

pub fn show(app: &mut WordApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let mut act = None;
    egui::Panel::right("ai_pane")
        .default_size(410.0)
        .min_size(330.0)
        .max_size(760.0)
        .resizable(true)
        .frame(egui::Frame::NONE.fill(t.panel).stroke(Stroke::new(1.0, t.border)))
        .show(ui, |ui| {
            egui::Panel::top("ai_head")
                .frame(egui::Frame::NONE.inner_margin(Margin { left: 16, right: 10, top: 12, bottom: 10 }))
                .resizable(false)
                .show(ui, |ui| {
                    header(app, ui, &t, &mut act);
                });
            let r = ui.cursor();
            ui.painter().line_segment([pos2(r.min.x, r.min.y), pos2(r.max.x, r.min.y)], Stroke::new(1.0, t.border));
            egui::Panel::bottom("ai_composer")
                .frame(egui::Frame::NONE.inner_margin(Margin { left: 14, right: 14, top: 8, bottom: 10 }))
                .resizable(false)
                .show(ui, |ui| {
                    composer(app, ui, &t, &mut act);
                });
            if app.ai.show_settings {
                settings_view(app, ui, &t);
            } else {
                conversation(app, ui, &t, &mut act);
            }
        });
    let ctx = ui.ctx().clone();
    match act {
        None => {}
        Some(Act::Fill(text)) => {
            app.ai.input = text.to_string();
            app.ai.focus_input = true;
        }
        Some(Act::Send(request, shown)) => super::send(app, &ctx, request, shown, None, super::Kind::Normal),
        Some(Act::Design) => super::design_document(app, &ctx),
        Some(Act::Continue) => super::continue_task(app, &ctx),
        Some(Act::Insert(text)) => super::insert_reply(app, &text),
        Some(Act::Attach) => super::pick_attachments(app),
        Some(Act::NewChat) => super::new_conversation(app),
        Some(Act::Stop) => super::stop(app),
        Some(Act::Agent(a)) => super::set_agent(app, a),
        Some(Act::Submit(text)) => super::submit(app, &ctx, &text),
        Some(Act::SkipSelection(sel)) => app.ai.skip_selection = Some(sel),
    }
}

/// Colours and style, in place of the conversation.
fn settings_view(app: &mut WordApp, ui: &mut Ui, t: &Tokens) {
    egui::ScrollArea::vertical().id_salt("ai_settings").auto_shrink([false, false]).show(ui, |ui| {
        ui.add_space(10.0);
        let w = ui.available_width() - 32.0;
        ui.horizontal(|ui| {
            ui.add_space(16.0);
            ui.vertical(|ui| {
                ui.set_width(w);
                super::design::settings(app, ui, t);
                if super::design::pill_button(ui, tx("Back to the chat", "Volver al chat"), true, t).clicked() {
                    app.ai.show_settings = false;
                }
                ui.add_space(16.0);
            });
        });
    });
}

fn header(app: &mut WordApp, ui: &mut Ui, t: &Tokens, act: &mut Option<Act>) {
    let agent = app.ai.agent();
    ui.horizontal(|ui| {
        // The agent: its mark, name and state; a click switches agent.
        let (r, resp) = ui.allocate_exact_size(vec2(170.0, 36.0), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(r.expand(3.0), CornerRadius::same(8), t.hover);
        }
        avatar(ui.painter(), Rect::from_min_size(r.min + vec2(0.0, 2.0), vec2(32.0, 32.0)), agent, t);
        let title = ui.painter().text(pos2(r.min.x + 42.0, r.min.y + 1.0), egui::Align2::LEFT_TOP, name(agent), semibold(14.5), t.text);
        icons::paint(
            ui.painter(),
            Rect::from_center_size(pos2(title.max.x + 9.0, title.center().y + 1.0), vec2(12.0, 12.0)),
            "aiChevron",
            t.text_dim,
            t.accent,
        );
        let (state, dot) = if app.ai.busy() {
            (tx("Working…", "Trabajando…"), t.accent)
        } else if app.ai.conv.pending.is_some() {
            (tx("Paused", "En pausa"), t.orange)
        } else {
            (tx("Ready", "Listo"), t.green)
        };
        let pulse = if app.ai.busy() { 0.55 + 0.45 * (ui.input(|i| i.time) as f32 * 4.0).sin().abs() } else { 1.0 };
        ui.painter().circle_filled(pos2(r.min.x + 46.0, r.min.y + 27.0), 3.5, dot.gamma_multiply(pulse));
        ui.painter().text(pos2(r.min.x + 55.0, r.min.y + 27.0), egui::Align2::LEFT_CENTER, state, regular(11.5), t.text_dim);
        egui::Popup::menu(&resp).show(|ui| {
            for a in [Agent::Claude, Agent::Codex] {
                let hint = match a {
                    Agent::Claude => tx("Careful writing and editing", "Redacta y edita con cuidado"),
                    Agent::Codex => tx("Quick, precise changes", "Cambios rápidos y precisos"),
                };
                if menu_row(ui, name(a), hint, a == agent, t, |p, r| avatar(p, r, a, t)).clicked() {
                    *act = Some(Act::Agent(a));
                    ui.close();
                }
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if round_button(ui, "close", tx("Close", "Cerrar"), 28.0, t).clicked() {
                app.ai.open = false;
            }
            let on = app.ai.show_settings;
            if round_button(ui, "options", tx("Colours and style", "Colores y estilo"), 28.0, t).clicked() {
                app.ai.show_settings = !on;
            }
            if !app.ai.conv.messages.is_empty()
                && round_button(ui, "aiNewChat", tx("New conversation (/clear)", "Nueva conversación (/clear)"), 28.0, t).clicked()
            {
                *act = Some(Act::NewChat);
            }
            if agent == Agent::Claude {
                ui.add_space(4.0);
                model_pill(app, ui, t);
            }
        });
    });
}

/// The model in a small pill; a click lists the others with what each is good at.
fn model_pill(app: &mut WordApp, ui: &mut Ui, t: &Tokens) {
    let label = model_label(app.ai.settings.model);
    let g = ui.painter().layout_no_wrap(label.to_string(), medium(11.5), t.text);
    let (r, resp) = ui.allocate_exact_size(vec2(g.size().x + 30.0, 26.0), Sense::click());
    ui.painter().rect_filled(r, CornerRadius::same(13), if resp.hovered() { t.pressed } else { t.hover });
    ui.painter().galley(pos2(r.min.x + 11.0, r.center().y - g.size().y / 2.0), g, t.text);
    icons::paint(ui.painter(), Rect::from_center_size(pos2(r.max.x - 11.0, r.center().y), vec2(10.0, 10.0)), "aiChevron", t.text_dim, t.accent);
    let resp = resp.on_hover_text(tx("Model", "Modelo"));
    egui::Popup::menu(&resp).show(|ui| {
        for m in Model::ALL {
            let on = app.ai.settings.model == m;
            if menu_row(ui, model_label(m), model_hint(m), on, t, |p, r| icons::paint(p, r.shrink(4.0), "aiSpark", t.icon, t.accent)).clicked() {
                app.ai.settings.model = m;
                super::save_settings(app);
                ui.close();
            }
        }
    });
}

fn conversation(app: &mut WordApp, ui: &mut Ui, t: &Tokens, act: &mut Option<Act>) {
    let agent = app.ai.agent();
    let has_sel = super::selection(app).is_some();
    egui::ScrollArea::vertical().id_salt("ai_chat").stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
        ui.add_space(10.0);
        let w = ui.available_width() - 36.0;
        ui.horizontal(|ui| {
            ui.add_space(18.0);
            ui.vertical(|ui| {
                ui.set_width(w);
                ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
                if app.ai.conv.messages.is_empty() && app.ai.turn.is_none() {
                    welcome(ui, agent, t, act);
                }
                let msgs = &app.ai.conv.messages;
                let last = msgs.len().saturating_sub(1);
                let can_continue = app.ai.conv.pending.is_some() && !app.ai.busy();
                let mut i = 0;
                while i < msgs.len() {
                    match &msgs[i] {
                        Msg::User { text, files } => user_bubble(ui, text, files, t),
                        Msg::Assistant { text } => {
                            // The answer carries its turn's summary in its footer.
                            let meta = match msgs.get(i + 1) {
                                Some(Msg::Work { edits, secs }) => {
                                    i += 1;
                                    Some((*edits, *secs))
                                }
                                _ => None,
                            };
                            assistant(ui, text, meta, has_sel, agent, t, act);
                        }
                        Msg::Work { edits, secs } => work_line(ui, *edits, *secs, t),
                        Msg::Stopped { by_user, detail } => stopped_card(ui, *by_user, detail, can_continue && i == last, agent, t, act),
                        Msg::Note { text } => note_line(ui, text, t),
                    }
                    ui.add_space(12.0);
                    i += 1;
                }
                if let Some(turn) = &app.ai.turn {
                    let started = app.ai.running.as_ref().map_or(0.0, |r| r.started);
                    live(ui, turn.phase, turn.edits, started, agent, t);
                }
                ui.add_space(8.0);
            });
        });
    });
}

/// The empty conversation: a greeting and what the assistant can do, as cards.
fn welcome(ui: &mut Ui, agent: Agent, t: &Tokens, act: &mut Option<Act>) {
    ui.add_space((ui.available_height() * 0.06).clamp(8.0, 40.0));
    let w = ui.available_width();
    let (r, _) = ui.allocate_exact_size(vec2(w, 56.0), Sense::hover());
    avatar(ui.painter(), Rect::from_center_size(r.center(), vec2(52.0, 52.0)), agent, t);
    ui.add_space(10.0);
    centred(ui, tx("What shall we work on?", "¿En qué trabajamos hoy?"), semibold(19.0), t.text);
    centred(
        ui,
        tx("Select text in the document and I'll work on just that.", "Selecciona texto en el documento y trabajaré solo sobre eso."),
        regular(12.5),
        t.text_dim,
    );
    ui.add_space(16.0);
    let cards: [(&str, &str, &str, Act); 5] = [
        (
            "aiWrite",
            tx("Draft a document", "Redactar un documento"),
            tx("Written and designed for you", "Escrito y diseñado para ti"),
            Act::Fill(tx("Write a document about ", "Escribe un documento sobre ")),
        ),
        (
            "aiDesign",
            tx("Design with AI", "Diseñar con IA"),
            tx("Cover, type and colours, new each time", "Portada, tipografía y colores, distinto cada vez"),
            Act::Design,
        ),
        (
            "aiProof",
            tx("Proofread", "Revisar y corregir"),
            tx("Spelling, grammar and clarity", "Ortografía, gramática y claridad"),
            Act::Send(PROOFREAD, tx("Proofread and correct", "Revisa y corrige")),
        ),
        (
            "aiSummary",
            tx("Summarize", "Resumir"),
            tx("The key points, here in the chat", "Las ideas clave, aquí en el chat"),
            Act::Send(SUMMARIZE, tx("Summarize", "Resume")),
        ),
        (
            "aiTranslate",
            tx("Translate", "Traducir"),
            tx("The selection or the whole text", "La selección o el texto entero"),
            Act::Fill(tx("Translate into ", "Traduce al ")),
        ),
    ];
    for (icon, title, hint, a) in cards {
        if suggestion(ui, icon, title, hint, t).clicked() {
            *act = Some(a);
        }
        ui.add_space(2.0);
    }
    ui.add_space(8.0);
    centred(
        ui,
        tx("Type / for commands such as /clear and /compact", "Escribe / para ver comandos como /clear y /compact"),
        regular(11.0),
        t.text_dim,
    );
}

fn centred(ui: &mut Ui, text: &str, font: egui::FontId, color: Color32) {
    let g = ui.painter().layout(text.to_string(), font, color, ui.available_width());
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), g.size().y), Sense::hover());
    ui.painter().galley(pos2(r.center().x - g.size().x / 2.0, r.min.y), g, color);
}

fn suggestion(ui: &mut Ui, icon: &str, title: &str, hint: &str, t: &Tokens) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 56.0), Sense::click());
    let k = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.12);
    ui.painter().rect_filled(r, CornerRadius::same(12), mix(t.panel, t.hover, 0.4 + 0.6 * k));
    ui.painter().rect_stroke(r, CornerRadius::same(12), Stroke::new(1.0, mix(t.border, t.accent, k * 0.6)), StrokeKind::Inside);
    let tile = Rect::from_min_size(pos2(r.min.x + 12.0, r.center().y - 16.0), vec2(32.0, 32.0));
    ui.painter().rect_filled(tile, CornerRadius::same(9), mix(t.panel, t.accent, if t.dark { 0.25 } else { 0.12 }));
    icons::paint(ui.painter(), tile.shrink(7.0), icon, t.accent_text, t.accent);
    ui.painter().text(pos2(tile.max.x + 12.0, r.min.y + 11.0), egui::Align2::LEFT_TOP, title, medium(13.0), t.text);
    ui.painter().text(pos2(tile.max.x + 12.0, r.min.y + 30.0), egui::Align2::LEFT_TOP, hint, regular(11.5), t.text_dim);
    let arrow = Rect::from_center_size(pos2(r.max.x - 20.0 + 4.0 * (1.0 - k), r.center().y), vec2(14.0, 14.0));
    icons::paint(ui.painter(), arrow, "aiArrow", t.text_dim.gamma_multiply(0.4 + 0.6 * k), t.accent);
    resp
}

fn user_bubble(ui: &mut Ui, text: &str, files: &[String], t: &Tokens) {
    let max = ui.available_width() * 0.84;
    let g = ui.painter().layout(text.to_string(), regular(13.0), t.text, max - 28.0);
    let file_h = files.len() as f32 * 22.0;
    let bw = g.size().x.max(if files.is_empty() { 0.0 } else { 160.0_f32.min(max - 28.0) }) + 28.0;
    let bh = g.size().y + 20.0 + file_h + if files.is_empty() { 0.0 } else { 6.0 };
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), bh), Sense::hover());
    let r = Rect::from_min_size(pos2(row.max.x - bw, row.min.y), vec2(bw, bh));
    let fill = mix(t.panel, t.accent, if t.dark { 0.3 } else { 0.13 });
    ui.painter().rect_filled(r, CornerRadius { nw: 18, ne: 18, sw: 18, se: 6 }, fill);
    ui.painter().galley(pos2(r.min.x + 14.0, r.min.y + 10.0), g.clone(), t.text);
    let mut y = r.min.y + 10.0 + g.size().y + 6.0;
    for f in files {
        let chip = Rect::from_min_size(pos2(r.min.x + 10.0, y), vec2(bw - 20.0, 20.0));
        ui.painter().rect_filled(chip, CornerRadius::same(6), t.panel.gamma_multiply(0.6));
        icons::paint(
            ui.painter(),
            Rect::from_center_size(pos2(chip.min.x + 11.0, chip.center().y), vec2(13.0, 13.0)),
            "aiFile",
            t.text_dim,
            t.accent,
        );
        let name = ui.painter().layout(f.clone(), regular(11.0), t.text_dim, f32::INFINITY);
        let clip = ui.painter().with_clip_rect(chip.shrink(2.0));
        clip.galley(pos2(chip.min.x + 22.0, chip.center().y - name.size().y / 2.0), name, t.text_dim);
        y += 22.0;
    }
}

/// An answer: a small byline, the text set in clean type and a quiet footer with its actions and
/// what the turn did.
fn assistant(ui: &mut Ui, text: &str, meta: Option<(u32, u32)>, has_sel: bool, agent: Agent, t: &Tokens, act: &mut Option<Act>) {
    ui.horizontal(|ui| {
        let (a, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
        avatar(ui.painter(), a, agent, t);
        ui.add_space(2.0);
        ui.label(egui::RichText::new(name(agent)).font(semibold(12.0)).color(t.text_dim));
    });
    ui.add_space(2.0);
    markdown::show(ui, text, t);
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        if action(ui, "copy", tx("Copy", "Copiar"), t).clicked() {
            ui.ctx().copy_text(text.to_string());
        }
        let (label, tip) = if has_sel {
            (tx("Replace selection", "Sustituir selección"), tx("Put it in place of the selected text", "Ponerlo en lugar del texto seleccionado"))
        } else {
            (tx("Insert", "Insertar"), tx("Insert at the cursor, with its format", "Insertar en el cursor, con su formato"))
        };
        if action(ui, "aiInsert", label, t).on_hover_text(tip).clicked() {
            *act = Some(Act::Insert(text.to_string()));
        }
        if let Some((edits, secs)) = meta {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(meta_text(edits, secs)).font(regular(11.0)).color(t.text_dim));
            });
        }
    });
}

/// A quiet footer action: an icon and a word, lit on hover.
fn action(ui: &mut Ui, icon: &str, text: &str, t: &Tokens) -> egui::Response {
    let g = ui.painter().layout_no_wrap(text.to_string(), regular(11.5), t.text_dim);
    let (r, resp) = ui.allocate_exact_size(vec2(g.size().x + 32.0, 24.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(r, CornerRadius::same(7), t.hover);
    }
    let c = if resp.hovered() { t.text } else { t.text_dim };
    icons::paint(ui.painter(), Rect::from_center_size(pos2(r.min.x + 13.0, r.center().y), vec2(13.0, 13.0)), icon, c, t.accent);
    ui.painter().galley(pos2(r.min.x + 24.0, r.center().y - g.size().y / 2.0), g, c);
    resp
}

/// "2 changes · 6 s", or just the time when nothing in the document changed.
fn meta_text(edits: u32, secs: u32) -> String {
    let time = if secs >= 60 { format!("{} min {} s", secs / 60, secs % 60) } else { format!("{secs} s") };
    match edits {
        0 => super::noted(time),
        1 => txf("1 change · {name}", "1 cambio · {name}", &time),
        n => super::noted(txf("{n} changes · {name}", "{n} cambios · {name}", &time).replace("{n}", &n.to_string())),
    }
}

/// What a turn without words did, in one quiet line.
fn work_line(ui: &mut Ui, edits: u32, secs: u32, t: &Tokens) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
        icons::paint(ui.painter(), r, "aiCheck", t.green, t.green);
        let text = if edits == 0 { txf("Done · {name}", "Hecho · {name}", &meta_text(0, secs)) } else { meta_text(edits, secs) };
        ui.label(egui::RichText::new(text).font(regular(11.5)).color(t.text_dim));
    });
}

fn note_line(ui: &mut Ui, text: &str, t: &Tokens) {
    egui::Frame::NONE.fill(t.hover).corner_radius(CornerRadius::same(10)).inner_margin(Margin::symmetric(12, 8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(egui::RichText::new(text).font(regular(12.0)).color(t.text));
    });
}

/// A task that stopped before the end, with Continue on the latest one.
fn stopped_card(ui: &mut Ui, by_user: bool, detail: &str, can_continue: bool, agent: Agent, t: &Tokens, act: &mut Option<Act>) {
    egui::Frame::NONE
        .fill(t.input)
        .stroke(Stroke::new(1.0, t.border))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                icons::paint(ui.painter(), r, "aiPause", if by_user { t.text_dim } else { t.orange }, t.accent);
                let text = if by_user {
                    tx("You stopped the task.", "Has detenido la tarea.").to_string()
                } else {
                    txf("{name} stopped before finishing.", "{name} se detuvo antes de terminar.", name(agent))
                };
                ui.label(egui::RichText::new(text).font(medium(12.5)).color(t.text));
            });
            if !detail.is_empty() {
                let short: String = detail.chars().take(180).collect();
                ui.label(egui::RichText::new(short).font(regular(11.0)).color(t.text_dim));
            }
            if can_continue {
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(tx(
                        "What's already done stays in the document. I can pick up where it stopped.",
                        "Lo hecho sigue en el documento. Puedo seguir donde se quedó.",
                    ))
                    .font(regular(12.0))
                    .color(t.text_dim),
                );
                ui.add_space(6.0);
                if super::design::pill_button(ui, tx("Continue", "Continuar"), true, t).clicked() {
                    *act = Some(Act::Continue);
                }
            }
        });
}

/// The turn in progress: what the agent is doing, in plain words, with a moving bar.
fn live(ui: &mut Ui, phase: Phase, edits: u32, started: f64, agent: Agent, t: &Tokens) {
    let time = ui.input(|i| i.time) as f32;
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 46.0), Sense::hover());
    let a = Rect::from_min_size(r.min, vec2(24.0, 24.0));
    let ring = 0.5 + 0.5 * (time * 3.0).sin();
    ui.painter().rect_stroke(
        a.expand(2.0 + ring * 2.0),
        CornerRadius::same(9),
        Stroke::new(1.5, t.accent.gamma_multiply(0.5 * (1.0 - ring))),
        StrokeKind::Outside,
    );
    avatar(ui.painter(), a, agent, t);
    let dots = ".".repeat(1 + (time * 2.5) as usize % 3);
    let label = super::noted(format!("{}{dots}", phase.text()));
    let shimmer = mix(t.text_dim, t.text, 0.5 + 0.5 * (time * 2.0).sin());
    ui.painter().text(pos2(a.max.x + 10.0, a.center().y), egui::Align2::LEFT_CENTER, label, medium(12.5), shimmer);
    let secs = ((crate::now_ms() - started) / 1000.0).max(0.0) as u32;
    let right = if edits > 0 {
        super::noted(format!("{} · {secs} s", tx("{n} changes", "{n} cambios").replace("{n}", &edits.to_string())))
    } else {
        super::noted(format!("{secs} s"))
    };
    ui.painter().text(pos2(r.max.x, a.center().y), egui::Align2::RIGHT_CENTER, right, regular(11.0), t.text_dim);
    // Indeterminate progress: a soft segment gliding along a thin track.
    let track = Rect::from_min_size(pos2(a.max.x + 10.0, r.min.y + 34.0), vec2(r.max.x - a.max.x - 10.0, 3.0));
    ui.painter().rect_filled(track, CornerRadius::same(2), t.hover);
    let k = (time * 0.6).fract();
    let seg_w = track.width() * 0.3;
    let x0 = track.min.x + (track.width() + seg_w) * k - seg_w;
    let seg = Rect::from_min_max(pos2(x0.max(track.min.x), track.min.y), pos2((x0 + seg_w).min(track.max.x), track.max.y));
    if seg.width() > 0.0 {
        ui.painter().rect_filled(seg, CornerRadius::same(2), t.accent);
    }
}

/// The slash commands that start with what was typed.
fn slash_matches(input: &str) -> Vec<&'static (&'static str, [&'static str; 2], &'static str, &'static str)> {
    let typed = input.trim().to_lowercase();
    if !typed.starts_with('/') || typed.contains(' ') {
        return Vec::new();
    }
    SLASH.iter().filter(|(_, names, _, _)| names.iter().any(|n| n.starts_with(typed.as_str()))).collect()
}

fn composer(app: &mut WordApp, ui: &mut Ui, t: &Tokens, act: &mut Option<Act>) {
    let busy = app.ai.busy();
    let agent = app.ai.agent();
    let sel = super::selection(app);
    // What goes with the message: the selection and track changes, as chips.
    if sel.is_some() || app.ai.settings.track_changes {
        ui.horizontal_wrapped(|ui| {
            if let Some(sel) = &sel {
                let words = sel.split_whitespace().count();
                let label = if words == 1 {
                    tx("Selection · 1 word", "Selección · 1 palabra").to_string()
                } else {
                    super::noted(tx("Selection · {n} words", "Selección · {n} palabras").replace("{n}", &words.to_string()))
                };
                if chip(ui, "aiSpark", &label, t)
                    .on_hover_text(tx(
                        "I'll work on the selected text. Click to leave it out.",
                        "Trabajaré sobre el texto seleccionado. Pulsa para no usarlo.",
                    ))
                    .clicked()
                {
                    *act = Some(Act::SkipSelection(sel.clone()));
                }
            }
            if app.ai.settings.track_changes && chip(ui, "trackChanges", tx("Changes tracked", "Cambios marcados"), t).clicked() {
                app.ai.settings.track_changes = false;
                super::save_settings(app);
            }
        });
        ui.add_space(6.0);
    }
    let focus_id = egui::Id::new("ai_input");
    let focused = ui.ctx().memory(|m| m.has_focus(focus_id));
    let border = if focused { mix(t.input_border, t.accent, 0.7) } else { t.input_border };
    egui::Frame::NONE
        .fill(t.input)
        .stroke(Stroke::new(if focused { 1.5 } else { 1.0 }, border))
        .corner_radius(CornerRadius::same(18))
        .inner_margin(Margin { left: 12, right: 10, top: 10, bottom: 8 })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // The slash commands, while one is being typed.
            let matches = slash_matches(&app.ai.input);
            for &&(_, names, en, es) in &matches {
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(r, CornerRadius::same(8), t.hover);
                }
                ui.painter().text(pos2(r.min.x + 8.0, r.min.y + 7.0), egui::Align2::LEFT_TOP, super::note(names[0]), medium(12.5), t.accent_text);
                ui.painter().text(pos2(r.min.x + 8.0, r.min.y + 24.0), egui::Align2::LEFT_TOP, tx(en, es), regular(11.0), t.text_dim);
                if resp.clicked() {
                    *act = Some(Act::Submit(names[0].to_string()));
                    app.ai.input.clear();
                }
            }
            if !matches.is_empty() {
                ui.separator();
            }
            if !app.ai.attachments.is_empty() {
                let mut remove = None;
                ui.horizontal_wrapped(|ui| {
                    for (i, p) in app.ai.attachments.iter().enumerate() {
                        let n = p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned());
                        if chip(ui, "aiFile", &super::noted(n), t).on_hover_text(p.display().to_string()).clicked() {
                            remove = Some(i);
                        }
                    }
                });
                if let Some(i) = remove {
                    app.ai.attachments.remove(i);
                }
                ui.add_space(4.0);
            }
            let hint = if sel.is_some() {
                tx("Ask about the selection…", "Pide algo sobre la selección…").to_string()
            } else {
                txf("Message {name}… (/ for commands)", "Escribe a {name}… (/ para comandos)", name(agent))
            };
            let edit = egui::ScrollArea::vertical().id_salt("ai_input_scroll").max_height(180.0).show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut app.ai.input)
                        .id(focus_id)
                        .frame(egui::Frame::NONE)
                        .margin(Margin::ZERO)
                        .desired_rows(2)
                        .desired_width(f32::INFINITY)
                        .font(regular(13.5))
                        .hint_text(egui::RichText::new(hint).color(t.text_dim))
                        .return_key(KeyboardShortcut::new(Modifiers::SHIFT, Key::Enter)),
                )
            });
            let edit = edit.inner;
            if std::mem::take(&mut app.ai.focus_input) {
                edit.request_focus();
            }
            let enter = edit.has_focus() && ui.input(|i| i.key_pressed(Key::Enter) && !i.modifiers.shift);
            ui.add_space(4.0);
            let mut send_now = false;
            ui.horizontal(|ui| {
                let plus = round_button(ui, "aiPlus", tx("Add", "Añadir"), 30.0, t);
                egui::Popup::menu(&plus).show(|ui| {
                    if menu_row(
                        ui,
                        tx("Attach files", "Adjuntar archivos"),
                        tx("Read as Markdown to save tokens", "Se leen como Markdown para ahorrar tokens"),
                        false,
                        t,
                        |p, r| icons::paint(p, r.shrink(4.0), "aiAttach", t.icon, t.accent),
                    )
                    .clicked()
                    {
                        *act = Some(Act::Attach);
                        ui.close();
                    }
                    if menu_row(
                        ui,
                        tx("Design with AI", "Diseñar con IA"),
                        tx("A new, different design each time", "Un diseño nuevo y distinto cada vez"),
                        false,
                        t,
                        |p, r| icons::paint(p, r.shrink(4.0), "aiDesign", t.icon, t.accent),
                    )
                    .clicked()
                    {
                        *act = Some(Act::Design);
                        ui.close();
                    }
                    if menu_row(
                        ui,
                        tx("Colours and style", "Colores y estilo"),
                        tx("What every design must keep", "Lo que todo diseño debe respetar"),
                        false,
                        t,
                        |p, r| icons::paint(p, r.shrink(4.0), "colors", t.icon, t.accent),
                    )
                    .clicked()
                    {
                        app.ai.show_settings = true;
                        ui.close();
                    }
                    ui.separator();
                    let track = app.ai.settings.track_changes;
                    if menu_row(
                        ui,
                        tx("Track its changes", "Marcar sus cambios"),
                        tx("Review them later in Review", "Revísalos después en Revisar"),
                        track,
                        t,
                        |p, r| icons::paint(p, r.shrink(4.0), "trackChanges", t.icon, t.accent),
                    )
                    .clicked()
                    {
                        app.ai.settings.track_changes = !track;
                        super::save_settings(app);
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (r, resp) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::click());
                    if busy {
                        ui.painter().circle_filled(r.center(), 16.0, if resp.hovered() { t.text_dim } else { t.text });
                        icons::paint(ui.painter(), r.shrink(9.0), "aiStop", t.panel, t.panel);
                        if resp.on_hover_text(tx("Stop", "Detener")).clicked() {
                            *act = Some(Act::Stop);
                        }
                    } else {
                        let ready = !app.ai.input.trim().is_empty();
                        let fill = match (ready, resp.hovered()) {
                            (true, false) => t.accent,
                            (true, true) => t.accent_text,
                            _ => t.border_strong,
                        };
                        ui.painter().circle_filled(r.center(), 16.0, fill);
                        icons::paint(ui.painter(), r.shrink(8.0), "aiSend", t.on_accent, t.on_accent);
                        if resp.on_hover_text(tx("Send (Enter)", "Enviar (Intro)")).clicked() && ready {
                            send_now = true;
                        }
                    }
                });
            });
            if (send_now || enter) && !busy && !app.ai.input.trim().is_empty() {
                // The agent's edits go through the document: leave the keyboard to it.
                edit.surrender_focus();
                let mut text = app.ai.input.trim().to_string();
                // A command typed in part runs the first one it matches.
                if super::slash(&text).is_none()
                    && let Some((_, names, _, _)) = matches.first()
                {
                    text = names[0].to_string();
                }
                app.ai.input.clear();
                *act = Some(Act::Submit(text));
            }
        });
    ui.add_space(6.0);
    centred(
        ui,
        tx("AI can make mistakes. Check important changes.", "La IA puede equivocarse. Revisa los cambios importantes."),
        regular(10.5),
        t.text_dim,
    );
}

/// A small rounded chip with an icon and a close mark; a click removes it.
fn chip(ui: &mut Ui, icon: &str, text: &str, t: &Tokens) -> egui::Response {
    let g = ui.painter().layout_no_wrap(text.to_string(), regular(11.5), t.text);
    let (r, resp) = ui.allocate_exact_size(vec2(g.size().x.min(180.0) + 46.0, 24.0), Sense::click());
    ui.painter().rect_filled(r, CornerRadius::same(12), if resp.hovered() { t.pressed } else { t.hover });
    icons::paint(ui.painter(), Rect::from_center_size(pos2(r.min.x + 13.0, r.center().y), vec2(13.0, 13.0)), icon, t.icon, t.accent);
    let clip = ui.painter().with_clip_rect(Rect::from_min_max(pos2(r.min.x + 24.0, r.min.y), pos2(r.max.x - 20.0, r.max.y)));
    clip.galley(pos2(r.min.x + 24.0, r.center().y - g.size().y / 2.0), g, t.text);
    icons::paint(ui.painter(), Rect::from_center_size(pos2(r.max.x - 11.0, r.center().y), vec2(9.0, 9.0)), "close", t.text_dim, t.accent);
    resp.on_hover_text(tx("Remove", "Quitar"))
}
