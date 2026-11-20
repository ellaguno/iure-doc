//! Fórmulas: LaTeX (lo que se escribe entre `$…$` y `$$…$$`) a matemáticas de Typst, que dibujan
//! el editor y el PDF.
//!
//! Cubre lo habitual: letras griegas, fracciones, raíces, índices y exponentes, sumas, integrales,
//! límites, relaciones y flechas, funciones (`\sin`, `\log`…), acentos (`\hat`, `\bar`, `\vec`),
//! estilos (`\mathbf`, `\mathbb`, `\mathcal`, `\text`), matrices (`pmatrix`, `bmatrix`…), `cases`
//! y `aligned` (con `&` y `\\`). Lo que no conoce se escribe tal cual, en texto recto.
//!
//! La salida es segura para evaluarla en modo matemático: nunca lleva `#` (código), ni comillas
//! sin escapar, y las letras sueltas van separadas (en Typst `ab` sería una variable).

/// Tamaño máximo que se convierte.
const MAX_INPUT: usize = 20_000;
/// Anidado máximo de llaves.
const MAX_DEPTH: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Tok {
    Cmd(String),
    Open,
    Close,
    Sup,
    Sub,
    Amp,
    /// `\\`
    NewLine,
    Char(char),
    Space,
}

fn tokens(s: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut it = s.chars().take(MAX_INPUT).peekable();
    while let Some(c) = it.next() {
        match c {
            '\\' => match it.peek().copied() {
                Some('\\') => {
                    it.next();
                    out.push(Tok::NewLine);
                }
                Some(n) if n.is_ascii_alphabetic() => {
                    let mut name = String::new();
                    while let Some(&n) = it.peek() {
                        if n.is_ascii_alphabetic() && name.len() < 40 {
                            name.push(n);
                            it.next();
                        } else {
                            break;
                        }
                    }
                    out.push(Tok::Cmd(name));
                }
                Some(n) => {
                    it.next();
                    out.push(Tok::Cmd(n.to_string()));
                }
                None => {}
            },
            '{' => out.push(Tok::Open),
            '}' => out.push(Tok::Close),
            '^' => out.push(Tok::Sup),
            '_' => out.push(Tok::Sub),
            '&' => out.push(Tok::Amp),
            c if c.is_whitespace() => {
                if out.last() != Some(&Tok::Space) {
                    out.push(Tok::Space);
                }
            }
            c => out.push(Tok::Char(c)),
        }
    }
    out
}

/// Símbolos y funciones sin argumentos: comando de LaTeX → Typst.
fn symbol(name: &str) -> Option<&'static str> {
    Some(match name {
        "alpha" => "alpha",
        "beta" => "beta",
        "gamma" => "gamma",
        "delta" => "delta",
        "epsilon" => "epsilon.alt",
        "varepsilon" => "epsilon",
        "zeta" => "zeta",
        "eta" => "eta",
        "theta" => "theta",
        "vartheta" => "theta.alt",
        "iota" => "iota",
        "kappa" => "kappa",
        "lambda" => "lambda",
        "mu" => "mu",
        "nu" => "nu",
        "xi" => "xi",
        "pi" => "pi",
        "varpi" => "pi.alt",
        "rho" => "rho",
        "sigma" => "sigma",
        "tau" => "tau",
        "upsilon" => "upsilon",
        "phi" => "phi.alt",
        "varphi" => "phi",
        "chi" => "chi",
        "psi" => "psi",
        "omega" => "omega",
        "Gamma" => "Gamma",
        "Delta" => "Delta",
        "Theta" => "Theta",
        "Lambda" => "Lambda",
        "Xi" => "Xi",
        "Pi" => "Pi",
        "Sigma" => "Sigma",
        "Upsilon" => "Upsilon",
        "Phi" => "Phi",
        "Psi" => "Psi",
        "Omega" => "Omega",
        "sum" => "sum",
        "prod" => "product",
        "coprod" => "product.co",
        "int" => "integral",
        "iint" => "integral.double",
        "iiint" => "integral.triple",
        "oint" => "integral.cont",
        "lim" => "lim",
        "limsup" => "limsup",
        "liminf" => "liminf",
        "sup" => "sup",
        "inf" => "inf",
        "max" => "max",
        "min" => "min",
        "sin" => "sin",
        "cos" => "cos",
        "tan" => "tan",
        "cot" => "cot",
        "sec" => "sec",
        "csc" => "csc",
        "arcsin" => "arcsin",
        "arccos" => "arccos",
        "arctan" => "arctan",
        "sinh" => "sinh",
        "cosh" => "cosh",
        "tanh" => "tanh",
        "log" => "log",
        "ln" => "ln",
        "lg" => "lg",
        "exp" => "exp",
        "det" => "det",
        "gcd" => "gcd",
        "deg" => "deg",
        "dim" => "dim",
        "arg" => "arg",
        "ker" => "ker",
        "Pr" => "Pr",
        "infty" => "infinity",
        "partial" => "diff",
        "nabla" => "nabla",
        "cdot" => "dot.op",
        "times" => "times",
        "div" => "div",
        "pm" => "plus.minus",
        "mp" => "minus.plus",
        "ast" => "ast",
        "star" => "star",
        "circ" => "circle.small",
        "bullet" => "bullet",
        "le" | "leq" => "<=",
        "ge" | "geq" => ">=",
        "ne" | "neq" => "!=",
        "ll" => "<<",
        "gg" => ">>",
        "approx" => "approx",
        "sim" => "tilde.op",
        "simeq" => "tilde.eq",
        "cong" => "tilde.equiv",
        "equiv" => "equiv",
        "propto" => "prop",
        "to" | "rightarrow" => "->",
        "leftarrow" | "gets" => "<-",
        "Rightarrow" | "implies" => "=>",
        "Leftarrow" => "arrow.l.double",
        "leftrightarrow" => "<->",
        "Leftrightarrow" | "iff" => "<=>",
        "mapsto" => "|->",
        "uparrow" => "arrow.t",
        "downarrow" => "arrow.b",
        "in" => "in",
        "notin" => "in.not",
        "ni" => "in.rev",
        "subset" => "subset",
        "subseteq" => "subset.eq",
        "supset" => "supset",
        "supseteq" => "supset.eq",
        "cup" => "union",
        "cap" => "sect",
        "bigcup" => "union.big",
        "bigcap" => "sect.big",
        "setminus" => "without",
        "emptyset" | "varnothing" => "emptyset",
        "forall" => "forall",
        "exists" => "exists",
        "neg" | "lnot" => "not",
        "land" | "wedge" => "and",
        "lor" | "vee" => "or",
        "ldots" | "dots" => "dots",
        "cdots" => "dots.c",
        "vdots" => "dots.v",
        "ddots" => "dots.down",
        "prime" => "prime",
        "angle" => "angle",
        "perp" => "perp",
        "parallel" => "parallel",
        "mid" => "divides",
        "hbar" => "planck.reduce",
        "ell" => "ell",
        "Re" => "Re",
        "Im" => "Im",
        "aleph" => "aleph",
        "langle" => "angle.l",
        "rangle" => "angle.r",
        "lfloor" => "floor.l",
        "rfloor" => "floor.r",
        "lceil" => "ceil.l",
        "rceil" => "ceil.r",
        "quad" => "quad",
        "qquad" => "wide",
        "," | ":" | ";" | ">" => "thin",
        "!" => "",
        " " => "space",
        "%" => "%",
        "$" => "\\$",
        "&" => "\\&",
        "#" => "\\#",
        "_" => "\\_",
        "{" => "{",
        "}" => "}",
        "|" => "||",
        _ => return None,
    })
}

/// Comandos con un argumento: LaTeX → función de Typst.
fn unary(name: &str) -> Option<&'static str> {
    Some(match name {
        "sqrt" => "sqrt",
        "mathbf" | "boldsymbol" | "bm" => "bold",
        "mathit" => "italic",
        "mathbb" => "bb",
        "mathcal" => "cal",
        "mathfrak" => "frak",
        "mathsf" => "sans",
        "mathtt" => "mono",
        "overline" => "overline",
        "underline" => "underline",
        "hat" | "widehat" => "hat",
        "bar" => "macron",
        "vec" => "arrow",
        "dot" => "dot",
        "ddot" => "dot.double",
        "tilde" | "widetilde" => "tilde",
        "check" => "caron",
        "breve" => "breve",
        "acute" => "acute",
        "grave" => "grave",
        "overbrace" => "overbrace",
        "underbrace" => "underbrace",
        "cancel" => "cancel",
        _ => return None,
    })
}

struct Conv {
    toks: Vec<Tok>,
    pos: usize,
    depth: usize,
}

/// Cadena de Typst en modo matemático (texto recto).
fn quoted(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars().take(500) {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

impl Conv {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        self.pos += 1;
        t
    }

    fn skip_space(&mut self) {
        while self.peek() == Some(&Tok::Space) {
            self.pos += 1;
        }
    }

    /// Texto crudo de un grupo `{…}` (para `\text`, `\begin{…}`).
    fn raw_group(&mut self) -> String {
        self.skip_space();
        if self.peek() != Some(&Tok::Open) {
            return match self.next() {
                Some(Tok::Char(c)) => c.to_string(),
                _ => String::new(),
            };
        }
        self.pos += 1;
        let mut depth = 1usize;
        let mut out = String::new();
        while let Some(t) = self.next() {
            match t {
                Tok::Open => {
                    depth += 1;
                    out.push('{');
                }
                Tok::Close => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    out.push('}');
                }
                Tok::Char(c) => out.push(c),
                Tok::Space => out.push(' '),
                Tok::Cmd(c) => match symbol(&c) {
                    Some(_) if c.len() == 1 => out.push_str(&c),
                    _ => {
                        out.push('\\');
                        out.push_str(&c);
                    }
                },
                Tok::Sup => out.push('^'),
                Tok::Sub => out.push('_'),
                Tok::Amp => out.push('&'),
                Tok::NewLine => out.push(' '),
            }
        }
        out
    }

    /// Un argumento: un grupo `{…}` o un solo elemento.
    fn arg(&mut self) -> String {
        self.skip_space();
        if self.peek() == Some(&Tok::Open) {
            self.pos += 1;
            self.seq(Some(&Tok::Close), false)
        } else {
            self.atom().unwrap_or_default()
        }
    }

    /// Argumento opcional `[…]`.
    fn opt_arg(&mut self) -> Option<String> {
        self.skip_space();
        if self.peek() != Some(&Tok::Char('[')) {
            return None;
        }
        self.pos += 1;
        Some(self.seq(Some(&Tok::Char(']')), false))
    }

    /// Un elemento (sin exponentes).
    fn atom(&mut self) -> Option<String> {
        self.depth += 1;
        let out = if self.depth > MAX_DEPTH { self.next().map(|_| String::new()) } else { self.atom_inner() };
        self.depth -= 1;
        out
    }

    fn atom_inner(&mut self) -> Option<String> {
        let t = self.next()?;
        Some(match t {
            Tok::Open => self.seq(Some(&Tok::Close), false),
            Tok::Close => String::new(),
            Tok::Space => " ".into(),
            Tok::Sup | Tok::Sub => String::new(),
            Tok::Amp => "&".into(),
            Tok::NewLine => " \\ ".into(),
            Tok::Char(c) => match c {
                c if c.is_ascii_alphabetic() => format!("{c} "),
                c if c.is_ascii_digit() => c.to_string(),
                ',' => "\\,".into(),
                ';' => "\\;".into(),
                '"' => "\\\"".into(),
                '#' => "\\#".into(),
                '$' => "\\$".into(),
                '\'' => "'".into(),
                c if c.is_alphabetic() => format!("{c} "),
                c => c.to_string(),
            },
            Tok::Cmd(name) => self.command(&name),
        })
    }

    fn command(&mut self, name: &str) -> String {
        match name {
            "frac" | "dfrac" | "tfrac" | "cfrac" => {
                let a = self.arg();
                let b = self.arg();
                format!("frac({}, {})", a.trim(), b.trim())
            }
            "binom" | "dbinom" | "tbinom" => {
                let a = self.arg();
                let b = self.arg();
                format!("binom({}, {})", a.trim(), b.trim())
            }
            "sqrt" => match self.opt_arg() {
                Some(n) => {
                    let x = self.arg();
                    format!("root({}, {})", n.trim(), x.trim())
                }
                None => format!("sqrt({})", self.arg().trim()),
            },
            "text" | "textrm" | "mathrm" | "operatorname" | "textit" | "textbf" | "mbox" => {
                let t = self.raw_group();
                let q = quoted(&t);
                match name {
                    "operatorname" => format!("op({q}) "),
                    "textbf" => format!("bold({q}) "),
                    "textit" => format!("italic({q}) "),
                    _ => format!("{q} "),
                }
            }
            "left" | "right" | "big" | "Big" | "bigg" | "Bigg" | "bigl" | "bigr" | "Bigl" | "Bigr" | "displaystyle"
            | "textstyle" | "limits" | "nolimits" => {
                // Typst ajusta solo los paréntesis; el `.` de `\right.` no se ve.
                self.skip_space();
                if matches!(name, "left" | "right") && self.peek() == Some(&Tok::Char('.')) {
                    self.pos += 1;
                }
                String::new()
            }
            "begin" => {
                let env = self.raw_group();
                self.environment(env.trim())
            }
            "end" => {
                self.raw_group();
                String::new()
            }
            n => {
                if let Some(f) = unary(n) {
                    return format!("{f}({}) ", self.arg().trim());
                }
                match symbol(n) {
                    Some("") => String::new(),
                    Some(s) => format!("{s} "),
                    None => format!("{} ", quoted(n)),
                }
            }
        }
    }

    /// `\begin{…} … \end{…}`: matrices, casos y alineados.
    fn environment(&mut self, env: &str) -> String {
        let body = self.seq(None, true);
        let rows: Vec<Vec<String>> = body
            .split(" \\ ")
            .map(|r| r.split('&').map(|c| c.trim().to_string()).collect::<Vec<_>>())
            .filter(|r| r.iter().any(|c| !c.is_empty()))
            .collect();
        let mat = |delim: &str| {
            let rows: Vec<String> = rows.iter().map(|r| r.join(", ")).collect();
            format!("mat(delim: {delim}, {}) ", rows.join("; "))
        };
        match env {
            "pmatrix" => mat("\"(\""),
            "bmatrix" => mat("\"[\""),
            "Bmatrix" => mat("\"{\""),
            "vmatrix" => mat("\"|\""),
            "Vmatrix" => mat("\"||\""),
            "matrix" | "smallmatrix" | "array" => mat("none"),
            "cases" | "dcases" => {
                let rows: Vec<String> = rows.iter().map(|r| r.join(" & ")).collect();
                format!("cases({}) ", rows.join(", "))
            }
            _ => {
                // aligned, align, gathered, split, eqnarray…
                let rows: Vec<String> = rows.iter().map(|r| r.join(" & ")).collect();
                rows.join(" \\ ")
            }
        }
    }

    /// Una secuencia hasta `end` (o `\end{…}` si `env`), con exponentes e índices.
    fn seq(&mut self, end: Option<&Tok>, env: bool) -> String {
        self.depth += 1;
        let mut out = String::new();
        if self.depth > MAX_DEPTH {
            self.depth -= 1;
            return out;
        }
        let mut guard = 0usize;
        while let Some(t) = self.peek().cloned() {
            guard += 1;
            if guard > MAX_INPUT {
                break;
            }
            if end == Some(&t) {
                self.pos += 1;
                break;
            }
            if env && t == Tok::Cmd("end".into()) {
                self.pos += 1;
                self.raw_group();
                break;
            }
            match t {
                Tok::Sup | Tok::Sub => {
                    self.pos += 1;
                    let a = self.arg();
                    let mark = if t == Tok::Sup { '^' } else { '_' };
                    // Sin espacio antes: el índice es del elemento anterior.
                    while out.ends_with(' ') {
                        out.pop();
                    }
                    if out.is_empty() {
                        out.push_str("\"\"");
                    }
                    out.push(mark);
                    out.push('(');
                    out.push_str(a.trim());
                    out.push_str(") ");
                }
                Tok::Close if end.is_none() || env => {
                    // Llave de más: se ignora.
                    self.pos += 1;
                }
                _ => {
                    if let Some(a) = self.atom() {
                        out.push_str(&a);
                    }
                }
            }
        }
        self.depth -= 1;
        out
    }
}

/// La fórmula de LaTeX en matemáticas de Typst (sin los `$`).
pub fn latex_to_typst(latex: &str) -> String {
    let mut c = Conv { toks: tokens(latex), pos: 0, depth: 0 };
    let out = c.seq(None, false);
    // Espacios de sobra.
    let mut clean = String::with_capacity(out.len());
    for ch in out.chars() {
        if ch == ' ' && clean.ends_with(' ') {
            continue;
        }
        clean.push(ch);
    }
    clean.trim().to_string()
}

/// El LaTeX de un bloque `$$…$$` (sin los `$$`), si lo es.
pub fn display_math(block: &str) -> Option<&str> {
    let t = block.trim();
    let inner = t.strip_prefix("$$")?.strip_suffix("$$")?;
    (!inner.trim().is_empty()).then_some(inner.trim())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn common_formulas() {
        let t = latex_to_typst;
        assert_eq!(t(r"\frac{a+b}{2}"), "frac(a +b, 2)");
        assert_eq!(t(r"x^2 + y_{i,j}"), "x^(2) + y_(i \\,j)");
        assert_eq!(t(r"\sqrt[3]{x}"), "root(3, x)");
        assert_eq!(t(r"\sum_{i=1}^{n} i"), "sum_(i =1)^(n) i");
        assert_eq!(t(r"\alpha \le \beta"), "alpha <= beta");
        assert_eq!(t(r"ab"), "a b", "letras separadas");
        assert_eq!(t(r"\text{si } x > 0"), "\"si \" x > 0");
        assert_eq!(t(r"\mathbb{R}"), "bb(R)");
        assert_eq!(t(r"\begin{pmatrix} 1 & 0 \\ 0 & 1 \end{pmatrix}"), "mat(delim: \"(\", 1, 0; 0, 1)");
        assert_eq!(
            t(r"f(x) = \begin{cases} 1 & x > 0 \\ 0 & \text{otro} \end{cases}"),
            "f (x ) = cases(1 & x > 0, 0 & \"otro\")"
        );
        assert_eq!(t(r"\left( \frac{1}{2} \right)"), "( frac(1, 2) )");
        assert_eq!(t(r"\unknown"), "\"unknown\"");
        assert!(!t("#let x = 1 $ \" \\").contains('#') || t("#").contains("\\#"));
        assert_eq!(display_math("$$\nE = mc^2\n$$"), Some("E = mc^2"));
        assert_eq!(display_math("$$ $$"), None);
    }

    proptest! {
        /// Nunca falla, termina y no deja `#` sin escapar (sería código de Typst).
        #[test]
        fn never_panics_and_never_emits_code(s in "\\PC{0,200}") {
            let out = latex_to_typst(&s);
            let bytes = out.as_bytes();
            for (i, b) in bytes.iter().enumerate() {
                if *b == b'#' {
                    prop_assert!(i > 0 && bytes.get(i - 1) == Some(&b'\\'), "{out}");
                }
            }
        }

        #[test]
        fn deep_nesting_is_bounded(n in 0usize..2000) {
            let s = format!("{}x{}", "\\frac{".repeat(n), "}{1}".repeat(n));
            let _ = latex_to_typst(&s);
        }
    }
}
