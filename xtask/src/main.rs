//! Cross-platform driver for `cargo xtask setup` / `cargo xtask uninstall`.
//!
//! Replaces the legacy `setup.sh` / `uninstall.sh` so the workspace boots
//! the same way on Linux, macOS, and Windows. Wired up through the alias
//! in the workspace `.cargo/config.toml`:
//!
//!   cargo xtask setup            # install plugin + CLI, build frontend, pull runner
//!   cargo xtask setup --build-runner
//!   cargo xtask uninstall        # reverse — cargo uninstall + cleanup
//!   cargo xtask uninstall --everything
//!   cargo xtask <subcommand> --help
//!
//! Cross-platform notes:
//!   * Subprocesses use `std::process::Command`, which on Windows resolves
//!     `.exe` extensions automatically. The exception is `npm` / `npx`,
//!     which ship as `.cmd` shims and aren't picked up by Command's
//!     PATHEXT logic — those go through `cmd /C` on Windows. See
//!     `build_command`.
//!   * Filesystem operations use `std::fs`, which is portable.
//!   * No bash, no `rm -rf`, no `awk`. Everything works in a vanilla
//!     PowerShell or cmd.exe session as long as cargo + rustup are on
//!     PATH.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(version, about = "Build / install / uninstall driver for the RustViz workspace.")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Build + install everything: rustviz-plugin, rustviz-cli, frontend
    /// dist, and (optionally) the rustviz-runner docker image.
    Setup(SetupArgs),
    /// Reverse of `setup`. Removes the cargo-installed binaries, the
    /// local docker runner image, and frontend build artifacts. Spares
    /// the rustup toolchain and `target/` by default — pass --toolchain
    /// / --target / --everything to also remove those.
    Uninstall(UninstallArgs),
}

#[derive(Parser, Debug)]
struct SetupArgs {
    /// Force-build the docker runner image locally instead of pulling
    /// from GHCR. Useful when iterating on plugin changes that need to
    /// land in the sandboxed runner before the playground picks them up.
    #[arg(long)]
    build_runner: bool,
}

#[derive(Parser, Debug)]
struct UninstallArgs {
    /// Also `rustup toolchain uninstall` the nightly pinned by
    /// rust-toolchain.toml. Off by default because that nightly may be
    /// in use by other projects on the same machine.
    #[arg(long)]
    toolchain: bool,
    /// Also remove the cargo `target/` tree. Off by default because
    /// it's slow to regenerate (~minutes for a fresh workspace build).
    #[arg(long)]
    target: bool,
    /// Shorthand for --toolchain --target.
    #[arg(long)]
    everything: bool,
    /// Print what would run, don't execute.
    #[arg(short = 'n', long)]
    dry_run: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let root = workspace_root()?;
    std::env::set_current_dir(&root)
        .with_context(|| format!("chdir into workspace root {}", root.display()))?;
    match cli.cmd {
        Cmd::Setup(a) => setup(a),
        Cmd::Uninstall(a) => uninstall(a),
    }
}

fn workspace_root() -> Result<std::path::PathBuf> {
    // CARGO_MANIFEST_DIR points at xtask/'s dir when this binary runs
    // through the cargo alias; the workspace root is its parent.
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .context("CARGO_MANIFEST_DIR must be set (run via `cargo xtask …`)")?;
    Ok(Path::new(&manifest_dir)
        .parent()
        .ok_or_else(|| anyhow::anyhow!("xtask/'s parent dir not found"))?
        .to_path_buf())
}

// ─── setup ───────────────────────────────────────────────────────────

fn setup(args: SetupArgs) -> Result<()> {
    // 1. Toolchain. rust-toolchain.toml triggers rustup auto-install of
    //    the pinned nightly with the components the plugin needs;
    //    `rustup show active-toolchain` makes that step explicit and
    //    fails fast if rustup isn't on PATH.
    section("rustup show active-toolchain");
    run("rustup", &["show", "active-toolchain"])?;

    // 2. The rustc plugin (`cargo rv-plugin` / `rv-plugin-driver`).
    section("cargo install --path rustviz-plugin --locked");
    run("cargo", &["install", "--path", "rustviz-plugin", "--locked"])?;

    // 3. The CLI (`rustviz svg|html|init`).
    section("cargo install --path rustviz-cli --locked");
    run("cargo", &["install", "--path", "rustviz-cli", "--locked"])?;

    // 4. Frontend bundle. The Vite build copies frontend/public/ into
    //    dist/ so ex-assets/{helpers.js,visualization.css} ride along
    //    with the SPA.
    section("playground/frontend: npm install + build");
    run_in("playground/frontend", "npm", &["install"])?;
    run_in("playground/frontend", "npm", &["run", "build"])?;

    // 5. Rest of the workspace. `--exclude xtask` skips the binary
    //    that's currently running this code: on Windows the build
    //    would otherwise fail at link time trying to overwrite the
    //    in-use xtask.exe (Windows holds an exclusive lock on
    //    running executables, unlike Linux / macOS where the file
    //    can be replaced under a live process).
    section("cargo build --workspace --release (excluding xtask)");
    run(
        "cargo",
        &["build", "--workspace", "--release", "--exclude", "xtask"],
    )?;

    // 6. Sandboxed runner image. Skipped silently when docker isn't on
    //    PATH so devs who only iterate against RV_RUNNER=local don't
    //    need it installed.
    section("docker runner image");
    docker_runner(args.build_runner)?;

    println!();
    println!("Setup complete. To run the playground:");
    println!("  RV_RUNNER=local (cd playground && cargo run --release)   # local dev");
    println!("  cd playground && cargo run --release                      # docker (default)");
    println!("  open http://127.0.0.1:8080/");
    println!();
    println!("To iterate on the frontend (hot reload, proxies API to :8080):");
    println!("  cd playground/frontend && npm run dev");
    println!();
    println!("To render a single Rust file through the plugin:");
    println!("  rustviz svg path/to/foo.rs    # writes foo.code.svg + foo.timeline.svg");
    println!("  rustviz html path/to/foo.rs   # writes one self-contained HTML page");
    Ok(())
}

fn docker_runner(force_build: bool) -> Result<()> {
    if !command_exists("docker") || !docker_running() {
        println!();
        println!("Skipping runner image setup: docker is not available.");
        println!();
        println!("For local dev, set RV_RUNNER=local to run the plugin in-process");
        println!("(NEVER do this on a public deployment — see playground/SECURITY.md).");
        return Ok(());
    }

    if force_build {
        println!("Building rustviz/rustviz-runner image locally...");
        run(
            "docker",
            &[
                "build",
                "-t",
                "rustviz/rustviz-runner:latest",
                "-f",
                "playground/runner/Dockerfile",
                ".",
            ],
        )?;
        return Ok(());
    }

    if image_exists("rustviz/rustviz-runner:latest") {
        println!("Runner image already present locally; pass --build-runner to rebuild.");
        return Ok(());
    }

    println!("Pulling rustviz/rustviz-runner image from GHCR...");
    let pulled = silent("docker", &["pull", "ghcr.io/rustviz/rustviz-runner:latest"])
        .and_then(|_| {
            silent(
                "docker",
                &[
                    "tag",
                    "ghcr.io/rustviz/rustviz-runner:latest",
                    "rustviz/rustviz-runner:latest",
                ],
            )
        });
    if pulled.is_ok() {
        println!("Runner image pulled.");
    } else {
        eprintln!(
            "Pull failed (registry unreachable, image not yet published, or no internet)."
        );
        eprintln!("Building locally as fallback (~5 min)...");
        run(
            "docker",
            &[
                "build",
                "-t",
                "rustviz/rustviz-runner:latest",
                "-f",
                "playground/runner/Dockerfile",
                ".",
            ],
        )?;
    }
    Ok(())
}

// ─── uninstall ───────────────────────────────────────────────────────

fn uninstall(args: UninstallArgs) -> Result<()> {
    let dry = args.dry_run;
    let remove_toolchain = args.toolchain || args.everything;
    let remove_target = args.target || args.everything;

    // 1. cargo-installed binaries. `cargo uninstall <pkg>` removes
    //    the binaries the package produced from ~/.cargo/bin AND the
    //    registry metadata under ~/.cargo. Tolerate "not installed"
    //    silently so the script is idempotent.
    section("cargo uninstall");
    for pkg in ["rustviz-cli", "rustviz-plugin"] {
        if cargo_installed(pkg) {
            run_or_dry(dry, "cargo", &["uninstall", pkg])?;
        } else {
            println!("  - {} not installed; skipping", pkg);
        }
    }

    // 2. Docker runner image. Removing the local copy doesn't touch
    //    anything in GHCR; subsequent `setup` runs will re-pull or
    //    re-build as needed. Two tags can be left over depending on
    //    whether setup pulled from GHCR or built locally.
    section("docker runner image");
    if command_exists("docker") && docker_running() {
        let mut removed_any = false;
        for tag in [
            "rustviz/rustviz-runner:latest",
            "ghcr.io/rustviz/rustviz-runner:latest",
        ] {
            if image_exists(tag) {
                run_or_dry(dry, "docker", &["rmi", tag])?;
                removed_any = true;
            }
        }
        if !removed_any {
            println!("  - rustviz/rustviz-runner not present; skipping");
        }
    } else {
        println!("  - docker not available; skipping");
    }

    // 3. Frontend artifacts. dist/ + node_modules/ + *.tsbuildinfo are
    //    all regenerable by `npm install && npm run build`.
    section("playground frontend artifacts");
    rm_rf_if_exists(dry, "playground/frontend/dist")?;
    rm_rf_if_exists(dry, "playground/frontend/node_modules")?;
    if let Ok(entries) = std::fs::read_dir("playground/frontend") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name.to_string_lossy().ends_with(".tsbuildinfo") {
                rm_file_if_exists(dry, entry.path())?;
            }
        }
    }

    // 4. Optional: rustup toolchain. Channel string read from the same
    //    rust-toolchain.toml that triggers auto-install on `setup`, so
    //    the two stay in sync automatically.
    if remove_toolchain {
        let channel = read_channel("rust-toolchain.toml")?;
        section(&format!("rustup toolchain ({})", channel));
        let installed = capture("rustup", &["toolchain", "list"]).unwrap_or_default();
        if installed
            .lines()
            .any(|l| l.trim_start().starts_with(&channel))
        {
            run_or_dry(dry, "rustup", &["toolchain", "uninstall", &channel])?;
        } else {
            println!("  - {} not installed; skipping", channel);
        }
    }

    // 5. Optional: cargo target/. Slow to regenerate, only matters for
    //    from-scratch testing.
    if remove_target {
        section("cargo target/");
        rm_rf_if_exists(dry, "target")?;
    }

    section("done.");
    if dry {
        println!("(dry run — nothing was actually removed)");
    }
    Ok(())
}

// ─── helpers ─────────────────────────────────────────────────────────

fn section(label: &str) {
    println!();
    println!("==> {}", label);
}

/// Build a `Command` invocation, handling the Windows gotcha that
/// `npm` / `npx` ship as `.cmd` shims which Rust's `Command` doesn't
/// resolve through the same PATHEXT logic cmd.exe does. Wrapping those
/// specifically in `cmd /C` makes them behave the same way they would
/// when typed at a prompt. Everything else (cargo, rustup, docker)
/// resolves with the implicit `.exe` suffix on Windows.
fn build_command(program: &str, args: &[&str]) -> Command {
    if cfg!(windows) && matches!(program, "npm" | "npx") {
        let mut cmd = Command::new("cmd");
        cmd.arg("/C").arg(program).args(args);
        cmd
    } else {
        let mut cmd = Command::new(program);
        cmd.args(args);
        cmd
    }
}

fn run(program: &str, args: &[&str]) -> Result<()> {
    print_cmd(None, program, args);
    let status = build_command(program, args)
        .status()
        .with_context(|| format!("failed to spawn {}", program))?;
    if !status.success() {
        bail!("{} exited with {}", program, status);
    }
    Ok(())
}

fn run_in(dir: &str, program: &str, args: &[&str]) -> Result<()> {
    print_cmd(Some(dir), program, args);
    let status = build_command(program, args)
        .current_dir(dir)
        .status()
        .with_context(|| format!("failed to spawn {} in {}", program, dir))?;
    if !status.success() {
        bail!("{} (in {}) exited with {}", program, dir, status);
    }
    Ok(())
}

fn run_or_dry(dry: bool, program: &str, args: &[&str]) -> Result<()> {
    print_cmd(None, program, args);
    if dry {
        return Ok(());
    }
    let status = build_command(program, args).status()?;
    if !status.success() {
        bail!("{} exited with {}", program, status);
    }
    Ok(())
}

fn silent(program: &str, args: &[&str]) -> Result<()> {
    let status = build_command(program, args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if !status.success() {
        bail!("{} exited with {}", program, status);
    }
    Ok(())
}

fn capture(program: &str, args: &[&str]) -> Result<String> {
    let out = build_command(program, args).output()?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn print_cmd(dir: Option<&str>, program: &str, args: &[&str]) {
    let mut stdout = std::io::stdout().lock();
    let _ = match dir {
        Some(d) => writeln!(stdout, "$ (cd {}; {} {})", d, program, args.join(" ")),
        None => writeln!(stdout, "$ {} {}", program, args.join(" ")),
    };
}

fn command_exists(program: &str) -> bool {
    build_command(program, &["--version"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn docker_running() -> bool {
    Command::new("docker")
        .arg("info")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn image_exists(tag: &str) -> bool {
    Command::new("docker")
        .args(["image", "inspect", tag])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn cargo_installed(pkg: &str) -> bool {
    let out = capture("cargo", &["install", "--list"]).unwrap_or_default();
    out.lines()
        .any(|l| l.trim_start().starts_with(&format!("{} v", pkg)))
}

fn rm_rf_if_exists<P: AsRef<Path>>(dry: bool, path: P) -> Result<()> {
    let p = path.as_ref();
    if !p.exists() {
        return Ok(());
    }
    println!("$ rm -rf {}", p.display());
    if dry {
        return Ok(());
    }
    std::fs::remove_dir_all(p).with_context(|| format!("rm -rf {}", p.display()))
}

fn rm_file_if_exists<P: AsRef<Path>>(dry: bool, path: P) -> Result<()> {
    let p = path.as_ref();
    if !p.exists() {
        return Ok(());
    }
    println!("$ rm {}", p.display());
    if dry {
        return Ok(());
    }
    std::fs::remove_file(p).with_context(|| format!("rm {}", p.display()))
}

fn read_channel(path: &str) -> Result<String> {
    let s = std::fs::read_to_string(path).with_context(|| format!("read {}", path))?;
    for line in s.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("channel") {
            let rest = rest.trim_start();
            if let Some(rest) = rest.strip_prefix('=') {
                let rest = rest.trim();
                if let Some(s) = rest.strip_prefix('"') {
                    if let Some(end) = s.find('"') {
                        return Ok(s[..end].to_string());
                    }
                }
            }
        }
    }
    bail!("could not find channel = \"…\" in {}", path);
}
