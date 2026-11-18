//! S1.12 con egui_kittest: las teclas del editor sobre un texto real.

use egui::text::{CCursor, CCursorRange};
use egui::text_edit::TextEditState;
use egui::{Id, Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

struct State {
    text: String,
}

fn harness(text: &str) -> Harness<'static, State> {
    Harness::builder().with_size(egui::vec2(600.0, 400.0)).build_ui_state(
        |ui, st: &mut State| {
            let out = iure_doc_ui::show(ui, Id::unique("ed"), &mut st.text, "Escribe");
            out.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::MultilineTextInput, true, "editor"));
        },
        State { text: text.to_string() },
    )
}

/// Enfoca el editor y pone el cursor en `at` (índice de carácter).
fn cursor_at(h: &mut Harness<'static, State>, at: usize) {
    h.get_by_label("editor").focus();
    h.run();
    let ctx = h.ctx.clone();
    let id = Id::unique("ed");
    let mut st = TextEditState::load(&ctx, id).unwrap_or_default();
    st.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(at))));
    st.store(&ctx, id);
    h.run();
}

#[test]
fn enter_continues_a_list_and_leaves_it_when_empty() {
    let mut h = harness("- uno");
    cursor_at(&mut h, 5);
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(h.state().text, "- uno\n- ");
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(h.state().text, "- uno\n", "Intro en un elemento vacío sale de la lista");
}

#[test]
fn tab_indents_and_ctrl_b_bolds() {
    let mut h = harness("- uno\n- dos");
    cursor_at(&mut h, 11);
    h.key_press(Key::Tab);
    h.run();
    assert_eq!(h.state().text, "- uno\n  - dos");
    h.key_press_modifiers(Modifiers::SHIFT, Key::Tab);
    h.run();
    assert_eq!(h.state().text, "- uno\n- dos");
    // Selección «dos» y Ctrl+B.
    let ctx = h.ctx.clone();
    let id = Id::unique("ed");
    let mut st = TextEditState::load(&ctx, id).unwrap_or_default();
    st.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(8), CCursor::new(11))));
    st.store(&ctx, id);
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::B);
    h.run();
    assert_eq!(h.state().text, "- uno\n- **dos**");
}

#[test]
fn plain_enter_outside_lists_is_a_newline() {
    let mut h = harness("párrafo");
    cursor_at(&mut h, 7);
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(h.state().text, "párrafo\n");
}

// --- Editor por bloques ---

fn blocks_harness(text: &str) -> Harness<'static, State> {
    Harness::builder().with_size(egui::vec2(600.0, 400.0)).build_ui_state(
        |ui, st: &mut State| {
            let out = iure_doc_ui::show_blocks(ui, Id::unique("bl"), &mut st.text, "Escribe");
            out.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::MultilineTextInput, true, "bloques"));
        },
        State { text: text.to_string() },
    )
}

/// Enfoca el primer bloque (o el que tenga el foco) y pone el cursor en `at`.
fn block_cursor_at(h: &mut Harness<'static, State>, at: usize) {
    if h.ctx.memory(|m| m.focused()).is_none() {
        h.get_by_label("bloques").focus();
        h.run();
    }
    let ctx = h.ctx.clone();
    let Some(id) = ctx.memory(|m| m.focused()) else { panic!("sin foco") };
    let mut st = TextEditState::load(&ctx, id).unwrap_or_default();
    st.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(at))));
    st.store(&ctx, id);
    h.run();
}

#[test]
fn blocks_open_untouched() {
    let t = "# Título\n\nUno\ndos  \n\n- a\n- b\n\n```\nx\n\ny\n```\n";
    let mut h = blocks_harness(t);
    h.run_steps(3);
    assert_eq!(h.state().text, t);
}

#[test]
fn enter_at_the_end_of_a_block_opens_a_new_one() {
    let mut h = blocks_harness("uno\n\ntres\n");
    block_cursor_at(&mut h, 3);
    h.key_press(Key::Enter);
    h.run_steps(2);
    assert_eq!(h.state().text, "uno\n\ntres\n", "vacío no cambia el documento");
    h.get_by_label("bloques").type_text("dos");
    h.run_steps(2);
    assert_eq!(h.state().text, "uno\n\ndos\n\ntres\n");
}

#[test]
fn backspace_at_the_start_joins_with_the_previous_block() {
    let mut h = blocks_harness("uno\n\ndos\n");
    block_cursor_at(&mut h, 3);
    h.key_press(Key::ArrowDown);
    h.run_steps(2);
    h.key_press(Key::Backspace);
    h.run_steps(2);
    assert_eq!(h.state().text, "unodos\n");
    // El cursor quedó en la unión.
    h.get_by_label("bloques").type_text("-");
    h.run_steps(2);
    assert_eq!(h.state().text, "uno-dos\n");
}

#[test]
fn enter_inside_a_list_block_continues_the_list() {
    let mut h = blocks_harness("- uno\n\nfin\n");
    block_cursor_at(&mut h, 5);
    h.key_press(Key::Enter);
    h.run_steps(2);
    assert_eq!(h.state().text, "- uno\n- \n\nfin\n");
}

#[test]
fn undo_and_redo_cover_the_whole_document() {
    let mut h = blocks_harness("uno\n\ntres\n");
    block_cursor_at(&mut h, 3);
    h.key_press(Key::Enter);
    h.run_steps(2);
    h.get_by_label("bloques").type_text("dos");
    h.run_steps(2);
    assert_eq!(h.state().text, "uno\n\ndos\n\ntres\n");
    // Ctrl+Z deshace «dos» aunque vive en un bloque que se creó después.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    assert_eq!(h.state().text, "uno\n\ntres\n");
    h.key_press_modifiers(Modifiers { shift: true, ..Modifiers::COMMAND }, Key::Z);
    h.run_steps(2);
    assert_eq!(h.state().text, "uno\n\ndos\n\ntres\n");
}

#[test]
fn undo_reverts_joining_blocks() {
    let mut h = blocks_harness("uno\n\ndos\n");
    block_cursor_at(&mut h, 3);
    h.key_press(Key::ArrowDown);
    h.run_steps(2);
    h.key_press(Key::Backspace);
    h.run_steps(2);
    assert_eq!(h.state().text, "unodos\n");
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    assert_eq!(h.state().text, "uno\n\ndos\n");
    // Y se sigue escribiendo donde quedó el cursor.
    h.get_by_label("bloques").type_text("!");
    h.run_steps(2);
    assert!(h.state().text.contains('!'), "{:?}", h.state().text);
}

#[test]
fn ctrl_a_selects_the_whole_document_and_typing_replaces_it() {
    let mut h = blocks_harness("uno\n\ndos\n\ntres\n");
    block_cursor_at(&mut h, 1);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(2);
    h.event(egui::Event::Text("x".into()));
    h.run_steps(2);
    assert_eq!(h.state().text, "x");
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    assert_eq!(h.state().text, "uno\n\ndos\n\ntres\n");
}

#[test]
fn shift_arrows_select_across_blocks() {
    let mut h = blocks_harness("uno\n\ndos\n");
    block_cursor_at(&mut h, 1);
    // De «u|no» al inicio de «dos», y dos más: «no\n\ndo».
    h.key_press_modifiers(Modifiers::SHIFT, Key::ArrowDown);
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::SHIFT, Key::ArrowRight);
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::SHIFT, Key::ArrowRight);
    h.run_steps(2);
    h.key_press(Key::Backspace);
    h.run_steps(2);
    assert_eq!(h.state().text, "us\n");
    // El cursor quedó donde estaba la selección.
    h.event(egui::Event::Text("-".into()));
    h.run_steps(2);
    assert_eq!(h.state().text, "u-s\n");
}

#[test]
fn cut_copies_the_selection_and_escape_cancels_it() {
    let mut h = blocks_harness("uno\n\ndos\n");
    block_cursor_at(&mut h, 0);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(2);
    h.key_press(Key::Escape);
    h.run_steps(2);
    h.event(egui::Event::Text("¡".into()));
    h.run_steps(2);
    assert!(h.state().text.contains('¡') && h.state().text.contains("dos"), "{:?}", h.state().text);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(2);
    h.event(egui::Event::Cut);
    h.step();
    let copied = h
        .output()
        .platform_output
        .commands
        .iter()
        .any(|c| matches!(c, egui::OutputCommand::CopyText(t) if t.contains("uno\n\ndos")));
    h.run();
    assert_eq!(h.state().text, "");
    assert!(copied, "lo cortado va al portapapeles");
}

// --- Tablas en su lugar ---

const TABLE_DOC: &str = "Antes\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\nDespués\n";

#[test]
fn tables_show_as_cells_and_stay_untouched_until_edited() {
    let mut h = blocks_harness(TABLE_DOC);
    h.run_steps(3);
    assert!(h.query_by_label("Fila 2, columna 2").is_some(), "la tabla es una cuadrícula");
    assert_eq!(h.state().text, TABLE_DOC, "sin tocarla no se reescribe");
    h.get_by_label("Fila 2, columna 2").focus();
    h.run_steps(2);
    h.get_by_label("Fila 2, columna 2").type_text("0");
    h.run_steps(2);
    assert_eq!(h.state().text, "Antes\n\n| a   | b   |\n| --- | --- |\n| 1   | 20  |\n\nDespués\n");
}

#[test]
fn tab_in_the_last_cell_adds_a_row_and_down_leaves_the_table() {
    let mut h = blocks_harness(TABLE_DOC);
    h.run_steps(3);
    h.get_by_label("Fila 2, columna 2").focus();
    h.run_steps(2);
    h.key_press(Key::Tab);
    h.run_steps(3);
    assert!(h.state().text.contains("| 1   | 2   |\n|     |     |"), "{:?}", h.state().text);
    // La celda nueva tiene el foco: se escribe en ella.
    h.event(egui::Event::Text("3".into()));
    h.run_steps(2);
    assert!(h.state().text.contains("| 3   |     |"), "{:?}", h.state().text);
    // Abajo desde la última fila: al bloque siguiente.
    h.key_press(Key::ArrowDown);
    h.run_steps(3);
    h.event(egui::Event::Text("¡".into()));
    h.run_steps(2);
    assert!(h.state().text.ends_with("¡Después\n"), "{:?}", h.state().text);
}

#[test]
fn the_toolbar_adds_columns_and_shows_the_markdown() {
    let mut h = blocks_harness(TABLE_DOC);
    h.run_steps(3);
    h.get_by_label("Fila 1, columna 1").focus();
    h.run_steps(2);
    h.get_by_label("+ Columna").click();
    h.run_steps(3);
    assert!(h.state().text.contains("| a   |     | b   |"), "{:?}", h.state().text);
    h.get_by_label("Markdown").click();
    h.run_steps(3);
    assert!(h.query_by_label("Fila 1, columna 1").is_none(), "se edita como texto");
}

// --- Diagramas en su lugar ---

struct FakeDiagrams;

impl iure_doc_ui::DiagramRenderer for FakeDiagrams {
    fn accepts(&self, block: &str) -> bool {
        block.starts_with("```mermaid") && block.trim_end().ends_with("```")
    }
    fn key(&self) -> u64 {
        1
    }
    fn render(&self, block: &str, _scale: f32) -> Result<iure_doc_ui::DiagramImage, String> {
        if block.contains("mal") {
            return Err("La línea 2 no se entiende.".into());
        }
        Ok(iure_doc_ui::DiagramImage { width: 20, height: 10, rgba: vec![200; 20 * 10 * 4], points: [200.0, 100.0] })
    }
}

const DIAGRAM_DOC: &str = "Antes\n\n```mermaid\nflowchart LR\n  A-->B\n```\n\nDespués\n";

fn diagram_harness(text: &str) -> Harness<'static, State> {
    let r: std::sync::Arc<dyn iure_doc_ui::DiagramRenderer> = std::sync::Arc::new(FakeDiagrams);
    Harness::builder().with_size(egui::vec2(600.0, 400.0)).build_ui_state(
        move |ui, st: &mut State| {
            let out = iure_doc_ui::show_blocks_with(ui, Id::unique("dg"), &mut st.text, "Escribe", Some(r.clone()));
            out.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::MultilineTextInput, true, "bloques"));
        },
        State { text: text.to_string() },
    )
}

/// Corre cuadros hasta que aparece `label` (el dibujo llega de otro hilo).
fn wait_for(h: &mut Harness<'static, State>, label: &str) -> bool {
    for _ in 0..200 {
        h.step();
        if h.query_by_label(label).is_some() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    false
}

#[test]
fn diagrams_are_drawn_in_place_and_a_click_edits_the_code() {
    let mut h = diagram_harness(DIAGRAM_DOC);
    assert!(wait_for(&mut h, "Diagrama 2"), "la imagen en el lugar del bloque");
    assert_eq!(h.state().text, DIAGRAM_DOC);
    h.get_by_label("Diagrama 2").click();
    h.run_steps(3);
    assert!(h.query_by_label("Diagrama 2").is_none(), "se ve el código");
    h.get_by_label("bloques").type_text("%% ");
    h.run_steps(2);
    assert_eq!(
        h.state().text,
        "Antes\n\n%% ```mermaid\nflowchart LR\n  A-->B\n```\n\nDespués\n",
        "el cursor al inicio del código"
    );
}

#[test]
fn a_diagram_with_an_error_shows_the_code_and_what_fails() {
    let mut h = diagram_harness("```mermaid\nflowchart LR\n  mal\n```\n");
    assert!(wait_for(&mut h, "La línea 2 no se entiende."), "el error en palabras");
    assert!(h.query_by_label("Diagrama 1").is_none());
}

// --- Imágenes en su lugar ---

fn images_harness(text: &str, base: std::path::PathBuf) -> Harness<'static, State> {
    let r: std::sync::Arc<dyn iure_doc_ui::DiagramRenderer> = std::sync::Arc::new(iure_doc_ui::Renderers(vec![
        std::sync::Arc::new(FakeDiagrams),
        std::sync::Arc::new(iure_doc_ui::LocalImages::new(Some(base))),
    ]));
    Harness::builder().with_size(egui::vec2(600.0, 500.0)).build_ui_state(
        move |ui, st: &mut State| {
            let out = iure_doc_ui::show_blocks_with(ui, Id::unique("img"), &mut st.text, "Escribe", Some(r.clone()));
            out.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::MultilineTextInput, true, "bloques"));
        },
        State { text: text.to_string() },
    )
}

#[test]
fn local_images_are_shown_in_place_and_remote_ones_are_explained() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("img")).unwrap();
    image::RgbaImage::from_pixel(40, 20, image::Rgba([0, 90, 180, 255]))
        .save(dir.path().join("img/sello.png"))
        .unwrap();
    let doc = "Antes\n\n![Sello del despacho](img/sello.png)\n\n![Logo](https://x.mx/logo.png)\n\n```mermaid\nA\n```\n";
    let mut h = images_harness(doc, dir.path().to_path_buf());
    assert!(wait_for(&mut h, "Imagen 2: Sello del despacho"), "la imagen con su texto alternativo");
    assert!(wait_for(&mut h, "Diagrama 4"), "los diagramas siguen con el otro dibujante");
    assert!(wait_for(&mut h, "Las imágenes de internet no se descargan aquí."));
    assert_eq!(h.state().text, doc);
    h.get_by_label("Imagen 2: Sello del despacho").click();
    h.run_steps(3);
    assert!(h.query_by_label("Imagen 2: Sello del despacho").is_none(), "se edita el Markdown");
}

/// Regresión: con una imagen como primer bloque, el anfitrión renombraba la imagen al nombrar la
/// respuesta del editor (perdía «Imagen 1: …» para lectores de pantalla).
#[test]
fn an_image_as_the_first_block_keeps_its_name() {
    let dir = tempfile::tempdir().unwrap();
    image::RgbaImage::from_pixel(8, 8, image::Rgba([0, 90, 180, 255])).save(dir.path().join("p.png")).unwrap();
    let mut h = images_harness("![Punto](p.png)\n", dir.path().to_path_buf());
    assert!(wait_for(&mut h, "Imagen 1: Punto"));
}
