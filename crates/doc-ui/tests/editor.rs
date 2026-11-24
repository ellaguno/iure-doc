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
        Ok(iure_doc_ui::DiagramImage {
            width: 20,
            height: 10,
            rgba: vec![200; 20 * 10 * 4],
            points: [200.0, 100.0],
            hotspots: Vec::new(),
        })
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

/// El portapapeles con HTML de otra app (una imagen de la web) y sin texto.
struct OtherAppHtml;

impl iure_doc_ui::PasteSource for OtherAppHtml {
    fn html(&self) -> Option<String> {
        Some("<img src=\"blob:https://web.example.com/1234\">".into())
    }
}

/// Regresión (2026-11-23): soltar una imagen la inserta como un pegado interno, y ese pegado se
/// cambiaba por el HTML que hubiera en el portapapeles (lo copiado antes en otra app).
#[test]
fn dropping_an_image_ignores_the_clipboard() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let src = outside.path().join("logo.png");
    image::RgbaImage::from_pixel(4, 4, image::Rgba([10, 20, 30, 255])).save(&src).unwrap();
    let hooks = iure_doc_ui::Hooks {
        paste: Some(std::sync::Arc::new(OtherAppHtml)),
        images: Some(std::sync::Arc::new(iure_doc_ui::FolderImages::new(Some(dir.path().to_path_buf()), "img"))),
        ..Default::default()
    };
    let mut h = Harness::builder().with_size(egui::vec2(600.0, 400.0)).build_ui_state(
        move |ui, st: &mut State| {
            let out = iure_doc_ui::show_blocks_hooks(ui, Id::unique("drop"), &mut st.text, "Escribe", hooks.clone());
            out.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::MultilineTextInput, true, "bloques"));
        },
        State { text: "Antes\n".to_string() },
    );
    h.run_steps(2);
    h.input_mut().dropped_files.push(std::sync::Arc::new(Dropped(src)));
    h.run_steps(4);
    assert_eq!(h.state().text, "Antes\n\n![logo](img/logo.png)\n");
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

// --- Menús de `/` y `[[`, enlaces y plantillas (como IureEditor 1.x) ---

struct Notes;

impl iure_doc_ui::NoteIndex for Notes {
    fn notes(&self) -> Vec<String> {
        vec!["Acta de junta".into(), "Clientes/Altamira".into()]
    }
}

struct LinkState {
    text: String,
    link: Option<String>,
}

fn menus_harness(text: &str) -> Harness<'static, LinkState> {
    let hooks = iure_doc_ui::Hooks { notes: Some(std::sync::Arc::new(Notes)), ..Default::default() };
    Harness::builder().with_size(egui::vec2(600.0, 500.0)).build_ui_state(
        move |ui, st: &mut LinkState| {
            let out = iure_doc_ui::show_blocks_hooks(ui, Id::unique("mn"), &mut st.text, "Escribe", hooks.clone());
            if let Some(l) = out.link {
                st.link = Some(l);
            }
            out.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::MultilineTextInput, true, "bloques"));
        },
        LinkState { text: text.to_string(), link: None },
    )
}

fn menus_cursor_at(h: &mut Harness<'static, LinkState>, at: usize) {
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
fn slash_opens_the_menu_and_enter_runs_the_command() {
    iurefficient_ui::lang::set_lang(iurefficient_ui::lang::Lang::Es);
    let mut h = menus_harness("/menta\n");
    menus_cursor_at(&mut h, 6);
    h.run();
    assert!(h.query_by_label("Mapa mental").is_some(), "el menú se ve junto al cursor");
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert!(h.state().text.contains("> [!MINDMAP] Tema\n> - Idea A"), "{:?}", h.state().text);
    assert!(!h.state().text.contains("/menta"));
}

#[test]
fn slash_menu_closes_with_escape_and_arrows_choose() {
    iurefficient_ui::lang::set_lang(iurefficient_ui::lang::Lang::Es);
    let mut h = menus_harness("/tít\n");
    menus_cursor_at(&mut h, 4);
    h.key_press(Key::ArrowDown);
    h.run();
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert_eq!(h.state().text, "## \n", "la segunda opción: Título 2");
    let mut h = menus_harness("/tít\n");
    menus_cursor_at(&mut h, 4);
    h.key_press(Key::Escape);
    h.run();
    assert!(h.query_by_label("Título 1").is_none(), "Esc lo cierra");
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert!(h.state().text.starts_with("/tít"), "Intro ya es un salto normal: {:?}", h.state().text);
}

#[test]
fn double_brackets_link_notes_and_ctrl_click_opens_them() {
    iurefficient_ui::lang::set_lang(iurefficient_ui::lang::Lang::Es);
    let mut h = menus_harness("Ver [[alta\n");
    menus_cursor_at(&mut h, 10);
    assert!(h.query_by_label("Altamira").is_some());
    h.key_press(Key::Tab);
    h.run_steps(3);
    assert_eq!(h.state().text, "Ver [[Altamira]]\n");
    // Ctrl+clic en el enlace (con el cursor en otra línea los corchetes no se ven).
    let mut h = menus_harness("Ver [[Acta de junta|el acta]] hoy\n\notro\n");
    h.run();
    let r = h.get_all_by_role(egui::accesskit::Role::MultilineTextInput).next().map(|n| n.rect()).unwrap();
    h.event(egui::Event::ModifiersChanged(Modifiers::COMMAND));
    h.run();
    h.event(egui::Event::PointerMoved(egui::pos2(r.left() + 60.0, r.center().y)));
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos: egui::pos2(r.left() + 60.0, r.center().y),
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::COMMAND,
        });
    }
    h.run_steps(3);
    h.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    assert_eq!(h.state().link.as_deref(), Some("[[Acta de junta]]"));
}

#[test]
fn mermaid_blocks_offer_templates() {
    iurefficient_ui::lang::set_lang(iurefficient_ui::lang::Lang::Es);
    let mut h = menus_harness("```mermaid\nflowchart TD\n```\n");
    menus_cursor_at(&mut h, 3);
    h.get_by_label("Plantilla…").click();
    h.run_steps(2);
    h.get_by_label("Pastel").click();
    h.run_steps(3);
    assert!(h.state().text.starts_with("```mermaid\npie title Distribución"), "{:?}", h.state().text);
}

/// Regresión: una lista «suelta» (línea en blanco entre elementos, como muchos índices) se veía con
/// un renglón vacío entero entre cada elemento.
#[test]
fn loose_lists_are_not_double_spaced() {
    let height = |t: &str| {
        let mut h = blocks_harness(t);
        h.run();
        h.get_all_by_role(egui::accesskit::Role::MultilineTextInput).nth(1).map(|n| n.rect().height()).unwrap()
    };
    let tight = height("Intro\n\n- [Uno](#a)\n    - [Dos](#b)\n    - [Tres](#c)\n\nFin\n");
    let loose = height("Intro\n\n- [Uno](#a)\n\n    - [Dos](#b)\n\n    - [Tres](#c)\n\nFin\n");
    assert!(loose < tight * 1.4, "suelta {loose} frente a compacta {tight}");
}

// --- Nodos de un mapa mental pulsables en el dibujo ---

struct FakeMap;

impl iure_doc_ui::DiagramRenderer for FakeMap {
    fn accepts(&self, block: &str) -> bool {
        block.starts_with("> [!MINDMAP")
    }
    fn key(&self) -> u64 {
        2
    }
    fn render(&self, _block: &str, _scale: f32) -> Result<iure_doc_ui::DiagramImage, String> {
        Ok(iure_doc_ui::DiagramImage {
            width: 20,
            height: 10,
            rgba: vec![200; 20 * 10 * 4],
            points: [200.0, 100.0],
            hotspots: vec![iure_doc_ui::Hotspot { id: 1, rect: [10.0, 10.0, 60.0, 30.0], ..Default::default() }],
        })
    }
    fn node_action(&self, block: &str, id: usize, a: iure_doc_ui::NodeAction) -> Option<(String, Option<usize>)> {
        (id == 1 && a == iure_doc_ui::NodeAction::AddChild).then(|| (format!("{block}\n>   - Nueva"), None))
    }
}

#[test]
fn clicking_a_mind_map_node_offers_its_bar() {
    iurefficient_ui::lang::set_lang(iurefficient_ui::lang::Lang::Es);
    let r: std::sync::Arc<dyn iure_doc_ui::DiagramRenderer> = std::sync::Arc::new(FakeMap);
    let doc = "Antes\n\n> [!MINDMAP|map] C\n> - Uno\n\nDespués\n";
    let mut h = Harness::builder().with_size(egui::vec2(600.0, 400.0)).build_ui_state(
        move |ui, st: &mut State| {
            let out = iure_doc_ui::show_blocks_with(ui, Id::unique("mm"), &mut st.text, "Escribe", Some(r.clone()));
            out.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::MultilineTextInput, true, "bloques"));
        },
        State { text: doc.to_string() },
    );
    assert!(wait_for(&mut h, "Diagrama 2"));
    let img = h.get_by_label("Diagrama 2").rect();
    let k = img.width() / 200.0;
    let p = img.min + egui::vec2(35.0 * k, 20.0 * k);
    h.event(egui::Event::PointerMoved(p));
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
    }
    h.run_steps(3);
    assert!(h.query_by_label("Diagrama 2").is_some(), "un clic en un nodo no abre el código");
    h.get_by_label("Añadir rama hija").click();
    h.run_steps(3);
    assert_eq!(h.state().text, "Antes\n\n> [!MINDMAP|map] C\n> - Uno\n>   - Nueva\n\nDespués\n");
}

// --- S1.33: acciones propias de un diagrama (cuadrantes) ---

/// Un «cuadrante» falso: un punto arrastrable en (10,10)-(30,30); un hueco ofrece «Agregar aquí»
/// (pide la etiqueta); el punto ofrece «Renombrar» (texto) y «Conectar con…» (otra zona).
struct FakeQuad;

/// Se pidió la guía del arrastre.
static HINT_ASKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

impl iure_doc_ui::DiagramRenderer for FakeQuad {
    fn accepts(&self, block: &str) -> bool {
        block.starts_with("```cuad")
    }
    fn key(&self) -> u64 {
        3
    }
    fn render(&self, _block: &str, _scale: f32) -> Result<iure_doc_ui::DiagramImage, String> {
        Ok(iure_doc_ui::DiagramImage {
            width: 20,
            height: 10,
            rgba: vec![220; 20 * 10 * 4],
            points: [200.0, 100.0],
            hotspots: vec![
                iure_doc_ui::Hotspot { id: 0, rect: [10.0, 10.0, 30.0, 30.0], draggable: true },
                iure_doc_ui::Hotspot { id: 1, rect: [150.0, 10.0, 170.0, 30.0], draggable: false },
                // Justo debajo del 0: la barra del 0 no debe taparlo.
                iure_doc_ui::Hotspot { id: 2, rect: [10.0, 40.0, 30.0, 60.0], draggable: false },
            ],
        })
    }
    fn canvas_actions(&self, _block: &str, _at: [f32; 2]) -> Vec<iure_doc_ui::ActionDef> {
        vec![iure_doc_ui::ActionDef {
            id: 1,
            icon: iurefficient_ui::icons::Icon::Plus,
            label: "Agregar aquí".into(),
            input: iure_doc_ui::ActionInput::Text { prompt: "Etiqueta".into(), initial: String::new() },
        }]
    }
    fn spot_actions(&self, _block: &str, _id: usize) -> Vec<iure_doc_ui::ActionDef> {
        vec![
            iure_doc_ui::ActionDef {
                id: 2,
                icon: iurefficient_ui::icons::Icon::Hyperlink,
                label: "Conectar con…".into(),
                input: iure_doc_ui::ActionInput::OtherSpot("Elige el destino".into()),
            },
            iure_doc_ui::ActionDef {
                id: 3,
                icon: iurefficient_ui::icons::Icon::Plus,
                label: "Agregar siguiente".into(),
                input: iure_doc_ui::ActionInput::ChoiceText {
                    options: vec!["Rectángulo".into(), "Decisión".into()],
                    prompt: "Texto".into(),
                },
            },
        ]
    }
    fn apply(
        &self,
        block: &str,
        target: iure_doc_ui::Target,
        action: u32,
        data: iure_doc_ui::ActionData,
    ) -> Option<(String, Option<usize>)> {
        let body = block.trim_end().strip_suffix("```")?;
        let line = match (target, action, data) {
            (iure_doc_ui::Target::Canvas([x, y]), 1, iure_doc_ui::ActionData::Text(t)) => {
                format!("{t} {:.0} {:.0}", x / 10.0, y / 10.0)
            }
            (iure_doc_ui::Target::Spot(a), 2, iure_doc_ui::ActionData::Spot(b)) => format!("{a} --> {b}"),
            (iure_doc_ui::Target::Spot(a), 3, iure_doc_ui::ActionData::ChoiceText(k, t)) => {
                format!("{a} siguiente {k} {t}")
            }
            _ => return None,
        };
        Some((format!("{body}{line}\n```"), None))
    }
    fn spot_label(&self, _block: &str, id: usize) -> Option<String> {
        ["Uno", "Dos", "Tres"].get(id).map(|s| s.to_string())
    }
    fn drag_hint(&self, _block: &str, _id: usize, to: [f32; 2]) -> Option<String> {
        HINT_ASKED.store(true, std::sync::atomic::Ordering::Relaxed);
        Some(format!("{:.0}, {:.0}", to[0], to[1]))
    }
    fn drag(&self, block: &str, id: usize, to: [f32; 2]) -> Option<String> {
        let body = block.trim_end().strip_suffix("```")?;
        Some(format!("{body}mover {id} {:.0} {:.0}\n```", to[0] / 10.0, to[1] / 10.0))
    }
}

fn click(h: &mut Harness<'static, State>, p: egui::Pos2) {
    h.event(egui::Event::PointerMoved(p));
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
    }
    h.run_steps(3);
}

fn quad_harness() -> Harness<'static, State> {
    iurefficient_ui::lang::set_lang(iurefficient_ui::lang::Lang::Es);
    let r: std::sync::Arc<dyn iure_doc_ui::DiagramRenderer> = std::sync::Arc::new(FakeQuad);
    let mut h = Harness::builder().with_size(egui::vec2(600.0, 400.0)).build_ui_state(
        move |ui, st: &mut State| {
            let out = iure_doc_ui::show_blocks_with(ui, Id::unique("cuad"), &mut st.text, "Escribe", Some(r.clone()));
            out.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::MultilineTextInput, true, "bloques"));
        },
        State { text: "```cuad\n```\n".to_string() },
    );
    assert!(wait_for(&mut h, "Diagrama 1"));
    h
}

/// Un clic en un hueco ofrece «Agregar aquí»; pide la etiqueta en un globo y la app recibe el lugar.
#[test]
fn a_click_on_empty_space_adds_with_a_label() {
    let mut h = quad_harness();
    let img = h.get_by_label("Diagrama 1").rect();
    let k = img.width() / 200.0;
    click(&mut h, img.min + egui::vec2(100.0 * k, 50.0 * k));
    assert!(h.query_by_label("Diagrama 1").is_some(), "el hueco no abre el código");
    h.get_by_label("Agregar aquí").click();
    h.run_steps(3);
    h.event(egui::Event::Text("Idea".into()));
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert_eq!(h.state().text, "```cuad\nIdea 10 5\n```\n");
}

/// «Conectar con…»: se elige la zona de origen, la acción y luego la de destino.
#[test]
fn an_action_can_ask_for_another_spot() {
    let mut h = quad_harness();
    let img = h.get_by_label("Diagrama 1").rect();
    let k = img.width() / 200.0;
    click(&mut h, img.min + egui::vec2(20.0 * k, 20.0 * k));
    h.get_by_label("Conectar con…").click();
    h.run_steps(2);
    click(&mut h, img.min + egui::vec2(160.0 * k, 20.0 * k));
    assert_eq!(h.state().text, "```cuad\n0 --> 1\n```\n");
}

/// «Agregar siguiente»: se elige la forma en un menú y luego se escribe el texto en el globo.
#[test]
fn an_action_can_ask_for_a_choice_and_then_a_text() {
    let mut h = quad_harness();
    let img = h.get_by_label("Diagrama 1").rect();
    let k = img.width() / 200.0;
    click(&mut h, img.min + egui::vec2(20.0 * k, 20.0 * k));
    h.get_by_label("Agregar siguiente").click();
    h.run_steps(3);
    h.get_by_label("Decisión").click();
    h.run_steps(3);
    h.event(egui::Event::Text("Paso".into()));
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert_eq!(h.state().text, "```cuad\n0 siguiente 1 Paso\n```\n");
}

/// Regresión (dueño 2026-11-23): la barra tapaba el nodo de abajo. Ahora busca un lugar libre.
#[test]
fn the_bar_does_not_cover_the_neighbour() {
    let mut h = quad_harness();
    let img = h.get_by_label("Diagrama 1").rect();
    let k = img.width() / 200.0;
    click(&mut h, img.min + egui::vec2(20.0 * k, 20.0 * k));
    h.run_steps(3);
    let bar = h.get_by_label("Conectar con…").rect().union(h.get_by_label("Agregar siguiente").rect());
    let below =
        egui::Rect::from_min_max(img.min + egui::vec2(10.0 * k, 40.0 * k), img.min + egui::vec2(30.0 * k, 60.0 * k));
    assert!(!bar.intersects(below), "la barra {bar:?} tapa el nodo de abajo {below:?}");
}

/// Accesibilidad: cada zona tiene su nombre y se alcanza con el teclado; Intro lleva a su barra y,
/// en «Conectar con…», Intro sobre otra zona la elige.
#[test]
fn zones_work_with_the_keyboard() {
    let mut h = quad_harness();
    h.get_by_label("Uno").focus();
    h.run_steps(3);
    assert!(h.query_by_label("Conectar con…").is_some(), "al enfocar la zona aparece su barra");
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert!(h.get_by_label("Conectar con…").is_focused(), "Intro pasa a la barra");
    h.key_press(Key::Enter);
    h.run_steps(3);
    h.get_by_label("Dos").focus();
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert_eq!(h.state().text, "```cuad\n0 --> 1\n```\n");
}

/// Un punto arrastrable se mueve: al soltarlo, la app recibe dónde quedó.
#[test]
fn a_draggable_spot_moves() {
    let mut h = quad_harness();
    let img = h.get_by_label("Diagrama 1").rect();
    let k = img.width() / 200.0;
    let from = img.min + egui::vec2(20.0 * k, 20.0 * k);
    let to = img.min + egui::vec2(120.0 * k, 70.0 * k);
    h.event(egui::Event::PointerMoved(from));
    h.event(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(2);
    for t in 1..=5 {
        let p = from + (to - from) * (t as f32 / 5.0);
        h.event(egui::Event::PointerMoved(p));
        h.run();
    }
    h.event(egui::Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(3);
    assert_eq!(h.state().text, "```cuad\nmover 0 12 7\n```\n");
    assert!(HINT_ASKED.load(std::sync::atomic::Ordering::Relaxed), "mientras se arrastra se muestra la guía");
}

/// Regresión (dueño 2026-11-20): un diagrama al final (o al principio) no dejaba escribir después
/// (o antes). La franja junto al dibujo abre un párrafo ahí.
#[test]
fn you_can_write_after_and_before_a_diagram() {
    iurefficient_ui::lang::set_lang(iurefficient_ui::lang::Lang::Es);
    let doc = "```mermaid\nflowchart LR\n  A-->B\n```\n";
    let mut h = diagram_harness(doc);
    assert!(wait_for(&mut h, "Diagrama 1"));
    // Después: la última franja.
    let after = h.get_all_by_label("Clic para escribir aquí").last().map(|n| n.rect()).unwrap();
    let p = after.center();
    h.event(egui::Event::PointerMoved(p));
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
    }
    h.run_steps(3);
    h.event(egui::Event::Text("Fin".into()));
    h.run_steps(3);
    assert_eq!(h.state().text, "```mermaid\nflowchart LR\n  A-->B\n```\n\nFin\n", "{:?}", h.state().text);
    // Antes: la franja de arriba.
    let before = h.get_all_by_label("Clic para escribir aquí").next().map(|n| n.rect()).unwrap();
    let p = before.center();
    h.event(egui::Event::PointerMoved(p));
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
    }
    h.run_steps(3);
    h.event(egui::Event::Text("Inicio".into()));
    h.run_steps(3);
    assert!(h.state().text.starts_with("Inicio\n\n```mermaid"), "{:?}", h.state().text);
}

/// Al editar el código de un diagrama, su dibujo se ve debajo (vista previa, como IureEditor 1.x).
#[test]
fn editing_a_diagram_shows_its_preview() {
    iurefficient_ui::lang::set_lang(iurefficient_ui::lang::Lang::Es);
    let mut h = diagram_harness(DIAGRAM_DOC);
    assert!(wait_for(&mut h, "Diagrama 2"));
    h.get_by_label("Diagrama 2").click();
    h.run_steps(3);
    assert!(h.query_by_label("Diagrama 2").is_none(), "se ve el código");
    assert!(wait_for(&mut h, "Dibujo del diagrama"), "y debajo, el dibujo");
}

/// Regresión (dueño 2026-11-20): en el redactor no salían los acentos. La tecla muerta llega como
/// preedición («´») y luego el texto final («á»); el editor por bloques volvía a poner el cursor y
/// quedaba «´á».
#[test]
fn dead_keys_compose_accents() {
    for blocks in [false, true] {
        let mut h = if blocks { blocks_harness("Hola\n") } else { harness("Hola\n") };
        if blocks {
            block_cursor_at(&mut h, 4);
        } else {
            cursor_at(&mut h, 4);
        }
        h.event(egui::Event::Ime(egui::ImeEvent::Preedit { text: "´".into(), active_range_chars: None }));
        h.run_steps(2);
        h.event(egui::Event::Ime(egui::ImeEvent::Commit("á".into())));
        h.run_steps(2);
        h.event(egui::Event::Text(" y ñ".into()));
        h.run_steps(2);
        assert_eq!(h.state().text, "Holaá y ñ\n", "por bloques: {blocks}");
    }
}

/// Regresión (dueño 2026-11-20): en el menú de `/`, elegir con el ratón no hacía nada (el clic le
/// quitaba el foco al campo y el menú se cerraba antes de soltar).
#[test]
fn slash_menu_items_can_be_clicked() {
    iurefficient_ui::lang::set_lang(iurefficient_ui::lang::Lang::Es);
    let mut h = menus_harness("/menta\n");
    menus_cursor_at(&mut h, 6);
    h.run();
    let p = h.get_by_label("Mapa mental").rect().center();
    h.event(egui::Event::PointerMoved(p));
    h.run();
    h.event(egui::Event::PointerButton {
        pos: p,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.run();
    h.event(egui::Event::PointerButton {
        pos: p,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(3);
    assert!(h.state().text.starts_with("> [!MINDMAP]"), "{:?}", h.state().text);
}

/// La barra de formato compartida: cada botón dice qué hace y devuelve su cambio; el subrayado lo
/// resuelve la app.
#[test]
fn the_shared_format_bar_reports_what_was_pressed() {
    use iure_doc_ui::toolbar::{self, Pressed, Tool};
    iurefficient_ui::lang::set_lang(iurefficient_ui::lang::Lang::Es);
    let mut h = Harness::builder().with_size(egui::vec2(900.0, 60.0)).build_ui_state(
        |ui, got: &mut Vec<Pressed>| {
            ui.horizontal(|ui| {
                if let Some(p) = toolbar::tools(ui, toolbar::FULL) {
                    got.push(p);
                }
                if let Some(p) = toolbar::tools(ui, &[Tool::Underline]) {
                    got.push(p);
                }
            });
        },
        Vec::new(),
    );
    h.run();
    for label in ["Negrita (Ctrl+B)", "Cita", "Lista con viñetas", "Lista numerada", "Enlace", "Subrayado"] {
        h.get_by_label(label).click();
        h.run();
    }
    assert_eq!(
        h.state(),
        &vec![
            Pressed::Edit(iure_doc_ui::ExternalEdit::Wrap("**".into())),
            Pressed::Edit(iure_doc_ui::ExternalEdit::Quote),
            Pressed::Edit(iure_doc_ui::ExternalEdit::Line(iure_doc::edit::LineKind::Bullet)),
            Pressed::Edit(iure_doc_ui::ExternalEdit::Line(iure_doc::edit::LineKind::Ordered)),
            Pressed::Edit(iure_doc_ui::ExternalEdit::Link),
            Pressed::Other(Tool::Underline),
        ]
    );
}

/// Números de línea junto al texto con formato: cada renglón que empieza una línea del Markdown
/// lleva la suya (las líneas vacías entre bloques cuentan).
#[test]
fn line_numbers_follow_the_markdown_lines() {
    let mut h = Harness::builder().with_size(egui::vec2(600.0, 400.0)).build_ui_state(
        |ui, text: &mut String| {
            let hooks = iure_doc_ui::Hooks { line_numbers: true, gutter: Some(40.0), ..Default::default() };
            iure_doc_ui::show_blocks_hooks(ui, Id::unique("lineas"), text, "", hooks);
        },
        "uno\n\ndos\ntres\n\n- a\n\n- b\n".to_string(),
    );
    h.run_steps(2);
    let mut nums: Vec<String> = Vec::new();
    for c in &h.output().shapes {
        if let egui::epaint::Shape::Text(t) = &c.shape {
            let s = t.galley.text().to_string();
            if s.chars().all(|c| c.is_ascii_digit()) && !s.is_empty() {
                nums.push(s);
            }
        }
    }
    nums.sort();
    // La línea vacía dentro de la lista (7) cuenta pero no se numera.
    assert_eq!(nums, ["1", "3", "4", "6", "8"]);
}

/// Solo para leer: se ve igual que el editor, pero lo tecleado no cambia el texto.
#[test]
fn read_only_shows_the_same_and_does_not_change() {
    let mut h = Harness::builder().with_size(egui::vec2(600.0, 400.0)).build_ui_state(
        |ui, st: &mut State| {
            let hooks = iure_doc_ui::Hooks { read_only: true, ..Default::default() };
            let out = iure_doc_ui::show_blocks_hooks(ui, Id::unique("lectura"), &mut st.text, "", hooks);
            assert!(!out.changed);
            out.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::MultilineTextInput, true, "nota"));
        },
        State { text: "# Título\n\nUno\nDos\n".to_string() },
    );
    h.run_steps(2);
    let r = h.get_by_label("nota").rect();
    let p = egui::pos2(r.left() + 30.0, r.top() + 60.0);
    h.event(egui::Event::PointerButton {
        pos: p,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.event(egui::Event::PointerButton {
        pos: p,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(2);
    h.event(egui::Event::Text("XYZ".into()));
    h.key_press(Key::Backspace);
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert_eq!(h.state().text, "# Título\n\nUno\nDos\n");
}

// --- Regresiones del dueño (2026-11-23, IureEditor 2.0) ---

/// Elige `a..b` (caracteres) en el bloque con el foco.
fn block_select(h: &mut Harness<'static, State>, a: usize, b: usize) {
    block_cursor_at(h, a);
    let ctx = h.ctx.clone();
    let Some(id) = ctx.memory(|m| m.focused()) else { panic!("sin foco") };
    let mut st = TextEditState::load(&ctx, id).unwrap_or_default();
    st.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(a), CCursor::new(b))));
    st.store(&ctx, id);
    h.run();
}

/// Tab con varias líneas elegidas de una lista numerada: todas se meten bajo el número de arriba
/// y la numeración se rehace (la sublista empieza en 1, la de afuera sigue).
#[test]
fn tab_on_several_numbered_lines_indents_and_renumbers() {
    let mut h = blocks_harness("1. uno\n2. dos\n3. tres\n4. cuatro\n");
    block_select(&mut h, 8, 20);
    h.key_press(Key::Tab);
    h.run_steps(3);
    assert_eq!(h.state().text, "1. uno\n   1. dos\n   2. tres\n2. cuatro\n");
}

/// Borrar un elemento de una lista numerada la renumera.
#[test]
fn deleting_an_item_renumbers_the_list() {
    let mut h = blocks_harness("1. uno\n2. dos\n3. tres\n");
    // «2. dos\n» entera, y Retroceso.
    block_select(&mut h, 7, 14);
    h.key_press(Key::Backspace);
    h.run_steps(3);
    assert_eq!(h.state().text, "1. uno\n2. tres\n");
}

/// Pegar una lista con sangrías dentro de un mapa mental: cada línea es un nodo (con su `> `).
#[test]
fn pasting_a_list_into_a_mind_map_keeps_every_node() {
    let doc = "> [!MINDMAP|list] Proyectos\n> - \n";
    let mut h = blocks_harness(doc);
    // El cursor al final de «> - ».
    block_cursor_at(&mut h, "> [!MINDMAP|list] Proyectos\n> - ".chars().count());
    h.event(egui::Event::Paste("- Iurefficient\n  - App\n- Salud".into()));
    h.run_steps(3);
    assert_eq!(h.state().text, "> [!MINDMAP|list] Proyectos\n> - Iurefficient\n>   - App\n> - Salud\n");
}

/// Re Pág / Av Pág: en un documento largo, Av Pág lleva el cursor una pantalla abajo (otro bloque)
/// y Re Pág lo regresa.
#[test]
fn page_down_and_up_move_by_a_screen() {
    let doc: String = (1..=80).map(|n| format!("Párrafo {n}\n\n")).collect();
    let mut h = blocks_harness(&doc);
    block_cursor_at(&mut h, 0);
    let first = h.ctx.memory(|m| m.focused());
    h.key_press(Key::PageDown);
    h.run_steps(3);
    let after = h.ctx.memory(|m| m.focused());
    assert_ne!(first, after, "Av Pág cambió de bloque");

    h.key_press(Key::PageUp);
    h.run_steps(3);
    assert_eq!(h.ctx.memory(|m| m.focused()), first, "Re Pág volvió arriba");
}

/// Regresión (dueño 2026-11-23): en un bloque largo (una lista de muchos elementos), Av Pág movía
/// el cursor pero la vista no lo seguía. Ahora el documento se desplaza una pantalla.
#[test]
fn page_down_inside_a_long_block_scrolls_the_view() {
    let doc: String = (1..=150).map(|n| format!("- elemento {n}\n")).collect();
    let mut h = blocks_harness(&doc);
    block_cursor_at(&mut h, 0);
    let id = h.ctx.memory(|m| m.focused()).unwrap();
    let top = |h: &Harness<'static, State>| h.ctx.read_response(id).map(|r| r.rect.top()).unwrap();
    let before = top(&h);
    h.key_press(Key::PageDown);
    h.run_steps(4);
    assert_eq!(h.ctx.memory(|m| m.focused()), Some(id), "el mismo bloque");
    let after = top(&h);
    assert!(after < before - 100.0, "la vista no bajó: {before} → {after}");
    h.key_press(Key::PageUp);
    h.run_steps(4);
    assert!((top(&h) - before).abs() < 2.0, "Re Pág la regresa");
}
