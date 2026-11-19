//! Piezas comunes: texto plano, escape, enlaces seguros.

use iure_doc::Inline;

/// Texto plano de un contenido en línea.
pub(crate) fn plain(c: &[Inline]) -> String {
    let mut out = String::new();
    for i in c {
        match i {
            Inline::Text { text, .. } | Inline::Code { text, .. } | Inline::Math(text) => out.push_str(text),
            Inline::Link { content, .. } => out.push_str(&plain(content)),
            Inline::Image { alt, .. } => out.push_str(alt),
            Inline::Break | Inline::SoftBreak => out.push(' '),
            Inline::Html(_) => {}
        }
    }
    out
}

/// ¿Un carácter vale en XML 1.0? (Los de control, salvo tabulador y saltos, rompen Word y los
/// navegadores los muestran raros.)
fn xml_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..)
}

/// Escapa texto para HTML o XML (contenido y atributos) y quita los caracteres no válidos.
pub(crate) fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars().filter(|c| xml_char(*c)) {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// Un enlace que se puede exportar: `http(s):`, `mailto:`, `tel:`, anclas y rutas relativas.
/// `javascript:`, `data:`, `file:` y demás esquemas, fuera.
pub(crate) fn safe_url(url: &str) -> Option<String> {
    let u = url.trim();
    if u.is_empty() || u.len() > 4096 {
        return None;
    }
    // El esquema, sin los espacios y controles con que se suele esconder `java\tscript:`.
    let squashed: String = u.chars().filter(|c| !c.is_whitespace() && !c.is_control()).collect();
    let lower = squashed.to_ascii_lowercase();
    let scheme_end = lower.find(':');
    let first_sep = lower.find(['/', '?', '#']);
    let has_scheme = match (scheme_end, first_sep) {
        (Some(c), Some(s)) => c < s,
        (Some(_), None) => true,
        _ => false,
    };
    if has_scheme && !["http:", "https:", "mailto:", "tel:"].iter().any(|p| lower.starts_with(p)) {
        return None;
    }
    Some(squashed)
}

/// Texto plano de un trozo de Markdown que el modelo no entiende ([`iure_doc::Block::Raw`]):
/// párrafos con su texto, sin el HTML.
pub(crate) fn raw_paragraphs(md: &str) -> Vec<String> {
    use pulldown_cmark::{Event, Parser, TagEnd};
    let mut out = Vec::new();
    let mut cur = String::new();
    for ev in Parser::new(md) {
        match ev {
            Event::Text(t) | Event::Code(t) => cur.push_str(&t),
            Event::SoftBreak | Event::HardBreak => cur.push(' '),
            Event::End(TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::Item | TagEnd::CodeBlock) => {
                if !cur.trim().is_empty() {
                    out.push(std::mem::take(&mut cur).trim().to_string());
                }
                cur.clear();
            }
            _ => {}
        }
        if out.len() >= 10_000 {
            break;
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// Base64 estándar (para incrustar imágenes en HTML).
pub(crate) fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let sym = |n: u32| char::from(T.get((n & 63) as usize).copied().unwrap_or(b'A'));
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = |i: usize| u32::from(chunk.get(i).copied().unwrap_or(0));
        let n = (b(0) << 16) | (b(1) << 8) | b(2);
        out.push(sym(n >> 18));
        out.push(sym(n >> 12));
        out.push(if chunk.len() > 1 { sym(n >> 6) } else { '=' });
        out.push(if chunk.len() > 2 { sym(n) } else { '=' });
    }
    out
}
