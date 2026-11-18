//! S1.12: corrector ortográfico.
//!
//! [`Speller`] carga diccionarios Hunspell (`.aff` + `.dic`) con `spellbook` (Rust puro) en otro
//! hilo; mientras tanto, y si no encuentra ninguno, no marca nada. Una palabra está bien si la
//! acepta alguno de los diccionarios cargados (las notas mezclan español e inglés), si está en el
//! diccionario personal o si se ignoró en esta sesión.
//!
//! No se revisan el código, las marcas de Markdown, las URL, los correos, las rutas, las siglas
//! (TODO EN MAYÚSCULAS) ni las palabras con números.

use std::collections::{HashMap, HashSet};
use std::io::Write as _;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

/// Diccionario más grande que se carga (bytes del `.dic`).
const MAX_DIC_BYTES: u64 = 64 * 1024 * 1024;
/// Palabras recordadas (bien o mal) como mucho.
const MAX_CACHE: usize = 100_000;
/// Palabra más larga que se revisa.
const MAX_WORD: usize = 48;
/// Sugerencias que se ofrecen.
const MAX_SUGGESTIONS: usize = 6;

#[derive(Default)]
struct State {
    dicts: Vec<spellbook::Dictionary>,
    personal: HashSet<String>,
    ignored: HashSet<String>,
    cache: HashMap<String, bool>,
    /// Cambia al terminar de cargar o al agregar/ignorar palabras (invalida los dibujos).
    generation: u64,
    names: Vec<String>,
}

/// El corrector (compartido entre editores y hilos).
pub struct Speller {
    state: RwLock<State>,
    personal_path: Option<PathBuf>,
}

/// Carpetas donde suele haber diccionarios Hunspell.
pub fn default_dirs() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> =
        ["/usr/share/hunspell", "/usr/share/myspell/dicts", "/usr/share/myspell", "/usr/local/share/hunspell"]
            .iter()
            .map(PathBuf::from)
            .collect();
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        v.push(home.join(".local/share/hunspell"));
        v.push(home.join("Library/Spelling"));
    }
    v
}

/// El primer diccionario que hay para un idioma (`es` → `es_MX`, `es_ES`, cualquier `es_*`).
fn find(lang: &str, dirs: &[PathBuf]) -> Option<(PathBuf, PathBuf)> {
    let single = [lang];
    let prefer: &[&str] = match lang {
        "es" => &["es_MX", "es_ES", "es"],
        "en" => &["en_US", "en_GB", "en"],
        _ => &single,
    };
    let pair = |dir: &Path, name: &str| {
        let (aff, dic) = (dir.join(format!("{name}.aff")), dir.join(format!("{name}.dic")));
        (aff.is_file() && dic.is_file()).then_some((aff, dic))
    };
    for name in prefer {
        if let Some(p) = dirs.iter().find_map(|d| pair(d, name)) {
            return Some(p);
        }
    }
    // Cualquier variante del idioma.
    dirs.iter().find_map(|d| {
        let mut names: Vec<String> = std::fs::read_dir(d)
            .ok()?
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .filter_map(|f| f.strip_suffix(".dic").map(str::to_string))
            .filter(|n| n.starts_with(&format!("{lang}_")))
            .collect();
        names.sort();
        names.iter().find_map(|n| pair(d, n))
    })
}

fn read_bounded(p: &Path) -> Option<String> {
    let meta = std::fs::metadata(p).ok()?;
    if meta.len() > MAX_DIC_BYTES {
        return None;
    }
    std::fs::read_to_string(p).ok()
}

/// ¿Un trozo sin espacios es una URL, un correo o una ruta (no se revisa)?
fn is_technical(token: &str) -> bool {
    token.contains("://")
        || token.starts_with("www.")
        || token.chars().any(|c| matches!(c, '@' | '/' | '\\' | '_' | '=' | '<' | '>' | '{' | '}'))
}

/// Palabras de un texto (bytes), sin lo técnico.
pub(crate) fn words(text: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    for token in text.split_inclusive(char::is_whitespace) {
        let start = pos;
        pos += token.len();
        if is_technical(token.trim_end()) {
            continue;
        }
        let mut w: Option<usize> = None;
        let mut chars = token.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            // Un apóstrofo entre letras es parte de la palabra (don't, l'eau).
            let inner_apostrophe =
                matches!(c, '\'' | '’') && w.is_some() && chars.peek().is_some_and(|(_, n)| n.is_alphabetic());
            if c.is_alphabetic() || inner_apostrophe {
                w.get_or_insert(i);
            } else if let Some(s) = w.take() {
                out.push(start + s..start + i);
            }
        }
        if let Some(s) = w {
            out.push(start + s..start + token.trim_end().len());
        }
    }
    out
}

impl Speller {
    /// Un corrector para `langs` (`es`, `en`, `fr`… o `es_MX`) con los diccionarios de `dirs`; los
    /// carga en otro hilo. `personal`: archivo de palabras agregadas (una por línea).
    pub fn new(langs: &[&str], dirs: Vec<PathBuf>, personal: Option<PathBuf>) -> Arc<Speller> {
        let mut st = State::default();
        if let Some(p) = &personal
            && let Ok(t) = std::fs::read_to_string(p)
        {
            st.personal = t.lines().map(|l| l.trim().to_lowercase()).filter(|l| !l.is_empty()).take(100_000).collect();
        }
        let sp = Arc::new(Speller { state: RwLock::new(st), personal_path: personal });
        let langs: Vec<String> = langs.iter().map(|l| l.to_string()).collect();
        let weak = Arc::downgrade(&sp);
        std::thread::spawn(move || {
            for lang in langs {
                let Some((aff, dic)) = find(&lang, &dirs) else { continue };
                let (Some(a), Some(d)) = (read_bounded(&aff), read_bounded(&dic)) else { continue };
                let parsed = std::panic::catch_unwind(|| spellbook::Dictionary::new(&a, &d));
                let Ok(Ok(dict)) = parsed else { continue };
                let Some(sp) = weak.upgrade() else { return };
                let mut st = sp.write();
                st.dicts.push(dict);
                st.names.push(dic.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or(lang));
                st.cache.clear();
                st.generation = st.generation.wrapping_add(1);
            }
        });
        sp
    }

    /// Con los diccionarios del sistema (Linux: Hunspell de la distribución) y español e inglés.
    pub fn system(personal: Option<PathBuf>) -> Arc<Speller> {
        Speller::new(&["es", "en"], default_dirs(), personal)
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, State> {
        self.state.read().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, State> {
        self.state.write().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Cambia cuando el resultado de revisar puede cambiar (para las cachés de dibujo).
    pub fn generation(&self) -> u64 {
        self.read().generation
    }

    /// Diccionarios cargados (p. ej. `es_MX`, `en_US`).
    pub fn languages(&self) -> Vec<String> {
        self.read().names.clone()
    }

    /// ¿Está bien escrita? Sin diccionarios todo está bien.
    pub fn check(&self, word: &str) -> bool {
        if word.chars().count() < 2 || word.chars().count() > MAX_WORD {
            return true;
        }
        // Siglas y palabras con mayúsculas internas raras: no se revisan.
        if word.chars().all(|c| !c.is_lowercase()) {
            return true;
        }
        let lower = word.to_lowercase();
        {
            let st = self.read();
            if st.dicts.is_empty() || st.personal.contains(&lower) || st.ignored.contains(&lower) {
                return true;
            }
            if let Some(ok) = st.cache.get(word) {
                return *ok;
            }
        }
        let ok = {
            let st = self.read();
            st.dicts.iter().any(|d| d.check(word))
        };
        let mut st = self.write();
        if st.cache.len() >= MAX_CACHE {
            st.cache.clear();
        }
        st.cache.insert(word.to_string(), ok);
        ok
    }

    /// Las palabras mal escritas de `text` (bytes); `skip(i)`: el byte `i` no se revisa (código,
    /// marcas).
    pub(crate) fn misspelled(&self, text: &str, skip: impl Fn(usize) -> bool) -> Vec<Range<usize>> {
        if self.read().dicts.is_empty() {
            return Vec::new();
        }
        words(text)
            .into_iter()
            .filter(|r| !(r.start..r.end).any(&skip))
            .filter(|r| text.get(r.clone()).is_some_and(|w| !self.check(w)))
            .collect()
    }

    /// Sugerencias para una palabra mal escrita.
    pub fn suggest(&self, word: &str) -> Vec<String> {
        if word.chars().count() > MAX_WORD {
            return Vec::new();
        }
        let st = self.read();
        let mut out: Vec<String> = Vec::new();
        for d in &st.dicts {
            let mut v = Vec::new();
            d.suggest(word, &mut v);
            for s in v {
                if !out.contains(&s) {
                    out.push(s);
                }
            }
        }
        out.truncate(MAX_SUGGESTIONS);
        out
    }

    /// Agrega una palabra al diccionario personal (y a su archivo).
    pub fn add(&self, word: &str) {
        let lower = word.to_lowercase();
        {
            let mut st = self.write();
            st.personal.insert(lower.clone());
            st.generation = st.generation.wrapping_add(1);
        }
        if let Some(p) = &self.personal_path {
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
                let _ = writeln!(f, "{lower}");
            }
        }
    }

    /// No marca la palabra en esta sesión.
    pub fn ignore(&self, word: &str) {
        let mut st = self.write();
        st.ignored.insert(word.to_lowercase());
        st.generation = st.generation.wrapping_add(1);
    }

    /// ¿Ya cargó algún diccionario?
    pub fn is_ready(&self) -> bool {
        !self.read().dicts.is_empty()
    }
}

/// Archivo del diccionario personal de una app (`~/.local/share/<app>/diccionario.txt`, o
/// `%APPDATA%\<app>\…` en Windows).
pub fn personal_path(app: &str) -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    Some(base.join(app).join("diccionario.txt"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un diccionario Hunspell mínimo en una carpeta temporal.
    pub(crate) fn tiny(dir: &Path, name: &str, words: &[&str]) {
        std::fs::write(dir.join(format!("{name}.aff")), "SET UTF-8\nTRY esianrtolcdugmphbyfvkwz\n").unwrap();
        std::fs::write(dir.join(format!("{name}.dic")), format!("{}\n{}\n", words.len(), words.join("\n"))).unwrap();
    }

    pub(crate) fn wait(sp: &Speller) {
        for _ in 0..500 {
            if sp.is_ready() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn words_skip_technical_tokens() {
        let t = "Hola mundo, visita https://x.mx o escribe a ana@x.mx; don't l'eau ruta/a/b café";
        let w: Vec<&str> = words(t).into_iter().filter_map(|r| t.get(r)).collect();
        assert_eq!(w, ["Hola", "mundo", "visita", "o", "escribe", "a", "don't", "l'eau", "café"]);
    }

    #[test]
    fn checks_with_any_dictionary_and_learns_words() {
        let dir = tempfile::tempdir().unwrap();
        tiny(dir.path(), "es_MX", &["hola", "mundo", "casa"]);
        tiny(dir.path(), "en_US", &["hello", "world"]);
        let personal = dir.path().join("p/diccionario.txt");
        let sp = Speller::new(&["es", "en"], vec![dir.path().to_path_buf()], Some(personal.clone()));
        wait(&sp);
        for _ in 0..200 {
            if sp.languages().len() == 2 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(sp.languages(), ["es_MX", "en_US"]);
        assert!(sp.check("hola") && sp.check("Hola") && sp.check("world"));
        assert!(!sp.check("holla"));
        assert!(sp.check("ONU"), "siglas");
        let t = "Hola mundo holla `codigo` world";
        let bad: Vec<&str> = sp.misspelled(t, |i| (18..26).contains(&i)).into_iter().filter_map(|r| t.get(r)).collect();
        assert_eq!(bad, ["holla"], "el código no se revisa");
        assert!(sp.suggest("holla").iter().any(|s| s == "hola"));
        let g = sp.generation();
        sp.add("Iurefficient");
        assert!(sp.check("iurefficient") && sp.generation() != g);
        assert_eq!(std::fs::read_to_string(&personal).unwrap(), "iurefficient\n");
        sp.ignore("Altamira");
        assert!(sp.check("Altamira"));
        // Lo personal se carga de nuevo.
        let again = Speller::new(&[], vec![], Some(personal));
        assert!(again.read().personal.contains("iurefficient"));
    }

    #[test]
    fn without_dictionaries_nothing_is_marked() {
        let sp = Speller::new(&["xx"], vec![], None);
        assert!(sp.misspelled("asdfg qwerty", |_| false).is_empty());
        assert!(sp.check("asdfg"));
    }
}

#[cfg(test)]
mod system {
    use super::*;

    /// Con los diccionarios del sistema: `cargo test --release -p iure-doc-ui system_dictionaries -- --ignored --nocapture`.
    #[test]
    #[ignore = "usa los diccionarios Hunspell instalados"]
    fn system_dictionaries() {
        let t0 = std::time::Instant::now();
        let sp = Speller::system(None);
        while sp.languages().len() < 2 && t0.elapsed().as_secs() < 30 {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        eprintln!("cargados {:?} en {:.0} ms", sp.languages(), t0.elapsed().as_secs_f64() * 1000.0);
        let text = "La reunión con el cliente fue productiva; acordamos enviar la propuesta mañana. \
            The meeting went well and we will send the proposal tomorrow. Exepción y aceptacion. "
            .repeat(500);
        let t1 = std::time::Instant::now();
        let bad = sp.misspelled(&text, |_| false);
        eprintln!(
            "{} KB revisados en {:.1} ms; mal escritas: {}",
            text.len() / 1024,
            t1.elapsed().as_secs_f64() * 1000.0,
            bad.len()
        );
        let t2 = std::time::Instant::now();
        eprintln!(
            "sugerencias para «exepción»: {:?} ({:.0} ms)",
            sp.suggest("Exepción"),
            t2.elapsed().as_secs_f64() * 1000.0
        );
        eprintln!("sugerencias para «aceptacion»: {:?}", sp.suggest("aceptacion"));
        assert!(sp.check("reunión") && sp.check("proposal") && !sp.check("aceptacion"));
    }
}
