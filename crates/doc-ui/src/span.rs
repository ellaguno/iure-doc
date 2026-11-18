//! Selección que cruza bloques: operaciones puras sobre el texto del documento (sin egui).
//!
//! Las posiciones son caracteres del documento entero (no bytes).

/// Hacia dónde mueve la cabeza de la selección una tecla con Mayús.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Move {
    Left,
    Right,
    Up,
    Down,
    LineStart,
    LineEnd,
    DocStart,
    DocEnd,
}

fn byte_at(text: &str, ch: usize) -> usize {
    text.char_indices().nth(ch).map_or(text.len(), |(i, _)| i)
}

/// El texto seleccionado (`a` y `b` en cualquier orden).
pub(crate) fn slice(text: &str, a: usize, b: usize) -> String {
    let (a, b) = (a.min(b), a.max(b));
    text.chars().skip(a).take(b - a).collect()
}

/// Cambia lo seleccionado por `with`; devuelve el texto nuevo y dónde queda el cursor.
pub(crate) fn replace(text: &str, a: usize, b: usize, with: &str) -> (String, usize) {
    let n = text.chars().count();
    let (a, b) = (a.min(b).min(n), a.max(b).min(n));
    let (ab, bb) = (byte_at(text, a), byte_at(text, b));
    let mut out = String::with_capacity(text.len() + with.len());
    out.push_str(text.get(..ab).unwrap_or(""));
    out.push_str(with);
    out.push_str(text.get(bb..).unwrap_or(""));
    (out, a.saturating_add(with.chars().count()))
}

/// Inicio de la línea (en caracteres) y columna de la posición `at`.
fn line_of(chars: &[char], at: usize) -> (usize, usize) {
    let at = at.min(chars.len());
    let start = chars.get(..at).and_then(|c| c.iter().rposition(|&x| x == '\n')).map_or(0, |i| i + 1);
    (start, at - start)
}

fn line_end(chars: &[char], start: usize) -> usize {
    chars.get(start..).and_then(|c| c.iter().position(|&x| x == '\n')).map_or(chars.len(), |i| start + i)
}

/// La nueva cabeza tras mover `head` (por líneas del texto, no por filas en pantalla).
pub(crate) fn move_head(text: &str, head: usize, m: Move) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let head = head.min(n);
    let (start, col) = line_of(&chars, head);
    match m {
        Move::Left => head.saturating_sub(1),
        Move::Right => (head + 1).min(n),
        Move::LineStart => start,
        Move::LineEnd => line_end(&chars, start),
        Move::DocStart => 0,
        Move::DocEnd => n,
        Move::Up => {
            if start == 0 {
                return 0;
            }
            let (prev, _) = line_of(&chars, start - 1);
            (prev + col).min(start - 1)
        }
        Move::Down => {
            let end = line_end(&chars, start);
            if end >= n {
                return n;
            }
            let next = end + 1;
            (next + col).min(line_end(&chars, next))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn replace_and_slice_work_in_characters() {
        assert_eq!(slice("añob\n\ncé", 2, 7), "ob\n\nc");
        assert_eq!(slice("abc", 2, 0), "ab");
        assert_eq!(replace("uno\n\ndós", 2, 7, "X"), ("unXs".to_string(), 3));
        assert_eq!(replace("abc", 9, 1, ""), ("a".to_string(), 1));
    }

    #[test]
    fn moving_by_lines_keeps_the_column() {
        let t = "uno\n\ndoscientos\nx";
        assert_eq!(move_head(t, 2, Move::Down), 4, "línea vacía");
        assert_eq!(move_head(t, 4, Move::Down), 5);
        assert_eq!(move_head(t, 8, Move::Down), 17, "columna 3 en «x»: su final");
        assert_eq!(move_head(t, 8, Move::Up), 4);
        assert_eq!(move_head(t, 1, Move::Up), 0);
        assert_eq!(move_head(t, 17, Move::Down), 17);
        assert_eq!(move_head(t, 7, Move::LineStart), 5);
        assert_eq!(move_head(t, 7, Move::LineEnd), 15);
        assert_eq!(move_head(t, 7, Move::DocEnd), 17);
    }

    proptest! {
        #[test]
        fn operations_never_leave_the_text(t in "(\\PC|\n){0,40}", head in 0usize..60, a in 0usize..60, b in 0usize..60, w in "\\PC{0,4}") {
            let n = t.chars().count();
            for m in [Move::Left, Move::Right, Move::Up, Move::Down, Move::LineStart, Move::LineEnd, Move::DocStart, Move::DocEnd] {
                prop_assert!(move_head(&t, head, m) <= n);
            }
            let (out, cur) = replace(&t, a, b, &w);
            prop_assert!(cur <= out.chars().count());
            let lo = a.min(b).min(n);
            let hi = a.max(b).min(n);
            prop_assert_eq!(out.chars().count(), n - (hi - lo) + w.chars().count());
        }
    }
}
