//! Where a reference's borrow ends, read from the fact layer (RustViz 3
//! P2-1b). For each reference the 2.x walk keeps a loan for (`borrow_map`,
//! its retired and earlier-arm loans), the facts say on which row the
//! borrow ends and how:
//!
//! - the reference is the last holder of the loan on some path (the loan's
//!   `BorrowEnd` names it): it gives the lender back there, an arrow;
//! - it stops holding a loan that someone else still holds (`HoldEnd`: a
//!   copy's own last use, a loop binding at the end of each time round):
//!   its own end, with no arrow (Q7);
//! - the loan never ends in the body (returned to the caller, or stored
//!   where the caller can reach it): nothing.
//!
//! A path that leaves a loop from its head (the iterator held the loan
//! there) ends the loan where the loop ends, on the loop's closing row: the
//! body's rows still have it lent on the paths that go round again (Q6).

use crate::facts::schema::{BodyFacts, Fact, LoanInfo};
use rustc_hir::intravisit::{self, Visitor};
use rustc_middle::ty::TyCtxt;
use std::collections::BTreeSet;

/// One body's facts, with its loops.
pub struct BodyEnds<'a> {
  pub facts: &'a BodyFacts,
  /// Each loop as (head row, closing row).
  pub loops: Vec<(usize, usize)>,
}

/// What the facts say about one holder's borrow of some loans.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct HolderEnd {
  /// The last row where the holder is the last one holding the loan on a
  /// path (it gives the lender back there).
  pub gives_back: Option<usize>,
  /// Every such end leaves a loop from its head: the loop's iterator held
  /// the loan there, not the holder.
  pub only_loop_exits: bool,
  /// The last row where it stops holding a loan that goes on.
  pub stops: Option<usize>,
  /// The last row any of the loans ends on, whoever holds it there (the
  /// borrow's last row, as 2.x's single end line has it).
  pub last_end: Option<usize>,
}

/// `n` names the holder `k`, a field of it, or the variable it is a field of
/// (`h`, `h.r`).
pub fn names_match(k: &str, n: &str) -> bool {
  n == k
    || n.strip_prefix(k).is_some_and(|r| r.starts_with('.'))
    || k.strip_prefix(n).is_some_and(|r| r.starts_with('.'))
}

/// The variable a lender or holder place is in (`s` of `*s`, `p.a`, `v[_]`).
fn root(place: &str) -> &str {
  let p = place.trim_start_matches('*').trim_start_matches('(');
  p.split(['.', '[', ' ', ')']).next().unwrap_or(p)
}

impl<'a> BodyEnds<'a> {
  pub fn new<'tcx>(tcx: TyCtxt<'tcx>, facts: &'a BodyFacts, hir_body: &rustc_hir::Body<'tcx>) -> Self {
    struct Loops<'tcx> {
      tcx: TyCtxt<'tcx>,
      out: Vec<(usize, usize)>,
    }
    impl<'tcx> Visitor<'tcx> for Loops<'tcx> {
      fn visit_expr(&mut self, e: &'tcx rustc_hir::Expr<'tcx>) {
        if let rustc_hir::ExprKind::Loop(..) = e.kind {
          let sm = self.tcx.sess.source_map();
          let sp = e.span.source_callsite();
          self.out.push((sm.lookup_char_pos(sp.lo()).line, sm.lookup_char_pos(sp.hi()).line));
        }
        intravisit::walk_expr(self, e);
      }
    }
    let mut v = Loops { tcx, out: Vec::new() };
    v.visit_expr(hir_body.value);
    BodyEnds { facts, loops: v.out }
  }

  /// The loans whose own reference `k` holds (stored in it, copied or moved
  /// into it, or out of a call's result), less those a new value of `k`
  /// replaced (the walk ends those where it gives `k` that value).
  pub fn loans_held_by(&self, k: &str) -> Vec<&'a LoanInfo> {
    let mut replaced: BTreeSet<usize> = BTreeSet::new();
    let mut renewed: BTreeSet<usize> = BTreeSet::new();
    for r in &self.facts.rows {
      for e in &r.facts {
        if let Fact::Repoint { holder, old, new } = &e.fact {
          if names_match(k, holder) {
            replaced.extend(old.iter().copied());
            renewed.extend(new.iter().copied());
          }
        }
      }
    }
    self.facts.loans.iter()
      .filter(|l| l.direct_holders.iter().chain(l.via_call.iter()).any(|h| names_match(k, h)))
      .filter(|l| !replaced.contains(&l.loan) || renewed.contains(&l.loan))
      .collect()
  }

  /// Whether `l` borrows from the variable `name` (`Caller` for a
  /// parameter's), itself or through the reference chain it is made through.
  pub fn lends(&self, l: &LoanInfo, name: &str) -> bool {
    let mut seen: BTreeSet<usize> = BTreeSet::new();
    let mut work = vec![l.loan];
    while let Some(id) = work.pop() {
      if !seen.insert(id) {
        continue;
      }
      let Some(m) = self.facts.loans.iter().find(|x| x.loan == id) else { continue };
      if (name == "Caller" && m.source == "caller")
        || root(&m.lender) == name
        || m.through.as_deref().is_some_and(|t| root(t) == name)
      {
        return true;
      }
      work.extend(m.made_through.iter().copied());
    }
    false
  }

  /// The row an end on the edge from `row` to `to_line` is drawn on: the
  /// loop's closing row for a path that leaves a loop from its head.
  fn drawn_row(&self, row: usize, to_line: usize) -> (usize, bool) {
    if to_line > row && self.loops.iter().any(|(h, e)| *h == row && to_line >= *e) {
      (to_line, true)
    } else {
      (row, false)
    }
  }

  /// The ends of `k`'s hold on `loans`, on rows in `rows` (from, to: to
  /// exclusive).
  pub fn holder_end(&self, k: &str, loans: &[usize], rows: (usize, usize)) -> HolderEnd {
    let mut out = HolderEnd { only_loop_exits: true, ..HolderEnd::default() };
    for r in &self.facts.rows {
      if r.line < rows.0 || r.line >= rows.1 {
        continue;
      }
      for e in &r.facts {
        match &e.fact {
          Fact::BorrowEnd { loan, holder, to_line, .. } if loans.contains(loan) => {
            let (row, exit) = self.drawn_row(r.line, *to_line);
            out.last_end = Some(out.last_end.map_or(row, |g| g.max(row)));
            if holder.split('|').any(|h| names_match(k, h)) {
              out.gives_back = Some(out.gives_back.map_or(row, |g| g.max(row)));
              out.only_loop_exits &= exit;
            }
          }
          Fact::HoldEnd { loan, holder, .. } if loans.contains(loan) && names_match(k, holder) => {
            out.stops = Some(out.stops.map_or(r.line, |s| s.max(r.line)));
          }
          _ => {}
        }
      }
    }
    if out.gives_back.is_none() {
      out.only_loop_exits = false;
    }
    out
  }
}
