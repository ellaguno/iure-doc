//! S1.12: lo que hacen las teclas del editor sobre el texto Markdown (funciones puras, sin egui).
//!
//! Las posiciones son **índices de carácter** (como los cursores de egui). Cada función devuelve
//! `None` si no le toca (la tecla hace lo normal) o el texto nuevo con la selección.
//!
//! - [`on_enter`]: en una lista, numerada, de tareas o en una cita, Intro sigue con el mismo
//!   marcador (el número siguiente, una casilla sin marcar); en un elemento vacío, sale de la
//!   lista. Dentro de un bloque de código, lo normal.
//! - [`indent`]: Tab / Mayús+Tab sangran o desangran un elemento de lista.
//! - [`toggle_wrap`]: Ctrl+B / Ctrl+I / tachado: pone o quita las marcas alrededor de la selección.
//! - [`link`]: Ctrl+K: `[texto](url)` con la `url` seleccionada para escribirla.
//! - [`code_exit`]: tres Intro al final de un bloque de código salen de él.
//! - [`set_line_kind`]: Ctrl+Mayús+7/8/9 (lista numerada, viñetas, tareas) y Ctrl+Alt+1–3
//!   (títulos) ponen o quitan el tipo de las líneas seleccionadas.

/// Resultado de una edición: el texto nuevo y la selección (índices de carácter).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    pub text: String,
    pub sel_start: usize,
    pub sel_end: usize,
}

/// Índice de byte del carácter `ci` (o el final).
fn byte_of(text: &str, ci: usize) -> usize {
    text.char_indices().nth(ci).map_or(text.len(), |(b, _)| b)
}

fn char_of(text: &str, byte: usize) -> usize {
    text.get(..byte).map_or(0, |s| s.chars().count())
}

/// Bytes de la línea que contiene `byte`: (inicio, fin sin el salto).
fn line_at(text: &str, byte: usize) -> (usize, usize) {
    let start = text.get(..byte).and_then(|s| s.rfind('\n')).map_or(0, |i| i + 1);
    let end = text.get(byte..).and_then(|s| s.find('\n')).map_or(text.len(), |i| byte + i);
    (start, end)
}

/// ¿El byte `at` está dentro de un bloque de código cercado? (cercas antes de su línea).
fn in_fence(text: &str, at: usize) -> bool {
    let (start, _) = line_at(text, at);
    let before = text.get(..start).unwrap_or("");
    before
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            t.starts_with("```") || t.starts_with("~~~")
        })
        .count()
        % 2
        == 1
}

/// Prefijo de bloque de una línea: sangría, marcador y casilla.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prefix {
    /// Sangría (espacios).
    pub indent: String,
    /// `- `, `* `, `+ `, `3. `, `3) ` o `> `.
    pub marker: String,
    /// Número de una lista numerada.
    pub number: Option<u64>,
    /// `Some(marcada)` en una lista de tareas.
    pub task: Option<bool>,
    /// Bytes que ocupa todo el prefijo.
    pub len: usize,
}

/// El prefijo de lista o cita de `line`, si tiene.
pub fn prefix(line: &str) -> Option<Prefix> {
    let indent_len = line.len() - line.trim_start_matches(' ').len();
    let indent = line.get(..indent_len)?.to_string();
    let rest = line.get(indent_len..)?;
    let (marker, number) = if let Some(r) = rest.strip_prefix("> ") {
        let _ = r;
        ("> ".to_string(), None)
    } else if rest.starts_with("- ") || rest.starts_with("* ") || rest.starts_with("+ ") {
        (rest.get(..2)?.to_string(), None)
    } else {
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        if !(1..=9).contains(&digits) {
            return None;
        }
        let delim = rest.get(digits..digits + 2)?;
        if delim != ". " && delim != ") " {
            return None;
        }
        (rest.get(..digits + 2)?.to_string(), rest.get(..digits)?.parse().ok())
    };
    let after = rest.get(marker.len()..).unwrap_or("");
    let task = if marker != "> " {
        if after.starts_with("[ ] ") || after == "[ ]" {
            Some(false)
        } else if after.starts_with("[x] ") || after.starts_with("[X] ") || after == "[x]" || after == "[X]" {
            Some(true)
        } else {
            None
        }
    } else {
        None
    };
    let task_len = if task.is_some() { after.len().min(4) } else { 0 };
    Some(Prefix { indent, len: indent_len + marker.len() + task_len, marker, number, task })
}

/// Intro en una lista o cita: sigue con el marcador, o sale si el elemento está vacío.
pub fn on_enter(text: &str, cursor: usize) -> Option<Edit> {
    let at = byte_of(text, cursor);
    if in_fence(text, at) {
        return None;
    }
    // Una lista dentro de una cita (`> - …`, como la de un mapa mental): la regla de la lista,
    // conservando las marcas de cita.
    if let Some(e) = in_quote(text, cursor, on_enter) {
        return e;
    }
    let (start, end) = line_at(text, at);
    let line = text.get(start..end)?;
    let p = prefix(line)?;
    // El cursor dentro del prefijo: lo normal.
    if at < start + p.len {
        return None;
    }
    let content = line.get(p.len..).unwrap_or("").trim();
    if content.is_empty() {
        // Elemento vacío: se quita el marcador (sale de la lista o de la cita).
        let mut out = String::with_capacity(text.len());
        out.push_str(text.get(..start)?);
        out.push_str(text.get(end..)?);
        let c = char_of(&out, start);
        return Some(Edit { text: out, sel_start: c, sel_end: c });
    }
    let marker = match p.number {
        Some(n) => {
            let delim = p.marker.chars().rev().nth(1).unwrap_or('.');
            format!("{}{delim} ", n.saturating_add(1))
        }
        None => p.marker.clone(),
    };
    let task = if p.task.is_some() { "[ ] " } else { "" };
    let insert = format!("\n{}{marker}{task}", p.indent);
    let mut out = String::with_capacity(text.len() + insert.len());
    out.push_str(text.get(..at)?);
    out.push_str(&insert);
    out.push_str(text.get(at..)?);
    let c = cursor + insert.chars().count();
    Some(Edit { text: out, sel_start: c, sel_end: c })
}

/// Bytes de las marcas de cita al principio de una línea (`> `, `> > `…; 0 si no es cita).
fn quote_lead(line: &str) -> usize {
    let mut at = 0usize;
    loop {
        let rest = line.get(at..).unwrap_or("");
        let sp = rest.len() - rest.trim_start_matches(' ').len();
        if sp > 3 || !rest.get(sp..).is_some_and(|r| r.starts_with('>')) {
            return at;
        }
        at += sp + 1;
        if line.get(at..).is_some_and(|r| r.starts_with(' ')) {
            at += 1;
        }
    }
}

/// Aplica `f` a una línea de lista dentro de una cita como si no tuviera las marcas de cita, y
/// las vuelve a poner (también en las líneas nuevas). `None`: la línea no es una lista en cita.
fn in_quote(text: &str, cursor: usize, f: impl Fn(&str, usize) -> Option<Edit>) -> Option<Option<Edit>> {
    let at = byte_of(text, cursor);
    let (start, end) = line_at(text, at);
    let line = text.get(start..end)?;
    let q = quote_lead(line);
    if q == 0 {
        return None;
    }
    let inner = line.get(q..)?;
    let p = prefix(inner)?;
    if p.marker == "> " {
        return None;
    }
    let lead = line.get(..q)?;
    let inner_cursor = cursor.checked_sub(char_of(text, start + q))?;
    let Some(e) = f(inner, inner_cursor) else { return Some(None) };
    // Las marcas de cita, también en cada línea nueva.
    let lead_chars = lead.chars().count();
    let rebuilt = e.text.replace('\n', &format!("\n{lead}"));
    let newlines_before = |c: usize| e.text.chars().take(c).filter(|ch| *ch == '\n').count();
    let shift = |c: usize| char_of(text, start) + lead_chars + c + newlines_before(c) * lead_chars;
    let out = format!("{}{lead}{rebuilt}{}", text.get(..start)?, text.get(end..)?);
    Some(Some(Edit { text: out, sel_start: shift(e.sel_start), sel_end: shift(e.sel_end) }))
}

/// La cerca que abre el bloque de código donde está `at` (p. ej. ```` ``` ```` o `~~~~`).
fn open_fence(text: &str, at: usize) -> Option<String> {
    let (start, _) = line_at(text, at);
    text.get(..start)?
        .lines()
        .rev()
        .find(|l| {
            let t = l.trim_start();
            t.starts_with("```") || t.starts_with("~~~")
        })
        .map(|l| {
            let t = l.trim_start();
            let ch = t.chars().next().unwrap_or('`');
            t.chars().take_while(|c| *c == ch).collect()
        })
}

/// Intro en la segunda línea vacía seguida al final de un bloque de código (tres Intro, como en
/// TipTap): sale del bloque. Quita las líneas vacías, cierra la cerca si faltaba y deja el cursor
/// en un párrafo nuevo después.
pub fn code_exit(text: &str, cursor: usize) -> Option<Edit> {
    let at = byte_of(text, cursor);
    if !in_fence(text, at) {
        return None;
    }
    let (start, end) = line_at(text, at);
    if !text.get(start..end)?.trim().is_empty() {
        return None;
    }
    let (ps, pe) = line_at(text, start.checked_sub(1)?);
    if !text.get(ps..pe)?.trim().is_empty() || in_fence(text, ps) != in_fence(text, at) {
        return None;
    }
    let fence = open_fence(text, at)?;
    let fence_char = fence.chars().next()?;
    let rest = text.get(end..)?.trim_start_matches(['\n', '\r']);
    // ¿Solo queda la cerca de cierre (o nada) después? Si hay más código, no es el final.
    let first = rest.lines().next().unwrap_or("");
    let closing = {
        let t = first.trim();
        !t.is_empty() && t.chars().all(|c| c == fence_char) && t.len() >= fence.len()
    };
    if !rest.trim().is_empty() && !closing {
        return None;
    }
    let mut out = String::with_capacity(text.len() + fence.len() + 2);
    out.push_str(text.get(..ps)?);
    let after = if closing {
        out.push_str(first.trim_end());
        rest.get(first.len()..).unwrap_or("").trim_start_matches(['\n', '\r'])
    } else {
        out.push_str(&fence);
        rest
    };
    out.push_str("\n\n");
    let c = out.chars().count();
    out.push_str(after);
    Some(Edit { text: out, sel_start: c, sel_end: c })
}

/// Tipo de línea que ponen los atajos (Ctrl+Mayús+7/8/9, Ctrl+Alt+1–3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Bullet,
    Ordered,
    Task,
    Heading(u8),
}

/// La línea sin su marca de título o de lista (pero con su sangría) y qué tipo era.
fn strip_kind(line: &str) -> (String, &str, Option<LineKind>) {
    let indent_len = line.len() - line.trim_start_matches(' ').len();
    let indent = line.get(..indent_len).unwrap_or("").to_string();
    let rest = line.get(indent_len..).unwrap_or("");
    let hashes = rest.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&hashes) && (rest.len() == hashes || rest.get(hashes..hashes + 1) == Some(" ")) {
        let body = rest.get(hashes..).unwrap_or("").strip_prefix(' ').unwrap_or("");
        return (indent, body, Some(LineKind::Heading(hashes as u8)));
    }
    match prefix(line) {
        Some(p) if p.marker != "> " => {
            let kind = if p.task.is_some() {
                LineKind::Task
            } else if p.number.is_some() {
                LineKind::Ordered
            } else {
                LineKind::Bullet
            };
            (p.indent.clone(), line.get(p.len..).unwrap_or(""), Some(kind))
        }
        _ => (indent, rest, None),
    }
}

/// Pone (o quita, si todas ya lo son) el tipo de línea en las líneas de la selección.
pub fn set_line_kind(text: &str, sel_start: usize, sel_end: usize, kind: LineKind) -> Option<Edit> {
    let (a, b) = (byte_of(text, sel_start.min(sel_end)), byte_of(text, sel_start.max(sel_end)));
    if in_fence(text, a) {
        return None;
    }
    let (first, _) = line_at(text, a);
    let (_, last) = line_at(text, b);
    let block = text.get(first..last)?;
    let lines: Vec<&str> = block.split('\n').take(10_000).collect();
    let all_same = lines.iter().all(|l| strip_kind(l).2 == Some(kind));
    // Por línea: (inicio y largo del prefijo antes; inicio y largo del prefijo después).
    let mut spans: Vec<(usize, usize, usize, usize)> = Vec::with_capacity(lines.len());
    let mut new_lines = Vec::with_capacity(lines.len());
    let (mut old_pos, mut new_pos) = (first, first);
    for (n, l) in lines.iter().enumerate() {
        let (indent, body, _) = strip_kind(l);
        let line = if all_same {
            format!("{indent}{body}")
        } else {
            match kind {
                LineKind::Bullet => format!("{indent}- {body}"),
                LineKind::Ordered => format!("{indent}{}. {body}", n + 1),
                LineKind::Task => format!("{indent}- [ ] {body}"),
                LineKind::Heading(h) => format!("{}{} {body}", indent, "#".repeat(usize::from(h.clamp(1, 6)))),
            }
        };
        spans.push((old_pos, l.len() - body.len(), new_pos, line.len() - body.len()));
        old_pos += l.len() + 1;
        new_pos += line.len() + 1;
        new_lines.push(line);
    }
    let mut out = String::with_capacity(text.len() + 16);
    out.push_str(text.get(..first)?);
    out.push_str(&new_lines.join("\n"));
    out.push_str(text.get(last..)?);
    // El cursor sigue a su texto (dentro del prefijo viejo: al inicio del texto).
    let moved = |byte: usize| -> usize {
        let nb = match spans.iter().rev().find(|(os, ..)| *os <= byte) {
            Some(&(os, op, ns, np)) => ns + np + (byte - os).saturating_sub(op),
            None => byte,
        };
        let mut nb = nb.min(out.len());
        while nb > 0 && !out.is_char_boundary(nb) {
            nb -= 1;
        }
        char_of(&out, nb)
    };
    let (s0, s1) = (moved(byte_of(text, sel_start)), moved(byte_of(text, sel_end)));
    Some(Edit { text: out, sel_start: s0, sel_end: s1 })
}

/// Tab (o Mayús+Tab con `outdent`) en un elemento de lista: dos espacios más o menos al principio.
pub fn indent(text: &str, cursor: usize, outdent: bool) -> Option<Edit> {
    let at = byte_of(text, cursor);
    if in_fence(text, at) {
        return None;
    }
    if let Some(e) = in_quote(text, cursor, |t, c| indent(t, c, outdent)) {
        return e;
    }
    let (start, end) = line_at(text, at);
    let line = text.get(start..end)?;
    let p = prefix(line)?;
    if p.marker == "> " {
        return None;
    }
    let mut out = String::with_capacity(text.len() + 2);
    out.push_str(text.get(..start)?);
    let delta: isize = if outdent {
        let remove = p.indent.len().min(2);
        if remove == 0 {
            return None;
        }
        out.push_str(line.get(remove..)?);
        -(remove as isize)
    } else {
        out.push_str("  ");
        out.push_str(line);
        2
    };
    out.push_str(text.get(end..)?);
    let c = cursor.saturating_add_signed(delta).max(char_of(text, start));
    Some(Edit { text: out, sel_start: c, sel_end: c })
}

/// Pone (o quita, si ya están) las marcas `mark` alrededor de la selección. Sin selección, deja
/// el cursor entre las marcas.
pub fn toggle_wrap(text: &str, sel_start: usize, sel_end: usize, mark: &str) -> Edit {
    let (a, b) = (sel_start.min(sel_end), sel_start.max(sel_end));
    let (ab, bb) = (byte_of(text, a), byte_of(text, b));
    let m = mark.chars().count();
    let before = text.get(..ab).unwrap_or("");
    let after = text.get(bb..).unwrap_or("");
    let inside = text.get(ab..bb).unwrap_or("");
    // Ya rodeada: se quitan las marcas.
    if before.ends_with(mark) && after.starts_with(mark) && !inside.is_empty() {
        let out = format!(
            "{}{inside}{}",
            before.get(..before.len() - mark.len()).unwrap_or(""),
            after.get(mark.len()..).unwrap_or("")
        );
        return Edit { text: out, sel_start: a - m, sel_end: b - m };
    }
    if inside.starts_with(mark) && inside.ends_with(mark) && inside.chars().count() >= 2 * m {
        let core = inside.get(mark.len()..inside.len() - mark.len()).unwrap_or("");
        let out = format!("{before}{core}{after}");
        return Edit { text: out, sel_start: a, sel_end: b - 2 * m };
    }
    let out = format!("{before}{mark}{inside}{mark}{after}");
    Edit { text: out, sel_start: a + m, sel_end: b + m }
}

/// Color del texto o resaltado (como IureEditor 1.x, HTML dentro del Markdown).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Paint {
    /// `<span style="color: #RRGGBB">…</span>`
    Color,
    /// `<mark style="background-color: #RRGGBB">…</mark>`
    Highlight,
}

/// `#RRGGBB` válido (en mayúsculas), o `None`.
pub fn hex_color(c: &str) -> Option<String> {
    let h = c.trim().strip_prefix('#')?;
    (h.len() == 6 && h.chars().all(|x| x.is_ascii_hexdigit())).then(|| format!("#{}", h.to_ascii_uppercase()))
}

/// Las etiquetas de apertura y cierre de una pintura.
pub fn paint_tags(kind: Paint, color: &str) -> (String, &'static str) {
    match kind {
        Paint::Color => (format!("<span style=\"color: {color}\">"), "</span>"),
        Paint::Highlight => (format!("<mark style=\"background-color: {color}\">"), "</mark>"),
    }
}

/// La etiqueta de apertura de esa pintura con la que termina `before`, si la hay (su largo en bytes).
fn open_tag_at_end(before: &str, kind: Paint) -> Option<usize> {
    let start = before.rfind('<')?;
    let tag = before.get(start..)?;
    let ok = match kind {
        Paint::Color => tag.starts_with("<span") && tag.contains("color"),
        Paint::Highlight => tag.starts_with("<mark"),
    };
    (ok && tag.ends_with('>') && !tag.contains('\n')).then_some(tag.len())
}

/// Pone el color (o el resaltado) a la selección; con `None` lo quita. Si la selección ya tiene esa
/// pintura justo alrededor, se cambia (o se quita) en vez de anidar otra.
pub fn paint(text: &str, sel_start: usize, sel_end: usize, kind: Paint, color: Option<&str>) -> Edit {
    let (a, b) = (sel_start.min(sel_end), sel_start.max(sel_end));
    let (ab, bb) = (byte_of(text, a), byte_of(text, b));
    let before = text.get(..ab).unwrap_or("");
    let after = text.get(bb..).unwrap_or("");
    let inside = text.get(ab..bb).unwrap_or("");
    let color = color.and_then(hex_color);
    let close = paint_tags(kind, "").1;
    let (before, after, shift) = match open_tag_at_end(before, kind) {
        Some(n) if after.starts_with(close) => {
            let open = before.get(before.len() - n..).unwrap_or("");
            (before.get(..before.len() - n).unwrap_or(""), after.get(close.len()..).unwrap_or(""), open.chars().count())
        }
        _ => (before, after, 0),
    };
    match color {
        Some(c) if !inside.is_empty() => {
            let (open, close) = paint_tags(kind, &c);
            let n = open.chars().count();
            Edit {
                text: format!("{before}{open}{inside}{close}{after}"),
                sel_start: a - shift + n,
                sel_end: b - shift + n,
            }
        }
        _ => Edit { text: format!("{before}{inside}{after}"), sel_start: a - shift, sel_end: b - shift },
    }
}

/// Ctrl+K: `[selección](url)` con «url» seleccionada para escribir la dirección.
pub fn link(text: &str, sel_start: usize, sel_end: usize) -> Edit {
    let (a, b) = (sel_start.min(sel_end), sel_start.max(sel_end));
    let (ab, bb) = (byte_of(text, a), byte_of(text, b));
    let inside = text.get(ab..bb).unwrap_or("");
    let label = if inside.is_empty() { "texto" } else { inside };
    let out = format!("{}[{label}](url){}", text.get(..ab).unwrap_or(""), text.get(bb..).unwrap_or(""));
    let url_at = a + 1 + label.chars().count() + 2;
    Edit { text: out, sel_start: url_at, sel_end: url_at + 3 }
}

#[cfg(test)]
mod tests {

    #[test]
    fn paint_wraps_changes_and_removes() {
        use super::{Paint, paint};
        let t = "uno dos tres";
        let e = paint(t, 4, 7, Paint::Color, Some("#ff0000"));
        assert_eq!(e.text, "uno <span style=\"color: #FF0000\">dos</span> tres");
        let (a, b) = (e.sel_start, e.sel_end);
        assert_eq!(e.text.chars().skip(a).take(b - a).collect::<String>(), "dos");
        // Otro color: se cambia, no se anida.
        let e2 = paint(&e.text, a, b, Paint::Color, Some("#0000FF"));
        assert_eq!(e2.text, "uno <span style=\"color: #0000FF\">dos</span> tres");
        // Sin color: se quita.
        let e3 = paint(&e2.text, e2.sel_start, e2.sel_end, Paint::Color, None);
        assert_eq!((e3.text.as_str(), e3.sel_start, e3.sel_end), (t, 4, 7));
        let h = paint(t, 0, 3, Paint::Highlight, Some("#FFF2CC"));
        assert_eq!(h.text, "<mark style=\"background-color: #FFF2CC\">uno</mark> dos tres");
        // Un color que no es `#RRGGBB` no entra.
        assert_eq!(paint(t, 0, 3, Paint::Color, Some("red\" onload=\"x")).text, t);
    }

    #[test]
    fn lists_inside_quotes_continue_and_indent() {
        use super::{indent, on_enter};
        // Intro en un elemento de un mapa mental: otro elemento, con su `> `.
        let t = "> [!MINDMAP] C\n> - Uno";
        let e = on_enter(t, t.chars().count()).unwrap();
        assert_eq!(e.text, "> [!MINDMAP] C\n> - Uno\n> - ");
        assert_eq!(e.sel_start, e.text.chars().count());
        // Tab lo hace hijo; Mayús+Tab lo regresa.
        let i = indent(&e.text, e.sel_start, false).unwrap();
        assert_eq!(i.text, "> [!MINDMAP] C\n> - Uno\n>   - ");
        assert_eq!(i.sel_start, i.text.chars().count());
        let o = indent(&i.text, i.sel_start, true).unwrap();
        assert_eq!(o.text, e.text);
        // Intro en el elemento vacío sale de la lista (queda la cita); otra vez, de la cita.
        let x = on_enter(&e.text, e.sel_start).unwrap();
        assert_eq!(x.text, "> [!MINDMAP] C\n> - Uno\n> ");
        // Fuera de las citas, igual que siempre.
        assert_eq!(on_enter("- a", 3).unwrap().text, "- a\n- ");
    }

    #[test]
    fn three_enters_leave_a_code_block() {
        // Sin cerrar: se cierra la cerca y el cursor queda en un párrafo nuevo.
        let t = "```rust\nfn f() {}\n\n";
        let e = code_exit(t, t.chars().count()).unwrap();
        assert_eq!(e.text, "```rust\nfn f() {}\n```\n\n");
        assert_eq!(e.sel_start, e.text.chars().count());
        // Cerrado: se respeta la cerca y lo que sigue.
        let t = "```\nx\n\n\n```\ndespués";
        // La segunda línea vacía empieza en el carácter 7.
        let e = code_exit(t, 7).unwrap();
        assert_eq!(e.text, "```\nx\n```\n\ndespués");
        assert_eq!(&e.text.chars().take(e.sel_start).collect::<String>(), "```\nx\n```\n\n");
        // Con una sola línea vacía, o con más código después, o fuera de código: lo normal.
        assert!(code_exit("```\nx\n", 6).is_none());
        assert!(code_exit("```\nx\n\n\ny\n```", 6).is_none());
        assert!(code_exit("párrafo\n\n", 10).is_none());
    }

    #[test]
    fn line_kind_shortcuts_set_and_toggle() {
        let e = set_line_kind("uno\ndos", 0, 7, LineKind::Ordered).unwrap();
        assert_eq!(e.text, "1. uno\n2. dos");
        assert_eq!((e.sel_start, e.sel_end), (3, 13), "la selección sigue a su texto");
        let e = set_line_kind(&e.text, 3, 13, LineKind::Ordered).unwrap();
        assert_eq!(e.text, "uno\ndos", "otra vez: se quita");
        let e = set_line_kind("- [x] hecha", 11, 11, LineKind::Bullet).unwrap();
        assert_eq!(e.text, "- hecha");
        assert_eq!(e.sel_start, 7);
        let e = set_line_kind("  - sub", 7, 7, LineKind::Task).unwrap();
        assert_eq!(e.text, "  - [ ] sub", "conserva la sangría");
        let e = set_line_kind("## Título", 4, 4, LineKind::Heading(1)).unwrap();
        assert_eq!(e.text, "# Título");
        assert_eq!(e.sel_start, 3);
        assert_eq!(set_line_kind("# T", 3, 3, LineKind::Heading(1)).unwrap().text, "T");
        assert!(set_line_kind("```\nx", 5, 5, LineKind::Bullet).is_none(), "nada dentro de código");
    }
    use super::*;

    fn at(text: &str, marker: char) -> (String, usize) {
        let i = text.chars().position(|c| c == marker).unwrap();
        (text.replacen(marker, "", 1), i)
    }

    #[test]
    fn enter_continues_lists_and_quotes() {
        let (t, c) = at("- uno|", '|');
        assert_eq!(on_enter(&t, c).unwrap().text, "- uno\n- ");
        let (t, c) = at("  3. tres|\n", '|');
        assert_eq!(on_enter(&t, c).unwrap().text, "  3. tres\n  4. \n");
        let (t, c) = at("1) a|", '|');
        assert_eq!(on_enter(&t, c).unwrap().text, "1) a\n2) ");
        let (t, c) = at("- [x] hecha|", '|');
        let e = on_enter(&t, c).unwrap();
        assert_eq!(e.text, "- [x] hecha\n- [ ] ");
        assert_eq!(e.sel_start, e.text.chars().count());
        let (t, c) = at("> cita|", '|');
        assert_eq!(on_enter(&t, c).unwrap().text, "> cita\n> ");
        // Partir un elemento por la mitad.
        let (t, c) = at("- uno| dos", '|');
        assert_eq!(on_enter(&t, c).unwrap().text, "- uno\n-  dos");
    }

    #[test]
    fn enter_on_empty_item_leaves_the_list() {
        let (t, c) = at("- uno\n- |", '|');
        let e = on_enter(&t, c).unwrap();
        assert_eq!(e.text, "- uno\n");
        assert_eq!(e.sel_start, 6);
        let (t, c) = at("- [ ] |", '|');
        assert_eq!(on_enter(&t, c).unwrap().text, "");
    }

    #[test]
    fn enter_is_normal_elsewhere() {
        for s in ["párrafo|", "```\n- en código|\n```", "-sin espacio|", "|- al principio"] {
            let (t, c) = at(s, '|');
            assert!(on_enter(&t, c).is_none(), "{s}");
        }
    }

    #[test]
    fn tab_indents_list_items() {
        let (t, c) = at("- uno\n- dos|", '|');
        let e = indent(&t, c, false).unwrap();
        assert_eq!(e.text, "- uno\n  - dos");
        assert_eq!(e.sel_start, c + 2);
        let back = indent(&e.text, e.sel_start, true).unwrap();
        assert_eq!(back.text, t);
        assert!(indent(&t, c, true).is_none(), "sin sangría que quitar");
        assert!(indent("texto", 2, false).is_none());
    }

    #[test]
    fn wrap_and_unwrap() {
        let e = toggle_wrap("hola mundo", 5, 10, "**");
        assert_eq!(e.text, "hola **mundo**");
        assert_eq!((e.sel_start, e.sel_end), (7, 12));
        let back = toggle_wrap(&e.text, e.sel_start, e.sel_end, "**");
        assert_eq!(back.text, "hola mundo");
        let inner = toggle_wrap("a *b* c", 2, 5, "*");
        assert_eq!(inner.text, "a b c");
        let empty = toggle_wrap("ab", 1, 1, "*");
        assert_eq!((empty.text.as_str(), empty.sel_start), ("a**b", 2));
        let ñ = toggle_wrap("año ñu", 4, 6, "~~");
        assert_eq!(ñ.text, "año ~~ñu~~");
    }

    #[test]
    fn link_selects_the_url() {
        let e = link("ver acá", 4, 7);
        assert_eq!(e.text, "ver [acá](url)");
        let sel: String = e.text.chars().skip(e.sel_start).take(e.sel_end - e.sel_start).collect();
        assert_eq!(sel, "url");
    }

    proptest::proptest! {
        #[test]
        fn never_panics(text in "(\\PC|- |> |1\\. |\\[ \\] |```|\n|  ){0,60}", c in 0usize..80, d in 0usize..80) {
            let n = text.chars().count();
            let (c, d) = (c.min(n), d.min(n));
            let _ = on_enter(&text, c);
            let _ = indent(&text, c, false);
            let _ = indent(&text, c, true);
            let e = toggle_wrap(&text, c, d, "**");
            proptest::prop_assert!(e.sel_end <= e.text.chars().count());
            let l = link(&text, c, d);
            proptest::prop_assert!(l.sel_end <= l.text.chars().count());
        }
    }
}
