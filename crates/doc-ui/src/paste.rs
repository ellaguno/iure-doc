//! S1.12: pegar HTML como Markdown.
//!
//! egui solo entrega el texto plano al pegar. Si la app da una [`PasteSource`] (normalmente
//! [`SystemClipboard`]), el editor pide además el HTML del portapapeles y, si lo hay, pega su
//! Markdown ([`iure_doc::html::to_markdown`]): títulos, listas, tablas, enlaces y estilos de la
//! web, Word o Google Docs llegan como Markdown. Dentro de un bloque de código se pega el texto
//! plano.

/// De dónde sale el HTML del portapapeles.
pub trait PasteSource: Send + Sync {
    /// El HTML que hay ahora en el portapapeles, si lo hay.
    fn html(&self) -> Option<String>;
    /// La imagen que hay ahora en el portapapeles, si la hay.
    fn image(&self) -> Option<crate::PastedImage> {
        None
    }
}

/// El portapapeles del sistema (`arboard`: X11/Wayland, Windows, macOS).
pub struct SystemClipboard;

impl PasteSource for SystemClipboard {
    fn html(&self) -> Option<String> {
        let html = arboard::Clipboard::new().ok()?.get().html().ok()?;
        (!html.trim().is_empty()).then_some(html)
    }

    fn image(&self) -> Option<crate::PastedImage> {
        let img = arboard::Clipboard::new().ok()?.get_image().ok()?;
        Some(crate::PastedImage { width: img.width, height: img.height, rgba: img.bytes.into_owned() })
    }
}

/// ¿El bloque es un bloque de código (ahí se pega texto plano)?
pub(crate) fn in_code_block(block: &str) -> bool {
    let t = block.trim_start();
    t.starts_with("```") || t.starts_with("~~~") || block.starts_with("    ")
}

/// ¿El Markdown pegado son bloques (títulos, listas, varios párrafos…) y no solo texto en línea?
fn is_blockish(md: &str) -> bool {
    let t = md.trim_start();
    let digits = t.chars().take_while(char::is_ascii_digit).count();
    md.contains('\n')
        || t.starts_with('#')
        || t.starts_with("- ")
        || t.starts_with("* ")
        || t.starts_with("+ ")
        || t.starts_with('>')
        || t.starts_with('|')
        || t.starts_with("```")
        || t.starts_with("![")
        || (digits > 0 && matches!(t.get(digits..digits + 1), Some(".") | Some(")")))
}

/// Cambia el texto de los `Paste` de este cuadro por el Markdown del HTML del portapapeles.
/// `line_start` / `line_end`: ¿el cursor está al principio / al final de su línea? Unos bloques
/// pegados a media línea van en bloques propios (un título no se pega al final de un párrafo).
pub(crate) fn rewrite(ui: &egui::Ui, source: &dyn PasteSource, line_start: bool, line_end: bool) {
    if !ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Paste(_)))) {
        return;
    }
    let Some(html) = source.html() else { return };
    let md = iure_doc::html::to_markdown(&html);
    if md.trim().is_empty() {
        return;
    }
    let md = if is_blockish(&md) {
        let before = if line_start { "" } else { "\n\n" };
        let after = if line_end { "" } else { "\n\n" };
        format!("{before}{md}{after}")
    } else {
        md
    };
    ui.input_mut(|i| {
        for e in &mut i.events {
            if let egui::Event::Paste(t) = e {
                *t = md.clone();
            }
        }
    });
}
