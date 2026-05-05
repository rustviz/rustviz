// This file is based on the rust plugin example.
#![feature(
  rustc_private,
  box_patterns,
  associated_type_defaults,
  min_specialization,
  type_alias_impl_trait,
  trait_alias,
  unboxed_closures,
  exact_size_is_empty,
  btree_cursors,
  type_changing_struct_update,
)]
#![deny(
  clippy::all,
  clippy::bool_to_int_with_if,
  clippy::case_sensitive_file_extension_comparisons,
  clippy::cloned_instead_of_copied,
  clippy::default_trait_access,
  clippy::empty_enum,
  clippy::enum_glob_use,
  clippy::expl_impl_clone_on_copy,
  clippy::explicit_deref_methods,
  clippy::filter_map_next,
  clippy::flat_map_option,
  clippy::float_cmp,
  clippy::fn_params_excessive_bools,
  clippy::from_iter_instead_of_collect,
  clippy::if_not_else,
  clippy::implicit_clone,
  clippy::inconsistent_struct_constructor,
  clippy::large_stack_arrays,
  clippy::large_types_passed_by_value,
  clippy::macro_use_imports,
  clippy::manual_assert,
  clippy::manual_let_else,
  clippy::manual_ok_or,
  clippy::manual_string_new,
  clippy::many_single_char_names,
  clippy::map_unwrap_or,
  clippy::match_bool,
  clippy::match_on_vec_items,
  clippy::match_same_arms,
  clippy::mut_mut,
  clippy::needless_for_each,
  clippy::option_option,
  clippy::similar_names
)]

// Need to include rustc crates like this
// Haven't figured out how to ignore the red lines in rust-analyzer
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
      plugin::rv_visitor(tcx, &self.args);
      // allow compilation to continue.
      rustc_driver::Compilation::Continue
    }
}


 