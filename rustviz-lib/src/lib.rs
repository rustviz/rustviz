//! Public interface to the RustViz rustc plugin.
//!
//! [`Rustviz::new`] runs the plugin against a single-file crate built
//! from `code` and returns the rendered code-panel and timeline-panel SVGs.
//!
//! # Example
//!
//! ```no_run
//! use rustviz_lib::Rustviz;
//!
//! let code = r#"
//! fn main() {
//!     let s = String::from("hello");
//!     let t = s;
//!     println!("{}", t);
//! }
//! "#;
//!
//! let rv = Rustviz::new(code).expect("plugin invocation failed");
//! std::fs::write("code.svg", rv.code_panel_string()).unwrap();
//! std::fs::write("timeline.svg", rv.timeline_panel_string()).unwrap();
//! ```
//!
//! # Execution backends
//!
//! There are two backends, selected by the `RV_RUNNER` env var:
//!
//! - `RV_RUNNER=local` (default) shells out to `cargo rv-plugin` against
//!   a generated tempdir crate. Fast, no Docker dependency. **Not safe for
//!   untrusted input** — proc-macro expansion in user code is arbitrary
//!   code execution. Right choice for library callers, the CLI, mdbook
//!   builds, and any other context where the input is trusted.
//! - `RV_RUNNER=docker` runs the plugin inside the
//!   `rustviz/rustviz-runner` image with no network, a read-only
//!   filesystem, tmpfs-backed `/work`, and capped memory / CPU / pids /
//!   wall-time. The only backend appropriate for untrusted input; the
//!   playground server explicitly opts into it. See
//!   `playground/SECURITY.md` for the full sandboxing contract.
//!
//! The default flipped from `docker` to `local` in PR C of the reorg —
//! library use is the common case, and the previous default forced every
//! caller to either install Docker or set the env var.

use std::{
    env,
    error::Error,
    fmt, fs,
    io::Write,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use tempfile::tempdir;

/// Verbatim contents of `rust-toolchain.toml`, embedded at compile
/// time. The single source of truth for which nightly + components
/// RustViz needs — `setup.sh` (defers to rustup auto-install via the
/// workspace-root copy), the local-runner backend below (writes it
/// into the per-request temp crate), and `rustviz init` (parses it
/// for the rustup install command) all derive from this one constant.
///
/// The file ships in two locations: the workspace root (where rustup
/// reads it for `cargo` invocations against the workspace) and inside
/// this crate (so `cargo publish` can include it in the published
/// tarball — `include_str!` paths can't reach above the crate root).
/// `scripts/bump-version.sh` keeps the two in sync, and a unit test
/// below asserts they match when both exist.
pub const TOOLCHAIN: &str = include_str!("../rust-toolchain.toml");

/// Channel string from `rust-toolchain.toml` — e.g. `"nightly-2025-08-20"`.
/// Used by `rustviz init` to pin the `+toolchain` selector when
/// invoking `cargo install` against the plugin.
pub fn toolchain_channel() -> &'static str {
    parse_toolchain_field("channel")
        .and_then(|v| trim_quotes(v))
        .expect("rust-toolchain.toml is missing a `channel = \"…\"` line")
}

/// Components from `rust-toolchain.toml`, e.g.
/// `["rust-src", "rustc-dev", "llvm-tools-preview"]`. Used by
/// `rustviz init` for the `--component` flag of `rustup toolchain
/// install`.
pub fn toolchain_components() -> Vec<&'static str> {
    let raw = parse_toolchain_field("components")
        .expect("rust-toolchain.toml is missing a `components = […]` line");
    let inside = raw.trim().trim_start_matches('[').trim_end_matches(']');
    inside
        .split(',')
        .map(str::trim)
        .filter_map(trim_quotes)
        .filter(|s| !s.is_empty())
        .collect()
}

/// Tiny, format-specific parser. We don't want a full TOML
/// dependency just for two fields — `rust-toolchain.toml` is
/// hand-edited and stable in shape, so a line scan is plenty.
/// Returns the slice after the first `=` on a `key = …` line at
/// any indentation, or `None` if no such line exists.
fn parse_toolchain_field(key: &str) -> Option<&'static str> {
    for line in TOOLCHAIN.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix(key) {
            // Allow `key=` and `key =` (any whitespace).
            let after_key = rest.trim_start();
            if let Some(eq_rest) = after_key.strip_prefix('=') {
                return Some(eq_rest.trim());
            }
        }
    }
    None
}

fn trim_quotes(s: &str) -> Option<&str> {
    let s = s.trim();
    s.strip_prefix('"').and_then(|s| s.strip_suffix('"'))
}

/// Default image tag for the docker backend.
const DEFAULT_RUNNER_IMAGE: &str = "rustviz/rustviz-runner:latest";

/// Tooltip + cross-panel highlighting glue for RustViz visualizations,
/// as a raw JS string. Embed this between `<script>…</script>` tags
/// in any HTML page that hosts a code panel + timeline panel pair
/// (whether they're rendered as `<object>` tags loading external
/// SVGs, or inlined directly as `<svg>` elements). Each visualization
/// pair is identified by a shared CSS class plus the `code_panel` /
/// `tl_panel` discriminator (e.g. `class="example-1 code_panel"`).
///
/// Both consumers — the mdbook preprocessor and the `rustviz` CLI's
/// `--html` mode — pull the same JS from here so we have one place
/// to update if the SVG schema shifts.
pub const HELPERS_JS: &str = include_str!("helpers.js");

/// Hard wall-clock cap on a single visualization request, in seconds.
/// Compilation of legitimate examples completes well under a second; this is
/// a safety net against malicious input that wedges rustc.
const RUNNER_TIMEOUT_SECS: u64 = 20;

#[derive(Debug)]
enum RvError {
    FsError(String),
    PluginError(String),
    Timeout,
    DockerUnavailable(String),
    BadConfig(String),
}

impl fmt::Display for RvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RvError::FsError(msg) => write!(f, "File system error: {}", msg),
            RvError::PluginError(msg) => write!(f, "Internal plugin error {}", msg),
            RvError::Timeout => write!(f, "Visualization timed out"),
            RvError::DockerUnavailable(msg) => write!(f, "Docker unavailable: {}", msg),
            RvError::BadConfig(msg) => write!(f, "Invalid runner configuration: {}", msg),
        }
    }
}

impl Error for RvError {}

/// A rendered RustViz visualization: the code panel, the timeline
/// panel, and the SVG height needed to display them together.
///
/// Construct via [`Rustviz::new`]. The two SVG strings are designed
/// to be rendered side by side; see [`HELPERS_JS`] for the tooltip
/// + cross-panel-highlighting glue you'll want to drop on the page
/// alongside them.
#[derive(Debug)]
pub struct Rustviz {
    code_panel: String,
    timeline_panel: String,
    height: i32,
}

impl Rustviz {
    /// Render `code_str` (a single Rust source file's contents)
    /// through the RustViz plugin and return the two SVG panels.
    ///
    /// Backend selection is governed by the `RV_RUNNER` env var
    /// (default `local` — see crate-level docs for the security
    /// implications). Both backends require the nightly toolchain
    /// pinned by `rust-toolchain.toml`; the `local` backend
    /// additionally needs `cargo rv-plugin` on `PATH`.
    pub fn new(code_str: &str) -> Result<Rustviz, Box<dyn Error>> {
        let raw = match runner_backend()?.as_str() {
            "docker" => run_docker(code_str)?,
            "local" => run_local(code_str)?,
            other => {
                return Err(Box::new(RvError::BadConfig(format!(
                    "RV_RUNNER must be \"docker\" or \"local\" (got {:?})",
                    other
                ))));
            }
        };
        parse_output(&raw)
    }

    /// SVG markup for the code panel — the source listing with
    /// per-variable colored spans that the helpers script
    /// highlights on hover.
    pub fn code_panel_string(&self) -> String {
        self.code_panel.clone()
    }

    /// SVG markup for the timeline panel — one column per RAP
    /// (resource access point), with arrows for ownership transfers
    /// and dots for events.
    pub fn timeline_panel_string(&self) -> String {
        self.timeline_panel.clone()
    }

    /// Height in pixels needed to render both panels without
    /// clipping. The two panels share this height; pass it to your
    /// container element when laying them out side by side.
    pub fn height(&self) -> i32 {
        self.height
    }
}

fn runner_backend() -> Result<String, Box<dyn Error>> {
    // Default = `local` (fast, no Docker dep). Callers that handle
    // untrusted input — i.e. the playground — explicitly set
    // `RV_RUNNER=docker` to opt into the sandboxed backend.
    Ok(env::var("RV_RUNNER").unwrap_or_else(|_| "local".to_string()))
}

/// Production path: spawn a sandboxed container, pipe the user's source on
/// stdin, capture stdout. Container flags must be kept in sync with the
/// guarantees described in playground/SECURITY.md.
fn run_docker(code_str: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let image = env::var("RV_RUNNER_IMAGE").unwrap_or_else(|_| DEFAULT_RUNNER_IMAGE.to_string());

    let mut child = Command::new("docker")
        .args([
            "run",
            "--rm",
            "-i",
            "--network=none",
            "--read-only",
            "--memory=512m",
            "--memory-swap=512m",
            "--cpus=1",
            "--pids-limit=64",
            "--cap-drop=ALL",
            "--security-opt=no-new-privileges",
            "--tmpfs=/work:rw,size=128m,mode=1777",
            "--tmpfs=/tmp:rw,size=32m",
            &image,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| RvError::DockerUnavailable(format!("failed to spawn docker: {}", e)))?;

    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(code_str.as_bytes())?;
    }
    drop(child.stdin.take());

    let deadline = Instant::now() + Duration::from_secs(RUNNER_TIMEOUT_SECS);
    loop {
        if let Some(status) = child.try_wait()? {
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            if let Some(mut s) = child.stdout.take() {
                std::io::Read::read_to_end(&mut s, &mut stdout)?;
            }
            if let Some(mut s) = child.stderr.take() {
                std::io::Read::read_to_end(&mut s, &mut stderr)?;
            }
            if !status.success() {
                return Err(Box::new(RvError::PluginError(
                    String::from_utf8_lossy(&stderr).to_string(),
                )));
            }
            return Ok(stdout);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Box::new(RvError::Timeout));
        }
        thread::sleep(Duration::from_millis(50));
    }
}

/// Dev-only path: run the plugin in-process against a tempdir. Convenient
/// when iterating without Docker, but UNSAFE for untrusted input — proc
/// macros in user code execute as the host process.
fn run_local(code_str: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let tempdir = tempdir()?;
    let root = tempdir.path();
    // Crate name surfaces in rustc/cargo error messages
    // ("could not compile `user-code`"), so use a name that reads as
    // "this is the user's code" rather than a leaked internal
    // placeholder.
    let status = Command::new("cargo")
        .args(["new", "--lib", "user-code"])
        .current_dir(root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if !status.success() {
        return Err(Box::new(RvError::FsError(
            "cargo new failed".to_string(),
        )));
    }

    let cwd = root.join("user-code");
    fs::write(cwd.join("rust-toolchain.toml"), TOOLCHAIN)?;
    fs::write(cwd.join("src").join("lib.rs"), code_str)?;

    let output = Command::new("cargo")
        .arg("rv-plugin")
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;

    if !output.status.success() {
        return Err(Box::new(RvError::PluginError(
            String::from_utf8_lossy(&output.stderr).to_string(),
        )));
    }
    Ok(output.stdout)
}

fn parse_output(stdout: &[u8]) -> Result<Rustviz, Box<dyn Error>> {
    let stdout = std::str::from_utf8(stdout)?;
    let parts: Vec<&str> = stdout.splitn(2, ":::").collect();
    if parts.len() != 2 {
        return Err(Box::new(RvError::PluginError(format!(
            "Unexpected output format {}",
            stdout
        ))));
    }
    let code_p = parts[0];
    let time_p = parts[1];

    let height = match time_p.find("height=") {
        Some(index) => {
            let start = index + "height=\"".len();
            if let Some(end) = time_p[start..].find("px\"") {
                time_p[start..start + end].parse::<i32>()?
            } else {
                return Err(Box::new(RvError::PluginError(format!(
                    "couldn't find px height identifier {}",
                    time_p
                ))));
            }
        }
        None => {
            return Err(Box::new(RvError::PluginError(format!(
                "couldn't find height identifier {}",
                time_p
            ))));
        }
    };

    Ok(Rustviz {
        code_panel: code_p.to_string(),
        timeline_panel: time_p.to_string(),
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pin: the channel from `rust-toolchain.toml` parses as a
    /// non-empty `nightly-…` string. Catches a future malformed
    /// edit to that file before it hits the rustup invocation
    /// in `rustviz init`.
    #[test]
    fn channel_is_a_nightly() {
        let c = toolchain_channel();
        assert!(
            c.starts_with("nightly-"),
            "expected nightly-* channel, got {:?}",
            c
        );
        assert!(c.len() > "nightly-".len(), "channel suspiciously short: {:?}", c);
    }

    /// Pin: the rustc plugin needs all three of these components;
    /// dropping any of them silently in `rust-toolchain.toml`
    /// would manifest as a confusing build failure later.
    #[test]
    fn components_include_required_set() {
        let c = toolchain_components();
        for required in ["rust-src", "rustc-dev", "llvm-tools-preview"] {
            assert!(
                c.contains(&required),
                "missing required toolchain component {:?}; got {:?}",
                required,
                c
            );
        }
    }

    /// The crate ships its own copy of `rust-toolchain.toml` so the
    /// `include_str!` above survives `cargo publish`, but rustup reads
    /// the workspace-root copy when running cargo from the repo root.
    /// Drift between the two would mean the published lib pins one
    /// nightly while local builds resolve another — assert they match
    /// whenever the workspace copy is reachable (i.e. checkout-mode
    /// builds; published-crate builds skip the check silently).
    #[test]
    fn toolchain_files_match_in_workspace() {
        let workspace_copy = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("rust-toolchain.toml");
        if let Ok(workspace) = std::fs::read_to_string(&workspace_copy) {
            assert_eq!(
                workspace.trim(),
                TOOLCHAIN.trim(),
                "rustviz-lib/rust-toolchain.toml is out of sync with the workspace copy at {}; \
                 run scripts/bump-version.sh or copy the file manually",
                workspace_copy.display()
            );
        }
    }
}
