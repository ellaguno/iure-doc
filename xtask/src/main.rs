//! Tareas de mantenimiento (`cargo xtask <tarea>`).
//!
//! - `sweep [--dry-run] [--days N]`: borra de `target/` las copias viejas de los binarios de prueba y
//!   ejemplo y la compilación incremental sin usar (ver `sweep.rs`). Conviene correrlo de vez en
//!   cuando o tras la puerta de calidad del `README.md`.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

mod sweep;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

type Res = Result<(), String>;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

fn main() -> ExitCode {
    let task = std::env::args().nth(1).unwrap_or_default();
    let r = match task.as_str() {
        "sweep" => sweep::sweep(std::env::args().skip(2).collect()),
        other => Err(format!("tarea desconocida: «{other}» (hay: sweep)")),
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask {task}: {e}");
            ExitCode::FAILURE
        }
    }
}
