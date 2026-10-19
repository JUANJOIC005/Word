//! Document Design (AI tab, and the pane's + menu): a cover, a colour and type theme, a table of
//! contents, page numbers, a header and paragraph spacing, chosen on visual cards with a live
//! preview. The design is applied with the document's own commands — instant and with no tokens
//! spent; only "Let the AI tidy the headings" asks the agent for anything.

use egui::{Color32, CornerRadius, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use serde_json::{Value, json};
use wordcraft_engine::cmd::design::THEMES;

use super::tx;
use crate::WordApp;
use crate::theme::{Tokens, medium, regular, semibold};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cover {
    None,
    Classic,
    Band,
    Minimal,
    Modern,
}

impl Cover {
    const ALL: [Cover; 5] = [Cover::None, Cover::Classic, Cover::Band, Cover::Minimal, Cover::Modern];

    fn id(self) -> &'static str {
        match self {
            Cover::None => "none",
            Cover::Classic => "classic",
            Cover::Band => "band",
            Cover::Minimal => "minimal",
            Cover::Modern => "modern",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Cover::None => tx("No cover", "Sin portada"),
            Cover::Classic => tx("Classic", "Clásica"),
            Cover::Band => tx("Band", "Banda"),
            Cover::Minimal => tx("Minimal", "Minimalista"),
            Cover::Modern => tx("Modern", "Moderna"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Numbers {
    None,
    Center,
    Right,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Spacing {
    Compact,
    Normal,
    Open,
}

pub struct Studio {
    pub cover: Cover,
    pub title: String,
    pub subtitle: String,
    pub author: String,
    /// Index into [`THEMES`].
    pub theme: usize,
    pub toc: bool,
    pub levels: u8,
    pub numbers: Numbers,
    pub header: String,
    pub spacing: Spacing,
    /// Ask the agent to give the headings real heading styles afterwards.
    pub polish: bool,
    pub error: Option<String>,
}

impl Studio {
    pub fn new(app: &WordApp) -> Self {
        let title = app.session.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let theme = THEMES.iter().position(|t| t.0 == app.session.doc.settings.theme_name).unwrap_or(0);
        Studio {
            cover: Cover::Band,
            title,
            subtitle: String::new(),
            author: app.session.author.clone(),
            theme,
            toc: true,
            levels: 3,
            numbers: Numbers::Center,
            header: String::new(),
            spacing: Spacing::Normal,
            polish: false,
            error: None,
        }
    }

    fn accent(&self) -> wordcraft_doc::props::Rgb {
        THEMES.get(self.theme).map_or(wordcraft_doc::props::Rgb(0x15, 0x60, 0x82), |t| t.3[0])
    }

    /// The commands that apply the design, in order. Header and page numbers come first (they
    /// leave the caret in the header, so it is closed after them); the table of contents goes at
    /// the start on a page of its own, and the cover before it.
    pub fn commands(&self) -> Vec<(&'static str, Value)> {
        let mut out = Vec::new();
        if let Some(t) = THEMES.get(self.theme) {
            out.push(("design.theme", json!({"name": t.0})));
        }
        let spacing = match self.spacing {
            Spacing::Compact => "compact",
            Spacing::Normal => "default",
            Spacing::Open => "open",
        };
        out.push(("design.paragraphSpacing", json!({"value": spacing})));
        if !self.header.trim().is_empty() {
            out.push(("insert.header", json!({"text": self.header.trim()})));
        }
        match self.numbers {
            Numbers::None => {}
            Numbers::Center => out.push(("insert.pageNumber", json!({"position": "bottom", "align": "center"}))),
            Numbers::Right => out.push(("insert.pageNumber", json!({"position": "bottom", "align": "right"}))),
        }
        if !self.header.trim().is_empty() || self.numbers != Numbers::None {
            out.push(("insert.closeHeader", json!({})));
        }
        if self.toc {
            out.push(("caret.docStart", json!({})));
            out.push(("text.pageBreak", json!({})));
            out.push(("caret.docStart", json!({})));
            out.push(("references.toc", json!({"levels": self.levels.clamp(1, 9), "title": tx("Contents", "Índice")})));
        }
        if self.cover != Cover::None {
            let a = self.accent();
            let mut p = json!({"style": self.cover.id(), "accent": format!("{:02X}{:02X}{:02X}", a.0, a.1, a.2)});
            for (key, v) in [("title", &self.title), ("subtitle", &self.subtitle), ("author", &self.author)] {
                if !v.trim().is_empty() {
                    p[key] = json!(v.trim());
                }
            }
            out.push(("caret.docStart", json!({})));
            out.push(("insert.coverPage", p));
        }
        out
    }
}

/// What Apply asks the agent for, when the user wants the headings tidied.
pub const POLISH: &str = "The user just applied a document design with a table of contents. Make sure every section title in the \
body uses Heading 1, Heading 2 or Heading 3 (para.style), without changing any wording, then refresh the table with \
execute references.updateToc. Reply in one short sentence.";

fn rgb(c: wordcraft_doc::props::Rgb) -> Color32 {
    Color32::from_rgb(c.0, c.1, c.2)
}

/// Run the design; `Some(prompt)` when the agent should tidy the headings afterwards.
pub fn apply(app: &mut WordApp) -> Result<Option<&'static str>, String> {
    let Some(st) = &app.ai.studio else { return Ok(None) };
    let (cmds, polish) = (st.commands(), st.polish && st.toc);
    for (id, params) in cmds {
        match app.run(id, params) {
            Ok(_) => {}
            // Not in a header any more (nothing to close): fine.
            Err(_) if id == "insert.closeHeader" => {}
            Err(e) => return Err(e),
        }
    }
    Ok(polish.then_some(POLISH))
}

/// A row of mutually exclusive choices drawn as one rounded control.
fn segmented<T: Copy + PartialEq>(ui: &mut Ui, value: &mut T, options: &[(T, &str)], t: &Tokens) {
    let h = 28.0;
    let widths: Vec<f32> =
        options.iter().map(|(_, l)| ui.painter().layout_no_wrap((*l).to_string(), regular(12.0), t.text).size().x + 22.0).collect();
    let (r, _) = ui.allocate_exact_size(vec2(widths.iter().sum::<f32>() + 4.0, h), Sense::hover());
    ui.painter().rect_filled(r, CornerRadius::same(8), t.hover);
    let mut x = r.min.x + 2.0;
    for ((v, label), w) in options.iter().zip(&widths) {
        let cell = Rect::from_min_size(pos2(x, r.min.y + 2.0), vec2(*w, h - 4.0));
        let resp = ui.interact(cell, ui.id().with(("seg", r.min.y as i32, x as i32)), Sense::click());
        let on = *value == *v;
        if on {
            ui.painter().rect_filled(cell, CornerRadius::same(6), t.panel);
            ui.painter().rect_stroke(cell, CornerRadius::same(6), Stroke::new(1.0, t.border), StrokeKind::Inside);
        } else if resp.hovered() {
            ui.painter().rect_filled(cell, CornerRadius::same(6), t.pressed);
        }
        ui.painter().text(
            cell.center(),
            egui::Align2::CENTER_CENTER,
            *label,
            if on { medium(12.0) } else { regular(12.0) },
            if on { t.text } else { t.text_dim },
        );
        if resp.clicked() {
            *value = *v;
        }
        x += w;
    }
}

/// An on/off switch with its label.
pub fn switch(ui: &mut Ui, on: &mut bool, label: &str, t: &Tokens) -> egui::Response {
    let text = ui.painter().layout_no_wrap(label.to_string(), regular(12.5), t.text);
    let (r, resp) = ui.allocate_exact_size(vec2(text.size().x + 46.0, 24.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
    }
    let k = ui.ctx().animate_bool(resp.id, *on);
    let track = Rect::from_min_size(pos2(r.min.x, r.center().y - 9.0), vec2(34.0, 18.0));
    let fill = if *on { t.accent } else { t.border_strong };
    ui.painter().rect_filled(track, CornerRadius::same(9), fill);
    let knob = pos2(track.min.x + 9.0 + k * 16.0, track.center().y);
    ui.painter().circle_filled(knob, 7.0, Color32::WHITE);
    ui.painter().galley(pos2(track.max.x + 10.0, r.center().y - text.size().y / 2.0), text, t.text);
    resp
}

fn section(ui: &mut Ui, title: &str, t: &Tokens) {
    ui.add_space(14.0);
    ui.label(egui::RichText::new(title).font(semibold(13.0)).color(t.text));
    ui.add_space(6.0);
}

fn field(ui: &mut Ui, value: &mut String, hint: &str, t: &Tokens) {
    egui::Frame::NONE
        .fill(t.input)
        .stroke(Stroke::new(1.0, t.input_border))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.add(egui::TextEdit::singleline(value).hint_text(hint).frame(egui::Frame::NONE).desired_width(f32::INFINITY).font(regular(12.5)));
        });
}

/// A miniature page in `r`: the cover in the chosen style and colour, or (`cover` None) a page of
/// text with headings in the accent.
fn page(p: &egui::Painter, r: Rect, cover: Cover, accent: Color32, title: &str, t: &Tokens) {
    p.rect_filled(r.translate(vec2(0.0, 2.0)), CornerRadius::same(3), t.page_shadow);
    p.rect_filled(r, CornerRadius::same(3), Color32::WHITE);
    let w = r.width();
    let ink = Color32::from_gray(0x33);
    let soft = Color32::from_gray(0xC8);
    let line = |y: f32, x0: f32, x1: f32, c: Color32, th: f32| {
        p.line_segment([pos2(r.min.x + x0 * w, r.min.y + y * r.height()), pos2(r.min.x + x1 * w, r.min.y + y * r.height())], Stroke::new(th, c));
    };
    let name = if title.trim().is_empty() { tx("Title", "Título") } else { title.trim() };
    let title_at = |y: f32, size: f32, c: Color32, center: bool| {
        let g = p.layout(name.to_string(), semibold(size), c, w * 0.78);
        let x = if center { r.center().x - g.size().x / 2.0 } else { r.min.x + w * 0.12 };
        p.galley(pos2(x, r.min.y + y * r.height()), g, c);
    };
    match cover {
        Cover::None => {
            line(0.12, 0.12, 0.55, accent, 3.0);
            for i in 0..5 {
                line(0.2 + i as f32 * 0.045, 0.12, 0.88, soft, 1.5);
            }
            line(0.46, 0.12, 0.45, accent, 2.5);
            for i in 0..6 {
                line(0.53 + i as f32 * 0.045, 0.12, if i == 5 { 0.6 } else { 0.88 }, soft, 1.5);
            }
        }
        Cover::Classic => {
            title_at(0.3, w * 0.085, accent, false);
            line(0.42, 0.12, 0.88, accent, 1.5);
            line(0.47, 0.12, 0.6, soft, 2.0);
            line(0.86, 0.12, 0.4, accent, 2.0);
        }
        Cover::Band => {
            p.rect_filled(Rect::from_min_size(r.min, vec2(w, r.height() * 0.42)), CornerRadius { nw: 3, ne: 3, sw: 0, se: 0 }, accent);
            title_at(0.2, w * 0.085, Color32::WHITE, false);
            line(0.34, 0.12, 0.55, Color32::from_white_alpha(170), 2.0);
            line(0.86, 0.12, 0.4, ink, 2.0);
        }
        Cover::Minimal => {
            title_at(0.4, w * 0.075, ink, true);
            line(0.5, 0.36, 0.64, soft, 1.5);
            line(0.88, 0.42, 0.58, soft, 1.5);
        }
        Cover::Modern => {
            p.rect_filled(Rect::from_min_size(pos2(r.min.x + w * 0.08, r.min.y + r.height() * 0.3), vec2(w * 0.025, r.height() * 0.16)), 0.0, accent);
            title_at(0.31, w * 0.095, ink, false);
            line(0.42, 0.12, 0.5, accent, 2.0);
            line(0.86, 0.12, 0.35, ink, 2.0);
        }
    }
}

/// The table-of-contents page of the preview.
fn toc_page(p: &egui::Painter, r: Rect, accent: Color32, levels: u8, t: &Tokens) {
    p.rect_filled(r.translate(vec2(0.0, 2.0)), CornerRadius::same(3), t.page_shadow);
    p.rect_filled(r, CornerRadius::same(3), Color32::WHITE);
    let w = r.width();
    let soft = Color32::from_gray(0xB8);
    p.line_segment(
        [pos2(r.min.x + w * 0.12, r.min.y + r.height() * 0.12), pos2(r.min.x + w * 0.45, r.min.y + r.height() * 0.12)],
        Stroke::new(3.0, accent),
    );
    for (i, level) in [1u8, 2, 3, 2, 1, 2, 3, 3, 1].into_iter().enumerate() {
        if level > levels {
            continue;
        }
        let y = r.min.y + r.height() * (0.22 + i as f32 * 0.07);
        let x0 = r.min.x + w * (0.12 + 0.06 * f32::from(level - 1));
        p.line_segment(
            [pos2(x0, y), pos2(r.min.x + w * 0.62, y)],
            Stroke::new(if level == 1 { 2.0 } else { 1.5 }, if level == 1 { Color32::from_gray(0x55) } else { soft }),
        );
        p.circle_filled(pos2(r.min.x + w * 0.84, y), 1.5, soft);
    }
}

/// The preview: the table-of-contents page fanned behind the cover.
fn preview(ui: &mut Ui, st: &Studio, t: &Tokens) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), ui.available_height()), Sense::hover());
    let accent = rgb(st.accent());
    let pw = (r.width() * 0.62).min(170.0);
    let size = vec2(pw, pw * 1.414);
    let front = Rect::from_center_size(r.center() - vec2(14.0, 10.0), size);
    let p = ui.painter();
    if st.toc {
        toc_page(p, front.translate(vec2(30.0, 22.0)), accent, st.levels, t);
    }
    page(p, front, st.cover, accent, &st.title, t);
}

/// A cover choice: the miniature with its name under it.
fn cover_card(ui: &mut Ui, st: &mut Studio, c: Cover, t: &Tokens) {
    let (r, resp) = ui.allocate_exact_size(vec2(78.0, 124.0), Sense::click());
    let on = st.cover == c;
    let pg = Rect::from_min_size(r.min + vec2(6.0, 6.0), vec2(66.0, 93.0));
    let frame = pg.expand(4.0);
    if on {
        ui.painter().rect_stroke(frame, CornerRadius::same(7), Stroke::new(2.0, t.accent), StrokeKind::Outside);
    } else if resp.hovered() {
        ui.painter().rect_stroke(frame, CornerRadius::same(7), Stroke::new(1.0, t.border_strong), StrokeKind::Outside);
    }
    page(ui.painter(), pg, c, rgb(st.accent()), "", t);
    ui.painter().text(
        pos2(r.center().x, r.max.y - 10.0),
        egui::Align2::CENTER_CENTER,
        c.label(),
        if on { medium(11.5) } else { regular(11.5) },
        if on { t.text } else { t.text_dim },
    );
    if resp.clicked() {
        st.cover = c;
    }
}

/// A theme choice: its six colours, its name and its heading font.
fn theme_card(ui: &mut Ui, st: &mut Studio, i: usize, t: &Tokens) {
    let Some(theme) = THEMES.get(i) else { return };
    let (r, resp) = ui.allocate_exact_size(vec2(148.0, 62.0), Sense::click());
    let on = st.theme == i;
    ui.painter().rect_filled(r, CornerRadius::same(10), if resp.hovered() && !on { t.hover } else { t.input });
    ui.painter().rect_stroke(r, CornerRadius::same(10), if on { Stroke::new(2.0, t.accent) } else { Stroke::new(1.0, t.border) }, StrokeKind::Inside);
    let sw = (r.width() - 24.0) / 6.0;
    for (k, c) in theme.3.iter().enumerate() {
        let cell = Rect::from_min_size(pos2(r.min.x + 12.0 + k as f32 * sw, r.min.y + 12.0), vec2(sw, 14.0));
        let round = match k {
            0 => CornerRadius { nw: 4, sw: 4, ne: 0, se: 0 },
            5 => CornerRadius { ne: 4, se: 4, nw: 0, sw: 0 },
            _ => CornerRadius::ZERO,
        };
        ui.painter().rect_filled(cell, round, rgb(*c));
    }
    ui.painter().text(pos2(r.min.x + 12.0, r.min.y + 34.0), egui::Align2::LEFT_TOP, super::noted(theme.0.to_string()), medium(12.0), t.text);
    ui.painter().text(pos2(r.max.x - 12.0, r.min.y + 35.0), egui::Align2::RIGHT_TOP, super::noted(theme.1.to_string()), regular(10.5), t.text_dim);
    if resp.clicked() {
        st.theme = i;
    }
}

/// Draw the Design window while it is open; returns what Apply asked the agent for, if anything.
pub fn show(app: &mut WordApp, ctx: &egui::Context) -> Option<&'static str> {
    app.ai.studio.as_ref()?;
    let t = Tokens::get(ctx);
    let screen = ctx.content_rect();
    let size = vec2((screen.width() - 48.0).clamp(420.0, 860.0), (screen.height() - 64.0).clamp(360.0, 620.0));
    let mut close = false;
    let mut apply_now = false;
    let modal = egui::Modal::new(egui::Id::new("ai_studio"))
        .frame(egui::Frame::NONE.fill(t.panel).corner_radius(CornerRadius::same(16)).stroke(Stroke::new(1.0, t.border)))
        .show(ctx, |ui| {
            ui.set_min_size(size);
            ui.set_max_size(size);
            let Some(st) = app.ai.studio.as_mut() else { return };
            // Header.
            ui.horizontal(|ui| {
                ui.add_space(22.0);
                ui.vertical(|ui| {
                    ui.add_space(18.0);
                    ui.label(egui::RichText::new(tx("Document design", "Diseño del documento")).font(semibold(18.0)).color(t.text));
                    ui.label(
                        egui::RichText::new(tx("Cover, colours, type and contents in one go.", "Portada, colores, tipografía e índice de una vez."))
                            .font(regular(12.5))
                            .color(t.text_dim),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    ui.add_space(14.0);
                    ui.vertical(|ui| {
                        ui.add_space(14.0);
                        if crate::widgets::icon_button(ui, "close", tx("Close", "Cerrar")).clicked() {
                            close = true;
                        }
                    });
                });
            });
            ui.add_space(12.0);
            let footer_h = 64.0;
            let body_h = ui.available_height() - footer_h;
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                // Preview.
                let preview_w = (size.x * 0.34).clamp(200.0, 300.0);
                let (pr, _) = ui.allocate_exact_size(vec2(preview_w, body_h), Sense::hover());
                ui.painter().rect_filled(pr.shrink2(vec2(16.0, 0.0)), CornerRadius::same(14), t.canvas);
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(pr.shrink2(vec2(16.0, 0.0))));
                preview(&mut child, st, &t);
                // Choices.
                ui.allocate_ui(vec2(size.x - preview_w - 8.0, body_h), |ui| {
                    egui::ScrollArea::vertical().id_salt("ai_studio_scroll").auto_shrink([false, false]).show(ui, |ui| {
                        ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
                        ui.set_max_width(ui.available_width() - 16.0);
                        section(ui, tx("Cover", "Portada"), &t);
                        ui.horizontal_wrapped(|ui| {
                            for c in Cover::ALL {
                                cover_card(ui, st, c, &t);
                            }
                        });
                        if st.cover != Cover::None {
                            ui.add_space(4.0);
                            field(ui, &mut st.title, tx("Title", "Título"), &t);
                            field(ui, &mut st.subtitle, tx("Subtitle", "Subtítulo"), &t);
                            field(ui, &mut st.author, tx("Author", "Autor"), &t);
                        }
                        section(ui, tx("Colours and type", "Colores y tipografía"), &t);
                        ui.horizontal_wrapped(|ui| {
                            for i in 0..THEMES.len() {
                                theme_card(ui, st, i, &t);
                            }
                        });
                        section(ui, tx("Table of contents", "Índice"), &t);
                        ui.horizontal(|ui| {
                            switch(ui, &mut st.toc, tx("Add a table of contents", "Añadir un índice"), &t);
                            if st.toc {
                                ui.add_space(12.0);
                                ui.label(egui::RichText::new(tx("Levels", "Niveles")).font(regular(12.0)).color(t.text_dim));
                                segmented(ui, &mut st.levels, &[(1, "1"), (2, "2"), (3, "3")], &t);
                            }
                        });
                        section(ui, tx("Pages", "Páginas"), &t);
                        segmented(
                            ui,
                            &mut st.numbers,
                            &[
                                (Numbers::None, tx("No numbers", "Sin numerar")),
                                (Numbers::Center, tx("Numbered, centre", "Número centrado")),
                                (Numbers::Right, tx("Numbered, right", "Número a la derecha")),
                            ],
                            &t,
                        );
                        ui.add_space(4.0);
                        field(ui, &mut st.header, tx("Header text (optional)", "Texto del encabezado (opcional)"), &t);
                        section(ui, tx("Spacing", "Espaciado"), &t);
                        segmented(
                            ui,
                            &mut st.spacing,
                            &[
                                (Spacing::Compact, tx("Compact", "Compacto")),
                                (Spacing::Normal, tx("Normal", "Normal")),
                                (Spacing::Open, tx("Airy", "Amplio")),
                            ],
                            &t,
                        );
                        if st.toc {
                            ui.add_space(14.0);
                            switch(
                                ui,
                                &mut st.polish,
                                tx("Let the AI tidy the headings for the contents", "Que la IA ordene los títulos para el índice"),
                                &t,
                            );
                        }
                        ui.add_space(16.0);
                    });
                });
            });
            // Footer.
            let (fr, _) = ui.allocate_exact_size(vec2(size.x, footer_h), Sense::hover());
            ui.painter().line_segment([fr.left_top(), fr.right_top()], Stroke::new(1.0, t.border));
            let mut bar =
                ui.new_child(egui::UiBuilder::new().max_rect(fr.shrink2(vec2(22.0, 14.0))).layout(egui::Layout::right_to_left(egui::Align::Center)));
            if pill_button(&mut bar, tx("Apply design", "Aplicar diseño"), true, &t).clicked() {
                apply_now = true;
            }
            bar.add_space(8.0);
            if pill_button(&mut bar, tx("Cancel", "Cancelar"), false, &t).clicked() {
                close = true;
            }
            if let Some(e) = &st.error {
                bar.add_space(12.0);
                bar.label(egui::RichText::new(e.as_str()).font(regular(12.0)).color(t.red));
            }
        });
    if modal.should_close() {
        close = true;
    }
    let mut ask = None;
    if apply_now {
        match apply(app) {
            Ok(prompt) => {
                ask = prompt;
                close = true;
            }
            Err(e) => {
                if let Some(st) = app.ai.studio.as_mut() {
                    st.error = Some(e);
                }
            }
        }
    }
    if close {
        app.ai.studio = None;
    }
    ask
}

/// A rounded button: filled with the accent (`primary`) or outlined.
pub fn pill_button(ui: &mut Ui, text: &str, primary: bool, t: &Tokens) -> egui::Response {
    let g = ui.painter().layout_no_wrap(text.to_string(), medium(12.5), if primary { t.on_accent } else { t.text });
    let (r, resp) = ui.allocate_exact_size(vec2(g.size().x + 32.0, 34.0), Sense::click());
    let fill = match (primary, resp.hovered()) {
        (true, false) => t.accent,
        (true, true) => t.accent_text,
        (false, true) => t.hover,
        (false, false) => t.panel,
    };
    ui.painter().rect_filled(r, CornerRadius::same(17), fill);
    if !primary {
        ui.painter().rect_stroke(r, CornerRadius::same(17), Stroke::new(1.0, t.border_strong), StrokeKind::Inside);
    }
    ui.painter().galley(r.center() - g.size() / 2.0, g, t.text);
    resp
}

#[cfg(test)]
mod tests {
    use super::*;
    use wordcraft_engine::Session;

    fn studio() -> Studio {
        Studio {
            cover: Cover::Band,
            title: "Plan".into(),
            subtitle: String::new(),
            author: "Ana".into(),
            theme: 1,
            toc: true,
            levels: 2,
            numbers: Numbers::Right,
            header: "Confidencial".into(),
            spacing: Spacing::Open,
            polish: false,
            error: None,
        }
    }

    #[test]
    fn a_design_applies_with_the_documents_own_commands() {
        let mut s = Session::new(wordcraft_engine::sample::sample_document());
        for (id, params) in studio().commands() {
            let r = s.run(id, &params);
            assert!(r.is_ok() || id == "insert.closeHeader", "{id} {params}: {r:?}");
        }
        let text = s.doc.plain_text(wordcraft_doc::StoryRef::Body);
        assert!(text.contains("Plan"), "the cover is there");
        assert_eq!(s.doc.settings.theme_name, THEMES[1].0);
        assert_eq!(s.sel.focus.story, wordcraft_doc::StoryRef::Body, "the caret is back in the body");
    }

    #[test]
    fn only_what_was_chosen_is_applied() {
        let mut st = studio();
        st.cover = Cover::None;
        st.toc = false;
        st.numbers = Numbers::None;
        st.header.clear();
        let ids: Vec<&str> = st.commands().into_iter().map(|(id, _)| id).collect();
        assert_eq!(ids, ["design.theme", "design.paragraphSpacing"]);
        let ids: Vec<&str> = studio().commands().into_iter().map(|(id, _)| id).collect();
        assert_eq!(ids.last(), Some(&"insert.coverPage"), "the cover goes in last, before everything");
        let cover = studio().commands().pop().unwrap().1;
        assert_eq!(cover["accent"], format!("{:02X}{:02X}{:02X}", THEMES[1].3[0].0, THEMES[1].3[0].1, THEMES[1].3[0].2));
        assert!(cover.get("subtitle").is_none(), "an empty field keeps the command's default");
    }
}
