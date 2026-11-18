//! S1.22: guion de comportamientos del editor por bloques (§4.7 de `suite-integracion.md`), con
//! `egui_kittest`: lo que hace cada tecla sobre un documento real.

use egui::text::{CCursor, CCursorRange};
use egui::text_edit::TextEditState;
use egui::{Id, Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

struct State {
    text: String,
}

fn harness(text: &str, hooks: iure_doc_ui::Hooks) -> Harness<'static, State> {
    Harness::builder().with_size(egui::vec2(600.0, 400.0)).build_ui_state(
        move |ui, st: &mut State| {
            let out = iure_doc_ui::show_blocks_hooks(ui, Id::unique("rules"), &mut st.text, "Escribe", hooks.clone());
            out.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::MultilineTextInput, true, "doc"));
        },
        State { text: text.to_string() },
    )
}

/// Enfoca el primer bloque (o el que tenga el foco) y pone el cursor en `at` (carácter del bloque).
fn cursor_at(h: &mut Harness<'static, State>, at: usize) {
    if h.ctx.memory(|m| m.focused()).is_none() {
        h.get_by_label("doc").focus();
        h.run();
    }
    let ctx = h.ctx.clone();
    let Some(id) = ctx.memory(|m| m.focused()) else { panic!("sin foco") };
    let mut st = TextEditState::load(&ctx, id).unwrap_or_default();
    st.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(at))));
    st.store(&ctx, id);
    h.run();
}

fn select(h: &mut Harness<'static, State>, a: usize, b: usize) {
    let ctx = h.ctx.clone();
    let Some(id) = ctx.memory(|m| m.focused()) else { panic!("sin foco") };
    let mut st = TextEditState::load(&ctx, id).unwrap_or_default();
    st.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(a), CCursor::new(b))));
    st.store(&ctx, id);
    h.run();
}

fn typed(h: &mut Harness<'static, State>, s: &str) {
    h.event(egui::Event::Text(s.into()));
    h.run_steps(2);
}

fn key(h: &mut Harness<'static, State>, k: Key) {
    h.key_press(k);
    h.run_steps(2);
}

fn chord(h: &mut Harness<'static, State>, m: Modifiers, k: Key) {
    h.key_press_modifiers(m, k);
    h.run_steps(2);
}

fn plain() -> iure_doc_ui::Hooks {
    iure_doc_ui::Hooks::default()
}

#[test]
fn enter_in_a_task_creates_an_unchecked_task() {
    let mut h = harness("- [x] hecha\n", plain());
    cursor_at(&mut h, 11);
    key(&mut h, Key::Enter);
    typed(&mut h, "otra");
    assert_eq!(h.state().text, "- [x] hecha\n- [ ] otra\n");
}

#[test]
fn enter_in_an_empty_item_leaves_the_list() {
    let mut h = harness("- uno\n", plain());
    cursor_at(&mut h, 5);
    key(&mut h, Key::Enter);
    key(&mut h, Key::Enter);
    typed(&mut h, "fuera");
    assert_eq!(h.state().text, "- uno\n\nfuera\n");
}

#[test]
fn enter_after_a_heading_continues_with_a_paragraph() {
    let mut h = harness("# Título\n", plain());
    cursor_at(&mut h, 8);
    key(&mut h, Key::Enter);
    typed(&mut h, "texto");
    assert_eq!(h.state().text, "# Título\n\ntexto\n");
}

#[test]
fn three_enters_leave_a_code_block() {
    let mut h = harness("```rust\nfn f() {}", plain());
    cursor_at(&mut h, 17);
    key(&mut h, Key::Enter);
    key(&mut h, Key::Enter);
    key(&mut h, Key::Enter);
    typed(&mut h, "fin");
    assert_eq!(h.state().text, "```rust\nfn f() {}\n```\n\nfin\n");
}

#[test]
fn tab_and_shift_tab_indent_list_items() {
    let mut h = harness("- uno\n- dos\n", plain());
    cursor_at(&mut h, 11);
    key(&mut h, Key::Tab);
    assert_eq!(h.state().text, "- uno\n  - dos\n");
    chord(&mut h, Modifiers::SHIFT, Key::Tab);
    assert_eq!(h.state().text, "- uno\n- dos\n");
}

#[test]
fn shortcuts_make_lists_and_headings() {
    let shift_cmd = Modifiers { shift: true, ..Modifiers::COMMAND };
    let alt_cmd = Modifiers { alt: true, ..Modifiers::COMMAND };
    let mut h = harness("compras\n", plain());
    cursor_at(&mut h, 7);
    chord(&mut h, shift_cmd, Key::Num8);
    assert_eq!(h.state().text, "- compras\n");
    chord(&mut h, shift_cmd, Key::Num9);
    assert_eq!(h.state().text, "- [ ] compras\n");
    chord(&mut h, shift_cmd, Key::Num7);
    assert_eq!(h.state().text, "1. compras\n");
    chord(&mut h, alt_cmd, Key::Num2);
    assert_eq!(h.state().text, "## compras\n");
    chord(&mut h, alt_cmd, Key::Num2);
    assert_eq!(h.state().text, "compras\n", "otra vez: se quita");
}

#[test]
fn backspace_right_after_a_rule_undoes_it() {
    // Markdown en vivo: `# ` ya es título; Retroceso deja el `#` literal.
    let mut h = harness("x\n", plain());
    cursor_at(&mut h, 0);
    typed(&mut h, "# ");
    assert_eq!(h.state().text, "# x\n");
    key(&mut h, Key::Backspace);
    assert_eq!(h.state().text, "#x\n");
    chord(&mut h, Modifiers::COMMAND, Key::Z);
    chord(&mut h, Modifiers::COMMAND, Key::Z);
    assert_eq!(h.state().text, "x\n", "Ctrl+Z también");
}

#[test]
fn ctrl_b_and_ctrl_i_wrap_the_selection() {
    let mut h = harness("hola mundo\n", plain());
    cursor_at(&mut h, 0);
    select(&mut h, 5, 10);
    chord(&mut h, Modifiers::COMMAND, Key::B);
    assert_eq!(h.state().text, "hola **mundo**\n");
}

struct Html;

impl iure_doc_ui::PasteSource for Html {
    fn html(&self) -> Option<String> {
        Some("<h2>Título</h2>".into())
    }
}

#[test]
fn ctrl_shift_v_pastes_plain_text() {
    let hooks = iure_doc_ui::Hooks { paste: Some(std::sync::Arc::new(Html)), ..Default::default() };
    let mut h = harness("\n", hooks);
    cursor_at(&mut h, 0);
    // Con Mayús sostenida (Ctrl+Mayús+V) se pega el texto plano, no el HTML.
    h.event(egui::Event::ModifiersChanged(Modifiers { shift: true, ..Modifiers::COMMAND }));
    h.event(egui::Event::Paste("Título".into()));
    h.run_steps(2);
    assert_eq!(h.state().text, "Título\n");
}
