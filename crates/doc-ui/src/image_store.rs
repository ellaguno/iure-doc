//! S1.12: pegar y arrastrar imágenes al editor.
//!
//! Una imagen pegada (Ctrl+V con una imagen en el portapapeles) se guarda como PNG y una soltada
//! sobre el editor se copia, las dos en una carpeta junto al documento (`assets/` en IureEditor,
//! como la versión Tauri; `img/` en las notas de IureDesk); el editor inserta
//! `![imagen](assets/….png)`, que [`crate::LocalImages`] dibuja en su lugar.

use std::path::{Path, PathBuf};

/// Imagen más grande que se pega (lado, en píxeles).
const MAX_SIDE: u32 = 8_192;
/// Archivo más grande que se copia al soltarlo.
const MAX_FILE: u64 = 20 * 1024 * 1024;
/// Extensiones de imagen que se aceptan al soltar.
pub(crate) const EXTS: [&str; 6] = ["png", "jpg", "jpeg", "gif", "webp", "bmp"];

/// Píxeles de una imagen del portapapeles (RGBA8, fila a fila).
#[derive(Clone, Debug)]
pub struct PastedImage {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

/// Dónde guarda la app las imágenes de un documento.
pub trait ImageStore: Send + Sync {
    /// Guarda una imagen pegada; devuelve su ruta para el Markdown (relativa al documento).
    fn save_pasted(&self, img: &PastedImage) -> Result<String, String>;
    /// Copia una imagen soltada junto al documento; devuelve su ruta para el Markdown.
    fn import(&self, path: &Path) -> Result<String, String>;
}

/// Las imágenes en una subcarpeta (`sub`) de la carpeta del documento (`base`).
pub struct FolderImages {
    base: Option<PathBuf>,
    sub: String,
}

impl FolderImages {
    /// `base`: la carpeta del documento (`None` si aún no se guardó); `sub`: `assets`, `img`…
    pub fn new(base: Option<PathBuf>, sub: &str) -> Self {
        FolderImages { base, sub: sub.trim_matches('/').to_string() }
    }

    fn dir(&self) -> Result<(PathBuf, PathBuf), String> {
        let base = self.base.clone().ok_or_else(|| {
            iurefficient_ui::lang::pick(
                "Save the document to add images.",
                "Guarda el documento para agregar imágenes.",
            )
            .to_string()
        })?;
        let dir = base.join(&self.sub);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{e}"))?;
        Ok((base, dir))
    }

    /// Un nombre libre en `dir` a partir de `stem` y `ext`.
    fn free_name(dir: &Path, stem: &str, ext: &str) -> String {
        let first = format!("{stem}.{ext}");
        if !dir.join(&first).exists() {
            return first;
        }
        (2..10_000).map(|n| format!("{stem}-{n}.{ext}")).find(|n| !dir.join(n).exists()).unwrap_or(first)
    }

    fn rel(&self, name: &str) -> String {
        format!("{}/{name}", self.sub)
    }
}

/// Fecha y hora UTC (`AAAA-MM-DD-HHMMSS`) de unos segundos desde 1970, sin dependencias.
fn stamp(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Días civiles (algoritmo de Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}-{:02}{:02}{:02}", rem / 3_600, rem % 3_600 / 60, rem % 60)
}

/// Un nombre de archivo seguro para el Markdown (sin espacios ni separadores).
fn clean_stem(stem: &str) -> String {
    let s: String = stem
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '-' })
        .take(80)
        .collect();
    // Varios guiones seguidos (de «Mi Foto (1)») quedan en uno.
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if !(c == '-' && out.ends_with('-')) {
            out.push(c);
        }
    }
    let s = out.trim_matches(['-', '.']).to_string();
    if s.is_empty() { "imagen".into() } else { s }
}

impl ImageStore for FolderImages {
    fn save_pasted(&self, img: &PastedImage) -> Result<String, String> {
        let pick = iurefficient_ui::lang::pick;
        let (w, h) = (u32::try_from(img.width).unwrap_or(u32::MAX), u32::try_from(img.height).unwrap_or(u32::MAX));
        if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
            return Err(pick("The image is too big to paste.", "La imagen es demasiado grande para pegarla.").into());
        }
        let buf = image::RgbaImage::from_raw(w, h, img.rgba.clone())
            .ok_or_else(|| pick("The pasted image is damaged.", "La imagen pegada está dañada.").to_string())?;
        let (_, dir) = self.dir()?;
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let name = Self::free_name(&dir, &format!("imagen-{}", stamp(secs)), "png");
        buf.save_with_format(dir.join(&name), image::ImageFormat::Png).map_err(|e| format!("{e}"))?;
        Ok(self.rel(&name))
    }

    fn import(&self, path: &Path) -> Result<String, String> {
        let pick = iurefficient_ui::lang::pick;
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        if !EXTS.contains(&ext.as_str()) {
            return Err(pick("Only images can be dropped here.", "Aquí solo se pueden soltar imágenes.").into());
        }
        let meta = std::fs::metadata(path).map_err(|e| format!("{e}"))?;
        if !meta.is_file() || meta.len() > MAX_FILE {
            return Err(
                pick("The image is too big (more than 20 MB).", "La imagen es muy grande (más de 20 MB).").into()
            );
        }
        let (base, dir) = self.dir()?;
        // Ya está junto al documento: se usa tal cual.
        if let (Ok(real), Ok(rb)) = (path.canonicalize(), base.canonicalize())
            && let Ok(rel) = real.strip_prefix(&rb)
            && rel.components().all(|c| matches!(c, std::path::Component::Normal(_)))
            && let Some(s) = rel.to_str()
            && !s.contains(' ')
        {
            return Ok(s.replace('\\', "/"));
        }
        let stem = clean_stem(path.file_stem().and_then(|s| s.to_str()).unwrap_or("imagen"));
        let name = Self::free_name(&dir, &stem, &ext);
        std::fs::copy(path, dir.join(&name)).map_err(|e| format!("{e}"))?;
        Ok(self.rel(&name))
    }
}

/// El Markdown de una imagen insertada; sin `alt`, el texto alternativo sale del nombre.
pub(crate) fn markdown(rel: &str, alt: Option<&str>) -> String {
    let stem = rel.rsplit('/').next().unwrap_or(rel).rsplit_once('.').map_or(rel, |(s, _)| s);
    let alt: String = match alt {
        Some(a) => a.to_string(),
        None => stem.chars().map(|c| if c == '-' || c == '_' { ' ' } else { c }).collect(),
    };
    format!("![{}]({rel})", alt.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_without_dependencies() {
        assert_eq!(stamp(0), "1970-01-01-000000");
        assert_eq!(stamp(1_795_015_412), "2026-11-18-152332");
        assert_eq!(stamp(951_782_400), "2000-02-29-000000", "año bisiesto");
    }

    #[test]
    fn pasted_images_are_saved_as_png_next_to_the_document() {
        let dir = tempfile::tempdir().unwrap();
        let store = FolderImages::new(Some(dir.path().to_path_buf()), "assets");
        let img = PastedImage { width: 3, height: 2, rgba: vec![200; 24] };
        let a = store.save_pasted(&img).unwrap();
        let b = store.save_pasted(&img).unwrap();
        assert!(a.starts_with("assets/imagen-") && a.ends_with(".png"));
        assert_ne!(a, b, "nombres únicos");
        let back = image::open(dir.path().join(&a)).unwrap();
        assert_eq!((back.width(), back.height()), (3, 2));
        let bad = PastedImage { width: 3, height: 2, rgba: vec![0; 5] };
        assert!(store.save_pasted(&bad).is_err());
        let none = FolderImages::new(None, "assets");
        assert!(none.save_pasted(&img).unwrap_err().contains("Guarda"));
    }

    #[test]
    fn dropped_images_are_copied_and_named_safely() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let src = outside.path().join("Mi Foto (1).png");
        image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 255])).save(&src).unwrap();
        let store = FolderImages::new(Some(dir.path().to_path_buf()), "img");
        assert_eq!(store.import(&src).unwrap(), "img/Mi-Foto-1.png");
        assert_eq!(store.import(&src).unwrap(), "img/Mi-Foto-1-2.png", "sin pisar");
        // Ya dentro de la carpeta del documento: no se copia.
        let inside = dir.path().join("fotos");
        std::fs::create_dir_all(&inside).unwrap();
        std::fs::copy(&src, inside.join("a.png")).unwrap();
        assert_eq!(store.import(&inside.join("a.png")).unwrap(), "fotos/a.png");
        let txt = outside.path().join("nota.txt");
        std::fs::write(&txt, "x").unwrap();
        assert!(store.import(&txt).is_err());
        assert_eq!(markdown("img/Mi-Foto-1.png", None), "![Mi Foto 1](img/Mi-Foto-1.png)");
        assert_eq!(markdown("assets/imagen-x.png", Some("imagen")), "![imagen](assets/imagen-x.png)");
    }
}
