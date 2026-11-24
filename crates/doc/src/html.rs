//! S1.12: HTML → Markdown, para pegar desde la web, Word, Google Docs, Outlook o un editor de
//! código sin perder la estructura.
//!
//! Se lee con el tokenizador de html5ever (sin DOM de terceros: un árbol pequeño propio con pilas
//! acotadas y los cierres implícitos de `p`, `li`, `td`, `tr`… que haría un navegador), se pasa al
//! modelo de bloques ([`Block`]) y se escribe con el escritor de este crate, que escapa lo que haga
//! falta (un `1.` o un `#` de texto no se vuelven lista ni título).
//!
//! Se conservan títulos, párrafos, listas (anidadas, numeradas, de tareas), citas, código (con su
//! lenguaje), separadores, tablas (con alineación), negrita, cursiva, tachado, código en línea,
//! enlaces e imágenes. Se entienden los estilos en línea de Word, Outlook y Google Docs
//! (`font-weight`, `font-style`, `text-decoration`; el `<b style="font-weight:normal">` de Google
//! Docs no es negrita), las listas de Word (`MsoListParagraph`) y el `white-space: pre` de los
//! editores de código (bloque de código). Se descartan guiones, estilos, comentarios, enlaces
//! `javascript:` e imágenes `data:`. Nunca falla: el HTML más roto da algo razonable.

use std::cell::RefCell;

use html5ever::tendril::StrTendril;
use html5ever::tokenizer::states::RawKind;
use html5ever::tokenizer::{BufferQueue, Tag, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts};

use crate::{Align, Block, Document, Inline, Item, MAX_DEPTH, Style};

/// HTML más grande que se convierte (lo demás se descarta).
pub const MAX_HTML: usize = 8 * 1024 * 1024;
/// Elementos y textos como mucho en el árbol.
const MAX_NODES: usize = 200_000;
/// Elementos abiertos a la vez como mucho (lo más profundo se aplana).
const MAX_STACK: usize = 256;

/// Elementos cuyo contenido se descarta.
const SKIP: &[&str] = &[
    "script", "style", "head", "title", "template", "iframe", "object", "applet", "noembed", "noframes", "svg", "math",
    "select", "textarea", "button", "noscript",
];

/// Elementos sin cierre.
const VOID: &[&str] =
    &["br", "img", "hr", "wbr", "input", "meta", "link", "col", "area", "base", "source", "embed", "param", "track"];

/// Elementos de bloque.
const BLOCK: &[&str] = &[
    "p",
    "div",
    "section",
    "article",
    "header",
    "footer",
    "main",
    "nav",
    "aside",
    "center",
    "address",
    "figure",
    "figcaption",
    "details",
    "summary",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "ul",
    "ol",
    "li",
    "blockquote",
    "pre",
    "table",
    "hr",
    "dl",
    "dt",
    "dd",
    "form",
    "fieldset",
    "body",
    "html",
    "tbody",
    "thead",
    "tfoot",
    "tr",
    "td",
    "th",
    "caption",
];

enum Node {
    Text(String),
    El(El),
}

#[derive(Default)]
struct El {
    name: String,
    attrs: Vec<(String, String)>,
    kids: Vec<Node>,
}

impl El {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    /// El valor de una propiedad de `style` (en minúsculas, sin espacios).
    fn css(&self, prop: &str) -> Option<String> {
        let style = self.attr("style")?.to_ascii_lowercase();
        style.split(';').find_map(|decl| {
            let (k, v) = decl.split_once(':')?;
            (k.trim() == prop).then(|| v.trim().replace(' ', ""))
        })
    }
}

fn is_block(name: &str) -> bool {
    BLOCK.contains(&name)
}

/// El árbol, construido por tokens.
struct Tree {
    root: El,
    stack: Vec<El>,
    nodes: usize,
    skipping: Option<(String, u32)>,
}

impl Tree {
    fn current(&mut self) -> &mut El {
        match self.stack.last_mut() {
            Some(e) => e,
            None => &mut self.root,
        }
    }

    fn close_top(&mut self) {
        if let Some(e) = self.stack.pop() {
            self.current().kids.push(Node::El(e));
        }
    }

    /// Cierra hasta `name` (incluido) si está abierto antes de alguno de `stop`.
    fn close_open(&mut self, names: &[&str], stop: &[&str]) {
        let pos = self.stack.iter().rposition(|e| names.contains(&e.name.as_str()) || stop.contains(&e.name.as_str()));
        if let Some(p) = pos
            && self.stack.get(p).is_some_and(|e| names.contains(&e.name.as_str()))
        {
            while self.stack.len() > p {
                self.close_top();
            }
        }
    }

    fn start(&mut self, name: &str, attrs: Vec<(String, String)>, self_closing: bool) {
        if self.nodes >= MAX_NODES {
            return;
        }
        self.nodes += 1;
        // Cierres implícitos, como en un navegador.
        match name {
            "li" => self.close_open(&["li"], &["ul", "ol"]),
            "td" | "th" => self.close_open(&["td", "th"], &["tr", "table"]),
            "tr" => self.close_open(&["tr"], &["table", "tbody", "thead", "tfoot"]),
            "tbody" | "thead" | "tfoot" => self.close_open(&["tbody", "thead", "tfoot"], &["table"]),
            "dt" | "dd" => self.close_open(&["dt", "dd"], &["dl"]),
            _ => {}
        }
        if is_block(name) {
            self.close_open(&["p"], &["div", "li", "td", "th", "blockquote", "table", "body"]);
        }
        let el = El { name: name.to_string(), attrs, kids: Vec::new() };
        if VOID.contains(&name) || self_closing || self.stack.len() >= MAX_STACK {
            self.current().kids.push(Node::El(el));
        } else {
            self.stack.push(el);
        }
    }

    fn end(&mut self, name: &str) {
        if let Some(p) = self.stack.iter().rposition(|e| e.name == name) {
            while self.stack.len() > p {
                self.close_top();
            }
        } else if name == "p" || name == "br" {
            // `</p>` sin abrir y `</br>` son un salto, como en los navegadores.
            self.current().kids.push(Node::El(El { name: "br".into(), ..Default::default() }));
        }
    }

    fn text(&mut self, t: &str) {
        if self.nodes >= MAX_NODES {
            return;
        }
        let cur = self.current();
        if let Some(Node::Text(prev)) = cur.kids.last_mut() {
            prev.push_str(t);
        } else {
            cur.kids.push(Node::Text(t.to_string()));
            self.nodes += 1;
        }
    }

    fn finish(mut self) -> El {
        while !self.stack.is_empty() {
            self.close_top();
        }
        self.root
    }
}

struct Sink {
    tree: RefCell<Tree>,
}

fn raw_kind(name: &str) -> TokenSinkResult<()> {
    match name {
        "style" | "xmp" | "iframe" | "noembed" | "noframes" | "noscript" => TokenSinkResult::RawData(RawKind::Rawtext),
        "script" => TokenSinkResult::RawData(RawKind::ScriptData),
        "title" | "textarea" => TokenSinkResult::RawData(RawKind::Rcdata),
        _ => TokenSinkResult::Continue,
    }
}

impl Sink {
    fn tag(&self, tag: Tag) -> TokenSinkResult<()> {
        let Ok(mut t) = self.tree.try_borrow_mut() else { return TokenSinkResult::Continue };
        let name = tag.name.to_string().to_ascii_lowercase();
        let start = tag.kind == TagKind::StartTag;
        if let Some((skip, depth)) = &mut t.skipping {
            if name == *skip {
                if start && !tag.self_closing {
                    *depth = depth.saturating_add(1);
                } else if !start {
                    if *depth == 0 {
                        t.skipping = None;
                    } else {
                        *depth = depth.saturating_sub(1);
                    }
                }
            }
            return if start { raw_kind(&name) } else { TokenSinkResult::Continue };
        }
        if SKIP.contains(&name.as_str()) {
            if start && !tag.self_closing {
                t.skipping = Some((name.clone(), 0));
                return raw_kind(&name);
            }
            return TokenSinkResult::Continue;
        }
        if start {
            let attrs = tag
                .attrs
                .iter()
                .take(32)
                .map(|a| (a.name.local.to_string().to_ascii_lowercase(), a.value.to_string()))
                .collect();
            t.start(&name, attrs, tag.self_closing);
        } else {
            t.end(&name);
        }
        TokenSinkResult::Continue
    }
}

impl TokenSink for Sink {
    type Handle = ();

    fn process_token(&self, token: Token, _line: u64) -> TokenSinkResult<()> {
        match token {
            Token::TagToken(tag) => self.tag(tag),
            Token::CharacterTokens(s) => {
                if let Ok(mut t) = self.tree.try_borrow_mut()
                    && t.skipping.is_none()
                {
                    t.text(&s);
                }
                TokenSinkResult::Continue
            }
            _ => TokenSinkResult::Continue,
        }
    }
}

fn clip(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s.get(..end).unwrap_or("")
}

fn parse_tree(html: &str) -> El {
    // El portapapeles de Windows (CF_HTML) antepone «Version:… StartHTML:…».
    let html = if html.trim_start().starts_with("Version:") {
        html.find('<').and_then(|i| html.get(i..)).unwrap_or("")
    } else {
        html
    };
    let sink = Sink { tree: RefCell::new(Tree { root: El::default(), stack: Vec::new(), nodes: 0, skipping: None }) };
    let tok = Tokenizer::new(sink, TokenizerOpts::default());
    let input = BufferQueue::default();
    input.push_back(StrTendril::from_slice(clip(html, MAX_HTML)));
    let _ = tok.feed(&input);
    tok.end();
    tok.sink.tree.into_inner().finish()
}

// --- Árbol → bloques ---

fn safe_url(url: &str) -> bool {
    let l = url.trim().to_ascii_lowercase();
    !(l.starts_with("javascript:") || l.starts_with("vbscript:") || l.starts_with("data:") || l.starts_with("file:"))
}

/// El texto de un elemento; `pre`: conserva espacios y saltos.
fn text_of(nodes: &[Node], out: &mut String, depth: usize) {
    for n in nodes {
        match n {
            Node::Text(t) => out.push_str(t),
            Node::El(e) if e.name == "br" => out.push('\n'),
            Node::El(e) if depth < MAX_STACK => {
                if is_block(&e.name) && !out.is_empty() && !out.ends_with('\n') && e.name != "td" && e.name != "th" {
                    out.push('\n');
                }
                text_of(&e.kids, out, depth + 1);
            }
            Node::El(_) => {}
        }
    }
}

/// El estilo que añade un elemento en línea (y sus estilos CSS).
fn style_of(e: &El, mut s: Style) -> Style {
    match e.name.as_str() {
        "b" | "strong" => s.bold = true,
        "i" | "em" | "cite" | "var" | "dfn" => s.italic = true,
        "s" | "strike" | "del" => s.strike = true,
        _ => {}
    }
    if let Some(w) = e.css("font-weight") {
        let n: u32 = w.parse().unwrap_or(0);
        s.bold = w == "bold"
            || w == "bolder"
            || n >= 600
            || (s.bold && w != "normal" && w != "lighter" && (n == 0 || n >= 600));
    }
    if let Some(f) = e.css("font-style") {
        s.italic = f == "italic" || f == "oblique";
    }
    if e.css("text-decoration").or_else(|| e.css("text-decoration-line")).is_some_and(|d| d.contains("line-through")) {
        s.strike = true;
    }
    s
}

fn is_code_like(e: &El) -> bool {
    e.css("white-space").is_some_and(|w| w.starts_with("pre"))
}

/// Contenido en línea de unos nodos.
fn inlines(nodes: &[Node], style: Style, out: &mut Vec<Inline>, depth: usize) {
    for n in nodes {
        match n {
            Node::Text(t) => out.push(Inline::Text { text: t.clone(), style }),
            Node::El(e) => inline_el(e, style, out, depth),
        }
    }
}

fn inline_el(e: &El, style: Style, out: &mut Vec<Inline>, depth: usize) {
    if depth > MAX_STACK {
        return;
    }
    // Viñetas que Word escribe a mano en sus listas.
    if e.css("mso-list").is_some_and(|v| v == "ignore") {
        return;
    }
    match e.name.as_str() {
        "br" => out.push(Inline::Break),
        "img" => {
            let src = e.attr("src").unwrap_or("").trim().to_string();
            let alt = e.attr("alt").unwrap_or("").to_string();
            if !src.is_empty() && safe_url(&src) {
                out.push(Inline::Image { src, alt, title: e.attr("title").unwrap_or("").to_string() });
            } else if !alt.trim().is_empty() {
                out.push(Inline::Text { text: alt, style });
            }
        }
        "code" | "kbd" | "samp" | "tt" => {
            let mut t = String::new();
            text_of(&e.kids, &mut t, depth + 1);
            let t = t.replace(['\n', '\r'], " ");
            if !t.is_empty() {
                out.push(Inline::Code { text: t, style });
            }
        }
        "a" => {
            let href = e.attr("href").unwrap_or("").trim().to_string();
            let mut content = Vec::new();
            inlines(&e.kids, style, &mut content, depth + 1);
            if href.is_empty() || href.starts_with('#') || !safe_url(&href) {
                out.extend(content);
            } else {
                let content = normalize(content);
                if content.is_empty() {
                    out.push(Inline::Link {
                        url: href.clone(),
                        title: String::new(),
                        content: vec![Inline::text(href)],
                    });
                } else {
                    out.push(Inline::Link { url: href, title: e.attr("title").unwrap_or("").to_string(), content });
                }
            }
        }
        "input" => {
            if e.attr("type").is_some_and(|t| t.eq_ignore_ascii_case("checkbox")) {
                // Las casillas se leen en la lista.
            }
        }
        _ => {
            let s = style_of(e, style);
            if is_block(&e.name) {
                // Un bloque dentro de algo en línea: un espacio que lo separe.
                out.push(Inline::Text { text: " ".into(), style });
            }
            inlines(&e.kids, s, out, depth + 1);
        }
    }
}

fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{0C}' | '\u{A0}')
}

/// Espacios como en HTML: los seguidos son uno, sin espacios en los bordes; textos vacíos fuera.
fn normalize(items: Vec<Inline>) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::with_capacity(items.len());
    let mut space_before = true;
    for it in items {
        match it {
            Inline::Text { text, style } => {
                let mut t = String::with_capacity(text.len());
                for c in text.chars() {
                    if is_space(c) {
                        if !space_before {
                            t.push(' ');
                            space_before = true;
                        }
                    } else {
                        t.push(c);
                        space_before = false;
                    }
                }
                if t.is_empty() {
                    continue;
                }
                match out.last_mut() {
                    Some(Inline::Text { text: prev, style: ps }) if *ps == style => prev.push_str(&t),
                    _ => out.push(Inline::Text { text: t, style }),
                }
            }
            Inline::Break => {
                trim_end(&mut out);
                out.push(Inline::Break);
                space_before = true;
            }
            other => {
                out.push(other);
                space_before = false;
            }
        }
    }
    trim_end(&mut out);
    while matches!(out.last(), Some(Inline::Break)) {
        out.pop();
        trim_end(&mut out);
    }
    while matches!(out.first(), Some(Inline::Break)) {
        out.remove(0);
    }
    out
}

fn trim_end(out: &mut Vec<Inline>) {
    if let Some(Inline::Text { text, .. }) = out.last_mut() {
        let n = text.trim_end_matches(' ').len();
        text.truncate(n);
        if text.is_empty() {
            out.pop();
        }
    }
}

/// Lista de Word: párrafos `MsoListParagraph…` seguidos.
fn word_list_item(e: &El) -> Option<bool> {
    let class = e.attr("class").unwrap_or("");
    if e.name != "p" || !class.starts_with("MsoListParagraph") {
        return None;
    }
    // ¿Numerada? La viñeta que Word escribe a mano (`1.`, `a)`) va en el span `mso-list:Ignore`.
    fn marker(nodes: &[Node]) -> Option<String> {
        nodes.iter().find_map(|n| match n {
            Node::El(e) if e.css("mso-list").is_some_and(|v| v == "ignore") => {
                let mut t = String::new();
                text_of(&e.kids, &mut t, 0);
                Some(t)
            }
            Node::El(e) => marker(&e.kids),
            Node::Text(_) => None,
        })
    }
    let m = marker(&e.kids).unwrap_or_default();
    let m = m.trim_matches(is_space);
    Some(m.ends_with('.') || m.ends_with(')'))
}

struct Conv {
    blocks: Vec<Block>,
    para: Vec<Inline>,
}

impl Conv {
    fn flush(&mut self) {
        let p = normalize(std::mem::take(&mut self.para));
        if !p.is_empty() {
            self.blocks.push(Block::Paragraph(p));
        }
    }
}

/// Bloques de unos nodos.
fn blocks_of(nodes: &[Node], depth: usize) -> Vec<Block> {
    let mut c = Conv { blocks: Vec::new(), para: Vec::new() };
    let mut i = 0;
    while let Some(n) = nodes.get(i) {
        i += 1;
        match n {
            Node::Text(t) => c.para.push(Inline::Text { text: t.clone(), style: Style::default() }),
            Node::El(e) if e.name == "br" => {
                // Un salto suelto entre bloques corta el párrafo; dentro, es un salto de línea.
                if c.para.iter().any(|x| !matches!(x, Inline::Text { text, .. } if text.trim().is_empty())) {
                    c.para.push(Inline::Break);
                }
            }
            Node::El(e) if word_list_item(e).is_some() => {
                c.flush();
                let ordered = word_list_item(e).unwrap_or(false);
                let mut items = vec![Item { checked: None, blocks: para_blocks(e, depth) }];
                while let Some(Node::El(next)) = nodes.get(i) {
                    if word_list_item(next).is_none() {
                        break;
                    }
                    items.push(Item { checked: None, blocks: para_blocks(next, depth) });
                    i += 1;
                }
                c.blocks.push(Block::List { ordered, start: 1, tight: true, items });
            }
            Node::El(e) if is_block(&e.name) || is_code_like(e) => {
                c.flush();
                c.blocks.extend(block_el(e, depth));
            }
            Node::El(e) => inline_el(e, Style::default(), &mut c.para, depth),
        }
    }
    c.flush();
    merge_list_runs(c.blocks)
}

/// Una lista «envoltorio»: un solo elemento sin texto cuyo único bloque es otra lista. Así marcan
/// LibreOffice y Word el nivel de un tramo de lista (`<ol><ol><li>…`). Devuelve cuántos niveles
/// envuelve y la lista de adentro.
fn unwrap_levels(b: Block) -> (usize, Block) {
    let mut depth = 0;
    let mut b = b;
    loop {
        let inner = match &b {
            Block::List { items, .. } if items.len() == 1 => {
                matches!(items.first().map(|i| i.blocks.as_slice()), Some([Block::List { .. }]))
            }
            _ => false,
        };
        if !inner || depth >= MAX_STACK {
            return (depth, b);
        }
        let Block::List { mut items, .. } = b else { return (depth, b) };
        let Some(Item { blocks, .. }) = items.pop() else {
            return (depth, Block::List { ordered: false, start: 1, tight: true, items: Vec::new() });
        };
        let Some(next) = blocks.into_iter().next() else {
            return (depth, Block::List { ordered: false, start: 1, tight: true, items: Vec::new() });
        };
        b = next;
        depth += 1;
    }
}

/// Cuelga los elementos de `run` (una lista) `depth` niveles debajo de `root`: en la lista del
/// último elemento de cada nivel (la crea si no hay). Devuelve la lista si no se pudo colgar.
fn attach(root: &mut Block, depth: usize, run: Block) -> Option<Block> {
    let Block::List { items, ordered, .. } = root else { return Some(run) };
    if depth == 0 {
        match run {
            Block::List { ordered: o, items: more, .. } if o == *ordered => {
                items.extend(more);
                return None;
            }
            other => return Some(other),
        }
    }
    let last = items.last_mut()?;
    match last.blocks.last_mut() {
        Some(sub @ Block::List { .. }) => attach(sub, depth - 1, run),
        _ if depth == 1 => {
            last.blocks.push(run);
            None
        }
        _ => {
            // Falta un nivel intermedio: se cuelga donde se pueda.
            last.blocks.push(run);
            None
        }
    }
}

/// LibreOffice y Word (al copiar) parten una lista con niveles en tramos seguidos: cada tramo es un
/// `<ol>` aparte con su nivel marcado por listas envoltorio. Se juntan en una sola lista con sus
/// niveles, para que la estructura y la numeración (que sigue de un tramo al otro) sean las del
/// documento. También se continúa una lista numerada que sigue con `start` (`<ol start="2">`).
fn merge_list_runs(blocks: Vec<Block>) -> Vec<Block> {
    let mut out: Vec<Block> = Vec::with_capacity(blocks.len());
    for b in blocks {
        let continues = match (out.last(), &b) {
            (Some(Block::List { ordered: true, items, .. }), Block::List { ordered: true, start, .. }) => {
                *start > 1 && *start == items.len() as u64 + 1
            }
            _ => false,
        };
        let (depth, inner) = unwrap_levels(b);
        let can_attach = matches!(out.last(), Some(Block::List { .. }))
            && matches!(inner, Block::List { .. })
            && (depth > 0 || continues);
        if can_attach && let Some(root) = out.last_mut() {
            if let Some(rest) = attach(root, depth, inner) {
                out.push(rest);
            }
            continue;
        }
        // Un tramo envoltorio sin lista antes: se queda con sus niveles (no se pierde nada).
        out.push(if depth > 0 { rewrap(depth, inner) } else { inner });
    }
    out
}

/// Vuelve a envolver una lista `depth` niveles (la forma original).
fn rewrap(depth: usize, inner: Block) -> Block {
    let mut b = inner;
    for _ in 0..depth {
        let ordered = matches!(b, Block::List { ordered: true, .. });
        b = Block::List { ordered, start: 1, tight: true, items: vec![Item { checked: None, blocks: vec![b] }] };
    }
    b
}

/// Un párrafo (de una lista de Word) como bloques de elemento.
fn para_blocks(e: &El, depth: usize) -> Vec<Block> {
    let mut b = blocks_of(&e.kids, depth + 1);
    if b.is_empty() {
        b.push(Block::Paragraph(Vec::new()));
    }
    b
}

fn heading_level(name: &str) -> Option<u8> {
    let d = name.strip_prefix('h')?;
    let n: u8 = d.parse().ok()?;
    (1..=6).contains(&n).then_some(n)
}

fn lang_of(e: &El) -> String {
    let from = |e: &El| {
        e.attr("class").and_then(|c| {
            c.split_whitespace()
                .find_map(|w| w.strip_prefix("language-").or_else(|| w.strip_prefix("lang-")).map(str::to_string))
        })
    };
    from(e)
        .or_else(|| {
            e.kids.iter().find_map(|k| match k {
                Node::El(c) if c.name == "code" => from(c),
                _ => None,
            })
        })
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '+' | '#' | '_'))
        .take(32)
        .collect()
}

fn block_el(e: &El, depth: usize) -> Vec<Block> {
    if depth >= MAX_DEPTH {
        // Demasiado anidado: su texto como un párrafo.
        let mut t = String::new();
        text_of(&e.kids, &mut t, 0);
        let p = normalize(vec![Inline::text(t)]);
        return if p.is_empty() { Vec::new() } else { vec![Block::Paragraph(p)] };
    }
    if let Some(level) = heading_level(&e.name) {
        let mut content = Vec::new();
        inlines(&e.kids, Style::default(), &mut content, depth + 1);
        let content: Vec<Inline> = normalize(content)
            .into_iter()
            .map(|x| if matches!(x, Inline::Break) { Inline::text(" ") } else { x })
            .collect();
        return if content.is_empty() { Vec::new() } else { vec![Block::Heading { level, content }] };
    }
    if e.name == "pre" || is_code_like(e) {
        let mut t = String::new();
        text_of(&e.kids, &mut t, 0);
        let t = t.strip_suffix('\n').unwrap_or(&t).to_string();
        return if t.trim().is_empty() { Vec::new() } else { vec![Block::Code { lang: lang_of(e), text: t }] };
    }
    match e.name.as_str() {
        "hr" => vec![Block::Rule],
        "blockquote" => {
            let inner = blocks_of(&e.kids, depth + 1);
            if inner.is_empty() { Vec::new() } else { vec![Block::Quote(inner)] }
        }
        "ul" | "ol" => list(e, depth),
        "table" => table(e, depth),
        _ => blocks_of(&e.kids, depth + 1),
    }
}

fn list(e: &El, depth: usize) -> Vec<Block> {
    let ordered = e.name == "ol";
    let start = e.attr("start").and_then(|s| s.trim().parse::<u64>().ok()).unwrap_or(1).min(999_999_999);
    let mut items = Vec::new();
    let mut loose: Vec<Node> = Vec::new();
    let take_item = |kids: &[Node], items: &mut Vec<Item>| {
        // Casilla de una lista de tareas: `<input type=checkbox [checked]>` al principio.
        let checked = kids.iter().find_map(|k| match k {
            Node::El(c) if c.name == "input" && c.attr("type").is_some_and(|t| t.eq_ignore_ascii_case("checkbox")) => {
                Some(c.attr("checked").is_some())
            }
            Node::El(c) if c.name == "p" || c.name == "label" || c.name == "span" => {
                c.kids.iter().find_map(|k| match k {
                    Node::El(i)
                        if i.name == "input" && i.attr("type").is_some_and(|t| t.eq_ignore_ascii_case("checkbox")) =>
                    {
                        Some(i.attr("checked").is_some())
                    }
                    _ => None,
                })
            }
            _ => None,
        });
        let mut blocks = blocks_of(kids, depth + 1);
        if blocks.is_empty() {
            blocks.push(Block::Paragraph(Vec::new()));
        }
        items.push(Item { checked, blocks });
    };
    for k in &e.kids {
        match k {
            Node::El(li) if li.name == "li" => {
                if !loose.is_empty() {
                    let l = std::mem::take(&mut loose);
                    if !blocks_of(&l, depth + 1).is_empty() {
                        take_item(&l, &mut items);
                    }
                }
                take_item(&li.kids, &mut items);
            }
            // Una lista dentro de otra sin `li` (HTML roto pero frecuente): va con el elemento anterior.
            Node::El(sub) if (sub.name == "ul" || sub.name == "ol") && !items.is_empty() => {
                if let Some(last) = items.last_mut() {
                    last.blocks.extend(list(sub, depth + 1));
                }
            }
            other => loose.push(match other {
                Node::Text(t) => Node::Text(t.clone()),
                Node::El(x) => Node::El(clone_el(x)),
            }),
        }
    }
    if !loose.is_empty() && !blocks_of(&loose, depth + 1).is_empty() {
        take_item(&loose, &mut items);
    }
    if items.is_empty() { Vec::new() } else { vec![Block::List { ordered, start, tight: true, items }] }
}

fn clone_el(e: &El) -> El {
    El {
        name: e.name.clone(),
        attrs: e.attrs.clone(),
        kids: e
            .kids
            .iter()
            .map(|k| match k {
                Node::Text(t) => Node::Text(t.clone()),
                Node::El(x) => Node::El(clone_el(x)),
            })
            .collect(),
    }
}

fn align_of(e: &El) -> Align {
    let a = e.attr("align").map(str::to_ascii_lowercase).or_else(|| e.css("text-align"));
    match a.as_deref() {
        Some("left") | Some("start") => Align::Left,
        Some("center") => Align::Center,
        Some("right") | Some("end") => Align::Right,
        _ => Align::None,
    }
}

/// Filas (`tr`) de una tabla, en orden, sin entrar en tablas anidadas.
fn rows_of<'a>(e: &'a El, out: &mut Vec<&'a El>, depth: usize) {
    for k in &e.kids {
        if let Node::El(c) = k {
            match c.name.as_str() {
                "tr" => out.push(c),
                "thead" | "tbody" | "tfoot" if depth < 4 => rows_of(c, out, depth + 1),
                _ => {}
            }
        }
    }
}

/// Celdas (`td`, `th`) de una fila.
fn cells(tr: &El) -> Vec<&El> {
    tr.kids
        .iter()
        .filter_map(|k| match k {
            Node::El(c) if c.name == "td" || c.name == "th" => Some(c),
            _ => None,
        })
        .collect()
}

fn table(e: &El, depth: usize) -> Vec<Block> {
    let mut rows = Vec::new();
    rows_of(e, &mut rows, 0);
    let cols = rows.iter().map(|r| cells(r).len()).max().unwrap_or(0).min(crate::table::MAX_COLS);
    // Una tabla de maquetación (una columna, o celdas con bloques o tablas): su contenido, en bloques.
    let layout = rows.iter().any(|r| {
        cells(r).iter().any(|c| {
            c.kids.iter().any(|k| matches!(k, Node::El(x) if matches!(x.name.as_str(), "table" | "ul" | "ol" | "pre" | "blockquote" | "h1" | "h2" | "h3")))
        })
    });
    if cols <= 1 || layout {
        let mut out = Vec::new();
        for r in &rows {
            for c in cells(r) {
                out.extend(blocks_of(&c.kids, depth + 1));
            }
        }
        return out;
    }
    let cell_inlines = |c: &El| -> Vec<Inline> {
        let mut v = Vec::new();
        inlines(&c.kids, Style::default(), &mut v, depth + 1);
        // En una celda no hay saltos de línea.
        normalize(v.into_iter().map(|x| if matches!(x, Inline::Break) { Inline::text(" ") } else { x }).collect())
    };
    let first = rows.first().map(|r| cells(r)).unwrap_or_default();
    let mut align: Vec<Align> = first.iter().map(|c| align_of(c)).collect();
    align.resize(cols, Align::None);
    let row = |r: &El| -> Vec<Vec<Inline>> {
        let mut v: Vec<Vec<Inline>> = cells(r).into_iter().take(cols).map(cell_inlines).collect();
        v.resize(cols, Vec::new());
        v
    };
    let head = rows.first().map(|r| row(r)).unwrap_or_default();
    let body = rows.iter().skip(1).take(crate::table::MAX_ROWS).map(|r| row(r)).collect();
    vec![Block::Table { align, head, rows: body }]
}

/// Markdown de un fragmento HTML (lo que se pega). Vacío si no queda nada.
pub fn to_markdown(html: &str) -> String {
    let root = parse_tree(html);
    let mut doc = Document::default();
    for b in blocks_of(&root.kids, 0) {
        doc.insert(usize::MAX, b, None);
    }
    doc.to_markdown().trim_end_matches('\n').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// Regresión (dueño 2026-11-23): pegar una lista con niveles copiada de LibreOffice o Word. Cada
    /// tramo llega como un `<ol>` aparte, con su nivel marcado por `<ol>` dentro de `<ol>` sin `<li>`;
    /// salían viñetas vacías («1)», «1.») y la numeración empezaba de nuevo en cada tramo.
    #[test]
    fn libreoffice_list_runs_keep_their_levels_and_numbering() {
        let html = "<body><ol><li><p>Iurefficient</p></li></ol>\
            <ol><ol><li><p>App</p></li><li><p>Portal</p></li></ol></ol>\
            <ol><ol><ol><li><p>Tema</p></li></ol></ol></ol>\
            <ol><ol><li><p>Blog</p></li></ol></ol>\
            <ol start=\"2\"><li><p>Conversaciones</p></li></ol>\
            <ol><ol><li><p>Pláticas</p></li></ol></ol></body>";
        assert_eq!(
            to_markdown(html),
            "1. Iurefficient\n   1. App\n   2. Portal\n      1. Tema\n   3. Blog\n2. Conversaciones\n   1. Pláticas"
        );
    }

    #[test]
    fn web_structure_becomes_markdown() {
        let html = "<h2>Acuerdos</h2><p>Hola <b>mundo</b>, <i>esto</i> y <a href=\"https://x.mx\">el sitio</a>.</p>\
            <ul><li>uno</li><li>dos<ul><li>sub</li></ul></li></ul><ol start=\"3\"><li>tres</li></ol>\
            <blockquote><p>cita</p></blockquote><hr><pre><code class=\"language-rust\">fn f() {\n    1\n}\n</code></pre>";
        assert_eq!(
            to_markdown(html),
            "## Acuerdos\n\nHola **mundo**, *esto* y [el sitio](https://x.mx).\n\n- uno\n- dos\n  - sub\n\n3. tres\n\n> cita\n\n---\n\n```rust\nfn f() {\n    1\n}\n```"
        );
    }

    #[test]
    fn tables_keep_their_cells_and_alignment() {
        let html = "<table><thead><tr><th>Concepto</th><th align=right>Monto</th></tr></thead>\
            <tbody><tr><td>Honorarios <b>base</b></td><td style=\"text-align: right\">12 000</td></tr><tr><td>a|b</td></tr></tbody></table>";
        assert_eq!(
            to_markdown(html),
            "| Concepto | Monto |\n| --- | ---: |\n| Honorarios **base** | 12 000 |\n| a\\|b |  |"
        );
        // Tabla de maquetación (una columna): su contenido.
        assert_eq!(to_markdown("<table><tr><td><p>Solo</p><p>texto</p></td></tr></table>"), "Solo\n\ntexto");
    }

    #[test]
    fn office_and_google_quirks() {
        // Google Docs envuelve todo en <b style="font-weight:normal">.
        let g = "<meta charset=utf-8><b style=\"font-weight:normal;\" id=\"docs-internal-guid-1\"><p dir=ltr><span style=\"font-weight:700\">Negrita</span><span style=\"font-weight:400\"> normal </span><span style=\"font-style:italic\">cursiva</span></p></b>";
        assert_eq!(to_markdown(g), "**Negrita** normal *cursiva*");
        // Word: listas a mano con la viñeta en un span `mso-list:Ignore`.
        let w = "<p class=MsoListParagraphCxSpFirst><span style='mso-list:Ignore'>1.<span>&nbsp;&nbsp;</span></span>Primero</p>\
            <p class=MsoListParagraphCxSpLast><span style='mso-list:Ignore'>2.<span>&nbsp;</span></span>Segundo</p><p class=MsoNormal>Fin<o:p></o:p></p>";
        assert_eq!(to_markdown(w), "1. Primero\n2. Segundo\n\nFin");
        // Un editor de código (VS Code): `white-space: pre` es código.
        let v = "<div style=\"color: #d4d4d4;font-family: 'Droid Sans Mono', monospace;white-space: pre;\"><div><span>fn</span><span> main() {</span></div><div><span>    x</span></div><div><span>}</span></div></div>";
        assert_eq!(to_markdown(v), "```\nfn main() {\n    x\n}\n```");
        // Windows (CF_HTML) antepone un encabezado.
        assert_eq!(
            to_markdown(
                "Version:0.9\r\nStartHTML:0000000105\r\n<html><body><!--StartFragment--><p>Hola</p><!--EndFragment--></body></html>"
            ),
            "Hola"
        );
    }

    #[test]
    fn hostile_html_is_cleaned() {
        let html = "<script>alert(1)</script><style>p{}</style><p><a href=\"javascript:alert(1)\">clic</a> <img src=\"data:image/png;base64,AAAA\" alt=\"logo\"> <img src=\"img/a.png\" alt=\"A\"></p><!-- nota -->";
        assert_eq!(to_markdown(html), "clic logo ![A](img/a.png)");
        // Texto que parecería Markdown se escapa.
        assert_eq!(
            to_markdown("<p>1. no es lista y #no título *ni* cursiva</p>"),
            "1\\. no es lista y #no título \\*ni\\* cursiva"
        );
        // Casillas de una lista de tareas.
        assert_eq!(
            to_markdown("<ul><li><input type=checkbox checked> hecho</li><li><input type=checkbox> no</li></ul>"),
            "- [x] hecho\n- [ ] no"
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]
        /// Ningún HTML hace fallar la conversión, y lo que sale es Markdown que se relee sin cambios.
        #[test]
        fn any_html_converts(html in "(<(/)?(p|b|i|ul|ol|li|table|tr|td|th|h2|pre|code|a href=x|blockquote|br|div|span style=\"font-weight:700\")>|[a-zé *#|`1.\\-]{0,6}|&nbsp;|&lt;){0,60}") {
            let md = to_markdown(&html);
            let again = Document::parse(&md).to_markdown();
            prop_assert_eq!(again.trim_end_matches('\n'), md.as_str());
        }

        /// Las palabras de los párrafos sobreviven.
        #[test]
        fn words_survive(words in proptest::collection::vec("[a-zñ]{1,8}", 1..12)) {
            let html = format!("<p>{}</p>", words.join(" <b>x</b> "));
            let md = to_markdown(&html);
            for w in &words {
                prop_assert!(md.contains(w.as_str()), "{} en {}", w, md);
            }
        }
    }
}
