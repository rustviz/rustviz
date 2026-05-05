//! Public interface to the rustviz2 plugin.
//!
//! [`Rustviz::new`] runs the plugin against a single-file crate built
//! from `code` and returns the rendered code-panel and timeline-panel SVGs.
//!
//! # Example
//!
//! ```no_run
//! use rustviz2::Rustviz;
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
//!   playground server explicitly opts into it. See `SECURITY.md` for
//!   the full sandboxing contract.
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

const TOOLCHAIN: &str = r#"[toolchain]
channel = "nightly-2025-08-20"
components = ["rust-src", "rustc-dev", "llvm-tools-preview"]"#;

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
/// guarantees described in SECURITY.md.
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
    let status = Command::new("cargo")
        .args(["new", "--lib", "test-crate"])
        .current_dir(root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if !status.success() {
        return Err(Box::new(RvError::FsError(
            "cargo new failed".to_string(),
        )));
    }

    let cwd = root.join("test-crate");
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
