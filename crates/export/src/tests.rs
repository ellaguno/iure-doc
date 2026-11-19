use std::io::Read as _;

use iure_doc::Document;
use proptest::prelude::*;

use super::*;

/// Una imagen PNG de `w`×`h`.
fn png(w: u32, h: u32) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(w, h, image::Rgba([10, 20, 30, 255]))
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
    out.into_inner()
}

/// Recursos de prueba: una imagen `a.png` y diagramas Mermaid.
struct Fake;

impl Resources for Fake {
    fn image(&self, src: &str) -> Option<Picture> {
        (src == "a.png").then(|| picture_from_bytes(png(4, 2)).unwrap())
    }
    fn diagram(&self, block: &str) -> Option<Diagram> {
        block.starts_with("```mermaid").then(|| Diagram {
            svg: Some("<svg width=\"40\" height=\"20\"><rect/></svg>".into()),
            png: Some(picture_from_bytes(png(40, 20)).unwrap()),
            points: (30.0, 15.0),
        })
    }
}

/// Las partes de un DOCX.
fn unzip(bytes: &[u8]) -> std::collections::BTreeMap<String, Vec<u8>> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut out = std::collections::BTreeMap::new();
    for i in 0..z.len() {
        let mut f = z.by_index(i).unwrap();
        let mut v = Vec::new();
        f.read_to_end(&mut v).unwrap();
        out.insert(f.name().to_string(), v);
    }
    out
}

/// ¿Etiquetas XML bien anidadas? (Comprobación sencilla: sin comentarios ni CDATA, que no
/// escribimos.)
fn balanced(xml: &str) -> Result<(), String> {
    let mut stack: Vec<&str> = Vec::new();
    let mut rest = xml;
    while let Some(lt) = rest.find('<') {
        let after = &rest[lt + 1..];
        let gt = after.find('>').ok_or("etiqueta sin cerrar")?;
        let tag = &after[..gt];
        rest = &after[gt + 1..];
        if tag.starts_with('?') || tag.starts_with('!') || tag.ends_with('/') {
            continue;
        }
        if let Some(name) = tag.strip_prefix('/') {
            let open = stack.pop().ok_or_else(|| format!("cierre sobrante </{name}>"))?;
            if open != name {
                return Err(format!("<{open}> cerrado con </{name}>"));
            }
        } else {
            stack.push(tag.split_whitespace().next().unwrap_or(""));
        }
    }
    if stack.is_empty() { Ok(()) } else { Err(format!("sin cerrar: {stack:?}")) }
}

const SAMPLE: &str = "---\ntitle: \"Informe\"\n---\n\n# Hola **mundo**\n\nTexto con *cursiva*, `código` y \
[enlace](https://example.com?a=1&b=2).\n\n- uno\n- dos\n  1. a\n  2. b\n\n- [x] hecho\n- [ ] por hacer\n\n\
5. cinco\n6. seis\n\n> cita\n\n> [!WARNING] Ojo\n> cuidado\n\n```rust\nfn main() {}\n```\n\n```mermaid\ngraph LR\nA-->B\n```\n\n\
| a | b |\n|:--|--:|\n| 1 | 2 |\n\n---\n\n![foto](a.png)\n\n$$x^2$$\n";

#[test]
fn html_is_self_contained() {
    let doc = Document::parse(SAMPLE);
    let h = html::html(&doc, &Fake, &Options::default());
    assert!(h.starts_with("<!DOCTYPE html>"));
    assert!(h.contains("<title>Informe</title>"), "título de la cabecera");
    assert!(h.contains("<h1>Hola <strong>mundo</strong></h1>"));
    assert!(h.contains("<a href=\"https://example.com?a=1&amp;b=2\">enlace</a>"));
    assert!(h.contains("<ol start=\"5\">"));
    assert!(h.contains("<input type=\"checkbox\" disabled checked>hecho"));
    assert!(h.contains("<aside class=\"callout warning\">"));
    assert!(h.contains("<pre><code class=\"language-rust\">fn main() {}</code></pre>"));
    assert!(h.contains("<figure class=\"diagram\" style=\"max-width:30pt\"><svg"), "diagrama en SVG");
    assert!(h.contains("<th style=\"text-align:left\">a</th>"));
    assert!(h.contains("<img src=\"data:image/png;base64,"), "imagen incrustada");
    assert!(!h.contains("title: "), "la cabecera YAML no se ve");
    assert!(!h.contains("src=\"http") && !h.contains("<link"), "nada externo");
    // Sin recursos: la imagen es su texto y el diagrama, su código.
    let plain = html::html(&doc, &NoResources, &Options { title: "Otro".into(), ..Options::default() });
    assert!(plain.contains("<title>Otro</title>"));
    assert!(plain.contains("<pre><code class=\"language-mermaid\">graph LR"));
    assert!(plain.contains("<p>foto</p>"));
}

#[test]
fn html_from_the_document_never_runs() {
    let md = "<script>alert(1)</script>\n\nHola <img src=x onerror=alert(1)>\n\n[a](javascript:alert(1)) \
              [b](JaVa\tScRiPt:alert(1)) [c](data:text/html,x) [d](mailto:a@b.c) [e](#arriba) [f](otro.md)\n\n\
              ![x](https://rastreo.example/p.png)\n";
    let h = html::body(&Document::parse(md), &FolderResources::new(None));
    assert!(!h.contains("<script"), "{h}");
    assert!(!h.contains("<img"), "{h}");
    assert!(!h.to_ascii_lowercase().contains("javascript:"), "{h}");
    assert!(!h.contains("href=\"data:"), "{h}");
    assert!(h.contains("href=\"mailto:a@b.c\""));
    assert!(h.contains("href=\"#arriba\""));
    assert!(h.contains("href=\"otro.md\""));
    assert!(h.contains("&lt;img src=x onerror=alert(1)&gt;"), "el HTML en línea, como texto: {h}");
}

#[test]
fn folder_images_stay_inside_the_folder() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("img")).unwrap();
    std::fs::write(dir.path().join("img/a b.png"), png(3, 5)).unwrap();
    std::fs::write(dir.path().join("img/falsa.png"), b"no es una imagen").unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("x.png"), png(1, 1)).unwrap();
    let r = FolderResources::new(Some(dir.path().to_path_buf()));
    let p = r.image("img/a%20b.png").unwrap();
    assert_eq!((p.width, p.height, p.mime.as_str()), (3, 5, "image/png"));
    assert!(r.image("./img/a%20b.png").is_some());
    assert!(r.image("img/falsa.png").is_none(), "no es una imagen");
    assert!(r.image("../x.png").is_none());
    assert!(r.image(&outside.path().join("x.png").to_string_lossy()).is_none(), "ruta absoluta");
    assert!(r.image("https://example.com/a.png").is_none());
    assert!(r.image("img/no.png").is_none());
    assert!(FolderResources::new(None).image("img/a%20b.png").is_none());
}

#[test]
fn docx_has_word_structure() {
    let doc = Document::parse(SAMPLE);
    let bytes = docx::docx(&doc, &Fake, &Options { paper: Paper::A4, ..Options::default() }).unwrap();
    let parts = unzip(&bytes);
    for p in [
        "[Content_Types].xml",
        "_rels/.rels",
        "word/_rels/document.xml.rels",
        "word/document.xml",
        "word/styles.xml",
        "word/numbering.xml",
        "docProps/core.xml",
        "word/media/image1.png",
        "word/media/image2.png",
    ] {
        assert!(parts.contains_key(p), "falta {p}");
    }
    let s = |p: &str| String::from_utf8(parts.get(p).unwrap().clone()).unwrap();
    let d = s("word/document.xml");
    for (name, xml) in [
        ("document", &d),
        ("styles", &s("word/styles.xml")),
        ("numbering", &s("word/numbering.xml")),
        ("rels", &s("word/_rels/document.xml.rels")),
    ] {
        balanced(xml).unwrap_or_else(|e| panic!("{name}: {e}"));
    }
    assert!(d.contains("<w:pStyle w:val=\"Heading1\"/>"));
    assert!(d.contains("<w:b/></w:rPr><w:t xml:space=\"preserve\">mundo</w:t>"));
    assert!(d.contains("<w:numPr><w:ilvl w:val=\"1\"/>"), "lista anidada");
    assert!(d.contains("☒ ") && d.contains("☐ "), "tareas");
    assert!(d.contains("<w:hyperlink r:id="));
    assert!(d.contains("<w:pStyle w:val=\"Code\"/>"));
    assert!(d.contains("<w:tblHeader/>"));
    assert!(d.contains("<w:pgSz w:w=\"11906\""), "A4");
    assert_eq!(d.matches("<w:drawing>").count(), 2, "imagen y diagrama");
    assert!(!d.contains("title:"));
    let rels = s("word/_rels/document.xml.rels");
    assert!(rels.contains("Target=\"https://example.com?a=1&amp;b=2\" TargetMode=\"External\""));
    assert!(s("word/numbering.xml").contains("<w:startOverride w:val=\"5\"/>"), "la lista empieza en 5");
    assert!(s("docProps/core.xml").contains("<dc:title>Informe</dc:title>"));
}

#[test]
fn docx_converts_webp_and_fits_wide_images() {
    struct Wide;
    impl Resources for Wide {
        fn image(&self, src: &str) -> Option<Picture> {
            let fmt = if src == "w.webp" { image::ImageFormat::WebP } else { image::ImageFormat::Png };
            let mut out = std::io::Cursor::new(Vec::new());
            image::RgbaImage::from_pixel(4000, 100, image::Rgba([1, 2, 3, 255])).write_to(&mut out, fmt).ok()?;
            picture_from_bytes(out.into_inner())
        }
        fn diagram(&self, _block: &str) -> Option<Diagram> {
            None
        }
    }
    let doc = Document::parse("![](w.webp)\n\n![](a.png)\n");
    let parts = unzip(&docx::docx(&doc, &Wide, &Options::default()).unwrap());
    assert!(parts.contains_key("word/media/image1.png"), "WebP → PNG");
    let d = String::from_utf8(parts.get("word/document.xml").unwrap().clone()).unwrap();
    // Carta con márgenes de 1": 6,5" de texto = 5 943 600 EMU.
    assert!(d.contains("<wp:extent cx=\"5943600\" cy=\"148590\"/>"), "{d}");
}

#[test]
fn hostile_text_is_valid_xml() {
    let md = "a\u{1}b\u{FFFF} <w:p> & \"x\" ]]>\n\n| <b> |\n|---|\n| \u{0} |\n";
    let parts = unzip(&docx::docx(&Document::parse(md), &NoResources, &Options::default()).unwrap());
    let d = String::from_utf8(parts.get("word/document.xml").unwrap().clone()).unwrap();
    balanced(&d).unwrap();
    assert!(!d.contains('\u{1}') && !d.contains('\u{0}') && !d.contains('\u{FFFF}'));
    assert!(d.contains("&lt;w:p&gt; &amp; &quot;x&quot;"));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    #[test]
    fn any_text_exports(md in "(\\PC|[#*_`>|\\[\\]()!$~\\-\\n 1.])*") {
        let doc = Document::parse(&md);
        let h = html::html(&doc, &Fake, &Options::default());
        prop_assert!(!h.contains("<script"));
        let bytes = docx::docx(&doc, &Fake, &Options::default()).unwrap();
        let parts = unzip(&bytes);
        let d = String::from_utf8(parts.get("word/document.xml").cloned().unwrap_or_default()).unwrap();
        prop_assert!(balanced(&d).is_ok(), "{:?}", balanced(&d));
    }
}
