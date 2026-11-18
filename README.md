# iure-doc

El documento de la suite Iurefficient, **100 % en Rust**:

- **`iure-doc`** (sin E/S ni egui): Markdown como modelo de bloques que se lee y se vuelve a
  escribir **sin pérdidas** (lo que no se toca queda byte a byte), metadatos por bloque
  (`<!-- iure:segment {...} -->`), avisos y mapas mentales de IureEditor, teclas del editor como
  funciones puras (`edit`) y `chunks` para editores por bloques.
- **`iure-doc-ui`** (egui 0.37): `DocEditor`, Markdown con formato al teclear. `show` es un solo
  campo (notas cortas); `show_blocks` es el editor por bloques para documentos largos (~1 ms por
  tecla con 100 páginas), con deshacer de todo el documento, selección que cruza bloques tablas editables en su lugar (`table`: cuadrícula sin pérdidas para lo que no se toca) y diagramas dibujados en su lugar (`show_blocks_with` con un `DiagramRenderer` de la app: el editor no dibuja, la app sí) e imágenes locales en su lugar (`LocalImages`, solo dentro de la carpeta del documento; `Renderers` junta dibujantes); pegar HTML como Markdown (`iure_doc::html`, y en el editor con `Hooks { paste: Some(SystemClipboard) }`) y corrector ortográfico (`Speller`, diccionarios Hunspell del sistema con `spellbook`; `Hooks { spell }`).

Lo usan IureDesk (notas), IureProject (notas de tareas) e IureEditor 2.0. El plan está en
[`iuredesk/docs/suite-integracion.md`](https://github.com/ellaguno/iuredesk/blob/main/docs/suite-integracion.md)
§4.5–4.7 (tareas S1.11–S1.17).

## Usarlo

```toml
iure-doc-ui = { git = "https://github.com/ellaguno/iure-doc", tag = "v0.11.0" }
```

Para trabajar a la vez en este repositorio y en una app, sin publicar una etiqueta, en la app
(sin subirlo):

```toml
# .cargo/config.toml
[patch."https://github.com/ellaguno/iure-doc"]
iure-doc = { path = "../iure-doc/crates/doc" }
iure-doc-ui = { path = "../iure-doc/crates/doc-ui" }
```

## Aceptación (S1.22)

- `tests/corpus/`: documentos reales (golden de IureEditor, plantillas, README, CHANGELOG, casos
  difíciles, CRLF) idénticos al abrir y guardar, en el modelo y en el editor dibujado
  (`IURE_DOC_CORPUS=/carpeta` agrega otros sin copiarlos).
- `tests/rules.rs` y `input_rules_style_the_text`: cada regla de entrada y comportamiento.
- `hundred_pages_with_diagrams` (en `--release`): 100 páginas + 20 diagramas + corrector.
- [`docs/paridad-tiptap.md`](docs/paridad-tiptap.md): paridad con IureEditor (TipTap), para firmar.

## Puerta de calidad

```bash
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
```

Cargo guarda una copia nueva de cada binario de prueba cada vez que cambian sus dependencias y
nunca borra las anteriores: `cargo xtask sweep` (`--dry-run` para ver qué haría) deja solo la más
reciente y quita la compilación incremental sin usar en una semana, sin tener que recompilar.

Historia anterior: `crates/doc` y `crates/doc-ui` de IureDesk (b22e5d0 … 893c833).

Licencia Apache-2.0, titular Eduardo Llaguno Velasco.
