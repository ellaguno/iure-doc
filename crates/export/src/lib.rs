//! S1.14: exportar un documento de la suite (Markdown de [`iure_doc`]) a otros formatos, sin egui
//! ni nada fuera de Rust:
//!
//! - [`html`]: una página HTML **autocontenida** (estilos dentro, imágenes y diagramas incrustados);
//!   se abre en cualquier navegador y se puede mandar por correo.
//! - [`docx`]: un documento de Word (WordprocessingML escrito aquí, empaquetado con `zip`): títulos
//!   con estilos de Word (el índice de Word los reconoce), listas numeradas de verdad, tablas,
//!   imágenes y diagramas como imágenes.
//! - `pdf` (característica `pdf`): PDF con Typst compilado aquí y sus letras incluidas.
//!
//! Imágenes y diagramas los pone la app con [`Resources`]: el exportador no lee nada por su cuenta.
//! [`FolderResources`] da las imágenes locales con la misma política que el editor (solo dentro de
//! la carpeta del documento); la app le agrega los diagramas con el motor que ya usa para dibujarlos.
//!
//! Todo lo que llega es hostil: textos escapados, enlaces `javascript:` fuera, tamaños acotados.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]

pub mod docx;
pub mod html;
#[cfg(feature = "math")]
pub mod math;
#[cfg(feature = "pdf")]
pub mod pdf;
mod text;
#[cfg(feature = "math")]
mod world;

use std::path::{Component, Path, PathBuf};

use iure_doc::{Block, Document};

/// Imagen más grande que se incrusta.
pub const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;
/// Lado más grande de una imagen que se incrusta (en píxeles).
pub const MAX_SIDE: u32 = 16_384;

/// Una imagen lista para incrustar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    /// Los bytes del archivo (PNG, JPEG, GIF, WebP o BMP).
    pub bytes: Vec<u8>,
    /// `image/png`, `image/jpeg`…
    pub mime: String,
    /// Tamaño en píxeles.
    pub width: u32,
    pub height: u32,
}

/// Un diagrama dibujado por la app.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Diagram {
    /// SVG (preferido en HTML: se ve nítido a cualquier tamaño).
    pub svg: Option<String>,
    /// PNG (DOCX y, si no hay SVG, HTML).
    pub png: Option<Picture>,
    /// Tamaño en puntos (1/72 de pulgada) con que se debe ver.
    pub points: (f32, f32),
}

/// Lo que el exportador pide a la app.
pub trait Resources {
    /// La imagen de `src` (tal como está en el Markdown), o `None` (se exporta su texto
    /// alternativo).
    fn image(&self, src: &str) -> Option<Picture>;
    /// El dibujo del bloque `block` (su Markdown: ```` ```mermaid ````, `> [!MINDMAP…]`), o `None`
    /// si no es un diagrama o no se pudo dibujar (se exporta como código).
    fn diagram(&self, block: &str) -> Option<Diagram>;
}

/// Sin imágenes ni diagramas (solo texto).
pub struct NoResources;

impl Resources for NoResources {
    fn image(&self, _src: &str) -> Option<Picture> {
        None
    }
    fn diagram(&self, _block: &str) -> Option<Diagram> {
        None
    }
}

/// Opciones de la exportación.
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// Título del documento; vacío: el `title:` de la cabecera YAML o el primer título.
    pub title: String,
    /// Idioma (`es`, `en`); vacío: `es`.
    pub lang: String,
    /// Tamaño del papel (DOCX).
    pub paper: Paper,
}

/// Tamaño del papel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Paper {
    #[default]
    Letter,
    A4,
}

/// Un error al exportar (en los dos idiomas: lo muestra la app).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportError {
    pub en: String,
    pub es: String,
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.en)
    }
}

impl std::error::Error for ExportError {}

/// El título del documento: el de las opciones, el `title:` de la cabecera o el primer título.
pub fn title_of(doc: &Document, opts: &Options) -> String {
    if !opts.title.trim().is_empty() {
        return opts.title.trim().to_string();
    }
    for n in doc.nodes() {
        match &n.block {
            Block::FrontMatter(y) => {
                if let Some(t) = y.lines().find_map(|l| l.trim().strip_prefix("title:")) {
                    let t = t.trim().trim_matches(['"', '\'']).trim();
                    if !t.is_empty() {
                        return t.to_string();
                    }
                }
            }
            Block::Heading { content, .. } => {
                let t = text::plain(content);
                if !t.trim().is_empty() {
                    return t.trim().to_string();
                }
            }
            _ => {}
        }
    }
    String::new()
}

/// Imágenes locales con la política del editor: rutas relativas dentro de `base` (sin `..` ni
/// enlaces que salgan), extensiones de imagen, hasta [`MAX_IMAGE_BYTES`]. Las de internet no se
/// descargan. Sin diagramas: la app envuelve este tipo y agrega los suyos.
pub struct FolderResources {
    base: Option<PathBuf>,
}

impl FolderResources {
    /// `base`: la carpeta del documento (`None`: sin guardar, sin imágenes).
    pub fn new(base: Option<PathBuf>) -> Self {
        FolderResources { base }
    }
}

const EXTS: [&str; 6] = ["png", "jpg", "jpeg", "gif", "webp", "bmp"];

/// `%20` y compañía de una ruta de Markdown.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        if b == b'%'
            && let Some(hex) = s.get(i + 1..i + 3)
            && let Ok(v) = u8::from_str_radix(hex, 16)
        {
            out.push(v);
            i += 3;
        } else {
            out.push(b);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

/// La ruta real de una imagen relativa a `base`, si cumple la política.
fn resolve(base: &Path, src: &str) -> Option<PathBuf> {
    let rel = percent_decode(src.trim());
    let rel = rel.strip_prefix("./").unwrap_or(&rel);
    let rel_path = Path::new(rel);
    if rel.is_empty() || rel.len() > 1024 || rel.contains('\\') || rel.contains(':') {
        return None;
    }
    if !rel_path.components().all(|c| matches!(c, Component::Normal(_))) {
        return None;
    }
    let ext = rel_path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if !EXTS.contains(&ext.as_str()) {
        return None;
    }
    let real_base = base.canonicalize().ok()?;
    let real = real_base.join(rel_path).canonicalize().ok()?;
    real.starts_with(&real_base).then_some(real)
}

/// Lee una imagen acotada y averigua su tipo y tamaño (sin decodificarla entera).
pub fn read_picture(path: &Path) -> Option<Picture> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_IMAGE_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    picture_from_bytes(bytes)
}

/// Tipo y tamaño de unos bytes de imagen; `None` si no es una imagen conocida o es enorme.
pub fn picture_from_bytes(bytes: Vec<u8>) -> Option<Picture> {
    if bytes.len() as u64 > MAX_IMAGE_BYTES {
        return None;
    }
    let format = image::guess_format(&bytes).ok()?;
    let mime = match format {
        image::ImageFormat::Png => "image/png",
        image::ImageFormat::Jpeg => "image/jpeg",
        image::ImageFormat::Gif => "image/gif",
        image::ImageFormat::WebP => "image/webp",
        image::ImageFormat::Bmp => "image/bmp",
        _ => return None,
    };
    let reader = image::ImageReader::with_format(std::io::Cursor::new(&bytes), format);
    let (width, height) = reader.into_dimensions().ok()?;
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return None;
    }
    Some(Picture { bytes, mime: mime.to_string(), width, height })
}

impl Resources for FolderResources {
    fn image(&self, src: &str) -> Option<Picture> {
        let l = src.trim().to_ascii_lowercase();
        if l.starts_with("http:") || l.starts_with("https:") || l.starts_with("//") || l.starts_with("data:") {
            return None;
        }
        read_picture(&resolve(self.base.as_deref()?, src)?)
    }
    fn diagram(&self, _block: &str) -> Option<Diagram> {
        None
    }
}

#[cfg(test)]
mod tests;
