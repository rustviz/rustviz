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
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};


/// True if `expr` is an `&&` chain with at least one `let` in it.
fn is_let_chain(expr: &Expr) -> bool {
  match expr.kind {
    ExprKind::DropTemps(inner) => is_let_chain(inner),
    ExprKind::Binary(op, lhs, rhs) if op.node == rustc_hir::BinOpKind::And => {
      is_let_chain(lhs) || is_let_chain(rhs)
    }
    ExprKind::Let(_) => true,
    _ => false,
  }
}

/// True if `pat` matches every value of its type. Conservative: only
/// bindings, `_`, tuples, references / boxes, and struct patterns (not
/// enum variants) of such patterns count.
fn pat_is_irrefutable(pat: &Pat, typeck: &rustc_middle::ty::TypeckResults) -> bool {
  let is_struct = |qpath: &QPath| matches!(typeck.qpath_res(qpath, pat.hir_id),
    Res::Def(DefKind::Struct | DefKind::Ctor(CtorOf::Struct, _), _));
  match pat.kind {
    PatKind::Wild => true,
    PatKind::Binding(_, _, _, sub) => sub.is_none_or(|p| pat_is_irrefutable(p, typeck)),
    PatKind::Tuple(pats, _) => pats.iter().all(|p| pat_is_irrefutable(p, typeck)),
    PatKind::Box(p) | PatKind::Deref(p) | PatKind::Ref(p, ..) => pat_is_irrefutable(p, typeck),
    PatKind::Struct(ref qpath, fields, _) =>
      is_struct(qpath) && fields.iter().all(|f| pat_is_irrefutable(f.pat, typeck)),
    PatKind::TupleStruct(ref qpath, pats, _) =>
      is_struct(qpath) && pats.iter().all(|p| pat_is_irrefutable(p, typeck)),
    _ => false,
  }
}

/// A lasting effect of a part of an `if` let-chain on a variable
/// declared outside the condition (#281), recorded while walking the
/// chain (`visit_if_let_chain`). `checks_before`: how many parts that
/// can fail come before it (for a `let`'s bindings, including that
/// `let`'s own pattern). Whether the effect is on the path into the
/// `else` depends on what comes after it, so it's judged once the
/// whole chain has been walked.
struct ChainEffect {
  kind: ChainEffectKind,
  checks_before: usize,
  line: usize,
}

enum ChainEffectKind {
  /// `rap` is moved out of, to `to`. `binding`: by a `let`'s pattern.
  /// `how`: also a move out of `*rap`, or a call of the closure `rap`
  /// (#579).
  Move { rap: ResourceAccessPoint, to: ResourceTy, binding: bool, how: ChainFailHow },
  /// `rap` is assigned a new value (#579), or with `place` the place
  /// reached through it (`*r`, `v[i]`, `r.f`; #603). `drops_old`: the
  /// assignment drops the old value. `held`: it held a value before the
  /// part (a place always does).
  Assign { rap: ResourceAccessPoint, place: Option<String>, drops_old: bool, held: bool },
  /// `rap` is lent to `to` (a function the part calls, or with
  /// `binding` a binding of the chain), which doesn't last past the
  /// condition but happened on the path into the `else` too (#688).
  Use { rap: ResourceAccessPoint, to: ResourceTy, mutable: bool, binding: bool },
  /// Anything else that lasts: not drawn. `what` / `lasts` are for the
  /// refusal's message.
  Other { name: String, what: &'static str, lasts: &'static str },
}

const CHAIN_ASSIGN_REF: &str = "assign a new reference to a variable declared outside the condition";
const CHAIN_MOVE_THROUGH: &str = "move out of a value behind a variable declared outside the condition";

/// State of a walk down an `if` let-chain (`visit_if_let_chain`).
#[derive(Default)]
struct ChainWalk {
  /// The event id just after the chain's first part: see
  /// `visit_if_let_chain`.
  first_lo: Option<usize>,
  /// Parts that can fail, so far: `bool` conditions and refutable
  /// `let` patterns.
  checks: usize,
  effects: Vec<ChainEffect>,
  /// The chain's bindings whose type needs drop, by name.
  drop_bindings: HashSet<String>,
}

/// A variable that the condition of an `if` let-chain moves before a
/// part of it that can still fail (#281): on the path into the `else`
/// it may have been moved. See `MaybeMovedAt::ChainFail`.
struct ChainFailMove {
  rap: ResourceAccessPoint,
  definite: bool,
  to: Vec<String>,
  dropped: Vec<String>,
  how: ChainFailHow,
}

/// A variable that the condition of an `if` let-chain assigns a new
/// value before a part of it that can still fail (#579): on the path
/// into the `else` it holds the old value or the new one. See
/// `ExternalEvent::ChainFailAssign`.
struct ChainFailAssign {
  rap: ResourceAccessPoint,
  place: Option<String>,
  drops_old: bool,
}

/// A variable that the condition of an `if` let-chain lends before a
/// part of it that can still fail (#688): the else branch starts with
/// that said. See `ExternalEvent::ChainFailUse`.
struct ChainFailUse {
  rap: ResourceAccessPoint,
  uses: Vec<ChainUse>,
  definite: bool,
}

/// What `chain_fail_moves` finds: see there.
#[derive(Default)]
struct ChainFail {
  moves: Vec<ChainFailMove>,
  assigns: Vec<ChainFailAssign>,
  uses: Vec<ChainFailUse>,
}

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
    // `*r` with r a reference: a copy of the value r points to is
    // returned, not a reference (#670). (A `*r` that isn't Copy can only
    // be reborrowed, never moved out from behind r; that's left as is.)
    let deref = if matches!(evt, Evt::Copy) { self.deref_of_ref(e) } else { None };
    let from_ro = if let Some(rty) = deref { rty } else { match fetch_rap(e, &self.tcx, &self.raps) {
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
    }};

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

  /// The values `e`, a returned value, can be: the value each branch
  /// of an `if` / `else` or `match` yields, through nested ones and
  /// blocks (`if b { s } else { t }` → `s`, `t`), or `e` itself.
  /// A branch that diverges (`return`, `panic!()`) yields nothing.
  fn returned_values(&self, e: &'tcx Expr<'tcx>, out: &mut Vec<&'tcx Expr<'tcx>>) {
    if self.expr_diverges(e) {
      return;
    }
    if e.span.from_expansion() {
      out.push(e);
      return;
    }
    match e.kind {
      ExprKind::If(_, then, Some(els)) => {
        self.returned_values(then, out);
        self.returned_values(els, out);
      }
      ExprKind::Match(_, arms, rustc_hir::MatchSource::Normal) => {
        for arm in arms {
          self.returned_values(arm.body, out);
        }
      }
      ExprKind::Block(b, None) => match b.expr {
        Some(tail) => self.returned_values(tail, out),
        None => {}
      },
      ExprKind::DropTemps(inner) => self.returned_values(inner, out),
      _ => out.push(e),
    }
  }

  /// `e` is returned to the caller (the function's tail, or what a
  /// `return` carries). If it's an `if` / `else` or a `match`, mark
  /// the value each branch yields, which goes to the caller inside its
  /// branch, where `visit_expr` reaches it (#674): on the paths
  /// through the other branches it stays where it is. False for any
  /// other `e`, which `return_to_caller` sends to the caller as a whole.
  fn mark_branch_returns(&mut self, e: &'tcx Expr<'tcx>) -> bool {
    if e.span.from_expansion()
      || !matches!(e.kind, ExprKind::If(_, _, Some(_)) | ExprKind::Match(_, _, rustc_hir::MatchSource::Normal)) {
      return false;
    }
    let mut values = Vec::new();
    self.returned_values(e, &mut values);
    self.tail_values.extend(values.iter().map(|v| v.hir_id));
    true
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
        let (bound, _) = self.register_iflet_let_bindings(let_expr, None);
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

  /// Visit an `if` condition that is a let-chain (`if let A = x && let
  /// B = y && cond`, #198). Each `let` registers its bindings into
  /// `decls` (the caller scopes them to the if-branch, like a lone `if
  /// let`'s); other parts are visited as ordinary expressions.
  ///
  /// The chain's first part always runs, so its events (for a `let`,
  /// its scrutinee's) are drawn before the split, like a lone `if
  /// let`'s scrutinee: `walk.first_lo` is set to the event id just
  /// after it, and the caller starts the if-branch's events there.
  /// Everything after that is drawn in the if-branch. That's only the
  /// whole story for effects that end inside the condition: if a later
  /// part fails, the else branch (or, without an `else`, the code after
  /// the `if`) runs in the state the earlier parts left behind (rustc:
  /// "borrow of partially moved value" after `if let Some(a) = x &&
  /// a.len() > 5`). So the effects on variables in `outer` are recorded
  /// in `walk.effects`, and `chain_fail_moves` turns them into what the
  /// else branch starts with (#281). The `while let` caller passes an
  /// empty `outer`: the loop is drawn inline, with no branch to get
  /// wrong.
  fn visit_if_let_chain(
    &mut self,
    expr: &'tcx Expr<'tcx>,
    outer: &HashSet<ResourceAccessPoint>,
    decls: &mut HashSet<ResourceAccessPoint>,
    walk: &mut ChainWalk,
  ) {
    match expr.kind {
      ExprKind::DropTemps(inner) => self.visit_if_let_chain(inner, outer, decls, walk),
      ExprKind::Binary(op, lhs, rhs) if op.node == rustc_hir::BinOpKind::And => {
        self.visit_if_let_chain(lhs, outer, decls, walk);
        self.visit_if_let_chain(rhs, outer, decls, walk);
      }
      ExprKind::Let(let_expr) => {
        let mark = self.preprocessed_events.len();
        self.visit_expr(let_expr.init);
        if walk.first_lo.is_some() {
          self.record_chain_effects(mark, outer, expr, walk, false);
        }
        walk.first_lo.get_or_insert(*self.unique_id);
        let typeck = self.tcx.typeck(let_expr.pat.hir_id.owner);
        if !pat_is_irrefutable(let_expr.pat, typeck) {
          walk.checks += 1;
        }
        let env = rustc_middle::ty::TypingEnv::post_analysis(self.tcx, let_expr.pat.hir_id.owner);
        // By column name: a binding that shadows a variable is `a₂`.
        let_expr.pat.each_binding(|_, hir_id, _, _| {
          if typeck.node_type(hir_id).needs_drop(self.tcx, env) {
            walk.drop_bindings.insert(binding_name(&self.tcx, hir_id));
          }
        });
        let mark = self.preprocessed_events.len();
        let bound = self.register_iflet_let_bindings(let_expr, None).0;
        self.record_chain_effects(mark, outer, expr, walk, true);
        decls.extend(bound);
      }
      _ => {
        let mark = self.preprocessed_events.len();
        self.visit_expr(expr);
        if walk.first_lo.is_some() {
          self.record_chain_effects(mark, outer, expr, walk, false);
        }
        walk.first_lo.get_or_insert(*self.unique_id);
        walk.checks += 1;
      }
    }
  }

  /// Record in `walk.effects` the lasting effects that events from
  /// `preprocessed_events[mark..]` (a let-chain part) have on variables
  /// in `outer`. See `visit_if_let_chain`.
  fn record_chain_effects(
    &self,
    mark: usize,
    outer: &HashSet<ResourceAccessPoint>,
    part: &'tcx Expr<'tcx>,
    walk: &mut ChainWalk,
    binding: bool,
  ) {
    let line = expr_to_line(part, &self.tcx);
    let is_outer = |r: &ResourceTy| r.extract_rap().is_some_and(|rap| outer.contains(rap));
    let checks_before = walk.checks;
    let mut push = |kind| walk.effects.push(ChainEffect { kind, checks_before, line });
    let held = |rap: &ResourceAccessPoint| self.rap_ownership_before(mark, rap.name()) == Ownership::Held;
    // Also the events of a conditional inside the part (`{ if c { z =
    // a; } z > 0 }`): they last the same way.
    fn flatten<'e>(events: &'e [(usize, ExternalEvent)], out: &mut Vec<&'e ExternalEvent>) {
      for (_, ev) in events {
        match ev {
          ExternalEvent::Branch { branches, .. } => branches.iter().for_each(|b| flatten(&b.e_data, out)),
          _ => out.push(ev),
        }
      }
    }
    let mut events = Vec::new();
    flatten(&self.preprocessed_events[mark..], &mut events);
    // `*r = s` also puts an `OwnerDropAtReassign` on r's lender, not a
    // variable the part assigns to: the write through `*r` (#603) says
    // that drop.
    let assigned: HashSet<&ResourceAccessPoint> = events.iter().filter_map(|ev| match ev {
      ExternalEvent::Bind { to: ResourceTy::Value(rap), .. }
      | ExternalEvent::Copy { to: ResourceTy::Value(rap), .. }
      | ExternalEvent::Move { to: ResourceTy::Value(rap), .. } => Some(rap),
      _ => None,
    }).collect();
    for ev in events.iter().copied() {
      // What the event does to the place it takes from.
      match ev {
        ExternalEvent::Move { from: from @ ResourceTy::Value(rap), to, .. } if is_outer(from) =>
          push(ChainEffectKind::Move { rap: rap.clone(), to: to.clone(), binding, how: ChainFailHow::Move }),
        // An outer closure that can only be called once: the call
        // moves it (#579).
        ExternalEvent::ClosureCall { closure: c @ ResourceTy::Value(rap), .. } if is_outer(c) =>
          push(ChainEffectKind::Move { rap: rap.clone(), to: ResourceTy::Anonymous, binding: false, how: ChainFailHow::Call }),
        // Out of a `Box`: `take(*b)` (#579).
        ExternalEvent::Move { from: from @ ResourceTy::Deref(rap), to, .. } if is_outer(from) =>
          push(ChainEffectKind::Move { rap: rap.clone(), to: to.clone(), binding, how: ChainFailHow::Out(format!("*{}", rap.name())) }),
        ExternalEvent::Move { from, .. } if is_outer(from) =>
          push(ChainEffectKind::Other { name: from.real_name(), what: CHAIN_MOVE_THROUGH, lasts: "the move lasts" }),
        _ => {}
      }
      // What it does to the place it writes to: an assignment.
      match ev {
        ExternalEvent::Bind { to, .. }
        | ExternalEvent::Copy { to, .. }
        | ExternalEvent::Move { to, .. } if is_outer(to) => match to {
          ResourceTy::Value(rap) if rap.is_ref() =>
            push(ChainEffectKind::Other { name: rap.name().to_owned(), what: CHAIN_ASSIGN_REF, lasts: "the assignment lasts" }),
          ResourceTy::Value(rap) =>
            push(ChainEffectKind::Assign { rap: rap.clone(), place: None, drops_old: false, held: held(rap) }),
          // A write through `*r` / into `v[i]` / `r.f`: drawn on the
          // column of the variable it's reached through (#603).
          ResourceTy::Deref(rap) =>
            push(ChainEffectKind::Assign { rap: rap.clone(), place: Some(format!("*{}", rap.name())),
              drops_old: matches!(rap.place_write(), PlaceWrite::AssignDropsOld), held: true }),
          ResourceTy::Index(rap, place, pw) | ResourceTy::DerefField(rap, place, pw) =>
            push(ChainEffectKind::Assign { rap: rap.clone(), place: Some(place.clone()),
              drops_old: matches!(pw, PlaceWrite::AssignDropsOld), held: true }),
          _ => {}
        },
        ExternalEvent::StaticBorrow { to, .. }
        | ExternalEvent::MutableBorrow { to, .. } if is_outer(to) =>
          push(ChainEffectKind::Other { name: to.real_name(), what: CHAIN_ASSIGN_REF, lasts: "the assignment lasts" }),
        ExternalEvent::OwnerDropAtReassign { ro, .. } if outer.contains(ro) && assigned.contains(ro) =>
          push(ChainEffectKind::Assign { rap: ro.clone(), place: None, drops_old: true, held: held(ro) }),
        _ => {}
      }
      // A borrow of it that ends inside the condition (#688): lent to a
      // call (`y.take()`, `s.len()`), or to a binding of the chain
      // (`let Some(a) = &x`), whose scope ends when the condition fails.
      // (A borrow into an outer variable is an assignment: above.)
      match ev {
        ExternalEvent::PassByStaticReference { from: from @ ResourceTy::Value(rap), to, .. }
        | ExternalEvent::StaticBorrow { from: from @ ResourceTy::Value(rap), to, .. } if is_outer(from) && !is_outer(to) =>
          push(ChainEffectKind::Use { rap: rap.clone(), to: to.clone(), mutable: false, binding }),
        ExternalEvent::PassByMutableReference { from: from @ ResourceTy::Value(rap), to, .. }
        | ExternalEvent::MutableBorrow { from: from @ ResourceTy::Value(rap), to, .. } if is_outer(from) && !is_outer(to) =>
          push(ChainEffectKind::Use { rap: rap.clone(), to: to.clone(), mutable: true, binding }),
        _ => {}
      }
    }
  }

  /// What the `else` branch of an `if` whose condition is the let-chain
  /// `walk` was recorded from starts with (#281): each outer variable
  /// that a part of the condition moves before a later part that can
  /// fail. When that later part fails, the else branch runs after the
  /// move. A move with no part that can fail after it (the last
  /// `let`'s bindings, typically) happens only when the whole condition
  /// holds, and is drawn in the if-branch alone. `chain_lo`: index in
  /// `preprocessed_events` of the chain's first event.
  ///
  /// A call of an outer `FnOnce` closure moves it, and `take(*b)` moves
  /// out of the `Box` `b`: both are such moves (#579). An assignment to
  /// an outer variable lasts the same way: the else branch starts with
  /// it holding its old value or the new one (#579). Other lasting
  /// effects that come before a part that can fail are refused, as for
  /// match guards: a new reference assigned to an outer variable (which
  /// borrow is live in the else branch isn't known), an assignment to a
  /// variable that held no value before (the else branch would start
  /// with it maybe initialized), or a variable both moved and assigned.
  /// A write through an outer reference, into an element or a field
  /// reached through one (`*r = a`, `v[i] = a`, `r.f = a`) is an
  /// assignment to that place, drawn on that variable's column (#603).
  ///
  /// A borrow that ends inside the condition (lent to a call, or to a
  /// binding of the chain) also happened on the path into the else
  /// branch, which starts with that said (#688), unless the variable
  /// is moved or assigned there too (that dot is the one that matters).
  fn chain_fail_moves(&self, walk: &ChainWalk, chain_lo: usize, has_else: bool) -> ChainFail {
    let mut out: Vec<ChainFailMove> = Vec::new();
    let mut assigns: Vec<ChainFailAssign> = Vec::new();
    let mut uses: Vec<ChainFailUse> = Vec::new();
    let later = if has_else { "the condition is false and the else branch runs" }
      else { "the condition is false and the body is skipped" };
    for eff in walk.effects.iter().filter(|e| e.checks_before < walk.checks) {
      match &eff.kind {
        ChainEffectKind::Other { name, what, lasts } => {
          panic!("`if let` chains that {} (`{}`, line {}) aren't supported yet: {} even when {}",
            what, name, eff.line, lasts, later);
        }
        ChainEffectKind::Assign { rap, place, drops_old, held } => {
          if !held {
            panic!("`if let` chains that assign to a variable declared outside the condition that \
                    may hold no value before (`{}`, line {}) aren't supported yet: after the \
                    assignment it holds one even when {}", rap.name(), eff.line, later);
          }
          match assigns.iter_mut().find(|a| a.rap == *rap && a.place == *place) {
            Some(a) => a.drops_old |= drops_old,
            None => assigns.push(ChainFailAssign { rap: rap.clone(), place: place.clone(), drops_old: *drops_old }),
          }
        }
        ChainEffectKind::Move { rap, to, binding, how } => {
          // A `let`'s bindings with only irrefutable parts up to them:
          // every way into the else branch passes the move. A move by
          // an expression may sit inside a conditional of its own
          // (`a || take(s)`), so it's only "may have been".
          let definite = *binding && eff.checks_before == 0;
          let i = match out.iter().position(|m| m.rap == *rap) {
            Some(i) => i,
            None => {
              out.push(ChainFailMove { rap: rap.clone(), definite: false, to: Vec::new(), dropped: Vec::new(), how: how.clone() });
              out.len() - 1
            }
          };
          let m = &mut out[i];
          m.definite |= definite;
          let ResourceTy::Value(dest) = to else { continue };
          let dest_name = dest.name().to_owned();
          if m.to.contains(&dest_name) {
            continue;
          }
          m.to.push(dest_name.clone());
          // A binding of the chain that took the value is dropped when
          // the condition fails, unless a later part moved it on.
          let moved_on = self.preprocessed_events[chain_lo..].iter().any(|(_, ev)| matches!(ev,
            ExternalEvent::Move { from, .. }
              if from.extract_rap().is_some_and(|r| r == dest
                || r.name().starts_with(&format!("{}.", dest_name)))));
          if *binding && !dest.is_fn() && walk.drop_bindings.contains(&dest_name) && !moved_on {
            m.dropped.push(dest_name);
          }
        }
        ChainEffectKind::Use { rap, to, mutable, binding } => {
          let to_name = match to {
            ResourceTy::Value(r) => r.name().to_owned(),
            _ => String::new(),
          };
          // A binding lent mutably changes the variable only if a later
          // part writes or lends on through it.
          let changes = *mutable && (!*binding || match to {
            ResourceTy::Value(b) => self.chain_writes_through(chain_lo, b),
            _ => false,
          });
          let use_ = ChainUse { to: to_name, mutable: *mutable, binding: *binding, changes };
          let definite = *binding && eff.checks_before == 0;
          match uses.iter_mut().find(|u| u.rap == *rap) {
            Some(u) => {
              u.definite |= definite;
              if !u.uses.contains(&use_) {
                u.uses.push(use_);
              }
            }
            None => uses.push(ChainFailUse { rap: rap.clone(), uses: vec![use_], definite }),
          }
        }
      }
    }
    uses.retain(|u| !out.iter().any(|m| m.rap == u.rap) && !assigns.iter().any(|a| a.rap == u.rap));
    // The else branch would start with it moved or holding a value
    // depending on the order, which one dot can't say. A write through a
    // place (`*b = a`, `v[0] = a`) counts too when the variable it goes
    // through is moved: the new value went with it.
    if let Some(a) = assigns.iter().find(|a| out.iter().any(|m| m.rap == a.rap)) {
      panic!("`if let` chains that both move and assign a variable declared outside the condition \
              (`{}`) aren't supported yet: both last even when {}", a.rap.name(), later);
    }
    ChainFail { moves: out, assigns, uses }
  }

  /// True if an event of the let-chain from `preprocessed_events[chain_lo..]`
  /// writes through the chain's binding `b`, or lends it on mutably
  /// (`*b += 1`, `b.push('x')`). See `chain_fail_moves`.
  fn chain_writes_through(&self, chain_lo: usize, b: &ResourceAccessPoint) -> bool {
    fn any(events: &[(usize, ExternalEvent)], b: &ResourceAccessPoint) -> bool {
      let through = |r: &ResourceTy| !matches!(r, ResourceTy::Value(_)) && r.extract_rap() == Some(b);
      events.iter().any(|(_, ev)| match ev {
        ExternalEvent::Branch { branches, .. } => branches.iter().any(|br| any(&br.e_data, b)),
        ExternalEvent::PassByMutableReference { from, .. }
        | ExternalEvent::MutableBorrow { from, .. } => from.extract_rap() == Some(b),
        ExternalEvent::Bind { to, .. }
        | ExternalEvent::Copy { to, .. }
        | ExternalEvent::Move { to, .. } => through(to),
        _ => false,
      })
    }
    any(&self.preprocessed_events[chain_lo..], b)
  }

  /// The pattern-binding events of a let-chain: those handed out ids
  /// in `ids` that bind one of the chain's bindings `decls`, with their
  /// lines.
  fn chain_binding_events(
    &self,
    ids: &std::ops::Range<usize>,
    decls: &HashSet<ResourceAccessPoint>,
  ) -> Vec<(usize, ExternalEvent)> {
    self.preprocessed_events.iter()
      .filter(|(_, ev)| ids.contains(&ev.get_id()) && matches!(ev,
        ExternalEvent::Move { to: ResourceTy::Value(to), .. }
        | ExternalEvent::Copy { to: ResourceTy::Value(to), .. }
        | ExternalEvent::StaticBorrow { to: ResourceTy::Value(to), .. }
        | ExternalEvent::MutableBorrow { to: ResourceTy::Value(to), .. } if decls.contains(to)))
      .cloned()
      .collect()
  }

  /// A let-chain that binds two or more variables on one line, such as
  /// `while let Some(t) = v.pop() && let Some(u) = y {` (#584) or `if
  /// let Some(a) = &x && let Some(b) = y {` (#583): the (source,
  /// binding) column pairs of the bindings made from a variable declared
  /// outside the chain, for the renderer to put each binding's column
  /// right after its source's (`VisualizationData::binding_columns`).
  /// Then each arrow runs straight along the line between two adjacent
  /// columns, instead of hopping over another binding's dot (u after t,
  /// with `pop`'s value arriving on t). A binding from a function's
  /// result (`v.pop()`) or from a struct keeps its place; a chain that
  /// binds twice from one variable is left as it is.
  fn chain_binding_columns(&self, binds: &[(usize, ExternalEvent)], decls: &HashSet<ResourceAccessPoint>) -> Vec<(u64, u64)> {
    if binds.len() < 2 || binds.iter().any(|(l, _)| *l != binds[0].0) {
      return Vec::new();
    }
    let plain = |r: &ResourceAccessPoint| !r.is_fn() && !r.is_struct_group() && !r.is_member();
    let pairs: Vec<(u64, u64)> = binds.iter().filter_map(|(_, ev)| {
      let (ResourceTy::Value(from), ResourceTy::Value(to)) = ResourceAccessPoint_extract(ev) else { return None };
      (plain(from) && plain(to) && !decls.contains(from)).then(|| (*from.hash(), *to.hash()))
    }).collect();
    let distinct = |hs: Vec<u64>| hs.iter().collect::<HashSet<_>>().len() == hs.len();
    if distinct(pairs.iter().map(|p| p.0).collect()) && distinct(pairs.iter().map(|p| p.1).collect()) {
      pairs
    } else {
      Vec::new()
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
        // A panic macro unwinds once its message's arguments have
        // been evaluated (and borrowed) (#410).
        self.note_panic(expr);
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
              result_keeps_borrow: false, stored_in: StoredIn::default(), break_alt: None,
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
      ExprKind::Match(scrutinee, _, source) => {
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
        // `e?`: the `Err` / `None` arm returns from the function.
        if let rustc_hir::MatchSource::TryDesugar(_) = source {
          self.note_try(expr);
        }
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
      self.add_external_event(line_num, ExternalEvent::PassByStaticReference { from, to, id, implicit: ImplicitRef::MacroArg, refcell_borrow_mut: false, result_keeps_borrow: false, stored_in: StoredIn::default(), break_alt: None });
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

/// A name an or-pattern arm binds from a different place in each
/// alternative (#533): the places, in the order of the alternatives.
pub(crate) struct OrConflict {
  name: String,
  places: Vec<ResourceAccessPoint>,
}

impl OrConflict {
  /// Whether this is the conflict of the binding `name`, which is
  /// numbered (`x₂`) when an earlier `x` is in the same function.
  fn binds(&self, name: &str) -> bool {
    split_numbered_name(name).map_or(name, |(base, _)| base) == self.name
  }
}

/// What `or_conflict_events` draws for an or-pattern arm's names bound
/// from different places: events at the pattern, events where a
/// reference's possible borrows end (#565), and the references it took
/// out of the pattern's bindings, which are still the arm's own names.
#[derive(Default)]
struct OrArmEvents {
  start: Vec<ExternalEvent>,
  end: Vec<(usize, ExternalEvent)>,
  decls: Vec<ResourceAccessPoint>,
}

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

/// Whether `loop_expr` (a `loop`) has a `break value` that leaves it.
fn loop_has_break_value<'tcx>(loop_expr: &'tcx Expr<'tcx>) -> bool {
  struct Find { id: HirId, found: bool }
  impl<'tcx> Visitor<'tcx> for Find {
    fn visit_expr(&mut self, e: &'tcx Expr<'tcx>) {
      if let ExprKind::Break(dest, Some(_)) = e.kind {
        if dest.target_id == Ok(self.id) {
          self.found = true;
        }
      }
      intravisit::walk_expr(self, e);
    }
  }
  let mut f = Find { id: loop_expr.hir_id, found: false };
  intravisit::walk_expr(&mut f, loop_expr);
  f.found
}

/// Names of the locals that evaluating `e` for its value may move out
/// of, wholly or in part (a field), so a `return e` / `break e` may
/// hand them on rather than drop them. One that `e` only borrows
/// (`s.len()`, `&s`, `s == t`) or copies is left out: it's still
/// dropped at the exit (#410). Shapes not handled below count every
/// local they name as moved, which errs on the side of not drawing a
/// drop.
fn locals_moved_by<'tcx>(tcx: rustc_middle::ty::TyCtxt<'tcx>, e: &'tcx Expr<'tcx>) -> BTreeSet<String> {
  struct Scan<'tcx> {
    tcx: rustc_middle::ty::TyCtxt<'tcx>,
    typeck: &'tcx rustc_middle::ty::TypeckResults<'tcx>,
    env: rustc_middle::ty::TypingEnv<'tcx>,
    moved: BTreeSet<String>,
  }
  impl<'tcx> Scan<'tcx> {
    /// `by_value`: the value of `e` is taken here, not borrowed.
    fn scan(&mut self, e: &'tcx Expr<'tcx>, by_value: bool) {
      // An autoref (`s.len()`) borrows the place; a Copy value is
      // copied out of it.
      let borrowed = self.typeck.expr_adjustments(e).iter()
        .any(|a| matches!(a.kind, Adjust::Borrow(_)));
      let ty = self.typeck.expr_ty(e);
      let by_value = by_value && !borrowed && !self.tcx.type_is_copy_modulo_regions(self.env, ty);
      match e.kind {
        ExprKind::Path(QPath::Resolved(_, p)) => {
          if let Res::Local(id) = p.res {
            if by_value {
              self.moved.insert(binding_name(&self.tcx, id));
            }
          }
        }
        ExprKind::Path(_) | ExprKind::Lit(_) => {}
        ExprKind::AddrOf(_, _, inner) => self.scan(inner, false),
        // Moving a field out moves out of the struct.
        ExprKind::Field(base, _) => self.scan(base, by_value),
        // Nothing moves out of an index (only Copy values are read).
        ExprKind::Index(base, idx, _) => {
          self.scan(base, false);
          self.scan(idx, true);
        }
        // Only a Box can be moved out of through `*`.
        ExprKind::Unary(UnOp::Deref, inner) => {
          let boxed = self.typeck.expr_ty(inner).is_box();
          self.scan(inner, by_value && boxed);
        }
        ExprKind::Unary(_, inner) | ExprKind::Cast(inner, _) | ExprKind::DropTemps(inner) =>
          self.scan(inner, true),
        // Comparisons take their operands by reference.
        ExprKind::Binary(op, l, r) => {
          let by_ref = matches!(op.node, rustc_hir::BinOpKind::Eq | rustc_hir::BinOpKind::Ne
            | rustc_hir::BinOpKind::Lt | rustc_hir::BinOpKind::Le
            | rustc_hir::BinOpKind::Gt | rustc_hir::BinOpKind::Ge);
          self.scan(l, !by_ref);
          self.scan(r, !by_ref);
        }
        ExprKind::MethodCall(_, recv, args, _) => {
          self.scan(recv, true);
          for a in args { self.scan(a, true); }
        }
        ExprKind::Call(callee, args) => {
          self.scan(callee, true);
          for a in args { self.scan(a, true); }
        }
        ExprKind::Tup(items) | ExprKind::Array(items) => {
          for i in items { self.scan(i, true); }
        }
        // A macro's own blocks, e.g. `format!`'s: its arguments are
        // borrowed (`(&s,)`) in a `let` inside.
        ExprKind::Block(b, _) if e.span.from_expansion() => {
          for st in b.stmts {
            match st.kind {
              StmtKind::Let(l) => if let Some(init) = l.init { self.scan(init, true) },
              StmtKind::Expr(x) | StmtKind::Semi(x) => self.scan(x, true),
              StmtKind::Item(_) => {}
            }
          }
          if let Some(tail) = b.expr {
            self.scan(tail, by_value);
          }
        }
        _ => {
          struct All<'a, 'tcx> { tcx: rustc_middle::ty::TyCtxt<'tcx>, moved: &'a mut BTreeSet<String> }
          impl<'a, 'tcx> Visitor<'tcx> for All<'a, 'tcx> {
            fn visit_expr(&mut self, e: &'tcx Expr<'tcx>) {
              if let ExprKind::Path(QPath::Resolved(_, p)) = e.kind {
                if let Res::Local(id) = p.res {
                  self.moved.insert(binding_name(&self.tcx, id));
                }
              }
              // A closure's body isn't walked; what it captures is.
              if let ExprKind::Closure(c) = e.kind {
                if let Some(upvars) = self.tcx.upvars_mentioned(c.def_id) {
                  for id in upvars.keys() {
                    self.moved.insert(binding_name(&self.tcx, *id));
                  }
                }
              }
              intravisit::walk_expr(self, e);
            }
          }
          All { tcx: self.tcx, moved: &mut self.moved }.visit_expr(e);
        }
      }
    }
  }
  let owner = e.hir_id.owner;
  let mut scan = Scan {
    tcx,
    typeck: tcx.typeck(owner),
    env: rustc_middle::ty::TypingEnv::post_analysis(tcx, owner),
    moved: BTreeSet::new(),
  };
  scan.scan(e, true);
  scan.moved
}

impl<'a, 'tcx> ExprVisitor<'a, 'tcx> {
  /// Emit a GoOutOfScope where the function or a loop iteration is
  /// left early, for each variable dropped there that still owns a
  /// resource. Rust drops them right there: at a `return` (#363) or
  /// a panic, which unwinds (#410), every local of the function; at a
  /// `break` / `continue` (`loop_id` is its target), those declared
  /// inside the loop. Copy values and references own nothing to drop.
  ///
  /// The exit is drawn on the columns of the innermost if/else or
  /// match arm it's in. What that branch declared (its `let`s, an
  /// `if let` / arm pattern's bindings) is dropped here too, first, as
  /// Rust drops in reverse declaration order (#630); the branch then
  /// doesn't drop it again at its closing brace (`ends_at_exits`). A
  /// loop inside the branch ends what its body declares itself.
  /// `value` is the expression the exit carries out (`return e`,
  /// `break e`); what it may move isn't dropped. `how`: `return`,
  /// `break`, `continue` or `panic`, for the drop of a temporary.
  ///
  /// A tuple-literal scrutinee whose arm or if branch the exit leaves
  /// (`live_tuples`) is dropped too, after what the arm or branch
  /// declared and before the locals declared before its match /
  /// `if let` (#327). It has no column, and its drop doesn't end one,
  /// so it is stated even where the locals' drops aren't drawn (the
  /// exit is in an `if` without `else`).
  fn drop_locals_on_exit(&mut self, line: usize, value: Option<&'tcx Expr<'tcx>>, loop_id: Option<HirId>, labeled: bool, how: &str) {
    let Some(branch_at) = self.exit_frames.iter()
      .rposition(|f| matches!(f, ExitFrame::Branch { .. })) else { return };
    let ExitFrame::Branch { len: branch_len, .. } = self.exit_frames[branch_at] else { return };
    // `frame_floor`: the exit leaves the constructs from this frame on.
    let (from, frame_floor) = match loop_id {
      None => (0, 0),
      Some(target) => match self.exit_frames.iter()
        .rposition(|f| matches!(f, ExitFrame::Loop { id, .. } if *id == target))
      {
        // The branch is inside the loop: drop what the iteration
        // declared before the branch.
        Some(i) if i < branch_at => match &self.exit_frames[i] {
          // A labeled `break` leaves the target `for` loop with its
          // iterator, which its head's temporaries hold (#596).
          ExitFrame::Loop { len, head_from, .. } => (if labeled { *head_from } else { *len }, i + 1),
          ExitFrame::Branch { .. } => return,
        },
        // A labelled block, or a loop inside the branch (no branch
        // ends there).
        _ => return,
      },
    };
    // Of an `if` / `match` value's branches, only what every branch
    // moves is gone on all paths; the drop of the rest (drawn with a
    // drop flag) depends on the branch taken (#674).
    // The ones some branches move and others don't are dropped on the
    // paths through the others: stated by a dot saying so (#697).
    let (moved, moved_some) = value.map(|e| {
      let mut values = Vec::new();
      if matches!(e.kind, ExprKind::If(_, _, Some(_)) | ExprKind::Match(_, _, rustc_hir::MatchSource::Normal)) && !e.span.from_expansion() {
        self.returned_values(e, &mut values);
      }
      let sets: Vec<BTreeSet<String>> = values.into_iter().map(|v| locals_moved_by(self.tcx, v)).collect();
      match sets.split_first() {
        Some((first, rest)) => {
          let all = rest.iter().fold(first.clone(), |acc, m| acc.intersection(m).cloned().collect());
          let any = rest.iter().fold(first.clone(), |acc, m| acc.union(m).cloned().collect());
          let some = any.difference(&all).cloned().collect::<BTreeSet<String>>();
          (all, some)
        }
        None => (locals_moved_by(self.tcx, e), BTreeSet::new()),
      }
    }).unwrap_or_default();
    let end = self.exit_frames[branch_at + 1..].iter()
      .find_map(|f| match f {
        ExitFrame::Loop { len, .. } => Some(*len),
        ExitFrame::Branch { .. } => None,
      })
      .unwrap_or(self.in_scope.len())
      .max(branch_len)
      .min(self.in_scope.len());
    // Innermost last, so popped first: Rust drops the tuple of an
    // inner match before what an outer one holds.
    let mut tuples: Vec<LiveTuple<'tcx>> = self.live_tuples.iter()
      .filter(|t| t.frame >= frame_floor).cloned().collect();
    for (idx, rap) in self.droppable_locals_at(from, end, &moved, true, line, false) {
      // A tuple is dropped after what its arm declared, before the
      // locals declared before its match.
      while tuples.last().is_some_and(|t| idx < t.len) {
        let t = tuples.pop().unwrap();
        self.drop_tuple_on_exit(&t, line);
      }
      let id = *self.unique_id;
      *self.unique_id += 1;
      self.exit_drops.insert(id);
      // An unbound temporary's drop is stated by its own event (#457),
      // which says what it is (and whether a `let` pattern's literal
      // built it, #439); the exit drops it before its scope's end.
      let temp_ty = rap.is_unbound_temporary().then(|| self.preprocessed_events.iter()
        .find_map(|(_, e)| match e {
          ExternalEvent::TemporaryDrop { ro, ty, literal, .. } if ro == &rap => ty.clone().map(|ty| (ty, *literal)),
          _ => None,
        })).flatten();
      // A `let`-extended temporary (#400) is dropped here too, not at
      // the end of the block its `let` is in (#553).
      let extended_temp = rap.is_temporary() && !rap.is_unbound_temporary();
      let when = if how == "panic" {
        "here, as the panic unwinds".to_owned()
      } else {
        format!("here, where `{}` leaves early", how)
      };
      match temp_ty {
        Some((ty, literal)) => {
          let early = self.unbranched_temps.contains(&rap);
          self.add_external_event(line, ExternalEvent::TemporaryDrop { ro: rap, ty: Some(ty), when, literal, early, id })
        }
        None if extended_temp => self.add_external_event(line, ExternalEvent::TemporaryDrop { ro: rap, ty: None, when, literal: false, early: false, id }),
        None => self.add_external_event(line, ExternalEvent::GoOutOfScope { ro: rap, id }),
      }
    }
    while let Some(t) = tuples.pop() {
      self.drop_tuple_on_exit(&t, line);
    }
    // Innermost first, like the drops above. Not `exit_drops`: the
    // column doesn't end in this dot, whose drop happens on some paths
    // only (#697).
    let mut seen: HashSet<&str> = HashSet::new();
    let maybe_dropped: Vec<ResourceAccessPoint> = self.in_scope[from..end].iter().rev()
      .filter(|n| seen.insert(n.as_str()))
      .filter(|n| moved_some.contains(n.split('.').next().unwrap_or(n)))
      .filter_map(|n| self.raps.get(n))
      .filter(|rd| !rd.rap.is_copy() && !rd.rap.is_fn() && !rd.rap.is_ref() && !self.skip_raps.contains(&rd.rap.name().to_string()))
      .map(|rd| rd.rap.clone())
      .collect();
    for rap in maybe_dropped {
      let id = *self.unique_id;
      *self.unique_id += 1;
      self.add_external_event(line, ExternalEvent::OwnerDropUnlessReturned { ro: rap, id });
    }
  }

  /// The drop of the live tuple scrutinee `t` at an exit on `line`
  /// (#327), worded as #327's "when this arm leaves the match early" /
  /// "when the if branch leaves early". Not an `exit_drops` event: it
  /// ends no column, so it stays wherever the exit is drawn.
  fn drop_tuple_on_exit(&mut self, t: &LiveTuple<'tcx>, line: usize) {
    for ev in self.tuple_drop_events(t.scrutinee, &t.tuple, Some(t.pat), t.at) {
      self.add_external_event(line, ev);
    }
  }

  /// Moves the unbound temporaries (#457, #469) declared since
  /// `in_scope[from]` (an `if let` scrutinee's) ahead of the other
  /// names declared since (its pattern's bindings), keeping each
  /// group's order, and returns where the temporaries end: an exit in
  /// the branch drops what comes before that, and the bindings at the
  /// branch's end.
  fn hoist_unbound_temporaries(&mut self, from: usize) -> usize {
    let tail = self.in_scope.split_off(from);
    let (temps, rest): (Vec<String>, Vec<String>) = tail.into_iter()
      .partition(|n| self.raps.get(n).is_some_and(|rd| rd.rap.is_unbound_temporary()));
    self.in_scope.extend(temps);
    let len = self.in_scope.len();
    self.in_scope.extend(rest);
    len
  }

  /// The locals in `in_scope[from..end]` that own a value to drop at
  /// an exit, in the order Rust drops them: reverse declaration order,
  /// a name declared twice (shadowing) once. `moved` are the ones the
  /// exit's value hands on; `refs` says whether to include
  /// non-`Copy` references (`&mut T`). A temporary whose scope ended
  /// before `line` was dropped already. Each comes with its index in
  /// `in_scope`.
  fn droppable_locals_at(&self, from: usize, end: usize, moved: &BTreeSet<String>, refs: bool, line: usize, binds: bool) -> Vec<(usize, ResourceAccessPoint)> {
    if from >= end {
      return Vec::new();
    }
    let mut seen: HashSet<&str> = HashSet::new();
    let mut out = Vec::new();
    for (idx, name) in self.in_scope[from..end].iter().enumerate().rev()
      .map(|(i, n)| (from + i, n)).filter(|(_, n)| seen.insert(n.as_str())) {
      let base = name.split('.').next().unwrap_or(name);
      let Some(rd) = self.raps.get(name) else { continue };
      if rd.rap.is_copy() || rd.rap.is_fn() || (!refs && rd.rap.is_ref())
        || moved.contains(base) || self.skip_raps.contains(name) {
        continue;
      }
      // A temporary (#400, #457) stays in `in_scope` until its block
      // ends, but one whose own scope (its statement, for an unbound
      // one) ended above was dropped there already.
      if rd.rap.is_temporary() && rd.scope < line {
        continue;
      }
      if !self.owns_value_here(*rd.rap.hash(), binds) {
        continue;
      }
      out.push((idx, rd.rap.clone()));
    }
    out
  }

  /// The `?` operator `e?` returns early on `Err` / `None`, dropping
  /// every value the function owns at that point (#430). That happens
  /// on the error path only, so it isn't a drop that ends a column:
  /// each such owner gets a dot saying so, on the line of the `?`.
  ///
  /// `r?` with `r` a non-`Copy` variable (or field) moves `r` first, on
  /// both paths (#652): unless `match_rhs` drew that move into the
  /// variable `r?` is bound to, it is drawn here as a move into the
  /// `?`. `r` isn't dropped by the early return: the error in it goes
  /// to the caller.
  ///
  /// A tuple-literal scrutinee whose arm or if branch the `?` is in
  /// (`live_tuples`) is dropped too if it returns early, after what
  /// the arm declared and before what was declared before the match
  /// (#653). It has no column: like at an exit, each variable whose
  /// value is still in it gets a dot that says so.
  fn note_try(&mut self, try_expr: &'tcx Expr<'tcx>) {
    let line = span_end_line(&try_expr.span, &self.tcx);
    let moved = self.try_moved_place(try_expr);
    if let Some((rap, kind, operand)) = &moved {
      if !self.try_moves.contains_key(&try_expr.hir_id) {
        let id = *self.unique_id;
        *self.unique_id += 1;
        self.add_external_event(span_to_line(&operand.span, &self.tcx), ExternalEvent::Move {
          from: ResourceTy::Value(rap.clone()),
          to: ResourceTy::Anonymous,
          is_partial: false,
          into_closure: false,
          loop_head: LoopHead::No,
          break_alt: None,
          break_out: false,
          via_try: Some(*kind),
          closure_value: false,
          id,
        });
        self.try_moves.insert(try_expr.hir_id, Some(id));
      }
    }
    if !self.in_closure(try_expr) {
      self.note_try_loop_iterators(line);
    }
    let end = self.in_scope.len();
    let mut tuples: Vec<LiveTuple<'tcx>> = self.live_tuples.clone();
    let tuple_dup = self.preprocessed_events.iter().any(|(l, e)| *l == line
      && matches!(e, ExternalEvent::TupleTempDrop { at: TupleDropAt::TryReturns, .. }));
    if tuple_dup {
      tuples.clear();
    }
    for (idx, rap) in self.droppable_locals_at(0, end, &BTreeSet::new(), false, line, true) {
      while tuples.last().is_some_and(|t| idx < t.len) {
        let t = tuples.pop().unwrap();
        self.drop_tuple_if_try_returns(&t, line);
      }
      if moved.as_ref().is_some_and(|(r, _, _)| r.hash() == rap.hash()) {
        continue;
      }
      let dup = self.preprocessed_events.iter().any(|(l, e)| *l == line
        && matches!(e, ExternalEvent::OwnerDropIfTryReturns { ro, .. } if *ro == rap));
      if dup {
        continue;
      }
      let id = *self.unique_id;
      *self.unique_id += 1;
      self.add_external_event(line, ExternalEvent::OwnerDropIfTryReturns { ro: rap, id });
    }
    while let Some(t) = tuples.pop() {
      self.drop_tuple_if_try_returns(&t, line);
    }
  }

  /// The drop of the live tuple scrutinee `t` if the `?` on `line`
  /// returns early (#653).
  fn drop_tuple_if_try_returns(&mut self, t: &LiveTuple<'tcx>, line: usize) {
    for ev in self.tuple_drop_events(t.scrutinee, &t.tuple, Some(t.pat), TupleDropAt::TryReturns) {
      self.add_external_event(line, ev);
    }
  }

  /// A `return` also drops the iterators of the `for` loops it leaves,
  /// which variables were moved into (#482), where its other drops are
  /// drawn (`drop_locals_on_exit`: in a branch). The caller leaves out
  /// a `return` in a closure, which leaves only the closure.
  fn drop_loop_iterators_on_return(&mut self, line: usize) {
    if !self.exit_frames.iter().any(|f| matches!(f, ExitFrame::Branch { .. })) {
      return;
    }
    for (_, ro) in self.loop_iterators.clone() {
      let id = self.add_loop_iterator_drop(ro, line, LoopLeft::Return);
      self.exit_drops.insert(id);
    }
  }

  /// A `?` inside a `for` loop over an iterator variable returns early
  /// on `Err` / `None`, dropping that iterator with the rest (#649).
  /// Conditional, so it ends no column (like `note_try`'s dots).
  fn note_try_loop_iterators(&mut self, line: usize) {
    for (_, ro) in self.loop_iterators.clone() {
      let dup = self.preprocessed_events.iter().any(|(l, e)| *l == line
        && matches!(e, ExternalEvent::LoopIteratorDrop { ro: r, left: LoopLeft::Try, .. } if *r == ro));
      if !dup {
        self.add_loop_iterator_drop(ro, line, LoopLeft::Try);
      }
    }
  }

  /// A `break 'outer` out of an inner `for` loop leaves it too (#592):
  /// the iterator variables the loops inside the target took (`for s in
  /// it`) are dropped at the `break`, on their own columns, where
  /// `drop_locals_on_exit` draws the exit's other drops (in a branch
  /// inside the target loop). A labeled `break` leaves the target `for`
  /// loop too, so its own iterator variable is dropped here as well
  /// (`labeled`; #596). An iterator a loop built from its head (`for
  /// s in row`) is a `(temporary N)` column declared inside the target
  /// loop, which `drop_locals_on_exit` drops itself (#410).
  fn drop_inner_loop_iterators(&mut self, line: usize, target: HirId, labeled: bool, left: LoopLeft) {
    let Some(branch_at) = self.exit_frames.iter()
      .rposition(|f| matches!(f, ExitFrame::Branch { .. })) else { return };
    let loop_at = self.exit_frames.iter()
      .rposition(|f| matches!(f, ExitFrame::Loop { id, .. } if *id == target));
    if !loop_at.is_some_and(|i| i < branch_at) {
      return;
    }
    let inner: Vec<ResourceAccessPoint> = self.loop_iterators.iter()
      .filter(|(lp, _)| (labeled && *lp == target)
        || (*lp != target
        && self.tcx.hir_parent_iter(*lp).any(|(id, _)| id == target)))
      .map(|(_, ro)| ro.clone())
      .collect();
    for ro in inner {
      let id = self.add_loop_iterator_drop(ro, line, left);
      self.exit_drops.insert(id);
    }
  }

  /// The drop of the iterator `ro` was moved into by `for s in ro`, at
  /// `line`: where the loop ends, or (`early`) at a `return` inside it
  /// (#482). Drawn on `ro`'s column, which doesn't own it any more.
  /// Returns the event's id.
  pub fn add_loop_iterator_drop(&mut self, ro: ResourceAccessPoint, line: usize, left: LoopLeft) -> usize {
    let id = *self.unique_id;
    *self.unique_id += 1;
    self.add_external_event(line, ExternalEvent::LoopIteratorDrop { ro, left, id });
    id
  }

  /// If `call` is a call to a panic function (what `panic!()`,
  /// `unreachable!()`, `todo!()`, `unimplemented!()` and
  /// `std::panic::panic_any` call), the line of the panic in the
  /// user's code. A panic unwinds the stack, dropping every local on
  /// the way (#410). `std::process::exit` also never returns, but
  /// runs no destructors, so it isn't one.
  fn panic_line(&self, call: &'tcx Expr<'tcx>) -> Option<usize> {
    let ExprKind::Call(callee, _) = call.kind else { return None };
    let ExprKind::Path(QPath::Resolved(_, path)) = callee.kind else { return None };
    let Res::Def(DefKind::Fn, def_id) = path.res else { return None };
    if !matches!(self.tcx.crate_name(def_id.krate).as_str(), "core" | "std") {
      return None;
    }
    let def_path = self.tcx.def_path(def_id).to_string_no_crate_verbose();
    let unwinds = (def_path.starts_with("::panicking::")
        && !def_path.contains("nounwind") && !def_path.contains("cannot_unwind"))
      || def_path.ends_with("::begin_panic")
      || def_path == "::panic::panic_any"
      || def_path == "::panic::resume_unwind";
    unwinds.then(|| span_to_line(&call.span.source_callsite(), &self.tcx))
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

  /// Record what an exit in the conditional `expr`, whose Branch event
  /// gets the next id, leaves (see `ExitScope`).
  fn note_exit_scope(&mut self, expr: &'tcx Expr<'tcx>) {
    let sm = self.tcx.sess.source_map();
    let loop_lines = self.exit_frames.iter().rev()
      .find_map(|f| match f {
        ExitFrame::Loop { id, .. } => Some(self.tcx.hir_span(*id)),
        ExitFrame::Branch { .. } => None,
      })
      .map(|sp| (sm.lookup_char_pos(sp.lo()).line, sm.lookup_char_pos(sp.hi()).line));
    self.exit_scopes.insert(*self.unique_id, ExitScope { in_closure: self.in_closure(expr), loop_lines });
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

  /// Remove the drops of exits (`drop_locals_on_exit`) emitted while
  /// event ids `ids` were handed out (see `take_branch_events`) and
  /// still on the function's own column: they're in a branch that
  /// isn't drawn as one (an `if` without `else`, a single-arm match),
  /// where they'd end the column for every path.
  fn discard_exit_drops(&mut self, ids: std::ops::Range<usize>) {
    let exit_drops = &self.exit_drops;
    self.preprocessed_events.retain(|(_, e)|
      !(ids.contains(&e.get_id()) && exit_drops.contains(&e.get_id())));
  }

  /// The ranges of event ids that bind the value of each of these
  /// branch bodies to a `let` (`rhs_arm_ids`), for those that have one.
  fn rhs_ids_of(&self, bodies: impl IntoIterator<Item = Option<&'tcx Expr<'tcx>>>) -> Vec<std::ops::Range<usize>> {
    bodies.into_iter().flatten()
      .filter_map(|b| self.rhs_arm_ids.get(&b.hir_id))
      .map(|&(lo, hi)| lo..hi)
      .collect()
  }

  /// `drop_locals_on_exit` for a panic call inside a branch.
  fn note_panic(&mut self, call: &'tcx Expr<'tcx>) {
    if let Some(line) = self.panic_line(call) {
      self.drop_locals_on_exit(line, None, None, false, "panic");
      if !self.in_closure(call) && self.exit_frames.iter().any(|f| matches!(f, ExitFrame::Branch { .. })) {
        for (_, ro) in self.loop_iterators.clone() {
          let id = self.add_loop_iterator_drop(ro, line, LoopLeft::Panic);
          self.exit_drops.insert(id);
        }
      }
    }
  }

  /// Whether the variable `hash` owns a value at this point on every
  /// path that reaches it, from the events so far: given one (`let`,
  /// assignment, by-value parameter) and not moved out since. When a
  /// conditional above moved it, or gave it a value, on some paths
  /// only, it's false: whether it's dropped here then depends on the
  /// path (Rust's drop flag), which an exit's drop dot can't say.
  ///
  /// With `binds`, a match arm's pattern bindings count too (#636):
  /// `Some(s) =>` moved the String out of the scrutinee into `s`.
  /// (Without, an exit in the arm still emits a scope end for the
  /// scrutinee, whose dot says from the column's state that nothing is
  /// dropped.)
  fn owns_value_here(&self, hash: u64, binds: bool) -> bool {
    #[derive(Clone, Copy, PartialEq)]
    enum Own { Yes, No, Maybe }
    fn step(events: &[(usize, ExternalEvent)], hash: u64, hide: &[std::ops::Range<usize>], mut own: Own) -> Own {
      let is = |r: &ResourceTy| *r.hash() == hash;
      for (_, e) in events {
        if hide.iter().any(|ids| ids.contains(&e.get_id())) {
          continue;
        }
        own = match e {
          ExternalEvent::Move { to, .. } | ExternalEvent::Copy { to, .. } | ExternalEvent::Bind { to, .. }
            if is(to) => Own::Yes,
          ExternalEvent::Move { from, .. } if is(from) => Own::No,
          // Moved into an or-pattern's binding only if its alternative
          // matched (#533). (Out of a tuple scrutinee, `tuple`, the
          // place was moved into the tuple already.)
          ExternalEvent::OrAltMove { ro, tuple: None, .. } if *ro.hash() == hash && own == Own::Yes => Own::Maybe,
          ExternalEvent::InitRefParam { param, .. } if *param.hash() == hash => Own::Yes,
          // A branch that diverges doesn't reach this point.
          ExternalEvent::Branch { branches, .. } => {
            let mut ends = branches.iter().filter(|b| !b.diverges).map(|b| step(&b.e_data, hash, &[], own));
            match ends.next() {
              None => own,
              Some(first) => if ends.all(|o| o == first) { first } else { Own::Maybe },
            }
          }
          _ => own,
        };
      }
      own
    }
    // Events of other branches of the conditionals the exit is in.
    let hide: Vec<std::ops::Range<usize>> = self.exit_frames.iter().flat_map(|f| match f {
      ExitFrame::Branch { hide, .. } => hide.clone(),
      ExitFrame::Loop { .. } => Vec::new(),
    }).collect();
    let frame_binds: Vec<(usize, &Vec<(usize, ExternalEvent)>)> = if binds {
      self.exit_frames.iter().filter_map(|f| match f {
        ExitFrame::Branch { binds, binds_at, .. } if !binds.is_empty() => Some((*binds_at, binds)),
        _ => None,
      }).collect()
    } else {
      Vec::new()
    };
    if frame_binds.is_empty() {
      return step(self.preprocessed_events, hash, &hide, Own::No) == Own::Yes;
    }
    // The events in the order they happened: each arm's bindings
    // where its body starts.
    let mut events: Vec<(usize, ExternalEvent)> = Vec::new();
    let mut from = 0;
    for (at, b) in frame_binds {
      let at = at.clamp(from, self.preprocessed_events.len());
      events.extend_from_slice(&self.preprocessed_events[from..at]);
      events.extend_from_slice(b);
      from = at;
    }
    events.extend_from_slice(&self.preprocessed_events[from..]);
    step(&events, hash, &hide, Own::No) == Own::Yes
  }

  /// Whether every path through `e` leaves at an exit that drops what
  /// `e` declares, so its closing brace is never reached and states no
  /// drop (#630, #631): a `return`, a panic, or a `break` / `continue`
  /// out of a loop, each of whose drops `drop_locals_on_exit` draws.
  /// `branch`: `e` is an if/else branch or the arm of a match with
  /// several arms, whose columns an exit directly in it ends. A loop
  /// body (`branch` false) is drawn inline, so only an exit in an
  /// if/else or match nested in it is drawn with its drops.
  fn ends_at_exits(&self, e: &'tcx Expr<'tcx>, branch: bool) -> bool {
    use rustc_hir::Node;
    match e.kind {
      ExprKind::Ret(_) | ExprKind::Continue(_) => branch,
      // A `break` out of a labelled block drops nothing drawn.
      ExprKind::Break(dest, _) => branch && dest.target_id.is_ok_and(|t|
        matches!(self.tcx.hir_node(t), Node::Expr(Expr { kind: ExprKind::Loop(..), .. }))),
      ExprKind::Call(..) => branch && self.panic_line(e).is_some(),
      ExprKind::If(_, then, Some(els)) if !e.span.from_expansion() =>
        self.ends_at_exits(then, true) && self.ends_at_exits(els, true),
      ExprKind::Match(_, arms, rustc_hir::MatchSource::Normal) if !e.span.from_expansion() =>
        arms.len() > 1 && arms.iter().all(|a| self.ends_at_exits(a.body, true)),
      ExprKind::DropTemps(x) => self.ends_at_exits(x, branch),
      ExprKind::Block(b, _) => self.block_ends_at_exits(b, branch),
      _ => false,
    }
  }

  /// `ends_at_exits` for a block: what runs first that never completes
  /// decides.
  fn block_ends_at_exits(&self, b: &'tcx rustc_hir::Block<'tcx>, branch: bool) -> bool {
    for st in b.stmts {
      match st.kind {
        StmtKind::Expr(x) | StmtKind::Semi(x) if self.expr_diverges(x) => return self.ends_at_exits(x, branch),
        StmtKind::Let(l) if l.init.is_some_and(|i| self.expr_diverges(i)) => return false,
        _ => {}
      }
    }
    b.expr.is_some_and(|t| self.ends_at_exits(t, branch))
  }

  /// The variables of `decls` whose GoOutOfScope at the end of a body
  /// is left out because no path reaches that end (`ends`, from
  /// `ends_at_exits`): each one that owns a value is dropped at the
  /// exits, or was moved away before them. A Copy value or a reference
  /// has no drop at an exit, and keeps its scope end (as at the end of
  /// a function every path left, #515).
  fn left_at_exits(ends: bool, decls: &HashSet<ResourceAccessPoint>) -> HashSet<ResourceAccessPoint> {
    if !ends {
      return HashSet::new();
    }
    #[expect(clippy::disallowed_methods, reason = "collected into a set; order is lost")]
    let left = decls.iter().filter(|v| !v.is_copy()).cloned().collect();
    left
  }

  /// Keep an exit's drops (`drop_locals_on_exit`) only in a
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
    for (line, is_mut, r, lender) in std::mem::take(&mut self.pending_param_borrows) {
      self.add_borrow_ev(line, if is_mut { Evt::MBorrow } else { Evt::SBorrow }, r, lender, false);
    }

    // The tail of an `if` / `match` returned as the body's value: each
    // branch's value goes to the caller in its branch (#674).
    let tail = match body.value {
      Expr{kind: ExprKind::Block(b, _), ..} => b.expr,
      _ => {
        warn!("unexpected fn body {:#?}", body);
        None
      }
    };
    // A tail `return v` is handled by visit_expr's `Ret` arm. Only
    // append this event if the fn returns a value.
    let tail = tail.filter(|e| self.fn_ret && !matches!(e.kind, ExprKind::Ret(_)));
    let plain_tail = tail.filter(|e| !self.mark_branch_returns(e));
    self.visit_expr(body.value); // visit fn body
    if let Some(e) = plain_tail {
      self.return_to_caller(e);
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
        PatKind::Binding(binding_annotation, binding_hirid, ident, _op_pat) =>{
          let name: String = binding_name(&self.tcx, binding_hirid);
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
            // `ref s` on a by-value parameter (or part of one: `(ref a,
            // b)`), whose type needs drop: the value `f` owns gets a
            // column of its own, which `s` borrows and which is dropped
            // when `f` returns (#691, #698). (Without drop, `s` borrows
            // a value with no column and `Caller` stands in.)
            let value_col = own_param.as_ref().filter(|o| o.drops).map(|_| {
              let pointee = ty.builtin_deref(false).unwrap();
              self.param_value_count += 1;
              let col = parameter_label(self.param_value_count);
              let owner_id = param.hir_id.owner;
              let is_mut = bool_of_mut(ty.ref_mutability().unwrap());
              self.add_owner(col.clone(), is_mut, false, self.ty_is_plain(pointee, owner_id), self.ty_has_interior_mut(pointee, owner_id), self.current_scope, !self.inside_branch);
              self.mark_owner_type(&col, pointee);
              col
            });
            let lender = match &value_col {
              Some(col) => ResourceTy::Value(self.raps.get(col).unwrap().rap.to_owned()),
              None => ResourceTy::Caller,
            };
            self.add_ref(name.clone(),
            bool_of_mut(ty.ref_mutability().unwrap()),
            bool_of_mut(binding_annotation.1), line_num,
            lender.clone(), VecDeque::new(), self.current_scope, !self.inside_branch, pointee_copy);
            // Stretch the loan to the closing brace so the dashed
            // ref-line trapezoid covers the full body.
            if let Some(rd) = self.borrow_map.get_mut(&name) {
              rd.lifetime = self.current_scope;
            }
            if let Some(col) = value_col {
              // The caller's value arrives in the column, and `s`
              // borrows it, both on the signature's line.
              let col_rap = self.raps.get(&col).unwrap().rap.to_owned();
              self.add_external_event(line_num, ExternalEvent::InitRefParam { param: col_rap, id: *self.unique_id, own_param: own_param.clone(), in_pattern: None });
              *self.unique_id += 1;
              // The borrows are added once every parameter is
              // registered (`visit_body`): arrows on one line are
              // stacked together, which would push the next
              // parameter's init dot down a row.
              let r = self.raps.get(&name).unwrap().rap.to_owned();
              self.pending_param_borrows.push((line_num, ty.ref_mutability().unwrap() == Mutability::Mut, ResourceTy::Value(r), lender));
              self.annotate_src(name.clone(), ident.span, false, *self.raps.get(&name).unwrap().rap.hash());
              continue;
            }
          }
          else if let Some(inner) = self.box_with_field_moved_out(&name, ty) {
            // A `Box<Struct>` parameter a field is moved out of (#602):
            // drawn like a struct parameter, as in a `let`.
            let owner_hash = self.rap_hashes as u64;
            let muta = bool_of_mut(binding_annotation.1);
            self.add_struct(name.clone(), owner_hash, false, muta, false, false, InteriorMut::No, self.current_scope, !self.inside_branch);
            if let Some(ResourceAccessPoint::Struct(s)) = self.raps.get_mut(&name).map(|rd| &mut rd.rap) {
              s.is_box = true;
            }
            self.register_struct_members(&name, inner, owner_hash, muta, MemberSite {
              owner_id: param.hir_id.owner,
              assigned_at: line_num,
              scope: self.current_scope,
              is_global: !self.inside_branch,
              ref_lender: Some(ResourceTy::Caller),
            });
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
                  self.mark_owner_type(&name, ty);
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
    // A value a branch of the returned `if` / `match` yields goes to
    // the caller once it's evaluated, inside its branch (#674).
    if self.tail_values.remove(&expr.hir_id) {
      self.visit_expr(expr);
      self.return_to_caller(expr);
      return;
    }
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
        if let Some(name) = self.std_value_macro(expr) {
          self.note_macro_temporary_drop(expr, name);
        }
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
          self.note_temporary_drop(expr, name, Vec::new());
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
            // The callee is a name (`f(x)`, `Point::new(..)`, a local
            // closure `g()`), or an expression whose value is called:
            // a closure in a struct field (`(w.f)()`, #703), a call's
            // result (`make(s)(1)`), an element (`fs[0]()`). Such a
            // callee is evaluated before the arguments; its source text
            // names the call.
            let fn_name: String = match hirid_to_var_name(fn_expr.hir_id, &self.tcx) {
              Some(name) => name,
              None => {
                self.visit_expr(fn_expr);
                self.callee_text(fn_expr)
              }
            };
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
        // `std::panic::panic_any(…)` written out (#410): it unwinds
        // once its argument has been passed.
        self.note_panic(expr);
      }
      
      // <expr>.<function>([args])
      ExprKind::MethodCall(name_and_generic_args, rcvr, args, _) => {
        let line_num = expr_to_line(&rcvr, &self.tcx);
        let fn_name = name_and_generic_args.ident.as_str().to_owned();
        self.add_fn(fn_name.clone());
        self.note_temporary_drop(expr, fn_name.clone(), Vec::new());
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
              self.hand_loan_to_carriers(lhs_rap.name(), &ref_data);
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
                self.add_borrow_end_ev(line_num, to_ro, lhs_rty.clone(), &ref_data, Vec::new());
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
              // The reference whose loan the new value is (#644): in a
              // destructuring assignment, as found before any part was
              // drawn. One the same statement reassigns (`(m1, m2) =
              // (m2, m1)`) hands its loan over right away.
              let (src_loan, taken_over_now) = match destructure_hid {
                Some(h) => match self.destructure_ref_src.remove(&h) {
                  Some((n, sp, now)) => (Some((n, sp)), now),
                  None => (None, false),
                },
                None => (self.source_ref_loan(rhs_expr), false),
              };
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
              r.loan_line = call_loan_line(rhs_expr, line_num);
              r.co_lenders = new_co_lenders;
              if taken_over_now {
                self.note_ref_carrier(lhs_rap.name(), None);
                if let Some((_, sp)) = src_loan {
                  self.take_over_loan(lhs_rap.name(), sp);
                }
              } else {
                self.note_ref_carrier(lhs_rap.name(), src_loan);
              }
            }

            // ex:
            // let a = & mut b (where b is &i32)
            // *a = &c (where c is &i32)
            ResourceTy::Deref(_) => {
              let ref_data = self.borrow_map.get(lhs_rap.name()).unwrap().clone();
              let deref_index = num_derefs(&lhs_expr) - 1;
              let modified_ref_name = ref_data.aliasing.get(deref_index).unwrap().clone();
              let modified_ref_data = self.borrow_map.get(&modified_ref_name).unwrap().clone();
              let to_ro = modified_ref_data.lender.clone();
              // References that copied the old value of the modified
              // reference (`let h = o;` before `w = &mut o; *w = &q;`)
              // keep its loan, as for `o = &q` (#664).
              self.hand_loan_to_carriers(&modified_ref_name, &modified_ref_data);

              // add event
              // (Other references with the same lender, a carrier among
              // them, keep the lender borrowed.)
              let borrowers = get_borrowers(&modified_ref_name, &self.borrow_map);
              // `o` stays live across the write (w borrows it), so
              // unlike `o = &q`, which ends the old loan because `o` is
              // dead until then, the borrow checker keeps the old loan
              // live while `o` is: until the loan's end, past the write
              // (#664). `o` no longer points to the lender, so like a
              // field write (#531) the end is drawn on the lender's
              // column only.
              let old_end = self.loan_end(&modified_ref_data).filter(|&end| end > line_num)
                .zip(self.raps.get(&modified_ref_name).map(|r| r.rap.clone()));
              if borrowers.len() > 1 {
                // Another reference keeps the lender borrowed and gives
                // it back. (A RefDie from `*w` would be drawn on w's
                // column with w's mutability, not the old value's.)
              }
              else if let (Some((end_line, o_rap)), ResourceTy::Value(_)) = (old_end, &to_ro) {
                let place = ResourceTy::Index(o_rap, modified_ref_name.clone(), PlaceWrite::Update);
                let die = if modified_ref_data.ref_mutability { Evt::MDie } else { Evt::SDie };
                self.add_ev(end_line, die, to_ro, place, false);
              }
              else {
                self.add_borrow_end_ev(line_num, to_ro, lhs_rty.clone(), &modified_ref_data, Vec::new());
              }

              // update modified (derefed) reference's lender and aliasing data
              let src_loan = self.source_ref_loan(rhs_expr);
              let (new_lender, new_aliasing, new_static_lender, new_co_lenders) = self.get_ref_data_full(&rhs_expr);
              let r = self.borrow_map.get_mut(&modified_ref_name).unwrap();
              r.assigned_at = line_num;
              r.aliasing = new_aliasing.clone();
              r.lender = new_lender;
              r.static_lender = new_static_lender;
              r.borrow_span = borrow_span_of(rhs_expr);
              r.loan_line = call_loan_line(rhs_expr, line_num);
              r.co_lenders = new_co_lenders;
              self.note_ref_carrier(&modified_ref_name, src_loan);

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
        // `hold = H { r: &t };` overwrites a whole struct whose
        // reference fields hold borrows (#538). The old value is
        // dropped, so the borrow checker ends the loans it held at the
        // write (a `&mut s` right after compiles), unlike a direct
        // field write (#531), where `hold`'s single lifetime keeps
        // them. The old loans are read before the literal's fields
        // replace them and drawn after its Bind. The same holds for any
        // other new value of a whole local (`hold = make(&t);`,
        // `hold = other;`, #569), whose fields then hold that value's
        // borrows, not the old ones. A struct field of a struct
        // overwritten by a call (`hold.inner = make_inner(&t)`, #638)
        // or by another variable (`hold.inner = other`, #655) takes the
        // new value's borrows the same way (a variable's go with it,
        // `transfer_ref_fields`), while its old loans last until
        // hold's last use (#531; see `struct_field_borrow_ends`).
        let struct_field_ends = match (&lhs_rty, expr.kind) {
          (ResourceTy::Value(rap @ ResourceAccessPoint::Struct(_)), ExprKind::Assign(..))
            if !lhs_is_union_field && matches!(rhs_expr.kind, ExprKind::Struct(..)) => {
            let rap = rap.clone();
            let ends = self.struct_field_borrow_ends(&rap, line_num);
            // The literal's fields hold borrows made here: an earlier
            // move of the old value (`let h2 = hold;`, #637) doesn't
            // hand these on.
            self.restart_struct_field_loans(&rap, line_num);
            ends
          }
          (ResourceTy::Value(rap @ ResourceAccessPoint::Struct(_)), ExprKind::Assign(..))
            if !lhs_is_union_field => {
            let rap = rap.clone();
            let ends = self.struct_field_borrow_ends(&rap, line_num);
            self.reassign_struct_field_lenders(&rap, rhs_expr, lhs_ty, line_num);
            ends
          }
          _ => Vec::new(),
        };
        let mut drop_line = line_num;
        let mut drop_at: Option<usize> = None;
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
          self.draw_struct_field_borrow_ends(struct_field_ends);
          // `v[1] = v[2]`: match_rhs has nothing to say about some
          // RHS shapes (an index read, a field of a call, …), but the
          // element is still overwritten.
          if matches!(lhs_rty, ResourceTy::Index(..)) && self.preprocessed_events.len() == n_events {
            self.add_ev(line_num, Evt::Bind, lhs_rty.clone(), ResourceTy::Anonymous, false);
          }
          // `r = loop { … break a; };`: the loop runs first, and the
          // old value is dropped when its value is assigned, after the
          // loop ends: drawn where the new value arrives (#586), ahead
          // of the arrivals, so that several `break`s' arrows stacked
          // on that line (#561) don't push it onto a row of its own.
          if let (ResourceTy::Value(rap), ExprKind::Loop(_, _, rustc_hir::LoopSource::Loop, _)) = (&lhs_rty, rhs_expr.kind) {
            let arrivals: Vec<(usize, usize)> = self.preprocessed_events.iter().enumerate().skip(n_events)
              .filter(|(_, (_, ev))| matches!(ev,
                ExternalEvent::Move { to: ResourceTy::Value(t), .. }
                | ExternalEvent::Copy { to: ResourceTy::Value(t), .. }
                | ExternalEvent::Bind { to: ResourceTy::Value(t), .. } if t.hash() == rap.hash()))
              .map(|(i, (line, _))| (i, *line))
              .collect();
            match arrivals.first() {
              Some(&(i, line)) => { drop_line = line; drop_at = Some(i); }
              None => drop_line = span_end_line(&rhs_expr.span, &self.tcx),
            }
          }
        }

        let n_before_drops = self.preprocessed_events.len();
        for (rap, maybe_moved) in drop_target.into_iter().chain(field_drop_targets).zip(maybe_moved) {
          let by_cond = maybe_moved && self.moved_by_chain_condition(rap.name());
          self.add_external_event(drop_line, ExternalEvent::OwnerDropAtReassign {
            ro: rap,
            id: *self.unique_id,
            maybe_moved,
            by_cond,
          });
          *self.unique_id += 1;
        }
        if let Some(i) = drop_at {
          let n_drops = self.preprocessed_events.len() - n_before_drops;
          self.preprocessed_events[i..].rotate_right(n_drops);
        }
      }

      // `return v`: like a tail expression, `v` goes to the caller
      // (#318). The path through the `return` never reaches the end
      // of the function, so `v` isn't dropped there on that path;
      // when the `return` sits in a branch, that branch has type `!`
      // and doesn't count at the merge (#296), so code after the
      // conditional still sees `v` as owned.
      ExprKind::Ret(ret) => {
        // `return if b { s } else { t }`: what a branch yields goes to
        // the caller in that branch (#674).
        if let Some(e) = ret {
          // Not the `return` a macro or `?` expands to, and not one in
          // a closure (it returns from the closure) (#515).
          let to_caller = self.fn_ret && !expr.span.from_expansion() && !self.in_closure(expr);
          let plain = !to_caller || !self.mark_branch_returns(e);
          self.visit_expr(e);
          if to_caller && plain {
            self.return_to_caller(e);
          }
        }
        // Leaving the function also drops every value it still owns
        // (#363), except what `ret` moves to the caller (#410). Inside
        // a branch, those drops are drawn on the branch's columns, at
        // the `return`.
        self.drop_locals_on_exit(expr_to_line(expr, &self.tcx), ret, None, false, "return");
        if !self.in_closure(expr) {
          self.drop_loop_iterators_on_return(expr_to_line(expr, &self.tcx));
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
        let entry_len = self.in_scope.len();
        self.visit_block(block); // visit all the statements in the block
        self.in_scope.truncate(entry_len);
          self.current_scope = prev_scope;
        // A bare block (`{ let u = …; }`) in a branch: its locals aren't
        // global, so `print_out_of_scope` skips them and no enclosing
        // construct ends them. They go out of scope at its `}` (#591).
        // (An `if` / `else` / loop body, a match arm, a closure body
        // is not a bare block: its construct ends its own locals.)
        if self.inside_branch
          && matches!(self.tcx.parent_hir_node(expr.hir_id), rustc_hir::Node::Stmt(_) | rustc_hir::Node::Block(_))
        {
          let decls: HashSet<ResourceAccessPoint> = get_decl_of_block(block, &self.tcx, &self.raps)
            .into_iter().filter(|v| needs_scope_end(v)).collect();
          self.emit_scope_end(decls, new_scope);
        }
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
        let name = path_name(p, &self.tcx);
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
        let entry_len = self.in_scope.len();
        let if_lo;
        let if_hash_lo;
        // `if let … = (x, y)` moves `x` and `y` into the tuple even when
        // the pattern doesn't match; the else branch shows that (#327).
        let mut scrutinee_moves: Vec<ResourceAccessPoint> = Vec::new();
        let mut iflet_tuple: Option<&'tcx rustc_hir::LetExpr<'tcx>> = None;
        let mut chain_fail = ChainFail::default();
        // Events handed out ids in the if-branch's range that stay
        // before the split (see the let-chain case below).
        let mut pre_split: HashSet<usize> = HashSet::new();
        let iflet_bindings: HashSet<ResourceAccessPoint> =
          if let ExprKind::Let(let_expr) = guard_expr.kind {
            self.visit_expr(let_expr.init);
            if_hash_lo = self.rap_hashes;
            scrutinee_moves = self.tuple_scrutinee_moves(let_expr.init);
            iflet_tuple = Some(let_expr);
            let (bound, first_in_branch) = self.register_iflet_let_bindings(let_expr, None);
            // Copies out of the scrutinee stay before the split (#496).
            if_lo = first_in_branch;
            bound
          } else if is_let_chain(guard_expr) {
            // `if let A = x && let B = y && cond` (#198): see
            // visit_if_let_chain. The variables in scope here: a name
            // that was in scope earlier and has ended (an earlier `if
            // let`'s binding `a`) is free for the chain's own bindings
            // (#696).
            let outer: HashSet<ResourceAccessPoint> = self.in_scope.iter()
              .filter_map(|name| self.raps.get(name))
              .map(|rd| rd.rap.clone()).filter(|r| !r.is_fn()).collect();
            if_hash_lo = self.rap_hashes;
            let mut decls = HashSet::new();
            let start = *self.unique_id;
            let chain_lo = self.preprocessed_events.len();
            let mut walk = ChainWalk::default();
            self.visit_if_let_chain(guard_expr, &outer, &mut decls, &mut walk);
            if_lo = walk.first_lo.unwrap_or(start);
            chain_fail = self.chain_fail_moves(&walk, chain_lo, else_expr.is_some());
            let binds = self.chain_binding_events(&(if_lo..*self.unique_id), &decls);
            // Copying a value out of a variable declared before the `if`
            // changes nothing about that variable, so, as for a lone `if
            // let` (#496), the copy stays before the split and the
            // variable's column doesn't branch for it (#583).
            pre_split = binds.iter()
              .filter(|(_, ev)| matches!(ev, ExternalEvent::Copy { from: ResourceTy::Value(f), .. } if outer.contains(f)))
              .map(|(_, ev)| ev.get_id())
              .collect();
            let cols = self.chain_binding_columns(&binds, &decls);
            self.binding_columns.extend(cols);
            decls
          } else {
            self.visit_expr(&guard_expr);
            if_lo = *self.unique_id;
            if_hash_lo = self.rap_hashes;
            HashSet::new()
          };
        // Restored, not reset: this `if` may sit inside another branch
        // (#410).
        let prev_inside_branch = self.inside_branch;
        self.inside_branch = true; // need this flag to correctly handle variables that are declared inside blocks
        // Both branches start from the same set of uninitialized
        // `let x;` locals (#234); see join_uninit_locals.
        let uninit_before = self.uninit_locals.clone();
        // An exit in a branch sees neither the other branch's events
        // nor the binding of the `if`'s value to a `let` (#410).
        let hide = self.rhs_ids_of([Some(if_expr), else_expr]);
        // The scrutinee's temporaries (#469) outlive the pattern match,
        // so an exit in a branch drops them (#410).
        let frame_len = self.hoist_unbound_temporaries(entry_len);
        // In edition 2024 those of an `if let` (or a let chain) are
        // dropped before the `else` block runs (#469, #604): they aren't
        // alive on the else path. They get no column in the branches and
        // no join; an exit in the else branch doesn't drop them again.
        let else_drops_temps = is_let_chain(guard_expr)
          && !matches!(expr.span.desugaring_kind(), Some(rustc_span::DesugaringKind::WhileLoop))
          && expr.span.edition() >= rustc_span::edition::Edition::Edition2024;
        let scrutinee_temps: HashSet<ResourceAccessPoint> = if else_drops_temps {
          self.in_scope[entry_len..frame_len].iter()
            .filter_map(|n| self.raps.get(n).map(|rd| rd.rap.clone()))
            .collect()
        } else {
          HashSet::new()
        };
        #[expect(clippy::disallowed_methods, reason = "inserted into a set; order is lost")]
        self.unbranched_temps.extend(scrutinee_temps.iter().cloned());
        self.exit_frames.push(ExitFrame::Branch { len: frame_len, hide, binds: Vec::new(), binds_at: 0 });
        // An exit in the if branch drops the tuple scrutinee (#327).
        let live_tuple = iflet_tuple.filter(|_| !scrutinee_moves.is_empty()).map(|let_expr| LiveTuple {
          frame: self.exit_frames.len() - 1,
          len: entry_len,
          scrutinee: let_expr.init,
          tuple: match self.tuple_scrutinee_target(let_expr.init) {
            ResourceTy::Temporary { tuple: Some(t), .. } => t,
            _ => String::new(),
          },
          pat: let_expr.pat,
          at: TupleDropAt::IfExit,
        });
        let live = live_tuple.is_some();
        self.live_tuples.extend(live_tuple);
        self.visit_expr(&if_expr);
        if live {
          self.live_tuples.pop();
        }
        // Everything declared in each branch, also inside an inline
        // construct nested in it (an `else if let`'s bindings, #501):
        // see raps_declared_since.
        let if_inner = self.raps_declared_since(if_hash_lo, &uninit_before);
        let else_hash_lo = self.rap_hashes;
        let uninit_after_if = std::mem::replace(&mut self.uninit_locals, uninit_before.clone());
        let if_ids = if_lo..*self.unique_id;
        // A let-chain condition that moved a variable before a part
        // that then failed (#281): the else branch (or the implicit
        // empty one) starts with that variable maybe-moved, or, for an
        // assignment, maybe holding its new value (#579). Emitted
        // before the else branch is visited, so it's the first of its
        // events and a reassignment in it sees the drop flag (#335).
        // It's drawn on the `} else {` line; without an `else`, on the
        // `if` line, where the condition fails and the body is skipped.
        let fail_line = match else_expr {
          Some(_) => self.tcx.sess.source_map().lookup_char_pos(if_expr.span.hi()).line,
          None => self.tcx.sess.source_map().lookup_char_pos(if_expr.span.lo()).line,
        };
        for a in std::mem::take(&mut chain_fail.assigns) {
          self.add_external_event(fail_line, ExternalEvent::ChainFailAssign {
            ro: a.rap,
            place: a.place,
            drops_old: a.drops_old,
            no_else: else_expr.is_none(),
            id: *self.unique_id,
          });
          *self.unique_id += 1;
        }
        for m in std::mem::take(&mut chain_fail.moves) {
          self.add_external_event(fail_line, ExternalEvent::LoopExitMove {
            ro: m.rap,
            definite: m.definite,
            at: MaybeMovedAt::ChainFail { to: m.to, dropped: m.dropped, no_else: else_expr.is_none(), how: m.how },
            id: *self.unique_id,
          });
          *self.unique_id += 1;
        }
        // Borrows it made that have ended by then (#688).
        let uses_lo = *self.unique_id;
        for u in std::mem::take(&mut chain_fail.uses) {
          self.add_external_event(fail_line, ExternalEvent::ChainFailUse {
            ro: u.rap,
            uses: u.uses,
            definite: u.definite,
            no_else: else_expr.is_none(),
            id: *self.unique_id,
          });
          *self.unique_id += 1;
        }
        let uses_ids = uses_lo..*self.unique_id;
        let else_decl = match else_expr {
          Some(e) => {
            // The scrutinee's temporaries are gone by now (see
            // `scrutinee_temps`).
            if !scrutinee_temps.is_empty() && self.in_scope.len() >= frame_len {
              self.in_scope.drain(entry_len..frame_len);
              if let Some(ExitFrame::Branch { len, .. }) = self.exit_frames.last_mut() {
                *len = entry_len;
              }
            }
            // The if-body's events stay in `preprocessed_events` until
            // both branches are visited; they aren't on the `else`'s
            // path.
            if let Some(ExitFrame::Branch { hide, .. }) = self.exit_frames.last_mut() {
              hide.push(if_ids.clone());
            }
            // Nor are the if-body's `if let` bindings: an exit in the
            // `else` doesn't drop them.
            self.in_scope.truncate(entry_len);
            self.visit_expr(e);
            get_decl_of_expr(e, &self.tcx, &self.raps)
          }
          None => HashSet::new(),
        };
        let mut else_inner = self.raps_declared_since(else_hash_lo, &uninit_before);
        if let Some(e) = else_expr {
          else_inner = &else_inner | &else_if_let_bindings(e, &self.tcx, &self.raps);
        }
        let uninit_after_else = std::mem::take(&mut self.uninit_locals);
        self.join_uninit_locals(vec![(Some(if_expr), uninit_after_if), (else_expr, uninit_after_else)]);
        let else_ids = if_ids.end..*self.unique_id;
        self.exit_frames.pop();
        // The `if let` bindings go out of scope with the `if`.
        self.in_scope.truncate(entry_len);
        self.inside_branch = prev_inside_branch;

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
        let if_scope = &(&if_decl | &if_inner) | &scrutinee_temps;
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
              .filter(|(_, e)| (if_ids.contains(&e.get_id()) && !pre_split.contains(&e.get_id()))
                || rhs.is_some_and(|(lo, hi)| (lo..hi).contains(&e.get_id())))
              .cloned().collect();
            // (Borrows that ended in the condition, #688, don't make a
            // second path: drawn inline, the condition's events are on
            // the one path already. Their dots are taken out.)
            let chain_fail_ev = self.preprocessed_events.iter()
              .any(|(_, e)| else_ids.contains(&e.get_id()) && !uses_ids.contains(&e.get_id()));
            // A body that always exits (`return`, a panic, `break` /
            // `continue`) and drops something there is drawn as a
            // branch too: its column ends in those drops, and the
            // implicit `else` carries on (#429). On one line it's drawn
            // like any other one-line branch (#360). A body that only
            // sometimes exits (the exit is in a nested conditional drawn
            // inline) has no column to end in them, so they aren't drawn
            // (#410).
            let exit_branch = self.expr_diverges(if_expr)
              && body_ev.iter().any(|(_, e)| self.exit_drops.contains(&e.get_id()));
            // An `if let` whose pattern matches every value always runs
            // its body: there's no implicit empty `else` path, so what
            // the body does to outer variables is drawn inline, not as
            // a branch.
            let always_runs = matches!(guard_expr.kind, ExprKind::Let(l)
              if pat_is_irrefutable(l.pat, self.tcx.typeck(l.pat.hir_id.owner)));
            let drawn_as_branch = (events_move_out_of_outer(&body_ev, &if_scope) && !always_runs) || exit_branch;
            // A tuple scrutinee's variables are moved whether or not the
            // body runs (#327), so that's a second path as well.
            if scrutinee_moves.is_empty() && !drawn_as_branch && !chain_fail_ev {
                self.preprocessed_events.retain(|(_, e)| !uses_ids.contains(&e.get_id()));
                for evs in self.event_line_map.values_mut() {
                  evs.retain(|e| !uses_ids.contains(&e.get_id()));
                }
                // The body may be skipped, so a `return` / `break` /
                // panic in it doesn't end the columns it's drawn on;
                // its drops aren't drawn (#410).
                self.discard_exit_drops(if_ids.clone());
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
        let mut if_ev = self.take_branch_events(if_ids, Some(if_expr), &pre_split);
        let mut else_ev = self.take_branch_events(else_ids, else_expr, &HashSet::new());
        // The arrows now inside the branches are drawn from each
        // branch's own line map. Take them out of the global one, or an
        // arrow that stays on the split line (a copy out of an if-let
        // scrutinee, #496) would drag them out of their branches.
        let in_branches: HashSet<usize> = if_ev.iter().chain(else_ev.iter()).map(|(_, e)| e.get_id()).collect();
        for evs in self.event_line_map.values_mut() {
          evs.retain(|e| !in_branches.contains(&e.get_id()));
        }

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

        // add gos events for variables declared in each block, except
        // where every path through the branch leaves it at an exit
        // that drops them (#630)
        let if_left = Self::left_at_exits(self.ends_at_exits(if_expr, true), &if_decl);
        let else_left = Self::left_at_exits(else_expr.is_some_and(|e| self.ends_at_exits(e, true)), &else_decl);
        for var in in_decl_order(&if_decl).into_iter().filter(|v| needs_scope_end(v) && !if_scoped.contains(v) && !if_left.contains(v)) {
          if_ev.push((if_end, ExternalEvent::GoOutOfScope { ro: var.clone(), id: *self.unique_id }));
          *self.unique_id += 1;
        }

        for var in in_decl_order(&else_decl).into_iter().filter(|v| needs_scope_end(v) && !else_scoped.contains(v) && !else_left.contains(v)) {
          else_ev.push((merge, ExternalEvent::GoOutOfScope { ro: var.clone(), id: *self.unique_id }));
          *self.unique_id += 1;
        }
        if_decl.extend(if_inner);
        if_decl.extend(if_scoped);
        let mut else_decl = else_decl;
        else_decl.extend(else_inner);
        else_decl.extend(else_scoped);
        // The pattern didn't match, but the tuple literal it was matched
        // against already took `x` / `y` (the if branch's own moves come
        // from register_iflet_let_bindings).
        //
        // The tuple is dropped (#327) where an edition-2024 `if let`
        // drops its scrutinee's temporaries: at the end of the if
        // branch (when it leaves early, then), or, when the pattern
        // doesn't match, before the else branch runs. It has no column,
        // so a drop dot on each variable whose value (or part of it)
        // is still in it says so.
        if let Some(let_expr) = iflet_tuple.filter(|_| !scrutinee_moves.is_empty()) {
          let tuple_name = match self.tuple_scrutinee_target(let_expr.init) {
            ResourceTy::Temporary { tuple: Some(t), .. } => t,
            _ => String::new(),
          };
          let mut front: Vec<(usize, ExternalEvent)> = Vec::new();
          for r in &scrutinee_moves {
            front.push((if_end, ExternalEvent::Move {
              from: ResourceTy::Value(r.clone()),
              to: self.tuple_scrutinee_target(let_expr.init),
              id: *self.unique_id,
              is_partial: false,
              into_closure: false,
              loop_head: LoopHead::No,
              break_alt: None,
              break_out: false,
              via_try: None,
              closure_value: false,
            }));
            *self.unique_id += 1;
            liveness.insert(r.clone());
          }
          let at = if else_expr.is_some() { TupleDropAt::Else } else { TupleDropAt::NoElse };
          front.extend(self.tuple_drop_events(let_expr.init, &tuple_name, None, at).into_iter().map(|e| (if_end, e)));
          else_ev.splice(0..0, front);

          // A branch that leaves early drops it at its exit instead:
          // see drop_locals_on_exit.
          if !if_diverges {
            let evs = self.tuple_drop_events(let_expr.init, &tuple_name, Some(let_expr.pat), TupleDropAt::IfEnd);
            if_ev.extend(evs.into_iter().map(|e| (if_end, e)));
          }
        }

        
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
        let (if_diverges, if_exits, if_carries) = self.branch_divergence(if_expr, &if_ev, &if_decl);
        let (else_diverges, else_exits, else_carries) = match else_expr {
          Some(e) => self.branch_divergence(e, &else_ev, &else_decl),
          None => (false, Vec::new(), Vec::new()),
        };
        let (b_labels, b_slices, b_branches) = match else_expr {
          Some(_) => {
            let else_map = create_line_map(&else_ev);
            (
              vec!["If".to_owned(), "Else".to_owned()],
              vec![if_slice, else_slice],
              vec![
                ExtBranchData { e_data: if_ev,   line_map: if_map,   decl_vars: if_decl,   diverges: if_diverges,   exits: if_exits, break_carries: if_carries   },
                ExtBranchData { e_data: else_ev, line_map: else_map, decl_vars: else_decl, diverges: else_diverges, exits: else_exits, break_carries: else_carries },
              ],
            )
          }
          // No `else` (and the body moves something, see above): the
          // implicit empty `else` is a second path that leaves every
          // variable as it was (#257).
          // Its events are the scrutinee's moves (#327), or those of a
          // let-chain condition that failed after a move (#281), if any.
          None => {
            let else_map = create_line_map(&else_ev);
            (
              vec!["If".to_owned(), "Else".to_owned()],
              vec![if_slice, else_slice],
              vec![
                ExtBranchData { e_data: if_ev, line_map: if_map, decl_vars: if_decl, diverges: if_diverges, exits: if_exits, break_carries: if_carries },
                ExtBranchData { e_data: else_ev, line_map: else_map, decl_vars: HashSet::new(), diverges: false, exits: Vec::new(), break_carries: Vec::new() },
              ],
            )
          }
        };
        // The arm of an `if let` with pattern bindings is named so for
        // the drawing: they bind on the split line, and the arm's column
        // starts there (#382).
        let mut b_labels = b_labels;
        if !iflet_bindings.is_empty() {
          b_labels[0] = IF_LET_ARM.to_owned();
        }
        let b_ty = BranchType::If(b_labels, b_slices);
        self.note_exit_scope(expr);
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
        // What a `break` / `continue` in the body drops (#410). A
        // for-loop's frame is pushed where its body is visited, in the
        // ForLoopDesugar arm.
        let entry_len = self.in_scope.len();
        if !matches!(loop_ty, rustc_hir::LoopSource::ForLoop) {
          self.exit_frames.push(ExitFrame::Loop { id: expr.hir_id, len: entry_len, head_from: entry_len });
        }
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
                  bindings = self.register_iflet_let_bindings(let_expr, None).0;
                } else if is_let_chain(cond) {
                  // `while let A = x && cond` (#198): register each
                  // `let`'s bindings like a lone while-let's. The loop
                  // is drawn inline (no Branch), so the chain's events
                  // are shown unconditionally; nothing to refuse. Their
                  // scope ends are emitted after the body, below.
                  let lo = *self.unique_id;
                  self.visit_if_let_chain(cond, &HashSet::new(), &mut bindings, &mut ChainWalk::default());
                  let binds = self.chain_binding_events(&(lo..*self.unique_id), &bindings);
                  let cols = self.chain_binding_columns(&binds, &bindings);
                  self.binding_columns.extend(cols);
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
                // Not where every path breaks or continues first (#631).
                let decls = &decls - &Self::left_at_exits(self.ends_at_exits(then_body, false), &decls);
                self.emit_scope_end(decls, body_end);
                // A `while` also ends when its condition is false.
                self.emit_loop_exit_moves(expr, None, &before, true);
              } else {
                self.inside_branch = true;
                self.visit_block(block);
                self.inside_branch = prev;
              }
            } else {
              // Defensive fallback if a future rustc tweaks the desugar.
              self.inside_branch = true;
              self.visit_block(block);
              self.inside_branch = prev;
            }
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
            // Not where every path breaks or continues first (#631).
            let decls = &decls - &Self::left_at_exits(self.block_ends_at_exits(block, false), &decls);
            self.emit_scope_end(decls, body_end);
            // A bare `loop` ends only through a `break`.
            self.emit_loop_exit_moves(expr, None, &before, false);
            self.pair_deferred_breaks(expr);
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
        if !matches!(loop_ty, rustc_hir::LoopSource::ForLoop) {
          self.exit_frames.pop();
          self.in_scope.truncate(entry_len);
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
            // An exit in an arm doesn't see the binding of any arm's
            // value to a `let` (#410): it happens after the match. The
            // earlier arms' own events are out of `preprocessed_events`
            // by then (`take_branch_events` below), except a lone
            // arm's, which has no siblings.
            let entry_len = self.in_scope.len();
            let match_lo = *self.unique_id;
            let hide = self.rhs_ids_of(arms.iter().map(|a| Some(a.body)));
            self.exit_frames.push(ExitFrame::Branch { len: entry_len, hide, binds: Vec::new(), binds_at: 0 });
            // `match (x, y) { … }` moves `x` and `y` into the tuple
            // whatever arm is taken (#327).
            let scrutinee_moves = self.tuple_scrutinee_moves(guard_expr);
            let into_tuple = self.tuple_scrutinee_target(guard_expr);
            let tuple_name = match &into_tuple {
              ResourceTy::Temporary { tuple: Some(t), .. } => t.clone(),
              _ => String::new(),
            };
            // The tuple lives to the end of the statement holding the
            // match, usually the match's own last line.
            let past_match = self.temporary_scope_end(guard_expr).is_some_and(|(_, l, _)| l > merge);
            for arm in arms.iter() {
              // An arm's pattern bindings are out of scope in the next.
              self.in_scope.truncate(entry_len);
              let arm_hash_lo = self.rap_hashes;
              let mut branch_e_data: Vec<(usize, ExternalEvent)> = Vec::new();
              let mut callback_events: Vec<(ResourceTy, ResourceTy, Evt)> = Vec::new();
              // Parents this arm's pattern moves out of.
              let mut arm_moved: Vec<ResourceTy> = Vec::new();

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
              let (arm_pat, or_conflicts) = self.or_arm_stand_in(arm.pat, &parents, &parents_ty, pat_line, typeck_res, &scrutinee_moves);
              // The OrAltMove dots of the names `or_conflicts` lists,
              // added after the moves into the scrutinee tuple below
              // (the tuple holds the values they may take).
              let mut or_events: Vec<ExternalEvent> = Vec::new();
              // Where the possible borrows of an or-pattern's reference
              // end (#565); added with the arm's other end events.
              let mut or_end_events: Vec<(usize, ExternalEvent)> = Vec::new();
              match arm_pat.kind {
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
                    self.pat_parts(arm_pat, &parents[0], parents_ty[0])
                  } else {
                    None
                  };
                  // Borrows the bindings take from what the scrutinee
                  // borrows (#391) follow all the bindings, as in
                  // `register_iflet_let_bindings`.
                  let mut pending_borrows: Vec<ExternalEvent> = Vec::new();
                  for (i, p) in pat_list.iter().enumerate() {
                    let parent_idx = if same_arity { i } else { 0 };
                    let (parent, parent_ty) = match part_parents.as_ref().and_then(|v| v[i].1.clone()) {
                      Some(pp) => pp,
                      None => (parents[parent_idx].clone(), parents_ty[parent_idx]),
                    };
                    let mut associated_ro = Vec::new();
                    self.get_dec_of_pat_parts(p, &typeck_res, &parent, parent_ty, end, & mut associated_ro);
                    // A `ref` binding out of `(x, y)` borrows the tuple's
                    // element, not x (#580).
                    for (r, e, from, _) in associated_ro.iter_mut() {
                      *from = self.retarget_tuple_borrow(guard_expr, r, e, from.clone());
                    }
                    let or_evs = self.or_conflict_events(&or_conflicts, &mut associated_ro, pat_line, &scrutinee_moves, &tuple_name, end);
                    or_events.extend(or_evs.start);
                    or_end_events.extend(or_evs.end);
                    pat_decls.extend(or_evs.decls);
                    let temp: Vec<ResourceAccessPoint> = associated_ro.iter().map(|(r, _, _, _)| {r.clone()}).collect();
                    let temp2: HashSet<ResourceAccessPoint> = temp.into_iter().collect();
                    pat_decls.extend(temp2);
                    for (to_ro, e, from, partial) in associated_ro.iter() {
                      if matches!(e, Evt::Move) {
                        arm_moved.push(from.clone());
                      }
                      // If the type of the pat is not the same as the parent associated with it then it must be a partial move
                      let is_partial = partial.unwrap_or(!(parent_ty == typeck_res.node_type(p.hir_id)));
                      let id = *self.unique_id;
                      *self.unique_id += 1;
                      let ext_ev = self.ext_ev_of_evt(e.clone(), ResourceTy::Value(to_ro.clone()), from.clone(), id, is_partial);
                      // A reference copied out of a value that holds a
                      // borrow also borrows what that value borrows (#391).
                      let scrutinee = match guard_expr.kind {
                        ExprKind::Tup(fields) if same_arity => &fields[parent_idx],
                        _ => guard_expr,
                      };
                      pending_borrows.extend(self.pattern_ref_copy_borrow(to_ro, e, scrutinee));
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
                  for ev in pending_borrows {
                    if inline {
                      self.add_external_event(pat_line, ev);
                    } else {
                      branch_e_data.push((pat_line, ev));
                    }
                  }
                }
                // <expr> => <expr> (just a singleton variable)
                _ => {
                  let mut pending_borrows: Vec<ExternalEvent> = Vec::new();
                  for i in 0..parents.len() {
                    let mut associated_ro = Vec::new();
                    // `P { a, b } => …` on a struct variable: `a` / `b`
                    // come from `p.a` / `p.b` (#326).
                    self.get_dec_of_pat_parts(arm_pat, &typeck_res, &parents[i], parents_ty[i], end, & mut associated_ro);
                    let or_evs = self.or_conflict_events(&or_conflicts, &mut associated_ro, pat_line, &scrutinee_moves, &tuple_name, end);
                    or_events.extend(or_evs.start);
                    or_end_events.extend(or_evs.end);
                    pat_decls.extend(or_evs.decls);
                    let temp: Vec<ResourceAccessPoint> = associated_ro.iter().map(|(r, _, _, _)| {r.clone()}).collect();
                    let temp2: HashSet<ResourceAccessPoint> = temp.into_iter().collect();
                    pat_decls.extend(temp2);
                    for (to_ro, e, from, partial) in associated_ro.iter() {
                      if matches!(e, Evt::Move) {
                        arm_moved.push(from.clone());
                      }
                      let id = *self.unique_id;
                      *self.unique_id += 1;
                      let ext_ev = self.ext_ev_of_evt(e.clone(), ResourceTy::Value(to_ro.clone()), from.clone(), id, partial.unwrap_or(false));
                      let scrutinee = match guard_expr.kind {
                        ExprKind::Tup(fields) if fields.len() == parents.len() => &fields[i],
                        _ => guard_expr,
                      };
                      pending_borrows.extend(self.pattern_ref_copy_borrow(to_ro, e, scrutinee));
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
                  for ev in pending_borrows {
                    if inline {
                      self.add_external_event(pat_line, ev);
                    } else {
                      branch_e_data.push((pat_line, ev));
                    }
                  }
                }
              }

              // A variable the scrutinee tuple took but this arm's
              // pattern doesn't move out (`_ => …`, `(Some(_), b)`): it
              // was still moved into the tuple, and isn't usable after
              // the match (#327).
              for r in scrutinee_moves.iter() {
                if arm_moved.contains(&ResourceTy::Value(r.clone())) {
                  continue;
                }
                let id = *self.unique_id;
                *self.unique_id += 1;
                let ext_ev = self.ext_ev_of_evt(Evt::Move, into_tuple.clone(), ResourceTy::Value(r.clone()), id, false);
                if inline {
                  self.add_external_event(pat_line, ext_ev);
                } else {
                  branch_e_data.push((pat_line, ext_ev));
                }
              }
              for ev in or_events {
                if inline {
                  self.add_external_event(pat_line, ev);
                } else {
                  branch_e_data.push((pat_line, ev));
                }
              }
              // An arm that leaves early (`_ => return 0`) drops the tuple
              // at its exit instead (#327): see drop_locals_on_exit.
              let arm_diverges = self.expr_diverges(arm.body);

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
              // While the body is visited, the pattern's and the guard's
              // events are on the global list too, so that an exit in it
              // sees the bindings own their values and drops them
              // (#630). They're taken off again after the body.
              let pat_ids: HashSet<usize> = branch_e_data.iter().map(|(_, e)| e.get_id()).collect();
              self.preprocessed_events.extend(branch_e_data.iter().cloned());

              // visit expr. inside_branch is restored, not reset: the
              // match may sit inside another branch (#410).
              let prev_inside_branch = self.inside_branch;
              self.inside_branch = true;
              self.uninit_locals = uninit_before.clone();
              let body_lo = *self.unique_id;
              // An exit in the arm drops the tuple scrutinee (#327).
              let live = !scrutinee_moves.is_empty();
              if live {
                self.live_tuples.push(LiveTuple {
                  frame: self.exit_frames.len() - 1,
                  len: entry_len,
                  scrutinee: guard_expr,
                  tuple: tuple_name.clone(),
                  pat: arm.pat,
                  at: TupleDropAt::MatchArmExit,
                });
              }
              // A `?` in the body sees the pattern's bindings (#636).
              let binds_at = self.preprocessed_events.len();
              if let Some(ExitFrame::Branch { binds: b, binds_at: at, .. }) = self.exit_frames.last_mut() {
                b.clone_from(&branch_e_data);
                *at = binds_at;
              }
              self.visit_expr(arm.body);
              if live {
                self.live_tuples.pop();
              }
              if let Some(ExitFrame::Branch { binds: b, .. }) = self.exit_frames.last_mut() {
                b.clear();
              }
              let body_ids = body_lo..*self.unique_id;
              uninit_ends.push((Some(arm.body), std::mem::take(&mut self.uninit_locals)));
              self.inside_branch = prev_inside_branch;

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
              self.preprocessed_events.retain(|(_, e)| !pat_ids.contains(&e.get_id()));
              if !inline {
                let body_ev = self.take_branch_events(body_ids, Some(arm.body), &HashSet::new());
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

              // The end of an or-pattern reference's possible borrows
              // (#565), where the borrow checker ends them: at the
              // reference's last use, which can be before the arm ends.
              for (line, ev) in or_end_events {
                // Its borrow is drawn by these events, not `print_lifetimes`.
                if let ExternalEvent::OrAltBorrow { into, .. } = &ev {
                  self.borrow_map.remove(into.name());
                }
                if inline {
                  self.add_external_event(line, ev);
                } else {
                  let at = branch_e_data.iter().rposition(|(l, _)| *l <= line).map_or(0, |i| i + 1);
                  branch_e_data.insert(at, (line, ev));
                }
              }

              // add callback events - events that need to happen at the end of an arm block
              // ex: if a pattern binding borrows from a parent
              let pat_end = span_end_line(&arm.pat.span, &self.tcx);
              let mir_b_data = if callback_events.is_empty() { Vec::new() } else { self.gather_borrow_data(self.bwf) };
              for (to, from, e) in callback_events {
                let name = from.real_name();
                self.borrow_map.remove(&name);
                // The borrow ends where the borrow checker ends it, at
                // the binding's last use (#599): the end of the region
                // of the loan MIR issues for the binding on the pattern's
                // lines. A binding that reborrows through a shared
                // reference (`(x, _)` on `o: &(String, String)`) has no
                // loan of its own; its borrow lasts as long as it is
                // live.
                let ends_at = mir_b_data.iter()
                  .filter(|m| m.assigned_place.as_deref() == Some(name.as_str()) && (pat_line..=pat_end).contains(&m.region.0))
                  .map(|m| m.region.1)
                  .max()
                  .or_else(|| self.last_live_line(&name, (pat_line, end)))
                  .filter(|l| (pat_line..=end).contains(l))
                  .unwrap_or(end);
                // When the scrutinee is itself a reference (`match o`,
                // `o: &Option<String>`), the binding reborrows through
                // it, so `o`'s borrow lasts at least until here (#372).
                if to.is_ref() {
                  let floor = self.borrow_floor.entry(to.real_name()).or_insert(ends_at);
                  *floor = (*floor).max(ends_at);
                }
                let id = *self.unique_id;
                *self.unique_id += 1;
                let ext_ev = self.ext_ev_of_evt(e, to, from, id, false);
                if inline {
                  self.add_external_event(ends_at, ext_ev);
                } else {
                  // After the arm's events up to that line.
                  let at = branch_e_data.iter().rposition(|(l, _)| *l <= ends_at).map_or(0, |i| i + 1);
                  branch_e_data.insert(at, (ends_at, ext_ev));
                }
              }

              // add gos events, except in an arm that always leaves at
              // an exit that drops them (#630)
              let arm_left = Self::left_at_exits(!inline && self.ends_at_exits(arm.body, true), &arm_decls);
              for r in in_decl_order(&arm_decls).into_iter().filter(|v| needs_scope_end(v) && !arm_scoped.contains(v) && !arm_left.contains(v)) {
                let id = *self.unique_id;
                *self.unique_id += 1;
                let gos = ExternalEvent::GoOutOfScope { ro: r.clone(), id };
                if inline {
                  self.add_external_event(end, gos);
                } else {
                  branch_e_data.push((end, gos));
                }
              }

              // The tuple is dropped after the arm, and with it what of
              // each variable's value the pattern didn't move out
              // (#327). It has no column: a drop dot on each such
              // variable's says so. (An arm that leaves early drops it
              // at its `return` / `break` / `continue` / panic.)
              if !arm_diverges {
                let (at, line) = if past_match {
                  (TupleDropAt::MatchArmStmt, end)
                } else if inline {
                  (TupleDropAt::MatchHere, merge)
                } else {
                  (TupleDropAt::MatchArm, end)
                };
                for ev in self.tuple_drop_events(guard_expr, &tuple_name, Some(arm.pat), at) {
                  if inline {
                    self.add_external_event(line, ev);
                  } else {
                    branch_e_data.push((line, ev));
                  }
                }
              }

              arm_decls.extend(arm_inner);
              arm_decls.extend(arm_scoped);
              if !inline {
                let branch_line_map = create_line_map(&branch_e_data);
                // An arm of type `!` (`_ => unreachable!()`) never
                // reaches the merge (#296).
                let (diverges, exits, break_carries) = self.branch_divergence(arm.body, &branch_e_data, &arm_decls);
                branch_data.push(ExtBranchData { e_data: branch_e_data, line_map: branch_line_map, decl_vars: arm_decls, diverges, exits, break_carries });
              }
            }
            self.join_uninit_locals(uninit_ends);
            self.exit_frames.pop();
            self.in_scope.truncate(entry_len);
            // Single-arm: events already routed through
            // add_external_event during the loop, nothing else
            // to do — bail out of the Normal arm before the
            // multi-arm Branch construction below. An exit in the
            // lone arm isn't drawn: the arm isn't a column of its
            // own (#410).
            if inline {
                self.discard_exit_drops(match_lo..*self.unique_id);
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
            self.note_exit_scope(expr);
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
                let head_from = self.in_scope.len();
                self.note_for_head_temporary(head_arg, guard_expr, expr);
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

                // The loop owns its iterator and drops it when the loop
                // ends: a head that is moved in (`for s in v`, `for s in
                // it`) whose iterator has drop glue (`vec::IntoIter`)
                // says where it goes on the move (#482). `slice::Iter`
                // and the like have nothing to drop. The iterator built
                // from a collection (`v`) is a `(temporary N)` column
                // (`note_for_head_temporary`); an iterator variable
                // (`it`) is the loop's iterator itself, so its drop is
                // on its own column (`iterator_head`).
                let iterator_needs_drop = matches!(iterand_evt, Some(Evt::Move))
                  && matches!(head_arg.kind, ExprKind::Path(_))
                  && self.tcx.typeck(guard_expr.hir_id.owner)
                    .node_type(guard_expr.hir_id)
                    .needs_drop(self.tcx, rustc_middle::ty::TypingEnv::post_analysis(self.tcx, guard_expr.hir_id.owner));

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
                // A `break` / `continue` names the inner `loop` as its
                // target, and drops the loop variable too (#410).
                let entry_len = self.in_scope.len();
                if let Some(outer_arm) = arms.first() {
                  self.exit_frames.push(ExitFrame::Loop { id: outer_arm.body.hir_id, len: entry_len, head_from });
                }
                // The variable moved into the loop as its iterator, whose
                // drop is stated on its column (#482).
                let mut iterator_head: Option<ResourceAccessPoint> = None;
                if let PatKind::Binding(_, pat_hirid, pat_ident, None) = user_pat.kind {
                  let pat_name = binding_name(&self.tcx, pat_hirid);
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
                    if iterator_needs_drop {
                      iterator_head = head_rty.extract_rap().cloned();
                      self.add_loop_head_move_ev(for_line, ResourceTy::Anonymous, head_rty, LoopHead::Iterator);
                    } else {
                      self.add_ev(for_line, Evt::Move, ResourceTy::Anonymous, head_rty, false);
                    }
                    self.add_ev(for_line, Evt::Bind, to_rty, ResourceTy::Anonymous, false);
                  } else if let Some(e) = iterand_evt {
                    if iterator_needs_drop {
                      self.add_loop_head_move_ev(for_line, to_rty, head_rty, LoopHead::Collection);
                    } else {
                      self.add_ev(for_line, e, to_rty, head_rty, false);
                    }
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
                  //
                  // A head that is a variable moved into the loop
                  // (`for (n, s) in pairs`) still moves it: draw that on
                  // the head, into no particular binding, as the single
                  // binding does (#527).
                  if matches!(iterand_evt, Some(Evt::Move)) {
                    if iterator_needs_drop {
                      if head_is_iterator {
                        iterator_head = head_rty.extract_rap().cloned();
                        self.add_loop_head_move_ev(for_line, ResourceTy::Anonymous, head_rty.clone(), LoopHead::Iterator);
                      } else {
                        self.add_loop_head_move_ev(for_line, ResourceTy::Anonymous, head_rty.clone(), LoopHead::Collection);
                      }
                    } else {
                      self.add_ev(for_line, Evt::Move, ResourceTy::Anonymous, head_rty.clone(), false);
                    }
                  }
                  // `for (a, b) in &pairs` / `&mut pairs`: each reference
                  // binding borrows the head (#576).
                  if matches!(iterand_evt, Some(Evt::SBorrow) | Some(Evt::MBorrow)) {
                    self.pattern_lender = iterand_evt.clone().map(|e| (e, head_rty.clone()));
                  }
                  self.register_pattern_bindings_anon(user_pat, for_line);
                  self.pattern_lender = None;
                }

                // Visit body — events flow into the global timeline.
                let before = self.rap_names();
                if let Some(ro) = &iterator_head {
                  if let Some(loop_expr) = arms.first().map(|a| a.body) {
                    self.loop_iterators.push((loop_expr.hir_id, ro.clone()));
                  }
                }
                self.visit_expr(body_expr);
                if iterator_head.is_some() {
                  self.loop_iterators.pop();
                }
                self.inside_branch = prev_inside_branch;

                // The pattern bindings are fresh each iteration and,
                // like the body's own `let`s, go out of scope at the
                // body's closing brace (#205, #256). They're registered
                // with is_global = false, so print_out_of_scope won't
                // emit this for them.
                let body_end = self.tcx.sess.source_map()
                  .lookup_char_pos(body_expr.span.hi()).line;
                // `for x in it`: the loop owns the iterator `it` holds a
                // borrow in, until the loop ends, so the lender is
                // borrowed through the whole body and its borrow ends
                // where the loop does, not at the last use of `x`
                // (#722). (A `break` that leaves the loop early ends it
                // in its own branch.)
                if head_is_iterator && arms.first().is_some_and(|a| breaks_out_of(a.body) <= 1) {
                  if let Some(name) = expr_to_rap_name(head_arg, &self.tcx) {
                    if self.borrow_map.contains_key(&name) {
                      let floor = self.borrow_floor.entry(name).or_insert(body_end);
                      *floor = (*floor).max(body_end);
                    }
                  }
                }
                let mut loop_decls = get_decl_of_expr(body_expr, &self.tcx, &self.raps);
                collect_pat_bindings(user_pat, &self.tcx, &self.raps, &mut loop_decls);
                // Not where every path breaks or continues first (#631).
                let loop_decls = &loop_decls - &Self::left_at_exits(self.ends_at_exits(body_expr, false), &loop_decls);
                self.emit_scope_end(loop_decls, body_end);
                // `break`s in the body target the desugared `loop`
                // (the outer arm's body), which also ends when the
                // iterator runs out.
                if let Some(loop_expr) = arms.first().map(|a| a.body) {
                  self.emit_loop_exit_moves(loop_expr, None, &before, true);
                }
                if arms.first().is_some() {
                  self.exit_frames.pop();
                }
                self.in_scope.truncate(entry_len);
                // The loop drops its iterator where it ends, however it
                // ends (running out, a `break`) (#482).
                if let Some(ro) = iterator_head {
                  self.add_loop_iterator_drop(ro, span_end_line(&expr.span, &self.tcx), LoopLeft::End);
                }
              }
            }
          }
          // `e?`: the `Err` / `None` arm returns from the function
          // (#430). The scrutinee `Try::branch(e)` was visited above.
          rustc_hir::MatchSource::TryDesugar(_) => self.note_try(expr),
          _ => {}
        }
      }
      // `break` / `continue` leave the loop's iteration, dropping
      // what it declared (#410); a `break` in a labelled block
      // (no loop) drops nothing drawn here.
      ExprKind::Break(dest, value) => {
        intravisit::walk_expr(self, expr);
        // `let r = loop { break k; }`: `r` gets `k` (#431).
        if let (Ok(target), Some(value)) = (dest.target_id, value) {
          let has_receiver = self.break_pats.iter().any(|(id, ..)| *id == target)
            || self.break_binds.iter().any(|(id, ..)| *id == target);
          let several = self.loop_break_values(target) > 1;
          // The `break` as written, for the tooltips of a loop with
          // several (see BreakAlt).
          let text = if several { self.break_text(expr) } else { None };
          if (has_receiver && several) || (!several && self.break_is_conditional(target)) {
            // In a branch of the loop (`if c { break s; }`): the value
            // moves out of the loop with the `break`, so it reaches
            // `r` where the loop ends. Inside the loop `s` is still
            // owned where the branches join (#350). Also when the
            // receiver is known only after the loop (`r = loop`, an
            // argument): see `deferred_lines`. With several `break`s
            // that carry a value, each reaches `r` there, whichever of
            // them ended the loop (#561); with no receiver yet,
            // `match_rhs` / `match_arg` pair them at the loop's end.
            self.deferred_breaks.push((target, value, text));
          } else {
            self.pair_break_value(target, value, text);
          }
        }
        if let Ok(target) = dest.target_id {
          self.drop_locals_on_exit(expr_to_line(expr, &self.tcx), value, Some(target), true, "break");
          self.drop_inner_loop_iterators(expr_to_line(expr, &self.tcx), target, true, LoopLeft::Break);
        }
      }
      ExprKind::Continue(dest) => {
        if let Ok(target) = dest.target_id {
          self.drop_locals_on_exit(expr_to_line(expr, &self.tcx), None, Some(target), false, "continue");
        }
      }
      // `P { .. }`, `(a, b)`, `[a, b]` written by the user: a value
      // that nothing may take is a temporary too (#469).
      ExprKind::Struct(..) | ExprKind::Tup(_) | ExprKind::Array(_) => {
        intravisit::walk_expr(self, expr);
        if let Some(name) = literal_temporary_name(expr) {
          self.note_literal_temporary_drop(expr, name);
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

/// How a literal is named on the column of the value it builds:
/// `P { .. }`, `(..)`, `[..]`.
fn literal_temporary_name(expr: &Expr) -> Option<String> {
  match expr.kind {
    ExprKind::Struct(qpath, ..) => {
      let name = match qpath {
        QPath::Resolved(_, p) => p.segments.iter().map(|s| s.ident.as_str()).collect::<Vec<_>>().join("::"),
        QPath::TypeRelative(_, seg) => seg.ident.as_str().to_owned(),
        _ => return None,
      };
      Some(format!("{} {{ .. }}", name))
    }
    ExprKind::Tup(_) => Some("(..)".to_owned()),
    ExprKind::Array(_) => Some("[..]".to_owned()),
    _ => None,
  }
}

/// `x`, `p.f`, `v[i]`, `*r`: an expression that names a place.
fn is_place_expr(e: &Expr) -> bool {
  match e.kind {
    ExprKind::Path(QPath::Resolved(_, p)) => matches!(p.res, Res::Local(_)),
    ExprKind::Field(base, _) | ExprKind::Index(base, _, _) | ExprKind::Unary(UnOp::Deref, base) => is_place_expr(base),
    _ => false,
  }
}

impl<'a, 'tcx> ExprVisitor<'a, 'tcx> {
  /// The std macro whose result `expr` is, when the user wrote the macro
  /// call (not a macro inside another macro's expansion): `vec![..]`,
  /// `format!(..)`. Their expansion isn't a `Call` the visitor reaches,
  /// so `note_macro_temporary_drop` handles them. (`visit_expr` only
  /// sees the root of an expansion; `descend_through_expansion` walks
  /// the rest.)
  fn std_value_macro(&self, expr: &'tcx Expr<'tcx>) -> Option<&'static str> {
    let data = expr.span.ctxt().outer_expn_data();
    if data.call_site.from_expansion() || !data.macro_def_id.is_some_and(|d| !d.is_local()) {
      return None;
    }
    match bang_macro_name(&data.kind).as_deref() {
      Some("vec") => Some("vec!"),
      Some("format") => Some("format!"),
      _ => None,
    }
  }

  /// The columns that building the literal `e` moves a value that
  /// needs drop out of (`P { name: s, .. }`, `vec![s]`, `(t, 1)`), with
  /// the line of each move, nested literals included. A struct update
  /// (`P { n: 1, ..p }`) moves p's fields that the literal doesn't
  /// name (#574); None when those have no columns of their own.
  fn literal_moved_places(&self, e: &'tcx Expr<'tcx>) -> Option<Vec<(ResourceAccessPoint, usize)>> {
    let typeck = self.tcx.typeck(e.hir_id.owner);
    let env = rustc_middle::ty::TypingEnv::post_analysis(self.tcx, e.hir_id.owner);
    let mut places = Vec::new();
    let elems: Vec<&'tcx Expr<'tcx>> = match e.kind {
      ExprKind::Struct(_, fields, tail) => {
        if let rustc_hir::StructTailExpr::Base(b) = tail {
          if is_place_expr(b) {
            places.extend(self.struct_update_moves(e, fields, b, typeck, env)?);
          }
        }
        fields.iter().map(|f| f.expr).collect()
      }
      ExprKind::Tup(items) | ExprKind::Array(items) => items.iter().collect(),
      ExprKind::Repeat(x, _) => vec![x],
      _ => match vec_macro_elems(e) {
        Some(items) => items.iter().collect(),
        // `vec![]`, a `format!`: nothing is moved in.
        None => return Some(Vec::new()),
      },
    };
    for x in elems {
      if is_place_expr(x) {
        if typeck.expr_ty(x).needs_drop(self.tcx, env) {
          if let Some(src) = fetch_rap(x, &self.tcx, &self.raps) {
            places.push((src, expr_to_line(x, &self.tcx)));
          }
        }
      } else if matches!(x.kind, ExprKind::Struct(..) | ExprKind::Tup(_) | ExprKind::Array(_)) {
        places.extend(self.literal_moved_places(x)?);
      }
    }
    Some(places)
  }

  /// `P { n: 1, ..p }`: the columns of p's fields that need drop and
  /// that the literal doesn't name, which it moves out of p (#574).
  /// None when one of them has no column.
  fn struct_update_moves(
    &self,
    lit: &'tcx Expr<'tcx>,
    fields: &'tcx [rustc_hir::ExprField<'tcx>],
    base: &'tcx Expr<'tcx>,
    typeck: &'tcx rustc_middle::ty::TypeckResults<'tcx>,
    env: rustc_middle::ty::TypingEnv<'tcx>,
  ) -> Option<Vec<(ResourceAccessPoint, usize)>> {
    let rustc_middle::ty::TyKind::Adt(def, args) = typeck.expr_ty(lit).kind() else { return None };
    if !def.is_struct() {
      return None;
    }
    let base_name = expr_to_rap_name(base, &self.tcx)?;
    let line = expr_to_line(base, &self.tcx);
    let mut moves = Vec::new();
    for f in def.non_enum_variant().fields.iter() {
      if fields.iter().any(|ef| ef.ident.name == f.name) || !f.ty(self.tcx, args).needs_drop(self.tcx, env) {
        continue;
      }
      let col = self.raps.get(&format!("{}.{}", base_name, f.name))?;
      moves.push((col.rap.to_owned(), line));
    }
    Some(moves)
  }

  /// `vec![String::new()];`, `format!(..).len()`: a std macro's result
  /// is a temporary like a call's (#469). `name` is the column of the
  /// macro, like a fn's.
  fn note_macro_temporary_drop(&mut self, expr: &'tcx Expr<'tcx>, name: &str) {
    self.note_container_temporary_drop(expr, name.to_owned());
  }

  /// `P { .. };`, `let _ = (make(), 1);`, `[a(), b()].len()`: a literal
  /// that nothing takes is a temporary like a call's result (#469).
  fn note_literal_temporary_drop(&mut self, expr: &'tcx Expr<'tcx>, name: String) {
    if self.is_drawn_tuple_scrutinee(expr) {
      return;
    }
    self.note_container_temporary_drop(expr, name);
  }

  /// `match (x, y) { .. }`, `if let P = (x, y) { .. }`: a tuple of
  /// variables that a match or `if let` scrutinizes already has its
  /// moves and its drop drawn as "the temporary tuple (x, y)" (#327),
  /// so it gets no `(temporary)` column as well.
  fn is_drawn_tuple_scrutinee(&self, expr: &'tcx Expr<'tcx>) -> bool {
    use rustc_hir::Node;
    if !matches!(expr.kind, ExprKind::Tup(_)) || self.tuple_scrutinee_moves(expr).is_empty() {
      return false;
    }
    match self.tcx.parent_hir_node(expr.hir_id) {
      Node::Expr(m) => match m.kind {
        ExprKind::Match(s, _, rustc_hir::MatchSource::Normal) => s.hir_id == expr.hir_id,
        ExprKind::Let(l) if l.init.hir_id == expr.hir_id => matches!(self.tcx.parent_hir_node(m.hir_id),
          Node::Expr(i) if matches!(i.kind, ExprKind::If(c, ..) if c.hir_id == m.hir_id)),
        _ => false,
      },
      _ => false,
    }
  }

  /// A macro's or literal's temporary. A variable moved into it
  /// (`vec![s];`, `let _ = P { name: t, n: 1 };`) is drawn moving into
  /// its column, so it stops owning the value (#570).
  fn note_container_temporary_drop(&mut self, expr: &'tcx Expr<'tcx>, name: String) {
    let Some(places) = self.literal_moved_places(expr) else { return };
    self.note_temporary_drop(expr, name, places);
  }

  /// A call whose result is a temporary that Rust drops at the end of
  /// its statement (#457): `make();`, `let _ = make();`, `make().len()`,
  /// `f(&make())`. The temporary has no column, so say so on the
  /// producing fn's column with a drop dot. Only when the result's type
  /// needs drop, and only for the shapes where the value certainly isn't
  /// moved on: anything else (a field moved out of it, a `match` that
  /// binds part of it by value, a borrow a `let` extends) says nothing.
  fn note_temporary_drop(&mut self, expr: &'tcx Expr<'tcx>, fn_name: String, into: Vec<(ResourceAccessPoint, usize)>) -> Option<ResourceAccessPoint> {
    if self.temp_drops_noted.contains(&expr.hir_id) {
      return None;
    }
    let typeck = self.tcx.typeck(expr.hir_id.owner);
    let ty = typeck.expr_ty(expr);
    let env = rustc_middle::ty::TypingEnv::post_analysis(self.tcx, expr.hir_id.owner);
    if !ty.needs_drop(self.tcx, env) || !self.is_unmoved_temporary(expr, typeck, env) {
      return None;
    }
    let (when, end_line, owner) = self.temporary_scope_end(expr)?;
    self.record_temporary_drop(expr, strip_module_paths(&ty.to_string()), fn_name, when, end_line, owner, into)
  }

  /// The `(temporary N)` column of a value nothing takes: it acquires
  /// `ty`'s value from `fn_name` where `expr` is, and is dropped by a
  /// `TemporaryDrop` at `end_line`.
  fn record_temporary_drop(
    &mut self,
    expr: &'tcx Expr<'tcx>,
    ty: String,
    fn_name: String,
    when: &str,
    end_line: usize,
    owner: Option<HirId>,
    into: Vec<(ResourceAccessPoint, usize)>,
  ) -> Option<ResourceAccessPoint> {
    // Branch events are sliced out by line, so a one-line `match` / `if`
    // can't hold a column of its own (the same limit as #354 / #459).
    if self.inside_one_line_branch(expr) {
      return None;
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
    self.add_owner(tmp_name.clone(), false, false, false, InteriorMut::No, end_line, !self.inside_branch);
    if let Some(ResourceAccessPoint::Owner(o)) = self.raps.get_mut(&tmp_name).map(|rd| &mut rd.rap) {
      o.unbound_temp = true;
    }
    self.raps.get_mut(&tmp_name).unwrap().temp_owner = owner;
    let tmp = self.raps.get(&tmp_name).unwrap().rap.to_owned();
    let line = expr_to_line(expr, &self.tcx);
    self.add_ev(line, Evt::Move, ResourceTy::Value(tmp.clone()), ResourceTy::Value(fn_rap), false);
    // Variables moved into the temporary (`vec![s];`, #570; the fields
    // `P { n: 1, ..p }` takes from p, #574).
    for (src, src_line) in into {
      self.update_rap(&src, line);
      self.add_ev(src_line, Evt::Move, ResourceTy::Value(tmp.clone()), ResourceTy::Value(src), false);
    }
    // Plain text: `hover_messages::fmt_style` escapes it for the tooltip.
    let id = *self.unique_id;
    self.add_external_event(end_line, ExternalEvent::TemporaryDrop { ro: tmp.clone(), ty: Some(ty), when: when.to_owned(), literal: false, early: false, id });
    *self.unique_id += 1;
    Some(tmp)
  }

  /// `for x in make_vec()`, `for x in v`, `for x in vec![..]`: the head
  /// is moved into the iterator `IntoIterator::into_iter` makes, which
  /// the loop owns and drops when it ends; what the loop didn't take out
  /// of it is dropped with it (#469). That iterator has no column, so
  /// say so on one like a call's result. Nothing is stated for a head
  /// that is a reference or whose iterator owns nothing (`&v`, `0..n`,
  /// `v.iter()`, `s.chars()`).
  fn note_for_head_temporary(&mut self, head: &'tcx Expr<'tcx>, iter_call: &'tcx Expr<'tcx>, for_expr: &'tcx Expr<'tcx>) {
    let typeck = self.tcx.typeck(iter_call.hir_id.owner);
    let ty = typeck.expr_ty(iter_call);
    let env = rustc_middle::ty::TypingEnv::post_analysis(self.tcx, iter_call.hir_id.owner);
    if ty.is_ref() || !ty.needs_drop(self.tcx, env) || self.temp_drops_noted.contains(&head.hir_id) {
      return;
    }
    // `for s in it` with `it` an iterator already: `into_iter` hands
    // `it`'s value back unchanged, so no new value is built. It's moved
    // into the loop, and its drop is stated on its own column (#482).
    if matches!(head.kind, ExprKind::Path(QPath::Resolved(_, p)) if matches!(p.res, Res::Local(_)))
      && typeck.expr_ty(head) == ty
    {
      return;
    }
    let end_line = span_end_line(&for_expr.span, &self.tcx);
    // An adapter chain's type (`Map<IntoIter<String>, {closure@..}>`)
    // isn't something a student wrote.
    let shown = strip_module_paths(&ty.to_string());
    let shown = if shown.contains('{') { "iterator".to_owned() } else { shown };
    self.record_temporary_drop(head, shown, "into_iter".to_owned(), "when the `for` loop ends", end_line, None, Vec::new());
  }

  /// True iff `expr` sits inside an `if` / `match` that starts and ends
  /// on one line.
  fn inside_one_line_branch(&self, expr: &'tcx Expr<'tcx>) -> bool {
    let sm = self.tcx.sess.source_map();
    let mut child = expr.hir_id;
    self.tcx.hir_parent_iter(expr.hir_id).any(|(id, node)| {
      let from = std::mem::replace(&mut child, id);
      match node {
        // The scrutinee / condition is evaluated before the branches
        // split, so it isn't inside them (#469).
        rustc_hir::Node::Expr(e) if matches!(e.kind, ExprKind::Match(s, ..) | ExprKind::If(s, ..) if s.hir_id == from) => false,
        rustc_hir::Node::Expr(e) if matches!(e.kind, ExprKind::If(..) | ExprKind::Match(..)) && !e.span.from_expansion() => {
          sm.lookup_char_pos(e.span.lo()).line == sm.lookup_char_pos(e.span.hi()).line
        }
        _ => false,
      }
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
        // `adder(t)(1)` calls the closure `adder(t)` returned (#703):
        // through a reference for `Fn` / `FnMut`, so it is dropped at the
        // end of the statement; an `FnOnce` call consumes it.
        ExprKind::Call(callee, _) if is_me(callee) => {
          !typeck.type_dependent_def_id(p.hir_id)
            .is_some_and(|m| self.tcx.trait_of_assoc(m) == self.tcx.lang_items().fn_once_trait())
        }
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
        // `if let` / `while let`: the same, for the one pattern (#469).
        ExprKind::Let(l) if is_me(l.init) => {
          let mut moves_out = false;
          l.pat.each_binding(|ann, hid, _, _| {
            if ann.0 == rustc_hir::ByRef::No && typeck.node_type(hid).needs_drop(self.tcx, env) {
              moves_out = true;
            }
          });
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

  /// One TupleTempDrop per variable of the tuple-literal `scrutinee`
  /// whose value (or part of it) is still in the tuple after `pat`
  /// matched (`None`: it didn't), each listing all of them (#327).
  fn tuple_drop_events(&mut self, scrutinee: &'tcx Expr<'tcx>, tuple: &str, pat: Option<&'tcx rustc_hir::Pat<'tcx>>, at: TupleDropAt) -> Vec<ExternalEvent> {
    let left = self.tuple_scrutinee_left(scrutinee, pat);
    let with: Vec<TupleLeft> = left.iter().map(|(_, l)| l.clone()).collect();
    left.into_iter().map(|(ro, _)| {
      let id = *self.unique_id;
      *self.unique_id += 1;
      ExternalEvent::TupleTempDrop { ro, tuple: tuple.to_owned(), at, with: with.clone(), id }
    }).collect()
  }

  /// When the temporary of `expr` is dropped, in words, and the line
  /// that happens on: the end of the enclosing statement, the block's
  /// final expression, the match arm, the `if` / `while` condition, the
  /// `for` loop or the closure body.
  fn temporary_scope_end(&self, expr: &'tcx Expr<'tcx>) -> Option<(&'static str, usize, Option<HirId>)> {
    use rustc_hir::Node;
    // `a && let Some(_) = b && c`: an `&&` with a `let` somewhere in it.
    fn is_let_chain(e: &Expr) -> bool {
      match e.kind {
        ExprKind::Binary(op, l, r) if op.node == rustc_hir::BinOpKind::And => {
          [l, r].iter().any(|o| matches!(o.kind, ExprKind::Let(_)) || is_let_chain(o))
        }
        _ => false,
      }
    }
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
          // The scrutinee of an `if let` / `while let` (#469). Its
          // temporaries outlive the pattern match.
          ExprKind::If(c, then, _) if c.hir_id == cur && matches!(c.kind, ExprKind::Let(_)) => {
            if matches!(e.span.desugaring_kind(), Some(rustc_span::DesugaringKind::WhileLoop)) {
              return Some(("at the end of each iteration of this `while let` loop", end(e.span), None));
            }
            if expr.span.edition() >= rustc_span::edition::Edition::Edition2024 {
              // Edition 2024: dropped before the `else` block runs, or
              // when the matching block ends.
              return Some(("before the `else` block of this `if let` runs, or when the block it matched ends", end(then.span), None));
            }
            // Before edition 2024 they live to the end of the enclosing
            // statement, unless this is an `else if let`, whose whole
            // `else` is a scope of its own.
            if let Node::Expr(outer) = self.tcx.parent_hir_node(e.hir_id) {
              if matches!(outer.kind, ExprKind::If(_, _, Some(els)) if els.hir_id == e.hir_id) {
                return Some(("at the end of this `else if let`", end(e.span), None));
              }
            }
          }
          // A let chain (`if a && let Some(_) = make() { .. }`, edition
          // 2024): a `let`'s scrutinee lives like an `if let`'s, until the
          // block the chain matched ends or the `else` runs (#604).
          ExprKind::If(c, then, _) if c.hir_id == cur && is_let_chain(c) => {
            if matches!(e.span.desugaring_kind(), Some(rustc_span::DesugaringKind::WhileLoop)) {
              return Some(("at the end of each iteration of this `while let` loop", end(e.span), None));
            }
            return Some(("before the `else` block of this `if` runs, or when the block its let chain matched ends", end(then.span), None));
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
            // Except a `let` in a let chain, and the chain to its left:
            // its temporaries outlive the operand (the `If` arm above).
            let in_chain = matches!(operand.kind, ExprKind::Let(_))
              || matches!(operand.kind, ExprKind::Binary(o, ..)
                if matches!(o.node, rustc_hir::BinOpKind::And | rustc_hir::BinOpKind::Or));
            if !in_chain {
              return Some(("at the end of this `&&` / `||` operand", end(operand.span), None));
            }
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
    let mut ref_src: HashMap<HirId, (String, LoanId, bool)> = HashMap::new();
    for (_, place, hid) in &assigns {
      let ty = self.tcx.typeck(place.hir_id.owner).node_type(place.hir_id);
      if ty.is_ref() {
        if let Some(src) = src_of.get(hid) {
          ref_data.insert(*hid, self.get_ref_data_full(src));
          if let Some((n, sp)) = self.source_ref_loan(src) {
            let now = dest_name.contains(&Some(n.clone()));
            ref_src.insert(*hid, (n, sp, now));
          }
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
    self.destructure_ref_src = ref_src;
    // Parts nothing is assigned to (`(a, _) = (s, t)`) are moved
    // into the temporary and dropped with it, at the end of the
    // statement. The temporary has no column, so the move's tooltip
    // is where that drop is stated (unless the type has nothing to
    // drop).
    for e in &discarded {
      if moved_name(self, e).is_some() {
        let ty = self.tcx.typeck(e.hir_id.owner).node_type(e.hir_id);
        let needs_drop = ty.needs_drop(self.tcx, rustc_middle::ty::TypingEnv::post_analysis(self.tcx, e.hir_id.owner));
        let to = if needs_drop { ResourceTy::Temporary { extended: false, tuple: None } } else { ResourceTy::Anonymous };
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
    self.destructure_ref_src.clear();
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
        // `let r = loop { … break k; }`: the `break`s hand `r` its
        // value as they're visited (#431), so `r` is bound first.
        ExprKind::Loop(_, _, rustc_hir::LoopSource::Loop, _) => loop_has_break_value(init),
        ExprKind::Match(_, _, rustc_hir::MatchSource::TryDesugar(_)) => false,
        ExprKind::Match(..) => true,
        _ => false,
      };
    if init_is_branching {
      // `let (a, b) = loop { … break (x, y); }`: each `break` value is
      // bound against the pattern (#452).
      if matches!(init.kind, ExprKind::Loop(..)) && !matches!(pat.kind, PatKind::Binding(..)) {
        self.break_pats.push((init.hir_id, pat, self.current_scope, self.inside_branch));
      } else {
        self.bind_pat(pat, init, is_skipped);
      }
      self.visit_expr(init);
      // A reference returned by a call in a branch keeps that call's
      // borrow (#337).
      if let PatKind::Binding(_, hir_id, _, None) = pat.kind {
        self.mark_branch_results_kept(&binding_name(&self.tcx, hir_id), init);
      }
      self.break_pats.retain(|(id, ..)| *id != init.hir_id);
      self.break_binds.retain(|(id, ..)| *id != init.hir_id);
    } else {
      if !is_skipped {
        self.visit_expr(init);
      }
      self.bind_pat(pat, init, is_skipped);
    }
    self.keep_literal_temp_with_refs(pat);
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
    let PatKind::Binding(annotation, hirid, _, None) = pat.kind else { return false };
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

    let name = binding_name(&self.tcx, hirid);
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
      // `let (a, b) = (e1, e2);` — Issue #86. Sub-patterns pair with
      // the literal's elements by position. With a `..` (`let (a, ..) =
      // (e1, e2, e3);`) the patterns after it pair with the last
      // elements, and the elements it covers are paired with nothing.
      // Those, like an element matched by `_`, are still moved into the
      // tuple temporary (#439).
      (PatKind::Tuple(sub_pats, ddpos), ExprKind::Tup(sub_exprs))
        if sub_pats.len() <= sub_exprs.len()
          && (ddpos.as_opt_usize().is_some() || sub_pats.len() == sub_exprs.len()) =>
      {
        let d = ddpos.as_opt_usize().unwrap_or(sub_pats.len());
        let tail = sub_pats.len() - d;
        let tail_start = sub_exprs.len() - tail;
        let pairs: Vec<_> = sub_pats[..d].iter().zip(sub_exprs.iter())
          .chain(sub_pats[d..].iter().zip(sub_exprs[tail_start..].iter()))
          .collect();
        self.premove_into_literal(&pairs, sub_exprs[d..tail_start].iter().collect(), is_skipped);
        for (sp, se) in pairs {
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
        // A field the pattern leaves out (`..`) is still moved into the
        // struct temporary (#439).
        let pairs: Vec<_> = pat_fields.iter()
          .filter_map(|pf| expr_fields.iter().find(|f| f.ident.name == pf.ident.name).map(|ef| (pf.pat, ef.expr)))
          .collect();
        let ignored: Vec<_> = expr_fields.iter()
          .filter(|ef| !pat_fields.iter().any(|pf| pf.ident.name == ef.ident.name))
          .map(|ef| ef.expr)
          .collect();
        self.premove_into_literal(&pairs, ignored, is_skipped);
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
        self.bind_pat_from_member(pat, &place, ty, init, is_skipped, &[]);
      }
      // `let ((x, _) | (_, x)) = t;`: `x` comes from `t.0` or `t.1`,
      // whichever alternative matches, like in such a match arm
      // (#533, #597). Bind the first alternative from `t`'s parts, with
      // `x` a new value and each place moved into it only if its
      // alternative matched. An or-pattern whose alternatives bind
      // every name from the same place falls through to bind_walk.
      (PatKind::Or(_), ExprKind::Path(_) | ExprKind::Field(..))
        if self.member_place_name(init).is_some() && !is_skipped =>
      {
        let place = self.member_place_name(init).unwrap();
        let typeck = self.tcx.typeck(init.hir_id.owner);
        let ty = typeck.node_type(init.hir_id);
        let parent = ResourceTy::Value(self.raps.get(&place).unwrap().rap.to_owned());
        let line = span_to_line(&pat.span, &self.tcx);
        let (first, conflicts) = self.or_arm_stand_in(pat, std::slice::from_ref(&parent), &[ty], line, typeck, &[]);
        if conflicts.is_empty() {
          self.bind_walk(pat, init, is_skipped);
        } else {
          self.bind_pat_from_member(first, &place, ty, init, is_skipped, &conflicts);
        }
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
        let after_start = elems.len() - after.len();
        // mid-Wild: middle elements are discarded by the pattern; their
        // side effects were captured by the entry visit_expr, and they
        // are still moved into the array temporary (#439).
        let pairs: Vec<_> = before.iter().zip(elems.iter())
          .chain(after.iter().zip(elems[after_start..].iter()))
          .collect();
        self.premove_into_literal(&pairs, elems[before.len()..after_start].iter().collect(), is_skipped);
        for (sp, se) in pairs {
          self.bind_pat(sp, se, is_skipped);
        }
      }
      // Canonical leaf: a single named binding consuming the init.
      (PatKind::Binding(annotation, hirid, _, sub_pat), _) => {
        self.bind_one(binding_name(&self.tcx, hirid), annotation, hirid, init, is_skipped);
        // `name @ inner_pat` — register any nested bindings too.
        if let Some(sp) = sub_pat {
          self.bind_walk(sp, init, is_skipped);
        }
      }
      // `let _ = expr;` — value discarded. The RHS visit at entry
      // already covered side effects; nothing else to register.
      // An element of a literal it is paired with (`let (_, b) = (s,
      // 1);`) is still moved into the literal's temporary (#439);
      // `let _ = s;` on a place moves nothing.
      (PatKind::Wild, _) => { self.move_into_literal(init, is_skipped); }
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
    // The names an or-pattern binds from different places, with those
    // places (see the `PatKind::Or` arm of `bind_pat`).
    or_conflicts: &[OrConflict],
  ) {
    match pat.kind {
      PatKind::Wild => {}
      // A by-value binding of a non-reference part.
      PatKind::Binding(annotation, hirid, _, None)
        if matches!(annotation.0, rustc_hir::ByRef::No) && self.raps.contains_key(member) =>
      {
        let name = binding_name(&self.tcx, hirid);
        let lhs_ty = self.tcx.typeck(hirid.owner).node_type(hirid);
        let evt = self.bind_evt(lhs_ty, hirid);
        self.define_lhs(name.clone(), bool_of_mut(annotation.1), init, lhs_ty);
        // `let P { part: r } = p;` with `part: &T` copies the
        // reference in `p.part`: `r` borrows what that borrows (#709).
        if member_ty.is_ref() {
          self.adopt_part_borrow(&name, member);
        }
        if is_skipped {
          self.skip_raps.insert(name);
          return;
        }
        let line_num = span_to_line(&init.span, &self.tcx);
        let to = self.raps.get(&name).unwrap().rap.to_owned();
        if let Some(c) = or_conflicts.iter().find(|c| c.binds(&name)) {
          self.bind_or_conflict(c, to, evt, line_num);
          return;
        }
        let from = self.raps.get(member).unwrap().rap.to_owned();
        self.update_rap(&from, line_num);
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
                  self.bind_pat_from_member(sp, r.name(), part_ty, init, is_skipped, or_conflicts);
                }
                _ => self.bind_walk(sp, init, is_skipped),
              }
            }
          }
          None if or_conflicts.is_empty() || is_skipped => self.bind_walk(pat, init, is_skipped),
          // `Some(x)` on `t.0` in `let ((Some(x), _) | (_, Some(x))) =
          // t;`: `x` is still bound from `t.0` or `t.1`.
          None => {
            let mut bs = Vec::new();
            binding_pats(pat, &mut bs);
            for b in bs {
              let PatKind::Binding(annotation, hirid, _, _) = b.kind else { continue };
              let name = binding_name(&self.tcx, hirid);
              let Some(c) = or_conflicts.iter().find(|c| c.binds(&name)) else {
                self.bind_one(name, annotation, hirid, init, is_skipped);
                continue;
              };
              // `let ((ref x, _) | (_, ref x)) = t;` borrows one of the
              // places; only a match arm draws that (#565).
              if matches!(annotation.0, rustc_hir::ByRef::Yes(..)) {
                panic!("or-patterns that bind the reference `{}` from different places in the value being matched \
                        (line {}) aren't supported yet in an `if let` or `let`", name, span_to_line(&pat.span, &self.tcx));
              }
              let lhs_ty = self.tcx.typeck(hirid.owner).node_type(hirid);
              let evt = if self.ty_is_copy(lhs_ty, hirid.owner) { Evt::Copy } else { Evt::Move };
              self.define_lhs(name.clone(), bool_of_mut(annotation.1), init, lhs_ty);
              let to = self.raps.get(&name).unwrap().rap.to_owned();
              self.bind_or_conflict(c, to, evt, span_to_line(&init.span, &self.tcx));
            }
          }
        }
      }
    }
  }

  /// `let ((x, _) | (_, x)) = t;` (#597): `to` is `x`, bound from one
  /// of `c.places`, whichever alternative matches. Drawn as in such a
  /// match arm (`or_conflict_events`): `x` gets a new value, with no
  /// arrow, and when it moves (rather than is copied) each place gets
  /// an `OrAltMove`, so it's maybe-moved afterwards.
  fn bind_or_conflict(&mut self, c: &OrConflict, to: ResourceAccessPoint, evt: Evt, line: usize) {
    if matches!(to, ResourceAccessPoint::Struct(_)) {
      panic!("or-patterns that bind the struct or tuple `{}` from different places in the value being matched \
              (line {}) aren't supported yet in an `if let` or `let`", c.name, line);
    }
    if matches!(evt, Evt::Move) {
      let names: Vec<String> = c.places.iter().map(|r| r.name().to_string()).collect();
      for place in &c.places {
        self.update_rap(place, line);
        let id = *self.unique_id;
        *self.unique_id += 1;
        self.add_external_event(line, ExternalEvent::OrAltMove {
          ro: place.clone(), into: to.name().clone(), from: names.clone(), tuple: None, part_of: None, in_let: true, id,
        });
      }
    }
    self.add_ev(line, Evt::Bind, ResourceTy::Value(to), ResourceTy::Anonymous, false);
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
      PatKind::Binding(annotation, hirid, _, sub_pat) => {
        self.bind_one(binding_name(&self.tcx, hirid), annotation, hirid, init, is_skipped);
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
        let name = binding_name(&self.tcx, hirid);
        let pat_ty = self.tcx.typeck(hirid.owner).pat_ty(pat);
        let lender = if pat_ty.is_ref() { self.pattern_lender.clone() } else { None };
        if pat_ty.is_ref() {
          let pointee_copy = self.ref_pointee_is_copy(pat_ty, hirid.owner);
          self.add_ref(
            name.clone(),
            bool_of_mut(pat_ty.ref_mutability().unwrap()),
            false,
            line,
            lender.as_ref().map_or(ResourceTy::Anonymous, |(_, l)| l.clone()),
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
        match lender {
          Some((e, l)) => self.add_ev(line, e, to_rty, l, false),
          None => self.add_ev(line, Evt::Bind, to_rty, ResourceTy::Anonymous, false),
        }
        if let Some(sub) = sub_pat {
          self.register_pattern_bindings_anon(sub, line);
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
      self.draw_kept_closure_borrows(&name, init);
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

  /// The pattern that stands for a match arm's or-pattern
  /// (`Ok(s) | Err(s) =>`) when drawing its bindings: the first
  /// alternative (#503). Every alternative binds the same names with
  /// the same types and binding modes. What can differ is the place a
  /// name is bound from, when that place has a column of its own: a
  /// value of a tuple of several matched values (`match (a, b) {
  /// (Some(x), _) | (_, Some(x)) => … }`), or a part of a tuple or
  /// struct with element / field columns (`match t { (x, _) | (_, x)
  /// => … }`, #189). Which one `x` comes from is decided at run time.
  /// Those names are returned as `OrConflict`s: the arm draws `x`
  /// as a new value and each place as moved into it only if its
  /// alternative matched (#533), or, for a reference `x`, as borrowed
  /// by it only if its alternative matched (#565); see
  /// `or_conflict_events`. What can't be drawn that way panics
  /// (reported as "can't visualize this yet") rather than draw `x` as
  /// taken from the first place: a reference into both a tuple of values
  /// moved into the scrutinee (#580) and a place outside it, one through
  /// a place that is itself a reference, and a `&mut` moved out of one
  /// of the places. A reference into the moved values only borrows the
  /// tuple, whichever alternative matched: no conflict.
  pub(crate) fn or_arm_stand_in(
    &self,
    pat: &'tcx Pat<'tcx>,
    parents: &[ResourceTy],
    parents_ty: &[rustc_middle::ty::Ty<'tcx>],
    line: usize,
    typeck_res: &'tcx rustc_middle::ty::TypeckResults<'tcx>,
    tuple_vars: &[ResourceAccessPoint],
  ) -> (&'tcx Pat<'tcx>, Vec<OrConflict>) {
    let PatKind::Or(alts) = pat.kind else { return (pat, Vec::new()) };
    let sources: Vec<BTreeMap<String, (String, Option<ResourceTy>)>> =
      alts.iter().map(|alt| self.or_alt_sources(alt, parents, parents_ty)).collect();
    let mut conflicts = Vec::new();
    for name in sources[0].keys() {
      let mut places: Vec<(String, Option<ResourceTy>)> = Vec::new();
      for m in &sources {
        if let Some(src) = m.get(name) {
          if !places.iter().any(|(k, _)| *k == src.0) {
            places.push(src.clone());
          }
        }
      }
      if places.len() < 2 {
        continue;
      }
      let refuse = |why: &str| -> ! {
        panic!("or-patterns that bind {} `{}` from different places in the value being matched (line {}) \
                aren't supported yet", why, name, line)
      };
      let mut bs = Vec::new();
      binding_pats(&alts[0], &mut bs);
      let binding = bs.iter().find(|b| matches!(b.kind, PatKind::Binding(_, _, ident, _) if ident.to_string() == *name));
      let by_ref = binding.is_some_and(|b| matches!(
        typeck_res.pat_binding_modes().get(b.hir_id).map(|m| m.0),
        Some(rustc_hir::ByRef::Yes(..))
      ));
      let ty = binding.map(|b| typeck_res.node_type(b.hir_id));
      // `match (a, b)` moves `a` and `b` into a temporary tuple, which
      // is drawn only for variables without part columns (#327): with a
      // struct or tuple `a`, what `x` takes or borrows would be drawn as
      // taken from `a` itself, which no longer has it.
      let copied = !by_ref && ty.is_some_and(|t| self.ty_is_copy(t, pat.hir_id.owner));
      if parents.len() > 1 && !copied {
        let untracked = sources.iter().filter_map(|m| m.get(name)).any(|(k, _)| {
          k.split_once(':').and_then(|(i, _)| i.parse::<usize>().ok()).is_some_and(|i| {
            parents_ty.get(i).is_some_and(|t| !t.is_ref())
              && !parents[i].extract_rap().is_some_and(|r| tuple_vars.contains(r))
          })
        });
        if untracked {
          panic!("or-patterns that bind `{}` from different values of a tuple being matched, when a value is a \
                  struct or tuple or is copied into it (line {}), aren't supported yet", name, line);
        }
      }
      if by_ref {
        // `match (a, b) { (Some(ref x), _) | (_, Some(ref x)) => … }`:
        // `a` and `b` are moved into the tuple being matched, and `x`
        // borrows that tuple's parts, not `a` or `b` (#580). Whichever
        // alternative matched, it borrows the tuple, which has no column:
        // the arm draws that like any `ref` into the tuple, so it's no
        // conflict. Mixed with a place outside the tuple (`match (a, &b)`)
        // it would be.
        let from_tuple: Vec<bool> = sources.iter().filter_map(|m| m.get(name)).map(|(k, _)| {
          parents.len() > 1 && k.split_once(':').and_then(|(i, _)| i.parse::<usize>().ok())
            .is_some_and(|i| parents_ty.get(i).is_some_and(|t| !t.is_ref()))
        }).collect();
        if from_tuple.iter().all(|t| *t) {
          continue;
        }
        if from_tuple.iter().any(|t| *t) {
          panic!("or-patterns that bind the reference `{}` from both a value moved into the tuple being matched \
                  and a place outside it (line {}) aren't supported yet", name, line);
        }
      } else if ty.is_some_and(|t| t.is_ref() && t.ref_mutability() == Some(rustc_hir::Mutability::Mut)) {
        // A `&mut` stored in each place, moved out of one of them.
        refuse("the mutable reference");
      }
      let mut raps = Vec::new();
      for (_, place) in &places {
        match place.as_ref().and_then(|p| p.extract_rap()) {
          // A place that's itself a reference: `x` would borrow what
          // it points to, through it.
          Some(rap) if by_ref && rap.is_ref() => panic!(
            "or-patterns that bind `{}` through different references being matched (line {}) aren't supported yet",
            name, line),
          Some(rap) => raps.push(rap.clone()),
          None => refuse("the name"),
        }
      }
      conflicts.push(OrConflict { name: name.clone(), places: raps });
    }
    let (inner, _) = self.or_arm_stand_in(&alts[0], parents, parents_ty, line, typeck_res, tuple_vars);
    (inner, conflicts)
  }

  /// `or_conflict_events` for an `if let` / `while let` pattern
  /// (#597): only a name moved or copied from different places is drawn
  /// there. A reference or a struct or tuple name (drawn in a match arm
  /// since #565) panics, reported as "can't visualize this yet". The
  /// events all go at the pattern's line.
  pub(crate) fn if_let_or_conflict_events(
    &mut self,
    conflicts: &[OrConflict],
    bound: &mut Vec<(ResourceAccessPoint, Evt, ResourceTy, Option<bool>)>,
    line: usize,
    tuple_vars: &[ResourceAccessPoint],
    tuple: &str,
  ) -> Vec<ExternalEvent> {
    for entry in bound.iter() {
      let written = split_numbered_name(entry.0.name()).map_or(entry.0.name().as_str(), |(base, _)| base);
      let Some(c) = conflicts.iter().find(|c| c.name == written) else { continue };
      if matches!(entry.0, ResourceAccessPoint::Struct(_)) {
        panic!("or-patterns that bind the struct or tuple `{}` from different places in the value being matched \
                (line {}) aren't supported yet in an `if let` or `let`", c.name, line);
      }
      if !matches!(entry.1, Evt::Move | Evt::Copy) {
        panic!("or-patterns that bind the reference `{}` from different places in the value being matched \
                (line {}) aren't supported yet in an `if let` or `let`", c.name, line);
      }
    }
    let out = self.or_conflict_events(conflicts, bound, line, tuple_vars, tuple, line);
    debug_assert!(out.end.is_empty() && out.decls.is_empty());
    out.start
  }

  /// The events for the names an or-pattern binds from different
  /// places (#533), replacing what the first alternative alone would
  /// draw in `bound` (a name, how it's bound, from where):
  ///
  /// * A moved or copied name is bound as a new value (`Evt::Bind`, no
  ///   arrow), and each place it may come from, when the value moves
  ///   (rather than is copied), gets an `OrAltMove`: its resource moves
  ///   into the name if its alternative is the one that matched, so
  ///   it's maybe-moved afterwards. A place in `tuple_vars` (`match (a,
  ///   b)`) was already moved into the temporary tuple `tuple` (#327):
  ///   it's the tuple's copy of its value that may move, so it isn't
  ///   maybe-moved. A struct or tuple name's fields are bound with it,
  ///   and each place's fields get an `OrAltMove` into the matching
  ///   field (#565).
  /// * A reference (`ref x`, or a binding through a reference) borrows
  ///   whichever place matched (#565): an `OrAltBorrow` per place where
  ///   the borrow starts, and one where it ends. The borrow checker
  ///   treats every place as borrowed until the reference's last use,
  ///   which MIR gives (`arm_end`, the end of the arm, if it doesn't).
  ///   The reference is taken out of `bound` (its `OrAltBorrow`s draw
  ///   it) and returned in `decls`.
  fn or_conflict_events(
    &mut self,
    conflicts: &[OrConflict],
    bound: &mut Vec<(ResourceAccessPoint, Evt, ResourceTy, Option<bool>)>,
    line: usize,
    tuple_vars: &[ResourceAccessPoint],
    tuple: &str,
    arm_end: usize,
  ) -> OrArmEvents {
    let mut out = OrArmEvents::default();
    let mut taken = Vec::new();
    for (k, entry) in bound.iter_mut().enumerate() {
      // The conflict names the binding as written; its column may be
      // `x₂` when an earlier `x` is in the same function, or `x` with
      // `SCOPE_MARK`s when a same-named variable in a separate scope is
      // a different kind of value (#598).
      let written = written_name(entry.0.name());
      let Some(c) = conflicts.iter().find(|c| c.name == written) else { continue };
      let xname = entry.0.name().clone();
      let names: Vec<String> = c.places.iter().map(|r| r.name().to_string()).collect();
      match entry.1 {
        Evt::SBorrow | Evt::MBorrow => {
          let mutable = matches!(entry.1, Evt::MBorrow);
          let x = entry.0.clone();
          // Which place `x` borrows isn't known: give it no lender, so
          // nothing in the arm is drawn on the first place's column.
          if let Some(rd) = self.borrow_map.get_mut(x.name()) {
            rd.lender = ResourceTy::Anonymous;
          }
          let ends_at = self.or_ref_last_use(x.name(), line).filter(|l| *l >= line && *l <= arm_end).unwrap_or(arm_end);
          for (i, place) in c.places.iter().enumerate() {
            for (end, at) in [(false, line), (true, ends_at)] {
              let id = *self.unique_id;
              *self.unique_id += 1;
              let ev = ExternalEvent::OrAltBorrow {
                ro: place.clone(), into: x.clone(), from: names.clone(), mutable, end, first: i == 0, id,
              };
              if end { out.end.push((at, ev)); } else { out.start.push(ev); }
            }
          }
          out.decls.push(x);
          taken.push(k);
        }
        Evt::Move | Evt::Copy => {
          if matches!(entry.1, Evt::Move) {
            for place in &c.places {
              let id = *self.unique_id;
              *self.unique_id += 1;
              let in_tuple = tuple_vars.contains(place).then(|| tuple.to_owned());
              out.start.push(ExternalEvent::OrAltMove {
                ro: place.clone(), into: xname.clone(), from: names.clone(), tuple: in_tuple.clone(), part_of: None, in_let: false, id,
              });
              // A struct or tuple moves its fields with it.
              let prefix = format!("{}.", place.name());
              for member in self.struct_members_deep(place.name()) {
                // A reference field isn't moved out of anything that
                // owns it; the name's own reference fields are drawn
                // where the pattern binds them.
                if member.is_ref() {
                  continue;
                }
                let suffix = &member.name()[prefix.len()..];
                let into = format!("{}.{}", xname, suffix);
                if !self.raps.contains_key(&into) {
                  continue;
                }
                let id = *self.unique_id;
                *self.unique_id += 1;
                out.start.push(ExternalEvent::OrAltMove {
                  ro: member.clone(), into, from: names.clone(), tuple: in_tuple.clone(),
                  part_of: Some((xname.clone(), place.name().to_string())), in_let: false, id,
                });
              }
            }
          }
          // The name's own fields get their part of the value with it.
          if matches!(entry.0, ResourceAccessPoint::Struct(_)) {
            self.bind_struct_fields(line, &xname);
          }
          *entry = (entry.0.clone(), Evt::Bind, ResourceTy::Anonymous, None);
        }
        _ => panic!("or-patterns that bind `{}` from different places in the value being matched \
                     (line {}) aren't supported yet", c.name, line),
      }
    }
    for k in taken.into_iter().rev() {
      bound.remove(k);
    }
    out
  }

  /// The last line the reference `name`, bound by a pattern on `line`,
  /// keeps its borrow (#565): the end of the region of the loans MIR
  /// issues for it there.
  fn or_ref_last_use(&self, name: &str, line: usize) -> Option<usize> {
    self.gather_borrow_data(&self.bwf).iter()
      .filter(|m| m.region.0 == line && m.assigned_place.as_deref() == Some(name))
      .map(|m| m.region.1)
      .max()
  }

  /// Each binding of a match arm's pattern `alt`, and the place it's
  /// bound from (as a key that tells places apart, and the place), the
  /// way the arm code below picks it (one parent per element of a
  /// tuple-literal scrutinee, then `pat_parts`).
  fn or_alt_sources(
    &self,
    alt: &'tcx Pat<'tcx>,
    parents: &[ResourceTy],
    parents_ty: &[rustc_middle::ty::Ty<'tcx>],
  ) -> BTreeMap<String, (String, Option<ResourceTy>)> {
    let mut out = BTreeMap::new();
    match alt.kind {
      PatKind::Tuple(ps, _) | PatKind::TupleStruct(_, ps, _) if parents.len() > 1 && ps.len() == parents.len() => {
        for (i, p) in ps.iter().enumerate() {
          self.pat_sources(p, &parents[i], parents_ty[i], &format!("{}:", i), &mut out);
        }
      }
      _ if parents.len() == 1 => self.pat_sources(alt, &parents[0], parents_ty[0], "", &mut out),
      _ => {
        let mut bs = Vec::new();
        binding_pats(alt, &mut bs);
        for b in bs {
          if let PatKind::Binding(_, _, ident, _) = b.kind {
            out.insert(ident.to_string(), ("*".to_owned(), None));
          }
        }
      }
    }
    out
  }

  /// See `or_alt_sources`; mirrors `get_dec_of_pat_parts`.
  fn pat_sources(
    &self,
    pat: &'tcx Pat<'tcx>,
    parent: &ResourceTy,
    parent_ty: rustc_middle::ty::Ty<'tcx>,
    prefix: &str,
    out: &mut BTreeMap<String, (String, Option<ResourceTy>)>,
  ) {
    let whole = |p: &'tcx Pat<'tcx>, out: &mut BTreeMap<String, (String, Option<ResourceTy>)>| {
      let mut bs = Vec::new();
      binding_pats(p, &mut bs);
      for b in bs {
        if let PatKind::Binding(_, _, ident, _) = b.kind {
          out.insert(ident.to_string(), (format!("{}{}", prefix, parent.real_name()), Some(parent.clone())));
        }
      }
    };
    match self.pat_parts(pat, parent, parent_ty) {
      Some(subs) => for (sp, part) in subs {
        match part {
          Some((part_rty, part_ty)) => self.pat_sources(sp, &part_rty, part_ty, prefix, out),
          None => whole(sp, out),
        }
      },
      None => whole(pat, out),
    }
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
    let name = binding_name(&self.tcx, hirid);
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

  /// `e` is an element of a tuple / struct / array literal that a `let`
  /// pattern is matched against (`(s, t)` in `let (ref a, ..) = (s,
  /// t);`). Building the literal moves (or copies) a variable element
  /// into the literal's temporary, whether the pattern binds it by
  /// value, by `ref`, or not at all (`_`, `..`): rustc rejects a later
  /// use of `s` ("borrow of moved value"). This is called for the
  /// elements the pattern doesn't bind by value; they go into a
  /// `(temporary N)` column (see `literal_temp`) and return it.
  /// Does nothing for the whole initializer (`let _ = s;` moves
  /// nothing) or for a non-variable element, whose events the entry
  /// visit already drew. With no column to put it in (the literal
  /// owns nothing to drop, or sits in a one-line branch) the move has
  /// no target.
  fn move_into_literal(&mut self, e: &'tcx Expr<'tcx>, is_skipped: bool) -> Option<ResourceAccessPoint> {
    if is_skipped
      || matches!(self.tcx.parent_hir_node(e.hir_id), rustc_hir::Node::LetStmt(_))
      || e.span.from_expansion()
      || !matches!(
        e.kind,
        ExprKind::Path(QPath::Resolved(_, rustc_hir::Path { res: Res::Local(_), .. }))
          | ExprKind::Field(..)
          | ExprKind::Index(..)
          | ExprKind::Unary(UnOp::Deref, _)
      )
    {
      return None;
    }
    let src = fetch_rap(e, &self.tcx, &self.raps)?;
    let line_num = expr_to_line(e, &self.tcx);
    let src_ty = self.tcx.typeck(e.hir_id.owner).node_type(e.hir_id);
    let ev = if src_ty.is_ref() || self.ty_is_copy(src_ty, e.hir_id.owner) { Evt::Copy } else { Evt::Move };
    let tmp = self.literal_temp(e);
    if self.premoved.contains(&e.hir_id) {
      return tmp;
    }
    self.update_rap(&src, line_num);
    let to = tmp.clone().map_or(ResourceTy::Anonymous, ResourceTy::Value);
    self.add_ev(line_num, ev, to, ResourceTy::Value(src), false);
    tmp
  }

  /// A `let` whose pattern has `ref` bindings and whose literal got a
  /// `(temporary N)` column (see `literal_temp`): the temporary lives
  /// as long as those bindings do, so it is declared, and goes out of
  /// scope, with them (like #400's `let r = &String::from(..);`).
  fn keep_literal_temp_with_refs(&mut self, pat: &'tcx Pat<'tcx>) {
    let rustc_hir::Node::LetStmt(l) = self.tcx.parent_hir_node(pat.hir_id) else { return };
    let Some(tmp) = self.literal_temps.get(&l.hir_id) else { return };
    if tmp.is_unbound_temporary() {
      return;
    }
    let tmp_name = tmp.name().to_owned();
    let mut refs = Vec::new();
    pat.each_binding(|ann, _, _, ident| {
      if matches!(ann.0, rustc_hir::ByRef::Yes(..)) { refs.push(ident.to_string()); }
    });
    for r in refs {
      if let Some(rd) = self.raps.get_mut(&r) {
        rd.extended_temp = Some(tmp_name.clone());
      }
    }
  }

  /// The elements of the literal in `let PAT = LITERAL;` that the
  /// pattern doesn't bind by value go into the literal's temporary
  /// (#439): the `ignored` ones (`..`, an omitted field, an array's
  /// middle) and those `pairs` matches with `_` or a `ref` binding.
  /// All of them are moved before any `ref` binding borrows the
  /// temporary, so the borrow isn't drawn in the middle of the moves.
  fn premove_into_literal(&mut self, pairs: &[(&'tcx Pat<'tcx>, &'tcx Expr<'tcx>)], ignored: Vec<&'tcx Expr<'tcx>>, is_skipped: bool) {
    for &(sp, se) in pairs {
      let not_by_value = match sp.kind {
        PatKind::Wild => true,
        PatKind::Binding(ann, ..) => matches!(ann.0, rustc_hir::ByRef::Yes(..)),
        _ => false,
      };
      if not_by_value && self.move_into_literal(se, is_skipped).is_some() {
        self.premoved.insert(se.hir_id);
      }
    }
    // A fresh value (a call's result, a nested literal) that the pattern
    // leaves is part of the same temporary: with no variable moved in,
    // the temporary still exists, and is dropped (or, with a `ref`
    // binding, kept) with everything in it (#483).
    let leftover = pairs.iter()
      .filter(|(sp, _)| match sp.kind {
        PatKind::Wild => true,
        PatKind::Binding(ann, ..) => matches!(ann.0, rustc_hir::ByRef::Yes(..)),
        _ => false,
      })
      .map(|(_, se)| *se)
      .chain(ignored.iter().copied())
      .find(|e| !matches!(
        e.kind,
        ExprKind::Path(_) | ExprKind::Field(..) | ExprKind::Index(..) | ExprKind::Unary(UnOp::Deref, _)
      ));
    for se in ignored {
      self.move_into_literal(se, is_skipped);
    }
    if let Some(e) = leftover {
      self.start_literal_temp(e, is_skipped);
    }
  }

  /// The temporary of a literal that nothing was moved into (every
  /// element the pattern leaves is a fresh value): made here, and
  /// holding the literal's value from this line (#483).
  fn start_literal_temp(&mut self, e: &'tcx Expr<'tcx>, is_skipped: bool) {
    use rustc_hir::Node;
    if is_skipped || e.span.from_expansion() {
      return;
    }
    let Some(let_stmt) = self.tcx.hir_parent_iter(e.hir_id).find_map(|(_, n)| match n {
      Node::LetStmt(l) => Some(l),
      _ => None,
    }) else { return };
    if self.literal_temps.contains_key(&let_stmt.hir_id) {
      return;
    }
    if let Some(tmp) = self.literal_temp(e) {
      let line = expr_to_line(e, &self.tcx);
      self.add_ev(line, Evt::Bind, ResourceTy::Value(tmp), ResourceTy::Anonymous, false);
    }
  }

  /// `let ref t = String::from("tmp");`, `let P { ref name, .. } =
  /// make_p();`: the `let` borrows from the value on its right, which
  /// rustc keeps as a temporary to the end of the block (temporary
  /// lifetime extension), and drops there (#483). Draw it like `let r =
  /// &String::from("tmp");` (#400): a `(temporary N)` column that takes
  /// the value, is borrowed by the `ref` bindings, and goes out of
  /// scope with them. None when it has nothing to drop, sits in a
  /// one-line branch, or the pattern also moves a part of it out by
  /// value (what is left of it isn't drawn).
  fn ref_bound_value_temp(&mut self, init: &'tcx Expr<'tcx>, is_skipped: bool) -> Option<ResourceAccessPoint> {
    let rustc_hir::Node::LetStmt(l) = self.tcx.parent_hir_node(init.hir_id) else { return None };
    if is_skipped {
      return None;
    }
    if let Some(t) = self.literal_temps.get(&l.hir_id) {
      return Some(t.clone());
    }
    let typeck = self.tcx.typeck(init.hir_id.owner);
    let ty = typeck.expr_ty(init);
    let env = rustc_middle::ty::TypingEnv::post_analysis(self.tcx, init.hir_id.owner);
    if ty.is_ref() || !ty.needs_drop(self.tcx, env) || self.inside_one_line_branch(init) {
      return None;
    }
    let (mut moves_out, mut mutable) = (false, false);
    l.pat.each_binding(|ann, hid, _, _| match ann.0 {
      rustc_hir::ByRef::Yes(_, m) => mutable |= m == Mutability::Mut,
      rustc_hir::ByRef::No => moves_out |= typeck.node_type(hid).needs_drop(self.tcx, env),
    });
    if moves_out {
      return None;
    }
    self.temp_count += 1;
    let tmp_name = temporary_label(self.temp_count);
    self.add_owner(tmp_name.clone(), mutable, false, false, InteriorMut::No, self.current_scope, !self.inside_branch);
    let tmp = self.raps.get(&tmp_name).unwrap().rap.to_owned();
    let n_events = self.preprocessed_events.len();
    self.match_rhs(ResourceTy::Value(tmp.clone()), init, Evt::Move);
    if self.preprocessed_events.len() == n_events {
      let line = expr_to_line(init, &self.tcx);
      self.add_ev(line, Evt::Bind, ResourceTy::Value(tmp.clone()), ResourceTy::Anonymous, false);
    }
    self.literal_temps.insert(l.hir_id, tmp.clone());
    Some(tmp)
  }

  /// The `(temporary N)` column of the literal that `e` is an element
  /// of, in `let PAT = LITERAL;`, made on first use (#439). It holds
  /// what the pattern doesn't take by value. Rust drops it at the end
  /// of the statement, or, when a `ref` / `ref mut` binding borrows
  /// from it, at the end of the block (temporary lifetime extension):
  /// - the first is drawn like an unbound temporary (#457), dropped
  ///   by a `TemporaryDrop` where the statement ends;
  /// - the second like a `let`-extended one (#400): the `ref` bindings
  ///   borrow it and it goes out of scope with them.
  /// None when the literal's type has nothing to drop, or it sits in a
  /// one-line `if` / `match` (the limit `note_temporary_drop` has).
  fn literal_temp(&mut self, e: &'tcx Expr<'tcx>) -> Option<ResourceAccessPoint> {
    use rustc_hir::Node;
    let let_stmt = self.tcx.hir_parent_iter(e.hir_id).find_map(|(_, n)| match n {
      Node::LetStmt(l) => Some(l),
      _ => None,
    })?;
    if let Some(t) = self.literal_temps.get(&let_stmt.hir_id) {
      return Some(t.clone());
    }
    let literal = self.tcx.hir_parent_iter(e.hir_id).find_map(|(_, n)| match n {
      Node::Expr(p) => Some(p),
      _ => None,
    })?;
    let typeck = self.tcx.typeck(e.hir_id.owner);
    let ty = typeck.expr_ty(literal);
    let env = rustc_middle::ty::TypingEnv::post_analysis(self.tcx, e.hir_id.owner);
    if !ty.needs_drop(self.tcx, env) || self.inside_one_line_branch(e) {
      return None;
    }
    let (mut extended, mut mutable) = (false, false);
    let_stmt.pat.each_binding(|ann, _, _, _| {
      if let rustc_hir::ByRef::Yes(_, m) = ann.0 {
        extended = true;
        mutable |= m == Mutability::Mut;
      }
    });
    self.temp_count += 1;
    let tmp_name = temporary_label(self.temp_count);
    // The variables moved into it read as moving into the literal on
    // the right of the `let`, not into one nothing binds (#570).
    if extended {
      self.add_owner(tmp_name.clone(), mutable, false, false, InteriorMut::No, self.current_scope, !self.inside_branch);
      if let Some(ResourceAccessPoint::Owner(o)) = self.raps.get_mut(&tmp_name).map(|rd| &mut rd.rap) {
        o.let_literal_temp = true;
      }
    } else {
      let (when, end_line, owner) = self.temporary_scope_end(e)?;
      self.add_owner(tmp_name.clone(), false, false, false, InteriorMut::No, end_line, !self.inside_branch);
      if let Some(ResourceAccessPoint::Owner(o)) = self.raps.get_mut(&tmp_name).map(|rd| &mut rd.rap) {
        o.unbound_temp = true;
        o.let_literal_temp = true;
      }
      self.raps.get_mut(&tmp_name).unwrap().temp_owner = owner;
      let tmp = self.raps.get(&tmp_name).unwrap().rap.to_owned();
      let ty = strip_module_paths(&ty.to_string());
      let id = *self.unique_id;
      self.add_external_event(end_line, ExternalEvent::TemporaryDrop { ro: tmp, ty: Some(ty), when: when.to_owned(), literal: true, early: false, id });
      *self.unique_id += 1;
    }
    let tmp = self.raps.get(&tmp_name).unwrap().rap.to_owned();
    self.literal_temps.insert(let_stmt.hir_id, tmp.clone());
    Some(tmp)
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
  ///   a temporary, which gets a `(temporary N)` column (#483);
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
      let lender = match self.move_into_literal(init, is_skipped) {
        Some(tmp) => ResourceTy::Value(tmp),
        // `let ref t = String::from("tmp");`: the whole initializer is
        // a value, which this `let` keeps alive (#483).
        None if whole_init => match self.ref_bound_value_temp(init, is_skipped) {
          Some(tmp) => ResourceTy::Value(tmp),
          None => ResourceTy::Anonymous,
        },
        // `let (ref a, _) = (make(), make());`: `a` borrows the
        // temporary the literal is, though nothing was moved into it.
        None => {
          let tmp = self.tcx.hir_parent_iter(hirid)
            .find_map(|(_, n)| match n { rustc_hir::Node::LetStmt(l) => Some(l.hir_id), _ => None })
            .and_then(|l| self.literal_temps.get(&l));
          match tmp {
            Some(tmp) if !tmp.is_unbound_temporary() => ResourceTy::Value(tmp.clone()),
            _ => ResourceTy::Anonymous,
          }
        }
      };
      (lender, false)
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
  /// The variables its `break value`s out of an enclosing loop
  /// mention, sorted: what the break carries out of the loop.
  break_carries: Vec<String>,
}

/// The local variables `e` mentions, pushed onto `out`.
fn mentioned_locals<'tcx>(e: &'tcx Expr<'tcx>, out: &mut Vec<String>) {
  struct Locals<'a>(&'a mut Vec<String>);
  impl<'tcx> Visitor<'tcx> for Locals<'_> {
    fn visit_expr(&mut self, e: &'tcx Expr<'tcx>) {
      if let ExprKind::Path(rustc_hir::QPath::Resolved(None, path)) = &e.kind {
        if let Res::Local(_) = path.res {
          if let Some(seg) = path.segments.last() {
            self.0.push(seg.ident.to_string());
          }
        }
      }
      intravisit::walk_expr(self, e);
    }
  }
  Locals(out).visit_expr(e);
}

fn branch_leaves<'tcx>(tcx: rustc_middle::ty::TyCtxt<'tcx>, body: &'tcx Expr<'tcx>) -> BranchLeaves {
  struct Finder<'tcx> {
    tcx: rustc_middle::ty::TyCtxt<'tcx>,
    // Loops and labeled blocks inside the branch: a `break` /
    // `continue` targeting one of them doesn't leave the branch.
    inner: HashSet<HirId>,
    control: Vec<BranchExit>,
    loop_breaks: Vec<(HirId, HirId)>,
    break_carries: Vec<String>,
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
        ExprKind::Break(dest, value) => if let Ok(t) = dest.target_id {
          if !self.inner.contains(&t) {
            if let (Some(v), true) = (value, matches!(self.tcx.hir_node(t), rustc_hir::Node::Expr(Expr { kind: ExprKind::Loop(..), .. }))) {
              mentioned_locals(v, &mut self.break_carries);
            }
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
    tcx, inner: HashSet::new(), control: Vec::new(), loop_breaks: Vec::new(), break_carries: Vec::new(), panics: false,
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
  let mut break_carries = f.break_carries;
  break_carries.sort();
  break_carries.dedup();
  BranchLeaves { exits, loop_breaks: f.loop_breaks, ends_in_its_only_break, break_carries }
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
      // Moved only if the or-pattern alternative that binds from it
      // matched (#533): not on every path.
      ExternalEvent::OrAltMove { ro, tuple: None, .. } if outer(ro) => {
        if !out.iter().any(|(r, _)| r == ro) {
          out.push((ro.clone(), false));
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
  ) -> (bool, Vec<BranchExit>, Vec<String>) {
    if !self.expr_diverges(body) {
      return (false, Vec::new(), Vec::new());
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
    (true, leaves.exits, leaves.break_carries)
  }

  /// The names of the variables registered so far.
  fn rap_names(&self) -> HashSet<String> {
    #[expect(clippy::disallowed_methods, reason = "collected into a set; order is lost")]
    let names = self.raps.keys().cloned().collect();
    names
  }

  /// Pair what `break value` carries out of the loop `target` with
  /// the loop's receiver: `let r = loop { … }` (a binding or a
  /// pattern), as `let r = value;` would (#431, #452).
  /// `brk`: the `break` as written, kept for a receiver paired later.
  fn pair_break_value(&mut self, target: HirId, value: &'tcx Expr<'tcx>, brk: Option<String>) {
    if let Some((_, pat, scope, inside)) = self.break_pats.iter().rev().find(|(id, ..)| *id == target).cloned() {
      // The bindings live in the `let`'s scope, not the loop's.
      let loop_scope = std::mem::replace(&mut self.current_scope, scope);
      let loop_inside = std::mem::replace(&mut self.inside_branch, inside);
      self.bind_pat(pat, value, false);
      self.current_scope = loop_scope;
      self.inside_branch = loop_inside;
    } else if let Some((_, lhs, evt)) = self.break_binds.iter().rev().find(|(id, ..)| *id == target).cloned() {
      self.inherit_break_lenders(&lhs, target);
      self.match_rhs(lhs, value, evt);
    } else {
      self.loop_breaks.push((target, value, brk));
    }
  }

  /// Whether a `break` to the loop `target` is in a branch of an
  /// if/else or a match inside it, so it runs on some paths only.
  fn break_is_conditional(&self, target: HirId) -> bool {
    self.exit_frames.iter()
      .rposition(|f| matches!(f, ExitFrame::Loop { id, .. } if *id == target))
      .is_some_and(|i| self.exit_frames[i + 1..].iter().any(|f| matches!(f, ExitFrame::Branch { .. })))
  }

  /// How many `break value`s leave the loop `target`.
  fn loop_break_values(&self, target: HirId) -> usize {
    struct Count(HirId, usize);
    impl<'tcx> Visitor<'tcx> for Count {
      fn visit_expr(&mut self, e: &'tcx Expr<'tcx>) {
        if let ExprKind::Break(dest, Some(_)) = e.kind {
          if dest.target_id == Ok(self.0) {
            self.1 += 1;
          }
        }
        intravisit::walk_expr(self, e);
      }
    }
    let rustc_hir::Node::Expr(loop_expr) = self.tcx.hir_node(target) else { return 0 };
    let mut c = Count(target, 0);
    intravisit::walk_expr(&mut c, loop_expr);
    c.1
  }

  /// At the end of `loop_expr`: the values its `break`s in branches
  /// carried reach the loop's receiver, on the line the loop ends.
  /// With several `break`s that carry a value, only the one that ended
  /// the loop ran: each pairing is marked as such (#561).
  fn pair_deferred_breaks(&mut self, loop_expr: &'tcx Expr<'tcx>) {
    let (mine, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.deferred_breaks)
      .into_iter().partition(|(id, ..)| *id == loop_expr.hir_id);
    self.deferred_breaks = rest;
    match mine.as_slice() {
      [] => {}
      [(target, value, _)] => {
        let line = span_end_line(&loop_expr.span, &self.tcx);
        let names = self.break_out_names(loop_expr, &[*value]);
        let saved_line = self.line_override.replace(line);
        let saved_names = std::mem::replace(&mut self.break_out, names.clone());
        let waiting = self.loop_breaks.len();
        self.pair_break_value(*target, value, None);
        if self.loop_breaks.len() > waiting {
          // The receiver comes after the loop (`r = loop`, `f(loop
          // { … })`): pair it there, on this line.
          self.deferred_lines.push((*target, line, names));
        }
        self.line_override = saved_line;
        self.break_out = saved_names;
      }
      _ => {
        let target = loop_expr.hir_id;
        let values = mine.into_iter().map(|(_, v, t)| (v, t)).collect();
        self.pair_break_alternatives(loop_expr, values, |this, v| this.pair_break_value(target, v, None));
      }
    }
  }

  /// The variables declared inside `loop_expr` that `values` (what its
  /// `break`s carry) move out, if the body can reach its end: there
  /// they go out of scope on the iterations that don't break, and
  /// those drop them. Their Move is marked `break_out` (#431).
  fn break_out_names(&self, loop_expr: &'tcx Expr<'tcx>, values: &[&'tcx Expr<'tcx>]) -> Vec<String> {
    let ExprKind::Loop(body, ..) = loop_expr.kind else { return Vec::new() };
    if self.block_diverges(body) {
      return Vec::new();
    }
    let mut names: Vec<String> = Vec::new();
    for value in values {
      let parts: Vec<&Expr> = match value.kind {
        ExprKind::Tup(es) => es.iter().collect(),
        _ => vec![*value],
      };
      for part in parts {
        if let Some(l) = local_of_path(part) {
          if loop_expr.span.contains(self.tcx.hir_span(l)) {
            names.push(binding_name(&self.tcx, l));
          }
        }
      }
    }
    names
  }

  /// Whether a loop body never reaches its closing brace: some
  /// statement of it diverges (`if c { break k; } else { continue; }`).
  fn block_diverges(&self, b: &'tcx rustc_hir::Block<'tcx>) -> bool {
    b.expr.is_some_and(|t| self.expr_diverges(t))
      || b.stmts.iter().any(|st| matches!(st.kind,
        StmtKind::Expr(x) | StmtKind::Semi(x) if self.expr_diverges(x)))
  }

  /// The values several `break`s carry out of `loop_expr` (with each
  /// `break` as written) reach the loop's receiver through `pair`. Only
  /// the `break` that ended the loop ran, so each pairing is placed on
  /// the loop's last line and marked as one of them (#561).
  pub(crate) fn pair_break_alternatives(
    &mut self,
    loop_expr: &'tcx Expr<'tcx>,
    values: Vec<(&'tcx Expr<'tcx>, Option<String>)>,
    mut pair: impl FnMut(&mut Self, &'tcx Expr<'tcx>),
  ) {
    let line = self.line_override.replace(span_end_line(&loop_expr.span, &self.tcx));
    let all: Vec<&'tcx Expr<'tcx>> = values.iter().map(|(v, _)| *v).collect();
    let names = self.break_out_names(loop_expr, &all);
    let saved_names = std::mem::replace(&mut self.break_out, names);
    let first = local_of_path(values[0].0);
    if first.is_some() && values.iter().all(|(v, _)| local_of_path(v) == first) {
      // `if c { break a; } else { break a; }`: a moves to the receiver
      // whichever ran, so that is one plain move.
      pair(self, values[0].0);
    } else {
      for (value, brk) in &values {
        let in_loop = local_of_path(value)
          .is_some_and(|l| loop_expr.span.contains(self.tcx.hir_span(l)));
        self.break_alt = Some(BreakAlt { brk: brk.clone(), in_loop });
        pair(self, value);
        self.break_alt = None;
      }
    }
    self.line_override = line;
    self.break_out = saved_names;
  }

  /// `brk` (a `break` expression) as written, for a tooltip: `None`
  /// when it is too long or spans lines.
  fn break_text(&self, brk: &Expr) -> Option<String> {
    if brk.span.from_expansion() {
      return None;
    }
    let text = self.tcx.sess.source_map().span_to_snippet(brk.span).ok()?;
    (text.chars().count() <= 40 && !text.contains('\n')).then_some(text)
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
        ro: m.rap.clone(), definite,
        at: if block.is_some() { MaybeMovedAt::LabeledBlock } else { MaybeMovedAt::Loop },
        id: *self.unique_id,
      });
      *self.unique_id += 1;
    }
  }
}
