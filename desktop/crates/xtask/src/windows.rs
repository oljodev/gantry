//! Type-checking the Windows build from a machine that is not Windows (docs/dev/setup.md).
//!
//! The Windows code — PowerShell, the credential store, `npx.cmd`, the path gate — was written
//! long before anything compiled it, and a `#[cfg(windows)]` block on a Linux machine is a
//! comment. `rustup` will hand out the Windows standard library to anybody who asks, so the
//! Rust half of the problem is free: `cargo check --target x86_64-pc-windows-msvc` type-checks
//! every one of those blocks without Windows anywhere.
//!
//! The half that is not free is C. Two dependencies compile C for the target — SQLite, and
//! aws-lc behind rustls — and their build scripts want `cl.exe` and `lib.exe`, which Debian
//! does not have and cannot be talked into. **`cargo check` never links.** The object files
//! those build scripts produce are therefore never read by anything: all that matters is that
//! the build script *succeeds*, so that the Rust crate above it can be checked. So this hands
//! `cc` a pair of stand-ins that create the files they are asked for and exit 0.
//!
//! What that proves and does not prove is worth being exact about, because the temptation is to
//! read a green check as a working build:
//!
//! - **Proved:** every `#[cfg(windows)]` item type-checks, every Windows-only dependency
//!   resolves, and the crate graph builds for the target. This is what catches the missing
//!   import, the renamed API, the trait that is not in scope on Windows.
//! - **Not proved:** that anything links, runs, or behaves. SQLite is not really compiled here
//!   and TLS certainly is not. No test runs. The first real answer comes from a Windows machine
//!   or a Windows CI runner, and the checklist for that is in `docs/dev/setup.md`.

use std::{fs, path::Path, process::Command};

use anyhow::{Context, bail};

/// The target Tauri ships for Windows. `-gnu` would need a different set of stand-ins for no
/// extra coverage: `cfg(windows)` is true for both, and the difference between them is the
/// linker, which never runs here.
const TARGET: &str = "x86_64-pc-windows-msvc";

const CL: &str = r#"#!/bin/sh
# Stands in for cl.exe while cross-checking for Windows (xtask/src/windows.rs). `cargo check`
# never links, so nothing ever reads these object files: what matters is that the build script
# that asked for them succeeds. Every output the caller names is created empty.
out=""
take_next=""
for a in "$@"; do
  if [ -n "$take_next" ]; then out="$a"; take_next=""; continue; fi
  case "$a" in
    -o) take_next=1 ;;
    -Fo:*|/Fo:*) out="${a#*Fo:}" ;;
    -Fo*|/Fo*) out="${a#*Fo}" ;;
  esac
done
if [ -n "$out" ]; then
  case "$out" in
    */) mkdir -p "$out" ;;
    *) mkdir -p "$(dirname "$out")" 2>/dev/null; : > "$out" ;;
  esac
fi
exit 0
"#;

const LIB: &str = r#"#!/bin/sh
# Stands in for lib.exe, the MSVC archiver. Same bargain as `cl` beside it.
for a in "$@"; do
  case "$a" in
    -[Oo][Uu][Tt]:*|/[Oo][Uu][Tt]:*)
      out="${a#*:}"; mkdir -p "$(dirname "$out")" 2>/dev/null; : > "$out" ;;
  esac
done
exit 0
"#;

/// `cargo xtask check-windows [--clippy]`.
pub fn check(root: &Path, clippy: bool) -> anyhow::Result<()> {
    if cfg!(windows) {
        // On Windows the real thing is one command away and says more than this ever could.
        bail!("this is for cross-checking from a machine that is not Windows; run `cargo clippy`");
    }
    if !installed(TARGET)? {
        bail!("the Windows standard library is missing: run `rustup target add {TARGET}`");
    }
    let shims = root.join("target/win-shims");
    fs::create_dir_all(&shims).context("creating the stand-in toolchain directory")?;
    write_shim(&shims.join("cl"), CL)?;
    write_shim(&shims.join("lib"), LIB)?;

    let mut cargo = Command::new(env!("CARGO"));
    cargo
        .current_dir(root)
        .arg(if clippy { "clippy" } else { "check" })
        .args(["--workspace", "--all-targets", "--target", TARGET])
        .env(format!("CC_{}", TARGET.replace('-', "_")), shims.join("cl"))
        .env(
            format!("CXX_{}", TARGET.replace('-', "_")),
            shims.join("cl"),
        )
        .env(
            format!("AR_{}", TARGET.replace('-', "_")),
            shims.join("lib"),
        );
    if clippy {
        cargo.args(["--", "-D", "warnings"]);
    }
    let status = cargo
        .status()
        .context("running cargo for the Windows target")?;
    if !status.success() {
        bail!("the Windows target does not type-check");
    }
    println!(
        "\nThe Windows code type-checks. Nothing was linked, nothing was run: see the checklist \
         in docs/dev/setup.md for what only a Windows machine can answer."
    );
    Ok(())
}

fn write_shim(path: &Path, body: &str) -> anyhow::Result<()> {
    fs::write(path, body).with_context(|| format!("writing {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))
            .with_context(|| format!("making {} executable", path.display()))?;
    }
    Ok(())
}

fn installed(target: &str) -> anyhow::Result<bool> {
    let out = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .context("asking rustup which targets are installed")?;
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .any(|line| line.trim() == target))
}
