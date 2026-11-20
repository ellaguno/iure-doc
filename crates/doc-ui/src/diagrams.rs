//! S1.12: diagramas dibujados en su lugar dentro del editor por bloques.
//!
//! El editor no sabe dibujar diagramas (eso pesa: Mermaid, Blitz…): la app anfitriona le pasa un
//! [`DiagramRenderer`] que reconoce los bloques que sabe dibujar (```` ```mermaid ````, mapas
//! mentales…) y los convierte en píxeles. El editor los pide en otro hilo, guarda el resultado
//! como textura y muestra la imagen en el lugar del bloque; un clic (o entrar con las flechas)
//! muestra el código para editarlo, y al dejarlo se vuelve a dibujar.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

/// Píxeles de un diagrama dibujado.
#[derive(Clone, Debug)]
pub struct DiagramImage {
    pub width: usize,
    pub height: usize,
    /// RGBA8 sin premultiplicar, fila a fila (`width * height * 4` bytes).
    pub rgba: Vec<u8>,
    /// Tamaño con que se ve, en puntos de egui (los píxeles pueden ser más, para pantallas densas).
    pub points: [f32; 2],
}

/// Lo que la app anfitriona sabe dibujar.
pub trait DiagramRenderer: Send + Sync {
    /// ¿Este bloque (su Markdown completo) es un diagrama que sabe dibujar? Barato: se llama en
    /// cada cuadro para los bloques visibles.
    fn accepts(&self, block: &str) -> bool;
    /// Huella de lo que cambia el dibujo además del bloque (tema, preferencias): parte de la llave
    /// de la caché.
    fn key(&self) -> u64;
    /// Dibuja el bloque a `scale` píxeles por punto. Puede tardar: se llama en otro hilo. Los
    /// errores van en lenguaje humano (se muestran debajo del código).
    fn render(&self, block: &str, scale: f32) -> Result<DiagramImage, String>;
    /// Dónde va el dibujo respecto del texto del bloque (un mapa mental puede verse con su lista).
    fn placement(&self, _block: &str) -> Placement {
        Placement::Replace
    }
    /// Menú de clic derecho sobre el dibujo (opciones del diagrama). Devuelve el bloque nuevo si se
    /// eligió algo.
    fn menu(&self, _ui: &mut egui::Ui, _block: &str) -> Option<String> {
        None
    }
}

/// Dónde se ve el dibujo de un bloque.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Placement {
    /// En lugar del texto (un clic lo edita).
    #[default]
    Replace,
    /// El texto y, debajo, el dibujo.
    Below,
    /// El texto a la izquierda y el dibujo a la derecha.
    Right,
}

/// Diagramas guardados como mucho.
const MAX_SLOTS: usize = 64;

#[derive(Clone)]
enum Slot {
    Pending,
    Done(Arc<DiagramImage>),
    Ready(egui::TextureHandle, egui::Vec2),
    Failed(String),
}

/// Cómo está un diagrama.
pub(crate) enum View {
    Pending,
    Image(egui::TextureHandle, egui::Vec2),
    Failed(String),
}

/// Caché de diagramas de un editor (compartida con los hilos que dibujan).
#[derive(Clone, Default)]
pub(crate) struct Cache {
    slots: Arc<Mutex<HashMap<u64, (Slot, u64)>>>,
}

fn key_of(block: &str, renderer: u64, scale: f32) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    block.hash(&mut h);
    renderer.hash(&mut h);
    scale.to_bits().hash(&mut h);
    h.finish()
}

impl Cache {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<u64, (Slot, u64)>> {
        self.slots.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// El diagrama de `block`: si no está, se pide (en otro hilo) y queda pendiente.
    pub(crate) fn get(&self, ctx: &egui::Context, renderer: &Arc<dyn DiagramRenderer>, block: &str) -> View {
        let scale = ctx.pixels_per_point();
        let key = key_of(block, renderer.key(), scale);
        let frame = ctx.cumulative_frame_nr();
        let mut slots = self.lock();
        let slot = match slots.get_mut(&key) {
            Some((slot, used)) => {
                *used = frame;
                slot.clone()
            }
            None => {
                if slots.len() >= MAX_SLOTS {
                    // Fuera el que lleva más tiempo sin verse.
                    if let Some(old) = slots.iter().min_by_key(|(_, (_, used))| *used).map(|(k, _)| *k) {
                        slots.remove(&old);
                    }
                }
                slots.insert(key, (Slot::Pending, frame));
                let (cache, r, b, c) = (self.clone(), Arc::clone(renderer), block.to_string(), ctx.clone());
                std::thread::spawn(move || {
                    let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| r.render(&b, scale)));
                    let slot = match out {
                        Ok(Ok(img)) if img.rgba.len() == img.width.saturating_mul(img.height).saturating_mul(4) => {
                            Slot::Done(Arc::new(img))
                        }
                        Ok(Ok(_)) => Slot::Failed(
                            iurefficient_ui::lang::pick(
                                "The drawing came out incomplete.",
                                "El dibujo llegó incompleto.",
                            )
                            .into(),
                        ),
                        Ok(Err(e)) => Slot::Failed(e),
                        Err(_) => Slot::Failed(
                            iurefficient_ui::lang::pick(
                                "The diagram could not be drawn.",
                                "No se pudo dibujar el diagrama.",
                            )
                            .into(),
                        ),
                    };
                    if let Some(s) = cache.lock().get_mut(&key) {
                        s.0 = slot;
                    }
                    c.request_repaint();
                });
                Slot::Pending
            }
        };
        match slot {
            Slot::Pending => View::Pending,
            Slot::Failed(e) => View::Failed(e),
            Slot::Ready(t, s) => View::Image(t, s),
            Slot::Done(img) => {
                let color = egui::ColorImage::from_rgba_unmultiplied([img.width, img.height], &img.rgba);
                let tex = ctx.load_texture(format!("iure-doc-diagram-{key}"), color, egui::TextureOptions::LINEAR);
                let size = egui::vec2(img.points[0].max(1.0), img.points[1].max(1.0));
                if let Some(s) = slots.get_mut(&key) {
                    s.0 = Slot::Ready(tex.clone(), size);
                }
                View::Image(tex, size)
            }
        }
    }
}
