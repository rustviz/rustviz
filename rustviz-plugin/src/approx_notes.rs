//! Approximation notes (`ApproxNote`): where a figure simplifies what a
//! construct does in a way a student could misread, the dot drawn on it
//! says so in one parenthetical sentence. Found by a pass over a fn's
//! HIR after its events are collected, so the rest of the visitor is
//! unchanged by them.
//!
//! Each note is only made where the construct is recognised for sure
//! from the HIR; a note on code it doesn't apply to would be a new
//! false statement.

use crate::expr_visitor::ExprVisitor;
use crate::expr_visitor_utils::{binding_name, span_end_line, span_to_line};
use crate::svg_generator::data::ApproxNote;
use rustc_hir::intravisit::{self, Visitor};
use rustc_hir::{Expr, ExprKind, LoopSource, Pat, PatKind};

/// The loops of a body (not inside closures, whose bodies aren't drawn),
/// outermost first.
struct Loops<'tcx>(Vec<&'tcx Expr<'tcx>>);

impl<'tcx> Visitor<'tcx> for Loops<'tcx> {
  fn visit_expr(&mut self, e: &'tcx Expr<'tcx>) {
    if matches!(e.kind, ExprKind::Loop(..)) {
      self.0.push(e);
    }
    intravisit::walk_expr(self, e);
  }
}

/// The variables a pattern or expression declares in the user's code
/// (not the hidden ones of a `for` loop's desugaring), in source order.
struct Bindings<'tcx>(Vec<&'tcx Pat<'tcx>>);

impl<'tcx> Visitor<'tcx> for Bindings<'tcx> {
  fn visit_pat(&mut self, p: &'tcx Pat<'tcx>) {
    if matches!(p.kind, PatKind::Binding(..)) && !p.span.from_expansion() {
      self.0.push(p);
    }
    intravisit::walk_pat(self, p);
  }
}

impl<'a, 'tcx> ExprVisitor<'a, 'tcx> {
  /// The approximation notes of the fn whose body is `body`, added to
  /// `self.approx_notes`.
  pub fn collect_approx_notes(&mut self, body: &'tcx rustc_hir::Body<'tcx>) {
    self.loop_notes(body);
  }

  /// A loop is drawn as one pass through its body (#853). One note per
  /// loop, on the first dot of a pass (see `ApproxNote::Loop`).
  fn loop_notes(&mut self, body: &'tcx rustc_hir::Body<'tcx>) {
    let mut loops = Loops(Vec::new());
    loops.visit_expr(body.value);
    // Each loop the user wrote, with its lines.
    let mut found: Vec<(&'tcx Expr<'tcx>, usize, usize)> = Vec::new();
    for l in loops.0 {
      // A loop a macro wrote (not `for` / `while`, which rustc lowers
      // to `loop` with a desugaring mark) isn't the user's code.
      if l.span.from_expansion() && l.span.desugaring_kind().is_none() {
        continue;
      }
      found.push((l, span_to_line(&l.span, &self.tcx), span_end_line(&l.span, &self.tcx)));
    }
    for &(l, head, end) in &found {
      let ExprKind::Loop(block, _, source, _) = l.kind else { continue };
      // The lines that run on every pass: the body's, but not its
      // first line where a `for` loop's iterator is made or a `loop`'s
      // value is bound (`let n = loop {`), nor its last, where what
      // leaves the loop is drawn. A `while` loop's condition is
      // checked on every pass.
      let from = if matches!(source, LoopSource::While) { head } else { head + 1 };
      let pass = (from, end.saturating_sub(1));
      let mut bindings = Bindings(Vec::new());
      bindings.visit_block(block);
      let declares = !bindings.0.is_empty();
      let mut locals: Vec<u64> = Vec::new();
      let mut head_cols: Vec<u64> = Vec::new();
      for p in &bindings.0 {
        let name = binding_name(&self.tcx, p.hir_id);
        let Some(rd) = self.raps.get(&name) else { continue };
        let hash = *rd.rap.hash();
        if !locals.contains(&hash) {
          locals.push(hash);
        }
        // A `for` loop's pattern, bound on its first line on every pass.
        if matches!(source, LoopSource::ForLoop) && span_to_line(&p.span, &self.tcx) == head && !head_cols.contains(&hash) {
          head_cols.push(hash);
        }
      }
      // The loops inside this one, whose dots it takes only if it has
      // none of its own.
      let nested: Vec<(usize, usize)> = found
        .iter()
        .filter(|(o, ..)| o.hir_id != l.hir_id && l.span.contains(o.span))
        .map(|&(_, h, e)| (h, e))
        .collect();
      self.approx_notes.push(ApproxNote::Loop { head, pass, head_cols, locals, nested, declares });
    }
  }
}
