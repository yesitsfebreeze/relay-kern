# Auto-docs Session 1: Scaffold

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stand up the docs pipeline end-to-end with no LLM: xtask + rustdoc JSON + README collection + mdBook + CI publish to GitHub Pages. Produces a live Pages site showing every crate's README and full rustdoc, with empty `guides/` section.

**Architecture:** A new `xtask` crate drives everything via `cargo xtask docs <sub>`. `docs/book/` holds committed mdBook source. `flows.toml` exists but is empty. Rustdoc HTML is generated into `docs/book/src/api/` at build time (gitignored). A GitHub Actions workflow publishes to `gh-pages` on push to `relay`.

**Tech Stack:** Rust (xtask pattern), `cargo-metadata`, mdBook, GitHub Actions, Rust nightly (for rustdoc JSON — only in Session 2; Session 1 uses HTML rustdoc via stable `cargo doc`).

**Related spec:** `docs/superpowers/specs/2026-04-24-auto-docs-design.md`

---

## File Structure

**Create:**
- `tools/xtask/Cargo.toml` — new crate, bin target `xtask`.
- `tools/xtask/src/main.rs` — CLI dispatch (clap).
- `tools/xtask/src/docs/mod.rs` — `docs` subcommand module.
- `tools/xtask/src/docs/readmes.rs` — copy crate READMEs into mdBook tree.
- `tools/xtask/src/docs/summary.rs` — generate `SUMMARY.md` from workspace + flows.
- `tools/xtask/src/docs/build.rs` — orchestrate build (readmes + summary + rustdoc + mdbook).
- `tools/xtask/src/docs/check.rs` — CI lint: SUMMARY up-to-date, all crate READMEs present.
- `tools/xtask/src/docs/flows.rs` — load `flows.toml` (schema only; empty list OK).
- `tools/xtask/tests/integration.rs` — snapshot tests for summary generation.
- `docs/book/book.toml` — mdBook config.
- `docs/book/flows.toml` — empty flow registry.
- `docs/book/src/introduction.md` — hand-written intro.
- `docs/book/src/.gitignore` — ignore generated `api/`, `crates/`, `SUMMARY.md`.
- `docs/book/.gitignore` — ignore `book/` output.
- `.github/workflows/docs.yml` — CI publish.

**Modify:**
- `Cargo.toml` — add `tools/xtask` to workspace members, exclude from default build.
- `justfile` — add `docs`, `docs-serve`, `docs-check` recipes.
- `.cargo/config.toml` (create if absent) — alias `xtask = "run --package xtask --"`.

---

## Task 1: Workspace xtask crate

**Files:**
- Create: `tools/xtask/Cargo.toml`
- Create: `tools/xtask/src/main.rs`
- Create: `.cargo/config.toml`
- Modify: `Cargo.toml` (workspace members)

- [ ] **Step 1: Create xtask crate manifest**

`tools/xtask/Cargo.toml`:

```toml
[package]
name = "xtask"
version = "0.1.0"
edition = "2021"
publish = false

[[bin]]
name = "xtask"
path = "src/main.rs"

[dependencies]
anyhow = "1"
clap = { version = "4", features = ["derive"] }
cargo_metadata = "0.18"
serde = { version = "1", features = ["derive"] }
toml = "0.8"
walkdir = "2"
```

- [ ] **Step 2: Add stub `main.rs`**

`tools/xtask/src/main.rs`:

```rust
use anyhow::Result;
use clap::{Parser, Subcommand};

mod docs;

#[derive(Parser)]
#[command(name = "xtask", about = "Relay workspace tasks")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Documentation pipeline (integration guides + rustdoc + mdBook).
    Docs(docs::DocsArgs),
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Docs(args) => docs::run(args),
    }
}
```

- [ ] **Step 3: Add stub `docs` module**

`tools/xtask/src/docs/mod.rs`:

```rust
use anyhow::{bail, Result};
use clap::{Args, Subcommand};

mod build;
mod check;
mod flows;
mod readmes;
mod summary;

#[derive(Args)]
pub struct DocsArgs {
    #[command(subcommand)]
    sub: DocsSub,
}

#[derive(Subcommand)]
enum DocsSub {
    /// Regenerate integration guides (LLM). Stubbed in Session 1.
    Gen,
    /// Assemble mdBook tree and build.
    Build,
    /// Local preview server.
    Serve,
    /// Lint: SUMMARY current, crate READMEs present, flows valid.
    Check,
}

pub fn run(args: DocsArgs) -> Result<()> {
    match args.sub {
        DocsSub::Gen => bail!("`docs gen` lands in Session 2"),
        DocsSub::Build => build::run(),
        DocsSub::Serve => build::serve(),
        DocsSub::Check => check::run(),
    }
}
```

- [ ] **Step 4: Create empty module files**

`tools/xtask/src/docs/build.rs`:

```rust
use anyhow::Result;

pub fn run() -> Result<()> {
    todo!("implemented in Task 5")
}

pub fn serve() -> Result<()> {
    todo!("implemented in Task 5")
}
```

`tools/xtask/src/docs/check.rs`:

```rust
use anyhow::Result;

pub fn run() -> Result<()> {
    todo!("implemented in Task 7")
}
```

`tools/xtask/src/docs/flows.rs`: empty file (content in Task 6).
`tools/xtask/src/docs/readmes.rs`: empty file (content in Task 3).
`tools/xtask/src/docs/summary.rs`: empty file (content in Task 4).

- [ ] **Step 5: Register in workspace**

Modify top-level `Cargo.toml`, add at end of `[workspace]` members array:

```toml
    # Workspace tasks.
    "tools/xtask",
```

And add (create `[workspace.package]` section if absent, else add at bottom):

```toml
[workspace.metadata.xtask]
# Marker — ensures this key exists for future config.
```

- [ ] **Step 6: Add cargo alias**

`.cargo/config.toml`:

```toml
[alias]
xtask = "run --package xtask --release --quiet --"
```

- [ ] **Step 7: Verify build**

Run: `cargo build -p xtask`
Expected: compiles, zero warnings.

Run: `cargo xtask docs check`
Expected: fails with `not yet implemented` panic from `todo!` — confirms wiring.

- [ ] **Step 8: Commit**

```bash
git add tools/xtask Cargo.toml .cargo/config.toml
git commit -m "feat(xtask): scaffold workspace task runner with docs subcommand"
```

---

## Task 2: mdBook skeleton

**Files:**
- Create: `docs/book/book.toml`
- Create: `docs/book/flows.toml`
- Create: `docs/book/src/introduction.md`
- Create: `docs/book/src/.gitignore`
- Create: `docs/book/.gitignore`

- [ ] **Step 1: mdBook config**

`docs/book/book.toml`:

```toml
[book]
title = "Relay"
authors = ["Relay contributors"]
language = "en"
src = "src"

[output.html]
default-theme = "navy"
preferred-dark-theme = "navy"
git-repository-url = "https://github.com/febreeze/relay"
edit-url-template = "https://github.com/febreeze/relay/edit/relay/docs/book/{path}"
additional-css = []
no-section-label = false

[output.html.search]
enable = true
limit-results = 30
use-boolean-and = true
boost-title = 2
boost-hierarchy = 2
boost-paragraph = 1
expand = true
heading-split-level = 3

[output.html.fold]
enable = true
level = 1
```

- [ ] **Step 2: Empty flow registry**

`docs/book/flows.toml`:

```toml
# Integration flows. Hand-seeded. Each flow becomes a chapter in
# docs/book/src/guides/<name>.md regenerated by `cargo xtask docs gen`.
#
# Schema:
#   [[flow]]
#   name      = "slug"                # file name (required)
#   title     = "Display Title"       # chapter title in SUMMARY (required)
#   seeds     = ["crate-a", "crate-b"] # seed crates or relay node IDs (required, non-empty)
#   question  = "What does X look like end-to-end?" # narrative prompt (required)

flow = []
```

- [ ] **Step 3: Hand-written intro**

`docs/book/src/introduction.md`:

```markdown
# Relay

This book has two layers:

- **Guides** — how to use Relay's parts together. Each guide walks a
  flow across crates.
- **Crates** — per-crate READMEs. Reference material for each unit.
- **API** — rustdoc for everything. Linked from guides via symbol
  anchors.

If you're new, start with a guide. If you know what you need, jump to
the crate or the API.
```

- [ ] **Step 4: Gitignores for generated content**

`docs/book/.gitignore`:

```
book/
```

`docs/book/src/.gitignore`:

```
api/
crates/
SUMMARY.md
```

Rationale: `SUMMARY.md` is derived; `crates/` is copied from workspace
READMEs; `api/` is rustdoc output. All regenerated by `xtask docs build`.

- [ ] **Step 5: Commit**

```bash
git add docs/book
git commit -m "feat(docs): mdBook skeleton with empty flow registry"
```

---

## Task 3: README collector

**Files:**
- Modify: `tools/xtask/src/docs/readmes.rs`
- Create: `tools/xtask/tests/integration.rs`

- [ ] **Step 1: Write the failing test**

`tools/xtask/tests/integration.rs`:

```rust
use std::fs;
use tempfile::TempDir;

#[test]
fn collect_copies_crate_readmes_to_dest() {
    let tmp = TempDir::new().unwrap();
    let crate_dir = tmp.path().join("src/foo");
    fs::create_dir_all(&crate_dir).unwrap();
    fs::write(
        crate_dir.join("Cargo.toml"),
        "[package]\nname = \"foo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(crate_dir.join("README.md"), "# foo\n\nHi.\n").unwrap();
    // Minimal workspace.
    fs::write(
        tmp.path().join("Cargo.toml"),
        "[workspace]\nresolver = \"2\"\nmembers = [\"src/foo\"]\n",
    )
    .unwrap();
    fs::write(crate_dir.join("src").join("lib.rs"), "").unwrap_or(());
    fs::create_dir_all(crate_dir.join("src")).unwrap();
    fs::write(crate_dir.join("src/lib.rs"), "").unwrap();

    let dest = tmp.path().join("out");
    xtask::docs::readmes::collect(tmp.path(), &dest).unwrap();

    let copied = fs::read_to_string(dest.join("foo.md")).unwrap();
    assert!(copied.starts_with("# foo"));
}

#[test]
fn collect_skips_crates_without_readme() {
    let tmp = TempDir::new().unwrap();
    let crate_dir = tmp.path().join("src/bar");
    fs::create_dir_all(crate_dir.join("src")).unwrap();
    fs::write(
        crate_dir.join("Cargo.toml"),
        "[package]\nname = \"bar\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(crate_dir.join("src/lib.rs"), "").unwrap();
    fs::write(
        tmp.path().join("Cargo.toml"),
        "[workspace]\nresolver = \"2\"\nmembers = [\"src/bar\"]\n",
    )
    .unwrap();

    let dest = tmp.path().join("out");
    let report = xtask::docs::readmes::collect(tmp.path(), &dest).unwrap();

    assert_eq!(report.copied, 0);
    assert_eq!(report.skipped, vec!["bar".to_string()]);
}
```

Add to `tools/xtask/Cargo.toml`:

```toml
[lib]
path = "src/main.rs"

[dev-dependencies]
tempfile = "3"
```

Actually — `main.rs` can't be both lib and bin. Instead: move logic into a
lib and have `main.rs` thin. Adjust:

`tools/xtask/Cargo.toml` (replace prior):

```toml
[package]
name = "xtask"
version = "0.1.0"
edition = "2021"
publish = false

[lib]
path = "src/lib.rs"

[[bin]]
name = "xtask"
path = "src/main.rs"

[dependencies]
anyhow = "1"
clap = { version = "4", features = ["derive"] }
cargo_metadata = "0.18"
serde = { version = "1", features = ["derive"] }
toml = "0.8"
walkdir = "2"

[dev-dependencies]
tempfile = "3"
```

Move `mod docs;` into new `tools/xtask/src/lib.rs`:

```rust
pub mod docs;
```

`tools/xtask/src/main.rs` becomes:

```rust
use anyhow::Result;
use clap::{Parser, Subcommand};
use xtask::docs;

#[derive(Parser)]
#[command(name = "xtask", about = "Relay workspace tasks")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    Docs(docs::DocsArgs),
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Docs(args) => docs::run(args),
    }
}
```

And in `tools/xtask/src/docs/mod.rs` mark submodules `pub`:

```rust
pub mod build;
pub mod check;
pub mod flows;
pub mod readmes;
pub mod summary;
```

- [ ] **Step 2: Run test, verify it fails**

Run: `cargo test -p xtask`
Expected: FAIL — `collect` not defined.

- [ ] **Step 3: Implement `readmes::collect`**

`tools/xtask/src/docs/readmes.rs`:

```rust
use anyhow::{Context, Result};
use cargo_metadata::MetadataCommand;
use std::fs;
use std::path::Path;

pub struct Report {
    pub copied: usize,
    pub skipped: Vec<String>,
}

/// Copy every workspace crate's `README.md` into `<dest>/<crate-name>.md`.
/// Skips crates without a README (recorded in `Report::skipped`).
pub fn collect(workspace_root: &Path, dest: &Path) -> Result<Report> {
    fs::create_dir_all(dest)
        .with_context(|| format!("create dest {}", dest.display()))?;

    let meta = MetadataCommand::new()
        .current_dir(workspace_root)
        .no_deps()
        .exec()
        .context("cargo metadata")?;

    let mut copied = 0usize;
    let mut skipped = Vec::new();

    for pkg in &meta.workspace_packages() {
        let manifest_dir = pkg
            .manifest_path
            .parent()
            .context("crate manifest has no parent")?;
        let readme = manifest_dir.join("README.md");
        if !readme.as_std_path().is_file() {
            skipped.push(pkg.name.clone());
            continue;
        }
        let out = dest.join(format!("{}.md", pkg.name));
        fs::copy(readme.as_std_path(), &out)
            .with_context(|| format!("copy {} -> {}", readme, out.display()))?;
        copied += 1;
    }

    skipped.sort();
    Ok(Report { copied, skipped })
}
```

- [ ] **Step 4: Run tests, verify pass**

Run: `cargo test -p xtask`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add tools/xtask
git commit -m "feat(xtask): collect crate READMEs into mdBook tree"
```

---

## Task 4: SUMMARY.md generator

**Files:**
- Modify: `tools/xtask/src/docs/summary.rs`
- Modify: `tools/xtask/tests/integration.rs`

- [ ] **Step 1: Write failing test**

Append to `tools/xtask/tests/integration.rs`:

```rust
use xtask::docs::summary::{render, SummaryInput};

#[test]
fn summary_renders_with_no_flows_and_two_crates() {
    let input = SummaryInput {
        flows: vec![],
        crates: vec!["relay-base".into(), "relay-textarea".into()],
    };
    let out = render(&input);
    assert!(out.contains("# Summary"));
    assert!(out.contains("- [Introduction](introduction.md)"));
    assert!(out.contains("# Crates"));
    assert!(out.contains("- [relay-base](crates/relay-base.md)"));
    assert!(out.contains("- [relay-textarea](crates/relay-textarea.md)"));
    assert!(!out.contains("# Guides"));
}

#[test]
fn summary_renders_guides_section_when_flows_present() {
    let input = SummaryInput {
        flows: vec![("plugin-lifecycle".into(), "Using a plugin".into())],
        crates: vec!["relay-plugin".into()],
    };
    let out = render(&input);
    assert!(out.contains("# Guides"));
    assert!(out.contains("- [Using a plugin](guides/plugin-lifecycle.md)"));
}
```

- [ ] **Step 2: Run test, verify fail**

Run: `cargo test -p xtask summary`
Expected: FAIL — `render` / `SummaryInput` not found.

- [ ] **Step 3: Implement renderer**

`tools/xtask/src/docs/summary.rs`:

```rust
use std::fmt::Write;

pub struct SummaryInput {
    /// Tuples of `(slug, title)` — order preserved.
    pub flows: Vec<(String, String)>,
    /// Crate names — order preserved.
    pub crates: Vec<String>,
}

/// Render mdBook `SUMMARY.md` content. Fully derived; never hand-edited.
pub fn render(input: &SummaryInput) -> String {
    let mut s = String::new();
    s.push_str("# Summary\n\n");
    s.push_str("- [Introduction](introduction.md)\n\n");

    if !input.flows.is_empty() {
        s.push_str("# Guides\n\n");
        for (slug, title) in &input.flows {
            let _ = writeln!(s, "- [{title}](guides/{slug}.md)");
        }
        s.push('\n');
    }

    s.push_str("# Crates\n\n");
    for name in &input.crates {
        let _ = writeln!(s, "- [{name}](crates/{name}.md)");
    }

    s
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p xtask`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add tools/xtask
git commit -m "feat(xtask): render SUMMARY.md from flows + crate list"
```

---

## Task 5: Flow loader

**Files:**
- Modify: `tools/xtask/src/docs/flows.rs`
- Modify: `tools/xtask/tests/integration.rs`

- [ ] **Step 1: Write failing test**

Append to `tools/xtask/tests/integration.rs`:

```rust
use xtask::docs::flows::load;

#[test]
fn load_empty_flow_list_ok() {
    let tmp = TempDir::new().unwrap();
    let p = tmp.path().join("flows.toml");
    fs::write(&p, "flow = []\n").unwrap();
    let flows = load(&p).unwrap();
    assert!(flows.is_empty());
}

#[test]
fn load_single_flow_parses_fields() {
    let tmp = TempDir::new().unwrap();
    let p = tmp.path().join("flows.toml");
    fs::write(
        &p,
        r#"
[[flow]]
name = "plugin-lifecycle"
title = "Using a plugin"
seeds = ["relay-plugin", "relay-dispatch"]
question = "What touches a plugin from load to teardown?"
"#,
    )
    .unwrap();
    let flows = load(&p).unwrap();
    assert_eq!(flows.len(), 1);
    assert_eq!(flows[0].name, "plugin-lifecycle");
    assert_eq!(flows[0].title, "Using a plugin");
    assert_eq!(flows[0].seeds, vec!["relay-plugin", "relay-dispatch"]);
}

#[test]
fn load_rejects_empty_seeds() {
    let tmp = TempDir::new().unwrap();
    let p = tmp.path().join("flows.toml");
    fs::write(
        &p,
        r#"
[[flow]]
name = "bad"
title = "Bad"
seeds = []
question = "?"
"#,
    )
    .unwrap();
    let err = load(&p).unwrap_err();
    assert!(err.to_string().contains("seeds"));
}
```

- [ ] **Step 2: Run test, verify fail**

Run: `cargo test -p xtask flows`
Expected: FAIL — `load` not found.

- [ ] **Step 3: Implement loader**

`tools/xtask/src/docs/flows.rs`:

```rust
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct Flow {
    pub name: String,
    pub title: String,
    pub seeds: Vec<String>,
    pub question: String,
}

#[derive(Debug, Deserialize)]
struct FlowsFile {
    #[serde(default)]
    flow: Vec<Flow>,
}

/// Load and validate `flows.toml`. Empty list OK.
pub fn load(path: &Path) -> Result<Vec<Flow>> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("read {}", path.display()))?;
    let file: FlowsFile = toml::from_str(&text)
        .with_context(|| format!("parse {}", path.display()))?;
    for f in &file.flow {
        if f.seeds.is_empty() {
            bail!("flow `{}`: seeds must be non-empty", f.name);
        }
        if f.name.is_empty() {
            bail!("flow has empty name");
        }
    }
    Ok(file.flow)
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p xtask`
Expected: PASS (7 tests).

- [ ] **Step 5: Commit**

```bash
git add tools/xtask
git commit -m "feat(xtask): load and validate flows.toml"
```

---

## Task 6: `docs build` orchestrator

**Files:**
- Modify: `tools/xtask/src/docs/build.rs`

- [ ] **Step 1: Wire build + serve**

`tools/xtask/src/docs/build.rs`:

```rust
use anyhow::{Context, Result};
use cargo_metadata::MetadataCommand;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use super::{flows, readmes, summary};

fn workspace_root() -> Result<PathBuf> {
    let meta = MetadataCommand::new().no_deps().exec().context("cargo metadata")?;
    Ok(meta.workspace_root.into_std_path_buf())
}

fn book_dir(root: &std::path::Path) -> PathBuf {
    root.join("docs").join("book")
}

/// Assemble `docs/book/src/` tree, run `mdbook build`, run `cargo doc`.
pub fn run() -> Result<()> {
    let root = workspace_root()?;
    let book = book_dir(&root);
    let src = book.join("src");

    // 1. Collect READMEs.
    let crates_dir = src.join("crates");
    if crates_dir.exists() {
        fs::remove_dir_all(&crates_dir)?;
    }
    let report = readmes::collect(&root, &crates_dir)?;
    println!("readmes: copied {}, skipped {}", report.copied, report.skipped.len());
    if !report.skipped.is_empty() {
        println!("  skipped (no README): {}", report.skipped.join(", "));
    }

    // 2. Load flows.
    let flows = flows::load(&book.join("flows.toml"))?;

    // 3. Render SUMMARY.
    let crate_names = list_crate_readme_slugs(&crates_dir)?;
    let input = summary::SummaryInput {
        flows: flows.iter().map(|f| (f.name.clone(), f.title.clone())).collect(),
        crates: crate_names,
    };
    fs::write(src.join("SUMMARY.md"), summary::render(&input))?;

    // 4. Rustdoc HTML into src/api/.
    let api_dir = src.join("api");
    if api_dir.exists() {
        fs::remove_dir_all(&api_dir)?;
    }
    fs::create_dir_all(&api_dir)?;
    let status = Command::new("cargo")
        .args(["doc", "--no-deps", "--workspace", "--target-dir"])
        .arg(root.join("target"))
        .status()
        .context("cargo doc")?;
    anyhow::ensure!(status.success(), "cargo doc failed");
    copy_dir_all(&root.join("target/doc"), &api_dir)?;

    // 5. mdbook build.
    let status = Command::new("mdbook")
        .arg("build")
        .current_dir(&book)
        .status()
        .context("mdbook build (install via: cargo install mdbook)")?;
    anyhow::ensure!(status.success(), "mdbook build failed");

    println!("docs built: {}", book.join("book").display());
    Ok(())
}

pub fn serve() -> Result<()> {
    let root = workspace_root()?;
    let book = book_dir(&root);
    // Rebuild once for fresh SUMMARY, then serve.
    run()?;
    let status = Command::new("mdbook")
        .arg("serve")
        .current_dir(&book)
        .status()
        .context("mdbook serve")?;
    anyhow::ensure!(status.success(), "mdbook serve failed");
    Ok(())
}

fn list_crate_readme_slugs(dir: &std::path::Path) -> Result<Vec<String>> {
    let mut v = Vec::new();
    if !dir.exists() {
        return Ok(v);
    }
    for e in fs::read_dir(dir)? {
        let e = e?;
        let name = e.file_name().to_string_lossy().to_string();
        if let Some(base) = name.strip_suffix(".md") {
            v.push(base.to_string());
        }
    }
    v.sort();
    Ok(v)
}

fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in walkdir::WalkDir::new(src) {
        let entry = entry?;
        let rel = entry.path().strip_prefix(src).unwrap();
        let target = dst.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}
```

- [ ] **Step 2: Install mdbook**

Run: `cargo install mdbook`
Expected: installs to cargo bin, version ≥ 0.4.

- [ ] **Step 3: End-to-end build**

Run: `cargo xtask docs build`
Expected: succeeds. Output directory `docs/book/book/index.html` exists. At least 20 crate pages appear in `docs/book/src/crates/`. `docs/book/src/SUMMARY.md` lists them.

- [ ] **Step 4: Sanity-open the output**

Run: `cargo xtask docs serve` then visit `http://localhost:3000`.
Expected: Sidebar shows "Crates" section with all README'd crates. Search box works. Each crate page renders. No "Guides" section (empty flows).

- [ ] **Step 5: Commit**

```bash
git add tools/xtask
git commit -m "feat(xtask): orchestrate mdBook build with READMEs, SUMMARY, rustdoc"
```

---

## Task 7: `docs check` for CI

**Files:**
- Modify: `tools/xtask/src/docs/check.rs`

- [ ] **Step 1: Implement check**

`tools/xtask/src/docs/check.rs`:

```rust
use anyhow::{bail, Context, Result};
use cargo_metadata::MetadataCommand;
use std::fs;
use std::path::PathBuf;

use super::flows;

/// CI lint. Verifies:
/// - `flows.toml` parses and every flow's seeds are non-empty (already in loader).
/// - every seed crate exists in the workspace.
/// - `docs/book/src/introduction.md` exists.
pub fn run() -> Result<()> {
    let meta = MetadataCommand::new().no_deps().exec().context("cargo metadata")?;
    let root: PathBuf = meta.workspace_root.into();
    let book = root.join("docs/book");

    if !book.join("src/introduction.md").is_file() {
        bail!("docs/book/src/introduction.md missing");
    }

    let flows = flows::load(&book.join("flows.toml"))?;

    let members: std::collections::BTreeSet<String> =
        meta.workspace_packages().iter().map(|p| p.name.clone()).collect();

    let mut errs = Vec::new();
    for f in &flows {
        for seed in &f.seeds {
            // Seeds that look like crate names (no `::` or `:`) must match.
            if !seed.contains(':') && !members.contains(seed) {
                errs.push(format!("flow `{}`: unknown seed crate `{}`", f.name, seed));
            }
        }
    }
    if !errs.is_empty() {
        bail!("docs check failed:\n  {}", errs.join("\n  "));
    }
    println!("docs check: OK ({} flows)", flows.len());
    Ok(())
}
```

- [ ] **Step 2: Run check locally**

Run: `cargo xtask docs check`
Expected: prints `docs check: OK (0 flows)`.

- [ ] **Step 3: Commit**

```bash
git add tools/xtask
git commit -m "feat(xtask): docs check lint for CI"
```

---

## Task 8: justfile recipes

**Files:**
- Modify: `justfile`

- [ ] **Step 1: Append recipes**

Append to `justfile`:

```make
# Build docs (mdBook + rustdoc).
docs:
    cargo xtask docs build

# Serve docs locally.
docs-serve:
    cargo xtask docs serve

# Lint docs for CI.
docs-check:
    cargo xtask docs check
```

- [ ] **Step 2: Verify**

Run: `just docs-check`
Expected: same output as `cargo xtask docs check`.

- [ ] **Step 3: Commit**

```bash
git add justfile
git commit -m "chore: just recipes for docs pipeline"
```

---

## Task 9: GitHub Actions publish

**Files:**
- Create: `.github/workflows/docs.yml`

- [ ] **Step 1: Write workflow**

`.github/workflows/docs.yml`:

```yaml
name: docs

on:
  push:
    branches: [relay]
  workflow_dispatch:

permissions:
  contents: read
  pages: write
  id-token: write

concurrency:
  group: pages
  cancel-in-progress: false

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust stable
        uses: dtolnay/rust-toolchain@stable

      - name: Cache cargo
        uses: Swatinem/rust-cache@v2

      - name: Install mdbook
        run: cargo install mdbook --locked

      - name: Docs check
        run: cargo xtask docs check

      - name: Build docs
        run: cargo xtask docs build

      - name: Upload Pages artifact
        uses: actions/upload-pages-artifact@v3
        with:
          path: docs/book/book

  deploy:
    needs: build
    runs-on: ubuntu-latest
    environment:
      name: github-pages
      url: ${{ steps.deployment.outputs.page_url }}
    steps:
      - id: deployment
        uses: actions/deploy-pages@v4
```

- [ ] **Step 2: Commit**

```bash
git add .github/workflows/docs.yml
git commit -m "ci: publish mdBook to GitHub Pages on push to relay"
```

- [ ] **Step 3: Note for user**

Settings → Pages → Build and deployment → Source: **GitHub Actions**. User must toggle this once in the repo settings — can't be done via code. Call out in the PR description.

---

## Task 10: Self-review pass

**Files:** none (audit only)

- [ ] **Step 1: Full build from clean**

Run:
```
cargo clean -p xtask
cargo xtask docs build
```
Expected: completes without error. `docs/book/book/index.html` exists.

- [ ] **Step 2: Full test**

Run: `cargo test -p xtask`
Expected: all tests pass.

- [ ] **Step 3: Confirm gitignore works**

Run: `git status`
Expected: no `docs/book/src/api/`, `docs/book/src/crates/`, `docs/book/src/SUMMARY.md`, `docs/book/book/` appear as untracked.

- [ ] **Step 4: Confirm empty `Guides` section absent**

Open `docs/book/book/index.html` in a browser, confirm sidebar shows Crates but not Guides.

- [ ] **Step 5: Commit any adjustments, or note clean**

If adjustments needed, commit them. Otherwise no-op.

---

## Out of scope (future sessions)

- **Session 2:** LLM generator (`docs gen`), `flows.toml` entries, rustdoc JSON for symbol resolution, anchor rewriting, cache.
- **Session 3:** lint expansion (dead anchors, orphaned guides), snapshot tests for generator, relay neighborhood query.

## Notes for the implementer

- Workspace has ~30 crates; not all have READMEs. `readmes::collect` reports skipped crates — that's expected for Session 1.
- Rustdoc HTML copied into `src/api/` is large (tens of MB). Gitignored; CI regenerates.
- `cargo xtask ...` alias uses `--release` for startup speed since xtask is re-run locally often. Tests don't need release.
- Windows: `mdbook serve` works natively; paths normalized via `PathBuf`.
- Don't add `relay` dependencies in Session 1. Flow loader just parses; generator (Session 2) will do the graph query.
