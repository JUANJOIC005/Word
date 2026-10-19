//! How the AI designs documents: a catalogue of design directions (cover, type, colours, heading
//! style, spacing and finishing touches), palettes and font pairs, and the user's own choices
//! (Colours and style, in the pane). Each new document or redesign gets a direction it has not
//! used lately and a fresh palette, so no two designs come out alike; the user's colours and
//! fonts, when set, are kept in every one.

use std::path::Path;

use egui::{Color32, CornerRadius, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use serde::{Deserialize, Serialize};

use super::agent::Model;
use super::{tx, txf};
use crate::WordApp;
use crate::theme::{Tokens, medium, regular, semibold};

/// A palette: its name and four colours (main, second, highlight, contrast).
pub struct Palette {
    pub en: &'static str,
    pub es: &'static str,
    pub colors: [&'static str; 4],
}

pub const PALETTES: [Palette; 10] = [
    Palette { en: "Ocean", es: "Océano", colors: ["1D4E89", "00B2CA", "7DCFB6", "F79256"] },
    Palette { en: "Forest", es: "Bosque", colors: ["2D6A4F", "40916C", "95D5B2", "D4A373"] },
    Palette { en: "Sunset", es: "Atardecer", colors: ["9D0208", "DC2F02", "F48C06", "FFBA08"] },
    Palette { en: "Ink", es: "Tinta", colors: ["1B263B", "415A77", "778DA9", "E0A458"] },
    Palette { en: "Lavender", es: "Lavanda", colors: ["5A189A", "7B2CBF", "C77DFF", "F4A261"] },
    Palette { en: "Terracotta", es: "Terracota", colors: ["9C4221", "C05621", "DD6B20", "2C7A7B"] },
    Palette { en: "Graphite", es: "Grafito", colors: ["212529", "495057", "ADB5BD", "E63946"] },
    Palette { en: "Mint", es: "Menta", colors: ["006D77", "83C5BE", "B8E0D2", "E29578"] },
    Palette { en: "Wine", es: "Vino", colors: ["6A040F", "9D0208", "D4A373", "264653"] },
    Palette { en: "Cobalt", es: "Cobalto", colors: ["003F88", "00509D", "FDC500", "5C677D"] },
];

impl Palette {
    pub fn name(&self) -> &'static str {
        tx(self.en, self.es)
    }
}

/// A font pair: its name, the headings' font and the body's.
pub struct Fonts {
    pub en: &'static str,
    pub es: &'static str,
    pub heading: &'static str,
    pub body: &'static str,
}

pub const FONTS: [Fonts; 7] = [
    Fonts { en: "Modern sans", es: "Sans moderna", heading: "Avenir Next", body: "Avenir Next" },
    Fonts { en: "Classic serif", es: "Serif clásica", heading: "Georgia", body: "Georgia" },
    Fonts { en: "Elegant", es: "Elegante", heading: "Palatino", body: "Palatino" },
    Fonts { en: "Swiss", es: "Suiza", heading: "Helvetica Neue", body: "Helvetica Neue" },
    Fonts { en: "Geometric", es: "Geométrica", heading: "Futura", body: "Avenir Next" },
    Fonts { en: "Humanist", es: "Humanista", heading: "Gill Sans", body: "Gill Sans" },
    Fonts { en: "Contrast", es: "Contraste", heading: "Georgia", body: "Helvetica Neue" },
];

impl Fonts {
    pub fn name(&self) -> &'static str {
        tx(self.en, self.es)
    }
}

/// A design direction the AI follows.
#[derive(Debug, PartialEq, Eq)]
pub struct Direction {
    pub id: &'static str,
    pub en: &'static str,
    pub es: &'static str,
    /// `insert.coverPage` style.
    pub cover: &'static str,
    /// `design.styleSet` name.
    pub style_set: &'static str,
    /// `design.paragraphSpacing` value.
    pub spacing: &'static str,
    /// Index into [`FONTS`].
    pub fonts: usize,
    /// What makes it this design, beyond the commands.
    pub touches: &'static str,
}

impl Direction {
    pub fn name(&self) -> &'static str {
        tx(self.en, self.es)
    }
}

pub const DIRECTIONS: [Direction; 10] = [
    Direction {
        id: "editorial",
        en: "Editorial",
        es: "Editorial",
        cover: "editorial",
        style_set: "lines",
        spacing: "open",
        fonts: 6,
        touches: "magazine feel: an opening paragraph in a larger size, one or two pull quotes in the main colour with a left border, \
short sections with strong headings",
    },
    Direction {
        id: "corporate",
        en: "Corporate",
        es: "Corporativo",
        cover: "corporate",
        style_set: "basic",
        spacing: "default",
        fonts: 3,
        touches: "business report: an executive summary in a box shaded in a very light tint of the main colour, key figures in a \
table whose header row is shaded in the main colour with white text, clear numbered sections",
    },
    Direction {
        id: "academic",
        en: "Academic",
        es: "Académico",
        cover: "academic",
        style_set: "title",
        spacing: "relaxed",
        fonts: 1,
        touches: "paper or thesis: justified body text, numbered headings (1, 1.1), an abstract in italics, tables and figures with \
captions, references at the end",
    },
    Direction {
        id: "bold",
        en: "Bold",
        es: "Audaz",
        cover: "bold",
        style_set: "shaded",
        spacing: "open",
        fonts: 4,
        touches: "high contrast: Heading 1 as a full-width bar shaded in the main colour, big numbers or key phrases in the highlight \
colour, callouts shaded in a light tint",
    },
    Direction {
        id: "elegant",
        en: "Elegant",
        es: "Elegante",
        cover: "elegant",
        style_set: "minimalist",
        spacing: "relaxed",
        fonts: 2,
        touches: "refined: centred headings in spaced capitals, thin rules between sections, generous white space, an italic \
epigraph or quote",
    },
    Direction {
        id: "sidebar",
        en: "Sidebar",
        es: "Lateral",
        cover: "sidebar",
        style_set: "lines",
        spacing: "default",
        fonts: 5,
        touches: "guide or handbook: key paragraphs marked with a thick left border in the main colour, tips in shaded boxes, \
tables with a shaded header row",
    },
    Direction {
        id: "band",
        en: "Band",
        es: "Banda",
        cover: "band",
        style_set: "default",
        spacing: "open",
        fonts: 0,
        touches: "clean and friendly: headings in the main colour, short paragraphs, bulleted key points, tables with alternating \
light rows",
    },
    Direction {
        id: "modern",
        en: "Modern",
        es: "Moderno",
        cover: "modern",
        style_set: "minimalist",
        spacing: "open",
        fonts: 4,
        touches: "minimal tech look: headings in capitals with letter spacing, lots of air, highlights in the second colour, a \
summary of key points in a shaded box at the start",
    },
    Direction {
        id: "classic",
        en: "Classic",
        es: "Clásico",
        cover: "classic",
        style_set: "title",
        spacing: "default",
        fonts: 1,
        touches: "timeless: serif type, headings in the main colour with a rule under Heading 1, a first paragraph without \
indent, quotes in italics",
    },
    Direction {
        id: "minimal",
        en: "Minimal",
        es: "Minimalista",
        cover: "minimal",
        style_set: "minimalist",
        spacing: "open",
        fonts: 3,
        touches: "very sober: black and grey with the main colour only on small details, no shaded boxes, thin table borders",
    },
];

/// Which colours the AI uses.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Colors {
    /// A different palette for each design.
    #[default]
    Auto,
    /// Index into [`PALETTES`].
    Preset(usize),
    /// The user's own colours, `RRGGBB`.
    Custom(Vec<String>),
}

/// The user's AI choices, kept between runs in `<settings>/ai/settings.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub colors: Colors,
    /// Index into [`FONTS`]; none lets each design choose.
    pub fonts: Option<usize>,
    pub model: Model,
    pub track_changes: bool,
    /// The latest directions used, newest last, so the next design is a different one.
    pub recent: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { colors: Colors::Auto, fonts: None, model: Model::Auto, track_changes: false, recent: Vec::new() }
    }
}

/// How many recent directions a new design avoids.
const AVOID: usize = 5;

pub fn load(dir: Option<&Path>) -> Settings {
    dir.and_then(|d| std::fs::read_to_string(d.join("settings.json")).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn save(dir: Option<&Path>, s: &Settings) {
    let Some(dir) = dir else { return };
    if std::fs::create_dir_all(dir).is_ok()
        && let Ok(text) = serde_json::to_string(s)
    {
        let _ = std::fs::write(dir.join("settings.json"), text);
    }
}

/// A number that changes from one call to the next (enough to vary designs).
fn roll() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let mut x = (crate::now_ms() as u64) ^ N.fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed);
    // xorshift64*
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    x.wrapping_mul(0x2545_F491_4F6C_DD1D)
}

/// A direction different from `current` and from the latest ones used.
pub fn pick(s: &Settings, current: Option<&str>, n: u64) -> &'static Direction {
    let fresh: Vec<&'static Direction> =
        DIRECTIONS.iter().filter(|d| Some(d.id) != current && !s.recent.iter().rev().take(AVOID).any(|r| r == d.id)).collect();
    let pool: Vec<&'static Direction> = if fresh.is_empty() { DIRECTIONS.iter().filter(|d| Some(d.id) != current).collect() } else { fresh };
    let i = usize::try_from(n % pool.len().max(1) as u64).unwrap_or(0);
    pool.get(i).copied().unwrap_or(&DIRECTIONS[0])
}

/// Choose the next design and remember it as used.
pub fn next(s: &mut Settings, current: Option<&str>) -> &'static Direction {
    let d = pick(s, current, roll());
    s.recent.retain(|r| r != d.id);
    s.recent.push(d.id.to_string());
    let extra = s.recent.len().saturating_sub(DIRECTIONS.len());
    s.recent.drain(..extra);
    d
}

pub fn direction(id: &str) -> Option<&'static Direction> {
    DIRECTIONS.iter().find(|d| d.id == id)
}

/// The colours for a design: the user's, or a palette that varies with `n`.
fn colors(s: &Settings, n: u64) -> (Vec<String>, bool) {
    match &s.colors {
        Colors::Custom(c) if !c.is_empty() => (c.clone(), true),
        Colors::Preset(i) if *i < PALETTES.len() => (PALETTES[*i].colors.iter().map(|c| (*c).to_string()).collect(), true),
        _ => {
            let i = usize::try_from(n % PALETTES.len() as u64).unwrap_or(0);
            (PALETTES[i].colors.iter().map(|c| (*c).to_string()).collect(), false)
        }
    }
}

/// What the agent is told about the design to use: the direction, the user's or a fresh palette
/// and the fonts, with the commands that apply them.
pub fn brief(s: &Settings, d: &Direction) -> String {
    brief_with(s, d, roll())
}

fn brief_with(s: &Settings, d: &Direction, n: u64) -> String {
    let (cols, fixed) = colors(s, n);
    let f = s.fonts.and_then(|i| FONTS.get(i)).unwrap_or(&FONTS[d.fonts.min(FONTS.len() - 1)]);
    let list = cols.iter().map(|c| format!("\"{c}\"")).collect::<Vec<_>>().join(", ");
    let only = if fixed { " The user chose these colours: use only them (plus black, white and greys)." } else { "" };
    format!(
        "Design brief \"{name}\" — apply it whenever you create, write or design the document, without being asked:\n\
- execute design.themeFonts {{\"heading\": \"{h}\", \"body\": \"{b}\"}}, design.themeColors {{\"colors\": [{list}]}}, \
design.styleSet {{\"name\": \"{set}\"}}, design.paragraphSpacing {{\"value\": \"{sp}\"}}\n\
- cover: insert.coverPage {{\"style\": \"{cover}\", \"accent\": \"{accent}\", \"title\", \"subtitle\", \"author\"}} with the real title and a fitting subtitle\n\
- character: {touches}.{only}",
        name = d.id,
        h = f.heading,
        b = f.body,
        set = d.style_set,
        sp = d.spacing,
        cover = d.cover,
        accent = cols.first().map_or("1D4E89", String::as_str),
        touches = d.touches,
    )
}

/// What "Design with AI" asks for.
pub const DESIGN: &str = "Give this document a complete, professional design following the design brief. Keep every word of its \
content. If it already starts with a cover page, replace that cover instead of adding a second one. Apply fonts, colours, style set \
and spacing; make sure every section title uses a real Heading style; add the cover at the start; if there are three or more \
sections, add a table of contents on its own page after the cover (references.toc {\"levels\": 2}); add page numbers \
(insert.pageNumber, then insert.closeHeader); style tables and key passages with the design's character. If the document is empty, \
tell the user in one sentence that you need content or a topic first.";

fn rgb(hex: &str) -> Color32 {
    wordcraft_doc::props::Rgb::parse(hex).map_or(Color32::GRAY, |c| Color32::from_rgb(c.0, c.1, c.2))
}

fn hex(c: [u8; 3]) -> String {
    format!("{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

/// A row of colour swatches in `r`, rounded at the ends.
fn swatches(p: &egui::Painter, r: Rect, colors: &[Color32]) {
    let n = colors.len().max(1);
    let w = r.width() / n as f32;
    for (k, c) in colors.iter().enumerate() {
        let cell = Rect::from_min_size(pos2(r.min.x + k as f32 * w, r.min.y), vec2(w, r.height()));
        let round = match (k == 0, k + 1 == n) {
            (true, true) => CornerRadius::same(5),
            (true, false) => CornerRadius { nw: 5, sw: 5, ne: 0, se: 0 },
            (false, true) => CornerRadius { ne: 5, se: 5, nw: 0, sw: 0 },
            _ => CornerRadius::ZERO,
        };
        p.rect_filled(cell, round, *c);
    }
}

/// A selectable card: a frame that lights up when chosen.
fn card(ui: &mut Ui, size: egui::Vec2, on: bool, t: &Tokens) -> (Rect, egui::Response) {
    let (r, resp) = ui.allocate_exact_size(size, Sense::click());
    let k = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.12);
    let fill =
        if on { super::chat::mix(t.panel, t.accent, if t.dark { 0.16 } else { 0.07 }) } else { super::chat::mix(t.panel, t.hover, 0.5 + 0.5 * k) };
    ui.painter().rect_filled(r, CornerRadius::same(12), fill);
    let stroke = if on { Stroke::new(2.0, t.accent) } else { Stroke::new(1.0, super::chat::mix(t.border, t.border_strong, k)) };
    ui.painter().rect_stroke(r, CornerRadius::same(12), stroke, StrokeKind::Inside);
    (r, resp)
}

fn heading(ui: &mut Ui, title: &str, hint: &str, t: &Tokens) {
    ui.add_space(14.0);
    ui.label(egui::RichText::new(title).font(semibold(13.5)).color(t.text));
    ui.label(egui::RichText::new(hint).font(regular(11.5)).color(t.text_dim));
    ui.add_space(8.0);
}

/// Colours and style: the palette, the fonts and how the assistant works, in the pane.
pub fn settings(app: &mut WordApp, ui: &mut Ui, t: &Tokens) {
    let before = app.ai.settings.clone();
    let s = &mut app.ai.settings;
    let w = ui.available_width();
    let col = ((w - 10.0) / 2.0).floor();
    ui.add_space(6.0);
    ui.label(egui::RichText::new(tx("Colours and style", "Colores y estilo")).font(semibold(17.0)).color(t.text));
    ui.label(
        egui::RichText::new(tx(
            "Every design the AI makes is different. Choose here what it must keep.",
            "Cada diseño que hace la IA es distinto. Elige aquí lo que debe respetar.",
        ))
        .font(regular(12.0))
        .color(t.text_dim),
    );

    heading(
        ui,
        tx("Document colours", "Colores del documento"),
        tx("Used on covers, headings, tables and highlights.", "Se usan en portadas, títulos, tablas y destacados."),
        t,
    );
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(10.0, 10.0);
        // Automatic: a fan of palettes.
        let (r, resp) = card(ui, vec2(col, 64.0), s.colors == Colors::Auto, t);
        let strip = Rect::from_min_size(r.min + vec2(12.0, 12.0), vec2(r.width() - 24.0, 14.0));
        let mixed: Vec<Color32> = PALETTES.iter().take(6).map(|p| rgb(p.colors[0])).collect();
        swatches(ui.painter(), strip, &mixed);
        ui.painter().text(pos2(r.min.x + 12.0, r.min.y + 34.0), egui::Align2::LEFT_TOP, tx("Automatic", "Automático"), medium(12.0), t.text);
        ui.painter().text(
            pos2(r.min.x + 12.0, r.min.y + 49.0),
            egui::Align2::LEFT_CENTER,
            tx("A new palette each time", "Una paleta nueva cada vez"),
            regular(10.5),
            t.text_dim,
        );
        if resp.clicked() {
            s.colors = Colors::Auto;
        }
        for (i, p) in PALETTES.iter().enumerate() {
            let (r, resp) = card(ui, vec2(col, 54.0), s.colors == Colors::Preset(i), t);
            let strip = Rect::from_min_size(r.min + vec2(12.0, 12.0), vec2(r.width() - 24.0, 14.0));
            swatches(ui.painter(), strip, &p.colors.map(rgb));
            ui.painter().text(pos2(r.min.x + 12.0, r.min.y + 34.0), egui::Align2::LEFT_TOP, p.name(), medium(12.0), t.text);
            if resp.clicked() {
                s.colors = Colors::Preset(i);
            }
        }
    });
    // Your own colours.
    ui.add_space(10.0);
    let custom = matches!(s.colors, Colors::Custom(_));
    let (r, resp) = card(ui, vec2(w, 58.0), custom, t);
    ui.painter().text(pos2(r.min.x + 12.0, r.min.y + 12.0), egui::Align2::LEFT_TOP, tx("My colours", "Mis colores"), medium(12.0), t.text);
    ui.painter().text(
        pos2(r.min.x + 12.0, r.min.y + 30.0),
        egui::Align2::LEFT_TOP,
        tx("Main, second and highlight", "Principal, secundario y destacado"),
        regular(10.5),
        t.text_dim,
    );
    if resp.clicked() && !custom {
        s.colors = Colors::Custom(PALETTES[0].colors.iter().take(3).map(|c| (*c).to_string()).collect());
    }
    if let Colors::Custom(list) = &mut s.colors {
        while list.len() < 3 {
            list.push("888888".into());
        }
        let mut pick = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_max(pos2(r.max.x - 132.0, r.min.y), pos2(r.max.x - 10.0, r.max.y)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        pick.spacing_mut().interact_size = vec2(32.0, 26.0);
        for c in list.iter_mut().take(3) {
            let mut v = wordcraft_doc::props::Rgb::parse(c).map_or([0x88; 3], |x| [x.0, x.1, x.2]);
            if egui::color_picker::color_edit_button_srgb(&mut pick, &mut v).changed() {
                *c = hex(v);
            }
        }
    }

    heading(ui, tx("Typefaces", "Tipografía"), tx("Automatic lets each design choose its own.", "En automático, cada diseño elige la suya."), t);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        let mut chosen = s.fonts;
        if chip(ui, tx("Automatic", "Automática"), None, chosen.is_none(), t).clicked() {
            chosen = None;
        }
        for (i, f) in FONTS.iter().enumerate() {
            if chip(ui, f.name(), Some(f.heading), chosen == Some(i), t).clicked() {
                chosen = Some(i);
            }
        }
        s.fonts = chosen;
    });

    heading(
        ui,
        tx("Designs", "Diseños"),
        tx(
            "The AI rotates through these styles so documents don't repeat.",
            "La IA alterna entre estos estilos para que los documentos no se repitan.",
        ),
        t,
    );
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        for d in &DIRECTIONS {
            tag(ui, d.name(), t);
        }
    });

    heading(ui, tx("Changes", "Cambios"), tx("How its edits reach the document.", "Cómo llegan sus cambios al documento."), t);
    switch(ui, &mut s.track_changes, tx("Track its changes so I can review them", "Marcar sus cambios para revisarlos"), t);
    ui.add_space(18.0);
    if app.ai.settings != before {
        save(app.ai.store_dir.as_deref(), &app.ai.settings);
    }
}

/// A rounded choice with an optional second line.
fn chip(ui: &mut Ui, text: &str, sub: Option<&str>, on: bool, t: &Tokens) -> egui::Response {
    let g = ui.painter().layout_no_wrap(text.to_string(), medium(12.0), t.text);
    let sg = sub.map(|s| ui.painter().layout_no_wrap(super::noted(s.to_string()), regular(10.0), t.text_dim));
    let w = g.size().x.max(sg.as_ref().map_or(0.0, |s| s.size().x)) + 26.0;
    let h = if sg.is_some() { 42.0 } else { 32.0 };
    let (r, resp) = card(ui, vec2(w, h), on, t);
    let top = if sg.is_some() { r.min.y + 7.0 } else { r.center().y - g.size().y / 2.0 };
    ui.painter().galley(pos2(r.min.x + 13.0, top), g, t.text);
    if let Some(sg) = sg {
        ui.painter().galley(pos2(r.min.x + 13.0, r.min.y + 24.0), sg, t.text_dim);
    }
    resp
}

/// A quiet label in a pill.
fn tag(ui: &mut Ui, text: &str, t: &Tokens) {
    let g = ui.painter().layout_no_wrap(text.to_string(), regular(11.0), t.text_dim);
    let (r, _) = ui.allocate_exact_size(vec2(g.size().x + 18.0, 22.0), Sense::hover());
    ui.painter().rect_filled(r, CornerRadius::same(11), t.hover);
    ui.painter().galley(pos2(r.min.x + 9.0, r.center().y - g.size().y / 2.0), g, t.text_dim);
}

/// An on/off switch with its label.
pub fn switch(ui: &mut Ui, on: &mut bool, label: &str, t: &Tokens) -> egui::Response {
    let text = ui.painter().layout(label.to_string(), regular(12.5), t.text, ui.available_width() - 50.0);
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), text.size().y.max(24.0)), Sense::click());
    if resp.clicked() {
        *on = !*on;
    }
    let k = ui.ctx().animate_bool(resp.id, *on);
    let track = Rect::from_min_size(pos2(r.min.x, r.center().y - 9.0), vec2(34.0, 18.0));
    ui.painter().rect_filled(track, CornerRadius::same(9), if *on { t.accent } else { t.border_strong });
    ui.painter().circle_filled(pos2(track.min.x + 9.0 + k * 16.0, track.center().y), 7.0, Color32::WHITE);
    ui.painter().galley(pos2(track.max.x + 10.0, r.center().y - text.size().y / 2.0), text, t.text);
    resp
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

/// What the chat shows for a design request.
pub fn shown(d: &Direction) -> String {
    txf("Design the document · {name} style", "Diseña el documento · estilo {name}", d.name())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wordcraft_engine::Session;

    #[test]
    fn designs_do_not_repeat_until_all_have_been_used() {
        let mut s = Settings::default();
        let mut seen = Vec::new();
        let mut current: Option<&str> = None;
        for _ in 0..AVOID + 1 {
            let d = next(&mut s, current);
            assert!(!seen.contains(&d.id), "{} came back too soon: {seen:?}", d.id);
            seen.push(d.id);
            current = Some(d.id);
        }
        for n in 0..50 {
            assert_ne!(pick(&s, Some("bold"), n).id, "bold", "never the current one");
        }
    }

    #[test]
    fn the_brief_keeps_the_users_colours_and_fonts() {
        let mut s = Settings { colors: Colors::Custom(vec!["112233".into(), "445566".into()]), fonts: Some(2), ..Default::default() };
        let b = brief_with(&s, &DIRECTIONS[0], 7);
        assert!(b.contains("\"colors\": [\"112233\", \"445566\"]") && b.contains("use only them"), "{b}");
        assert!(b.contains("\"heading\": \"Palatino\"") && b.contains("\"style\": \"editorial\"") && b.contains("\"accent\": \"112233\""), "{b}");
        s.colors = Colors::Auto;
        s.fonts = None;
        let a = brief_with(&s, &DIRECTIONS[1], 0);
        let b = brief_with(&s, &DIRECTIONS[1], 1);
        assert_ne!(a, b, "automatic colours change from one design to the next");
        assert!(!a.contains("use only them"));
    }

    /// Every design applies with the document's own commands, and every cover fits one page.
    #[test]
    fn every_design_applies_and_every_cover_is_one_page() {
        for d in &DIRECTIONS {
            assert!(wordcraft_engine::cmd::insert::COVER_STYLES.contains(&d.cover), "{}", d.id);
            let mut s = Session::new(wordcraft_doc::Document::new());
            s.run("text.insert", &json!({"text": "Body"})).unwrap();
            let f = &FONTS[d.fonts];
            s.run("design.themeFonts", &json!({"heading": f.heading, "body": f.body})).unwrap();
            s.run("design.themeColors", &json!({"colors": PALETTES[3].colors})).unwrap();
            s.run("design.styleSet", &json!({"name": d.style_set})).unwrap();
            s.run("design.paragraphSpacing", &json!({"value": d.spacing})).unwrap();
            s.run(
                "insert.coverPage",
                &json!({"style": d.cover, "title": "Plan anual de la empresa", "subtitle": "2027", "author": "Ana", "accent": "1D4E89"}),
            )
            .unwrap();
            assert_eq!(s.layout().pages.len(), 2, "{}: the cover is one page and the body starts on the next", d.id);
            assert!(s.doc.plain_text(wordcraft_doc::StoryRef::Body).contains("Body"));
        }
    }

    #[test]
    fn settings_come_back() {
        let dir = std::env::temp_dir().join(format!("word-ai-settings-{}", std::process::id()));
        let s = Settings { colors: Colors::Preset(3), fonts: Some(1), track_changes: true, recent: vec!["bold".into()], ..Default::default() };
        save(Some(&dir), &s);
        assert_eq!(load(Some(&dir)), s);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
