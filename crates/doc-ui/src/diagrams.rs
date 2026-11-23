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

/// Una zona del dibujo que se puede pulsar (un nodo de un mapa mental): su número para la app y
/// su rectángulo en puntos, desde la esquina del dibujo (`[x0, y0, x1, y1]`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hotspot {
    pub id: usize,
    pub rect: [f32; 4],
    /// Se puede arrastrar (un punto de un diagrama de cuadrantes): al soltarlo se llama a
    /// [`DiagramRenderer::drag`].
    pub draggable: bool,
}

/// Lo que pide una acción antes de hacerse (S1.33).
#[derive(Clone, Debug, PartialEq)]
pub enum ActionInput {
    /// Nada: se hace al pulsarla.
    None,
    /// Un texto, en un globo junto al dibujo (`prompt` lo explica; `initial` ya viene escrito).
    Text { prompt: String, initial: String },
    /// Elegir una de estas opciones (sus nombres).
    Choice(Vec<String>),
    /// Elegir una opción y luego escribir un texto en el globo («Agregar siguiente»: la forma y su
    /// texto).
    ChoiceText { options: Vec<String>, prompt: String },
    /// Pulsar otra zona del dibujo (`prompt` lo explica: «Elige el nodo de destino»).
    OtherSpot(String),
}

/// Una acción que la app ofrece sobre una zona del dibujo o sobre un hueco.
#[derive(Clone, Debug, PartialEq)]
pub struct ActionDef {
    /// Su número para la app.
    pub id: u32,
    pub icon: iurefficient_ui::icons::Icon,
    /// Nombre (se ve al pasar y lo lee el lector de pantalla).
    pub label: String,
    pub input: ActionInput,
}

/// Sobre qué se hace una acción.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    /// Una zona ([`Hotspot::id`]).
    Spot(usize),
    /// Un hueco del dibujo, en puntos desde su esquina (como [`Hotspot::rect`]).
    Canvas([f32; 2]),
}

/// El dato que pidió la acción.
#[derive(Clone, Debug, PartialEq)]
pub enum ActionData {
    None,
    Text(String),
    /// El número de la opción elegida.
    Choice(usize),
    /// La opción elegida y el texto.
    ChoiceText(usize, String),
    /// La otra zona pulsada.
    Spot(usize),
}

/// Lo que se pide sobre una zona del dibujo (barra de un nodo de un mapa mental).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeAction {
    AddChild,
    AddSibling,
    Delete,
    /// Llevar el cursor a su texto (doble clic).
    Goto,
}

/// Píxeles de un diagrama dibujado.
#[derive(Clone, Debug, Default)]
pub struct DiagramImage {
    pub width: usize,
    pub height: usize,
    /// RGBA8 sin premultiplicar, fila a fila (`width * height * 4` bytes).
    pub rgba: Vec<u8>,
    /// Tamaño con que se ve, en puntos de egui (los píxeles pueden ser más, para pantallas densas).
    pub points: [f32; 2],
    /// Zonas que se pueden pulsar (vacío: ninguna).
    pub hotspots: Vec<Hotspot>,
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
    /// Una acción sobre la zona `id` del dibujo: el bloque nuevo y, si se da, dónde dejar el cursor
    /// (carácter del bloque).
    fn node_action(&self, _block: &str, _id: usize, _action: NodeAction) -> Option<(String, Option<usize>)> {
        None
    }
    /// S1.33: las acciones de la zona `id` (vacío: la barra de los mapas mentales, con
    /// [`DiagramRenderer::node_action`]).
    fn spot_actions(&self, _block: &str, _id: usize) -> Vec<ActionDef> {
        Vec::new()
    }
    /// Las acciones de un hueco del dibujo (`at`, en puntos): «agregar aquí». Vacío: un clic en el
    /// hueco edita el código, como siempre.
    fn canvas_actions(&self, _block: &str, _at: [f32; 2]) -> Vec<ActionDef> {
        Vec::new()
    }
    /// Hace la acción `action` sobre `target` con su dato: el bloque nuevo y, si se da, dónde dejar
    /// el cursor (carácter del bloque).
    fn apply(&self, _block: &str, _target: Target, _action: u32, _data: ActionData) -> Option<(String, Option<usize>)> {
        None
    }
    /// La zona arrastrable `id` se soltó en `to` (puntos desde la esquina del dibujo): el bloque
    /// nuevo.
    fn drag(&self, _block: &str, _id: usize, _to: [f32; 2]) -> Option<String> {
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
    Ready(egui::TextureHandle, egui::Vec2, Arc<Vec<Hotspot>>),
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
    /// Las zonas del dibujo de `block`, si ya está listo.
    pub(crate) fn hotspots(
        &self,
        ctx: &egui::Context,
        renderer: &Arc<dyn DiagramRenderer>,
        block: &str,
    ) -> Arc<Vec<Hotspot>> {
        let key = key_of(block, renderer.key(), ctx.pixels_per_point());
        match self.lock().get(&key) {
            Some((Slot::Ready(_, _, h), _)) => Arc::clone(h),
            _ => Arc::new(Vec::new()),
        }
    }

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
            Slot::Ready(t, s, _) => View::Image(t, s),
            Slot::Done(img) => {
                let color = egui::ColorImage::from_rgba_unmultiplied([img.width, img.height], &img.rgba);
                let tex = ctx.load_texture(format!("iure-doc-diagram-{key}"), color, egui::TextureOptions::LINEAR);
                let size = egui::vec2(img.points[0].max(1.0), img.points[1].max(1.0));
                if let Some(s) = slots.get_mut(&key) {
                    s.0 = Slot::Ready(tex.clone(), size, Arc::new(img.hotspots.clone()));
                }
                View::Image(tex, size)
            }
        }
    }
}
