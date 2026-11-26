//! RustViz 3 spike (not for merge): today's renderer fed from the MIR
//! fact layer. For a straight-line body (no MIR `SwitchInt`), the HIR
//! visitor still runs, for the columns (RAPs: names, hashes, kinds) and
//! the annotated code panel; its events are then replaced by events
//! synthesized from `rv3_facts`. Enabled by `RV3_RENDER=1`.
//!
//! This is the hybrid boundary: MIR decides *what happens on which row*
//! (moves, copies, borrows and their ends, drops), the HIR layer decides
//! *what the columns are called*, and today's renderer and tooltip text
//! are reused unchanged.
#![allow(clippy::disallowed_methods, clippy::iter_over_hash_type)]

use crate::expr_visitor::RapData;
use crate::rv3_facts::{BodyFacts, Fact};
use crate::svg_generator::data::{
    ExternalEvent, ImplicitRef, LoopHead, ResourceAccessPoint, ResourceTy, ScopeEndHolds, StoredIn,
};
use rustc_borrowck::consumers::BodyWithBorrowckFacts;
use rustc_middle::mir::TerminatorKind;
use std::collections::{BTreeMap, HashMap};

pub fn enabled() -> bool {
    std::env::var("RV3_RENDER").is_ok_and(|v| v == "1")
}

/// No conditional control flow: the drawing has no forks, so the event
/// list needs no `Branch` events (the part of the renderer contract the
/// spike doesn't synthesize).
pub fn straight_line(bwf: &BodyWithBorrowckFacts<'_>) -> bool {
    bwf.body.basic_blocks.iter().all(|d| d.is_cleanup || !matches!(d.terminator().kind, TerminatorKind::SwitchInt { .. }))
}

struct Ids<'a>(&'a mut usize);
impl Ids<'_> {
    fn next(&mut self) -> usize {
        let i = *self.0;
        *self.0 += 1;
        i
    }
}

pub fn synthesize(facts: &BodyFacts, raps: &HashMap<String, RapData>, ids: &mut usize) -> Vec<(usize, ExternalEvent)> {
    let mut ids = Ids(ids);
    let rap = |n: &str| -> Option<ResourceAccessPoint> {
        let n = n.trim_start_matches("closure ").trim_end_matches("()");
        raps.get(n).map(|d| d.rap.clone())
    };
    let val = |n: &str| rap(n).map(ResourceTy::Value);
    // A field path with no column of its own: its root's column, partially.
    let src_of = |n: &str| -> Option<(ResourceTy, bool)> {
        val(n).map(|v| (v, false)).or_else(|| val(n.split('.').next().unwrap_or(n)).map(|v| (v, n.contains('.'))))
    };
    // A call with no function column (the HIR layer drew none): Anonymous.
    let dst_of = |n: &str| -> Option<ResourceTy> {
        val(n).or_else(|| n.ends_with("()").then_some(ResourceTy::Anonymous))
    };
    let is_fn = |n: &str| n.ends_with("()");
    let mut out: Vec<(usize, ExternalEvent)> = Vec::new();
    for row in &facts.rows {
        let line = row.line;
        // A row's events in evaluation order, as today's plugin lists them:
        // values in, then borrows, then borrow ends, then scope ends.
        let mut phase: BTreeMap<u8, Vec<ExternalEvent>> = BTreeMap::new();
        let mut push = |k: u8, e: ExternalEvent| phase.entry(k).or_default().push(e);
        let moved_into: Vec<&str> = row.facts.iter().filter_map(|f| match f {
            Fact::Move { to, .. } | Fact::Copy { to, .. } => Some(to.as_str()),
            _ => None,
        }).collect();
        for f in &row.facts {
            match f {
                Fact::Init { path, from, reassign: _, maybe_had: _ } => {
                    let Some(to) = val(path) else { continue };
                    if moved_into.contains(&path.as_str()) || from.starts_with('&') {
                        continue; // the Move / Copy / Borrow fact draws it
                    }
                    if is_fn(from) {
                        if let Some(f) = val(from) {
                            push(1, ExternalEvent::Move { from: f, to, is_partial: false, into_closure: false, loop_head: LoopHead::No, via_try: None, id: ids.next(), break_alt: None, break_out: false, closure_value: false });
                            continue;
                        }
                    }
                    push(1, ExternalEvent::Bind { from: ResourceTy::Anonymous, to, id: ids.next(), break_alt: None });
                }
                Fact::Move { path, to } => {
                    let (Some((from, partial)), Some(to)) = (src_of(path), dst_of(to)) else { continue };
                    push(2, ExternalEvent::Move { from, to, is_partial: partial, into_closure: false, loop_head: LoopHead::No, via_try: None, id: ids.next(), break_alt: None, break_out: false, closure_value: false });
                }
                Fact::Copy { path, to } => {
                    let (Some((from, partial)), Some(to)) = (src_of(path), dst_of(to)) else { continue };
                    push(2, ExternalEvent::Copy { from, to, is_partial: partial, into_closure: false, closure_value: false, into_ctor: None, pointee: None, id: ids.next(), break_alt: None, of_const: None });
                }
                Fact::Borrow { lender, kind, holder, .. } => {
                    let Some(from) = val(lender) else { continue };
                    let first = holder.split('|').next().unwrap_or("");
                    let Some(to) = val(first) else { continue };
                    if is_fn(first) && !rap(first).is_some_and(|r| r.is_fn()) {
                        continue; // calling a closure variable: no function column
                    }
                    let is_mut = kind.starts_with("mut") || kind.starts_with("unique");
                    let e = if is_fn(first) {
                        if is_mut {
                            ExternalEvent::PassByMutableReference { from, to, is_reborrow: false, result_has_lifetime: false, implicit: ImplicitRef::No, stored_in: StoredIn::default(), result_keeps_borrow: false, break_alt: None, of_ref_var: false, deep: None, id: ids.next() }
                        } else {
                            ExternalEvent::PassByStaticReference { from, to, implicit: ImplicitRef::No, refcell_borrow_mut: false, pointee_interior_mut: false, stored_in: StoredIn::default(), result_keeps_borrow: false, break_alt: None, still_borrowed_by: Vec::new(), of_ref_var: false, deep: None, id: ids.next() }
                        }
                    } else if is_mut {
                        ExternalEvent::MutableBorrow { from, to, is_partial: false, is_reborrow: false, break_alt: None, via: None, id: ids.next(), of_ref_var: false }
                    } else {
                        ExternalEvent::StaticBorrow { from, to, is_partial: false, is_reborrow: false, break_alt: None, via: None, id: ids.next(), of_ref_var: false }
                    };
                    push(3, e);
                }
                Fact::BorrowEnd { loan, lender, holder, .. } => {
                    let first = holder.split('|').next().unwrap_or("");
                    if first.is_empty() || is_fn(first) {
                        continue; // a call's borrow: the PassBy event is its own end
                    }
                    let (Some(from), Some(to)) = (val(first), val(lender)) else { continue };
                    let is_mut = facts.loans.iter().find(|l| l.loan == *loan).is_some_and(|l| l.kind.starts_with("mut") || l.kind.starts_with("unique"));
                    let e = if is_mut {
                        ExternalEvent::MutableDie { from, to, id: ids.next(), static_lender: None, still_borrowed_by: Vec::new(), of_ref_var: false }
                    } else {
                        ExternalEvent::StaticDie { from, to, id: ids.next(), static_lender: None, still_borrowed_by: Vec::new(), of_ref_var: false }
                    };
                    push(4, e);
                }
                Fact::Drop { path, how, at_reassign } => {
                    if *at_reassign || path.contains('.') {
                        continue;
                    }
                    let Some(ro) = rap(path) else { continue };
                    let holds = match how.as_str() {
                        "drops" | "drops_rest" => ScopeEndHolds::Value,
                        "drops_if_init" => ScopeEndHolds::Maybe,
                        _ => ScopeEndHolds::Nothing,
                    };
                    push(5, ExternalEvent::GoOutOfScope { ro, holds: Some(holds), id: ids.next(), early: None });
                }
                Fact::ScopeEnd { path, .. } => {
                    let Some(ro) = rap(path) else { continue };
                    let holds = if ro.is_ref() { None } else { Some(ScopeEndHolds::Value) };
                    push(5, ExternalEvent::GoOutOfScope { ro, holds, id: ids.next(), early: None });
                }
                Fact::Repoint { .. } | Fact::TempDrop { .. } => {}
            }
        }
        for (_, es) in phase {
            for e in es {
                out.push((line, e));
            }
        }
    }
    out
}

/// The events that today's plugin also lists in the line map (arrows
/// between two columns).
pub fn is_arrow(e: &ExternalEvent) -> bool {
    use ExternalEvent::*;
    match e {
        Move { from, to, .. } | Copy { from, to, .. } | StaticBorrow { from, to, .. } | MutableBorrow { from, to, .. }
        | StaticDie { from, to, .. } | MutableDie { from, to, .. } | PassByStaticReference { from, to, .. }
        | PassByMutableReference { from, to, .. } => matches!((from, to), (ResourceTy::Value(_), ResourceTy::Value(_))),
        _ => false,
    }
}
