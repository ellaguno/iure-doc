//! Tablas de Markdown (GFM) como cuadrícula de celdas, para editarlas en su lugar.
//!
//! Cada celda es su texto en Markdown (sin saltos de línea; `**negrita**` y compañía se
//! conservan). [`Grid::parse`] lee una tabla; [`Grid::to_markdown`] la escribe con las columnas
//! alineadas. Solo se escribe una tabla que se cambió: la que no se toca queda como estaba.

use crate::Align;

/// Columnas como mucho (lo demás no se modela: la tabla queda como texto).
pub const MAX_COLS: usize = 64;
/// Filas como mucho.
pub const MAX_ROWS: usize = 10_000;
/// Caracteres como mucho en una celda al escribir (se recorta el resto).
pub const MAX_CELL: usize = 4_000;

/// Una tabla: alineación por columna y filas de celdas; la fila 0 es el encabezado. Todas las
/// filas tienen `align.len()` celdas.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grid {
    pub align: Vec<Align>,
    pub rows: Vec<Vec<String>>,
}

/// Parte una línea de tabla en celdas (por `|` sin escapar), sin los bordes y sin espacios.
fn cells(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = t.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'|') => {
                cur.push('|');
                chars.next();
            }
            '|' => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
        if out.len() > MAX_COLS {
            break;
        }
    }
    // Sin `|` final, lo que queda es la última celda.
    if !cur.trim().is_empty() || !t.trim_end().ends_with('|') || t.trim_end().ends_with("\\|") {
        out.push(cur);
    }
    out.into_iter().map(|c| c.trim().to_string()).collect()
}

/// La alineación de una celda de la fila separadora (`:--`, `:-:`, `--:`, `---`).
fn delimiter(cell: &str) -> Option<Align> {
    let c = cell.trim();
    let left = c.starts_with(':');
    let right = c.ends_with(':') && c.len() > 1;
    let dashes = c.trim_matches(':');
    if dashes.is_empty() || !dashes.chars().all(|x| x == '-') {
        return None;
    }
    Some(match (left, right) {
        (true, true) => Align::Center,
        (true, false) => Align::Left,
        (false, true) => Align::Right,
        (false, false) => Align::None,
    })
}

fn width(s: &str) -> usize {
    s.chars().count()
}

impl Grid {
    /// Una tabla nueva de `cols` columnas y `rows` filas (más el encabezado), vacía.
    pub fn new(cols: usize, rows: usize) -> Grid {
        let cols = cols.clamp(1, MAX_COLS);
        Grid { align: vec![Align::None; cols], rows: vec![vec![String::new(); cols]; rows.min(MAX_ROWS) + 1] }
    }

    /// Lee una tabla GFM (encabezado, fila separadora y filas). `None` si el texto no es una
    /// tabla así (o es demasiado grande).
    pub fn parse(src: &str) -> Option<Grid> {
        let lines: Vec<&str> = src.lines().filter(|l| !l.trim().is_empty()).collect();
        if lines.len() < 2 || lines.len() > MAX_ROWS + 1 {
            return None;
        }
        let head = cells(lines.first()?);
        let align: Vec<Align> = cells(lines.get(1)?).iter().map(|c| delimiter(c)).collect::<Option<_>>()?;
        if align.is_empty() || align.len() != head.len() || align.len() > MAX_COLS {
            return None;
        }
        // Cada fila tiene tantas celdas como el encabezado (GFM: las de más se ignoran, las de
        // menos van vacías).
        let fit = |mut r: Vec<String>| {
            r.resize(align.len(), String::new());
            r
        };
        let mut rows = vec![fit(head)];
        for l in lines.iter().skip(2) {
            rows.push(fit(cells(l)));
        }
        Some(Grid { align, rows })
    }

    pub fn cols(&self) -> usize {
        self.align.len()
    }

    /// Markdown de la tabla, con las columnas alineadas y `|` escapados en las celdas.
    pub fn to_markdown(&self) -> String {
        let cols = self.cols().max(1);
        let esc = |c: &str| -> String {
            let one: String = c.chars().take(MAX_CELL).map(|x| if x == '\n' || x == '\r' { ' ' } else { x }).collect();
            one.trim().replace('|', "\\|")
        };
        let rows: Vec<Vec<String>> =
            self.rows.iter().map(|r| (0..cols).map(|i| esc(r.get(i).map_or("", String::as_str))).collect()).collect();
        let widths: Vec<usize> = (0..cols)
            .map(|i| rows.iter().map(|r| r.get(i).map_or(0, |c| width(c))).max().unwrap_or(0).max(3))
            .collect();
        let line = |r: &Vec<String>| -> String {
            let mut s = String::from("|");
            for (i, w) in widths.iter().enumerate() {
                let c = r.get(i).map_or("", String::as_str);
                let pad = w.saturating_sub(width(c));
                let (l, rt) = match self.align.get(i).copied().unwrap_or_default() {
                    Align::Right => (pad, 0),
                    Align::Center => (pad / 2, pad - pad / 2),
                    _ => (0, pad),
                };
                s.push(' ');
                s.push_str(&" ".repeat(l));
                s.push_str(c);
                s.push_str(&" ".repeat(rt));
                s.push_str(" |");
            }
            s
        };
        let mut out = String::new();
        if let Some(h) = rows.first() {
            out.push_str(&line(h));
            out.push('\n');
        }
        out.push('|');
        for (i, w) in widths.iter().enumerate() {
            let (l, r) = match self.align.get(i).copied().unwrap_or_default() {
                Align::Left => (":", "-"),
                Align::Center => (":", ":"),
                Align::Right => ("-", ":"),
                Align::None => ("-", "-"),
            };
            out.push(' ');
            out.push_str(l);
            out.push_str(&"-".repeat(w.saturating_sub(2)));
            out.push_str(r);
            out.push_str(" |");
        }
        for r in rows.iter().skip(1) {
            out.push('\n');
            out.push_str(&line(r));
        }
        out
    }

    /// Inserta una fila vacía en `at` (después del encabezado como mínimo).
    pub fn insert_row(&mut self, at: usize) {
        if self.rows.len() > MAX_ROWS {
            return;
        }
        let at = at.clamp(1, self.rows.len());
        self.rows.insert(at, vec![String::new(); self.cols()]);
    }

    /// Quita la fila `at` (el encabezado no se quita).
    pub fn remove_row(&mut self, at: usize) {
        if at >= 1 && at < self.rows.len() {
            self.rows.remove(at);
        }
    }

    /// Inserta una columna vacía en `at`.
    pub fn insert_col(&mut self, at: usize) {
        if self.cols() >= MAX_COLS {
            return;
        }
        let at = at.min(self.cols());
        self.align.insert(at, Align::None);
        for r in &mut self.rows {
            r.insert(at.min(r.len()), String::new());
        }
    }

    /// Quita la columna `at` (queda al menos una).
    pub fn remove_col(&mut self, at: usize) {
        if self.cols() <= 1 || at >= self.cols() {
            return;
        }
        self.align.remove(at);
        for r in &mut self.rows {
            if at < r.len() {
                r.remove(at);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn reads_and_writes_aligned_tables() {
        let g = Grid::parse("| Concepto | Monto |\n|---|--:|\n| Honorarios | 12 000 |\n| Gastos \\| viáticos | 800")
            .unwrap();
        assert_eq!(g.align, vec![Align::None, Align::Right]);
        assert_eq!(g.rows.get(2).unwrap(), &vec!["Gastos | viáticos".to_string(), "800".to_string()]);
        assert_eq!(
            g.to_markdown(),
            "| Concepto           |  Monto |\n| ------------------ | -----: |\n| Honorarios         | 12 000 |\n| Gastos \\| viáticos |    800 |"
        );
        assert!(Grid::parse("no es tabla\n\notra línea").is_none());
        assert!(Grid::parse("| a | b |\n|---|").is_none(), "el separador no coincide");
        // Filas cortas se completan; largas se recortan.
        let g = Grid::parse("a | b\n-- | --\n1\n1 | 2 | 3").unwrap();
        assert_eq!(g.rows, vec![vec!["a", "b"], vec!["1", ""], vec!["1", "2"]]);
    }

    #[test]
    fn rows_and_columns() {
        let mut g = Grid::new(2, 1);
        g.insert_col(1);
        g.insert_row(99);
        assert_eq!((g.cols(), g.rows.len()), (3, 3));
        g.remove_row(0);
        assert_eq!(g.rows.len(), 3, "el encabezado no se quita");
        g.remove_col(0);
        g.remove_col(0);
        g.remove_col(0);
        assert_eq!(g.cols(), 1, "queda una columna");
        assert!(g.rows.iter().all(|r| r.len() == 1));
    }

    fn is_table(md: &str, cols: usize) -> bool {
        use pulldown_cmark::{Event, Options, Parser, Tag};
        Parser::new_ext(md, Options::ENABLE_TABLES).any(|e| matches!(e, Event::Start(Tag::Table(a)) if a.len() == cols))
    }

    fn grid() -> impl Strategy<Value = Grid> {
        (1usize..5, 0usize..5).prop_flat_map(|(c, r)| {
            let cell = "[a-zA-Z0-9ñé*_`|]{0,3}( [a-z|]{1,3}){0,2}";
            let align = prop::sample::select(vec![Align::None, Align::Left, Align::Center, Align::Right]);
            (proptest::collection::vec(align, c), proptest::collection::vec(proptest::collection::vec(cell, c), r + 1))
                // Las celdas no guardan espacios en los bordes (la tabla los recorta).
                .prop_map(|(align, rows)| Grid {
                    align,
                    rows: rows.into_iter().map(|r| r.into_iter().map(|c| c.trim().to_string()).collect()).collect(),
                })
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]
        /// Escribir y volver a leer da la misma cuadrícula, y es una tabla para pulldown-cmark.
        #[test]
        fn write_then_parse_is_the_same_grid(g in grid()) {
            let md = g.to_markdown();
            prop_assert_eq!(Grid::parse(&md), Some(g.clone()), "{}", md);
            prop_assert!(is_table(&md, g.cols()), "{}", md);
        }

        /// Texto cualquiera nunca rompe la lectura, y lo que se lee se escribe y relee igual.
        #[test]
        fn hostile_text_is_safe(t in "(\\PC|\n|\\||-|:){0,120}") {
            if let Some(g) = Grid::parse(&t) {
                prop_assert!(g.rows.iter().all(|r| r.len() == g.cols()));
                let md = g.to_markdown();
                prop_assert_eq!(Grid::parse(&md).map(|x| x.to_markdown()), Some(md));
            }
        }
    }
}
