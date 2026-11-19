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

  /// A loop is drawn as one pass through its body (#853).
  fn loop_notes(&mut self, body: &'tcx rustc_hir::Body<'tcx>) {
    let mut loops = Loops(Vec::new());
    loops.visit_expr(body.value);
    for l in loops.0 {
      let ExprKind::Loop(block, _, source, _) = l.kind else { continue };
      // A loop a macro wrote (not `for` / `while`, which rustc lowers
      // to `loop` with a desugaring mark) isn't the user's code.
      if l.span.from_expansion() && l.span.desugaring_kind().is_none() {
        continue;
      }
      let head = span_to_line(&l.span, &self.tcx);
      let end = span_end_line(&l.span, &self.tcx);
      // The lines that run as part of a pass: the body's, but not its
      // first line where a `for` loop's iterator is made or a `loop`'s
      // value is bound (`let n = loop {`), nor its last, where what
      // leaves the loop is drawn. A `while` loop's condition is
      // checked on every pass.
      let from = if matches!(source, LoopSource::While) { head } else { head + 1 };
      let body_lines = (from, end.saturating_sub(1));
      let mut bindings = Bindings(Vec::new());
      bindings.visit_block(block);
      let mut locals: Vec<(u64, String)> = Vec::new();
      for p in bindings.0 {
        let PatKind::Binding(..) = p.kind else { continue };
        let name = binding_name(&self.tcx, p.hir_id);
        if let Some(rd) = self.raps.get(&name) {
          let hash = *rd.rap.hash();
          if !locals.iter().any(|(h, _)| *h == hash) {
            locals.push((hash, rd.rap.name().to_owned()));
          }
        }
      }
      self.approx_notes.push(ApproxNote::Loop { head, end, body: body_lines, locals });
    }
  }
}
