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
use crate::svg_generator::data::{ApproxNote, ExternalEvent, ShadowNote};

/// Convert a body's presentation columns to their drawing-wide hashes once.
/// Function colors are shared across visitors. Variable names and shadow notes
/// belong to the visitor that allocated the column.
#[derive(Default)]
struct DrawingColumns {
  functions: HashMap<String, RapData>,
  start_lines: HashMap<u64, crate::facts::table::FunctionStart>,
  shadow_notes: HashMap<u64, ShadowNote>,
}

impl DrawingColumns {
  fn add_body(&mut self, tcx: TyCtxt<'_>, owner: rustc_hir::OwnerId, start_line: usize,
    raps: crate::expr_visitor::BodyRaps) {
    let names = crate::expr_visitor_utils::binding_names(&tcx, owner);
    #[expect(clippy::iter_over_hash_type, reason = "each variable has its own hash and each function its own key")]
    for (key, data) in raps.columns {
      if data.rap.is_fn() {
        self.functions.entry(key).or_insert(data);
      } else {
        let hash = *data.rap.hash();
        self.start_lines.insert(hash, crate::facts::table::FunctionStart { body: owner.def_id.to_def_id(), line: start_line });
        if let Some(note) = names.notes.get(&key) {
          self.shadow_notes.insert(hash, note.clone());
        }
      }
    }
  }
}

/// True for top-level items the user did not write: anything whose
/// span comes from a macro expansion, e.g. the `impl Debug for T`
/// that `#[derive(Debug)]` generates. A hand-written impl (even of
/// the same trait) has a plain source span and is still visited.
fn is_generated_item(item: &rustc_hir::Item<'_>) -> bool {
  item.span.from_expansion()
}

/// The local path of the file `span` is in, if it's a real file.
fn file_path_of(tcx: TyCtxt, span: rustc_span::Span) -> Option<std::path::PathBuf> {
  match tcx.sess.source_map().span_to_filename(span) {
    rustc_span::FileName::Real(f) => f.local_path().map(|p| p.to_path_buf()),
    _ => None,
  }
}

/// The path rustc uses for the crate source file `wanted` (relative to
/// the crate directory), or `None` if the crate doesn't compile it.
fn find_compiled_file(tcx: TyCtxt, wanted: &str) -> Option<std::path::PathBuf> {
  let want = std::fs::canonicalize(wanted).ok()?;
  tcx.sess.source_map().files().iter().find_map(|f| match &f.name {
    rustc_span::FileName::Real(r) => {
      let p = r.local_path()?;
      (std::fs::canonicalize(p).ok()? == want).then(|| p.to_path_buf())
    }
    _ => None,
  })
}

// "The main function"
pub fn rv_visitor(tcx: TyCtxt, args: &RVPluginArgs) {

  // The visitor's `info!` trace goes to a file only when asked for:
  // `RUSTVIZ_DEBUG_LOG=1` writes `output.log` in the current directory,
  // and any other non-empty value is the path to write. (It used to be
  // created, and truncated, in the cwd on every run.)
  if let Some(want) = std::env::var_os("RUSTVIZ_DEBUG_LOG").filter(|v| !v.is_empty()) {
    let path = if want == "1" { std::ffi::OsString::from("output.log") } else { want };
    let log_file = match OpenOptions::new()
      .create(true)
      .write(true)
      .truncate(true)
      .open(&path)
    {
      Ok(file) => file,
      Err(e) => {
          eprintln!("Error opening log file {:?}: {:?}", path, e);
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
  }

  // Data structures to be passed to visitor
  let mut testing_helper: RV1Helper = RV1Helper::new();
  let mut line_map2: BTreeMap<usize, Vec<ExternalEvent>> = BTreeMap::new();
  // The file to visualize: `--file`, or else the crate root rustc is
  // compiling (`src/lib.rs` or `src/main.rs`).
  let root_file = match &args.file {
    None => file_path_of(tcx, tcx.def_span(rustc_hir::def_id::CRATE_DEF_ID)),
    Some(wanted) => match find_compiled_file(tcx, wanted) {
      Some(p) => Some(p),
      None => {
        eprintln!("{wanted} isn't compiled as part of this crate (check its `mod` declaration)");
        std::process::exit(1);
      }
    },
  };
  let target_file = root_file.as_ref().and_then(|p| std::fs::canonicalize(p).ok());
  // True when `span` lies in the file being visualized; items from
  // other files of the crate (`mod` files) are skipped, since their
  // line numbers index a different file.
  let in_target = |span: rustc_span::Span| -> bool {
    match &target_file {
      Some(t) => file_path_of(tcx, span)
        .and_then(|p| std::fs::canonicalize(p).ok())
        .is_some_and(|p| &p == t),
      None => true,
    }
  };
  let a_map: BTreeMap<usize, String> = match testing_helper.initialize_line_map(root_file) {
    Ok(l) => l,
    Err(e) => {
      error!("{}", e);
      eprintln!("RustViz couldn't read the crate's source file ({e}). RustViz visualizes a single-file crate: src/lib.rs or src/main.rs.");
      std::process::exit(1);
    }
  };
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
  let mut approx_notes: Vec<ApproxNote> = Vec::new();
  let mut a_line_map: BTreeMap<usize, Vec<String>> = BTreeMap::new();
  let mut owner_to_hash: HashMap<String, usize> = HashMap::new();
  let mut row_layout = crate::event_order::RowLayout::default();
  let mut pre_events: Vec<(usize, ExternalEvent)> = Vec::new();
  let mut columns = DrawingColumns::default();
  for k in a_map.keys() {
    let s = a_map[k].clone();
    a_line_map.insert(*k, vec![s]);
    line_map2.insert(*k, vec![]);
  }
  let mut rap_hash_num: usize = 1;
  let mut ids: usize = 0;
  // Let-chain bindings drawn beside their sources (#583, #584).
  let mut binding_columns: Vec<(u64, u64)> = Vec::new();
  // Moves of destructuring-assignment cycles drawn before the source's
  // new value (#1343).
  let mut moves_out_first: Vec<usize> = Vec::new();
  // RustViz 3 fact layer: computed first, since the borrow ends come from
  // it (`emit_borrow_ends`); it also feeds the state table and, if
  // `RV3_FACTS` asks for them, the JSON dump.
  let crate_facts = crate::facts::collect_crate(tcx, &in_target);
  let mut state_table = crate::facts::table::StateTable::from_facts(&crate_facts);
  // MIR locations and loan ids are local to the exact body.
  let body_facts = |def_id: rustc_hir::def_id::LocalDefId| {
    crate_facts.bodies.iter().find(|b| b.def_id == def_id.to_def_id())
  };

  
  
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
    if is_generated_item(item) || !in_target(item.span) {
      continue;
    }
    // `// rustviz: hide` on a struct, enum, union, impl, use, trait, type
    // alias, const or static (#814): drop the whole item (leading
    // attributes, so `#[derive(..)]`, included) from the code panel, as for
    // a hidden fn. A hidden `impl` is not traced either, like a hidden fn.
    // `fn`s handle their own marker below; a marker on a `mod` or anywhere
    // else hides nothing.
    if !matches!(item.kind, ItemKind::Fn { .. } | ItemKind::Mod(..)) {
      let sm = tcx.sess.source_map();
      let first = sm.lookup_char_pos(item.span.lo()).line;
      let last = sm.lookup_char_pos(item.span.hi()).line;
      let mut start = first;
      while start >= 2
        && a_map.get(&(start - 1)).map(|l| l.trim_start().starts_with("#[")).unwrap_or(false)
      {
        start -= 1;
      }
      if let Some(m) = (start..=first).find(|l| hide_marker_lines.contains(l)) {
        for l in start..=last {
          hidden_source_lines.insert(l);
        }
        if start >= 2 && a_map.get(&(start - 1)).map(|l| l.trim().is_empty()).unwrap_or(false) {
          hidden_source_lines.insert(start - 1);
        }
        if matches!(item.kind, ItemKind::Impl(_)) {
          continue;
        }
      }
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

            // Each body's events in a Vec of their own: the passes after the
            // visit (`emit_borrow_ends`) walk the whole Vec, and on the shared
            // one they moved an earlier fn's events about (#1532).
            let mut fn_events: Vec<(usize, ExternalEvent)> = Vec::new();
            let mut visitor = ExprVisitor::new(
              tcx, body, hir_body, bwf, body_facts(body.source.def_id().expect_local()), fn_start_line,
              &mut line_map2, &mut fn_events, rap_hash_num, &a_map,
              &mut a_line_map, &mut owner_to_hash, &mut ids, output, &skip_lines,
            );
            visitor.facts = body_facts(local_def_id);
            visitor.visit_body(hir_body);
            visitor.emit_borrow_ends();
            visitor.mark_temp_moves(0);
            row_layout.append(&mut visitor.row_layout);
            visitor.collect_approx_notes(hir_body);
            approx_notes.append(&mut visitor.approx_notes);
            rap_hash_num = visitor.rap_hashes;
            state_table.end_loans.append(&mut visitor.end_loans);
            binding_columns.append(&mut visitor.binding_columns);
            moves_out_first.append(&mut visitor.moves_out_first);
            columns.add_body(tcx, impl_item.owner_id, fn_start_line, visitor.raps);
            pre_events.append(&mut fn_events);
          }
        }
      }
      ItemKind::Fn { sig: fn_sig, body: body_id, ident: _, .. } => {
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

        // Each body's events in a Vec of their own: the passes after the
        // visit (`emit_borrow_ends`) walk the whole Vec, and on the shared
        // one they moved an earlier fn's events about (#1532).
        let mut fn_events: Vec<(usize, ExternalEvent)> = Vec::new();
        let mut visitor = ExprVisitor::new(
          tcx, body, hir_body, bwf, body_facts(body.source.def_id().expect_local()), fn_start_line,
          &mut line_map2, &mut fn_events, rap_hash_num, &a_map,
          &mut a_line_map, &mut owner_to_hash, &mut ids, output, &skip_lines,
        );
        visitor.facts = body_facts(def_id);
        visitor.visit_body(hir_body);
        visitor.emit_borrow_ends();
        visitor.mark_temp_moves(0);
        row_layout.append(&mut visitor.row_layout);
        visitor.collect_approx_notes(hir_body);
        approx_notes.append(&mut visitor.approx_notes);
        rap_hash_num = visitor.rap_hashes;
        state_table.end_loans.append(&mut visitor.end_loans);
        binding_columns.append(&mut visitor.binding_columns);
        moves_out_first.append(&mut visitor.moves_out_first);
        columns.add_body(tcx, item.owner_id, fn_start_line, visitor.raps);
        pre_events.append(&mut fn_events);
      },
      _ => {}
    }
  }

  // annotate items after visiting all function bodies since we can assign hashes to top-level structures in fn bodies
  for id in tcx.hir_free_items() {
    let item = tcx.hir_item(id);
    if is_generated_item(item) || !in_target(item.span) {
      continue;
    }
    match &item.kind {
      ItemKind::Fn { sig: fn_sig, ident: fn_ident, .. } => {
        let line = tcx.sess.source_map().lookup_char_pos(fn_sig.span.lo()).line;
        let line_str = &a_map[&line];
        if fn_ident.as_str() != "main" {
          annotate_toplevel_fn(*fn_ident, line_str, &columns.functions, &mut a_line_map, &mut rap_hash_num, &tcx);
        }
      }
      ItemKind::Enum(_ident, _generics, e_def) => {
        for variant in e_def.variants.iter() {
          // The variant's own name; struct-like variants have no constructor
          // fn (no ctor HirId), and no fn entry in the rap map, so nothing is
          // annotated for them (#907).
          let ctor_name = variant.ident.name.as_str().to_owned();
          let parent_name = tcx.hir_name(tcx.parent_hir_id(variant.hir_id)).as_str().to_owned();
          annotate_enum_variant(ctor_name.as_str(), parent_name.as_str(), &variant, &columns.functions, & mut a_line_map, &tcx);
        }
      }
      ItemKind::Impl(imp) => {
        for item_ref in imp.items.iter() {
          let impl_item = tcx.hir_impl_item(*item_ref);
          if matches!(impl_item.kind, rustc_hir::ImplItemKind::Fn(_, _)) {
            let fn_ident = impl_item.ident;
            let line = tcx.sess.source_map().lookup_char_pos(fn_ident.span.lo()).line;
            let line_str = &a_map[&line];
            annotate_toplevel_fn(fn_ident, line_str, &columns.functions, &mut a_line_map, &mut rap_hash_num, &tcx);
          }
        }
      }

      _ => {}
    }
  }

  // A temporary's drop comes after what else happens on its line: a
  // field moved out of it (`let t = make_q().a;`) is drawn after the
  // temporary is made, but before it is dropped (#469).
  pre_events.sort_by_key(|k| (k.0, matches!(k.1, ExternalEvent::TemporaryDrop { .. })));

  let num_raps = columns.functions.len() + columns.start_lines.len() + 1;
  let facts = crate::facts::dump(&crate_facts);

  // Pass information to svg-generator
  match testing_helper.generate_vis(line_map2, pre_events, & mut a_line_map, num_raps, columns.start_lines, columns.shadow_notes, binding_columns, moves_out_first, hidden_source_lines, approx_notes, args.write_to_cwd, facts, state_table, row_layout) {
    Ok(_) => {}
    Err(e) => {
      eprintln!("{}", e);
    }
  }

}