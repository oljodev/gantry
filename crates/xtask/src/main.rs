//! Developer tasks, run as `cargo xtask <task>` (alias in `.cargo/config.toml`).
//!
//! - `gen-bindings`    regenerate `src/bindings.ts` from the Tauri commands
//! - `check-bindings`  regenerate into a temp file and fail if `src/bindings.ts` differs
//! - `validate-connectors`, `validate-skills`, `icons`  arrive with M6, M12 and M13

#![forbid(unsafe_code)]

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

use anyhow::{Context, bail};

const BINDINGS: &str = "src/bindings.ts";

fn main() -> ExitCode {
    let task = env::args().nth(1).unwrap_or_default();
    let result = match task.as_str() {
        "gen-bindings" => gen_bindings(),
        "check-bindings" => check_bindings(),
        "validate-connectors" | "validate-skills" | "icons" => {
            eprintln!("xtask {task}: not implemented yet (see docs/plan/09-roadmap.md)");
            Ok(())
        }
        _ => {
            eprintln!(
                "usage: cargo xtask <gen-bindings | check-bindings | validate-connectors | validate-skills | icons>"
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
    // crates/xtask -> crates -> root
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("xtask lives two levels below the workspace root")
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
