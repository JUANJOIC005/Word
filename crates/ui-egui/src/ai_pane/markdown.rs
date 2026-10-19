//! The agent's replies drawn as formatted text: headings, bullet and numbered lists, code blocks,
//! **bold**, *italic* and `code`. Anything else is plain text.

use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, CornerRadius, FontId, Sense, Ui, pos2, vec2};

use crate::theme::{Tokens, regular, semibold};

const SIZE: f32 = 13.0;

#[derive(Debug, PartialEq)]
enum Block<'a> {
    Heading(u8, &'a str),
    Bullet(&'a str),
    Numbered(&'a str, &'a str),
    Code(String),
    Para(String),
    Gap,
}

fn blocks(text: &str) -> Vec<Block<'_>> {
    let mut out = Vec::new();
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            let mut code = Vec::new();
            for l in lines.by_ref() {
                if l.trim_start().starts_with("```") {
                    break;
                }
                code.push(l);
            }
            out.push(Block::Code(code.join("\n")));
        } else if trimmed.is_empty() {
            if !matches!(out.last(), None | Some(Block::Gap)) {
                out.push(Block::Gap);
            }
        } else if let Some(h) = heading(trimmed) {
            out.push(h);
        } else if let Some(rest) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")).or_else(|| trimmed.strip_prefix("• ")) {
            out.push(Block::Bullet(rest));
        } else if let Some((n, rest)) = numbered(trimmed) {
            out.push(Block::Numbered(n, rest));
        } else {
            // Consecutive plain lines are one paragraph.
            let mut para = trimmed.to_string();
            while let Some(next) = lines.peek() {
                let n = next.trim_start();
                if n.is_empty() || n.starts_with("```") || heading(n).is_some() || n.starts_with("- ") || n.starts_with("* ") || numbered(n).is_some()
                {
                    break;
                }
                para.push('\n');
                para.push_str(n);
                lines.next();
            }
            out.push(Block::Para(para));
        }
    }
    while matches!(out.last(), Some(Block::Gap)) {
        out.pop();
    }
    out
}

fn heading(line: &str) -> Option<Block<'_>> {
    let level = line.bytes().take_while(|b| *b == b'#').count();
    let rest = line.get(level..)?.strip_prefix(' ')?;
    (1..=6).contains(&level).then_some(Block::Heading(level.min(3) as u8, rest))
}

fn numbered(line: &str) -> Option<(&str, &str)> {
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || digits > 3 {
        return None;
    }
    let rest = line.get(digits..)?.strip_prefix(". ")?;
    Some((line.get(..digits)?, rest))
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Span {
    Plain,
    Bold,
    Italic,
    Code,
}

/// Inline spans: `**bold**`, `*italic*` and `` `code` ``; a mark with no partner is plain text.
fn spans(text: &str) -> Vec<(Span, &str)> {
    let mut out = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let next = ["**", "`", "*"].iter().filter_map(|m| rest.find(*m).map(|i| (i, *m))).min_by_key(|(i, m)| (*i, std::cmp::Reverse(m.len())));
        let Some((at, mark)) = next else {
            out.push((Span::Plain, rest));
            break;
        };
        let after = &rest[at + mark.len()..];
        let Some(end) = after.find(mark).filter(|e| *e > 0) else {
            out.push((Span::Plain, &rest[..at + mark.len()]));
            rest = after;
            continue;
        };
        if at > 0 {
            out.push((Span::Plain, &rest[..at]));
        }
        let kind = match mark {
            "**" => Span::Bold,
            "`" => Span::Code,
            _ => Span::Italic,
        };
        out.push((kind, &after[..end]));
        rest = &after[end + mark.len()..];
    }
    out
}

fn inline(job: &mut LayoutJob, text: &str, font: &FontId, color: Color32, t: &Tokens) {
    for (kind, s) in spans(text) {
        let fmt = match kind {
            Span::Plain => TextFormat { font_id: font.clone(), color, ..Default::default() },
            Span::Bold => TextFormat { font_id: semibold(font.size), color, ..Default::default() },
            Span::Code => TextFormat { font_id: FontId::monospace(font.size - 1.0), color, background: t.hover, ..Default::default() },
            Span::Italic => TextFormat { font_id: font.clone(), color, italics: true, ..Default::default() },
        };
        job.append(s, 0.0, fmt);
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn inline_html(text: &str) -> String {
    spans(text)
        .into_iter()
        .map(|(kind, s)| match kind {
            Span::Plain => esc(s).replace('\n', "<br>"),
            Span::Bold => format!("<b>{}</b>", esc(s)),
            Span::Italic => format!("<i>{}</i>", esc(s)),
            Span::Code => format!("<code>{}</code>", esc(s)),
        })
        .collect()
}

/// The reply as HTML, for pasting into the document with real headings and lists.
pub fn to_html(text: &str) -> String {
    let mut out = String::new();
    let mut list: Option<&str> = None;
    for b in blocks(text) {
        let want = match b {
            Block::Bullet(_) => Some("ul"),
            Block::Numbered(..) => Some("ol"),
            Block::Gap => list,
            _ => None,
        };
        if list != want {
            if let Some(l) = list {
                out.push_str(&format!("</{l}>"));
            }
            if let Some(l) = want {
                out.push_str(&format!("<{l}>"));
            }
            list = want;
        }
        match b {
            Block::Heading(level, s) => out.push_str(&format!("<h{level}>{}</h{level}>", inline_html(s))),
            Block::Bullet(s) | Block::Numbered(_, s) => out.push_str(&format!("<li>{}</li>", inline_html(s))),
            Block::Code(c) => out.push_str(&format!("<pre>{}</pre>", esc(&c))),
            Block::Para(s) => out.push_str(&format!("<p>{}</p>", inline_html(&s))),
            Block::Gap => {}
        }
    }
    if let Some(l) = list {
        out.push_str(&format!("</{l}>"));
    }
    out
}

fn paragraph(ui: &mut Ui, text: &str, font: FontId, color: Color32, t: &Tokens, indent: f32, marker: Option<&str>) {
    let width = (ui.available_width() - indent).max(40.0);
    let mut job = LayoutJob::default();
    inline(&mut job, text, &font, color, t);
    job.wrap.max_width = width;
    let galley = ui.painter().layout_job(job);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), galley.size().y), Sense::hover());
    if let Some(m) = marker {
        ui.painter().text(pos2(r.min.x + 2.0, r.min.y), egui::Align2::LEFT_TOP, m, font.clone(), t.text_dim);
    }
    ui.painter().galley(pos2(r.min.x + indent, r.min.y), galley, color);
}

/// Draw `text` at the available width.
pub fn show(ui: &mut Ui, text: &str, t: &Tokens) {
    ui.spacing_mut().item_spacing.y = 4.0;
    for b in blocks(text) {
        match b {
            Block::Heading(level, s) => {
                ui.add_space(2.0);
                let size = match level {
                    1 => 16.0,
                    2 => 14.5,
                    _ => 13.5,
                };
                paragraph(ui, s, semibold(size), t.text, t, 0.0, None);
            }
            Block::Bullet(s) => paragraph(ui, s, regular(SIZE), t.text, t, 16.0, Some("•")),
            Block::Numbered(n, s) => paragraph(ui, s, regular(SIZE), t.text, t, 20.0, Some(&format!("{n}."))),
            Block::Para(s) => paragraph(ui, &s, regular(SIZE), t.text, t, 0.0, None),
            Block::Code(code) => {
                let galley = ui.painter().layout(code, FontId::monospace(11.5), t.text, ui.available_width() - 20.0);
                let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), galley.size().y + 16.0), Sense::hover());
                ui.painter().rect_filled(r, CornerRadius::same(8), t.hover);
                ui.painter().galley(pos2(r.min.x + 10.0, r.min.y + 8.0), galley, t.text);
            }
            Block::Gap => ui.add_space(4.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replies_split_into_blocks() {
        let b = blocks("## Hecho\n\nHe añadido:\n- una portada\n- un índice\n\n1. Revisa\n2. Guarda\n```\nx\n```\n");
        assert_eq!(
            b,
            vec![
                Block::Heading(2, "Hecho"),
                Block::Gap,
                Block::Para("He añadido:".into()),
                Block::Bullet("una portada"),
                Block::Bullet("un índice"),
                Block::Gap,
                Block::Numbered("1", "Revisa"),
                Block::Numbered("2", "Guarda"),
                Block::Code("x".into()),
            ]
        );
        assert_eq!(blocks("#hashtag"), vec![Block::Para("#hashtag".into())]);
    }

    #[test]
    fn inline_marks_become_spans_and_stray_ones_stay_text() {
        assert_eq!(
            spans("Un **título** y `code` y 2 * 3"),
            [(Span::Plain, "Un "), (Span::Bold, "título"), (Span::Plain, " y "), (Span::Code, "code"), (Span::Plain, " y 2 *"), (Span::Plain, " 3")]
        );
        let t = Tokens::light();
        let mut job = LayoutJob::default();
        inline(&mut job, "Un **título** y *nota*", &regular(13.0), t.text, &t);
        assert_eq!(job.text, "Un título y nota");
    }

    #[test]
    fn replies_paste_as_html_with_real_structure() {
        assert_eq!(
            to_html("## Plan\n\nUn **gran** <paso>:\n- uno\n- dos\n\n1. tres"),
            "<h2>Plan</h2><p>Un <b>gran</b> &lt;paso&gt;:</p><ul><li>uno</li><li>dos</li></ul><ol><li>tres</li></ol>"
        );
    }
}
