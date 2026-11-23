//! S1.12: imágenes en su lugar dentro del editor por bloques.
//!
//! Un párrafo que es solo una imagen (`![texto](img/sello.png)`) se ve como la imagen; un clic o
//! entrar con las flechas muestra su Markdown. [`LocalImages`] las lee de la carpeta del documento
//! con la misma política que la vista previa de IureDesk:
//!
//! - solo rutas relativas dentro de esa carpeta (ni `..`, ni absolutas, ni unidades; la ruta real,
//!   tras seguir enlaces, sigue dentro);
//! - solo PNG, JPEG, GIF, WebP y BMP de como mucho [`MAX_IMAGE_BYTES`];
//! - las de internet no se descargan (el editor muestra el Markdown y lo dice).
//!
//! Decodificar está acotado ([`MAX_SIDE`] por lado, memoria limitada) y una imagen grande se
//! reduce antes de subirla como textura. [`Renderers`] junta varios dibujantes (las imágenes y
//! los diagramas de la app) en uno.

use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use iurefficient_ui::lang::pick;

use crate::diagrams::{DiagramImage, DiagramRenderer};

/// Archivo de imagen más grande que se lee.
pub const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;
/// Lado más grande que se decodifica.
pub const MAX_SIDE: u32 = 8_192;
/// Lado más grande de la textura (las fotos grandes se reducen).
const MAX_TEXTURE_SIDE: u32 = 2_048;
/// Extensiones que se leen.
const EXTS: [&str; 6] = ["png", "jpg", "jpeg", "gif", "webp", "bmp"];

/// La ruta y el texto alternativo de un bloque que es solo una imagen.
pub(crate) fn image_of(block: &str) -> Option<(String, String)> {
    use pulldown_cmark::{Event, Parser, Tag, TagEnd};
    if !block.trim_start().starts_with("![") {
        return None;
    }
    let mut ev = Parser::new(block);
    if !matches!(ev.next()?, Event::Start(Tag::Paragraph)) {
        return None;
    }
    let src = match ev.next()? {
        Event::Start(Tag::Image { dest_url, .. }) => dest_url.to_string(),
        _ => return None,
    };
    let mut alt = String::new();
    loop {
        match ev.next()? {
            Event::End(TagEnd::Image) => break,
            Event::Text(t) | Event::Code(t) => alt.push_str(&t),
            _ => {}
        }
    }
    if !matches!(ev.next()?, Event::End(TagEnd::Paragraph)) {
        return None;
    }
    ev.next().is_none().then_some((src, alt))
}

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

fn is_remote(src: &str) -> bool {
    let l = src.trim().to_ascii_lowercase();
    l.starts_with("http://") || l.starts_with("https://") || l.starts_with("//")
}

/// La ruta real de una imagen relativa a `base`, si cumple la política.
fn resolve(base: &Path, src: &str) -> Result<PathBuf, String> {
    let outside = || {
        pick(
            "The image must be inside the document's folder (a relative path, without “..”).",
            "La imagen debe estar dentro de la carpeta del documento (una ruta relativa, sin «..»).",
        )
        .to_string()
    };
    let rel = percent_decode(src.trim());
    let rel = rel.strip_prefix("./").unwrap_or(&rel);
    let rel_path = Path::new(rel);
    if rel.is_empty() || rel.len() > 1024 || rel.contains('\\') {
        return Err(outside());
    }
    if !rel_path.components().all(|c| matches!(c, Component::Normal(_))) {
        return Err(outside());
    }
    let ext = rel_path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if !EXTS.contains(&ext.as_str()) {
        return Err(pick(
            "Only PNG, JPEG, GIF, WebP and BMP images are shown here.",
            "Aquí solo se ven imágenes PNG, JPEG, GIF, WebP y BMP.",
        )
        .to_string());
    }
    let missing = || format!("{} «{rel}».", pick("The image was not found:", "No se encontró la imagen:"));
    let real_base = base.canonicalize().map_err(|_| missing())?;
    let real = real_base.join(rel_path).canonicalize().map_err(|_| missing())?;
    if !real.starts_with(&real_base) {
        return Err(outside());
    }
    Ok(real)
}

/// Lee y decodifica una imagen acotada; devuelve los píxeles (reducidos si hace falta) y su
/// tamaño original.
fn decode(path: &Path) -> Result<(image::RgbaImage, (u32, u32)), String> {
    let unreadable = || pick("The image could not be read.", "No se pudo leer la imagen.").to_string();
    let meta = std::fs::metadata(path).map_err(|_| unreadable())?;
    if !meta.is_file() || meta.len() > MAX_IMAGE_BYTES {
        return Err(pick("The image is too big (more than 20 MB).", "La imagen es muy grande (más de 20 MB).").into());
    }
    let mut buf = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(MAX_IMAGE_BYTES.saturating_add(1)).read_to_end(&mut buf))
        .map_err(|_| unreadable())?;
    let mut reader =
        image::ImageReader::new(std::io::Cursor::new(buf)).with_guessed_format().map_err(|_| unreadable())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(512 * 1024 * 1024);
    reader.limits(limits);
    let img = reader.decode().map_err(|_| {
        pick(
            "The image is damaged or too large to show.",
            "La imagen está dañada o es demasiado grande para mostrarla.",
        )
        .to_string()
    })?;
    let size = (img.width(), img.height());
    let side = size.0.max(size.1);
    let img = if side > MAX_TEXTURE_SIDE {
        let k = MAX_TEXTURE_SIDE as f32 / side as f32;
        let (w, h) = (((size.0 as f32 * k) as u32).max(1), ((size.1 as f32 * k) as u32).max(1));
        img.resize_exact(w, h, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    Ok((img.to_rgba8(), size))
}

/// Imágenes locales de la carpeta de un documento.
pub struct LocalImages {
    base: Option<PathBuf>,
}

impl LocalImages {
    /// `base`: la carpeta contra la que se resuelven las rutas (la del documento); `None` si el
    /// documento aún no tiene archivo.
    pub fn new(base: Option<PathBuf>) -> Self {
        LocalImages { base }
    }
}

impl DiagramRenderer for LocalImages {
    fn accepts(&self, block: &str) -> bool {
        image_of(block).is_some_and(|(src, _)| {
            let l = src.to_ascii_lowercase();
            !l.ends_with(".svg")
        })
    }

    fn key(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.base.hash(&mut h);
        h.finish()
    }

    fn render(&self, block: &str, _scale: f32) -> Result<DiagramImage, String> {
        let (src, _) = image_of(block).ok_or_else(|| pick("Not an image.", "No es una imagen.").to_string())?;
        if is_remote(&src) {
            return Err(pick(
                "Images from the internet aren't downloaded here.",
                "Las imágenes de internet no se descargan aquí.",
            )
            .into());
        }
        let base = self.base.as_ref().ok_or_else(|| {
            pick("Save the document to see its images.", "Guarda el documento para ver sus imágenes.").to_string()
        })?;
        let (img, (w, h)) = decode(&resolve(base, &src)?)?;
        Ok(DiagramImage {
            width: img.width() as usize,
            height: img.height() as usize,
            rgba: img.into_raw(),
            points: [w as f32, h as f32],
            hotspots: Vec::new(),
        })
    }
}

/// Varios dibujantes como uno: el primero que acepta el bloque lo dibuja.
pub struct Renderers(pub Vec<Arc<dyn DiagramRenderer>>);

impl DiagramRenderer for Renderers {
    fn accepts(&self, block: &str) -> bool {
        self.0.iter().any(|r| r.accepts(block))
    }

    fn key(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for r in &self.0 {
            r.key().hash(&mut h);
        }
        h.finish()
    }

    fn render(&self, block: &str, scale: f32) -> Result<DiagramImage, String> {
        match self.0.iter().find(|r| r.accepts(block)) {
            Some(r) => r.render(block, scale),
            None => Err(pick("Nothing here can draw this block.", "Nada aquí sabe dibujar este bloque.").into()),
        }
    }

    fn placement(&self, block: &str) -> crate::diagrams::Placement {
        self.0.iter().find(|r| r.accepts(block)).map(|r| r.placement(block)).unwrap_or_default()
    }

    fn menu(&self, ui: &mut egui::Ui, block: &str) -> Option<String> {
        self.0.iter().find(|r| r.accepts(block)).and_then(|r| r.menu(ui, block))
    }

    fn node_action(
        &self,
        block: &str,
        id: usize,
        action: crate::diagrams::NodeAction,
    ) -> Option<(String, Option<usize>)> {
        self.0.iter().find(|r| r.accepts(block)).and_then(|r| r.node_action(block, id, action))
    }

    fn spot_actions(&self, block: &str, id: usize) -> Vec<crate::ActionDef> {
        self.0.iter().find(|r| r.accepts(block)).map(|r| r.spot_actions(block, id)).unwrap_or_default()
    }

    fn canvas_actions(&self, block: &str, at: [f32; 2]) -> Vec<crate::ActionDef> {
        self.0.iter().find(|r| r.accepts(block)).map(|r| r.canvas_actions(block, at)).unwrap_or_default()
    }

    fn apply(
        &self,
        block: &str,
        target: crate::Target,
        action: u32,
        data: crate::ActionData,
    ) -> Option<(String, Option<usize>)> {
        self.0.iter().find(|r| r.accepts(block)).and_then(|r| r.apply(block, target, action, data))
    }

    fn spot_label(&self, block: &str, id: usize) -> Option<String> {
        self.0.iter().find(|r| r.accepts(block)).and_then(|r| r.spot_label(block, id))
    }

    fn drag(&self, block: &str, id: usize, to: [f32; 2]) -> Option<String> {
        self.0.iter().find(|r| r.accepts(block)).and_then(|r| r.drag(block, id, to))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(path: &Path, w: u32, h: u32) {
        image::RgbaImage::from_pixel(w, h, image::Rgba([10, 20, 30, 255])).save(path).unwrap();
    }

    #[test]
    fn only_image_paragraphs_are_images() {
        assert_eq!(image_of("![Sello](img/sello.png)"), Some(("img/sello.png".into(), "Sello".into())));
        assert_eq!(image_of("![*a* b](x%20y.png \"t\")"), Some(("x%20y.png".into(), "a b".into())));
        assert!(image_of("Texto ![a](a.png)").is_none());
        assert!(image_of("![a](a.png) y más").is_none());
        assert!(image_of("![a](a.png)\n\n![b](b.png)").is_none());
        assert_eq!(percent_decode("mi%20foto%C3%B1.png"), "mi fotoñ.png");
        assert_eq!(percent_decode("100%"), "100%");
    }

    #[test]
    fn local_images_stay_inside_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("img")).unwrap();
        png(&dir.path().join("img/sello.png"), 30, 20);
        let r = LocalImages::new(Some(dir.path().to_path_buf()));
        let img = r.render("![Sello](img/sello.png)", 1.0).unwrap();
        assert_eq!((img.width, img.height, img.points), (30, 20, [30.0, 20.0]));
        assert_eq!(img.rgba.len(), 30 * 20 * 4);
        for bad in ["![](../x.png)", "![](/etc/x.png)", "![](img/../../x.png)", "![](img\\\\x.png)"] {
            assert!(r.render(bad, 1.0).unwrap_err().contains("carpeta"), "{bad}");
        }
        assert!(r.render("![](https://x.mx/a.png)", 1.0).unwrap_err().contains("internet"));
        assert!(r.render("![](img/no.png)", 1.0).unwrap_err().contains("No se encontró"));
        assert!(r.render("![](img/a.txt)", 1.0).is_err());
        assert!(!r.accepts("![](logo.svg)"), "SVG se queda como texto");
        assert!(LocalImages::new(None).render("![](a.png)", 1.0).unwrap_err().contains("Guarda"));
    }

    #[test]
    fn big_images_are_reduced_and_damaged_ones_explained() {
        let dir = tempfile::tempdir().unwrap();
        png(&dir.path().join("grande.png"), 3000, 1000);
        std::fs::write(dir.path().join("rota.png"), b"no soy png").unwrap();
        let r = LocalImages::new(Some(dir.path().to_path_buf()));
        let img = r.render("![](grande.png)", 1.0).unwrap();
        assert_eq!((img.width, img.height), (2048, 682));
        assert_eq!(img.points, [3000.0, 1000.0], "se ve con su tamaño");
        assert!(r.render("![](rota.png)", 1.0).unwrap_err().contains("dañada"));
    }
}
