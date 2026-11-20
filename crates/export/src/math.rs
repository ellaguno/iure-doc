//! Fórmulas dibujadas: LaTeX → Typst ([`iure_doc::math`]) → imagen (`typst-render`, Rust puro).
//! El editor las pinta en su lugar y el PDF las lleva de verdad.
//!
//! La fórmula convertida entra a Typst como **cadena** evaluada en modo matemático
//! (`eval(mode: "math", "…")`): el texto del documento nunca es código. Si Typst no la entiende,
//! [`checked`] devuelve `None` y quien la usa muestra el LaTeX tal cual.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use typst::syntax::Source;

use crate::ExportError;
use crate::world::{DocWorld, file_id};

/// Fórmula más larga que se dibuja.
const MAX_LATEX: usize = 8_000;
/// Lado máximo de la imagen, en píxeles.
const MAX_SIDE: f32 = 4_000.0;
/// Resultados de [`checked`] que se recuerdan.
const MAX_CACHE: usize = 2_000;

/// Una fórmula dibujada: RGBA sin premultiplicar y su tamaño en puntos.
#[derive(Clone, Debug)]
pub struct MathImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    /// Tamaño en puntos (de Typst = de egui a escala 1).
    pub points: (f32, f32),
}

fn fail(detail: &str) -> ExportError {
    ExportError {
        en: format!("The formula could not be drawn ({detail})."),
        es: format!("No se pudo dibujar la fórmula ({detail})."),
    }
}

/// Una cadena de Typst escapada.
fn lit(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// La expresión de Typst (código) de una fórmula ya convertida.
pub(crate) fn equation(typst_math: &str, display: bool) -> String {
    format!("math.equation(block: {display}, eval(mode: \"math\", {}))", lit(typst_math))
}

fn compile(source: String) -> Result<typst_layout::PagedDocument, ExportError> {
    let id = file_id("/math.typ").ok_or_else(|| fail("id"))?;
    let world = DocWorld { main: Source::new(id, source), files: HashMap::new() };
    typst::compile(&world)
        .output
        .map_err(|errs| fail(&errs.first().map_or_else(String::new, |e| e.message.to_string())))
}

fn page_source(typst_math: &str, display: bool, size_pt: f32, color: [u8; 3]) -> String {
    let [r, g, b] = color;
    format!(
        "#set page(width: auto, height: auto, margin: (x: 1pt, y: 2pt), fill: none)\n\
         #set text(size: {size_pt}pt, fill: rgb({r}, {g}, {b}))\n\
         #{}\n",
        equation(typst_math, display)
    )
}

/// La fórmula en Typst si Typst la entiende (se recuerda el resultado).
pub fn checked(latex: &str) -> Option<String> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(hit) = cache.lock().ok().and_then(|c| c.get(latex).cloned()) {
        return hit;
    }
    let out = (latex.len() <= MAX_LATEX)
        .then(|| iure_doc::math::latex_to_typst(latex))
        .filter(|t| compile(page_source(t, true, 11.0, [0, 0, 0])).is_ok());
    if let Ok(mut c) = cache.lock() {
        if c.len() >= MAX_CACHE {
            c.clear();
        }
        c.insert(latex.to_string(), out.clone());
    }
    out
}

/// Dibuja la fórmula (`display`: en bloque, centrada y más grande) con letra de `size_pt` puntos,
/// en `color`, a `scale` píxeles por punto.
pub fn render_math(
    latex: &str,
    display: bool,
    size_pt: f32,
    color: [u8; 3],
    scale: f32,
) -> Result<MathImage, ExportError> {
    if latex.trim().is_empty() || latex.len() > MAX_LATEX {
        return Err(fail("vacía o muy larga"));
    }
    let t = iure_doc::math::latex_to_typst(latex);
    let doc = compile(page_source(&t, display, size_pt.clamp(4.0, 96.0), color))?;
    let page = doc.pages().first().ok_or_else(|| fail("sin página"))?;
    let size = page.frame.size();
    let (wp, hp) = (size.x.to_pt() as f32, size.y.to_pt() as f32);
    let scale = scale.clamp(0.5, 4.0).min(MAX_SIDE / wp.max(hp).max(1.0));
    let opts = typst_render::RenderOptions {
        pixel_per_pt: typst::utils::Scalar::new(f64::from(scale)),
        ..Default::default()
    };
    let pix = typst_render::render(page, &opts);
    let rgba: Vec<u8> = pix
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    Ok(MathImage { width: pix.width(), height: pix.height(), rgba, points: (wp, hp) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formulas_are_drawn_and_bad_ones_are_rejected() {
        let img = render_math(r"\frac{a+b}{2} = \sqrt{x^2 + y^2}", true, 14.0, [20, 20, 20], 2.0).unwrap();
        assert!(img.width > 40 && img.height > 20, "{}x{}", img.width, img.height);
        assert_eq!(img.rgba.len(), (img.width * img.height * 4) as usize);
        assert!(img.rgba.chunks(4).any(|p| p[3] > 200), "algo se dibujó");
        assert!(checked(r"\sum_{i=1}^{n} i = \frac{n(n+1)}{2}").is_some());
        assert!(checked(r"\begin{pmatrix} 1 & 0 \\ 0 & 1 \end{pmatrix}").is_some());
        assert!(checked(r"\frac{(}{2}").is_none(), "un paréntesis suelto en un argumento");
        // El texto del documento no es código.
        assert!(checked(r#"\text{#read("/etc/passwd")}"#).is_some());
    }
}
