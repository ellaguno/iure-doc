//! S1.12: `DocEditor` **por bloques**, para documentos largos (IureEditor 2.0).
//!
//! Con un solo `TextEdit`, egui vuelve a maquetar todo el texto en cada tecla: ~40 ms con 100
//! páginas. Aquí el texto se parte en sus bloques de primer nivel ([`iure_doc::chunks`]) y cada uno
//! es su propio campo con su caché de dibujo; solo se dibujan los que se ven (más una pantalla
//! arriba y abajo), los demás ocupan su alto medido o estimado. Teclear rehace solo el bloque
//! editado.
//!
//! Cada bloque guarda su texto sin el espacio final (`content`) y ese espacio aparte (`sep`):
//! juntos dan el documento byte a byte, así que abrir y guardar sin tocar nada no lo cambia.
//!
//! Entre bloques: **Intro** al final de un bloque abre uno nuevo debajo (dentro de una lista sigue
//! la lista); **Retroceso** al inicio lo une con el anterior y **Supr** al final con el siguiente;
//! las flechas pasan de un bloque a otro. Un bloque que al editarse queda en varios (pegar, dos
//! Intro seguidos) se parte solo.
//!
//! **Deshacer y rehacer** (Ctrl+Z, Ctrl+Mayús+Z o Ctrl+Y) son del documento entero
//! ([`crate::history`]): deshacen también partir, unir y crear bloques, y lo tecleado seguido es un
//! solo paso.
//!
//! **Selección que cruza bloques** ([`crate::span`]): Ctrl+A, Mayús+flechas en el borde de un
//! bloque, Mayús+clic en otro bloque o arrastrar de un bloque a otro. Mientras existe, el editor
//! toma copiar, cortar, pegar, borrar, teclear encima, Intro y Mayús+flechas (por líneas del
//! texto); Esc, una flecha o un clic la quitan. La pinta el editor sobre cada bloque visible.
//!
//! **Tablas en su lugar** ([`crate::table_ui`]): un bloque que es una tabla se ve como cuadrícula
//! de celdas; solo se reescribe (con las columnas alineadas) si se cambia.
//!
//! **Diagramas en su lugar** ([`crate::diagrams`], con [`show_with`]): los bloques que la app sabe
//! dibujar se ven como imagen; un clic o entrar con las flechas muestra su código.

use std::hash::{Hash, Hasher};

use egui::text::{CCursor, CCursorRange};
use egui::text_edit::TextEditState;
use egui::{Id, Key, Ui};
use iure_doc::edit;

use std::sync::Arc;

use iure_doc::table::Grid;

use crate::diagrams::{DiagramRenderer, View};
use crate::table_ui::{self, TableAct};
use crate::{EditorStyle, Output, cached_galley, field, span, style_key};

/// Un bloque del editor.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Chunk {
    /// Identidad estable (estado del campo: cursor, deshacer).
    pub(crate) uid: u64,
    /// El texto del bloque, sin el espacio que lo sigue.
    pub(crate) content: String,
    /// El espacio que lo sigue (saltos de línea).
    pub(crate) sep: String,
    /// Creado en el editor (Intro al final de un bloque): vacío no escribe nada y, con texto, va
    /// separado del anterior por una línea en blanco.
    pub(crate) born: bool,
    /// Alto (medido, o estimado mientras no se ha dibujado) y el ancho con que se calculó.
    height: f32,
    width: f32,
    /// Una tabla que se edita como Markdown (vuelve a cuadrícula al dejarla).
    raw: bool,
}

/// Texto y espacio final de un trozo: el texto acaba al final de la línea de su último carácter
/// visible (los espacios de esa línea, que pueden ser un salto forzado, se quedan).
fn split_ws(piece: &str) -> (String, String) {
    let Some((i, c)) = piece.char_indices().rev().find(|(_, c)| !c.is_whitespace()) else {
        return (String::new(), piece.to_string());
    };
    let after = i + c.len_utf8();
    let mut end = piece.get(after..).and_then(|t| t.find('\n')).map_or(piece.len(), |n| after + n);
    if end > after && piece.get(end - 1..end) == Some("\r") {
        end -= 1;
    }
    (piece.get(..end).unwrap_or("").to_string(), piece.get(end..).unwrap_or("").to_string())
}

/// Los bloques de un documento y las operaciones entre ellos (sin egui).
#[derive(Clone, Debug, Default)]
pub(crate) struct Blocks {
    pub(crate) chunks: Vec<Chunk>,
    next_uid: u64,
}

/// Dónde poner el cursor: bloque (`uid`) y posición en caracteres.
pub(crate) type Focus = (u64, usize);

impl Blocks {
    pub(crate) fn new(text: &str) -> Blocks {
        let mut b = Blocks::default();
        b.chunks = b.pieces(text, false);
        if b.chunks.is_empty() {
            let c = b.chunk(String::new(), String::new(), true);
            b.chunks.push(c);
        }
        b
    }

    fn chunk(&mut self, content: String, sep: String, born: bool) -> Chunk {
        self.next_uid = self.next_uid.wrapping_add(1);
        Chunk { uid: self.next_uid, content, sep, born, height: 0.0, width: 0.0, raw: false }
    }

    fn pieces(&mut self, text: &str, born: bool) -> Vec<Chunk> {
        iure_doc::chunks(text)
            .into_iter()
            .map(|r| {
                let (c, s) = split_ws(text.get(r).unwrap_or(""));
                self.chunk(c, s, born)
            })
            .collect()
    }

    /// El documento.
    pub(crate) fn text(&self) -> String {
        let mut out = String::with_capacity(self.chunks.iter().map(|c| c.content.len() + c.sep.len()).sum());
        for c in &self.chunks {
            if c.born {
                if c.content.is_empty() {
                    continue;
                }
                if !out.is_empty() {
                    while !out.ends_with("\n\n") {
                        out.push('\n');
                    }
                }
            }
            out.push_str(&c.content);
            out.push_str(&c.sep);
        }
        out
    }

    /// El bloque `i` cambió (el cursor quedó en `cursor`, en caracteres): se vuelve a partir por si
    /// ahora son varios. Devuelve dónde queda el cursor.
    pub(crate) fn edited(&mut self, i: usize, cursor: usize) -> Option<Focus> {
        let old = self.chunks.get(i)?;
        let (uid, born, height, width) = (old.uid, old.born, old.height, old.width);
        let text = format!("{}{}", old.content, old.sep);
        let mut pieces = self.pieces(&text, born);
        if pieces.is_empty() {
            // Quedó vacío: se queda el bloque, sin texto.
            let c = self.chunks.get_mut(i)?;
            c.content.clear();
            return Some((uid, 0));
        }
        if let Some(p) = pieces.first_mut() {
            p.uid = uid;
            p.height = height;
            p.width = width;
        }
        // El cursor, contado sobre el texto entero, cae en uno de los trozos.
        let mut left = cursor;
        let mut target = None;
        for p in &pieces {
            let n = p.content.chars().count();
            let total = n.saturating_add(p.sep.chars().count());
            if left <= n {
                target = Some((p.uid, left));
                break;
            }
            if left < total {
                target = Some((p.uid, n));
                break;
            }
            left -= total;
        }
        let target = target.or_else(|| pieces.last().map(|p| (p.uid, p.content.chars().count())));
        self.chunks.splice(i..=i, pieces);
        target
    }

    /// Intro al final del bloque `i`: uno nuevo y vacío debajo.
    pub(crate) fn break_after(&mut self, i: usize) -> Option<Focus> {
        if i >= self.chunks.len() || self.chunks.len() >= iure_doc::MAX_BLOCKS {
            return None;
        }
        let c = self.chunk(String::new(), "\n\n".to_string(), true);
        let uid = c.uid;
        self.chunks.insert(i + 1, c);
        Some((uid, 0))
    }

    /// Retroceso al inicio del bloque `i`: se une con el anterior.
    pub(crate) fn merge_prev(&mut self, i: usize) -> Option<Focus> {
        let p = i.checked_sub(1)?;
        let cur = self.chunks.get(i)?.clone();
        let prev = self.chunks.get(p)?.clone();
        if cur.born && cur.content.is_empty() {
            self.chunks.remove(i);
            return Some((prev.uid, prev.content.chars().count()));
        }
        if prev.born && prev.content.is_empty() {
            self.chunks.remove(p);
            return Some((cur.uid, 0));
        }
        let at = prev.content.chars().count();
        let c = self.chunks.get_mut(p)?;
        c.content.push_str(&cur.content);
        c.sep = cur.sep;
        c.born = prev.born && cur.born;
        self.chunks.remove(i);
        self.edited(p, at)
    }

    /// Supr al final del bloque `i`: se une con el siguiente.
    pub(crate) fn merge_next(&mut self, i: usize) -> Option<Focus> {
        if i + 1 >= self.chunks.len() {
            return None;
        }
        self.merge_prev(i + 1)
    }

    /// Dónde empieza (en caracteres del documento, [`Blocks::text`]) el texto de cada bloque.
    pub(crate) fn starts(&self) -> Vec<usize> {
        fn push(s: &str, n: &mut usize, tail: &mut (Option<char>, Option<char>)) {
            let mut last: Vec<char> = s.chars().rev().take(2).collect();
            last.reverse();
            for ch in last {
                *tail = (tail.1, Some(ch));
            }
            *n = n.saturating_add(s.chars().count());
        }
        let mut out = Vec::with_capacity(self.chunks.len());
        let mut n = 0usize;
        let mut tail = (None, None);
        for c in &self.chunks {
            if c.born {
                if c.content.is_empty() {
                    out.push(n);
                    continue;
                }
                if n > 0 {
                    while tail != (Some('\n'), Some('\n')) {
                        push("\n", &mut n, &mut tail);
                    }
                }
            }
            out.push(n);
            push(&c.content, &mut n, &mut tail);
            push(&c.sep, &mut n, &mut tail);
        }
        out
    }

    /// Caracteres del documento antes del bloque `uid`.
    pub(crate) fn offset_of(&self, uid: u64) -> usize {
        let starts = self.starts();
        self.index_of(uid).and_then(|i| starts.get(i).copied()).unwrap_or(0)
    }

    /// El bloque y la posición de un carácter del documento (en su texto, no en su espacio final).
    pub(crate) fn at_offset(&self, offset: usize) -> Focus {
        let starts = self.starts();
        let mut best: Option<(&Chunk, usize)> = None;
        for (c, &s) in self.chunks.iter().zip(&starts) {
            if s > offset {
                break;
            }
            if !(c.born && c.content.is_empty()) || best.is_none() {
                best = Some((c, s));
            }
        }
        match best {
            Some((c, s)) => (c.uid, (offset - s).min(c.content.chars().count())),
            None => self.chunks.first().map_or((0, 0), |c| (c.uid, 0)),
        }
    }

    fn index_of(&self, uid: u64) -> Option<usize> {
        self.chunks.iter().position(|c| c.uid == uid)
    }
}

/// Estado del editor entre cuadros.
#[derive(Clone, Default)]
struct State {
    blocks: Blocks,
    /// Huella del texto que el editor conoce (si el anfitrión lo cambia, se vuelve a partir).
    hash: u64,
    /// Cursor pendiente de poner (tras partir o unir bloques).
    focus: Option<Focus>,
    /// Deshacer y rehacer de todo el documento.
    history: crate::history::History,
    /// ¿Algún bloque tenía el foco el cuadro anterior? (Entonces Ctrl+Z es del editor.)
    active: bool,
    /// Selección que cruza bloques: (ancla, cabeza) en caracteres del documento.
    span: Option<(usize, usize)>,
    /// Cursor del bloque con el foco (bloque, posición) cuando no había botón pulsado: de ahí
    /// parte Mayús+clic en otro bloque.
    caret: Option<Focus>,
    /// Celda activa de una tabla (bloque, fila, columna): su barra se ve.
    cell: Option<(u64, usize, usize)>,
    /// Celda que toma el foco cuando se dibuje.
    cell_focus: Option<(u64, usize, usize)>,
    /// Diagramas dibujados (texturas), compartida con los hilos que dibujan.
    diagrams: crate::diagrams::Cache,
}

fn hash_str(s: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

/// Alto estimado de un bloque que aún no se dibuja.
fn estimate(content: &str, row_h: f32, width: f32, size: f32) -> f32 {
    let per_row = (width / (size * 0.55)).max(10.0) as usize;
    let rows: usize = content.split('\n').take(10_000).map(|l| l.chars().count() / per_row.max(1) + 1).sum();
    let scale = if content.starts_with('#') { 1.6 } else { 1.0 };
    rows as f32 * row_h * scale
}

/// ¿Intro al final de `content` cierra el bloque? (No dentro de una cerca de código abierta.)
fn closes(content: &str) -> bool {
    !content.trim().is_empty() && iure_doc::chunks(&format!("{content}\n\nx")).len() >= 2
}

/// Lo que pide una tecla entre bloques.
enum Act {
    Edited(usize, usize),
    Break(usize),
    MergePrev(usize),
    MergeNext(usize),
    Focus(Focus),
    /// Selección entre bloques: de (bloque, posición) a (bloque, posición); `caret` pone el
    /// cursor en la cabeza (con teclado; arrastrando no).
    Span {
        from: (usize, usize),
        to: (usize, usize),
        caret: bool,
    },
    /// Una tabla cambió desde su cuadrícula (el foco sigue en la celda).
    Table(usize),
}

/// La tecla entre bloques que toca en el bloque `i` (antes de que la vea su campo).
fn block_key(ui: &Ui, fid: Id, blocks: &Blocks, i: usize) -> Option<Act> {
    let c = blocks.chunks.get(i)?;
    let range = TextEditState::load(ui.ctx(), fid)?.cursor.char_range()?;
    let p = range.primary.index.0;
    let len = c.content.chars().count();
    let row = cached_galley(ui, fid).map(|g| {
        let r = g.layout_from_cursor(CCursor::new(p)).row;
        (r == 0, r + 1 >= g.rows.len())
    });
    let (first_row, last_row) = row.unwrap_or((true, true));
    let prev = i.checked_sub(1).and_then(|j| blocks.chunks.get(j));
    let next = blocks.chunks.get(i + 1);
    // Mayús+flecha en el borde del bloque: la selección sigue en el bloque de al lado.
    let shift_only = ui.input(|x| x.modifiers.shift && !x.modifiers.command && !x.modifiers.alt && !x.modifiers.ctrl);
    if shift_only {
        let anchor = (i, range.secondary.index.0);
        return ui.input_mut(|inp| {
            let mut take = |k: Key| inp.consume_key(egui::Modifiers::SHIFT, k);
            if next.is_some() && ((last_row && take(Key::ArrowDown)) || (p == len && take(Key::ArrowRight))) {
                return Some(Act::Span { from: anchor, to: (i + 1, 0), caret: true });
            }
            if let Some(pr) = prev
                && ((first_row && take(Key::ArrowUp)) || (p == 0 && take(Key::ArrowLeft)))
            {
                return Some(Act::Span { from: anchor, to: (i - 1, pr.content.chars().count()), caret: true });
            }
            None
        });
    }
    if range.primary.index != range.secondary.index {
        return None;
    }
    ui.input_mut(|inp| {
        if !inp.modifiers.is_none() {
            return None;
        }
        let mut take = |k: Key| inp.consume_key(egui::Modifiers::NONE, k);
        if p == len && edit::on_enter(&c.content, p).is_none() && closes(&c.content) && take(Key::Enter) {
            return Some(Act::Break(i));
        }
        if p == 0 && prev.is_some() && take(Key::Backspace) {
            return Some(Act::MergePrev(i));
        }
        if p == len && next.is_some() && take(Key::Delete) {
            return Some(Act::MergeNext(i));
        }
        if let Some(pr) = prev
            && ((first_row && take(Key::ArrowUp)) || (p == 0 && take(Key::ArrowLeft)))
        {
            return Some(Act::Focus((pr.uid, pr.content.chars().count())));
        }
        if let Some(nx) = next
            && ((last_row && take(Key::ArrowDown)) || (p == len && take(Key::ArrowRight)))
        {
            return Some(Act::Focus((nx.uid, 0)));
        }
        None
    })
}

/// La tabla de un bloque, si lo es (y `pulldown-cmark` también la lee como tabla).
fn table_of(content: &str) -> Option<Grid> {
    use pulldown_cmark::{Event, Options, Parser, Tag};
    if !content.contains('|') {
        return None;
    }
    let g = Grid::parse(content)?;
    let first = Parser::new_ext(content, Options::ENABLE_TABLES).next();
    matches!(first, Some(Event::Start(Tag::Table(_)))).then_some(g)
}

/// Lo que pide la entrada con una selección entre bloques.
enum SpanOp {
    Copy,
    Cut,
    /// Cambiar lo seleccionado por este texto (teclear, pegar, borrar).
    Put(String),
    Move(span::Move),
    /// Quitar la selección y dejar el cursor aquí.
    End(usize),
    All,
    Clear,
}

/// Toma (antes que los campos) las teclas y el portapapeles que actúan sobre la selección.
fn span_input(ui: &Ui, a: usize, head: usize) -> Option<SpanOp> {
    use egui::Event;
    use span::Move;
    ui.input_mut(|i| {
        if i.pointer.any_pressed() && !i.modifiers.shift {
            return Some(SpanOp::Clear);
        }
        let mut op = None;
        let events = std::mem::take(&mut i.events);
        let mut keep = Vec::with_capacity(events.len());
        for e in events {
            if op.is_some() {
                keep.push(e);
                continue;
            }
            let got = match &e {
                Event::Copy => Some(SpanOp::Copy),
                Event::Cut => Some(SpanOp::Cut),
                Event::Paste(t) | Event::Text(t) => Some(SpanOp::Put(t.clone())),
                Event::Key { key, pressed: true, modifiers: m, .. } => match key {
                    Key::Backspace | Key::Delete => Some(SpanOp::Put(String::new())),
                    Key::Enter if !m.command => Some(SpanOp::Put("\n".into())),
                    Key::Escape => Some(SpanOp::End(head)),
                    Key::A if m.command => Some(SpanOp::All),
                    Key::ArrowLeft if m.shift => Some(SpanOp::Move(Move::Left)),
                    Key::ArrowRight if m.shift => Some(SpanOp::Move(Move::Right)),
                    Key::ArrowUp if m.shift => Some(SpanOp::Move(Move::Up)),
                    Key::ArrowDown if m.shift => Some(SpanOp::Move(Move::Down)),
                    Key::Home if m.shift => {
                        Some(SpanOp::Move(if m.command { Move::DocStart } else { Move::LineStart }))
                    }
                    Key::End if m.shift => Some(SpanOp::Move(if m.command { Move::DocEnd } else { Move::LineEnd })),
                    Key::ArrowLeft | Key::ArrowUp | Key::Home => Some(SpanOp::End(a.min(head))),
                    Key::ArrowRight | Key::ArrowDown | Key::End => Some(SpanOp::End(a.max(head))),
                    _ => None,
                },
                _ => None,
            };
            match got {
                Some(g) => op = Some(g),
                None => keep.push(e),
            }
        }
        i.events = keep;
        op
    })
}

/// Pinta la selección sobre las posiciones `a..b` (caracteres del bloque) de un bloque dibujado
/// en `pos`; `past`: la selección sigue después del bloque (se pinta hasta el final de la fila).
fn paint_span(ui: &Ui, galley: &egui::Galley, pos: egui::Pos2, a: usize, b: usize, past: bool) {
    let color = ui.visuals().selection.bg_fill.gamma_multiply(0.6);
    let rows = galley.rows.len();
    let mut r0 = 0usize;
    for (k, row) in galley.rows.iter().enumerate() {
        let n = row.char_count_excluding_newline().0;
        let all = row.char_count_including_newline().0;
        let last = k + 1 == rows;
        if a <= r0 + n && b >= r0 {
            let (s, e) = (a.max(r0), b.min(r0 + n));
            let to_end = b > r0 + n || (past && last);
            if s < e || to_end {
                let x0 = row.x_offset(egui::text::CharIndex(s - r0));
                let x1 = if to_end { row.size.x + 6.0 } else { row.x_offset(egui::text::CharIndex(e - r0)) };
                let min = pos + row.pos.to_vec2() + egui::vec2(x0, 0.0);
                let max = pos + row.pos.to_vec2() + egui::vec2(x1, row.size.y);
                ui.painter().rect_filled(egui::Rect::from_min_max(min, max), 0.0, color);
            }
        }
        r0 += all;
    }
}

/// El editor por bloques, con su propio desplazamiento. `id` identifica su estado entre cuadros.
pub fn show(ui: &mut Ui, id: Id, text: &mut String, hint: &str) -> Output {
    show_with(ui, id, text, hint, None)
}

/// Como [`show`], con los diagramas que la app sabe dibujar en su lugar.
pub fn show_with(
    ui: &mut Ui,
    id: Id,
    text: &mut String,
    hint: &str,
    diagrams: Option<Arc<dyn DiagramRenderer>>,
) -> Output {
    let style = EditorStyle::from_ui(ui);
    let skey = style_key(&style);
    let h = hash_str(text);
    let mut st = ui.data_mut(|d| d.remove_temp::<State>(id)).unwrap_or_default();
    let now = ui.input(|i| i.time);
    let mut changed = false;
    // Deshacer y rehacer son del documento entero (no de cada bloque): se toman antes que los
    // campos.
    if st.active && st.hash == h {
        let (undo, redo) = ui.input_mut(|i| {
            let shift_cmd = egui::Modifiers { shift: true, ..egui::Modifiers::COMMAND };
            let redo = i.consume_key(shift_cmd, Key::Z) || i.consume_key(egui::Modifiers::COMMAND, Key::Y);
            let undo = !redo && i.consume_key(egui::Modifiers::COMMAND, Key::Z);
            (undo, redo)
        });
        if undo || redo {
            let mut t = text.clone();
            let cursor = if undo { st.history.undo(&mut t) } else { st.history.redo(&mut t) };
            if let Some(byte) = cursor {
                let at = t.get(..byte).map_or(0, |p| p.chars().count());
                *text = t;
                st.blocks = Blocks::new(text);
                st.focus = Some(st.blocks.at_offset(at));
                changed = true;
            }
        }
    }
    let h = hash_str(text);
    if changed {
        st.hash = h;
    }
    if st.hash != h || st.blocks.chunks.is_empty() {
        // El anfitrión cambió el texto: se vuelve a partir y el cursor sigue en el mismo punto.
        let keep = st.blocks.chunks.iter().find_map(|c| {
            let fid = id.with(("block", c.uid));
            ui.memory(|m| m.has_focus(fid)).then(|| {
                let at = TextEditState::load(ui.ctx(), fid)
                    .and_then(|s| s.cursor.char_range())
                    .map_or(0, |r| r.primary.index.0);
                st.blocks.offset_of(c.uid).saturating_add(at)
            })
        });
        st.blocks = Blocks::new(text);
        st.hash = h;
        st.focus = keep.map(|g| st.blocks.at_offset(g));
        st.history.clear();
    }
    // Selección entre bloques: Ctrl+A la crea; con ella, teclas y portapapeles son del editor.
    if !st.active {
        st.span = None;
    }
    if st.active && st.span.is_none() && ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, Key::A)) {
        let n = text.chars().count();
        st.span = (n > 0).then_some((0, n));
    }
    if let Some((a, head)) = st.span {
        let op = span_input(ui, a, head);
        let mut put = |st: &mut State, text: &mut String, with: &str| {
            let (new, cur) = span::replace(text, a, head, with);
            let before = std::mem::replace(text, new);
            st.history.record(&before, text, now);
            st.blocks = Blocks::new(text);
            st.hash = hash_str(text);
            st.focus = Some(st.blocks.at_offset(cur));
            st.span = None;
            changed = true;
        };
        match op {
            Some(SpanOp::Copy) => ui.ctx().copy_text(span::slice(text, a, head)),
            Some(SpanOp::Cut) => {
                ui.ctx().copy_text(span::slice(text, a, head));
                put(&mut st, text, "");
            }
            Some(SpanOp::Put(w)) => put(&mut st, text, &w),
            Some(SpanOp::Move(m)) => {
                let h2 = span::move_head(text, head, m);
                st.span = (h2 != a).then_some((a, h2));
                st.focus = Some(st.blocks.at_offset(h2));
            }
            Some(SpanOp::End(at)) => {
                st.span = None;
                st.focus = Some(st.blocks.at_offset(at));
            }
            Some(SpanOp::All) => {
                let n = text.chars().count();
                st.span = (n > 0).then_some((0, n));
            }
            Some(SpanOp::Clear) => st.span = None,
            None => {}
        }
        ui.ctx().request_repaint();
    }
    let starts = st.span.map(|_| st.blocks.starts());
    let shift = ui.input(|i| i.modifiers.shift);
    let pointer_busy = ui.input(|i| i.pointer.any_down() || i.pointer.any_released());
    let row_h = (style.size * 1.35).max(1.0);
    let gap = (style.size * 0.6).round();
    let mut act: Option<Act> = None;
    let mut active = false;
    let mut resp: Option<egui::Response> = None;
    let mut placed: Vec<(usize, egui::Pos2, Arc<egui::Galley>)> = Vec::new();
    // Arrastre que sale de su bloque: (bloque, ancla en él, puntero).
    let mut dragging: Option<(usize, usize, egui::Pos2)> = None;
    egui::ScrollArea::vertical().id_salt(id.with("scroll")).auto_shrink([false, false]).show_viewport(
        ui,
        |ui, viewport| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let width = ui.available_width();
            // Se dibuja una pantalla más arriba y abajo: desplazar no muestra huecos.
            let margin = viewport.height();
            let mut y = 0.0f32;
            let mut space = 0.0f32;
            let only = st.blocks.chunks.len() == 1;
            for i in 0..st.blocks.chunks.len() {
                let pending = st.focus;
                let Some(c) = st.blocks.chunks.get_mut(i) else { break };
                if (c.width - width).abs() >= 1.0 {
                    c.height = estimate(&c.content, row_h, width, style.size);
                    c.width = width;
                }
                let fid = id.with(("block", c.uid));
                let g = if c.born || c.sep.matches('\n').count() >= 2 { gap } else { 0.0 };
                let wanted = pending.and_then(|(u, at)| (u == c.uid).then_some(at));
                let focused = ui.memory(|m| m.has_focus(fid));
                active |= focused || wanted.is_some();
                let in_table = st.cell.is_some_and(|x| x.0 == c.uid) || st.cell_focus.is_some_and(|x| x.0 == c.uid);
                // Una tabla editada como Markdown vuelve a cuadrícula al dejarla.
                if c.raw && !focused && wanted.is_none() {
                    c.raw = false;
                }
                let seen = y + c.height + g >= viewport.min.y - margin && y <= viewport.max.y + margin;
                if !(seen || focused || wanted.is_some() || in_table) {
                    space += c.height + g;
                    y += c.height + g;
                    continue;
                }
                if space > 0.0 {
                    ui.add_space(space);
                    space = 0.0;
                }
                // Un diagrama que la app sabe dibujar: la imagen en su lugar (con el foco en su
                // campo se edita el código).
                let mut diagram_error: Option<String> = None;
                if let Some(r) = diagrams.as_ref()
                    && let Some(c) = st.blocks.chunks.get_mut(i)
                    && !c.raw
                    && !focused
                    && r.accepts(&c.content)
                {
                    if wanted.is_some() {
                        // Entrar con las flechas: se edita el código.
                        c.raw = true;
                    } else {
                        let uid = c.uid;
                        let resp_d = match st.diagrams.get(ui.ctx(), r, &c.content) {
                            View::Failed(e) => {
                                diagram_error = Some(e);
                                None
                            }
                            View::Pending => {
                                let h = c.height.max(48.0);
                                let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, h), egui::Sense::hover());
                                ui.painter().text(
                                    rect.left_center() + egui::vec2(4.0, 0.0),
                                    egui::Align2::LEFT_CENTER,
                                    iurefficient_ui::lang::pick("Drawing the diagram…", "Dibujando el diagrama…"),
                                    egui::FontId::proportional(style.size * 0.9),
                                    style.muted,
                                );
                                Some(resp)
                            }
                            View::Image(tex, size) => {
                                let w = size.x.min(width).max(1.0);
                                let h = size.y * (w / size.x.max(1.0));
                                let resp = ui
                                    .add(egui::Image::new((tex.id(), egui::vec2(w, h))).sense(egui::Sense::click()))
                                    .on_hover_text(iurefficient_ui::lang::pick(
                                        "Click to edit the diagram",
                                        "Clic para editar el diagrama",
                                    ))
                                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                                let pos = i + 1;
                                resp.widget_info(|| {
                                    let label = format!("{} {pos}", iurefficient_ui::lang::pick("Diagram", "Diagrama"));
                                    egui::WidgetInfo::labeled(egui::Role::Button, true, label)
                                });
                                if resp.clicked() {
                                    if let Some(c) = st.blocks.chunks.get_mut(i) {
                                        c.raw = true;
                                    }
                                    st.focus = Some((uid, 0));
                                }
                                Some(resp)
                            }
                        };
                        if let Some(rd) = resp_d {
                            let dh = rd.rect.height();
                            if let Some(c) = st.blocks.chunks.get_mut(i) {
                                c.height = dh;
                            }
                            if let (Some((a, b)), Some(starts)) = (st.span, starts.as_ref()) {
                                let s0 = starts.get(i).copied().unwrap_or(0);
                                if a.max(b) > s0 && a.min(b) <= s0 {
                                    ui.painter().rect_filled(
                                        rd.rect,
                                        0.0,
                                        ui.visuals().selection.bg_fill.gamma_multiply(0.4),
                                    );
                                }
                            }
                            if g > 0.0 {
                                ui.add_space(g);
                            }
                            y += dh + g;
                            if resp.is_none() {
                                resp = Some(rd);
                            }
                            continue;
                        }
                    }
                }
                // Una tabla: cuadrícula de celdas.
                if let Some(grid) = st.blocks.chunks.get(i).filter(|c| !c.raw).and_then(|c| table_of(&c.content)) {
                    let uid = st.blocks.chunks.get(i).map_or(0, |c| c.uid);
                    let mut focus_to = st.cell_focus.and_then(|(u, r, cc)| (u == uid).then_some((r, cc)));
                    if let Some(at) = wanted {
                        focus_to = Some(if at == 0 { (0, 0) } else { (grid.rows.len().saturating_sub(1), 0) });
                        st.focus = None;
                    }
                    if focus_to.is_some() {
                        st.cell_focus = None;
                    }
                    let bar = st.cell.and_then(|(u, r, cc)| (u == uid).then_some((r, cc))).or(focus_to);
                    let out = table_ui::show(ui, fid, &grid, &style, focus_to, bar);
                    if let Some((r, cc)) = out.focused.or(focus_to) {
                        st.cell = Some((uid, r, cc));
                        active = true;
                    }
                    if let Some((r, cc)) = out.focus_next {
                        st.cell_focus = Some((uid, r, cc));
                        st.cell = Some((uid, r, cc));
                        active = true;
                    }
                    match out.act {
                        Some(TableAct::Changed(md)) => {
                            if let Some(c) = st.blocks.chunks.get_mut(i) {
                                c.content = md;
                            }
                            if act.is_none() {
                                act = Some(Act::Table(i));
                            }
                        }
                        Some(TableAct::Leave(down)) => {
                            st.cell = None;
                            let to = if down {
                                st.blocks.chunks.get(i + 1).map(|n| (n.uid, 0))
                            } else {
                                i.checked_sub(1)
                                    .and_then(|j| st.blocks.chunks.get(j))
                                    .map(|p| (p.uid, p.content.chars().count()))
                            };
                            if let Some(f) = to
                                && act.is_none()
                            {
                                act = Some(Act::Focus(f));
                            }
                        }
                        Some(TableAct::Raw) => {
                            if let Some(c) = st.blocks.chunks.get_mut(i) {
                                c.raw = true;
                            }
                            st.cell = None;
                            st.focus = Some((uid, 0));
                        }
                        None => {}
                    }
                    let th = out.rect.height();
                    if let Some(c) = st.blocks.chunks.get_mut(i) {
                        c.height = th;
                    }
                    // Dentro de una selección entre bloques: la tabla entera se ve seleccionada.
                    if let (Some((a, b)), Some(starts)) = (st.span, starts.as_ref()) {
                        let s0 = starts.get(i).copied().unwrap_or(0);
                        if a.max(b) > s0 && a.min(b) <= s0 {
                            ui.painter().rect_filled(out.rect, 0.0, ui.visuals().selection.bg_fill.gamma_multiply(0.4));
                        }
                    }
                    if g > 0.0 {
                        ui.add_space(g);
                    }
                    y += th + g;
                    if resp.is_none() {
                        resp = Some(ui.interact(out.rect, fid.with("table"), egui::Sense::hover()));
                    }
                    continue;
                }
                if focused {
                    st.cell = None;
                }
                if let Some(at) = wanted {
                    let mut s = TextEditState::load(ui.ctx(), fid).unwrap_or_default();
                    s.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(at))));
                    s.store(ui.ctx(), fid);
                    ui.memory_mut(|m| m.request_focus(fid));
                }
                if focused && act.is_none() {
                    act = block_key(ui, fid, &st.blocks, i);
                }
                let Some(c) = st.blocks.chunks.get_mut(i) else { break };
                let hint = if only && c.content.is_empty() { hint } else { "" };
                let (changed, out) = field(ui, fid, &mut c.content, hint, 1, &style, skey);
                c.height = out.response.response.rect.height();
                let mut ch = c.height;
                // Un diagrama con error: el código y, debajo, qué falla.
                if let Some(e) = &diagram_error {
                    ch += ui.colored_label(ui.visuals().error_fg_color, e.as_str()).rect.height();
                    c.height = ch;
                }
                // Cada bloque con nombre para lectores de pantalla (el anfitrión nombra el que devuelve).
                let pos = i + 1;
                out.response.response.widget_info(|| {
                    let label = format!("{} {pos}", iurefficient_ui::lang::pick("Block", "Bloque"));
                    egui::WidgetInfo::labeled(egui::Role::MultilineTextInput, true, label)
                });
                if changed && act.is_none() {
                    let at = out.state.cursor.char_range().map_or(0, |r| r.primary.index.0);
                    act = Some(Act::Edited(i, at));
                }
                let (uid, len) = (c.uid, c.content.chars().count());
                // La parte de la selección entre bloques que cae en este bloque.
                if let (Some((a, b)), Some(starts)) = (st.span, starts.as_ref()) {
                    let s0 = starts.get(i).copied().unwrap_or(0);
                    let (lo, hi) = (a.min(b), a.max(b));
                    if hi > s0 && lo <= s0 + len {
                        paint_span(
                            ui,
                            &out.galley,
                            out.galley_pos,
                            lo.saturating_sub(s0),
                            (hi - s0).min(len),
                            hi > s0 + len,
                        );
                    }
                }
                let r = &out.response.response;
                let primary = out.state.cursor.char_range().map_or(0, |x| x.primary.index.0);
                // Mayús+clic en otro bloque: selección desde el cursor que había.
                if r.clicked()
                    && shift
                    && act.is_none()
                    && let Some((cu, cl)) = st.caret
                    && cu != uid
                    && let Some(fi) = st.blocks.index_of(cu)
                {
                    act = Some(Act::Span { from: (fi, cl), to: (i, primary), caret: false });
                }
                if r.dragged()
                    && let Some(pp) = ui.input(|x| x.pointer.interact_pos())
                    && !r.rect.contains(pp)
                {
                    let anchor = out.state.cursor.char_range().map_or(0, |x| x.secondary.index.0);
                    dragging = Some((i, anchor, pp));
                }
                if focused && !pointer_busy {
                    st.caret = Some((uid, primary));
                }
                placed.push((i, out.galley_pos, Arc::clone(&out.galley)));
                if wanted.is_some() {
                    out.response.response.scroll_to_me(None);
                    st.focus = None;
                }
                if g > 0.0 {
                    ui.add_space(g);
                }
                y += ch + g;
                if focused || resp.is_none() {
                    resp = Some(out.response.response);
                }
            }
            if space > 0.0 {
                ui.add_space(space);
            }
            // Arrastrar fuera del bloque: la selección sigue en el bloque bajo el puntero.
            if let Some((di, anchor, pp)) = dragging
                && act.is_none()
                && let Some((j, gp, g)) = placed.iter().rev().find(|(_, gp, _)| gp.y <= pp.y).or(placed.first())
            {
                if *j == di {
                    st.span = None;
                } else {
                    let local = g.cursor_from_pos(pp - *gp).index.0;
                    act = Some(Act::Span { from: (di, anchor), to: (*j, local), caret: false });
                }
            }
        },
    );
    st.active = active;
    if !active {
        st.cell = None;
    }
    if let Some(a) = act {
        let (focus, edits) = match a {
            Act::Edited(i, at) => (st.blocks.edited(i, at), true),
            Act::Break(i) => (st.blocks.break_after(i), true),
            Act::MergePrev(i) => (st.blocks.merge_prev(i), true),
            Act::MergeNext(i) => (st.blocks.merge_next(i), true),
            Act::Focus(f) => (Some(f), false),
            Act::Table(i) => {
                // Se vuelve a partir por si acaso, pero el foco sigue en la celda.
                let _ = st.blocks.edited(i, 0);
                (None, true)
            }
            Act::Span { from, to, caret } => {
                let starts = st.blocks.starts();
                let at = |(i, l): (usize, usize)| starts.get(i).copied().unwrap_or(0).saturating_add(l);
                let (a, h) = (at(from), at(to));
                st.span = (a != h).then_some((a, h));
                (if caret { st.blocks.chunks.get(to.0).map(|c| (c.uid, to.1)) } else { None }, false)
            }
        };
        // Un cursor en un bloque que ya no existe no se pone.
        st.focus = focus.filter(|(u, _)| st.blocks.index_of(*u).is_some());
        if edits {
            let new = st.blocks.text();
            if new != *text {
                changed = true;
                let before = std::mem::replace(text, new);
                st.history.record(&before, text, now);
            }
            st.hash = hash_str(text);
        }
        ui.ctx().request_repaint();
    }
    ui.data_mut(|d| d.insert_temp(id, st));
    let response = resp.unwrap_or_else(|| ui.allocate_response(egui::Vec2::ZERO, egui::Sense::hover()));
    Output { changed, response }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn contents(b: &Blocks) -> Vec<&str> {
        b.chunks.iter().map(|c| c.content.as_str()).collect()
    }

    #[test]
    fn splits_into_blocks_and_joins_back() {
        let t = "# Título\n\nUno\ndos  \n\n- a\n- b\n\n```\nx\n\ny\n```\n";
        let b = Blocks::new(t);
        assert_eq!(contents(&b), ["# Título", "Uno\ndos  ", "- a\n- b", "```\nx\n\ny\n```"]);
        assert_eq!(b.text(), t);
        assert_eq!(Blocks::new("").text(), "");
        assert_eq!(Blocks::new("a\r\n\r\nb").text(), "a\r\n\r\nb");
    }

    #[test]
    fn two_enters_split_a_block() {
        let mut b = Blocks::new("uno dos\n\ntres\n");
        // «uno» Intro Intro «dos»: el cursor queda al inicio de «dos».
        if let Some(c) = b.chunks.get_mut(0) {
            c.content = "uno\n\ndos".into();
        }
        let f = b.edited(0, 5);
        assert_eq!(contents(&b), ["uno", "dos", "tres"]);
        assert_eq!(f, b.chunks.get(1).map(|c| (c.uid, 0)));
        assert_eq!(b.text(), "uno\n\ndos\n\ntres\n");
    }

    #[test]
    fn enter_at_the_end_opens_a_block_that_writes_nothing_while_empty() {
        let mut b = Blocks::new("uno\n\ntres");
        let f = b.break_after(0);
        assert_eq!(b.text(), "uno\n\ntres", "vacío no cambia el documento");
        let uid = f.map(|f| f.0);
        if let Some(c) = b.chunks.get_mut(1) {
            c.content = "dos".into();
        }
        b.edited(1, 3);
        assert_eq!(b.text(), "uno\n\ndos\n\ntres");
        // Al final de un documento sin salto final.
        let mut b = Blocks::new("uno");
        b.break_after(0);
        if let Some(c) = b.chunks.get_mut(1) {
            c.content = "dos".into();
        }
        assert_eq!(b.text(), "uno\n\ndos\n\n");
        assert!(uid.is_some());
    }

    #[test]
    fn backspace_and_delete_join_blocks() {
        let mut b = Blocks::new("uno\n\ndos\n\ntres\n");
        let f = b.merge_prev(1);
        assert_eq!(contents(&b), ["unodos", "tres"]);
        assert_eq!(f, b.chunks.first().map(|c| (c.uid, 3)));
        let f = b.merge_next(0);
        assert_eq!(contents(&b), ["unodostres"]);
        assert_eq!(f, b.chunks.first().map(|c| (c.uid, 6)));
        assert_eq!(b.text(), "unodostres\n");
        assert_eq!(b.merge_prev(0), None);
        assert_eq!(b.merge_next(0), None);
        // Un bloque nuevo y vacío solo se quita.
        let mut b = Blocks::new("uno\n");
        b.break_after(0);
        let f = b.merge_prev(1);
        assert_eq!(contents(&b), ["uno"]);
        assert_eq!(f, b.chunks.first().map(|c| (c.uid, 3)));
    }

    #[test]
    fn offsets_find_the_same_point_after_reparsing() {
        let b = Blocks::new("uno\n\ndós\n\ntres");
        let dos = b.chunks.get(1).map_or(0, |c| c.uid);
        assert_eq!(b.offset_of(dos), 5);
        let again = Blocks::new("uno\n\ndós\n\ntres");
        assert_eq!(again.at_offset(5 + 2), again.chunks.get(1).map_or((0, 0), |c| (c.uid, 2)));
        // En el espacio entre bloques: al final del texto del anterior.
        assert_eq!(again.at_offset(4), again.chunks.first().map_or((0, 0), |c| (c.uid, 3)));
        // Más allá del final: al final del último.
        assert_eq!(again.at_offset(999), again.chunks.get(2).map_or((0, 0), |c| (c.uid, 4)));
    }

    #[test]
    fn starts_match_the_written_document() {
        let mut b = Blocks::new("uno");
        b.break_after(0);
        if let Some(c) = b.chunks.get_mut(1) {
            c.content = "dos".into();
        }
        b.break_after(1);
        let t = b.text();
        let chars: Vec<char> = t.chars().collect();
        for (c, s) in b.chunks.iter().zip(b.starts()) {
            let got: String = chars.iter().skip(s).take(c.content.chars().count()).collect();
            assert_eq!(got, c.content, "{t:?}");
        }
    }

    #[test]
    fn enter_closes_paragraphs_but_not_open_fences() {
        assert!(closes("párrafo"));
        assert!(closes("# Título"));
        assert!(!closes("```rust\nfn f() {}"));
        assert!(!closes(""));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]
        /// Partir en bloques y juntar nunca cambia el documento.
        #[test]
        fn blocks_are_lossless(md in "(\\PC|#|> |- |```|\\| |---|\n|  |\r\n){0,200}") {
            prop_assert_eq!(Blocks::new(&md).text(), md);
        }

        /// Editar un bloque (y volver a partirlo) solo cambia ese trozo del documento.
        #[test]
        fn editing_one_block_keeps_the_rest(md in "(\\PC|#|- |```|\n){0,120}", ins in "(\\PC|\n){0,8}") {
            let mut b = Blocks::new(&md);
            let n = b.chunks.len();
            let i = n / 2;
            let before: String = b.chunks.iter().take(i).map(|c| format!("{}{}", c.content, c.sep)).collect();
            let after: String = b.chunks.iter().skip(i + 1).map(|c| format!("{}{}", c.content, c.sep)).collect();
            let mut cursor = 0;
            if let Some(c) = b.chunks.get_mut(i) {
                c.content.push_str(&ins);
                cursor = c.content.chars().count();
            }
            b.edited(i, cursor);
            let t = b.text();
            prop_assert!(t.starts_with(&before));
            prop_assert!(t.ends_with(&after));
        }
    }
}

#[cfg(test)]
mod perf {
    use super::*;

    /// Unas 100 páginas por bloques: teclear rehace un bloque, no el documento.
    #[test]
    fn hundred_pages_by_blocks() {
        let page = "## Sección\n\nTexto con **negrita**, *cursiva*, `código` y [enlace](https://x.mx).\n\n- uno\n- [ ] dos\n  - tres\n\n> cita\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n```rust\nfn f() {}\n```\n\n";
        let mut text = page.repeat(100 * 15);
        let ctx = egui::Context::default();
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0))),
            ..Default::default()
        };
        let id = Id::unique("perf-blocks");
        let frame = |text: &mut String| {
            let t = std::time::Instant::now();
            let mut out = ctx.run_ui(input(), |ui| {
                show(ui, id, text, "");
            });
            out.textures_delta.clear();
            t.elapsed().as_secs_f64() * 1000.0
        };
        let first = frame(&mut text);
        let steady = frame(&mut text);
        // Una tecla: el bloque 10 cambia (como lo haría su campo) y se rehace.
        let t = std::time::Instant::now();
        let mut st = ctx.data_mut(|d| d.remove_temp::<State>(id)).unwrap_or_default();
        if let Some(c) = st.blocks.chunks.get_mut(10) {
            c.content.push('x');
        }
        st.blocks.edited(10, 1);
        text = st.blocks.text();
        st.hash = hash_str(&text);
        ctx.data_mut(|d| d.insert_temp(id, st));
        let op = t.elapsed().as_secs_f64() * 1000.0;
        let typed = frame(&mut text);
        eprintln!(
            "por bloques, {} KB: primer cuadro {first:.1} ms, estable {steady:.1} ms, tecla {op:.1} ms + cuadro {typed:.1} ms",
            text.len() / 1024
        );
        assert!(steady < 1_000.0 && typed < 1_000.0);
    }
}
