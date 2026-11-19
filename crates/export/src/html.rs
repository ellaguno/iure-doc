//! HTML autocontenido: una sola página con sus estilos, imágenes en `data:` y diagramas en SVG.
//!
//! El HTML que trae el Markdown (bloques y en línea) **no** pasa: se exporta como texto. Así un
//! documento recibido no puede meter guiones ni formularios en la página exportada.

use iure_doc::{Align, Block, Document, Inline, Item, Style};

use crate::text::{base64, escape, raw_paragraphs, safe_url};
use crate::{Options, Picture, Resources, title_of};

/// Anidado máximo que se recorre (citas, listas, avisos).
const MAX_DEPTH: usize = 32;

/// Estilos de la página: legibles en pantalla e impresos, sin letras externas.
const CSS: &str = r#"
:root { color-scheme: light; }
body { margin: 0; background: #fff; color: #1f2328;
  font: 16px/1.6 -apple-system, "Segoe UI", system-ui, "Noto Sans", Ubuntu, Cantarell, sans-serif; }
main { max-width: 46rem; margin: 0 auto; padding: 2.5rem 1.25rem 4rem; }
h1, h2, h3, h4, h5, h6 { line-height: 1.25; margin: 1.6em 0 .6em; }
h1 { font-size: 2em; } h2 { font-size: 1.5em; } h3 { font-size: 1.25em; }
h4 { font-size: 1.05em; } h5, h6 { font-size: 1em; color: #59636e; }
p, ul, ol, blockquote, pre, table, figure, aside { margin: 0 0 1em; }
a { color: #0969da; }
code, pre { font-family: ui-monospace, "Cascadia Mono", "SFMono-Regular", Menlo, Consolas, monospace; font-size: .9em; }
code { background: #eff1f3; border-radius: 4px; padding: .1em .35em; }
pre { background: #f6f8fa; border: 1px solid #d1d9e0; border-radius: 6px; padding: .8em 1em; overflow-x: auto; }
pre code { background: none; padding: 0; }
blockquote { border-left: 4px solid #d1d9e0; color: #59636e; margin-left: 0; padding: 0 1em; }
ul.tasks { list-style: none; padding-left: 1.2em; }
ul.tasks input { margin: 0 .45em 0 -1.2em; }
table { border-collapse: collapse; display: block; overflow-x: auto; }
th, td { border: 1px solid #d1d9e0; padding: .35em .75em; }
th { background: #f6f8fa; }
hr { border: 0; border-top: 1px solid #d1d9e0; margin: 2em 0; }
img { max-width: 100%; height: auto; }
figure { text-align: center; }
figure svg { max-width: 100%; height: auto; }
.math { font-family: "Cambria Math", "STIX Two Math", serif; font-style: italic; }
aside.callout { border-left: 4px solid #0969da; background: #f2f7ff; border-radius: 0 6px 6px 0; padding: .6em 1em; }
aside.callout > p.title { font-weight: 600; margin-bottom: .4em; }
aside.warning, aside.caution { border-color: #bf8700; background: #fff8e5; }
aside.danger, aside.important { border-color: #cf222e; background: #fff0f0; }
aside.tip, aside.success { border-color: #1a7f37; background: #effaf2; }
@media print { main { max-width: none; padding: 0; } pre, table, figure { break-inside: avoid; } }
"#;

/// El documento como una página HTML completa.
pub fn html(doc: &Document, res: &dyn Resources, opts: &Options) -> String {
    let title = title_of(doc, opts);
    let lang = if opts.lang.trim().is_empty() { "es" } else { opts.lang.trim() };
    let lang: String = lang.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(16).collect();
    let mut w = Writer { res, out: String::new() };
    for n in doc.nodes() {
        w.block(&n.block, 0);
    }
    format!(
        "<!DOCTYPE html>\n<html lang=\"{lang}\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <meta name=\"generator\" content=\"IureDoc\">\n<title>{}</title>\n<style>{CSS}</style>\n</head>\n\
         <body>\n<main>\n{}</main>\n</body>\n</html>\n",
        escape(&title),
        w.out
    )
}

/// Solo el cuerpo (los bloques), para incrustar en otra página o en un correo.
pub fn body(doc: &Document, res: &dyn Resources) -> String {
    let mut w = Writer { res, out: String::new() };
    for n in doc.nodes() {
        w.block(&n.block, 0);
    }
    w.out
}

fn img_tag(p: &Picture, alt: &str, title: &str) -> String {
    let t = if title.is_empty() { String::new() } else { format!(" title=\"{}\"", escape(title)) };
    format!(
        "<img src=\"data:{};base64,{}\" alt=\"{}\" width=\"{}\" height=\"{}\"{t}>",
        escape(&p.mime),
        base64(&p.bytes),
        escape(alt),
        p.width,
        p.height
    )
}

struct Writer<'a> {
    res: &'a dyn Resources,
    out: String,
}

impl Writer<'_> {
    fn blocks(&mut self, blocks: &[Block], depth: usize) {
        for b in blocks {
            self.block(b, depth);
        }
    }

    /// Un diagrama si la app lo dibuja; `false` si no.
    fn diagram(&mut self, b: &Block) -> bool {
        let Some(d) = self.res.diagram(&iure_doc::write::block(b)) else { return false };
        let (w, h) = (d.points.0.max(1.0).round(), d.points.1.max(1.0).round());
        if let Some(svg) = d.svg.as_deref().filter(|s| s.trim_start().starts_with("<svg")) {
            // El SVG lo genera la app (no viene del documento): va tal cual.
            self.out.push_str(&format!("<figure class=\"diagram\" style=\"max-width:{w}pt\">{svg}</figure>\n"));
            return true;
        }
        if let Some(p) = &d.png {
            self.out.push_str(&format!(
                "<figure class=\"diagram\"><img src=\"data:{};base64,{}\" alt=\"\" style=\"width:{w}pt;max-width:100%;height:auto\" data-h=\"{h}\"></figure>\n",
                escape(&p.mime),
                base64(&p.bytes)
            ));
            return true;
        }
        false
    }

    fn block(&mut self, b: &Block, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        match b {
            Block::Paragraph(c) => {
                // Un párrafo que es solo una imagen va como figura.
                if let [Inline::Image { src, alt, title }] = c.as_slice()
                    && let Some(p) = self.res.image(src)
                {
                    self.out.push_str(&format!("<figure>{}</figure>\n", img_tag(&p, alt, title)));
                    return;
                }
                self.out.push_str("<p>");
                self.inlines(c);
                self.out.push_str("</p>\n");
            }
            Block::Heading { level, content } => {
                let l = (*level).clamp(1, 6);
                self.out.push_str(&format!("<h{l}>"));
                self.inlines(content);
                self.out.push_str(&format!("</h{l}>\n"));
            }
            Block::List { ordered, start, items, .. } => self.list(*ordered, *start, items, depth),
            Block::Quote(blocks) => {
                self.out.push_str("<blockquote>\n");
                self.blocks(blocks, depth + 1);
                self.out.push_str("</blockquote>\n");
            }
            Block::Callout { kind, title, blocks, .. } => {
                if self.diagram(b) {
                    return;
                }
                let k: String = kind
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric())
                    .take(24)
                    .collect::<String>()
                    .to_ascii_lowercase();
                self.out.push_str(&format!("<aside class=\"callout {k}\">\n"));
                if !title.trim().is_empty() {
                    self.out.push_str(&format!("<p class=\"title\">{}</p>\n", escape(title.trim())));
                }
                self.blocks(blocks, depth + 1);
                self.out.push_str("</aside>\n");
            }
            Block::Code { lang, text } => {
                if self.diagram(b) {
                    return;
                }
                let l: String = lang
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '+' | '#'))
                    .take(32)
                    .collect();
                let class = if l.is_empty() { String::new() } else { format!(" class=\"language-{l}\"") };
                self.out.push_str(&format!("<pre><code{class}>{}</code></pre>\n", escape(text.trim_end_matches('\n'))));
            }
            Block::Math(m) => self.out.push_str(&format!("<p class=\"math\">{}</p>\n", escape(m.trim()))),
            Block::Table { align, head, rows } => {
                let style = |i: usize| match align.get(i).copied().unwrap_or_default() {
                    Align::None => "",
                    Align::Left => " style=\"text-align:left\"",
                    Align::Center => " style=\"text-align:center\"",
                    Align::Right => " style=\"text-align:right\"",
                };
                self.out.push_str("<table>\n<thead><tr>");
                for (i, cell) in head.iter().enumerate() {
                    self.out.push_str(&format!("<th{}>", style(i)));
                    self.inlines(cell);
                    self.out.push_str("</th>");
                }
                self.out.push_str("</tr></thead>\n<tbody>\n");
                for row in rows {
                    self.out.push_str("<tr>");
                    for (i, cell) in row.iter().enumerate() {
                        self.out.push_str(&format!("<td{}>", style(i)));
                        self.inlines(cell);
                        self.out.push_str("</td>");
                    }
                    self.out.push_str("</tr>\n");
                }
                self.out.push_str("</tbody>\n</table>\n");
            }
            Block::Rule => self.out.push_str("<hr>\n"),
            Block::FrontMatter(_) => {}
            Block::Raw(md) => {
                for p in raw_paragraphs(md) {
                    self.out.push_str(&format!("<p>{}</p>\n", escape(&p)));
                }
            }
        }
    }

    fn list(&mut self, ordered: bool, start: u64, items: &[Item], depth: usize) {
        let tasks = items.iter().any(|i| i.checked.is_some());
        let open = match (ordered, start) {
            (true, 1) => "<ol>".to_string(),
            (true, s) => format!("<ol start=\"{s}\">"),
            (false, _) if tasks => "<ul class=\"tasks\">".to_string(),
            (false, _) => "<ul>".to_string(),
        };
        self.out.push_str(&open);
        self.out.push('\n');
        for item in items {
            self.out.push_str("<li>");
            if let Some(done) = item.checked {
                self.out.push_str(if done {
                    "<input type=\"checkbox\" disabled checked>"
                } else {
                    "<input type=\"checkbox\" disabled>"
                });
            }
            // Un elemento de un solo párrafo, sin `<p>` (listas compactas).
            match item.blocks.as_slice() {
                [Block::Paragraph(c)] => self.inlines(c),
                blocks => {
                    self.out.push('\n');
                    self.blocks(blocks, depth + 1);
                }
            }
            self.out.push_str("</li>\n");
        }
        self.out.push_str(if ordered { "</ol>\n" } else { "</ul>\n" });
    }

    fn inlines(&mut self, c: &[Inline]) {
        for i in c {
            self.inline(i);
        }
    }

    fn styled(&mut self, style: Style, inner: String) {
        let mut s = inner;
        if style.strike {
            s = format!("<del>{s}</del>");
        }
        if style.italic {
            s = format!("<em>{s}</em>");
        }
        if style.bold {
            s = format!("<strong>{s}</strong>");
        }
        self.out.push_str(&s);
    }

    fn inline(&mut self, i: &Inline) {
        match i {
            Inline::Text { text, style } => self.styled(*style, escape(text)),
            Inline::Code { text, style } => self.styled(*style, format!("<code>{}</code>", escape(text))),
            Inline::Link { url, title, content } => match safe_url(url) {
                Some(u) => {
                    let t = if title.is_empty() { String::new() } else { format!(" title=\"{}\"", escape(title)) };
                    self.out.push_str(&format!("<a href=\"{}\"{t}>", escape(&u)));
                    self.inlines(content);
                    self.out.push_str("</a>");
                }
                None => self.inlines(content),
            },
            Inline::Image { src, alt, title } => match self.res.image(src) {
                Some(p) => self.out.push_str(&img_tag(&p, alt, title)),
                None => self.out.push_str(&escape(alt)),
            },
            Inline::Math(m) => self.out.push_str(&format!("<span class=\"math\">{}</span>", escape(m))),
            Inline::Break => self.out.push_str("<br>\n"),
            Inline::SoftBreak => self.out.push('\n'),
            // HTML del documento: como texto (no se ejecuta ni se interpreta).
            Inline::Html(h) => self.out.push_str(&escape(h)),
        }
    }
}
