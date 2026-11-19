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
use crate::svg_generator::hover_messages;
use rustc_hir::def::Res;
use rustc_hir::intravisit::{self, Visitor};
use rustc_hir::{
  ClosureKind, CoroutineDesugaring, CoroutineKind, Expr, ExprKind, LetStmt, LoopSource, Node, Pat, PatKind, QPath, UnOp,
};
use rustc_middle::ty::{GenericArgKind, TypeVisitableExt};

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

/// The expressions and `let`s of a body, in source order (not inside
/// closures).
struct Sites<'tcx> {
  exprs: Vec<&'tcx Expr<'tcx>>,
  lets: Vec<&'tcx LetStmt<'tcx>>,
}

impl<'tcx> Visitor<'tcx> for Sites<'tcx> {
  fn visit_expr(&mut self, e: &'tcx Expr<'tcx>) {
    self.exprs.push(e);
    intravisit::walk_expr(self, e);
  }
  fn visit_local(&mut self, l: &'tcx LetStmt<'tcx>) {
    self.lets.push(l);
    intravisit::walk_local(self, l);
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
    let mut sites = Sites { exprs: Vec::new(), lets: Vec::new() };
    sites.visit_expr(body.value);
    self.async_fn_notes(body);
    for e in &sites.exprs {
      if e.span.from_expansion() {
        continue;
      }
      self.runtime_lock_note(e);
      self.raw_deref_note(e);
      self.element_ref_deref_note(e);
      self.method_chain_note(e);
    }
    for l in &sites.lets {
      if l.span.from_expansion() {
        continue;
      }
      self.raw_cast_note(l);
      self.closure_let_note(l);
    }
  }

  /// `(hash, name)` of the column of the local variable `e` names.
  fn local_column(&self, e: &Expr) -> Option<(u64, String)> {
    let ExprKind::Path(QPath::Resolved(None, path)) = e.kind else { return None };
    let Res::Local(id) = path.res else { return None };
    self.column_of_binding(id)
  }

  /// `(hash, name)` of the column of the variable bound at `id`.
  fn column_of_binding(&self, id: rustc_hir::HirId) -> Option<(u64, String)> {
    let rd = self.raps.get(&binding_name(&self.tcx, id))?;
    if rd.rap.is_fn() {
      return None;
    }
    Some((*rd.rap.hash(), rd.rap.name().to_owned()))
  }

  /// `text` on the first dot of column `hash` on the lines of `span`.
  fn note_at(&mut self, hash: u64, span: rustc_span::Span, text: String) {
    let lines = (span_to_line(&span, &self.tcx), span_end_line(&span, &self.tcx));
    self.approx_notes.push(ApproxNote::At { hash, lines, text });
  }

  /// The source text of `e`, for a note that names a place.
  fn snippet(&self, e: &Expr) -> Option<String> {
    self.tcx.sess.source_map().span_to_snippet(e.span).ok()
  }

  /// `RefCell::borrow` / `borrow_mut`, `Mutex::lock`, `RwLock::read` /
  /// `write` (#685): at compile time a shared borrow of the receiver,
  /// which is what's drawn; the run-time borrow or lock lasts until the
  /// guard the call returns is dropped, and isn't drawn.
  fn runtime_lock_note(&mut self, e: &'tcx Expr<'tcx>) {
    let Some(text) = self.runtime_lock_text(e) else { return };
    let ExprKind::MethodCall(_, recv, ..) = e.kind else { return };
    let Some((hash, _)) = self.local_column(recv) else { return };
    self.note_at(hash, e.span, text);
  }

  /// The note of `runtime_lock_note` for the call `e`, if it is one of
  /// those methods.
  fn runtime_lock_text(&self, e: &'tcx Expr<'tcx>) -> Option<String> {
    let ExprKind::MethodCall(seg, ..) = e.kind else { return None };
    let typeck = self.tcx.typeck(e.hir_id.owner);
    let def_id = typeck.type_dependent_def_id(e.hir_id)?;
    let impl_id = self.tcx.impl_of_assoc(def_id)?;
    let adt = self.tcx.type_of(impl_id).skip_binder().ty_adt_def()?;
    let method = seg.ident.name.as_str();
    let is = |sym| self.tcx.is_diagnostic_item(sym, adt.did());
    match method {
      "borrow" | "borrow_mut" if is(rustc_span::sym::RefCell) => Some(hover_messages::approx_note_refcell(method)),
      "lock" if is(rustc_span::sym::Mutex) => Some(hover_messages::approx_note_lock("Mutex", method, None)),
      "read" | "write" if is(rustc_span::sym::RwLock) => Some(hover_messages::approx_note_lock("RwLock", method, Some(method == "read"))),
      _ => None,
    }
  }

  /// `*p` with `p` a raw pointer (#194): drawn on p's column only.
  fn raw_deref_note(&mut self, e: &'tcx Expr<'tcx>) {
    let ExprKind::Unary(UnOp::Deref, inner) = e.kind else { return };
    if !self.tcx.typeck(e.hir_id.owner).expr_ty(inner).is_raw_ptr() {
      return;
    }
    let Some((hash, name)) = self.local_column(inner) else { return };
    self.note_at(hash, e.span, hover_messages::approx_note_raw_deref(&name));
  }

  /// `let p = &mut x as *mut T;` (#194): no borrow of x is drawn.
  fn raw_cast_note(&mut self, l: &'tcx LetStmt<'tcx>) {
    let (PatKind::Binding(..), Some(init)) = (l.pat.kind, l.init) else { return };
    let typeck = self.tcx.typeck(l.hir_id.owner);
    if !typeck.expr_ty(init).is_raw_ptr() {
      return;
    }
    let mut inner = init;
    let mut cast = false;
    while let ExprKind::Cast(e, _) = inner.kind {
      inner = e;
      cast = true;
    }
    let ExprKind::AddrOf(_, _, place) = inner.kind else { return };
    let (true, Some((_, lender))) = (cast, self.local_column(place)) else { return };
    let Some((hash, name)) = self.column_of_binding(l.pat.hir_id) else { return };
    self.note_at(hash, l.span, hover_messages::approx_note_raw_cast(&name, &lender));
  }

  /// `*slots[0]` with `slots[0]` a reference stored in an element
  /// (#689): drawn on slots's column, not on what it borrows from.
  fn element_ref_deref_note(&mut self, e: &'tcx Expr<'tcx>) {
    let ExprKind::Unary(UnOp::Deref, place) = e.kind else { return };
    if !self.tcx.typeck(e.hir_id.owner).expr_ty(place).is_ref() {
      return;
    }
    let mut base = place;
    while let ExprKind::Field(b, _) = base.kind {
      base = b;
    }
    let ExprKind::Index(coll, ..) = base.kind else { return };
    let Some((hash, _)) = self.local_column(coll) else { return };
    let Some(text) = self.snippet(place) else { return };
    self.note_at(hash, e.span, hover_messages::approx_note_element_ref(&text));
  }

  /// `v.iter().map(f).collect()` (#687): the value each call after the
  /// first works on is a temporary with no column. Noted on the
  /// variable's dot at the first call, unless that value is drawn (a
  /// temporary column of its own) or is a plain Copy value with no
  /// reference in it (`s.len().max(1)`).
  fn method_chain_note(&mut self, e: &'tcx Expr<'tcx>) {
    let ExprKind::MethodCall(first, recv, ..) = e.kind else { return };
    let Some((hash, _)) = self.local_column(recv) else { return };
    // The later calls, each one's receiver the one before.
    let mut later = Vec::new();
    let mut cur = e;
    while let Node::Expr(p) = self.tcx.parent_hir_node(cur.hir_id) {
      match p.kind {
        ExprKind::MethodCall(seg, r, ..) if r.hir_id == cur.hir_id && !p.span.from_expansion() => {
          later.push(seg.ident.name.as_str().to_owned());
          cur = p;
        }
        _ => break,
      }
    }
    // (A guard's run-time borrow or lock has its own note.)
    if later.is_empty() || self.temp_drops_noted.contains(&e.hir_id) || self.runtime_lock_text(e).is_some() {
      return;
    }
    let ty = self.tcx.typeck(e.hir_id.owner).expr_ty(e);
    let has_ref = ty.walk().any(|a| matches!(a.kind(), GenericArgKind::Lifetime(_)));
    let env = rustc_middle::ty::TypingEnv::post_analysis(self.tcx, e.hir_id.owner.to_def_id());
    if !has_ref && !ty.has_param() && self.tcx.type_is_copy_modulo_regions(env, ty) {
      return;
    }
    let span = e.span.to(cur.span);
    self.note_at(hash, span, hover_messages::approx_note_method_chain(first.ident.name.as_str(), &later));
  }

  /// `let f = || …;` with captures (#133, #278, #347): what f captures
  /// is drawn; what its body does where f is called isn't (an `Fn` /
  /// `FnMut` closure's calls aren't drawn at all).
  /// `let fut = async { … };` (#135): the block's body isn't drawn.
  fn closure_let_note(&mut self, l: &'tcx LetStmt<'tcx>) {
    let (PatKind::Binding(..), Some(init)) = (l.pat.kind, l.init) else { return };
    let Some((hash, name)) = self.column_of_binding(l.pat.hir_id) else { return };
    let text = match init.kind {
      ExprKind::Closure(c) => match c.kind {
        // Not an FnOnce closure: its call is drawn, with what happens to
        // what it captured by value ("t … is dropped during the call").
        ClosureKind::Closure if !self.tcx.closure_captures(c.def_id).is_empty() && !self.is_fn_once(init) => hover_messages::approx_note_closure(&name),
        ClosureKind::Coroutine(CoroutineKind::Desugared(CoroutineDesugaring::Async, _)) => hover_messages::approx_note_async_block(&name),
        _ => return,
      },
      // `let fut = get(&s);` with `get` an async fn.
      ExprKind::Call(callee, _) => {
        let ExprKind::Path(ref qpath) = callee.kind else { return };
        let Res::Def(_, def_id) = self.tcx.typeck(callee.hir_id.owner).qpath_res(qpath, callee.hir_id) else { return };
        if !matches!(self.tcx.def_kind(def_id), rustc_hir::def::DefKind::Fn | rustc_hir::def::DefKind::AssocFn) || !self.tcx.asyncness(def_id).is_async() {
          return;
        }
        hover_messages::approx_note_async_call(&name, self.tcx.item_name(def_id).as_str())
      }
      _ => return,
    };
    self.note_at(hash, l.span, text);
  }

  /// Whether `e` is a closure rustc inferred to be `FnOnce`.
  fn is_fn_once(&self, e: &'tcx Expr<'tcx>) -> bool {
    match self.tcx.typeck(e.hir_id.owner).expr_ty(e).kind() {
      rustc_middle::ty::Closure(_, args) => args.as_closure().kind() == rustc_middle::ty::ClosureKind::FnOnce,
      _ => false,
    }
  }

  /// The parameters of an `async fn` (#135): its body isn't drawn.
  fn async_fn_notes(&mut self, body: &'tcx rustc_hir::Body<'tcx>) {
    let owner = body.value.hir_id.owner.def_id;
    if !matches!(self.tcx.def_kind(owner), rustc_hir::def::DefKind::Fn | rustc_hir::def::DefKind::AssocFn) || !self.tcx.asyncness(owner).is_async() {
      return;
    }
    let fn_name = self.tcx.item_name(owner.to_def_id()).as_str().to_owned();
    // (rustc gives an async fn's parameters spans of its desugaring.)
    let mut ids: Vec<rustc_hir::HirId> = Vec::new();
    for param in body.params {
      param.pat.walk_always(|p| if matches!(p.kind, PatKind::Binding(..)) { ids.push(p.hir_id) });
    }
    for id in ids {
      let Some((hash, _)) = self.column_of_binding(id) else { continue };
      let span = self.tcx.def_span(owner).to(body.value.span);
      self.note_at(hash, span, hover_messages::approx_note_async_fn(&fn_name));
    }
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
