//! S1.12: `DocEditor`, el editor de documentos de la suite para egui (primera entrega).
//!
//! **Markdown con estilo al teclear**, como el modo en vivo de Obsidian: el texto es Markdown y se
//! ve con su estilo mientras se escribe (títulos grandes, negrita, cursiva, código en
//! monoespaciado, enlaces en color de acento, listas y casillas resaltadas). Las marcas (`#`,
//! `**`, `` ` ``, `[`…`](url)`) se ven tenues: borrar una marca deshace la conversión, sin modo
//! especial.
//!
//! Es un solo `TextEdit` de egui (selección entre bloques, IME, deshacer, copiar y pegar son los
//! de egui) con un *layouter* que pinta el estilo a partir de `pulldown-cmark`, más las teclas del
//! editor ([`iure_doc::edit`]):
//!
//! - **Intro** sigue una lista, numerada, de tareas o una cita; en un elemento vacío sale de ella.
//! - **Tab** / **Mayús+Tab** sangran o desangran un elemento de lista.
//! - **Ctrl+B** / **Ctrl+I** / **Ctrl+Mayús+X**: negrita, cursiva, tachado (ponen o quitan).
//! - **Ctrl+Mayús+K**: enlace (`Ctrl+K` es la paleta de comandos de la suite).
//!
//! Las marcas en línea (`**`, `*`, `~~`, comillas, sintaxis de enlace, `#` de títulos) se ocultan
//! fuera de la línea del cursor (la negrita se ve como negrita); en esa línea se ven para poder
//! borrarlas. Viñetas, `>`, cercas y `|` de tablas siempre se ven: son estructura.
//!
//! Lo que viene (S1.12, siguientes entregas): diagramas y tablas dibujados en su lugar, pegar HTML
//! como Markdown, corrector.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]

use std::sync::Arc;

use egui::text::{CCursor, CCursorRange, LayoutJob, TextFormat};
use egui::text_edit::TextEditState;
use egui::{Color32, FontFamily, FontId, Id, Key, Modifiers, Stroke, Ui};
use iure_doc::edit::{self, Edit};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag};

mod blocks;
mod diagrams;
mod find;
mod highlight;
mod history;
mod image_store;
mod images;
mod paste;
mod span;
pub mod spell;
mod suggest;
mod table_ui;
mod templates;
pub use blocks::{
    ExternalEdit, Hooks, apply_external, caret_offset, focus_offset, selection, show as show_blocks,
    show_hooks as show_blocks_hooks, show_with as show_blocks_with,
};
pub use diagrams::{DiagramImage, DiagramRenderer};
pub use highlight::wait_ready as wait_highlighting;
pub use image_store::{FolderImages, ImageStore, PastedImage};
pub use images::{LocalImages, Renderers};
pub use paste::{PasteSource, SystemClipboard};
pub use spell::Speller;
pub use suggest::NoteIndex;
pub use templates::mermaid as mermaid_templates;

/// Colores y letra del editor.
#[derive(Clone, Debug, PartialEq)]
pub struct EditorStyle {
    pub text: Color32,
    /// Marcas de Markdown y citas.
    pub muted: Color32,
    pub accent: Color32,
    /// Fondo del código.
    pub code_bg: Color32,
    /// Tamaño de la letra del cuerpo.
    pub size: f32,
    /// Familia en negrita (la del ui-kit).
    pub bold_family: FontFamily,
    pub body_family: FontFamily,
    /// Subrayado de las palabras mal escritas.
    pub error: Color32,
    /// Tema oscuro (los colores del código cambian).
    pub dark: bool,
}

impl EditorStyle {
    /// Con los colores y la letra del tema actual.
    pub fn from_ui(ui: &Ui) -> Self {
        let c = iurefficient_ui::theme::colors(ui);
        let body = iurefficient_ui::theme::body_font(ui.ctx());
        let bold = iurefficient_ui::theme::semibold_font(ui.ctx(), body.size);
        EditorStyle {
            text: c.text,
            muted: c.text_muted,
            accent: c.accent,
            code_bg: c.surface_alt,
            size: body.size,
            bold_family: bold.family,
            body_family: body.family,
            error: ui.visuals().error_fg_color,
            dark: ui.visuals().dark_mode,
        }
    }
}

/// Formato de un byte del texto (se juntan los iguales en tramos).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Fmt {
    heading: u8,
    bold: bool,
    italic: bool,
    strike: bool,
    code: bool,
    link: bool,
    /// Marca de Markdown (tenue).
    marker: bool,
    /// Marca en línea (`**`, `*`, `~~`, comillas, sintaxis de enlace, `#` de títulos): se oculta
    /// fuera de la línea del cursor.
    inline: bool,
    /// Viñeta, número o casilla (acento).
    bullet: bool,
    quote: bool,
    /// Oculta (marca en línea fuera de la línea del cursor).
    hidden: bool,
    /// Palabra mal escrita (subrayada).
    misspelled: bool,
    /// Clase de resaltado de sintaxis en un bloque de código ([`highlight`]; 0: ninguna).
    syn: u8,
    /// Marca de bloque que se dibuja en vez de verse como texto (WYSIWYG): [`DECO_BULLET`]…
    deco: u8,
    /// Cuerpo de un bloque de código (el fondo es una caja, no tramo a tramo).
    block_code: bool,
    /// Texto de una tarea hecha (tenue y tachado).
    done: bool,
    /// Invisible pero con su ancho: encima se dibuja la viñeta, la casilla, la barra de la cita.
    concealed: bool,
    /// Sangría de un elemento de lista anidado: se ve más ancha que sus espacios.
    indent: bool,
    /// Color del texto (`<span style="color: …">`): `0x01RRGGBB`, 0 sin color.
    fg: u32,
    /// Resaltado (`<mark>`): `0x01RRGGBB`, 0 sin resaltado.
    bg: u32,
    /// Fórmula en línea (`$…$`).
    math: bool,
    /// Hueco de la fórmula dibujada número `n - 1` de [`MathImg`] (0: ninguno).
    math_slot: u16,
    /// Línea vacía dentro de un bloque (entre elementos de una lista «suelta»): poco alto.
    blank: bool,
}

/// Una fórmula en línea ya dibujada: su LaTeX (sin `$`), la textura y su tamaño en puntos.
#[derive(Clone, Debug)]
pub(crate) struct MathImg {
    pub latex: String,
    pub tex: egui::TextureId,
    pub size: egui::Vec2,
}

/// Las fórmulas en línea (`$…$`) de un bloque, sin los `$`.
pub(crate) fn inline_maths(text: &str) -> Vec<String> {
    let opts = Options::ENABLE_MATH;
    Parser::new_ext(text, opts)
        .filter_map(|e| match e {
            Event::InlineMath(m) => Some(m.trim().to_string()),
            _ => None,
        })
        .filter(|m| !m.is_empty())
        .take(64)
        .collect()
}

/// `#RRGGBB` de una etiqueta (`… color: #RRGGBB …`) como `0x01RRGGBB`.
fn tag_color(tag: &str, key: &str) -> Option<u32> {
    let at = tag.find(key)? + key.len();
    let rest = tag.get(at..)?.trim_start_matches([':', ' ']);
    let hex = iure_doc::edit::hex_color(rest.get(..7)?)?;
    u32::from_str_radix(hex.get(1..)?, 16).ok().map(|v| v | 0x0100_0000)
}

fn rgb_of(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// Marcas de bloque dibujadas (WYSIWYG). El texto sigue ahí (el cursor, deshacer y el Markdown no
/// cambian); se oculta y encima se pinta lo que representa.
const DECO_BULLET: u8 = 1;
const DECO_TASK: u8 = 2;
const DECO_TASK_DONE: u8 = 3;
const DECO_QUOTE: u8 = 4;
/// Cerca de un bloque de código: se pliega cuando el bloque no tiene el cursor.
const DECO_FENCE: u8 = 5;
/// Marca de un aviso (`[!NOTE]`…): fuera de su línea se dibuja su título. Uno por tipo, en el
/// orden de [`CALLOUTS`].
const DECO_CALLOUT: u8 = 10;
/// Hueco de una fórmula en línea (su número en `level`, desde 1).
const DECO_MATH: u8 = 9;

/// Tipos de aviso (como GitHub y la 1.x): marca, título en inglés y en español.
const CALLOUTS: [(&str, &str, &str); 5] = [
    ("NOTE", "Note", "Nota"),
    ("TIP", "Tip", "Consejo"),
    ("IMPORTANT", "Important", "Importante"),
    ("WARNING", "Warning", "Advertencia"),
    ("CAUTION", "Caution", "Precaución"),
];

/// El tipo de aviso con que empieza una cita (`> [!NOTE]`): su índice en [`CALLOUTS`].
fn callout_of(block: &str) -> Option<usize> {
    let t = block.trim_start().strip_prefix('>')?.trim_start().strip_prefix("[!")?;
    let tag: String = t.chars().take_while(|c| c.is_ascii_alphabetic()).collect::<String>().to_ascii_uppercase();
    CALLOUTS.iter().position(|(k, _, _)| *k == tag)
}

/// Color de cada tipo de aviso.
fn callout_color(i: usize, s: &EditorStyle) -> Color32 {
    match i {
        0 => s.accent,
        1 => Color32::from_rgb(0x1a, 0x7f, 0x37),
        2 => Color32::from_rgb(0x82, 0x50, 0xdf),
        3 => Color32::from_rgb(0x9a, 0x67, 0x00),
        _ => Color32::from_rgb(0xcf, 0x22, 0x2e),
    }
}
const DECO_RULE: u8 = 6;
/// El `- ` delante de una casilla: invisible, con su ancho (las tareas se alinean con las viñetas).
const DECO_TASK_DASH: u8 = 7;

/// Una marca de bloque para dibujar: tipo, bytes del texto y nivel de sangría.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Deco {
    kind: u8,
    start: usize,
    end: usize,
    level: u8,
}

fn inline_marker(x: &mut Fmt) {
    x.marker = true;
    x.inline = true;
}

/// Formatos byte a byte del texto.
fn formats(text: &str) -> Vec<Fmt> {
    let mut f = vec![Fmt::default(); text.len()];
    let mut set = |r: std::ops::Range<usize>, g: &dyn Fn(&mut Fmt)| {
        for x in f.get_mut(r.start.min(text.len())..r.end.min(text.len())).into_iter().flatten() {
            g(x);
        }
    };
    let opts = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_MATH
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS;
    // Lenguaje del bloque de código en el que se está (para el resaltado de sintaxis).
    let mut code_lang: Option<String> = None;
    // Colores y resaltados abiertos: (resaltado, color, dónde empieza el texto).
    let mut paints: Vec<(bool, u32, usize)> = Vec::new();
    for (ev, r) in Parser::new_ext(text, opts).into_offset_iter() {
        let slice = text.get(r.clone()).unwrap_or("");
        match ev {
            Event::Text(_) if code_lang.is_some() => {
                let lang = code_lang.as_deref().unwrap_or("");
                for (lr, class) in highlight::spans(lang, slice) {
                    set(r.start + lr.start..r.start + lr.end, &|x| x.syn = class);
                }
            }
            Event::End(pulldown_cmark::TagEnd::CodeBlock) => code_lang = None,
            Event::Start(Tag::Heading { level, .. }) => {
                let n = match level {
                    HeadingLevel::H1 => 1,
                    HeadingLevel::H2 => 2,
                    HeadingLevel::H3 => 3,
                    HeadingLevel::H4 => 4,
                    HeadingLevel::H5 => 5,
                    HeadingLevel::H6 => 6,
                };
                set(r.clone(), &|x| x.heading = n);
                // `## ` del principio.
                let hashes = slice.len() - slice.trim_start_matches('#').len();
                if hashes > 0 {
                    let sp = slice.get(hashes..).map_or(0, |s| s.len() - s.trim_start().len());
                    set(r.start..r.start + hashes + sp, &|x| {
                        x.marker = true;
                        x.inline = true;
                    });
                }
            }
            Event::Start(Tag::Strong) => {
                set(r.clone(), &|x| x.bold = true);
                set(r.start..r.start + 2, &inline_marker);
                set(r.end.saturating_sub(2)..r.end, &inline_marker);
            }
            Event::Start(Tag::Emphasis) => {
                set(r.clone(), &|x| x.italic = true);
                set(r.start..r.start + 1, &inline_marker);
                set(r.end.saturating_sub(1)..r.end, &inline_marker);
            }
            Event::Start(Tag::Strikethrough) => {
                set(r.clone(), &|x| x.strike = true);
                let n = slice.len() - slice.trim_start_matches('~').len();
                set(r.start..r.start + n, &inline_marker);
                set(r.end.saturating_sub(n)..r.end, &inline_marker);
            }
            Event::Code(_) | Event::InlineMath(_) | Event::DisplayMath(_) => {
                let inline_math = matches!(ev, Event::InlineMath(_));
                set(r.clone(), &|x| {
                    x.code = true;
                    x.math = inline_math;
                });
                let n = slice.len() - slice.trim_start_matches(['`', '$']).len();
                set(r.start..r.start + n, &inline_marker);
                set(r.end.saturating_sub(n)..r.end, &inline_marker);
            }
            Event::Start(Tag::Image { .. }) => {
                // `![texto](ruta)`: el texto como enlace; `![` y `](ruta)` son marcas.
                set(r.clone(), &|x| x.link = true);
                if slice.starts_with("![")
                    && let Some(close) = slice.rfind("](")
                {
                    set(r.start..r.start + 2, &inline_marker);
                    set(r.start + close..r.end, &inline_marker);
                }
            }
            Event::Start(Tag::Link { .. }) => {
                set(r.clone(), &|x| x.link = true);
                if slice.starts_with('[')
                    && let Some(close) = slice.rfind("](")
                {
                    set(r.start..r.start + 1, &inline_marker);
                    set(r.start + close..r.end, &inline_marker);
                }
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                code_lang = match kind {
                    pulldown_cmark::CodeBlockKind::Fenced(info) => Some(
                        info.split(|c: char| c.is_whitespace() || c == ',' || c == '{')
                            .next()
                            .unwrap_or("")
                            .to_string(),
                    ),
                    pulldown_cmark::CodeBlockKind::Indented => None,
                };
                set(r.clone(), &|x| {
                    x.code = true;
                    x.block_code = true;
                });
                // Las cercas (con su salto de línea: plegadas no ocupan renglón).
                let first = slice.find('\n').unwrap_or(slice.len());
                if slice.trim_start().starts_with(['`', '~']) {
                    set(r.start..r.start + first, &|x| x.marker = true);
                    set(r.start..(r.start + first + 1).min(r.end), &|x| x.deco = DECO_FENCE);
                    let body = slice.trim_end_matches('\n');
                    if let Some(last) = body.rfind('\n')
                        && body.get(last + 1..).is_some_and(|l| l.trim_start().starts_with(['`', '~']))
                    {
                        set(r.start + last + 1..r.start + body.len(), &|x| x.marker = true);
                        set(r.start + last..r.start + body.len(), &|x| x.deco = DECO_FENCE);
                    }
                }
            }
            Event::Start(Tag::Item) => {
                // La viñeta o el número (y la casilla), desde el principio de su línea.
                let ls = text.get(..r.start).and_then(|b| b.rfind('\n')).map_or(0, |i| i + 1);
                let line = text.get(ls..).and_then(|t| t.lines().next()).unwrap_or("");
                // Dentro de una cita, la línea empieza con `> ` (o varios): solo eso se salta; la
                // sangría de la lista es del elemento.
                let mut quote = 0usize;
                while let Some(rest) = line.get(quote..) {
                    let t = rest.trim_start_matches(' ');
                    if !t.starts_with('>') {
                        break;
                    }
                    quote += rest.len() - t.len() + 1;
                    if line.get(quote..).is_some_and(|r| r.starts_with(' ')) {
                        quote += 1;
                    }
                }
                let inner = line.get(quote..).unwrap_or("");
                if let Some(p) = edit::prefix(inner) {
                    let at = ls + quote;
                    let m0 = at + p.indent.len();
                    set(m0..at + p.len, &|x| x.bullet = true);
                    if !p.indent.is_empty() {
                        set(at..m0, &|x| x.indent = true);
                    }
                    let ordered = p.number.is_some();
                    match p.task {
                        Some(done) => {
                            if !ordered {
                                set(m0..m0 + p.marker.len(), &|x| x.deco = DECO_TASK_DASH);
                            }
                            let b0 = m0 + p.marker.len();
                            let kind = if done { DECO_TASK_DONE } else { DECO_TASK };
                            set(b0..b0 + 3, &|x| x.deco = kind);
                            if done {
                                let end = text
                                    .get(at + p.len..)
                                    .and_then(|t| t.find('\n'))
                                    .map_or(text.len(), |n| at + p.len + n);
                                set(at + p.len..end, &|x| x.done = true);
                            }
                        }
                        None if !ordered => set(m0..m0 + 1, &|x| x.deco = DECO_BULLET),
                        None => {}
                    }
                }
            }
            Event::Start(Tag::Table(_)) => {
                // Los `|` y la fila de alineación, tenues.
                for (i, ch) in slice.char_indices() {
                    if ch == '|' {
                        set(r.start + i..r.start + i + 1, &|x| x.marker = true);
                    }
                }
                if let Some(first_nl) = slice.find('\n') {
                    let second =
                        slice.get(first_nl + 1..).and_then(|t| t.find('\n')).map_or(slice.len(), |j| first_nl + 1 + j);
                    set(r.start + first_nl + 1..r.start + second, &|x| x.marker = true);
                }
            }
            Event::Start(Tag::BlockQuote(_)) => {
                set(r.clone(), &|x| x.quote = true);
                // Los `>` de cada línea.
                // Un aviso: `[!TIPO]` de la primera línea, en negrita (y fuera de su línea, su título).
                if let Some(k) = callout_of(slice)
                    && let Some(open) = slice.find("[!")
                    && let Some(close) = slice.get(open..).and_then(|t| t.find(']'))
                {
                    let deco = DECO_CALLOUT + k as u8;
                    set(r.start + open..r.start + open + close + 1, &|x| {
                        x.bold = true;
                        x.deco = deco;
                    });
                }
                let mut at = r.start;
                for line in slice.split_inclusive('\n') {
                    let lead = line.len() - line.trim_start().len();
                    if line.trim_start().starts_with('>') {
                        set(at + lead..at + lead + 1, &|x| {
                            x.marker = true;
                            x.deco = DECO_QUOTE;
                        });
                    }
                    at += line.len();
                }
            }
            Event::Rule => set(r, &|x| {
                x.marker = true;
                x.deco = DECO_RULE;
            }),
            Event::InlineHtml(h) => {
                let tag = h.trim().to_ascii_lowercase();
                let paint = if tag.starts_with("<span") {
                    tag_color(&tag, "color").map(|c| (false, c))
                } else if tag.starts_with("<mark") {
                    Some((true, tag_color(&tag, "background-color").unwrap_or(0x01FF_F59D)))
                } else {
                    None
                };
                let closing = match tag.as_str() {
                    "</span>" => Some(false),
                    "</mark>" => Some(true),
                    _ => None,
                };
                if let Some((hl, c)) = paint {
                    set(r.clone(), &inline_marker);
                    if paints.len() < 64 {
                        paints.push((hl, c, r.end));
                    }
                } else if let Some(hl) = closing
                    && let Some(k) = paints.iter().rposition(|p| p.0 == hl)
                {
                    set(r.clone(), &inline_marker);
                    let (_, c, start) = paints.remove(k);
                    if hl {
                        set(start..r.start, &|x| x.bg = c);
                    } else {
                        set(start..r.start, &|x| x.fg = c);
                    }
                } else {
                    set(r, &|x| x.marker = true);
                }
            }
            Event::Html(_) => set(r, &|x| x.marker = true),
            Event::Start(Tag::MetadataBlock(_)) => set(r, &|x| x.marker = true),
            _ => {}
        }
    }
    // Enlaces entre notas (`[[nota#título|alias]]`, fuera del código): se ve lo que se lee (el
    // alias o el destino) como enlace; los corchetes y el destino con alias son marcas.
    for (s0, e0, shown) in wiki_links(text) {
        if f.get(s0).is_some_and(|x| x.code) {
            continue;
        }
        for (k, x) in f.iter_mut().enumerate().take(e0).skip(s0) {
            x.link = true;
            if k < shown || k + 2 >= e0 {
                inline_marker(x);
            }
        }
    }
    f
}

/// Enlaces `[[…]]` de una línea: (inicio, fin, dónde empieza lo que se ve), en bytes. Con `![[`, el
/// `!` va dentro.
pub(crate) fn wiki_links(text: &str) -> Vec<(usize, usize, usize)> {
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(i) = text.get(from..).and_then(|t| t.find("[[")) {
        let s0 = from + i;
        let inner_start = s0 + 2;
        let Some(len) = text.get(inner_start..).and_then(|t| t.find("]]")) else { break };
        let inner = text.get(inner_start..inner_start + len).unwrap_or("");
        if inner.is_empty() || inner.contains(['[', ']', '\n']) {
            from = inner_start;
            continue;
        }
        let e0 = inner_start + len + 2;
        let start = if s0 > 0 && text.get(s0 - 1..s0) == Some("!") { s0 - 1 } else { s0 };
        let shown = inner.find('|').map_or(inner_start, |p| inner_start + p + 1);
        out.push((start, e0, shown));
        if out.len() >= 1_000 {
            break;
        }
        from = e0;
    }
    out
}

/// El enlace bajo el byte `at`: el destino de un `[[…]]` (sin el alias) o la dirección de un
/// `[texto](dirección)`.
pub(crate) fn link_at(text: &str, at: usize) -> Option<String> {
    if let Some((s0, e0, _)) = wiki_links(text).into_iter().find(|(s0, e0, _)| (*s0..*e0).contains(&at)) {
        let inner = text.get(s0..e0)?.trim_start_matches('!').trim_start_matches("[[").trim_end_matches("]]");
        let target = inner.split('|').next().unwrap_or(inner).trim();
        return (!target.is_empty()).then(|| format!("[[{target}]]"));
    }
    let parser = pulldown_cmark::Parser::new_ext(text, Options::ENABLE_TABLES).into_offset_iter();
    for (e, r) in parser {
        if let Event::Start(Tag::Link { dest_url, .. }) = e
            && r.contains(&at)
        {
            return Some(dest_url.to_string());
        }
    }
    None
}

/// Color de una clase de sintaxis, con el tema: palabras clave en el acento, comentarios tenues.
fn syntax_color(class: u8, s: &EditorStyle) -> Color32 {
    let (string, number, function, ty) = if s.dark {
        (
            Color32::from_rgb(0x8f, 0xd1, 0x8f),
            Color32::from_rgb(0xf0, 0xa3, 0x5e),
            Color32::from_rgb(0xc3, 0x9e, 0xf0),
            Color32::from_rgb(0x63, 0xc5, 0xda),
        )
    } else {
        (
            Color32::from_rgb(0x0a, 0x7d, 0x33),
            Color32::from_rgb(0xb3, 0x59, 0x00),
            Color32::from_rgb(0x6f, 0x42, 0xc1),
            Color32::from_rgb(0x0b, 0x6e, 0x99),
        )
    };
    match class {
        highlight::COMMENT => s.muted,
        highlight::STRING => string,
        highlight::NUMBER => number,
        highlight::KEYWORD => s.accent,
        highlight::FUNCTION => function,
        highlight::TYPE => ty,
        _ => s.text,
    }
}

/// Interlineado del cuerpo, como factor del tamaño de la letra (el de un documento, no el de
/// una etiqueta de la interfaz).
pub(crate) const LINE_SPACING: f32 = 1.5;

/// Interlineado de una línea con este formato y tamaño (los títulos y el código, más apretados).
fn line_height(fmt: Fmt, size: f32) -> Option<f32> {
    let k = if fmt.heading > 0 {
        1.25
    } else if fmt.code {
        1.35
    } else {
        LINE_SPACING
    };
    Some((size * k).round())
}

/// Factor de tamaño de un título.
fn heading_scale(level: u8) -> f32 {
    match level {
        1 => 1.9,
        2 => 1.5,
        3 => 1.25,
        4 => 1.1,
        _ => 1.0,
    }
}

fn text_format(fmt: Fmt, s: &EditorStyle) -> TextFormat {
    if fmt.hidden {
        // Casi sin ancho e invisible: sigue en el texto (el cursor pasa por ahí) pero no se ve.
        return TextFormat {
            font_id: FontId::new(0.05, s.body_family.clone()),
            color: Color32::TRANSPARENT,
            ..Default::default()
        };
    }
    let size = s.size * if fmt.heading > 0 { heading_scale(fmt.heading) } else { 1.0 };
    if fmt.concealed {
        // Con su ancho (encima se dibuja la viñeta, la casilla o la barra), sin verse.
        return TextFormat {
            font_id: FontId::new(size, s.body_family.clone()),
            line_height: line_height(fmt, size),
            color: Color32::TRANSPARENT,
            ..Default::default()
        };
    }
    let family = if fmt.code {
        FontFamily::Monospace
    } else if fmt.bold || fmt.heading > 0 {
        s.bold_family.clone()
    } else {
        s.body_family.clone()
    };
    let size = if fmt.code { size * 0.92 } else { size };
    let mut color = if fmt.marker {
        s.muted.gamma_multiply(0.75)
    } else if fmt.bullet || fmt.link {
        s.accent
    } else if fmt.quote {
        s.muted
    } else {
        s.text
    };
    if fmt.marker && fmt.bullet {
        color = s.accent;
    }
    if fmt.code && !fmt.marker && fmt.syn != 0 {
        color = syntax_color(fmt.syn, s);
    }
    if fmt.done && !fmt.marker {
        color = s.muted;
    }
    if fmt.fg != 0 && !fmt.marker {
        color = rgb_of(fmt.fg);
    }
    // Sobre un resaltado claro, en el tema oscuro, el texto va oscuro para leerse.
    if fmt.bg != 0 && !fmt.marker && s.dark && fmt.fg == 0 {
        color = Color32::from_rgb(0x1f, 0x1f, 0x1f);
    }
    TextFormat {
        font_id: FontId::new(size, family),
        line_height: line_height(fmt, size),
        color,
        italics: fmt.italic || (fmt.quote && !fmt.marker) || (fmt.code && fmt.syn == highlight::COMMENT),
        // El código en bloque va en una caja (la dibuja el campo), no tramo a tramo.
        background: if fmt.bg != 0 && !fmt.marker {
            rgb_of(fmt.bg)
        } else if fmt.code && !fmt.marker && !fmt.block_code {
            s.code_bg
        } else {
            Color32::TRANSPARENT
        },
        strikethrough: if (fmt.strike || fmt.done) && !fmt.marker {
            Stroke::new(1.0, if fmt.done { s.muted } else { s.text })
        } else {
            Stroke::NONE
        },
        underline: if fmt.misspelled && !fmt.marker {
            Stroke::new(1.5, s.error)
        } else if fmt.link && !fmt.marker {
            Stroke::new(1.0, s.accent)
        } else {
            Stroke::NONE
        },
        ..Default::default()
    }
}

/// El texto con su estilo, en tramos de formato igual. Las marcas en línea fuera de `reveal`
/// (bytes de la línea del cursor; `None` sin cursor) se ocultan: la negrita se ve como negrita y
/// el enlace como enlace; en la línea donde se escribe se ven para poder borrarlas.
pub fn layout_job(text: &str, s: &EditorStyle, wrap_width: f32, reveal: Option<std::ops::Range<usize>>) -> LayoutJob {
    layout_job_spell(text, s, wrap_width, reveal, None, None).0
}

/// Bytes que el corrector no revisa: código y marcas de Markdown.
fn spell_skip(f: &[Fmt], i: usize) -> bool {
    f.get(i).is_some_and(|x| x.code || x.marker)
}

/// Como [`layout_job`], con las palabras mal escritas subrayadas (menos la que se está tecleando,
/// la que toca `typing`, en bytes).
pub(crate) fn layout_job_spell(
    text: &str,
    s: &EditorStyle,
    wrap_width: f32,
    reveal: Option<std::ops::Range<usize>>,
    spell: Option<&Speller>,
    typing: Option<usize>,
) -> (LayoutJob, Vec<Deco>) {
    layout_job_full(text, s, wrap_width, reveal, spell, typing, &[])
}

/// Como [`layout_job_spell`], con las fórmulas en línea ya dibujadas (`math`): fuera de la línea
/// del cursor, cada una deja un hueco de su tamaño (encima se pinta la imagen).
pub(crate) fn layout_job_full(
    text: &str,
    s: &EditorStyle,
    wrap_width: f32,
    reveal: Option<std::ops::Range<usize>>,
    spell: Option<&Speller>,
    typing: Option<usize>,
    math: &[MathImg],
) -> (LayoutJob, Vec<Deco>) {
    let mut f = formats(text);
    if let Some(sp) = spell {
        for r in sp.misspelled(text, |i| spell_skip(&f, i)) {
            if typing.is_some_and(|t| r.start <= t && t <= r.end) {
                continue;
            }
            for x in f.get_mut(r).into_iter().flatten() {
                x.misspelled = true;
            }
        }
    }
    // Con el cursor en el bloque (`reveal`), las cercas del código se ven para editarlas; las
    // viñetas, casillas y citas se dibujan siempre; la regla se ve como `---` en su línea.
    let focused = reveal.is_some();
    for (i, x) in f.iter_mut().enumerate() {
        let on_line = reveal.as_ref().is_some_and(|r| r.contains(&i) || r.end == i);
        x.hidden = (x.inline && !on_line) || (x.deco == DECO_FENCE && !focused);
        x.concealed = !x.hidden
            && (matches!(x.deco, DECO_BULLET | DECO_TASK | DECO_TASK_DONE | DECO_QUOTE | DECO_TASK_DASH)
                || ((x.deco == DECO_RULE || x.deco >= DECO_CALLOUT) && !on_line));
    }
    // Líneas vacías dentro del bloque (fuera del código): un poco de aire, no un renglón entero.
    let bytes = text.as_bytes();
    for (i, x) in f.iter_mut().enumerate() {
        if bytes.get(i) == Some(&b'\n') && i > 0 && bytes.get(i - 1) == Some(&b'\n') && !x.code {
            x.blank = true;
        }
    }
    // Fórmulas en línea dibujadas, fuera de la línea del cursor: el primer `$` hace de hueco y el
    // resto se oculta.
    let mut holes: Vec<(usize, usize, u16)> = Vec::new();
    if !math.is_empty() {
        let mut i = 0usize;
        while i < f.len() {
            if !f.get(i).is_some_and(|x| x.math) {
                i += 1;
                continue;
            }
            let mut e = i;
            while f.get(e).is_some_and(|x| x.math) {
                e += 1;
            }
            let on_line = reveal.as_ref().is_some_and(|r| r.contains(&i) || r.end == i);
            let latex = text.get(i + 1..e.saturating_sub(1)).map(str::trim).unwrap_or("");
            if !on_line
                && let Some(k) = math.iter().position(|m| m.latex == latex)
                && let Ok(slot) = u16::try_from(k + 1)
                && text.is_char_boundary(i + 1)
            {
                for x in f.get_mut(i..e).into_iter().flatten() {
                    x.hidden = true;
                    x.concealed = false;
                }
                // epaint pone el espacio extra antes de la letra: el hueco queda a su izquierda.
                if let Some(x) = f.get_mut(i) {
                    x.hidden = false;
                    x.math_slot = slot;
                }
                holes.push((i, i + 1, slot));
            }
            i = e;
        }
    }
    // Las marcas que se dibujan, en tramos.
    let mut decos: Vec<Deco> = Vec::new();
    for (i, ch) in text.char_indices() {
        let Some(x) = f.get(i) else { continue };
        let draw = x.concealed || (x.deco == DECO_FENCE && !x.hidden);
        // Los saltos de línea no se dibujan (una regla o una cerca los incluye).
        if !draw || ch == '\n' {
            continue;
        }
        let end = i + ch.len_utf8();
        match decos.last_mut() {
            Some(d) if d.kind == x.deco && d.end == i => d.end = end,
            _ => {
                let ls = text.get(..i).and_then(|b| b.rfind('\n')).map_or(0, |n| n + 1);
                let indent = text.get(ls..i).map_or(0, |l| l.len() - l.trim_start_matches([' ', '>']).len());
                decos.push(Deco { kind: x.deco, start: i, end, level: u8::try_from(indent / 2).unwrap_or(u8::MAX) });
            }
        }
    }
    for (at, end, slot) in holes {
        decos.push(Deco { kind: DECO_MATH, start: at, end, level: u8::try_from(slot).unwrap_or(0) });
    }
    let fmt_of = |fmt: Fmt| -> TextFormat {
        match math.get(usize::from(fmt.math_slot).wrapping_sub(1)).filter(|_| fmt.math_slot > 0) {
            Some(m) => TextFormat {
                font_id: FontId::new(0.05, s.body_family.clone()),
                color: Color32::TRANSPARENT,
                extra_letter_spacing: m.size.x + 2.0,
                line_height: Some((m.size.y + 4.0).max(s.size * LINE_SPACING).round()),
                valign: egui::Align::Center,
                ..Default::default()
            },
            None if fmt.blank => TextFormat {
                font_id: FontId::new(s.size * 0.4, s.body_family.clone()),
                line_height: Some((s.size * 0.45).round()),
                ..Default::default()
            },
            None => text_format(fmt, s),
        }
    };
    let mut job = LayoutJob::default();
    // La sangría de una lista anidada: cada nivel (dos espacios) se ve como 1,2 em.
    let lead = |run: &str, fmt: Fmt| -> f32 {
        if fmt.indent && !fmt.hidden { (run.len() as f32 / 2.0) * s.size * 0.9 } else { 0.0 }
    };
    let mut start = 0usize;
    let mut cur = f.first().copied().unwrap_or_default();
    for (i, _) in text.char_indices().skip(1) {
        let here = f.get(i).copied().unwrap_or_default();
        if here != cur {
            let run = text.get(start..i).unwrap_or("");
            job.append(run, lead(run, cur), fmt_of(cur));
            start = i;
            cur = here;
        }
    }
    if start < text.len() || text.is_empty() {
        let run = text.get(start..).unwrap_or("");
        job.append(run, lead(run, cur), fmt_of(cur));
    }
    job.wrap.max_width = wrap_width;
    (job, decos)
}

/// Lo que pasó en el editor este cuadro.
pub struct Output {
    /// El texto cambió (teclas o atajos del editor).
    pub changed: bool,
    pub response: egui::Response,
    /// Ctrl+clic en un enlace: `[[nota#título]]` (sin alias) o la dirección de un enlace Markdown.
    /// Lo abre la app.
    pub link: Option<String>,
}

/// Dónde deja el campo el enlace pulsado hasta que lo recoge el editor.
pub(crate) fn link_slot() -> Id {
    Id::unique("iure-doc-link-clicked")
}

/// El enlace pulsado en este cuadro (y se olvida).
pub(crate) fn take_link(ui: &Ui) -> Option<String> {
    ui.data_mut(|d| d.remove_temp::<String>(link_slot()))
}

/// Consume un atajo por su tecla lógica o física (Ctrl+Mayús+7 en un teclado donde Mayús+7 es
/// «/»), con exactamente esos modificadores.
pub(crate) fn take_shortcut(i: &mut egui::InputState, mods: Modifiers, key: Key) -> bool {
    let pos = i.events.iter().position(|e| {
        matches!(e, egui::Event::Key { key: k, physical_key: pk, pressed: true, modifiers: m, .. }
            if (*k == key || *pk == Some(key)) && m.command == mods.command && m.shift == mods.shift && m.alt == mods.alt)
    });
    match pos {
        Some(p) => {
            i.events.remove(p);
            true
        }
        None => false,
    }
}

/// La tecla del editor que toca (antes de que la vea el `TextEdit`).
fn editor_key(ui: &Ui, text: &str, sel: (usize, usize)) -> Option<Edit> {
    let (a, b) = sel;
    let cursor = a.max(b);
    let collapsed = a == b;
    ui.input_mut(|i| {
        // Tres Intro al final de un bloque de código: fuera del bloque.
        if collapsed
            && let Some(e) = edit::code_exit(text, cursor)
            && i.consume_key(Modifiers::NONE, Key::Enter)
        {
            return Some(e);
        }
        if collapsed
            && let Some(e) = edit::on_enter(text, cursor)
            && i.consume_key(Modifiers::NONE, Key::Enter)
        {
            return Some(e);
        }
        // Listas (Ctrl+Mayús+7/8/9) y títulos (Ctrl+Alt+1–3), como en TipTap y Google Docs.
        let shift_cmd = Modifiers { shift: true, ..Modifiers::COMMAND };
        let alt_cmd = Modifiers { alt: true, ..Modifiers::COMMAND };
        for (key, kind) in [
            (Key::Num7, edit::LineKind::Ordered),
            (Key::Num8, edit::LineKind::Bullet),
            (Key::Num9, edit::LineKind::Task),
        ] {
            if take_shortcut(i, shift_cmd, key) {
                return edit::set_line_kind(text, a, b, kind);
            }
        }
        for (key, level) in [(Key::Num1, 1u8), (Key::Num2, 2), (Key::Num3, 3)] {
            if take_shortcut(i, alt_cmd, key) {
                return edit::set_line_kind(text, a, b, edit::LineKind::Heading(level));
            }
        }
        // Mayús+Tab primero: `consume_key` sin modificadores también acepta Mayús.
        if let Some(e) = edit::indent(text, cursor, true)
            && i.consume_key(Modifiers::SHIFT, Key::Tab)
        {
            return Some(e);
        }
        if !i.modifiers.shift
            && let Some(e) = edit::indent(text, cursor, false)
            && i.consume_key(Modifiers::NONE, Key::Tab)
        {
            return Some(e);
        }
        let shift_cmd = Modifiers { shift: true, ..Modifiers::COMMAND };
        if i.consume_key(Modifiers::COMMAND, Key::B) {
            return Some(edit::toggle_wrap(text, a, b, "**"));
        }
        if i.consume_key(Modifiers::COMMAND, Key::I) {
            return Some(edit::toggle_wrap(text, a, b, "*"));
        }
        if i.consume_key(shift_cmd, Key::X) {
            return Some(edit::toggle_wrap(text, a, b, "~~"));
        }
        if i.consume_key(shift_cmd, Key::K) {
            return Some(edit::link(text, a, b));
        }
        None
    })
}

/// El editor. `id` identifica su estado (cursor, deshacer) entre cuadros.
pub fn show(ui: &mut Ui, id: Id, text: &mut String, hint: &str) -> Output {
    let style = EditorStyle::from_ui(ui);
    let (changed, out) = field(ui, id, text, hint, 16, &style, style_key(&style), None, None, false, &[]);
    Output { changed, response: out.response.response, link: take_link(ui) }
}

/// Huella del estilo (parte de la llave de la caché del dibujo).
pub(crate) fn style_key(style: &EditorStyle) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    format!("{style:?}").hash(&mut h);
    h.finish()
}

/// Un campo con estilo al teclear y las teclas del editor; `true` si el texto cambió.
pub(crate) fn field(
    ui: &mut Ui,
    id: Id,
    text: &mut String,
    hint: &str,
    rows: usize,
    style: &EditorStyle,
    style_key: u64,
    spell: Option<&Speller>,
    notes: Option<&dyn NoteIndex>,
    menus: bool,
    math: &[MathImg],
) -> (bool, egui::text_edit::TextEditOutput) {
    let mut changed = false;
    // Los menús de `/` y `[[`: sus teclas antes que las del campo.
    if menus
        && ui.memory(|m| m.has_focus(id))
        && let Some(mut state) = TextEditState::load(ui.ctx(), id)
        && let Some(range) = state.cursor.char_range()
        && let Some((t, c)) = suggest::keys(ui, id, text, range.primary.index.0, notes)
    {
        *text = t;
        state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(c))));
        state.store(ui.ctx(), id);
        changed = true;
    }
    if ui.memory(|m| m.has_focus(id))
        && let Some(mut state) = TextEditState::load(ui.ctx(), id)
        && let Some(range) = state.cursor.char_range()
    {
        let sel = (range.primary.index.0, range.secondary.index.0);
        if let Some(e) = editor_key(ui, text, sel) {
            *text = e.text;
            state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(e.sel_start), CCursor::new(e.sel_end))));
            state.store(ui.ctx(), id);
            changed = true;
        }
    }
    // La línea del cursor (en bytes), para mostrar ahí las marcas.
    let reveal = if ui.memory(|m| m.has_focus(id)) {
        TextEditState::load(ui.ctx(), id).and_then(|st| st.cursor.char_range()).map(|r| {
            let (a, b) = (r.primary.index.0.min(r.secondary.index.0), r.primary.index.0.max(r.secondary.index.0));
            let ab = text.char_indices().nth(a).map_or(text.len(), |(i, _)| i);
            let bb = text.char_indices().nth(b).map_or(text.len(), |(i, _)| i);
            let start = text.get(..ab).and_then(|t| t.rfind('\n')).map_or(0, |i| i + 1);
            let end = text.get(bb..).and_then(|t| t.find('\n')).map_or(text.len(), |i| bb + i);
            start..end
        })
    } else {
        None
    };
    // Dónde se teclea (bytes): esa palabra no se marca mientras se escribe.
    let typing = if spell.is_some() && ui.memory(|m| m.has_focus(id)) {
        TextEditState::load(ui.ctx(), id)
            .and_then(|st| st.cursor.char_range())
            .map(|r| text.char_indices().nth(r.primary.index.0).map_or(text.len(), |(i, _)| i))
    } else {
        None
    };
    let spell_gen = spell.map(Speller::generation);
    // Las gramáticas de código se cargan en otro hilo: hasta que lleguen, otro cuadro en un rato.
    if !highlight::ready() && (text.contains("```") || text.contains("~~~")) {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
    }
    // Caché del dibujo: si no cambian el texto, la línea del cursor, el ancho ni el estilo (lo
    // normal mientras el cursor parpadea o se desplaza la vista), no se vuelve a calcular.
    let cache_id = id.with("galley");
    let deco_id = id.with("decos");
    let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, wrap: f32| -> Arc<egui::Galley> {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        buf.as_str().hash(&mut h);
        reveal.hash(&mut h);
        wrap.to_bits().hash(&mut h);
        style_key.hash(&mut h);
        spell_gen.hash(&mut h);
        typing.hash(&mut h);
        highlight::ready().hash(&mut h);
        for m in math {
            m.latex.hash(&mut h);
            m.size.x.to_bits().hash(&mut h);
            m.size.y.to_bits().hash(&mut h);
        }
        let key = h.finish();
        if let Some((k, g)) = ui.data(|d| d.get_temp::<(u64, Arc<egui::Galley>)>(cache_id))
            && k == key
        {
            return g;
        }
        let (job, decos) = layout_job_full(buf.as_str(), style, wrap, reveal.clone(), spell, typing, math);
        let g = ui.fonts_mut(|f| f.layout_job(job));
        ui.data_mut(|d| {
            d.insert_temp(cache_id, (key, Arc::clone(&g)));
            d.insert_temp(deco_id, Arc::new(decos));
        });
        g
    };
    // La caja del código va detrás del texto: se reserva su lugar antes de dibujarlo.
    let code_bg = ui.painter().add(egui::Shape::Noop);
    // Un bloque de código lleva margen dentro de su caja.
    let t0 = text.trim_start();
    let margin = if t0.starts_with("```") || t0.starts_with("~~~") {
        egui::Margin::symmetric(12, 8)
    } else {
        egui::Margin::ZERO
    };
    let out = egui::TextEdit::multiline(text)
        .id(id)
        .margin(margin)
        .frame(egui::Frame::NONE.inner_margin(margin))
        .hint_text(hint)
        .desired_width(f32::INFINITY)
        .desired_rows(rows)
        .lock_focus(true)
        .layouter(&mut layouter)
        .show(ui);
    changed |= out.response.changed();
    let decos = ui.data(|d| d.get_temp::<Arc<Vec<Deco>>>(deco_id)).unwrap_or_default();
    let focused_now = ui.memory(|m| m.has_focus(id));
    paint_decos(ui, &out, text, &decos, style, code_bg, focused_now);
    // Las fórmulas en línea, en sus huecos.
    for d in decos.iter().filter(|d| d.kind == DECO_MATH) {
        if let Some(m) = math.get(usize::from(d.level).wrapping_sub(1)) {
            // El hueco (el espacio extra de la letra) está justo a la izquierda de ella.
            let r = span_rect(&out, text, d.start, d.end);
            let rect =
                egui::Rect::from_min_size(egui::pos2(r.left() - m.size.x - 1.0, r.center().y - m.size.y / 2.0), m.size);
            ui.painter().image(
                m.tex,
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
    }
    // Clic en una casilla: se marca o desmarca (`[ ]` ↔ `[x]`, mismo largo: el cursor no se mueve).
    if out.response.clicked()
        && let Some(pp) = ui.input(|i| i.pointer.interact_pos())
        && let Some(d) = decos
            .iter()
            .find(|d| matches!(d.kind, DECO_TASK | DECO_TASK_DONE) && task_rect(&out, text, d).expand(3.0).contains(pp))
    {
        let mark = if d.kind == DECO_TASK { "x" } else { " " };
        if text.get(d.start + 1..d.start + 2).is_some()
            && text.is_char_boundary(d.start + 1)
            && text.is_char_boundary(d.start + 2)
        {
            text.replace_range(d.start + 1..d.start + 2, mark);
            changed = true;
        }
    }
    if let Some(sp) = spell {
        changed |= spell_menu(ui, id, text, sp, &out);
    }
    // Ctrl+clic en un enlace: lo abre la app.
    if out.response.clicked()
        && ui.input(|i| i.modifiers.command)
        && let Some(pp) = ui.input(|i| i.pointer.interact_pos())
    {
        let cc = out.galley.cursor_from_pos(pp - out.galley_pos).index.0;
        let at = text.char_indices().nth(cc).map_or(text.len(), |(b, _)| b);
        if let Some(l) = link_at(text, at).or_else(|| link_at(text, at.saturating_sub(1))) {
            ui.data_mut(|d| d.insert_temp(link_slot(), l));
        }
    }
    if menus && let Some((t, c)) = suggest::popup(ui, id, &out, text, notes) {
        *text = t;
        if let Some(mut state) = TextEditState::load(ui.ctx(), id) {
            state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(c))));
            state.store(ui.ctx(), id);
        }
        changed = true;
    }
    (changed, out)
}

/// Columna de documento: ancho máximo del texto, en múltiplos del tamaño de letra (≈ 760 px a 15 px).
pub(crate) const COLUMN_EMS: f32 = 52.0;

/// Zoom mínimo del documento ([`Hooks::zoom`]).
pub const ZOOM_MIN: f32 = 0.5;
/// Zoom máximo del documento.
pub const ZOOM_MAX: f32 = 3.0;
/// Pasos de zoom de las apps (los de un navegador o un procesador de textos).
const ZOOM_STEPS: [f32; 13] = [0.5, 0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0];

/// El paso de zoom siguiente (`up`) o anterior a `current`; con un valor raro, vuelve al 100 %.
pub fn zoom_step(current: f32, up: bool) -> f32 {
    if !current.is_finite() {
        return 1.0;
    }
    let found = if up {
        ZOOM_STEPS.iter().copied().find(|&z| z > current + 0.001)
    } else {
        ZOOM_STEPS.iter().rev().copied().find(|&z| z < current - 0.001)
    };
    found.unwrap_or(current.clamp(ZOOM_MIN, ZOOM_MAX))
}

/// Rectángulo (en pantalla) de los caracteres `[a, b)` de bytes de un renglón del campo.
fn span_rect(out: &egui::text_edit::TextEditOutput, text: &str, a: usize, b: usize) -> egui::Rect {
    let ca = text.get(..a).map_or(0, |t| t.chars().count());
    let cb = text.get(..b).map_or(ca, |t| t.chars().count());
    let ra = out.galley.pos_from_cursor(CCursor::new(ca));
    let rb = out.galley.pos_from_cursor(CCursor::new(cb));
    let x1 = if (rb.min.y - ra.min.y).abs() < 0.5 { rb.min.x } else { ra.max.x };
    egui::Rect::from_min_max(ra.min, egui::pos2(x1.max(ra.min.x + 1.0), ra.max.y)).translate(out.galley_pos.to_vec2())
}

/// La casilla de una tarea (cuadrada, centrada en el `[ ]`).
fn task_rect(out: &egui::text_edit::TextEditOutput, text: &str, d: &Deco) -> egui::Rect {
    let r = span_rect(out, text, d.start, d.end);
    let side = (r.height() * 0.66).clamp(10.0, 18.0);
    egui::Rect::from_center_size(r.center(), egui::vec2(side, side))
}

/// Dibuja lo que las marcas ocultas representan: viñetas, casillas, la barra de las citas, la
/// regla, y la caja del código (en `code_bg`, detrás del texto) con su lenguaje.
fn paint_decos(
    ui: &Ui,
    out: &egui::text_edit::TextEditOutput,
    text: &str,
    decos: &[Deco],
    s: &EditorStyle,
    code_bg: egui::layers::ShapeIdx,
    focused: bool,
) {
    let p = ui.painter();
    let field = out.response.response.rect;
    // Código en bloque: caja detrás de todo el campo y el lenguaje arriba a la derecha.
    let t = text.trim_start();
    if t.starts_with("```") || t.starts_with("~~~") {
        let r = field;
        p.set(code_bg, egui::Shape::rect_filled(r, 6.0, s.code_bg));
        let lang: String =
            t.trim_start_matches(['`', '~']).chars().take_while(|c| !c.is_whitespace()).take(24).collect();
        if !focused && !lang.is_empty() {
            p.text(
                r.right_top() + egui::vec2(-8.0, 4.0),
                egui::Align2::RIGHT_TOP,
                lang,
                FontId::proportional(s.size * 0.75),
                s.muted,
            );
        }
    }
    // Un aviso: fondo tenue del color de su tipo (detrás del texto) y la barra de ese color.
    let callout = callout_of(text);
    let bar = callout.map_or(s.muted.gamma_multiply(0.6), |k| callout_color(k, s));
    if let Some(k) = callout {
        p.set(code_bg, egui::Shape::rect_filled(field, 4.0, callout_color(k, s).gamma_multiply(0.08)));
    }
    let quotes: Vec<egui::Rect> =
        decos.iter().filter(|d| d.kind == DECO_QUOTE).map(|d| span_rect(out, text, d.start, d.end)).collect();
    for (k, r) in quotes.iter().enumerate() {
        // La barra baja hasta la siguiente línea de la cita (o el final del campo): los renglones
        // que se parten quedan dentro.
        let bottom = quotes.get(k + 1).map_or(out.galley_pos.y + out.galley.size().y, |n| n.top());
        let x = r.left() + 1.5;
        p.line_segment([egui::pos2(x, r.top()), egui::pos2(x, bottom.max(r.bottom()))], Stroke::new(3.0, bar));
    }
    // El título del aviso en lugar de `[!TIPO]`.
    for d in decos.iter().filter(|d| d.kind >= DECO_CALLOUT) {
        let k = usize::from(d.kind - DECO_CALLOUT);
        if let Some((_, en, es)) = CALLOUTS.get(k) {
            let r = span_rect(out, text, d.start, d.end);
            p.text(
                r.left_center(),
                egui::Align2::LEFT_CENTER,
                iurefficient_ui::lang::pick(en, es),
                FontId::new(s.size, s.bold_family.clone()),
                callout_color(k, s),
            );
        }
    }
    for d in decos {
        match d.kind {
            DECO_BULLET => {
                let r = span_rect(out, text, d.start, d.end);
                let c = egui::pos2(r.center().x, r.center().y);
                let rad = (s.size * 0.17).max(2.0);
                match d.level % 3 {
                    0 => p.circle_filled(c, rad, s.text),
                    1 => p.circle_stroke(c, rad, Stroke::new(1.2, s.text)),
                    _ => p.rect_filled(egui::Rect::from_center_size(c, egui::vec2(rad * 1.7, rad * 1.7)), 0.0, s.text),
                };
            }
            DECO_TASK | DECO_TASK_DONE => {
                let r = task_rect(out, text, d);
                if d.kind == DECO_TASK_DONE {
                    p.rect_filled(r, 3.0, s.accent);
                    let w = r.width();
                    let pts = [
                        r.left_top() + egui::vec2(w * 0.22, w * 0.52),
                        r.left_top() + egui::vec2(w * 0.42, w * 0.72),
                        r.left_top() + egui::vec2(w * 0.78, w * 0.3),
                    ];
                    p.line_segment([pts[0], pts[1]], Stroke::new(1.8, Color32::WHITE));
                    p.line_segment([pts[1], pts[2]], Stroke::new(1.8, Color32::WHITE));
                } else {
                    p.rect_stroke(r, 3.0, Stroke::new(1.3, s.muted), egui::StrokeKind::Inside);
                }
            }
            DECO_RULE => {
                let r = span_rect(out, text, d.start, d.end);
                p.line_segment(
                    [egui::pos2(field.left(), r.center().y), egui::pos2(field.right(), r.center().y)],
                    Stroke::new(1.0, s.muted.gamma_multiply(0.5)),
                );
            }
            _ => {}
        }
    }
}

/// Palabra mal escrita bajo el clic derecho: (bytes, palabra, sugerencias).
type SpellHit = Option<(std::ops::Range<usize>, String, Vec<String>)>;

/// Clic derecho sobre una palabra mal escrita: sugerencias, agregar al diccionario, ignorar.
/// `true` si cambió el texto.
fn spell_menu(ui: &Ui, id: Id, text: &mut String, sp: &Speller, out: &egui::text_edit::TextEditOutput) -> bool {
    let menu_id = id.with("spell-menu");
    let resp = &out.response.response;
    if resp.secondary_clicked() {
        let hit: SpellHit = ui.input(|i| i.pointer.interact_pos()).and_then(|p| {
            let c = out.galley.cursor_from_pos(p - out.galley_pos).index.0;
            let b = text.char_indices().nth(c).map_or(text.len(), |(i, _)| i);
            let f = formats(text);
            let r = sp.misspelled(text, |i| spell_skip(&f, i)).into_iter().find(|r| r.start <= b && b <= r.end)?;
            let w = text.get(r.clone())?.to_string();
            let sug = sp.suggest(&w);
            Some((r, w, sug))
        });
        ui.data_mut(|d| d.insert_temp(menu_id, hit));
    }
    let hit: SpellHit = ui.data(|d| d.get_temp::<SpellHit>(menu_id)).flatten();
    let Some((r, w, sug)) = hit else { return false };
    let mut changed = false;
    resp.context_menu(|ui| {
        let pick = iurefficient_ui::lang::pick;
        if sug.is_empty() {
            ui.weak(pick("No suggestions", "Sin sugerencias"));
        }
        for s in &sug {
            if ui.button(s).clicked() {
                if text.get(r.clone()) == Some(w.as_str()) {
                    text.replace_range(r.clone(), s);
                    changed = true;
                }
                ui.close();
            }
        }
        ui.separator();
        if ui.button(format!("{} «{w}»", pick("Add to dictionary", "Agregar al diccionario"))).clicked() {
            sp.add(&w);
            ui.close();
        }
        if ui.button(pick("Ignore", "Ignorar")).clicked() {
            sp.ignore(&w);
            ui.close();
        }
    });
    changed
}

/// El último dibujo de un campo (para saber en qué fila está el cursor).
pub(crate) fn cached_galley(ui: &Ui, id: Id) -> Option<Arc<egui::Galley>> {
    ui.data(|d| d.get_temp::<(u64, Arc<egui::Galley>)>(id.with("galley"))).map(|(_, g)| g)
}

#[cfg(test)]
mod tests {
    #[test]
    fn zoom_steps_stop_at_the_limits() {
        assert!((super::zoom_step(1.0, true) - 1.1).abs() < 1e-6);
        assert!((super::zoom_step(1.0, false) - 0.9).abs() < 1e-6);
        assert!((super::zoom_step(1.05, true) - 1.1).abs() < 1e-6);
        assert!((super::zoom_step(3.0, true) - 3.0).abs() < 1e-6);
        assert!((super::zoom_step(0.5, false) - 0.5).abs() < 1e-6);
        assert!((super::zoom_step(f32::NAN, true) - 1.0).abs() < 1e-6);
        assert!((super::zoom_step(9.0, true) - 3.0).abs() < 1e-6);
    }

    use super::*;

    fn fmt_at(text: &str, needle: &str) -> Fmt {
        let i = text.find(needle).unwrap_or(0);
        formats(text)[i]
    }

    #[test]
    fn styles_follow_the_markdown() {
        let t =
            "# Título\n\nTexto **negrita** y *cursiva* y `código` y [enlace](https://x.mx).\n\n- [ ] tarea\n> cita\n";
        assert_eq!(fmt_at(t, "Título").heading, 1);
        assert!(fmt_at(t, "# ").marker, "el # tenue");
        assert!(fmt_at(t, "negrita").bold && !fmt_at(t, "negrita").marker);
        assert!(fmt_at(t, "**negrita").marker);
        assert!(fmt_at(t, "cursiva").italic);
        assert!(fmt_at(t, "código").code);
        assert!(fmt_at(t, "enlace").link && fmt_at(t, "](https").marker);
        assert!(fmt_at(t, "- [ ]").bullet && fmt_at(t, "[ ] tarea").bullet);
        assert!(fmt_at(t, "cita").quote && fmt_at(t, "> cita").marker);
        assert!(!fmt_at(t, "Texto").bold);
    }

    /// S1.22: cada regla de entrada de §4.7 da su estilo al teclearla (Markdown en vivo: lo
    /// tecleado ya es el formato; borrar la marca lo deshace).
    #[test]
    fn input_rules_style_the_text() {
        let f = |t: &str, needle: &str| fmt_at(t, needle);
        for (level, t) in (1u8..=6).map(|n| (n, format!("{} T", "#".repeat(usize::from(n))))) {
            assert_eq!(f(&t, "T").heading, level, "{t}");
        }
        for t in ["- a", "* a", "+ a", "1. a", "1) a", "7. a"] {
            assert!(f(t, &t[..1]).bullet, "viñeta o número: {t}");
        }
        assert!(f("- [ ] a", "[ ]").bullet && f("- [x] a", "[x]").bullet, "tareas");
        assert!(f("> cita", "cita").quote);
        assert!(f("> [!NOTE] Aviso\n> cuerpo", "cuerpo").quote, "aviso");
        assert!(f("```rust\nfn f() {}\n```", "fn f").code, "bloque de código");
        assert!(f("a\n\n---\n\nb", "---").marker, "separador");
        for t in ["**n**", "__n__"] {
            assert!(f(t, "n").bold, "negrita: {t}");
        }
        for t in ["*c*", "_c_"] {
            assert!(f(t, "c").italic, "cursiva: {t}");
        }
        assert!(!f("snake_case_name", "case").italic, "sin falsos positivos en snake_case");
        // En CommonMark `2*3*4` sí es cursiva (el asterisco vale dentro de la palabra), igual que
        // en cualquier visor: el editor no lo contradice. Con espacios no lo es.
        assert!(f("2*3*4", "3").italic);
        assert!(!f("2 * 3 * 4", "3").italic, "con espacios, multiplicación");
        assert!(f("~~t~~", "t").strike);
        assert!(f("`x`", "x").code);
        let link = "[texto](https://x.mx)";
        assert!(f(link, "texto").link && f(link, "](https").marker);
        assert!(f("| a | b |\n|---|---|\n| 1 | 2 |", "|---").marker, "tabla");
        // Nada se convierte dentro de código.
        assert!(!f("`**no**`", "no").bold && !f("```\n# no\n```", "no").bold);
        assert_eq!(f("```\n# no\n```", "no").heading, 0);
    }

    #[test]
    fn code_blocks_are_highlighted_with_the_theme() {
        assert!(highlight::wait_ready(std::time::Duration::from_secs(20)));
        let t = "```rust\n// nota\nfn main() { \"hola\" }\n```\n\n`fn` en línea";
        assert_eq!(fmt_at(t, "fn main").syn, highlight::KEYWORD);
        assert_eq!(fmt_at(t, "nota").syn, highlight::COMMENT);
        assert_eq!(fmt_at(t, "hola").syn, highlight::STRING);
        assert_eq!(fmt_at(t, "```rust").syn, 0, "la cerca, sin color");
        assert_eq!(fmt_at(t, "fn` en").syn, 0, "el código en línea no se resalta");
        let s = EditorStyle {
            text: Color32::BLACK,
            muted: Color32::GRAY,
            accent: Color32::BLUE,
            code_bg: Color32::LIGHT_GRAY,
            size: 14.0,
            bold_family: FontFamily::Proportional,
            body_family: FontFamily::Proportional,
            error: Color32::RED,
            dark: false,
        };
        assert_eq!(text_format(fmt_at(t, "fn main"), &s).color, Color32::BLUE, "palabras clave en el acento");
        assert!(text_format(fmt_at(t, "nota"), &s).italics, "comentarios en cursiva");
    }

    #[test]
    fn inline_markers_hide_outside_the_cursor_line() {
        let s = EditorStyle {
            text: Color32::BLACK,
            muted: Color32::GRAY,
            accent: Color32::BLUE,
            code_bg: Color32::LIGHT_GRAY,
            size: 14.0,
            bold_family: FontFamily::Proportional,
            body_family: FontFamily::Proportional,
            error: Color32::RED,
            dark: false,
        };
        let t = "uno **dos**\ntres **cuatro**\n- lista";
        let hidden = |job: &LayoutJob, needle: &str| {
            let at = t.find(needle).unwrap_or(0);
            job.sections.iter().any(|x| {
                x.byte_range.start.0 <= at && at < x.byte_range.end.0 && x.format.color == Color32::TRANSPARENT
            })
        };
        // Cursor en la primera línea: sus marcas se ven, las de la segunda no.
        let job = layout_job(t, &s, 300.0, Some(0..11));
        assert!(!hidden(&job, "**dos"));
        assert!(hidden(&job, "**cuatro"));
        // La viñeta se dibuja (su `-` queda invisible con su ancho), también en la línea del cursor.
        assert!(hidden(&job, "- lista"), "la viñeta se dibuja, no se ve el guion");
        // Sin cursor: todas ocultas.
        let job = layout_job(t, &s, 300.0, None);
        assert!(hidden(&job, "**dos") && hidden(&job, "**cuatro"));
    }

    fn test_style() -> EditorStyle {
        EditorStyle {
            text: Color32::BLACK,
            muted: Color32::GRAY,
            accent: Color32::BLUE,
            code_bg: Color32::LIGHT_GRAY,
            size: 14.0,
            bold_family: FontFamily::Proportional,
            body_family: FontFamily::Proportional,
            error: Color32::RED,
            dark: false,
        }
    }

    /// WYSIWYG: viñetas, casillas, citas, cercas y reglas son marcas dibujadas; el texto no cambia.
    #[test]
    fn inline_formulas_leave_a_hole_off_the_cursor_line() {
        let s = test_style();
        let t = "Área $x^2$ total";
        assert_eq!(inline_maths(t), ["x^2"]);
        let m = [MathImg { latex: "x^2".into(), tex: egui::TextureId::Managed(0), size: egui::vec2(30.0, 18.0) }];
        let (job, decos) = layout_job_full(t, &s, 400.0, None, None, None, &m);
        assert_eq!(decos.iter().filter(|d| d.kind == DECO_MATH).count(), 1);
        let hole = job.sections.iter().find(|x| x.format.extra_letter_spacing > 1.0).map(|x| x.byte_range.clone());
        assert_eq!(hole.map(|r| (r.start.0, r.end.0)), Some((t.find('$').unwrap(), t.find('$').unwrap() + 1)));
        // En la línea del cursor se ve el LaTeX.
        let (job, decos) = layout_job_full(t, &s, 400.0, Some(0..t.len()), None, None, &m);
        assert!(decos.iter().all(|d| d.kind != DECO_MATH));
        assert!(job.sections.iter().all(|x| x.format.extra_letter_spacing < 1.0));
    }

    #[test]
    fn colors_and_highlights_paint_the_text_and_hide_their_tags() {
        let t = "a <span style=\"color: #FF0000\">rojo</span> y <mark style=\"background-color: #FFF2CC\">amarillo</mark> <mark>x</mark>";
        let f = formats(t);
        let at = |n: &str| f.get(t.find(n).unwrap()).copied().unwrap();
        assert_eq!(at("rojo").fg, 0x01FF_0000);
        assert_eq!(at("amarillo").bg, 0x01FF_F2CC);
        assert_eq!(at("x</mark>").bg, 0x01FF_F59D, "sin color: amarillo");
        assert!(at("<span").inline && at("</mark>").inline, "las etiquetas se ocultan fuera de su línea");
        assert_eq!(at(" y ").fg, 0);
    }

    #[test]
    fn callouts_show_their_title_off_their_line() {
        let s = test_style();
        assert_eq!(callout_of("> [!tip] Ojo\n> texto"), Some(1));
        assert_eq!(callout_of("> [!MINDMAP] Tema"), None, "los mapas son diagramas");
        assert_eq!(callout_of("> cita"), None);
        let t = "> [!WARNING]\n> Plazo el lunes.";
        let (_, decos) = layout_job_spell(t, &s, 400.0, None, None, None);
        let d = decos.iter().find(|d| d.kind >= DECO_CALLOUT).copied();
        assert_eq!(d.map(|d| (d.kind - DECO_CALLOUT, t.get(d.start..d.end))), Some((3, Some("[!WARNING]"))));
        // Con el cursor en esa línea se ve (y se edita) la marca.
        let (_, decos) = layout_job_spell(t, &s, 400.0, Some(0..12), None, None);
        assert!(decos.iter().all(|d| d.kind < DECO_CALLOUT));
    }

    #[test]
    fn block_marks_are_drawn_not_shown() {
        let s = test_style();
        let t = "- uno\n  - dos\n- [ ] tarea\n- [x] hecha\n\n> cita\n\n```rust\nfn f() {}\n```\n\n---\n\n1. numerada\n";
        let (job, decos) = layout_job_spell(t, &s, 400.0, None, None, None);
        let kinds = |k: u8| decos.iter().filter(|d| d.kind == k).count();
        assert_eq!(kinds(DECO_BULLET), 2, "{decos:?}");
        assert_eq!(kinds(DECO_TASK), 1);
        assert_eq!(kinds(DECO_TASK_DONE), 1);
        assert_eq!(kinds(DECO_QUOTE), 1);
        assert_eq!(kinds(DECO_RULE), 1);
        // La viñeta anidada es de nivel 1.
        let nested = decos.iter().filter(|d| d.kind == DECO_BULLET).nth(1).copied();
        assert_eq!(nested.map(|d| (d.level, t.get(d.start..d.end))), Some((1, Some("-"))));
        // La casilla cubre `[ ]` / `[x]`.
        let task = decos.iter().find(|d| d.kind == DECO_TASK_DONE).copied();
        assert_eq!(task.and_then(|d| t.get(d.start..d.end)), Some("[x]"));
        // Los números de una lista numerada se ven tal cual.
        assert!(decos.iter().all(|d| !t.get(d.start..d.end).unwrap_or("").contains('1')));
        // Sin el cursor en el bloque las cercas se pliegan (sin ancho); con él se ven.
        let fence_hidden = |job: &LayoutJob| {
            let at = t.find("```rust").unwrap_or(0);
            job.sections
                .iter()
                .any(|x| x.byte_range.start.0 <= at && at < x.byte_range.end.0 && x.format.font_id.size < 1.0)
        };
        assert!(fence_hidden(&job));
        let (job, decos) = layout_job_spell(t, &s, 400.0, Some(0..0), None, None);
        assert!(!fence_hidden(&job), "con el cursor, las cercas se editan");
        assert_eq!(decos.iter().filter(|d| d.kind == DECO_FENCE).count(), 2);
        // La tarea hecha va tenue y tachada.
        let done_fmt = formats(t).get(t.find("hecha").unwrap_or(0)).copied().unwrap_or_default();
        assert!(done_fmt.done);
        // Cubre cada byte (el texto no cambia).
        let covered: usize = job.sections.iter().map(|x| x.byte_range.end.0 - x.byte_range.start.0).sum();
        assert_eq!(covered, t.len());
    }

    /// Una lista dentro de una cita conserva su sangría (antes se comía los espacios).
    #[test]
    fn nested_lists_keep_their_indent() {
        let f = formats("- a\n  - b\n");
        assert!(f.get(4).is_some_and(|x| x.indent), "los dos espacios son sangría");
        let f = formats("> - a\n>   - b\n");
        let at = "> - a\n>".len();
        assert!(f.get(at + 1).is_some_and(|x| x.indent), "sangría dentro de la cita");
        let (job, _) = layout_job_spell("- a\n  - b\n", &test_style(), 400.0, None, None, None);
        assert!(job.sections.iter().any(|x| x.leading_space > 5.0), "la sangría se ve más ancha");
    }

    #[test]
    fn misspelled_words_are_underlined_except_while_typing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("es_MX.aff"), "SET UTF-8\nTRY esianrtolcdugmphbyfvkwz\n").unwrap();
        std::fs::write(dir.path().join("es_MX.dic"), "2\nhola\nmundo\n").unwrap();
        let sp = Speller::new(&["es"], vec![dir.path().to_path_buf()], None);
        for _ in 0..500 {
            if sp.is_ready() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let s = EditorStyle {
            text: Color32::BLACK,
            muted: Color32::GRAY,
            accent: Color32::BLUE,
            code_bg: Color32::LIGHT_GRAY,
            size: 14.0,
            bold_family: FontFamily::Proportional,
            body_family: FontFamily::Proportional,
            error: Color32::RED,
            dark: false,
        };
        let t = "hola holla `holla` mundo";
        let red = |job: &LayoutJob, at: usize| {
            job.sections.iter().any(|x| {
                x.byte_range.start.0 <= at && at < x.byte_range.end.0 && x.format.underline.color == Color32::RED
            })
        };
        let job = layout_job_spell(t, &s, 300.0, None, Some(&sp), None).0;
        assert!(red(&job, 6), "«holla» subrayada");
        assert!(!red(&job, 1) && !red(&job, 13), "bien escrita y dentro de código, no");
        let job = layout_job_spell(t, &s, 300.0, None, Some(&sp), Some(10)).0;
        assert!(!red(&job, 6), "no mientras se teclea");
    }

    #[test]
    fn layout_covers_every_byte() {
        let s = EditorStyle {
            text: Color32::BLACK,
            muted: Color32::GRAY,
            accent: Color32::BLUE,
            code_bg: Color32::LIGHT_GRAY,
            size: 14.0,
            bold_family: FontFamily::Proportional,
            body_family: FontFamily::Proportional,
            error: Color32::RED,
            dark: false,
        };
        for t in ["", "hola", "# a\n**b** ñú `c`\n- [x] d", "```\ncódigo\n```"] {
            let job = layout_job(t, &s, 300.0, None);
            assert_eq!(job.text, t);
            let covered: usize = job.sections.iter().map(|x| x.byte_range.end.0 - x.byte_range.start.0).sum();
            assert_eq!(covered, t.len(), "{t:?}");
        }
    }
}

#[cfg(test)]
mod perf {
    use super::*;

    /// Unas 100 páginas: cuánto tarda pintar el estilo (se mide; el límite es holgado en depuración).
    #[test]
    fn hundred_pages_layout_time() {
        let page = "## Sección\n\nTexto con **negrita**, *cursiva*, `código` y [enlace](https://x.mx).\n\n- uno\n- [ ] dos\n  - tres\n\n> cita\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n```rust\nfn f() {}\n```\n\n";
        // ~250 KB: unas 100 páginas de texto con formato.
        let text = page.repeat(100 * 15);
        let s = EditorStyle {
            text: Color32::BLACK,
            muted: Color32::GRAY,
            accent: Color32::BLUE,
            code_bg: Color32::LIGHT_GRAY,
            size: 14.0,
            bold_family: FontFamily::Proportional,
            body_family: FontFamily::Proportional,
            error: Color32::RED,
            dark: false,
        };
        // Con las gramáticas ya cargadas (se cargan una vez, en otro hilo).
        let _ = highlight::wait_ready(std::time::Duration::from_secs(20));
        let _ = formats(page);
        let tf = std::time::Instant::now();
        let _f = formats(&text);
        eprintln!("solo formats: {:.1} ms", tf.elapsed().as_secs_f64() * 1000.0);
        let t0 = std::time::Instant::now();
        let job = layout_job(&text, &s, 800.0, Some(0..10));
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        eprintln!("100 páginas ({} KB, {} tramos): {ms:.1} ms", text.len() / 1024, job.sections.len());
        // Lo que hace egui con ese trabajo: los glifos de todo el texto (al teclear, otra vez).
        let ctx = egui::Context::default();
        let mut glyph_ms = 0.0;
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let t = std::time::Instant::now();
            let g = ui.fonts_mut(|f| f.layout_job(job.clone()));
            glyph_ms = t.elapsed().as_secs_f64() * 1000.0;
            eprintln!("glifos de egui: {glyph_ms:.1} ms ({} filas)", g.rows.len());
        });
        out.textures_delta.clear();
        // Holgado: en depuración el resaltado (fancy-regex) es mucho más lento que optimizado.
        let limit = if cfg!(debug_assertions) { 30_000.0 } else { 2_000.0 };
        assert!(ms < limit, "{ms} ms");
    }
}
