# iure-doc — instrucciones para agentes

El documento de la suite Iurefficient (ver `README.md`). Mismas reglas que IureDesk:

- **Solo Rust**: nada de `-sys` que envuelvan C/C++, webviews ni JS.
- **Nunca fallar**: sin `unwrap()`, `expect()`, `panic!`, `unreachable!`, `todo!`, `unimplemented!`
  ni indexado fuera de pruebas (cada crate empieza con su `#![deny(...)]`); `unsafe_code = "forbid"`.
  Todo texto que llega es hostil: tamaños y anidado acotados, `get()` en vez de `[i]`.
- **Sin pérdidas**: abrir y guardar sin tocar nada da el mismo archivo byte a byte; cada cambio del
  modelo o del editor llega con pruebas de propiedades. Cada corrección, con su prueba de regresión.
- `iure-doc` no depende de egui; `iure-doc-ui` sí (egui 0.37 y `iurefficient-ui`).
- Código en inglés; comentarios y documentación en español; textos de interfaz en los dos idiomas
  (`iurefficient_ui::lang::pick`).
- Antes de cada commit, la puerta de calidad del `README.md`. Mensajes con el id de tarea de
  `iuredesk/docs/gaps.md` (p. ej. `S1.12: …`). Una versión nueva: subir `version`, etiqueta
  `vX.Y.Z` y actualizar la etiqueta en IureDesk, IureProject e IureEditor.
