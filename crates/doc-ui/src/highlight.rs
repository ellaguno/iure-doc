//! S1.12: resaltado de sintaxis de los bloques de código (```` ```rust ````…).
//!
//! Gramáticas de Sublime Text con `syntect` (Rust puro con `fancy-regex`), sin sus temas: cada
//! trozo recibe una clase (comentario, cadena, número, palabra clave, función, tipo) que el editor
//! pinta con los colores del tema de la suite. Las gramáticas se cargan en otro hilo la primera vez
//! que hacen falta; mientras, el código se ve sin colores.

use std::ops::Range;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxSet};
use syntect::util::LinesWithEndings;

/// Líneas que se resaltan por bloque como mucho.
const MAX_LINES: usize = 5_000;
/// Una línea más larga ya no se resalta (las expresiones de las gramáticas pueden tardar).
const MAX_LINE: usize = 2_000;

/// Clases de un trozo de código (0: sin color).
pub(crate) const COMMENT: u8 = 1;
pub(crate) const STRING: u8 = 2;
pub(crate) const NUMBER: u8 = 3;
pub(crate) const KEYWORD: u8 = 4;
pub(crate) const FUNCTION: u8 = 5;
pub(crate) const TYPE: u8 = 6;

static SET: OnceLock<SyntaxSet> = OnceLock::new();
static LOADING: AtomicBool = AtomicBool::new(false);
static CLASSES: OnceLock<Vec<(Scope, u8)>> = OnceLock::new();

/// ¿Ya están las gramáticas? (Parte de la llave de las cachés de dibujo.)
pub(crate) fn ready() -> bool {
    SET.get().is_some()
}

/// Las gramáticas, o `None` mientras se cargan (la primera vez empieza a cargarlas).
fn syntaxes() -> Option<&'static SyntaxSet> {
    if let Some(s) = SET.get() {
        return Some(s);
    }
    if !LOADING.swap(true, Ordering::SeqCst) {
        std::thread::spawn(|| {
            if let Ok(ss) = std::panic::catch_unwind(SyntaxSet::load_defaults_newlines) {
                let _ = SET.set(ss);
            }
        });
    }
    None
}

/// Espera a las gramáticas (pruebas y capturas).
pub fn wait_ready(timeout: std::time::Duration) -> bool {
    let _ = syntaxes();
    let t = std::time::Instant::now();
    while !ready() && t.elapsed() < timeout {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    ready()
}

/// Nombre de lenguaje → el que conoce `syntect` (abreviaturas y lenguajes parecidos).
fn alias(lang: &str) -> &str {
    match lang {
        "ts" | "typescript" | "tsx" | "jsx" | "mjs" | "cjs" => "js",
        "sh" | "zsh" | "shell" | "console" | "shell-session" => "bash",
        "yml" => "yaml",
        "py3" => "py",
        "c++" | "hpp" | "cc" => "cpp",
        "jsonc" | "json5" => "json",
        "htm" => "html",
        other => other,
    }
}

/// Clases por prefijo de ámbito (la más interna gana).
fn classes() -> &'static [(Scope, u8)] {
    CLASSES.get_or_init(|| {
        [
            ("comment", COMMENT),
            ("string", STRING),
            ("constant.numeric", NUMBER),
            ("constant.language", NUMBER),
            ("constant.character", STRING),
            ("keyword", KEYWORD),
            ("storage", KEYWORD),
            ("entity.name.tag", KEYWORD),
            ("entity.name.function", FUNCTION),
            ("support.function", FUNCTION),
            ("entity.other.attribute-name", FUNCTION),
            ("entity.name.type", TYPE),
            ("entity.name.class", TYPE),
            ("entity.name.struct", TYPE),
            ("support.type", TYPE),
            ("support.class", TYPE),
        ]
        .iter()
        .filter_map(|(s, c)| Scope::new(s).ok().map(|s| (s, *c)))
        .collect()
    })
}

fn class_of(stack: &ScopeStack) -> u8 {
    let table = classes();
    for scope in stack.as_slice().iter().rev() {
        if let Some((_, c)) = table.iter().find(|(p, _)| p.is_prefix_of(*scope)) {
            return *c;
        }
    }
    0
}

// Cuántas veces se resaltó en este hilo (pruebas: un cuadro estable no resalta).
#[cfg(test)]
thread_local! {
    pub(crate) static CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Las clases de `code` en el lenguaje `lang` (bytes de `code`). Vacío si no se conoce el lenguaje
/// o aún no están las gramáticas.
pub(crate) fn spans(lang: &str, code: &str) -> Vec<(Range<usize>, u8)> {
    #[cfg(test)]
    CALLS.with(|c| c.set(c.get() + 1));
    let lang = lang.trim().to_ascii_lowercase();
    if lang.is_empty() || lang == "mermaid" {
        return Vec::new();
    }
    let Some(ss) = syntaxes() else { return Vec::new() };
    let Some(syntax) = ss.find_syntax_by_token(alias(&lang)) else { return Vec::new() };
    let mut state = ParseState::new(syntax);
    let mut stack = ScopeStack::new();
    let mut out: Vec<(Range<usize>, u8)> = Vec::new();
    let mut push = |r: Range<usize>, c: u8| {
        if c == 0 || r.is_empty() {
            return;
        }
        match out.last_mut() {
            Some((prev, pc)) if *pc == c && prev.end == r.start => prev.end = r.end,
            _ => out.push((r, c)),
        }
    };
    let mut off = 0usize;
    for line in LinesWithEndings::from(code).take(MAX_LINES) {
        if line.len() > MAX_LINE {
            break;
        }
        let Ok(ops) = state.parse_line(line, ss) else { break };
        let mut last = 0usize;
        for (i, op) in ops {
            let i = i.min(line.len());
            if i > last {
                push(off + last..off + i, class_of(&stack));
            }
            if stack.apply(&op).is_err() {
                break;
            }
            last = i;
        }
        push(off + last..off + line.len(), class_of(&stack));
        off += line.len();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn class_at(spans: &[(Range<usize>, u8)], code: &str, needle: &str) -> u8 {
        let at = code.find(needle).unwrap_or(usize::MAX);
        spans.iter().find(|(r, _)| r.contains(&at)).map_or(0, |(_, c)| *c)
    }

    #[test]
    fn common_languages_get_their_classes() {
        assert!(wait_ready(std::time::Duration::from_secs(20)));
        let rust = "// nota\nfn main() { let x = \"hola\"; 42 }\n";
        let s = spans("rust", rust);
        assert_eq!(class_at(&s, rust, "nota"), COMMENT);
        assert_eq!(class_at(&s, rust, "fn"), KEYWORD);
        assert_eq!(class_at(&s, rust, "main"), FUNCTION);
        assert_eq!(class_at(&s, rust, "hola"), STRING);
        assert_eq!(class_at(&s, rust, "42"), NUMBER);
        let py = "def f():\n    return None  # fin\n";
        let s = spans("py", py);
        assert_eq!(class_at(&s, py, "def"), KEYWORD);
        assert_eq!(class_at(&s, py, "fin"), COMMENT);
        let ts = "const a: number = 1;";
        assert_eq!(class_at(&spans("ts", ts), ts, "const"), KEYWORD, "TypeScript con la gramática de JavaScript");
        let sh = "echo \"hola\"";
        assert_eq!(class_at(&spans("sh", sh), sh, "hola"), STRING);
        assert!(spans("lenguaje-inventado", "x").is_empty());
        assert!(spans("mermaid", "graph LR").is_empty());
        assert!(spans("", "x").is_empty());
    }

    #[test]
    fn hostile_code_is_bounded() {
        assert!(wait_ready(std::time::Duration::from_secs(20)));
        let long = "a".repeat(MAX_LINE + 10);
        assert!(spans("rust", &long).is_empty(), "una línea larguísima no se resalta");
        let many = "let x = 1;\n".repeat(MAX_LINES + 100);
        let s = spans("rust", &many);
        assert!(s.iter().all(|(r, _)| r.end <= "let x = 1;\n".len() * MAX_LINES));
    }
}
