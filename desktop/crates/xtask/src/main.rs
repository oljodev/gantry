//! Developer tasks, run as `cargo xtask <task>` (alias in `.cargo/config.toml`).
//!
//! - `gen-bindings`    regenerate `desktop/frontend/src/bindings.ts` from the Tauri commands
//! - `check-bindings`  regenerate into a temp file and fail if the committed bindings differ
//! - `validate-connectors`  the catalogue checks a per-file schema cannot make (17 §5)
//! - `probe-connectors`     ask every catalogued server what it is; `--offline` checks the
//!   recorded fixtures instead, which is how CI runs it
//! - `validate-skills`   the rules of a bundled skill that `build.rs` does not stop the
//!   build for (12 §A2)
//! - `icons`             arrives with M13

#![forbid(unsafe_code)]

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

use anyhow::{Context, bail};

mod connectors;
mod probe;
mod skills;

const BINDINGS: &str = "desktop/frontend/src/bindings.ts";

fn main() -> ExitCode {
    let task = env::args().nth(1).unwrap_or_default();
    let result = match task.as_str() {
        "gen-bindings" => gen_bindings(),
        "check-bindings" => check_bindings(),
        "validate-connectors" => connectors::validate(&workspace_root()),
        "probe-connectors" => {
            let flags: Vec<String> = env::args().skip(2).collect();
            probe::run(
                &workspace_root(),
                flags.iter().any(|f| f == "--offline"),
                flags.iter().any(|f| f == "--spawn"),
            )
        }
        "validate-skills" => skills::validate(&workspace_root()),
        "icons" => {
            eprintln!("xtask {task}: not implemented yet (see docs/plan/09-roadmap.md)");
            Ok(())
        }
        _ => {
            eprintln!(
                "usage: cargo xtask <gen-bindings | check-bindings | validate-connectors | probe-connectors [--offline] [--spawn] | validate-skills | icons>"
            );
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("xtask {task}: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn workspace_root() -> PathBuf {
    // The first ancestor holding the workspace lockfile, wherever this crate sits below it.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .find(|dir| dir.join("Cargo.lock").is_file())
        .expect("xtask lives below the workspace root")
        .to_path_buf()
}

/// Runs the app crate's `gen_bindings` test, which writes the TypeScript bindings to
/// `GANTRY_BINDINGS_OUT`. Building the app crate needs the Tauri toolchain.
fn export_bindings_to(out: &Path) -> anyhow::Result<()> {
    let status = Command::new(env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .current_dir(workspace_root())
        .args([
            "test",
            "--quiet",
            "--package",
            "gantry-app",
            "--lib",
            "tests::gen_bindings",
            "--",
            "--exact",
        ])
        .env("GANTRY_BINDINGS_OUT", out)
        .status()
        .context("running cargo test for the bindings export")?;
    if !status.success() {
        bail!("bindings export failed ({status})");
    }
    Ok(())
}

fn gen_bindings() -> anyhow::Result<()> {
    let out = workspace_root().join(BINDINGS);
    export_bindings_to(&out)?;
    println!("wrote {}", out.display());
    Ok(())
}

fn check_bindings() -> anyhow::Result<()> {
    let root = workspace_root();
    let committed = root.join(BINDINGS);
    let tmp = env::temp_dir().join(format!("gantry-bindings-{}.ts", std::process::id()));
    export_bindings_to(&tmp)?;
    let fresh = fs::read(&tmp).context("reading the regenerated bindings")?;
    let _ = fs::remove_file(&tmp);
    let current = fs::read(&committed).with_context(|| {
        format!(
            "reading {}; run `cargo xtask gen-bindings`",
            committed.display()
        )
    })?;
    if fresh != current {
        bail!("{BINDINGS} is out of date; run `cargo xtask gen-bindings` and commit the result");
    }
    println!("{BINDINGS} is up to date");
    Ok(())
}
