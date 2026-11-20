//! PDF con Typst, compilado aquí (sin programas externos) y con las letras que trae Typst
//! (Libertinus Serif para el texto, DejaVu Sans Mono para el código): el PDF sale igual en
//! cualquier sistema.
//!
//! El documento se traduce a **código** de Typst en el que todo el texto va en cadenas
//! (`"…"`): nada del documento se interpreta como marcado ni como código, solo hay que escapar
//! las cadenas. Imágenes y diagramas son archivos virtuales (`/img/N.png`) que [`DocWorld`] sirve
//! desde memoria; Typst no lee el disco ni la red.
//!
//! Si una imagen está dañada Typst no compila: se vuelve a intentar con el texto alternativo en su
//! lugar.

use std::collections::HashMap;

use iure_doc::{Align, Block, Document, Inline, Item, Style};
use typst::foundations::Bytes;
use typst::syntax::Source;

use crate::world::{DocWorld, file_id};

use crate::text::{raw_paragraphs, safe_url};
use crate::{ExportError, Options, Paper, Picture, Resources, title_of};

/// Anidado máximo que se recorre.
const MAX_DEPTH: usize = 32;
/// Imágenes como mucho (cada una es un archivo virtual).
const MAX_MEDIA: usize = 1_000;
/// Margen de la página, en puntos (2,5 cm).
const MARGIN_PT: f64 = 70.87;

/// El documento como PDF.
pub fn pdf(doc: &Document, res: &dyn Resources, opts: &Options) -> Result<Vec<u8>, ExportError> {
    match compile(doc, res, opts, true) {
        Ok(b) => Ok(b),
        // ¿Una imagen dañada? Sin imágenes.
        Err(_) => compile(doc, res, opts, false),
    }
}

fn compile(doc: &Document, res: &dyn Resources, opts: &Options, images: bool) -> Result<Vec<u8>, ExportError> {
    let (source, media) = typst_source(doc, res, opts, images);
    let main_id = file_id("/main.typ").ok_or_else(|| fail("main"))?;
    let mut files = HashMap::new();
    for (i, bytes) in media.into_iter().enumerate() {
        let id = file_id(&format!("/img/{i}")).ok_or_else(|| fail("img"))?;
        files.insert(id, Bytes::new(bytes));
    }
    let world = DocWorld { main: Source::new(main_id, source), files };
    let paged: typst_layout::PagedDocument = typst::compile(&world)
        .output
        .map_err(|errs| fail(&errs.first().map_or_else(String::new, |e| e.message.to_string())))?;
    typst_pdf::pdf(&paged, &typst_pdf::PdfOptions::default())
        .map_err(|errs| fail(&errs.first().map_or_else(String::new, |e| e.message.to_string())))
}

fn fail(detail: &str) -> ExportError {
    ExportError {
        en: format!("The PDF could not be made ({detail})."),
        es: format!("No se pudo hacer el PDF ({detail})."),
    }
}

/// Una cadena de Typst (`"…"`), escapada.
fn lit(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// El código de Typst del documento y las imágenes que usa.
pub(crate) fn typst_source(
    doc: &Document,
    res: &dyn Resources,
    opts: &Options,
    images: bool,
) -> (String, Vec<Vec<u8>>) {
    let (paper, width_pt) = match opts.paper {
        Paper::Letter => ("us-letter", 612.0),
        Paper::A4 => ("a4", 595.28),
    };
    let lang: String = opts.lang.chars().filter(char::is_ascii_lowercase).take(3).collect();
    let lang = if lang.is_empty() { "es".to_string() } else { lang };
    let mut w = Writer { res, images, media: Vec::new(), text_width: width_pt - 2.0 * MARGIN_PT };
    let mut body = String::new();
    for n in doc.nodes() {
        if let Some(b) = w.block(&n.block, 0) {
            body.push_str("  ");
            body.push_str(&b);
            body.push_str("\n  parbreak()\n");
        }
    }
    let title = title_of(doc, opts);
    let src = format!(
        "#set document(title: {title})\n\
         #set page(paper: \"{paper}\", margin: {MARGIN_PT}pt, numbering: \"1\")\n\
         #set text(font: \"Libertinus Serif\", size: 11pt, lang: \"{lang}\", hyphenate: true)\n\
         #set par(justify: true, leading: 0.62em, spacing: 1.1em)\n\
         #show raw: set text(font: \"DejaVu Sans Mono\", size: 0.85em)\n\
         #show raw.where(block: true): block.with(fill: luma(246), inset: 8pt, radius: 3pt, width: 100%)\n\
         #show raw.where(block: false): box.with(fill: luma(240), inset: (x: 2pt), outset: (y: 2pt), radius: 2pt)\n\
         #show heading: set block(above: 1.4em, below: 0.7em)\n\
         #show heading.where(level: 1): set text(size: 1.6em)\n\
         #show heading.where(level: 2): set text(size: 1.3em)\n\
         #show link: set text(fill: rgb(\"#0b57d0\"))\n\
         #show quote.where(block: true): block.with(stroke: (left: 2pt + luma(200)), inset: (left: 10pt, y: 4pt))\n\
         #set table(stroke: 0.5pt + luma(190), inset: 6pt)\n\
         #show table.cell.where(y: 0): strong\n\
         #{{\n{body}}}\n",
        title = lit(&title),
    );
    (src, w.media)
}

struct Writer<'a> {
    res: &'a dyn Resources,
    images: bool,
    media: Vec<Vec<u8>>,
    /// Ancho del texto, en puntos.
    text_width: f64,
}

impl Writer<'_> {
    /// Una imagen como archivo virtual: `image(...)` a su tamaño (96 ppp) o a `points`, sin pasar
    /// del ancho del texto.
    fn image(&mut self, p: &Picture, points: Option<(f32, f32)>) -> Option<String> {
        if !self.images || self.media.len() >= MAX_MEDIA {
            return None;
        }
        let w = match points {
            Some((w, _)) if w > 0.0 => f64::from(w),
            _ => f64::from(p.width) * 0.75,
        };
        let w = w.clamp(1.0, self.text_width);
        let i = self.media.len();
        self.media.push(p.bytes.clone());
        Some(format!("image(\"/img/{i}\", width: {w:.1}pt)"))
    }

    fn diagram(&mut self, b: &Block) -> Option<String> {
        let d = self.res.diagram(&iure_doc::write::block(b))?;
        let png = d.png.as_ref()?;
        let pts = (d.points.0 > 0.0 && d.points.1 > 0.0).then_some(d.points);
        let img = self.image(png, pts)?;
        Some(format!("align(center, {img})"))
    }

    /// Un bloque como expresión de Typst (contenido), o nada.
    fn block(&mut self, b: &Block, depth: usize) -> Option<String> {
        if depth > MAX_DEPTH {
            return None;
        }
        Some(match b {
            Block::Paragraph(c) => {
                if let [Inline::Image { src, .. }] = c.as_slice()
                    && let Some(p) = self.res.image(src)
                    && let Some(img) = self.image(&p, None)
                {
                    return Some(format!("align(center, {img})"));
                }
                format!("par({{ {} }})", self.inlines(c, Style::default()))
            }
            Block::Heading { level, content } => {
                format!("heading(level: {}, {{ {} }})", (*level).clamp(1, 6), self.inlines(content, Style::default()))
            }
            Block::List { ordered, start, tight, items } => self.list(*ordered, *start, *tight, items, depth),
            Block::Quote(blocks) => format!("quote(block: true, {{ {} }})", self.blocks(blocks, depth)),
            Block::Callout { title, blocks, .. } => {
                if let Some(d) = self.diagram(b) {
                    return Some(d);
                }
                let head = if title.trim().is_empty() {
                    String::new()
                } else {
                    format!("strong({}); parbreak(); ", lit(title.trim()))
                };
                format!(
                    "block(fill: rgb(\"#f2f7ff\"), stroke: (left: 3pt + rgb(\"#0969da\")), inset: 9pt, radius: 2pt, width: 100%, {{ {head}{} }})",
                    self.blocks(blocks, depth)
                )
            }
            Block::Code { lang, text } => {
                if let Some(d) = self.diagram(b) {
                    return Some(d);
                }
                let l: String = lang
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '+' | '#'))
                    .take(32)
                    .collect();
                let lang = if l.is_empty() { "none".to_string() } else { lit(&l) };
                format!("raw(block: true, lang: {lang}, {})", lit(text.trim_end_matches('\n')))
            }
            Block::Math(m) => match crate::math::checked(m.trim()) {
                Some(t) => crate::math::equation(&t, true),
                None => format!("align(center, emph({}))", lit(m.trim())),
            },
            Block::Table { align, head, rows } => self.table(align, head, rows),
            Block::Rule => "line(length: 100%, stroke: 0.5pt + luma(190))".to_string(),
            Block::FrontMatter(_) => return None,
            Block::Raw(md) => {
                let ps: Vec<String> = raw_paragraphs(md).iter().map(|p| format!("par({})", lit(p))).collect();
                if ps.is_empty() {
                    return None;
                }
                format!("{{ {} }}", ps.join("; parbreak(); "))
            }
        })
    }

    fn blocks(&mut self, blocks: &[Block], depth: usize) -> String {
        let parts: Vec<String> = blocks.iter().filter_map(|b| self.block(b, depth + 1)).collect();
        parts.join("; parbreak(); ")
    }

    fn list(&mut self, ordered: bool, start: u64, tight: bool, items: &[Item], depth: usize) -> String {
        let mut parts = Vec::new();
        for item in items.iter().take(100_000) {
            let mut content = match item.checked {
                Some(true) => "box(stroke: 0.6pt, width: 0.8em, height: 0.8em, baseline: 0.1em, align(center + horizon, text(font: \"DejaVu Sans Mono\", size: 0.7em, \"✓\"))); h(0.4em); ".to_string(),
                Some(false) => "box(stroke: 0.6pt, width: 0.8em, height: 0.8em, baseline: 0.1em); h(0.4em); ".to_string(),
                None => String::new(),
            };
            // El primer párrafo va en línea (sin `par`), como en una lista compacta.
            let mut rest: &[Block] = &item.blocks;
            if let Some((Block::Paragraph(c), tail)) = item.blocks.split_first() {
                let style = Style { strike: item.checked == Some(true), ..Style::default() };
                content.push_str(&self.inlines(c, style));
                rest = tail;
            }
            if !rest.is_empty() {
                content.push_str("; parbreak(); ");
                content.push_str(&self.blocks(rest, depth));
            }
            parts.push(format!("{{ {content} }}"));
        }
        // Una lista de tareas lleva la casilla en lugar de la viñeta.
        let tasks = !items.is_empty() && items.iter().all(|i| i.checked.is_some());
        let func = match (ordered, tasks) {
            (true, _) => format!("enum(start: {}, ", start.min(999_999)),
            (false, true) => "list(marker: [], body-indent: 0pt, ".to_string(),
            (false, false) => "list(".to_string(),
        };
        format!("{func}tight: {tight}, {})", parts.join(", "))
    }

    fn table(&mut self, align: &[Align], head: &[Vec<Inline>], rows: &[Vec<Vec<Inline>>]) -> String {
        let cols = head.len().max(rows.iter().map(Vec::len).max().unwrap_or(0)).clamp(1, 64);
        let aligns: Vec<&str> = (0..cols)
            .map(|i| match align.get(i).copied().unwrap_or_default() {
                Align::Center => "center",
                Align::Right => "right",
                Align::None | Align::Left => "left",
            })
            .collect();
        let mut cell = |row: &[Vec<Inline>], i: usize| match row.get(i) {
            Some(c) => format!("{{ {} }}", self.inlines(c, Style::default())),
            None => "[]".to_string(),
        };
        let header: Vec<String> = (0..cols).map(|i| cell(head, i)).collect();
        let mut body = Vec::new();
        for row in rows.iter().take(100_000) {
            for i in 0..cols {
                body.push(cell(row, i));
            }
        }
        format!(
            "table(columns: {cols}, align: ({},), table.header({}), {})",
            aligns.join(", "),
            header.join(", "),
            body.join(", ")
        )
    }

    /// Contenido en línea como una secuencia de expresiones (`"a"; strong("b"); …`).
    fn inlines(&mut self, c: &[Inline], base: Style) -> String {
        let parts: Vec<String> = c.iter().filter_map(|i| self.inline(i, base)).collect();
        if parts.is_empty() { "[]".to_string() } else { parts.join("; ") }
    }

    fn inline(&mut self, i: &Inline, base: Style) -> Option<String> {
        let merge = |s: Style| Style {
            bold: s.bold || base.bold,
            italic: s.italic || base.italic,
            strike: s.strike || base.strike,
        };
        Some(match i {
            Inline::Text { text, style } => styled(lit(text), merge(*style)),
            Inline::Code { text, style } => styled(format!("raw({})", lit(text)), merge(*style)),
            Inline::Link { url, content, .. } => {
                let inner = self.inlines(content, base);
                match safe_url(url).filter(|u| u.contains(':')) {
                    Some(u) => format!("link({}, {{ {inner} }})", lit(&u)),
                    None => format!("{{ {inner} }}"),
                }
            }
            Inline::Image { src, alt, .. } => {
                match self.res.image(src).and_then(|p| self.image(&p, None)).map(|img| format!("box({img})")) {
                    Some(b) => b,
                    None => styled(lit(alt), base),
                }
            }
            Inline::Math(m) => match crate::math::checked(m) {
                Some(t) => crate::math::equation(&t, false),
                None => format!("emph({})", lit(m)),
            },
            Inline::Break => "linebreak()".to_string(),
            Inline::SoftBreak => "\" \"".to_string(),
            Inline::Html(h) => styled(lit(h), base),
        })
    }
}

/// Aplica negrita, cursiva y tachado a una expresión.
fn styled(expr: String, s: Style) -> String {
    let mut e = expr;
    if s.strike {
        e = format!("strike({e})");
    }
    if s.italic {
        e = format!("emph({e})");
    }
    if s.bold {
        e = format!("strong({e})");
    }
    e
}
