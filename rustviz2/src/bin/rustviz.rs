//! `rustviz` — render a single Rust source file to a RustViz
//! visualization.
//!
//! Output modes:
//!
//! - `rustviz svg foo.rs [-o DIR]` writes `foo.code.svg` and
//!   `foo.timeline.svg` side by side. Useful for embedding in your
//!   own HTML/Markdown workflow.
//! - `rustviz html foo.rs [-o FILE]` writes a single self-contained
//!   `foo.html` with both SVGs inlined and the tooltip JS embedded.
//!   Open in any browser; no server required.
//! - `rustviz init` installs the nightly toolchain + plugin needed
//!   for the above. One-time setup after `cargo install rustviz2`.
//!
//! `svg` and `html` call the same `rustviz2::Rustviz::new(code)`
//! API the playground and the mdbook preprocessor use, which
//! shells out to the rustc plugin (`cargo rv-plugin`).

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand};
use rustviz2::{HELPERS_JS, Rustviz};

/// Toolchain the rustc plugin links against. Must match
/// `rust-toolchain.toml` at the workspace root.
const NIGHTLY_TOOLCHAIN: &str = "nightly-2025-08-20";

/// Default git source for the rustviz2-plugin install. Forward-
/// looking to the eventual `rustviz/rustviz` consolidation; today
/// the code lives at `rustviz/rustviz2`. Override with
/// `--plugin-git` if you're working off a fork or a different
/// branch.
const DEFAULT_PLUGIN_GIT: &str = "https://github.com/rustviz/rustviz";

#[derive(Parser)]
#[command(
    name = "rustviz",
    version,
    about = "Render a Rust source file to a RustViz visualization.",
    long_about = None,
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Write the two SVG panels (code + timeline) as separate files.
    Svg {
        /// Path to the Rust source file to visualize.
        input: PathBuf,
        /// Directory to write the output SVGs into. Defaults to the
        /// input file's directory. Outputs are named
        /// `<stem>.code.svg` and `<stem>.timeline.svg`.
        #[arg(short, long, value_name = "DIR")]
        output: Option<PathBuf>,
    },
    /// Write a single self-contained HTML page with both SVGs +
    /// tooltip JS inlined.
    Html {
        /// Path to the Rust source file to visualize.
        input: PathBuf,
        /// Path to the output HTML file. Defaults to
        /// `<input-stem>.html` next to the input.
        #[arg(short, long, value_name = "FILE")]
        output: Option<PathBuf>,
        /// Page title. Defaults to the input file's stem.
        #[arg(long)]
        title: Option<String>,
    },
    /// Install the nightly toolchain + rustc plugin needed by
    /// `rustviz svg` / `rustviz html`. Run once after
    /// `cargo install rustviz2`.
    Init {
        /// Git URL to install the rustviz2-plugin from. Defaults to
        /// the canonical RustViz repo.
        #[arg(long, default_value = DEFAULT_PLUGIN_GIT, value_name = "URL")]
        plugin_git: String,
        /// Git branch / tag / commit to install the plugin from.
        /// Defaults to whatever the repo's default branch is.
        #[arg(long, value_name = "REF")]
        plugin_rev: Option<String>,
        /// Skip the rustup step. Use if the toolchain is already
        /// installed and you only want the plugin install retry.
        #[arg(long)]
        skip_toolchain: bool,
        /// Skip the cargo-install step. Use if you only want the
        /// rustup step.
        #[arg(long)]
        skip_plugin: bool,
        /// Print the commands that would run, but don't execute.
        #[arg(long)]
        dry_run: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Svg { input, output } => render_svg(&input, output.as_deref()),
        Commands::Html { input, output, title } => {
            render_html(&input, output.as_deref(), title.as_deref())
        }
        Commands::Init {
            plugin_git,
            plugin_rev,
            skip_toolchain,
            skip_plugin,
            dry_run,
        } => init(&plugin_git, plugin_rev.as_deref(), skip_toolchain, skip_plugin, dry_run),
    }
}

/// Stem of the input file's basename, used to name output files.
/// `path/to/foo.rs` → `"foo"`.
fn stem(input: &Path) -> Result<String> {
    input
        .file_stem()
        .and_then(|s| s.to_str())
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("input path has no usable file stem: {}", input.display()))
}

fn render(input: &Path) -> Result<Rustviz> {
    let code = fs::read_to_string(input)
        .with_context(|| format!("failed to read {}", input.display()))?;
    Rustviz::new(&code).map_err(|e| anyhow!(e.to_string()))
}

fn render_svg(input: &Path, output_dir: Option<&Path>) -> Result<()> {
    let rv = render(input)?;
    let stem = stem(input)?;
    let dir = match output_dir {
        Some(d) => d.to_path_buf(),
        None => input.parent().unwrap_or_else(|| Path::new(".")).to_path_buf(),
    };
    fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create {}", dir.display()))?;

    let code_path = dir.join(format!("{stem}.code.svg"));
    let tl_path = dir.join(format!("{stem}.timeline.svg"));
    fs::write(&code_path, rv.code_panel_string())
        .with_context(|| format!("failed to write {}", code_path.display()))?;
    fs::write(&tl_path, rv.timeline_panel_string())
        .with_context(|| format!("failed to write {}", tl_path.display()))?;

    eprintln!("wrote {}", code_path.display());
    eprintln!("wrote {}", tl_path.display());
    Ok(())
}

fn render_html(input: &Path, output: Option<&Path>, title: Option<&str>) -> Result<()> {
    let rv = render(input)?;
    let stem = stem(input)?;
    let title = title.unwrap_or(&stem);
    let out_path = match output {
        Some(p) => p.to_path_buf(),
        None => input
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(format!("{stem}.html")),
    };

    // Plain string concatenation, not format!() — the helpers JS
    // body has unescaped `{` / `}` everywhere and would blow up
    // any format-template engine. We give each panel its own pair
    // of identifying classes (`example-1 code_panel` and
    // `example-1 tl_panel`) so the helpers script's
    // `getElementsByClassName('example-1')[0|1]` lookup matches.
    let code_panel = annotate_panel(&rv.code_panel_string(), "example-1 code_panel");
    let tl_panel = annotate_panel(&rv.timeline_panel_string(), "example-1 tl_panel");
    let height = rv.height();

    let mut html = String::new();
    html.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n  <meta charset=\"utf-8\" />\n  <title>");
    html.push_str(&html_escape(title));
    html.push_str("</title>\n  <style>\n");
    html.push_str(STYLE_CSS);
    html.push_str("  </style>\n</head>\n<body>\n  <div class=\"vis-container\" style=\"height: ");
    html.push_str(&height.to_string());
    html.push_str("px\" onmouseenter=\"helpers('example-1')\">\n");
    html.push_str(&code_panel);
    html.push_str("\n");
    html.push_str(&tl_panel);
    html.push_str("\n  </div>\n  <script>\n");
    html.push_str(HELPERS_JS);
    html.push_str("\n  </script>\n</body>\n</html>\n");

    fs::write(&out_path, html)
        .with_context(|| format!("failed to write {}", out_path.display()))?;
    eprintln!("wrote {}", out_path.display());
    Ok(())
}

/// Inject a `class="…"` attribute into the leading `<svg …>` tag of
/// a panel string so the helpers script can find both panels by
/// class name (e.g. `example-1 code_panel` + `example-1 tl_panel`).
/// Returns the original string unchanged if no `<svg` tag is found.
fn annotate_panel(svg: &str, class: &str) -> String {
    let Some(open_start) = svg.find("<svg") else { return svg.to_owned() };
    // Insert `class="…"` right after `<svg` and before the rest of
    // the open tag's attributes.
    let mut out = String::with_capacity(svg.len() + class.len() + 12);
    out.push_str(&svg[..open_start + 4]);
    out.push_str(" class=\"");
    out.push_str(class);
    out.push('"');
    out.push_str(&svg[open_start + 4..]);
    out
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

const STYLE_CSS: &str = r#"
    body { margin: 1em; font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; }
    .vis-container { display: flex; align-items: flex-start; gap: 1em; }
    .vis-container > svg { background: #f1f1f1; }
"#;

/// One-shot toolchain + plugin bootstrap, equivalent to running
/// the two `rustup` / `cargo install` commands a setup-from-scratch
/// user would otherwise need to discover from docs.
fn init(
    plugin_git: &str,
    plugin_rev: Option<&str>,
    skip_toolchain: bool,
    skip_plugin: bool,
    dry_run: bool,
) -> Result<()> {
    if skip_toolchain && skip_plugin {
        bail!("--skip-toolchain and --skip-plugin together leave nothing to do");
    }

    if !skip_toolchain {
        let toolchain_args = [
            "toolchain",
            "install",
            NIGHTLY_TOOLCHAIN,
            "--profile",
            "minimal",
            "--component",
            "rust-src,rustc-dev,llvm-tools-preview",
        ];
        run_cmd("rustup", &toolchain_args, dry_run)
            .context("rustup toolchain install failed — make sure rustup is on PATH")?;
    }

    if !skip_plugin {
        // Install via cargo + the +nightly toolchain selector so the
        // plugin compiles against the same nightly the renderer
        // expects, regardless of the user's `rustup default`.
        let toolchain_arg = format!("+{}", NIGHTLY_TOOLCHAIN);
        let mut args: Vec<&str> = vec![
            &toolchain_arg,
            "install",
            "--git",
            plugin_git,
        ];
        if let Some(r) = plugin_rev {
            args.push("--rev");
            args.push(r);
        }
        // The plugin crate name doesn't match the repo name, so name
        // it explicitly. `--locked` keeps us on the lockfile the
        // upstream repo committed.
        args.extend_from_slice(&["--locked", "rustviz2-plugin"]);
        run_cmd("cargo", &args, dry_run)
            .context("cargo install rustviz2-plugin failed")?;
    }

    if dry_run {
        eprintln!("(dry run — nothing was actually installed)");
    } else {
        eprintln!();
        eprintln!("✓ rustviz init complete. Try: rustviz svg some_file.rs");
    }
    Ok(())
}

/// Run a child process inheriting stdio, mirroring its exit code.
/// In dry-run mode just print what would have run and return Ok.
fn run_cmd(program: &str, args: &[&str], dry_run: bool) -> Result<()> {
    eprintln!("$ {} {}", program, args.join(" "));
    if dry_run {
        return Ok(());
    }
    let status = Command::new(program)
        .args(args)
        .status()
        .with_context(|| format!("failed to spawn {}", program))?;
    if !status.success() {
        bail!("{} exited with {}", program, status);
    }
    Ok(())
}
