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

// --- Pegar HTML como Markdown ---

struct FakePaste(&'static str);

impl iure_doc_ui::PasteSource for FakePaste {
    fn html(&self) -> Option<String> {
        Some(self.0.to_string())
    }
}

fn paste_harness(text: &str, html: &'static str) -> Harness<'static, State> {
    let hooks = iure_doc_ui::Hooks { paste: Some(std::sync::Arc::new(FakePaste(html))), ..Default::default() };
    Harness::builder().with_size(egui::vec2(600.0, 400.0)).build_ui_state(
        move |ui, st: &mut State| {
            let out = iure_doc_ui::show_blocks_hooks(ui, Id::unique("pg"), &mut st.text, "Escribe", hooks.clone());
            out.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::MultilineTextInput, true, "bloques"));
        },
        State { text: text.to_string() },
    )
}

#[test]
fn pasting_html_pastes_its_markdown() {
    let mut h = paste_harness("Antes\n", "<h2>Acuerdos</h2><ul><li><b>uno</b></li><li>dos</li></ul>");
    block_cursor_at(&mut h, 5);
    h.event(egui::Event::Paste("Acuerdos\nuno\ndos".into()));
    h.run_steps(3);
    assert_eq!(h.state().text, "Antes\n\n## Acuerdos\n\n- **uno**\n- dos\n", "los bloques, en bloques propios");
}

struct StalePaste;

impl iure_doc_ui::PasteSource for StalePaste {
    fn html(&self) -> Option<String> {
        Some("<h1>Copia vieja</h1>".into())
    }
    fn text(&self) -> Option<String> {
        Some("Copia vieja".into())
    }
}

/// Regresión: en GNOME el HTML del portapapeles podía ser de otra copia (el de X11) y se pegaba
/// eso en vez de lo cortado en el editor.
#[test]
fn a_stale_clipboard_html_does_not_replace_the_pasted_text() {
    let hooks = iure_doc_ui::Hooks { paste: Some(std::sync::Arc::new(StalePaste)), ..Default::default() };
    let mut h = Harness::builder().with_size(egui::vec2(600.0, 400.0)).build_ui_state(
        move |ui, st: &mut State| {
            let out = iure_doc_ui::show_blocks_hooks(ui, Id::unique("pg"), &mut st.text, "Escribe", hooks.clone());
            out.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::MultilineTextInput, true, "bloques"));
        },
        State { text: "ab\n".to_string() },
    );
    block_cursor_at(&mut h, 1);
    h.event(egui::Event::Paste("cortado".into()));
    h.run_steps(3);
    assert_eq!(h.state().text, "acortadob\n");
}

#[test]
fn inline_html_is_pasted_inline() {
    let mut h = paste_harness("ab\n", "<span>hola <b>mundo</b></span>");
    block_cursor_at(&mut h, 1);
    h.event(egui::Event::Paste("hola mundo".into()));
    h.run_steps(3);
    assert_eq!(h.state().text, "ahola **mundo**b\n");
}

#[test]
fn inside_a_code_block_the_plain_text_is_pasted() {
    let mut h = paste_harness("```\nx\n```\n", "<p><b>no</b></p>");
    block_cursor_at(&mut h, 5);
    h.event(egui::Event::Paste("plano".into()));
    h.run_steps(3);
    assert_eq!(h.state().text, "```\nxplano\n```\n");
}

// --- Corrector ---

fn spell_harness(text: &str) -> (Harness<'static, State>, std::sync::Arc<iure_doc_ui::Speller>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("es_MX.aff"), "SET UTF-8\nTRY esianrtolcdugmphbyfvkwz\n").unwrap();
    std::fs::write(dir.path().join("es_MX.dic"), "2\nhola\nmundo\n").unwrap();
    let sp = iure_doc_ui::Speller::new(&["es"], vec![dir.path().to_path_buf()], Some(dir.path().join("personal.txt")));
    for _ in 0..500 {
        if sp.is_ready() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let hooks = iure_doc_ui::Hooks { spell: Some(sp.clone()), ..Default::default() };
    let h = Harness::builder().with_size(egui::vec2(600.0, 400.0)).build_ui_state(
        move |ui, st: &mut State| {
            let out = iure_doc_ui::show_blocks_hooks(ui, Id::unique("sp"), &mut st.text, "Escribe", hooks.clone());
            out.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::MultilineTextInput, true, "bloques"));
        },
        State { text: text.to_string() },
    );
    (h, sp, dir)
}

#[test]
fn right_click_on_a_misspelled_word_offers_suggestions() {
    let (mut h, _sp, _dir) = spell_harness("holla\n");
    h.run_steps(2);
    h.get_by_label("bloques").click_secondary();
    h.run_steps(2);
    h.get_by_label("hola").click();
    h.run_steps(3);
    assert_eq!(h.state().text, "hola\n");
}

#[test]
fn adding_a_word_to_the_dictionary_stops_marking_it() {
    let (mut h, sp, dir) = spell_harness("Altamira\n");
    h.run_steps(2);
    h.get_by_label("bloques").click_secondary();
    h.run_steps(2);
    h.get_by_label("Agregar al diccionario «Altamira»").click();
    h.run_steps(2);
    assert!(sp.check("Altamira"));
    assert_eq!(std::fs::read_to_string(dir.path().join("personal.txt")).unwrap(), "altamira\n");
    assert_eq!(h.state().text, "Altamira\n", "el texto no cambia");
}

// --- Reglas de entrada (S1.22) ---

/// Regresión: Intro tras abrir una cerca (```` ```rust ````) se perdía (el bloque quitaba los
/// saltos del final y el cursor volvía atrás): no se podía escribir un bloque de código nuevo.
#[test]
fn a_new_code_block_can_be_typed() {
    let mut h = blocks_harness("Antes\n\n```rust");
    h.get_by_label("bloques").focus();
    h.run();
    // Al final del documento (la cerca).
    h.key_press(Key::ArrowDown);
    h.run_steps(2);
    h.key_press(Key::End);
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(2);
    h.event(egui::Event::Text("fn main() {}".into()));
    h.run_steps(2);
    assert_eq!(h.state().text, "Antes\n\n```rust\nfn main() {}");
}

// --- Pegar y soltar imágenes ---

struct ImagePaste;

impl iure_doc_ui::PasteSource for ImagePaste {
    fn html(&self) -> Option<String> {
        None
    }
    fn image(&self) -> Option<iure_doc_ui::PastedImage> {
        Some(iure_doc_ui::PastedImage { width: 4, height: 3, rgba: vec![90; 48] })
    }
}

#[derive(Debug)]
struct Dropped(std::path::PathBuf);

impl egui::DroppedFile for Dropped {
    fn path(&self) -> &std::path::Path {
        &self.0
    }
    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

fn image_harness(text: &str, dir: std::path::PathBuf) -> Harness<'static, State> {
    let hooks = iure_doc_ui::Hooks {
        paste: Some(std::sync::Arc::new(ImagePaste)),
        images: Some(std::sync::Arc::new(iure_doc_ui::FolderImages::new(Some(dir), "img"))),
        ..Default::default()
    };
    Harness::builder().with_size(egui::vec2(600.0, 400.0)).build_ui_state(
        move |ui, st: &mut State| {
            let out = iure_doc_ui::show_blocks_hooks(ui, Id::unique("imgs"), &mut st.text, "Escribe", hooks.clone());
            out.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::MultilineTextInput, true, "bloques"));
        },
        State { text: text.to_string() },
    )
}

#[test]
fn pasting_an_image_saves_it_and_inserts_its_own_block() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = image_harness("Antes\n", dir.path().to_path_buf());
    block_cursor_at(&mut h, 5);
    // Ctrl+V con solo una imagen en el portapapeles: egui no trae texto.
    h.event(egui::Event::Paste(String::new()));
    h.run_steps(3);
    let t = h.state().text.clone();
    assert!(t.starts_with("Antes\n\n![imagen](img/imagen-"), "en su propio bloque: {t:?}");
    let rel = t.split("](").nth(1).and_then(|r| r.split(')').next()).unwrap();
    assert!(rel.starts_with("img/imagen-") && dir.path().join(rel).is_file(), "{rel}");
}

#[test]
fn text_in_the_clipboard_wins_over_the_image() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = image_harness("\n", dir.path().to_path_buf());
    block_cursor_at(&mut h, 0);
    h.event(egui::Event::Paste("hola".into()));
    h.run_steps(3);
    assert_eq!(h.state().text, "hola\n");
    assert!(!dir.path().join("img").exists(), "no se guardó ninguna imagen");
}

#[test]
fn dropping_an_image_file_copies_and_inserts_it() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let src = outside.path().join("Sello del despacho.png");
    image::RgbaImage::from_pixel(5, 5, image::Rgba([0, 90, 180, 255])).save(&src).unwrap();
    let mut h = image_harness("Antes\n", dir.path().to_path_buf());
    h.run_steps(2);
    h.input_mut().dropped_files.push(std::sync::Arc::new(Dropped(src)));
    h.run_steps(4);
    assert_eq!(h.state().text, "Antes\n\n![Sello del despacho](img/Sello-del-despacho.png)\n");
    assert!(dir.path().join("img/Sello-del-despacho.png").is_file());
    // Algo que no es imagen: se explica y no se inserta.
    let txt = outside.path().join("notas.txt");
    std::fs::write(&txt, "x").unwrap();
    h.input_mut().dropped_files.push(std::sync::Arc::new(Dropped(txt)));
    h.run_steps(2);
    assert!(h.query_by_label("Aquí solo se pueden soltar imágenes.").is_some());
    assert!(!h.state().text.contains("notas"));
}
