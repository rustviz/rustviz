//! Approximation notes (`ApproxNote`): where a figure simplifies what a
//! construct does in a way a student could misread, the dot drawn on it
//! says so in one parenthetical sentence. MIR proves which operation
//! happens and which place it uses. HIR supplies the user-facing spelling
//! and the bridge between that compiler occurrence and its drawn event.

use crate::expr_visitor::ExprVisitor;
use crate::expr_visitor_utils::{span_end_line, span_to_line};
use crate::svg_generator::data::{ApproxNote, NoteEvent};
use crate::svg_generator::hover_messages;
use rustc_hir::def::Res;
use rustc_middle::mir::visit::{PlaceContext, Visitor as MirVisitor};
use rustc_middle::mir::{AggregateKind, Local, Location, Place, ProjectionElem, Rvalue, StatementKind, TerminatorKind};
use rustc_span::Span;
use rustc_hir::intravisit::{self, Visitor};
use rustc_hir::{
  ClosureKind, CoroutineDesugaring, CoroutineKind, Expr, ExprKind, LetStmt, LoopSource, Pat, PatKind, QPath, UnOp,
};

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
    let mut sites = Sites { exprs: Vec::new(), lets: Vec::new() };
    sites.visit_expr(body.value);
    self.loop_notes(&sites.exprs);
    self.async_fn_notes(body);
    for e in &sites.exprs {
      if e.span.from_expansion() {
        continue;
      }
      self.raw_deref_note(e);
      self.element_ref_deref_note(e);
    }
    for l in &sites.lets {
      if l.span.from_expansion() || l.init.is_some_and(|init| init.span.from_expansion()) {
        continue;
      }
      self.raw_cast_note(l);
      self.closure_let_note(l);
      self.branch_extended_temp_note(l);
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
    let rd = self.raps.binding(id)?;
    if rd.rap.is_fn() {
      return None;
    }
    Some((*rd.rap.hash(), rd.rap.name().to_owned()))
  }

  /// A projected source row, for notes on pictured bindings and writes.
  fn note_at_line(&mut self, hash: u64, span: Span, event: NoteEvent, text: String) {
    let line = span_to_line(&span, &self.tcx);
    self.approx_notes.push(ApproxNote::SourceRow { hash, line, event, text });
  }

  fn definition_span(&self, local: Local) -> Span {
    self.mir_body.local_decls[local].source_info.span
  }

  fn assignments(&self) -> impl Iterator<Item = (Location, Place<'tcx>, &Rvalue<'tcx>)> + '_ {
    self.mir_body.basic_blocks.iter_enumerated().flat_map(move |(block, bb)| {
      bb.statements.iter().enumerate().filter_map(move |(statement_index, statement)| {
        let StatementKind::Assign(a) = &statement.kind else { return None };
        let at = Location { block, statement_index };
        self.runs_at(at).then_some((at, a.0, &a.1))
      })
    })
  }

  /// The source text of `e`, for a note that names a place.
  fn snippet(&self, e: &Expr) -> Option<String> {
    self.tcx.sess.source_map().span_to_snippet(e.span).ok()
  }

  /// `RefCell::borrow` / `borrow_mut`, `Mutex::lock`, `RwLock::read` /
  /// `write` (#685): at compile time a shared borrow of the receiver,
  /// which is what's drawn; the run-time borrow or lock lasts until the
  /// guard the call returns is dropped, and isn't drawn.
  pub(crate) fn runtime_lock_note(&mut self, e: &Expr<'tcx>, id: usize) {
    let ExprKind::MethodCall(_, recv, ..) = e.kind else { return };
    let Some((hash, _)) = self.local_column(recv) else { return };
    let Some(def_id) = self.tcx.typeck(e.hir_id.owner).type_dependent_def_id(e.hir_id) else { return };
    let Some(text) = self.runtime_lock_text(def_id) else { return };
    // The receiver event is emitted for the resolved method, rather
    // than for any compiler-inserted deref call with the same span.
    let Some(site) = self.mir_call(e).and_then(|call| call.site) else { return };
    if !e.span.from_expansion() && self.runs_at(site.at) {
      self.approx_notes.push(ApproxNote::Operation { hash, id, text });
    }
  }

  fn binding_local(&self, expr: &Expr<'_>) -> Option<Local> {
    let ExprKind::Path(QPath::Resolved(None, path)) = expr.kind else { return None };
    let Res::Local(id) = path.res else { return None };
    self.raps.bindings.declarations.get(&id).copied()
  }

  fn runs_at(&self, at: Location) -> bool {
    let span = self.mir_body.source_info(at).span.with_parent(None);
    self.body_facts.and_then(|b| b.effect_sites.get(&span))
      .is_some_and(|sites| sites.contains(&(at.block, true)))
  }

  /// Runtime behavior of a resolved standard-library guard acquisition.
  fn runtime_lock_text(&self, def_id: rustc_hir::def_id::DefId) -> Option<String> {
    let impl_id = self.tcx.impl_of_assoc(def_id)?;
    let adt = self.tcx.type_of(impl_id).skip_binder().ty_adt_def()?;
    let method = self.tcx.item_name(def_id);
    let method = method.as_str();
    let is = |sym| self.tcx.is_diagnostic_item(sym, adt.did());
    match method {
      "borrow" | "borrow_mut" if is(rustc_span::sym::RefCell) => Some(hover_messages::approx_note_refcell(method)),
      "lock" if is(rustc_span::sym::Mutex) => Some(hover_messages::approx_note_lock("Mutex", method, None)),
      "read" | "write" if is(rustc_span::sym::RwLock) => Some(hover_messages::approx_note_lock("RwLock", method, Some(method == "read"))),
      _ => None,
    }
  }

  /// When an extended temporary lacks a column, explain the drop on
  /// its proven holder's normal scope end.
  fn branch_extended_temp_note(&mut self, l: &'tcx LetStmt<'tcx>) {
    let Some(init) = l.init else { return };
    let Some(facts) = self.body_facts else { return };
    let mut bindings = Bindings(Vec::new());
    bindings.visit_pat(l.pat);
    for (expr, borrow, end) in self.branch_extended_temps(init) {
      if self.branch_temp_names.get(&expr.hir_id).is_some_and(|n| self.raps.contains_key(n)) {
        continue;
      }
      let Some(temp) = facts.expression_temp(expr.hir_id) else { continue };
      if !temp.root_flags.contains_key(&None) {
        continue;
      }
      for p in &bindings.0 {
        let Some(&holder) = self.raps.bindings.declarations.get(&p.hir_id) else { continue };
        if !facts.loans.iter().any(|loan| {
          loan.runs && loan.lender_local == Some(temp.local) && loan.direct_locals.contains(&holder)
        }) {
          continue;
        }
        let Some((hash, name)) = self.column_of_binding(p.hir_id) else { continue };
        let line = span_to_line(&borrow.span, &self.tcx);
        let code = self.snippet(borrow).filter(|t| t.chars().count() <= 32 && !t.contains('\n'));
        let text = hover_messages::approx_note_branch_extended_temp(&name, code.as_deref(), line);
        self.approx_notes.push(ApproxNote::SourceRow { hash, line: end, event: NoteEvent::ScopeEnd, text });
      }
    }
  }

  /// Rustc's remainder scope selects the extending let. Its MIR branches identify paths that bypass creation. Source order here only numbers the pictured columns.
  pub(crate) fn branch_extended_temps(&self, init: &Expr<'_>) -> Vec<(&'tcx Expr<'tcx>, &'tcx Expr<'tcx>, usize)> {
    let mut temps: Vec<_> = self.body_facts.into_iter().flat_map(|b| &b.temps).filter_map(|t| {
      let ext = t.extension.as_ref()?;
      if !t.conditional || ext.initializer != init.hir_id { return None; }
      Some((self.tcx.hir_expect_expr(t.expression?), self.tcx.hir_expect_expr(ext.borrow?), ext.end))
    }).collect();
    temps.sort_by_key(|(_, b, _)| b.span);
    temps
  }

  /// `*p` with `p` a raw pointer (#194): drawn on p's column only.
  fn raw_deref_note(&mut self, e: &'tcx Expr<'tcx>) {
    let ExprKind::Unary(UnOp::Deref, inner) = e.kind else { return };
    let Some((hash, name)) = self.local_column(inner) else { return };
    let Some(local) = self.binding_local(inner) else { return };
    if !self.mir_body.local_decls[local].ty.is_raw_ptr() {
      return;
    }
    let Some(at) = self.deref_site(e.span, |p| {
      p.local == local && p.projection.first() == Some(&ProjectionElem::Deref)
    }) else { return };
    self.note_at_line(hash, self.mir_body.source_info(at).span, NoteEvent::Access, hover_messages::approx_note_raw_deref(&name));
  }

  /// `let p = &mut x as *mut T;` (#194). The place behind the
  /// conversion must be the reaching reference borrow's actual place.
  fn raw_cast_note(&mut self, l: &'tcx LetStmt<'tcx>) {
    let (PatKind::Binding(..), Some(init)) = (l.pat.kind, l.init) else { return };
    let Some((hash, name)) = self.column_of_binding(l.pat.hir_id) else { return };
    let Some(&local) = self.raps.bindings.declarations.get(&l.pat.hir_id) else { return };
    if !matches!(init.kind, ExprKind::Cast(..)) || !self.mir_body.local_decls[local].ty.is_raw_ptr() {
      return;
    }
    let mut origins = self.assignments().filter(|(at, dest, _)| {
      dest.as_local() == Some(local) && init.span.contains(self.mir_body.source_info(*at).span)
    });
    let Some((at, _, rhs)) = origins.next() else { return };
    if origins.next().is_some() {
      return;
    }
    let origin = match rhs {
      Rvalue::RawPtr(_, place) => Some((at, *place)),
      Rvalue::Cast(rustc_middle::mir::CastKind::PtrToPtr, operand, _) => operand.place().and_then(|p| {
        crate::facts::call_wording::raw_pointer_origin(self.mir_body, at, p)
      }),
      _ => None,
    };
    let Some((raw_at, place)) = origin else { return };
    if place.projection.as_slice() != [ProjectionElem::Deref] {
      return;
    }
    let Some((_, lender)) = crate::facts::call_wording::operand_borrow(self.mir_body, raw_at, Place::from(place.local)) else { return };
    let Some(lender) = lender.as_local().and_then(|local| self.raps.bindings.locals.get(&local)) else { return };
    let text = hover_messages::approx_note_raw_cast(&name, lender);
    drop(origins);
    self.note_at_line(hash, self.definition_span(local), NoteEvent::Acquire, text);
  }

  /// Finds an assignment with a place access for this source expression.
  /// Rustc can give a write the containing assignment's span.
  fn deref_site(&self, span: Span, accepts: impl Fn(Place<'tcx>) -> bool) -> Option<Location> {
    struct Access<F> {
      accepts: F,
      found: bool,
    }
    impl<'tcx, F: Fn(Place<'tcx>) -> bool> MirVisitor<'tcx> for Access<F> {
      fn visit_place(&mut self, place: &Place<'tcx>, _: PlaceContext, _: Location) {
        self.found |= (self.accepts)(*place);
      }
    }
    self.assignments().find_map(|(at, dest, rhs)| {
      let site = self.mir_body.source_info(at).span;
      if !site.contains(span) && !span.contains(site) {
        return None;
      }
      let mut access = Access { accepts: &accepts, found: accepts(dest) };
      access.visit_rvalue(rhs, at);
      access.found.then_some(at)
    })
  }

  /// `*slots[0]` with `slots[0]` a reference stored in an element
  /// (#689): drawn on slots's column.
  fn element_ref_deref_note(&mut self, e: &'tcx Expr<'tcx>) {
    let ExprKind::Unary(UnOp::Deref, place) = e.kind else { return };
    if !self.tcx.typeck(e.hir_id.owner).expr_ty(place).is_ref() {
      return;
    }
    let mut base = place;
    while let ExprKind::Field(b, _) = base.kind {
      base = b;
    }
    let ExprKind::Index(coll, _, brackets) = base.kind else { return };
    let Some((hash, _)) = self.local_column(coll) else { return };
    let Some(local) = self.binding_local(coll) else { return };
    let site = if let Some(method) = self.tcx.typeck(base.hir_id.owner).type_dependent_def_id(base.hir_id) {
      // In overloaded indexing the call yields a reference to the
      // selected element. The explicit dereference of that element can
      // be lowered into a macro's argument borrow through another temp.
      self.index_call(brackets, method)
    } else {
      self.deref_site(e.span, |p| {
        p.local == local && p.projection.split_last().is_some_and(|(last, prefix)| {
          *last == ProjectionElem::Deref && prefix.iter().any(|part| {
            matches!(part, ProjectionElem::Index(_) | ProjectionElem::ConstantIndex { .. })
          })
        })
      })
    };
    let Some(at) = site else { return };
    let Some(text) = self.snippet(place) else { return };
    self.note_at_line(hash, self.mir_body.source_info(at).span, NoteEvent::Access, hover_messages::approx_note_element_ref(&text));
  }

  /// The call selecting the element of the resolved receiver.
  fn index_call(&self, brackets: Span, method: rustc_hir::def_id::DefId) -> Option<Location> {
    // THIR gives this call the brackets' span. The full index expression
    // also contains any nested index calls used to calculate its operand.
    let mut calls = self.mir_body.basic_blocks.iter_enumerated().filter_map(|(bb, data)| {
      let term = data.terminator();
      let TerminatorKind::Call { func, .. } = &term.kind else { return None };
      let at = self.mir_body.terminator_loc(bb);
      (brackets.source_equal(term.source_info.span) && func.const_fn_def()?.0 == method && self.runs_at(at)).then_some(at)
    });
    let at = calls.next()?;
    calls.next().is_none().then_some(at)
  }

  /// Captures may have a picture while uses in the closure or future
  /// have none. The note belongs to the stored closure or call result.
  fn closure_let_note(&mut self, l: &'tcx LetStmt<'tcx>) {
    let (PatKind::Binding(..), Some(init)) = (l.pat.kind, l.init) else { return };
    let Some((hash, name)) = self.column_of_binding(l.pat.hir_id) else { return };
    let Some(&local) = self.raps.bindings.declarations.get(&l.pat.hir_id) else { return };
    let (event, text) = match init.kind {
      ExprKind::Closure(c) => {
        let stored = self.assignments().any(|(_, dest, rhs)| {
          dest.as_local() == Some(local) && matches!(rhs, Rvalue::Aggregate(kind, _)
            if matches!(**kind, AggregateKind::Closure(id, _) | AggregateKind::Coroutine(id, _)
              if id == c.def_id.to_def_id()))
        });
        if !stored { return; }
        let text = match c.kind {
          ClosureKind::Closure if !self.tcx.closure_captures(c.def_id).is_empty() && !self.is_fn_once(init) => hover_messages::approx_note_closure(&name),
          ClosureKind::Coroutine(CoroutineKind::Desugared(CoroutineDesugaring::Async, _)) => hover_messages::approx_note_async_block(&name),
          _ => return,
        };
        (NoteEvent::Initialize, text)
      }
      ExprKind::Call(..) | ExprKind::MethodCall(..) => {
        let Some(def_id) = self.mir_body.basic_blocks.iter_enumerated().find_map(|(bb, data)| {
          let term = data.terminator();
          let TerminatorKind::Call { func, destination, .. } = &term.kind else { return None };
          if destination.as_local() != Some(local) || !init.span.source_equal(term.source_info.span)
            || !self.runs_at(self.mir_body.terminator_loc(bb))
          {
            return None;
          }
          let (id, _) = func.const_fn_def()?;
          self.tcx.asyncness(id).is_async().then_some(id)
        }) else { return };
        (NoteEvent::Initialize, hover_messages::approx_note_async_call(&name, self.tcx.item_name(def_id).as_str()))
      }
      _ => return,
    };
    self.note_at_line(hash, self.definition_span(local), event, text);
  }

  /// Whether `e` is a closure rustc inferred to be `FnOnce`.
  fn is_fn_once(&self, e: &Expr<'_>) -> bool {
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
    // The outer MIR takes these arguments and constructs a coroutine.
    // In particular, the named bindings in the source pattern need not
    // be the locals used inside the desugared coroutine body.
    for (param, local) in body.params.iter().zip(self.mir_body.args_iter()) {
      let span = self.definition_span(local);
      param.pat.walk_always(|p| {
        if let PatKind::Binding(..) = p.kind {
          if let Some((hash, _)) = self.column_of_binding(p.hir_id) {
            self.note_at_line(hash, span, NoteEvent::Parameter, hover_messages::approx_note_async_fn(&fn_name));
          }
        }
      });
    }
  }

  /// A loop is drawn as one pass through its body (#853). One note per
  /// loop, on the first dot of a pass (see `ApproxNote::Loop`).
  fn loop_notes(&mut self, exprs: &[&'tcx Expr<'tcx>]) {
    // Each loop the user wrote, with its lines.
    let mut found: Vec<(&'tcx Expr<'tcx>, usize, usize)> = Vec::new();
    for &l in exprs {
      if !matches!(l.kind, ExprKind::Loop(..)) {
        continue;
      }
      // A loop a macro wrote (not `for` / `while`, which rustc lowers
      // to `loop` with a desugaring mark) isn't the user's code.
      if l.span.from_expansion() && l.span.desugaring_kind().is_none() {
        continue;
      }
      let Some(boundary) = self.body_facts.and_then(|f| f.loops.get(&l.hir_id)) else { continue };
      found.push((l, boundary.row, span_end_line(&l.span, &self.tcx)));
    }
    for &(l, head, end) in &found {
      let ExprKind::Loop(block, _, source, _) = l.kind else { continue };
      // One that never comes back round (#1467): a bare `loop` runs its
      // body once, as drawn; a `while` / `for` once or not at all.
      let at_most_once = self.loop_never_rounds(l.hir_id) == Some(true);
      // An irrefutable while-let has no pattern-failure path on which
      // to skip the body. Its scrutinee can still exit during evaluation.
      let always_matches = block.expr.is_some_and(|e| matches!(e.kind, ExprKind::If(cond, ..)
        if matches!(cond.kind, ExprKind::Let(binding) if crate::visitor::pat_is_irrefutable(binding.pat, self.tcx.typeck(binding.pat.hir_id.owner))
          || self.facts.is_some_and(|facts| crate::facts::loop_enters_pattern(self.tcx, &self.bwf.body, facts, l.span, binding.pat.span)))));
      if at_most_once && (matches!(source, LoopSource::Loop) || always_matches) {
        continue;
      }
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
      let storage = if matches!(source, LoopSource::ForLoop) {
        self.tcx.hir_parent_iter(l.hir_id).find_map(|(_, node)| {
          let rustc_hir::Node::Expr(parent) = node else { return None };
          let ExprKind::Match(constructor, [arm], rustc_hir::MatchSource::ForLoopDesugar) = parent.kind else { return None };
          (arm.body.hir_id == l.hir_id).then_some(constructor)
        }).and_then(|constructor| self.iterator_storage(constructor))
      } else {
        None
      };
      for p in &bindings.0 {
        let Some(rd) = self.raps.binding(p.hir_id) else { continue };
        let hash = *rd.rap.hash();
        if !locals.contains(&hash) {
          locals.push(hash);
        }
        // A `for` loop's pattern, bound on its first line on every pass.
        if self.raps.bindings.declarations.get(&p.hir_id).is_some_and(|local| {
            storage.is_some_and(|it| it.bindings.iter().any(|b| b.local == *local))
          })
          && !head_cols.contains(&hash)
        {
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
      let constant_pruned_backedge = self.facts.and_then(|f| f.loops.get(&l.hir_id))
        .is_some_and(|boundary| boundary.constant_pruned_backedge);
      let text = hover_messages::approx_note_loop(declares, at_most_once, constant_pruned_backedge);
      self.approx_notes.push(ApproxNote::Loop { head, pass, head_cols, locals, nested, text });
    }
  }
}
