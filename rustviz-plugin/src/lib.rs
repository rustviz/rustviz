// This file is based on the rust plugin example.
#![feature(
  rustc_private,
  min_specialization,
  unboxed_closures,
)]
// Clippy runs in CI with only these two lints (see the "clippy" step in
// .github/workflows/build.yml). std HashMap/HashSet iterate in a per-process
// random order, and letting that order reach the output made diagrams differ
// between runs (#211); clippy.toml lists the banned methods. Where the order
// provably can't matter, opt out on the smallest statement with
// `#[expect(clippy::..., reason = "...")]`.
#![deny(clippy::disallowed_methods, clippy::iter_over_hash_type)]

// Need to include rustc crates like this
// Haven't figured out how to ignore the red lines in rust-analyzer
extern crate rustc_ast;
extern crate rustc_driver;
extern crate rustc_hir;
extern crate rustc_interface;
extern crate rustc_middle;
extern crate rustc_span;
extern crate rustc_borrowck;
extern crate polonius_engine;
extern crate rustc_mir_dataflow;


use std::{borrow::Cow, env};
use clap::Parser;
use rustc_plugin::{CrateFilter, RustcPlugin, RustcPluginArgs, Utf8Path};
use serde::{Deserialize, Serialize};
use rustc_utils::mir::borrowck_facts;
pub mod visitor;
pub mod plugin;
pub mod utils;
pub mod expr_visitor;
pub mod expr_visitor_utils;
pub mod annotated_source_gen;
pub mod mir_data;
pub mod ref_flow;
pub mod rustviz_library;
pub mod svg_generator;


pub struct RVPlugin;

#[derive(Parser, Serialize, Deserialize, Debug)]
pub struct RVPluginArgs {
  #[arg(short, long)]
  write_to_cwd: bool, // write to cwd (from where the plugin is called)
}


// https://github.com/cognitive-engineering-lab/rustc_plugin
impl RustcPlugin for RVPlugin {
  type Args = RVPluginArgs;

  fn version(&self) -> Cow<'static, str> { 
    env!("CARGO_PKG_VERSION").into()
  }

  fn driver_name(&self) -> Cow<'static, str> {//which driver to use
    "rv-plugin-driver".into()
  }

  fn args(&self, _target_dir: &Utf8Path) -> RustcPluginArgs<Self::Args> {
    let args = RVPluginArgs::parse_from(env::args().skip(1));//read command line arguments
    let filter = CrateFilter::AllCrates; //decide which crate to compile
    RustcPluginArgs { args, filter }
  }


  //'run' is called when the plugin is executed by the Rust compiler
  fn run(
    self,
    compiler_args: Vec<String>, //vector of command line arguments
    plugin_args: Self::Args, //any plugin-specific arguments
  ) -> rustc_interface::interface::Result<()> {
    let mut compiler_args=compiler_args;
    compiler_args.extend(
      "-Z identify-regions -Z mir-opt-level=0 -Z track-diagnostics=yes -Z maximal-hir-to-mir-coverage --allow warnings"
      .split(' ')
      .map(|s| s.to_owned()),
    );
    let mut callbacks = RVCallbacks { args: plugin_args };
    rustc_driver::run_compiler(&compiler_args, &mut callbacks);
    Ok(())
  }
}

struct RVCallbacks {
  args: RVPluginArgs,
}

impl rustc_driver::Callbacks for RVCallbacks {

    fn config(&mut self, config: &mut rustc_interface::Config) {
      // You MUST configure rustc to ensure `get_body_with_borrowck_facts` will work.
      config.override_queries = Some(borrowck_facts::override_queries);
    }

    fn after_analysis<'tcx>(
      &mut self,
      _compiler: &rustc_interface::interface::Compiler,
      tcx: rustc_middle::ty::TyCtxt<'tcx>,
    ) -> rustc_driver::Compilation {
      match catch_plugin_panic(|| plugin::rv_visitor(tcx, &self.args)) {
        // allow compilation to continue.
        Ok(()) => rustc_driver::Compilation::Continue,
        Err(msg) => {
          // Report through rustc's own diagnostics rather than letting
          // the panic escape: cargo then prints a normal "could not
          // compile" summary instead of the panic, a backtrace hint and
          // the full driver command line.
          tcx.dcx().err(msg);
          rustc_driver::Compilation::Stop
        }
      }
    }
}

/// Stable prefix of the one-line message the plugin prints when it hits a
/// construct it can't handle. `rustviz-lib` matches on it to tell "RustViz
/// can't visualize this yet" apart from "this program doesn't compile";
/// keep the two in sync (`rustviz_lib::UNSUPPORTED_PREFIX`).
pub const UNSUPPORTED_PREFIX: &str = "rustviz: unsupported: ";

const ISSUES_URL: &str = "https://github.com/rustviz/rustviz/issues";

/// Panic message and location captured by the hook in `catch_plugin_panic`.
static LAST_PANIC: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Developers get the stock panic output (and a backtrace, with
/// `RUST_BACKTRACE=1`) by setting `RUSTVIZ_DEBUG=1` or `RUST_BACKTRACE`.
fn want_full_panic_output() -> bool {
  let set = |k: &str| env::var(k).is_ok_and(|v| !v.is_empty() && v != "0");
  set("RUSTVIZ_DEBUG") || set("RUST_BACKTRACE")
}

/// A short reason a student can read for a plugin panic. Our own
/// `panic!("internal state: ...")` guards say what went wrong; the std
/// messages from an `unwrap()` / `expect()` / index on a missing entry
/// ("called `Option::unwrap()` on a `None` value") say nothing, so those
/// are replaced by a generic reason (the file:line is kept for maintainers).
fn friendly_reason(msg: &str) -> String {
  if let Some(rest) = msg.strip_prefix("internal state: ") {
    return rest.to_string();
  }
  if let Some(rest) = msg.strip_prefix("unsupported: ") {
    return rest.to_string();
  }
  if msg.starts_with("called `Option::unwrap()`") || msg.starts_with("called `Result::unwrap()`") {
    return "an internal lookup found nothing where RustViz expected something".to_string();
  }
  msg.to_string()
}

/// Run `f`, turning a panic into the one-line `rustviz: unsupported: ...`
/// message. The panic hook is swapped only for the duration of `f`, so
/// rustc's own handling of panics elsewhere is unchanged.
fn catch_plugin_panic(f: impl FnOnce()) -> Result<(), String> {
  use std::{panic, sync::Arc};

  let verbose = want_full_panic_output();
  let prev: Arc<dyn Fn(&panic::PanicHookInfo<'_>) + Send + Sync> = Arc::from(panic::take_hook());
  let prev_in_hook = Arc::clone(&prev);
  panic::set_hook(Box::new(move |info| {
    let msg = info
      .payload()
      .downcast_ref::<&str>()
      .map(|s| (*s).to_string())
      .or_else(|| info.payload().downcast_ref::<String>().cloned())
      .unwrap_or_else(|| "unknown panic".to_string());
    let loc = info.location().map(|l| {
      let file = std::path::Path::new(l.file())
        .file_name()
        .map_or_else(|| l.file().to_string(), |f| f.to_string_lossy().into_owned());
      format!("{}:{}", file, l.line())
    });
    // (Without the marks of a same-named variable's column, #598.)
    let msg = crate::expr_visitor_utils::display_text(&friendly_reason(&msg));
    let mut slot = LAST_PANIC.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if slot.is_none() {
      *slot = Some(match loc {
        Some(loc) => format!("{}: {}", loc, msg),
        None => msg,
      });
    }
    drop(slot);
    if verbose {
      prev_in_hook(info);
    }
  }));

  let result = panic::catch_unwind(panic::AssertUnwindSafe(f));
  panic::set_hook(Box::new(move |info| prev(info)));

  result.map_err(|_| {
    let detail = LAST_PANIC
      .lock()
      .unwrap_or_else(std::sync::PoisonError::into_inner)
      .take()
      .unwrap_or_else(|| "unknown panic".to_string());
    // Keep it on one line: rustviz-lib looks for the prefix line-wise.
    let detail: String = detail.split_whitespace().collect::<Vec<_>>().join(" ");
    let detail = match detail.char_indices().nth(300) {
      Some((i, _)) => format!("{}...", &detail[..i]),
      None => detail,
    };
    format!(
      "{}RustViz can't visualize this program yet (internal error at {}). \
       Please report it at {} with your snippet.",
      UNSUPPORTED_PREFIX, detail, ISSUES_URL
    )
  })
}


 