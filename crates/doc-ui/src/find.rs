//! S1.12: buscar y reemplazar (sin egui). Las posiciones son caracteres del documento.

use std::ops::Range;

/// Coincidencias como mucho.
pub(crate) const MAX_MATCHES: usize = 100_000;
/// Búsqueda más larga.
const MAX_QUERY: usize = 1_000;

/// `\n` y `\t` de la búsqueda como salto de línea y tabulador (`\\` es la barra), como en
/// IureEditor.
pub(crate) fn unescape(q: &str) -> String {
    let mut out = String::with_capacity(q.len());
    let mut chars = q.chars().take(MAX_QUERY).peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.peek() {
                Some('n') => {
                    out.push('\n');
                    chars.next();
                }
                Some('t') => {
                    out.push('\t');
                    chars.next();
                }
                Some('\\') => {
                    out.push('\\');
                    chars.next();
                }
                _ => out.push(c),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Un carácter para comparar sin distinguir mayúsculas (uno por uno: las posiciones no cambian).
fn fold(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// Todas las coincidencias de `query` en `text` (sin encimarse), en caracteres.
pub(crate) fn find_all(text: &str, query: &str, case: bool) -> Vec<Range<usize>> {
    let q: Vec<char> = if case { query.chars().collect() } else { query.chars().map(fold).collect() };
    let Some(&first) = q.first() else { return Vec::new() };
    let t: Vec<char> = if case { text.chars().collect() } else { text.chars().map(fold).collect() };
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + q.len() <= t.len() && out.len() < MAX_MATCHES {
        if t.get(i) == Some(&first) && t.get(i..i + q.len()) == Some(q.as_slice()) {
            out.push(i..i + q.len());
            i += q.len();
        } else {
            i += 1;
        }
    }
    out
}

/// Cambia las coincidencias (ordenadas, sin encimarse) por `with`; devuelve el texto nuevo.
pub(crate) fn replace_all(text: &str, matches: &[Range<usize>], with: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut m = matches.iter().peekable();
    let mut skip_until = 0usize;
    for (i, c) in text.chars().enumerate() {
        if i < skip_until {
            continue;
        }
        if let Some(r) = m.peek().map(|r| (*r).clone())
            && r.start == i
        {
            out.push_str(with);
            skip_until = r.end;
            m.next();
            if r.end > r.start {
                continue;
            }
        }
        out.push(c);
    }
    out
}

/// La coincidencia que sigue a la posición `at` (o la primera), para empezar a buscar desde el
/// cursor.
pub(crate) fn first_from(matches: &[Range<usize>], at: usize) -> usize {
    matches.iter().position(|r| r.start >= at).unwrap_or(0)
}

#[cfg(test)]
#[allow(clippy::single_range_in_vec_init)] // las coincidencias esperadas son listas de rangos
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn finds_with_and_without_case() {
        let t = "Contrato y contrato y CONTRATO";
        assert_eq!(find_all(t, "contrato", false), [0..8, 11..19, 22..30]);
        assert_eq!(find_all(t, "contrato", true), vec![11usize..19]);
        assert_eq!(find_all("añaña", "aña", false), vec![0usize..3], "sin encimarse");
        assert_eq!(find_all("Ñandú", "ñ", false), vec![0usize..1], "acentos y eñes");
        assert!(find_all("x", "", false).is_empty());
        assert_eq!(find_all("uno\ndos", &unescape("o\\nd"), false), vec![2usize..5], "saltos con \\n");
        assert_eq!(unescape("a\\\\nb\\t"), "a\\nb\t");
    }

    #[test]
    fn replaces_all_matches() {
        let t = "el contrato y el Contrato";
        let m = find_all(t, "contrato", false);
        assert_eq!(replace_all(t, &m, "convenio"), "el convenio y el convenio");
        assert_eq!(replace_all("aaa", &find_all("aaa", "a", true), ""), "");
        assert_eq!(first_from(&m, 5), 1);
        assert_eq!(first_from(&m, 99), 0, "pasado el final, la primera");
    }

    proptest! {
        /// Reemplazar todo deja el texto sin la búsqueda (con un reemplazo de otras letras) y con el
        /// largo esperado.
        #[test]
        fn replace_all_is_consistent(t in "[a-cñ \\n]{0,60}", q in "[a-cñ]{1,3}", w in "[x-z]{0,3}") {
            let m = find_all(&t, &q, true);
            let out = replace_all(&t, &m, &w);
            if !w.is_empty() {
                // El reemplazo separa lo que queda: ya no hay coincidencias.
                prop_assert!(find_all(&out, &q, true).is_empty(), "{:?}", out);
            }
            let n = t.chars().count() - m.len() * q.chars().count() + m.len() * w.chars().count();
            prop_assert_eq!(out.chars().count(), n);
        }
    }
}
