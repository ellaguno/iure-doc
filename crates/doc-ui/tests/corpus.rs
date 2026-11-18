//! S1.22: corpus de documentos reales (IureEditor, IureDesk, casos difíciles). Abrir y guardar sin
//! tocar da el mismo archivo byte a byte, en el modelo (`iure_doc::Document`) y en el editor
//! completo dibujado (bloques, tablas, diagramas e imágenes en su lugar).
//!
//! `IURE_DOC_CORPUS=/carpeta` agrega los `.md` de esa carpeta (p. ej. notas propias) sin copiarlos
//! al repositorio.

use std::path::{Path, PathBuf};

use egui::Id;
use egui_kittest::Harness;

fn collect(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() && depth < 6 {
            collect(&p, out, depth + 1);
        } else if p.extension().is_some_and(|x| x == "md") && out.len() < 5_000 {
            out.push(p);
        }
    }
}

fn corpus() -> Vec<(PathBuf, String)> {
    let mut files = Vec::new();
    collect(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus"), &mut files, 0);
    if let Some(extra) = std::env::var_os("IURE_DOC_CORPUS") {
        collect(Path::new(&extra), &mut files, 0);
    }
    files.sort();
    files.into_iter().filter_map(|p| std::fs::read_to_string(&p).ok().map(|t| (p, t))).collect()
}

#[test]
fn every_document_survives_the_model() {
    let docs = corpus();
    assert!(docs.len() >= 9, "el corpus está completo");
    for (p, t) in &docs {
        assert_eq!(&iure_doc::Document::parse(t).to_markdown(), t, "{}", p.display());
    }
}

struct Doc {
    text: String,
}

/// Dibuja diagramas e imágenes como cualquier cosa: lo que importa es que no toquen el texto.
struct AnyDrawing;

impl iure_doc_ui::DiagramRenderer for AnyDrawing {
    fn accepts(&self, block: &str) -> bool {
        block.starts_with("```mermaid") || block.starts_with("> [!MINDMAP") || block.starts_with("![")
    }
    fn key(&self) -> u64 {
        7
    }
    fn render(&self, _block: &str, _scale: f32) -> Result<iure_doc_ui::DiagramImage, String> {
        Ok(iure_doc_ui::DiagramImage { width: 4, height: 2, rgba: vec![128; 32], points: [40.0, 20.0] })
    }
}

#[test]
fn every_document_survives_the_editor() {
    for (p, t) in corpus() {
        let hooks = iure_doc_ui::Hooks { diagrams: Some(std::sync::Arc::new(AnyDrawing)), ..Default::default() };
        // Los documentos reales traen símbolos (→, ✓) que la tipografía mínima de las pruebas no tiene.
        let mut h = Harness::builder().with_size(egui::vec2(800.0, 2000.0)).allow_missing_glyphs().build_ui_state(
            move |ui, d: &mut Doc| {
                let out = iure_doc_ui::show_blocks_hooks(ui, Id::unique("corpus"), &mut d.text, "", hooks.clone());
                out.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::MultilineTextInput, true, "doc"));
            },
            Doc { text: t.clone() },
        );
        h.run_steps(4);
        assert_eq!(h.state().text, t, "{}", p.display());
    }
}
