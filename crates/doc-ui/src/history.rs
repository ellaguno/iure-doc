//! Deshacer y rehacer de todo el documento (sin egui).
//!
//! Cada paso guarda solo lo que cambió (dónde, qué había y qué hay), no copias del documento: con
//! 100 páginas un paso de una tecla ocupa unos bytes. Teclear seguido (menos de un segundo entre
//! teclas, sin saltos de línea) es un solo paso, como en cualquier editor.

/// Un cambio: en `at` (bytes) el texto `old` pasó a ser `new`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Change {
    at: usize,
    old: String,
    new: String,
    /// Cuándo (segundos de egui), para juntar lo que se teclea seguido.
    time: f64,
}

/// Pasos como mucho.
const MAX_STEPS: usize = 500;
/// Bytes guardados como mucho entre todos los pasos.
const MAX_BYTES: usize = 32 * 1024 * 1024;
/// Teclas a menos de este tiempo se juntan en un paso.
const MERGE_SECS: f64 = 1.0;

/// El cambio de `a` a `b` (lo que no es prefijo ni sufijo común); `None` si son iguales.
pub(crate) fn diff(a: &str, b: &str, time: f64) -> Option<Change> {
    if a == b {
        return None;
    }
    let (ab, bb) = (a.as_bytes(), b.as_bytes());
    let mut pre = ab.iter().zip(bb).take_while(|(x, y)| x == y).count();
    while pre > 0 && !a.is_char_boundary(pre) {
        pre -= 1;
    }
    let room = ab.len().min(bb.len()) - pre;
    let mut suf = ab.iter().rev().zip(bb.iter().rev()).take(room).take_while(|(x, y)| x == y).count();
    while suf > 0 && !a.is_char_boundary(ab.len() - suf) {
        suf -= 1;
    }
    Some(Change {
        at: pre,
        old: a.get(pre..ab.len() - suf).unwrap_or("").to_string(),
        new: b.get(pre..bb.len() - suf).unwrap_or("").to_string(),
        time,
    })
}

/// La historia de un documento.
#[derive(Clone, Debug, Default)]
pub(crate) struct History {
    undo: Vec<Change>,
    redo: Vec<Change>,
}

impl History {
    /// El documento pasó de `before` a `after`.
    pub(crate) fn record(&mut self, before: &str, after: &str, time: f64) {
        let Some(c) = diff(before, after, time) else { return };
        self.redo.clear();
        if let Some(last) = self.undo.last_mut()
            && time - last.time < MERGE_SECS
        {
            // Se sigue tecleando donde acabó el paso anterior.
            if c.old.is_empty() && !c.new.contains('\n') && c.at == last.at + last.new.len() && !last.new.contains('\n')
            {
                last.new.push_str(&c.new);
                last.time = time;
                return;
            }
            // Se sigue borrando hacia atrás desde donde acabó el borrado anterior.
            if c.new.is_empty() && last.new.is_empty() && c.at + c.old.len() == last.at && !c.old.contains('\n') {
                last.old.insert_str(0, &c.old);
                last.at = c.at;
                last.time = time;
                return;
            }
        }
        self.undo.push(c);
        while self.undo.len() > MAX_STEPS || self.bytes() > MAX_BYTES {
            if self.undo.is_empty() {
                break;
            }
            self.undo.remove(0);
        }
    }

    fn bytes(&self) -> usize {
        self.undo.iter().map(|c| c.old.len() + c.new.len()).sum()
    }

    /// Deshace un paso en `text`; devuelve dónde queda el cursor (bytes). Si el texto ya no es el
    /// que la historia espera (lo cambió otro), la historia se vacía y no se toca nada.
    pub(crate) fn undo(&mut self, text: &mut String) -> Option<usize> {
        let c = self.undo.pop()?;
        if text.get(c.at..c.at + c.new.len()) != Some(c.new.as_str()) {
            *self = History::default();
            return None;
        }
        text.replace_range(c.at..c.at + c.new.len(), &c.old);
        let cursor = c.at + c.old.len();
        self.redo.push(c);
        Some(cursor)
    }

    /// Rehace un paso deshecho.
    pub(crate) fn redo(&mut self, text: &mut String) -> Option<usize> {
        let c = self.redo.pop()?;
        if text.get(c.at..c.at + c.old.len()) != Some(c.old.as_str()) {
            *self = History::default();
            return None;
        }
        text.replace_range(c.at..c.at + c.old.len(), &c.new);
        let cursor = c.at + c.new.len();
        // Rehecho no se junta con lo que se teclee después.
        self.undo.push(Change { time: f64::NEG_INFINITY, ..c });
        Some(cursor)
    }

    /// Lo que el anfitrión cambió por su cuenta deja la historia sin sentido.
    pub(crate) fn clear(&mut self) {
        *self = History::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn diff_finds_the_changed_middle() {
        let c = diff("hola mundo", "hola gran mundo", 0.0).unwrap();
        assert_eq!((c.at, c.old.as_str(), c.new.as_str()), (5, "", "gran "));
        let c = diff("añño", "año", 0.0).unwrap();
        assert_eq!(c.old.chars().count(), 1, "en límites de carácter: {c:?}");
        assert!(diff("x", "x", 0.0).is_none());
    }

    #[test]
    fn typing_quickly_is_one_step_and_newlines_split_steps() {
        let mut h = History::default();
        let mut t = String::new();
        for (i, s) in ["h", "ho", "hol", "hola", "hola\n", "hola\nx"].iter().enumerate() {
            h.record(&t, s, i as f64 * 0.2);
            t = s.to_string();
        }
        assert_eq!(h.undo(&mut t), Some(5));
        assert_eq!(t, "hola\n");
        assert_eq!(h.undo(&mut t), Some(4));
        assert_eq!(t, "hola");
        assert_eq!(h.undo(&mut t), Some(0));
        assert_eq!(t, "");
        assert_eq!(h.undo(&mut t), None);
        assert_eq!(h.redo(&mut t), Some(4));
        assert_eq!(t, "hola");
        // Tras una pausa larga, otro paso.
        let mut h = History::default();
        h.record("", "a", 0.0);
        h.record("a", "ab", 5.0);
        let mut t = "ab".to_string();
        h.undo(&mut t);
        assert_eq!(t, "a");
    }

    #[test]
    fn backspacing_quickly_is_one_step() {
        let mut h = History::default();
        h.record("hola", "hol", 0.0);
        h.record("hol", "ho", 0.1);
        h.record("ho", "h", 0.2);
        let mut t = "h".to_string();
        assert_eq!(h.undo(&mut t), Some(4));
        assert_eq!(t, "hola");
    }

    #[test]
    fn a_text_changed_by_someone_else_empties_the_history() {
        let mut h = History::default();
        h.record("a", "ab", 0.0);
        let mut t = "otro".to_string();
        assert_eq!(h.undo(&mut t), None);
        assert_eq!(t, "otro");
        assert_eq!(h.redo(&mut t), None);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]
        /// Deshacer todo vuelve al principio y rehacer todo, al final, sea cual sea la secuencia.
        #[test]
        fn undo_all_and_redo_all(steps in proptest::collection::vec(("(\\PC|\n){0,6}", 0usize..40, 0usize..4, 0u8..3), 0..30)) {
            let mut h = History::default();
            let mut t = String::new();
            let mut versions = vec![t.clone()];
            for (i, (ins, at, del, pause)) in steps.iter().enumerate() {
                let chars: Vec<char> = t.chars().collect();
                let at = (*at).min(chars.len());
                let del = (*del).min(chars.len() - at);
                let mut next: String = chars.iter().take(at).collect();
                next.push_str(ins);
                next.extend(chars.iter().skip(at + del));
                h.record(&t, &next, i as f64 * if *pause == 0 { 2.0 } else { 0.1 });
                if next != t {
                    versions.push(next.clone());
                }
                t = next;
            }
            let last = t.clone();
            while h.undo(&mut t).is_some() {
                prop_assert!(versions.contains(&t), "un paso intermedio que no existió: {:?}", t);
            }
            prop_assert_eq!(&t, &versions[0]);
            while h.redo(&mut t).is_some() {}
            prop_assert_eq!(t, last);
        }
    }
}
