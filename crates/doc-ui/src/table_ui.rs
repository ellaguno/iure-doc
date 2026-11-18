//! S1.12: tablas dibujadas en su lugar dentro del editor por bloques.
//!
//! Una tabla de Markdown se ve como cuadrícula: cada celda es un campo con su texto (Markdown en
//! línea). Tab / Mayús+Tab pasan de celda en celda (Tab en la última agrega una fila), Intro baja
//! (y agrega una fila al final), las flechas arriba/abajo cambian de fila y salen de la tabla. Con
//! una celda activa, una barra pequeña agrega o quita filas y columnas, alinea la columna o muestra
//! el Markdown de la tabla. Solo una tabla que se cambia se vuelve a escribir.

use egui::{FontId, Id, Key, Modifiers, Ui};
use iure_doc::Align;
use iure_doc::table::Grid;
use iurefficient_ui::lang::pick;

use crate::EditorStyle;

/// Qué pidió la tabla este cuadro.
pub(crate) enum TableAct {
    /// La tabla cambió: su Markdown nuevo.
    Changed(String),
    /// Salir de la tabla hacia arriba (`false`) o hacia abajo (`true`).
    Leave(bool),
    /// Ver y editar el Markdown de la tabla.
    Raw,
}

pub(crate) struct TableOut {
    pub act: Option<TableAct>,
    /// La celda con el foco (fila, columna).
    pub focused: Option<(usize, usize)>,
    /// Celda que debe tomar el foco el cuadro siguiente.
    pub focus_next: Option<(usize, usize)>,
    pub rect: egui::Rect,
}

fn cell_id(base: Id, r: usize, c: usize) -> Id {
    base.with(("cell", r, c))
}

/// Ancho de una columna según su texto más largo.
fn col_width(grid: &Grid, c: usize, size: f32) -> f32 {
    let chars = grid.rows.iter().map(|r| r.get(c).map_or(0, |x| x.chars().count())).max().unwrap_or(0).min(40);
    (chars as f32 * size * 0.56 + 14.0).clamp(56.0, 360.0)
}

/// Teclas de la celda con el foco (antes de que las vea su campo).
fn cell_keys(
    ui: &Ui,
    grid: &mut Grid,
    (r, c): (usize, usize),
    changed: &mut bool,
) -> Option<Result<(usize, usize), bool>> {
    let rows = grid.rows.len();
    let cols = grid.cols();
    let last_row = r + 1 >= rows;
    let (tab, back, enter, up, down) = ui.input_mut(|i| {
        let back = i.consume_key(Modifiers::SHIFT, Key::Tab);
        let tab = !back && i.consume_key(Modifiers::NONE, Key::Tab);
        let enter = i.consume_key(Modifiers::NONE, Key::Enter);
        let up = i.consume_key(Modifiers::NONE, Key::ArrowUp);
        let down = i.consume_key(Modifiers::NONE, Key::ArrowDown);
        (tab, back, enter, up, down)
    });
    if tab {
        if c + 1 < cols {
            return Some(Ok((r, c + 1)));
        }
        if last_row {
            grid.insert_row(rows);
            *changed = true;
        }
        return Some(Ok((r + 1, 0)));
    }
    if back {
        return match (r, c) {
            (0, 0) => None,
            (r, 0) => Some(Ok((r - 1, cols.saturating_sub(1)))),
            (r, c) => Some(Ok((r, c - 1))),
        };
    }
    if enter {
        if last_row {
            grid.insert_row(rows);
            *changed = true;
        }
        return Some(Ok((r + 1, c)));
    }
    if up {
        return Some(if r == 0 { Err(false) } else { Ok((r - 1, c)) });
    }
    if down {
        return Some(if last_row { Err(true) } else { Ok((r + 1, c)) });
    }
    None
}

/// Barra de la tabla con la celda activa `(r, c)`.
fn toolbar(
    ui: &mut Ui,
    grid: &mut Grid,
    (r, c): (usize, usize),
    changed: &mut bool,
    raw: &mut bool,
) -> Option<(usize, usize)> {
    let mut next = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let small = |ui: &mut Ui, label: &str, tip: &str| ui.small_button(label).on_hover_text(tip).clicked();
        if small(ui, pick("+ Row", "+ Fila"), pick("Add a row below", "Agregar una fila debajo")) {
            grid.insert_row(r + 1);
            *changed = true;
            next = Some((r + 1, c));
        }
        if r >= 1 && small(ui, pick("− Row", "− Fila"), pick("Remove this row", "Quitar esta fila")) {
            grid.remove_row(r);
            *changed = true;
            next = Some((r.min(grid.rows.len().saturating_sub(1)), c));
        }
        if small(
            ui,
            pick("+ Column", "+ Columna"),
            pick("Add a column to the right", "Agregar una columna a la derecha"),
        ) {
            grid.insert_col(c + 1);
            *changed = true;
            next = Some((r, c + 1));
        }
        if grid.cols() > 1
            && small(ui, pick("− Column", "− Columna"), pick("Remove this column", "Quitar esta columna"))
        {
            grid.remove_col(c);
            *changed = true;
            next = Some((r, c.min(grid.cols().saturating_sub(1))));
        }
        ui.separator();
        let cur = grid.align.get(c).copied().unwrap_or_default();
        for (a, label, tip) in [
            (Align::Left, pick("Left", "Izq."), pick("Align the column left", "Alinear la columna a la izquierda")),
            (Align::Center, pick("Center", "Centro"), pick("Center the column", "Centrar la columna")),
            (Align::Right, pick("Right", "Der."), pick("Align the column right", "Alinear la columna a la derecha")),
        ] {
            if ui.selectable_label(cur == a, label).on_hover_text(tip).clicked() {
                // Volver a pulsar la alineación activa la quita.
                if let Some(x) = grid.align.get_mut(c) {
                    *x = if cur == a { Align::None } else { a };
                }
                *changed = true;
                next = Some((r, c));
            }
        }
        ui.separator();
        if small(ui, "Markdown", pick("Edit the table as Markdown", "Editar la tabla como Markdown")) {
            *raw = true;
        }
    });
    next
}

/// Dibuja la tabla. `focus_to`: celda que toma el foco este cuadro; `bar`: mostrar la barra con
/// esa celda activa.
pub(crate) fn show(
    ui: &mut Ui,
    base: Id,
    grid: &Grid,
    style: &EditorStyle,
    focus_to: Option<(usize, usize)>,
    bar: Option<(usize, usize)>,
) -> TableOut {
    let mut g = grid.clone();
    let mut changed = false;
    let mut raw = false;
    let mut act = None;
    let mut focus_next = None;
    let rows = g.rows.len();
    let cols = g.cols();
    let mut focused = None;
    'find: for r in 0..rows {
        for c in 0..cols {
            if ui.memory(|m| m.has_focus(cell_id(base, r, c))) {
                focused = Some((r, c));
                break 'find;
            }
        }
    }
    if let Some(rc) = focused {
        match cell_keys(ui, &mut g, rc, &mut changed) {
            Some(Ok(to)) => focus_next = Some(to),
            Some(Err(down)) => act = Some(TableAct::Leave(down)),
            None => {}
        }
    }
    let top = ui.cursor().min;
    if let Some(rc) = focused.or(bar)
        && let Some(n) = toolbar(ui, &mut g, rc, &mut changed, &mut raw)
    {
        focus_next = Some(n);
    }
    let widths: Vec<f32> = (0..cols).map(|c| col_width(&g, c, style.size)).collect();
    let border = ui.visuals().widgets.noninteractive.bg_stroke;
    let body = FontId::new(style.size, style.body_family.clone());
    let head = FontId::new(style.size, style.bold_family.clone());
    egui::ScrollArea::horizontal().id_salt(base.with("hscroll")).auto_shrink([false, true]).show(ui, |ui| {
        // Los bordes de celdas vecinas se enciman (una línea, no dos).
        ui.spacing_mut().item_spacing = egui::vec2(-border.width, -border.width);
        for r in 0..rows {
            ui.horizontal(|ui| {
                for (c, w) in widths.iter().enumerate() {
                    let id = cell_id(base, r, c);
                    let align = match g.align.get(c).copied().unwrap_or_default() {
                        Align::Center => egui::Align2::CENTER_CENTER,
                        Align::Right => egui::Align2::RIGHT_CENTER,
                        _ => egui::Align2::LEFT_CENTER,
                    };
                    let Some(text) = g.rows.get_mut(r).and_then(|row| row.get_mut(c)) else { continue };
                    let frame = egui::Frame::new()
                        .inner_margin(egui::Margin::symmetric(6, 3))
                        .fill(if r == 0 { style.code_bg } else { egui::Color32::TRANSPARENT })
                        .stroke(border);
                    frame.show(ui, |ui| {
                        let out = egui::TextEdit::singleline(text)
                            .id(id)
                            .frame(egui::Frame::NONE)
                            .font(if r == 0 { head.clone() } else { body.clone() })
                            .align(align)
                            .desired_width(*w)
                            .show(ui);
                        let name =
                            format!("{} {}, {} {}", pick("Row", "Fila"), r + 1, pick("column", "columna"), c + 1);
                        out.response
                            .response
                            .widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, name.clone()));
                        if out.response.response.changed() {
                            changed = true;
                        }
                        if focus_to == Some((r, c)) {
                            ui.memory_mut(|m| m.request_focus(id));
                            let mut st = out.state;
                            let end = text.chars().count();
                            st.cursor
                                .set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(end))));
                            st.store(ui.ctx(), id);
                        }
                    });
                }
            });
        }
    });
    let rect = egui::Rect::from_min_max(top, egui::pos2(ui.max_rect().right(), ui.cursor().min.y));
    if raw {
        act = Some(TableAct::Raw);
    } else if changed && act.is_none() {
        act = Some(TableAct::Changed(g.to_markdown()));
    }
    // Una celda que ya no existe (se quitó su fila o columna) no se enfoca.
    let focus_next = focus_next.filter(|&(r, c)| r < g.rows.len() && c < g.cols());
    TableOut { act, focused, focus_next, rect }
}
