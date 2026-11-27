//! RustViz 3 fact layer: what is true about each place on each source
//! row, from rustc's MIR and borrow-check facts only.
//!
//! Per body (fns, methods, closures), one pass computes:
//!
//! - ownership state per column (variable, field path, Box payload):
//!   owns / moved / maybe-moved / partly moved / unassigned
//!   (`MaybeInitializedPlaces`, `MaybeUninitializedPlaces`,
//!   `EverInitializedPlaces`);
//! - lent state per column: the loans in scope (NLL `Borrows`, the
//!   dataflow rustc's borrowck checks) whose place conflicts with the
//!   column's (rustc's own `places_conflict`), so a loan of `v[_]`, `*b`
//!   or `(o as Some).0` restricts `v`, `b`, `o`;
//! - events: init / reassign, move, copy, borrow (lender → holder, kind,
//!   two-phase, reborrow "through", macro argument), borrow end per CFG
//!   edge, re-point, drop (definite / drop flag / rest / nothing, early
//!   exits), scope end, temporary drop (lifetime-extended or not).
//!
//! Locations become rows through the row model (`rows.rs`). The HIR is
//! used for names (the 2.x column names) and to recognise early exits.
//!
//! The facts are computed on every run and feed the state table
//! (`table.rs`). `RV3_FACTS=1` also adds them to the plugin's JSON output
//! (`facts`); any other non-empty value (other than `0`) is a file to
//! write them to as well.

mod body;
mod control;
pub use control::SourceArm;
pub(crate) use control::{enclosing_arms, location_arms};
mod call_stores;
pub(crate) mod call_wording;
mod holder_path;
mod iterators;
mod places;
pub(crate) mod repoints;
mod direct_holders;
mod rows;
mod runtime;
pub mod schema;
pub mod table;
pub(crate) mod value_flow;

use rustc_hir::def::DefKind;
use rustc_middle::ty::TyCtxt;
use rustc_span::Span;
pub use schema::*;
pub(crate) use rows::real_successor_blocks;
pub(crate) use body::{dropped_on_residual, loop_enters_pattern, reference_argument_source, terminal_argument};

pub(crate) fn is_vec_expansion(tcx: TyCtxt<'_>, expansion: rustc_span::hygiene::ExpnId) -> bool {
    expansion.expn_data().macro_def_id.is_some_and(|id| tcx.is_diagnostic_item(rustc_span::Symbol::intern("vec_macro"), id))
}

/// `RV3_FACTS` is set (and not `0`).
pub fn enabled() -> bool {
    std::env::var_os("RV3_FACTS").is_some_and(|v| !v.is_empty() && v != "0")
}

/// Facts of every fn, method and closure body in the visualized file.
pub fn collect_crate(tcx: TyCtxt<'_>, in_target: &dyn Fn(Span) -> bool) -> CrateFacts {
    let mut bodies = Vec::new();
    let mut errors = Vec::new();
    let mut owners: Vec<_> = tcx
        .hir_body_owners()
        .filter(|d| matches!(tcx.def_kind(*d), DefKind::Fn | DefKind::AssocFn | DefKind::Closure))
        .filter(|d| {
            let sp = tcx.def_span(*d);
            !sp.from_expansion() && in_target(sp)
        })
        .collect();
    // Source order; a closure after the body it is in.
    owners.sort_by_key(|d| {
        let sp = tcx.def_span(*d);
        (tcx.is_closure_like(d.to_def_id()), sp.lo())
    });
    for def_id in owners {
        let func = tcx.def_path_str(def_id.to_def_id());
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let bwf = rustc_utils::mir::borrowck_facts::get_body_with_borrowck_facts(tcx, def_id);
            body::body_facts(tcx, bwf, func.clone())
        }));
        match r {
            Ok(b) => bodies.push(b),
            Err(e) => {
                // Handled here: not the panic a later failure reports.
                crate::forget_last_panic();
                let msg = e
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_else(|| "panic".into());
                errors.push(format!("{}: {}", func, msg));
            }
        }
    }
    CrateFacts { version: VERSION, bodies, errors }
}

/// The facts as JSON if `RV3_FACTS` asks for them, also written to its
/// file if it names one.
pub fn dump(facts: &CrateFacts) -> Option<serde_json::Value> {
    if !enabled() {
        return None;
    }
    let v = serde_json::to_value(facts).ok()?;
    if let Some(path) = std::env::var_os("RV3_FACTS").filter(|p| p != "1") {
        let written = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())
            .and_then(|s| std::fs::write(&path, s).map_err(|e| e.to_string()));
        if let Err(e) = written {
            eprintln!("rustviz: couldn't write the RV3_FACTS dump to {:?}: {}", path, e);
        }
    }
    Some(v)
}

#[cfg(test)]
mod scope_tests;
