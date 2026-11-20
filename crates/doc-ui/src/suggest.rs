//! Menús que se abren al teclear, como en IureEditor 1.x:
//!
//! - **`/`** al principio de una línea o tras un espacio: títulos, listas, cita, código, tabla,
//!   diagrama, fórmula, separador, avisos y mapas mentales (se filtra con lo que sigue a la barra:
//!   `/mental`, `/diagrama`, `/tabla`…).
//! - **`[[`**: enlace a otra nota (las da la app con [`NoteIndex`]); `[[nota#` sugiere sus títulos;
//!   si lo escrito no es una nota, «Crear nota «…»» deja el enlace (la app la crea al abrirlo).
//!
//! Flechas para elegir, Intro (o Tab en `[[`) para aceptar, Esc para cerrar. Fuera de los bloques
//! de código.

use egui::{Id, Key, Modifiers, Ui};
use iure_doc::edit::{self, LineKind};
use iurefficient_ui::lang::pick;

/// Las notas que puede enlazar `[[` (las da la app: la carpeta del documento o su bóveda).
pub trait NoteIndex: Send + Sync {
    /// Notas, como rutas relativas con `/` y sin la extensión (`Actas/2026-11-03`).
    fn notes(&self) -> Vec<String>;
    /// Títulos de una nota (para `[[nota#…`).
    fn headings(&self, _note: &str) -> Vec<String> {
        Vec::new()
    }
}

/// Opciones que se muestran como mucho.
const MAX_ITEMS: usize = 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Kind {
    Slash,
    Wiki,
}

/// Lo que se está escribiendo tras `/` o `[[`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Trigger {
    pub kind: Kind,
    /// Carácter donde empieza la marca (`/` o el primer `[`).
    pub start: usize,
    pub query: String,
}

/// Qué hace una opción.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Line(LineKind),
    /// Una marca al principio de la línea (`> `).
    Prefix(&'static str),
    /// Un bloque; el cursor queda `back` caracteres antes del final.
    Block {
        text: String,
        back: usize,
    },
    /// `[[destino]]`.
    Link(String),
    /// `[[nota#` → completar con el título (`[[nota#Título]]`).
    Heading(String, String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Item {
    pub label: String,
    pub detail: String,
    pub action: Action,
}

/// ¿Hay una marca abierta antes del cursor (`caret`, en caracteres)?
pub(crate) fn detect(text: &str, caret: usize) -> Option<Trigger> {
    let t0 = text.trim_start();
    if t0.starts_with("```") || t0.starts_with("~~~") || text.starts_with("    ") {
        return None;
    }
    let before: Vec<char> = text.chars().take(caret).collect();
    let line_start = before.iter().rposition(|&c| c == '\n').map_or(0, |i| i + 1);
    let line = before.get(line_start..)?;
    // `[[` sin cerrar.
    let mut k = line.len();
    while k >= 2 {
        if line.get(k - 2) == Some(&'[') && line.get(k - 1) == Some(&'[') {
            let rest: String = line.get(k..).unwrap_or_default().iter().collect();
            if !rest.contains("]]") && !rest.contains('[') && rest.chars().count() <= 150 {
                return Some(Trigger { kind: Kind::Wiki, start: line_start + k - 2, query: rest });
            }
            break;
        }
        k -= 1;
    }
    // `/` al principio o tras un espacio, sin espacios después.
    let slash = line.iter().rposition(|&c| c == '/')?;
    let ok_before = slash == 0 || line.get(slash - 1).is_some_and(|c| c.is_whitespace());
    let rest: String = line.get(slash + 1..).unwrap_or_default().iter().collect();
    (ok_before && !rest.chars().any(char::is_whitespace) && rest.chars().count() <= 30).then(|| Trigger {
        kind: Kind::Slash,
        start: line_start + slash,
        query: rest,
    })
}

/// En minúsculas y sin acentos.
fn fold(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'á' | 'à' | 'ä' | 'â' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            c => c,
        })
        .collect()
}

/// Puntuación aproximada de `query` en `target` (como la 1.x): primero una subcadena (mejor al
/// principio y en frontera de palabra), luego una subsecuencia. `None`: no aparece.
pub(crate) fn fuzzy(query: &str, target: &str) -> Option<f32> {
    let q: String = fold(query).chars().filter(|c| !c.is_whitespace()).collect();
    if q.is_empty() {
        return Some(0.0);
    }
    let t = fold(target);
    let boundary = |s: &str, i: usize| {
        i == 0
            || s.get(..i).and_then(|p| p.chars().last()).is_some_and(|c| c.is_whitespace() || "/\\_-.()[]".contains(c))
    };
    let len_diff = t.chars().count().saturating_sub(q.chars().count()) as f32;
    if let Some(i) = t.find(&q) {
        let at = t.get(..i).map_or(0, |p| p.chars().count()) as f32;
        return Some(1000.0 - at + if boundary(&t, i) { 200.0 } else { 0.0 } - len_diff);
    }
    let mut score = 0.0f32;
    let mut qi = q.chars().peekable();
    let mut last: Option<usize> = None;
    for (n, (bi, c)) in t.char_indices().enumerate() {
        let Some(&want) = qi.peek() else { break };
        if c == want {
            qi.next();
            score += if last.is_some_and(|l| l + 1 == n) { 15.0 } else { 0.0 };
            score += if boundary(&t, bi) { 10.0 } else { 0.0 };
            if let Some(l) = last {
                score -= ((n - l - 1) as f32).min(10.0);
            }
            last = Some(n);
        }
    }
    (qi.peek().is_none()).then_some(score - 0.1 * len_diff)
}

fn block(text: String, back: usize) -> Action {
    Action::Block { text, back }
}

/// Las órdenes de `/`, en el orden de la 1.x, filtradas por `query`.
pub(crate) fn slash_items(query: &str) -> Vec<Item> {
    let callout = |en: &'static str, es: &'static str, tag: &str, kw: &str| {
        (
            format!("{}: {}", pick("Callout", "Aviso"), pick(en, es)),
            format!("callout admonition aviso nota {kw}"),
            block(format!("> [!{tag}]\n> "), 0),
        )
    };
    let all: Vec<(String, String, Action)> = vec![
        (
            pick("Heading 1", "Título 1").into(),
            "title heading titulo encabezado h1 #".into(),
            Action::Line(LineKind::Heading(1)),
        ),
        (
            pick("Heading 2", "Título 2").into(),
            "title heading titulo encabezado h2 ##".into(),
            Action::Line(LineKind::Heading(2)),
        ),
        (
            pick("Heading 3", "Título 3").into(),
            "title heading titulo encabezado h3 ###".into(),
            Action::Line(LineKind::Heading(3)),
        ),
        (
            pick("Bullet list", "Lista con viñetas").into(),
            "bullet list unordered lista viñetas".into(),
            Action::Line(LineKind::Bullet),
        ),
        (
            pick("Numbered list", "Lista numerada").into(),
            "ordered numbered list lista numerada".into(),
            Action::Line(LineKind::Ordered),
        ),
        (
            pick("Task list", "Lista de tareas").into(),
            "task todo checkbox tareas lista".into(),
            Action::Line(LineKind::Task),
        ),
        (pick("Quote", "Cita").into(), "quote blockquote cita".into(), Action::Prefix("> ")),
        (pick("Code block", "Bloque de código").into(), "code block codigo pre".into(), block("```\n\n```".into(), 4)),
        (
            pick("Table", "Tabla").into(),
            "table tabla grid".into(),
            block(
                format!(
                    "| {a} 1 | {a} 2 | {a} 3 |\n| --- | --- | --- |\n|  |  |  |\n|  |  |  |",
                    a = pick("Column", "Columna")
                ),
                0,
            ),
        ),
        (
            pick("Diagram (mermaid)", "Diagrama (mermaid)").into(),
            "diagram mermaid diagrama flowchart flujo".into(),
            block(format!("```mermaid\n{}\n```", crate::templates::first()), 4),
        ),
        (
            pick("Formula (block)", "Fórmula (bloque)").into(),
            "math formula latex katex ecuacion formula".into(),
            block("$$\n\n$$".into(), 3),
        ),
        (
            pick("Divider", "Separador").into(),
            "divider horizontal rule separador hr line".into(),
            block("---\n".into(), 0),
        ),
        callout("Note", "Nota", "NOTE", "note nota"),
        callout("Tip", "Consejo", "TIP", "tip consejo"),
        callout("Important", "Importante", "IMPORTANT", "important importante"),
        callout("Warning", "Advertencia", "WARNING", "warning advertencia"),
        callout("Caution", "Precaución", "CAUTION", "caution precaucion"),
        (
            pick("Mind map", "Mapa mental").into(),
            "mindmap mind map mapa mental diagrama ramas".into(),
            block(
                format!(
                    "> [!MINDMAP] {}\n> - {}\n>   - {}\n> - {}",
                    pick("Topic", "Tema"),
                    pick("Idea A", "Idea A"),
                    pick("Detail", "Detalle"),
                    pick("Idea B", "Idea B")
                ),
                0,
            ),
        ),
    ];
    let q = fold(query);
    all.into_iter()
        .filter(|(label, kw, _)| q.is_empty() || fold(label).contains(&q) || fold(kw).contains(&q))
        .map(|(label, _, action)| Item { label, detail: String::new(), action })
        .collect()
}

/// Lo que se escribe en el enlace: el nombre si basta, si no la ruta.
fn link_text(note: &str, notes: &[String]) -> String {
    let name = note.rsplit('/').next().unwrap_or(note);
    let same = notes.iter().filter(|n| fold(n.rsplit('/').next().unwrap_or(n)) == fold(name)).count();
    if same <= 1 { name.to_string() } else { note.to_string() }
}

/// Las opciones de `[[` para `query`.
pub(crate) fn wiki_items(query: &str, index: Option<&dyn NoteIndex>) -> Vec<Item> {
    if query.contains('|') {
        return vec![Item {
            label: query.replace('|', " | "),
            detail: String::new(),
            action: Action::Link(query.into()),
        }];
    }
    let notes = index.map(NoteIndex::notes).unwrap_or_default();
    if let Some((target, head)) = query.split_once('#') {
        let heads = index.map(|i| i.headings(target.trim())).unwrap_or_default();
        let mut scored: Vec<(f32, String)> =
            heads.into_iter().filter_map(|h| fuzzy(head, &h).map(|s| (s, h))).collect();
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        return scored
            .into_iter()
            .take(MAX_ITEMS)
            .map(|(_, h)| Item {
                label: format!("# {h}"),
                detail: target.to_string(),
                action: Action::Heading(target.to_string(), h),
            })
            .collect();
    }
    let mut scored: Vec<(f32, &String)> = notes
        .iter()
        .take(10_000)
        .filter_map(|n| {
            let name = n.rsplit('/').next().unwrap_or(n);
            let by_name = fuzzy(query, name).map(|s| s + 50.0);
            let by_path = fuzzy(query, n);
            by_name.into_iter().chain(by_path).reduce(f32::max).map(|s| (s, n))
        })
        .collect();
    if !query.trim().is_empty() {
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    }
    let mut out: Vec<Item> = scored
        .into_iter()
        .take(MAX_ITEMS)
        .map(|(_, n)| Item {
            label: n.rsplit('/').next().unwrap_or(n).to_string(),
            detail: if n.contains('/') { n.clone() } else { String::new() },
            action: Action::Link(link_text(n, &notes)),
        })
        .collect();
    let q = query.trim();
    let exact = notes.iter().any(|n| fold(n.rsplit('/').next().unwrap_or(n)) == fold(q) || fold(n) == fold(q));
    if !q.is_empty() && !exact {
        out.truncate(MAX_ITEMS - 1);
        out.push(Item {
            label: format!("{} «{q}»", pick("Create note", "Crear nota")),
            detail: String::new(),
            action: Action::Link(q.to_string()),
        });
    }
    out
}

/// Aplica la opción: quita la marca y lo escrito (`start..caret`) y hace lo suyo. Devuelve el
/// texto nuevo y el cursor (en caracteres).
pub(crate) fn apply(text: &str, trig: &Trigger, caret: usize, item: &Item) -> (String, usize) {
    let chars: Vec<char> = text.chars().collect();
    let start = trig.start.min(chars.len());
    let caret = caret.clamp(start, chars.len());
    let head: String = chars.get(..start).unwrap_or_default().iter().collect();
    let tail: String = chars.get(caret..).unwrap_or_default().iter().collect();
    let at = start;
    let put = |ins: &str, back: usize| {
        let n = ins.chars().count();
        (format!("{head}{ins}{tail}"), at + n.saturating_sub(back))
    };
    match &item.action {
        Action::Link(t) => {
            // Si ya venían los `]]` (escritos a mano), no se duplican.
            let tail_closes = tail.starts_with("]]");
            let ins = if tail_closes { format!("[[{t}") } else { format!("[[{t}]]") };
            let (s, c) = put(&ins, 0);
            (s, if tail_closes { c + 2 } else { c })
        }
        Action::Heading(target, h) => {
            let tail_closes = tail.starts_with("]]");
            let ins = if tail_closes { format!("[[{target}#{h}") } else { format!("[[{target}#{h}]]") };
            let (s, c) = put(&ins, 0);
            (s, if tail_closes { c + 2 } else { c })
        }
        Action::Line(k) => {
            let base = format!("{head}{tail}");
            match edit::set_line_kind(&base, at, at, *k) {
                Some(e) => (e.text, e.sel_end),
                None => (base, at),
            }
        }
        Action::Prefix(p) => {
            let line_start = head.rfind('\n').map_or(0, |i| i + 1);
            let (a, b) = head.split_at(line_start);
            let n = p.chars().count();
            (format!("{a}{p}{b}{tail}"), at + n)
        }
        Action::Block { text: block, back } => {
            // Un bloque va en su propia línea.
            let line_start = head.rsplit('\n').next().is_none_or(|l| l.trim().is_empty());
            let ins = if line_start { block.clone() } else { format!("\n\n{block}") };
            put(&ins, *back)
        }
    }
}

/// Estado del menú de un campo (entre cuadros).
#[derive(Clone, Debug, Default)]
struct State {
    sel: usize,
    /// Cerrado con Esc para la marca que empieza aquí.
    hidden: Option<(Kind, usize)>,
    /// Se mostró en el último cuadro (las teclas entre bloques no lo pisan).
    open: bool,
    /// Dónde se dibujó: un clic ahí quita el foco al campo, pero el menú sigue hasta elegir.
    rect: Option<egui::Rect>,
}

fn state_id(id: Id) -> Id {
    id.with("suggest")
}

/// ¿El menú del campo `id` está abierto? (Flechas e Intro son suyas.)
pub(crate) fn is_open(ui: &Ui, id: Id) -> bool {
    ui.data(|d| d.get_temp::<State>(state_id(id))).is_some_and(|s| s.open)
}

/// Antes del campo: si hay menú, sus teclas. Devuelve el texto nuevo y el cursor si se eligió algo.
pub(crate) fn keys(
    ui: &Ui,
    id: Id,
    text: &str,
    caret: usize,
    notes: Option<&dyn NoteIndex>,
) -> Option<(String, usize)> {
    let mut st = ui.data(|d| d.get_temp::<State>(state_id(id))).unwrap_or_default();
    let Some(trig) = detect(text, caret) else {
        if st.open {
            st.open = false;
            ui.data_mut(|d| d.insert_temp(state_id(id), st));
        }
        return None;
    };
    if st.hidden == Some((trig.kind, trig.start)) {
        return None;
    }
    let items = match trig.kind {
        Kind::Slash => slash_items(&trig.query),
        Kind::Wiki => wiki_items(&trig.query, notes),
    };
    if items.is_empty() {
        return None;
    }
    let n = items.len();
    let mut out = None;
    ui.input_mut(|i| {
        if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
            st.sel = (st.sel + 1) % n;
        }
        if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
            st.sel = (st.sel + n - 1) % n;
        }
        if i.consume_key(Modifiers::NONE, Key::Escape) {
            st.hidden = Some((trig.kind, trig.start));
            st.open = false;
            return;
        }
        let accept = i.consume_key(Modifiers::NONE, Key::Enter)
            || (trig.kind == Kind::Wiki && i.consume_key(Modifiers::NONE, Key::Tab));
        if accept && let Some(item) = items.get(st.sel.min(n - 1)) {
            out = Some(apply(text, &trig, caret, item));
            st.sel = 0;
            st.open = false;
        }
    });
    ui.data_mut(|d| d.insert_temp(state_id(id), st));
    out
}

/// Después del campo: dibuja el menú junto al cursor. Devuelve el texto nuevo y el cursor si se
/// eligió algo con el ratón.
pub(crate) fn popup(
    ui: &Ui,
    id: Id,
    out: &egui::text_edit::TextEditOutput,
    text: &str,
    notes: Option<&dyn NoteIndex>,
) -> Option<(String, usize)> {
    let mut st = ui.data(|d| d.get_temp::<State>(state_id(id))).unwrap_or_default();
    let focused = ui.memory(|m| m.has_focus(id));
    // Pulsar una opción con el ratón le quita el foco al campo: mientras el puntero esté en el
    // menú, sigue abierto (si no, el clic nunca llegaba a la opción).
    let in_popup = st.open
        && st.rect.is_some_and(|r| {
            ui.input(|i| i.pointer.interact_pos().or(i.pointer.hover_pos()).is_some_and(|p| r.expand(2.0).contains(p)))
        });
    let caret = out.cursor_range.map(|r| r.primary.index.0).or_else(|| {
        egui::text_edit::TextEditState::load(ui.ctx(), id)
            .and_then(|s| s.cursor.char_range())
            .map(|r| r.primary.index.0)
    });
    let trig = caret.filter(|_| focused || in_popup).and_then(|c| detect(text, c).map(|t| (t, c)));
    let any = trig.is_some();
    let Some((trig, caret)) = trig.filter(|(t, _)| st.hidden != Some((t.kind, t.start))) else {
        if st.open || st.hidden.is_some() {
            // Fuera de la marca cerrada con Esc: se olvida.
            st.open = false;
            if !any {
                st.hidden = None;
            }
            ui.data_mut(|d| d.insert_temp(state_id(id), st));
        }
        return None;
    };
    let items = match trig.kind {
        Kind::Slash => slash_items(&trig.query),
        Kind::Wiki => wiki_items(&trig.query, notes),
    };
    if items.is_empty() && trig.kind == Kind::Slash {
        st.open = false;
        ui.data_mut(|d| d.insert_temp(state_id(id), st));
        return None;
    }
    if st.sel >= items.len() {
        st.sel = 0;
    }
    let pos = out.galley.pos_from_cursor(egui::text::CCursor::new(caret));
    let anchor = out.galley_pos + pos.left_bottom().to_vec2() + egui::vec2(0.0, 6.0);
    let c = iurefficient_ui::theme::colors(ui);
    let mut chosen = None;
    let width = if trig.kind == Kind::Wiki { 320.0 } else { 256.0 };
    let area = egui::Area::new(id.with("suggest-area"))
        .order(egui::Order::Foreground)
        .fixed_pos(anchor)
        .constrain(true)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).inner_margin(egui::Margin::symmetric(0, 4)).show(ui, |ui| {
                ui.set_width(width);
                if items.is_empty() {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.add_space(12.0);
                        ui.label(
                            egui::RichText::new(pick("No matching notes", "Ninguna nota coincide"))
                                .italics()
                                .color(c.text_muted),
                        );
                    });
                    ui.add_space(4.0);
                    return;
                }
                egui::ScrollArea::vertical().max_height(288.0).show(ui, |ui| {
                    for (k, item) in items.iter().enumerate() {
                        let h = if item.detail.is_empty() { 28.0 } else { 40.0 };
                        let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, h), egui::Sense::click());
                        if resp.hovered() && ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
                            st.sel = k;
                        }
                        let on = k == st.sel;
                        if on {
                            ui.painter().rect_filled(rect.shrink2(egui::vec2(4.0, 0.0)), 4.0, c.selection_inactive);
                            if resp.rect.is_positive() && !ui.clip_rect().contains_rect(rect) {
                                resp.scroll_to_me(None);
                            }
                        }
                        let ink = if on { c.accent } else { c.text };
                        let font = iurefficient_ui::theme::body_font(ui.ctx());
                        ui.painter().text(
                            egui::pos2(rect.left() + 12.0, rect.top() + 6.0),
                            egui::Align2::LEFT_TOP,
                            &item.label,
                            font.clone(),
                            ink,
                        );
                        if !item.detail.is_empty() {
                            ui.painter().text(
                                egui::pos2(rect.left() + 12.0, rect.top() + 22.0),
                                egui::Align2::LEFT_TOP,
                                &item.detail,
                                egui::FontId::new(font.size * 0.8, font.family),
                                c.text_muted,
                            );
                        }
                        resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, on, &item.label));
                        if resp.clicked() {
                            chosen = Some(apply(text, &trig, caret, item));
                        }
                    }
                });
            });
        });
    st.rect = Some(area.response.rect);
    st.open = chosen.is_none();
    if chosen.is_some() {
        st.sel = 0;
    }
    ui.data_mut(|d| d.insert_temp(state_id(id), st));
    // El clic se llevó el foco: vuelve al campo.
    if chosen.is_some() {
        ui.memory_mut(|m| m.request_focus(id));
    }
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Notes;
    impl NoteIndex for Notes {
        fn notes(&self) -> Vec<String> {
            vec!["Acta de junta".into(), "Clientes/Altamira".into(), "Viejo/Altamira".into(), "Contrato".into()]
        }
        fn headings(&self, note: &str) -> Vec<String> {
            if note == "Contrato" { vec!["Partes".into(), "Pagos".into()] } else { Vec::new() }
        }
    }

    #[test]
    fn triggers_need_a_space_or_the_line_start() {
        let t = |s: &str| detect(s, s.chars().count());
        assert_eq!(t("/tab").map(|x| (x.kind, x.start, x.query)), Some((Kind::Slash, 0, "tab".into())));
        assert_eq!(t("hola /men").map(|x| (x.start, x.query)), Some((5, "men".into())));
        assert_eq!(t("https://x.mx"), None, "una URL no abre el menú");
        assert_eq!(t("a/b"), None);
        assert_eq!(t("/dos palabras"), None, "un espacio lo cierra");
        assert_eq!(t("ver [[Acta de"), Some(Trigger { kind: Kind::Wiki, start: 4, query: "Acta de".into() }));
        assert_eq!(t("ver [[Acta]] y"), None, "ya cerrado");
        assert_eq!(t("```\n/no"), None, "en código no");
        assert_eq!(t("uno\n/ta").map(|x| x.start), Some(4));
    }

    #[test]
    fn slash_filters_by_label_and_keywords() {
        iurefficient_ui::lang::set_lang(iurefficient_ui::lang::Lang::Es);
        let labels = |q: &str| slash_items(q).into_iter().map(|i| i.label).collect::<Vec<_>>();
        assert_eq!(labels("mental"), ["Mapa mental"]);
        assert!(labels("diagram").contains(&"Diagrama (mermaid)".to_string()));
        assert_eq!(labels("tabla"), ["Tabla"]);
        assert_eq!(labels("").len(), 18);
        assert!(labels("zzz").is_empty());
    }

    #[test]
    fn slash_actions() {
        iurefficient_ui::lang::set_lang(iurefficient_ui::lang::Lang::Es);
        let go = |text: &str, label: &str| {
            let caret = text.chars().count();
            let trig = detect(text, caret).unwrap();
            let item = slash_items(&trig.query).into_iter().find(|i| i.label == label).unwrap();
            apply(text, &trig, caret, &item)
        };
        assert_eq!(go("Acuerdos /tit", "Título 2"), ("## Acuerdos ".into(), 12));
        assert_eq!(go("/cita", "Cita"), ("> ".into(), 2));
        let (t, c) = go("/cod", "Bloque de código");
        assert_eq!((t.as_str(), c), ("```\n\n```", 4));
        let (t, _) = go("texto /tabla", "Tabla");
        assert!(t.starts_with("texto \n\n| Columna 1 |"), "{t}");
        let (t, _) = go("/mental", "Mapa mental");
        assert!(t.starts_with("> [!MINDMAP] Tema\n> - "), "{t}");
    }

    #[test]
    fn wiki_links_rank_notes_and_offer_to_create() {
        iurefficient_ui::lang::set_lang(iurefficient_ui::lang::Lang::Es);
        let items = wiki_items("alta", Some(&Notes));
        let labels: Vec<_> = items.iter().map(|i| (i.label.as_str(), i.detail.as_str())).collect();
        assert_eq!(labels.first(), Some(&("Altamira", "Clientes/Altamira")));
        // Dos «Altamira»: el enlace lleva la ruta.
        assert_eq!(items.first().map(|i| i.action.clone()), Some(Action::Link("Clientes/Altamira".into())));
        assert_eq!(items.last().map(|i| i.label.as_str()), Some("Crear nota «alta»"));
        let acta = wiki_items("acta de junta", Some(&Notes));
        assert!(acta.iter().all(|i| !i.label.starts_with("Crear")), "existe: no se ofrece crearla");
        assert_eq!(acta.first().map(|i| i.action.clone()), Some(Action::Link("Acta de junta".into())));
        let heads = wiki_items("Contrato#pa", Some(&Notes));
        assert_eq!(heads.first().map(|i| i.action.clone()), Some(Action::Heading("Contrato".into(), "Pagos".into())));
        let text = "ver [[Contrato#pa";
        let trig = detect(text, text.chars().count()).unwrap();
        let (t, c) = apply(text, &trig, text.chars().count(), heads.first().unwrap());
        assert_eq!((t.as_str(), c), ("ver [[Contrato#Pagos]]", 22));
        let text = "ver [[Con]] y";
        let trig = detect(text, 9).unwrap();
        let (t, c) = apply(
            text,
            &trig,
            9,
            &Item { label: String::new(), detail: String::new(), action: Action::Link("Contrato".into()) },
        );
        assert_eq!((t.as_str(), c), ("ver [[Contrato]] y", 16), "no duplica los ]] ya escritos");
    }

    #[test]
    fn fuzzy_prefers_substrings_and_word_starts() {
        assert!(fuzzy("alt", "Altamira").unwrap() > fuzzy("alt", "Palta").unwrap());
        assert!(fuzzy("acj", "Acta de junta").is_some(), "subsecuencia");
        assert!(fuzzy("xyz", "Acta").is_none());
        assert!(fuzzy("ACTÁ", "acta").is_some(), "sin mayúsculas ni acentos");
    }
}
