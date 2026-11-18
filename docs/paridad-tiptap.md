# Paridad del editor de la suite con IureEditor (TipTap)

S1.22 de `iuredesk/docs/gaps.md`: lista de lo que hace hoy IureEditor (Tauri + TipTap) y su estado
en el editor de la suite (`iure-doc-ui`, el de IureEditor 2.0 y las notas de IureDesk). **El dueño
la firma antes de S1.15** (retirar la versión Tauri al lograr paridad en lo que la gente usa).

Estado al 2026-11-18 (`iure-doc` v0.10.0). ✅ hecho · ◐ parcial · ❌ falta · — no aplica.

## Edición (P0, uso diario)

| Función | Estado | Notas |
|---|---|---|
| Títulos, listas (anidadas, numeradas, tareas), citas, separador | ✅ | Markdown en vivo: `# `, `- `, `1. `, `[ ] `, `> `, `---` ya son el formato; borrar la marca lo deshace |
| Negrita, cursiva, tachado, código en línea, enlaces | ✅ | Las marcas se ocultan fuera de la línea del cursor. `2*3*4` es cursiva (así lo lee CommonMark y todo visor); para multiplicar: `2 * 3 * 4`, `2\*3\*4` o `×` |
| Bloques de código con lenguaje | ✅ | Resaltado de sintaxis (`syntect`, ~40 lenguajes, Rust puro con `fancy-regex`) con los colores del tema; abreviaturas `ts`, `sh`, `yml`, `py`… |
| Tablas | ✅ | Cuadrícula en su lugar: Tab/Mayús+Tab, Intro, filas/columnas, alineación, «Markdown» |
| Imágenes | ◐ | Locales de la carpeta del documento en su lugar; **falta pegar o arrastrar una imagen** (TipTap la guarda en `assets/`); SVG e imágenes dentro de un párrafo se ven como Markdown |
| Deshacer/rehacer | ✅ | Del documento entero, también partir y unir bloques |
| Copiar/pegar | ✅ | Pegar HTML (web, Word, Google Docs) como Markdown; Ctrl+Mayús+V texto plano. **Falta**: copiar con formato hacia otras apps (se copia el Markdown) |
| Selección entre bloques | ✅ | Ctrl+A, Mayús+flechas, Mayús+clic, arrastrar |
| Intro inteligente, Tab en listas | ✅ | Tareas nuevas sin marcar, salir de la lista en vacío, título → párrafo, tres Intro salen del código |
| Atajos | ◐ | Ctrl+B/I, Ctrl+Mayús+X/K, Ctrl+Mayús+7/8/9, Ctrl+Alt+1–3. **Faltan en la paleta de comandos** y Ctrl+U (no hay subrayado en Markdown) |
| Buscar y reemplazar | ◐ | En el documento: Ctrl+F / Ctrl+H, «n de m», Intro/Mayús+Intro/F3, «Aa», `\n` y `\t`, reemplazar uno o todo (un paso de deshacer), resaltado y desplazamiento, Esc deja la coincidencia seleccionada. **Falta**: buscar en los archivos de la carpeta (IureEditor 2.0) y expresiones regulares |
| IME y acentos | ◐ | Los de egui (acentos con tecla muerta funcionan); **sin probar con IME de chino/japonés** |
| Guardado automático y recuperación de borradores | ✅ | IureDesk guarda solo; IureEditor 2.0 escribe borradores 2,5 s después del último cambio y ofrece recuperarlos tras un cierre inesperado (como la versión Tauri) |
| Corrector ortográfico | ◐ | Hunspell del sistema (es + en), sugerencias, diccionario personal; **sin diccionarios en Windows/macOS** |
| Fidelidad al guardar | ✅ | Corpus (golden de IureEditor, plantillas, README, CHANGELOG, casos difíciles, CRLF): byte a byte en el modelo y en el editor dibujado |
| Rendimiento | ✅ | 100 páginas + 20 diagramas + corrector: < 2 ms por tecla (meta < 8 ms) |

## Frecuente (P1)

| Función | Estado | Notas |
|---|---|---|
| Mermaid en vivo, clic para editar | ✅ | Mismo motor que IureDesk (`mermaid-svg`), errores en palabras sencillas |
| Exportar un diagrama a SVG/PNG | ◐ | En IureDesk sí (menú del diagrama); **en el editor por bloques, no** |
| Mapas mentales | ✅ | Dibujados en su lugar con sus opciones (distribución, forma, centro, colores) |
| Avisos `> [!NOTE]` | ◐ | Se conservan y se ven como cita; **sin el recuadro de aviso** |
| Notas al pie `[^1]` | ◐ | Se conservan y se ven; **sin botón de inserción ni numeración automática** |
| Índice (tabla de contenido) | ❌ | |
| Menú `/` de bloques | ❌ | |
| Resaltado `==texto==`, enlaces automáticos de URL | ❌ | No los lee `pulldown-cmark` |
| Títulos 4–6 | ✅ | |

## Ocasional (P2)

| Función | Estado | Notas |
|---|---|---|
| Fórmulas LaTeX `$…$`, `$$…$$` | ◐ | Se conservan y se ven como texto; **sin dibujarlas** (KaTeX en TipTap) |
| Front matter YAML | ✅ | Se conserva tal cual |

## Aplicación (IureEditor 2.0 frente a la versión Tauri)

| Función | Estado | Notas |
|---|---|---|
| Abrir, guardar, guardar como, nuevo, avisos de cambios | ✅ | |
| Pestañas, ventana por pestaña | ❌ | |
| Plantillas | ❌ | |
| Exportar PDF, DOCX, HTML | ❌ | S1.14 |
| Vista de código fuente | — | Markdown en vivo: el texto ya es el código |
| Esquema del documento, panel de carpeta, números de línea, zoom | ❌ | |
| Tema claro/oscuro, español/inglés | ✅ | Los del sistema; **sin elegirlos en un menú** |
| Deep links `iureditor://`, `--mcp` | ❌ | S1.8 |
| Conexión con Iurefficient | ❌ | |
| Actualizador | ❌ | |

## Firma

- [ ] Revisada por el dueño (fecha y decisión: qué ❌ deben estar antes de retirar la versión Tauri).
