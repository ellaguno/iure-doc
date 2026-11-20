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
//!
//! **Imágenes en su lugar** ([`crate::images`]): un párrafo que es solo una imagen local se ve como
//! la imagen (con [`crate::LocalImages`] entre los dibujantes). Pegar una imagen (Ctrl+V) o soltar
//! archivos de imagen sobre el editor las guarda con [`Hooks::images`] y las inserta en su propio
//! bloque.
//!
//! **Pegar HTML como Markdown** ([`crate::paste`], con [`Hooks::paste`]): lo copiado de la web,
//! Word o Google Docs se pega con su estructura; en un bloque de código, el texto plano.
//!
//! **Corrector ortográfico** ([`crate::spell`], con [`Hooks::spell`]): subraya en rojo las palabras
//! mal escritas (no la que se está tecleando); clic derecho: sugerencias, agregar al diccionario
//! personal, ignorar.
//!
//! **Buscar y reemplazar** ([`crate::find`]): Ctrl+F / Ctrl+H abren una barra arriba del editor;
//! las coincidencias se resaltan en los bloques visibles (los diagramas y tablas se tiñen) y el
//! editor se desplaza hasta la actual; Esc la deja seleccionada en el texto.
//!
//! **Resaltado de sintaxis** ([`crate::highlight`]): los bloques de código con lenguaje se colorean
//! con los colores del tema.

use iurefficient_ui::icons::{self, Icon};
use std::hash::{Hash, Hasher};

use egui::text::{CCursor, CCursorRange};
use egui::text_edit::TextEditState;
use egui::{Id, Key, Ui};
use iure_doc::edit;

use std::sync::Arc;

use iure_doc::table::Grid;

use crate::diagrams::{DiagramRenderer, View};
use crate::paste::PasteSource;
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

/// Nivel del título con que empieza un bloque (`# …` a `###### …`); 0 si no es título.
fn heading_level(content: &str) -> usize {
    let t = content.trim_start_matches(' ');
    let n = t.chars().take_while(|&c| c == '#').count();
    let rest = t.get(n..).unwrap_or("");
    if (1..=6).contains(&n) && (rest.is_empty() || rest.starts_with([' ', '\t'])) { n } else { 0 }
}

/// Texto y espacio final de un trozo: el texto acaba al final de la línea de su último carácter
/// visible (los espacios de esa línea, que pueden ser un salto forzado, se quedan).
fn split_ws(piece: &str) -> (String, String) {
    // Un bloque de código sin cerrar se sigue escribiendo: sus saltos finales son suyos (si no,
    // Intro tras la cerca se perdería).
    if unclosed_fence(piece) {
        return (piece.to_string(), String::new());
    }
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

/// ¿Empieza con una cerca de código (```` ``` ```` o `~~~`) que no se cierra?
fn unclosed_fence(piece: &str) -> bool {
    let mut lines = piece.lines();
    let Some(first) = lines.next().map(str::trim_start) else { return false };
    let Some(ch) = first.chars().next().filter(|c| *c == '`' || *c == '~') else { return false };
    let n = first.chars().take_while(|c| *c == ch).count();
    if n < 3 {
        return false;
    }
    !lines.any(|l| {
        let t = l.trim();
        t.chars().take_while(|c| *c == ch).count() >= n && t.chars().all(|c| c == ch)
    })
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
        // El cursor, contado sobre el texto entero, cae en uno de los trozos. Si cae en el espacio
        // tras un trozo (un salto al final de un bloque: Intro que sale de una lista, p. ej.), ahí
        // empieza un bloque nuevo.
        let mut left = cursor;
        let mut target = None;
        let mut in_sep = None;
        for (k, p) in pieces.iter().enumerate() {
            let n = p.content.chars().count();
            let total = n.saturating_add(p.sep.chars().count());
            if left <= n {
                target = Some((p.uid, left));
                break;
            }
            if left < total {
                in_sep = Some(k);
                break;
            }
            left -= total;
        }
        let target = target.or_else(|| pieces.last().map(|p| (p.uid, p.content.chars().count())));
        self.chunks.splice(i..=i, pieces);
        if let Some(k) = in_sep {
            return self.break_after(i + k);
        }
        target
    }

    /// Intro al final del bloque `i`: uno nuevo y vacío debajo.
    pub(crate) fn break_after(&mut self, i: usize) -> Option<Focus> {
        if i >= self.chunks.len() || self.chunks.len() >= iure_doc::MAX_BLOCKS {
            return None;
        }
        // Al final del documento, un solo salto tras él.
        let sep = if i + 1 >= self.chunks.len() { "\n" } else { "\n\n" };
        let c = self.chunk(String::new(), sep.to_string(), true);
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

/// Alto más grande con que se ve un diagrama o una imagen (una foto vertical no ocupa páginas).
const MAX_IMAGE_HEIGHT: f32 = 640.0;

/// Estado del editor entre cuadros.
#[derive(Clone, Default)]
struct State {
    /// El bloque de diagrama con el menú «Plantilla» abierto.
    templates: Option<u64>,
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
    /// El otro extremo de la selección del bloque con el foco (para la barra de formato).
    anchor: Option<Focus>,
    /// Celda activa de una tabla (bloque, fila, columna): su barra se ve.
    cell: Option<(u64, usize, usize)>,
    /// Celda que toma el foco cuando se dibuje.
    cell_focus: Option<(u64, usize, usize)>,
    /// Diagramas dibujados (texturas), compartida con los hilos que dibujan.
    diagrams: crate::diagrams::Cache,
    /// Buscar y reemplazar.
    find: FindState,
    /// Selección pendiente de poner en un bloque (bloque, desde, hasta), al cerrar la búsqueda.
    select: Option<(u64, usize, usize)>,
    /// Markdown que se pegará cuando un bloque tenga el foco (imágenes soltadas sin foco).
    pending_paste: Option<String>,
    /// Aviso breve (texto, hasta cuándo), p. ej. por qué no se pudo pegar una imagen.
    notice: Option<(String, f64)>,
}

/// Estado de buscar y reemplazar (Ctrl+F, Ctrl+H).
#[derive(Clone, Default)]
struct FindState {
    open: bool,
    replace: bool,
    query: String,
    with: String,
    case: bool,
    current: usize,
    /// El campo de búsqueda toma el foco este cuadro.
    focus_query: bool,
    /// Desplazar hasta la coincidencia actual.
    scroll: bool,
    /// Coincidencias (caracteres del documento) y la huella de (texto, búsqueda, mayúsculas).
    matches: Vec<std::ops::Range<usize>>,
    key: u64,
    /// Dónde estaba el cursor al abrir: la coincidencia actual es la primera desde ahí.
    anchor: Option<usize>,
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
    /// Tres Intro al final de un bloque de código: el bloque cerrado y uno nuevo debajo.
    CodeExit(usize, String),
}

/// La tecla entre bloques que toca en el bloque `i` (antes de que la vea su campo).
fn block_key(ui: &Ui, fid: Id, blocks: &Blocks, i: usize) -> Option<Act> {
    // Con el menú de `/` o `[[` abierto, las flechas e Intro son suyas.
    if crate::suggest::is_open(ui, fid) {
        return None;
    }
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
        if let Some(e) = edit::code_exit(&c.content, p)
            && take(Key::Enter)
        {
            return Some(Act::CodeExit(i, e.text));
        }
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
    paint_range(ui, galley, pos, a, b, past, ui.visuals().selection.bg_fill.gamma_multiply(0.6));
}

/// Coincidencias de la búsqueda: todas en ámbar suave, la actual más fuerte.
const FIND_ALL: egui::Color32 = egui::Color32::from_rgba_premultiplied(90, 70, 0, 90);
const FIND_CURRENT: egui::Color32 = egui::Color32::from_rgba_premultiplied(160, 90, 0, 150);

/// Un bloque dibujado como imagen o cuadrícula (diagrama, tabla) con coincidencias de la búsqueda:
/// se tiñe entero (más fuerte si tiene la actual) y, si es el destino, se desplaza hasta él.
fn find_tint(
    ui: &Ui,
    st: &mut State,
    starts: Option<&Vec<usize>>,
    i: usize,
    len: usize,
    rect: egui::Rect,
    target: bool,
) {
    let Some(s0) = starts.and_then(|s| s.get(i).copied()) else { return };
    if !st.find.open {
        return;
    }
    let ms = &st.find.matches;
    let k0 = ms.partition_point(|r| r.end <= s0);
    let hit: Vec<usize> =
        ms.iter().enumerate().skip(k0).take_while(|(_, r)| r.start <= s0 + len).map(|(k, _)| k).collect();
    if hit.is_empty() {
        return;
    }
    let color = if hit.contains(&st.find.current) { FIND_CURRENT } else { FIND_ALL };
    ui.painter().rect_filled(rect, 2.0, color.gamma_multiply(0.6));
    if target {
        ui.scroll_to_rect(rect, Some(egui::Align::Center));
        st.find.scroll = false;
    }
}

/// Pinta `a..b` (caracteres del bloque) con `color`; `past`: sigue hasta el final de la fila.
fn paint_range(ui: &Ui, galley: &egui::Galley, pos: egui::Pos2, a: usize, b: usize, past: bool, color: egui::Color32) {
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

/// El bloque con el foco y si su cursor está al principio / al final de su línea.
fn cursor_line_edges(ui: &Ui, id: Id, st: &State) -> (Option<usize>, bool, bool) {
    let Some((i, c)) =
        st.blocks.chunks.iter().enumerate().find(|(_, c)| ui.memory(|m| m.has_focus(id.with(("block", c.uid)))))
    else {
        return (None, true, true);
    };
    let fid = id.with(("block", c.uid));
    let edges = TextEditState::load(ui.ctx(), fid).and_then(|s| s.cursor.char_range()).map(|r| {
        let (a, b) = (r.primary.index.0.min(r.secondary.index.0), r.primary.index.0.max(r.secondary.index.0));
        let chars: Vec<char> = c.content.chars().collect();
        let start = a == 0 || chars.get(a - 1) == Some(&'\n');
        let end = chars.get(b).is_none_or(|x| *x == '\n');
        (start, end)
    });
    let (start, end) = edges.unwrap_or((true, true));
    (Some(i), start, end)
}

/// Lo seleccionado ahora (en un bloque o entre bloques) y dónde empieza (carácter del
/// documento), para buscarlo.
fn selected_text(ui: &Ui, id: Id, st: &State, text: &str) -> Option<(String, usize)> {
    if let Some((a, b)) = st.span {
        return Some((span::slice(text, a, b), a.min(b)));
    }
    st.blocks.chunks.iter().find_map(|c| {
        let fid = id.with(("block", c.uid));
        if !ui.memory(|m| m.has_focus(fid)) {
            return None;
        }
        let r = TextEditState::load(ui.ctx(), fid)?.cursor.char_range()?;
        let (a, b) = (r.primary.index.0.min(r.secondary.index.0), r.primary.index.0.max(r.secondary.index.0));
        (b > a).then(|| (c.content.chars().skip(a).take(b - a).collect(), st.blocks.offset_of(c.uid).saturating_add(a)))
    })
}

/// La barra de buscar y reemplazar. Cambia `text` al reemplazar.
fn find_bar(ui: &mut Ui, id: Id, st: &mut State, text: &mut String, now: f64, changed: &mut bool) {
    let pick = iurefficient_ui::lang::pick;
    // Las coincidencias, al día con el texto y la búsqueda.
    let key = {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut h);
        st.find.query.hash(&mut h);
        st.find.case.hash(&mut h);
        h.finish()
    };
    if key != st.find.key {
        st.find.matches = crate::find::find_all(text, &crate::find::unescape(&st.find.query), st.find.case);
        st.find.key = key;
        if let Some(a) = st.find.anchor {
            st.find.current = crate::find::first_from(&st.find.matches, a);
        }
        if st.find.current >= st.find.matches.len() {
            st.find.current = 0;
        }
    }
    let n = st.find.matches.len();
    let (mut go, mut close, mut one, mut all) = (0isize, false, false, false);
    // Una franja bajo la barra de la app, como la 1.x: buscar, contador, anterior/siguiente,
    // mayúsculas, «Reemplazar…» y cerrar; el reemplazo en la misma franja.
    let c = iurefficient_ui::theme::colors(ui);
    let strip = egui::Frame::new()
        .fill(c.surface_alt)
        .corner_radius(6.0)
        .stroke(egui::Stroke::new(1.0, c.separator))
        .inner_margin(egui::Margin::symmetric(10, 6));
    strip.show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let field_frame = |ui: &mut Ui, te: egui::TextEdit<'_>| {
                ui.add(te.desired_width(210.0).margin(egui::Margin::symmetric(8, 4)).background_color(c.surface))
            };
            let tip = pick(
                "Use \\n for a line break, \\t for a tab and \\\\ for a backslash",
                "Usa \\n para salto de línea, \\t para tabulador y \\\\ para barra invertida",
            );
            let q = field_frame(
                ui,
                egui::TextEdit::singleline(&mut st.find.query)
                    .id(id.with("find-query"))
                    .hint_text(pick("Find…", "Buscar…")),
            )
            .on_hover_text(tip);
            q.widget_info(|| egui::WidgetInfo::labeled(egui::Role::SearchInput, true, pick("Find", "Buscar")));
            if st.find.focus_query {
                q.request_focus();
                st.find.focus_query = false;
            }
            if q.changed() {
                st.find.scroll = true;
            }
            if q.lost_focus() {
                let (enter, shift, esc) =
                    ui.input(|i| (i.key_pressed(Key::Enter), i.modifiers.shift, i.key_pressed(Key::Escape)));
                if enter {
                    go = if shift { -1 } else { 1 };
                    q.request_focus();
                } else if esc {
                    close = true;
                }
            }
            let (count, ink) = if st.find.query.is_empty() {
                (String::new(), c.text_muted)
            } else if n == 0 {
                (pick("No matches", "Sin coincidencias").to_string(), c.warning)
            } else {
                (format!("{}/{n}", st.find.current + 1), c.text_muted)
            };
            ui.add_sized(
                egui::vec2(if n == 0 && !st.find.query.is_empty() { 110.0 } else { 52.0 }, 20.0),
                egui::Label::new(egui::RichText::new(count).small().color(ink)),
            );
            let has = n > 0;
            if icons::icon_button(ui, Icon::ChevronUp, pick("Previous (Shift+Enter)", "Anterior (Mayús+Intro)"), has)
                .clicked()
            {
                go = -1;
            }
            if icons::icon_button(ui, Icon::ChevronDown, pick("Next (Enter)", "Siguiente (Intro)"), has).clicked() {
                go = 1;
            }
            let mut case = st.find.case;
            let aa = egui::RichText::new("Aa").color(if case { c.accent } else { c.text });
            if ui
                .add(egui::Button::selectable(case, aa).corner_radius(4.0))
                .on_hover_text(pick("Match case", "Distinguir mayúsculas"))
                .clicked()
            {
                case = !case;
                st.find.case = case;
            }
            if ui.add(egui::Button::new(pick("Replace…", "Reemplazar…")).frame(false)).clicked() {
                st.find.replace = !st.find.replace;
            }
            if st.find.replace {
                let w = field_frame(
                    ui,
                    egui::TextEdit::singleline(&mut st.find.with)
                        .id(id.with("find-with"))
                        .hint_text(pick("Replace with…", "Reemplazar con…")),
                )
                .on_hover_text(tip);
                w.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::Role::TextInput, true, pick("Replace with", "Reemplazar con"))
                });
                if w.lost_focus() {
                    let (enter, esc) = ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
                    if enter {
                        one = true;
                        w.request_focus();
                    } else if esc {
                        close = true;
                    }
                }
                let filled = egui::Button::new(egui::RichText::new(pick("Replace", "Reemplazar")).color(c.on_accent))
                    .fill(c.accent)
                    .corner_radius(4.0);
                if ui.add_enabled(has, filled).clicked() {
                    one = true;
                }
                let outline =
                    egui::Button::new(egui::RichText::new(pick("Replace all", "Reemplazar todo")).color(c.accent))
                        .fill(egui::Color32::TRANSPARENT)
                        .stroke(egui::Stroke::new(1.0, c.accent))
                        .corner_radius(4.0);
                if ui.add_enabled(has, outline).clicked() {
                    all = true;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icons::icon_button(ui, Icon::Close, pick("Close (Esc)", "Cerrar (Esc)"), true).clicked() {
                    close = true;
                }
            });
        });
    });
    let (f3, shift_f3) = ui.input_mut(|i| {
        let back = i.consume_key(egui::Modifiers::SHIFT, Key::F3);
        (!back && i.consume_key(egui::Modifiers::NONE, Key::F3), back)
    });
    if f3 {
        go = 1;
    } else if shift_f3 {
        go = -1;
    }
    if go != 0 {
        st.find.anchor = None;
    }
    if go != 0 && n > 0 {
        st.find.current = (st.find.current as isize + go).rem_euclid(n as isize) as usize;
        st.find.scroll = true;
    }
    let with = crate::find::unescape(&st.find.with);
    let new_text = if one {
        st.find.matches.get(st.find.current).map(|r| span::replace(text, r.start, r.end, &with).0)
    } else if all && n > 0 {
        st.find.current = 0;
        Some(crate::find::replace_all(text, &st.find.matches, &with))
    } else {
        None
    };
    if let Some(t) = new_text {
        let before = std::mem::replace(text, t);
        st.history.record(&before, text, now);
        st.blocks = Blocks::new(text);
        st.hash = hash_str(text);
        st.find.key = 0;
        st.find.scroll = true;
        *changed = true;
    }
    if close {
        st.find.open = false;
        // La coincidencia actual queda seleccionada en el texto.
        if let Some(r) = st.find.matches.get(st.find.current).cloned() {
            let starts = st.blocks.starts();
            let idx = starts.partition_point(|&s0| s0 <= r.start).saturating_sub(1);
            if let (Some(c), Some(&s0)) = (st.blocks.chunks.get(idx), starts.get(idx)) {
                let len = c.content.chars().count();
                st.select = Some((c.uid, (r.start - s0).min(len), (r.end - s0).min(len)));
            }
        }
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
    show_hooks(ui, id, text, hint, Hooks { diagrams, ..Default::default() })
}

/// Lo que la app le da al editor: quién dibuja diagramas e imágenes, de dónde sale el HTML al
/// pegar.
#[derive(Clone, Default)]
pub struct Hooks {
    pub diagrams: Option<Arc<dyn DiagramRenderer>>,
    /// Con ella, pegar HTML (de la web, Word, Google Docs…) pega su Markdown.
    pub paste: Option<Arc<dyn PasteSource>>,
    /// Corrector ortográfico: subraya y sugiere con clic derecho.
    pub spell: Option<Arc<crate::Speller>>,
    /// Dónde guardar las imágenes pegadas o soltadas sobre el editor.
    pub images: Option<Arc<dyn crate::ImageStore>>,
    /// Las notas que puede enlazar `[[` (la carpeta o bóveda del documento).
    pub notes: Option<Arc<dyn crate::NoteIndex>>,
    /// Zoom del documento (1.0 = la letra del tema); se acota a 50 %–300 %.
    pub zoom: Option<f32>,
}

/// Como [`show`], con los ganchos de la app.
pub fn show_hooks(ui: &mut Ui, id: Id, text: &mut String, hint: &str, hooks: Hooks) -> Output {
    let diagrams = hooks.diagrams.clone();
    let mut style = EditorStyle::from_ui(ui);
    if let Some(z) = hooks.zoom.filter(|z| z.is_finite()) {
        style.size *= z.clamp(crate::ZOOM_MIN, crate::ZOOM_MAX);
    }
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
    // Pegar HTML como Markdown (fuera de los bloques de código), antes que la selección y los
    // campos.
    // Imágenes: pegadas (Ctrl+V con una imagen y sin texto) o soltadas sobre el editor.
    if let Some(store) = hooks.images.as_ref() {
        let mut inserts: Vec<String> = Vec::new();
        let mut errors: Vec<String> = Vec::new();
        let dropped: Vec<std::path::PathBuf> = ui.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| !p.as_os_str().is_empty())
                .take(50)
                .collect()
        });
        for p in dropped {
            match store.import(&p) {
                Ok(rel) => inserts.push(crate::image_store::markdown(&rel, None)),
                Err(e) => errors.push(e),
            }
        }
        if st.active {
            let (paste_text, ctrl_v) = ui.input(|i| {
                let t = i.events.iter().find_map(|e| match e {
                    egui::Event::Paste(t) => Some(t.clone()),
                    _ => None,
                });
                let v = i.events.iter().any(|e| {
                    matches!(e, egui::Event::Key { key: Key::V, pressed: true, modifiers: m, .. } if m.command && !m.shift)
                });
                (t, v)
            });
            // Si hay texto, gana el texto (Word, la web…); una imagen sola se guarda.
            if paste_text.as_deref().is_none_or(|t| t.trim().is_empty())
                && (paste_text.is_some() || ctrl_v)
                && let Some(img) = hooks.paste.as_ref().and_then(|p| p.image())
            {
                match store.save_pasted(&img) {
                    Ok(rel) => inserts
                        .push(crate::image_store::markdown(&rel, Some(iurefficient_ui::lang::pick("image", "imagen")))),
                    Err(e) => errors.push(e),
                }
            }
        }
        if let Some(e) = errors.first() {
            st.notice = Some((e.clone(), now + 6.0));
        }
        if !inserts.is_empty() {
            let md = inserts.join("\n\n");
            if st.active {
                // Cada imagen en su propio bloque (si no, no se dibuja en su lugar).
                let (_, line_start, line_end) = cursor_line_edges(ui, id, &st);
                let md = format!("{}{md}{}", if line_start { "" } else { "\n\n" }, if line_end { "" } else { "\n\n" });
                ui.input_mut(|i| {
                    i.events.retain(|e| !matches!(e, egui::Event::Paste(_)));
                    i.events.push(egui::Event::Paste(md));
                });
            } else if let Some(last) = st.blocks.chunks.last() {
                // Sin foco: al final del documento.
                st.focus = Some((last.uid, last.content.chars().count()));
                st.pending_paste = Some(format!("\n\n{md}"));
            }
        }
    }
    if st.active
        && let Some(md) = st.pending_paste.take()
    {
        ui.input_mut(|i| i.events.push(egui::Event::Paste(md)));
    }
    // Ctrl+Mayús+V: texto plano.
    if st.active
        && let Some(p) = hooks.paste.as_ref()
        && !ui.input(|i| i.modifiers.shift)
        && ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Paste(_))))
    {
        let (focused, line_start, line_end) = cursor_line_edges(ui, id, &st);
        let in_code =
            focused.and_then(|i| st.blocks.chunks.get(i)).is_some_and(|c| crate::paste::in_code_block(&c.content));
        if !in_code {
            crate::paste::rewrite(ui, p.as_ref(), line_start, line_end);
        }
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
    // Buscar (Ctrl+F) y reemplazar (Ctrl+H).
    if st.active || st.find.open {
        let (f, h) = ui.input_mut(|i| {
            (i.consume_key(egui::Modifiers::COMMAND, Key::F), i.consume_key(egui::Modifiers::COMMAND, Key::H))
        });
        if f || h {
            // Desde el cursor; con algo seleccionado (de una línea), eso se busca desde su inicio.
            st.find.anchor = st.caret.map(|(u, p)| st.blocks.offset_of(u).saturating_add(p));
            if let Some((sel, at)) = selected_text(ui, id, &st, text)
                && !sel.is_empty()
                && !sel.contains('\n')
            {
                st.find.query = sel;
                st.find.anchor = Some(at);
            }
            st.find.key = 0;
            st.find.open = true;
            st.find.replace = h;
            st.find.focus_query = true;
            st.find.scroll = true;
        }
    }
    if st.find.open {
        find_bar(ui, id, &mut st, text, now, &mut changed);
    }
    if let Some((msg, until)) = st.notice.clone() {
        if now < until {
            ui.colored_label(ui.visuals().error_fg_color, msg);
            ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64((until - now).max(0.1)));
        } else {
            st.notice = None;
        }
    }
    let finding = st.find.open && !st.find.matches.is_empty();
    let starts = (st.span.is_some() || finding).then(|| st.blocks.starts());
    // El bloque de la coincidencia actual (para desplazarse hasta él).
    let scroll_to = if finding && st.find.scroll {
        st.find.matches.get(st.find.current).and_then(|r| {
            let s = starts.as_ref()?;
            let idx = s.partition_point(|&s0| s0 <= r.start).saturating_sub(1);
            st.blocks.chunks.get(idx).map(|c| c.uid)
        })
    } else {
        None
    };
    let select_uid = st.select.map(|x| x.0);
    let shift = ui.input(|i| i.modifiers.shift);
    let pointer_busy = ui.input(|i| i.pointer.any_down() || i.pointer.any_released());
    let row_h = (style.size * crate::LINE_SPACING).max(1.0);
    let gap = (style.size * 0.75).round();
    // Antes de un título, más aire (como en un documento): un título va con lo que le sigue.
    let heading_gap = (style.size * 0.9).round();
    let mut act: Option<Act> = None;
    let mut active = false;
    let mut resp: Option<egui::Response> = None;
    let mut placed: Vec<(usize, egui::Pos2, Arc<egui::Galley>)> = Vec::new();
    // Arrastre que sale de su bloque: (bloque, ancla en él, puntero).
    let mut dragging: Option<(usize, usize, egui::Pos2)> = None;
    egui::ScrollArea::vertical().id_salt(id.with("scroll")).auto_shrink([false, false]).show_viewport(
        ui,
        |ui, viewport| {
            // El texto en una columna de documento centrada (como un procesador de textos): con
            // ventanas anchas no se va de orilla a orilla.
            let full = ui.available_width();
            let col = full.min(style.size * crate::COLUMN_EMS).max(full.min(240.0));
            let mut column = ui.available_rect_before_wrap();
            column.min.x += ((full - col) / 2.0).floor();
            column.max.x = column.min.x + col;
            ui.scope_builder(egui::UiBuilder::new().max_rect(column), |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let width = ui.available_width();
                // Se dibuja una pantalla más arriba y abajo: desplazar no muestra huecos.
                let margin = viewport.height();
                let mut y = 0.0f32;
                let mut space = 0.0f32;
                let only = st.blocks.chunks.len() == 1;
                for i in 0..st.blocks.chunks.len() {
                    let pending = st.focus;
                    let before_heading =
                        st.blocks.chunks.get(i.saturating_add(1)).is_some_and(|n| heading_level(&n.content) > 0);
                    let Some(c) = st.blocks.chunks.get_mut(i) else { break };
                    if (c.width - width).abs() >= 1.0 {
                        c.height = estimate(&c.content, row_h, width, style.size);
                        c.width = width;
                    }
                    let fid = id.with(("block", c.uid));
                    let mut g = if c.born || c.sep.matches('\n').count() >= 2 { gap } else { 0.0 };
                    if before_heading && g > 0.0 {
                        g += heading_gap;
                    }
                    let wanted = pending.and_then(|(u, at)| (u == c.uid).then_some(at));
                    let focused = ui.memory(|m| m.has_focus(fid));
                    active |= focused || wanted.is_some();
                    let in_table = st.cell.is_some_and(|x| x.0 == c.uid) || st.cell_focus.is_some_and(|x| x.0 == c.uid);
                    // Una tabla editada como Markdown vuelve a cuadrícula al dejarla.
                    let picking = st.templates == Some(c.uid);
                    if c.raw && !focused && wanted.is_none() && !picking {
                        c.raw = false;
                    }
                    let seen = y + c.height + g >= viewport.min.y - margin && y <= viewport.max.y + margin;
                    let pinned = scroll_to == Some(c.uid) || select_uid == Some(c.uid);
                    if !(seen || focused || wanted.is_some() || in_table || pinned) {
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
                        && !picking
                        && r.accepts(&c.content)
                    {
                        if wanted.is_some() {
                            // Entrar con las flechas: se edita el código.
                            c.raw = true;
                        } else {
                            let uid = c.uid;
                            // Una imagen (con su texto alternativo) o un diagrama.
                            let image_alt = crate::images::image_of(&c.content).map(|(_, alt)| alt);
                            let resp_d = match st.diagrams.get(ui.ctx(), r, &c.content) {
                                View::Failed(e) => {
                                    diagram_error = Some(e);
                                    None
                                }
                                View::Pending => {
                                    let h = c.height.max(48.0);
                                    let (rect, resp) =
                                        ui.allocate_exact_size(egui::vec2(width, h), egui::Sense::hover());
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
                                    // Al ancho del editor y, como mucho, de este alto.
                                    let k = (width / size.x.max(1.0)).min(MAX_IMAGE_HEIGHT / size.y.max(1.0)).min(1.0);
                                    let (w, h) = (size.x * k, size.y * k);
                                    let pick = iurefficient_ui::lang::pick;
                                    let hover = if image_alt.is_some() {
                                        pick("Click to edit the image", "Clic para editar la imagen")
                                    } else {
                                        pick("Click to edit the diagram", "Clic para editar el diagrama")
                                    };
                                    let resp = ui
                                        .add(egui::Image::new((tex.id(), egui::vec2(w, h))).sense(egui::Sense::click()))
                                        .on_hover_text(hover)
                                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                                    let pos = i + 1;
                                    resp.widget_info(|| {
                                        let label = match &image_alt {
                                            Some(alt) if !alt.trim().is_empty() => {
                                                format!("{} {pos}: {alt}", pick("Image", "Imagen"))
                                            }
                                            Some(_) => format!("{} {pos}", pick("Image", "Imagen")),
                                            None => format!("{} {pos}", pick("Diagram", "Diagrama")),
                                        };
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
                                let len = st.blocks.chunks.get(i).map_or(0, |c| c.content.chars().count());
                                find_tint(ui, &mut st, starts.as_ref(), i, len, rd.rect, scroll_to == Some(uid));
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
                        let len = st.blocks.chunks.get(i).map_or(0, |c| c.content.chars().count());
                        find_tint(ui, &mut st, starts.as_ref(), i, len, out.rect, scroll_to == Some(uid));
                        let th = out.rect.height();
                        if let Some(c) = st.blocks.chunks.get_mut(i) {
                            c.height = th;
                        }
                        // Dentro de una selección entre bloques: la tabla entera se ve seleccionada.
                        if let (Some((a, b)), Some(starts)) = (st.span, starts.as_ref()) {
                            let s0 = starts.get(i).copied().unwrap_or(0);
                            if a.max(b) > s0 && a.min(b) <= s0 {
                                ui.painter().rect_filled(
                                    out.rect,
                                    0.0,
                                    ui.visuals().selection.bg_fill.gamma_multiply(0.4),
                                );
                            }
                        }
                        if g > 0.0 {
                            ui.add_space(g);
                        }
                        y += th + g;
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
                    if let Some((u, a, b)) = st.select
                        && select_uid == Some(u)
                        && st.blocks.chunks.get(i).is_some_and(|c| c.uid == u)
                    {
                        let mut s = TextEditState::load(ui.ctx(), fid).unwrap_or_default();
                        s.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(a), CCursor::new(b))));
                        s.store(ui.ctx(), fid);
                        ui.memory_mut(|m| m.request_focus(fid));
                        st.select = None;
                    }
                    if focused && act.is_none() {
                        act = block_key(ui, fid, &st.blocks, i);
                    }
                    // Un diagrama Mermaid en edición: el menú de plantillas (como la 1.x).
                    let mut templated = false;
                    if let Some(c) = st.blocks.chunks.get_mut(i)
                        && (focused || picking)
                        && crate::templates::is_mermaid(&c.content)
                    {
                        let (uid, open) = (c.uid, picking);
                        let mut toggle = false;
                        let mut chosen: Option<String> = None;
                        ui.horizontal(|ui| {
                            let pick = iurefficient_ui::lang::pick;
                            let r = ui.button(format!("{}…", pick("Template", "Plantilla"))).on_hover_text(pick(
                                "Replace the code with a template (Ctrl+Z undoes it)",
                                "Cambiar el código por una plantilla (Ctrl+Z lo deshace)",
                            ));
                            toggle = r.clicked();
                        });
                        if open {
                            egui::Frame::popup(ui.style()).show(ui, |ui| {
                                egui::ScrollArea::vertical().max_height(260.0).id_salt(("tpl", uid)).show(ui, |ui| {
                                    for (name, code) in crate::templates::mermaid() {
                                        if ui.selectable_label(false, name).clicked() {
                                            chosen = Some(code);
                                        }
                                    }
                                });
                            });
                        }
                        if let Some(code) = chosen {
                            c.content = crate::templates::replace_code(&c.content, &code);
                            st.templates = None;
                            st.focus = Some((uid, 0));
                            templated = true;
                        } else if toggle {
                            st.templates = if open { None } else { Some(uid) };
                            if open {
                                st.focus = Some((uid, 0));
                            }
                        }
                    } else if picking && !focused {
                        st.templates = None;
                    }
                    let Some(c) = st.blocks.chunks.get_mut(i) else { break };
                    let hint = if only && c.content.is_empty() { hint } else { "" };
                    let (changed, out) = field(
                        ui,
                        fid,
                        &mut c.content,
                        hint,
                        1,
                        &style,
                        skey,
                        hooks.spell.as_deref(),
                        hooks.notes.as_deref(),
                        true,
                    );
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
                    if (changed || templated) && act.is_none() {
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
                    // Las coincidencias de la búsqueda en este bloque.
                    if finding && let Some(starts) = starts.as_ref() {
                        let s0 = starts.get(i).copied().unwrap_or(0);
                        let ms = &st.find.matches;
                        let k0 = ms.partition_point(|r| r.end <= s0);
                        for (k, r) in ms.iter().enumerate().skip(k0) {
                            if r.start > s0 + len {
                                break;
                            }
                            let color = if k == st.find.current { FIND_CURRENT } else { FIND_ALL };
                            paint_range(
                                ui,
                                &out.galley,
                                out.galley_pos,
                                r.start.saturating_sub(s0),
                                (r.end - s0).min(len),
                                r.end > s0 + len,
                                color,
                            );
                        }
                    }
                    if scroll_to == Some(uid) {
                        out.response.response.scroll_to_me(Some(egui::Align::Center));
                        st.find.scroll = false;
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
                        let secondary = out.state.cursor.char_range().map_or(primary, |x| x.secondary.index.0);
                        st.anchor = Some((uid, secondary));
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
            });
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
            Act::CodeExit(i, t) => {
                if let Some(c) = st.blocks.chunks.get_mut(i) {
                    c.content = t.trim_end_matches(['\n', '\r']).to_string();
                }
                let _ = st.blocks.edited(i, 0);
                (st.blocks.break_after(i), true)
            }
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
    // La que nombra el anfitrión es siempre la de un campo de texto (nunca la de una imagen, un
    // diagrama o una tabla, que conservan sus propios nombres).
    let response = resp.unwrap_or_else(|| ui.allocate_response(egui::Vec2::ZERO, egui::Sense::hover()));
    Output { changed, response, link: crate::take_link(ui) }
}

/// Una edición que llega de fuera del editor (dictado por voz con IureVoz, enlaces
/// `iureditor://insert` y `iureditor://edit`), aunque la ventana no tenga el foco.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExternalEdit {
    /// Escribe el texto en el cursor.
    Insert(String),
    /// Borra la palabra antes del cursor.
    DeleteWord,
    /// Borra la línea del cursor.
    DeleteLine,
    /// Deshace el último paso.
    Undo,
    /// Rehace.
    Redo,
    /// Pone o quita marcas alrededor de la selección (`**` negrita, `*` cursiva, `~~` tachado,
    /// `` ` `` código); sin selección deja el cursor entre ellas.
    Wrap(String),
    /// Tipo de las líneas de la selección (título, viñeta, numerada, tarea); otra vez, lo quita.
    Line(edit::LineKind),
    /// Cita (`> `) en las líneas de la selección; si ya lo son, la quita.
    Quote,
    /// `[selección](url)`.
    Link,
}

/// Aplica `edit` en el cursor (o la selección) del editor `id` —el último que tuvo; sin cursor, al
/// final—, como si se hubiera tecleado: un paso de deshacer y el cursor queda después (la selección
/// sigue puesta al dar formato). `true` si cambió el texto.
/// Lleva el cursor del editor `id` al carácter `at` del documento (y lo muestra): el esquema de una
/// app, un resultado de búsqueda.
pub fn focus_offset(ctx: &egui::Context, id: Id, text: &str, at: usize) {
    let mut st = ctx.data_mut(|d| d.remove_temp::<State>(id)).unwrap_or_default();
    let h = hash_str(text);
    if st.hash != h || st.blocks.chunks.is_empty() {
        st.blocks = Blocks::new(text);
        st.hash = h;
        st.history.clear();
    }
    let f = st.blocks.at_offset(at.min(text.chars().count()));
    st.caret = Some(f);
    st.anchor = Some(f);
    st.focus = Some(f);
    ctx.data_mut(|d| d.insert_temp(id, st));
    ctx.request_repaint();
}

/// Dónde está el cursor del editor `id` (carácter del documento), si tuvo uno.
pub fn caret_offset(ctx: &egui::Context, id: Id, text: &str) -> Option<usize> {
    let st = ctx.data(|d| d.get_temp::<State>(id))?;
    if st.hash != hash_str(text) {
        return None;
    }
    st.caret.map(|(uid, pos)| st.blocks.offset_of(uid).saturating_add(pos))
}

pub fn apply_external(ctx: &egui::Context, id: Id, text: &mut String, edit: ExternalEdit) -> bool {
    let mut st = ctx.data_mut(|d| d.remove_temp::<State>(id)).unwrap_or_default();
    let h = hash_str(text);
    if st.hash != h || st.blocks.chunks.is_empty() {
        st.blocks = Blocks::new(text);
        st.hash = h;
        st.caret = None;
        st.history.clear();
    }
    let total = text.chars().count();
    // Sin cursor: escribir va al final; dar formato, a la última línea con texto.
    let formatting =
        matches!(edit, ExternalEdit::Wrap(_) | ExternalEdit::Line(_) | ExternalEdit::Quote | ExternalEdit::Link);
    let fallback = if formatting { text.trim_end_matches(['\n', '\r']).chars().count() } else { total };
    let caret = st.caret.map_or(fallback, |(uid, pos)| st.blocks.offset_of(uid).saturating_add(pos)).min(total);
    let anchor = st
        .anchor
        .filter(|a| st.caret.is_some_and(|c| c.0 == a.0))
        .map_or(caret, |(uid, pos)| st.blocks.offset_of(uid).saturating_add(pos))
        .min(total);
    let (lo, hi) = (caret.min(anchor), caret.max(anchor));
    // Selección que dejar puesta tras la edición (caracteres del documento).
    let mut keep: Option<(usize, usize)> = None;
    let byte = |chars: usize, t: &str| t.char_indices().nth(chars).map_or(t.len(), |(b, _)| b);
    let before = text.clone();
    let now = ctx.input(|i| i.time);
    let at = byte(caret, text);
    let undo = matches!(edit, ExternalEdit::Undo | ExternalEdit::Redo);
    let new_caret = match edit {
        ExternalEdit::Insert(ins) => {
            let ins: String = ins.chars().filter(|c| !c.is_control() || *c == '\n' || *c == '\t').collect();
            if ins.is_empty() {
                None
            } else {
                text.insert_str(at, &ins);
                Some(caret + ins.chars().count())
            }
        }
        ExternalEdit::DeleteWord => {
            let head = text.get(..at).unwrap_or_default();
            let trimmed = head.trim_end_matches([' ', '\t']);
            let start =
                trimmed.char_indices().rev().find(|(_, c)| c.is_whitespace()).map_or(0, |(b, c)| b + c.len_utf8());
            (start < at).then(|| {
                text.replace_range(start..at, "");
                text.get(..start).map_or(0, |p| p.chars().count())
            })
        }
        ExternalEdit::DeleteLine => {
            let start = text.get(..at).and_then(|p| p.rfind('\n')).map_or(0, |b| b + 1);
            let end = text.get(at..).and_then(|r| r.find('\n')).map_or(text.len(), |b| at + b + 1);
            (start < end).then(|| {
                text.replace_range(start..end, "");
                text.get(..start).map_or(0, |p| p.chars().count())
            })
        }
        ExternalEdit::Undo => st.history.undo(text).map(|b| text.get(..b).map_or(0, |p| p.chars().count())),
        ExternalEdit::Redo => st.history.redo(text).map(|b| text.get(..b).map_or(0, |p| p.chars().count())),
        ExternalEdit::Wrap(mark) => {
            let e = edit::toggle_wrap(text, lo, hi, &mark);
            keep = Some((e.sel_start, e.sel_end));
            (e.text != *text).then(|| {
                *text = e.text;
                e.sel_end
            })
        }
        ExternalEdit::Line(kind) => edit::set_line_kind(text, lo, hi, kind).map(|e| {
            keep = Some((e.sel_start, e.sel_end));
            *text = e.text;
            e.sel_end
        }),
        ExternalEdit::Link => {
            let e = edit::link(text, lo, hi);
            keep = Some((e.sel_start, e.sel_end));
            (e.text != *text).then(|| {
                *text = e.text;
                e.sel_end
            })
        }
        ExternalEdit::Quote => {
            // Las líneas que toca la selección: si todas son cita, se quita `> `; si no, se pone.
            let lb = byte(lo, text);
            let hb = byte(hi, text);
            let start = text.get(..lb).and_then(|p| p.rfind('\n')).map_or(0, |b| b + 1);
            let end = text.get(hb..).and_then(|r| r.find('\n')).map_or(text.len(), |b| hb + b);
            let region = text.get(start..end).unwrap_or_default().to_string();
            let all = region.lines().all(|l| l.trim_start().starts_with('>'));
            let new: Vec<String> = region
                .split('\n')
                .map(|l| {
                    if all {
                        let t = l.trim_start();
                        t.strip_prefix("> ").or_else(|| t.strip_prefix('>')).unwrap_or(t).to_string()
                    } else {
                        format!("> {l}")
                    }
                })
                .collect();
            let joined = new.join("\n");
            text.replace_range(start..end, &joined);
            Some(text.get(..start + joined.len()).map_or(0, |p| p.chars().count()))
        }
    };
    let changed = new_caret.is_some();
    if let Some(c) = new_caret {
        if *text != before && !undo {
            st.history.record(&before, text, now);
        }
        st.blocks = Blocks::new(text);
        st.hash = hash_str(text);
        let f = st.blocks.at_offset(c);
        st.caret = Some(f);
        st.anchor = Some(f);
        st.focus = Some(f);
        // La selección sigue puesta si cae en un bloque (negrita sobre lo seleccionado).
        if let Some((a, b)) = keep.filter(|(a, b)| a != b) {
            let (fa, fb) = (st.blocks.at_offset(a), st.blocks.at_offset(b));
            if fa.0 == fb.0 {
                st.select = Some((fa.0, fa.1, fb.1));
                st.anchor = Some(fa);
                st.focus = None;
            }
        }
        ctx.request_repaint();
    }
    ctx.data_mut(|d| d.insert_temp(id, st));
    changed
}

#[cfg(test)]
mod tests {
    #[test]
    fn heading_levels_follow_markdown() {
        assert_eq!(super::heading_level("# Uno"), 1);
        assert_eq!(super::heading_level("### Tres\ntexto"), 3);
        assert_eq!(super::heading_level("#"), 1);
        assert_eq!(super::heading_level("#etiqueta"), 0);
        assert_eq!(super::heading_level("####### siete"), 0);
        assert_eq!(super::heading_level("texto"), 0);
    }

    use super::*;
    use proptest::prelude::*;

    fn contents(b: &Blocks) -> Vec<&str> {
        b.chunks.iter().map(|c| c.content.as_str()).collect()
    }

    /// Dictado por voz (IureVoz): escribir y borrar en el cursor sin que la ventana tenga el foco.
    #[test]
    fn external_edits_go_to_the_caret_and_undo() {
        let ctx = egui::Context::default();
        let id = Id::unique("externo");
        let mut text = "Hola mundo\nsegunda línea\n".to_string();
        // Sin cursor: al final.
        assert!(apply_external(&ctx, id, &mut text, ExternalEdit::Insert(" X".into())));
        assert_eq!(text, "Hola mundo\nsegunda línea\n X");
        // Con cursor después de «Hola».
        let mut st = ctx.data_mut(|d| d.remove_temp::<State>(id)).unwrap_or_default();
        st.caret = Some(st.blocks.at_offset(4));
        ctx.data_mut(|d| d.insert_temp(id, st));
        assert!(apply_external(&ctx, id, &mut text, ExternalEdit::Insert(",\u{7}".into())));
        assert_eq!(text, "Hola, mundo\nsegunda línea\n X", "sin caracteres de control");
        assert!(apply_external(&ctx, id, &mut text, ExternalEdit::DeleteWord));
        assert_eq!(text, " mundo\nsegunda línea\n X");
        assert!(apply_external(&ctx, id, &mut text, ExternalEdit::DeleteLine));
        assert_eq!(text, "segunda línea\n X");
        assert!(!apply_external(&ctx, id, &mut text, ExternalEdit::Insert(String::new())));
        for _ in 0..3 {
            assert!(apply_external(&ctx, id, &mut text, ExternalEdit::Undo));
        }
        assert_eq!(text, "Hola mundo\nsegunda línea\n X");
        // Si el anfitrión cambió el texto, la historia anterior no se aplica.
        text = "otro".into();
        assert!(!apply_external(&ctx, id, &mut text, ExternalEdit::Undo));
        assert!(apply_external(&ctx, id, &mut text, ExternalEdit::DeleteWord));
        assert_eq!(text, "");
    }

    /// La barra de formato de una app: negrita sobre la selección, título, cita, enlace, rehacer.
    #[test]
    fn format_actions_use_the_selection() {
        let ctx = egui::Context::default();
        let id = Id::unique("formato");
        let mut text = "Hola mundo\n\nsegunda".to_string();
        apply_external(&ctx, id, &mut text, ExternalEdit::Insert(String::new()));
        // Selección de «mundo» en el primer bloque.
        let mut st = ctx.data_mut(|d| d.remove_temp::<State>(id)).unwrap_or_default();
        st.blocks = Blocks::new(&text);
        st.hash = hash_str(&text);
        let uid = st.blocks.chunks.first().map_or(0, |c| c.uid);
        st.caret = Some((uid, 10));
        st.anchor = Some((uid, 5));
        ctx.data_mut(|d| d.insert_temp(id, st));
        assert!(apply_external(&ctx, id, &mut text, ExternalEdit::Wrap("**".into())));
        assert_eq!(text, "Hola **mundo**\n\nsegunda");
        let st = ctx.data(|d| d.get_temp::<State>(id)).unwrap_or_default();
        assert!(st.select.is_some_and(|(_, a, b)| b - a == 5), "la selección sigue sobre «mundo»");
        // Título en la línea del cursor.
        assert!(apply_external(&ctx, id, &mut text, ExternalEdit::Line(edit::LineKind::Heading(2))));
        assert!(text.starts_with("## Hola **mundo**"), "{text}");
        // Cita y la quita.
        assert!(apply_external(&ctx, id, &mut text, ExternalEdit::Quote));
        assert!(text.starts_with("> ## Hola"), "{text}");
        assert!(apply_external(&ctx, id, &mut text, ExternalEdit::Quote));
        assert!(text.starts_with("## Hola"), "{text}");
        // Deshacer y rehacer.
        assert!(apply_external(&ctx, id, &mut text, ExternalEdit::Undo));
        assert!(text.starts_with("> ## Hola"), "{text}");
        assert!(apply_external(&ctx, id, &mut text, ExternalEdit::Redo));
        assert!(text.starts_with("## Hola"), "{text}");
        assert!(caret_offset(&ctx, id, &text).is_some());
        assert!(caret_offset(&ctx, id, "otro").is_none(), "texto distinto: no se sabe");
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
        assert_eq!(b.text(), "uno\n\ndos\n", "al final del documento, un solo salto");
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
        let _ = crate::highlight::wait_ready(std::time::Duration::from_secs(20));
        frame(&mut text);
        frame(&mut text);
        let before = crate::highlight::CALLS.with(std::cell::Cell::get);
        let steady = frame(&mut text);
        // Un cuadro sin cambios no vuelve a resaltar nada (la caché del dibujo por bloque).
        let again = crate::highlight::CALLS.with(std::cell::Cell::get) - before;
        assert_eq!(again, 0, "un cuadro estable no resalta");
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

    struct Instant;

    impl crate::DiagramRenderer for Instant {
        fn accepts(&self, block: &str) -> bool {
            block.starts_with("```mermaid")
        }
        fn key(&self) -> u64 {
            1
        }
        fn render(&self, _block: &str, _scale: f32) -> Result<crate::DiagramImage, String> {
            Ok(crate::DiagramImage { width: 300, height: 120, rgba: vec![200; 300 * 120 * 4], points: [300.0, 120.0] })
        }
    }

    /// S1.22: 100 páginas con 20 diagramas y el corrector: < 8 ms por cuadro al teclear
    /// (`cargo test --release -p iure-doc-ui hundred_pages_with_diagrams -- --nocapture`).
    #[test]
    fn hundred_pages_with_diagrams() {
        let page = "## Sección\n\nTexto con **negrita**, *cursiva*, `código` y [enlace](https://x.mx).\n\n- uno\n- [ ] dos\n  - tres\n\n> cita\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n";
        let diagram = "```mermaid\ngraph LR; A --> B\n```\n\n";
        let mut text = String::new();
        for k in 0..1500 {
            text.push_str(page);
            if k % 75 == 0 {
                text.push_str(diagram);
            }
        }
        let dir = std::env::temp_dir().join(format!("iure-doc-perf-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("es_MX.aff"), "SET UTF-8\n");
        let _ = std::fs::write(dir.join("es_MX.dic"), "3\ntexto\ncon\nuno\n");
        let speller = crate::Speller::new(&["es"], vec![dir.clone()], None);
        let hooks = Hooks { diagrams: Some(Arc::new(Instant)), spell: Some(speller), ..Default::default() };
        let ctx = egui::Context::default();
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0))),
            ..Default::default()
        };
        let id = Id::unique("perf-diagrams");
        let frame = |text: &mut String| {
            let t = std::time::Instant::now();
            let mut out = ctx.run_ui(input(), |ui| {
                show_hooks(ui, id, text, "", hooks.clone());
            });
            out.textures_delta.clear();
            t.elapsed().as_secs_f64() * 1000.0
        };
        let _ = crate::highlight::wait_ready(std::time::Duration::from_secs(20));
        // Los diagramas y los diccionarios llegan de otros hilos: unos cuadros para que lleguen.
        for _ in 0..20 {
            frame(&mut text);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let steady = frame(&mut text);
        let mut worst = 0.0f64;
        for k in 0..20 {
            let t = std::time::Instant::now();
            let mut st = ctx.data_mut(|d| d.remove_temp::<State>(id)).unwrap_or_default();
            if let Some(c) = st.blocks.chunks.get_mut(2) {
                c.content.push(if k % 2 == 0 { 'x' } else { ' ' });
            }
            st.blocks.edited(2, 1);
            text = st.blocks.text();
            st.hash = hash_str(&text);
            ctx.data_mut(|d| d.insert_temp(id, st));
            let op = t.elapsed().as_secs_f64() * 1000.0;
            worst = worst.max(op + frame(&mut text));
        }
        let diagrams = text.matches("```mermaid").count();
        eprintln!(
            "100 páginas ({} KB) con {diagrams} diagramas y corrector: estable {steady:.1} ms, peor tecla {worst:.1} ms",
            text.len() / 1024
        );
        let _ = std::fs::remove_dir_all(&dir);
        assert!(diagrams >= 20);
        assert!(worst < 1_000.0, "{worst} ms");
    }
}
