use super::*;
use crate::expr_visitor::ExprVisitor;
use crate::expr_visitor_utils::collect_pat_bindings;
use crate::svg_generator::data::ExternalEvent;
use rustc_hir::intravisit::Visitor;
use rustc_interface::interface::Compiler;
use rustc_middle::mir::TerminatorKind;
use std::collections::{BTreeMap, HashSet};

// These occurrences need not have a separate SVG dot. Inspect the same
// borrowck query and visitor as the installed plugin, before layout.
fn with_body(name: &str, check: impl Fn(TyCtxt<'_>, &BodyFacts) + Send) {
    struct Check<'a, F>(&'a str, F);
    impl<F: Fn(TyCtxt<'_>, &BodyFacts) + Send> rustc_driver::Callbacks for Check<'_, F> {
        fn config(&mut self, cfg: &mut rustc_interface::Config) {
            cfg.override_queries = Some(if self.0 == "folded" {
                |session, providers| {
                    rustc_utils::mir::borrowck_facts::override_queries(session, providers);
                    providers.queries.mir_promoted = |tcx, id| {
                        let (mir, promoted) = (rustc_interface::DEFAULT_QUERY_PROVIDERS.queries.mir_promoted)(tcx, id);
                        if tcx.def_path_str(id) != "folded" { return (mir, promoted); }
                        let mut body = mir.steal();
                        share_loop_drop(&mut body);
                        (tcx.alloc_steal_mir(body), promoted)
                    };
                }
            } else { rustc_utils::mir::borrowck_facts::override_queries });
        }
        fn after_analysis<'tcx>(&mut self, _: &Compiler, tcx: TyCtxt<'tcx>) -> rustc_driver::Compilation {
            let id = tcx.hir_body_owners().find(|id| tcx.def_path_str(*id) == self.0).unwrap();
            let bwf = rustc_utils::mir::borrowck_facts::get_body_with_borrowck_facts(tcx, id);
            (self.1)(tcx, &body::body_facts(tcx, bwf, self.0.into()));
            rustc_driver::Compilation::Stop
        }
    }
    let sysroot = std::process::Command::new("rustc").args(["--print", "sysroot"]).output().unwrap();
    let fixture = if name == "exit_with_guard" { "diverging_field_holder_1884" } else { "scope_drop_review" };
    let path = format!("{}/tests/{fixture}.rs", env!("CARGO_MANIFEST_DIR"));
    let args = ["rustc", &path, "--sysroot", std::str::from_utf8(&sysroot.stdout).unwrap().trim(),
        "--edition=2024", "--crate-type=lib", "-Awarnings", "-Zmir-opt-level=0", "-Zmaximal-hir-to-mir-coverage"].map(str::to_owned);
    rustc_driver::run_compiler(&args, &mut Check(name, check));
}

#[test]
fn scope_review_ordinary_unwind_drops_live_argument() {
    with_body("unwind", |tcx, facts| {
        let bwf = rustc_utils::mir::borrowck_facts::get_body_with_borrowck_facts(tcx, facts.def_id.expect_local());
        let call = bwf.body.basic_blocks.iter().find(|bb| matches!(&bb.terminator().kind,
            TerminatorKind::Call { func, .. } if func.place().is_some())).unwrap().terminator();
        let site = call.source_info.span.source_callsite();
        assert!(facts.scope_ends.at_exit(site).any(|(_, n, e)| n == "text" && e.effect == ScopeDrop::Whole));
    });
}

#[test]
fn scope_review_process_exit_skips_cleanup() {
    with_body("exit_with_guard", |tcx, facts| {
        let bwf = rustc_utils::mir::borrowck_facts::get_body_with_borrowck_facts(tcx, facts.def_id.expect_local());
        let call = bwf.body.basic_blocks.iter().find(|bb| matches!(bb.terminator().kind,
            TerminatorKind::Call { target: None, .. })).unwrap().terminator();
        assert!(!facts.scope_ends.at_exit(call.source_info.span.source_callsite()).any(|(_, n, _)| n == "text" || n == "guard"));
    });
}

#[test]
fn scope_review_return_ends_plain_argument_storage() {
    with_body("early", |tcx, facts| {
        let text = |span| tcx.sess.source_map().span_to_snippet(span).unwrap();
        let local = facts.bindings.columns["plain"][0];
        assert!(facts.scope_ends.for_place(local, "plain").iter().any(|e|
            e.site.is_some_and(|site| text(site) == "return") && e.has_value && e.effect == ScopeDrop::Storage));
    });
}

#[test]
fn scope_review_merged_effect_is_independent_of_operand_order() {
    with_body("early", |_, facts| {
        let local = facts.bindings.columns["plain"][0];
        let mut whole = facts.scope_ends.for_place(local, "plain")[0].clone();
        whole.site = None;
        whole.effect = ScopeDrop::Whole;
        whole.flag = false;
        let mut tested = whole.clone();
        tested.flag = true;
        let mut empty = tested.clone();
        empty.effect = ScopeDrop::Empty;
        for inputs in [[&whole, &tested, &empty], [&whole, &empty, &tested], [&empty, &whole, &tested],
            [&empty, &tested, &whole], [&tested, &whole, &empty], [&tested, &empty, &whole]] {
            assert_eq!(ScopeEnd::across(&inputs).unwrap().effect, ScopeDrop::ByPath);
        }
    });
}

fn with_visitor(name: &str, check: impl Fn(&ExprVisitor<'_, '_>) + Send) {
    with_body(name, move |tcx, facts| {
        let local = facts.def_id.expect_local();
        let bwf = rustc_utils::mir::borrowck_facts::get_body_with_borrowck_facts(tcx, local);
        let hir = tcx.hir_body_owned_by(local);
        let lines: BTreeMap<_, _> = include_str!("../../tests/scope_drop_review.rs").lines().enumerate()
            .map(|(i, line)| (i + 1, line.to_owned())).collect();
        let mut annotations = lines.iter().map(|(&i, s)| (i, vec![s.clone()])).collect();
        let mut line_events = lines.keys().map(|&i| (i, Vec::new())).collect();
        let (mut events, mut ids, mut next, skip) = (Vec::new(), Default::default(), 0, HashSet::new());
        let mut visitor = ExprVisitor::new(tcx, &bwf.body, hir, bwf, Some(facts), facts.first_line,
            &mut line_events, &mut events, 1, &lines, &mut annotations, &mut ids, &mut next, false, &skip);
        visitor.facts = Some(facts);
        visitor.visit_body(hir);
        check(&visitor);
    });
}

#[test]
fn scope_review_each_box_binding_declares_only_its_own_fields() {
    with_visitor("siblings", |v| {
        let mut seen = Vec::new();
        for local in &v.raps.bindings.columns["p"] {
            let id = v.raps.bindings.declarations.iter().find(|(_, l)| *l == local).unwrap().0;
            let rustc_hir::Node::Pat(pat) = v.tcx.hir_node(*id) else { panic!("expected binding") };
            let mut declared = HashSet::new();
            collect_pat_bindings(pat, &v.tcx, &v.raps, &mut declared);
            assert!(declared.iter().all(|r| r.scope_local() == Some(*local)), "{declared:?}");
            assert!(declared.iter().any(|r| r.name() == "p.take"));
            seen.extend(declared.iter().filter(|r| r.name() == "p.first" || r.name() == "p.second").map(|r| r.name().clone()));
        }
        seen.sort();
        assert_eq!(seen, ["p.first", "p.second"]);
    });
}

#[test]
fn scope_review_return_orders_fields_by_declaration() {
    fn ends(events: &[(usize, ExternalEvent)], ordered: &mut Vec<(usize, String)>) {
        for (_, e) in events {
            match e {
                ExternalEvent::GoOutOfScope { ro, exit_site: Some(_), id, .. } if ro.name() == "p.z" || ro.name() == "p.a" =>
                    ordered.push((*id, ro.name().clone())),
                ExternalEvent::Branch { branches, .. } => {
                    for b in branches { ends(&b.e_data, ordered); }
                }
                _ => {}
            }
        }
    }
    with_visitor("early", |v| {
        let mut ordered = Vec::new();
        ends(v.preprocessed_events, &mut ordered);
        ordered.sort();
        assert_eq!(ordered.into_iter().map(|(_, n)| n).collect::<Vec<_>>(), ["p.z", "p.a"]);
    });
}

// This rustc version duplicates the identical destruction suffix after a
// continue. Coalesce the two Drops *before borrow checking*, so rustc itself
// recomputes move paths, loans and elaborated drop flags for the shared CFG.
fn share_loop_drop(body: &mut rustc_middle::mir::Body<'_>) {
    use rustc_middle::mir::{BasicBlockData, Operand, Rvalue, StatementKind, VarDebugInfoContents};
    let s = body.var_debug_info.iter().find(|v| v.name.as_str() == "s").unwrap();
    let VarDebugInfoContents::Place(s) = s.value else { panic!("s is a binding") };
    let moved = body.basic_blocks.iter_enumerated().find(|(_, b)| b.statements.iter().any(|st|
        matches!(&st.kind, StatementKind::Assign(a) if matches!(a.1, Rvalue::Use(Operand::Move(p)) if p == s)))).unwrap().0;
    let drops: Vec<_> = body.basic_blocks.iter_enumerated().filter(|(_, b)| !b.is_cleanup
        && matches!(b.terminator().kind, TerminatorKind::Drop { place, .. } if place == s)).map(|(bb, _)| bb).collect();
    assert_eq!(drops.len(), 2);
    let normal = *drops.iter().find(|bb| !body.basic_blocks.dominators().dominates(moved, **bb)).unwrap();
    let term = body[normal].terminator().clone();
    let blocks = body.basic_blocks_mut();
    let shared = blocks.push(BasicBlockData::new(Some(term), false));
    for bb in drops { blocks[bb].terminator_mut().kind = TerminatorKind::Goto { target: shared }; }
}

#[test]
fn scope_review_folded_continue_is_excluded_from_the_ordinary_drop() {
    with_body("folded", |tcx, facts| {
        let s = facts.bindings.columns["s"][0];
        let ends = facts.scope_ends.for_place(s, "s");
        let early = ends.iter().find(|e| e.site.is_some_and(|span|
            tcx.sess.source_map().span_to_snippet(span).is_ok_and(|text| text == "continue"))).unwrap();
        assert_eq!(early.effect, ScopeDrop::Empty);
        let ordinary = ends.iter().find(|e| e.site.is_none()).unwrap();
        assert_eq!(ordinary.effect, ScopeDrop::Whole);
        assert!(ordinary.flag, "the shared destructor tests the initialization flag");
    });
}

#[test]
fn scope_review_returning_helper_does_not_make_process_exit_unwind() {
    with_body("forwarded_exit", |_, facts| {
        let local = facts.bindings.columns["text"][0];
        assert!(facts.scope_ends.for_place(local, "text").is_empty());
    });
}
