//! `cargo xtask licenses`: writes `THIRD_PARTY_LICENSES.md`, the notices the licences of
//! Gantry's dependencies ask to travel with the program (LICENSING.md, docs/plan/07).
//!
//! Three sources, because three kinds of thing end up in a release:
//!
//! - **Rust crates** compiled into the binary, from `cargo about` over `desktop/app` for the two
//!   targets v0.1.0 ships. Its configuration is `about.toml` beside this file.
//! - **JavaScript packages** bundled into the interface: the production dependencies of the
//!   frontend and the artifact runtime, from `pnpm licenses`. The licence text is read from each
//!   package's own files, because a licence identifier is not a copyright notice.
//! - **Fonts and marks** whose files are copied into the app although the packages they come
//!   from are development dependencies, and so invisible to `pnpm licenses --prod`.
//!
//! The output is sorted throughout and read only from files on disk, so the same two lockfiles
//! produce the same file on any machine. `--check` compares instead of writing; the release
//! workflow runs it, so a build never ships with notices for a dependency graph it does not have.

use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::Path,
    process::Command,
};

use anyhow::{Context, bail};
use serde::Deserialize;
use serde_json::Value;

pub const OUTPUT: &str = "THIRD_PARTY_LICENSES.md";

/// The version the release workflow installs. A different cargo-about may read a licence file
/// differently, and the committed file would then disagree with the one CI generates.
const CARGO_ABOUT: &str = "0.9.2";

/// The workspace packages whose production dependencies are bundled into the interface.
const JS_PACKAGES: [&str; 2] = ["@gantry/frontend", "@gantry/artifact-runtime"];

/// Development dependencies whose files are copied into the app anyway, and why.
const ASSETS: [(&str, &str); 3] = [
    (
        "@fontsource-variable/inter",
        "Inter, the interface typeface, copied into the app by `pnpm fonts`.",
    ),
    (
        "@fontsource-variable/jetbrains-mono",
        "JetBrains Mono, the code typeface, copied into the app by `pnpm fonts`.",
    ),
    (
        "simple-icons",
        "Connector marks, written into `marks.generated.ts` by `pnpm marks`. The marks themselves \
         are their owners' trademarks (docs/plan/17 §8); the licence covers the drawings.",
    ),
];

/// Packages whose `package.json` misstates their licence, with what the shipped file says.
const CORRECTIONS: [(&str, &str); 1] = [("khroma", "MIT")];

pub fn run(root: &Path, check: bool) -> anyhow::Result<()> {
    let rust = rust(root)?;
    let js = javascript(root)?;
    let assets = assets(root)?;
    let text = render(&rust, &js, &assets);

    let path = root.join(OUTPUT);
    if check {
        let current = fs::read_to_string(&path).unwrap_or_default();
        if current != text {
            bail!("{OUTPUT} is out of date; run `cargo xtask licenses` and commit the result");
        }
        println!("{OUTPUT} is up to date");
        return Ok(());
    }
    fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
    println!(
        "wrote {OUTPUT}: {} crates, {} packages, {} assets",
        count(&rust),
        count(&js),
        assets.len()
    );
    Ok(())
}

/// One licence text and everything distributed under it.
struct Group {
    licence: String,
    text: String,
    users: BTreeSet<String>,
}

fn count(groups: &[Group]) -> usize {
    groups
        .iter()
        .flat_map(|g| &g.users)
        .collect::<BTreeSet<_>>()
        .len()
}

// ---- Rust -------------------------------------------------------------------------------------

#[derive(Deserialize)]
struct About {
    licenses: Vec<AboutLicence>,
}

#[derive(Deserialize)]
struct AboutLicence {
    id: String,
    text: String,
    used_by: Vec<AboutUse>,
}

#[derive(Deserialize)]
struct AboutUse {
    #[serde(rename = "crate")]
    krate: AboutCrate,
}

#[derive(Deserialize)]
struct AboutCrate {
    name: String,
    version: String,
}

fn rust(root: &Path) -> anyhow::Result<Vec<Group>> {
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".into());

    // `--frozen` below reads only what is on disk, and a Linux machine that has only ever built
    // for Linux has never downloaded the Windows-only crates. Fetching is a no-op when they are
    // there, and it is what makes the run the same on a fresh CI runner as here.
    let fetched = Command::new(&cargo)
        .current_dir(root)
        .args(["fetch", "--locked", "--quiet"])
        .status()
        .context("running cargo fetch")?;
    if !fetched.success() {
        bail!("cargo fetch failed ({fetched})");
    }

    let out = Command::new(&cargo)
        .current_dir(root)
        .args([
            "about",
            "generate",
            "--format",
            "json",
            "--frozen",
            "--fail",
            "--manifest-path",
            "desktop/app/Cargo.toml",
            "--config",
            "desktop/crates/xtask/about.toml",
        ])
        .output()
        .context("running cargo about")?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        if stderr.contains("no such command") {
            bail!(
                "cargo-about is not installed; \
                 `cargo install cargo-about --locked --features cli --version {CARGO_ABOUT}`"
            );
        }
        bail!("cargo about failed ({}):\n{stderr}", out.status);
    }
    let about: About =
        serde_json::from_slice(&out.stdout).context("reading cargo about's output")?;

    Ok(about
        .licenses
        .into_iter()
        .map(|l| Group {
            licence: l.id,
            text: l.text,
            users: l
                .used_by
                .into_iter()
                .map(|u| format!("{} {}", u.krate.name, u.krate.version))
                .collect(),
        })
        .collect())
}

// ---- JavaScript -------------------------------------------------------------------------------

#[derive(Deserialize)]
struct Package {
    name: String,
    versions: Vec<String>,
    paths: Vec<String>,
    license: String,
    #[serde(default)]
    author: Value,
}

fn javascript(root: &Path) -> anyhow::Result<Vec<Group>> {
    let mut args = Vec::new();
    for package in JS_PACKAGES {
        args.extend(["--filter", package]);
    }
    args.extend(["licenses", "list", "--prod", "--json"]);
    let out = Command::new("pnpm")
        .current_dir(root)
        .args(&args)
        .output()
        .context("running pnpm licenses (is pnpm on PATH, and `pnpm install` done?)")?;
    if !out.status.success() {
        bail!(
            "pnpm licenses failed ({}):\n{}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let listed: BTreeMap<String, Vec<Package>> =
        serde_json::from_slice(&out.stdout).context("reading pnpm licenses' output")?;

    // Keyed by the declared licence as well as the text, so a package that says
    // `Apache-2.0 OR MIT` is never filed under the heading of one that says `MIT` merely because
    // both ship the same file.
    let mut by_text: BTreeMap<(String, String), Group> = BTreeMap::new();
    for package in listed.into_values().flatten() {
        let licence = CORRECTIONS
            .iter()
            .find(|(name, _)| *name == package.name)
            .map_or(package.license.clone(), |(_, licence)| {
                (*licence).to_owned()
            });
        for (version, dir) in package.versions.iter().zip(&package.paths) {
            let text = match licence_files(Path::new(dir))? {
                Some(text) => text,
                None => standard_text(&package, &licence)?,
            };
            by_text
                .entry((licence.clone(), text.clone()))
                .or_insert_with(|| Group {
                    licence: licence.clone(),
                    text,
                    users: BTreeSet::new(),
                })
                .users
                .insert(format!("{} {version}", package.name));
        }
    }
    Ok(by_text.into_values().collect())
}

/// Every licence and notice file a package ships, in name order, or `None` when it ships none.
/// Apache-2.0 asks for the NOTICE file as well as the licence, so both are read.
fn licence_files(dir: &Path) -> anyhow::Result<Option<String>> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|name| {
            let lower = name.to_lowercase();
            lower.starts_with("licen")
                || lower.starts_with("copying")
                || lower.starts_with("notice")
        })
        .collect();
    if names.is_empty() {
        return Ok(None);
    }
    names.sort();
    let mut texts = Vec::new();
    for name in names {
        let path = dir.join(&name);
        let text =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        texts.push(normalise(&text));
    }
    Ok(Some(texts.join("\n\n")))
}

/// The standard text of a licence for a package that declares it and ships no file of its own,
/// with the author the package names as the holder. Only the licences that actually occur are
/// known here: a new one fails the run, so it gets looked at rather than listed without a text.
fn standard_text(package: &Package, licence: &str) -> anyhow::Result<String> {
    let holder = match &package.author {
        Value::String(name) => name.clone(),
        Value::Object(fields) => fields
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("its authors")
            .to_owned(),
        _ => "its authors".to_owned(),
    };
    let texts: Vec<&str> = match licence {
        "MIT" => vec![MIT],
        "ISC" => vec![ISC],
        "MIT AND ISC" => vec![MIT, ISC],
        other => bail!(
            "{} declares {other} and ships no licence file; add its text to licenses.rs",
            package.name
        ),
    };
    let notice = format!(
        "{} ships no licence file; its package.json declares {licence}, and the standard text \
         is reproduced here.\n\nCopyright (c) {holder}",
        package.name
    );
    Ok(std::iter::once(notice)
        .chain(texts.into_iter().map(str::to_owned))
        .collect::<Vec<_>>()
        .join("\n\n"))
}

// ---- Fonts and marks --------------------------------------------------------------------------

struct Asset {
    name: String,
    version: String,
    licence: String,
    why: &'static str,
    text: String,
}

fn assets(root: &Path) -> anyhow::Result<Vec<Asset>> {
    let modules = root.join("desktop/frontend/node_modules");
    ASSETS
        .iter()
        .map(|(name, why)| {
            let dir = modules.join(name);
            let manifest: Value = serde_json::from_str(
                &fs::read_to_string(dir.join("package.json"))
                    .with_context(|| format!("reading {name}'s package.json"))?,
            )?;
            let field = |key: &str| manifest[key].as_str().unwrap_or_default().to_owned();
            let text =
                licence_files(&dir)?.with_context(|| format!("{name} ships no licence file"))?;
            Ok(Asset {
                name: (*name).to_owned(),
                version: field("version"),
                licence: field("license"),
                why,
                text,
            })
        })
        .collect()
}

// ---- Output -----------------------------------------------------------------------------------

fn render(rust: &[Group], js: &[Group], assets: &[Asset]) -> String {
    let mut out = String::new();
    out.push_str(HEADER);
    out.push_str(&format!(
        "- **Rust crates:** {} crates under {} licence texts\n\
         - **JavaScript packages:** {} packages under {} licence texts\n\
         - **Fonts and marks:** {}\n\n",
        count(rust),
        rust.len(),
        count(js),
        js.len(),
        assets.len()
    ));
    out.push_str(LINUX_NOTE);

    section(&mut out, "Rust crates", RUST_INTRO, rust);
    section(&mut out, "JavaScript packages", JS_INTRO, js);

    out.push_str("## Fonts and marks\n\n");
    for asset in assets {
        out.push_str(&format!(
            "### {} {} — {}\n\n{}\n\n",
            asset.name, asset.version, asset.licence, asset.why
        ));
        fenced(&mut out, &asset.text);
    }
    out
}

fn section(out: &mut String, title: &str, intro: &str, groups: &[Group]) {
    out.push_str(&format!("## {title}\n\n{intro}\n\n"));

    let mut totals: BTreeMap<&str, usize> = BTreeMap::new();
    for group in groups {
        *totals.entry(&group.licence).or_default() += group.users.len();
    }
    out.push_str("| Licence | Count |\n| --- | --- |\n");
    for (licence, n) in &totals {
        out.push_str(&format!("| {licence} | {n} |\n"));
    }
    out.push('\n');

    let mut sorted: Vec<&Group> = groups.iter().collect();
    sorted.sort_by(|a, b| (&a.licence, a.users.first()).cmp(&(&b.licence, b.users.first())));
    for group in sorted {
        let users: Vec<String> = group.users.iter().map(|u| format!("`{u}`")).collect();
        out.push_str(&format!(
            "### {}\n\nUsed by {}.\n\n",
            group.licence,
            users.join(", ")
        ));
        fenced(out, &group.text);
    }
}

/// A licence text in a fence longer than any run of backticks inside it, so no text can close
/// its own block and be read as Markdown.
fn fenced(out: &mut String, text: &str) {
    let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest.max(2) + 1);
    out.push_str(&format!("{fence}text\n{}\n{fence}\n\n", normalise(text)));
}

fn normalise(text: &str) -> String {
    text.replace("\r\n", "\n").trim().to_owned()
}

const HEADER: &str = "# Third-party licences

Gantry is built on other people's work, and most of their licences ask that their notice travel
with the program. This file is that notice for everything in a released build: the Rust crates
compiled into the binary, the JavaScript packages bundled into its interface, and the fonts and
marks it ships. It is installed with the app and attached to every release.

Gantry's own licence is in [`LICENSE`](LICENSE), and [`LICENSING.md`](LICENSING.md) explains it.

This file is generated by `cargo xtask licenses` from `Cargo.lock` and `pnpm-lock.yaml`; do not
edit it by hand. The release workflow refuses to publish a build it is out of date for.

";

const LINUX_NOTE: &str = "The Linux AppImage also carries the shared libraries it needs from Ubuntu
24.04 — WebKitGTK, JavaScriptCore, GTK, GLib, libsoup, GStreamer and what they depend on — so
that it runs on distributions that lack them. They are unmodified, they are under their own
licences (mostly the LGPL), and their sources are in the Ubuntu archive. The `.deb` and `.rpm`
packages and the Windows installer carry none of them: they use the system's copies, and on
Windows the WebView2 runtime is installed from Microsoft.

";

const RUST_INTRO: &str = "Every crate in the dependency graph of the app for Linux and Windows
x86_64, build scripts and development dependencies excluded. Where a crate offers a choice of
licences it is listed under the one taken, which is the first its expression allows from the
`accepted` list in `desktop/crates/xtask/about.toml`.";

const JS_INTRO: &str = "Every production dependency of the interface and the artifact runtime,
with the licence files each package ships. Packages that declare the same licence and ship an
identical text are listed together under it.";

const MIT: &str = "Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the \"Software\"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED \"AS IS\", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.";

const ISC: &str = "Permission to use, copy, modify, and/or distribute this software for any
purpose with or without fee is hereby granted, provided that the above
copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED \"AS IS\" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.";
