//! Most of the work happens here, if you want to learn more about the visitor then look here:  
//! https://doc.rust-lang.org/beta/nightly-rustc/rustc_hir/intravisit/trait.Visitor.html
//! Essentially we recursively traverse (walk) the HIR, visiting statements, expressions, etc
//! See ExprKind at : https://doc.rust-lang.org/stable/nightly-rustc/rustc_hir/hir/enum.ExprKind.html
//! See StmtKind at : https://doc.rust-lang.org/stable/nightly-rustc/rustc_hir/hir/enum.StmtKind.html
//! See tcx: https://doc.rust-lang.org/nightly/nightly-rustc/rustc_middle/ty/struct.TyCtxt.html

use log::{info, warn};
use rustc_hir::{StmtKind, Stmt, Expr, ExprKind, PathSegment, UnOp, Param, QPath, Pat, PatKind, BindingMode, HirId, Mutability, LetStmt, def::*};
use rustc_hir::intravisit::{self, Visitor};
use crate::expr_visitor::*;
use crate::expr_visitor_utils::*;
use crate::svg_generator::data::*;
use rustc_middle::ty::adjustment::*;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};


impl<'a, 'tcx> ExprVisitor<'a, 'tcx> {
  /// `e` is the value the function returns (its tail expression, or
  /// the operand of a `return`): record it moving (or, for a Copy
  /// type or a shared reference, being copied) to the caller.
  fn return_to_caller(&mut self, e: &'tcx Expr<'tcx>) {
    let tycheck_results = self.tcx.typeck(e.hir_id.owner);
    let lhs_ty = tycheck_results.node_type(e.hir_id);
    let is_copyable = self.tcx.type_is_copy_modulo_regions(rustc_middle::ty::TypingEnv::post_analysis(self.tcx, e.hir_id.owner), lhs_ty);
    let evt = if lhs_ty.is_ref() {
      match lhs_ty.ref_mutability().unwrap() {
        Mutability::Not => Evt::Copy,
        Mutability::Mut => Evt::Move,
      }
    } else {
      match is_copyable {
        true => Evt::Copy,
        false => Evt::Move
      }
    };

    let to_ro = ResourceTy::Caller;
    let from_ro = match fetch_rap(e, &self.tcx, &self.raps) {
      // A unit struct / enum constructor (`Marker`) or a fn item
      // named as the returned value has no column of its own, and
      // neither has the caller: an arrow between the two can't be
      // drawn (it used to panic in the renderer, #136). Treat the
      // value like a literal.
      Some(r) if r.is_fn() => ResourceTy::Anonymous,
      Some(r) => ResourceTy::Value(r), // todo, technically need to check for deref here
      // `self.n` / `p.x` returned from behind a reference: a copy of
      // the field (#266). Not `&self.name`, which returns a reference
      // rather than a copy.
      None => self.field_through_ref(e).unwrap_or(ResourceTy::Anonymous),
    };

    let line_num = expr_to_line(e, &self.tcx);
    if (matches!(e.kind, ExprKind::Struct(..) | ExprKind::Tup(..) | ExprKind::Array(..)) && !e.span.from_expansion())
      || vec_macro_elems(e).is_some() {
      // `User { email, .. }` / `(s, 1)` / `[s, t]` as the returned value: the
      // literal's field initialisers move/copy their sources into
      // the returned value.
      self.match_rhs(to_ro, e, evt);
    } else {
      self.add_ev(line_num, evt, to_ro, from_ro, false);
    }
  }

  /// Visit a match-arm guard. `if let` parts register their bindings
  /// into `guard_decls` (the caller scopes them to the arm); `&&`
  /// chains are walked part by part; anything else is visited as an
  /// ordinary expression.
  ///
  /// Every event a guard produces is drawn in its arm's column, which
  /// is only accurate for effects that end inside the guard: when the
  /// guard is false, matching continues with later arms in the state
  /// the guard left behind. So a guard that moves out of, or assigns
  /// to, a variable that outlives it (`x if consume(s) =>`) is
  /// refused instead of drawn. The one exception is the bindings of a
  /// guard that is a lone `if let`: those happen only when the whole
  /// guard succeeds, i.e. exactly when the arm is taken.
  fn visit_match_guard(
    &mut self,
    expr: &'tcx Expr<'tcx>,
    whole_guard: bool,
    outer: &HashSet<ResourceAccessPoint>,
    guard_decls: &mut HashSet<ResourceAccessPoint>,
  ) {
    match expr.kind {
      ExprKind::DropTemps(inner) => self.visit_match_guard(inner, whole_guard, outer, guard_decls),
      ExprKind::Binary(op, lhs, rhs) if op.node == rustc_hir::BinOpKind::And => {
        self.visit_match_guard(lhs, false, outer, guard_decls);
        self.visit_match_guard(rhs, false, outer, guard_decls);
      }
      ExprKind::Let(let_expr) => {
        let mark = self.preprocessed_events.len();
        self.visit_expr(let_expr.init);
        self.refuse_lasting_guard_effects(mark, outer, expr);
        let mark = self.preprocessed_events.len();
        let bound = self.register_iflet_let_bindings(let_expr, None);
        if !whole_guard {
          self.refuse_lasting_guard_effects(mark, outer, expr);
        }
        guard_decls.extend(bound);
      }
      _ => {
        let mark = self.preprocessed_events.len();
        self.visit_expr(expr);
        self.refuse_lasting_guard_effects(mark, outer, expr);
      }
    }
  }

  /// Panic (reported as "can't visualize this yet") if any event from
  /// `preprocessed_events[mark..]` moves out of, or assigns to, a
  /// variable in `outer`. See `visit_match_guard`.
  fn refuse_lasting_guard_effects(
    &self,
    mark: usize,
    outer: &HashSet<ResourceAccessPoint>,
    guard_part: &'tcx Expr<'tcx>,
  ) {
    let is_outer = |r: &ResourceTy| r.extract_rap().is_some_and(|rap| outer.contains(rap));
    for (_, ev) in &self.preprocessed_events[mark..] {
      let culprit = match ev {
        ExternalEvent::Move { from, .. } if is_outer(from) => from,
        ExternalEvent::ClosureCall { closure, .. } if is_outer(closure) => closure,
        ExternalEvent::Bind { to, .. }
        | ExternalEvent::Copy { to, .. }
        | ExternalEvent::Move { to, .. }
        | ExternalEvent::StaticBorrow { to, .. }
        | ExternalEvent::MutableBorrow { to, .. } if is_outer(to) => to,
        ExternalEvent::OwnerDropAtReassign { ro, .. } if outer.contains(ro) => {
          panic!("match guards that assign to an outer variable (`{}`, line {}) aren't supported yet",
            ro.name(), expr_to_line(guard_part, &self.tcx));
        }
        _ => continue,
      };
      panic!("match guards that move or assign a variable declared outside the guard (`{}`, line {}) \
              aren't supported yet: the effect lasts even when the guard is false and a later arm runs",
        culprit.real_name(), expr_to_line(guard_part, &self.tcx));
    }
  }

  /// What a method call does to its receiver (`self`): a borrow
  /// (`&self` / `&mut self`), or a move / copy (`self` by value).
  /// Emitted only when the receiver is a place with a column.
  ///
  /// In a chain such as `v.iter().map(f).collect()`, only `iter` acts
  /// on `v`. `map` takes the iterator that `iter` returned (a
  /// temporary) and `collect` takes the `Map` that `map` returned, so
  /// neither borrows nor moves `v`, and neither gets an event: the
  /// temporaries have no column (#132). Likewise in
  /// `s.clone().into_bytes()` it is the clone that is moved, not `s`.
  /// The same holds for any receiver we can't name (`f().push(..)`,
  /// `self.items.push(..)` when `self.items` has no column).
  fn method_receiver_events(
    &mut self,
    expr: &'tcx Expr<'tcx>,
    name_and_generic_args: &'tcx PathSegment<'tcx>,
    rcvr: &'tcx Expr<'tcx>,
    fn_name: &String,
    line_num: usize,
  ) {
    if let ExprKind::MethodCall(..) = rcvr.kind {
      return;
    }
    // The receiver of `r.s.method()` is a Field expression, not
    // a bare path, so `hirid_to_var_name` can't name it.
    // `expr_to_rap_name` walks Path / Field / AddrOf chains and
    // returns the qualified name we registered the receiver
    // under (e.g. `r.s`).
    let Some(rcvr_name) = expr_to_rap_name(rcvr, &self.tcx) else { return };
    // `rcvr_rty` is what the call reads: the receiver's own RAP, or
    // for `p.name.len()` / `self.n.abs()` with p / self a reference,
    // the field through it, drawn on p's column (#266).
    let (rcvr_rap, rcvr_rty) = match self.raps.get(&rcvr_name) {
      Some(rd) => (rd.rap.to_owned(), ResourceTy::Value(rd.rap.to_owned())),
      None => match self.field_access_through_ref(rcvr) {
        Some(rty @ ResourceTy::DerefField(..)) => (rty.deref_rap().unwrap().clone(), rty),
        _ => return,
      },
    };
    self.update_rap(&rcvr_rap, line_num);
    let fn_rap = self.raps.get(&fn_key(fn_name)).unwrap().rap.to_owned();
    // typecheck
    // Annotate passByRef event, can check the type of borrow by looking at adjusments (usually borrows that are not explicit)
    // for example, in rust you don't have to dereference a reference to access members
    // https://doc.rust-lang.org/nightly/nightly-rustc/rustc_middle/ty/adjustment/struct.Adjustment.html
    let adjustment_map = self.tcx.typeck(name_and_generic_args.hir_id.owner).adjustments();
    // `s.len()` borrows `s` without a written `&` (rustc inserts
    // it); `r.len()` with `r: &String` and `(&s).len()` don't.
    let autoref = if is_method_autoref(rcvr, self.tcx.typeck(rcvr.hir_id.owner)) {
      ImplicitRef::Autoref
    } else {
      ImplicitRef::No
    };
    match adjustment_map.get(rcvr.hir_id) {
      Some(adj_vec) => { 
        for a in adj_vec.iter() {
          match a.kind {
            Adjust::Borrow(AutoBorrow::Ref(m)) => {
              match m {
                // `r.a.push_str(..)` with r a `&mut S`: a mutable
                // reborrow of the field through r (#202).
                AutoBorrowMutability::Mut{..} if matches!(rcvr_rty, ResourceTy::DerefField(..)) => {
                  let typeck = self.tcx.typeck(name_and_generic_args.hir_id.owner);
                  let result_has_lifetime = call_result_has_lifetime(expr, typeck);
                  self.add_pass_by_mref_ev(line_num, ResourceTy::Value(fn_rap.clone()), rcvr_rty.clone(), true, result_has_lifetime, autoref);
                }
                AutoBorrowMutability::Mut{allow_two_phase_borrow: AllowTwoPhase::Yes} => {
                  // `r.push_str(..)` with r: `&mut String` is
                  // `String::push_str(&mut *r, ..)` — an implicit
                  // reborrow of r. With r an owner it's a plain
                  // autoref (`&mut s`), not a reborrow. A written
                  // `(&mut s).push_str(..)` is a fresh borrow of s
                  // (rustc reborrows the temporary `&mut s`, not
                  // s), and `(&mut *r).push_str(..)` a reborrow of
                  // r; decide by the `&mut` operand, as match_arg
                  // does for `f(&mut s)` (#367).
                  let typeck = self.tcx.typeck(name_and_generic_args.hir_id.owner);
                  // `rv[0].push(..)` / `p.name.push(..)` with rv / p
                  // a `&mut`: the autoref'd place is reached through
                  // the reference, so this reborrows it too (#407).
                  let is_reborrow = match rcvr.kind {
                    ExprKind::AddrOf(_, _, operand) => is_explicit_reborrow(operand, typeck),
                    _ => is_implicit_mut_reborrow(rcvr, typeck) || is_explicit_reborrow(rcvr, typeck),
                  };
                  let result_has_lifetime = call_result_has_lifetime(expr, typeck);
                  self.add_pass_by_mref_ev(line_num, ResourceTy::Value(fn_rap.clone()), ResourceTy::Value(rcvr_rap.clone()), is_reborrow, result_has_lifetime, autoref);
                },
                AutoBorrowMutability::Not => {
                  let refcell_borrow_mut = self.is_refcell_borrow_mut_call(expr);
                  self.add_pass_by_sref_ev(line_num, ResourceTy::Value(fn_rap.clone()), rcvr_rty.clone(), autoref, refcell_borrow_mut);
                }
                _ => {}
              }
            }
            _ => {}
          }
        }
      }
      None => {}
    }

    // `self` taken by value (`v.into_iter()`, `opt.expect(..)`,
    // `fn into_name(self)`): the receiver is moved into the call
    // (or copied, if its type is Copy). The autoref branch above
    // only covers `&self` / `&mut self`; without this, a consumed
    // receiver looked untouched and got a false "Its resource is
    // dropped." at scope end (issue #172).
    //
    // The adjusted receiver type is exactly what the callee's
    // `self` parameter receives (the method signature instantiated
    // at this call), so it is a reference iff the method takes
    // `&self` / `&mut self`, or `self` where Self is itself a
    // reference (`(&v).into_iter()`); neither moves the receiver.
    let typeck = self.tcx.typeck(expr.hir_id.owner);
    let self_ty = typeck.expr_ty_adjusted(rcvr);
    if !self_ty.is_ref() && !self_ty.is_raw_ptr() {
      let is_copy = self.tcx.type_is_copy_modulo_regions(
        rustc_middle::ty::TypingEnv::post_analysis(self.tcx, expr.hir_id.owner), self_ty);
      // A Copy value reached through auto-deref (`r.by_value()`
      // with `r: &T`, `T: Copy`) is a copy of `*r`, not of `r`.
      let from = if matches!(rcvr_rty, ResourceTy::DerefField(..)) {
        rcvr_rty.clone()
      } else if typeck.expr_ty(rcvr).is_ref() {
        ResourceTy::Deref(rcvr_rap.clone())
      } else {
        ResourceTy::Value(rcvr_rap.clone())
      };
      let evt = if is_copy { Evt::Copy } else { Evt::Move };
      self.add_ev(line_num, evt, ResourceTy::Value(fn_rap.clone()), from, false);
    }
  }

  /// Walk through synthetic macro-expansion scaffolding to find any
  /// user-written subexpressions and dispatch them back through
  /// `visit_expr`. Called from the `from_expansion` guard at the top
  /// of `visit_expr` (see comment there for why).
  ///
  /// Descended through:
  /// - "transparent container" shapes (Block, Call, MethodCall,
  ///   AddrOf, Unary, DropTemps, Array, Tup) — these are the wrappers
  ///   `println!` / `format_args!` build around their args.
  /// - the *condition* of synthetic If / Match — `assert!(cond)`
  ///   lowers to a `match cond { true => {}, _ => panic!(…) }` whose
  ///   scrutinee is the user's `cond`, including any function calls
  ///   inside it. Without descending the scrutinee we'd miss the
  ///   call-site PassByRef events for `assert!(f(&x))` shapes (the
  ///   user-visible symptom: no `f` icon on the timeline at the
  ///   assert line).
  ///
  /// Not descended through: arm bodies, loop bodies, closure bodies.
  /// Those are where macro internals (the synthetic panic arm of
  /// assert!, the closure of `?`'s ControlFlow continuation, etc.)
  /// live, and surfacing their events would pollute the diagram with
  /// branches the user didn't write.
  fn descend_through_expansion(&mut self, expr: &'tcx Expr<'tcx>) {
    if !expr.span.from_expansion() {
      // Re-enter normal dispatch — the user wrote this subexpression.
      self.visit_expr(expr);
      return;
    }
    // A node the user's own `macro_rules!` body produced (as opposed to
    // one a std macro it forwards to produced): if it uses a variable
    // the caller passed in, what it does to that variable (a move into
    // `drop($e)`, a `let t = $e;`, an assignment, ...) isn't modelled,
    // and drawing nothing would make later tooltips false (#342).
    // Blocks and `DropTemps` are just containers; their statements are
    // checked one by one below.
    if !matches!(expr.kind, ExprKind::Block(..) | ExprKind::DropTemps(_)) {
      self.refuse_user_macro_use(expr.span, |v| v.visit_expr(expr));
    }
    match expr.kind {
      ExprKind::Block(b, _) => {
        for s in b.stmts {
          match s.kind {
            StmtKind::Let(l) => {
              if let Some(init) = l.init {
                if l.span.from_expansion() {
                  self.refuse_user_macro_use(l.span, |v| v.visit_expr(init));
                }
                self.descend_through_expansion(init);
              }
            }
            StmtKind::Expr(e) | StmtKind::Semi(e) => {
              self.descend_through_expansion(e);
            }
            StmtKind::Item(_) => {}
          }
        }
        if let Some(e) = b.expr {
          self.descend_through_expansion(e);
        }
      }
      ExprKind::Call(_, args) => {
        for a in args {
          self.descend_through_expansion(a);
        }
      }
      ExprKind::MethodCall(_, recv, args, _) => {
        // `write!(dst, …)` is `dst.write_fmt(format_args!(…))`, which
        // borrows `dst` mutably for the call.
        if !recv.span.from_expansion()
          && matches!(innermost_macro(expr.span).as_deref(), Some("write" | "writeln"))
        {
          self.visit_expr(recv);
          if let (Some(from), Some(name)) =
            (place_rap(recv, &self.tcx, &self.raps), user_macro_name(expr.span))
          {
            self.add_fn(name.clone());
            let to = ResourceTy::Value(self.raps.get(&fn_key(&name)).unwrap().rap.to_owned());
            let line_num = expr_to_line(recv, &self.tcx);
            let id = *self.unique_id;
            self.add_external_event(line_num, ExternalEvent::PassByMutableReference {
              from, to, id, is_reborrow: false, result_has_lifetime: false, implicit: ImplicitRef::MacroArg,
              result_keeps_borrow: false, stored_in: StoredIn::default(),
            });
            *self.unique_id += 1;
          }
        } else {
          self.descend_through_expansion(recv);
        }
        for a in args {
          self.descend_through_expansion(a);
        }
      }
      // `format_args!` (behind `println!`, `format!`, `write!`,
      // `panic!`, …) and `assert_eq!` / `assert_ne!` take each
      // argument by shared reference: the expansion is
      // `super let args = (&a, &b);` / `match (&left, &right)`, where
      // the `&` is synthetic and `a` is the user's expression. Record
      // that borrow as a read by the user-facing macro (#177).
      ExprKind::AddrOf(rustc_hir::BorrowKind::Ref, Mutability::Not, inner)
        if !inner.span.from_expansion() && borrows_its_args(expr.span) =>
      {
        self.visit_expr(inner);
        if let Some(name) = user_macro_name(expr.span) {
          self.macro_reads_arg(inner, name);
        }
      }
      ExprKind::AddrOf(_, _, inner)
      | ExprKind::Unary(_, inner)
      | ExprKind::DropTemps(inner) => {
        self.descend_through_expansion(inner);
      }
      ExprKind::Array(items) | ExprKind::Tup(items) => {
        for i in items {
          self.descend_through_expansion(i);
        }
      }
      // `assert!(cond)` and `assert_eq!(...)` desugar to a `Match`
      // whose scrutinee is the user-written cond and whose arms are
      // synthesized panic / no-op branches. Descend into the
      // scrutinee so calls like `compare_strings(r1, r2)` inside the
      // assert get their normal Call-arm handling (PassByStaticRef
      // events, function-dot icon on the timeline). Skip the arms —
      // their bodies are synthetic panics we don't want to surface
      // as user events.
      ExprKind::Match(scrutinee, _, _) => {
        // `dbg!(a, b)` is `match (a, b,) { args => { …eprint…; (tmp_1, tmp_2) } }`:
        // each argument is passed by value into the macro and handed
        // back as its result. As a statement the result is then
        // dropped, so this is exactly a call `dbg(a, b)`: `dbg!(&x)`
        // lends x, `dbg!(x)` moves (or copies) x. (`let y = dbg!(e)`
        // never gets here: process_let_binding unwraps it to
        // `let y = e`.)
        if let Some(args) = dbg_args(expr) {
          let name = user_macro_name(expr.span).unwrap_or_else(|| String::from("dbg!"));
          for arg in args {
            self.visit_expr(arg);
            if place_rap(arg, &self.tcx, &self.raps).is_some() {
              self.match_arg(arg, name.clone());
            }
          }
          return;
        }
        self.descend_through_expansion(scrutinee);
      }
      // Symmetric for `if cond { … } else { … }` desugarings (older
      // assert! shapes, `?`'s ControlFlow check, etc.). Walk the
      // guard; skip the arms.
      ExprKind::If(guard, _, _) => {
        self.descend_through_expansion(guard);
      }
      _ => {}
    }
  }

  /// Panic (reported as "can't visualize this yet") if `span` was
  /// produced directly by a user-defined `macro_rules!` and the
  /// subtree walked by `walk` uses a local variable written outside
  /// the macro (i.e. passed in as an argument). A user macro that only
  /// forwards its argument to a std macro (`println!`) never gets here:
  /// the nodes carrying the argument belong to the std macro.
  fn refuse_user_macro_use(
    &self,
    span: rustc_span::Span,
    walk: impl FnOnce(&mut LocalUseFinder<'tcx>),
  ) {
    if !span.from_expansion() {
      return;
    }
    let data = span.ctxt().outer_expn_data();
    let is_user_bang = matches!(data.kind,
        rustc_span::hygiene::ExpnKind::Macro(rustc_span::hygiene::MacroKind::Bang, _))
      && data.macro_def_id.is_some_and(|d| d.is_local());
    if !is_user_bang {
      return;
    }
    let mut finder = LocalUseFinder { tcx: self.tcx, found: None };
    walk(&mut finder);
    if let Some(var) = finder.found {
      let name = user_macro_name(span).unwrap_or_else(|| String::from("a macro!"));
      panic!("user-defined macros that use a variable (`{}`, line {}) aren't supported yet \
              (the use inside `{}` isn't visualized)",
        var.1, var.0, name);
    }
  }

  /// Record that the user-facing macro `name` (e.g. `println!`)
  /// reads `arg` through a shared borrow for the duration of the
  /// call. Rendered like any `f(&x)` call: a function dot on `x`'s
  /// timeline ("println! reads from x"), no arrow and no new column.
  /// Only place expressions (`x`, `x.f`, `*r`, `v[i]`) get an
  /// event; for `s.len()`, `x + 1`, etc. the visit of `arg` already
  /// records what the expression itself does. Repeated arguments
  /// (`println!("{} {}", x, x)`) collapse to one dot.
  fn macro_reads_arg(&mut self, arg: &'tcx Expr<'tcx>, name: String) {
    let Some(from) = place_rap(arg, &self.tcx, &self.raps) else { return };
    self.add_fn(name.clone());
    let line_num = expr_to_line(arg, &self.tcx);
    if let ResourceTy::Value(rap) = &from {
      self.update_rap(rap, line_num);
    }
    let to = ResourceTy::Value(self.raps.get(&fn_key(&name)).unwrap().rap.to_owned());
    let already = self.preprocessed_events.iter().any(|(l, e)| {
      *l == line_num
        && matches!(e, ExternalEvent::PassByStaticReference { from: f, to: t, .. }
          if *f == from && *t == to)
    });
    if !already {
      // format_args! takes `&arg`; the user didn't write the `&`.
      let id = *self.unique_id;
      self.add_external_event(line_num, ExternalEvent::PassByStaticReference { from, to, id, implicit: ImplicitRef::MacroArg, refcell_borrow_mut: false, result_keeps_borrow: false, stored_in: StoredIn::default() });
      *self.unique_id += 1;
    }
  }
}

/// Every `PatKind::Binding` in `pat`, in source order (left to right),
/// including `name @ sub` and the bindings inside `sub`. Or-pattern
/// alternatives bind the same names, so only the first is walked.
fn binding_pats<'tcx>(pat: &'tcx Pat<'tcx>, out: &mut Vec<&'tcx Pat<'tcx>>) {
  match pat.kind {
    PatKind::Binding(_, _, _, sub) => {
      out.push(pat);
      if let Some(sp) = sub {
        binding_pats(sp, out);
      }
    }
    PatKind::Tuple(ps, _) | PatKind::TupleStruct(_, ps, _) => {
      for p in ps { binding_pats(p, out); }
    }
    PatKind::Struct(_, fs, _) => {
      for f in fs { binding_pats(f.pat, out); }
    }
    PatKind::Or(ps) => {
      if let Some(p) = ps.first() { binding_pats(p, out); }
    }
    PatKind::Ref(inner, _, _)
    | PatKind::Box(inner)
    | PatKind::Deref(inner)
    | PatKind::Guard(inner, _) => binding_pats(inner, out),
    PatKind::Slice(before, mid, after) => {
      for p in before { binding_pats(p, out); }
      if let Some(p) = mid { binding_pats(p, out); }
      for p in after { binding_pats(p, out); }
    }
    PatKind::Wild | PatKind::Never | PatKind::Missing | PatKind::Err(_)
    | PatKind::Expr(_) | PatKind::Range(..) => {}
  }
}

/// The by-reference bindings in a fn parameter's pattern that borrow
/// the parameter value itself, which the function owns: `ref s` in
/// `fn f(ref s: String)`, `ref a` in `fn g((ref a, b): (String, i32))`.
/// A binding reached through a reference, written (`&(ref a, b)`) or
/// implicit (`(a, b): &(String, i32)`), borrows the caller's data
/// instead, so it isn't collected. Each entry is the binding's
/// `HirId` and whether it binds the whole parameter (`ref s`) rather
/// than part of it (#377).
fn own_param_ref_bindings<'tcx>(
  pat: &'tcx Pat<'tcx>,
  typeck: &rustc_middle::ty::TypeckResults<'tcx>,
  is_root: bool,
  out: &mut Vec<(HirId, bool)>,
) {
  // Matching through an implicit deref (default binding modes): what's
  // below borrows from behind the caller's reference.
  if typeck.pat_adjustments().get(pat.hir_id).is_some_and(|a| !a.is_empty()) {
    return;
  }
  match pat.kind {
    PatKind::Binding(_, _, _, sub) => {
      if let Some(m) = typeck.pat_binding_modes().get(pat.hir_id) {
        if matches!(m.0, rustc_hir::ByRef::Yes(..)) {
          out.push((pat.hir_id, is_root));
        }
      }
      if let Some(sp) = sub {
        own_param_ref_bindings(sp, typeck, false, out);
      }
    }
    PatKind::Tuple(ps, _) | PatKind::TupleStruct(_, ps, _) => {
      for p in ps { own_param_ref_bindings(p, typeck, false, out); }
    }
    PatKind::Struct(_, fs, _) => {
      for f in fs { own_param_ref_bindings(f.pat, typeck, false, out); }
    }
    PatKind::Or(ps) => {
      if let Some(p) = ps.first() { own_param_ref_bindings(p, typeck, false, out); }
    }
    // An owned `Box` (or a deref pattern) the function owns: its
    // contents are still the function's own.
    PatKind::Box(inner) | PatKind::Deref(inner) => own_param_ref_bindings(inner, typeck, false, out),
    PatKind::Guard(inner, _) => own_param_ref_bindings(inner, typeck, is_root, out),
    PatKind::Slice(before, mid, after) => {
      for p in before { own_param_ref_bindings(p, typeck, false, out); }
      if let Some(p) = mid { own_param_ref_bindings(p, typeck, false, out); }
      for p in after { own_param_ref_bindings(p, typeck, false, out); }
    }
    // `&(ref a, b)`: behind the caller's reference.
    PatKind::Ref(..) => {}
    PatKind::Wild | PatKind::Never | PatKind::Missing | PatKind::Err(_)
    | PatKind::Expr(_) | PatKind::Range(..) => {}
  }
}

/// Finds the first use of a local variable written by the user (not
/// synthesized by a macro) inside an expression.
struct LocalUseFinder<'tcx> {
  tcx: rustc_middle::ty::TyCtxt<'tcx>,
  /// (line, name) of the first such use.
  found: Option<(usize, String)>,
}

impl<'tcx> LocalUseFinder<'tcx> {
  fn visit_expr(&mut self, expr: &'tcx Expr<'tcx>) {
    Visitor::visit_expr(self, expr);
  }
}

impl<'tcx> Visitor<'tcx> for LocalUseFinder<'tcx> {
  fn visit_expr(&mut self, expr: &'tcx Expr<'tcx>) {
    if self.found.is_some() {
      return;
    }
    if let ExprKind::Path(QPath::Resolved(None, path)) = expr.kind {
      if matches!(path.res, Res::Local(_)) && !expr.span.from_expansion() {
        let name = path.segments.last().map(|s| s.ident.as_str().to_owned()).unwrap_or_default();
        self.found = Some((expr_to_line(expr, &self.tcx), name));
        return;
      }
    }
    intravisit::walk_expr(self, expr);
  }
}

/// The name, with its `!`, of the outermost bang macro the user wrote
/// that `span` was expanded from: `println!` for the `&x` that
/// `format_args_nl!` builds inside `println!`, `show!` if the user's
/// own `macro_rules! show` calls `println!`.
fn user_macro_name(span: rustc_span::Span) -> Option<String> {
  let mut span = span;
  let mut name = None;
  while span.from_expansion() {
    let data = span.ctxt().outer_expn_data();
    if let Some(n) = bang_macro_name(&data.kind) {
      name = Some(format!("{}!", n));
    }
    span = data.call_site;
  }
  name
}

/// The bare name of a bang macro expansion. rustc records the path as
/// written, so `std::println!` / `$crate::macros::dbg_internal!` come
/// back qualified; keep the last segment.
fn bang_macro_name(kind: &rustc_span::hygiene::ExpnKind) -> Option<String> {
  use rustc_span::hygiene::{ExpnKind, MacroKind};
  match kind {
    ExpnKind::Macro(MacroKind::Bang, sym) => {
      Some(sym.as_str().rsplit("::").next().unwrap_or("").to_owned())
    }
    _ => None,
  }
}

/// The macro whose expansion directly produced `span`, if it is a bang
/// macro.
fn innermost_macro(span: rustc_span::Span) -> Option<String> {
  bang_macro_name(&span.ctxt().outer_expn_data().kind)
}

/// True for a synthetic `&` built by a macro that borrows each of its
/// arguments: `format_args!` and its variants (behind every formatting
/// macro), `assert_eq!` / `assert_ne!` (`match (&left, &right)`), and
/// the one-argument `panic!` shortcut.
fn borrows_its_args(span: rustc_span::Span) -> bool {
  use rustc_span::hygiene::{DesugaringKind, ExpnKind};
  // `format_args!`'s lowering marks the `&arg` it builds with a
  // `FormatLiteral` desugaring whose call site is the macro.
  if let ExpnKind::Desugaring(DesugaringKind::FormatLiteral { .. }) = span.ctxt().outer_expn_data().kind {
    return true;
  }
  matches!(
    innermost_macro(span).as_deref(),
    Some("format_args" | "format_args_nl" | "const_format_args" | "assert_eq" | "assert_ne"
      // `panic!("{}", x)` / `unreachable!("{}", x)` skip format_args:
      // `panic_display(&x)`.
      | "panic_2021" | "unreachable_2021")
  )
}

/// If `expr` is the expansion of `dbg!(a, b, …)`, return the user's
/// argument expressions. On the pinned toolchain `dbg!` expands (via
/// `dbg_internal!`) to `match (a, b,) { args => { … } }`.
fn dbg_args<'tcx>(expr: &'tcx Expr<'tcx>) -> Option<&'tcx [Expr<'tcx>]> {
  if !expr.span.from_expansion()
    || !matches!(innermost_macro(expr.span).as_deref(), Some("dbg" | "dbg_internal"))
  {
    return None;
  }
  let ExprKind::Match(scrutinee, _, rustc_hir::MatchSource::Normal) = expr.kind else { return None };
  match scrutinee.kind {
    ExprKind::Tup(items) if scrutinee.span.from_expansion()
      && items.iter().all(|e| !e.span.from_expansion()) => Some(items),
    _ => None,
  }
}

/// `dbg!(e)` with a single argument evaluates to `e`'s value.
pub fn dbg_single_arg<'tcx>(expr: &'tcx Expr<'tcx>) -> Option<&'tcx Expr<'tcx>> {
  match dbg_args(expr) {
    Some([e]) => Some(e),
    _ => None,
  }
}

/// The resource a place expression (`x`, `x.f`, `*r`, `v[i]`, or a
/// borrow of one, `&x`) names, if it is a registered RAP. Calls,
/// literals, arithmetic, etc. return None: they produce a temporary,
/// not a read of a variable.
fn place_rap(expr: &Expr, tcx: &rustc_middle::ty::TyCtxt, raps: &std::collections::HashMap<String, RapData>) -> Option<ResourceTy> {
  fn is_place(e: &Expr) -> bool {
    match e.kind {
      ExprKind::Path(QPath::Resolved(_, p)) => matches!(p.res, Res::Local(_)),
      ExprKind::Field(base, _) | ExprKind::Unary(UnOp::Deref, base)
      | ExprKind::Index(base, _, _) | ExprKind::AddrOf(_, _, base) => is_place(base),
      _ => false,
    }
  }
  if !is_place(expr) { return None; }
  match get_rap(expr, tcx, raps) {
    r @ ResourceTy::Value(_) => Some(r),
    // `*r`: reading the referent goes through r; name it "r" rather
    // than "*r" ("*r's resource is immutably borrowed" reads oddly).
    ResourceTy::Deref(rap) => Some(ResourceTy::Value(rap)),
    // A field we don't track as its own column (`r.w` through a
    // reference, `p.0` of a tuple): the read goes through the base.
    _ => match expr.kind {
      ExprKind::Field(base, _) | ExprKind::Index(base, _, _) => place_rap(base, tcx, raps),
      _ => None,
    },
  }
}

#[derive(Clone, Copy, PartialEq)]
enum ManuallyDropFn { Drop, Take }

impl<'a, 'tcx> ExprVisitor<'a, 'tcx> {
  /// If `fn_expr` names the inherent `ManuallyDrop::drop` or
  /// `ManuallyDrop::take` (resolved through the callee's DefId, not
  /// its spelling), which of the two.
  fn manually_drop_assoc_fn(&self, fn_expr: &'tcx Expr<'tcx>) -> Option<ManuallyDropFn> {
    let def_id = self.tcx.typeck(fn_expr.hir_id.owner).type_dependent_def_id(fn_expr.hir_id)?;
    if !matches!(self.tcx.def_kind(def_id), DefKind::AssocFn) {
      return None;
    }
    let imp = self.tcx.impl_of_assoc(def_id)?;
    if self.tcx.impl_opt_trait_ref(imp).is_some()
      || !self.tcx.type_of(imp).skip_binder().ty_adt_def().map_or(false, |d| d.is_manually_drop())
    {
      return None;
    }
    match self.tcx.item_name(def_id).as_str() {
      "drop" => Some(ManuallyDropFn::Drop),
      "take" => Some(ManuallyDropFn::Take),
      _ => None,
    }
  }

  /// `ManuallyDrop::drop(&mut md)` drops the value inside `md` by
  /// hand; `ManuallyDrop::take(&mut md)` moves it out into the
  /// result. Returns false (nothing emitted) when the argument isn't
  /// `&mut` of a tracked ManuallyDrop place, so the generic call
  /// handling runs instead.
  fn manually_drop_by_hand(&mut self, fn_expr: &'tcx Expr<'tcx>, args: &'tcx [Expr<'tcx>], which: ManuallyDropFn) -> bool {
    let [arg] = args else { return false };
    let ExprKind::AddrOf(_, Mutability::Mut, inner) = arg.kind else { return false };
    let Some(from) = place_rap(inner, &self.tcx, &self.raps) else { return false };
    let ResourceTy::Value(rap) = from.clone() else { return false };
    if !rap.is_manually_drop() {
      return false;
    }
    let Some(fn_name) = hirid_to_var_name(fn_expr.hir_id, &self.tcx) else { return false };
    self.add_fn(fn_name.clone());
    self.visit_expr(arg);
    let line_num = expr_to_line(arg, &self.tcx);
    match which {
      ManuallyDropFn::Drop => {
        // Keep the usual call-site events (the `&mut` borrow), then
        // mark the value as dropped.
        self.match_arg(arg, fn_name);
        self.add_external_event(line_num, ExternalEvent::ManuallyDropByHand { ro: rap, id: *self.unique_id });
        *self.unique_id += 1;
      }
      ManuallyDropFn::Take => {
        // The value moves out of `md` into the call (and on into the
        // result), like passing an owner by value.
        let to = ResourceTy::Value(self.raps.get(&fn_key(&fn_name)).unwrap().rap.to_owned());
        self.add_ev(line_num, Evt::Move, to, from, false);
      }
    }
    true
  }
}

impl<'a, 'tcx> ExprVisitor<'a, 'tcx> {
  /// Emit a GoOutOfScope at a `return` for each local that is in
  /// scope there and still owns a resource (#363): Rust drops them
  /// when the function returns. Copy values and references own
  /// nothing to drop. A local named in the returned expression is
  /// left out: it may be what's returned (moved to the caller).
  /// Locals declared inside a branch already get their own
  /// GoOutOfScope at the branch's end.
  fn drop_locals_on_return(&mut self, ret_expr: &'tcx Expr<'tcx>, ret: Option<&'tcx Expr<'tcx>>) {
    struct LocalNames<'tcx> { tcx: rustc_middle::ty::TyCtxt<'tcx>, names: HashSet<String> }
    impl<'tcx> Visitor<'tcx> for LocalNames<'tcx> {
      fn visit_expr(&mut self, e: &'tcx Expr<'tcx>) {
        if let ExprKind::Path(QPath::Resolved(_, p)) = e.kind {
          if let Res::Local(id) = p.res {
            self.names.insert(self.tcx.hir_name(id).to_string());
          }
        }
        intravisit::walk_expr(self, e);
      }
    }
    let mut returned = LocalNames { tcx: self.tcx, names: HashSet::new() };
    if let Some(e) = ret {
      returned.visit_expr(e);
    }
    let line = expr_to_line(ret_expr, &self.tcx);
    // Sorted, so the events' order (and ids) don't depend on HashMap
    // iteration order.
    #[expect(clippy::disallowed_methods, reason = "sorted by name below")]
    let mut owned: Vec<(String, ResourceAccessPoint)> = self.raps.iter()
      .filter(|(name, rd)| {
        let base = name.split('.').next().unwrap_or(name);
        !rd.rap.is_copy() && rd.is_global && rd.scope >= line
          && !returned.names.contains(base) && !self.skip_raps.contains(*name)
      })
      .map(|(name, rd)| (name.clone(), rd.rap.clone()))
      .collect();
    owned.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, rap) in owned {
      if !self.rap_holds_resource_now(&name) && !self.param_never_moved(&name) {
        continue;
      }
      let id = *self.unique_id;
      *self.unique_id += 1;
      self.exit_drops.insert(id);
      self.add_external_event(line, ExternalEvent::GoOutOfScope { ro: rap, id });
    }
  }

  /// A by-value parameter that nothing has moved out of yet. Its
  /// ownership starts with an `InitRefParam`, which
  /// `rap_holds_resource_now` doesn't count.
  fn param_never_moved(&self, name: &str) -> bool {
    fn moved_from(events: &[(usize, ExternalEvent)], name: &str) -> bool {
      events.iter().any(|(_, e)| match e {
        ExternalEvent::Move { from, .. } => from.extract_rap().map_or(false, |r| r.name() == name),
        ExternalEvent::Branch { branches, .. } => branches.iter().any(|b| moved_from(&b.e_data, name)),
        _ => false,
      })
    }
    let is_param = self.preprocessed_events.iter().any(|(_, e)| matches!(e,
      ExternalEvent::InitRefParam { param, .. } if param.name() == name));
    is_param && !moved_from(self.preprocessed_events, name)
  }

  /// Whether `e` never completes normally: its type is `!` (`return`,
  /// `panic!()`), or it is an `if`/`else` chain or `match` every branch
  /// of which diverges, or a block that ends in one (#491). rustc types
  /// `if c { return 1; } else { return 2; }` as `()`, not `!`, so the
  /// type alone misses a nested `else if` whose branches all return.
  pub fn expr_diverges(&self, e: &'tcx Expr<'tcx>) -> bool {
    if self.tcx.typeck(e.hir_id.owner).expr_ty_opt(e).is_some_and(|ty| ty.is_never()) {
      return true;
    }
    match e.kind {
      ExprKind::If(_, then, Some(els)) => self.expr_diverges(then) && self.expr_diverges(els),
      ExprKind::Match(_, arms, _) => !arms.is_empty() && arms.iter().all(|a| self.expr_diverges(a.body)),
      ExprKind::Block(b, _) => {
        b.expr.is_some_and(|t| self.expr_diverges(t))
          || b.stmts.iter().any(|st| matches!(st.kind,
            StmtKind::Expr(x) | StmtKind::Semi(x) if self.expr_diverges(x)))
      }
      _ => false,
    }
  }

  /// Whether `e` is inside a closure of the function being visited.
  fn in_closure(&self, e: &'tcx Expr<'tcx>) -> bool {
    self.tcx.hir_parent_iter(e.hir_id)
      .any(|(_, n)| matches!(n, rustc_hir::Node::Expr(Expr { kind: ExprKind::Closure(..), .. })))
  }

  /// Whether `e` is a statement or the tail of the function's own
  /// body block (or of blocks directly in it): not inside a branch,
  /// a loop, a closure or a `let`'s value. What follows it in the
  /// body runs only if it does (#515).
  fn at_fn_level(&self, e: &'tcx Expr<'tcx>) -> bool {
    use rustc_hir::Node;
    for (_, node) in self.tcx.hir_parent_iter(e.hir_id) {
      match node {
        Node::Stmt(_) | Node::Block(_) => {}
        Node::Expr(Expr { kind: ExprKind::Block(..), .. }) => {}
        Node::Item(_) | Node::ImplItem(_) | Node::TraitItem(_) => return true,
        _ => return false,
      }
    }
    true
  }

  /// Keep a `return`'s drops (`drop_locals_on_return`) only in a
  /// branch that diverges, i.e. one that always leaves through them;
  /// in one that doesn't (the `return` is in a nested `if` without
  /// `else`), they'd end the branch's columns for the paths that go
  /// on. Returns the variables they make live in the branch.
  pub fn keep_exit_drops(&self, events: &mut Vec<(usize, ExternalEvent)>, diverges: bool) -> HashSet<ResourceAccessPoint> {
    events.retain(|(_, e)| diverges || !self.exit_drops.contains(&e.get_id()));
    events.iter()
      .filter(|(_, e)| self.exit_drops.contains(&e.get_id()))
      .filter_map(|(_, e)| e.is_gos_ev().cloned())
      .collect()
  }
}

impl<'a, 'tcx> Visitor<'tcx> for ExprVisitor<'a, 'tcx> {
  // A fn body
  fn visit_body(&mut self, body: &rustc_hir::Body<'tcx>) -> Self::Result {
    self.current_scope = self.tcx.sess.source_map().lookup_char_pos(body.value.span.hi()).line;
    for param in body.params {
      self.visit_param(param);
    }

    self.visit_expr(body.value); // visit fn body
    match body.value { // handle return expression if there is one
      Expr{kind: ExprKind::Block(b, _), ..} => {
        match b.expr {
          // A tail `return v` is handled by visit_expr's `Ret` arm.
          Some(Expr{kind: ExprKind::Ret(_), ..}) => {}
          Some(e) => {
            if self.fn_ret { // only append this event if parent fn ctxt doesn't return void
              self.return_to_caller(e);
            }
          }
          _ => {}
        }
      }
      _ => {
        warn!("unexpected fn body {:#?}", body);
      }
    }
    self.annotate_expr(body.value); // then annotate the body
  } 

  // visit parameter of current fn (add them as RAPs)
  fn visit_param(&mut self, param: &'tcx Param<'tcx>){
    // A parameter written as a pattern (`(a, b): (String, i32)`,
    // `P { name, .. }: P`, `&(x, y): &(i32, i32)`) binds each of its
    // names from the caller's argument, so each name is registered
    // like a parameter of its own, at the type it's bound at (#331).
    // A plain `x: T` is the one-binding case.
    let mut bindings = Vec::new();
    binding_pats(param.pat, &mut bindings);
    let typeck = self.tcx.typeck(param.hir_id.owner);
    let mut own_refs = Vec::new();
    own_param_ref_bindings(param.pat, typeck, true, &mut own_refs);
    for pat in bindings {
      // `ref s` on a by-value parameter: `s` borrows the value this
      // function owns, not the caller's (#377).
      let own_param = own_refs.iter().find(|(id, _)| *id == pat.hir_id).map(|&(_, whole)| OwnParam {
        fn_name: self.tcx.item_name(param.hir_id.owner.to_def_id()).to_string(),
        whole,
        // The pointee of the `&T` the binding is: dropped with the
        // parameter when the function returns, if its type needs drop.
        drops: typeck.pat_ty(pat).builtin_deref(false).is_some_and(|t|
          t.needs_drop(self.tcx, rustc_middle::ty::TypingEnv::post_analysis(self.tcx, param.hir_id.owner))),
      });
      // add RAP corresponding to parameter type
      let line_num=span_to_line(&param.span, &self.tcx);
      let ty = self.tcx.typeck(param.hir_id.owner).pat_ty(pat);
      let is_special = ty_is_special_owner(&ty);
      match pat.kind {
        PatKind::Binding(binding_annotation, _ann_hirid, ident, _op_pat) =>{
          let name: String = ident.to_string();
          if ty.is_ref() {
            // (A `ref` binding of a by-value parameter, `own_param`,
            // takes the same path: its borrow also lasts the whole
            // body and has no lender with a column here, since the
            // value it borrows is the parameter itself. `Caller` is
            // that "no column" placeholder; its dot names the
            // function instead of the caller.)
            //
            // A fn parameter of reference type is conceptually a
            // borrow that came from the caller's frame. Model that
            // explicitly so the borrow region renders for the full
            // function body and the matching return-of-borrow
            // (StaticDie / MutableDie emitted by print_lifetimes)
            // lands at the fn body's closing brace rather than the
            // parameter declaration line.
            //
            // Without this fix, add_ref left the lender as
            // Anonymous and the lifetime equal to the param's own
            // line — print_lifetimes then emitted a phantom
            // StaticDie at the signature line whose `to` field
            // resolved to `Deref(s)`, surfacing the nonsensical
            // tooltip "Return immutably borrowed resource from s
            // to *s".
            let pointee_copy = self.ref_pointee_is_copy(ty, param.hir_id.owner);
            self.add_ref(name.clone(),
            bool_of_mut(ty.ref_mutability().unwrap()),
            bool_of_mut(binding_annotation.1), line_num,
            ResourceTy::Caller, VecDeque::new(), self.current_scope, !self.inside_branch, pointee_copy);
            // Stretch the loan to the closing brace so the dashed
            // ref-line trapezoid covers the full body.
            if let Some(rd) = self.borrow_map.get_mut(&name) {
              rd.lifetime = self.current_scope;
            }
          }
          else if matches!(ty.kind(), rustc_middle::ty::TyKind::Tuple(_)) && self.has_member_columns(ty) {
            // A by-value tuple parameter gets a column per element, like
            // a struct parameter's fields (#189).
            let owner_hash = self.rap_hashes as u64;
            let muta = bool_of_mut(binding_annotation.1);
            let parent_is_copy = self.ty_is_copy(ty, param.hir_id.owner);
            self.add_struct(name.clone(), owner_hash, false, muta, parent_is_copy, self.ty_is_plain(ty, param.hir_id.owner), self.ty_has_interior_mut(ty, param.hir_id.owner), self.current_scope, !self.inside_branch);
            self.mark_owner_type(&name, ty);
            for (field, field_ty) in self.member_fields(ty) {
              // A reference element would need its own loan bookkeeping;
              // leave it without a column.
              if field_ty.is_ref() { continue; }
              let field_is_copy = self.ty_is_copy(field_ty, param.hir_id.owner);
              let field_name = format!("{}.{}", name, field);
              self.add_struct(field_name.clone(), owner_hash, true, muta, field_is_copy, self.ty_is_plain(field_ty, param.hir_id.owner), self.ty_has_interior_mut(field_ty, param.hir_id.owner), self.current_scope, !self.inside_branch);
              self.mark_owner_type(&field_name, field_ty);
            }
          }
          else if ty.is_adt() && !is_special{ // kind of weird given we don't have a InitStructParam
            // Dispatch on the ADT kind so structs get per-field
            // timelines while enums and unions stay opaque (same
            // policy as the LHS-of-expr / pattern-binding sites in
            // expr_visitor.rs). Without this branching, an enum or
            // union parameter would silently get spurious field RAPs
            // registered as if it were a struct.
            match ty.ty_adt_def().unwrap().adt_kind() {
              rustc_middle::ty::AdtKind::Struct => {
                let owner_hash = self.rap_hashes as u64;
                let parent_is_copy = self.ty_is_copy(ty, param.hir_id.owner);
                self.add_struct(name.clone(), owner_hash, false, bool_of_mut(binding_annotation.1), parent_is_copy, self.ty_is_plain(ty, param.hir_id.owner), self.ty_has_interior_mut(ty, param.hir_id.owner), self.current_scope, !self.inside_branch);
                self.mark_owner_type(&name, ty);
                // Nested struct fields get columns too (`r.tl.x`, #239).
                // A reference field borrows from something in the
                // caller's frame, like a reference parameter.
                self.register_struct_members(&name, ty, owner_hash, bool_of_mut(binding_annotation.1), MemberSite {
                  owner_id: param.hir_id.owner,
                  assigned_at: line_num,
                  scope: self.current_scope,
                  is_global: !self.inside_branch,
                  ref_lender: Some(ResourceTy::Caller),
                });
              }
              rustc_middle::ty::AdtKind::Union | rustc_middle::ty::AdtKind::Enum => {
                let is_copy = self.ty_is_copy(ty, param.hir_id.owner);
                let interior_mut = self.ty_has_interior_mut(ty, param.hir_id.owner);
                if ty.is_union() {
                  self.add_union_owner(name.clone(), bool_of_mut(binding_annotation.1), is_copy, self.ty_is_plain(ty, param.hir_id.owner), interior_mut, self.current_scope, !self.inside_branch);
                } else {
                  self.add_owner(name.clone(), bool_of_mut(binding_annotation.1), is_copy, self.ty_is_plain(ty, param.hir_id.owner), interior_mut, self.current_scope, !self.inside_branch);
                }
              }
            }
          }
          else {
            let is_copy = self.ty_is_copy(ty, param.hir_id.owner);
            self.add_owner(name.clone(), bool_of_mut(binding_annotation.1), is_copy, self.ty_is_plain(ty, param.hir_id.owner), self.ty_has_interior_mut(ty, param.hir_id.owner), self.current_scope, !self.inside_branch);
            self.mark_owner_type(&name, ty);
          }
          self.add_external_event(line_num, ExternalEvent::InitRefParam { param: self.raps.get(&name).unwrap().rap.to_owned(), id: *self.unique_id, own_param, in_pattern: None });
          *self.unique_id += 1;
          // A by-value struct parameter owns its fields' resources too
          // (and drops them at the end of the body unless moved on).
          self.bind_struct_fields(line_num, &name);
          // Its reference fields (`e.part: &str`, also nested) hold
          // borrows the caller made, like a reference parameter (#351).
          let prefix = format!("{}.", name);
          #[expect(clippy::disallowed_methods, reason = "sorted by RAP hash below")]
          let mut ref_fields: Vec<ResourceAccessPoint> = self.raps.iter()
            .filter(|(n, rd)| n.starts_with(&prefix) && rd.rap.is_ref())
            .map(|(_, rd)| rd.rap.clone())
            .collect();
          ref_fields.sort_by_key(|r| *r.hash());
          for r in ref_fields {
            self.add_external_event(line_num, ExternalEvent::InitRefParam { param: r, id: *self.unique_id, own_param: None, in_pattern: None });
            *self.unique_id += 1;
          }
          self.annotate_src(name.clone(), ident.span, false, *self.raps.get(&name).unwrap().rap.hash());
        }
        _ => {}
      }
    }
  }

  fn visit_expr(&mut self, expr: &'tcx Expr<'tcx>) {
    // Two complementary behaviours for synthesized HIR nodes (those
    // whose span originates from a macro / desugar expansion):
    //
    //   1. Most synthetic shapes (`Block` / `Call` / `MethodCall` /
    //      `AddrOf` / `Unary` / `DropTemps` / `Array` / `Tup`) are
    //      transparent wrappers — the user's actual code lives at the
    //      bottom. Modern `println!` / `format_args!` expand to a
    //      tree like
    //         { ::std::io::_print({ Arguments::new_v1(&[], &[Argument::new_display(&user_expr)]) }); }
    //      and the historical blanket return discarded the lot,
    //      meaning an inline `r.method()` inside a `println!` (issue
    //      #74) never reached the MethodCall arm. `descend_through_expansion`
    //      walks those wrappers until it hits a user-spanned
    //      descendant and re-dispatches that node through `visit_expr`
    //      so the normal arm logic runs on it. Synthetic control-flow
    //      shapes (If / Match / Loop / Closure) are deliberately *not*
    //      descended through: those are how `assert!`, `?`, etc.
    //      desugar, and the existing arms have explicit
    //      `from_expansion` handling.
    //
    //   2. For-loop desugarings need to fall through to their normal
    //      arm dispatch. rustc's `lower_expr_for` marks the outer
    //      Match's span with `DesugaringKind::ForLoop` and wraps it
    //      in a `DropTemps` (so head temps drop before the surrounding
    //      scope — both spans are `from_expansion=true`). The Match
    //      arm's source-discriminator below has explicit
    //      `ForLoopDesugar` handling that extracts head/pattern/body;
    //      we exempt those two shapes from descend so that handling
    //      runs.
    if expr.span.from_expansion() {
      if !matches!(expr.kind,
          ExprKind::Match(_, _, rustc_hir::MatchSource::ForLoopDesugar)
          | ExprKind::DropTemps(_))
      {
        self.descend_through_expansion(expr);
        return;
      }
      // Fall through to the normal Match / DropTemps arm below.
    }
    match expr.kind {
      // fn call <expr>[<expr>]
      ExprKind::Call(fn_expr, args) => {
        // `ManuallyDrop::drop(&mut md)` / `ManuallyDrop::take(&mut md)`
        // on a tracked ManuallyDrop place (#283).
        if let Some(which) = self.manually_drop_assoc_fn(fn_expr) {
          if self.manually_drop_by_hand(fn_expr, args, which) {
            return;
          }
        }
        if let Some(name) = hirid_to_var_name(fn_expr.hir_id, &self.tcx) {
          self.note_temporary_drop(expr, name);
        }

        // need to specifically handle println! macro because it's common
        // note that other macros will need to be resolved similarly (vec![], assert!, etc)
        // Need to match through all the desugaring and onto the args to the format ({}) function
        match fn_expr.kind {
          ExprKind::Path(QPath::Resolved(_,rustc_hir::Path{res: rustc_hir::def::Res::Def(_, id), ..})) 
          if !id.is_local() => {
            // to see what the macro expansion looks like:
            // println!("{:#?}", expr);
            match args {
              [Expr{kind: ExprKind::Call(_, a),..}] => {
                match a {
                  [_, Expr{kind: ExprKind::AddrOf(_, _, 
                    Expr{kind: ExprKind::Array(x),..}),..}] => {
                      for exp in x.iter() {
                        match exp {
                          Expr{kind: ExprKind::Call(_, format_args), ..} => {
                            let fn_name: String = String::from("println!"); // manually overrwrite name
                            for arg in format_args.iter() {
                              self.visit_expr(&arg);
                              self.match_arg(&arg, fn_name.clone());
                            }
                          }
                          _ => {
                            info!("getting here to the println 1");
                          }
                        }
                      }
                    }
                  _ => {
                    let fn_name: String = hirid_to_var_name(fn_expr.hir_id, &self.tcx).unwrap();
                    self.add_fn(fn_name.clone());
                    for arg in a.iter(){
                      self.visit_expr(&arg);
                      self.match_arg(&arg, fn_name.clone());
                    }
                  }
                }
              }
              _ => {
                let fn_name: String = hirid_to_var_name(fn_expr.hir_id, &self.tcx).unwrap();
                self.add_fn(fn_name.clone());
                for arg in args.iter(){
                  self.visit_expr(&arg);
                  self.match_arg(&arg, fn_name.clone());
                  // self.match_args(&arg, fn_name.clone());
                }
              }
            }
          }
          _ => {
            let fn_name: String = hirid_to_var_name(fn_expr.hir_id, &self.tcx).unwrap();
            self.add_fn(fn_name.clone());
            for arg in args.iter(){
              self.visit_expr(&arg);
              self.match_arg(&arg, fn_name.clone());
            }
            // `f(..)` with `f` a local `FnOnce` closure: the call moves
            // it, after the arguments are evaluated (#271).
            self.closure_call_events(fn_expr, expr_to_line(expr, &self.tcx));
          }
        }
      }
      
      // <expr>.<function>([args])
      ExprKind::MethodCall(name_and_generic_args, rcvr, args, _) => {
        let line_num = expr_to_line(&rcvr, &self.tcx);
        let fn_name = name_and_generic_args.ident.as_str().to_owned();
        self.add_fn(fn_name.clone());
        self.note_temporary_drop(expr, fn_name.clone());
        // Recurse into the receiver first: in a chain like
        // `v.iter().map(f).collect()` that visits `v.iter()` (and
        // `.map(f)`) before this call, so events come out in
        // evaluation order.
        self.visit_expr(rcvr);
        self.method_receiver_events(expr, name_and_generic_args, rcvr, &fn_name, line_num);
        // The arguments are visited whether or not the receiver has a
        // column. In `a.last_mut().unwrap().push(s)` the receiver of
        // `push` is a temporary, but `s` is still moved into `push`
        // (#132); in `self.items.push(s)` the receiver is a field we
        // don't model, but `s` is still moved (#250).
        for arg in args.iter(){
          self.visit_expr(&arg);
          self.match_arg(&arg, fn_name.clone());
        }
        // `refs.push(&s)`: the receiver now holds s's borrow (#392).
        if !matches!(rcvr.kind, ExprKind::MethodCall(..)) {
          let rcvr_rap = expr_to_rap_name(rcvr, &self.tcx)
            .and_then(|n| self.raps.get(&n))
            .map(|rd| rd.rap.to_owned());
          if let Some(rcvr_rap) = rcvr_rap {
            self.record_receiver_store(expr, &rcvr_rap, line_num);
          }
        }
      }
      ExprKind::Binary(_, expra, exprb) => {
        self.visit_expr(expra);
        self.visit_expr(exprb);
        // An overloaded by-value operator is a call to the trait
        // method (`s1 + &s2` is `Add::add(s1, &s2)`), so its operands
        // are arguments: `s1` is moved, `&s2` is borrowed.
        if let Some(op_name) = overloaded_by_value_binop(expr, &self.tcx) {
          let fn_name = op_name.to_owned();
          self.add_fn(fn_name.clone());
          self.match_arg(expra, fn_name.clone());
          self.match_arg(exprb, fn_name);
        } else if let Some((op_name, operands)) = builtin_binop_copied_operands(expr, &self.tcx, &self.raps) {
          // A built-in arithmetic / bitwise operator on primitives
          // (`x + y` on i32) reads its operands the same way: each
          // one that names a column is copied into the operator
          // (#241), and so is each operand of a built-in comparison
          // (#497). A comparison that calls a trait method reads through a
          // reference: see below.
          let fn_name = op_name.to_owned();
          self.add_fn(fn_name.clone());
          for operand in operands {
            self.match_arg(operand, fn_name.clone());
          }
        } else if let Some((op_name, operands)) = overloaded_comparison(expr, &self.tcx) {
          // A comparison on non-primitive operands (`a == b` on
          // Strings) is `PartialEq::eq(&a, &b)`: each operand is
          // borrowed for the call through a `&` that rustc inserts,
          // the way it auto-references a method receiver (`s.len()`),
          // and the dot says so, with the same short note (#366).
          // A written `&a` is an ordinary argument. An operand that
          // already is a reference (`r == q` with `r: &String`), or is
          // reached through `*`, is read through, with no note, as for
          // a receiver.
          let fn_name = op_name.to_owned();
          let typeck = self.tcx.typeck(expr.hir_id.owner);
          for operand in operands {
            if let ExprKind::AddrOf(_, _, inner) = operand.kind {
              if !matches!(get_rap(inner, &self.tcx, &self.raps), ResourceTy::Anonymous) {
                self.match_arg(operand, fn_name.clone());
              }
              continue;
            }
            if !is_auto_borrowed(operand, typeck) {
              continue;
            }
            let Some(rap) = expr_to_rap_name(operand, &self.tcx)
              .and_then(|n| self.raps.get(&n))
              .map(|rd| rd.rap.to_owned()) else { continue };
            self.add_fn(fn_name.clone());
            let line_num = expr_to_line(operand, &self.tcx);
            self.update_rap(&rap, line_num);
            let implicit = if is_method_autoref(operand, typeck) { ImplicitRef::Autoref } else { ImplicitRef::No };
            let fn_rap = self.raps.get(&fn_key(&fn_name)).unwrap().rap.to_owned();
            self.add_pass_by_sref_ev(line_num, ResourceTy::Value(fn_rap), ResourceTy::Value(rap), implicit, false);
          }
        }
      }

      ExprKind::AddrOf(_, _, exp) => {
        self.visit_expr(exp);
      }

      // Compound assignment `a op= b` (#260). It doesn't store `b` in
      // `a`, so it isn't drawn like `a = b`:
      // - On primitives (`acc += x` with i32s) it's built in: `acc` is
      //   read, combined with `x`, and the result written back. Like
      //   `acc + x` (#241), each operand that names a column (`acc`
      //   and `x`) is copied into the operator, and `acc` then gets a
      //   new value. Nothing is
      //   dropped, and nothing is borrowed (a shared borrow of `acc`
      //   that's still live makes it E0506 "cannot assign", not a
      //   borrow conflict).
      // - Otherwise it calls the trait method:
      //   `s += &t` is `AddAssign::add_assign(&mut s, &t)`. `s` is
      //   mutably borrowed for the call and modified in place (it
      //   keeps its resource; nothing is dropped), and `&t` is an
      //   argument like any other, borrowed for the call.
      ExprKind::AssignOp(op, lhs_expr, rhs_expr) => {
        self.visit_expr(lhs_expr);
        self.visit_expr(rhs_expr);
        let fn_name = op.node.as_str().to_owned();
        let line_num = expr_to_line(&lhs_expr, &self.tcx);
        if self.tcx.typeck(expr.hir_id.owner).is_method_call(expr) {
          self.add_fn(fn_name.clone());
          let fn_rap = self.raps.get(&fn_key(&fn_name)).unwrap().rap.to_owned();
          // `s += …` borrows `s` for the call, without a written `&`
          // (like `s.len()`). A place reached through a reference
          // (`*r += …`, `v[i] += …`) is an in-place update of what the
          // reference points to, and says so on the reference's column
          // ("modified through *r"), as for a number (2026-11-06
          // decision); the operator isn't drawn reborrowing `r`.
          match self.resource_of_lhs(lhs_expr) {
            Some(ResourceTy::Value(rap)) => {
              self.add_pass_by_mref_ev(line_num, ResourceTy::Value(fn_rap), ResourceTy::Value(rap), false, false, ImplicitRef::Autoref);
            }
            Some(lhs_rty @ (ResourceTy::Deref(_) | ResourceTy::Index(..) | ResourceTy::DerefField(..))) => {
              let lhs_rty = match lhs_rty {
                ResourceTy::Index(r, place, w) => ResourceTy::Index(r, place, w.into_update()),
                other => other,
              };
              self.add_ev(line_num, Evt::Bind, lhs_rty, ResourceTy::Anonymous, false);
            }
            _ => {}
          }
          self.match_arg(rhs_expr, fn_name);
          return;
        }
        // Both operands are read, as in `acc = acc + x`: the old value
        // of `acc` is copied into the operator too ("Copy from acc to
        // +="), then `x`.
        let operands: Vec<_> = [lhs_expr, rhs_expr].into_iter()
          .filter(|e| operand_names_column(e, &self.tcx, &self.raps))
          .collect();
        if !operands.is_empty() {
          self.add_fn(fn_name.clone());
          for operand in operands {
            self.match_arg(operand, fn_name.clone());
          }
        }
        // `COUNTER += 1` on a `static mut`: no column to write to (see
        // the Path arm below).
        if let ExprKind::Path(QPath::Resolved(_, p)) = lhs_expr.kind {
          if matches!(p.res, Res::Def(DefKind::Static { .. }, _)) {
            return;
          }
        }
        // The write: "acc is bound to a value", or through a
        // reference, "the resource r points to is modified through *r".
        // An element (`v[i] += 1`) has no column of its own: the write
        // goes on the receiver's, as an in-place update.
        match self.resource_of_lhs(lhs_expr) {
          Some(lhs_rty @ (ResourceTy::Value(_) | ResourceTy::Deref(_) | ResourceTy::DerefField(..))) => {
            self.add_ev(line_num, Evt::Bind, lhs_rty, ResourceTy::Anonymous, false);
          }
          Some(ResourceTy::Index(r, place, w)) => {
            self.add_ev(line_num, Evt::Bind, ResourceTy::Index(r, place, w.into_update()), ResourceTy::Anonymous, false);
          }
          _ => {}
        }
      }

      // assignment 
      // ex a = <expr>
      ExprKind::Assign(lhs_expr, rhs_expr, _,) => {
        // `a = lhs` inside a destructuring assignment: draw it as
        // `a = <the part of the RHS lhs holds>`. That part was
        // already visited with the whole RHS.
        let destructure_hid = local_of_path(rhs_expr)
          .filter(|h| self.destructure_srcs.contains_key(h));
        let rhs_expr: &'tcx Expr<'tcx> = match destructure_hid {
          Some(h) => self.destructure_srcs[&h],
          None => rhs_expr,
        };
        self.visit_expr(lhs_expr);
        if destructure_hid.is_none() {
          self.visit_expr(rhs_expr);
        }

        // Writing to a `static mut` (`COUNTER += inc`): the static has no
        // column (see the Path arm below), so there's no LHS resource to
        // attribute the write to. Events inside the RHS were emitted by
        // the visit above; a non-Copy value assigned in (`LOG = v;`) is
        // moved out of its owner, so record that move (to no column).
        if let ExprKind::Path(QPath::Resolved(_, p)) = lhs_expr.kind {
          if matches!(p.res, Res::Def(DefKind::Static { .. }, _)) {
            let rhs_ty = self.tcx.typeck(rhs_expr.hir_id.owner).node_type(rhs_expr.hir_id);
            let rhs_copy = self.tcx.type_is_copy_modulo_regions(rustc_middle::ty::TypingEnv::post_analysis(self.tcx, rhs_expr.hir_id.owner), rhs_ty);
            if !rhs_copy {
              self.match_rhs(ResourceTy::Anonymous, rhs_expr, Evt::Move);
            }
            return;
          }
        }

        // `x = rhs` where x came from `let x;` and is still
        // uninitialized here: this initializes x (#234).
        if let (ExprKind::Assign(..), ExprKind::Path(QPath::Resolved(None, path))) = (expr.kind, lhs_expr.kind) {
          if let Res::Local(hirid) = path.res {
            if let Some(decl) = self.uninit_locals.remove(&hirid) {
              self.init_deferred_local(hirid, decl, rhs_expr);
              return;
            }
          }
        }

        // typecheck to figure out what type of event is going to occur
        let line_num = expr_to_line(&lhs_expr, &self.tcx);
        // `u._a = …` on a union: resource_of_lhs maps it to the
        // union's own column. The field's type (possibly a ref, or
        // a non-Copy ManuallyDrop<T>) says nothing about `u`, and a
        // union field write never runs a destructor, so skip the
        // ref-reassignment and drop-at-reassign handling below.
        let lhs_is_union_field = matches!(lhs_expr.kind,
          ExprKind::Field(recv, _) if self.is_union_typed(recv));
        let lhs_ty = self.tcx.typeck(lhs_expr.hir_id.owner).node_type(lhs_expr.hir_id);
        let is_copyable = self.tcx.type_is_copy_modulo_regions(rustc_middle::ty::TypingEnv::post_analysis(self.tcx, lhs_expr.hir_id.owner), lhs_ty);
        let e = if lhs_ty.is_ref() {
          match lhs_ty.ref_mutability().unwrap() {
            Mutability::Not => Evt::Copy,
            Mutability::Mut => Evt::Move,
          }
        } else {
          match is_copyable {
            true => Evt::Copy, 
            false => Evt::Move
          }
        };
        // None means the LHS is a place the plugin doesn't model (a
        // reference-typed field through a reference `h.r = &x`, a tuple
        // field `t.0`, an element that is itself a reference
        // `v[i] = &s`, …) or an unregistered name. The write itself
        // isn't drawn, but for `=` the RHS still goes into that place:
        // `t.1 = s` moves `s`. Record that
        // against an anonymous destination, otherwise `s` would later
        // be shown dropping a resource it no longer owns.
        let lhs_rty = match self.resource_of_lhs(lhs_expr) {
          Some(rty) => rty,
          None => {
            if let ExprKind::AddrOf(..) = rhs_expr.kind {
              // `v[0] = &u` / `self.r = &x`: a borrow stored in a
              // place with no column (#282).
              self.store_borrow_in_unmodelled_place(lhs_expr, rhs_expr, e);
            } else {
              self.match_rhs(ResourceTy::Anonymous, rhs_expr, e);
            }
            return;
          }
        };
        // `*r = v` / `p.name = v` assign to a place behind a reference:
        // they read like a plain assignment `y = e` (2026-11-05
        // decision), with the place as the subject. The place has no
        // column, so the tooltip also says the old value is dropped
        // when its type needs drop. `*r += v` / `p.n += v` are in-place
        // updates and keep their "modified through" wording.
        let is_assign = matches!(expr.kind, ExprKind::Assign(..));
        let assign_kind = || if is_copyable {
          PlaceWrite::AssignCopy
        } else if lhs_ty.needs_drop(self.tcx, rustc_middle::ty::TypingEnv::post_analysis(self.tcx, lhs_expr.hir_id.owner)) {
          PlaceWrite::AssignDropsOld
        } else {
          PlaceWrite::AssignOwned
        };
        let lhs_rty = match lhs_rty {
          ResourceTy::Deref(rap) if is_assign && !lhs_ty.is_ref() =>
            ResourceTy::Deref(rap.with_place_write(assign_kind())),
          // A reference-typed field (`h.r = &x`) is not an owned or Copy
          // value; leave its wording alone.
          ResourceTy::DerefField(rap, place, _) if is_assign && !lhs_ty.is_ref() =>
            ResourceTy::DerefField(rap, place, assign_kind()),
          // A write to a union (`u.f = e`, `u = U { .. }`) is flagged
          // too, with the Copy-ness of the value written, so the dot
          // can say "assigned a copy of y" or "acquires ownership of
          // a resource" (a union never drops either way).
          ResourceTy::Value(rap) if is_assign
            && rap.is_union() && (lhs_is_union_field || lhs_ty.is_union()) => {
            let write = if is_copyable { PlaceWrite::AssignCopy } else { PlaceWrite::AssignOwned };
            ResourceTy::Value(rap.with_place_write(write))
          }
          other => other,
        };
        // if we are pointing at a new piece of data
        //
        // `resource_of_lhs` returns Anonymous when the LHS is a deref
        // of a temporary rather than a named place:
        // `*c.borrow_mut() += 1`, `*v.last_mut().unwrap() = 5`,
        // `*map.entry(k).or_insert(0) += 1`. There is no RAP for the
        // temporary, so there's no reference to re-point here. The
        // method calls inside the LHS already produced their events
        // (`visit_expr(lhs_expr)` above), and `match_rhs` below still
        // records what happens to the RHS (e.g. `= v` moves v); the
        // temporary itself gets no ownership event.
        // (Union field writes are excluded too; see lhs_is_union_field.)
        if lhs_ty.is_ref() && !lhs_is_union_field && lhs_rty.extract_rap().is_some() {
          let lhs_rap = self.raps.get(&lhs_rty.real_name()).unwrap().rap.clone();
          match lhs_rty {
            // ex:
            // let mut a = &b (where b is &&i32)
            // a = &c (where c is &&i32)
            ResourceTy::Value(_) => {
              let ref_data = self.borrow_map.get(lhs_rap.name()).unwrap().clone();
              let to_ro = match ref_data.lender {
                ResourceTy::Anonymous => ResourceTy::Deref(lhs_rap.clone()),
                _ => ref_data.lender.clone()
              };

              // add event
              // (None if the old reference was moved on, `let r2 = r;`
              // before `r = &mut t;`: r had no borrow left to end, #209.)
              let borrowers = get_borrowers(&lhs_rty.real_name(), &self.borrow_map);
              // In a destructuring assignment, another destination may
              // take this borrow over in the same statement: in
              // `(r1, r2) = (r2, r1)` r1 stops borrowing x but r2 now
              // does, so x is still borrowed (#291). The parts not yet
              // drawn still have their new lender precomputed.
              #[expect(clippy::disallowed_methods, reason = "`any` over the map: order never observed")]
              let taken_over = matches!(ref_data.lender, ResourceTy::Value(_))
                && destructure_hid.is_some_and(|me| self.destructure_ref_data.iter()
                  .any(|(h, (lender, _, _, _))| *h != me && *lender == ref_data.lender));
              if self.ref_handed_off(lhs_rap.name()) {
              }
              else if borrowers.len() > 1 || taken_over { // there is another active reference at this point, the resource cannot be returned
                self.add_external_event(line_num, ExternalEvent::RefDie { from: lhs_rty.clone(), to: to_ro, num_curr_borrowers: borrowers.len() - 1, id: *self.unique_id });
                *self.unique_id += 1;
              }
              else if let Some(end_line) = self.field_write_loan_end(lhs_expr, &lhs_rap, &ref_data, line_num) {
                // `hold.r = &t;` where `hold` held `&s`: the field no
                // longer points to s, but `hold`'s type has one lifetime
                // for the field's reference, so the borrow checker keeps
                // s borrowed until the loan's last use (#531). Drawn on
                // the lender's column only, like #512's stored borrows.
                let place = ResourceTy::Index(lhs_rap.clone(), lhs_rap.name().to_owned(), PlaceWrite::Update);
                let die = if ref_data.ref_mutability { Evt::MDie } else { Evt::SDie };
                self.add_ev(end_line, die, to_ro, place, false);
              }
              else {
                self.add_borrow_end_ev(line_num, to_ro, lhs_rty.clone(), &ref_data);
                // ...and to each further lender (#171).
                for co in ref_data.co_lenders.iter() {
                  let evt = if ref_data.ref_mutability { Evt::MDie } else { Evt::SDie };
                  self.add_ev(line_num, evt, co.clone(), lhs_rty.clone(), false);
                }
              }

              // Add event for possible lender if necessary
              // set possible lender to None

              // check for a match in MIR
              // let mir_b_data = self.gather_borrow_data(&self.bwf);
              // for m_data in mir_b_data.iter() {
              //   match ExprVisitor::borrow_match(&ref_data, m_data) {
              //     Some(kill) => {
              //       if kill > line_num { // this loan needs to be extended 
                      
              //       }
              //       break;
              //     }
              //     None => {}
              //   }
              // }

              // update lhs_rty with new lender information (for a
              // destructuring assignment, as computed before any of
              // its parts was drawn: in `(r1, r2) = (r2, r1)`, r1's
              // lender has changed by the time r2 is assigned)
              let precomputed = destructure_hid.and_then(|h| self.destructure_ref_data.remove(&h));
              let (mut new_lender, new_aliasing, new_static_lender, new_co_lenders) = match precomputed {
                Some(d) => d,
                None => self.get_ref_data_full(&rhs_expr),
              };
              // `cur = &cur.next;` (or `cur = n` with n borrowed from
              // `cur.next`): the new pointee is a field of the *old*
              // cur's pointee, which no column names. Don't record cur
              // as its own lender ("returned to cur.next").
              if new_lender.deref_rap() == Some(&lhs_rap) {
                new_lender = ResourceTy::Anonymous;
              }
              let r = self.borrow_map.get_mut(&lhs_rty.real_name()).unwrap();
              r.assigned_at = line_num;
              r.aliasing = new_aliasing;
              r.lender = new_lender;
              r.static_lender = new_static_lender;
              r.borrow_span = borrow_span_of(rhs_expr);
              r.co_lenders = new_co_lenders;
            }

            // ex:
            // let a = & mut b (where b is &i32)
            // *a = &c (where c is &i32)
            ResourceTy::Deref(_) => {
              let ref_data = self.borrow_map.get(lhs_rap.name()).unwrap().clone();
              let deref_index = num_derefs(&lhs_expr) - 1;
              let modified_ref_name = ref_data.aliasing.get(deref_index).unwrap();
              let modified_ref_data = self.borrow_map.get(modified_ref_name).unwrap().clone();
              let to_ro =  self.borrow_map.get(modified_ref_name).unwrap().lender.to_owned();
              
              // add event
              let borrowers = get_borrowers(&lhs_rty.real_name(), &self.borrow_map);
              if borrowers.len() > 1 { // there is another active reference at this point, the resource cannot be returned
                self.add_external_event(line_num, ExternalEvent::RefDie { from: lhs_rty.clone(), to: to_ro, num_curr_borrowers: borrowers.len() - 1, id: *self.unique_id });
                *self.unique_id += 1;
              }
              else {
                self.add_borrow_end_ev(line_num, to_ro, lhs_rty.clone(), &modified_ref_data);
              }

              // update modified (derefed) reference's lender and aliasing data
              let (new_lender, new_aliasing, new_static_lender, new_co_lenders) = self.get_ref_data_full(&rhs_expr);
              let r = self.borrow_map.get_mut(modified_ref_name).unwrap();
              r.assigned_at = line_num;
              r.aliasing = new_aliasing.clone();
              r.lender = new_lender;
              r.static_lender = new_static_lender;
              r.borrow_span = borrow_span_of(rhs_expr);
              r.co_lenders = new_co_lenders;

              let old_aliasing_data = self.borrow_map.get(&lhs_rty.real_name()).unwrap().aliasing.clone();
              // update other aliases' aliasing data 
              for i in 0..deref_index {
                let ref_name = old_aliasing_data[i].clone();
                let offset = deref_index - i;
                let r = self.borrow_map.get_mut(&ref_name).unwrap();
                r.aliasing.drain(offset..r.aliasing.len()); // remove old aliasing data
                for (j, s) in new_aliasing.iter().enumerate() {
                  r.aliasing.insert(offset + j, s.to_owned());
                }
              }

              // update parent's aliasing data
              let r = self.borrow_map.get_mut(&lhs_rty.real_name()).unwrap();
              r.aliasing.drain(deref_index + 1..r.aliasing.len()); // remove old aliasing data
              for (j, s) in new_aliasing.iter().enumerate() {
                r.aliasing.insert(deref_index + 1 + j, s.to_owned());
              }
            }
            _ => panic!("internal state: a write through `*` whose target is neither a reference nor a plain dereference")
          }
        }

        // Owned non-Copy reassignment: the previous value is dropped
        // at this line. Two shapes:
        //   `y = x`  — drop y's prior resource (its own resource is
        //              what's being overwritten).
        //   `*p = x` — drop the resource currently pointed to via p
        //              (i.e. the lender's resource, found through the
        //              borrow_map alias chain).
        // Skip when the type is Copy (i32 etc.) or a reference (the
        // ref-reassignment branch above already emits the appropriate
        // SDie / MDie / RefDie events for those).
        // Decide the drop target BEFORE match_rhs runs, since
        // match_rhs would update rap_holds_resource_now's state for
        // the lhs (the new Move-into makes it look like the old
        // resource is still held). Emit the event AFTER match_rhs so
        // its dot is drawn over the regular Acquire dot at the same
        // (x, y) — otherwise the colored Acquire circle paints over
        // the white down-arrow triangle and the drop becomes
        // invisible.
        // Assigning a whole union (`u = U { .. }`) drops nothing unless
        // the union has its own `Drop` impl: a union never drops its
        // fields (#191).
        let union_without_drop = lhs_ty.is_union()
          && !lhs_ty.needs_drop(self.tcx, rustc_middle::ty::TypingEnv::post_analysis(self.tcx, lhs_expr.hir_id.owner));
        // Nor does overwriting a `ManuallyDrop<T>` drop the old `T` (#283).
        let lhs_is_manually_drop = lhs_ty.ty_adt_def().map_or(false, |d| d.is_manually_drop());
        let drop_target: Option<ResourceAccessPoint> =
          if !lhs_ty.is_ref() && !is_copyable && !lhs_is_union_field && !union_without_drop && !lhs_is_manually_drop && lhs_ty.needs_drop(self.tcx, rustc_middle::ty::TypingEnv::post_analysis(self.tcx, lhs_expr.hir_id.owner)) {
            match &lhs_rty {
              ResourceTy::Value(rap) => {
                // Plain reassign — only emit the drop if y currently
                // holds a resource (rules out `let y; y = x;` first
                // assignment, and `let y = a; let z = y; y = x;` where
                // y was moved out before reassignment). A value moved
                // on only some paths above counts; the event says so
                // (`maybe_moved` below).
                //
                // A destructuring assignment that also moves y's old
                // value out (`(a, b) = (b, a)`) doesn't drop it, even
                // if that move is drawn after y's new value arrives.
                if self.rap_holds_resource_now(rap.name())
                  && !self.destructure_moved.contains(rap.name()) {
                  Some(rap.clone())
                } else {
                  None
                }
              }
              ResourceTy::Deref(p_rap) => {
                // Deref reassign through a &mut — the owner p points to
                // (found through its lender, and through reborrows:
                // `let f = &mut r.a; *f = …` overwrites `s.a`) holds
                // the resource being overwritten. Borrow checker
                // guarantees it currently holds (otherwise *p
                // wouldn't be a valid place to write to), so we don't
                // need rap_holds_resource_now here.
                self.owner_behind_ref(p_rap, "")
              }
              ResourceTy::DerefField(p_rap, _, _) => {
                // `r.a = v` after `let r = &mut s;`: the field being
                // overwritten is the lender's `s.a`, so that's where
                // the old value is dropped (#152). Only when the lender
                // is a local with per-field columns; a parameter's
                // lender is the caller, which has no column here.
                expr_to_rap_name(lhs_expr, &self.tcx)
                  .and_then(|full| full.strip_prefix(p_rap.name().as_str()).map(str::to_owned))
                  .filter(|sfx| sfx.starts_with('.'))
                  .and_then(|sfx| self.owner_behind_ref(p_rap, &sfx))
              }
              _ => None,
            }
          } else {
            None
          };

        // Overwriting a whole struct drops the resources its fields
        // still own too (`d = c;` drops the old `d.data`), also when
        // the struct is overwritten through a reference (`*r = S {..}`
        // / `r.inner = I {..}` drop the lender's `s.a` / `s.inner.v`).
        let field_drop_targets: Vec<ResourceAccessPoint> = match (&drop_target, &lhs_rty) {
          (Some(rap @ ResourceAccessPoint::Struct(_)),
           ResourceTy::Value(_) | ResourceTy::Deref(_) | ResourceTy::DerefField(..)) => {
            self.struct_fields_holding_resource(rap.name())
          }
          _ => Vec::new(),
        };

        // Whether each old value is there on every path, decided
        // before match_rhs too (see above). `*p = x` writes through a
        // live `&mut`, so its lender holds a value.
        let maybe_moved: Vec<bool> = drop_target.iter().chain(field_drop_targets.iter())
          .map(|rap| matches!(lhs_rty, ResourceTy::Value(_))
            && self.rap_ownership_now(rap.name()) == Ownership::Maybe)
          .collect();

        // `it = b.iter();` where `it` holds a borrow (#336).
        if let ResourceTy::Value(rap) = &lhs_rty {
          if rap.holds_borrow() && !lhs_is_union_field && matches!(expr.kind, ExprKind::Assign(..)) {
            let rap = rap.clone();
            self.reassign_holder(&rap, rhs_expr, lhs_ty, line_num);
          }
        }
        if let (ResourceTy::Index(..), ExprKind::AssignOp(..)) = (&lhs_rty, expr.kind) {
          // `v[i] += k`: k is an operand, not the element's new
          // value, so don't draw it as copied into v[i].
          let compound = match &lhs_rty {
            ResourceTy::Index(r, place, w) => ResourceTy::Index(r.clone(), place.clone(), w.into_update()),
            other => other.clone(),
          };
          self.add_ev(line_num, Evt::Bind, compound, ResourceTy::Anonymous, false);
        } else {
          let n_events = self.preprocessed_events.len();
          self.match_rhs(lhs_rty.clone(), rhs_expr, e);
          // `v[1] = v[2]`: match_rhs has nothing to say about some
          // RHS shapes (an index read, a field of a call, …), but the
          // element is still overwritten.
          if matches!(lhs_rty, ResourceTy::Index(..)) && self.preprocessed_events.len() == n_events {
            self.add_ev(line_num, Evt::Bind, lhs_rty.clone(), ResourceTy::Anonymous, false);
          }
        }

        for (rap, maybe_moved) in drop_target.into_iter().chain(field_drop_targets).zip(maybe_moved) {
          self.add_external_event(line_num, ExternalEvent::OwnerDropAtReassign {
            ro: rap,
            id: *self.unique_id,
            maybe_moved,
          });
          *self.unique_id += 1;
        }
      }

      // `return v`: like a tail expression, `v` goes to the caller
      // (#318). The path through the `return` never reaches the end
      // of the function, so `v` isn't dropped there on that path;
      // when the `return` sits in a branch, that branch has type `!`
      // and doesn't count at the merge (#296), so code after the
      // conditional still sees `v` as owned.
      ExprKind::Ret(ret) => {
        if let Some(e) = ret {
          self.visit_expr(e);
          // Not the `return` a macro or `?` expands to, and not one in
          // a closure (it returns from the closure) (#515).
          if self.fn_ret && !expr.span.from_expansion() && !self.in_closure(expr) {
            self.return_to_caller(e);
          }
        }
        // Leaving the function also drops every value it still owns
        // (#363). Inside a branch, those drops are drawn on the
        // branch's columns, at the `return`.
        if self.inside_branch {
          self.drop_locals_on_return(expr, ret);
        }
      }

      // a block eg: {}
      ExprKind::Block(block, label) => {
        if self.visit_destructuring_assign(block) {
          return;
        }
        // this scoping logic isn't necessary except for when defining functions inside of functions
        let prev_scope = self.current_scope;
        let new_scope = self.tcx.sess.source_map().lookup_char_pos(expr.span.hi()).line;
        self.current_scope = new_scope;
        let before = label.map(|_| self.rap_names());
        self.visit_block(block); // visit all the statements in the block
          self.current_scope = prev_scope;
        // A labeled block (`'a: { … break 'a; … }`) can also end by
        // running to its end.
        if let Some(before) = before {
          self.emit_loop_exit_moves(expr, Some(block.hir_id), &before, true);
        }
      }

      // `-x` / `!b` take their operand by value, like `x + y` (#262):
      // a built-in one on a primitive copies a named operand into
      // the operator; an overloaded one is a call to `Neg::neg` /
      // `Not::not` with the operand as its argument.
      ExprKind::Unary(UnOp::Neg | UnOp::Not, operand) => {
        self.visit_expr(operand);
        if let Some((op_name, operand)) = unary_op_operand(expr, &self.tcx, &self.raps) {
          let fn_name = op_name.to_owned();
          self.add_fn(fn_name.clone());
          self.match_arg(operand, fn_name);
        }
      }

      //unary operator */! <expr>
      ExprKind::Unary(UnOp::Deref, exp) => { self.visit_expr(exp) }

      // A path is a name for something
      // can be a variable, or a path to a definition (function)
      ExprKind::Path(QPath::Resolved(_,p)) => {
        match p.res {
          Res::Def(DefKind::Ctor(_, CtorKind::Const), _id) => {
            let mut name = String::new();
            for (i, segment) in p.segments.iter().enumerate() {
              name.push_str(self.tcx.hir_name(segment.hir_id).as_str());
              if i < p.segments.len() - 1 {
                name.push_str("::");
              }
            }
            self.add_fn(name);
            return;
          }
          // `const` and `static` items don't get RAP timelines —
          // they're compile-time values (or `'static` references)
          // that exist outside the function-local ownership story.
          // Reading one in `let x = N;` makes `x` an owner of the
          // resulting copy / static borrow; the const/static itself
          // doesn't need a column. Skip the path-expression event so
          // the lookup below doesn't trip on the missing RAP.
          Res::Def(DefKind::Const { .. } | DefKind::AssocConst { .. } | DefKind::Static { .. }, _) => {
            return;
          }
          _ => ()
        }
        // Only a local binding can be a variable RAP. Other paths (fn
        // items, statics, consts) have nothing to update, and looking
        // their name up among the variables would update a variable
        // that merely shares the name (#232).
        if !matches!(p.res, Res::Local(_)) {
          return;
        }
        let name = self.tcx.hir_name(p.segments[0].hir_id).as_str().to_owned();
        // Skip path-expression references whose span is from a macro
        // expansion. Modern `println!`, `format_args!`, etc. expand
        // to a chain of synthetic references to internal locals (a
        // synthetic `args` binding, calls to `::core::fmt::*`, etc.)
        // — visit_local already declines to register synthetic
        // locals as RAPs, so a lookup here would unwrap None. The
        // path expressions to user-written variables (e.g. `s`
        // inside `println!("{}", s)`) keep their user source spans
        // intact, so this skip doesn't drop them.
        if expr.span.from_expansion() {
          return;
        }
        // A local that was never registered as a RAP. Match-arm `if let`
        // guard bindings used to land here; they're registered now (see
        // `visit_match_guard`). What's left is mostly bindings in `if`
        // let-chains (`if let A = x && let B = y`), whose condition the If
        // arm only registers when it is a single `let`. Skip the use
        // rather than crash the whole render.
        let Some(rap) = self.raps.get(&name) else {
          log::warn!("use of unregistered local `{}`; skipping", name);
          return;
        };
        let r = &rap.rap.clone();
        let line_num = span_to_line(&p.span, &self.tcx);
        self.update_rap(r, line_num);
      }
      
      // Don't know what this is honestly
      ExprKind::DropTemps(exp) => {
        self.visit_expr(exp);
      }

      // if <expr> { } Option<else>
      ExprKind::If(guard_expr, if_expr, else_expr) => {
        // Macro-expanded `if`s — `assert!(cond)` becomes
        // `match cond { true => {}, _ => panic!(...) }` (which the
        // HIR represents as an If after match-desugaring); the `?`
        // operator and several other macros do similar things.
        // Visualizing these as a control-flow Branch on the user's
        // timeline pollutes the diagram with branches the user
        // didn't write. Walk the guard so any user-side variable
        // accesses inside it (e.g. function arguments) are recorded
        // as ordinary events on their owners' timelines, then skip
        // the body/else and the Branch event entirely.
        if expr.span.from_expansion() {
          self.visit_expr(&guard_expr);
          return;
        }

        // if-let: `if let pat = expr { body }`. The guard is an
        // ExprKind::Let; register its pattern bindings against the
        // scrutinee so identifiers in the body resolve, and emit the
        // scrutinee→binding event + a GoOutOfScope at the body's
        // closing brace. Otherwise (plain `if cond { … }`) just visit
        // the guard for any variable accesses inside it.
        // If-let: collect the pattern bindings registered against the
        // scrutinee. We add them to `if_decl` below so the Branch's
        // decl_vars set covers them and `append_decl_branch_events`
        // populates each binding's timeline with a branch-shaped
        // history (the surrounding If arm already emits the
        // GoOutOfScope events from `if_decl`, so the helper skips
        // emitting them itself).
        //
        // Each branch's events are the ones whose ids are handed out
        // while it is visited (`if_ids`, `else_ids`), see
        // `take_branch_events`. An if-let's pattern bindings happen
        // only when the branch is taken, so they start `if_ids`; the
        // scrutinee is evaluated either way and stays outside.
        let if_lo;
        let if_hash_lo;
        let iflet_bindings: HashSet<ResourceAccessPoint> =
          if let ExprKind::Let(let_expr) = guard_expr.kind {
            self.visit_expr(let_expr.init);
            if_lo = *self.unique_id;
            if_hash_lo = self.rap_hashes;
            self.register_iflet_let_bindings(let_expr, None)
          } else {
            self.visit_expr(&guard_expr);
            if_lo = *self.unique_id;
            if_hash_lo = self.rap_hashes;
            HashSet::new()
          };
        self.inside_branch = true; // need this flag to correctly handle variables that are declared inside blocks
        // Both branches start from the same set of uninitialized
        // `let x;` locals (#234); see join_uninit_locals.
        let uninit_before = self.uninit_locals.clone();
        self.visit_expr(&if_expr);
        // Everything declared in each branch, also inside an inline
        // construct nested in it (an `else if let`'s bindings, #501):
        // see raps_declared_since.
        let if_inner = self.raps_declared_since(if_hash_lo, &uninit_before);
        let else_hash_lo = self.rap_hashes;
        let uninit_after_if = std::mem::replace(&mut self.uninit_locals, uninit_before.clone());
        let if_ids = if_lo..*self.unique_id;
        let else_decl = match else_expr {
          Some(e) => {
            self.visit_expr(e);
            get_decl_of_expr(e, &self.tcx, &self.raps)
          }
          None => HashSet::new(),
        };
        let mut else_inner = self.raps_declared_since(else_hash_lo, &uninit_before);
        if let Some(e) = else_expr {
          else_inner = &else_inner | &else_if_let_bindings(e, &self.raps);
        }
        let uninit_after_else = std::mem::take(&mut self.uninit_locals);
        self.join_uninit_locals(vec![(Some(if_expr), uninit_after_if), (else_expr, uninit_after_else)]);
        let else_ids = if_ids.end..*self.unique_id;
        self.inside_branch = false;

        // compute split and merge points
        let line_num = expr_to_line(&guard_expr, &self.tcx);
        let split = self.tcx.sess.source_map().lookup_char_pos(if_expr.span.lo()).line;
        let if_end = self.tcx.sess.source_map().lookup_char_pos(if_expr.span.hi()).line;
        let merge = match else_expr {
          Some(e) => self.tcx.sess.source_map().lookup_char_pos(e.span.hi()).line,
          None => self.tcx.sess.source_map().lookup_char_pos(if_expr.span.hi()).line
        };

        // Bindings declared by an `if let` guard belong in the if
        // branch's decl set (they live for the body block, same as a
        // top-level `let` inside it).
        let mut if_decl = get_decl_of_expr(if_expr, &self.tcx, &self.raps);
        if_decl = &if_decl | &iflet_bindings;
        // `if_decl` / `else_decl` are the variables this `if` ends at
        // its branches' closing braces. The `*_scope` sets add those
        // ended by a construct nested in the branch (it draws their
        // GoOutOfScope itself): none of them is live across the `if`,
        // and the branch's events for them go on their own columns.
        let if_scope = &if_decl | &if_inner;
        let else_scope = &else_decl | &else_inner;

        // `if cond { … }` / `if let pat = expr { … }` without an
        // `else`. When the body moves a variable that outlives the
        // `if`, the program has two paths: the body ran and moved it,
        // or it didn't (the implicit empty `else`), so the variable is
        // maybe-moved afterwards and dropped at scope end only if the
        // body didn't run (#257). That's drawn exactly like
        // `if cond { … } else {}`: the Branch below gets an empty
        // `else` branch. The body's own range is the same one the
        // if-branch filter below uses.
        //
        // Otherwise (reads, borrows, …: nothing that changes what the
        // variable holds after the `if`) the body is drawn inline:
        // its events are already in `preprocessed_events` from the
        // body visit above, and a branch would only add columns. Just
        // emit GoOutOfScope events for any branch-local bindings
        // (if-let pattern bindings, body-internal let-stmts) so they
        // wind down at the closing brace as they would inside the
        // Branch.
        if else_expr.is_none() {
            let rhs = self.rhs_arm_ids.get(&if_expr.hir_id).copied();
            let body_ev: Vec<(usize, ExternalEvent)> = self.preprocessed_events.iter()
              .filter(|(_, e)| if_ids.contains(&e.get_id())
                || rhs.is_some_and(|(lo, hi)| (lo..hi).contains(&e.get_id())))
              .cloned().collect();
            if !events_move_out_of_outer(&body_ev, &if_scope) {
                for var in in_decl_order(&if_decl).into_iter().filter(|v| needs_scope_end(v)) {
                    self.preprocessed_events.push((
                        if_end,
                        ExternalEvent::GoOutOfScope { ro: var.clone(), id: *self.unique_id },
                    ));
                    *self.unique_id += 1;
                }
                return;
            }
        }

        // Take each branch's events out of the global timeline. They
        // used to be picked by source line (the if-body's lines, then
        // the else-body's), which put everything in the wrong place
        // when branches share a line: a one-line `if c { take(s); }
        // else { … }` drew the move as unconditional, and the `} else
        // if c2 {` line put the inner `if` in the outer if-branch
        // (#117). The guard's events were handed out before `if_ids`,
        // so they stay on the global timeline.
        let _ = line_num; // no longer used for guard-live extension
        let mut if_ev = self.take_branch_events(if_ids, Some(if_expr));
        let mut else_ev = self.take_branch_events(else_ids, else_expr);

        // Compute liveness from the events actually inside the
        // branches — only variables touched by a real event get a
        // branched timeline. Avoids the previous behaviour where
        // `get_live_of_expr(guard_expr)` always promoted guard
        // variables (e.g. the `n` in `if n > 0 { … }`) into branched
        // columns even when neither branch references them.
        let mut liveness: HashSet<ResourceAccessPoint> = HashSet::new();
        collect_event_live_vars(&if_ev, &mut liveness);
        collect_event_live_vars(&else_ev, &mut liveness);
        // A branch of type `!` (`return`, `panic!()`) never reaches
        // the merge (#296). Values dropped at a `return` in it (#363)
        // are live in it.
        let if_diverges = self.expr_diverges(if_expr);
        let else_diverges = else_expr.is_some_and(|e| self.expr_diverges(e));
        liveness.extend(self.keep_exit_drops(&mut if_ev, if_diverges));
        liveness.extend(self.keep_exit_drops(&mut else_ev, else_diverges));
        // Variables declared inside a loop in a branch already go out
        // of scope there (see scoped_in_events). A `return`'s drops
        // (#363) are not that: they leave the enclosing scope.
        let if_scoped = scoped_in_events(&if_ev, &self.exit_drops);
        let else_scoped = scoped_in_events(&else_ev, &self.exit_drops);
        // Variables declared inside a branch (let-stmts within the
        // body, plus if-let pattern bindings) live only for that
        // branch — they're not pre-existing variables that need a
        // branched timeline. Subtract them out.
        let declared_in_branches: HashSet<ResourceAccessPoint> =
          &(&if_scope | &else_scope) | &(&if_scoped | &else_scoped);
        liveness = &liveness - &declared_in_branches;

        // add gos events for variables declared in each block
        for var in in_decl_order(&if_decl).into_iter().filter(|v| needs_scope_end(v) && !if_scoped.contains(v)) {
          if_ev.push((if_end, ExternalEvent::GoOutOfScope { ro: var.clone(), id: *self.unique_id }));
          *self.unique_id += 1;
        }

        for var in in_decl_order(&else_decl).into_iter().filter(|v| needs_scope_end(v) && !else_scoped.contains(v)) {
          else_ev.push((merge, ExternalEvent::GoOutOfScope { ro: var.clone(), id: *self.unique_id }));
          *self.unique_id += 1;
        }
        if_decl.extend(if_inner);
        if_decl.extend(if_scoped);
        let mut else_decl = else_decl;
        else_decl.extend(else_inner);
        else_decl.extend(else_scoped);

        
        let if_map = create_line_map(&if_ev);

        // Per-branch "active" line ranges. Each branch is rendered
        // solid inside its active range and dashed outside. The
        // ranges are non-overlapping so the if-branch transitions
        // to dashed at the line where the else-branch becomes
        // active.
        //
        // For the if-branch we use its body content range
        // (`split + 1 ..= if_end`); for the else-branch we use its
        // body span starting at the `} else {` line so the
        // active region has at least two rows even when the body
        // has a single content line. Without that, an empty or
        // single-line else (e.g. `else { println!(...) }` whose
        // formatter doesn't emit per-event entries on `s`) would
        // collapse to a zero-length solid segment, leaving the
        // entire else column dashed.
        //
        // `merge_point - 1` is the last line each branch column is
        // drawn on; the convergence diagonal then bridges that row
        // to the join dot at `merge_point` (see render_timeline's
        // Branch arm). Clamping the else-branch's end there keeps
        // it consistent with the column-trim done in
        // compute_branch_states.
        //
        // Branches that share lines (#117) are drawn on the split..merge
        // rows themselves; each is active on its own lines.
        let merge_minus_one = merge.saturating_sub(1);
        let collapsed = branch_is_collapsed(split, merge);
        let if_slice = if collapsed { (split, if_end) } else { (split + 1, if_end) };
        let else_slice = match else_expr {
          Some(e) if collapsed => (self.tcx.sess.source_map().lookup_char_pos(e.span.lo()).line, merge),
          _ if collapsed => (if_end, merge),
          _ => (if_end, merge_minus_one),
        };
        // A branch of type `!` (`return`, `panic!()`) never reaches
        // the merge (#296).
        let (if_diverges, if_exits) = self.branch_divergence(if_expr, &if_ev, &if_decl);
        let (else_diverges, else_exits) = match else_expr {
          Some(e) => self.branch_divergence(e, &else_ev, &else_decl),
          None => (false, Vec::new()),
        };
        let (b_labels, b_slices, b_branches) = match else_expr {
          Some(_) => {
            let else_map = create_line_map(&else_ev);
            (
              vec!["If".to_owned(), "Else".to_owned()],
              vec![if_slice, else_slice],
              vec![
                ExtBranchData { e_data: if_ev,   line_map: if_map,   decl_vars: if_decl,   diverges: if_diverges,   exits: if_exits   },
                ExtBranchData { e_data: else_ev, line_map: else_map, decl_vars: else_decl, diverges: else_diverges, exits: else_exits },
              ],
            )
          }
          // No `else` (and the body moves something, see above): the
          // implicit empty `else` is a second path that leaves every
          // variable as it was (#257).
          None => (
            vec!["If".to_owned(), "Else".to_owned()],
            vec![if_slice, else_slice],
            vec![
              ExtBranchData { e_data: if_ev, line_map: if_map, decl_vars: if_decl, diverges: if_diverges, exits: if_exits },
              ExtBranchData { e_data: Vec::new(), line_map: BTreeMap::new(), decl_vars: HashSet::new(), diverges: false, exits: Vec::new() },
            ],
          ),
        };
        // The arm of an `if let` with pattern bindings is named so for
        // the drawing: they bind on the split line, and the arm's column
        // starts there (#382).
        let mut b_labels = b_labels;
        if !iflet_bindings.is_empty() {
          b_labels[0] = IF_LET_ARM.to_owned();
        }
        let b_ty = BranchType::If(b_labels, b_slices);
        self.add_external_event(line_num,
          ExternalEvent::Branch {
            live_vars: liveness,
            branches: b_branches,
            branch_type: b_ty,
            split_point: split,
            merge_point: merge,
            id: *self.unique_id });
            *self.unique_id += 1;
        // Both branches leave the function: nothing after the `if` runs.
        if else_expr.is_some() && if_diverges && else_diverges && self.at_fn_level(expr) {
          self.all_paths_exit = true;
        }
      }

      ExprKind::Loop(block, _, loop_ty, _span) => {
        match loop_ty {
          rustc_hir::LoopSource::While => {
            // rustc lowers `while cond { body }` to:
            //   loop { if cond { body } else { break } }
            // and `while let pat = expr { body }` similarly with cond
            // = `Let(pat, expr)`. The wrapping span is marked
            // DesugaringKind::WhileLoop, so visit_expr's If arm bails
            // on `from_expansion` before reaching the body. Decode the
            // shape here: visit cond, then walk the then-block with
            // inside_branch = true, skipping the synthetic else-break.
            let prev = self.inside_branch;
            if let Some(e) = block.expr {
              if let ExprKind::If(cond, then_body, _else) = e.kind {
                // While-let: register the pattern bindings against the
                // scrutinee so identifiers inside the body resolve.
                // They go out of scope at the body's closing brace;
                // there's no surrounding Branch event to clean up after
                // them (loop bodies are visited inline into the global
                // timeline). That GoOutOfScope is emitted after the
                // body's events: inside an if/match branch, events are
                // kept in the order they're emitted, and one emitted
                // before the body put the binding's end before its uses
                // (`x goes out of scope. No resource is dropped.` for an
                // i32 `x`, #261).
                let mut bindings = HashSet::new();
                if let ExprKind::Let(let_expr) = cond.kind {
                  self.visit_expr(let_expr.init);
                  bindings = self.register_iflet_let_bindings(let_expr, None);
                } else {
                  self.visit_expr(cond);
                }
                let before = self.rap_names();
                self.inside_branch = true;
                self.visit_expr(then_body);
                self.inside_branch = prev;
                // The body's `let`s, and the pattern's bindings, go out
                // of scope at its closing brace (#256); nothing else
                // emits that for them.
                let body_end = self.tcx.sess.source_map()
                  .lookup_char_pos(then_body.span.hi()).line;
                let mut decls = get_decl_of_expr(then_body, &self.tcx, &self.raps);
                decls.extend(bindings);
                self.emit_scope_end(decls, body_end);
                // A `while` also ends when its condition is false.
                self.emit_loop_exit_moves(expr, None, &before, true);
                return;
              }
            }
            // Defensive fallback if a future rustc tweaks the desugar.
            self.inside_branch = true;
            self.visit_block(block);
            self.inside_branch = prev;
          }
          rustc_hir::LoopSource::Loop => {
            // Bare `loop { body }`. No condition, no iterand — render
            // as a single-iteration view: walk the body once with
            // inside_branch = true so any RAPs declared inside have
            // branch-scoped lifetimes. (The body always runs at least
            // once, but we don't yet have a vocabulary for "always
            // runs vs. may run", so use the same shape as while/for.)
            let prev = self.inside_branch;
            let before = self.rap_names();
            self.inside_branch = true;
            self.visit_block(block);
            self.inside_branch = prev;
            // Same as the while arm: the body's `let`s end at its
            // closing brace (#256).
            let body_end = self.tcx.sess.source_map()
              .lookup_char_pos(block.span.hi()).line;
            let decls = get_decl_of_block(block, &self.tcx, &self.raps);
            self.emit_scope_end(decls, body_end);
            // A bare `loop` ends only through a `break`.
            self.emit_loop_exit_moves(expr, None, &before, false);
          }
          rustc_hir::LoopSource::ForLoop => {
            // For-loops are entered via the outer Match(ForLoopDesugar)
            // arm, which drills into the inner Loop's Some-arm body
            // directly. We don't expect to reach the inner Loop through
            // the normal visit path; if we do, the Match arm has
            // already handled the body, so walking it again would
            // double-emit events. No-op.
          }
        }
      }

      // match <expr> {
      // <pat> => <expr>
      // }
      ExprKind::Match(guard_expr, arms, source) => {
        // Macro-expanded `match`s — `assert!(cond)` expands to
        // `match cond { true => {}, _ => panic!(...) }`, and several
        // other macros (notably `?`) also desugar through Match. Walk
        // the guard so user-written variable accesses inside it (e.g.
        // function arguments) are recorded, then skip the arms and
        // the Branch event entirely. Same rationale as the
        // from_expansion check on ExprKind::If above: macro-added
        // control flow shouldn't be rendered as branches the user
        // didn't write.
        //
        // Exception: ForLoopDesugar matches. They're from_expansion
        // (rustc marks lower_expr_for's outer span with
        // DesugaringKind::ForLoop) but the user's body still lives
        // inside, and the source-discriminator below has explicit
        // handling for it.
        if expr.span.from_expansion()
           && !matches!(source, rustc_hir::MatchSource::ForLoopDesugar)
        {
          self.visit_expr(guard_expr);
          return;
        }

        // first visit the guard expression, annotate any events that happen there
        self.visit_expr(guard_expr);
        let typeck_res = self.tcx.typeck(expr.hir_id.owner);

        // To my knowledge a match has to either contain a singular expression or Tuple
        // get all the 'parents' ie things being matched on and their types
        let (parents, parents_ty) = match guard_expr.kind {
          ExprKind::Tup(fields) => {
            let mut res = Vec::new();
            let mut res_ty = Vec::new();
            for field in fields.iter() {
              res.push(self.rap_or_field_through_ref(&field));
              res_ty.push(typeck_res.node_type(field.hir_id));
            }
            (res, res_ty)
          }
          _ => {
            // `match self.kind` with self a reference (#266).
            (vec![self.rap_or_field_through_ref(&guard_expr)], vec![typeck_res.node_type(guard_expr.hir_id)])
          }
        };
        let typeck_res = self.tcx.typeck(guard_expr.hir_id.owner);

        let split = span_end_line(&guard_expr.span, &self.tcx); // TODO: might need to alter this
        let merge = self.tcx.sess.source_map().lookup_char_pos(expr.span.hi()).line;

        let mut b_ty_names: Vec<String> = Vec::new();
        let mut b_slices: Vec<(usize, usize)> = Vec::new();
        let mut branch_data: Vec<ExtBranchData> = Vec::new();
        // Liveness is computed from the events actually folded into
        // each arm's `branch_e_data` below — see the symmetric note
        // in the If arm. Pat-decls (variables bound by the arm's
        // pattern) are subtracted off per arm so they don't show up
        // as branched columns of their own.
        let mut liveness: HashSet<ResourceAccessPoint> = HashSet::new();
        // Track every binding introduced by an arm pattern so we
        // can subtract the union from `liveness` after all arms have
        // been processed.
        let mut all_pat_decls: HashSet<ResourceAccessPoint> = HashSet::new();

        
        match source {
          // A normal match (not desugared)
          rustc_hir::MatchSource::Normal => {
            // Single-arm match: inline. Don't filter body events
            // out of preprocessed_events / don't emit a Branch
            // event — same rationale as single-arm if / if-let
            // above. The directly-built events (pattern
            // destructures, post-arm callbacks, per-arm
            // GoOutOfScope) get routed through add_external_event
            // after the loop so line_map picks them up; body
            // events are already in preprocessed_events from the
            // arm's visit_expr below.
            let inline = arms.len() == 1;
            // Every arm starts from the same uninitialized `let x;`
            // locals (#234); see join_uninit_locals.
            let uninit_before = self.uninit_locals.clone();
            let mut uninit_ends = Vec::new();
            for arm in arms.iter() {
              let arm_hash_lo = self.rap_hashes;
              let mut branch_e_data: Vec<(usize, ExternalEvent)> = Vec::new();
              let mut callback_events: Vec<(ResourceTy, ResourceTy, Evt)> = Vec::new();

              // get line info
              // An arm body can be a bare macro call
              // (`Some(s) => println!("{s}"),`), whose span lies in
              // the macro's expansion; span_to_line / span_end_line
              // resolve it to the user's call site.
              let begin = span_to_line(&arm.body.span, &self.tcx);
              let end = span_end_line(&arm.body.span, &self.tcx);
              let pat_line = span_to_line(&arm.pat.span, &self.tcx);
              b_slices.push((begin, end));
              b_ty_names.push(get_name_of_pat(arm.pat, &self.tcx));

              
              // add/fetch raps that are initialized in arm expr
              // need to also get their types, in order to figure out if we are moving, copying or borrowing something into the block
              let mut pat_decls: HashSet<ResourceAccessPoint> = HashSet::new();
              // Struct bindings whose field columns still need their
              // "acquires its part" events (see below).
              let mut member_binds: Vec<ResourceAccessPoint> = Vec::new();
              // Events the pattern adds to the global list (a struct
              // binding's reference fields, its fields' binds) belong
              // to this arm; see the drain below.
              let first_pat_ev = self.preprocessed_events.len();
              // println!("arm pat {:#?}", arm.pat);
              // Route pattern-destructure events into either
              // `branch_e_data` (multi-arm: events live inside the
              // Branch's per-arm event list) or `preprocessed_events`
              // via `add_external_event` (single-arm inline: events
              // join the global timeline before `visit_expr` so they
              // appear in source order with the body).
              match arm.pat.kind {
                // (<pat>, <pat>, ..) => <expr>
                // First need to annotate events that occur between parents (the variables being matched upon)
                // and the pattern bindings in each arm.
                //
                // `parents.len() == pat_list.len()` is the
                // "guard-was-a-tuple-literal" shape (`match (a, b)
                // { (x, y) => … }`): each inner pattern maps to its
                // own parent expression. Otherwise the guard is a
                // single tuple-/enum-typed scrutinee and every
                // inner pattern destructures out of it (`match
                // pair { (x, y) => … }`, `match res { Ok(a, b, c)
                // => … }`); use the single parent for all inner
                // pattern slots. Without that fallback, `parents[i]`
                // for i>0 panics with an index-out-of-bounds.
                PatKind::TupleStruct(_, pat_list, _) | PatKind::Tuple(pat_list, _)=> {
                  let same_arity = pat_list.len() == parents.len();
                  // `match t { (a, b) => … }` / `match p { P(x, y) => … }`
                  // on a variable whose parts have columns (#189, #326):
                  // each sub-pattern destructures its own part, `t.0` /
                  // `t.1`. (A tuple-literal scrutinee has one parent per
                  // element already.)
                  let part_parents = if parents.len() == 1 {
                    self.pat_parts(arm.pat, &parents[0], parents_ty[0])
                  } else {
                    None
                  };
                  for (i, p) in pat_list.iter().enumerate() {
                    let parent_idx = if same_arity { i } else { 0 };
                    let (parent, parent_ty) = match part_parents.as_ref().and_then(|v| v[i].1.clone()) {
                      Some(pp) => pp,
                      None => (parents[parent_idx].clone(), parents_ty[parent_idx]),
                    };
                    let mut associated_ro = Vec::new();
                    self.get_dec_of_pat_parts(p, &typeck_res, &parent, parent_ty, end, & mut associated_ro);
                    let temp: Vec<ResourceAccessPoint> = associated_ro.iter().map(|(r, _, _, _)| {r.clone()}).collect();
                    let temp2: HashSet<ResourceAccessPoint> = temp.into_iter().collect();
                    pat_decls.extend(temp2);
                    for (to_ro, e, from, partial) in associated_ro.iter() {
                      // If the type of the pat is not the same as the parent associated with it then it must be a partial move
                      let is_partial = partial.unwrap_or(!(parent_ty == typeck_res.node_type(p.hir_id)));
                      let id = *self.unique_id;
                      *self.unique_id += 1;
                      let ext_ev = self.ext_ev_of_evt(e.clone(), ResourceTy::Value(to_ro.clone()), from.clone(), id, is_partial);
                      // add_external_event gives a struct's fields their
                      // part of a whole (non-partial) move; otherwise
                      // that's done below.
                      if matches!(e, Evt::Move | Evt::Copy) && (!inline || is_partial) {
                        member_binds.push(to_ro.clone());
                      }
                      if inline {
                        self.add_external_event(pat_line, ext_ev);
                      } else {
                        branch_e_data.push((pat_line, ext_ev));
                      }
                      match e {
                        Evt::SBorrow => {
                          callback_events.push((from.clone(), ResourceTy::Value(to_ro.clone()), Evt::SDie));
                        }
                        Evt::MBorrow => {
                          callback_events.push((from.clone(), ResourceTy::Value(to_ro.clone()), Evt::MDie));
                        }
                        _ => {}
                      }
                    }
                  }
                }
                // <expr> => <expr> (just a singleton variable)
                _ => {
                  for i in 0..parents.len() {
                    let mut associated_ro = Vec::new();
                    // `P { a, b } => …` on a struct variable: `a` / `b`
                    // come from `p.a` / `p.b` (#326).
                    self.get_dec_of_pat_parts(arm.pat, &typeck_res, &parents[i], parents_ty[i], end, & mut associated_ro);
                    let temp: Vec<ResourceAccessPoint> = associated_ro.iter().map(|(r, _, _, _)| {r.clone()}).collect();
                    let temp2: HashSet<ResourceAccessPoint> = temp.into_iter().collect();
                    pat_decls.extend(temp2);
                    for (to_ro, e, from, partial) in associated_ro.iter() {
                      let id = *self.unique_id;
                      *self.unique_id += 1;
                      let ext_ev = self.ext_ev_of_evt(e.clone(), ResourceTy::Value(to_ro.clone()), from.clone(), id, partial.unwrap_or(false));
                      if matches!(e, Evt::Move | Evt::Copy) && !inline {
                        member_binds.push(to_ro.clone());
                      }
                      if inline {
                        self.add_external_event(pat_line, ext_ev);
                      } else {
                        branch_e_data.push((pat_line, ext_ev));
                      }
                      match e {               // A borrow, mut/immut must be returned at the end of the block
                        Evt::SBorrow => {
                          callback_events.push((from.clone(), ResourceTy::Value(to_ro.clone()), Evt::SDie));
                        }
                        Evt::MBorrow => {
                          callback_events.push((from.clone(), ResourceTy::Value(to_ro.clone()), Evt::MDie));
                        }
                        _ => {}
                      }
                    }
                  }
                }
              }

              // A struct bound by the pattern (`Some(y) =>` with `y: P`)
              // gets field columns (`y.name`, `y.n`). They receive their
              // part of the value with `y`, go out of scope with it,
              // and only exist in this arm (#405).
              member_binds.sort_by_key(|r| *r.hash());
              member_binds.dedup();
              for r in member_binds {
                if matches!(r, ResourceAccessPoint::Struct(Struct { is_member: false, .. })) {
                  self.bind_struct_fields(pat_line, r.name());
                }
              }
              pat_decls = self.with_struct_members(&pat_decls);
              if !inline {
                branch_e_data.extend(self.preprocessed_events.drain(first_pat_ev..));
              }

              // Match guard: `pat if cond =>` / `pat if let p = e =>`.
              // The guard runs after the pattern matched and before
              // the body, and only then; if it's false, matching goes
              // on with the later arms. We show its events in this
              // arm's own column, next to the pattern's bindings:
              // "when this arm is taken, this happened" is always true.
              // What that leaves out is that the guard also ran when
              // the pattern matched but the guard failed and a later
              // arm was taken. For borrows and reads that's harmless
              // (they end inside the guard, so later arms start from
              // the same state), but a move or an assignment would
              // still be in effect in those later arms, so
              // `visit_match_guard` refuses those rather than draw a
              // later arm with a value it no longer has.
              //
              // Bindings introduced by an `if let` guard are registered
              // like `if let` condition bindings and live until the
              // end of the arm, like the pattern's own bindings.
              if let Some(guard) = arm.guard {
                #[expect(clippy::disallowed_methods, reason = "collected into a set; order is lost")]
                let outer: HashSet<ResourceAccessPoint> = self.raps.values()
                  .map(|rd| rd.rap.clone())
                  .filter(|r| !r.is_fn() && !pat_decls.contains(r))
                  .collect();
                let first_guard_ev = self.preprocessed_events.len();
                let mut guard_decls: HashSet<ResourceAccessPoint> = HashSet::new();
                self.visit_match_guard(guard, true, &outer, &mut guard_decls);
                if !inline {
                  branch_e_data.extend(self.preprocessed_events.drain(first_guard_ev..));
                }
                pat_decls.extend(guard_decls);
              }

              // visit expr
              self.inside_branch = true;
              self.uninit_locals = uninit_before.clone();
              let body_lo = *self.unique_id;
              self.visit_expr(arm.body);
              let body_ids = body_lo..*self.unique_id;
              uninit_ends.push((Some(arm.body), std::mem::take(&mut self.uninit_locals)));
              self.inside_branch = false;

              all_pat_decls = &all_pat_decls | &pat_decls;
              let mut arm_decls: HashSet<ResourceAccessPoint> = &pat_decls | &get_decl_of_expr(arm.body, &self.tcx, &self.raps);
              // Everything declared in the arm, also inside an inline
              // construct nested in it (`Some(x) => { if let Some(y) =
              // b { … } }`, #501), exists only in the arm (the nested
              // construct ends what it declares itself).
              let arm_inner = self.raps_declared_since(arm_hash_lo, &uninit_before);
              all_pat_decls = &all_pat_decls | &arm_inner;
              // Variables declared inside a loop in this arm already go
              // out of scope there (see scoped_in_events).
              let mut arm_scoped: HashSet<ResourceAccessPoint> = HashSet::new();


              // get events that occured in the arm: those handed out
              // while its body was visited, plus the ones pairing its
              // value with a `let` (see `take_branch_events`). Picking
              // them by source line gave every event on a shared line
              // to the first arm (#117).
              if !inline {
                let body_ev = self.take_branch_events(body_ids, Some(arm.body));
                branch_e_data.extend(body_ev);

                // contribute events-touched variables to liveness; the
                // arm's own pat-decls are subtracted at the end.
                collect_event_live_vars(&branch_e_data, &mut liveness);
                // Values dropped at a `return` in the arm (#363).
                let arm_diverges = self.expr_diverges(arm.body);
                liveness.extend(self.keep_exit_drops(&mut branch_e_data, arm_diverges));
                arm_scoped = scoped_in_events(&branch_e_data, &self.exit_drops);
                all_pat_decls = &all_pat_decls | &arm_scoped;
              }

              // add callback events - events that need to happen at the end of an arm block
              // ex: if a pattern binding borrows from a parent
              for (to, from, e) in callback_events {
                let name = from.real_name();
                self.borrow_map.remove(&name);
                let id = *self.unique_id;
                *self.unique_id += 1;
                let ext_ev = self.ext_ev_of_evt(e, to, from, id, false);
                if inline {
                  self.add_external_event(end, ext_ev);
                } else {
                  branch_e_data.push((end, ext_ev));
                }
              }

              // add gos events
              for r in in_decl_order(&arm_decls).into_iter().filter(|v| needs_scope_end(v) && !arm_scoped.contains(v)) {
                let id = *self.unique_id;
                *self.unique_id += 1;
                let gos = ExternalEvent::GoOutOfScope { ro: r.clone(), id };
                if inline {
                  self.add_external_event(end, gos);
                } else {
                  branch_e_data.push((end, gos));
                }
              }

              arm_decls.extend(arm_inner);
              arm_decls.extend(arm_scoped);
              if !inline {
                let branch_line_map = create_line_map(&branch_e_data);
                // An arm of type `!` (`_ => unreachable!()`) never
                // reaches the merge (#296).
                let (diverges, exits) = self.branch_divergence(arm.body, &branch_e_data, &arm_decls);
                branch_data.push(ExtBranchData { e_data: branch_e_data, line_map: branch_line_map, decl_vars: arm_decls, diverges, exits });
              }
            }
            self.join_uninit_locals(uninit_ends);
            // Single-arm: events already routed through
            // add_external_event during the loop, nothing else
            // to do — bail out of the Normal arm before the
            // multi-arm Branch construction below.
            if inline {
                return;
            }
            // Variables introduced by an arm's pattern only exist
            // inside that arm — strip them out so they don't get a
            // branched timeline of their own.
            liveness = &liveness - &all_pat_decls;
            // add branch event. merge_point is the closing-brace
            // line of the match — same convention as If, where
            // merge_point is the line of the else-block's closing
            // `}`. Previously this used `merge - 1` (with a stale
            // TODO marker), which landed the join dot one row above
            // the `};`, on the last arm's body line.
            if !branch_data.is_empty() && branch_data.iter().all(|b| b.diverges) && self.at_fn_level(expr) {
              self.all_paths_exit = true;
            }
            self.add_external_event(split, ExternalEvent::Branch {
              live_vars: liveness,
              branches: branch_data,
              branch_type: BranchType::Match(b_ty_names, b_slices),
              split_point: split,
              merge_point: merge,
              id: *self.unique_id });
            *self.unique_id += 1;
          }
          rustc_hir::MatchSource::ForLoopDesugar => {
            // For-loops desugar (rustc_ast_lowering's lower_expr_for) to:
            //
            //   match IntoIterator::into_iter(<head>) {
            //     mut iter => loop {
            //       match Iterator::next(&mut iter) {
            //         None       => break,
            //         Some(<pat>) => <body>,
            //       }
            //     }
            //   }
            //
            // We render this as the issue's "single-iteration view":
            // emit one borrow/move event for <head> at the for line,
            // register the pattern binding(s) as branch-scoped RAPs,
            // and visit <body> with inside_branch = true so its events
            // flow into the timeline as if the loop ran once. We don't
            // emit a Branch event yet — the conditional-shading
            // visualization is design work tracked alongside #78.
            //
            // Shape pattern-matching is defensive: if the inner HIR
            // doesn't look like the lowering above (e.g. a future
            // rustc tweak, or a `for await`), we silently fall back to
            // the previous behaviour (no body events) rather than
            // panic.
            if let Some(head_arg) = match guard_expr.kind {
              ExprKind::Call(_, args) if args.len() == 1 => Some(&args[0]),
              _ => None,
            } {
              if let Some((user_pat, body_expr)) = arms.first()
                .and_then(|outer_arm| match outer_arm.body.kind {
                  ExprKind::Loop(loop_block, _, rustc_hir::LoopSource::ForLoop, _) => {
                    loop_block.stmts.first().and_then(|s| match s.kind {
                      StmtKind::Expr(e) | StmtKind::Semi(e) => match e.kind {
                        ExprKind::Match(_, inner_arms, rustc_hir::MatchSource::ForLoopDesugar) => {
                          // The Some-arm of the inner match wraps the
                          // user pattern in a `PatKind::Struct` with a
                          // single PatField (rustc's pat_some →
                          // pat_lang_item_variant — the wrapper is
                          // `Some(pat)` where pat lives in fields[0]).
                          inner_arms.iter().find_map(|a| match a.pat.kind {
                            PatKind::Struct(_, fields, _) if fields.len() == 1 => {
                              Some((fields[0].pat, a.body))
                            }
                            _ => None,
                          })
                        }
                        _ => None,
                      },
                      _ => None,
                    })
                  }
                  _ => None,
                })
              {
                // Iterand event: shape of head_arg dictates the arrow.
                //   &v       ⇒ SBorrow from v
                //   &mut v   ⇒ MBorrow from v
                //   v (owned, non-Copy) ⇒ Move from v
                //   anything else (Range, call result, …) ⇒ Bind (below)
                let for_line = self.tcx.sess.source_map()
                  .lookup_char_pos(expr.span.lo()).line;
                let iterand_evt = match head_arg.kind {
                  ExprKind::AddrOf(_, Mutability::Not, _) => Some(Evt::SBorrow),
                  ExprKind::AddrOf(_, Mutability::Mut, _) => Some(Evt::MBorrow),
                  ExprKind::Path(_) => {
                    let head_ty = self.tcx.typeck(head_arg.hir_id.owner)
                      .node_type(head_arg.hir_id);
                    let is_copy = self.tcx.type_is_copy_modulo_regions(
                      rustc_middle::ty::TypingEnv::post_analysis(self.tcx, head_arg.hir_id.owner),
                      head_ty);
                    if head_ty.is_ref() { None }   // already a ref binding; skip
                    else if is_copy { None }       // Range / Copy iterand
                    else { Some(Evt::Move) }
                  }
                  _ => None,
                };
                let head_rty = get_rap(head_arg, &self.tcx, &self.raps);
                // `for x in it` where `it` is already an iterator
                // (`let it = v.iter_mut();`): `IntoIterator::into_iter`
                // returns its argument unchanged, so the loop *moves*
                // `it` into itself and each `x` is something `next()`
                // hands out, not something `it` owns or lends (#417).
                // Compare the head's type with the type of the
                // `into_iter(head)` call: they are equal only for an
                // iterator.
                let head_is_iterator = matches!(head_arg.kind, ExprKind::Path(_))
                  && matches!(iterand_evt, Some(Evt::Move))
                  && {
                    let typeck = self.tcx.typeck(head_arg.hir_id.owner);
                    let head_ty = typeck.node_type(head_arg.hir_id);
                    !head_ty.is_ref()
                      && head_ty == typeck.node_type(guard_expr.hir_id)
                  };

                // Register the user pattern binding(s) as branch-scoped
                // RAPs so events inside the body referencing them
                // resolve. Two cases:
                //
                //   `for x in &v`           — single binding, attribute
                //                             an iterand-shaped event
                //                             (SBorrow / MBorrow / Move)
                //                             from the head expression.
                //
                //   `for (i, a) in iter`    — tuple / struct / etc. The
                //                             iterand semantics don't
                //                             pair element-wise against
                //                             a tuple pattern (the
                //                             iterator yields tuples but
                //                             the iter expression isn't
                //                             one), so fall back to
                //                             `bind_walk`: register every
                //                             binding the pattern names
                //                             against `head_arg`. Each
                //                             gets a best-effort event
                //                             (a Move/Copy via
                //                             match_rhs) — approximate,
                //                             but the body refs to
                //                             `i`/`a` resolve and the
                //                             columns aren't empty.
                let prev_inside_branch = self.inside_branch;
                self.inside_branch = true;
                if let PatKind::Binding(_, _, pat_ident, None) = user_pat.kind {
                  let pat_name = pat_ident.to_string();
                  let pat_ty = self.tcx.typeck(user_pat.hir_id.owner)
                    .pat_ty(user_pat);
                    let pointee_copy = self.ref_pointee_is_copy(pat_ty, user_pat.hir_id.owner);
                  if pat_ty.is_ref() && head_is_iterator {
                    // A reference an iterator hands out: its lender isn't
                    // known here, and nothing is "returned" to the
                    // iterator (or to anything) when it ends.
                    self.add_ref(
                      pat_name.clone(),
                      bool_of_mut(pat_ty.ref_mutability().unwrap()),
                      false,
                      for_line,
                      ResourceTy::Anonymous,
                      std::collections::VecDeque::new(),
                      self.current_scope,
                      false,
                      pointee_copy,
                    );
                    self.unlent_refs.insert(pat_name.clone());
                  } else if pat_ty.is_ref() {
                    self.add_ref(
                      pat_name.clone(),
                      bool_of_mut(pat_ty.ref_mutability().unwrap()),
                      false,
                      for_line,
                      head_rty.clone(),
                      std::collections::VecDeque::new(),
                      self.current_scope,
                      false,
                      pointee_copy,
                    );
                  } else {
                    let is_copy = self.tcx.type_is_copy_modulo_regions(
                      rustc_middle::ty::TypingEnv::post_analysis(self.tcx, user_pat.hir_id.owner),
                      pat_ty);
                    self.add_owner(pat_name.clone(), false, is_copy, self.ty_is_plain(pat_ty, user_pat.hir_id.owner), self.ty_has_interior_mut(pat_ty, user_pat.hir_id.owner), self.current_scope, false);
                    self.mark_owner_type(&pat_name, pat_ty);
                  }
                  self.annotate_src(pat_name.clone(), pat_ident.span, false,
                    *self.raps.get(&pat_name).unwrap().rap.hash());

                  // Emit the iterand event into the user's pattern binding.
                  let to_rty = ResourceTy::Value(self.raps.get(&pat_name).unwrap().rap.to_owned());
                  if head_is_iterator {
                    // The iterator is moved into the loop; nothing is
                    // moved into (or borrowed by) the loop variable.
                    self.add_ev(for_line, Evt::Move, ResourceTy::Anonymous, head_rty, false);
                    self.add_ev(for_line, Evt::Bind, to_rty, ResourceTy::Anonymous, false);
                  } else if let Some(e) = iterand_evt {
                    self.add_ev(for_line, e, to_rty, head_rty, false);
                  } else {
                    // A Range or a call result (`0..n`, `s.chars()`) has
                    // no place to borrow or move from: each iteration's
                    // value comes out of the iterator's `next()`. Give the
                    // binding a plain Bind so its column starts at the
                    // `for` line (#205).
                    self.add_ev(for_line, Evt::Bind, to_rty, ResourceTy::Anonymous, false);
                  }
                } else {
                  // Composite patterns: bind every name in the pattern
                  // as a branch-scoped RAP so body references resolve.
                  // Emit a Bind from Anonymous for each — we don't
                  // have a clean iterand shape (the iterator yields
                  // tuples but the iter expression isn't a tuple).
                  // Calling `match_rhs(head_arg)` would synthesize a
                  // Copy from the iter's tail Function (e.g.
                  // `enumerate`); those Function→Value events are
                  // filtered out of `event_line_map` and end up
                  // never reaching the destination's timeline, so the
                  // column wouldn't render at all. The Bind shortcut
                  // sidesteps that and matches the macro-RHS path.
                  let _ = iterand_evt;
                  let _ = head_rty;
                  self.register_pattern_bindings_anon(user_pat, for_line);
                }

                // Visit body — events flow into the global timeline.
                let before = self.rap_names();
                self.visit_expr(body_expr);
                self.inside_branch = prev_inside_branch;

                // The pattern bindings are fresh each iteration and,
                // like the body's own `let`s, go out of scope at the
                // body's closing brace (#205, #256). They're registered
                // with is_global = false, so print_out_of_scope won't
                // emit this for them.
                let body_end = self.tcx.sess.source_map()
                  .lookup_char_pos(body_expr.span.hi()).line;
                let mut loop_decls = get_decl_of_expr(body_expr, &self.tcx, &self.raps);
                collect_pat_bindings(user_pat, &self.raps, &mut loop_decls);
                self.emit_scope_end(loop_decls, body_end);
                // `break`s in the body target the desugared `loop`
                // (the outer arm's body), which also ends when the
                // iterator runs out.
                if let Some(loop_expr) = arms.first().map(|a| a.body) {
                  self.emit_loop_exit_moves(loop_expr, None, &before, true);
                }
              }
            }
          }
          _ => {}
        }
      }
      _ => {
        intravisit::walk_expr(self, expr);
      }
    }
  }
  fn visit_stmt(&mut self, statement: &'tcx Stmt<'tcx>) {
    match statement.kind {
      StmtKind::Let(ref local) => self.visit_local(local),
      StmtKind::Item(item) => self.visit_nested_item(item),
      StmtKind::Expr(ref expression) | StmtKind::Semi(ref expression) => {
          self.visit_expr(expression)
      }
    }
  }

  // locals are let statements: let <pat>:<ty> = <expr>
  fn visit_local(&mut self, local: &'tcx LetStmt<'tcx>) {
    // Skip macro-expanded `let` bindings. Modern `println!`,
    // `format_args!`, and friends expand to something like
    // `let args = ::core::fmt::Arguments::new(...)` followed by a
    // call to write to stdout — the synthetic `args` local isn't in
    // the user's source but the plugin would otherwise register it
    // as a RAP and give it its own timeline column. Same rationale
    // as the from_expansion check on ExprKind::If/Match: macro
    // internals shouldn't appear as user-visible variables.
    if local.span.from_expansion() {
      // ... but a `let t = $e;` from the user's own macro moves or
      // copies a variable the caller passed in (#342).
      if let Some(init) = local.init {
        self.refuse_user_macro_use(local.span, |v| v.visit_expr(init));
      }
      return;
    }
    let local_line = self.tcx.sess.source_map().lookup_char_pos(local.span.lo()).line;
    let is_skipped = self.skip_lines.contains(&local_line);
    if let Some(init) = local.init {
      self.process_let_binding(local.pat, init, is_skipped);
    } else {
      // `let x;` (deferred initialization, #234): x owns nothing yet,
      // so register no RAP and emit no event here. Remember each
      // binding; its first assignment registers the RAP with this
      // `let`'s scope and reads as initialization (see the Assign arm).
      let (scope, inside_branch) = (self.current_scope, self.inside_branch);
      local.pat.each_binding(|annotation, hir_id, _, _| {
        self.uninit_locals.insert(hir_id, DeferredLet { annotation, scope, inside_branch, is_skipped });
      });
    }

    if let Some(els) = local.els {
      self.visit_block(els);
    }
  }
}

impl<'a, 'tcx> ExprVisitor<'a, 'tcx> {
  /// A call whose result is a temporary that Rust drops at the end of
  /// its statement (#457): `make();`, `let _ = make();`, `make().len()`,
  /// `f(&make())`. The temporary has no column, so say so on the
  /// producing fn's column with a drop dot. Only when the result's type
  /// needs drop, and only for the shapes where the value certainly isn't
  /// moved on: anything else (a field moved out of it, a `match` that
  /// binds part of it by value, a borrow a `let` extends) says nothing.
  fn note_temporary_drop(&mut self, expr: &'tcx Expr<'tcx>, fn_name: String) {
    if self.temp_drops_noted.contains(&expr.hir_id) {
      return;
    }
    let typeck = self.tcx.typeck(expr.hir_id.owner);
    let ty = typeck.expr_ty(expr);
    let env = rustc_middle::ty::TypingEnv::post_analysis(self.tcx, expr.hir_id.owner);
    if !ty.needs_drop(self.tcx, env) || !self.is_unmoved_temporary(expr, typeck, env) {
      return;
    }
    let Some((when, end_line, owner)) = self.temporary_scope_end(expr) else { return };
    // Branch events are sliced out by line, so a one-line `match` / `if`
    // can't hold a column of its own (the same limit as #354 / #459).
    if self.inside_one_line_branch(expr) {
      return;
    }
    self.temp_drops_noted.insert(expr.hir_id);

    // The temporary gets a short column: it acquires the call's result
    // here and is dropped where its scope ends.
    self.add_fn(fn_name.clone());
    let fn_rap = self.raps.get(&fn_key(&fn_name)).unwrap().rap.to_owned();
    // Numbered in one sequence with the `let`-extended temporaries
    // (#400): the label doesn't say which kind it is, the tooltips do.
    self.temp_count += 1;
    let tmp_name = temporary_label(self.temp_count);
    self.add_owner(tmp_name.clone(), false, false, false, false, end_line, !self.inside_branch);
    if let Some(ResourceAccessPoint::Owner(o)) = self.raps.get_mut(&tmp_name).map(|rd| &mut rd.rap) {
      o.unbound_temp = true;
    }
    self.raps.get_mut(&tmp_name).unwrap().temp_owner = owner;
    let tmp = self.raps.get(&tmp_name).unwrap().rap.to_owned();
    let line = expr_to_line(expr, &self.tcx);
    self.add_ev(line, Evt::Move, ResourceTy::Value(tmp.clone()), ResourceTy::Value(fn_rap), false);
    // Plain text: `hover_messages::fmt_style` escapes it for the tooltip.
    let ty = strip_module_paths(&ty.to_string());
    let id = *self.unique_id;
    self.add_external_event(end_line, ExternalEvent::TemporaryDrop { ro: tmp, ty, when: when.to_owned(), id });
    *self.unique_id += 1;
  }

  /// True iff `expr` sits inside an `if` / `match` that starts and ends
  /// on one line.
  fn inside_one_line_branch(&self, expr: &'tcx Expr<'tcx>) -> bool {
    let sm = self.tcx.sess.source_map();
    self.tcx.hir_parent_iter(expr.hir_id).any(|(_, node)| match node {
      rustc_hir::Node::Expr(e) if matches!(e.kind, ExprKind::If(..) | ExprKind::Match(..)) && !e.span.from_expansion() => {
        sm.lookup_char_pos(e.span.lo()).line == sm.lookup_char_pos(e.span.hi()).line
      }
      _ => false,
    })
  }

  /// True iff the value of the call `expr` is used only in a way that
  /// leaves it a temporary to be dropped: discarded (`expr;`,
  /// `let _ = expr;`), borrowed (`expr.len()`, `&expr` as an argument,
  /// `expr == y`, `expr[i]`), or matched without moving anything out.
  fn is_unmoved_temporary(
    &self,
    expr: &'tcx Expr<'tcx>,
    typeck: &'tcx rustc_middle::ty::TypeckResults<'tcx>,
    env: rustc_middle::ty::TypingEnv<'tcx>,
  ) -> bool {
    use rustc_hir::Node;
    let is_me = |e: &Expr| e.hir_id == expr.hir_id;
    match self.tcx.parent_hir_node(expr.hir_id) {
      Node::Stmt(s) => matches!(s.kind, StmtKind::Semi(e) | StmtKind::Expr(e) if is_me(e)),
      Node::LetStmt(l) => l.init.is_some_and(is_me) && l.els.is_none() && matches!(l.pat.kind, PatKind::Wild),
      Node::Expr(p) => match p.kind {
        // `make().len()` autorefs the receiver; `make().into_iter()` moves it.
        ExprKind::MethodCall(_, rcvr, _, _) if is_me(rcvr) => typeck.expr_ty_adjusted(rcvr).is_ref(),
        ExprKind::AddrOf(..) => self.borrowed_as_argument(p, typeck),
        ExprKind::Index(base, _, _) if is_me(base) => !self.extended_by_let(p),
        // A Copy field read leaves the whole temporary to drop; moving a
        // field out leaves only the rest, which isn't stated.
        ExprKind::Field(base, _) if is_me(base) => {
          self.tcx.type_is_copy_modulo_regions(env, typeck.expr_ty(p)) && !self.extended_by_let(p)
        }
        // `==`, `<`, … take both operands by reference.
        ExprKind::Binary(op, l, r) if is_me(l) || is_me(r) => op.node.is_comparison(),
        ExprKind::Match(scrut, arms, rustc_hir::MatchSource::Normal) if is_me(scrut) => {
          let mut moves_out = false;
          for arm in arms {
            arm.pat.each_binding(|ann, hid, _, _| {
              if ann.0 == rustc_hir::ByRef::No && typeck.node_type(hid).needs_drop(self.tcx, env) {
                moves_out = true;
              }
            });
          }
          !moves_out
        }
        _ => false,
      },
      _ => false,
    }
  }

  /// `&call()` (or `&mut call()`) where the `&` is an argument of a
  /// call, a method call or a macro's tuple: the borrow ends with the
  /// call, so the temporary is dropped at the end of the statement. A
  /// `let` that borrows it instead extends it (#400), and a tuple
  /// struct / variant constructor extends it too.
  fn borrowed_as_argument(&self, addr: &'tcx Expr<'tcx>, typeck: &'tcx rustc_middle::ty::TypeckResults<'tcx>) -> bool {
    use rustc_hir::Node;
    match self.tcx.parent_hir_node(addr.hir_id) {
      Node::Expr(g) => match g.kind {
        ExprKind::Call(callee, _) => !is_ctor_path(callee, typeck),
        ExprKind::MethodCall(..) => true,
        ExprKind::Tup(_) => addr.span.from_expansion(),
        _ => false,
      },
      Node::Stmt(_) => true,
      _ => false,
    }
  }

  /// True iff `e`, reached through `&`, field/index projections or
  /// aggregate literals, is the initializer of a `let`: Rust then
  /// extends the lifetime of a temporary borrowed on the way.
  fn extended_by_let(&self, e: &'tcx Expr<'tcx>) -> bool {
    use rustc_hir::Node;
    let mut cur = e.hir_id;
    let mut saw_addr = matches!(e.kind, ExprKind::AddrOf(..));
    loop {
      match self.tcx.parent_hir_node(cur) {
        Node::LetStmt(_) => return saw_addr,
        Node::Expr(p) => match p.kind {
          ExprKind::AddrOf(..) => saw_addr = true,
          ExprKind::Field(..) | ExprKind::Index(..) | ExprKind::Unary(UnOp::Deref, _)
          | ExprKind::Tup(_) | ExprKind::Array(_) | ExprKind::Struct(..) | ExprKind::Cast(..)
          | ExprKind::Call(..) | ExprKind::Block(..) => {}
          _ => return false,
        },
        Node::Block(_) => {}
        _ => return false,
      }
      cur = self.tcx.parent_hir_id(cur);
    }
  }

  /// When the temporary of `expr` is dropped, in words, and the line
  /// that happens on: the end of the enclosing statement, the block's
  /// final expression, the match arm, the `if` / `while` condition, the
  /// `for` loop or the closure body.
  fn temporary_scope_end(&self, expr: &'tcx Expr<'tcx>) -> Option<(&'static str, usize, Option<HirId>)> {
    use rustc_hir::Node;
    let mut cur = expr.hir_id;
    loop {
      let parent = self.tcx.parent_hir_node(cur);
      let end = |span: rustc_span::Span| span_end_line(&span, &self.tcx);
      match parent {
        Node::Stmt(s) => {
          let block = match self.tcx.parent_hir_node(s.hir_id) { Node::Block(b) => Some(b.hir_id), _ => None };
          return Some(("at the end of this statement", end(s.span), block));
        }
        Node::Block(b) if b.expr.is_some_and(|e| e.hir_id == cur) => {
          return Some(("at the end of the block's final expression", end(b.expr?.span), Some(b.hir_id)));
        }
        Node::Arm(a) if a.body.hir_id == cur => return Some(("at the end of this match arm", end(a.body.span), Some(a.body.hir_id))),
        Node::Expr(e) => match e.kind {
          ExprKind::DropTemps(inner) => {
            if matches!(e.span.desugaring_kind(), Some(rustc_span::DesugaringKind::ForLoop)) {
              return Some(("when the `for` loop ends", end(e.span), None));
            }
            // An `if` / `while` condition's temporaries are dropped
            // before its body runs.
            let is_cond = matches!(self.tcx.parent_hir_node(e.hir_id),
              Node::Expr(i) if matches!(i.kind, ExprKind::If(c, ..) if c.hir_id == e.hir_id));
            return is_cond.then(|| ("at the end of the condition", end(inner.span), None));
          }
          // `if` / `while` conditions, and the operands of `&&` / `||`,
          // are scopes of their own.
          ExprKind::If(c, ..) if c.hir_id == cur => {
            return Some(("at the end of the condition", end(c.span), None));
          }
          ExprKind::Binary(op, l, r)
            if matches!(op.node, rustc_hir::BinOpKind::And | rustc_hir::BinOpKind::Or)
              && (l.hir_id == cur || r.hir_id == cur) =>
          {
            let operand = if l.hir_id == cur { l } else { r };
            return Some(("at the end of this `&&` / `||` operand", end(operand.span), None));
          }
          ExprKind::Closure(c) if self.tcx.hir_body(c.body).value.hir_id == cur => {
            return Some(("at the end of the closure body", end(self.tcx.hir_body(c.body).value.span), None));
          }
          _ => {}
        },
        Node::Item(_) | Node::ImplItem(_) | Node::TraitItem(_) => return None,
        _ => {}
      }
      cur = self.tcx.parent_hir_id(cur);
    }
  }

  /// Destructuring assignment: `(a, b) = (b, a);`, `[x, y] = arr;`,
  /// `P { x, y } = p;`, `(s, n) = pair();`. rustc lowers it, before
  /// HIR, to a block
  ///
  /// ```text
  /// { let (lhs, lhs) = (b, a); a = lhs; b = lhs; }
  /// ```
  ///
  /// whose `let` has `LocalSource::AssignDesugar`. The `lhs` bindings
  /// are compiler temporaries, so they get no columns (#151). Each
  /// `a = lhs` is drawn as an assignment from the part of the RHS
  /// that `lhs` holds, paired up the same way `let` patterns are
  /// (`bind_pat`): `a = b`, `b = a`.
  ///
  /// The parts are all read before anything is assigned, so in a
  /// swap of Strings both values move out before either arrives. The
  /// assignments are drawn so that a variable's move out comes before
  /// its new value arrives where possible (`(b, a) = (c, b)` draws
  /// `a = b` first). A cycle (`(a, b) = (b, a)`) can't be ordered
  /// that way; after drawing it, each variable whose new value was
  /// drawn before its old one left gets a second "acquires" event so
  /// it ends up owning. Its old value is never drawn as dropped.
  ///
  /// Returns false (and does nothing) if `block` isn't this lowering.
  pub fn visit_destructuring_assign(&mut self, block: &'tcx rustc_hir::Block<'tcx>) -> bool {
    let (local, init) = match block.stmts.first() {
      Some(Stmt { kind: StmtKind::Let(local), .. })
        if matches!(local.source, rustc_hir::LocalSource::AssignDesugar) =>
      {
        match local.init {
          Some(init) => (local, init),
          None => return false,
        }
      }
      _ => return false,
    };
    // The remaining statements are `place = lhs;`, one per binding.
    let mut assigns: Vec<(&'tcx Expr<'tcx>, &'tcx Expr<'tcx>, HirId)> = Vec::new();
    for stmt in &block.stmts[1..] {
      let e = match stmt.kind {
        StmtKind::Semi(e) | StmtKind::Expr(e) => e,
        _ => return false,
      };
      match e.kind {
        ExprKind::Assign(place, rhs, _) => match local_of_path(rhs) {
          Some(hid) => assigns.push((e, place, hid)),
          None => return false,
        },
        _ => return false,
      }
    }
    if block.expr.is_some() {
      return false;
    }

    // Calls, borrows and reads inside the RHS.
    self.visit_expr(init);

    let mut pairs: Vec<(HirId, &'tcx Expr<'tcx>)> = Vec::new();
    let mut discarded: Vec<&'tcx Expr<'tcx>> = Vec::new();
    self.pair_destructure(local.pat, init, &mut pairs, &mut discarded);
    let src_of: HashMap<HirId, &'tcx Expr<'tcx>> = pairs.iter().cloned().collect();

    // Places whose value this statement moves out (a non-Copy RHS
    // part that names a variable, `b` in `(a, b) = (b, a)`).
    let moved_name = |this: &Self, e: &'tcx Expr<'tcx>| -> Option<String> {
      if !matches!(e.kind, ExprKind::Path(_) | ExprKind::Field(..)) {
        return None;
      }
      let ty = this.tcx.typeck(e.hir_id.owner).node_type(e.hir_id);
      if ty.is_ref() && ty.ref_mutability() == Some(Mutability::Not) {
        return None;
      }
      if !ty.is_ref() && this.ty_is_copy(ty, e.hir_id.owner) {
        return None;
      }
      let name = expr_to_rap_name(e, &this.tcx)?;
      this.raps.contains_key(&name).then_some(name)
    };
    let mut moved: HashSet<String> = HashSet::new();
    for (_, e) in &pairs {
      if let Some(n) = moved_name(self, e) { moved.insert(n); }
    }
    for e in &discarded {
      if let Some(n) = moved_name(self, e) { moved.insert(n); }
    }

    // Draw order: an assignment to `x` waits for the assignment that
    // moves x's old value out (Kahn's algorithm, ties in source
    // order, so the output is deterministic).
    let dest_name: Vec<Option<String>> = assigns.iter()
      .map(|(_, place, _)| expr_to_rap_name(place, &self.tcx)).collect();
    let src_moved: Vec<Option<String>> = assigns.iter()
      .map(|(_, _, hid)| src_of.get(hid).and_then(|e| moved_name(self, e))).collect();
    let n = assigns.len();
    let mut order: Vec<usize> = Vec::with_capacity(n);
    let mut done = vec![false; n];
    while order.len() < n {
      let ready = (0..n).find(|&i| !done[i] && !(0..n).any(|j| {
        !done[j] && j != i && dest_name[i].is_some() && src_moved[j] == dest_name[i]
      }));
      // Only a cycle is left: take the first one in source order.
      let i = ready.unwrap_or_else(|| (0..n).find(|&i| !done[i]).unwrap());
      done[i] = true;
      order.push(i);
    }
    // Destinations whose new value is drawn before their old value
    // moves out (cycles only).
    let mut reacquire: Vec<String> = Vec::new();
    for (pos, &i) in order.iter().enumerate() {
      if let Some(d) = &dest_name[i] {
        if order[pos + 1..].iter().any(|&j| src_moved[j].as_ref() == Some(d)) && !reacquire.contains(d) {
          reacquire.push(d.clone());
        }
      }
    }

    // A reference destination's new lender, before anything moves.
    let mut ref_data: HashMap<HirId, (ResourceTy, VecDeque<String>, Option<StaticLender>, Vec<ResourceTy>)> = HashMap::new();
    for (_, place, hid) in &assigns {
      let ty = self.tcx.typeck(place.hir_id.owner).node_type(place.hir_id);
      if ty.is_ref() {
        if let Some(src) = src_of.get(hid) {
          ref_data.insert(*hid, self.get_ref_data_full(src));
        }
      }
    }

    {
      let lo = span_to_line(&local.span, &self.tcx);
      let hi = self.tcx.sess.source_map().lookup_char_pos(local.span.hi()).line;
      for d in dest_name.iter().flatten() {
        if moved.contains(d) {
          self.destructure_swapped.push((d.clone(), lo, hi));
        }
      }
    }
    self.destructure_srcs = src_of;
    self.destructure_moved = moved;
    self.destructure_ref_data = ref_data;
    // Parts nothing is assigned to (`(a, _) = (s, t)`) are moved
    // into the temporary and dropped with it, at the end of the
    // statement. The temporary has no column, so the move's tooltip
    // is where that drop is stated (unless the type has nothing to
    // drop).
    for e in &discarded {
      if moved_name(self, e).is_some() {
        let ty = self.tcx.typeck(e.hir_id.owner).node_type(e.hir_id);
        let needs_drop = ty.needs_drop(self.tcx, rustc_middle::ty::TypingEnv::post_analysis(self.tcx, e.hir_id.owner));
        let to = if needs_drop { ResourceTy::Temporary { extended: false } } else { ResourceTy::Anonymous };
        self.match_rhs(to, e, Evt::Move);
      }
    }
    for &i in &order {
      self.visit_expr(assigns[i].0);
    }
    let line = span_to_line(&local.span, &self.tcx);
    // A struct's non-Copy fields moved out with it, so they own
    // again too.
    let mut stack: Vec<String> = reacquire;
    stack.reverse();
    while let Some(name) = stack.pop() {
      if let Some(rd) = self.raps.get(&name) {
        let rap = rd.rap.clone();
        self.add_ev(line, Evt::Bind, ResourceTy::Value(rap), ResourceTy::Anonymous, false);
        let mut children: Vec<String> = self.struct_children(&name).into_iter()
          .filter(|c| !c.is_copy())
          .map(|c| c.name().clone())
          .collect();
        children.reverse();
        stack.extend(children);
      }
    }
    self.destructure_srcs.clear();
    self.destructure_moved.clear();
    self.destructure_ref_data.clear();
    true
  }

  /// Pair each binding of a destructuring assignment's pattern with
  /// the part of the RHS it receives (`(a, b)` against `(e1, e2)`
  /// gives a ← e1, b ← e2). Where the RHS isn't written out in the
  /// same shape (`(a, b) = pair()`, `(a, b) = t`), every binding
  /// inside gets the whole expression, as `let` does (`bind_walk`).
  /// RHS parts no binding receives (`_`, `..`) go to `discarded`.
  fn pair_destructure(
    &self,
    pat: &'tcx Pat<'tcx>,
    src: &'tcx Expr<'tcx>,
    pairs: &mut Vec<(HirId, &'tcx Expr<'tcx>)>,
    discarded: &mut Vec<&'tcx Expr<'tcx>>,
  ) {
    // `(p1, .., pn)` against `(e1, …, em)`: the parts before `..`
    // pair from the front, the rest from the back.
    let pair_seq = |this: &Self, pats: &'tcx [Pat<'tcx>], dd: Option<usize>, exprs: &'tcx [Expr<'tcx>],
                        pairs: &mut Vec<(HirId, &'tcx Expr<'tcx>)>, discarded: &mut Vec<&'tcx Expr<'tcx>>| -> bool {
      let split = dd.unwrap_or(pats.len());
      let (front, back) = pats.split_at(split);
      if front.len() + back.len() > exprs.len() || (dd.is_none() && pats.len() != exprs.len()) {
        return false;
      }
      let back_start = exprs.len() - back.len();
      for (p, e) in front.iter().zip(exprs.iter()) {
        this.pair_destructure(p, e, pairs, discarded);
      }
      for e in &exprs[front.len()..back_start] {
        discarded.push(e);
      }
      for (p, e) in back.iter().zip(exprs[back_start..].iter()) {
        this.pair_destructure(p, e, pairs, discarded);
      }
      true
    };
    let paired = match (pat.kind, src.kind) {
      (PatKind::Binding(_, hid, _, None), _) => {
        pairs.push((hid, src));
        true
      }
      (PatKind::Wild, _) => {
        discarded.push(src);
        true
      }
      (PatKind::Tuple(pats, dd), ExprKind::Tup(exprs)) => {
        pair_seq(self, pats, dd.as_opt_usize(), exprs, pairs, discarded)
      }
      (PatKind::TupleStruct(_, pats, dd), ExprKind::Call(callee, args))
        if matches!(callee.kind, ExprKind::Path(QPath::Resolved(_, p))
          if matches!(p.res, Res::Def(DefKind::Ctor(..), _))) =>
      {
        pair_seq(self, pats, dd.as_opt_usize(), args, pairs, discarded)
      }
      (PatKind::Slice(before, mid, after), ExprKind::Array(elems))
        if mid.is_none_or(|m| matches!(m.kind, PatKind::Wild)) =>
      {
        let dd = mid.map(|_| before.len());
        let pats: Vec<&'tcx Pat<'tcx>> = before.iter().chain(after.iter()).collect();
        let split = dd.unwrap_or(pats.len());
        if pats.len() > elems.len() || (dd.is_none() && pats.len() != elems.len()) {
          false
        } else {
          let back_start = elems.len() - (pats.len() - split);
          for (p, e) in pats[..split].iter().zip(elems.iter()) {
            self.pair_destructure(p, e, pairs, discarded);
          }
          for e in &elems[split..back_start] {
            discarded.push(e);
          }
          for (p, e) in pats[split..].iter().zip(elems[back_start..].iter()) {
            self.pair_destructure(p, e, pairs, discarded);
          }
          true
        }
      }
      (PatKind::Struct(_, pat_fields, _), ExprKind::Struct(_, expr_fields, rustc_hir::StructTailExpr::None)) => {
        for ef in expr_fields.iter() {
          match pat_fields.iter().find(|pf| pf.ident.name == ef.ident.name) {
            Some(pf) => self.pair_destructure(pf.pat, ef.expr, pairs, discarded),
            None => discarded.push(ef.expr),
          }
        }
        true
      }
      _ => false,
    };
    if !paired {
      // Shapes don't line up: every binding receives (part of) src.
      pat.walk_always(|p| {
        if let PatKind::Binding(_, hid, _, _) = p.kind {
          pairs.push((hid, src));
        }
      });
    }
  }

  /// Entry point for handling `let pat = init [else { … }];`.
  ///
  /// Walks the init expression once for side-effect events (calls,
  /// borrows, struct constructions, etc.), then dispatches to
  /// [`bind_pat`] which decomposes composite patterns against the
  /// init's shape. Issue #86 rolled this out as a replacement for the
  /// old `match local.pat.kind` in `visit_local`, which only handled
  /// `PatKind::Binding` and silently dropped everything else (so
  /// `let (a, b) = (…, …);` produced an empty timeline).
  ///
  /// `is_skipped` is true when the `let`'s source line carries a
  /// `// rv-skip` marker. Same semantics as before: define the LHS
  /// RAPs so later references still resolve, but suppress the RHS
  /// visit and the move/copy event so nothing touching this binding
  /// renders.
  pub fn process_let_binding(
    &mut self,
    pat: &'tcx Pat<'tcx>,
    init: &'tcx Expr<'tcx>,
    is_skipped: bool,
  ) {
    // `let y = dbg!(e);` binds y to e's value: dbg! takes e by value
    // and returns it. Treat it as `let y = e;` so the move / copy /
    // borrow (and, for `dbg!(&x)`, the lender) is e's.
    let init = dbg_single_arg(init).unwrap_or(init);
    // For `let s = if … { … } else { … };` (and the match analog),
    // emit the Move-into-LHS events FIRST. Match_rhs's If/Match arms
    // recurse into each branch's trailing expression and emit a
    // Move/Copy/Bind event at that line — when those events exist
    // in `preprocessed_events` before visit_expr's If arm filters
    // events into per-branch e_data, the LHS naturally becomes a
    // branched timeline with an Acquire dot inside each branch and
    // the merge dot picks up the BoundHere join tooltip.
    //
    // For other init shapes the original ordering (visit RHS for
    // side effects, then register LHS) stays — there's no Branch
    // event to fold the bind into. That includes `let n = e?;`:
    // the `?` lowers to a `MatchSource::TryDesugar` match, but
    // visit_expr emits no Branch for it and match_rhs unwraps it to
    // `e`, so it wants the plain visit-then-bind order (visiting `e`
    // first registers the callee, which gives the bind its source).
    // `let r = &String::from("a");`: the String gets a column of its
    // own (#400).
    if !is_skipped && self.bind_extended_temporary(pat, init) {
      return;
    }
    let init_is_branching = !is_skipped
      && match init.kind {
        ExprKind::If(..) => true,
        ExprKind::Match(_, _, rustc_hir::MatchSource::TryDesugar(_)) => false,
        ExprKind::Match(..) => true,
        _ => false,
      };
    if init_is_branching {
      self.bind_pat(pat, init, is_skipped);
      self.visit_expr(init);
      // A reference returned by a call in a branch keeps that call's
      // borrow (#337).
      if let PatKind::Binding(_, _, ident, None) = pat.kind {
        self.mark_branch_results_kept(ident.as_str(), init);
      }
    } else {
      if !is_skipped {
        self.visit_expr(init);
      }
      self.bind_pat(pat, init, is_skipped);
    }
  }

  /// `let r = &String::from("a");` (also `&mut`, `&s.clone()`,
  /// `&User { .. }`): the `&` borrows a temporary, and because a `let`
  /// borrows it, Rust extends the temporary's lifetime to the end of
  /// the enclosing block (temporary lifetime extension). It is dropped
  /// there, not at the end of the statement (#400). Draw it as
  /// `let tmp = String::from("a"); let r = &tmp;` would be: a column
  /// labelled `(temporary)` (numbered from the second one in a fn)
  /// that acquires the value, lends it to r, and drops it at scope
  /// end.
  ///
  /// Only for a temporary that owns a resource (its type needs
  /// dropping): `&Point { x: 4 }` with Copy fields has nothing to
  /// drop, and `&5` is promoted to a constant. Temporaries that die
  /// at the end of their statement (`f(&String::from("a"))`) aren't
  /// drawn. Returns false when `let pat = init` isn't this shape.
  fn bind_extended_temporary(&mut self, pat: &'tcx Pat<'tcx>, init: &'tcx Expr<'tcx>) -> bool {
    let PatKind::Binding(annotation, hirid, ident, None) = pat.kind else { return false };
    if annotation.0 != rustc_hir::ByRef::No || init.span.from_expansion() {
      return false;
    }
    let ExprKind::AddrOf(rustc_hir::BorrowKind::Ref, borrow_mut, operand) = init.kind else { return false };
    // `&x`, `&v[0]`, `&p.f`, `&*r` borrow a place, not a temporary.
    if matches!(operand.kind,
      ExprKind::Path(_) | ExprKind::Field(..) | ExprKind::Index(..) | ExprKind::Unary(UnOp::Deref, _)
      | ExprKind::AddrOf(..)) {
      return false;
    }
    let typeck = self.tcx.typeck(hirid.owner);
    let lhs_ty = typeck.node_type(hirid);
    let tmp_ty = typeck.expr_ty(operand);
    let env = rustc_middle::ty::TypingEnv::post_analysis(self.tcx, hirid.owner);
    if !lhs_ty.is_ref() || !tmp_ty.needs_drop(self.tcx, env) {
      return false;
    }
    self.visit_expr(init);

    let line_num = expr_to_line(init, &self.tcx);
    self.temp_count += 1;
    let tmp_name = temporary_label(self.temp_count);
    // `&mut Vec::new()` writes through the borrow: the temporary is
    // a mutable place.
    self.define_lhs(tmp_name.clone(), borrow_mut == Mutability::Mut, operand, tmp_ty);
    let tmp = self.raps.get(&tmp_name).unwrap().rap.to_owned();
    let n_events = self.preprocessed_events.len();
    self.match_rhs(ResourceTy::Value(tmp.clone()), operand, Evt::Move);
    if self.preprocessed_events.len() == n_events {
      // A shape match_rhs has nothing to say about: the temporary
      // still holds the value from here on.
      self.add_ev(line_num, Evt::Bind, ResourceTy::Value(tmp.clone()), ResourceTy::Anonymous, false);
    }

    let name = ident.to_string();
    self.add_ref(name.clone(), bool_of_mut(lhs_ty.ref_mutability().unwrap()), bool_of_mut(annotation.1),
      line_num, ResourceTy::Value(tmp.clone()), VecDeque::new(), self.current_scope, !self.inside_branch,
      self.ref_pointee_is_copy(lhs_ty, hirid.owner));
    let r_data = self.raps.get_mut(&name).unwrap();
    r_data.extended_temp = Some(tmp_name);
    let r = r_data.rap.to_owned();
    let evt = match borrow_mut {
      Mutability::Mut => Evt::MBorrow,
      Mutability::Not => Evt::SBorrow,
    };
    self.add_borrow_ev(line_num, evt, ResourceTy::Value(r), ResourceTy::Value(tmp), false);
    true
  }

  /// Structural pat-against-init decomposition. When the pattern's
  /// shape matches the init expression's shape — tuple-on-tuple,
  /// tuple-struct-on-call, struct-on-struct, ref-on-addrof, or
  /// slice-on-array — recurse element-wise so each binding gets
  /// paired with the corresponding sub-expression as its source.
  /// Otherwise fall back to [`bind_walk`], which registers every
  /// binding in the pattern against the whole init — semantically
  /// approximate, but never silently empty.
  fn bind_pat(
    &mut self,
    pat: &'tcx Pat<'tcx>,
    init: &'tcx Expr<'tcx>,
    is_skipped: bool,
  ) {
    match (pat.kind, init.kind) {
      // `let (a, b) = (e1, e2);` — Issue #86. We require an exact
      // arity match with no `..` so the field-to-pattern correspondence
      // is unambiguous; mismatched-arity / `..`-bearing tuples fall
      // through to bind_walk.
      (PatKind::Tuple(sub_pats, ddpos), ExprKind::Tup(sub_exprs))
        if ddpos.as_opt_usize().is_none()
          && sub_pats.len() == sub_exprs.len() =>
      {
        for (sp, se) in sub_pats.iter().zip(sub_exprs.iter()) {
          self.bind_pat(sp, se, is_skipped);
        }
      }
      // `let Foo(a, b) = Foo(e1, e2);` — single-variant tuple struct
      // (the only case `let` admits irrefutably). Same arity guard
      // as above.
      (PatKind::TupleStruct(_, sub_pats, ddpos), ExprKind::Call(_, sub_exprs))
        if ddpos.as_opt_usize().is_none()
          && sub_pats.len() == sub_exprs.len() =>
      {
        for (sp, se) in sub_pats.iter().zip(sub_exprs.iter()) {
          self.bind_pat(sp, se, is_skipped);
        }
      }
      // `let Foo { a, b } = Foo { a: e1, b: e2 };` — pair patterns to
      // initializer fields by name. Pat fields with no matching expr
      // field (struct-update syntax `..base`) fall through to
      // bind_walk against the whole init.
      (PatKind::Struct(_, pat_fields, _), ExprKind::Struct(_, expr_fields, _)) => {
        for pf in pat_fields {
          match expr_fields.iter().find(|f| f.ident.name == pf.ident.name) {
            Some(ef) => self.bind_pat(pf.pat, ef.expr, is_skipped),
            None => self.bind_walk(pf.pat, init, is_skipped),
          }
        }
      }
      // `let (a, b) = t;` / `let P { a, b } = p;` with `t` / `p` a
      // tuple or struct variable (or a field of one): its parts `t.0`,
      // `p.a`, … have columns, so pair each sub-pattern with its part
      // (#189, #326). The other shapes fall through to bind_walk, which
      // binds everything from the whole of `t`.
      (PatKind::Tuple(..) | PatKind::TupleStruct(..) | PatKind::Struct(..), ExprKind::Path(_) | ExprKind::Field(..))
        if self.member_place_name(init).is_some() =>
      {
        let place = self.member_place_name(init).unwrap();
        let ty = self.tcx.typeck(init.hir_id.owner).node_type(init.hir_id);
        self.bind_pat_from_member(pat, &place, ty, init, is_skipped);
      }
      // `let &x = &y;` — recurse through the borrow on both sides.
      (PatKind::Ref(inner_pat, _, _), ExprKind::AddrOf(_, _, inner_init)) => {
        self.bind_pat(inner_pat, inner_init, is_skipped);
      }
      // `let [a, b] = [e1, e2];` — fixed-length slice destructure
      // against an array literal of matching length. Pair element-wise.
      // The mid-`Some` case (`let [a, .., b] = …`) only pairs cleanly
      // when `mid` is Wild — when it's a binding we'd need a
      // slice-typed source for it, which we don't have without MIR.
      // Mid-binding falls through to bind_walk.
      (PatKind::Slice(before, mid, after), ExprKind::Array(elems))
        if mid.is_none_or(|m| matches!(m.kind, PatKind::Wild))
          && before.len() + after.len() <= elems.len()
          && (mid.is_some() || before.len() + after.len() == elems.len()) =>
      {
        for (sp, se) in before.iter().zip(elems.iter()) {
          self.bind_pat(sp, se, is_skipped);
        }
        let after_start = elems.len() - after.len();
        for (sp, se) in after.iter().zip(elems[after_start..].iter()) {
          self.bind_pat(sp, se, is_skipped);
        }
        // mid-Wild: middle elements are explicitly discarded; their
        // side effects were captured by the entry visit_expr.
      }
      // Canonical leaf: a single named binding consuming the init.
      (PatKind::Binding(annotation, hirid, ident, sub_pat), _) => {
        self.bind_one(ident.to_string(), annotation, hirid, init, is_skipped);
        // `name @ inner_pat` — register any nested bindings too.
        if let Some(sp) = sub_pat {
          self.bind_walk(sp, init, is_skipped);
        }
      }
      // `let _ = expr;` — value discarded. The RHS visit at entry
      // already covered side effects; nothing else to register.
      (PatKind::Wild, _) => {}
      // Shape mismatch (e.g. tuple pat against a non-tuple init), or
      // a refutable pattern under `let-else`, or a pattern shape we
      // don't structurally decompose (slice, or, deref, …). Best
      // effort: register every Binding the pattern names so the
      // timeline isn't empty.
      _ => self.bind_walk(pat, init, is_skipped),
    }
  }

  /// `init` names a tuple- or struct-typed variable or field (`t`,
  /// `p`, `p.pair`) whose parts have columns of their own: its RAP
  /// name.
  fn member_place_name(&self, init: &'tcx Expr<'tcx>) -> Option<String> {
    let ty = self.tcx.typeck(init.hir_id.owner).node_type(init.hir_id);
    if !self.has_member_columns(ty) || init.span.from_expansion() {
      return None;
    }
    // Only a chain of fields on a variable (`t`, `p.pair`, `t.0`):
    // expr_to_rap_name also looks through `*`, `&` and indexing.
    let mut cur = init;
    while let ExprKind::Field(inner, _) = cur.kind {
      cur = inner;
    }
    if !matches!(cur.kind, ExprKind::Path(QPath::Resolved(..))) {
      return None;
    }
    let name = expr_to_rap_name(init, &self.tcx)?;
    let has_members = self.member_fields(ty).iter()
      .any(|(f, _)| self.raps.contains_key(&format!("{}.{}", name, f)));
    has_members.then_some(name)
  }

  /// Destructure the part `member` (of type `member_ty`) of a tuple or
  /// struct place whose parts have columns: `let (a, b) = t;` moves
  /// (or copies) `t.0` into `a` and `t.1` into `b`, `let P { a, .. } =
  /// p;` moves `p.a` into `a`, and a `_` (or a field left out by `..`)
  /// leaves its part where it is (it isn't moved, and is dropped with
  /// the whole). Shapes this doesn't pair up fall back to
  /// [`bind_walk`] against the whole `init`.
  fn bind_pat_from_member(
    &mut self,
    pat: &'tcx Pat<'tcx>,
    member: &str,
    member_ty: rustc_middle::ty::Ty<'tcx>,
    init: &'tcx Expr<'tcx>,
    is_skipped: bool,
  ) {
    match pat.kind {
      PatKind::Wild => {}
      // A by-value binding of a non-reference part.
      PatKind::Binding(annotation, hirid, ident, None)
        if matches!(annotation.0, rustc_hir::ByRef::No) && !member_ty.is_ref()
          && self.raps.contains_key(member) =>
      {
        let name = ident.to_string();
        let lhs_ty = self.tcx.typeck(hirid.owner).node_type(hirid);
        let evt = if self.ty_is_copy(lhs_ty, hirid.owner) { Evt::Copy } else { Evt::Move };
        self.define_lhs(name.clone(), bool_of_mut(annotation.1), init, lhs_ty);
        if is_skipped {
          self.skip_raps.insert(name);
          return;
        }
        let line_num = span_to_line(&init.span, &self.tcx);
        let from = self.raps.get(member).unwrap().rap.to_owned();
        self.update_rap(&from, line_num);
        let to = self.raps.get(&name).unwrap().rap.to_owned();
        self.add_ev(line_num, evt, ResourceTy::Value(to), ResourceTy::Value(from), false);
      }
      _ => {
        let parts = self.raps.get(member)
          .map(|rd| ResourceTy::Value(rd.rap.to_owned()))
          .and_then(|from| self.pat_parts(pat, &from, member_ty));
        match parts {
          Some(subs) => {
            for (sp, part) in subs {
              match part {
                Some((ResourceTy::Value(r), part_ty)) => {
                  self.bind_pat_from_member(sp, r.name(), part_ty, init, is_skipped);
                }
                _ => self.bind_walk(sp, init, is_skipped),
              }
            }
          }
          None => self.bind_walk(pat, init, is_skipped),
        }
      }
    }
  }

  /// Walk every `PatKind::Binding` reachable inside `pat` and register
  /// each one as bound from the whole `init` expression. Used as the
  /// fallback for pattern shapes [`bind_pat`] can't pair element-wise
  /// against the init. Every PatKind variant is matched explicitly so
  /// future additions surface as build errors rather than silent
  /// drops.
  fn bind_walk(
    &mut self,
    pat: &'tcx Pat<'tcx>,
    init: &'tcx Expr<'tcx>,
    is_skipped: bool,
  ) {
    match pat.kind {
      PatKind::Binding(annotation, hirid, ident, sub_pat) => {
        self.bind_one(ident.to_string(), annotation, hirid, init, is_skipped);
        if let Some(sp) = sub_pat {
          self.bind_walk(sp, init, is_skipped);
        }
      }
      PatKind::Tuple(pats, _) | PatKind::TupleStruct(_, pats, _) => {
        for p in pats {
          self.bind_walk(p, init, is_skipped);
        }
      }
      PatKind::Struct(_, fields, _) => {
        for f in fields {
          self.bind_walk(f.pat, init, is_skipped);
        }
      }
      PatKind::Or(pats) => {
        // Or-pattern alternatives bind the same names; registering
        // from the first alt is enough — the other alts would
        // duplicate the same RAPs.
        if let Some(first) = pats.first() {
          self.bind_walk(first, init, is_skipped);
        }
      }
      PatKind::Ref(inner, _, _)
      | PatKind::Box(inner)
      | PatKind::Deref(inner)
      | PatKind::Guard(inner, _) => {
        self.bind_walk(inner, init, is_skipped);
      }
      PatKind::Slice(before, mid, after) => {
        for p in before { self.bind_walk(p, init, is_skipped); }
        if let Some(p) = mid { self.bind_walk(p, init, is_skipped); }
        for p in after { self.bind_walk(p, init, is_skipped); }
      }
      // No bindings inside these.
      PatKind::Wild
      | PatKind::Never
      | PatKind::Missing
      | PatKind::Err(_)
      | PatKind::Expr(_)
      | PatKind::Range(..) => {}
    }
  }

  /// Register a single named binding `name` introduced by a `let`
  /// pattern, with `init` as its resource source. Same effect the
  /// pre-#86 `PatKind::Binding` arm had: pick Move vs. Copy from the
  /// LHS type, define the LHS RAP, and emit the move/copy event —
  /// unless the binding's source line is on the skip list, in which
  /// case we still register the RAP (so later references resolve)
  /// but record it in `skip_raps` so events touching it get dropped.
  /// Walk a pattern (`(a, b)`, `Some(x)`, etc.) and register each
  /// binding as a branch-scoped RAP with a synthetic Bind from
  /// Anonymous on `line`. Used for for-loop tuple patterns where the
  /// "real" source (the iter expression's tail call) doesn't render
  /// cleanly — see the for-loop arm of `visit_expr` for context.
  pub fn register_pattern_bindings_anon(&mut self, pat: &'tcx Pat<'tcx>, line: usize) {
    match pat.kind {
      PatKind::Binding(_, hirid, ident, sub_pat) => {
        let name = ident.to_string();
        let pat_ty = self.tcx.typeck(hirid.owner).pat_ty(pat);
        if pat_ty.is_ref() {
          let pointee_copy = self.ref_pointee_is_copy(pat_ty, hirid.owner);
          self.add_ref(
            name.clone(),
            bool_of_mut(pat_ty.ref_mutability().unwrap()),
            false,
            line,
            ResourceTy::Anonymous,
            std::collections::VecDeque::new(),
            self.current_scope,
            false,
            pointee_copy,
          );
        } else {
          let is_copy = self.tcx.type_is_copy_modulo_regions(
            rustc_middle::ty::TypingEnv::post_analysis(self.tcx, hirid.owner),
            pat_ty,
          );
          self.add_owner(name.clone(), false, is_copy, self.ty_is_plain(pat_ty, hirid.owner), self.ty_has_interior_mut(pat_ty, hirid.owner), self.current_scope, false);
          self.mark_owner_type(&name, pat_ty);
        }
        self.annotate_src(name.clone(), ident.span, false,
          *self.raps.get(&name).unwrap().rap.hash());
        // Acquire event so the column has at least one entry on its
        // timeline (otherwise the renderer gives it no column).
        let to_rty = ResourceTy::Value(self.raps.get(&name).unwrap().rap.to_owned());
        self.add_ev(line, Evt::Bind, to_rty, ResourceTy::Anonymous, false);
        if let Some(sp) = sub_pat {
          self.register_pattern_bindings_anon(sp, line);
        }
      }
      PatKind::Tuple(ps, _) | PatKind::TupleStruct(_, ps, _) => {
        for p in ps { self.register_pattern_bindings_anon(p, line); }
      }
      PatKind::Struct(_, fs, _) => {
        for f in fs { self.register_pattern_bindings_anon(f.pat, line); }
      }
      PatKind::Or(ps) => {
        // alternatives bind the same names — first is enough.
        if let Some(p) = ps.first() { self.register_pattern_bindings_anon(p, line); }
      }
      PatKind::Ref(inner, _, _)
      | PatKind::Box(inner)
      | PatKind::Deref(inner)
      | PatKind::Guard(inner, _) => {
        self.register_pattern_bindings_anon(inner, line);
      }
      PatKind::Slice(before, mid, after) => {
        for p in before { self.register_pattern_bindings_anon(p, line); }
        if let Some(p) = mid { self.register_pattern_bindings_anon(p, line); }
        for p in after { self.register_pattern_bindings_anon(p, line); }
      }
      // No bindings inside.
      PatKind::Wild | PatKind::Never | PatKind::Missing | PatKind::Err(_)
      | PatKind::Expr(_) | PatKind::Range(..) => {}
    }
  }

  fn bind_one(
    &mut self,
    name: String,
    annotation: BindingMode,
    hirid: HirId,
    init: &'tcx Expr<'tcx>,
    is_skipped: bool,
  ) {
    let tycheck_results = self.tcx.typeck(hirid.owner);
    let lhs_ty = tycheck_results.node_type(hirid);
    // `let ref r = s;` is `let r = &s;`: a borrow, not a copy (#380).
    if let Some(mutbl) = self.own_ref_binding(hirid) {
      self.bind_ref_binding(name, annotation, hirid, init, is_skipped, mutbl, lhs_ty);
      return;
    }
    let is_copyable = self.tcx.type_is_copy_modulo_regions(
      rustc_middle::ty::TypingEnv::post_analysis(self.tcx, hirid.owner),
      lhs_ty,
    );
    let evt = if lhs_ty.is_ref() {
      match lhs_ty.ref_mutability().unwrap() {
        Mutability::Not => Evt::Copy,
        Mutability::Mut => Evt::Move,
      }
    } else if is_copyable {
      Evt::Copy
    } else {
      Evt::Move
    };
    self.define_lhs(name.clone(), bool_of_mut(annotation.1), init, lhs_ty);
    if is_skipped {
      self.skip_raps.insert(name);
    } else {
      let rap = self.raps.get(&name).unwrap().rap.to_owned();
      self.match_rhs(ResourceTy::Value(rap), init, evt);
      self.emit_holder_literal_borrows(&name, init);
    }
  }

  /// Move vs. Copy for binding a value of type `ty` (a `&T` is Copy,
  /// a `&mut T` moves); same choice as bind_one.
  fn bind_evt(&self, ty: rustc_middle::ty::Ty<'tcx>, hirid: HirId) -> Evt {
    if ty.is_ref() {
      match ty.ref_mutability().unwrap() {
        Mutability::Not => Evt::Copy,
        Mutability::Mut => Evt::Move,
      }
    } else if self.ty_is_copy(ty, hirid.owner) {
      Evt::Copy
    } else {
      Evt::Move
    }
  }

  /// The variables that came into existence while one branch of a
  /// conditional was visited: every RAP registered since the RAP
  /// counter stood at `first_hash` (hashes are handed out in order).
  /// That's the branch's own `let`s and pattern bindings, but also
  /// those of an inline `if` / `if let` / `else if let` / `for` nested
  /// in it (#501), which the branch's top-level statements don't show.
  /// They exist only inside the branch, so they mustn't be drawn as
  /// variables live across it. Left out: functions (call-site labels),
  /// and `let x;` locals declared before the branch and first assigned
  /// in it (`uninit`, #234), which do exist before the branch.
  fn raps_declared_since(
    &self,
    first_hash: usize,
    uninit: &HashMap<HirId, DeferredLet>,
  ) -> HashSet<ResourceAccessPoint> {
    #[expect(clippy::disallowed_methods, reason = "collected into a set; order is lost")]
    let deferred: HashSet<String> = uninit.keys().map(|h| self.tcx.hir_name(*h).to_string()).collect();
    #[expect(clippy::disallowed_methods, reason = "collected into a set; order is lost")]
    let declared = self.raps.values()
      .map(|rd| &rd.rap)
      // A deferred struct's field columns (`p.x`) come with it.
      .filter(|r| *r.hash() >= first_hash as u64 && !r.is_fn()
        && !r.name().split('.').next().is_some_and(|base| deferred.contains(base)))
      .cloned()
      .collect();
    declared
  }

  /// Set the uninitialized-locals state after a branch construct from
  /// the state at the end of each branch (`None` = the implicit empty
  /// else of an `if`). A `let x;` local is still definitely
  /// uninitialized after the join iff it is so at the end of every
  /// branch that reaches the join; branches of type `!` (`return`,
  /// `panic!()`) don't. If x is initialized on only some paths, a later
  /// `x = …` is treated as a reassignment.
  fn join_uninit_locals(&mut self, ends: Vec<(Option<&'tcx Expr<'tcx>>, HashMap<HirId, DeferredLet>)>) {
    let diverges = |e: &Option<&'tcx Expr<'tcx>>| {
      e.is_some_and(|e| self.expr_diverges(e))
    };
    let mut reaching: Vec<HashMap<HirId, DeferredLet>> =
      ends.iter().filter(|(e, _)| !diverges(e)).map(|(_, m)| m.clone()).collect();
    if reaching.is_empty() {
      reaching = ends.into_iter().map(|(_, m)| m).collect();
    }
    let mut joined = reaching.pop().unwrap_or_default();
    joined.retain(|id, _| reaching.iter().all(|m| m.contains_key(id)));
    self.uninit_locals = joined;
  }

  /// The first assignment `x = rhs` to a `let x;` binding (#234). It
  /// initializes x exactly as `let x = rhs;` would: register the RAP
  /// (with the `let`'s scope, so x lives to the end of its block),
  /// then emit the move/copy/borrow into it. No drop and no ref-die,
  /// since x held nothing before. In a later sibling branch the RAP
  /// already exists and only the event is emitted.
  fn init_deferred_local(&mut self, hirid: HirId, decl: DeferredLet, rhs: &'tcx Expr<'tcx>) {
    let name = self.tcx.hir_name(hirid).to_string();
    let lhs_ty = self.tcx.typeck(hirid.owner).node_type(hirid);
    let evt = self.bind_evt(lhs_ty, hirid);
    let saved = (self.current_scope, self.inside_branch);
    (self.current_scope, self.inside_branch) = (decl.scope, decl.inside_branch);
    self.define_lhs(name.clone(), bool_of_mut(decl.annotation.1), rhs, lhs_ty);
    (self.current_scope, self.inside_branch) = saved;
    if decl.is_skipped {
      self.skip_raps.insert(name);
    } else {
      let rap = self.raps.get(&name).unwrap().rap.to_owned();
      self.match_rhs(ResourceTy::Value(rap), rhs, evt);
    }
  }

  /// The mutability of the reference a `let` pattern's binding `hirid`
  /// makes when it's an explicit `ref` / `ref mut` on a value that
  /// isn't behind a reference (`let ref r = s;`, `let P { ref name, ..
  /// } = p;`), so it borrows the matched place itself. `None` for a
  /// by-value binding, and for one bound by reference because the
  /// pattern goes through a reference, written (`let &(ref a, _) =
  /// &t;`) or implicit (`let (a, _) = &t;`): those bind a reference
  /// the initializer already holds, which `match_rhs` draws.
  fn own_ref_binding(&self, hirid: HirId) -> Option<Mutability> {
    let typeck = self.tcx.typeck(hirid.owner);
    let mutbl = match typeck.pat_binding_modes().get(hirid)?.0 {
      rustc_hir::ByRef::Yes(_, m) => m,
      rustc_hir::ByRef::No => return None,
    };
    let mut id = hirid;
    loop {
      if typeck.pat_adjustments().get(id).is_some_and(|a| !a.is_empty()) {
        return None;
      }
      match self.tcx.parent_hir_node(id) {
        rustc_hir::Node::Pat(p) => {
          if matches!(p.kind, PatKind::Ref(..)) {
            return None;
          }
          id = p.hir_id;
        }
        rustc_hir::Node::PatField(f) => id = f.hir_id,
        _ => return Some(mutbl),
      }
    }
  }

  /// The field path from a `let` pattern's root to the binding
  /// `hirid`: `["name"]` for `let P { ref name, .. } = p;`, `["1"]`
  /// for `let (_, ref b) = t;`, `[]` for `let ref r = s;`. `None`
  /// when a step isn't a field (`x @ …`, a slice, an or-pattern).
  fn let_binding_field_path(&self, hirid: HirId) -> Option<Vec<String>> {
    let typeck = self.tcx.typeck(hirid.owner);
    let mut path = Vec::new();
    let mut id = hirid;
    loop {
      match self.tcx.parent_hir_node(id) {
        rustc_hir::Node::PatField(f) => {
          path.push(f.ident.to_string());
          // A PatField's parent is its struct pattern.
          match self.tcx.parent_hir_node(f.hir_id) {
            rustc_hir::Node::Pat(p) => id = p.hir_id,
            _ => return None,
          }
        }
        rustc_hir::Node::Pat(p) => match p.kind {
          PatKind::Tuple(ps, ddpos) | PatKind::TupleStruct(_, ps, ddpos) => {
            let k = ps.iter().position(|sp| sp.hir_id == id)?;
            let arity = match typeck.pat_ty(p).kind() {
              rustc_middle::ty::TyKind::Tuple(tys) => tys.len(),
              rustc_middle::ty::TyKind::Adt(def, _) => {
                let res = typeck.qpath_res(match p.kind {
                  PatKind::TupleStruct(ref q, ..) => q,
                  _ => return None,
                }, p.hir_id);
                def.variant_of_res(res).fields.len()
              }
              _ => return None,
            };
            // `(a, .., z)`: sub-patterns after `..` line up with the
            // last fields.
            let i = match ddpos.as_opt_usize() {
              Some(d) if k >= d => k + arity.saturating_sub(ps.len()),
              _ => k,
            };
            path.push(i.to_string());
            id = p.hir_id;
          }
          _ => return None,
        },
        rustc_hir::Node::LetStmt(_) => break,
        _ => return None,
      }
    }
    path.reverse();
    Some(path)
  }

  /// Register an explicit `ref` / `ref mut` binding of a `let` (see
  /// [`own_ref_binding`]) as the borrow it is, the same way `let r =
  /// &s;` is drawn (#380):
  /// - against a place, `let ref r = s;` borrows `s`, and `let P {
  ///   ref name, .. } = p;` borrows `p.name` (or, when that part has
  ///   no column, partially borrows `p`);
  /// - against a value (`let ref r = String::from("hi");`), it borrows
  ///   a temporary, which has no column;
  /// - against an element of a literal the pattern is paired with
  ///   (`let (ref a, b) = (s, 1);`), `s` is moved (or copied) into the
  ///   literal's temporary, and `a` borrows that, not `s`.
  #[allow(clippy::too_many_arguments)]
  fn bind_ref_binding(
    &mut self,
    name: String,
    annotation: BindingMode,
    hirid: HirId,
    init: &'tcx Expr<'tcx>,
    is_skipped: bool,
    mutbl: Mutability,
    lhs_ty: rustc_middle::ty::Ty<'tcx>,
  ) {
    let typeck = self.tcx.typeck(hirid.owner);
    let line_num = expr_to_line(init, &self.tcx);
    let evt = if bool_of_mut(mutbl) { Evt::MBorrow } else { Evt::SBorrow };
    let whole_init = matches!(self.tcx.parent_hir_node(init.hir_id), rustc_hir::Node::LetStmt(_));
    let is_place = matches!(
      init.kind,
      ExprKind::Path(QPath::Resolved(_, rustc_hir::Path { res: Res::Local(_), .. }))
        | ExprKind::Field(..)
        | ExprKind::Index(..)
        | ExprKind::Unary(UnOp::Deref, _)
    ) && !init.span.from_expansion();
    let (lender, is_partial) = if whole_init && is_place {
      let path = self.let_binding_field_path(hirid);
      let base = match init.kind {
        ExprKind::Path(..) | ExprKind::Field(..) => expr_to_rap_name(init, &self.tcx)
          .filter(|n| self.raps.contains_key(n)),
        _ => None,
      };
      match (base, path) {
        // The longest prefix of the field path that has a column.
        (Some(base), Some(path)) => {
          let mut best = base.clone();
          let mut matched = 0;
          for (k, f) in path.iter().enumerate() {
            let cand = format!("{}.{}", best, f);
            if !self.raps.contains_key(&cand) {
              break;
            }
            best = cand;
            matched = k + 1;
          }
          let rap = self.raps.get(&best).unwrap().rap.to_owned();
          (ResourceTy::Value(rap), matched < path.len())
        }
        (Some(base), None) => {
          (ResourceTy::Value(self.raps.get(&base).unwrap().rap.to_owned()), true)
        }
        (None, path) => {
          (get_rap(init, &self.tcx, &self.raps), path.is_none_or(|p| !p.is_empty()))
        }
      }
    } else {
      // A literal's element moves (or copies) into the literal's
      // temporary; the binding borrows the temporary.
      if !whole_init && is_place && !is_skipped {
        if let Some(src) = fetch_rap(init, &self.tcx, &self.raps) {
          let src_ty = typeck.node_type(init.hir_id);
          let e = if src_ty.is_ref() || self.ty_is_copy(src_ty, hirid.owner) { Evt::Copy } else { Evt::Move };
          self.update_rap(&src, line_num);
          self.add_ev(line_num, e, ResourceTy::Anonymous, ResourceTy::Value(src), false);
        }
      }
      (ResourceTy::Anonymous, false)
    };
    // Registered like `define_lhs` registers `let r = &init;`, with the
    // lender and aliasing `get_ref_data` gives `&init` (define_lhs
    // itself would treat `init` as copied into the binding).
    let mut aliasing = get_aliasing_data(&lender, &self.borrow_map);
    if lhs_ty.builtin_deref(false).is_some_and(|t| t.is_ref()) {
      if let ResourceTy::Value(x) | ResourceTy::Deref(x) = &lender {
        aliasing.push_front(x.name().to_owned());
      }
    }
    let pointee_copy = self.ref_pointee_is_copy(lhs_ty, hirid.owner);
    self.add_ref(name.clone(), bool_of_mut(mutbl), bool_of_mut(annotation.1), line_num,
      lender.clone(), aliasing, self.current_scope, !self.inside_branch, pointee_copy);
    if is_skipped {
      self.skip_raps.insert(name);
      return;
    }
    if let ResourceTy::Value(r) | ResourceTy::Deref(r) = &lender {
      self.update_rap(r, line_num);
    }
    let rap = self.raps.get(&name).unwrap().rap.to_owned();
    let mut ev = self.ext_ev_of_evt(evt, ResourceTy::Value(rap), lender, *self.unique_id, is_partial);
    if let ExternalEvent::StaticBorrow { is_reborrow, .. }
         | ExternalEvent::MutableBorrow { is_reborrow, .. } = &mut ev {
      *is_reborrow = is_explicit_reborrow(init, typeck);
    }
    self.add_external_event(line_num, ev);
    *self.unique_id += 1;
  }
}

/// True iff `callee` names a tuple struct / variant constructor
/// (`Some`, `Wrapper`).
fn is_ctor_path<'tcx>(callee: &Expr<'tcx>, typeck: &rustc_middle::ty::TypeckResults<'tcx>) -> bool {
  match callee.kind {
    ExprKind::Path(ref qpath) => matches!(typeck.qpath_res(qpath, callee.hir_id), Res::Def(DefKind::Ctor(..), _)),
    _ => false,
  }
}

/// `std::option::Option<std::string::String>` -> `Option<String>`: drop
/// the lower-case module prefixes from a printed type.
fn strip_module_paths(ty: &str) -> String {
  let mut out = String::new();
  let mut word = String::new();
  let mut chars = ty.chars().peekable();
  while let Some(c) = chars.next() {
    if c.is_alphanumeric() || c == '_' {
      word.push(c);
    } else if c == ':' && chars.peek() == Some(&':') && word.chars().next().is_some_and(|f| f.is_lowercase()) {
      chars.next();
      word.clear();
    } else {
      out.push_str(&word);
      word.clear();
      out.push(c);
    }
  }
  out.push_str(&word);
  out
}
/// What a branch of type `!` does on its way out (see `BranchExit`).
struct BranchLeaves {
  /// How it leaves, sorted; never empty.
  exits: Vec<BranchExit>,
  /// Its `break`s out of an enclosing loop or labeled block: (the
  /// `break`, its target).
  loop_breaks: Vec<(HirId, HirId)>,
  /// Its only way out is one `break`, the branch's last expression.
  ends_in_its_only_break: bool,
}

fn branch_leaves<'tcx>(tcx: rustc_middle::ty::TyCtxt<'tcx>, body: &'tcx Expr<'tcx>) -> BranchLeaves {
  struct Finder<'tcx> {
    tcx: rustc_middle::ty::TyCtxt<'tcx>,
    // Loops and labeled blocks inside the branch: a `break` /
    // `continue` targeting one of them doesn't leave the branch.
    inner: HashSet<HirId>,
    control: Vec<BranchExit>,
    loop_breaks: Vec<(HirId, HirId)>,
    panics: bool,
  }
  impl<'tcx> Visitor<'tcx> for Finder<'tcx> {
    fn visit_expr(&mut self, e: &'tcx Expr<'tcx>) {
      match e.kind {
        // A closure's body is a different function.
        ExprKind::Closure(..) => return,
        ExprKind::Loop(..) => { self.inner.insert(e.hir_id); }
        ExprKind::Block(b, Some(_)) => { self.inner.insert(e.hir_id); self.inner.insert(b.hir_id); }
        // `?`'s desugaring returns too, but only on the error path.
        ExprKind::Ret(_) if !e.span.from_expansion() => self.control.push(BranchExit::Return),
        ExprKind::Break(dest, _) => if let Ok(t) = dest.target_id {
          if !self.inner.contains(&t) {
            let is_loop = matches!(self.tcx.hir_node(t), rustc_hir::Node::Expr(Expr { kind: ExprKind::Loop(..), .. }));
            self.control.push(if is_loop { BranchExit::Break } else { BranchExit::BreakBlock });
            self.loop_breaks.push((e.hir_id, t));
          }
        },
        ExprKind::Continue(dest) => if let Ok(t) = dest.target_id {
          if !self.inner.contains(&t) {
            self.control.push(BranchExit::Continue);
          }
        },
        // `panic!()`, `unreachable!()`, `todo!()`, a failed `assert!`
        // call a never-returning panic function of core / std.
        ExprKind::Call(f, _) => {
          let typeck = self.tcx.typeck(e.hir_id.owner);
          if typeck.expr_ty(e).is_never() {
            if let ExprKind::Path(ref qp) = f.kind {
              if let Res::Def(_, def_id) = typeck.qpath_res(qp, f.hir_id) {
                if !def_id.is_local() && self.tcx.def_path_str(def_id).contains("panic") {
                  self.panics = true;
                }
              }
            }
          }
        }
        _ => {}
      }
      intravisit::walk_expr(self, e);
    }
  }
  let mut f = Finder {
    tcx, inner: HashSet::new(), control: Vec::new(), loop_breaks: Vec::new(), panics: false,
  };
  f.visit_expr(body);
  let last = match body.kind {
    ExprKind::Block(b, _) => b.expr.or_else(|| match b.stmts.last().map(|s| &s.kind) {
      Some(StmtKind::Semi(e)) | Some(StmtKind::Expr(e)) => Some(*e),
      _ => None,
    }),
    _ => Some(body),
  };
  let ends_in_its_only_break = f.control == [BranchExit::Break]
    && f.loop_breaks.len() == 1
    && last.is_some_and(|l| l.hir_id == f.loop_breaks[0].0);
  let mut exits = f.control;
  exits.sort();
  exits.dedup();
  // Only a call that never returns (or an endless loop) leaves it.
  if exits.is_empty() {
    exits.push(if f.panics { BranchExit::Panic } else { BranchExit::Other });
  }
  BranchLeaves { exits, loop_breaks: f.loop_breaks, ends_in_its_only_break }
}

/// The variables declared outside a branch (not in `decls`) that its
/// `events` move out of and don't give a new value afterwards, in the
/// order they're first moved. `true`: moved by the branch itself, on
/// every path through it; `false`: only in a conditional nested in it.
fn moved_out_of_outer(
  events: &[(usize, ExternalEvent)],
  decls: &HashSet<ResourceAccessPoint>,
) -> Vec<(ResourceAccessPoint, bool)> {
  let mut out: Vec<(ResourceAccessPoint, bool)> = Vec::new();
  let outer = |r: &ResourceAccessPoint| !r.is_fn() && !decls.contains(r);
  for (_, ev) in events {
    match ev {
      ExternalEvent::Move { from: ResourceTy::Value(rap), .. } if outer(rap) => {
        match out.iter_mut().find(|(r, _)| r == rap) {
          Some(e) => e.1 = true,
          None => out.push((rap.clone(), true)),
        }
      }
      ExternalEvent::Branch { branches, .. } => {
        for b in branches {
          let inner: HashSet<ResourceAccessPoint> = decls | &b.decl_vars;
          for (rap, _) in moved_out_of_outer(&b.e_data, &inner) {
            if !out.iter().any(|(r, _)| *r == rap) {
              out.push((rap, false));
            }
          }
        }
      }
      _ => {}
    }
    // A new value moved or copied in after the move.
    if let ExternalEvent::Move { to: ResourceTy::Value(to), .. }
         | ExternalEvent::Copy { to: ResourceTy::Value(to), .. }
         | ExternalEvent::Bind { to: ResourceTy::Value(to), .. } = ev {
      out.retain(|(r, _)| r != to);
    }
  }
  out
}

/// How many `break`s inside `loop_expr` target it.
fn breaks_out_of(loop_expr: &Expr<'_>) -> usize {
  struct Count(HirId, usize);
  impl<'tcx> Visitor<'tcx> for Count {
    fn visit_expr(&mut self, e: &'tcx Expr<'tcx>) {
      match e.kind {
        ExprKind::Closure(..) => return,
        ExprKind::Break(dest, _) if dest.target_id == Ok(self.0) => self.1 += 1,
        _ => {}
      }
      intravisit::walk_expr(self, e);
    }
  }
  let mut c = Count(loop_expr.hir_id, 0);
  intravisit::walk_expr(&mut c, loop_expr);
  c.1
}

impl<'a, 'tcx> ExprVisitor<'a, 'tcx> {
  /// Whether a branch (`body`, with its `events` and the variables
  /// `decls` it declares) has type `!` and so never reaches the merge
  /// after its conditional (#296), and if so how it leaves. A `break`
  /// out of a loop after a move is recorded for the loop, which then
  /// marks the moved variables as (maybe) moved where it ends (#344).
  fn branch_divergence(
    &mut self,
    body: &'tcx Expr<'tcx>,
    events: &[(usize, ExternalEvent)],
    decls: &HashSet<ResourceAccessPoint>,
  ) -> (bool, Vec<BranchExit>) {
    if !self.expr_diverges(body) {
      return (false, Vec::new());
    }
    let leaves = branch_leaves(self.tcx, body);
    let moved = moved_out_of_outer(events, decls);
    for (brk, target) in &leaves.loop_breaks {
      for (rap, every_path) in &moved {
        self.break_moves.push(BreakMove {
          target: *target,
          brk: *brk,
          rap: rap.clone(),
          definite: *every_path && leaves.ends_in_its_only_break,
        });
      }
    }
    (true, leaves.exits)
  }

  /// The names of the variables registered so far.
  fn rap_names(&self) -> HashSet<String> {
    #[expect(clippy::disallowed_methods, reason = "collected into a set; order is lost")]
    let names = self.raps.keys().cloned().collect();
    names
  }

  /// At the end of `loop_expr`: a variable declared before the loop
  /// (in `before`) that a branch moved before a `break` out of it may
  /// have been moved once the loop is over (#344). It has been when
  /// every way out of the loop is such a `break` (`ends_otherwise`:
  /// a `while` / `for` also ends when its condition / iterator does).
  /// Also used for a labeled block (`block`: its HirId, which a `break`
  /// may target instead of the block expression's).
  fn emit_loop_exit_moves(&mut self, loop_expr: &'tcx Expr<'tcx>, block: Option<HirId>, before: &HashSet<String>, ends_otherwise: bool) {
    let (mine, rest): (Vec<BreakMove>, Vec<BreakMove>) =
      std::mem::take(&mut self.break_moves).into_iter()
        .partition(|m| m.target == loop_expr.hir_id || Some(m.target) == block);
    self.break_moves = rest;
    if mine.is_empty() {
      return;
    }
    let all_breaks = breaks_out_of(loop_expr);
    let line = span_end_line(&loop_expr.span, &self.tcx);
    let mut done: Vec<&ResourceAccessPoint> = Vec::new();
    for m in &mine {
      if done.contains(&&m.rap) {
        continue;
      }
      done.push(&m.rap);
      let declared_before = before.contains(m.rap.name())
        && self.raps.get(m.rap.name()).is_some_and(|d| d.rap == m.rap);
      if !declared_before {
        continue;
      }
      let mut definite_breaks: Vec<HirId> = Vec::new();
      for o in mine.iter().filter(|o| o.rap == m.rap && o.definite) {
        if !definite_breaks.contains(&o.brk) {
          definite_breaks.push(o.brk);
        }
      }
      let definite = !ends_otherwise && definite_breaks.len() == all_breaks;
      self.add_external_event(line, ExternalEvent::LoopExitMove {
        ro: m.rap.clone(), definite, labeled_block: block.is_some(), id: *self.unique_id,
      });
      *self.unique_id += 1;
    }
  }
}
