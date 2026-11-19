//! DOCX: WordprocessingML escrito aquí y empaquetado con `zip`.
//!
//! - Títulos con los estilos de Word («Heading 1»…, con nivel de esquema: el panel de navegación y
//!   el índice de Word los reconocen) y «Title» no se usa: el primer `#` es «Heading 1».
//! - Listas con numeración de Word (viñetas y números que siguen al editar el documento), anidadas,
//!   cada lista empieza en su número; las de tareas llevan ☐ / ☒.
//! - Tablas con encabezado repetido en cada página y la alineación de cada columna.
//! - Código en monoespaciado con fondo; citas y avisos con sangría y borde.
//! - Imágenes (PNG, JPEG, GIF, BMP tal cual; WebP pasa a PNG) y diagramas como imágenes, a su
//!   tamaño y nunca más anchas que la página.
//!
//! El HTML del Markdown se exporta como texto, igual que en [`crate::html`].

use std::io::Write as _;

use iure_doc::{Align, Block, Document, Inline, Item, Style};

use crate::text::{escape, raw_paragraphs, safe_url};
use crate::{ExportError, Options, Paper, Picture, Resources, title_of};

/// Anidado máximo que se recorre.
const MAX_DEPTH: usize = 32;
/// Imágenes que se incrustan como mucho.
const MAX_MEDIA: usize = 1_000;
/// Enlaces como mucho (cada uno es una relación del paquete).
const MAX_LINKS: usize = 10_000;
/// Niveles de lista de Word.
const LIST_LEVELS: usize = 9;
/// Lado más grande de una imagen WebP que se convierte a PNG.
const MAX_CONVERT_SIDE: u32 = 8_192;
/// EMU por píxel (a 96 ppp) y por punto.
const EMU_PER_PX: u64 = 9_525;
const EMU_PER_PT: f64 = 12_700.0;
/// Twips (1/20 de punto) por EMU.
const EMU_PER_TWIP: u64 = 635;

/// Papel (ancho, alto) y margen, en twips.
fn page(paper: Paper) -> (u64, u64, u64) {
    match paper {
        Paper::Letter => (12_240, 15_840, 1_440),
        Paper::A4 => (11_906, 16_838, 1_440),
    }
}

/// El documento como un archivo `.docx`.
pub fn docx(doc: &Document, res: &dyn Resources, opts: &Options) -> Result<Vec<u8>, ExportError> {
    let (pw, ph, margin) = page(opts.paper);
    let mut w = Writer {
        res,
        body: String::new(),
        rels: Vec::new(),
        media: Vec::new(),
        nums: Vec::new(),
        drawing_id: 0,
        text_width_emu: pw.saturating_sub(2 * margin) * EMU_PER_TWIP,
    };
    for n in doc.nodes() {
        w.block(&n.block, &Ctx::default());
    }
    let lang = if opts.lang.trim().is_empty() { "es" } else { opts.lang.trim() };
    let lang: String = lang.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(16).collect();
    let lang = match lang.as_str() {
        "es" => "es-MX".to_string(),
        "en" => "en-US".to_string(),
        other => other.to_string(),
    };
    let document = format!(
        "{XML_HEAD}<w:document {NS}><w:body>{}<w:sectPr><w:pgSz w:w=\"{pw}\" w:h=\"{ph}\"/>\
         <w:pgMar w:top=\"{margin}\" w:right=\"{margin}\" w:bottom=\"{margin}\" w:left=\"{margin}\" \
         w:header=\"708\" w:footer=\"708\" w:gutter=\"0\"/></w:sectPr></w:body></w:document>",
        w.body
    );
    let title = title_of(doc, opts);
    w.pack(&document, &styles(&lang), &title)
}

const XML_HEAD: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";
const NS: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
    xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
    xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" \
    xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
    xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"";

/// Dónde va un bloque: estilo de párrafo heredado (cita, aviso) y nivel de lista.
#[derive(Clone, Default)]
struct Ctx {
    /// Estilo de párrafo de los párrafos sueltos (`Quote`, `Callout`).
    pstyle: Option<&'static str>,
    /// Profundidad del recorrido.
    depth: usize,
    /// Nivel de lista (0 fuera de listas).
    list_level: usize,
}

enum Rel {
    Link(String),
    Image(String),
}

struct Writer<'a> {
    res: &'a dyn Resources,
    body: String,
    rels: Vec<Rel>,
    /// (nombre en `word/media`, bytes).
    media: Vec<(String, Vec<u8>)>,
    /// Listas: (¿numerada?, nivel, inicio). Su número es la posición + 1.
    nums: Vec<(bool, usize, u64)>,
    drawing_id: u32,
    text_width_emu: u64,
}

/// Id de una relación (las dos primeras son estilos y numeración).
fn rid(i: usize) -> String {
    format!("rId{}", i + 10)
}

impl Writer<'_> {
    fn add_rel(&mut self, r: Rel) -> String {
        self.rels.push(r);
        rid(self.rels.len() - 1)
    }

    // ----- bloques -----

    fn block(&mut self, b: &Block, ctx: &Ctx) {
        if ctx.depth > MAX_DEPTH {
            return;
        }
        let inner = Ctx { depth: ctx.depth + 1, ..ctx.clone() };
        match b {
            Block::Paragraph(c) => {
                if let [Inline::Image { src, .. }] = c.as_slice()
                    && let Some(p) = self.res.image(src)
                    && let Some(run) = self.picture(&p, None)
                {
                    self.para(&ppr(ctx.pstyle, None, Some("center")), &run);
                    return;
                }
                let runs = self.inlines(c, Style::default());
                self.para(&ppr(ctx.pstyle, None, None), &runs);
            }
            Block::Heading { level, content } => {
                let l = (*level).clamp(1, 6);
                let runs = self.inlines(content, Style::default());
                self.para(&ppr(Some(HEADINGS.get(usize::from(l) - 1).copied().unwrap_or("Heading1")), None, None), &runs);
            }
            Block::List { ordered, start, items, .. } => self.list(*ordered, *start, items, ctx),
            Block::Quote(blocks) => {
                let q = Ctx { pstyle: Some("Quote"), ..inner };
                for b in blocks {
                    self.block(b, &q);
                }
            }
            Block::Callout { title, blocks, .. } => {
                if self.diagram(b, ctx) {
                    return;
                }
                if !title.trim().is_empty() {
                    let run = run_xml(title.trim(), Style { bold: true, ..Style::default() }, None);
                    self.para(&ppr(Some("Callout"), None, None), &run);
                }
                let c = Ctx { pstyle: Some("Callout"), ..inner };
                for b in blocks {
                    self.block(b, &c);
                }
            }
            Block::Code { text, .. } => {
                if self.diagram(b, ctx) {
                    return;
                }
                for line in text.trim_end_matches('\n').split('\n') {
                    let run = run_xml(line.trim_end_matches('\r'), Style::default(), None);
                    self.para(&ppr(Some("Code"), None, None), &run);
                }
            }
            Block::Math(m) => {
                let run = format!(
                    "<w:r><w:rPr><w:rFonts w:ascii=\"Cambria Math\" w:hAnsi=\"Cambria Math\"/><w:i/></w:rPr>{}</w:r>",
                    text_xml(m.trim())
                );
                self.para(&ppr(ctx.pstyle, None, Some("center")), &run);
            }
            Block::Table { align, head, rows } => self.table(align, head, rows),
            Block::Rule => self.body.push_str(
                "<w:p><w:pPr><w:pBdr><w:bottom w:val=\"single\" w:sz=\"6\" w:space=\"1\" w:color=\"BFBFBF\"/></w:pBdr></w:pPr></w:p>",
            ),
            Block::FrontMatter(_) => {}
            Block::Raw(md) => {
                for p in raw_paragraphs(md) {
                    let run = run_xml(&p, Style::default(), None);
                    self.para(&ppr(ctx.pstyle, None, None), &run);
                }
            }
        }
    }

    fn para(&mut self, ppr: &str, runs: &str) {
        self.body.push_str("<w:p>");
        self.body.push_str(ppr);
        self.body.push_str(runs);
        self.body.push_str("</w:p>");
    }

    fn diagram(&mut self, b: &Block, ctx: &Ctx) -> bool {
        let Some(d) = self.res.diagram(&iure_doc::write::block(b)) else { return false };
        let Some(png) = &d.png else { return false };
        let pts = (d.points.0 > 0.0 && d.points.1 > 0.0).then_some(d.points);
        let Some(run) = self.picture(png, pts) else { return false };
        self.para(&ppr(ctx.pstyle, None, Some("center")), &run);
        true
    }

    fn list(&mut self, ordered: bool, start: u64, items: &[Item], ctx: &Ctx) {
        let level = ctx.list_level.min(LIST_LEVELS - 1);
        self.nums.push((ordered, level, start.clamp(0, 999_999)));
        let num = self.nums.len();
        let inner = Ctx { depth: ctx.depth + 1, list_level: ctx.list_level + 1, pstyle: ctx.pstyle };
        let count = items.len();
        for (idx, item) in items.iter().enumerate() {
            // El último elemento de una lista de primer nivel deja aire antes de lo que sigue (otra
            // lista, incluso: Word junta los párrafos de lista).
            let end = ctx.list_level == 0 && idx + 1 == count && item.blocks.len() <= 1;
            let gap = if end { "<w:spacing w:after=\"160\"/><w:contextualSpacing w:val=\"0\"/>" } else { "" };
            let mut first = true;
            for b in &item.blocks {
                match b {
                    Block::Paragraph(c) if first => {
                        let mut runs = match item.checked {
                            Some(true) => run_xml("☒ ", Style::default(), None),
                            Some(false) => run_xml("☐ ", Style::default(), None),
                            None => String::new(),
                        };
                        let done = item.checked == Some(true);
                        runs.push_str(&self.inlines(c, Style { strike: done, ..Style::default() }));
                        let numpr =
                            format!("<w:numPr><w:ilvl w:val=\"{level}\"/><w:numId w:val=\"{num}\"/></w:numPr>{gap}");
                        self.para(&ppr(Some("ListParagraph"), Some(&numpr), None), &runs);
                    }
                    // Párrafos que siguen dentro del elemento: con la sangría de la lista.
                    Block::Paragraph(c) => {
                        let runs = self.inlines(c, Style::default());
                        let ind = format!("<w:ind w:left=\"{}\"/>", 720 * (level + 1));
                        self.para(&ppr(Some("ListParagraph"), Some(&ind), None), &runs);
                    }
                    other => self.block(other, &inner),
                }
                first = false;
            }
            // Un elemento vacío sigue siendo un elemento.
            if item.blocks.is_empty() {
                let numpr = format!("<w:numPr><w:ilvl w:val=\"{level}\"/><w:numId w:val=\"{num}\"/></w:numPr>");
                self.para(&ppr(Some("ListParagraph"), Some(&numpr), None), "");
            }
        }
    }

    fn table(&mut self, align: &[Align], head: &[Vec<Inline>], rows: &[Vec<Vec<Inline>>]) {
        let cols = head.len().max(rows.iter().map(Vec::len).max().unwrap_or(0)).clamp(1, 64);
        let col_w = (self.text_width_emu / EMU_PER_TWIP) / cols as u64;
        self.body.push_str(
            "<w:tbl><w:tblPr><w:tblStyle w:val=\"TableGrid\"/><w:tblW w:w=\"5000\" w:type=\"pct\"/>\
             <w:tblLook w:val=\"04A0\" w:firstRow=\"1\" w:lastRow=\"0\" w:firstColumn=\"0\" w:lastColumn=\"0\" w:noHBand=\"0\" w:noVBand=\"1\"/></w:tblPr><w:tblGrid>",
        );
        for _ in 0..cols {
            self.body.push_str(&format!("<w:gridCol w:w=\"{col_w}\"/>"));
        }
        self.body.push_str("</w:tblGrid>");
        let jc = |i: usize| match align.get(i).copied().unwrap_or_default() {
            Align::Center => Some("center"),
            Align::Right => Some("right"),
            Align::None | Align::Left => None,
        };
        let empty: Vec<Inline> = Vec::new();
        // Encabezado (se repite en cada página) y filas.
        let all = std::iter::once((true, head)).chain(rows.iter().map(|r| (false, r.as_slice())));
        for (is_head, row) in all.take(100_000) {
            self.body.push_str("<w:tr>");
            if is_head {
                self.body.push_str("<w:trPr><w:tblHeader/></w:trPr>");
            }
            for i in 0..cols {
                let cell = row.get(i).unwrap_or(&empty);
                let style = Style { bold: is_head, ..Style::default() };
                let runs = self.inlines(cell, style);
                let shade = if is_head { "<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"F2F2F2\"/>" } else { "" };
                self.body.push_str(&format!("<w:tc><w:tcPr><w:tcW w:w=\"{col_w}\" w:type=\"dxa\"/>{shade}</w:tcPr>"));
                self.para(&ppr(Some("TableText"), None, jc(i)), &runs);
                self.body.push_str("</w:tc>");
            }
            self.body.push_str("</w:tr>");
        }
        self.body.push_str("</w:tbl>");
        // Word junta dos tablas seguidas: un párrafo vacío las separa.
        self.body.push_str("<w:p><w:pPr><w:spacing w:after=\"0\"/></w:pPr></w:p>");
    }

    // ----- en línea -----

    fn inlines(&mut self, c: &[Inline], base: Style) -> String {
        let mut out = String::new();
        for i in c {
            out.push_str(&self.inline(i, base));
        }
        out
    }

    fn inline(&mut self, i: &Inline, base: Style) -> String {
        let merge = |s: Style| Style {
            bold: s.bold || base.bold,
            italic: s.italic || base.italic,
            strike: s.strike || base.strike,
        };
        match i {
            Inline::Text { text, style } => run_xml(text, merge(*style), None),
            Inline::Code { text, style } => run_xml(text, merge(*style), Some("CodeChar")),
            Inline::Link { url, content, .. } => {
                let runs = self.link_runs(content, base);
                match safe_url(url).filter(|u| !u.starts_with('#')) {
                    Some(u) if self.rels.len() < MAX_LINKS => {
                        let id = self.add_rel(Rel::Link(u));
                        format!("<w:hyperlink r:id=\"{id}\" w:history=\"1\">{runs}</w:hyperlink>")
                    }
                    _ => runs,
                }
            }
            Inline::Image { src, alt, .. } => match self.res.image(src).and_then(|p| self.picture(&p, None)) {
                Some(run) => run,
                None => run_xml(alt, base, None),
            },
            Inline::Math(m) => format!(
                "<w:r><w:rPr><w:rFonts w:ascii=\"Cambria Math\" w:hAnsi=\"Cambria Math\"/><w:i/></w:rPr>{}</w:r>",
                text_xml(m)
            ),
            Inline::Break => "<w:r><w:br/></w:r>".to_string(),
            Inline::SoftBreak => run_xml(" ", base, None),
            Inline::Html(h) => run_xml(h, base, None),
        }
    }

    /// Los tramos de un enlace, con el estilo «Hyperlink».
    fn link_runs(&mut self, content: &[Inline], base: Style) -> String {
        let mut out = String::new();
        for i in content {
            match i {
                Inline::Text { text, style } => {
                    let s = Style {
                        bold: style.bold || base.bold,
                        italic: style.italic || base.italic,
                        strike: style.strike || base.strike,
                    };
                    out.push_str(&run_xml(text, s, Some("Hyperlink")));
                }
                other => out.push_str(&self.inline(other, base)),
            }
        }
        out
    }

    /// Un tramo con una imagen: a su tamaño (96 ppp) o a `points`, nunca más ancha que la página.
    fn picture(&mut self, p: &Picture, points: Option<(f32, f32)>) -> Option<String> {
        if self.media.len() >= MAX_MEDIA {
            return None;
        }
        let (bytes, ext) = match p.mime.as_str() {
            "image/png" => (p.bytes.clone(), "png"),
            "image/jpeg" => (p.bytes.clone(), "jpeg"),
            "image/gif" => (p.bytes.clone(), "gif"),
            "image/bmp" => (p.bytes.clone(), "bmp"),
            // Word no abre WebP: pasa a PNG.
            _ => (to_png(&p.bytes)?, "png"),
        };
        let (mut cx, mut cy) = match points {
            Some((w, h)) => ((f64::from(w) * EMU_PER_PT) as u64, (f64::from(h) * EMU_PER_PT) as u64),
            None => (u64::from(p.width) * EMU_PER_PX, u64::from(p.height) * EMU_PER_PX),
        };
        if cx == 0 || cy == 0 {
            return None;
        }
        if cx > self.text_width_emu {
            cy = cy.saturating_mul(self.text_width_emu) / cx;
            cx = self.text_width_emu;
        }
        let name = format!("image{}.{ext}", self.media.len() + 1);
        self.media.push((name.clone(), bytes));
        let id = self.add_rel(Rel::Image(format!("media/{name}")));
        self.drawing_id = self.drawing_id.saturating_add(1);
        let did = self.drawing_id;
        Some(format!(
            "<w:r><w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\">\
             <wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:docPr id=\"{did}\" name=\"Imagen {did}\"/>\
             <a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">\
             <pic:pic><pic:nvPicPr><pic:cNvPr id=\"{did}\" name=\"{name}\"/><pic:cNvPicPr/></pic:nvPicPr>\
             <pic:blipFill><a:blip r:embed=\"{id}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>\
             <pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm>\
             <a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic>\
             </a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"
        ))
    }

    // ----- paquete -----

    fn pack(self, document: &str, styles: &str, title: &str) -> Result<Vec<u8>, ExportError> {
        let mut types = String::from(
            "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
             <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
             <Default Extension=\"xml\" ContentType=\"application/xml\"/>\
             <Default Extension=\"png\" ContentType=\"image/png\"/>\
             <Default Extension=\"jpeg\" ContentType=\"image/jpeg\"/>\
             <Default Extension=\"gif\" ContentType=\"image/gif\"/>\
             <Default Extension=\"bmp\" ContentType=\"image/bmp\"/>\
             <Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>\
             <Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/>\
             <Override PartName=\"/word/numbering.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml\"/>\
             <Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/>",
        );
        types.push_str("</Types>");
        let root_rels = "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
             <Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/>\
             <Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/>\
             </Relationships>";
        let mut doc_rels = String::from(
            "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
             <Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/>\
             <Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering\" Target=\"numbering.xml\"/>",
        );
        for (i, r) in self.rels.iter().enumerate() {
            match r {
                Rel::Link(u) => doc_rels.push_str(&format!(
                    "<Relationship Id=\"{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink\" Target=\"{}\" TargetMode=\"External\"/>",
                    rid(i),
                    escape(u)
                )),
                Rel::Image(t) => doc_rels.push_str(&format!(
                    "<Relationship Id=\"{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"{}\"/>",
                    rid(i),
                    escape(t)
                )),
            }
        }
        doc_rels.push_str("</Relationships>");
        let core = format!(
            "<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" \
             xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:title>{}</dc:title><dc:creator>IureDoc</dc:creator></cp:coreProperties>",
            escape(title)
        );
        let numbering = numbering(&self.nums);

        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let err = |e: &dyn std::fmt::Display| ExportError {
            en: format!("The Word file could not be written: {e}"),
            es: format!("No se pudo escribir el archivo de Word: {e}"),
        };
        let parts: [(&str, String); 7] = [
            ("[Content_Types].xml", format!("{XML_HEAD}{types}")),
            ("_rels/.rels", format!("{XML_HEAD}{root_rels}")),
            ("word/_rels/document.xml.rels", format!("{XML_HEAD}{doc_rels}")),
            ("word/document.xml", document.to_string()),
            ("word/styles.xml", styles.to_string()),
            ("word/numbering.xml", numbering),
            ("docProps/core.xml", format!("{XML_HEAD}{core}")),
        ];
        for (name, data) in &parts {
            zip.start_file(*name, opts).map_err(|e| err(&e))?;
            zip.write_all(data.as_bytes()).map_err(|e| err(&e))?;
        }
        // Las imágenes ya vienen comprimidas.
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, bytes) in &self.media {
            zip.start_file(format!("word/media/{name}"), stored).map_err(|e| err(&e))?;
            zip.write_all(bytes).map_err(|e| err(&e))?;
        }
        zip.finish().map(std::io::Cursor::into_inner).map_err(|e| err(&e))
    }
}

const HEADINGS: [&str; 6] = ["Heading1", "Heading2", "Heading3", "Heading4", "Heading5", "Heading6"];

/// Propiedades de párrafo: estilo, otras propiedades (`numPr`, `ind`) y alineación.
fn ppr(style: Option<&str>, extra: Option<&str>, jc: Option<&str>) -> String {
    if style.is_none() && extra.is_none() && jc.is_none() {
        return String::new();
    }
    let mut s = String::from("<w:pPr>");
    if let Some(st) = style {
        s.push_str(&format!("<w:pStyle w:val=\"{st}\"/>"));
    }
    if let Some(e) = extra {
        s.push_str(e);
    }
    if let Some(j) = jc {
        s.push_str(&format!("<w:jc w:val=\"{j}\"/>"));
    }
    s.push_str("</w:pPr>");
    s
}

/// `<w:t>` con el espacio conservado; los tabuladores son `<w:tab/>`.
fn text_xml(text: &str) -> String {
    let mut out = String::new();
    for (i, part) in text.split('\t').enumerate() {
        if i > 0 {
            out.push_str("<w:tab/>");
        }
        if !part.is_empty() {
            out.push_str(&format!("<w:t xml:space=\"preserve\">{}</w:t>", escape(&part.replace(['\n', '\r'], " "))));
        }
    }
    out
}

/// Un tramo de texto con su estilo (y un estilo de carácter: `CodeChar`, `Hyperlink`).
fn run_xml(text: &str, style: Style, rstyle: Option<&str>) -> String {
    if text.is_empty() {
        return String::new();
    }
    let mut rpr = String::new();
    if let Some(r) = rstyle {
        rpr.push_str(&format!("<w:rStyle w:val=\"{r}\"/>"));
    }
    if style.bold {
        rpr.push_str("<w:b/>");
    }
    if style.italic {
        rpr.push_str("<w:i/>");
    }
    if style.strike {
        rpr.push_str("<w:strike/>");
    }
    let rpr = if rpr.is_empty() { String::new() } else { format!("<w:rPr>{rpr}</w:rPr>") };
    format!("<w:r>{rpr}{}</w:r>", text_xml(text))
}

/// WebP (u otro formato que Word no abre) a PNG, acotado.
fn to_png(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_CONVERT_SIDE);
    limits.max_image_height = Some(MAX_CONVERT_SIDE);
    limits.max_alloc = Some(512 * 1024 * 1024);
    reader.limits(limits);
    let img = reader.decode().ok()?;
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

/// `numbering.xml`: una definición de viñetas y una de números; cada lista, su instancia (empieza en
/// su número).
fn numbering(nums: &[(bool, usize, u64)]) -> String {
    let mut s = format!("{XML_HEAD}<w:numbering {NS}>");
    let bullets = ["•", "◦", "▪"];
    let formats = ["decimal", "lowerLetter", "lowerRoman"];
    for (abs, ordered) in [(0, false), (1, true)] {
        s.push_str(&format!("<w:abstractNum w:abstractNumId=\"{abs}\"><w:multiLevelType w:val=\"hybridMultilevel\"/>"));
        for l in 0..LIST_LEVELS {
            let left = 720 * (l + 1);
            let (fmt, text) = if ordered {
                (formats.get(l % 3).copied().unwrap_or("decimal"), format!("%{}.", l + 1))
            } else {
                ("bullet", bullets.get(l % 3).copied().unwrap_or("•").to_string())
            };
            s.push_str(&format!(
                "<w:lvl w:ilvl=\"{l}\"><w:start w:val=\"1\"/><w:numFmt w:val=\"{fmt}\"/><w:lvlText w:val=\"{text}\"/>\
                 <w:lvlJc w:val=\"left\"/><w:pPr><w:ind w:left=\"{left}\" w:hanging=\"360\"/></w:pPr></w:lvl>"
            ));
        }
        s.push_str("</w:abstractNum>");
    }
    for (i, (ordered, level, start)) in nums.iter().enumerate() {
        let abs = u8::from(*ordered);
        s.push_str(&format!(
            "<w:num w:numId=\"{}\"><w:abstractNumId w:val=\"{abs}\"/>\
             <w:lvlOverride w:ilvl=\"{level}\"><w:startOverride w:val=\"{start}\"/></w:lvlOverride></w:num>",
            i + 1
        ));
    }
    s.push_str("</w:numbering>");
    s
}

/// `styles.xml`: los estilos de Word por su id, para que «Heading 1»… sean los de Word.
fn styles(lang: &str) -> String {
    let lang = escape(lang);
    let heading = |n: usize, size: u32, color: &str| {
        format!(
            "<w:style w:type=\"paragraph\" w:styleId=\"Heading{n}\"><w:name w:val=\"heading {n}\"/>\
             <w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:uiPriority w:val=\"9\"/><w:qFormat/>\
             <w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before=\"{}\" w:after=\"120\"/><w:outlineLvl w:val=\"{}\"/></w:pPr>\
             <w:rPr><w:b/><w:color w:val=\"{color}\"/><w:sz w:val=\"{size}\"/><w:szCs w:val=\"{size}\"/></w:rPr></w:style>",
            if n <= 2 { 360 } else { 240 },
            n - 1
        )
    };
    let mut s = format!(
        "{XML_HEAD}<w:styles {NS}><w:docDefaults><w:rPrDefault><w:rPr>\
         <w:rFonts w:ascii=\"Calibri\" w:hAnsi=\"Calibri\" w:eastAsia=\"Calibri\" w:cs=\"Calibri\"/>\
         <w:sz w:val=\"22\"/><w:szCs w:val=\"22\"/><w:lang w:val=\"{lang}\"/></w:rPr></w:rPrDefault>\
         <w:pPrDefault><w:pPr><w:spacing w:after=\"160\" w:line=\"276\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults>\
         <w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/><w:qFormat/></w:style>"
    );
    for (n, size, color) in [
        (1, 36, "1F3864"),
        (2, 30, "1F3864"),
        (3, 26, "1F3864"),
        (4, 24, "2F5496"),
        (5, 22, "2F5496"),
        (6, 22, "595959"),
    ] {
        s.push_str(&heading(n, size, color));
    }
    s.push_str(
        "<w:style w:type=\"paragraph\" w:styleId=\"ListParagraph\"><w:name w:val=\"List Paragraph\"/>\
         <w:basedOn w:val=\"Normal\"/><w:uiPriority w:val=\"34\"/><w:qFormat/><w:pPr><w:spacing w:after=\"60\"/><w:contextualSpacing/></w:pPr></w:style>\
         <w:style w:type=\"paragraph\" w:styleId=\"Quote\"><w:name w:val=\"Quote\"/><w:basedOn w:val=\"Normal\"/>\
         <w:uiPriority w:val=\"29\"/><w:qFormat/><w:pPr><w:pBdr><w:left w:val=\"single\" w:sz=\"18\" w:space=\"8\" w:color=\"D0D7DE\"/></w:pBdr>\
         <w:ind w:left=\"360\"/></w:pPr><w:rPr><w:color w:val=\"59636E\"/></w:rPr></w:style>\
         <w:style w:type=\"paragraph\" w:styleId=\"Callout\"><w:name w:val=\"Callout\"/><w:basedOn w:val=\"Normal\"/>\
         <w:pPr><w:pBdr><w:left w:val=\"single\" w:sz=\"24\" w:space=\"8\" w:color=\"0969DA\"/></w:pBdr>\
         <w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"F2F7FF\"/><w:spacing w:after=\"60\"/><w:ind w:left=\"360\"/></w:pPr></w:style>\
         <w:style w:type=\"paragraph\" w:styleId=\"Code\"><w:name w:val=\"Code\"/><w:basedOn w:val=\"Normal\"/>\
         <w:pPr><w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"F6F8FA\"/><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr>\
         <w:rPr><w:rFonts w:ascii=\"Consolas\" w:hAnsi=\"Consolas\" w:cs=\"Consolas\"/><w:sz w:val=\"19\"/><w:szCs w:val=\"19\"/></w:rPr></w:style>\
         <w:style w:type=\"paragraph\" w:styleId=\"TableText\"><w:name w:val=\"Table Text\"/><w:basedOn w:val=\"Normal\"/>\
         <w:pPr><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr></w:style>\
         <w:style w:type=\"character\" w:styleId=\"CodeChar\"><w:name w:val=\"Code Char\"/>\
         <w:rPr><w:rFonts w:ascii=\"Consolas\" w:hAnsi=\"Consolas\" w:cs=\"Consolas\"/><w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"EFF1F3\"/></w:rPr></w:style>\
         <w:style w:type=\"character\" w:styleId=\"Hyperlink\"><w:name w:val=\"Hyperlink\"/><w:uiPriority w:val=\"99\"/>\
         <w:rPr><w:color w:val=\"0563C1\"/><w:u w:val=\"single\"/></w:rPr></w:style>\
         <w:style w:type=\"table\" w:styleId=\"TableGrid\"><w:name w:val=\"Table Grid\"/><w:uiPriority w:val=\"39\"/>\
         <w:tblPr><w:tblBorders><w:top w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"BFBFBF\"/>\
         <w:left w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"BFBFBF\"/><w:bottom w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"BFBFBF\"/>\
         <w:right w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"BFBFBF\"/><w:insideH w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"BFBFBF\"/>\
         <w:insideV w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"BFBFBF\"/></w:tblBorders>\
         <w:tblCellMar><w:left w:w=\"108\" w:type=\"dxa\"/><w:right w:w=\"108\" w:type=\"dxa\"/></w:tblCellMar></w:tblPr></w:style>",
    );
    s.push_str("</w:styles>");
    s
}
