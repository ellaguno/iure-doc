//! La barra de formato de la suite (la de IureEditor 2.0): botones con icono o dibujados, iguales
//! en IureEditor, el redactor de correo y las notas de IureDesk, y las notas de IureProject. Cada
//! app elige qué herramientas lleva ([`Tool`]) y aplica lo pulsado con [`crate::apply_external`].

use egui::{Sense, Ui, vec2};
use iure_doc::edit::{LineKind, Paint};
use iurefficient_ui::icons::{self, Icon};
use iurefficient_ui::lang::pick;
use iurefficient_ui::theme;

use crate::ExternalEdit;

/// Una herramienta de la barra.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Undo,
    Redo,
    /// Títulos 1 a 3.
    Headings,
    Bold,
    Italic,
    Strike,
    /// Subrayado: no es Markdown; quien la pone decide cómo ([`Pressed::Other`]).
    Underline,
    Code,
    TextColor,
    Highlight,
    Bullet,
    Ordered,
    Task,
    Quote,
    CodeBlock,
    Rule,
    Link,
    Table,
    Diagram,
    /// Una separación entre grupos.
    Separator,
}

/// La barra completa (IureEditor).
pub const FULL: &[Tool] = &[
    Tool::Undo,
    Tool::Redo,
    Tool::Separator,
    Tool::Headings,
    Tool::Separator,
    Tool::Bold,
    Tool::Italic,
    Tool::Strike,
    Tool::Code,
    Tool::TextColor,
    Tool::Highlight,
    Tool::Separator,
    Tool::Bullet,
    Tool::Ordered,
    Tool::Task,
    Tool::Separator,
    Tool::Quote,
    Tool::CodeBlock,
    Tool::Rule,
    Tool::Separator,
    Tool::Link,
    Tool::Table,
    Tool::Diagram,
];

/// Lo pulsado en la barra.
#[derive(Clone, Debug, PartialEq)]
pub enum Pressed {
    /// Un cambio para el editor ([`crate::apply_external`]).
    Edit(ExternalEdit),
    /// Una herramienta que resuelve la app (el subrayado).
    Other(Tool),
}

/// Dibuja `tools` en la fila actual (la app pone el `horizontal`/`horizontal_wrapped`). Devuelve
/// lo pulsado.
pub fn tools(ui: &mut Ui, tools: &[Tool]) -> Option<Pressed> {
    use ExternalEdit as E;
    let mut out = None;
    // Los colores llevan su propio menú: se anotan aparte.
    let mut painted = None;
    let mut b = |ui: &mut Ui, f: Fmt<'_>, tip: &str, p: Pressed| {
        if fmt_button(ui, f, tip) {
            out = Some(p);
        }
    };
    let ed = Pressed::Edit;
    for &t in tools {
        match t {
            Tool::Separator => {
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);
            }
            Tool::Undo => b(ui, Fmt::Icon(Icon::Undo), pick("Undo (Ctrl+Z)", "Deshacer (Ctrl+Z)"), ed(E::Undo)),
            Tool::Redo => {
                b(ui, Fmt::Icon(Icon::Redo), pick("Redo (Ctrl+Shift+Z)", "Rehacer (Ctrl+Mayús+Z)"), ed(E::Redo));
            }
            Tool::Headings => {
                for n in 1..=3u8 {
                    let t = ["H1", "H2", "H3"].get(usize::from(n - 1)).copied().unwrap_or("H");
                    let tip = format!("{} {n}", pick("Heading", "Título"));
                    b(ui, Fmt::Text(t, TextStyle::Bold), &tip, ed(E::Line(LineKind::Heading(n))));
                }
            }
            Tool::Bold => b(
                ui,
                Fmt::Text("B", TextStyle::Bold),
                pick("Bold (Ctrl+B)", "Negrita (Ctrl+B)"),
                ed(E::Wrap("**".into())),
            ),
            Tool::Italic => b(
                ui,
                Fmt::Text("I", TextStyle::Italic),
                pick("Italic (Ctrl+I)", "Cursiva (Ctrl+I)"),
                ed(E::Wrap("*".into())),
            ),
            Tool::Strike => {
                b(ui, Fmt::Text("S", TextStyle::Strike), pick("Strikethrough", "Tachado"), ed(E::Wrap("~~".into())));
            }
            Tool::Underline => {
                b(ui, Fmt::Text("U", TextStyle::Underline), pick("Underline", "Subrayado"), Pressed::Other(t));
            }
            Tool::Code => {
                b(
                    ui,
                    Fmt::Text("</>", TextStyle::Mono),
                    pick("Inline code", "Código en línea"),
                    ed(E::Wrap("`".into())),
                );
            }
            Tool::TextColor => {
                if let Some(c) = color_button(ui, Glyph::TextColor, pick("Text color", "Color del texto")) {
                    painted = Some(ed(E::Paint(Paint::Color, c)));
                }
            }
            Tool::Highlight => {
                if let Some(c) = color_button(ui, Glyph::Highlight, pick("Highlight", "Resaltar")) {
                    painted = Some(ed(E::Paint(Paint::Highlight, c)));
                }
            }
            Tool::Bullet => {
                b(ui, Fmt::Icon(Icon::List), pick("Bulleted list", "Lista con viñetas"), ed(E::Line(LineKind::Bullet)));
            }
            Tool::Ordered => b(
                ui,
                Fmt::Text("1.", TextStyle::Plain),
                pick("Numbered list", "Lista numerada"),
                ed(E::Line(LineKind::Ordered)),
            ),
            Tool::Task => {
                b(ui, Fmt::Icon(Icon::Tasks), pick("Task list", "Lista de tareas"), ed(E::Line(LineKind::Task)));
            }
            Tool::Quote => b(ui, Fmt::Draw(Glyph::Quote), pick("Quote", "Cita"), ed(E::Quote)),
            Tool::CodeBlock => b(
                ui,
                Fmt::Text("{ }", TextStyle::Mono),
                pick("Code block", "Bloque de código"),
                ed(E::Insert("\n```\n\n```\n".into())),
            ),
            Tool::Rule => b(
                ui,
                Fmt::Draw(Glyph::Rule),
                pick("Horizontal rule", "Línea horizontal"),
                ed(E::Insert("\n\n---\n\n".into())),
            ),
            Tool::Link => b(ui, Fmt::Icon(Icon::Hyperlink), pick("Link", "Enlace"), ed(E::Link)),
            Tool::Table => b(
                ui,
                Fmt::Draw(Glyph::Table),
                pick("Table", "Tabla"),
                ed(E::Insert("\n\n| Columna 1 | Columna 2 |\n| --- | --- |\n|  |  |\n\n".into())),
            ),
            Tool::Diagram => b(
                ui,
                Fmt::Draw(Glyph::Diagram),
                pick("Diagram (Mermaid)", "Diagrama (Mermaid)"),
                ed(E::Insert("\n\n```mermaid\nflowchart LR\n  A[Inicio] --> B[Fin]\n```\n\n".into())),
            ),
        }
    }
    out.or(painted)
}

/// Botón de la barra de formato: un icono o un texto corto (H1, B, I…).
pub fn fmt_button(ui: &mut Ui, label: Fmt<'_>, tip: &str) -> bool {
    fmt_button_resp(ui, label, tip).clicked()
}

/// Como [`fmt_button`], con su respuesta (para colgarle un menú).
pub fn fmt_button_resp(ui: &mut Ui, label: Fmt<'_>, tip: &str) -> egui::Response {
    let c = theme::colors(ui);
    let (rect, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 5.0, c.hover);
    }
    match label {
        Fmt::Icon(ic) => icons::paint(ui.painter(), rect.shrink(6.0), ic, c.text, c.accent),
        Fmt::Draw(g) => draw_glyph(ui.painter(), rect.shrink(7.0), g, c.text),
        Fmt::Text(t, style) => {
            let size = 13.5;
            let font = match style {
                TextStyle::Bold => theme::bold_font(ui.ctx(), size),
                TextStyle::Mono => egui::FontId::monospace(size - 1.0),
                TextStyle::Plain | TextStyle::Underline | TextStyle::Italic | TextStyle::Strike => {
                    egui::FontId::proportional(size)
                }
            };
            let galley = ui.painter().layout_no_wrap(t.to_string(), font, c.text);
            let pos = rect.center() - galley.size() / 2.0;
            let mut job_galley = galley;
            if style == TextStyle::Italic {
                let mut job = egui::text::LayoutJob::default();
                job.append(
                    t,
                    0.0,
                    egui::TextFormat {
                        font_id: egui::FontId::proportional(size),
                        color: c.text,
                        italics: true,
                        ..Default::default()
                    },
                );
                job_galley = ui.painter().layout_job(job);
            }
            ui.painter().galley(pos, job_galley, c.text);
            if style == TextStyle::Underline {
                let y = rect.center().y + 7.0;
                ui.painter().line_segment(
                    [egui::pos2(rect.center().x - 5.0, y), egui::pos2(rect.center().x + 5.0, y)],
                    egui::Stroke::new(1.2, c.text),
                );
            }
            if style == TextStyle::Strike {
                let y = rect.center().y;
                ui.painter().line_segment(
                    [egui::pos2(rect.center().x - 6.0, y), egui::pos2(rect.center().x + 6.0, y)],
                    egui::Stroke::new(1.2, c.text),
                );
            }
        }
    }
    iurefficient_ui::a11y::button(&resp, tip);
    resp.on_hover_text(tip)
}

/// Los colores de la 1.x (grises, vivos y pastel; 10 por fila).
const PALETTE: [&str; 30] = [
    "#000000", "#434343", "#666666", "#999999", "#B7B7B7", "#CCCCCC", "#D9D9D9", "#EFEFEF", "#F3F3F3", "#FFFFFF",
    "#980000", "#FF0000", "#FF9900", "#FFFF00", "#00FF00", "#00FFFF", "#4A86E8", "#0000FF", "#9900FF", "#FF00FF",
    "#E6B8AF", "#F4CCCC", "#FCE5CD", "#FFF2CC", "#D9EAD3", "#D0E0E3", "#C9DAF8", "#CFE2F3", "#D9D2E9", "#EAD1DC",
];

fn hex_color32(h: &str) -> egui::Color32 {
    let v = u32::from_str_radix(h.trim_start_matches('#'), 16).unwrap_or(0);
    egui::Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// Botón de color (texto o resaltado) con su paleta. Devuelve el color elegido (`Some(None)`: sin
/// color).
pub fn color_button(ui: &mut Ui, glyph: Glyph, tip: &str) -> Option<Option<String>> {
    let resp = fmt_button_resp(ui, Fmt::Draw(glyph), tip);
    let mut out = None;
    egui::Popup::menu(&resp).show(|ui| {
        ui.spacing_mut().item_spacing = vec2(3.0, 3.0);
        egui::Grid::new(("paleta", tip)).spacing(vec2(3.0, 3.0)).show(ui, |ui| {
            for (k, hex) in PALETTE.iter().enumerate() {
                let (r, sw) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
                ui.painter().rect_filled(r, 3.0, hex_color32(hex));
                ui.painter().rect_stroke(
                    r,
                    3.0,
                    egui::Stroke::new(if sw.hovered() { 2.0 } else { 0.5 }, theme::colors(ui).separator),
                    egui::StrokeKind::Inside,
                );
                iurefficient_ui::a11y::button(&sw, hex);
                if sw.on_hover_text(*hex).clicked() {
                    out = Some(Some((*hex).to_string()));
                }
                if k % 10 == 9 {
                    ui.end_row();
                }
            }
        });
        ui.add_space(4.0);
        if ui.button(pick("No color", "Sin color")).clicked() {
            out = Some(None);
        }
    });
    out
}

/// Estilo de un botón de texto (H1, B, I, S, `</>`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TextStyle {
    Plain,
    Bold,
    Underline,
    Italic,
    Strike,
    Mono,
}

/// Lo que se ve en un botón: un icono del kit, un texto corto o un dibujo.
#[derive(Clone, Copy, Debug)]
pub enum Fmt<'a> {
    Icon(Icon),
    Text(&'a str, TextStyle),
    /// Dibujados con líneas (sin depender de los glifos de la letra).
    Draw(Glyph),
}

/// Iconos de la barra que no están en el kit (se dibujan con líneas).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Glyph {
    Quote,
    Table,
    Diagram,
    Rule,
    /// Panel lateral (un recuadro con su columna izquierda).
    Sidebar,
    /// Color del texto: una «A» con una barra de color debajo.
    TextColor,
    /// Resaltado: un marcador con su trazo amarillo.
    Highlight,
    /// Firma: un trazo a mano sobre una línea.
    Signature,
    /// Dictado: un micrófono.
    Microphone,
}

/// Dibuja un icono de la barra de formato que no está en el kit.
pub fn draw_glyph(p: &egui::Painter, r: egui::Rect, g: Glyph, ink: egui::Color32) {
    let s = egui::Stroke::new(1.4, ink);
    let at = |x: f32, y: f32| egui::pos2(r.left() + x * r.width(), r.top() + y * r.height());
    match g {
        Glyph::Quote => {
            p.line_segment([at(0.2, 0.15), at(0.2, 0.85)], egui::Stroke::new(2.4, ink));
            for y in [0.3, 0.5, 0.7] {
                p.line_segment([at(0.38, y), at(0.85, y)], s);
            }
        }
        Glyph::Table => {
            p.rect_stroke(egui::Rect::from_min_max(at(0.1, 0.18), at(0.9, 0.82)), 1.5, s, egui::StrokeKind::Middle);
            p.line_segment([at(0.1, 0.4), at(0.9, 0.4)], s);
            p.line_segment([at(0.1, 0.61), at(0.9, 0.61)], s);
            p.line_segment([at(0.5, 0.18), at(0.5, 0.82)], s);
        }
        Glyph::Diagram => {
            p.rect_stroke(egui::Rect::from_min_max(at(0.05, 0.38), at(0.35, 0.62)), 1.5, s, egui::StrokeKind::Middle);
            p.rect_stroke(egui::Rect::from_min_max(at(0.65, 0.1), at(0.95, 0.34)), 1.5, s, egui::StrokeKind::Middle);
            p.rect_stroke(egui::Rect::from_min_max(at(0.65, 0.66), at(0.95, 0.9)), 1.5, s, egui::StrokeKind::Middle);
            p.line_segment([at(0.35, 0.5), at(0.65, 0.22)], s);
            p.line_segment([at(0.35, 0.5), at(0.65, 0.78)], s);
        }
        Glyph::Rule => {
            p.line_segment([at(0.1, 0.5), at(0.9, 0.5)], egui::Stroke::new(1.8, ink));
        }
        Glyph::TextColor => {
            p.line_segment([at(0.22, 0.72), at(0.5, 0.08)], s);
            p.line_segment([at(0.5, 0.08), at(0.78, 0.72)], s);
            p.line_segment([at(0.33, 0.48), at(0.67, 0.48)], s);
            p.rect_filled(
                egui::Rect::from_min_max(at(0.1, 0.82), at(0.9, 0.98)),
                1.0,
                egui::Color32::from_rgb(0xE5, 0x39, 0x35),
            );
        }
        Glyph::Highlight => {
            p.rect_filled(
                egui::Rect::from_min_max(at(0.05, 0.78), at(0.95, 0.98)),
                1.0,
                egui::Color32::from_rgb(0xFF, 0xD5, 0x4F),
            );
            let body = [at(0.55, 0.05), at(0.9, 0.38), at(0.5, 0.72), at(0.18, 0.4)];
            p.add(egui::Shape::closed_line(body.to_vec(), s));
            p.line_segment([at(0.18, 0.4), at(0.12, 0.7)], s);
            p.line_segment([at(0.12, 0.7), at(0.5, 0.72)], s);
        }
        Glyph::Signature => {
            let pts: Vec<egui::Pos2> = (0..=24)
                .map(|k| {
                    let t = k as f32 / 24.0;
                    let y = 0.5 - 0.22 * (t * std::f32::consts::TAU * 1.5).sin() * (1.0 - 0.4 * t);
                    at(0.1 + 0.8 * t, y)
                })
                .collect();
            p.add(egui::Shape::line(pts, s));
            p.line_segment([at(0.05, 0.88), at(0.95, 0.88)], egui::Stroke::new(1.0, ink));
        }
        Glyph::Microphone => {
            p.rect_stroke(
                egui::Rect::from_min_max(at(0.36, 0.05), at(0.64, 0.6)),
                egui::CornerRadius::same((r.width() * 0.14) as u8),
                s,
                egui::StrokeKind::Middle,
            );
            let arc: Vec<egui::Pos2> = (0..=12)
                .map(|k| {
                    let a = std::f32::consts::PI * k as f32 / 12.0;
                    at(0.5 - 0.27 * a.cos(), 0.42 + 0.3 * a.sin())
                })
                .collect();
            p.add(egui::Shape::line(arc, s));
            p.line_segment([at(0.5, 0.72), at(0.5, 0.92)], s);
            p.line_segment([at(0.34, 0.95), at(0.66, 0.95)], s);
        }
        Glyph::Sidebar => {
            p.rect_stroke(egui::Rect::from_min_max(at(0.08, 0.15), at(0.92, 0.85)), 2.0, s, egui::StrokeKind::Middle);
            p.line_segment([at(0.38, 0.15), at(0.38, 0.85)], s);
        }
    }
}
