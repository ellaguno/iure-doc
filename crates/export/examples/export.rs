//! Exporta un Markdown a HTML y DOCX junto a él (imágenes de su carpeta, sin diagramas):
//! `cargo run -p iure-doc-export --example export -- nota.md [--a4]`.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = std::path::PathBuf::from(args.first().ok_or("uso: export <archivo.md> [--a4]")?);
    let md = std::fs::read_to_string(&path)?;
    let doc = iure_doc::Document::parse(&md);
    let res = iure_doc_export::FolderResources::new(path.parent().map(std::path::Path::to_path_buf));
    let paper =
        if args.iter().any(|a| a == "--a4") { iure_doc_export::Paper::A4 } else { iure_doc_export::Paper::Letter };
    let opts = iure_doc_export::Options { paper, ..Default::default() };
    std::fs::write(path.with_extension("html"), iure_doc_export::html::html(&doc, &res, &opts))?;
    std::fs::write(path.with_extension("docx"), iure_doc_export::docx::docx(&doc, &res, &opts)?)?;
    eprintln!("{} → .html y .docx", path.display());
    Ok(())
}
