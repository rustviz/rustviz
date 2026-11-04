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
//! - `RV_RUNNER=sandbox` runs `playground/runner/rv-runner.sh` under the
//!   per-request sandbox launcher named by `RV_SANDBOX_CMD` (on Cloud Run,
//!   its built-in `sandbox do`). Used by the Cloud Run playground image,
//!   which bundles the toolchain, plugin and runner script.
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
    io::{Read, Write},
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
/// `scripts/bump-nightly.sh` keeps the two in sync, and a unit test
/// below asserts they match when both exist.
pub const TOOLCHAIN: &str = include_str!("../rust-toolchain.toml");

/// Channel string from `rust-toolchain.toml` — e.g. `"nightly-2026-05-01"`.
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

/// Coarse classification of a [`Rustviz::new`] failure, for callers that
/// present failures differently (the playground picks an HTTP status and
/// a heading from it). Get it with [`error_kind`] or [`RvError::kind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorKind {
    /// The user's program doesn't compile (type error, borrow-check
    /// error, syntax error, ...). Not a RustViz bug.
    DoesNotCompile,
    /// The program compiles but uses something the plugin can't handle
    /// yet (the plugin panicked). A RustViz limitation or bug.
    Unsupported,
    /// The compile didn't finish within the runner's time limit.
    Timeout,
    /// The sandbox / runner couldn't be started or is misconfigured.
    SandboxUnavailable,
    /// Anything else.
    Other,
}

impl ErrorKind {
    /// Stable kebab-case name, e.g. for an HTTP header or a CSS class.
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorKind::DoesNotCompile => "does-not-compile",
            ErrorKind::Unsupported => "unsupported",
            ErrorKind::Timeout => "timeout",
            ErrorKind::SandboxUnavailable => "sandbox-unavailable",
            ErrorKind::Other => "other",
        }
    }
}

/// Classify an error returned by [`Rustviz::new`]. Errors that didn't
/// come from RustViz itself (I/O, UTF-8, ...) are [`ErrorKind::Other`].
pub fn error_kind(err: &(dyn Error + 'static)) -> ErrorKind {
    err.downcast_ref::<RvError>()
        .map_or(ErrorKind::Other, RvError::kind)
}

/// First sentence of the [`RvError::DoesNotCompile`] message. Its
/// `Display` is this sentence, a blank line, then rustc's diagnostics.
pub const DOES_NOT_COMPILE_INTRO: &str = "This program doesn't compile, and RustViz only visualizes \
programs that rustc accepts, including the borrow checker. Here is rustc's error:";

/// Prefix of the line the plugin prints on stderr when it hits a construct
/// it can't handle (see `catch_plugin_panic` in rustviz-plugin/src/lib.rs;
/// keep the two in sync).
pub const UNSUPPORTED_PREFIX: &str = "rustviz: unsupported: ";

const ISSUES_URL: &str = "https://github.com/rustviz/rustviz/issues";

/// Error returned (boxed) by [`Rustviz::new`] for failures RustViz
/// recognizes. Downcast a `Box<dyn Error>` to this, or call
/// [`error_kind`], to branch on the kind.
#[derive(Debug)]
#[non_exhaustive]
pub enum RvError {
    /// The program doesn't compile; `diagnostics` is rustc's output with
    /// cargo's noise removed and the generated crate's path shown as
    /// `your code`.
    DoesNotCompile { diagnostics: String },
    /// The plugin can't visualize this program; `message` is a full,
    /// student-readable sentence (without [`UNSUPPORTED_PREFIX`]).
    Unsupported { message: String },
    /// The runner's time limit was hit.
    Timeout,
    /// The sandbox or runner couldn't be launched.
    SandboxUnavailable(String),
    /// `RV_RUNNER` / `RV_SANDBOX_CMD` are invalid.
    BadConfig(String),
    /// Setting up the temporary crate failed.
    FsError(String),
    /// Unclassified plugin failure (cleaned-up stderr, or a description).
    PluginError(String),
}

impl RvError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            RvError::DoesNotCompile { .. } => ErrorKind::DoesNotCompile,
            RvError::Unsupported { .. } => ErrorKind::Unsupported,
            RvError::Timeout => ErrorKind::Timeout,
            RvError::SandboxUnavailable(_) | RvError::BadConfig(_) => {
                ErrorKind::SandboxUnavailable
            }
            RvError::FsError(_) | RvError::PluginError(_) => ErrorKind::Other,
        }
    }
}

impl fmt::Display for RvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RvError::DoesNotCompile { diagnostics } => {
                write!(f, "{}\n\n{}", DOES_NOT_COMPILE_INTRO, diagnostics)
            }
            RvError::Unsupported { message } => write!(f, "{}", message),
            RvError::FsError(msg) => write!(f, "File system error: {}", msg),
            RvError::PluginError(msg) => write!(f, "{}", msg),
            RvError::Timeout => write!(f, "Visualization timed out"),
            RvError::SandboxUnavailable(msg) => write!(f, "Sandbox unavailable: {}", msg),
            RvError::BadConfig(msg) => write!(f, "Invalid runner configuration: {}", msg),
        }
    }
}

impl Error for RvError {}

/// Turn the stderr of a failed `cargo rv-plugin` run into an [`RvError`]:
///
/// 1. the plugin's `rustviz: unsupported:` line (or, from a plugin without
///    the panic handler, a raw `panicked at`) → [`RvError::Unsupported`];
/// 2. rustc diagnostics → [`RvError::DoesNotCompile`];
/// 3. anything else → [`RvError::PluginError`] with the same cleanup.
fn classify_failure(stderr: &str) -> RvError {
    let stderr = strip_ansi(stderr);
    if let Some(message) = stderr.lines().find_map(|l| {
        l.find(UNSUPPORTED_PREFIX)
            .map(|i| l[i + UNSUPPORTED_PREFIX.len()..].trim().to_string())
    }) {
        return RvError::Unsupported { message };
    }
    if let Some(detail) = raw_panic_detail(&stderr) {
        return RvError::Unsupported {
            message: format!(
                "RustViz can't visualize this program yet (internal error at {}). \
                 Please report it at {} with your snippet.",
                detail, ISSUES_URL
            ),
        };
    }
    let cleaned = clean_diagnostics(&stderr);
    // `error[E…]` covers type and borrow-check errors; "previous error"
    // (cargo's "due to N previous error(s)") also catches syntax errors,
    // which have no error code.
    if stderr.contains("error[E") || stderr.contains("previous error") {
        RvError::DoesNotCompile { diagnostics: cleaned }
    } else if cleaned.is_empty() {
        RvError::PluginError("The RustViz plugin failed without an error message.".to_string())
    } else {
        RvError::PluginError(cleaned)
    }
}

/// `"expr_visitor.rs:862: called `Option::unwrap()` on a `None` value"`
/// from a stock Rust panic message
/// (`thread 'rustc' (…) panicked at <file>:<line>:<col>:` + message line).
fn raw_panic_detail(stderr: &str) -> Option<String> {
    let mut lines = stderr.lines();
    let at = lines.find_map(|l| l.split_once("panicked at ").map(|(_, rest)| rest))?;
    let at = at.trim().trim_end_matches(':');
    // `<path>:<line>:<col>` → `<file name>:<line>`.
    let mut parts = at.rsplitn(3, ':');
    let loc = match (parts.next(), parts.next(), parts.next()) {
        (Some(_col), Some(line), Some(path)) => {
            let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
            format!("{}:{}", file, line)
        }
        _ => at.to_string(),
    };
    let msg = lines.next().unwrap_or("").trim();
    Some(if msg.is_empty() { loc } else { format!("{}: {}", loc, msg) })
}

/// Remove ANSI color escapes (`ESC [ … letter`), in case a caller forced
/// cargo's color output on.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.next() == Some('[') {
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Keep rustc's diagnostics, drop cargo's and the plugin's noise, and show
/// the generated crate's source file as `your code`.
fn clean_diagnostics(stderr: &str) -> String {
    const CARGO_PROGRESS: &[&str] = &[
        "Compiling ", "Checking ", "Finished ", "Blocking ", "Updating ", "Locking ",
        "Downloading ", "Downloaded ", "Running ", "Fresh ", "Adding ", "Creating ",
    ];
    let mut kept: Vec<String> = Vec::new();
    let mut in_caused_by = false;
    for line in stderr.lines() {
        let t = line.trim_start();
        if in_caused_by {
            // cargo indents the body of a "Caused by:" block.
            if line.starts_with(' ') || t.is_empty() {
                continue;
            }
            in_caused_by = false;
        }
        if t.starts_with("Caused by:") {
            in_caused_by = true;
            continue;
        }
        if CARGO_PROGRESS.iter().any(|p| t.starts_with(p))
            || t.starts_with("error: could not compile `")
            || t.starts_with("warning: build failed")
            || t.contains("-Ztrack-diagnostics: created at")
            || t.starts_with("note: run with `RUST_BACKTRACE=1`")
        {
            continue;
        }
        kept.push(rewrite_crate_paths(line));
    }
    // Dropping a trailing `= note:` can leave a dangling `  |` gutter line
    // at the end of a diagnostic; rustc never ends one that way.
    let mut out: Vec<&str> = Vec::new();
    for (i, line) in kept.iter().enumerate() {
        let next_blank = kept.get(i + 1).is_none_or(|n| n.trim().is_empty());
        if line.trim() == "|" && next_blank {
            continue;
        }
        if line.trim().is_empty() && out.last().is_none_or(|l| l.trim().is_empty()) {
            continue;
        }
        out.push(line);
    }
    out.join("\n").trim().to_string()
}

/// Replace the generated crate's `src/lib.rs` (relative, as cargo reports
/// it, or absolute, e.g. `/tmp/.tmpXYZ/user-code/src/lib.rs`) with
/// `your code`, keeping any `:line:col` suffix. Windows paths
/// (`src\\lib.rs`, `C:\\...\\user-code\\src\\lib.rs`) are handled too.
fn rewrite_crate_paths(line: &str) -> String {
    const FILES: [&str; 2] = ["src/lib.rs", "src\\lib.rs"];
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some((i, file)) = FILES
        .iter()
        .filter_map(|f| rest.find(f).map(|i| (i, *f)))
        .min_by_key(|(i, _)| *i)
    {
        let end = i + file.len();
        let start = rest[..i]
            .rfind(|c: char| c.is_whitespace() || "`'\"(=".contains(c))
            .map_or(0, |j| j + 1);
        let token = &rest[start..end];
        let boundary_ok = rest[end..]
            .chars()
            .next()
            .is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
        let norm = token.replace('\\', "/");
        let ours = norm == "src/lib.rs" || norm == "./src/lib.rs" || norm.ends_with("user-code/src/lib.rs");
        out.push_str(&rest[..start]);
        if ours && boundary_ok {
            out.push_str("your code");
        } else {
            out.push_str(token);
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

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
            "sandbox" => run_sandbox(code_str)?,
            "local" => run_local(code_str)?,
            other => {
                return Err(Box::new(RvError::BadConfig(format!(
                    "RV_RUNNER must be \"docker\", \"sandbox\" or \"local\" (got {:?})",
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

/// Docker path (local dev; formerly Fly production): spawn a sandboxed container,
/// pipe the user's source on stdin, capture stdout. Container flags must be
/// kept in sync with the guarantees described in playground/SECURITY.md.
fn run_docker(code_str: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let image = env::var("RV_RUNNER_IMAGE").unwrap_or_else(|_| DEFAULT_RUNNER_IMAGE.to_string());
    let mut cmd = Command::new("docker");
    cmd.args([
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
    ]);
    run_with_deadline(cmd, code_str, "docker")
}

/// Production path (Cloud Run): run the runner script inside a
/// per-request sandbox whose launcher is given by `RV_SANDBOX_CMD`, an
/// argv prefix split on whitespace. On Cloud Run that is the built-in
/// launcher, e.g.
///
/// ```text
/// RV_SANDBOX_CMD="/usr/local/gcp/bin/sandbox do --env PATH=… --"
/// ```
///
/// which gives each request a fresh sandbox with no network egress, a
/// read-only view of the image, a private writable /tmp, and no access to
/// the service's env, secrets or metadata server. Any launcher with the
/// same "prefix + command" shape (bubblewrap, nsjail, firejail) works.
///
/// Fails closed: an unset or empty `RV_SANDBOX_CMD` is a configuration
/// error, never "run unsandboxed".
fn run_sandbox(code_str: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let prefix = env::var("RV_SANDBOX_CMD").unwrap_or_default();
    let mut argv = prefix.split_whitespace();
    let program = argv.next().ok_or_else(|| {
        RvError::BadConfig("RV_RUNNER=sandbox requires RV_SANDBOX_CMD".to_string())
    })?;
    let script =
        env::var("RV_RUNNER_SCRIPT").unwrap_or_else(|_| "/usr/local/bin/rv-runner".to_string());
    let mut cmd = Command::new(program);
    cmd.args(argv).arg(script);
    run_with_deadline(cmd, code_str, "sandbox launcher")
}

/// Printed on stderr by playground/runner/rv-runner.sh when its inner
/// timeout kills the plugin.
const RUNNER_TIMEOUT_SENTINEL: &str = "rustviz-runner: compile timed out";

/// Cap on how much of each child stream we keep. The largest curated
/// example renders to ~30 KiB; anything beyond a few MiB is pathological.
/// Excess output is drained (so the child never blocks on a full pipe)
/// and discarded.
const MAX_STREAM_BYTES: usize = 4 * 1024 * 1024;

/// Read `r` to EOF on a background thread, keeping at most
/// `MAX_STREAM_BYTES`.
fn drain_capped<R: Read + Send + 'static>(mut r: R) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut kept = Vec::new();
        let mut buf = [0u8; 16 * 1024];
        loop {
            match r.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let room = MAX_STREAM_BYTES.saturating_sub(kept.len());
                    kept.extend_from_slice(&buf[..n.min(room)]);
                }
            }
        }
        kept
    })
}

/// Spawn `cmd` with `code_str` on stdin and wait up to
/// `RUNNER_TIMEOUT_SECS`, returning stdout on success.
///
/// * stdin, stdout and stderr are each serviced on their own thread, so a
///   child that writes more than a pipe buffer before exiting can't
///   deadlock against us.
/// * On Unix the child leads its own process group and the deadline kills
///   the whole group, not just the launcher, so whatever it spawned
///   (cargo, rustc) dies with it. Launchers that daemonize their payload
///   (e.g. `docker run`, whose container outlives a killed client) must
///   also be bounded from the inside; `rv-runner.sh` wraps the plugin in
///   `timeout` for that reason.
fn run_with_deadline(
    mut cmd: Command,
    code_str: &str,
    what: &str,
) -> Result<Vec<u8>, Box<dyn Error>> {
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| RvError::SandboxUnavailable(format!("failed to spawn {}: {}", what, e)))?;

    let stdin = child.stdin.take();
    let input = code_str.as_bytes().to_vec();
    let writer = thread::spawn(move || {
        if let Some(mut s) = stdin {
            // A child that exits without reading stdin gives EPIPE here;
            // its exit status is the interesting error, not this one.
            let _ = s.write_all(&input);
        }
    });
    let out = drain_capped(child.stdout.take().expect("piped stdout"));
    let err = drain_capped(child.stderr.take().expect("piped stderr"));

    let deadline = Instant::now() + Duration::from_secs(RUNNER_TIMEOUT_SECS);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            kill_tree(&mut child);
            let _ = child.wait();
            let _ = writer.join();
            return Err(Box::new(RvError::Timeout));
        }
        thread::sleep(Duration::from_millis(20));
    };
    let _ = writer.join();
    let stdout = out.join().unwrap_or_default();
    let stderr = err.join().unwrap_or_default();
    if !status.success() {
        let stderr = String::from_utf8_lossy(&stderr).to_string();
        // rv-runner's own `timeout` fired before ours. Launchers that pass
        // the exit status through give 124; the ones that don't (Cloud
        // Run's `sandbox do` exits 1) still carry the sentinel on stderr.
        if status.code() == Some(124) || stderr.contains(RUNNER_TIMEOUT_SENTINEL) {
            return Err(Box::new(RvError::Timeout));
        }
        return Err(Box::new(classify_failure(&stderr)));
    }
    Ok(stdout)
}

#[cfg(unix)]
fn kill_tree(child: &mut std::process::Child) {
    // process_group(0) above made the child's pid its pgid.
    let pgid = child.id() as libc::pid_t;
    // SAFETY: plain syscall; a stale pgid just yields ESRCH.
    unsafe {
        libc::kill(-pgid, libc::SIGKILL);
    }
    let _ = child.kill();
}

#[cfg(not(unix))]
fn kill_tree(child: &mut std::process::Child) {
    let _ = child.kill();
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
        // Plain diagnostics: these end up in user-facing error cards, and
        // CI (or a user's shell) may set CARGO_TERM_COLOR=always.
        .env("CARGO_TERM_COLOR", "never")
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;

    if !output.status.success() {
        return Err(Box::new(classify_failure(&String::from_utf8_lossy(
            &output.stderr,
        ))));
    }
    Ok(output.stdout)
}

/// The plugin's stdout: one JSON object, written by `rustviz-plugin`'s
/// `Rustviz` (rustviz-plugin/src/rustviz_library/rv.rs). Keep the two in
/// sync; the corpus tests run the real pair end to end.
#[derive(serde::Deserialize)]
struct PluginOutput {
    code_panel: String,
    timeline_panel: String,
    height: i32,
}

fn parse_output(stdout: &[u8]) -> Result<Rustviz, Box<dyn Error>> {
    let out: PluginOutput = serde_json::from_slice(stdout).map_err(|e| {
        RvError::PluginError(format!(
            "couldn't read the plugin's output ({e}); the installed rustviz plugin \
             probably doesn't match this rustviz-lib. Reinstall it (`rustviz init`, \
             or `cargo install --locked --force --path rustviz-plugin`)"
        ))
    })?;
    Ok(Rustviz {
        code_panel: out.code_panel,
        timeline_panel: out.timeline_panel,
        height: out.height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The panels are JSON string fields, so nothing the user writes (a
    /// `:::` in a comment, say, which the old `<code>:::<timeline>` framing
    /// split on: #181) can be mistaken for structure.
    #[test]
    fn parse_output_reads_plugin_json() {
        let code = "<svg><text>// a ::: b \"x:::y\"</text></svg>";
        let timeline = "<svg height=\"42px\"><g/></svg>";
        let out = format!(
            "{}\n",
            serde_json::json!({ "code_panel": code, "timeline_panel": timeline, "height": 42 })
        );
        let rv = parse_output(out.as_bytes()).expect("parse");
        assert_eq!(rv.code_panel, code);
        assert_eq!(rv.timeline_panel, timeline);
        assert_eq!(rv.height, 42);
    }

    /// Output from a plugin that predates the JSON format must be an error
    /// that says to reinstall, not a half-parsed visualization.
    #[test]
    fn parse_output_rejects_old_plugin_output() {
        let err = parse_output(b"<svg></svg>:::<svg height=\"42px\"></svg>\n")
            .err()
            .expect("must not parse");
        assert!(err.to_string().contains("Reinstall"), "{err}");
    }

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
                 run scripts/bump-nightly.sh or copy the file manually",
                workspace_copy.display()
            );
        }
    }

    // ---- Failure classification. Fixtures are real `cargo rv-plugin`
    // stderr captured from the plugin (see tests/fixtures/stderr/).

    fn fixture(name: &str) -> &'static str {
        match name {
            "borrowck_e0499" => include_str!("../tests/fixtures/stderr/borrowck_e0499.txt"),
            "type_error_e0308" => include_str!("../tests/fixtures/stderr/type_error_e0308.txt"),
            "use_after_move_e0382" => {
                include_str!("../tests/fixtures/stderr/use_after_move_e0382.txt")
            }
            "syntax_error" => include_str!("../tests/fixtures/stderr/syntax_error.txt"),
            "unsupported" => include_str!("../tests/fixtures/stderr/unsupported.txt"),
            "raw_panic" => include_str!("../tests/fixtures/stderr/raw_panic.txt"),
            other => panic!("no fixture {}", other),
        }
    }

    /// Noise that must never reach a student, whatever the kind.
    fn assert_no_noise(msg: &str) {
        for noise in [
            "-Ztrack-diagnostics",
            "RUST_BACKTRACE",
            "could not compile",
            "Caused by",
            "process didn't exit",
            "rv-plugin-driver",
            "src/lib.rs",
            "panicked at",
        ] {
            assert!(!msg.contains(noise), "{:?} leaked into:\n{}", noise, msg);
        }
    }

    fn does_not_compile(name: &str) -> String {
        match classify_failure(fixture(name)) {
            RvError::DoesNotCompile { diagnostics } => diagnostics,
            other => panic!("{}: expected DoesNotCompile, got {:?}", name, other),
        }
    }

    #[test]
    fn borrowck_error_is_does_not_compile() {
        let d = does_not_compile("borrowck_e0499");
        assert_no_noise(&d);
        assert!(d.starts_with("error[E0499]: cannot borrow `s` as mutable more than once"));
        assert!(d.contains(" --> your code:5:14\n"), "{}", d);
        assert!(d.contains("second mutable borrow occurs here"));
        assert!(d.contains("For more information about this error, try `rustc --explain E0499`."));
        // The `  |` gutter that preceded the dropped note goes too.
        assert!(d.contains("first borrow later used here\n\nFor more"), "{}", d);
    }

    #[test]
    fn type_error_is_does_not_compile() {
        let d = does_not_compile("type_error_e0308");
        assert_no_noise(&d);
        assert!(d.starts_with("error[E0308]: mismatched types\n --> your code:2:18"), "{}", d);
        assert!(d.contains("expected `i32`, found `&str`"));
    }

    #[test]
    fn use_after_move_keeps_other_notes_and_help() {
        let d = does_not_compile("use_after_move_e0382");
        assert_no_noise(&d);
        assert!(d.contains("  |\n  = note: this error originates in the macro"), "{}", d);
        assert!(d.contains("help: consider cloning the value"));
        assert!(d.contains("let s2 = s1.clone();"));
    }

    #[test]
    fn syntax_error_without_code_is_does_not_compile() {
        let d = does_not_compile("syntax_error");
        assert_no_noise(&d);
        assert!(d.starts_with("error: expected expression, found `;`\n --> your code:1:21"), "{}", d);
    }

    #[test]
    fn does_not_compile_display_leads_with_explanation() {
        let e = classify_failure(fixture("borrowck_e0499"));
        assert_eq!(e.kind(), ErrorKind::DoesNotCompile);
        let shown = e.to_string();
        assert!(shown.starts_with(DOES_NOT_COMPILE_INTRO));
        assert!(shown.starts_with("This program doesn't compile, and RustViz only visualizes programs that rustc accepts, including the borrow checker."));
        assert!(shown.contains(":\n\nerror[E0499]"));
        let boxed: Box<dyn Error> = Box::new(e);
        assert_eq!(error_kind(boxed.as_ref()), ErrorKind::DoesNotCompile);
    }

    #[test]
    fn plugin_unsupported_line_is_unsupported() {
        let e = classify_failure(fixture("unsupported"));
        assert_eq!(e.kind(), ErrorKind::Unsupported);
        let shown = e.to_string();
        assert_no_noise(&shown);
        assert_eq!(
            shown,
            "RustViz can't visualize this program yet (internal error at expr_visitor.rs:862: \
             called `Option::unwrap()` on a `None` value). Please report it at \
             https://github.com/rustviz/rustviz/issues with your snippet."
        );
    }

    /// A panic the plugin's handler didn't catch (older plugin, or a panic
    /// outside the visitor) still reads as "unsupported", not as a compile
    /// error, even though cargo says "could not compile".
    #[test]
    fn raw_panic_is_unsupported() {
        let e = classify_failure(fixture("raw_panic"));
        assert_eq!(e.kind(), ErrorKind::Unsupported);
        let shown = e.to_string();
        assert_no_noise(&shown);
        assert!(shown.contains("(internal error at expr_visitor.rs:862: called `Option::unwrap()` on a `None` value)"), "{}", shown);
        assert!(shown.contains("https://github.com/rustviz/rustviz/issues"));
    }

    #[test]
    fn unrecognized_failure_is_other_and_cleaned() {
        let e = classify_failure(
            "    Blocking waiting for file lock on build directory\n\
             error: could not compile `user-code` (lib)\n\n\
             Caused by:\n  process didn't exit successfully: `/x/rv-plugin-driver rustc …` (signal: 9, SIGKILL)\n\
             something else went wrong\n",
        );
        assert_eq!(e.kind(), ErrorKind::Other);
        assert_eq!(e.to_string(), "something else went wrong");
        assert_eq!(classify_failure("").kind(), ErrorKind::Other);
        assert!(!classify_failure("").to_string().is_empty());
    }

    #[test]
    fn cargo_progress_and_ansi_are_stripped() {
        let raw = "   Compiling user-code v0.1.0 (/tmp/work/user-code)\n\
                   \u{1b}[0m\u{1b}[1m\u{1b}[38;5;9merror[E0425]\u{1b}[0m: cannot find value `y` in this scope\n\
                   \x20--> src/lib.rs:1:20\n";
        match classify_failure(raw) {
            RvError::DoesNotCompile { diagnostics } => assert_eq!(
                diagnostics,
                "error[E0425]: cannot find value `y` in this scope\n --> your code:1:20"
            ),
            other => panic!("expected DoesNotCompile, got {:?}", other),
        }
    }

    #[test]
    fn crate_paths_rewritten() {
        for (input, want) in [
            (" --> src/lib.rs:5:14", " --> your code:5:14"),
            (" --> /tmp/work/user-code/src/lib.rs:5:14", " --> your code:5:14"),
            (" --> /tmp/.tmpAbC123/user-code/src/lib.rs:12:1", " --> your code:12:1"),
            (" --> ./src/lib.rs:3:9", " --> your code:3:9"),
            // Windows: rustc reports backslash paths.
            (" --> src\\lib.rs:2:18", " --> your code:2:18"),
            (" --> C:\\Users\\r\\AppData\\Local\\Temp\\.tmpAb1\\user-code\\src\\lib.rs:5:14", " --> your code:5:14"),
            (" --> C:\\rustc\\abc\\library\\core\\src\\lib.rs:1:1", " --> C:\\rustc\\abc\\library\\core\\src\\lib.rs:1:1"),
            ("::: src/lib.rs:2:4 and `src/lib.rs`", "::: your code:2:4 and `your code`"),
            ("(src/lib.rs:1:1)", "(your code:1:1)"),
            // Not the user's crate: left alone.
            (" --> /rustc/abc/library/core/src/lib.rs:1:1", " --> /rustc/abc/library/core/src/lib.rs:1:1"),
            ("see mysrc/lib.rs", "see mysrc/lib.rs"),
            ("src/lib.rsx", "src/lib.rsx"),
            ("no path here", "no path here"),
        ] {
            assert_eq!(rewrite_crate_paths(input), want, "input {:?}", input);
        }
    }

    #[test]
    fn error_kind_names_are_stable() {
        // The playground sends these to the frontend; don't rename.
        assert_eq!(ErrorKind::DoesNotCompile.as_str(), "does-not-compile");
        assert_eq!(ErrorKind::Unsupported.as_str(), "unsupported");
        assert_eq!(RvError::Timeout.kind(), ErrorKind::Timeout);
        let io: Box<dyn Error> = Box::new(std::io::Error::other("x"));
        assert_eq!(error_kind(io.as_ref()), ErrorKind::Other);
    }
}
