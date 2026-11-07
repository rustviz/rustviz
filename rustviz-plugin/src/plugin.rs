use std::collections::{HashMap, BTreeMap, HashSet};
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
use crate::svg_generator::data::{ExternalEvent, ResourceAccessPoint, ShadowNote};

/// Drain a per-fn visitor's `raps` into a global rap_map.
///
/// Variable RAPs (Owner / Struct / MutRef / StaticRef) get a
/// fully-qualified key `<fn_start_line>::<name>` so two fns with
/// the same local name don't clobber each other. Function RAPs
/// keep their per-fn key (`fn_key(name)`) so `annotate_toplevel_fn`'s
/// lookup works and recursive / cross-fn calls share a single
/// Function entry.
fn merge_visitor_raps(
  global: &mut HashMap<String, RapData>,
  per_fn: HashMap<String, RapData>,
  fn_start_line: usize,
) {
  #[expect(clippy::iter_over_hash_type, reason = "per_fn's keys are distinct, so each goes to its own global entry")]
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

/// Record the header notes of the numbered columns (`s₂`) of the fn
/// body `owner`, under the same `<fn_start_line>::<name>` keys that
/// `merge_visitor_raps` gives its RAPs.
fn collect_shadow_notes(
  tcx: &TyCtxt,
  owner: rustc_hir::OwnerId,
  fn_start_line: usize,
  out: &mut HashMap<String, ShadowNote>,
) {
  #[expect(clippy::disallowed_methods, clippy::iter_over_hash_type, reason = "inserting into another map; order is irrelevant")]
  for (name, note) in crate::expr_visitor_utils::binding_names(tcx, owner).notes.iter() {
    out.insert(format!("{}::{}", fn_start_line, name), note.clone());
  }
}

/// True for top-level items the user did not write: anything whose
/// span comes from a macro expansion, e.g. the `impl Debug for T`
/// that `#[derive(Debug)]` generates. A hand-written impl (even of
/// the same trait) has a plain source span and is still visited.
fn is_generated_item(item: &rustc_hir::Item<'_>) -> bool {
  item.span.from_expansion()
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
  // Lines carrying a `// rustviz: skip` or `// rustviz: hide`
  // marker. Both kinds bypass body traversal here and visit_local
  // suppression in the per-fn visitor; the `hide` subset is also
  // consulted below to compute `hidden_source_lines`, which the
  // renderer uses to drop the entire fn from the displayed source.
  let skip_lines: HashSet<usize> = testing_helper.skip_lines.clone();
  let hide_marker_lines: HashSet<usize> = testing_helper.hide_lines.clone();
  // Filled below with every source line that belongs to a `hide`-
  // marked fn (signature through closing brace, plus the immediately
  // preceding blank line if any — keeps the surrounding spacing
  // tidy when fns at the end of the file are removed).
  let mut hidden_source_lines: HashSet<usize> = HashSet::new();
  let mut shadow_notes_by_fq: HashMap<String, ShadowNote> = HashMap::new();
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
  // Let-chain bindings drawn beside their sources (#583, #584).
  let mut binding_columns: Vec<(u64, u64)> = Vec::new();

  
  
  // loop through top-level hir items
  for id in tcx.hir_free_items() {
    let item = tcx.hir_item(id);
    // Compiler-generated items — chiefly the impls that
    // `#[derive(Debug, Clone, PartialEq, ..)]` expands to — have
    // macro-expansion spans that point at the `#[derive]` attribute.
    // Visiting their bodies drew ghost timelines (`self`, `f` from
    // `Debug::fmt`) on the attribute's line for code the student
    // never wrote. Calls to these impls from user code still go
    // through the normal call-site handling.
    if is_generated_item(item) {
      continue;
    }
    match &item.kind {
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

            // Marker on the fn signature line — leave the body
            // un-traced. Call sites in non-skipped fns will still
            // emit events whose endpoint is the Function RAP for
            // this fn (created by `add_fn` when the call is
            // visited), so moves into / out of it still appear.
            if skip_lines.contains(&fn_start_line) {
              if hide_marker_lines.contains(&fn_start_line) {
                let body_end_line = tcx.sess.source_map()
                  .lookup_char_pos(hir_body.value.span.hi()).line;
                for l in fn_start_line..=body_end_line {
                  hidden_source_lines.insert(l);
                }
                if fn_start_line >= 2
                  && a_map.get(&(fn_start_line - 1))
                    .map(|s| s.trim().is_empty()).unwrap_or(false)
                {
                  hidden_source_lines.insert(fn_start_line - 1);
                }
              }
              continue;
            }

            let mut visitor = ExprVisitor {
              tcx,
              mir_body: body,
              hir_body,
              bwf,
              current_scope: 0,
              current_fn_start: fn_start_line,
              borrow_map: HashMap::new(),
              raps: HashMap::new(),
              event_line_map: &mut line_map2,
              preprocessed_events: &mut pre_events,
              rap_hashes: rap_hash_num,
              source_map: &a_map,
              annotated_lines: &mut a_line_map,
              id_map: &mut owner_to_hash,
              unique_id: &mut ids,
              inside_branch: false,
              fn_ret: output,
              skip_lines: &skip_lines,
              skip_raps: HashSet::new(),
              uninit_locals: HashMap::new(),
              rhs_arm_ids: HashMap::new(),
              exit_drops: HashSet::new(),
              unlent_refs: HashSet::new(),
              tuple_temp_refs: HashSet::new(),
              pattern_copies: HashSet::new(),
              pattern_lender: None,
              temp_count: 0,
              param_value_count: 0,
              pending_param_borrows: Vec::new(),
              temp_drops_noted: HashSet::new(),
              unbranched_temps: HashSet::new(),
              loop_iterators: Vec::new(),
              destructure_srcs: HashMap::new(),
              destructure_moved: HashSet::new(),
              destructure_swapped: Vec::new(),
              destructure_ref_data: HashMap::new(),
              destructure_ref_src: HashMap::new(),
              ref_carriers: Vec::new(),
              taken_over_loans: HashSet::new(),
              break_moves: Vec::new(),
              all_paths_exit: false,
              borrow_floor: HashMap::new(),
              in_scope: Vec::new(),
              exit_frames: Vec::new(),
              literal_temps: HashMap::new(),
              premoved: HashSet::new(),
              live_tuples: Vec::new(),
              binding_columns: Vec::new(),
              break_binds: Vec::new(),
              deferred_breaks: Vec::new(),
              line_override: None,
              deferred_lines: Vec::new(),
              break_out: Vec::new(),
              break_pats: Vec::new(),
              loop_breaks: Vec::new(),
              try_move: None,
              try_moves: HashMap::new(),
            };
            visitor.visit_body(hir_body);
            visitor.print_lifetimes();
            visitor.print_out_of_scope();
            rap_hash_num = visitor.rap_hashes;
            binding_columns.append(&mut visitor.binding_columns);
            merge_visitor_raps(&mut rap_map, visitor.raps, fn_start_line);
            collect_shadow_notes(&tcx, impl_item.owner_id, fn_start_line, &mut shadow_notes_by_fq);
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

        // Marker on the fn signature line — leave the body
        // un-traced. Call sites in non-skipped fns will still emit
        // events whose endpoint is the Function RAP for this fn
        // (created by `add_fn` when the call is visited), so moves
        // into / out of it still appear.
        if skip_lines.contains(&fn_start_line) {
          if hide_marker_lines.contains(&fn_start_line) {
            let body_end_line = tcx.sess.source_map()
              .lookup_char_pos(hir_body.value.span.hi()).line;
            for l in fn_start_line..=body_end_line {
              hidden_source_lines.insert(l);
            }
            if fn_start_line >= 2
              && a_map.get(&(fn_start_line - 1))
                .map(|s| s.trim().is_empty()).unwrap_or(false)
            {
              hidden_source_lines.insert(fn_start_line - 1);
            }
          }
          continue;
        }

        let mut visitor = ExprVisitor {
          tcx,
          mir_body: body,
          hir_body: hir_body,
          bwf: bwf,
          current_scope: 0,
          current_fn_start: fn_start_line,
          borrow_map: HashMap::new(),
          raps: HashMap::new(),
          event_line_map: & mut line_map2,
          preprocessed_events: & mut pre_events,
          rap_hashes: rap_hash_num,
          source_map: & a_map,
          annotated_lines: & mut a_line_map,
          id_map: & mut owner_to_hash,
          unique_id: & mut ids,
          inside_branch: false,
          fn_ret: output,
          skip_lines: &skip_lines,
          skip_raps: HashSet::new(),
          uninit_locals: HashMap::new(),
          rhs_arm_ids: HashMap::new(),
          exit_drops: HashSet::new(),
          unlent_refs: HashSet::new(),
          tuple_temp_refs: HashSet::new(),
          pattern_copies: HashSet::new(),
          pattern_lender: None,
          temp_count: 0,
          param_value_count: 0,
              pending_param_borrows: Vec::new(),
              temp_drops_noted: HashSet::new(),
              unbranched_temps: HashSet::new(),
              loop_iterators: Vec::new(),
          destructure_srcs: HashMap::new(),
          destructure_moved: HashSet::new(),
          destructure_swapped: Vec::new(),
          destructure_ref_data: HashMap::new(),
          destructure_ref_src: HashMap::new(),
          ref_carriers: Vec::new(),
          taken_over_loans: HashSet::new(),
          break_moves: Vec::new(),
              all_paths_exit: false,
          borrow_floor: HashMap::new(),
          break_binds: Vec::new(),
          deferred_breaks: Vec::new(),
          line_override: None,
              deferred_lines: Vec::new(),
              break_out: Vec::new(),
          break_pats: Vec::new(),
          loop_breaks: Vec::new(),
          in_scope: Vec::new(),
          exit_frames: Vec::new(),
          literal_temps: HashMap::new(),
          premoved: HashSet::new(),
          live_tuples: Vec::new(),
          binding_columns: Vec::new(),
          try_move: None,
          try_moves: HashMap::new(),
        };
        visitor.visit_body(hir_body);
        visitor.print_lifetimes();
        visitor.print_out_of_scope();
        rap_hash_num = visitor.rap_hashes;
        binding_columns.append(&mut visitor.binding_columns);
        merge_visitor_raps(&mut rap_map, visitor.raps, fn_start_line);
        collect_shadow_notes(&tcx, item.owner_id, fn_start_line, &mut shadow_notes_by_fq);
      },
      _ => {}
    }
  }

  // annotate items after visiting all function bodies since we can assign hashes to top-level structures in fn bodies
  for id in tcx.hir_free_items() {
    let item = tcx.hir_item(id);
    if is_generated_item(item) {
      continue;
    }
    match &item.kind {
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
  // RAP hashes are unique (#231); sorting first means that if two ever
  // collided again, the same entry would win on every run.
  #[expect(clippy::disallowed_methods, reason = "sorted on the next line")]
  let mut fn_starts: Vec<(u64, usize)> = rap_map
    .values()
    .filter(|d| !matches!(d.rap, ResourceAccessPoint::Function(_)))
    .map(|d| (*d.rap.hash(), d.fn_start_line))
    .collect();
  fn_starts.sort();
  let fn_start_lines: HashMap<u64, usize> = fn_starts.into_iter().collect();

  // Header notes by RAP hash. (Sorted, as above, in case two RAPs ever
  // shared a hash.)
  #[expect(clippy::disallowed_methods, reason = "sorted on the next line")]
  let mut note_pairs: Vec<(u64, String, ShadowNote)> = shadow_notes_by_fq
    .iter()
    .filter_map(|(k, n)| rap_map.get(k).map(|d| (*d.rap.hash(), k.clone(), n.clone())))
    .collect();
  note_pairs.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
  let shadow_notes: HashMap<u64, ShadowNote> = note_pairs.into_iter().map(|(h, _, n)| (h, n)).collect();

  // Pass information to svg-generator
  match testing_helper.generate_vis(line_map2, pre_events, & mut a_line_map, rap_map.len() + 1, fn_start_lines, shadow_notes, binding_columns, hidden_source_lines, args.write_to_cwd) {
    Ok(_) => {}
    Err(e) => {
      eprintln!("{}", e);
    }
  }

}