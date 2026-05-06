use std::collections::{HashMap, BTreeMap};
use simplelog::*;
use std::fs::OpenOptions;
use log::error;
use rustc_hir::intravisit::Visitor;
use rustc_hir::{ItemKind, FnRetTy};
use rustc_middle::ty::TyCtxt;
use rustc_utils::mir::borrowck_facts;
use crate::expr_visitor::{ExprVisitor, RapData};
use crate::RVPluginArgs;
use crate::utils::{RV1Helper, annotate_enum_variant, annotate_toplevel_fn, annotate_struct_field};
use crate::svg_generator::data::{ExternalEvent, ResourceAccessPoint};

/// Drain a per-fn visitor's `raps` into a global rap_map.
///
/// Variable RAPs (Owner / Struct / MutRef / StaticRef) get a
/// fully-qualified key `<fn_start_line>::<name>` so two fns with
/// the same local name don't clobber each other. Function RAPs
/// keep their bare name so `annotate_toplevel_fn`'s lookup works
/// and recursive / cross-fn calls share a single Function entry.
fn merge_visitor_raps(
  global: &mut HashMap<String, RapData>,
  per_fn: HashMap<String, RapData>,
  fn_start_line: usize,
) {
  for (name, data) in per_fn {
    match data.rap {
      ResourceAccessPoint::Function(_) => {
        global.entry(name).or_insert(data);
      }
      _ => {
        let fq_name = format!("{}::{}", fn_start_line, name);
        global.insert(fq_name, data);
      }
    }
  }
}

// "The main function"
pub fn rv_visitor(tcx: TyCtxt, args: &RVPluginArgs) {

  // configure the logger
  let log_file = match OpenOptions::new()
    .create(true)
    .write(true)
    .truncate(true)
    .open("output.log")
  {
    Ok(file) => file,
    Err(e) => {
        eprintln!("Error opening log file: {:?}", e);
        return;
    }
  };
  CombinedLogger::init(
    vec![
        WriteLogger::new(
            LevelFilter::Info,
            Config::default(),
            log_file,
        )
    ]
  ).expect("Failed to initialize logger");

  // Data structures to be passed to visitor
  let mut testing_helper: RV1Helper = RV1Helper::new();
  let mut line_map: BTreeMap<usize, String> = BTreeMap::new();
  let mut line_map2: BTreeMap<usize, Vec<ExternalEvent>> = BTreeMap::new();
  match testing_helper.initialize_line_map() {
    Ok(l) => {
      line_map = l;
    }
    Err(e) => {
      error!("{}", e);
    }
  }
  let a_map: BTreeMap<usize, String> = line_map.clone();
  let mut a_line_map: BTreeMap<usize, Vec<String>> = BTreeMap::new();
  let mut owner_to_hash: HashMap<String, usize> = HashMap::new();
  let mut pre_events: Vec<(usize, ExternalEvent)> = Vec::new();
  let mut rap_map: HashMap<String, RapData> = HashMap::new();
  for k in a_map.keys() {
    let s = a_map[k].clone();
    a_line_map.insert(*k, vec![s]);
    line_map2.insert(*k, vec![]);
  }
  let mut rap_hash_num: usize = 1;
  let mut ids: usize = 0;

  
  
  // loop through top-level hir items
  for id in tcx.hir_free_items() {
    match &tcx.hir_item(id).kind {
      ItemKind::Struct(_ident, _generics, vardata) => {
        match vardata {
          rustc_hir::VariantData::Struct{fields, ..} => {
            // annotate all the struct fields
            if fields.len() > 0 {
              for field in fields.iter(){
                let line = tcx.sess.source_map().lookup_char_pos(field.span.lo()).line;
                let line_str = &a_map[&line];
                annotate_struct_field(line_str, & mut owner_to_hash, & mut a_line_map , & mut rap_hash_num, &field, &tcx);
              }
            }
          }
          _ => {}
        }
      }
      ItemKind::Impl(imp) => {
        for item_ref in imp.items.iter() {
          let impl_item = tcx.hir_impl_item(*item_ref);
          if let rustc_hir::ImplItemKind::Fn(fn_sig, body_id) = &impl_item.kind {
            let local_def_id = impl_item.owner_id.def_id;
            let hir_body = tcx.hir_body(*body_id);
            let bwf = borrowck_facts::get_body_with_borrowck_facts(tcx, local_def_id);
            let body = &bwf.body;
            let output = matches!(fn_sig.decl.output, FnRetTy::Return(_));
            let fn_start_line = tcx.sess.source_map().lookup_char_pos(fn_sig.span.lo()).line;

            let mut visitor = ExprVisitor {
              tcx,
              mir_body: body,
              hir_body,
              bwf,
              current_scope: 0,
              current_fn_start: fn_start_line,
              borrow_map: HashMap::new(),
              raps: HashMap::new(),
              analysis_result: HashMap::new(),
              event_line_map: &mut line_map2,
              preprocessed_events: &mut pre_events,
              rap_hashes: rap_hash_num,
              source_map: &a_map,
              annotated_lines: &mut a_line_map,
              id_map: &mut owner_to_hash,
              unique_id: &mut ids,
              inside_branch: false,
              fn_ret: output,
            };
            visitor.visit_body(hir_body);
            visitor.print_lifetimes();
            visitor.print_out_of_scope();
            rap_hash_num = visitor.rap_hashes;
            merge_visitor_raps(&mut rap_map, visitor.raps, fn_start_line);
          }
        }
      }
      ItemKind::Fn { sig: fn_sig, body: body_id, ident: _, .. } => {
        // Each fn body has its own raps map so two fns with the
        // same variable name (`let x = …` in both `main` and a
        // helper) don't clobber each other; the shared rap_hashes
        // counter still gives every RAP a globally unique hash, so
        // the renderer (keyed on hash) sees them as distinct
        // timelines. After each visit, merge_visitor_raps drains
        // the visitor's map into the global one with FQ keys for
        // variable RAPs and bare names for Function RAPs (so e.g.
        // `compare_strings` stays single-instance for source
        // colorization in annotate_toplevel_fn).
        let hir_body = tcx.hir_body(*body_id);
        let def_id = tcx.hir_body_owner_def_id(*body_id);
        let bwf = borrowck_facts::get_body_with_borrowck_facts(tcx, def_id);
        let body = &bwf.body;
        let output = match fn_sig.decl.output {
          FnRetTy::Return(_ty) => {
            true
          }
          _ => false
        };
        let fn_start_line = tcx.sess.source_map().lookup_char_pos(fn_sig.span.lo()).line;

        let mut visitor = ExprVisitor {
          tcx,
          mir_body: body,
          hir_body: hir_body,
          bwf: bwf,
          current_scope: 0,
          current_fn_start: fn_start_line,
          borrow_map: HashMap::new(),
          raps: HashMap::new(),
          analysis_result: HashMap::new(),
          event_line_map: & mut line_map2,
          preprocessed_events: & mut pre_events,
          rap_hashes: rap_hash_num,
          source_map: & a_map,
          annotated_lines: & mut a_line_map,
          id_map: & mut owner_to_hash,
          unique_id: & mut ids,
          inside_branch: false,
          fn_ret: output
        };
        visitor.visit_body(hir_body);
        visitor.print_lifetimes();
        visitor.print_out_of_scope();
        rap_hash_num = visitor.rap_hashes;
        merge_visitor_raps(&mut rap_map, visitor.raps, fn_start_line);
      },
      _ => {}
    }
  }

  // annotate items after visiting all function bodies since we can assign hashes to top-level structures in fn bodies
  for id in tcx.hir_free_items() {
    match &tcx.hir_item(id).kind {
      ItemKind::Fn { sig: fn_sig, ident: fn_ident, .. } => {
        let line = tcx.sess.source_map().lookup_char_pos(fn_sig.span.lo()).line;
        let line_str = &a_map[&line];
        if fn_ident.as_str() != "main" {
          annotate_toplevel_fn(*fn_ident, line_str, &rap_map, &mut a_line_map, &mut rap_hash_num, &tcx);
        }
      }
      ItemKind::Enum(_ident, _generics, e_def) => {
        for variant in e_def.variants.iter() {
          let ctor_name = tcx.hir_name(variant.data.ctor_hir_id().unwrap()).as_str().to_owned();
          let parent_name = tcx.hir_name(tcx.parent_hir_id(variant.hir_id)).as_str().to_owned();
          annotate_enum_variant(ctor_name.as_str(), parent_name.as_str(), &variant, &rap_map, & mut a_line_map, &tcx);
        }
      }
      ItemKind::Impl(imp) => {
        for item_ref in imp.items.iter() {
          let impl_item = tcx.hir_impl_item(*item_ref);
          if matches!(impl_item.kind, rustc_hir::ImplItemKind::Fn(_, _)) {
            let fn_ident = impl_item.ident;
            let line = tcx.sess.source_map().lookup_char_pos(fn_ident.span.lo()).line;
            let line_str = &a_map[&line];
            annotate_toplevel_fn(fn_ident, line_str, &rap_map, &mut a_line_map, &mut rap_hash_num, &tcx);
          }
        }
      }

      _ => {}
    }
  }

  pre_events.sort_by_key(|k| k.0);

  // Build the rap.hash() → fn_start_line map for the renderer.
  // Functions are excluded — they don't get columns and span the
  // whole file, so giving them a single fn_start would mis-locate
  // their (non-existent) labels.
  let fn_start_lines: HashMap<u64, usize> = rap_map
    .values()
    .filter(|d| !matches!(d.rap, ResourceAccessPoint::Function(_)))
    .map(|d| (*d.rap.hash(), d.fn_start_line))
    .collect();

  // Pass information to svg-generator
  match testing_helper.generate_vis(line_map2, pre_events, & mut a_line_map, rap_map.len() + 1, fn_start_lines, args.write_to_cwd) {
    Ok(_) => {}
    Err(e) => {
      eprintln!("{}", e);
    }
  }

}