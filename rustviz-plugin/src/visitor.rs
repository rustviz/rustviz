//! Most of the work happens here, if you want to learn more about the visitor then look here:  
//! https://doc.rust-lang.org/beta/nightly-rustc/rustc_hir/intravisit/trait.Visitor.html
//! Essentially we recursively traverse (walk) the HIR, visiting statements, expressions, etc
//! See ExprKind at : https://doc.rust-lang.org/stable/nightly-rustc/rustc_hir/hir/enum.ExprKind.html
//! See StmtKind at : https://doc.rust-lang.org/stable/nightly-rustc/rustc_hir/hir/enum.StmtKind.html
//! See tcx: https://doc.rust-lang.org/nightly/nightly-rustc/rustc_middle/ty/struct.TyCtxt.html

use log::warn;
use rustc_hir::{StmtKind, Stmt, Expr, ExprKind, PathSegment, UnOp, Param, QPath, Pat, PatKind, BindingMode, HirId, Mutability, LetStmt, Block, def::*};
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
pub(crate) fn pat_is_irrefutable(pat: &Pat, typeck: &rustc_middle::ty::TypeckResults) -> bool {
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
/// whole chain has been walked. `straight`: the part runs every
/// effect in it whenever it runs (no `&&`, `||`, `if`, `match` or `?`
/// inside it), so one after no part that can fail happens on every path
/// out of the condition (#968).
struct ChainEffect {
  kind: ChainEffectKind,
  checks_before: usize,
  line: usize,
  straight: bool,
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
  /// `rap`, a reference, is given a new one that borrows `lender`
  /// (`r = &s`, #603; `r = first(&v)`, `r = longest(&a, &b)`, #1068):
  /// `old` / `new` are its loans before and after the part, `rhs` the
  /// assigned expression (see `assigned_rhs`).
  AssignRef { rap: ResourceAccessPoint, lender: ResourceAccessPoint, mutable: bool, old: Option<RefData>, new: Option<RefData>,
    rhs: Option<AssignedRhs> },
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
  /// The last line of the body the chain's bindings are in scope in
  /// (see `register_iflet_let_bindings`).
  body_end: usize,
  /// The conditional is inside a loop's body (see `chain_fail_moves`).
  in_loop: bool,
}

/// A variable that the condition of an `if` let-chain moves before a
/// part of it that can still fail (#281): on the path into the `else`
/// it may have been moved. See `MaybeMovedAt::ChainFail`.
#[derive(Clone)]
struct ChainFailMove {
  rap: ResourceAccessPoint,
  definite: bool,
  to: Vec<String>,
  dropped: Vec<String>,
  how: ChainFailHow,
  /// Index in `ChainWalk::effects` of the first and last move of it.
  seq: (usize, usize),
}

/// A variable that the condition of an `if` let-chain assigns a new
/// value before a part of it that can still fail (#579): on the path
/// into the `else` it holds the old value or the new one. See
/// `ExternalEvent::ChainFailAssign`.
#[derive(Clone)]
struct ChainFailAssign {
  rap: ResourceAccessPoint,
  place: Option<String>,
  drops_old: bool,
  /// It held a value before the assignment (a place always does).
  held: bool,
  /// Index in `ChainWalk::effects` of the last assignment to it.
  seq: usize,
}

/// A variable that the condition of an `if` let-chain lends before a
/// part of it that can still fail (#688): the else branch starts with
/// that said. See `ExternalEvent::ChainFailUse`.
#[derive(Clone)]
struct ChainFailUse {
  rap: ResourceAccessPoint,
  uses: Vec<ChainUse>,
  definite: bool,
}

/// A reference that the condition of an `if` let-chain (or a match
/// guard) assigns a new one, `&s`, before a part that can still fail
/// (#603): on the failing path it points to `s` or to `t`, which it
/// pointed to before, and the borrow checker treats both as borrowed
/// while it's live. See `ExternalEvent::OrAltBorrow`'s `chain`. The new
/// one can be a call's result tied to one or several arguments, and the
/// old one too (`first(&v)`, `longest(&a, &b)`; #1068): each lender has
/// its own loan.
#[derive(Clone)]
struct ChainFailRef {
  rap: ResourceAccessPoint,
  /// The lenders of the new and of the old reference, each with its
  /// loan: (index in rustc's borrow set, last line in scope).
  new: Vec<(ResourceAccessPoint, (usize, usize))>,
  old: Vec<(ResourceAccessPoint, (usize, usize))>,
  mutable: bool,
  /// The assigned expression as written, and whether the new / old
  /// reference came from something other than `&x` of a variable (a
  /// call's result, `&v[0]`): it points into its lenders.
  rhs: String,
  new_via_call: bool,
  old_via_call: bool,
  /// The function the assigned expression calls, which its lenders are
  /// lent to.
  callee: Option<String>,
  /// What the old reference points to when that has no column (a
  /// string literal, a static); `old` is then empty.
  old_static: Option<StaticLender>,
  /// The new reference's loan as `borrow_map` holds it.
  new_id: (LoanId, ResourceTy),
  /// The condition's writes through `rap` (for "may have been changed").
  changes: bool,
  /// The line of the assignment.
  line: usize,
}

/// The right-hand side of `r = …` in a let-chain part (see
/// `ExprVisitor::assigned_rhs`).
#[derive(Clone)]
struct AssignedRhs {
  /// As written, whitespace collapsed.
  text: String,
  span: rustc_span::Span,
  /// `&x` / `&mut x` of a variable.
  direct: bool,
  /// The function or method it calls (`first(&v)`, `s.as_str()`).
  callee: Option<String>,
}

/// What `chain_fail_moves` finds: see there.
#[derive(Default, Clone)]
struct ChainFail {
  moves: Vec<ChainFailMove>,
  assigns: Vec<ChainFailAssign>,
  uses: Vec<ChainFailUse>,
  refs: Vec<ChainFailRef>,
}

impl<'a, 'tcx> ExprVisitor<'a, 'tcx> {
  fn arm_span(&self, span: rustc_span::Span) -> Option<crate::facts::table::ArmSpan> {
    if span.from_expansion() { return None; }
    let sm = self.tcx.sess.source_map();
    let pos = |p| {
      let loc = sm.lookup_char_pos(p);
      (loc.line, loc.col.0 + 1)
    };
    Some(crate::facts::table::ArmSpan { start: pos(span.lo()), end: pos(span.hi()) })
  }

  /// Give each `static` the body names a column (#690), after the
  /// parameters' ones. Like a parameter, it exists before the function
  /// runs (its dot is on the signature's line); unlike one, it doesn't
  /// come from the caller and isn't dropped at the end.
  fn register_statics(&mut self, body: &rustc_hir::Body<'tcx>) {
    let owner = body.value.hir_id.owner;
    let line_num = self.current_fn_start;
    let mut statics = statics_in(body.value);
    // A static only a called closure writes (#1203) gets a column too:
    // the call writes it.
    for id in statics_written_by_called_closures(&self.tcx, body.value) {
      if !statics.contains(&id) {
        statics.push(id);
      }
    }
    for def_id in statics {
      let name = self.tcx.def_path_str(def_id);
      if self.raps.contains_key(&name) {
        continue;
      }
      let ty = self.tcx.type_of(def_id).instantiate_identity().skip_normalization();
      let is_mut = matches!(self.tcx.def_kind(def_id), DefKind::Static { mutability: Mutability::Mut, .. });
      self.add_owner(name.clone(), is_mut, self.ty_is_copy(ty, owner), self.ty_is_plain(ty, owner), self.ty_has_interior_mut(ty, owner), self.current_scope, true);
      self.mark_owner_type(&name, ty);
      let Some(rd) = self.raps.get_mut(&name) else { continue };
      if let ResourceAccessPoint::Owner(o) = &mut rd.rap {
        o.is_static = true;
      }
      let rap = rd.rap.clone();
      self.add_external_event(line_num, ExternalEvent::InitRefParam { param: rap, id: *self.unique_id, own_param: None, in_pattern: None, rest: None });
      *self.unique_id += 1;
    }
  }

  /// `e` is the value the function returns (its tail expression, or
  /// the operand of a `return`): record it moving (or, for a Copy
  /// type or a shared reference, being copied) to the caller.
  fn return_to_caller(&mut self, e: &'tcx Expr<'tcx>) {
    // `return loop { … break v; }`, or such a loop as the tail: what
    // the `break` that ends it carries goes to the caller (#1171), as it
    // would go into the argument of `f(loop { … })` (`match_arg`).
    if let ExprKind::Loop(_, _, rustc_hir::LoopSource::Loop, _) = e.kind {
      if !e.span.from_expansion() {
        let seen = self.take_loop_breaks(e);
        let deferred = self.deferred_lines.iter().any(|(id, ..)| *id == e.hir_id);
        let saved = self.enter_deferred(e.hir_id);
        // A single `break` that isn't in a branch: the value leaves
        // where the loop ends (not a variable the loop declares, which
        // goes out of scope there after its move).
        let end_line = (!deferred && seen.len() == 1 && !self.mentions_local_declared_in(seen[0].0, e.span))
          .then(|| span_end_line(&e.span, &self.tcx));
        let saved_line = end_line.map(|l| self.line_override.replace(l));
        if seen.len() > 1 {
          // Several: only the `break` that ended the loop ran (#561).
          self.pair_break_alternatives(e, seen, |this, v| this.return_to_caller(v));
        } else {
          for (value, _) in seen {
            self.return_to_caller(value);
          }
        }
        if let Some(l) = saved_line {
          self.line_override = l;
        }
        self.leave_deferred(saved);
        return;
      }
    }
    // A labeled block as the returned value (`'blk: { if c { break 'blk
    // k; } t }`, #1288): what the `break` that ended it carries, or its
    // last expression, goes to the caller, as into a `let` (#1134).
    if let ExprKind::Block(_, Some(_)) = e.kind {
      if !e.span.from_expansion() {
        let values = self.labeled_block_values(e);
        if values.len() > 1 {
          let saved = self.enter_deferred(e.hir_id);
          self.pair_break_alternatives(e, values, |this, v| this.return_to_caller(v));
          self.leave_deferred(saved);
          return;
        }
      }
    }
    let tycheck_results = self.tcx.typeck(e.hir_id.owner);
    let lhs_ty = tycheck_results.node_type(e.hir_id);
    let is_copyable = self.ty_is_copy(lhs_ty, e.hir_id.owner);
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
    // `&self.s` returned: a reference reborrowed through `self` goes to
    // the caller, and `self`'s borrow goes on with it (#784). Drawn as a
    // copy of the reference for `&mut` too: `self` isn't moved, it is
    // reborrowed.
    let reborrow = if lhs_ty.is_ref() { self.reborrow_through_ref(e) } else { None };
    // `v.get_mut(0).unwrap()`: the borrow the call takes through `v`
    // goes on in the returned reference, so `get_mut` doesn't give it
    // back here (#1078).
    if reborrow.is_some() {
      if let Some(r) = self.call_reborrows_through(e) {
        self.mark_borrow_kept_by_result(e, &[ResourceTy::Value(r.through)]);
      }
    }
    let evt = if reborrow.is_some() { Evt::Copy } else { evt };
    // `v[0]` / `ORIGIN.x` returned: a copy of one element or field, on
    // the holder's column (#1052). `&PRIMES` / `&PRIMES[0]`: a reference
    // to (part of) a static goes to the caller (#1053).
    // Like a reborrow, a `&mut STATIC` returned is drawn as a copy of
    // the reference: the static isn't moved.
    // `unsafe { &mut TOTAL }` / `unsafe { GRID[1][0] }`: the block only
    // wraps the place (#1065).
    let mut place_e = e;
    while let ExprKind::Block(Block { stmts: [], expr: Some(inner), .. }, None) = place_e.kind {
      if place_e.span.from_expansion() {
        break;
      }
      place_e = inner;
    }
    let part = if deref.is_none() && reborrow.is_none()
      && (matches!(evt, Evt::Copy) || matches!(place_e.kind, ExprKind::AddrOf(..))) { self.returned_part(place_e) } else { None };
    let evt = if part.is_some() { Evt::Copy } else { evt };
    // `*b` returned with b a Box: what the Box holds goes to the caller;
    // the Box itself stays with b, which frees its heap allocation at
    // the end of the function (#1191).
    let box_contents = if deref.is_none() && reborrow.is_none() && part.is_none() { self.box_contents_of(place_e) } else { None };
    let from_ro = if let Some(rty) = deref.or(reborrow).or(part).or(box_contents) { rty } else { match fetch_rap(e, &self.tcx, &self.raps) {
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
    // Any other reference to (part of) a static returned (one that
    // `returned_part` doesn't name): not a copy of it (#690). Not drawn.
    if lhs_ty.is_ref() && !matches!(e.kind, ExprKind::Path(..))
      && matches!(&from_ro, ResourceTy::Value(r) if r.is_static()) {
      return;
    }

    let line_num = expr_to_line(e, &self.tcx);
    // A closure literal as the returned value: what it captures by
    // value goes to the caller inside it (#760), so those variables
    // are moved (or copied) out, not dropped at the end of the fn.
    if let ExprKind::Closure(c) = e.kind {
      if !e.span.from_expansion() && self.match_closure_arg(c, e, &to_ro, "") {
        return;
      }
    }
    // `Some(a)` as the returned value: what it wraps goes to the caller
    // (a constructor never receives anything, #768).
    let ctor_value = !e.span.from_expansion()
      && matches!(e.kind, ExprKind::Call(callee, args) if is_ctor_path(callee, tycheck_results)
        && args.iter().any(|a| ctor_part_carries_value(self.tcx, a, tycheck_results)
          // `Some(*b)`: the Box's contents (#1191).
          || self.box_contents_of(a).is_some()
          // `Some(B { name: s })`: what the literal is built from (#1143).
          || self.is_nested_literal(a)
          // `Some(x.trim())`: a reborrow through a reference (#1340).
          || (tycheck_results.node_type(a.hir_id).is_ref() && self.reborrow_through_ref(a).is_some())))
      && !self.is_copy_literal_value(e);
    if (matches!(e.kind, ExprKind::Struct(..) | ExprKind::Tup(..) | ExprKind::Array(..)) && !e.span.from_expansion())
      || ctor_value
      || vec_macro_elems(self.tcx, e).is_some() {
      // `User { email, .. }` / `(s, 1)` / `[s, t]` as the returned value: the
      // literal's field initialisers move/copy their sources into
      // the returned value.
      self.match_rhs(to_ro, e, evt);
    } else if self.is_returned_call_result(e) {
      // A bare call's result (`make()`, `return make();`) goes to the
      // caller from the function that returned it (#792).
      self.match_rhs(to_ro, e, evt);
    } else {
      self.add_ev(line_num, evt, to_ro, from_ro, false);
    }
  }

  /// On a path of a returned `if` / `match` that yields the reference
  /// parameter `x` (or a literal), every other reference parameter
  /// whose lifetime the signature ties to the result isn't returned,
  /// but its borrow doesn't end here: it lasts as long as the result
  /// in the caller (`fn pick<'a>(c: bool, x: &'a String, y: &'a String)
  /// -> &'a String`). A dot on its column says so (#775). A parameter
  /// with a lifetime of its own gets none: its borrow really ends.
  fn note_tied_ref_params(&mut self, e: &'tcx Expr<'tcx>) {
    use rustc_middle::ty;
    let owner = e.hir_id.owner.def_id;
    if !self.tail_plain.contains(&e.hir_id) || self.tcx.hir_enclosing_body_owner(e.hir_id) != owner || !self.tcx.def_kind(owner).is_fn_like() {
      return;
    }
    let returned: Option<String> = match e.kind {
      ExprKind::Path(QPath::Resolved(None, path)) if path.segments.len() == 1 => Some(path.segments[0].ident.name.to_string()),
      ExprKind::Lit(_) => None,
      // `&self.s`: the parameter it reborrows through is the one returned.
      _ => match self.reborrow_through_ref(e) {
        Some(r) => Some(r.deref_rap().map(|r| r.name().to_string()).unwrap_or_default()),
        None => return,
      },
    };
    let sig = self.tcx.liberate_late_bound_regions(owner.to_def_id(), self.tcx.fn_sig(owner).instantiate_identity().skip_norm_wip());
    let out_regions: Vec<ty::Region<'tcx>> = sig.output().walk()
      .filter_map(|g| match g.kind() { ty::GenericArgKind::Lifetime(r) if !r.is_static() => Some(r), _ => None })
      .collect();
    if out_regions.is_empty() {
      return;
    }
    let body = self.tcx.hir_body_owned_by(owner);
    // A returned parameter is itself one the result borrows through.
    let mut tied: Vec<(String, String)> = Vec::new();
    for (param, input) in body.params.iter().zip(sig.inputs().iter()) {
      let PatKind::Binding(_, _, ident, None) = param.pat.kind else { continue };
      let ty::Ref(r, _, _) = input.kind() else { continue };
      let name = format!("{r}");
      if !out_regions.contains(r)
        || name == "'_"
        || !name.starts_with('\'')
        || !name[1..].chars().all(|c| c.is_alphanumeric() || c == '_') {
        continue;
      }
      tied.push((ident.name.to_string(), name));
    }
    if let Some(r) = &returned {
      if !tied.iter().any(|(n, _)| n == r) {
        return;
      }
    }
    let line = expr_to_line(e, &self.tcx);
    for (name, lifetime) in tied {
      if returned.as_deref() == Some(name.as_str()) {
        continue;
      }
      let Some(rd) = self.raps.get(&name) else { continue };
      let ro = rd.rap.to_owned();
      let id = *self.unique_id;
      *self.unique_id += 1;
      self.add_external_event(line, ExternalEvent::TiedParamNote { ro, lifetime, id });
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

  /// The `if` / `else` or `match` whose branches yield the value `e`:
  /// `e` itself, or the one a constructor wraps (`Some(match o { .. })`,
  /// #1174), which takes nothing itself (#768): what a branch yields
  /// goes where the built value goes. None for any other `e`.
  pub(crate) fn conditional_value(&self, e: &'tcx Expr<'tcx>) -> Option<&'tcx Expr<'tcx>> {
    if e.span.from_expansion() {
      return None;
    }
    match e.kind {
      ExprKind::If(_, _, Some(_)) | ExprKind::Match(_, _, rustc_hir::MatchSource::Normal) => Some(e),
      ExprKind::Call(callee, [inner]) if is_ctor_path(callee, self.tcx.typeck(e.hir_id.owner)) => self.conditional_value(inner),
      _ => None,
    }
  }

  /// `e` is a tuple or struct literal one of whose parts is an `if` /
  /// `else` or a `match`, or a constructor wrapping one (`(if c { d }
  /// else { e }, 1)`, `W { s: if … }`, #1256; `(Some(if …), 1)`, #1267):
  /// the part's value comes from each branch, in that branch.
  fn literal_has_conditional_part(&self, e: &'tcx Expr<'tcx>) -> bool {
    !e.span.from_expansion() && match e.kind {
      ExprKind::Tup(parts) => parts.iter().any(|p| self.conditional_value(p).is_some()),
      ExprKind::Struct(_, fields, _) => fields.iter().any(|f| self.conditional_value(f.expr).is_some()),
      _ => false,
    }
  }

  /// `e` is returned to the caller (the function's tail, or what a
  /// `return` carries). If it is an `if` / `else` or a `match` (or a
  /// constructor wrapping one, `conditional_value`), mark
  /// the value each branch yields, which goes to the caller inside its
  /// branch, where `visit_expr` reaches it (#674): on the paths
  /// through the other branches it stays where it is. False for any
  /// other `e`, which `return_to_caller` sends to the caller as a whole.
  fn mark_branch_returns(&mut self, e: &'tcx Expr<'tcx>) -> bool {
    let Some(e) = self.conditional_value(e) else { return false };
    let mut values = Vec::new();
    self.returned_values(e, &mut values);
    self.tail_values.extend(values.iter().map(|v| v.hir_id));
    // A branch that reborrows a parameter (`&self.s`) hands its borrow
    // to the caller on that path (#784), like one that returns it.
    if values.iter().all(|v| matches!(v.kind, ExprKind::Lit(_) | ExprKind::Path(QPath::Resolved(None, _)))
      || self.reborrow_through_ref(v).is_some()) {
      self.tail_plain.extend(values.iter().map(|v| v.hir_id));
    }
    true
  }

  /// Visits a call's arguments, in order, and passes each to `fn_name`
  /// (`match_arg`).
  ///
  /// An argument written as an `if` / `else` or a `match` passes the
  /// value each branch yields, inside its branch, where `visit_expr`
  /// reaches it (#1174), as for a returned one (`mark_branch_returns`,
  /// #674): `take(if c { a } else { b })` moves `a` on one path and `b`
  /// on the other, and an arm's binding `x` in `take(match o { Some(x)
  /// => x, .. })` is passed to the call, not dropped at the arm's end.
  ///
  /// `defer`: a `?` follows an argument (`take(s, f()?)`, `pair.eat(f()?)`
  /// with `eat(self)`). Every argument is evaluated before the call
  /// takes any: if the `?` returns early, the call never happens, and
  /// what the earlier arguments (and a by-value receiver) hold is
  /// dropped as the function returns (#1195). So all are visited (the
  /// `?` among them, which says what it drops) before `before_pass`
  /// runs and any is passed.
  pub fn visit_call_args(&mut self, args: &'tcx [Expr<'tcx>], fn_name: &str, defer: bool, manually_takes: bool, before_pass: impl FnOnce(&mut Self)) {
    let in_branches: Vec<bool> = args.iter().map(|a| self.mark_branch_args(a, fn_name, manually_takes)).collect();
    if defer {
      for arg in args {
        self.visit_expr(arg);
      }
    }
    before_pass(self);
    for (arg, branched) in args.iter().zip(&in_branches) {
      if !defer {
        self.visit_expr(arg);
      }
      if !branched {
        self.match_call_arg(arg, fn_name.to_owned(), manually_takes);
      }
    }
  }

  /// `arg`, a call's argument, is an `if` / `else` or a `match`: mark
  /// the value each branch yields to be passed to `fn_name` where it is
  /// visited (`arg_values`). False for any other `arg`.
  fn mark_branch_args(&mut self, arg: &'tcx Expr<'tcx>, fn_name: &str, manually_takes: bool) -> bool {
    // A tuple built in the argument list with an `if` / `match` element
    // (`take((if c { d } else { e }, 1))`, #1267): the element's value
    // goes into the call with the tuple, inside its branch; `match_arg`
    // passes the other elements.
    if let ExprKind::Tup(parts) = arg.kind {
      if !arg.span.from_expansion() {
        for part in parts.iter() {
          self.mark_branch_args(part, fn_name, manually_takes);
        }
      }
      return false;
    }
    let Some(arg) = self.conditional_value(arg) else { return false };
    let mut values = Vec::new();
    self.returned_values(arg, &mut values);
    for v in values {
      self.arg_values.insert(v.hir_id, (fn_name.to_owned(), manually_takes));
    }
    true
  }

  /// Visit a match-arm guard (`pat if cond =>`), walked like an `if`
  /// let-chain (`visit_if_let_chain`): `if let` parts register their
  /// bindings into `guard_decls` (the caller scopes them to the arm),
  /// and what each part does that lasts to the variables in `outer` is
  /// recorded.
  ///
  /// Every event a guard produces is drawn in its arm's column: if that
  /// arm is taken, its guard ran first. But when the guard is false,
  /// matching goes on with the later arms in the state the guard left
  /// behind, and the borrow checker counts every later arm as one that
  /// may run after it, whatever its pattern (`Some(_) if consume(s) =>
  /// …, None => s.len()` is E0382). So what a later arm starts with
  /// (`chain_fail_moves`) is returned, for the caller to put at the top
  /// of each later arm (#247, #248).
  ///
  /// `arm_end`: the arm's last line, where the guard's bindings go out
  /// of scope.
  fn visit_match_guard(
    &mut self,
    guard: &'tcx Expr<'tcx>,
    outer: &HashSet<ResourceAccessPoint>,
    guard_decls: &mut HashSet<ResourceAccessPoint>,
    arm_end: usize,
  ) -> (ChainFail, FailPath) {
    let chain_lo = self.preprocessed_events.len();
    // Nothing of a guard is drawn before the arms split, so every
    // part's effects count, the first one's too.
    let mut walk = ChainWalk { first_lo: Some(*self.unique_id), body_end: arm_end, in_loop: self.in_loop_body(guard.hir_id), ..ChainWalk::default() };
    self.visit_if_let_chain(guard, outer, guard_decls, &mut walk);
    let path = FailPath::Guard(expr_to_line(guard, &self.tcx));
    (self.chain_fail_moves(&walk, chain_lo, path), path)
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
  /// else branch starts with (#281). The `while let` caller keeps only
  /// the moves, which it draws where the loop ends (#968); it sets
  /// `walk.first_lo` first, so the first part's are recorded too. A
  /// match guard is walked the same way, with `first_lo` set
  /// beforehand: see `visit_match_guard`.
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
        let loans = self.outer_loans(outer);
        self.visit_expr(let_expr.init);
        if walk.first_lo.is_some() {
          self.record_chain_effects(mark, outer, expr, walk, false, &loans);
        }
        walk.first_lo.get_or_insert(*self.unique_id);
        let typeck = self.tcx.typeck(let_expr.pat.hir_id.owner);
        // A literal scrutinee (`Some(o)`, `(a, 1)`) moves its
        // variables into its temporary before the pattern is checked
        // (#949): the check counts after the bindings' effects.
        let check = !self.let_always_matches(let_expr.pat, let_expr.init);
        let check_after = check && !self.tuple_scrutinee_moves(let_expr.init).is_empty();
        if check && !check_after {
          walk.checks += 1;
        }
        // By column name: a binding that shadows a variable is `a₂`.
        let_expr.pat.each_binding(|_, hir_id, _, _| {
          if self.needs_drop(typeck.node_type(hir_id), let_expr.pat.hir_id.owner) {
            walk.drop_bindings.insert(binding_name(&self.tcx, hir_id));
          }
        });
        let mark = self.preprocessed_events.len();
        let loans = self.outer_loans(outer);
        let bound = self.register_iflet_let_bindings(let_expr, None, walk.body_end).0;
        self.record_chain_effects(mark, outer, expr, walk, true, &loans);
        if check_after {
          walk.checks += 1;
        }
        decls.extend(bound);
      }
      _ => {
        let mark = self.preprocessed_events.len();
        let loans = self.outer_loans(outer);
        self.visit_expr(expr);
        if walk.first_lo.is_some() {
          self.record_chain_effects(mark, outer, expr, walk, false, &loans);
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
    loans: &HashMap<String, RefData>,
  ) {
    let line = expr_to_line(part, &self.tcx);
    let is_outer = |r: &ResourceTy| r.extract_rap().is_some_and(|rap| outer.contains(rap));
    let checks_before = walk.checks;
    let straight = runs_straight(part);
    let mut push = |kind| walk.effects.push(ChainEffect { kind, checks_before, line, straight });
    let held = |rap: &ResourceAccessPoint| self.rap_ownership_before(mark, rap.name()) == Ownership::Held;
    // The variables a literal scrutinee moves into its temporary, which a
    // pattern that can fail takes apart (#949), and the temporary's text.
    let (literal_vars, literal_text) = match part.kind {
      ExprKind::Let(l) if binding && !self.let_always_matches(l.pat, l.init) => (
        self.tuple_scrutinee_moves(l.init),
        match self.tuple_scrutinee_target(l.init) {
          ResourceTy::Temporary { tuple: Some(t), .. } => t,
          _ => String::new(),
        },
      ),
      _ => (Vec::new(), String::new()),
    };
    // A move into a literal scrutinee's temporary (what `_` / `..`
    // leave of `(f, s)` in `let (x, _) = (f, s)`, #971) is dropped with
    // that temporary when the condition fails, whatever the pattern.
    let how_moved = |rap: &ResourceAccessPoint, to: &ResourceTy| match to {
      ResourceTy::Temporary { tuple: Some(t), .. } if binding => ChainFailHow::Literal(t.clone()),
      _ if literal_vars.contains(rap) => ChainFailHow::Literal(literal_text.clone()),
      _ => ChainFailHow::Move,
    };
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
          push(ChainEffectKind::Move { rap: rap.clone(), to: to.clone(), binding, how: how_moved(rap, to) }),
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
          ResourceTy::Index(rap, place, pw) | ResourceTy::DerefField(rap, place, pw, _) =>
            push(ChainEffectKind::Assign { rap: rap.clone(), place: Some(place.clone()),
              drops_old: matches!(pw, PlaceWrite::AssignDropsOld), held: true }),
          _ => {}
        },
        // `r = &s` (#603).
        ExternalEvent::StaticBorrow { from: from @ ResourceTy::Value(lender), to: ResourceTy::Value(rap), .. }
        | ExternalEvent::MutableBorrow { from: from @ ResourceTy::Value(lender), to: ResourceTy::Value(rap), .. }
          if outer.contains(rap) && is_outer(from) =>
          push(ChainEffectKind::AssignRef {
            rap: rap.clone(), lender: lender.clone(), mutable: matches!(ev, ExternalEvent::MutableBorrow { .. }),
            old: loans.get(rap.name()).cloned(), new: self.borrow_map.get(rap.name()).cloned(),
            rhs: self.assigned_rhs(part, rap.name()),
          }),
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
  /// it holding its old value or the new one (#579). An assignment to a
  /// variable that may hold no value before it leaves it holding the new
  /// value or nothing, a maybe-moved state (`ChainFailHow::AssignNoValue`);
  /// a variable both moved and assigned is maybe-moved, worded after
  /// which came last (`AssignThenMove`, `MoveThenAssign`; #603). A new
  /// reference `r = &s` leaves `r` pointing to `s` or to what it pointed
  /// to before, both borrowed while `r` is live (`ChainFailRef`, #603).
  /// Other lasting effects that come before a part that can fail are
  /// refused, as for match guards (see `ChainEffectKind::Other`, and
  /// the forms of `r = …` `ChainFailRef` doesn't cover).
  /// A write through an outer reference, into an element or a field
  /// reached through one (`*r = a`, `v[i] = a`, `r.f = a`) is an
  /// assignment to that place, drawn on that variable's column (#603).
  ///
  /// A borrow that ends inside the condition (lent to a call, or to a
  /// binding of the chain) also happened on the path into the else
  /// branch, which starts with that said (#688), unless the variable
  /// is moved or assigned there too (that dot is the one that matters).
  ///
  /// A match guard is walked the same way (#247): `path` is then
  /// `FailPath::Guard`, and what this finds is what every later arm
  /// starts with. Nothing is drawn before a guard's split, so all of
  /// its parts count; and none of it is definite, since a later arm is
  /// also reached when the guarded arm's pattern doesn't match.
  fn chain_fail_moves(&self, walk: &ChainWalk, chain_lo: usize, path: FailPath) -> ChainFail {
    let mut out: Vec<ChainFailMove> = Vec::new();
    let mut assigns: Vec<ChainFailAssign> = Vec::new();
    let mut uses: Vec<ChainFailUse> = Vec::new();
    let mut refs: Vec<ChainFailRef> = Vec::new();
    let guard = matches!(path, FailPath::Guard(_));
    let later = match path {
      FailPath::Else => "the condition is false and the else branch runs",
      FailPath::NoElse => "the condition is false and the body is skipped",
      FailPath::Guard(_) => "the guard is false and a later arm runs",
    };
    let (what_kind, outside) = if guard { ("match guards", "the guard") } else { ("`if let` chains", "the condition") };
    for (seq, eff) in walk.effects.iter().enumerate().filter(|(_, e)| e.checks_before < walk.checks) {
      match &eff.kind {
        ChainEffectKind::Other { name, what, lasts } => {
          panic!("{} that {} (`{}`, line {}) aren't supported yet: {} even when {}",
            what_kind, what.replace("the condition", outside), name, eff.line, lasts, later);
        }
        ChainEffectKind::Assign { rap, place, drops_old, held } => {
          match assigns.iter_mut().find(|a| a.rap == *rap && a.place == *place) {
            Some(a) => {
              a.drops_old |= drops_old;
              a.seq = seq;
            }
            None => assigns.push(ChainFailAssign { rap: rap.clone(), place: place.clone(), drops_old: *drops_old, held: *held, seq }),
          }
        }
        ChainEffectKind::Move { rap, to, binding, how } => {
          // A `let`'s bindings with only irrefutable parts up to them:
          // every way into the else branch passes the move. A move by
          // an expression may sit inside a conditional of its own
          // (`a || take(s)`), so it's only "may have been", unless its
          // part has none (#968).
          let definite = (*binding || eff.straight) && eff.checks_before == 0 && !guard;
          let i = match out.iter().position(|m| m.rap == *rap) {
            Some(i) => i,
            None => {
              out.push(ChainFailMove { rap: rap.clone(), definite: false, to: Vec::new(), dropped: Vec::new(), how: how.clone(),
                seq: (seq, seq) });
              out.len() - 1
            }
          };
          let m = &mut out[i];
          m.definite |= definite;
          m.seq.1 = seq;
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
        ChainEffectKind::AssignRef { rap, lender, mutable, old, new, rhs } => {
          let refuse = || -> ! {
            panic!("{} that {} (`{}`, line {}) aren't supported yet: the assignment lasts even when {}",
              what_kind, CHAIN_ASSIGN_REF.replace("the condition", outside), rap.name(), eff.line, later)
          };
          // Drawn when the reference held a loan of one or more plain
          // variables before (`&t`, `longest(&a, &b)`), and the new one
          // borrows others: `&s` (#603), or a call's result tied to its
          // arguments (`first(&v)`, #1068). Not in a loop's body, where
          // the loans can come back round to the condition: where they
          // end isn't worked out for that.
          let (Some(old), Some(new), Some(rhs)) = (old, new, rhs) else { refuse() };
          if walk.in_loop {
            refuse();
          }
          let lenders = |rd: &RefData| -> Option<Vec<ResourceAccessPoint>> {
            rd.all_lenders().into_iter().map(|l| match l {
              ResourceTy::Value(t) if !t.is_fn() => Some(t),
              _ => None,
            }).collect()
          };
          // An old reference to data with no column (`let mut r: &str =
          // "lit";`) is said by what it points to.
          let old_static = match (&old.lender, &old.static_lender) {
            (ResourceTy::Anonymous, Some(sl @ (StaticLender::Literal { .. } | StaticLender::Static(_)
              | StaticLender::PointeeOfStatic(_) | StaticLender::PointeeOfConst(_) | StaticLender::PromotedConst(_))))
              if old.co_lenders.is_empty() => Some(sl.clone()),
            _ => None,
          };
          let old_l = if old_static.is_some() { Some(Vec::new()) } else { lenders(old) };
          let (Some(new_l), Some(old_l)) = (lenders(&new), old_l) else { refuse() };
          if !new_l.contains(lender) || new_l.iter().any(|l| old_l.contains(l)) {
            refuse();
          }
          let Some(new_id) = new.loan().map(|id| (id, new.lender.clone())) else { refuse() };
          // A call tied to several arguments makes one event per lender.
          if let Some(r) = refs.iter().find(|r| r.rap == *rap) {
            if r.new_id != new_id {
              refuse();
            }
            continue;
          }
          let borrows = self.gather_borrow_data(self.bwf);
          let with_loans = |rd: &RefData, ls: Vec<ResourceAccessPoint>, within: Option<rustc_span::Span>| {
            ls.into_iter().map(|l| Some((self.loan_of_lender(&borrows, rd, &l, within)?, l)))
              .map(|o| o.map(|(loan, l)| (l, loan)))
              .collect::<Option<Vec<_>>>()
          };
          let (Some(new_loans), Some(old_loans)) = (with_loans(&new, new_l, Some(rhs.span)), with_loans(old, old_l, None)) else { refuse() };
          refs.push(ChainFailRef {
            rap: rap.clone(), new: new_loans, old: old_loans, mutable: *mutable,
            rhs: rhs.text.clone(), new_via_call: !rhs.direct, old_via_call: old.borrow_span.is_none(), callee: rhs.callee.clone(),
            old_static,
            new_id, changes: *mutable && self.chain_writes_through(chain_lo, rap),
            line: eff.line,
          });
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
          let definite = *binding && eff.checks_before == 0 && !guard;
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
    // The call that made a new reference (`r = first(&v)`, #1068) lends
    // v for as long as r holds the result: that borrow hasn't ended.
    for u in uses.iter_mut() {
      for r in refs.iter().filter(|r| r.new.iter().any(|(l, _)| *l == u.rap)) {
        u.uses.retain(|c| Some(&c.to) != r.callee.as_ref());
      }
    }
    uses.retain(|u| !out.iter().any(|m| m.rap == u.rap) && !assigns.iter().any(|a| a.rap == u.rap)
      && !refs.iter().any(|r| r.rap == u.rap) && !u.uses.is_empty());
    if let Some(r) = refs.iter().find(|r| out.iter().any(|m| m.rap == r.rap) || assigns.iter().any(|a| a.rap == r.rap)) {
      panic!("{} that {} (`{}`) and also move it or write through it aren't supported yet: both last even when {}",
        what_kind, CHAIN_ASSIGN_REF.replace("the condition", outside), r.rap.name(), later);
    }
    // A variable both moved and assigned (#603): what the failing path
    // leaves depends on which came last. Moved last (`{ z = a; true }
    // && take(z)`): its previous value or nothing, the maybe-moved state
    // with one sentence for both. Assigned last (`take(z) && { z = a;
    // true }`): the new value or nothing (and its previous value if a
    // part can fail before the move), as for an assignment to a variable
    // that held none. A write through a place (`*b = a`, `v[0] = a`)
    // counts too when the variable it goes through is moved: the new
    // value went with it.
    let mut i = 0;
    while i < assigns.len() {
      let Some(m) = out.iter_mut().find(|m| m.rap == assigns[i].rap) else { i += 1; continue };
      let a = assigns.remove(i);
      let first = m.seq.0.min(a.seq);
      let old = guard || walk.effects[first].checks_before > 0;
      if m.seq.1 > a.seq {
        m.how = ChainFailHow::AssignThenMove { place: a.place, drops_old: a.drops_old, to: m.to.clone() };
      } else {
        m.how = ChainFailHow::MoveThenAssign { old, to: m.to.clone() };
        m.definite = false;
        m.dropped.clear();
      }
    }
    // An assignment to a variable that may hold no value before it
    // (#603): the failing path has it holding the new value or nothing.
    let mut i = 0;
    while i < assigns.len() {
      if assigns[i].held || assigns[i].place.is_some() {
        i += 1;
        continue;
      }
      let a = assigns.remove(i);
      let before = self.no_value_before(&a.rap, chain_lo);
      out.push(ChainFailMove { rap: a.rap, definite: false, to: Vec::new(), dropped: Vec::new(),
        how: ChainFailHow::AssignNoValue { before }, seq: (a.seq, a.seq) });
    }
    ChainFail { moves: out, assigns, uses, refs }
  }

  /// Why `rap`, which a part of the let-chain (or guard) whose events
  /// start at `preprocessed_events[chain_lo]` gives a new value, may
  /// hold none before that (#603). See `NoValueBefore`.
  fn no_value_before(&self, rap: &ResourceAccessPoint, chain_lo: usize) -> NoValueBefore {
    fn last_move(events: &[(usize, ExternalEvent)], rap: &ResourceAccessPoint) -> Option<usize> {
      let mut found = None;
      for (line, ev) in events {
        match ev {
          ExternalEvent::Branch { branches, .. } => {
            if let Some(l) = branches.iter().filter_map(|b| last_move(&b.e_data, rap)).max() {
              found = Some(l);
            }
          }
          ExternalEvent::Move { from: ResourceTy::Value(r), .. }
          | ExternalEvent::ClosureCall { closure: ResourceTy::Value(r), .. }
          | ExternalEvent::ManuallyDropByHand { ro: r, .. } if r == rap => found = Some(*line),
          _ => {}
        }
      }
      found
    }
    match self.rap_ownership_before(chain_lo, rap.name()) {
      Ownership::Maybe => NoValueBefore::Maybe,
      // Held where the chain starts: its first part (drawn before the
      // split, so not one of `walk.effects`) moved it.
      Ownership::Held => NoValueBefore::MovedByCondition(last_move(&self.preprocessed_events[chain_lo..], rap)),
      Ownership::None => NoValueBefore::Moved(last_move(&self.preprocessed_events[..chain_lo], rap)),
    }
  }

  /// The events that say what `fail` (see `chain_fail_moves`) leaves
  /// behind on `path`, in order: assignments, moves, then borrows that
  /// have ended (#688). They take fresh ids.
  ///
  /// A new reference assigned by the condition (`r = &s`, #603) is said
  /// on `r`, `s` and `t` (what `r` pointed to before) where the path
  /// starts, and the borrows end where rustc's borrow checker ends them
  /// on that path: inside `part` (the else branch, a later arm; `None`
  /// for the path that skips the body) at the last line it has them in
  /// scope, returned as (line, event) for the caller to put among that
  /// part's events; or, when they are in scope after `whole` (the
  /// conditional), after it, at their last line (`late_old_ends`,
  /// `emit_borrow_ends` ends `s`'s). `fail_line`: where the path starts.
  fn chain_fail_events(&mut self, fail: ChainFail, path: FailPath, part: Option<rustc_span::Span>, whole: rustc_span::Span, fail_line: usize)
    -> (Vec<ExternalEvent>, Vec<(usize, ExternalEvent)>) {
    let mut evs = Vec::new();
    let mut ends = Vec::new();
    for r in fail.refs.iter() {
      let parts: Vec<rustc_span::Span> = part.into_iter().collect();
      // Where each lender's loan is in scope on this path: (lender,
      // loan, last line inside `part`, in scope after `whole`).
      let scope = |ls: &[(ResourceAccessPoint, (usize, usize))]| ls.iter().map(|(l, loan)| {
        let (last, past) = self.loan_scope_in(loan.0, &parts, whole);
        (l.clone(), *loan, last.first().copied().flatten(), past)
      }).collect::<Vec<_>>();
      let (new, old) = (scope(&r.new), scope(&r.old));
      let from: Vec<String> = new.iter().chain(old.iter()).map(|(l, ..)| l.name().to_owned()).collect();
      let chain = ChainRef { path, rhs: r.rhs.clone(), new: new.len(), new_via_call: r.new_via_call, old_via_call: r.old_via_call,
        old_static: r.old_static.clone() };
      let alt = |this: &mut Self, ro: &ResourceAccessPoint, end: bool, first: bool| {
        let id = *this.unique_id;
        *this.unique_id += 1;
        ExternalEvent::OrAltBorrow { ro: ro.clone(), into: r.rap.clone(), from: from.clone(), mutable: r.mutable, end, first,
          chain: Some(chain.clone()), id }
      };
      if new.iter().chain(old.iter()).all(|(_, _, last, past)| last.is_none() && !past) {
        // `r` isn't used again on this path: `s`'s borrow has ended (as
        // for a call the condition lent it to, #688), and `t`'s ends
        // where the path starts, unless it ended before the condition
        // (`r` was used last before it).
        for (l, ..) in new.iter() {
          let id = *self.unique_id;
          *self.unique_id += 1;
          evs.push(ExternalEvent::ChainFailUse { ro: l.clone(),
            uses: vec![ChainUse { to: r.rap.name().to_owned(), mutable: r.mutable, binding: false, changes: r.changes }],
            definite: false, path, id });
        }
        for (l, ..) in old.iter().filter(|(_, loan, ..)| loan.1 >= r.line) {
          let id = *self.unique_id;
          *self.unique_id += 1;
          let (from, to) = (ResourceTy::Value(r.rap.clone()), ResourceTy::Value(l.clone()));
          evs.push(if r.mutable {
            ExternalEvent::MutableDie { from, to, id, static_lender: None, still_borrowed_by: Vec::new(), of_ref_var: false, exhaustion: Vec::new() }
          } else {
            ExternalEvent::StaticDie { from, to, id, static_lender: None, still_borrowed_by: Vec::new(), of_ref_var: false, exhaustion: Vec::new() }
          });
        }
        continue;
      }
      for (i, (l, ..)) in new.iter().chain(old.iter()).enumerate() {
        let ev = alt(self, l, false, i == 0);
        evs.push(ev);
      }
      // `r`'s own end goes with `s`'s, unless that one is after the
      // conditional (drawn by `emit_borrow_ends`, with the new
      // reference's other lenders).
      for (i, (l, _, last, past)) in new.iter().enumerate() {
        if !past {
          let ev = alt(self, l, true, i == 0);
          ends.push((last.unwrap_or(fail_line), ev));
        }
      }
      for (l, loan, last, past) in old.iter() {
        let ev = alt(self, l, true, false);
        if *past {
          self.late_old_ends.push((loan.1, ev));
        } else {
          ends.push((last.unwrap_or(fail_line), ev));
        }
      }
    }
    let mut next_id = || {
      let id = *self.unique_id;
      *self.unique_id += 1;
      id
    };
    for a in fail.assigns {
      evs.push(ExternalEvent::ChainFailAssign { ro: a.rap, place: a.place, drops_old: a.drops_old, path, id: next_id() });
    }
    for m in fail.moves {
      evs.push(ExternalEvent::LoopExitMove {
        ro: m.rap,
        definite: m.definite,
        at: MaybeMovedAt::ChainFail { to: m.to, dropped: m.dropped, path, how: m.how },
        id: next_id(),
      });
    }
    for u in fail.uses {
      evs.push(ExternalEvent::ChainFailUse { ro: u.rap, uses: u.uses, definite: u.definite, path, id: next_id() });
    }
    (evs, ends)
  }

  /// Notes the references `fail` says a condition (or guard) gave a new
  /// loan (#603), for `emit_borrow_ends`: on the path into the body
  /// (`then`: the condition and the body, or the guarded arm) the loan
  /// ends where the facts end it there; the failing paths end it with
  /// their own events (`chain_fail_events`).
  fn note_chain_then(&mut self, fail: &ChainFail, then: rustc_span::Span, whole: rustc_span::Span) {
    let sm = self.tcx.sess.source_map();
    let line = |p: rustc_span::BytePos| sm.lookup_char_pos(p).line;
    for r in &fail.refs {
      self.chain_then.push((r.rap.name().to_owned(), (line(then.lo()), line(then.hi())), line(whole.hi())));
    }
  }

  /// Whether `id` is inside the body of a loop (`loop`, `while`, `for`)
  /// of its own function or closure.
  fn in_loop_body(&self, id: HirId) -> bool {
    for (_, node) in self.tcx.hir_parent_iter(id) {
      match node {
        rustc_hir::Node::Expr(e) if matches!(e.kind, ExprKind::Loop(..)) => return true,
        rustc_hir::Node::Expr(e) if matches!(e.kind, ExprKind::Closure(..)) => return false,
        rustc_hir::Node::Item(_) | rustc_hir::Node::ImplItem(_) | rustc_hir::Node::TraitItem(_) => return false,
        _ => {}
      }
    }
    false
  }

  /// The loans the references among `outer` hold, by name (see
  /// `ChainEffectKind::AssignRef`).
  fn outer_loans(&self, outer: &HashSet<ResourceAccessPoint>) -> HashMap<String, RefData> {
    #[expect(clippy::disallowed_methods, reason = "collected into a map")]
    let loans = self.borrow_map.iter()
      .filter(|(n, _)| outer.iter().any(|r| r.name() == *n && r.is_ref()))
      .map(|(n, d)| (n.clone(), d.clone()))
      .collect();
    loans
  }

  /// The loan of `lender` that `rd` holds: its index in rustc's borrow
  /// set and the last line rustc has it in scope. One made by `&x` is
  /// the loan issued at that expression. Otherwise (a call's result,
  /// `first(&v)`, `longest(&a, &b)`, or `&v[0]`, which borrows `v` for
  /// `Index::index`) it is one of that lender's issued on its line, or
  /// inside that expression: the one made inside `within` (the assigned
  /// expression), else the one in scope longest.
  fn loan_of_lender(&self, loans: &[crate::mir_data::MIRBorrowData], rd: &RefData, lender: &ResourceAccessPoint, within: Option<rustc_span::Span>) -> Option<(usize, usize)> {
    let root = lender.name().split('.').next().unwrap_or(lender.name()).to_owned();
    let of_lender = |b: &&crate::mir_data::MIRBorrowData| b.borrowed_place.as_deref() == Some(root.as_str())
      && within.is_none_or(|w| w.contains(b.span));
    let longest = |it: &mut dyn Iterator<Item = &'_ crate::mir_data::MIRBorrowData>| {
      it.max_by_key(|b| (b.region.1, std::cmp::Reverse(b.borrow_idx))).map(|b| (b.borrow_idx, b.region.1))
    };
    match rd.loan()? {
      LoanId::Span(sp) if rd.lender == ResourceTy::Value(lender.clone()) => match loans.iter().find(|b| b.span == sp) {
        Some(b) if b.borrowed_place.as_deref() == Some(root.as_str()) => Some((b.borrow_idx, b.region.1)),
        _ => longest(&mut loans.iter().filter(|b| sp.contains(b.span)).filter(of_lender)),
      },
      LoanId::Span(_) => None,
      LoanId::Line(l) => longest(&mut loans.iter().filter(|b| b.region.0 == l).filter(of_lender)),
    }
  }

  /// The right-hand side of the last assignment `name = …` in `part`.
  fn assigned_rhs(&self, part: &'tcx Expr<'tcx>, name: &str) -> Option<AssignedRhs> {
    struct Find<'a, 'tcx> { name: &'a str, found: Option<&'tcx Expr<'tcx>> }
    impl<'a, 'tcx> intravisit::Visitor<'tcx> for Find<'a, 'tcx> {
      fn visit_expr(&mut self, e: &'tcx Expr<'tcx>) {
        if let ExprKind::Assign(lhs, rhs, _) = e.kind {
          if let ExprKind::Path(rustc_hir::QPath::Resolved(None, p)) = lhs.kind {
            if p.segments.len() == 1 && p.segments[0].ident.as_str() == self.name {
              self.found = Some(rhs);
            }
          }
        }
        intravisit::walk_expr(self, e);
      }
    }
    let mut f = Find { name, found: None };
    intravisit::Visitor::visit_expr(&mut f, part);
    let rhs = f.found?;
    let text = self.tcx.sess.source_map().span_to_snippet(rhs.span).ok()?;
    let direct = matches!(rhs.kind, ExprKind::AddrOf(_, _, inner) if matches!(inner.kind, ExprKind::Path(_)));
    let callee = match rhs.kind {
      ExprKind::Call(f, _) => match f.kind {
        ExprKind::Path(QPath::Resolved(_, p)) => p.segments.last().map(|s| s.ident.to_string()),
        ExprKind::Path(QPath::TypeRelative(_, seg)) => Some(seg.ident.to_string()),
        _ => None,
      },
      ExprKind::MethodCall(seg, ..) => Some(seg.ident.to_string()),
      _ => None,
    };
    Some(AssignedRhs { text: text.split_whitespace().collect::<Vec<_>>().join(" "), span: rhs.span, direct, callee })
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

  /// An operand of a built-in operator on primitives (`item == b' '`,
  /// `x + y`, `-x`, `acc += x`) that names a column: its value is
  /// copied, and that is drawn on its column ("item's value is
  /// copied"). The operator is not an actor, like a constructor
  /// (#768, #873): it gets no column and no arrow (#908).
  fn read_copied_operand(&mut self, operand: &'tcx Expr<'tcx>) {
    let line_num = expr_to_line(operand, &self.tcx);
    let from = get_rap(operand, &self.tcx, &self.raps);
    let (ResourceTy::Value(rap) | ResourceTy::Deref(rap)) = &from else { return };
    self.update_rap(rap, line_num);
    self.add_ev(line_num, Evt::Copy, ResourceTy::Anonymous, from, false);
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
        // `people[0].name.push(..)` / `b.name.len()` (b a Box): a
        // borrow of part of people's / b's value (#1176). Not a by-value
        // `self`, which would move only that part out.
        _ => match self.place_holder(rcvr) {
          Some(h) if self.tcx.typeck(rcvr.hir_id.owner).expr_ty_adjusted(rcvr).is_ref() =>
            (h.clone(), ResourceTy::Value(h)),
          _ => return,
        },
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
    // `x.show()` / `(&x).show()` with x a reference variable and
    // `show(&self)` on Self = `&String`: the call borrows x itself (#938).
    let of_ref_var = ref_var_receiver_borrow(rcvr, self.tcx.typeck(rcvr.hir_id.owner));
    // `r.len()` with `r: &&String` borrows `**r`, not what r points to
    // (#952).
    let deep = if of_ref_var { None } else { self.deep_place(rcvr) };
    // `k.push('!')` with `k: Box<&mut String>` reborrows through the
    // reference k's Box holds (`&mut **k`), as `r.push('!')` does
    // through r: k itself isn't borrowed, nor need it be `mut` (#1282).
    let through_held_ref = deep.as_ref().is_some_and(|d| d.held_mut.is_some());
    let autoref = if is_method_autoref(rcvr, self.tcx.typeck(rcvr.hir_id.owner)) && !through_held_ref {
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
                  self.add_pass_by_mref_ev(line_num, ResourceTy::Value(fn_rap.clone()), rcvr_rty.clone(), true, result_has_lifetime, autoref, false);
                  // `r.name.push(..)` with `r = &mut p`: the call
                  // modifies p.name, drawn on its column too (#1216).
                  self.write_field_behind_ref(&rcvr_rty, rcvr, line_num);
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
                  let is_reborrow = through_held_ref || match rcvr.kind {
                    ExprKind::AddrOf(_, _, operand) => is_explicit_reborrow(operand, typeck)
                      || is_coerced_ref_var_reborrow(rcvr, typeck),
                    _ => is_implicit_mut_reborrow(rcvr, typeck) || is_explicit_reborrow(rcvr, typeck),
                  };
                  let result_has_lifetime = call_result_has_lifetime(expr, typeck);
                  self.add_pass_by_mref_ev_deep(line_num, ResourceTy::Value(fn_rap.clone()), ResourceTy::Value(rcvr_rap.clone()), is_reborrow, result_has_lifetime, autoref, of_ref_var, deep.clone());
                },
                AutoBorrowMutability::Not => {
                  let refcell_borrow_mut = self.is_refcell_borrow_mut_call(expr);
                  // `r.set(2)` with `r: &Cell<i32>` (#1149): the call can
                  // change what r points to, as `c.set(2)` changes c.
                  let rcvr_ty = self.tcx.typeck(rcvr.hir_id.owner).node_type(rcvr.hir_id);
                  let pointee_interior_mut = rcvr_rap.is_ref() && !of_ref_var && matches!(rcvr_rty, ResourceTy::Value(_))
                    && rcvr_ty.builtin_deref(true).is_some_and(|t| self.ty_has_interior_mut(t, rcvr.hir_id.owner) == InteriorMut::Yes);
                  self.add_pass_by_sref_ev_full(line_num, ResourceTy::Value(fn_rap.clone()), rcvr_rty.clone(), autoref, refcell_borrow_mut, pointee_interior_mut, of_ref_var, deep.clone());
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
        typing_env(self.tcx, expr.hir_id.owner), self_ty);
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
      self.refuse_user_macro_use(expr.span, expr);
    }
    match expr.kind {
      ExprKind::Block(b, _) => {
        for s in b.stmts {
          match s.kind {
            StmtKind::Let(l) => {
              if let Some(init) = l.init {
                if l.span.from_expansion() {
                  self.refuse_user_macro_use(l.span, init);
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
      // A try block's implicit success conversion wraps the user-written
      // tail. Visit it normally, including a tail that is itself desugared
      // (for example, a for loop whose body contains another `?`).
      ExprKind::Call(_, [tail]) if self.try_block.is_some()
        && callee_def_id(self.tcx, expr).is_some_and(|id| self.tcx.is_lang_item(id, rustc_hir::LangItem::TryTraitFromOutput)) =>
      {
        self.visit_expr(tail);
      }
      ExprKind::Call(_, args) => {
        let repeated = vec_macro_elems(self.tcx, expr).and_then(|elems| elems.first())
          .filter(|first| args.first().is_some_and(|a| a.hir_id == first.hir_id));
        for a in args {
          if repeated.is_some_and(|first| a.hir_id == first.hir_id) {
            self.draw_vec_place(a, None);
          }
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
              manually_takes: false, from, to, id, is_reborrow: false, result_has_lifetime: false, implicit: ImplicitRef::MacroArg,
              result_keeps_borrow: false, stored_in: StoredIn::default(), break_alt: None, of_ref_var: false, deep: None,
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
        let vec = vec_macro_elems(self.tcx, expr).is_some();
        for i in items {
          if vec {
            if !self.vec_element_reached(i, expr.span.ctxt().outer_expn()) { continue; }
            self.draw_vec_place(i, None);
          }
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
        // `e?`: draw drops on the residual path.
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
    e: &'tcx Expr<'tcx>,
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
    if let Some(var) = first_user_local_use(self.tcx, e) {
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
    let deep = from.extract_rap().and_then(|r| {
      let place = self.projected_access(arg.span, r, |f| match f {
        crate::facts::Fact::Borrow { lender, .. } => Some(lender), _ => None,
      })?;
      let ty = self.tcx.typeck(arg.hir_id.owner).expr_ty(arg);
      Some(DeepPlace { derefs: place.len() - r.name().len() - 1,
        holds_value: self.ty_is_copy(ty, arg.hir_id.owner) || self.ty_is_plain(ty, arg.hir_id.owner),
        held_mut: None, elem: None })
    });
    let already = self.preprocessed_events.iter().any(|(l, e)| {
      *l == line_num
        && matches!(e, ExternalEvent::PassByStaticReference { from: f, to: t, deep: d, .. }
          if *f == from && *t == to && d == &deep)
    });
    if !already {
      // format_args! takes `&arg`; the user didn't write the `&`.
      let id = *self.unique_id;
      self.add_external_event(line_num, ExternalEvent::PassByStaticReference { from, to, id, implicit: ImplicitRef::MacroArg, refcell_borrow_mut: false, pointee_interior_mut: false, result_keeps_borrow: false, stored_in: StoredIn::default(), break_alt: None, still_borrowed_by: Vec::new(), of_ref_var: false, deep });
      *self.unique_id += 1;
    }
  }
}

/// Every `PatKind::Binding` in `pat`, in source order (left to right),
/// including `name @ sub` and the bindings inside `sub`. Or-pattern
/// alternatives bind the same names, so only the first is walked.
impl<'a, 'tcx> ExprVisitor<'a, 'tcx> {
  /// The parts of a by-value parameter's argument (of type `ty`, at the
  /// field path `path`) that its pattern `pat` doesn't bind and that need
  /// drop (#1181): what a `..` or `_` leaves (`P { name, .. }: P` leaves
  /// `bio`, `(a, _): (String, String)` leaves `1`). Nothing through a
  /// `&` pattern: that's the caller's.
  fn unbound_param_parts(&self, pat: &'tcx Pat<'tcx>, ty: rustc_middle::ty::Ty<'tcx>, path: String, owner: rustc_hir::OwnerId, out: &mut Vec<String>) {
    let join = |f: &str| if path.is_empty() { f.to_owned() } else { format!("{}.{}", path, f) };
    let typeck = self.tcx.typeck(owner);
    // Matching through a reference (default binding modes): the
    // argument is the caller's.
    if typeck.pat_adjustments().get(pat.hir_id).is_some_and(|a| !a.is_empty()) {
      return;
    }
    match pat.kind {
      PatKind::Wild => {
        if !path.is_empty() && self.needs_drop(ty, owner) {
          out.push(path);
        }
      }
      PatKind::Tuple(pats, dd) => {
        let rustc_middle::ty::TyKind::Tuple(tys) = ty.kind() else { return };
        let split = dd.as_opt_usize().unwrap_or(pats.len());
        let back = pats.len() - split;
        for (i, ety) in tys.iter().enumerate() {
          let sub = if i < split { pats.get(i) } else if i >= tys.len() - back { pats.get(i + pats.len() - tys.len()) } else { None };
          match sub {
            Some(p) => self.unbound_param_parts(p, ety, join(&i.to_string()), owner, out),
            None => if self.needs_drop(ety, owner) { out.push(join(&i.to_string())) },
          }
        }
      }
      PatKind::Struct(_, fields, has_rest) => {
        let rustc_middle::ty::TyKind::Adt(adt, args) = ty.kind() else { return };
        if !adt.is_struct() {
          return;
        }
        for fd in adt.non_enum_variant().fields.iter() {
          let fty = fd.ty(self.tcx, args);
          match fields.iter().find(|f| f.ident.name == fd.name) {
            Some(f) => self.unbound_param_parts(f.pat, fty, join(fd.name.as_str()), owner, out),
            None => if has_rest.is_some() && self.needs_drop(fty, owner) { out.push(join(fd.name.as_str())) },
          }
        }
      }
      PatKind::TupleStruct(_, pats, dd) => {
        let rustc_middle::ty::TyKind::Adt(adt, args) = ty.kind() else { return };
        if !adt.is_struct() {
          return;
        }
        let fields: Vec<rustc_middle::ty::Ty<'tcx>> = adt.non_enum_variant().fields.iter().map(|fd| fd.ty(self.tcx, args)).collect();
        let split = dd.as_opt_usize().unwrap_or(pats.len());
        let back = pats.len() - split;
        for (i, fty) in fields.iter().enumerate() {
          let sub = if i < split { pats.get(i) } else if i >= fields.len() - back { pats.get(i + pats.len() - fields.len()) } else { None };
          match sub {
            Some(p) => self.unbound_param_parts(p, *fty, join(&i.to_string()), owner, out),
            None => if self.needs_drop(*fty, owner) { out.push(join(&i.to_string())) },
          }
        }
      }
      // `[p, _]: [String; 2]`: the elements a `_` or `..` leaves stay in
      // the parameter too (#1285).
      PatKind::Slice(before, mid, after) => {
        let rustc_middle::ty::TyKind::Array(ety, len) = ty.kind() else { return };
        let Some(len) = len.try_to_target_usize(self.tcx) else { return };
        let (len, front, back) = (len as usize, before.len(), after.len());
        if front + back > len {
          return;
        }
        for i in 0..len {
          let sub = if i < front {
            before.get(i)
          } else if i >= len - back {
            after.get(i - (len - back))
          } else {
            // `..` leaves the middle; `rest @ ..` takes it.
            mid.filter(|m| !matches!(m.kind, PatKind::Wild))
          };
          let elem = format!("{}[{}]", path, i);
          match sub {
            Some(p) if i < front || i >= len - back => self.unbound_param_parts(p, *ety, elem, owner, out),
            Some(_) => {}
            None => if self.needs_drop(*ety, owner) { out.push(elem) },
          }
        }
      }
      // A binding takes its part (by value, or a `ref` borrows it, #698);
      // `&pat` matches the caller's data.
      _ => {}
    }
  }

}

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

/// Whether `pat` has a `&`, `box` or deref pattern in it (`Some(&k)`).
fn pat_has_pointer_pat(pat: &Pat<'_>) -> bool {
  let mut found = false;
  pat.walk(|p| {
    found |= matches!(p.kind, PatKind::Ref(..) | PatKind::Box(_) | PatKind::Deref(_));
    !found
  });
  found
}

/// Refuse an or-pattern that binds `name` from different parts of the
/// variable `whole`, which a tuple (or struct) literal a `let` matches
/// moved into a temporary (`let (p, (x, _) | (_, x)) = (s, u);`): what
/// `x` doesn't take of `u` is dropped with the temporary, which `u`'s
/// parts don't show (#612). (`match` and `if let` draw it, #627.)
fn or_refuse_temporary_parts(name: &str, whole: &str, line: usize) -> ! {
  panic!("or-patterns that bind `{}` from different parts of `{}`, which the tuple or struct being matched \
          moves into a temporary (line {}), aren't supported yet", name, whole, line)
}

/// Whether `pat` has an or-pattern anywhere in it.
fn pat_has_or(pat: &Pat<'_>) -> bool {
  let mut found = false;
  pat.walk_short(|p| {
    found |= matches!(p.kind, PatKind::Or(_));
    !found
  });
  found
}

/// Every way of matching sub-patterns side by side (`(p, q)`), given
/// the ways each one matches (see `or_alt_sources`): one map per
/// combination, each with the bindings of both.
fn or_sources_product(
  left: Vec<BTreeMap<String, (String, Option<ResourceTy>)>>,
  right: Vec<BTreeMap<String, (String, Option<ResourceTy>)>>,
) -> Vec<BTreeMap<String, (String, Option<ResourceTy>)>> {
  let mut out = Vec::new();
  for l in &left {
    for r in &right {
      let mut m = l.clone();
      m.extend(r.iter().map(|(k, v)| (k.clone(), v.clone())));
      if !out.contains(&m) {
        out.push(m);
      }
    }
  }
  out
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

/// The first use of a local variable written by the user (not
/// synthesized by a macro) inside an expression: its line and name.
fn first_user_local_use<'tcx>(tcx: rustc_middle::ty::TyCtxt<'tcx>, e: &'tcx Expr<'tcx>) -> Option<(usize, String)> {
  locals_in(e, ClosureLocals::Skip).into_iter().find_map(|m| {
    let expr = m.expr.filter(|x| !x.span.from_expansion())?;
    let ExprKind::Path(QPath::Resolved(None, path)) = expr.kind else { return None };
    let name = path.segments.last().map(|s| s.ident.as_str().to_owned()).unwrap_or_default();
    Some((expr_to_line(expr, &tcx), name))
  })
}

/// The name, with its `!`, of the outermost bang macro the user wrote
/// that `span` was expanded from: `println!` for the `&x` that
/// `format_args_nl!` builds inside `println!`, `show!` if the user's
/// own `macro_rules! show` calls `println!`.
pub(crate) fn user_macro_name(span: rustc_span::Span) -> Option<String> {
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

/// Whether `e` holds a `?` that can return from the function `e` is in
/// (not one in a closure, which returns from the closure).
fn holds_try(e: &Expr<'_>) -> bool {
  struct Finder(bool);
  impl<'v> rustc_hir::intravisit::Visitor<'v> for Finder {
    fn visit_expr(&mut self, e: &'v Expr<'v>) {
      match e.kind {
        ExprKind::Match(_, _, rustc_hir::MatchSource::TryDesugar(_)) => self.0 = true,
        ExprKind::Closure(_) => {}
        _ => rustc_hir::intravisit::walk_expr(self, e),
      }
    }
  }
  let mut f = Finder(false);
  rustc_hir::intravisit::Visitor::visit_expr(&mut f, e);
  f.0
}

/// Whether `a`, an argument of a constructor whose value is returned
/// (`Some(a)`), puts something into the value that came from elsewhere:
/// a variable or field, a call's result (#783), or a tuple, array or
/// `vec![…]` literal holding one (`Some((s, 1))`, `Some(vec![s])`,
/// #1185). Those go to the caller inside the returned value.
fn ctor_part_carries_value<'tcx>(tcx: rustc_middle::ty::TyCtxt<'tcx>, a: &'tcx Expr<'tcx>, typeck: &rustc_middle::ty::TypeckResults<'_>) -> bool {
  if let Some(elems) = vec_macro_elems(tcx, a) {
    return elems.iter().any(|e| ctor_part_carries_value(tcx, e, typeck));
  }
  if a.span.from_expansion() {
    return matches!(a.kind, ExprKind::Path(_) | ExprKind::Field(..));
  }
  match a.kind {
    ExprKind::Path(_) | ExprKind::Field(..) => true,
    ExprKind::Call(..) | ExprKind::MethodCall(..) => !typeck.node_type(a.hir_id).is_ref(),
    ExprKind::Tup(elems) | ExprKind::Array(elems) => elems.iter().any(|e| ctor_part_carries_value(tcx, e, typeck)),
    _ => false,
  }
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
      // (A static has a column too, #690.)
      ExprKind::Path(QPath::Resolved(_, p)) => matches!(p.res, Res::Local(_)) || static_of_path(p).is_some(),
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

/// A name an or-pattern arm binds from a different place in each
/// alternative (#533): the places, in the order of the alternatives.
pub(crate) struct OrConflict {
  sole_runtime_source: Option<String>,
  source_names: BTreeMap<String, String>,
  other_bindings: BTreeMap<String, crate::facts::OtherBindings>,
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
/// What the pattern of one `match` arm is drawn from: the scrutinee, the
/// arm, and where it sits (see `visit_match_normal`).
#[derive(Clone, Copy)]
struct MatchArmCtx<'c, 'tcx> {
  guard_expr: &'tcx Expr<'tcx>,
  typeck_res: &'tcx rustc_middle::ty::TypeckResults<'tcx>,
  parents: &'c [ResourceTy],
  parents_ty: &'c [rustc_middle::ty::Ty<'tcx>],
  scrutinee_moves: &'c [ResourceAccessPoint],
  tuple_name: &'c str,
  or_conflicts: &'c [OrConflict],
  arm_pat: &'tcx Pat<'tcx>,
  pat_line: usize,
  end: usize,
  inline: bool,
}

/// What an arm's pattern adds while it is drawn (`match_arm_tuple_pat` /
/// `match_arm_singleton_pat`).
#[derive(Default)]
struct MatchArmPat {
  /// Variables the pattern binds.
  pat_decls: HashSet<ResourceAccessPoint>,
  /// Struct bindings whose field columns still need their
  /// "acquires its part" events.
  member_binds: Vec<ResourceAccessPoint>,
  /// The arm's events (multi-arm: they live inside the Branch's per-arm
  /// event list).
  branch_e_data: Vec<(usize, ExternalEvent)>,
  /// Events that happen at the end of the arm (a borrow's end).
  callback_events: Vec<(ResourceTy, ResourceTy, Evt)>,
  /// Parents this arm's pattern moves out of.
  arm_moved: Vec<ResourceTy>,
  /// The OrAltMove dots of the names `or_conflicts` lists, added after
  /// the moves into the scrutinee tuple (the tuple holds the values they
  /// may take).
  or_events: Vec<ExternalEvent>,
  /// Where the possible borrows of an or-pattern's reference end (#565);
  /// added with the arm's other end events.
  or_end_events: Vec<(usize, ExternalEvent)>,
}


impl<'a, 'tcx> ExprVisitor<'a, 'tcx> {
  /// `ManuallyDrop::drop(&mut md)` drops the value inside `md` by
  /// hand. Handle the inherent function on a tracked place.
  fn manually_drop_by_hand(&mut self, fn_expr: &'tcx Expr<'tcx>, args: &'tcx [Expr<'tcx>]) -> bool {
    let Some(def_id) = self.tcx.typeck(fn_expr.hir_id.owner).type_dependent_def_id(fn_expr.hir_id) else { return false };
    if !matches!(self.tcx.def_kind(def_id), DefKind::AssocFn) || self.tcx.item_name(def_id).as_str() != "drop" {
      return false;
    }
    let Some(imp) = self.tcx.impl_of_assoc(def_id) else { return false };
    if self.tcx.impl_opt_trait_ref(imp).is_some()
      || !self.tcx.type_of(imp).skip_binder().ty_adt_def().is_some_and(|d| d.is_manually_drop())
    {
      return false;
    }
    let [arg] = args else { return false };
    let ExprKind::AddrOf(_, Mutability::Mut, inner) = arg.kind else { return false };
    let Some(ResourceTy::Value(rap)) = place_rap(inner, &self.tcx, &self.raps) else { return false };
    if !rap.is_manually_drop() {
      return false;
    }
    let Some(fn_name) = hirid_to_var_name(fn_expr.hir_id, &self.tcx) else { return false };
    self.add_fn(fn_name.clone());
    self.visit_expr(arg);
    let line_num = expr_to_line(arg, &self.tcx);
    self.match_arg(arg, fn_name);
    self.add_external_event(line_num, ExternalEvent::ManuallyDropByHand { ro: rap, id: *self.unique_id });
    *self.unique_id += 1;
    true
  }
}

/// Names of the locals that evaluating `e` for its value may move out
/// of, wholly or in part (a field), so a `return e` / `break e` may
/// hand them on rather than drop them. One that `e` only borrows
/// (`s.len()`, `&s`, `s == t`) or copies is left out: it's still
/// dropped at the exit (#410). Shapes not handled below count every
/// local they name as moved, which errs on the side of not drawing a
/// drop.
/// Whether the place `name` is `of` or a part of it (`p.name` within
/// `p`, `p.name` within `p.name`; not `p` within `p.name`).
pub(crate) fn place_within(name: &str, of: &str) -> bool {
  name == of || name.strip_prefix(of).is_some_and(|rest| rest.starts_with('.'))
}

fn locals_moved_by<'tcx>(tcx: rustc_middle::ty::TyCtxt<'tcx>, e: &'tcx Expr<'tcx>) -> BTreeSet<String> {
  struct Scan<'tcx> {
    tcx: rustc_middle::ty::TyCtxt<'tcx>,
    typeck: &'tcx rustc_middle::ty::TypeckResults<'tcx>,
    env: rustc_middle::ty::TypingEnv<'tcx>,
    moved: BTreeSet<String>,
  }
  impl<'tcx> Scan<'tcx> {
    /// `p.a.b` as a place name, when each step is a field of a local
    /// struct or tuple, or of a Box's payload (a built-in deref; a
    /// non-Copy value can't be moved out from behind a reference). Not
    /// through an overloaded `Deref`.
    fn field_place(&self, e: &'tcx Expr<'tcx>) -> Option<String> {
      match e.kind {
        ExprKind::Field(base, f) if self.typeck.expr_adjustments(base).iter()
          .all(|a| matches!(a.kind, Adjust::Deref(rustc_middle::ty::adjustment::DerefAdjustKind::Builtin))) =>
          self.field_place(base).map(|b| format!("{}.{}", b, f.name)),
        ExprKind::Path(QPath::Resolved(None, p)) => match p.res {
          Res::Local(id) => Some(binding_name(&self.tcx, id)),
          _ => None,
        },
        _ => None,
      }
    }
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
        // Moving a field out moves out of the struct: `p.name` is
        // recorded as that place, so what else `p` holds is still
        // dropped where the value leaves (#1141). Through a Box or a
        // reference, the root.
        ExprKind::Field(base, _) => match self.field_place(e) {
          Some(place) if by_value => { self.moved.insert(place); }
          _ => self.scan(base, by_value),
        },
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
        // `P { a: p.b, b: p.a }`: each field's value, as for a tuple
        // (#1138). `..base` takes the rest of base.
        ExprKind::Struct(_, fields, tail) => {
          for f in fields.iter() { self.scan(f.expr, true); }
          if let rustc_hir::StructTailExpr::Base(b) = tail { self.scan(b, true); }
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
          // A closure's body isn't walked; what it captures is.
          for m in locals_in(e, ClosureLocals::Captures(self.tcx)) {
            self.moved.insert(binding_name(&self.tcx, m.id));
          }
        }
      }
    }
  }
  let owner = e.hir_id.owner;
  let mut scan = Scan {
    tcx,
    typeck: tcx.typeck(owner),
    env: typing_env(tcx, owner),
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
    let no_drop_exit = self.no_drop_exit.take();
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
    // A `let x;` local declared outside the loop the exit leaves, but
    // first assigned inside it, isn't one of its locals (#786).
    let declared_outside = |rap: &ResourceAccessPoint| loop_id.is_some()
      && self.deferred_decl_at.get(rap.name().split('.').next().unwrap_or("")).is_some_and(|at| *at < frame_floor);
    // A struct some of whose fields only some of the values move
    // (`return if d { p.name } else { p.bio }`): those fields get the
    // dots below; what else it holds is dropped with them (#1141).
    let part_maybe_moved = |rap: &ResourceAccessPoint| moved_some.iter()
      .any(|m| place_within(m, rap.name()) && m != rap.name());
    // (What the value moves on some paths only gets its own dot below.)
    let handed_on: BTreeSet<String> = moved.union(&moved_some).cloned().collect();
    let droppable: Vec<(usize, ResourceAccessPoint)> = self.droppable_locals_at(from, end, &handed_on, true, line, false)
      .into_iter().filter(|(_, rap)| !declared_outside(rap)).collect();
    // A Copy value or a reference declared in the branch the exit is in
    // (in the outermost one, if they nest) has nothing to drop, but its
    // scope ends here too: the `}` that would end it is never reached
    // on this path (#1348). Not one declared outside every branch (the
    // function's own, or the frame `drop_locals_at_fn_return` stands
    // in): that keeps its scope end at the function's end, as on the
    // paths that don't leave.
    let branch_from = self.exit_frames.iter().enumerate().find_map(|(i, f)| match f {
      ExitFrame::Branch { len, fn_level: false, .. } => Some((i, *len)),
      _ => None,
    });
    let scoped: Vec<ResourceAccessPoint> = match branch_from {
      None => Vec::new(),
      Some((frame_at, branch_from)) => {
        // (A `let n;` declared before the branch, first given a value in
        // it, is the enclosing code's, #930.)
        let before_branch = |rap: &ResourceAccessPoint| self.deferred_decl_at
          .get(rap.name().split('.').next().unwrap_or("")).is_some_and(|at| *at <= frame_at);
        let mut seen: HashSet<&str> = HashSet::new();
        self.in_scope[from.max(branch_from).min(end)..end].iter().rev()
          .filter(|n| seen.insert(n.as_str()))
          .filter_map(|n| self.raps.get(n))
          .filter(|rd| rd.rap.never_drops() && !rd.rap.is_fn() && !rd.rap.is_static() && !rd.rap.is_temporary()
            && !handed_on.iter().any(|m| place_within(rd.rap.name(), m))
            && !self.skip_raps.contains(rd.rap.name())
            && !declared_outside(&rd.rap)
            && !before_branch(&rd.rap)
            // (A reference holds a value once it has a lender.)
            && if rd.rap.is_ref() { self.borrow_map.contains_key(rd.rap.name()) } else { self.ownership_here(*rd.rap.hash(), false) != OwnHere::No })
          .map(|rd| rd.rap.clone())
          .collect()
      }
    };
    // `process::exit` ends the process: nothing is dropped, and each
    // local's column ends here with a dot that says so (#1172). (A
    // temporary or a tuple scrutinee isn't dropped either; they have
    // no column to end.)
    // (So does a call to a function of the user's that never returns and
    // doesn't always panic, `no_drop_exit`: it says how, #1553.)
    let process_exit = no_drop_exit.or_else(|| match how {
      "process::exit" | "process::abort" => Some(NoDropExit { call: how.to_owned(), how: NeverReturns::Exits }),
      _ => None,
    });
    if process_exit.is_some() {
      for (_, rap) in droppable {
        if rap.is_temporary() {
          continue;
        }
        let id = *self.unique_id;
        *self.unique_id += 1;
        self.exit_drops.insert(id);
        let early = Some(EarlyReturns { process_exit: process_exit.clone(), ..EarlyReturns::default() });
        self.add_external_event(line, ExternalEvent::GoOutOfScope { ro: rap, id, early });
      }
      for rap in scoped {
        let id = *self.unique_id;
        *self.unique_id += 1;
        self.exit_drops.insert(id);
        self.exit_scope_ended.insert(rap.name().to_string());
        let early = Some(EarlyReturns { process_exit: process_exit.clone(), ..EarlyReturns::default() });
        self.add_external_event(line, ExternalEvent::GoOutOfScope { ro: rap, id, early });
      }
      return;
    }
    let droppable: Vec<(usize, ResourceAccessPoint)> = droppable.into_iter()
      .filter(|(_, rap)| !part_maybe_moved(rap)).collect();
    for (idx, rap) in droppable {
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
          ExternalEvent::TemporaryDrop { ro, ty, literal, part, .. } if ro == &rap => ty.clone().map(|ty| (ty, *literal, part.clone())),
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
        Some((ty, literal, part)) => {
          let early = self.unbranched_temps.contains(&rap);
          self.add_external_event(line, ExternalEvent::TemporaryDrop { ro: rap, ty: Some(ty), when, literal, early, part, id })
        }
        None if extended_temp => self.add_external_event(line, ExternalEvent::TemporaryDrop { ro: rap, ty: None, when, literal: false, early: false, part: None, id }),
        None => self.add_external_event(line, ExternalEvent::GoOutOfScope { ro: rap, id, early: None }),
      }
    }
    while let Some(t) = tuples.pop() {
      self.drop_tuple_on_exit(&t, line);
    }
    for rap in scoped {
      let id = *self.unique_id;
      *self.unique_id += 1;
      self.exit_drops.insert(id);
      self.exit_scope_ended.insert(rap.name().to_string());
      self.add_external_event(line, ExternalEvent::GoOutOfScope { ro: rap, id, early: None });
    }
    // Innermost first, like the drops above. Not `exit_drops`: the
    // column doesn't end in this dot, whose drop happens on some paths
    // only (#697).
    let mut seen: HashSet<&str> = HashSet::new();
    let maybe_dropped: Vec<ResourceAccessPoint> = self.scope_names(from, end).into_iter().rev()
      .map(|(_, n)| n)
      .filter(|n| seen.insert(n))
      .filter(|n| moved_some.iter().any(|m| place_within(n, m)))
      .filter_map(|n| self.raps.get(n))
      .filter(|rd| !rd.rap.never_drops() && !rd.rap.is_fn() && !rd.rap.is_ref() && !self.skip_raps.contains(&rd.rap.name().to_string()))
      .map(|rd| rd.rap.clone())
      .collect();
    for rap in maybe_dropped {
      let id = *self.unique_id;
      *self.unique_id += 1;
      self.add_external_event(line, ExternalEvent::OwnerDropUnlessReturned { ro: rap, is_break: how == "break", id });
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

  /// `in_scope[from..end]`'s names, each with its index, in order. The
  /// slot a `let x;` local (or a branch-extended temporary) holds from
  /// its `let` (`deferred_slot`) stands for the columns its assignment
  /// registered (`deferred_slots`), once it has been assigned: an exit
  /// after the branch that assigned it, whose end took its own entry out
  /// of `in_scope`, drops it if it holds a value (#1436). Not yet
  /// assigned, it stands for nothing.
  fn scope_names(&self, from: usize, end: usize) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    for (i, n) in self.in_scope.iter().enumerate().take(end).skip(from) {
      if !is_deferred_slot(n) {
        out.push((i, n.as_str()));
      } else if let Some(names) = self.deferred_slots.get(n) {
        out.extend(names.iter().map(|m| (i, m.as_str())));
      }
    }
    out
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
    for (idx, name) in self.scope_names(from, end).into_iter().rev().filter(|(_, n)| seen.insert(n)) {
      let Some(rd) = self.raps.get(name) else { continue };
      if rd.rap.never_drops() || rd.rap.is_fn() || (!refs && rd.rap.is_ref())
        || moved.iter().any(|m| place_within(name, m)) || self.skip_raps.contains(name) {
        continue;
      }
      // A temporary (#400, #457) stays in `in_scope` until its block
      // ends, but one whose own scope (its statement, for an unbound
      // one) ended above was dropped there already.
      if rd.rap.is_temporary() && rd.scope < line {
        continue;
      }
      // One its drop flag decides about (moved, or given a value, on
      // some paths only) is included too: the exit drops it if it
      // still holds a value (#1194).
      if self.ownership_here(*rd.rap.hash(), binds) == OwnHere::No {
        continue;
      }
      out.push((idx, rd.rap.clone()));
    }
    out
  }

  /// When `e?` returns early, Rust drops what the scopes it leaves still
  /// own (#430, #1656). This is conditional, so it isn't a drop that ends a column:
  /// each such owner gets a dot saying so, on the line of the `?`.
  ///
  /// `r?` with `r` a non-`Copy` variable (or field) moves `r` first, on
  /// both paths (#652): unless `match_rhs` drew that move into the
  /// variable `r?` is bound to, it is drawn here as a move into the
  /// `?`. `r` isn't dropped by the early return: the residual in it goes
  /// on with the early return (to the caller, or to an enclosing `try` block).
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
          carried_with: None, via_try: Some(*kind),
          closure_value: false,
          id,
        });
        self.try_moves.insert(try_expr.hir_id, Some(id));
      }
    }
    // rustc lowers the residual arm to either a return or a break to the
    // exact enclosing try block. The operand move above happens in both cases.
    let ExprKind::Match(_, arms, _) = try_expr.kind else { return };
    let block = arms.iter().find_map(|arm| match arm.body.kind {
      ExprKind::Break(dest, _) => dest.target_id.ok(),
      _ => None,
    });
    let (locals_from, tuples_from, iterators_from) = if let Some(id) = block {
      let Some(scope) = self.try_block.filter(|s| s.id == id) else { return };
      (scope.locals, scope.tuples, scope.iterators)
    } else {
      (0, 0, 0)
    };
    if !self.in_closure(try_expr) || block.is_some() {
      self.note_try_loop_iterators(line, iterators_from);
    }
    if block.is_none() && !self.in_closure(try_expr) {
      // What it leaves in scope if it returns early, for the scope ends
      // said to happen at a later `return` (#1044).
      let vars = self.scope_names(0, self.in_scope.len()).into_iter().filter_map(|(_, name)| self.raps.get(name))
        .filter(|rd| !rd.rap.is_fn() && !(rd.rap.is_temporary() && rd.scope < line))
        .map(|rd| *rd.rap.hash()).collect();
      self.fn_tries.push((line, vars));
    }
    let end = self.in_scope.len();
    let mut tuples = self.live_tuples[tuples_from..].to_vec();
    let tuple_dup = self.preprocessed_events.iter().any(|(l, e)| *l == line
      && matches!(e, ExternalEvent::TupleTempDrop { at: TupleDropAt::TryReturns, .. }));
    if tuple_dup {
      tuples.clear();
    }
    // An outer `let x;` first assigned inside the block also registered
    // a column inside it. Its declaration still owns its drop scope.
    let outer = self.scope_names(0, locals_from).into_iter().map(|(_, name)| name.to_owned()).collect();
    for (idx, rap) in self.droppable_locals_at(locals_from, end, &outer, false, line, true) {
      while tuples.last().is_some_and(|t| idx < t.len) {
        let t = tuples.pop().unwrap();
        self.drop_tuple_if_try_returns(&t, line);
      }
      if moved.as_ref().is_some_and(|(r, _, _)| r.hash() == rap.hash()) {
        continue;
      }
      // Nothing to drop if it returns early: no dot (#1137). (An exit's
      // scope end still ends such a column; a `?` ends none.)
      if rap.drops_nothing() {
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

  /// Whether the `return` `ret` sits directly in a loop body: only
  /// statements and blocks, or bodies drawn inline (an `if let` whose
  /// pattern always matches, a one-arm `match`), between them, not an
  /// `if` / `match` drawn as a branch, a closure or a `let`'s value. Its
  /// drops are then drawn nowhere near it (#1020): the loop body is drawn
  /// inline, and may not run, so they stay at the scope ends.
  fn directly_in_loop_body(&self, ret: &'tcx Expr<'tcx>) -> bool {
    use rustc_hir::Node;
    use rustc_span::DesugaringKind;
    let mut child = ret.hir_id;
    for (id, node) in self.tcx.hir_parent_iter(ret.hir_id) {
      let from = std::mem::replace(&mut child, id);
      match node {
        Node::Stmt(_) | Node::Block(_) | Node::Arm(_) => {}
        Node::Expr(Expr { kind: ExprKind::Loop(..), .. }) => return true,
        Node::Expr(Expr { kind: ExprKind::Block(..) | ExprKind::DropTemps(_), .. }) => {}
        // A `while` loop's body is the then-branch of the `if` it
        // desugars to.
        Node::Expr(x @ Expr { kind: ExprKind::If(c, then, _), .. }) => {
          let inline = if x.span.is_desugaring(DesugaringKind::WhileLoop) {
            true
          } else {
            !x.span.from_expansion() && self.always_matching_if_let(c)
          };
          if !inline || then.hir_id != from {
            return false;
          }
        }
        // A `for` loop's body is the `Some(pat) =>` arm of the `match`
        // it desugars to.
        Node::Expr(Expr { kind: ExprKind::Match(_, _, rustc_hir::MatchSource::ForLoopDesugar), .. }) => {}
        Node::Expr(x @ Expr { kind: ExprKind::Match(_, [_], rustc_hir::MatchSource::Normal), .. })
          if !x.span.from_expansion() => {}
        _ => return false,
      }
    }
    false
  }

  /// A `return` directly in a loop body (`directly_in_loop_body`): its
  /// drops aren't drawn, so the scope ends after it, where they are,
  /// say they happen at the `return` instead (#1020). Recorded here, and
  /// marked on those events once the function is visited
  /// (`mark_loop_returns`). A variable the loop body declared never
  /// reaches its closing brace on a path through the body (it is dropped
  /// at the `return`, or at an exit drawn with its own drops); one
  /// declared before the loop, the loop's iterator and its head's
  /// temporaries are dropped at the `return` only if it runs (the loop
  /// body may not).
  fn note_return_in_loop(&mut self, ret: &'tcx Expr<'tcx>) {
    if self.in_closure(ret) || ret.span.from_expansion() || !self.directly_in_loop_body(ret) {
      return;
    }
    let Some(len) = self.exit_frames.iter().rev().find_map(|f| match f {
      ExitFrame::Loop { len, .. } => Some(*len),
      ExitFrame::Branch { .. } => None,
    }) else { return };
    let line = expr_to_line(ret, &self.tcx);
    let mut seen: HashSet<&str> = HashSet::new();
    let mut vars = Vec::new();
    for (idx, name) in self.scope_names(0, self.in_scope.len()).into_iter().rev() {
      if !seen.insert(name) {
        continue;
      }
      let Some(rd) = self.raps.get(name) else { continue };
      if rd.rap.is_fn() || self.skip_raps.contains(name) || (rd.rap.is_temporary() && rd.scope < line) {
        continue;
      }
      // An owner declared before the loop that holds nothing where the
      // `return` is (`drop(r); return;`) drops nothing there, so its
      // scope end after the loop (reached by a `break`, where it still
      // held a value, #1000) isn't one the `return` could come before.
      if idx < len && matches!(rd.rap, ResourceAccessPoint::Owner(_)) && !rd.rap.is_temporary()
        && self.rap_ownership_now(name) == Ownership::None {
        continue;
      }
      vars.push((rd.rap.clone(), idx >= len));
    }
    let iters = self.loop_iterators.iter().map(|(_, ro)| ro.clone()).collect();
    self.loop_returns.push(LoopReturn { line, vars, iters });
  }

  /// Whether `l` is a `loop` (not a `for` / `while`, not from a macro)
  /// that nothing breaks out of: no `break` inside it targets it or
  /// anything outside it, and no `continue` targets anything outside it.
  /// It never ends; only a `return` (or a panic) leaves it (#1041).
  fn loop_never_ends(&self, l: &'tcx Expr<'tcx>) -> bool {
    use rustc_hir::LoopSource;
    if !matches!(l.kind, ExprKind::Loop(_, _, LoopSource::Loop, _)) || l.span.from_expansion() {
      return false;
    }
    struct Leaves<'tcx> { tcx: rustc_middle::ty::TyCtxt<'tcx>, typeck: &'tcx rustc_middle::ty::TypeckResults<'tcx>, l: HirId, found: bool }
    impl<'tcx> Visitor<'tcx> for Leaves<'tcx> {
      fn visit_expr(&mut self, e: &'tcx Expr<'tcx>) {
        let inside = |t: HirId| self.tcx.hir_parent_iter(t).any(|(id, _)| id == self.l);
        match e.kind {
          ExprKind::Break(dest, _) => self.found |= !dest.target_id.is_ok_and(inside),
          ExprKind::Continue(dest) => self.found |= !dest.target_id.is_ok_and(|t| t == self.l || inside(t)),
          _ => {}
        }
        intravisit::walk_expr(self, e);
      }
      // Not what comes after a statement that never completes (`loop {
      // return; break; }`): it is never reached (#1518).
      fn visit_block(&mut self, b: &'tcx rustc_hir::Block<'tcx>) {
        for st in b.stmts {
          self.visit_stmt(st);
          if matches!(st.kind, StmtKind::Expr(x) | StmtKind::Semi(x) if self.typeck.expr_ty_opt(x).is_some_and(|t| t.is_never())) {
            return;
          }
        }
        if let Some(t) = b.expr {
          self.visit_expr(t);
        }
      }
    }
    let mut v = Leaves { tcx: self.tcx, typeck: self.tcx.typeck(l.hir_id.owner), l: l.hir_id, found: false };
    intravisit::walk_expr(&mut v, l);
    !v.found
  }

  /// Whether the `return` `ret` sits directly in the body of a `loop`
  /// that never ends (`loop_never_ends`), which itself sits directly in
  /// the function's body: only statements, plain blocks, `let` values and
  /// other such loops between them, no branch, `for` / `while` loop or
  /// closure. The loop body always runs, and a `return` is the only way
  /// out, so this one leaves the function whenever the code gets to it:
  /// its drops are drawn on its row, and nothing after the loop runs, as
  /// for a body drawn inline that always returns (#1004, #1041).
  fn returns_from_breakless_loop(&self, ret: &'tcx Expr<'tcx>) -> bool {
    use rustc_hir::Node;
    if self.in_closure(ret) || ret.span.from_expansion()
      || self.exit_frames.iter().any(|f| matches!(f, ExitFrame::Branch { .. }))
    {
      return false;
    }
    let mut in_loop = false;
    let mut child = ret.hir_id;
    for (id, node) in self.tcx.hir_parent_iter(ret.hir_id) {
      let from = std::mem::replace(&mut child, id);
      match node {
        Node::Stmt(_) | Node::Block(_) => {}
        Node::LetStmt(LetStmt { init: Some(init), .. }) if in_loop && init.hir_id == from => {}
        Node::Expr(Expr { kind: ExprKind::Block(_, None) | ExprKind::DropTemps(_), .. }) => {}
        Node::Expr(l) if self.loop_never_ends(l) => in_loop = true,
        Node::Item(_) | Node::ImplItem(_) | Node::TraitItem(_) => return in_loop,
        _ => return false,
      }
    }
    false
  }

  /// Whether the panic `call` sits where every path that gets to it
  /// leaves the function there: in no branch, through statements, plain
  /// blocks, `let` values, labeled blocks nothing breaks out of and
  /// `loop`s that never end, up to the function's body (#1545). Like
  /// `returns_from_breakless_loop`, but also directly in the body
  /// (where a `return` is `returns_before_body_end`'s), and through the
  /// blocks the `panic!` macro expands to.
  fn panic_leaves_every_path(&self, call: &'tcx Expr<'tcx>) -> bool {
    use rustc_hir::Node;
    if self.in_closure(call) || self.exit_frames.iter().any(|f| matches!(f, ExitFrame::Branch { .. })) {
      return false;
    }
    let mut child = call.hir_id;
    for (id, node) in self.tcx.hir_parent_iter(call.hir_id) {
      let from = std::mem::replace(&mut child, id);
      match node {
        Node::Stmt(_) | Node::Block(_) => {}
        Node::LetStmt(LetStmt { init: Some(init), .. }) if init.hir_id == from => {}
        Node::Expr(Expr { kind: ExprKind::Block(_, None) | ExprKind::DropTemps(_), .. }) => {}
        Node::Expr(e @ Expr { kind: ExprKind::Block(b, Some(_)), .. }) if breaks_to_id(e, b.hir_id).is_empty() => {}
        Node::Expr(l) if self.loop_never_ends(l) => {}
        Node::Item(_) | Node::ImplItem(_) | Node::TraitItem(_) => return true,
        _ => return false,
      }
    }
    false
  }

  /// A `loop` that never ends (`loop_never_ends`): the closing braces
  /// after it on its own path are never reached: those of the blocks it
  /// is a statement (or the tail) of, and on up through plain blocks,
  /// `let` values and enclosing loops like it, up to anything that may
  /// run without it (an `if` / `match`, a labelled block, a loop that can
  /// end). Where the loop sits directly in the function's body, its
  /// `return`s drop everything on their rows (`returns_from_breakless_loop`)
  /// and those braces draw nothing; elsewhere (in a `for` body, say), a
  /// variable in scope at a `return` in it whose scope ends at one of them
  /// is always dropped at a `return`, which its scope end says (#1041).
  ///
  /// Each brace remembers the innermost such loop that keeps the code
  /// from reaching it (a loop nested in the one recorded comes later;
  /// one after it never runs): while it keeps running, its own `}` is
  /// the last row the code reaches, where the variables still in scope
  /// are drawn as alive (#1057).
  fn note_breakless_loop(&mut self, l: &'tcx Expr<'tcx>) {
    use rustc_hir::Node;
    if self.in_closure(l) || !self.loop_never_ends(l) {
      return;
    }
    let this = EndlessLoop {
      span: l.span,
      first: span_to_line(&l.span, &self.tcx),
      last: span_end_line(&l.span, &self.tcx),
      exits: self.loop_exits(l),
      runs: self.loop_repeats(l),
    };
    let mut child = l.hir_id;
    for (id, node) in self.tcx.hir_parent_iter(l.hir_id) {
      let from = std::mem::replace(&mut child, id);
      match node {
        Node::Stmt(_) => {}
        Node::LetStmt(LetStmt { init: Some(init), .. }) if init.hir_id == from => {}
        Node::Block(b) => {
          let at = self.dead_ends.entry(span_end_line(&b.span, &self.tcx)).or_insert(this);
          if at.span.contains(this.span) {
            *at = this;
          }
        }
        Node::Expr(Expr { kind: ExprKind::Block(_, None) | ExprKind::DropTemps(_), .. }) => {}
        Node::Expr(m) if self.loop_never_ends(m) => {}
        _ => break,
      }
    }
  }

  /// Whether the `loop` `l` can start its body again: the body can reach
  /// its end (no statement in it, nor its tail, has type `!`: a `return`,
  /// a panic, a loop that never ends), or a `continue` goes back to it
  /// (#1057). One whose body always returns (#1041) never repeats.
  fn loop_repeats(&self, l: &'tcx Expr<'tcx>) -> bool {
    let ExprKind::Loop(body, ..) = l.kind else { return true };
    let typeck = self.tcx.typeck(l.hir_id.owner);
    let never = |e: &Expr<'tcx>| typeck.expr_ty_opt(e).is_some_and(|t| t.is_never());
    let diverges = body.stmts.iter().any(|st| match st.kind {
      StmtKind::Expr(e) | StmtKind::Semi(e) => never(e),
      StmtKind::Let(LetStmt { init: Some(e), .. }) => never(e),
      _ => false,
    }) || body.expr.is_some_and(never);
    if !diverges {
      return true;
    }
    struct Continues { l: HirId, found: bool }
    impl<'tcx> Visitor<'tcx> for Continues {
      fn visit_expr(&mut self, e: &'tcx Expr<'tcx>) {
        if let ExprKind::Continue(dest) = e.kind {
          self.found |= dest.target_id == Ok(self.l);
        }
        intravisit::walk_expr(self, e);
      }
    }
    let mut v = Continues { l: l.hir_id, found: false };
    intravisit::walk_expr(&mut v, l);
    v.found
  }

  /// What can leave the `loop` `l` that nothing breaks out of (#1057):
  /// a `return` or a `?` (not in a closure), or a call that never
  /// returns (a panic) drop what is in scope; `std::process::exit` /
  /// `abort` drop nothing. With none of those, the loop never ends
  /// (`LoopExits::None`) only if nothing in it may panic either (a call,
  /// arithmetic that may overflow, indexing); otherwise what leaves it
  /// isn't drawn (`LoopExits::Other`).
  fn loop_exits(&self, l: &'tcx Expr<'tcx>) -> LoopExits {
    struct Exits<'a, 'tcx> {
      tcx: rustc_middle::ty::TyCtxt<'tcx>,
      typeck: &'a rustc_middle::ty::TypeckResults<'tcx>,
      dropping: bool,
      other: bool,
    }
    impl<'a, 'tcx> Visitor<'tcx> for Exits<'a, 'tcx> {
      fn visit_expr(&mut self, e: &'tcx Expr<'tcx>) {
        use rustc_hir::BinOpKind::*;
        match e.kind {
          // A `return` or `?` in a closure leaves the closure.
          ExprKind::Closure(..) => return,
          ExprKind::Ret(_) => self.dropping = true,
          ExprKind::Call(f, _) => {
            let never = self.typeck.expr_ty_opt(e).is_some_and(|t| t.is_never());
            let callee = match f.kind {
              ExprKind::Path(ref qp) => self.typeck.qpath_res(qp, f.hir_id).opt_def_id(),
              _ => None,
            };
            // (Nor a function of the user's that never returns without
            // panicking, or may not panic, #1553.)
            let quits = callee.is_some_and(|d| std_exit_fn(self.tcx, d).is_some()
              || matches!(never_fn_kind(self.tcx, d), Some(NeverFn::NoDrop(_))));
            if never && !quits {
              self.dropping = true;
            } else {
              self.other = true;
            }
          }
          ExprKind::MethodCall(..) | ExprKind::Index(..) | ExprKind::Unary(rustc_hir::UnOp::Neg, _) => {
            self.dropping |= self.typeck.expr_ty_opt(e).is_some_and(|t| t.is_never());
            self.other = true;
          }
          ExprKind::Binary(op, ..) if matches!(op.node, Add | Sub | Mul | Div | Rem | Shl | Shr) => self.other = true,
          ExprKind::AssignOp(op, ..) if matches!(rustc_hir::BinOpKind::from(op.node), Add | Sub | Mul | Div | Rem | Shl | Shr) => self.other = true,
          _ => {}
        }
        intravisit::walk_expr(self, e);
      }
    }
    let typeck = self.tcx.typeck(l.hir_id.owner);
    let mut v = Exits { tcx: self.tcx, typeck, dropping: false, other: false };
    intravisit::walk_expr(&mut v, l);
    if v.dropping {
      LoopExits::Dropping
    } else if v.other {
      LoopExits::Other
    } else {
      LoopExits::None
    }
  }

  /// Marks the scope ends that the `return`s `note_return_in_loop`
  /// recorded come before (#1020), once the function is visited:
  /// - the `GoOutOfScope` of each variable in scope at one of them that
  ///   comes after it (not one at an exit drawn with its own drops, in
  ///   `exit_ids`, nor that of a later variable sharing the column, #589):
  ///   for a variable the loop body declared, its first, at the body's
  ///   end; one the code never reaches (`dead_ends`, #1041) is always
  ///   dropped at one of them instead; and one that is always dropped
  ///   at them names each `?` before them it is in scope at (`fn_tries`,
  ///   #1044), which may return first;
  /// - moves the `GoOutOfScope` of a variable the code never reaches
  ///   that none of them comes before (#1043) to the closing `}` of the
  ///   loop that keeps the code from reaching it, the last row the code
  ///   reaches while the loop keeps running (`runs_on`, #1057): the loop
  ///   is left only at exits drawn with their own drops, or never;
  /// - the drop of a `for` loop's head temporary where the loop ends;
  /// - the drop of a `for` loop's iterator variable where its loop ends.
  pub fn mark_loop_returns(&mut self, exit_ids: &HashSet<usize>) {
    let returns = std::mem::take(&mut self.loop_returns);
    let dead = std::mem::take(&mut self.dead_ends);
    let tries = std::mem::take(&mut self.fn_tries);
    if returns.is_empty() && dead.is_empty() {
      return;
    }
    // Each scope end of a variable: its line, and whether it is a later
    // variable's of the same name (#589).
    let shared_lines = |ro: &ResourceAccessPoint| self.raps.get(ro.name())
      .filter(|rd| rd.rap.hash() == ro.hash()).map(|rd| rd.extra_scopes.clone()).unwrap_or_default();
    let mut ends: Vec<(usize, u64)> = Vec::new();
    {
      let mut collect = |line: usize, ev: &mut ExternalEvent| {
        if let ExternalEvent::GoOutOfScope { ro, id, .. } = ev {
          if !exit_ids.contains(id) {
            ends.push((line, *ro.hash()));
          }
        }
      };
      walk(self.preprocessed_events.as_mut_slice(), &mut collect);
      walk_map(&mut *self.event_line_map, &mut collect);
    }
    // (return line, variable) -> the line of the scope end it comes
    // before: every one after it, or (`always`) the first.
    let ends_after = |r: &LoopReturn, ro: &ResourceAccessPoint, always: bool, line: usize| {
      if line <= r.line || shared_lines(ro).contains(&line) {
        return false;
      }
      !always || ends.iter().filter(|(l, h)| h == ro.hash() && *l > r.line).map(|(l, _)| *l).min() == Some(line)
    };
    // Whether the variable `h` is moved on the lines `first..=last` (at
    // an exit of the loop there, #1057).
    let mut moves: Vec<(usize, u64)> = Vec::new();
    {
      let mut collect = |line: usize, ev: &mut ExternalEvent| {
        if let ExternalEvent::Move { from: ResourceTy::Value(from), .. } = ev {
          moves.push((line, *from.hash()));
        }
      };
      walk(self.preprocessed_events.as_mut_slice(), &mut collect);
      walk_map(&mut *self.event_line_map, &mut collect);
    }
    let moved_in = |h: &u64, first: usize, last: usize| moves.iter().any(|(l, m)| m == h && (first..=last).contains(l));
    // The scope ends the code never reaches that nothing else marks
    // (#1043), by id: moved below to the line of the closing `}` of the
    // loop that keeps the code from reaching them (#1057).
    let mut unreached: HashMap<usize, usize> = HashMap::new();
    // Those of an owner after a loop that never repeats: removed.
    let mut removed: HashSet<usize> = HashSet::new();
    let mut mark = |line: usize, ev: &mut ExternalEvent| match ev {
      ExternalEvent::GoOutOfScope { ro, early, id, .. } if !exit_ids.contains(id)
        && !early.as_ref().is_some_and(|e| e.process_exit.is_some()) => {
        let mut e = EarlyReturns::default();
        for r in &returns {
          match r.vars.iter().find(|(v, _)| v.hash() == ro.hash()) {
            Some((_, true)) if e.always.is_empty() && ends_after(r, ro, true, line) => e.always.push(r.line),
            Some((_, false)) if ends_after(r, ro, false, line) => e.maybe.push(r.line),
            _ => {}
          }
        }
        // Never reached (after a `loop` nothing breaks out of): every
        // path that leaves through the loop does so at one of the
        // `return`s (#1041).
        if dead.contains_key(&line) && !e.maybe.is_empty() {
          e.always.append(&mut e.maybe);
          e.always.sort_unstable();
          e.always.dedup();
        }
        // A `?` it is in scope at, before the last of those `return`s,
        // may return first and drop it there instead (#1044).
        if let Some(&last) = e.always.iter().max() {
          e.tries = tries.iter().filter(|(l, vars)| *l < last && vars.contains(ro.hash())).map(|(l, _)| *l).collect();
          e.tries.sort_unstable();
          e.tries.dedup();
        }
        if e != EarlyReturns::default() {
          *early = Some(e);
        } else if dead.get(&line).is_some_and(|lp| !lp.runs) {
          // Its loop never starts its body again (it always returns,
          // #1041): an owner is dropped at those exits, and a reference
          // or Copy value keeps its plain scope end, as after a
          // conditional whose every branch leaves (#515).
          if !ro.never_drops() && !ro.is_ref() {
            removed.insert(*id);
          }
        } else if let Some(lp) = dead.get(&line) {
          // Never reached, and no `return` directly in the loop: the
          // loop is left only at exits drawn with their own drops (a
          // `return` / panic in a branch, a `?`), or never. Like after
          // a conditional whose every branch leaves (#515), no scope
          // end drops it here (#1043). It is still in scope while the
          // loop keeps running: its column runs on to the loop's `}`
          // (#1057).
          let moved = moved_in(ro.hash(), lp.first, lp.last);
          *early = Some(EarlyReturns { runs_on: Some(LoopRunsOn { exits: lp.exits, moved }), ..EarlyReturns::default() });
          unreached.insert(*id, lp.last);
        }
      }
      // A `for` loop's head temporary, where the loop ends.
      ExternalEvent::TemporaryDrop { ro, when, id, .. } if !exit_ids.contains(id) => {
        let lines: Vec<usize> = returns.iter()
          .filter(|r| r.line < line && r.vars.iter().any(|(v, always)| v.hash() == ro.hash() && !always))
          .map(|r| r.line).collect();
        let suffix = crate::svg_generator::hover_messages::or_at_returns(&lines);
        if !lines.is_empty() && !when.ends_with(&suffix) {
          when.push_str(&suffix);
        }
      }
      ExternalEvent::LoopIteratorDrop { ro, left: LoopLeft::End, early, .. } => {
        // (Events of the functions visited before are walked too: leave
        // theirs alone.)
        let lines: Vec<usize> = returns.iter().filter(|r| r.line < line && r.iters.iter().any(|i| i.hash() == ro.hash())).map(|r| r.line).collect();
        if !lines.is_empty() {
          *early = lines;
        }
      }
      _ => {}
    };
    walk(self.preprocessed_events.as_mut_slice(), &mut mark);
    walk_map(&mut *self.event_line_map, &mut mark);
    if !unreached.is_empty() {
      move_ids(&mut self.preprocessed_events, &mut *self.event_line_map, &unreached);
    }
    if !removed.is_empty() {
      drop_ids(&mut self.preprocessed_events, &mut *self.event_line_map, &removed);
    }

    // Every event, those in branches too (with each branch's line map).
    fn walk(events: &mut [(usize, ExternalEvent)], f: &mut impl FnMut(usize, &mut ExternalEvent)) {
      for (line, ev) in events.iter_mut() {
        walk_one(*line, ev, f);
      }
    }
    fn walk_map(map: &mut BTreeMap<usize, Vec<ExternalEvent>>, f: &mut impl FnMut(usize, &mut ExternalEvent)) {
      for (line, evs) in map.iter_mut() {
        for ev in evs.iter_mut() {
          walk_one(*line, ev, f);
        }
      }
    }
    // Removes the events `ids`, those in branches too.
    fn drop_ids(events: &mut Vec<(usize, ExternalEvent)>, map: &mut BTreeMap<usize, Vec<ExternalEvent>>, ids: &HashSet<usize>) {
      events.retain(|(_, e)| !ids.contains(&e.get_id()));
      for evs in map.values_mut() {
        evs.retain(|e| !ids.contains(&e.get_id()));
      }
      let branches = events.iter_mut().map(|(_, e)| e).chain(map.values_mut().flatten());
      for ev in branches {
        if let ExternalEvent::Branch { branches, .. } = ev {
          for b in branches.iter_mut() {
            drop_ids(&mut b.e_data, &mut b.line_map, ids);
          }
        }
      }
    }
    // Moves each event of `to` (by id) to its line there, in branches
    // too.
    fn move_ids(events: &mut [(usize, ExternalEvent)], map: &mut BTreeMap<usize, Vec<ExternalEvent>>, to: &HashMap<usize, usize>) {
      for (line, e) in events.iter_mut() {
        if let Some(&l) = to.get(&e.get_id()) {
          *line = l;
        }
      }
      let mut moved: Vec<(usize, ExternalEvent)> = Vec::new();
      for evs in map.values_mut() {
        let (go, stay): (Vec<_>, Vec<_>) = std::mem::take(evs).into_iter().partition(|e| to.contains_key(&e.get_id()));
        *evs = stay;
        moved.extend(go.into_iter().map(|e| (to[&e.get_id()], e)));
      }
      for (l, e) in moved {
        map.entry(l).or_default().push(e);
      }
      let branches = events.iter_mut().map(|(_, e)| e).chain(map.values_mut().flatten());
      for ev in branches {
        if let ExternalEvent::Branch { branches, .. } = ev {
          for b in branches.iter_mut() {
            move_ids(&mut b.e_data, &mut b.line_map, to);
          }
        }
      }
    }
    fn walk_one(line: usize, ev: &mut ExternalEvent, f: &mut impl FnMut(usize, &mut ExternalEvent)) {
      if let ExternalEvent::Branch { branches, .. } = ev {
        for b in branches.iter_mut() {
          walk(&mut b.e_data, f);
          walk_map(&mut b.line_map, f);
        }
      } else {
        f(line, ev);
      }
    }
  }

  /// A `?` inside a `for` loop over an iterator variable returns early
  /// on `Err` / `None`, dropping that iterator with the rest (#649).
  /// Conditional, so it ends no column (like `note_try`'s dots).
  fn note_try_loop_iterators(&mut self, line: usize, from: usize) {
    for (_, ro) in self.loop_iterators[from..].to_vec() {
      let dup = self.preprocessed_events.iter().any(|(l, e)| *l == line
        && matches!(e, ExternalEvent::LoopIteratorDrop { ro: r, left: LoopLeft::Try, .. } if *r == ro));
      if !dup {
        self.add_loop_iterator_drop(ro, line, LoopLeft::Try);
      }
    }
  }

  /// A `break 'outer` / `continue 'outer` out of an inner `for` loop
  /// leaves it too (#592, #600): the iterator variables the loops inside
  /// the target took (`for s in it`) are dropped there, on their own
  /// columns, where
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
    self.add_external_event(line, ExternalEvent::LoopIteratorDrop { ro, left, id, early: Vec::new() });
    id
  }

  /// If `call` is a call to a panic function (what `panic!()`,
  /// `unreachable!()`, `todo!()`, `unimplemented!()` and
  /// `std::panic::panic_any` call), the line of the panic in the
  /// user's code. A panic unwinds the stack, dropping every local on
  /// the way (#410). `std::process::exit` also never returns, but
  /// runs no destructors, so it isn't one. Nor is every call to a
  /// function of the user's that returns `!`: one is a panic when its
  /// body always panics (`fn fail() -> ! { panic!("x") }`, #1553).
  /// So is a call of a local closure whose body always panics (`let f =
  /// || panic!("x"); f();`, #1562).
  fn panic_line(&self, call: &'tcx Expr<'tcx>) -> Option<usize> {
    let unwinds = match callee_def_id(self.tcx, call) {
      Some(def_id) => std_panic_fn(self.tcx, def_id) || never_fn_kind(self.tcx, def_id) == Some(NeverFn::Panics),
      None => closure_call_never_kind(self.tcx, call).is_some_and(|(k, _)| k == NeverFn::Panics),
    };
    unwinds.then(|| span_to_line(&call.span.source_callsite(), &self.tcx))
  }

  /// If `call` is a call to `std::process::exit` or `abort`, which end
  /// the process without running any destructor (#1172), the line of
  /// the call and how to name it. Or to a function of the user's that
  /// never returns and doesn't always panic (#1553): it ends the
  /// process, loops forever, or its body doesn't say which (it may
  /// panic).
  fn process_exit_line(&self, call: &'tcx Expr<'tcx>) -> Option<(usize, NoDropExit)> {
    // A local closure's call (#1562).
    let Some(def_id) = callee_def_id(self.tcx, call) else {
      let (NeverFn::NoDrop(how), name) = closure_call_never_kind(self.tcx, call)? else { return None };
      return Some((span_to_line(&call.span.source_callsite(), &self.tcx), NoDropExit { call: name, how }));
    };
    let exit = match std_exit_fn(self.tcx, def_id) {
      Some(name) => NoDropExit { call: name.to_owned(), how: NeverReturns::Exits },
      None => match never_fn_kind(self.tcx, def_id)? {
        NeverFn::Panics => return None,
        NeverFn::NoDrop(how) => NoDropExit { call: self.tcx.item_name(def_id).to_string(), how },
      },
    };
    Some((span_to_line(&call.span.source_callsite(), &self.tcx), exit))
  }

  /// Whether `e` never completes normally: its type is `!` (`return`,
  /// `panic!()`), or it is an `if`/`else` chain or `match` every branch
  /// of which diverges, or a block that ends in one (#491). rustc types
  /// `if c { return 1; } else { return 2; }` as `()`, not `!`, so the
  /// type alone misses a nested `else if` whose branches all return.
  pub fn expr_diverges(&self, e: &'tcx Expr<'tcx>) -> bool {
    let value = match e.kind { ExprKind::Assign(_, rhs, _) => rhs, _ => e };
    if self.unfinished_vec(value) { return true; }
    if self.tcx.typeck(e.hir_id.owner).expr_ty_opt(e).is_some_and(|ty| ty.is_never()) {
      return true;
    }
    match e.kind {
      // A local closure whose body never completes (#1562): rustc may
      // type the call `()` (before edition 2024 the closure's return type
      // falls back to `()`).
      ExprKind::Call(..) if closure_call_never_kind(self.tcx, e).is_some() => true,
      // An `if let` whose pattern always matches runs its body (#1013).
      ExprKind::If(c, then, _) if self.always_matching_if_let(c) => self.expr_diverges(then),
      ExprKind::If(_, then, Some(els)) => self.expr_diverges(then) && self.expr_diverges(els),
      ExprKind::Match(_, arms, _) => !arms.is_empty() && arms.iter().all(|a| self.expr_diverges(a.body)),
      // A `loop` whose only `break`s can't be reached (`loop { return;
      // break; }`, typed `()`, #1518).
      ExprKind::Loop(..) => self.loop_never_ends(e),
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
    let loop_id = self.exit_frames.iter().rev()
      .find_map(|f| match f {
        ExitFrame::Loop { id, .. } => Some(*id),
        ExitFrame::Branch { .. } => None,
      });
    let loop_lines = loop_id.map(|id| self.tcx.hir_span(id))
      .map(|sp| (sm.lookup_char_pos(sp.lo()).line, sm.lookup_char_pos(sp.hi()).line));
    self.exit_scopes.insert(*self.unique_id, ExitScope { in_closure: self.in_closure(expr), loop_lines, loop_id });
  }

  /// Whether a borrow made before the `if` without `else` `if_expr`
  /// (on lines `split..=merge`, `if_decl` its own variables), whose
  /// body always leaves by `break`, `return`, `continue` or a panic
  /// (#1573), may still be live after the `if` but end on the body's
  /// way out: then the `if` is drawn as a conditional on the borrowed
  /// columns, so that the end in its arm (`end_borrows_at_exits_in`,
  /// as for an `if`/`else`) has a place to go (#882). `for a in &v {
  /// ..; if c { break; } }`: the loan v's iterator holds lasts to the
  /// loop's end on the paths that go round again, but ends at the
  /// `break`.
  /// The `else` block of a let-else likewise (#1573): `at` is the
  /// expression the branch belongs to (the `if`, or the let-else's
  /// initializer).
  fn borrow_may_end_at_exit(&self, body: BranchBody<'tcx>, at: &'tcx Expr<'tcx>, split: usize, merge: usize, if_decl: &HashSet<ResourceAccessPoint>) -> bool {
    let leaves = branch_leaves(self.tcx, body);
    if !leaves.exits.iter().all(|x| matches!(x, BranchExit::Return | BranchExit::Panic | BranchExit::Break | BranchExit::Continue))
      || self.in_closure(at)
    {
      return false;
    }
    let sm = self.tcx.sess.source_map();
    let loop_lines = self.exit_frames.iter().rev()
      .find_map(|f| match f {
        ExitFrame::Loop { id, .. } => Some(self.tcx.hir_span(*id)),
        ExitFrame::Branch { .. } => None,
      })
      .map(|sp| (sm.lookup_char_pos(sp.lo()).line, sm.lookup_char_pos(sp.hi()).line));
    #[expect(clippy::disallowed_methods, reason = "if_decl is only tested with any; order doesn't matter")]
    let candidates: Vec<(&String, &RefData)> = self.in_scope.iter()
      .filter_map(|n| self.borrow_map.get(n).map(|rd| (n, rd)))
      .filter(|(n, rd)| rd.assigned_at < split
        && !if_decl.iter().any(|d| d.name() == n.as_str())
        && rd.lender.extract_rap().is_some_and(|l| !l.is_fn())
        // What each way out leaves (see `end_borrows_at_exits_in`).
        && leaves.exits.iter().all(|x| match x {
          BranchExit::Return | BranchExit::Panic => true,
          BranchExit::Break => loop_lines.is_some() && !leaves.break_carries.iter().any(|c| c == *n),
          // Not a `for` loop's pattern binding, whose loan the loop's
          // iterator holds into the next iteration (#872).
          BranchExit::Continue => loop_lines.is_some_and(|(lo, _)| lo < rd.assigned_at),
          _ => false,
        }))
      .collect();
    if candidates.is_empty() {
      return false;
    }
    // Where the borrow ends after the `if`: the last line its loan is
    // live on (rustc's), else the reference's own last live line. It
    // must be live after the `if` and, for a `break` / `continue`, end
    // in the loop (one used after the loop doesn't end at the `break`).
    let loans = self.gather_borrow_data(self.bwf);
    let in_loop = leaves.exits.iter().any(|x| matches!(x, BranchExit::Break | BranchExit::Continue));
    candidates.iter().any(|(n, rd)| {
      let end = loans.iter()
        .filter_map(|m| if Self::borrow_span_match(rd, m) { Some(m.region.1) } else { Self::borrow_match(rd, m) })
        .max()
        .or_else(|| self.last_live_line(n, (split, usize::MAX)));
      end.is_some_and(|end| end >= merge && (!in_loop || loop_lines.is_some_and(|(_, hi)| end <= hi)))
    })
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

  /// Whether the condition `c` of an `if` is a lone `let` whose pattern
  /// always matches (#995): the body always runs.
  fn always_matching_if_let(&self, c: &'tcx Expr<'tcx>) -> bool {
    matches!(c.kind, ExprKind::Let(l) if self.let_always_matches(l.pat, l.init))
  }

  /// Whether `e`, a body drawn inline (see `keep_inline_exit_drops`),
  /// sits directly in a branch of an `if` / `match` drawn as one: only
  /// statements and blocks, or other bodies drawn inline, between them,
  /// not a loop, a closure or a `let`'s value. An exit in it always
  /// leaves that branch, whose columns its drops end (#1013).
  fn inline_in_branch(&self, e: &'tcx Expr<'tcx>) -> bool {
    use rustc_hir::Node;
    let mut child = e.hir_id;
    for (id, node) in self.tcx.hir_parent_iter(e.hir_id) {
      let from = std::mem::replace(&mut child, id);
      match node {
        Node::Stmt(_) | Node::Block(_) => {}
        Node::Expr(Expr { kind: ExprKind::Block(..), .. }) => {}
        Node::Expr(x) if x.span.from_expansion() => return false,
        Node::Expr(Expr { kind: ExprKind::If(c, ..), .. }) => {
          if c.hir_id == from {
            return false;
          }
          // (One drawn inline too: look further out.)
          if !self.always_matching_if_let(c) {
            return true;
          }
        }
        Node::Arm(_) => {}
        Node::Expr(Expr { kind: ExprKind::Match(_, arms, rustc_hir::MatchSource::Normal), .. }) => {
          if arms.len() > 1 {
            return true;
          }
        }
        _ => return false,
      }
    }
    false
  }

  /// Whether the `return` `ret`, directly in the function's body (not in
  /// a branch, a loop or a closure), has something after it before the
  /// body's closing brace: it sits in a block of its own (`{ let x = f;
  /// return x.len(); }`), or statements follow it, which never run. Its
  /// drops are then drawn on its line, as for a body drawn inline that
  /// always returns (#1004, #1013); otherwise they are at the closing
  /// brace, the line after.
  fn returns_before_body_end(&self, ret: &'tcx Expr<'tcx>) -> bool {
    use rustc_hir::Node;
    if !self.exit_frames.is_empty() || self.in_closure(ret) || ret.span.from_expansion() || !self.at_fn_level(ret) {
      return false;
    }
    let mut cur = ret.hir_id;
    loop {
      match self.tcx.parent_hir_node(cur) {
        Node::Stmt(st) => cur = st.hir_id,
        Node::Block(b) => {
          let last = match b.expr {
            Some(t) => t.hir_id == cur,
            None => b.stmts.last().is_some_and(|st| st.hir_id == cur),
          };
          if !last {
            return true;
          }
          // The function's own body block: its `}` comes right after.
          return !matches!(self.tcx.parent_hir_node(self.tcx.parent_hir_id(b.hir_id)),
            Node::Item(_) | Node::ImplItem(_) | Node::TraitItem(_));
        }
        _ => return false,
      }
    }
  }

  /// A `return` for which `returns_before_body_end`: it drops every
  /// value the function still owns, on its line, and every path has left
  /// the function (`all_paths_exit`): nothing after it is drawn.
  fn drop_locals_at_fn_return(&mut self, ret: &'tcx Expr<'tcx>, value: Option<&'tcx Expr<'tcx>>) {
    let lo = *self.unique_id;
    self.exit_frames.push(ExitFrame::Branch { len: 0, hide: Vec::new(), binds: Vec::new(), binds_at: 0, fn_level: true });
    self.drop_locals_on_exit(expr_to_line(ret, &self.tcx), value, None, false, "return");
    self.exit_frames.pop();
    self.keep_inline_exit_drops(lo..*self.unique_id, ret);
  }

  /// Whether statement `st` never completes normally (its expression
  /// diverges, `expr_diverges`): what comes after it in its block never
  /// runs (#1013).
  fn stmt_leaves(&self, st: &'tcx rustc_hir::Stmt<'tcx>) -> bool {
    match st.kind {
      StmtKind::Expr(x) | StmtKind::Semi(x) => self.expr_diverges(x),
      StmtKind::Let(l) => l.init.is_some_and(|i| self.expr_diverges(i)),
      StmtKind::Item(_) => false,
    }
  }

  /// The id of the move among `events` (one arm's, not inside a
  /// conditional nested in it) that leaves nothing that needs drop in
  /// the split scrutinee temporary `t`: the last of the moves out of it
  /// into the bindings of a pattern that takes all of it
  /// (`temp_emptiers`, #1386). None when the arm leaves something in it.
  fn emptying_move(&self, events: &[(usize, ExternalEvent)], t: &ResourceAccessPoint) -> Option<usize> {
    let groups = self.temp_emptiers.get(t.hash())?;
    let moves: Vec<(String, usize)> = events.iter().filter_map(|(_, e)| match e {
      ExternalEvent::Move { from: ResourceTy::Value(f), to, id, .. } if f == t => to.extract_rap().map(|r| (r.name().to_owned(), *id)),
      _ => None,
    }).collect();
    groups.iter()
      .filter(|g| g.iter().all(|n| moves.iter().any(|(m, _)| m == n)))
      .filter_map(|g| moves.iter().filter(|(m, _)| g.contains(m)).map(|(_, id)| *id).max())
      .max()
  }

  /// Every arm's move that empties a split scrutinee temporary
  /// (`emptying_move`), into `temp_emptying_moves`: the temporary's leg
  /// in that arm ends there (#1386). When every arm that reaches the
  /// join emptied it (the others leave the conditional), nothing is left
  /// to drop where the conditional ends: that drop goes (it would be a
  /// dot on no line).
  pub fn mark_temp_emptying_moves(&mut self, from: usize) {
    fn walk(this: &ExprVisitor<'_, '_>, events: &[(usize, ExternalEvent)], temps: &[ResourceAccessPoint], out: &mut Vec<usize>, dead: &mut HashSet<usize>) {
      for (_, e) in events {
        let ExternalEvent::Branch { branches, live_vars, merge_point, .. } = e else { continue };
        for t in temps {
          let mut all = branches.iter().any(|b| !b.diverges) && live_vars.contains(t);
          for b in branches {
            let m = this.emptying_move(&b.e_data, t);
            out.extend(m);
            all &= b.diverges || m.is_some();
          }
          if all {
            // (The drop was recorded with the temporary, before the arms.)
            if let Some((_, ExternalEvent::TemporaryDrop { id, .. })) = events.iter()
              .find(|(l, x)| *l >= *merge_point && matches!(x, ExternalEvent::TemporaryDrop { ro, id, .. } if ro == t && !this.exit_drops.contains(id))) {
              dead.insert(*id);
            }
          }
        }
        for b in branches {
          walk(this, &b.e_data, temps, out, dead);
        }
      }
    }
    fn prune(events: &mut Vec<(usize, ExternalEvent)>, dead: &HashSet<usize>) {
      events.retain(|(_, e)| !(matches!(e, ExternalEvent::TemporaryDrop { .. }) && dead.contains(&e.get_id())));
      for (_, e) in events.iter_mut() {
        if let ExternalEvent::Branch { branches, .. } = e {
          for b in branches.iter_mut() {
            prune(&mut b.e_data, dead);
          }
        }
      }
    }
    // A split temporary's drop is recorded with it, before its arms: in
    // an arm's own events (`Some(r) => match make() {`), the drop would
    // come before the fork and end the column there. It goes after.
    // (Also: a split temporary needs drop, so its type isn't Copy, and
    // what a pattern copies out of it is a part of it: `n` in `((a, n),
    // b)` on `((make(), 1), make())`.)
    fn order_drops(events: &mut Vec<(usize, ExternalEvent)>) {
      events.iter_mut().for_each(|(_, e)| part_copy(e));
      let mut i = 0;
      while i < events.len() {
        let moved = match &events[i].1 {
          ExternalEvent::TemporaryDrop { ro, .. } if ro.is_split_scrutinee_temporary() => {
            let line = events[i].0;
            events.iter().enumerate().skip(i + 1).filter(|(_, (_, e))| matches!(e,
              ExternalEvent::Branch { live_vars, merge_point, .. } if live_vars.contains(ro) && *merge_point <= line))
              .map(|(j, _)| j).last()
          }
          _ => None,
        };
        match moved {
          Some(j) => {
            let ev = events.remove(i);
            events.insert(j, ev);
          }
          None => i += 1,
        }
      }
      for (_, e) in events.iter_mut() {
        if let ExternalEvent::Branch { branches, .. } = e {
          for b in branches.iter_mut() {
            order_drops(&mut b.e_data);
            b.line_map.values_mut().flatten().for_each(part_copy);
          }
        }
      }
    }
    fn part_copy(e: &mut ExternalEvent) {
      if let ExternalEvent::Copy { from: ResourceTy::Value(f), is_partial, .. } = e {
        if f.is_split_scrutinee_temporary() {
          *is_partial = true;
        }
      }
    }
    for (_, e) in self.preprocessed_events[from..].iter_mut() {
      match e {
        ExternalEvent::Branch { branches, .. } => {
          for b in branches.iter_mut() {
            order_drops(&mut b.e_data);
            b.line_map.values_mut().flatten().for_each(part_copy);
          }
        }
        e => part_copy(e),
      }
    }
    self.event_line_map.values_mut().flatten().for_each(part_copy);
    if self.temp_emptiers.is_empty() {
      return;
    }
    #[expect(clippy::disallowed_methods, reason = "sorted by hash below")]
    let mut temps: Vec<ResourceAccessPoint> = self.raps.values()
      .map(|rd| rd.rap.clone())
      .filter(|r| self.temp_emptiers.contains_key(r.hash()))
      .collect();
    temps.sort_by_key(|r| *r.hash());
    let mut out = Vec::new();
    // A `while let` body (also a `while` let chain's) is drawn inline, one
    // pass, on which the pattern matched (#382): its moves are on the
    // function's own timeline.
    for t in &temps {
      out.extend(self.emptying_move(&self.preprocessed_events[from..], t));
    }
    let mut dead = HashSet::new();
    walk(self, &self.preprocessed_events[from..], &temps, &mut out, &mut dead);
    out.sort_unstable();
    out.dedup();
    self.temp_emptying_moves.extend(out);
    if !dead.is_empty() {
      let mut tail = self.preprocessed_events.split_off(from);
      prune(&mut tail, &dead);
      self.preprocessed_events.extend(tail);
      for evs in self.event_line_map.values_mut() {
        evs.retain(|e| !(matches!(e, ExternalEvent::TemporaryDrop { .. }) && dead.contains(&e.get_id())));
      }
    }
  }

  /// The scrutinee temporaries (`temps`) of an edition-2024 `if let` /
  /// let chain drawn as a branch that an exit in its matched block drops
  /// (`if_ev` holds that drop, or a nested conditional of it does), or
  /// would drop if a `?` in it returned early (that dot was lost: the
  /// temporary had no column in the branch). With one unsplit column,
  /// the exit's drop and the drop before the `else` were two drops of
  /// one value on one line (#1360). So each forks at the `if`, like a
  /// `match` scrutinee: the drop that said both
  /// paths at once becomes the else branch's drop (before the `else`
  /// block runs, or, without one, when the pattern doesn't match) and,
  /// when the matched block can reach its end, the if branch's drop
  /// there. Returns those temporaries: live in the branches.
  fn fork_exited_temps(
    &mut self,
    temps: &HashSet<ResourceAccessPoint>,
    if_ev: &mut Vec<(usize, ExternalEvent)>,
    else_ev: &mut Vec<(usize, ExternalEvent)>,
    if_diverges: bool,
  ) -> HashSet<ResourceAccessPoint> {
    // A drop at an exit (`exit_drops`), or the dot of a `?`, in the
    // block or a conditional nested in it.
    fn exits_in(evs: &[(usize, ExternalEvent)], exit_drops: &HashSet<usize>, out: &mut HashSet<ResourceAccessPoint>) {
      for (_, e) in evs {
        match e {
          ExternalEvent::TemporaryDrop { ro, id, .. } if exit_drops.contains(id) => { out.insert(ro.clone()); }
          ExternalEvent::OwnerDropIfTryReturns { ro, .. } => { out.insert(ro.clone()); }
          ExternalEvent::Branch { branches, .. } => branches.iter().for_each(|b| exits_in(&b.e_data, exit_drops, out)),
          _ => {}
        }
      }
    }
    let mut exited = HashSet::new();
    exits_in(if_ev, &self.exit_drops, &mut exited);
    // So does one its pattern moves something out of, or borrows from
    // (#1386): the matched block takes it from the temporary's leg.
    #[expect(clippy::iter_over_hash_type, reason = "filtering into a set; order is irrelevant")]
    for t in temps {
      if t.is_split_scrutinee_temporary() && takes_from(if_ev, t) {
        exited.insert(t.clone());
      }
    }
    let forked: HashSet<ResourceAccessPoint> = &exited & temps;
    if forked.is_empty() {
      return forked;
    }
    // The drop that said both paths: on the function's timeline, or
    // (a let chain's, whose later parts are the if branch's) in the if
    // branch.
    let is_end = |e: &ExternalEvent, exit_drops: &HashSet<usize>| matches!(e,
      ExternalEvent::TemporaryDrop { ro, id, .. } if forked.contains(ro) && !exit_drops.contains(id));
    let mut ends: Vec<(usize, ExternalEvent)> = Vec::new();
    let exit_drops = &self.exit_drops;
    self.preprocessed_events.retain(|(l, e)| if is_end(e, exit_drops) { ends.push((*l, e.clone())); false } else { true });
    if_ev.retain(|(l, e)| if is_end(e, exit_drops) { ends.push((*l, e.clone())); false } else { true });
    let mut else_front = Vec::new();
    for (line, ev) in ends {
      let ExternalEvent::TemporaryDrop { ro, ty, when, literal, part, id, .. } = ev else { continue };
      // "before the `else` block … runs, or when the block it matched
      // ends": one path each.
      let (else_when, then_when) = match when.split_once(", or when the block ") {
        Some((e, _)) if when.contains("its let chain matched") => (e.to_owned(), "when the block its let chain matched ends".to_owned()),
        Some((e, _)) => (e.to_owned(), "when the block of this `if let` ends (its pattern matched)".to_owned()),
        None => (when.clone(), when.clone()),
      };
      // Made by a later part of a let chain (`if k && let Ok(s) =
      // make()`): on the else path it exists only if the parts before
      // got as far as its `let` (#1386).
      let made_in_if = if_ev.iter().any(|(_, e)| matches!(e,
        ExternalEvent::Move { to: ResourceTy::Value(t), .. } | ExternalEvent::Bind { to: ResourceTy::Value(t), .. } if *t == ro));
      let else_when = if made_in_if { format!("{}, if the let chain got as far as the `let` that makes it", else_when) } else { else_when };
      else_front.push((line, ExternalEvent::TemporaryDrop { ro: ro.clone(), ty: ty.clone(), when: else_when, literal, early: false, part: part.clone(), id }));
      // (Not when the matched block's pattern took all of it: that
      // leg ends at the move, #1386.)
      if !if_diverges && self.emptying_move(if_ev, &ro).is_none() {
        if_ev.push((line, ExternalEvent::TemporaryDrop { ro, ty, when: then_when, literal, early: false, part, id: *self.unique_id }));
        *self.unique_id += 1;
      }
    }
    // (A drop at an exit directly in the matched block ends the if
    // branch's column: see `take_branch_events`.)
    for (_, e) in if_ev.iter_mut() {
      if let ExternalEvent::TemporaryDrop { ro, early, .. } = e {
        if forked.contains(ro) {
          *early = false;
        }
      }
    }
    else_ev.splice(0..0, else_front);
    forked
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

  /// The dead `else` block `e` of a lone `if let` whose pattern always
  /// matches (#987) was visited while event ids `ids` were handed out:
  /// remove its events (and those binding its value to a `let`), which
  /// never happen. Returns each variable in `outer` (those in scope
  /// before the `if`) that one of them touched, with the line of the
  /// first such event: where a `DeadElseNote` says the branch never runs.
  fn discard_dead_else(&mut self, ids: std::ops::Range<usize>, e: &'tcx Expr<'tcx>, outer: &[String]) -> Vec<(usize, ResourceAccessPoint)> {
    let rhs = self.rhs_arm_ids.get(&e.hir_id).copied();
    let dead = |id: usize| ids.contains(&id) || rhs.is_some_and(|(lo, hi)| (lo..hi).contains(&id));
    let mut gone: Vec<(usize, ExternalEvent)> = Vec::new();
    self.preprocessed_events.retain(|(l, ev)| {
      if dead(ev.get_id()) {
        gone.push((*l, ev.clone()));
        false
      } else {
        true
      }
    });
    for evs in self.event_line_map.values_mut() {
      evs.retain(|ev| !dead(ev.get_id()));
    }
    self.late_old_ends.retain(|(_, ev)| !dead(ev.get_id()));
    // (A struct's field columns, `p.name`, count as `p`'s.)
    let is_outer = |r: &ResourceAccessPoint| !r.is_fn()
      && r.name().split('.').next().is_some_and(|base| outer.iter().any(|o| o == base));
    gone.sort_by_key(|(l, ev)| (*l, ev.get_id()));
    let mut notes: Vec<(usize, ResourceAccessPoint)> = Vec::new();
    for (line, ev) in &gone {
      let mut touched = HashSet::new();
      collect_event_live_vars(std::slice::from_ref(&(*line, ev.clone())), &mut touched);
      for r in in_decl_order(&touched) {
        if is_outer(r) && !notes.iter().any(|(_, n)| n == r) {
          notes.push((*line, r.clone()));
        }
      }
    }
    notes
  }

  /// The ranges of event ids that bind the value of each of these
  /// branch bodies to a `let` (`rhs_arm_ids`), for those that have one.
  fn rhs_ids_of(&self, bodies: impl IntoIterator<Item = Option<&'tcx Expr<'tcx>>>) -> Vec<std::ops::Range<usize>> {
    bodies.into_iter().flatten()
      .filter_map(|b| self.rhs_arm_ids.get(&b.hir_id))
      .map(|&(lo, hi)| lo..hi)
      .collect()
  }

  /// Visits the statements of `b`, and its tail, up to where every
  /// path has left the function (`all_paths_exit`, #515): what comes
  /// after that never runs, so nothing is drawn for it (#1004). (Set
  /// only by a conditional directly in the function's body, so only
  /// the rest of the body, and of blocks directly in it, is skipped.)
  fn visit_live_stmts(&mut self, b: &'tcx rustc_hir::Block<'tcx>) {
    // Also after a statement that never completes, anywhere (#1013).
    let mut left = false;
    for st in b.stmts {
      if (self.all_paths_exit || left) && !matches!(st.kind, StmtKind::Item(_)) {
        self.dead_code.insert(st.hir_id);
      } else {
        self.visit_stmt(st);
        left |= self.stmt_leaves(st);
      }
    }
    if let Some(e) = b.expr {
      if self.all_paths_exit || left {
        self.dead_tail |= self.all_paths_exit;
        self.dead_code.insert(e.hir_id);
      } else {
        self.visit_expr(e);
      }
    }
  }

  /// A body drawn inline (an `if let` whose pattern always matches, a
  /// one-arm `match`) directly in the function's body that always
  /// leaves the function (`return`, a panic): the exit's drops, among
  /// events `ids`, are what ends those variables (#1004). They stay on
  /// the function's own columns, and every path has left the function
  /// (`all_paths_exit`): the code after it never runs and draws
  /// nothing (`visit_live_stmts`), and no scope end drops what the
  /// exit did.
  fn keep_inline_exit_drops(&mut self, ids: std::ops::Range<usize>, at: &'tcx Expr<'tcx>) {
    let kept: HashSet<usize> = self.preprocessed_events.iter()
      .map(|(_, e)| e.get_id())
      .filter(|id| ids.contains(id) && self.exit_drops.contains(id))
      .collect();
    self.exit_drops.retain(|id| !kept.contains(id));
    self.note_all_paths_exit(at);
  }

  /// Every path leaves the function at `at` (directly in its body):
  /// see `all_paths_exit`.
  fn note_all_paths_exit(&mut self, at: &'tcx Expr<'tcx>) {
    self.all_paths_exit = true;
    self.all_paths_exit_from = self.all_paths_exit_from.min(expr_to_line(at, &self.tcx));
  }

  /// `drop_locals_on_exit` for a panic call inside a branch. A call to
  /// `process::exit` (`process_exit_line`) ends what is in scope there
  /// without dropping it, in a branch or directly in the function's
  /// body (#1172).
  fn note_panic(&mut self, call: &'tcx Expr<'tcx>) {
    if let Some((line, exit)) = self.process_exit_line(call) {
      if self.in_closure(call) {
        return;
      }
      let name = exit.call.clone();
      if self.exit_frames.is_empty() && self.at_fn_level(call) {
        let lo = *self.unique_id;
        self.exit_frames.push(ExitFrame::Branch { len: 0, hide: Vec::new(), binds: Vec::new(), binds_at: 0, fn_level: true });
        self.no_drop_exit = Some(exit);
        self.drop_locals_on_exit(line, None, None, false, &name);
        self.exit_frames.pop();
        self.keep_inline_exit_drops(lo..*self.unique_id, call);
      } else {
        self.no_drop_exit = Some(exit);
        self.drop_locals_on_exit(line, None, None, false, &name);
      }
      return;
    }
    if let Some(line) = self.panic_line(call) {
      // One every path that gets here reaches (directly in the
      // function's body, or in a `loop` that never ends there): it
      // drops what the function owns on its row, and nothing after it
      // runs, as such a `return` does (#1545). Its drops were lost.
      if self.panic_leaves_every_path(call) {
        let lo = *self.unique_id;
        self.exit_frames.push(ExitFrame::Branch { len: 0, hide: Vec::new(), binds: Vec::new(), binds_at: 0, fn_level: true });
        self.drop_locals_on_exit(line, None, None, false, "panic");
        self.exit_frames.pop();
        self.keep_inline_exit_drops(lo..*self.unique_id, call);
        return;
      }
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
  ///
  /// Tells a variable that holds no value on any path (`No`) from one
  /// that holds one on every path (`Yes`) and from one that holds one on
  /// some paths only (`Maybe`: its drop flag decides whether an exit here
  /// drops it, #1194).
  fn ownership_here(&self, hash: u64, binds: bool) -> OwnHere {
    use OwnHere as Own;
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
          // An `FnOnce` closure's call moves it into the call (#271):
          // it holds nothing after (#1136).
          ExternalEvent::ClosureCall { closure, .. } if is(closure) => Own::No,
          // Moved into an or-pattern's binding only if its alternative
          // matched (#533). (Out of a tuple scrutinee, `tuple`, the
          // place was moved into the tuple already.)
          ExternalEvent::OrAltMove { ro, tuple: None, .. } if *ro.hash() == hash && own == Own::Yes => Own::Maybe,
          ExternalEvent::InitRefParam { param, .. } if *param.hash() == hash => Own::Yes,
          // After a loop: owns a value at every `break` (#1000), or
          // may have ended before giving it one (#786, #1001).
          ExternalEvent::LoopExitKept { ro, .. } if *ro.hash() == hash => Own::Yes,
          ExternalEvent::LoopMaybeInit { ro, .. } if *ro.hash() == hash && own == Own::Yes => Own::Maybe,
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
      return step(self.preprocessed_events, hash, &hide, Own::No);
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
    step(&events, hash, &hide, Own::No)
  }

  /// At the start of the body of `loop_expr` (#992): the variables
  /// declared before the loop (the first `len` of `in_scope`) that
  /// hold their value here the first time round, but that the loop may
  /// come back to with them moved (`moved_round_loop`: a move further
  /// down the body, or before a `continue`, that nothing after gave a
  /// new value). The body is drawn once (#853), so each gets a
  /// `LoopExitMove { at: LoopHead }` on the loop's first line: what the
  /// body does before giving it a new value (a reassignment, a
  /// `break`) sees it maybe-moved. Returns them.
  fn note_loop_head_moves(&mut self, loop_expr: &'tcx Expr<'tcx>, len: usize) -> Vec<ResourceAccessPoint> {
    // A struct moved whole takes its field columns with it.
    let mut names: Vec<String> = Vec::new();
    for name in self.moved_round_loop(loop_expr.span) {
      let members = self.struct_members_deep(&name);
      names.push(name);
      names.extend(members.iter().map(|m| m.name().clone()));
    }
    names.sort();
    names.dedup();
    let line = span_to_line(&loop_expr.span, &self.tcx);
    let outer = &self.in_scope[..len.min(self.in_scope.len())];
    names.retain(|n| outer.iter().any(|o| o == n.split('.').next().unwrap_or("")) && !self.skip_raps.contains(n));
    let mut out = Vec::new();
    for name in names {
      let Some(rd) = self.raps.get(&name) else { continue };
      let ro = rd.rap.clone();
      if ro.is_ref() || ro.is_fn() || self.rap_ownership_now(&name) != Ownership::Held {
        continue;
      }
      self.add_external_event(line, ExternalEvent::LoopExitMove {
        ro: ro.clone(), definite: false, at: MaybeMovedAt::LoopHead, id: *self.unique_id,
      });
      *self.unique_id += 1;
      out.push(ro);
    }
    out
  }

  /// At the start of the body of `loop_expr` (#1001), the mirror of
  /// `note_loop_head_moves`: the variables declared before the loop
  /// (the first `len` of `in_scope`) that hold no value here on the
  /// path from before the loop (moved before it), but that the loop may
  /// come back to with a value the body gave them (`given_round_loop`).
  /// (In a loop inside another, the outer loop's earlier times round
  /// may have given it one too; the outer loop sees to that.)
  /// Pushed onto `loop_given`: an assignment that the body makes before
  /// anything else touches one drops, a later time round, the value an
  /// earlier one gave it (`given_by_loop_now`), and the loop's end
  /// says whether it has one after the loop (`emit_loop_exit_states`).
  /// Not a `let x;` local, which `enter_loop_inits` sees to (#786).
  /// `before_cond`: for a `while` loop, the length `preprocessed_events`
  /// had before its condition, which runs again each time round: one
  /// that held a value there (the condition moved it, `while let
  /// Some(x) = o`) has none when the body starts on any time round.
  fn note_loop_head_given(&mut self, loop_expr: &'tcx Expr<'tcx>, len: usize, before_cond: Option<usize>) {
    let mut names: Vec<String> = Vec::new();
    for name in self.given_round_loop(loop_expr.span) {
      let members = self.struct_members_deep(&name);
      names.push(name);
      names.extend(members.iter().map(|m| m.name().clone()));
    }
    // A loop that never comes back round (#1467): what its body assigns
    // may be given a value by its one time round, which may not happen
    // (a `while` / `for`, a `break` before the assignment). There is no
    // earlier time round for an assignment in it to drop the value of
    // (`given_by_loop_now`).
    let rounds_back = self.loop_never_rounds(loop_expr.span) != Some(true);
    let once_assigned: Vec<String> = if rounds_back {
      Vec::new()
    } else {
      let assigned = assigned_names(&self.tcx, loop_expr);
      #[expect(clippy::disallowed_methods, reason = "sorted below")]
      let names: Vec<String> = self.raps.keys()
        .filter(|n| assigned.iter().any(|a| *a == **n || n.starts_with(&format!("{}.", a))))
        .cloned().collect();
      names
    };
    names.extend(once_assigned.iter().cloned());
    names.sort();
    names.dedup();
    #[expect(clippy::disallowed_methods, reason = "only tested for membership")]
    let deferred: HashSet<String> = self.uninit_locals.keys().chain(self.maybe_init_locals.iter())
      .map(|id| self.deferred_name(*id)).collect();
    let outer = &self.in_scope[..len.min(self.in_scope.len())];
    names.retain(|n| {
      let root = n.split('.').next().unwrap_or("");
      outer.iter().any(|o| o == root) && !deferred.contains(root) && !self.skip_raps.contains(n)
    });
    let mut given = Vec::new();
    for name in names {
      let Some(rd) = self.raps.get(&name) else { continue };
      let ro = rd.rap.clone();
      if ro.is_ref() || ro.is_fn() || self.rap_ownership_now(&name) != Ownership::None
        || before_cond.is_some_and(|at| self.rap_ownership_before(at, &name) != Ownership::None) {
        continue;
      }
      given.push(ro);
    }
    // Those that only may hold a value where it starts (#1012), and
    // what each holds where the loop comes back round: `Held` also
    // when the loop never moves it (it keeps what it had, and may give
    // it a value). A struct's field columns go with it. In a loop
    // inside another that it was declared before (#1019), a run of this
    // loop may follow an earlier one, not only the branch above: what
    // it holds where this loop is entered is rustc's, on every way in
    // (an outer loop's later times round too), and `nested` says
    // whether the outermost such loop ever moves it.
    let enclosing: Vec<(rustc_hir::HirId, usize)> = self.exit_frames.iter()
      .filter_map(|f| match f { ExitFrame::Loop { id, len, .. } if *id != loop_expr.hir_id => Some((*id, *len)), _ => None })
      .collect();
    let mut entry_states: Option<Vec<(String, Ownership)>> = None;
    let mut outer_moves: Vec<(rustc_hir::HirId, Vec<(String, bool)>)> = Vec::new();
    let mut rounds: Vec<(String, Ownership)> = Vec::new();
    for (name, round, moved) in self.round_states_loop(loop_expr.span) {
      let round = if round == Ownership::Maybe && !moved { Ownership::Held } else { round };
      let members = self.struct_members_deep(&name);
      rounds.push((name, round));
      rounds.extend(members.iter().map(|m| (m.name().clone(), round)));
    }
    // One that never comes back round (#1467, `while c { s = …;
    // break; }`): a `while` / `for` may still not run its body, and a
    // `break` may come before the assignment. As if each one it assigns
    // kept what it had where it would come back round
    // (`MaybeEntry::rounds`).
    let mut once_assigned = once_assigned;
    once_assigned.sort();
    rounds.extend(once_assigned.into_iter().map(|n| (n, Ownership::Held)));
    let mut maybe_entry: Vec<MaybeEntry> = Vec::new();
    for (name, round) in rounds {
      let root = name.split('.').next().unwrap_or("");
      if !outer.iter().any(|o| o == root) || deferred.contains(root) || self.skip_raps.contains(&name) {
        continue;
      }
      let Some(rd) = self.raps.get(&name) else { continue };
      let ro = rd.rap.clone();
      if ro.is_ref() || ro.is_fn() || maybe_entry.iter().any(|m| m.ro == ro)
        || self.rap_ownership_now(&name) != Ownership::Maybe
        || before_cond.is_some_and(|at| self.rap_ownership_before(at, &name) != Ownership::Maybe) {
        continue;
      }
      // The outermost enclosing loop it was declared before.
      let outermost = enclosing.iter()
        .find(|(_, l)| self.in_scope[..(*l).min(self.in_scope.len())].iter().any(|o| o == root))
        .map(|(id, _)| *id);
      let nested = match outermost {
        None => None,
        Some(id) => {
          // (A field rustc has no move path for goes with its struct.)
          let entry = entry_states.get_or_insert_with(|| self.entry_states_loop(loop_expr.span));
          let own = entry.iter().find(|(n, _)| *n == name).or_else(|| entry.iter().find(|(n, _)| n == root));
          if !own.is_some_and(|(_, o)| *o == Ownership::Maybe) {
            continue;
          }
          if !outer_moves.iter().any(|(o, _)| *o == id) {
            let span = self.tcx.hir_span(id);
            let moves = self.round_states_loop(span).into_iter().map(|(n, _, m)| (n, m)).collect();
            outer_moves.push((id, moves));
          }
          let moves = outer_moves.iter().find(|(o, _)| *o == id)
            .and_then(|(_, ms)| ms.iter().find(|(n, _)| *n == root).map(|(_, m)| *m));
          // (Unknown to rustc's move data: left as before.)
          let Some(moves) = moves else { continue };
          Some(moves)
        }
      };
      let by_cond = self.moved_by_chain_condition(&ro) || self.maybe_moved_by_let_now(&name);
      maybe_entry.push(MaybeEntry { ro, round, by_cond, nested, rounds: rounds_back });
    }
    self.loop_given.push((loop_expr.hir_id, given));
    self.loop_maybe_entry.push((loop_expr.hir_id, maybe_entry));
  }

  /// Whether an assignment to `rap` here is the first thing since the
  /// start of the body of a loop it's in that changes it, where `rap`
  /// only may hold a value as the loop starts (`loop_maybe_entry`,
  /// #1012): the first time round, the assignment drops the old value
  /// only if no branch above moved it; a later time round, only if the
  /// time round before left it holding one. Only the innermost loop
  /// here. `Some((nested, rounds))` (`MaybeEntry::nested`, `::rounds`)
  /// if so: in a loop inside another that `rap` was declared before,
  /// "the first time round" would be each run of the inner loop
  /// (#1019); a loop that never comes back round has no later time
  /// round (#1467).
  fn maybe_entry_untouched_now(&self, rap: &ResourceAccessPoint) -> Option<(Option<bool>, bool)> {
    let loop_at = self.exit_frames.iter().rposition(|f| matches!(f, ExitFrame::Loop { .. }))?;
    let ExitFrame::Loop { id: innermost, .. } = self.exit_frames[loop_at] else { return None };
    let entry = self.loop_maybe_entry.iter()
      .find_map(|(id, entry)| if *id == innermost { entry.iter().find(|m| m.ro == *rap).map(|m| (m.nested, m.rounds)) } else { None })?;
    let (events, hide) = self.loop_path_events(loop_at);
    (path_own(&events, rap, &hide, PathOwn::Yes) == PathOwn::Yes
      && path_own(&events, rap, &hide, PathOwn::No) == PathOwn::No).then_some(entry)
  }

  /// Whether the innermost loop here may end, on its first time round,
  /// before `at` (#1467): a `while` / `for` loop (its condition fails,
  /// its iterator is empty), or something in its body before `at` that
  /// leaves it (`exits_loop_before`).
  fn loop_may_end_before(&self, at: rustc_span::Span) -> bool {
    let Some(ExitFrame::Loop { id, .. }) = self.exit_frames.iter().rev().find(|f| matches!(f, ExitFrame::Loop { .. })) else { return false };
    let rustc_hir::Node::Expr(loop_expr) = self.tcx.hir_node(*id) else { return true };
    match loop_expr.kind {
      ExprKind::Loop(_, _, rustc_hir::LoopSource::Loop, _) => exits_loop_before(loop_expr, at),
      _ => true,
    }
  }

  /// Of `live`, the variables (and field columns) that a loop this is
  /// inside, and that they were declared before, gives a new value
  /// somewhere in its body (#1026): at a join here that leaves one
  /// maybe-moved, the value the branches that didn't move it left is
  /// dropped by that assignment when the loop comes back round, not at
  /// the scope end.
  fn loop_assigned_now(&self, live: &HashSet<ResourceAccessPoint>) -> HashSet<ResourceAccessPoint> {
    let mut out = HashSet::new();
    for f in &self.exit_frames {
      let ExitFrame::Loop { id, len, .. } = f else { continue };
      let rustc_hir::Node::Expr(loop_expr) = self.tcx.hir_node(*id) else { continue };
      let assigned = assigned_names(&self.tcx, loop_expr);
      if assigned.is_empty() {
        continue;
      }
      let outer = &self.in_scope[..(*len).min(self.in_scope.len())];
      #[expect(clippy::iter_over_hash_type, reason = "only collects into a set; order doesn't matter")]
      for r in live {
        let name = r.name();
        let root = name.split('.').next().unwrap_or("");
        if outer.iter().any(|o| o == root)
          && assigned.iter().any(|a| a == name || name.starts_with(&format!("{}.", a))) {
          out.insert(r.clone());
        }
      }
    }
    out
  }

  /// Whether an assignment to `rap` here, which holds no value on the
  /// path from before the loop (moved before it), drops what an
  /// earlier time round a loop it's in gave it (#1001): `rap` is one
  /// of the loop's `loop_given`, and nothing since the start of the
  /// loop's body moved it or gave it a value.
  fn given_by_loop_now(&self, rap: &ResourceAccessPoint) -> bool {
    self.loop_given.iter().rev().any(|(id, given)| {
      // (Not one that never comes back round, #1467.)
      given.contains(rap) && self.loop_never_rounds(self.tcx.hir_span(*id)) != Some(true) && self.exit_frames.iter()
        .rposition(|f| matches!(f, ExitFrame::Loop { id: l, .. } if l == id))
        .is_some_and(|loop_at| {
          let (events, hide) = self.loop_path_events(loop_at);
          path_own(&events, rap, &hide, PathOwn::Maybe) == PathOwn::Maybe
        })
    })
  }

  /// Whether what last changed `rap`'s state, in the events so far, is
  /// the end of a loop that may have ended before giving it a value
  /// (`LoopMaybeInit`, #1001): its next assignment drops the old value
  /// only if the loop gave it one (`InitBy::Loop`). `Some(nested)`, the
  /// event's `nested` (#1032), if so.
  fn maybe_init_by_loop_now(&self, rap: &ResourceAccessPoint) -> Option<bool> {
    self.loop_maybe_init_now(rap).map(|(_, nested)| nested)
  }

  /// `maybe_init_by_loop_now`, with where in `preprocessed_events` that
  /// `LoopMaybeInit` is.
  fn loop_maybe_init_now(&self, rap: &ResourceAccessPoint) -> Option<(usize, bool)> {
    let is = |r: &ResourceTy| r.extract_rap().is_some_and(|r| r.hash() == rap.hash());
    for (at, (_, e)) in self.preprocessed_events.iter().enumerate().rev() {
      match e {
        ExternalEvent::LoopMaybeInit { ro, nested, .. } if ro.hash() == rap.hash() => return Some((at, *nested)),
        ExternalEvent::LoopExitMove { ro, .. } | ExternalEvent::LoopExitKept { ro, .. }
        | ExternalEvent::OrAltMove { ro, .. } | ExternalEvent::ManuallyDropByHand { ro, .. }
        | ExternalEvent::OwnerDropAtReassign { ro, .. } if ro.hash() == rap.hash() => return None,
        ExternalEvent::Move { from, to, .. } if is(from) || is(to) => return None,
        ExternalEvent::Copy { to, .. } | ExternalEvent::Bind { to, .. } if is(to) => return None,
        ExternalEvent::Branch { branches, .. }
          if branches.iter().filter(|b| !b.diverges).any(|b| path_own(&b.e_data, rap, &[], PathOwn::Yes) != PathOwn::Yes
            || path_own(&b.e_data, rap, &[], PathOwn::No) != PathOwn::No) => return None,
        _ => {}
      }
    }
    None
  }

  /// Where in `preprocessed_events` the conditional is, if what last
  /// changed `rap`'s state, in the events so far, is a conditional that
  /// gives it a new value in only some of its branches where it may
  /// have held none as it started (`maybe_held`, #1034).
  fn maybe_held_branch_now(&self, rap: &ResourceAccessPoint) -> Option<usize> {
    let is = |r: &ResourceTy| r.extract_rap().is_some_and(|r| r.hash() == rap.hash());
    for (at, (_, e)) in self.preprocessed_events.iter().enumerate().rev() {
      match e {
        ExternalEvent::Branch { maybe_held, .. } if maybe_held.contains(rap) => return Some(at),
        ExternalEvent::LoopMaybeInit { ro, .. } | ExternalEvent::LoopExitMove { ro, .. } | ExternalEvent::LoopExitKept { ro, .. }
        | ExternalEvent::OrAltMove { ro, .. } | ExternalEvent::ManuallyDropByHand { ro, .. }
        | ExternalEvent::OwnerDropAtReassign { ro, .. } if ro.hash() == rap.hash() => return None,
        ExternalEvent::Move { from, to, .. } if is(from) || is(to) => return None,
        ExternalEvent::Copy { to, .. } | ExternalEvent::Bind { to, .. } if is(to) => return None,
        ExternalEvent::Branch { branches, .. }
          if branches.iter().filter(|b| !b.diverges).any(|b| path_own(&b.e_data, rap, &[], PathOwn::Yes) != PathOwn::Yes
            || path_own(&b.e_data, rap, &[], PathOwn::No) != PathOwn::No) => return None,
        _ => {}
      }
    }
    None
  }

  /// At the end of `loop_expr` (#1000, #1001), what a variable declared
  /// before it holds after it, where that isn't what the end of its
  /// body (drawn once, #853) left: the code after the loop is reached
  /// through a `break`, or for a `while` / `for` (`!bare`) also where
  /// the loop starts (its condition false, its iterator done).
  ///
  /// - `entry`: the variables that owned a value (or maybe did) where
  ///   the loop started. In a bare `loop`, one that owns a value at
  ///   every `break` (Held where the loop started and moved on the way
  ///   to none, `broke_moved`; or given a value on the way to each)
  ///   owns it after the loop: a `LoopExitKept` event, if the body's
  ///   end left it moved (`loop { s = …; if c { break; } drop(s); }`).
  /// - The loop's `loop_given` (moved before the loop, given a value in
  ///   it): with a `break` before that, or for a `while` / `for`, the
  ///   loop can end without one: a `LoopMaybeInit` event, if the body's
  ///   end left it owning one.
  fn emit_loop_exit_states(&mut self, loop_expr: &'tcx Expr<'tcx>, entry: Vec<(ResourceAccessPoint, Ownership)>, broke_moved: &[ResourceAccessPoint], bare: bool) {
    let given = match self.loop_given.last() {
      Some((id, _)) if *id == loop_expr.hir_id => self.loop_given.pop().map(|(_, g)| g).unwrap_or_default(),
      _ => Vec::new(),
    };
    let maybe_entry = match self.loop_maybe_entry.last() {
      Some((id, _)) if *id == loop_expr.hir_id => self.loop_maybe_entry.pop().map(|(_, e)| e).unwrap_or_default(),
      _ => Vec::new(),
    };
    let (mine, rest): (Vec<BreakState>, Vec<BreakState>) =
      std::mem::take(&mut self.break_states).into_iter().partition(|b| b.target == loop_expr.hir_id);
    self.break_states = rest;
    // A bare `loop` with no `break`: nothing after it runs.
    if bare && mine.is_empty() {
      return;
    }
    let line = span_end_line(&loop_expr.span, &self.tcx);
    // What a `break`'s value mentions (`break s`, `break f(s)`) may be
    // moved by it, after the variable's state at the `break` is taken.
    let in_values: HashSet<String> = breaks_to(loop_expr).into_iter()
      .filter_map(|(_, v)| v)
      .flat_map(|v| locals_in(v, ClosureLocals::Captures(self.tcx)))
      .map(|m| binding_name(&self.tcx, m.id))
      .collect();
    // Still the same variable here (not shadowed by one the body
    // declared), and not in a `break`'s value.
    let current = |this: &Self, ro: &ResourceAccessPoint| {
      this.raps.get(ro.name()).is_some_and(|rd| rd.rap == *ro)
        && !this.skip_raps.contains(ro.name())
        && !in_values.contains(ro.name().split('.').next().unwrap_or(""))
    };
    let mut kept: Vec<(ResourceAccessPoint, bool)> = Vec::new();
    let mut maybe_init: Vec<ResourceAccessPoint> = Vec::new();
    if bare {
      for (ro, own) in entry {
        if broke_moved.contains(&ro) || !current(self, &ro) {
          continue;
        }
        let reassigned = mine.iter().all(|b| b.assigned.contains(&ro));
        let held = match own {
          Ownership::Held => true,
          _ => reassigned,
        };
        if held && self.rap_ownership_now(ro.name()) != Ownership::Held {
          kept.push((ro, reassigned));
        }
      }
    }
    for ro in given {
      if broke_moved.contains(&ro) || !current(self, &ro) {
        continue;
      }
      let at_breaks: Vec<Ownership> = mine.iter()
        .filter_map(|b| b.given.iter().find(|(r, _)| *r == ro).map(|(_, o)| *o))
        .collect();
      let after = if !bare || at_breaks.is_empty() {
        Ownership::Maybe
      } else if at_breaks.iter().all(|o| *o == at_breaks[0]) {
        at_breaks[0]
      } else {
        Ownership::Maybe
      };
      match (after, self.rap_ownership_now(ro.name())) {
        (Ownership::Held, now) if now != Ownership::Held => kept.push((ro, true)),
        (Ownership::Maybe, Ownership::Held) => maybe_init.push(ro),
        _ => {}
      }
    }
    // One that only may have held a value as the loop started (#1012):
    // the loop can end before giving it one (a `while` / `for` whose
    // body never ran, a `break` before the assignment the first time
    // round), so what it holds after the loop is also what it held
    // before it. When every time round leaves it holding a value, it
    // has one after the loop if the loop assigned it or nothing moved
    // it before (`before_loop`); when every time round leaves it
    // moved, it may have been moved (`LoopRound`); when some time
    // rounds leave it moved and others don't, too, but then a drop
    // flag's "nothing above moved it" can't tell the branch above from
    // the loop's own moves (#1019): `kept`. (One that every `break`
    // comes after an assignment to is `kept` above; other mixes are
    // left as the body's end draws them.) In a loop inside another
    // that it was declared before (#1019), each run of this loop may
    // start with it holding nothing (the outer loop's first time
    // round, after the branch above) or a value (from the outer loop's
    // time round before): when every time round leaves it holding a
    // value, it only may hold one after the loop, a `LoopMaybeInit`
    // with `nested` (#1032); the rest is the outer loop's to say.
    let mut maybe_init: Vec<(ResourceAccessPoint, Option<bool>, bool)> =
      maybe_init.into_iter().map(|ro| (ro, None, false)).collect();
    let mut round_moved: Vec<(ResourceAccessPoint, bool)> = Vec::new();
    // Where this loop's body starts in `preprocessed_events`.
    let body_from = self.exit_frames.iter().rev().find_map(|f| match f {
      ExitFrame::Loop { events_at, .. } => Some(*events_at),
      ExitFrame::Branch { .. } => None,
    });
    for MaybeEntry { ro, round, by_cond, nested, .. } in maybe_entry {
      if broke_moved.contains(&ro) || !current(self, &ro) || kept.iter().any(|(k, _)| *k == ro) {
        continue;
      }
      // The ways out of the loop: its `break`s, and for a `while` /
      // `for` its start, where nothing in the body has changed it yet.
      let untouched = mine.iter().filter(|b| b.untouched.contains(&ro)).count() + usize::from(!bare);
      let assigned = mine.iter().filter(|b| b.assigned.contains(&ro)).count();
      let exits = mine.len() + usize::from(!bare);
      let ok = untouched > 0 && match round {
        Ownership::Held => untouched + assigned == exits,
        _ => untouched == exits,
      };
      if !ok {
        continue;
      }
      if nested.is_some() {
        if round == Ownership::Held && self.rap_ownership_now(ro.name()) == Ownership::Held {
          maybe_init.push((ro, Some(by_cond), true));
        }
        continue;
      }
      match (round, self.rap_ownership_now(ro.name())) {
        (Ownership::Held, Ownership::Held) => maybe_init.push((ro, Some(by_cond), false)),
        // Only may hold one because of a loop inside this one (#1032):
        // as if it held one, what this loop says goes.
        (Ownership::Held, Ownership::Maybe) if body_from.is_some_and(|from|
          self.loop_maybe_init_now(&ro).is_some_and(|(at, nested)| nested && at >= from)) =>
          maybe_init.push((ro, Some(by_cond), false)),
        // Or because the body gives it a value in only some branches
        // of a conditional (#1034): the loop may still end before
        // giving it one.
        (Ownership::Held, Ownership::Maybe) if body_from.is_some_and(|from|
          self.maybe_held_branch_now(&ro).is_some_and(|at| at >= from)) =>
          maybe_init.push((ro, Some(by_cond), false)),
        (Ownership::None, Ownership::None) => round_moved.push((ro, false)),
        (Ownership::Maybe, Ownership::Maybe | Ownership::Held) => round_moved.push((ro, true)),
        _ => {}
      }
    }
    for (ro, reassigned) in kept {
      self.add_external_event(line, ExternalEvent::LoopExitKept { ro, reassigned, id: *self.unique_id });
      *self.unique_id += 1;
    }
    for (ro, before_loop, nested) in maybe_init {
      self.add_external_event(line, ExternalEvent::LoopMaybeInit { ro, before_loop, nested, id: *self.unique_id });
      *self.unique_id += 1;
    }
    for (ro, kept) in round_moved {
      self.add_external_event(line, ExternalEvent::LoopExitMove {
        ro, definite: false, at: MaybeMovedAt::LoopRound { kept }, id: *self.unique_id,
      });
      *self.unique_id += 1;
    }
  }

  /// The variables in scope (and their field columns) that own a value,
  /// on every path or some, as a loop starts: `emit_loop_exit_states`'s
  /// `entry`.
  fn loop_entry_owners(&self) -> Vec<(ResourceAccessPoint, Ownership)> {
    #[expect(clippy::disallowed_methods, reason = "sorted by hash below")]
    let mut out: Vec<(ResourceAccessPoint, Ownership)> = self.raps.values()
      .map(|rd| rd.rap.clone())
      .filter(|r| !r.is_ref() && !r.is_fn())
      .filter(|r| self.in_scope.iter().any(|n| n == r.name().split('.').next().unwrap_or("")))
      .map(|r| { let own = self.rap_ownership_now(r.name()); (r, own) })
      .filter(|(_, own)| *own != Ownership::None)
      .collect();
    out.sort_by_key(|(r, _)| *r.hash());
    out
  }

  /// At the end of a `while` / `for` loop (#992): the variables of
  /// `note_loop_head_moves` are maybe-moved, as the loop can end after
  /// a time round that moved them; not those a `break` already says
  /// so for (`broke_moved`). `kept` when rustc says some time rounds
  /// come back round with it holding a value (#1019): only some of
  /// them move it.
  fn emit_loop_round_moves(&mut self, loop_expr: &'tcx Expr<'tcx>, head: Vec<ResourceAccessPoint>, broke_moved: &[ResourceAccessPoint]) {
    let line = span_end_line(&loop_expr.span, &self.tcx);
    let head: Vec<ResourceAccessPoint> = head.into_iter().filter(|r| !broke_moved.contains(r)).collect();
    let rounds = if head.is_empty() { Vec::new() } else { self.round_states_loop(loop_expr.span) };
    for ro in head {
      let kept = rounds.iter().any(|(n, own, _)| n == ro.name() && *own == Ownership::Maybe);
      self.add_external_event(line, ExternalEvent::LoopExitMove {
        ro, definite: false, at: MaybeMovedAt::LoopRound { kept }, id: *self.unique_id,
      });
      *self.unique_id += 1;
    }
  }

  /// At a `break` (`brk`) out of the loop `target`: the variables the
  /// iteration moved on the way here and didn't give a new value since
  /// are (maybe) moved where the loop ends, if they were declared
  /// before it (`emit_loop_exit_moves`). The path is the events since
  /// the body started, minus the other branches of the conditionals the
  /// `break` is in, plus their match arms' pattern bindings (as
  /// `ownership_here`). The branch that breaks also records its own
  /// moves (`branch_divergence`); this sees those made before it too
  /// (`loop { drop(s); if c { break; } s = …; }`, #980), and a value
  /// given back before the `break` (`drop(s); s = …; if c { break; }`).
  fn note_break_path_moves(&mut self, target: HirId, brk: HirId, value_moves: &BTreeSet<String>, call: bool) {
    let Some(loop_at) = self.exit_frames.iter()
      .rposition(|f| matches!(f, ExitFrame::Loop { id, .. } if *id == target)) else { return };
    let ExitFrame::Loop { len, .. } = self.exit_frames[loop_at] else { return };
    // Only what was in scope where the loop started: not a `for`
    // loop's pattern bindings, which an inner `for row in row` may move.
    // (A `let s;` local assigned since stands for its columns, #1507.)
    let outer: Vec<String> = self.scope_names(0, len.min(self.in_scope.len())).into_iter().map(|(_, n)| n.to_owned()).collect();
    let declared_before = |rap: &ResourceAccessPoint| {
      let root = rap.name().trim_start_matches('*').split('.').next().unwrap_or("");
      outer.iter().any(|n| n == root)
    };
    let (events, hide) = self.loop_path_events(loop_at);
    let mut candidates = Vec::new();
    moved_from(&events, &mut candidates);
    let mut way_out = value_moves.clone();
    for rap in candidates.into_iter().filter(|r| declared_before(r)) {
      let definite = match path_own(&events, &rap, &hide, PathOwn::Yes) {
        PathOwn::Yes => continue,
        PathOwn::No => true,
        PathOwn::Maybe => false,
      };
      if definite {
        way_out.insert(rap.name().to_owned());
      }
      self.break_moves.push(BreakMove { target, brk, rap, definite, on_path: true });
    }
    // What a call in the `break`'s value moves (`break id(s)`, `break
    // f()` consuming an `FnOnce` closure `f`) is moved on this way out
    // too, as the `break` runs (#1526). (A value that is the variable
    // itself, `break s`, is paired where the loop ends. With one
    // `break`, the move is all there is to say.)
    let several = matches!(self.tcx.hir_node(target), rustc_hir::Node::Expr(l) if breaks_to(l).len() > 1);
    if call && several {
      for n in value_moves {
        let Some(rap) = self.raps.get(n).map(|rd| rd.rap.clone()) else { continue };
        if declared_before(&rap) && !self.break_moves.iter().any(|m| m.brk == brk && m.rap == rap) {
          self.break_moves.push(BreakMove { target, brk, rap, definite: true, on_path: true });
        }
      }
    }
    self.way_out_moves.push((target, Some(brk), way_out));
    // What the code after the loop gets from this `break` (#1000,
    // #1001): the variables declared before the loop that the path
    // gives a value to, and what the loop's `loop_given` ones hold
    // here (one an earlier time round may have given a value: maybe).
    #[expect(clippy::disallowed_methods, reason = "sorted by hash below")]
    let mut assigned: Vec<ResourceAccessPoint> = self.raps.values()
      .map(|rd| rd.rap.clone())
      .filter(|r| !r.is_ref() && !r.is_fn() && declared_before(r))
      .filter(|r| path_own(&events, r, &hide, PathOwn::No) == PathOwn::Yes)
      .collect();
    assigned.sort_by_key(|r| *r.hash());
    let given = self.loop_given.iter().rev().find(|(id, _)| *id == target)
      .map(|(_, g)| g.iter().map(|r| {
        let own = match path_own(&events, r, &hide, PathOwn::Maybe) {
          PathOwn::Yes => Ownership::Held,
          PathOwn::No => Ownership::None,
          PathOwn::Maybe => Ownership::Maybe,
        };
        (r.clone(), own)
      }).collect())
      .unwrap_or_default();
    let untouched = self.loop_maybe_entry.iter().rev().find(|(id, _)| *id == target)
      .map(|(_, e)| e.iter()
        .filter(|m| path_own(&events, &m.ro, &hide, PathOwn::Yes) == PathOwn::Yes
          && path_own(&events, &m.ro, &hide, PathOwn::No) == PathOwn::No)
        .map(|m| m.ro.clone())
        .collect())
      .unwrap_or_default();
    self.break_states.push(BreakState { target, assigned, given, untouched });
  }

  /// At a `break` out of the labeled block `target` (#1286): the path
  /// to it from the block's start, for `emit_block_break_skips`.
  fn note_labeled_break_path(&mut self, target: HirId, brk: HirId, value_moves: BTreeSet<String>) {
    let Some(i) = self.labeled_frames.iter().rposition(|f| f.id == target) else { return };
    let (path, hide) = self.path_events_since(self.labeled_frames[i].events_at, self.labeled_frames[i].frames_at);
    let mut way_out = value_moves.clone();
    way_out.extend(moved_on_path(&path, &hide));
    self.way_out_moves.push((target, Some(brk), way_out));
    self.labeled_frames[i].paths.push((path, hide, value_moves));
  }

  fn emit_assignment_joins(&mut self, expr: &'tcx Expr<'tcx>, imaginary: bool) {
    let Some(join) = self.facts.and_then(|f| f.joins.get(&expr.span)) else { return };
    for (name, always_assigned) in &join.assignments {
      if *always_assigned != imaginary { continue; }
      if let Some(rd) = self.raps.get(name) {
        let ro = rd.rap.clone();
        self.add_external_event(span_end_line(&expr.span, &self.tcx), ExternalEvent::AssignmentJoin {
          ro, always_assigned: *always_assigned, id: *self.unique_id,
        });
        *self.unique_id += 1;
      }
    }
  }

  /// At the end of the labeled block `expr` (#1286): a variable declared
  /// before it that the way to its end moves, where a `break` out of it
  /// before the move skipped it (`'a: { if c { break 'a 1; } drop(s); 2
  /// }`), may have been moved after the block. A `break` that comes after
  /// the move is `emit_loop_exit_moves`' (`emitted`).
  /// `through`: the events on the way to its end.
  fn emit_block_break_skips(&mut self, expr: &'tcx Expr<'tcx>, frame: LabeledFrame, through: &[(usize, ExternalEvent)], emitted: &[ResourceAccessPoint]) {
    if frame.paths.is_empty() {
      return;
    }
    let events = through;
    // (A `let s;` local assigned since stands for its columns, #1507.)
    let outer: Vec<String> = self.scope_names(0, frame.len.min(self.in_scope.len())).into_iter().map(|(_, n)| n.to_owned()).collect();
    let mut candidates = Vec::new();
    moved_from(events, &mut candidates);
    let skipped: Vec<ResourceAccessPoint> = candidates.into_iter()
      .filter(|r| {
        let root = r.name().trim_start_matches('*').split('.').next().unwrap_or("");
        outer.iter().any(|n| n == root) && !emitted.contains(r)
      })
      .filter(|r| path_own(events, r, &[], PathOwn::Yes) == PathOwn::No
        // (Not a `break` whose value moves it: `break 'a s` moves s
        // on that path too, #1504.)
        && frame.paths.iter().any(|(path, hide, value_moves)| !value_moves.contains(r.name())
          && path_own(path, r, hide, PathOwn::Yes) == PathOwn::Yes))
      .collect();
    let line = span_end_line(&expr.span, &self.tcx);
    for ro in skipped {
      self.add_external_event(line, ExternalEvent::LoopExitMove {
        ro, definite: false, at: MaybeMovedAt::LabeledBlock { skipped: true, carried: false }, id: *self.unique_id,
      });
      *self.unique_id += 1;
    }
  }

  /// The events on the path from the start of the loop whose frame is
  /// `exit_frames[loop_at]` (its body, after a `while`'s condition or
  /// a `for`'s head) to here, in the order they happened (each match
  /// arm's pattern bindings where its body starts), and the ranges of
  /// event ids among them that aren't on the path (the other branches
  /// of the conditionals this point is in). See `ownership_here`.
  fn loop_path_events(&self, loop_at: usize) -> (Vec<(usize, ExternalEvent)>, Vec<std::ops::Range<usize>>) {
    let ExitFrame::Loop { events_at, .. } = self.exit_frames[loop_at] else { return (Vec::new(), Vec::new()) };
    self.path_events_since(events_at, loop_at + 1)
  }

  /// `loop_path_events` from the event `events_at` on, inside the
  /// conditionals of `exit_frames[frames_at..]`.
  fn path_events_since(&self, events_at: usize, frames_at: usize) -> (Vec<(usize, ExternalEvent)>, Vec<std::ops::Range<usize>>) {
    let events_at = events_at.min(self.preprocessed_events.len());
    let hide: Vec<std::ops::Range<usize>> = self.exit_frames.iter().flat_map(|f| match f {
      ExitFrame::Branch { hide, .. } => hide.clone(),
      ExitFrame::Loop { .. } => Vec::new(),
    }).collect();
    let mut events: Vec<(usize, ExternalEvent)> = Vec::new();
    let mut from = events_at;
    for f in &self.exit_frames[frames_at.min(self.exit_frames.len())..] {
      if let ExitFrame::Branch { binds, binds_at, .. } = f {
        if binds.is_empty() {
          continue;
        }
        let at = (*binds_at).clamp(from, self.preprocessed_events.len());
        events.extend_from_slice(&self.preprocessed_events[from..at]);
        events.extend_from_slice(binds);
        from = at;
      }
    }
    events.extend_from_slice(&self.preprocessed_events[from..]);
    (events, hide)
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
      ExprKind::Continue(_) => branch,
      // Or a `return` that ends every path, drawn with its drops
      // (#1041).
      ExprKind::Ret(_) => branch || self.returns_from_breakless_loop(e),
      // A `loop` that never ends, through its body (#1041).
      ExprKind::Loop(b, ..) if !branch && self.loop_never_ends(e) => self.block_ends_at_exits(b, false),
      // A `break` out of a labelled block drops nothing drawn.
      ExprKind::Break(dest, _) => branch && dest.target_id.is_ok_and(|t|
        matches!(self.tcx.hir_node(t), Node::Expr(Expr { kind: ExprKind::Loop(..), .. }))),
      ExprKind::Call(..) | ExprKind::MethodCall(..) => (branch && (self.panic_line(e).is_some() || self.process_exit_line(e).is_some()))
        || (self.panic_line(e).is_some() && self.panic_leaves_every_path(e)),
      // A body drawn inline (an `if let` whose pattern always matches,
      // a lone match arm) is where the code around it is: its exits'
      // drops are drawn as they would be there (#1013).
      ExprKind::If(c, then, _) if !e.span.from_expansion() && self.always_matching_if_let(c) =>
        self.ends_at_exits(then, branch),
      ExprKind::Match(_, [arm], rustc_hir::MatchSource::Normal) if !e.span.from_expansion() =>
        self.ends_at_exits(arm.body, branch),
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
  /// A Copy value or a reference declared in a branch that the exits
  /// ended (`exit_scope_ended`) is left too: its scope ends at the
  /// exit (#1348).
  fn left_at_exits(&self, ends: bool, decls: &HashSet<ResourceAccessPoint>) -> HashSet<ResourceAccessPoint> {
    if !ends {
      return HashSet::new();
    }
    #[expect(clippy::disallowed_methods, reason = "collected into a set; order is lost")]
    let left = decls.iter().filter(|v| !v.never_drops() || self.exit_scope_ended.contains(v.name().as_str())).cloned().collect();
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
    self.register_statics(body);

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
    // (Not a tail that never runs, #1004.)
    if let Some(e) = plain_tail.filter(|_| !self.dead_tail) {
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
    // The parts of the argument the pattern doesn't bind are dropped
    // when the function returns: the first binding's dot says so (#1181).
    let mut rest = {
      let mut parts = Vec::new();
      self.unbound_param_parts(param.pat, typeck.pat_ty(param.pat), String::new(), param.hir_id.owner, &mut parts);
      (!parts.is_empty()).then(|| ParamRest {
        fn_name: self.tcx.item_name(param.hir_id.owner.to_def_id()).to_string(),
        parts,
      })
    };
    // A parameter whose pattern binds no name (`_: String`, `(_, _):
    // (String, String)`) still owns the caller's argument: it stays in
    // the parameter and is dropped when the function returns. With no
    // binding to draw it on, it gets a `(parameter)` column, like the
    // value a `ref` binding borrows (#691), so the drop is stated (#1243).
    if bindings.is_empty() {
      let ty = typeck.pat_ty(param.pat);
      let owner_id = param.hir_id.owner;
      if self.needs_drop(ty, owner_id) {
        let line_num = span_to_line(&param.span, &self.tcx);
        self.param_value_count += 1;
        let col = parameter_label(self.param_value_count);
        self.add_owner(col.clone(), false, false, self.ty_is_plain(ty, owner_id), self.ty_has_interior_mut(ty, owner_id), self.current_scope, !self.inside_branch);
        self.mark_owner_type(&col, ty);
        let col_rap = self.raps.get(&col).unwrap().rap.to_owned();
        let fn_name = self.tcx.item_name(owner_id.to_def_id()).to_string();
        self.add_external_event(line_num, ExternalEvent::InitRefParam { param: col_rap, id: *self.unique_id, own_param: None, in_pattern: None, rest: Some(ParamRest { fn_name, parts: Vec::new() }) });
        *self.unique_id += 1;
      }
      return;
    }
    for pat in bindings {
      // `ref s` on a by-value parameter: `s` borrows the value this
      // function owns, not the caller's (#377).
      let own_param = own_refs.iter().find(|(id, _)| *id == pat.hir_id).map(|&(_, whole)| OwnParam {
        fn_name: self.tcx.item_name(param.hir_id.owner.to_def_id()).to_string(),
        whole,
        // The pointee of the `&T` the binding is: dropped with the
        // parameter when the function returns, if its type needs drop.
        drops: typeck.pat_ty(pat).builtin_deref(false).is_some_and(|t|
          self.needs_drop(t, param.hir_id.owner)),
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
            // (StaticDie / MutableDie emitted by emit_borrow_ends)
            // lands at the fn body's closing brace rather than the
            // parameter declaration line.
            //
            // Without this fix, add_ref left the lender as
            // Anonymous and the lifetime equal to the param's own
            // line — emit_borrow_ends then emitted a phantom
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
              self.add_external_event(line_num, ExternalEvent::InitRefParam { param: col_rap, id: *self.unique_id, own_param: own_param.clone(), in_pattern: None, rest: rest.take() });
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
                  self.mark_owner_type(&name, ty);
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
          self.add_external_event(line_num, ExternalEvent::InitRefParam { param: self.raps.get(&name).unwrap().rap.to_owned(), id: *self.unique_id, own_param, in_pattern: None, rest: rest.take() });
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
            self.add_external_event(line_num, ExternalEvent::InitRefParam { param: r, id: *self.unique_id, own_param: None, in_pattern: None, rest: None });
            *self.unique_id += 1;
          }
          self.annotate_src(name.clone(), ident.span, false, *self.raps.get(&name).unwrap().rap.hash());
        }
        _ => {}
      }
    }
  }

  fn visit_expr(&mut self, expr: &'tcx Expr<'tcx>) {
    let outer = self.emitting_span.replace(expr.span);
    self.visit_expr_at(expr);
    self.emitting_span = outer;
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
        self.refuse_user_macro_use(local.span, init);
      }
      return;
    }
    let local_line = self.tcx.sess.source_map().lookup_char_pos(local.span.lo()).line;
    let is_skipped = self.skip_lines.contains(&local_line);
    if let Some(init) = local.init {
      self.process_let_binding(local.pat, init, is_skipped, local.els);
    } else {
      // `let x;` (deferred initialization, #234): x owns nothing yet,
      // so register no RAP and emit no event here. Remember each
      // binding; its first assignment registers the RAP with this
      // `let`'s scope and reads as initialization (see the Assign arm).
      let (scope, inside_branch, frames_at) = (self.current_scope, self.inside_branch, self.exit_frames.len());
      local.pat.each_binding(|annotation, hir_id, _, _| {
        self.uninit_locals.insert(hir_id, DeferredLet { annotation, scope, inside_branch, is_skipped, frames_at });
        // Its place among the locals an exit drops (#1436).
        self.in_scope.push(deferred_slot(hir_id));
      });
    }

    if let (Some(els), None) = (local.els, local.init) {
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
      };
      Some(format!("{} {{ .. }}", name))
    }
    ExprKind::Tup(_) => Some("(..)".to_owned()),
    ExprKind::Array(_) => Some("[..]".to_owned()),
    _ => None,
  }
}

/// True iff `pat` may not match a value of its type. Conservative: an
/// or-pattern, a literal or a variant (`Some(_)`) counts as refutable;
/// a struct or tuple pattern is refutable iff a sub-pattern is.
fn pat_refutable(pat: &Pat) -> bool {
  match pat.kind {
    PatKind::Wild | PatKind::Binding(.., None) => false,
    PatKind::Binding(.., Some(sub)) | PatKind::Box(sub) | PatKind::Deref(sub) | PatKind::Ref(sub, ..) => pat_refutable(sub),
    PatKind::Tuple(ps, _) => ps.iter().any(pat_refutable),
    PatKind::Struct(QPath::Resolved(_, p), fields, _) if matches!(p.res, Res::Def(DefKind::Struct, _) | Res::SelfTyAlias { .. }) =>
      fields.iter().any(|f| pat_refutable(f.pat)),
    PatKind::TupleStruct(QPath::Resolved(_, p), ps, _) if matches!(p.res, Res::Def(DefKind::Ctor(rustc_hir::def::CtorOf::Struct, _), _) | Res::SelfCtor(_)) =>
      ps.iter().any(pat_refutable),
    _ => true,
  }
}

/// Whether `a` and `b` name the same place: the same local, or the same
/// field of the same place (`v` and `v`, `p.f` and `p.f`).
fn same_place(a: &Expr, b: &Expr) -> bool {
  match (a.kind, b.kind) {
    (ExprKind::Path(QPath::Resolved(None, pa)), ExprKind::Path(QPath::Resolved(None, pb))) =>
      matches!((pa.res, pb.res), (Res::Local(x), Res::Local(y)) if x == y),
    (ExprKind::Field(ra, fa), ExprKind::Field(rb, fb)) => fa.name == fb.name && same_place(ra, rb),
    _ => false,
  }
}

/// The temporary an extending `&` borrows (part of): `operand` itself
/// when it's a value expression (`&String::from("a")`), or the value
/// expression a field or index projection is applied to
/// (`&make_user().name`, `&String::from("a")[1..]`, `&mk()[0].name`),
/// which Rust's temporary lifetime extension reaches too (#1165). None
/// when the `&` borrows a place (`&x`, `&v[0]`, `&p.f`, `&*r`) or
/// another borrow (`&&x`).
pub(crate) fn extended_temp_operand<'a, 'tcx>(operand: &'a Expr<'tcx>) -> Option<&'a Expr<'tcx>> {
  let mut base = operand;
  while let ExprKind::Field(b, _) | ExprKind::Index(b, _, _) = base.kind {
    base = b;
  }
  match base.kind {
    ExprKind::Path(_) | ExprKind::Unary(UnOp::Deref, _) | ExprKind::AddrOf(..) => None,
    _ => Some(base),
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

/// Iterator adapters that hand out their receiver's items as they are.
const SAME_ITEMS: &[&str] = &["rev", "skip", "take", "step_by", "skip_while", "take_while", "filter", "peekable", "fuse", "inspect"];

impl<'a, 'tcx> ExprVisitor<'a, 'tcx> {
  fn visit_expr_at(&mut self, expr: &'tcx Expr<'tcx>) {
    // A value a branch of the returned `if` / `match` yields goes to
    // the caller once it's evaluated, inside its branch (#674).
    if self.tail_values.remove(&expr.hir_id) {
      self.visit_expr(expr);
      self.return_to_caller(expr);
      self.note_tied_ref_params(expr);
      return;
    }
    // Likewise a value a branch of an `if` / `match` written as a call's
    // argument yields is passed to the call inside its branch (#1174).
    if let Some((fn_name, manually_takes)) = self.arg_values.remove(&expr.hir_id) {
      self.visit_expr(expr);
      self.match_call_arg(expr, fn_name, manually_takes);
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
        if self.manually_drop_by_hand(fn_expr, args) {
          return;
        }
        // `let W(y) = W(g);`: a constructor that a `let` pattern takes
        // apart is no actor either (as below, #950). `bind_pat` draws
        // `g` to `y`, and moves what the pattern leaves into the
        // literal's temporary. (Registered for the code panel only.)
        if self.let_ctor_literals.remove(&expr.hir_id) {
          if let Some(name) = hirid_to_var_name(fn_expr.hir_id, &self.tcx) {
            self.add_fn(name);
          }
          for arg in args.iter() {
            self.visit_expr(arg);
          }
          return;
        }
        // `match Some(f) { Some(x) => .. }`: the constructor is taken
        // apart like a tuple literal (#807). What goes into it is drawn
        // where the pattern puts it (`f` to `x`), and what the pattern
        // leaves of a variable is dropped with the temporary on the
        // variable's column. The constructor has no column: no event
        // names it (it's registered for the code panel only).
        if !expr.span.from_expansion() && self.literal_elems(expr).is_some() {
          if let Some(name) = hirid_to_var_name(fn_expr.hir_id, &self.tcx) {
            self.add_fn(name);
          }
          for arg in args.iter() {
            self.visit_expr(arg);
          }
          // (After the arguments: the temporary takes what a call
          // among them returned.)
          if !self.is_drawn_tuple_scrutinee(expr) {
            self.note_ctor_literal_temporary_drop(expr);
          }
          return;
        }
        if let Some(name) = hirid_to_var_name(fn_expr.hir_id, &self.tcx) {
          self.note_temporary_drop(expr, name, Vec::new());
        }

        // A tuple struct / variant constructor (`Some(&s)`, `Wrapper(s)`)
        // is not a function that acts: an argument that is a borrow isn't
        // borrowed *by the constructor*. The variable the built value
        // is bound to holds that borrow (drawn from the binding), and a
        // temporary passed to a call is borrowed by that call
        // (`match_arg`'s constructor case). Moves and copies into the
        // constructor are still drawn (#768).
        if !expr.span.from_expansion() && is_ctor_path(fn_expr, self.tcx.typeck(expr.hir_id.owner)) {
          if let Some(name) = hirid_to_var_name(fn_expr.hir_id, &self.tcx) {
            self.add_fn(name.clone());
            let typeck = self.tcx.typeck(expr.hir_id.owner);
            let lo = *self.unique_id;
            let to_call = matches!(self.tcx.parent_hir_node(expr.hir_id),
              rustc_hir::Node::Expr(p) if matches!(p.kind, ExprKind::Call(..) | ExprKind::MethodCall(..)));
            for arg in args.iter() {
              self.visit_expr(arg);
              let arg_ty = typeck.node_type(arg.hir_id);
              if !arg_ty.is_ref() {
                self.match_arg(arg, name.clone());
              } else if !to_call && matches!(arg.kind, ExprKind::Path(_) | ExprKind::Field(..)) {
                // A reference variable put into the value: its copy
                // (or move, for `&mut`) goes where the value goes, and
                // is drawn there once that's known (#768).
                let from = self.rap_or_field_through_ref(arg);
                let line = expr_to_line(arg, &self.tcx);
                if let (ResourceTy::Value(rap) | ResourceTy::Deref(rap), Some(to)) = (&from, self.raps.get(&fn_key(&name))) {
                  let (rap, to) = (rap.to_owned(), ResourceTy::Value(to.rap.to_owned()));
                  self.update_rap(&rap, line);
                  // `let o: Option<&i32> = Some(r);` (or `Option<&mut
                  // i32>`) with r a `&mut`: rustc reborrows r where the
                  // expected type is a reference, so r isn't moved; the
                  // holder's borrow is drawn as for a shared reference's
                  // copy (#1329).
                  let evt = if arg_ty.ref_mutability() == Some(Mutability::Mut) && !is_implicit_reborrow(arg, typeck) { Evt::Move } else { Evt::Copy };
                  self.add_ev(line, evt, to, from, false);
                }
              } else if !to_call {
                // `Some(x.trim())` with x a reference: the reborrow goes
                // where the value goes, as `Some(x)` does (#1340).
                if let Some(to) = self.raps.get(&fn_key(&name)).map(|rd| ResourceTy::Value(rd.rap.to_owned())) {
                  self.ref_part_to(arg, to);
                }
              }
            }
            self.ctor_arg_ids.insert(expr.hir_id, (lo, *self.unique_id));
            // An unbound temporary (`Some(make());`) takes what the
            // arguments moved in, not the constructor (#783).
            if let Some((tmp, id)) = self.ctor_temps.remove(&expr.hir_id) {
              if self.retarget_ctor_arg_events(expr, ResourceTy::Value(tmp)) {
                self.preprocessed_events.retain(|(_, e)| e.get_id() != id);
              }
            }
            return;
          }
        }

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
        self.closure_call_events(expr, fn_expr);
        let defer = args.iter().skip(1).any(|a| holds_try(a));
        let manually_takes = self.facts.is_some_and(|b| b.take_calls.contains(&expr.span.with_parent(None)));
        self.visit_call_args(args, &fn_name, defer, manually_takes, |_| {});
        // `g()` with `g` a `Box<dyn Fn() + '_>` holding a borrow:
        // calling it uses it, so it holds the borrow at least until
        // here (#1104). A borrow rustc's facts don't give an end for
        // (`|| r.len()` reborrows through the shared reference r)
        // would otherwise end where g was made. (A reference `f: &F`
        // called as `f()` keeps the end it has.)
        if let ExprKind::Path(QPath::Resolved(None, p)) = fn_expr.kind {
          let line = expr_to_line(expr, &self.tcx);
          let holder = self.raps.get(&fn_name).map(|rd| rd.rap.to_owned())
            .filter(|r| matches!(p.res, Res::Local(_)) && !r.is_ref() && r.holds_borrow())
            .filter(|r| self.borrow_map.get(r.name()).is_some_and(|d| d.lifetime < line));
          if let Some(rap) = holder {
            self.update_rap(&rap, line);
          }
        }
        // `f()` with `let f = || unsafe { COUNT = 9; };`: the call
        // writes the static (a closure names a static, it doesn't
        // capture it; #1203).
        if let Some(c) = local_closure_of(&self.tcx, fn_expr) {
          let line = expr_to_line(expr, &self.tcx);
          for id in statics_written_in_closure(&self.tcx, c) {
            let name = self.tcx.def_path_str(id);
            if let Some(rap) = self.raps.get(&name).map(|r| r.rap.to_owned()) {
              self.add_ev(line, Evt::Bind, ResourceTy::Value(rap), ResourceTy::Anonymous, false);
            }
          }
        }
        self.swap_ref_vars(fn_expr, args, expr_to_line(expr, &self.tcx));
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
        // The arguments are visited whether or not the receiver has a
        // column. In `a.last_mut().unwrap().push(s)` the receiver of
        // `push` is a temporary, but `s` is still moved into `push`
        // (#132); in `self.items.push(s)` the receiver is a field we
        // don't model, but `s` is still moved (#250).
        // A receiver taken by value (`pair.eat(f()?)`) is moved into
        // the call only once a `?` in the arguments has gone on (#1195).
        let by_value = !self.tcx.typeck(expr.hir_id.owner).expr_ty_adjusted(rcvr).is_ref();
        let defer = args.iter().skip(1).any(|a| holds_try(a)) || (by_value && args.iter().any(|a| holds_try(a)));
        if !(defer && by_value) {
          self.method_receiver_events(expr, name_and_generic_args, rcvr, &fn_name, line_num);
        }
        self.visit_call_args(args, &fn_name, defer, false, |this| if defer && by_value {
          this.method_receiver_events(expr, name_and_generic_args, rcvr, &fn_name, line_num);
        });
        // `refs.push(&s)`: the receiver now holds s's borrow (#392).
        if !matches!(rcvr.kind, ExprKind::MethodCall(..)) {
          let rcvr_rap = expr_to_rap_name(rcvr, &self.tcx)
            .and_then(|n| self.raps.get(&n))
            .map(|rd| rd.rap.to_owned());
          if let Some(rcvr_rap) = rcvr_rap {
            self.record_receiver_store(expr, &rcvr_rap, line_num);
          }
        }
        // A method of the user's that never returns (#1553).
        self.note_panic(expr);
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
        } else if let Some((_, operands)) = builtin_binop_copied_operands(expr, &self.tcx, &self.raps) {
          // A built-in arithmetic / bitwise operator on primitives
          // (`x + y` on i32) reads its operands: each one that names a
          // column is copied (#241), and so is each operand of a
          // built-in comparison (#497, `item == b' '`). The operator
          // is not an actor: no column, no arrow (#908). A comparison
          // that calls a trait method reads through a reference: see
          // below.
          for operand in operands {
            self.read_copied_operand(operand);
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
            self.add_pass_by_sref_ev(line_num, ResourceTy::Value(fn_rap), ResourceTy::Value(rap), implicit, false, false);
          }
        }
      }

      ExprKind::AddrOf(_, _, exp) => {
        self.visit_expr(exp);
        // A temporary a `let` extends through a branch (#1397).
        self.make_branch_temp(expr);
      }

      // Compound assignment `a op= b` (#260). It doesn't store `b` in
      // `a`, so it isn't drawn like `a = b`:
      // - On primitives (`acc += x` with i32s) it's built in: `acc` is
      //   read, combined with `x`, and the result written back. Like
      //   `acc + x` (#241), each operand that names a column (`acc`
      //   and `x`) is copied (the operator is not an actor, #908), and
      //   `acc` then gets a new value. Nothing is dropped, and nothing
      //   is borrowed (a shared borrow of `acc` that's still live
      //   makes it E0506 "cannot assign", not a borrow conflict).
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
              self.add_pass_by_mref_ev(line_num, ResourceTy::Value(fn_rap), ResourceTy::Value(rap), false, false, ImplicitRef::Autoref, false);
            }
            Some(lhs_rty @ (ResourceTy::Deref(_) | ResourceTy::Index(..) | ResourceTy::DerefField(..))) => {
              self.write_field_behind_ref(&lhs_rty, lhs_expr, line_num);
              let lhs_rty = match lhs_rty {
                ResourceTy::Index(r, place, w) => ResourceTy::Index(r, place, w.into_update()),
                other => other,
              };
              self.add_ev(line_num, Evt::Bind, lhs_rty.clone(), ResourceTy::Anonymous, false);
              self.write_part_behind_ref_update(&lhs_rty, lhs_expr, line_num);
            }
            _ => {}
          }
          self.match_arg(rhs_expr, fn_name);
          return;
        }
        // Both operands are read, as in `acc = acc + x`: the old value
        // of `acc` is copied ("acc's value is copied"), then `x`. As
        // for `acc + x`, the operator is not an actor (#908).
        for operand in [lhs_expr, rhs_expr] {
          if operand_names_column(operand, &self.tcx, &self.raps) {
            self.read_copied_operand(operand);
          }
        }
        // The write: "acc is bound to a value", or through a
        // reference, "the resource r points to is modified through *r".
        // An element (`v[i] += 1`) has no column of its own: the write
        // goes on the receiver's, as an in-place update.
        match self.resource_of_lhs(lhs_expr) {
          Some(lhs_rty @ (ResourceTy::Value(_) | ResourceTy::Deref(_) | ResourceTy::DerefField(..))) => {
            let lhs_rty = match lhs_rty {
              ResourceTy::Deref(r) => match self.projected_access(expr.span, &r, |f| match f {
                crate::facts::Fact::Init { path, .. } => Some(path), _ => None,
              }) {
                Some(place) => {
                  let ty = self.tcx.typeck(expr.hir_id.owner).expr_ty(lhs_expr);
                  ResourceTy::DerefField(r, place, PlaceWrite::Update, self.field_holds(ty, expr.hir_id.owner))
                }
                None => ResourceTy::Deref(r),
              },
              other => other,
            };
            self.write_field_behind_ref(&lhs_rty, lhs_expr, line_num);
            self.note_reference_slot_write(expr.span, &lhs_rty);
            self.add_ev(line_num, Evt::Bind, lhs_rty.clone(), ResourceTy::Anonymous, false);
            self.write_part_behind_ref_update(&lhs_rty, lhs_expr, line_num);
          }
          Some(ResourceTy::Index(r, place, w)) => {
            self.add_ev(line_num, Evt::Bind, ResourceTy::Index(r, place, w.into_update()), ResourceTy::Anonymous, false);
          }
          _ => {}
        }
      }

      // assignment 
      // ex a = <expr>
      ExprKind::Assign(lhs_expr, rhs_expr, _,) => self.visit_assign(expr, lhs_expr, rhs_expr),

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
        // (Or one directly in a `loop` that never ends, #1041.)
        let breakless = self.returns_from_breakless_loop(expr);
        if breakless || self.returns_before_body_end(expr) {
          self.drop_locals_at_fn_return(expr, ret);
        } else {
          self.drop_locals_on_exit(expr_to_line(expr, &self.tcx), ret, None, false, "return");
        }
        if !self.in_closure(expr) {
          self.drop_loop_iterators_on_return(expr_to_line(expr, &self.tcx));
        }
        if !breakless {
          self.note_return_in_loop(expr);
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
        if label.is_some() {
          self.labeled_frames.push(LabeledFrame {
            id: block.hir_id, events_at: self.preprocessed_events.len(),
            frames_at: self.exit_frames.len(), len: entry_len, paths: Vec::new(),
          });
        }
        let exited_before = self.all_paths_exit;
        let outer_try = self.try_block;
        if block.targeted_by_break && label.is_none() {
          self.try_block = Some(TryBlockScope {
            id: block.hir_id, locals: entry_len,
            tuples: self.live_tuples.len(), iterators: self.loop_iterators.len(),
          });
        }
        self.visit_live_stmts(block);
        self.try_block = outer_try;
        // An exit in it that every path leaves the function at (a
        // `return` or panic in a `loop` that never ends, #1545) dropped
        // what it declares.
        let exited_in = self.all_paths_exit && !exited_before;
        self.in_scope.truncate(entry_len);
          self.current_scope = prev_scope;
        // A bare block (`{ let u = …; }`) in a branch: its locals aren't
        // global, so `print_out_of_scope` skips them and no enclosing
        // construct ends them. They go out of scope at its `}` (#591).
        // (An `if` / `else` / loop body, a match arm, a closure body
        // is not a bare block: its construct ends its own locals.)
        // Unless every path through it leaves the branch it is in at an
        // exit that drops them: its `}` is never reached (#1013).
        if self.inside_branch
          && matches!(self.tcx.parent_hir_node(expr.hir_id), rustc_hir::Node::Stmt(_) | rustc_hir::Node::Block(_))
        {
          let left = self.left_at_exits(exited_in || (self.inline_in_branch(expr) && self.ends_at_exits(expr, true)),
            &get_decl_of_block(block, &self.tcx, &self.raps));
          let decls: HashSet<ResourceAccessPoint> = get_decl_of_block(block, &self.tcx, &self.raps)
            .into_iter().filter(|v| needs_scope_end(v) && !left.contains(v)).collect();
          self.emit_scope_end(decls, new_scope);
        }
        // A labeled block (`'a: { … break 'a; … }`) can also end by
        // running to its end.
        if let Some(before) = before {
          // The events on the way to its end, if that is reached.
          let frame = self.labeled_frames.pop();
          let through = frame.as_ref().filter(|_| !self.block_diverges(block))
            .map(|f| self.preprocessed_events[f.events_at.min(self.preprocessed_events.len())..].to_vec());
          // Its end is a way out too, moving what the way there and
          // its last expression move (#1504).
          if let Some(through) = &through {
            let mut way_out = moved_on_path(through, &[]);
            if let Some(tail) = block.expr {
              way_out.extend(locals_moved_by(self.tcx, tail));
            }
            self.way_out_moves.push((block.hir_id, None, way_out));
          }
          let emitted = self.emit_loop_exit_moves(expr, Some(block.hir_id), &before, true, through.as_deref());
          if let (Some(frame), Some(through)) = (frame, through) {
            self.emit_block_break_skips(expr, frame, &through, &emitted);
          }
          self.emit_assignment_joins(expr, false);
        }
      }

      // `-x` / `!b` take their operand by value, like `x + y` (#262):
      // a built-in one on a primitive copies a named operand (the
      // operator is not an actor, #908); an overloaded one is a call
      // to `Neg::neg` / `Not::not` with the operand as its argument.
      ExprKind::Unary(UnOp::Neg | UnOp::Not, operand) => {
        self.visit_expr(operand);
        if let Some((op_name, operand)) = unary_op_operand(expr, &self.tcx, &self.raps) {
          if self.tcx.typeck(expr.hir_id.owner).is_method_call(expr) {
            let fn_name = op_name.to_owned();
            self.add_fn(fn_name.clone());
            self.match_arg(operand, fn_name);
          } else {
            self.read_copied_operand(operand);
          }
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
          // `const` items don't get RAP timelines — they're
          // compile-time values: each use is a new copy of the value.
          // Reading one in `let x = N;` makes `x` an owner of the
          // resulting copy; the const itself doesn't need a column.
          // Skip the path-expression event so the lookup below doesn't
          // trip on the missing RAP. (A `static` has a column, #690.)
          Res::Def(DefKind::Const { .. } | DefKind::AssocConst { .. }, _) => {
            return;
          }
          _ => ()
        }
        // Only a local binding or a static (#690) can be a variable RAP.
        // Other paths (fn items, consts) have nothing to update, and
        // looking their name up among the variables would update a
        // variable that merely shares the name (#232).
        if !matches!(p.res, Res::Local(_)) && static_of_path(p).is_none() {
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
        self.visit_if(expr, guard_expr, if_expr, else_expr);
        self.emit_assignment_joins(expr, true);
      }

      ExprKind::Loop(block, _, loop_ty, _span) => {
        self.note_breakless_loop(expr);
        // What a `break` / `continue` in the body drops (#410). A
        // for-loop's frame is pushed where its body is visited, in the
        // ForLoopDesugar arm.
        let entry_len = self.in_scope.len();
        if !matches!(loop_ty, rustc_hir::LoopSource::ForLoop) {
          let events_at = self.preprocessed_events.len();
          self.exit_frames.push(ExitFrame::Loop { id: expr.hir_id, len: entry_len, head_from: entry_len, events_at });
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
                let body_end = self.tcx.sess.source_map()
                  .lookup_char_pos(then_body.span.hi()).line;
                // The loop ends when the condition turns out false, which
                // may be after it moved a variable declared before the
                // loop (#968): `while let Some(x) = it.next() && { let w
                // = s; … }`, `while let Some(c) = first(s)`. Its moves are
                // recorded like an `if` let-chain's (`ChainWalk`), from
                // its first part on: the loop runs it again after the
                // body, so the body may have given the variable a new
                // value, but not after the loop ends.
                let outer: HashSet<ResourceAccessPoint> = self.in_scope.iter()
                  .filter_map(|name| self.raps.get(name))
                  .map(|rd| rd.rap.clone()).filter(|r| !r.is_fn()).collect();
                let cond_lo = self.preprocessed_events.len();
                let mut walk = ChainWalk { first_lo: Some(*self.unique_id), body_end, ..ChainWalk::default() };
                if let ExprKind::Let(let_expr) = cond.kind {
                  let mark = self.preprocessed_events.len();
                  let loans = self.outer_loans(&outer);
                  self.visit_expr(let_expr.init);
                  self.record_chain_effects(mark, &outer, cond, &mut walk, false, &loans);
                  if !pat_is_irrefutable(let_expr.pat, self.tcx.typeck(let_expr.pat.hir_id.owner)) {
                    walk.checks += 1;
                  }
                  bindings = self.register_iflet_let_bindings(let_expr, None, body_end).0;
                } else if is_let_chain(cond) {
                  // `while let A = x && cond` (#198): register each
                  // `let`'s bindings like a lone while-let's. The loop
                  // is drawn inline (no Branch), so the chain's events
                  // are shown unconditionally. Their scope ends are
                  // emitted after the body, below.
                  let lo = *self.unique_id;
                  self.visit_if_let_chain(cond, &outer, &mut bindings, &mut walk);
                  let binds = self.chain_binding_events(&(lo..*self.unique_id), &bindings);
                  let cols = self.chain_binding_columns(&binds, &bindings);
                  self.binding_columns.extend(cols);
                } else {
                  let mark = self.preprocessed_events.len();
                  let loans = self.outer_loans(&outer);
                  self.visit_expr(cond);
                  self.record_chain_effects(mark, &outer, cond, &mut walk, false, &loans);
                  walk.checks += 1;
                }
                // Only its moves: what else it does to an outer variable
                // is drawn where it happens and lasts the same way
                // whether or not the loop goes on.
                walk.effects.retain(|e| matches!(e.kind, ChainEffectKind::Move { .. }));
                let cond_moves = self.chain_fail_moves(&walk, cond_lo, FailPath::NoElse).moves;
                // A `break` in the body comes after the condition held:
                // its moves are the condition's own way out (above), so
                // the path to a `break` starts here (#980).
                let len = self.preprocessed_events.len();
                if let Some(ExitFrame::Loop { id, events_at, .. }) = self.exit_frames.last_mut() {
                  if *id == expr.hir_id {
                    *events_at = len;
                  }
                }
                // What the loop may come back round to moved (#992), or
                // given a value after a move before it (#1001).
                let head_moved = self.note_loop_head_moves(expr, entry_len);
                self.note_loop_head_given(expr, entry_len, Some(cond_lo));
                let before = self.rap_names();
                self.inside_branch = true;
                // The body may run no times or many (#786).
                let inits = self.enter_loop_inits(expr.hir_id);
                self.visit_expr(then_body);
                self.leave_loop_inits(expr, expr.hir_id, inits, true);
                self.inside_branch = prev;
                // The body's `let`s, and the pattern's bindings, go out
                // of scope at its closing brace (#256); nothing else
                // emits that for them.
                let mut decls = get_decl_of_expr(then_body, &self.tcx, &self.raps);
                decls.extend(bindings);
                // Not where every path breaks or continues first (#631).
                let decls = &decls - &self.left_at_exits(self.ends_at_exits(then_body, false), &decls);
                self.emit_scope_end(decls, body_end);
                // A `while` also ends when its condition is false.
                let broke_moved = self.emit_loop_exit_moves(expr, None, &before, true, None);
                self.emit_loop_round_moves(expr, head_moved, &broke_moved);
                // After the condition moved something (#968). A variable
                // a `break` also moved says so already.
                let line = span_end_line(&expr.span, &self.tcx);
                for m in cond_moves.into_iter().filter(|m| !broke_moved.contains(&m.rap)) {
                  self.add_external_event(line, ExternalEvent::LoopExitMove {
                    ro: m.rap,
                    definite: m.definite,
                    at: MaybeMovedAt::WhileCondFail { to: m.to, dropped: m.dropped, how: m.how },
                    id: *self.unique_id,
                  });
                  *self.unique_id += 1;
                }
                // What it gave a variable moved before it (#1001).
                self.emit_loop_exit_states(expr, Vec::new(), &broke_moved, false);
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
            // What owns a value as it starts (#1000).
            let entry = self.loop_entry_owners();
            // What the loop may come back round to moved (#992), or
            // given a value after a move before it (#1001). A bare
            // `loop` ends only through a `break`, which sees it.
            self.note_loop_head_moves(expr, entry_len);
            self.note_loop_head_given(expr, entry_len, None);
            self.inside_branch = true;
            // Left only by a `break` (#786).
            let inits = self.enter_loop_inits(expr.hir_id);
            self.visit_block(block);
            self.leave_loop_inits(expr, expr.hir_id, inits, false);
            self.inside_branch = prev;
            // Same as the while arm: the body's `let`s end at its
            // closing brace (#256).
            let body_end = self.tcx.sess.source_map()
              .lookup_char_pos(block.span.hi()).line;
            let decls = get_decl_of_block(block, &self.tcx, &self.raps);
            // Not where every path breaks or continues first (#631).
            let decls = &decls - &self.left_at_exits(self.block_ends_at_exits(block, false), &decls);
            self.emit_scope_end(decls, body_end);
            // A bare `loop` ends only through a `break`.
            let broke_moved = self.emit_loop_exit_moves(expr, None, &before, false, None);
            // What a variable holds at every `break`, where the body's
            // end left something else (#1000, #1001).
            self.emit_loop_exit_states(expr, entry, &broke_moved, true);
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
      ExprKind::Match(guard_expr, arms, source) => self.visit_match(expr, guard_expr, arms, source),
      // `break` / `continue` leave the loop's iteration, dropping
      // what it declared (#410); a `break` in a labelled block
      // (no loop) drops nothing drawn here.
      ExprKind::Break(dest, value) => {
        // What the iteration moved on the way to this `break`, before
        // its value (which a `break s` hands to the loop's receiver).
        if let Ok(target) = dest.target_id {
          let value_moves = value.map(|v| locals_moved_by(self.tcx, v)).unwrap_or_default();
          let call = value.is_some_and(|v| matches!(v.kind, ExprKind::Call(..) | ExprKind::MethodCall(..)));
          self.note_break_path_moves(target, expr.hir_id, &value_moves, call);
          self.note_labeled_break_path(target, expr.hir_id, value_moves);
        }
        intravisit::walk_expr(self, expr);
        // `let r = loop { break k; }`: `r` gets `k` (#431).
        if let (Ok(target), Some(value)) = (dest.target_id, value) {
          let has_receiver = self.break_pats.iter().any(|(id, ..)| *id == target)
            || self.break_assigns.iter().any(|(id, _)| *id == target)
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
        // What `let x;` locals have no value where this `break` leaves
        // its loop (#786). After its value is paired: `(a, n) = loop {
        // break (s, 1); }` assigns a and n on this way out (#1396).
        if let Ok(target) = dest.target_id {
          if !self.uninit_locals.is_empty() || !self.maybe_init_locals.is_empty() {
            #[expect(clippy::disallowed_methods, reason = "collected into a set; order is lost")]
            let uninit = self.uninit_locals.keys().copied().collect();
            self.break_uninit.push((target, uninit, self.maybe_init_locals.clone()));
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
          // `continue 'outer` leaves the inner `for` loops the same way,
          // then goes on to the next iteration of `'outer` (#600).
          self.drop_inner_loop_iterators(expr_to_line(expr, &self.tcx), target, false, LoopLeft::Continue);
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

  /// The `ExprKind::Assign` arm of `visit_expr`: `lhs = rhs`.
  /// `r.n = 2` / `r.n += 1` through a reference to a local struct (#854):
  /// the write also lands on the field's own column (`s.n`): an assignment
  /// reads "s.n is assigned a new value through r.n", an update "s.n's
  /// value is modified through r.n".
  /// Nothing when the struct has no column here (a parameter's) or the
  /// write isn't to a field behind a reference. (An overwritten old
  /// value that needs drop is drawn as its drop, on the same column.)
  pub(crate) fn write_field_behind_ref(&mut self, lhs_rty: &ResourceTy, lhs_expr: &'tcx Expr<'tcx>, line_num: usize) {
    let ResourceTy::DerefField(p_rap, place, write, _) = lhs_rty else { return };
    if let Some(field) = self.field_column_behind_ref(lhs_expr, p_rap) {
      self.add_ev(line_num, Evt::Bind, ResourceTy::Index(field, place.clone(), *write), ResourceTy::Anonymous, false);
    }
  }

  /// `*x` (of type `lhs_ty`) with x a reference to part of a variable's
  /// resource: an element (`v.get_mut(0).unwrap()`, `if let Some(x) =
  /// v.get_mut(0)`, `&mut v[0]`, `v.iter_mut()`), a map's value. Returns
  /// that variable, the owner x borrows from (a field column such as
  /// `b.items` too). When that resource is the caller's (x from
  /// `v.get_mut(0)` with `v: &mut Vec<_>` a parameter, #967), it is the
  /// reference parameter v, whose column says "the resource v points to"
  /// as for `v[0] += 1`. Told apart from a reference to the whole of it
  /// (`&mut s`, `&mut *v`) by type: `*x` isn't of its (pointee's) type.
  /// None for a reference-typed `*x` (a borrow stored in an element
  /// isn't drawn on the collection, as for `v[0] = &u`).
  fn part_owner_behind_ref(&self, lhs_rty: &ResourceTy, lhs_expr: &'tcx Expr<'tcx>, lhs_ty: rustc_middle::ty::Ty<'tcx>) -> Option<ResourceAccessPoint> {
    let ResourceTy::Deref(r) = lhs_rty else { return None };
    if lhs_ty.is_ref() {
      return None;
    }
    let owner = self.part_holder(r, 0)?;
    let erase = |ty: rustc_middle::ty::Ty<'tcx>| self.tcx.erase_and_anonymize_regions(ty.peel_refs());
    let tys: Vec<_> = self.column_tys(owner.name(), lhs_expr.hir_id.owner).into_iter().map(erase).collect();
    (!tys.is_empty() && !tys.contains(&erase(lhs_ty))).then_some(owner)
  }

  /// The `&mut` bindings of a `for` loop's tuple pattern and the
  /// variable each points into (as `mut_call_chain_root`), following
  /// the head's adapters: `enumerate` pairs an index with its
  /// receiver's item, `zip` pairs its receiver's item with its
  /// argument's (also as `std::iter::zip(a, b)`; #1082), and `rev` /
  /// `skip` / `take` / ... hand out their receiver's items (#1067).
  fn loop_pat_write_roots(&self, pat: &'tcx Pat<'tcx>, head: &'tcx Expr<'tcx>, out: &mut Vec<(String, Vec<ResourceAccessPoint>)>) {
    let typeck = self.tcx.typeck(pat.hir_id.owner);
    match (pat.kind, head.kind) {
      (PatKind::Binding(BindingMode(rustc_hir::ByRef::No, _), hirid, _, None), _) => {
        if typeck.pat_ty(pat).ref_mutability() == Some(Mutability::Mut) && typeck.pat_adjustments().get(pat.hir_id).is_none() {
          if let Some(roots) = self.loop_item_roots(head) {
            out.push((binding_name(&self.tcx, hirid), roots));
          }
        }
      }
      (PatKind::Tuple(ps, dd), ExprKind::MethodCall(seg, rcvr, args, _)) if ps.len() == 2 && dd.as_opt_usize().is_none()
        && matches!(seg.ident.as_str(), "enumerate" | "zip") =>
      {
        if seg.ident.as_str() == "enumerate" {
          self.loop_pat_write_roots(&ps[1], rcvr, out);
        } else if let [arg] = args {
          self.loop_pat_write_roots(&ps[0], rcvr, out);
          self.loop_pat_write_roots(&ps[1], arg, out);
        }
      }
      (PatKind::Tuple(ps, dd), ExprKind::Call(f, [a, b])) if ps.len() == 2 && dd.as_opt_usize().is_none() && self.is_iter_zip_fn(f) => {
        self.loop_pat_write_roots(&ps[0], a, out);
        self.loop_pat_write_roots(&ps[1], b, out);
      }
      (_, ExprKind::MethodCall(seg, rcvr, _, _)) if SAME_ITEMS.contains(&seg.ident.as_str()) => {
        self.loop_pat_write_roots(pat, rcvr, out);
      }
      _ => {}
    }
  }

  /// `std::iter::zip` (`core::iter::zip`), called as a function.
  fn is_iter_zip_fn(&self, f: &'tcx Expr<'tcx>) -> bool {
    let ExprKind::Path(ref qpath) = f.kind else { return false };
    let Res::Def(DefKind::Fn, def_id) = self.tcx.typeck(f.hir_id.owner).qpath_res(qpath, f.hir_id) else { return false };
    !def_id.is_local() && self.tcx.item_name(def_id).as_str() == "zip"
      && matches!(self.tcx.crate_name(def_id.krate).as_str(), "core" | "std")
  }

  /// The variables what a `for` loop's `&mut` items point into (as
  /// `mut_call_chain_root`, or a `&mut` reference moved into an adapter,
  /// `moved_mut_ref_arg`), following `rev` / `skip` / ... to their
  /// receiver. `chain` hands out its receiver's items and then its
  /// argument's, so an item points into either (`a.iter_mut().chain(b)`;
  /// #1082). None when one of them isn't known: `x` in
  /// `a.iter_mut().chain(once(&mut z))` doesn't always point into a.
  fn loop_item_roots(&self, e: &'tcx Expr<'tcx>) -> Option<Vec<ResourceAccessPoint>> {
    match e.kind {
      ExprKind::MethodCall(seg, rcvr, [arg], _) if seg.ident.as_str() == "chain" => {
        let mut roots = self.loop_item_roots(rcvr)?;
        for r in self.loop_item_roots(arg)? {
          if !roots.contains(&r) {
            roots.push(r);
          }
        }
        Some(roots)
      }
      ExprKind::MethodCall(seg, rcvr, _, _) if SAME_ITEMS.contains(&seg.ident.as_str()) && self.has_chain_call(rcvr) =>
        self.loop_item_roots(rcvr),
      // `.chain(..).map(f)`: f may hand out anything.
      _ if self.has_chain_call(e) => None,
      _ => self.mut_call_chain_root(e).or_else(|| self.moved_mut_ref_arg(e)).map(|r| vec![r]),
    }
  }

  /// Whether the method chain `e` has a `chain` call in it
  /// (`a.iter_mut().chain(b.iter_mut()).rev()`).
  fn has_chain_call(&self, e: &'tcx Expr<'tcx>) -> bool {
    let mut e = e;
    while let ExprKind::MethodCall(seg, rcvr, _, _) = e.kind {
      if seg.ident.as_str() == "chain" {
        return true;
      }
      e = rcvr;
    }
    false
  }

  /// `b` in `a.iter_mut().zip(b)` with `b: &mut Vec<_>`: a `&mut`
  /// reference variable passed as it is to an adapter, which moves it
  /// in (#1076). What the adapter hands out points into the resource b
  /// points to.
  fn moved_mut_ref_arg(&self, e: &'tcx Expr<'tcx>) -> Option<ResourceAccessPoint> {
    let typeck = self.tcx.typeck(e.hir_id.owner);
    // (Also a field holding one, `for x in h.r`, #1129.)
    if !matches!(e.kind, ExprKind::Path(_) | ExprKind::Field(..)) || typeck.expr_ty(e).ref_mutability() != Some(Mutability::Mut)
      || !typeck.expr_adjustments(e).is_empty() {
      return None;
    }
    match get_rap(e, &self.tcx, &self.raps) {
      ResourceTy::Value(r) if r.is_mutref() => Some(r),
      _ => None,
    }
  }

  /// The variable whose resource (or, for a reference parameter, the
  /// caller's resource it points to) the reference r points into: r's
  /// lender's owner, or the reference parameter r borrows through. For
  /// a loop variable with no lender (`for e in v.iter_mut()` with v a
  /// reference, #1046), the same for the head's receiver v, or v itself
  /// when it is an owner (`v.iter_mut().rev()`) or a parameter.
  fn part_holder(&self, r: &ResourceAccessPoint, depth: usize) -> Option<ResourceAccessPoint> {
    let unlent = self.borrow_map.get(r.name()).is_some_and(|d| matches!(d.lender, ResourceTy::Anonymous));
    match self.loop_ref_into.get(r.name()).and_then(|v| v.first()).filter(|_| unlent && depth < 16) {
      Some(h) if !h.is_ref() => Some(h.clone()),
      Some(h) if self.borrow_map.get(h.name()).is_some_and(|d| matches!(d.lender, ResourceTy::Caller)) => Some(h.clone()),
      // `for row in v.iter_mut() { for e in row.iter_mut() { .. } }`.
      Some(h) => self.part_holder(h, depth + 1),
      None => self.owner_behind_ref(r, "").filter(|o| !o.is_fn())
        .or_else(|| self.callers_ref_behind_ref(r, 0)),
    }
  }

  /// Each variable `part_holder` may stand for when r is a loop
  /// variable whose items come from a `chain` (`for x in
  /// a.iter_mut().chain(b.iter_mut())`, also `for e in row.iter_mut()`
  /// nested in such a loop over rows; #1082): x points into a's
  /// resource on some iterations and into b's on the others. Otherwise
  /// `part_holder` alone.
  fn part_holders(&self, r: &ResourceAccessPoint, depth: usize) -> Vec<ResourceAccessPoint> {
    let unlent = self.borrow_map.get(r.name()).is_some_and(|d| matches!(d.lender, ResourceTy::Anonymous));
    match self.loop_ref_into.get(r.name()).filter(|_| unlent && depth < 16) {
      Some(roots) => {
        let mut out: Vec<ResourceAccessPoint> = Vec::new();
        for h in roots {
          let hs = if !h.is_ref() || self.borrow_map.get(h.name()).is_some_and(|d| matches!(d.lender, ResourceTy::Caller)) {
            vec![h.clone()]
          } else {
            self.part_holders(h, depth + 1)
          };
          for h in hs {
            if !out.contains(&h) {
              out.push(h);
            }
          }
        }
        out
      }
      None => self.part_holder(r, depth).into_iter().collect(),
    }
  }

  /// The reference r borrows through whose lender is the caller (a
  /// reference parameter `v: &mut Vec<_>`, or a reference field of a
  /// by-value parameter): x from `v.get_mut(0)`, `&mut v[0]`, or
  /// through a local reborrow of v. None when r's lender chain ends
  /// anywhere else.
  fn callers_ref_behind_ref(&self, r: &ResourceAccessPoint, depth: usize) -> Option<ResourceAccessPoint> {
    if depth > 16 {
      return None;
    }
    match &self.borrow_map.get(r.name())?.lender {
      // (A field of what q points to: `first` from
      // `self.items.first_mut().unwrap()`, #1073.)
      ResourceTy::Value(q) | ResourceTy::Deref(q) | ResourceTy::DerefField(q, _, _, _) if q.is_ref() && !q.is_fn() => {
        match self.borrow_map.get(q.name())?.lender {
          ResourceTy::Caller => Some(q.clone()),
          _ => self.callers_ref_behind_ref(q, depth + 1),
        }
      }
      _ => None,
    }
  }

  /// The types the column `name` may have: of each binding so named, or
  /// for a field column (`b.items`, `t.0`), of that field of each.
  fn column_tys(&self, name: &str, owner: rustc_hir::OwnerId) -> Vec<rustc_middle::ty::Ty<'tcx>> {
    let typeck = self.tcx.typeck(owner);
    let mut path = name.split('.');
    let Some(root) = path.next() else { return Vec::new() };
    let fields: Vec<&str> = path.collect();
    let names = binding_names(&self.tcx, owner);
    #[expect(clippy::disallowed_methods, reason = "collected only to test emptiness and membership")]
    let tys: Vec<_> = names.by_hir.iter()
      .filter(|(_, n)| n.as_str() == root)
      .filter_map(|(h, _)| typeck.node_type_opt(*h))
      .filter_map(|ty| fields.iter().try_fold(ty, |ty, f| self.field_ty_named(ty, f)))
      .collect();
    tys
  }

  /// `*x += 1` / `*x = 7` with x a reference to part of `owner`'s
  /// resource (`part_owner_behind_ref`): the write is drawn on owner's
  /// column too, like an element write `v[0] += 1` (#861): "v's
  /// resource is modified through x", or for `=` a plain assignment to
  /// the place `*x` that says the old value is dropped if it needs drop.
  ///
  /// When x may point into any of several variables' resources (`for x
  /// in a.iter_mut().chain(b.iter_mut())`; #1082), the write is drawn on
  /// each of their columns, and each tooltip says x points into one of
  /// them (`place_with_alternatives`).
  fn write_part_behind_ref(&mut self, owner: ResourceAccessPoint, lhs_rty: &ResourceTy, lhs_expr: &'tcx Expr<'tcx>, line_num: usize, write: PlaceWrite) {
    let written = self.plain_source_text(lhs_expr).unwrap_or_else(|| format!("*{}", lhs_rty.real_name()));
    let place = match write {
      PlaceWrite::Update | PlaceWrite::UpdateValue => written.trim_start_matches(|c: char| c == '*' || c.is_whitespace()).to_owned(),
      _ => written,
    };
    let holders = match lhs_rty {
      ResourceTy::Deref(r) => self.part_holders(r, 0),
      _ => Vec::new(),
    };
    // (Not on a `&mut` parameter a `for` loop's head moved into the
    // loop's iterator: its column ended at the move, and the caller's
    // resource it pointed to has no column, #1365.)
    let moved: Vec<String> = holders.iter().chain(std::iter::once(&owner))
      .filter(|h| h.is_ref() && self.borrow_map.get(h.name())
        .is_some_and(|rd| self.loop_moved_refs.get(h.name()) == Some(&rd.assigned_at)))
      .map(|h| h.name().to_owned()).collect();
    let moved_away = |h: &ResourceAccessPoint| moved.iter().any(|m| m == h.name());
    if holders.len() > 1 && holders.contains(&owner) {
      let ResourceTy::Deref(r) = lhs_rty else { return };
      let place = place_with_alternatives(&place, r.name(), &holders);
      for h in holders.iter().filter(|h| !moved_away(h)).cloned() {
        self.add_ev(line_num, Evt::Bind, ResourceTy::Index(h, place.clone(), write), ResourceTy::Anonymous, false);
      }
      return;
    }
    if moved_away(&owner) {
      return;
    }
    self.add_ev(line_num, Evt::Bind, ResourceTy::Index(owner, place, write), ResourceTy::Anonymous, false);
  }

  /// `*x op= k` with x a reference to part of a variable's resource,
  /// or to all of what a reference parameter points to: see
  /// `write_part_behind_ref`, `whole_param_behind_ref`.
  fn write_part_behind_ref_update(&mut self, lhs_rty: &ResourceTy, lhs_expr: &'tcx Expr<'tcx>, line_num: usize) {
    let lhs_ty = self.tcx.typeck(lhs_expr.hir_id.owner).node_type(lhs_expr.hir_id);
    if let Some(owner) = self.part_owner_behind_ref(lhs_rty, lhs_expr, lhs_ty)
      .or_else(|| self.whole_param_behind_ref(lhs_rty, lhs_expr, lhs_ty)) {
      self.write_part_behind_ref(owner, lhs_rty, lhs_expr, line_num, PlaceWrite::Update);
    }
  }

  /// `*r = ..` / `*r += 1` with r a reborrow of all of what a reference
  /// parameter v points to (`let r = &mut *v;`, #1048): v, whose column
  /// stands for the caller's resource, so the write is drawn there too,
  /// as for `*v = ..` / `*v += 1` written on v. None when `*r` is only
  /// part of it (`part_owner_behind_ref`), or r is v itself.
  fn whole_param_behind_ref(&self, lhs_rty: &ResourceTy, lhs_expr: &'tcx Expr<'tcx>, lhs_ty: rustc_middle::ty::Ty<'tcx>) -> Option<ResourceAccessPoint> {
    let ResourceTy::Deref(r) = lhs_rty else { return None };
    if lhs_ty.is_ref() {
      return None;
    }
    let param = self.callers_ref_behind_ref(r, 0)?;
    let erase = |ty: rustc_middle::ty::Ty<'tcx>| self.tcx.erase_and_anonymize_regions(ty.peel_refs());
    let tys: Vec<_> = self.column_tys(param.name(), lhs_expr.hir_id.owner).into_iter().map(erase).collect();
    tys.contains(&erase(lhs_ty)).then_some(param)
  }

  /// Whether `lhs_expr`, `*r` (or `**r`, …) of type `lhs_ty`, a
  /// reference, is a reference variable with a column: the one on r's
  /// aliasing chain at that depth. The chain names the references r
  /// borrows through, so it is the place `*r` only when that
  /// variable's type is `lhs_ty`. Not so when r points to a reference
  /// with no column: a caller's (`r: &mut &str` a parameter), or one
  /// stored in an element (`&mut words[1]`, `words.get_mut(1)`), or
  /// reached through another reference (`&mut v[1]` with
  /// `v: &mut Vec<&str>`, `&mut *x`), whose chain is empty or names
  /// the collection or `v` / `x` (#897). (A field column such as
  /// `p.0` has no binding of its own; the chain is trusted for it.)
  fn deref_is_ref_var(&self, r: &str, lhs_expr: &'tcx Expr<'tcx>, lhs_ty: rustc_middle::ty::Ty<'tcx>) -> bool {
    let Some(name) = self.borrow_map.get(r)
      .and_then(|d| d.aliasing.get(num_derefs(lhs_expr).saturating_sub(1)))
      .filter(|name| self.borrow_map.contains_key(*name)) else { return false };
    let owner = lhs_expr.hir_id.owner;
    let typeck = self.tcx.typeck(owner);
    let erase = |ty| self.tcx.erase_and_anonymize_regions(ty);
    let names = binding_names(&self.tcx, owner);
    #[expect(clippy::disallowed_methods, reason = "`filter_map` + `any` over the map: order never observed")]
    let mut tys = names.by_hir.iter()
      .filter(|(_, n)| *n == name)
      .filter_map(|(h, _)| typeck.node_type_opt(*h))
      .peekable();
    tys.peek().is_none() || tys.any(|ty| erase(ty) == erase(lhs_ty))
  }

  /// `v = if c { f } else { g };` (or a `match`) where v came from `let
  /// v;` and has no value yet: drawn like `let v = if …` (#1006). v's
  /// value is paired with each branch's before the branches are visited
  /// (`match_rhs_arm`; or after, for a value declared in the branch,
  /// #994), so it lands in that branch: only the branch that ran moved
  /// f (or g), and the join says so. Returns false, having done
  /// nothing, for any other assignment.
  fn init_from_branches(&mut self, expr: &'tcx Expr<'tcx>, lhs_expr: &'tcx Expr<'tcx>, rhs_expr: &'tcx Expr<'tcx>) -> bool {
    let (ExprKind::Assign(..), ExprKind::Path(QPath::Resolved(None, path))) = (expr.kind, lhs_expr.kind) else { return false };
    let Res::Local(hirid) = path.res else { return false };
    let branching = matches!(rhs_expr.kind, ExprKind::If(..) | ExprKind::Match(_, _, rustc_hir::MatchSource::Normal))
      && !rhs_expr.span.from_expansion();
    // `x = Some(if c { a } else { b });` (#1225): the constructor takes
    // nothing itself; x gets each branch's value in its branch.
    let cond = if branching {
      rhs_expr
    } else {
      match self.conditional_value(rhs_expr) {
        Some(c) if c.hir_id != rhs_expr.hir_id => c,
        _ => return false,
      }
    };
    let Some(decl) = self.uninit_locals.remove(&hirid) else {
      return self.reassign_from_branches(hirid, lhs_expr, cond, rhs_expr);
    };
    let prev = self.defer_arm_binds.replace(cond.span);
    self.init_deferred_local(hirid, decl, cond);
    self.defer_arm_binds = prev;
    self.visit_expr(rhs_expr);
    self.bind_left_arms(cond);
    self.mark_branch_results_kept(&binding_name(&self.tcx, hirid), cond);
    // (An immutable `let x;` can't be assigned twice, E0384: a loop
    // that assigns it leaves right after.)
    if matches!(decl.annotation.1, Mutability::Mut) {
      self.drop_earlier_iteration(hirid, lhs_expr);
    }
    true
  }

  /// `q = Some(if c { let r = &a; r } else { let r = &b; r });`, `q = if
  /// c { let r = &a; Some(r) } else { .. };` with q a holder (#1472,
  /// #1463): a branch whose value names a variable the branch declares
  /// is paired with q at the branch's end (`deferred_arm_binds`), as a
  /// `let` pairs it (#994), while that variable is still the branch's
  /// own: after the conditional, `r` names the last branch's. q's old
  /// loan ends, and q takes what the other branches' values borrow
  /// (`reassign_holder`), before the branches are visited; the other
  /// branches are paired after it, as before (`nest_arm_pairings`).
  /// Also for a holder that isn't Copy (`v = if c { let r = &a; vec![r]
  /// } else { let r = &b; vec![r] };`, `Option<&mut T>`, #1477): its old
  /// value's drop is drawn where the assignment starts, as before. And
  /// for a struct drawn with columns for its reference fields (`h = if c
  /// { let r = &a; H(r) } else { .. };`, #1483): its fields' old loans
  /// end where the assignment starts, as for `h = H(&a);`. Some(the
  /// struct field ends left to draw: none) when paired; None, doing
  /// nothing, unless some branch's value is such.
  fn pair_holder_arms_in_branch(&mut self, expr: &'tcx Expr<'tcx>, lhs_expr: &'tcx Expr<'tcx>, rhs_expr: &'tcx Expr<'tcx>) -> Option<Vec<(usize, Vec<ResourceTy>, ResourceTy, bool)>> {
    if !matches!(expr.kind, ExprKind::Assign(..)) || expr.span.from_expansion() {
      return None;
    }
    let cond = self.conditional_value(rhs_expr)?;
    let owner = lhs_expr.hir_id.owner;
    let lhs_ty = self.tcx.typeck(owner).node_type(lhs_expr.hir_id);
    if lhs_ty.is_ref() {
      return None;
    }
    // (A Copy holder is copied into, any other moved into: `Some(r)`
    // with r a `&mut` moves r's reference into q.)
    let evt = if self.ty_is_copy(lhs_ty, owner) { Evt::Copy } else { Evt::Move };
    let Some(ResourceTy::Value(rap)) = self.resource_of_lhs(lhs_expr) else { return None };
    let fields = matches!(rap, ResourceAccessPoint::Struct(Struct { is_member: false, .. }))
      && self.struct_members_deep(rap.name()).iter().any(|m| m.is_ref() || m.holds_borrow());
    if rap.is_ref() || !(rap.holds_borrow() || fields) || rap.is_union() {
      return None;
    }
    let mut bodies = Vec::new();
    conditional_branch_bodies(cond, &mut bodies);
    bodies.retain(|b| self.arm_value_uses_local_in(b, cond.span));
    if bodies.is_empty() {
      return None;
    }
    let line = expr_to_line(lhs_expr, &self.tcx);
    let ends = if rap.holds_borrow() {
      self.reassign_holder(&rap, cond, lhs_ty, line);
      Vec::new()
    } else {
      let ends = self.struct_field_borrow_ends(&rap, line);
      self.reassign_struct_field_lenders(&rap, cond, lhs_ty, line);
      // A field holding a borrow (`o: Option<&mut String>`) has none until
      // a branch gives it one, as for `let w = if ..`: no loan with no
      // lender is left to end ("what it borrows from isn't shown").
      for m in self.struct_members_deep(rap.name()) {
        if !m.is_ref() {
          self.borrow_map.remove(m.name());
        }
      }
      // Drawn now, before the conditional's fork on this row, as a
      // holder's old loan is (`reassign_holder`): a branch's new borrow of
      // an old lender is a loan of its own.
      self.draw_struct_field_borrow_ends(ends, true);
      Vec::new()
    };
    for b in bodies {
      self.deferred_arm_binds.entry(b.hir_id).or_insert_with(|| (b, Vec::new())).1
        .push(ArmBind::Rhs(ResourceTy::Value(rap.clone()), evt.clone()));
      self.arms_paired_in_branch.insert(b.hir_id);
    }
    Some(ends)
  }

  /// `q = if c { let r = &a; Some(r) } else { Some(&b) };` (#1463): an
  /// assignment drawn as such pairs each branch's value with q after the
  /// conditional `cond` was visited (event ids `rhs_ids`) and its events
  /// were put in its `Branch` event, so the pairing's events
  /// (`rhs_arm_ids`) were added on the function's own column, where a
  /// lender the branches split has no line. Each goes into its branch,
  /// as a `let`'s do (`take_branch_events`), and what it touches from
  /// outside the branch is live in it.
  fn nest_arm_pairings(&mut self, cond: &'tcx Expr<'tcx>, rhs_ids: std::ops::Range<usize>) {
    let Some(k) = self.preprocessed_events.iter()
      .rposition(|(_, e)| matches!(e, ExternalEvent::Branch { .. }) && rhs_ids.contains(&e.get_id())) else { return };
    let mut arms: Vec<(Vec<(usize, usize)>, &'tcx Expr<'tcx>)> = Vec::new();
    arm_value_paths(cond, &self.preprocessed_events, k, Vec::new(), &mut arms);
    for (path, body) in arms {
      let Some((lo, hi)) = self.rhs_arm_ids.get(&body.hir_id).copied() else { continue };
      let mut taken: Vec<(usize, ExternalEvent)> = Vec::new();
      self.preprocessed_events.retain(|(l, e)| {
        let mine = (lo..hi).contains(&e.get_id()) && !matches!(e, ExternalEvent::Branch { .. });
        if mine {
          taken.push((*l, e.clone()));
        }
        !mine
      });
      if taken.is_empty() {
        continue;
      }
      let ids: HashSet<usize> = taken.iter().map(|(_, e)| e.get_id()).collect();
      for evs in self.event_line_map.values_mut() {
        evs.retain(|e| !ids.contains(&e.get_id()));
      }
      let mut touched: HashSet<ResourceAccessPoint> = HashSet::new();
      collect_event_live_vars(&taken, &mut touched);
      nest_in_arm(&mut self.preprocessed_events, &path, taken, &touched);
    }
  }

  /// `v = if c { f } else { g };` (or a `match`) where v may hold a
  /// value already (#1010): drawn like `if c { v = f; } else { v = g;
  /// }`. Each branch's value is paired with v once the branch has been
  /// visited (`ArmBind::Reassign`): only the branch that ran moved f (or
  /// g), and v's old value, if the branch left it there, is dropped where
  /// the new one arrives, after the branch's value is computed. Only for
  /// a local owning a value of a type that isn't Copy, a reference, a
  /// union or a `ManuallyDrop`, or drawn with columns for its fields or
  /// as holding a borrow; returns false, having done nothing, for others.
  /// `rhs_expr` is the conditional; `whole`, the right-hand side it is
  /// in (the same, or a constructor around it, #1225).
  fn reassign_from_branches(&mut self, hirid: HirId, lhs_expr: &'tcx Expr<'tcx>, rhs_expr: &'tcx Expr<'tcx>, whole: &'tcx Expr<'tcx>) -> bool {
    if self.maybe_init_locals.contains(&hirid) || self.destructure_srcs.contains_key(&hirid) {
      return false;
    }
    let owner = lhs_expr.hir_id.owner;
    let lhs_ty = self.tcx.typeck(owner).node_type(lhs_expr.hir_id);
    if lhs_ty.is_ref() || lhs_ty.is_union() || self.ty_is_copy(lhs_ty, owner)
      || lhs_ty.ty_adt_def().is_some_and(|d| d.is_manually_drop()) {
      return false;
    }
    let Some(ResourceTy::Value(rap @ ResourceAccessPoint::Owner(_))) = self.resource_of_lhs(lhs_expr) else { return false };
    if rap.holds_borrow() || rap.is_union() || !self.struct_children(rap.name()).is_empty() {
      return false;
    }
    let drops = self.needs_drop(lhs_ty, owner);
    let prev = self.reassign_arms.replace((rap.clone(), drops));
    self.match_rhs(ResourceTy::Value(rap.clone()), rhs_expr, Evt::Move);
    self.reassign_arms = prev;
    self.visit_expr(whole);
    self.bind_left_arms(rhs_expr);
    true
  }

  /// `slice = &mut slice[1..];`, `s = &s[i..];`, `r = &mut *r;` (#1123):
  /// the reference is given a reborrow of (part of) what it already
  /// points to. That new reference is derived from the loan the
  /// reference holds, so the lender stays borrowed until the
  /// reference's last use: no end of the old loan, no new borrow of the
  /// lender, and the reference doesn't lend to itself. Drawn as a note
  /// on its column. False (nothing drawn) unless the reference holds a
  /// loan of a named variable.
  fn reslice_in_place(&mut self, lhs_expr: &'tcx Expr<'tcx>, rhs_expr: &'tcx Expr<'tcx>) -> bool {
    let Some(lhs_hid) = local_of_path(lhs_expr) else { return false };
    let ExprKind::AddrOf(_, _, mut place) = rhs_expr.kind else { return false };
    let mut indices: Vec<&'tcx Expr<'tcx>> = Vec::new();
    // Taken apart through an index (`&s[1..]`), not just `&mut *r`.
    let mut steps = 0;
    loop {
      match place.kind {
        ExprKind::Index(base, idx, _) => { indices.push(idx); place = base; }
        ExprKind::Unary(UnOp::Deref, inner) => place = inner,
        _ => break,
      }
      steps += 1;
    }
    if steps == 0 || local_of_path(place) != Some(lhs_hid) {
      return false;
    }
    self.note_resliced_in_place(lhs_expr, indices)
  }

  /// The drawing of `reslice_in_place` for the reference `lhs_expr`
  /// given a reborrow of what it points to (or of a part, through
  /// `indices`). Also `r = r;` with r a `&mut`, which rustc reborrows
  /// (`r = &mut *r;`, an assignment being a coercion site, #1556).
  fn note_resliced_in_place(&mut self, lhs_expr: &'tcx Expr<'tcx>, indices: Vec<&'tcx Expr<'tcx>>) -> bool {
    if !self.tcx.typeck(lhs_expr.hir_id.owner).node_type(lhs_expr.hir_id).is_ref() {
      return false;
    }
    let Some(name) = expr_to_rap_name(lhs_expr, &self.tcx) else { return false };
    let Some(rap) = self.raps.get(&name).map(|r| r.rap.to_owned()) else { return false };
    let Some(rd) = self.borrow_map.get(&name) else { return false };
    let ResourceTy::Value(lender) = &rd.lender else { return false };
    if !rd.co_lenders.is_empty() {
      return false;
    }
    let (lender, mutable, part) = (lender.name().to_owned(), rd.ref_mutability, !indices.is_empty());
    for idx in indices {
      self.visit_expr(idx);
    }
    let line = expr_to_line(lhs_expr, &self.tcx);
    self.update_rap(&rap, line);
    let id = *self.unique_id;
    *self.unique_id += 1;
    self.add_external_event(line, ExternalEvent::ReslicedInPlace { ro: rap, lender, mutable, part, id });
    true
  }

  /// `r = if c { .. } else { .. };` (or a `match`) where r is a local
  /// reference variable with a column that holds a value already
  /// (#1393): drawn like `let r = if ..`, each branch's value paired
  /// with r in its branch. `Some(keeps_old)`: whether a branch gives r
  /// back its own value (`best = if c { &b } else { best };`, #1430).
  /// On that path r keeps its old loan, so the write ends none: the old
  /// lenders stay borrowed with the new ones until r's last use (rustc
  /// keeps both, as for `if c { best = &b; }`), and that branch draws
  /// nothing.
  fn reassigns_ref_from_branches(&self, expr: &'tcx Expr<'tcx>, lhs_expr: &'tcx Expr<'tcx>, rhs_expr: &'tcx Expr<'tcx>) -> Option<bool> {
    if !matches!(expr.kind, ExprKind::Assign(..)) || expr.span.from_expansion() || rhs_expr.span.from_expansion() {
      return None;
    }
    if !matches!(rhs_expr.kind, ExprKind::If(..) | ExprKind::Match(_, _, rustc_hir::MatchSource::Normal)) {
      return None;
    }
    let hirid = local_of_path(lhs_expr)?;
    if self.uninit_locals.contains_key(&hirid) || self.maybe_init_locals.contains(&hirid) {
      return None;
    }
    if !self.tcx.typeck(lhs_expr.hir_id.owner).node_type(lhs_expr.hir_id).is_ref() {
      return None;
    }
    let keeps_old = branch_tails(rhs_expr)?.iter().any(|t| local_of_path(t) == Some(hirid));
    let name = expr_to_rap_name(lhs_expr, &self.tcx)?;
    (self.raps.get(&name).is_some_and(|r| r.rap.is_ref() && !r.rap.is_static())
      && self.borrow_map.contains_key(&name)).then_some(keeps_old)
  }

  /// After `r = if ..` / `let r = if ..` (r a reference) was visited:
  /// r's lenders, as rustc sees them. A reference a branch declared,
  /// gone now, that r reborrows through (`w` in `m = if c { let w =
  /// &mut s; w } else { &mut t };`, an implicit `&mut *w`, #1431) is
  /// replaced by what it borrowed (s): that stays borrowed until r's
  /// last use, and w's own borrow ends with its scope. r itself is no
  /// lender (`m` in `m = if c { &mut t } else { m }`, reborrowed through
  /// m, #1430); `kept`, r's old lenders, are added.
  pub(crate) fn resolve_branch_lenders(&mut self, r_name: &str, kept: Vec<ResourceTy>) {
    let gone: BTreeSet<String> = self.borrow_map.get(r_name).into_iter()
      .flat_map(|rd| rd.all_lenders().into_iter().chain(kept.iter().cloned()))
      .filter_map(|l| match l {
        ResourceTy::Deref(x) if self.went_out_of_scope(x.name()) => Some(x.name().to_owned()),
        _ => None,
      })
      .collect();
    self.replace_lenders_through(r_name, kept, &gone);
  }

  /// `v = if c { let r = &mut a; vec![r] } else { .. };`, `y = if c {
  /// Some(&mut s1) } else { let r2 = &mut s2; Some(r2) };` (#1482): v's
  /// type is known, so rustc reborrows r (`&mut *r`) where `let v = if
  /// ..` moves it. r goes out of scope at its branch's end, and the loan
  /// v holds from then on, until v's last use, is the one r made, of a:
  /// once branch body `body` was paired with v (`name`, or one of its
  /// parts), v's lenders reached through a reference `body` declares are
  /// what that reference borrows, as `resolve_branch_lenders` does for a
  /// reference left-hand side (#1431). r's own borrow ends with its scope.
  pub(crate) fn resolve_arm_local_lenders(&mut self, name: &str, body: &'tcx Expr<'tcx>) {
    struct Decls<'tcx> { tcx: rustc_middle::ty::TyCtxt<'tcx>, names: BTreeSet<String> }
    impl<'tcx> Visitor<'tcx> for Decls<'tcx> {
      fn visit_pat(&mut self, p: &'tcx Pat<'tcx>) {
        if let PatKind::Binding(..) = p.kind {
          self.names.insert(binding_name(&self.tcx, p.hir_id));
        }
        intravisit::walk_pat(self, p);
      }
    }
    let mut d = Decls { tcx: self.tcx, names: BTreeSet::new() };
    d.visit_expr(body);
    self.replace_lenders_through(name, Vec::new(), &d.names);
  }

  /// `v = { let r = &mut a; vec![r] };`, `x = { let r = &mut a; r };`,
  /// `let v: Vec<&mut String> = { .. };` (#1486): the block's value
  /// reborrows (`&mut *r`) a reference the block declares, which goes out
  /// of scope at the block's end. From then on the loan `name` (or a part
  /// of it) holds is the one r made, of a, until name's last use, as for
  /// a branch's value (`resolve_arm_local_lenders`, #1482); r's own
  /// borrow ends with its scope.
  pub(crate) fn resolve_block_local_lenders(&mut self, name: &str, rhs: &'tcx Expr<'tcx>) {
    let mut e = rhs;
    while let ExprKind::DropTemps(inner) = e.kind {
      e = inner;
    }
    if e.span.from_expansion() || !matches!(e.kind, ExprKind::Block(b, _) if !b.stmts.is_empty()) {
      return;
    }
    let prefix = format!("{}.", name);
    #[expect(clippy::disallowed_methods, reason = "sorted on the next line")]
    let mut names: Vec<String> = self.raps.iter()
      .filter(|(n, rd)| (*n == name || n.starts_with(&prefix)) && (rd.rap.is_ref() || rd.rap.holds_borrow()))
      .map(|(n, _)| n.clone())
      .collect();
    names.sort();
    // (Made where the block's value is: the loan the borrow checker
    // keeps for name is issued there, not on the statement's first line.)
    let at = branch_value_expr(e).map(|v| expr_to_line(v, &self.tcx));
    for n in &names {
      let before = self.borrow_map.get(n).map(|d| d.all_lenders());
      self.resolve_arm_local_lenders(n, e);
      if let (Some(at), Some(rd)) = (at, self.borrow_map.get_mut(n)) {
        if before.is_some_and(|b| b != rd.all_lenders()) && rd.assigned_at < at {
          rd.assigned_at = at;
          rd.lifetime = rd.lifetime.max(at);
        }
      }
    }
  }

  /// `r_name`'s lenders (and `kept`), each `Deref(x)` of a reference x
  /// in `gone` replaced by what x borrows.
  fn replace_lenders_through(&mut self, r_name: &str, kept: Vec<ResourceTy>, gone: &BTreeSet<String>) {
    let Some(rd) = self.borrow_map.get(r_name) else { return };
    let before = rd.all_lenders();
    let mut all: Vec<ResourceTy> = Vec::new();
    for l in before.iter().cloned().chain(kept) {
      let through = match &l {
        ResourceTy::Deref(x) if x.is_ref() && x.name() != r_name && gone.contains(x.name()) =>
          self.borrow_map.get(x.name()).map(|d| d.all_lenders()),
        _ => None,
      };
      for m in through.unwrap_or_else(|| vec![l]) {
        if matches!(m, ResourceTy::Value(_) | ResourceTy::Deref(_))
          && m.extract_rap().is_none_or(|x| x.name() != r_name) && !all.contains(&m) {
          all.push(m);
        }
      }
    }
    if all.is_empty() || all == before {
      return;
    }
    let aliasing = get_aliasing_data(&all[0], &self.borrow_map);
    if let Some(rd) = self.borrow_map.get_mut(r_name) {
      rd.lender = all.remove(0);
      rd.co_lenders = all;
      rd.aliasing = aliasing;
    }
  }

  /// Whether `e` is inside the condition of an `if` (a let-chain part
  /// included) or a match arm's guard.
  fn in_condition(&self, e: &'tcx Expr<'tcx>) -> bool {
    self.tcx.hir_parent_iter(e.hir_id).any(|(_, node)| match node {
      rustc_hir::Node::Expr(p) => matches!(p.kind, ExprKind::If(cond, ..) if cond.span.contains(e.span)),
      rustc_hir::Node::Arm(arm) => arm.guard.is_some_and(|g| g.span.contains(e.span)),
      _ => false,
    })
  }

  fn visit_assign(&mut self, expr: &'tcx Expr<'tcx>, lhs_expr: &'tcx Expr<'tcx>, rhs_expr: &'tcx Expr<'tcx>) {
    if self.unfinished_vec(rhs_expr) {
      self.visit_expr(rhs_expr);
      return;
    }
    // `a = lhs` inside a destructuring assignment: draw it as
    // `a = <the part of the RHS lhs holds>`. That part was
    // already visited with the whole RHS.
    let destructure_hid = local_of_path(rhs_expr)
      .filter(|h| self.destructure_srcs.contains_key(h));
    let rhs_expr: &'tcx Expr<'tcx> = match destructure_hid {
      Some(h) => self.destructure_srcs[&h],
      None => rhs_expr,
    };
    // `v = v;` / `p.f = p.f;` (rustc warns: useless assignment, #1021):
    // the value is moved out of the place and straight back in. Nothing
    // is dropped and the place owns the same value after, so nothing is
    // drawn, as for a branch giving v its own value back in `v = if c
    // { v } else { g };` (#1010). So is `r = r;` with r a shared
    // reference: r holds the same reference, with the same loan, after
    // (#1497); it used to end r's borrow there and draw a copy arrow from
    // r to itself. (A Copy value is drawn as the copy it is.) With r a
    // `&mut`, rustc reborrows (`r = &mut *r;`): r still points to what it
    // did, with a loan derived from its old one, so its lender stays
    // borrowed until r's last use. Drawn as `r = &mut *r;` is (a note on
    // r's column), or as nothing for a place with no column of its own
    // to note it on (`p.r = p.r;`); it used to draw r lending to itself
    // (#1556).
    if destructure_hid.is_none() && matches!(expr.kind, ExprKind::Assign(..))
      && !expr.span.from_expansion() && same_place(lhs_expr, rhs_expr) {
      let owner = lhs_expr.hir_id.owner;
      let ty = self.tcx.typeck(owner).node_type(lhs_expr.hir_id);
      if ty.ref_mutability() == Some(Mutability::Mut) {
        if local_of_path(lhs_expr).is_none() || !self.note_resliced_in_place(lhs_expr, Vec::new()) {
          self.visit_expr(lhs_expr);
        }
        return;
      }
      if (!ty.is_ref() && !self.ty_is_copy(ty, owner)) || ty.ref_mutability() == Some(Mutability::Not) {
        self.visit_expr(lhs_expr);
        return;
      }
    }
    self.visit_expr(lhs_expr);
    // `s = return;`, `s = panic!()`, `s = break;` (#1142): the right-hand
    // side runs first and never finishes, so the assignment never
    // happens: no new value, and no drop of the old one. What the exit
    // itself drops is drawn by the exit.
    if destructure_hid.is_none() && matches!(expr.kind, ExprKind::Assign(..)) && self.expr_diverges(rhs_expr) {
      self.visit_expr(rhs_expr);
      return;
    }
    if destructure_hid.is_none() && self.init_from_branches(expr, lhs_expr, rhs_expr) {
      return;
    }
    if destructure_hid.is_none() && matches!(expr.kind, ExprKind::Assign(..)) && self.reslice_in_place(lhs_expr, rhs_expr) {
      return;
    }
    // `r = if c { let y = &a; y } else { &b };` with r a reference that
    // holds one already (#1393): each branch's value is paired with r in
    // that branch, before its variables go out of scope, as for `let r =
    // if ..` (#1327). The old loan is ended first (below), then the
    // conditional is visited (`ref_arms_assign`).
    let ref_arms_keep = if destructure_hid.is_none() { self.reassigns_ref_from_branches(expr, lhs_expr, rhs_expr) } else { None };
    let ref_arms = ref_arms_keep.is_some();
    // A branch gives r back its own value (#1430): r's old loan lives on
    // along that path, so the write ends none of it.
    let keeps_old_loan = ref_arms_keep == Some(true);
    let branch_field_ends = if destructure_hid.is_none() { self.pair_holder_arms_in_branch(expr, lhs_expr, rhs_expr) } else { None };
    let paired_in_branch = branch_field_ends.is_some();
    let rhs_lo = *self.unique_id;
    if destructure_hid.is_none() && !ref_arms {
      self.visit_expr(rhs_expr);
    }
    let rhs_ids = rhs_lo..*self.unique_id;

    // `x = rhs` where x came from `let x;` and is still
    // uninitialized here: this initializes x (#234). If only some
    // paths to here initialized x (#244), it is a reassignment that
    // drops x's old value only if there is one, and x has a value
    // from here on.
    let mut maybe_init = None;
    if let (ExprKind::Assign(..), ExprKind::Path(QPath::Resolved(None, path))) = (expr.kind, lhs_expr.kind) {
      if let Res::Local(hirid) = path.res {
        if let Some(decl) = self.uninit_locals.remove(&hirid) {
          self.init_deferred_local(hirid, decl, rhs_expr);
          // (An immutable `let x;` can't be assigned twice, E0384:
          // a loop that assigns it leaves right after.)
          if matches!(decl.annotation.1, Mutability::Mut) {
            self.drop_earlier_iteration(hirid, lhs_expr);
          }
          return;
        }
        if self.maybe_init_locals.remove(&hirid) {
          maybe_init = Some(if self.maybe_init_by_loop.remove(&hirid) { InitBy::Loop } else { InitBy::Branch });
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
    let is_copyable = self.ty_is_copy(lhs_ty, lhs_expr.hir_id.owner);
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
    // `rh.r = &t` with `let rh = &mut hold;` writes the reference field
    // `hold.r`, which has a column: drawn as `hold.r = &t;` is (#531),
    // with t's borrow going into hold.r and the old lender kept
    // borrowed while hold is in use (#1361).
    let lhs_rty = match self.resource_of_lhs(lhs_expr).or_else(|| self.ref_field_through_local_ref(lhs_expr)) {
      Some(rty) => rty,
      None => {
        if let ExprKind::AddrOf(..) = rhs_expr.kind {
          // `v[0] = &u` / `self.r = &x`: a borrow stored in a
          // place with no column (#282).
          self.store_borrow_in_unmodelled_place(lhs_expr, rhs_expr, e);
        } else {
          let n_events = self.preprocessed_events.len();
          self.match_rhs(ResourceTy::Anonymous, rhs_expr, e);
          // `refs[0] = tr;`: refs now holds tr's borrow (#1384).
          if matches!(expr.kind, ExprKind::Assign(..)) {
            self.store_ref_copy_into_holder(lhs_expr, rhs_expr, n_events);
          }
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
    // `*r = t` with `r: &mut &T` re-points the reference variable r
    // borrows (`let r = &mut s; *r = "c";`). Otherwise there is no
    // variable to re-point, and the write replaces the reference
    // stored in a place, like `*r = 5` through `r: &mut i32` (#897).
    let repoints_ref_var = match &lhs_rty {
      ResourceTy::Deref(_) if lhs_ty.is_ref() => self.deref_is_ref_var(&lhs_rty.real_name(), lhs_expr, lhs_ty),
      // A reference-typed `static mut` (`NAME = "b"` with
      // `NAME: &str`) has a column but no loan to end or re-point:
      // it can only hold a `&'static`, so it is drawn like a Copy
      // value (#690), and the write is a plain assignment (#1162).
      ResourceTy::Value(rap) if rap.is_static() => false,
      _ => true,
    };
    let assign_kind = || if is_copyable {
      PlaceWrite::AssignCopy
    } else if self.needs_drop(lhs_ty, lhs_expr.hir_id.owner) {
      PlaceWrite::AssignDropsOld
    } else {
      PlaceWrite::AssignOwned
    };
    // `*b = v` on a Box `b` whose contents a `let w = *b;` moved out
    // before (#928): on every path, nothing of the old contents is
    // left to drop; on some paths only, a drop flag decides.
    let box_contents_write = |rap: &ResourceAccessPoint, write: PlaceWrite| {
      let box_deref = !rap.is_ref() && matches!(lhs_expr.kind, ExprKind::Unary(UnOp::Deref, inner)
        if self.tcx.typeck(inner.hir_id.owner).node_type(inner.hir_id).is_box());
      match write {
        PlaceWrite::AssignDropsOld if box_deref => match self.rap_ownership_now(rap.name()) {
          Ownership::None => PlaceWrite::AssignOverMoved,
          Ownership::Maybe => PlaceWrite::AssignMaybeMoved {
            by_cond: self.maybe_moved_by_let_now(rap.name()) || self.moved_by_chain_condition(rap),
          },
          Ownership::Held => write,
        },
        _ => write,
      }
    };
    let lhs_rty = match lhs_rty {
      ResourceTy::Deref(rap) if is_assign && (!lhs_ty.is_ref() || !repoints_ref_var) => {
        let write = box_contents_write(&rap, assign_kind());
        ResourceTy::Deref(rap.with_place_write(write))
      }
      // A reference-typed field (`h.r = &x`) is not an owned or Copy
      // value; leave its wording alone.
      ResourceTy::DerefField(rap, place, _, holds) if is_assign && !lhs_ty.is_ref() =>
        ResourceTy::DerefField(rap, place, assign_kind(), holds),
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
    // `*b = (..)` on a Box drawn as its payload's part columns (`b.0`,
    // `b.1`): the new payload's parts go into those columns, and the
    // old payload is dropped, as for `t = (..)` on a local tuple
    // (what of it a move left, #628). A part moved out earlier holds
    // a value again, dropped at b's scope end (#893). The Box keeps
    // its heap allocation: the drop names `*b` (`payload`).
    let box_payload = self.box_payload_write(lhs_expr)
      .filter(|b| is_assign && self.rap_holds_resource_now(b.name()));
    let lhs_rty = match &box_payload {
      Some(b) => ResourceTy::Value(b.clone()),
      None => lhs_rty,
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
    // The reference variable a `*r = <reference>` write re-points
    // (x in `let r = &mut x; *r = &t;`, #904).
    let mut repointed: Option<ResourceAccessPoint> = None;
    if lhs_ty.is_ref() && !lhs_is_union_field && lhs_rty.extract_rap().is_some() && repoints_ref_var {
      let lhs_rap = self.raps.get(&lhs_rty.real_name()).unwrap().rap.clone();
      match lhs_rty {
        // ex:
        // let mut a = &b (where b is &&i32)
        // a = &c (where c is &&i32)
        ResourceTy::Value(_) => {
          let ref_data = self.borrow_map.get(lhs_rap.name()).unwrap().clone();
          // `r = r;`, or `a = b;` with `b` a copy of a's reference (`let
          // b = a;`): r gets the reference it held back, with the same
          // loan, so the write ends none of it (#1497, #1517). It used to
          // end the borrow at the write, and once more at r's last use.
          let same_value = destructure_hid.is_none() && !ref_arms && matches!(rhs_expr.kind, ExprKind::Path(..))
            && expr_to_rap_name(rhs_expr, &self.tcx).is_some_and(|n| n == *lhs_rap.name()
              || (self.borrow_map.get(&n).is_some_and(|d| d.lender == ref_data.lender && d.co_lenders == ref_data.co_lenders)
                && ((ref_data.loan().is_some()
                  && (self.ref_carriers.iter().any(|(c, src, l)| *c == n && src == lhs_rap.name() && Some(*l) == ref_data.loan())
                    || self.borrow_map.get(&n).is_some_and(|d| d.loan() == ref_data.loan())))
                  // (Or given its value by a copy of r since: `let Holder { r,
                  // .. } = h;` before `h.r = r;`.)
                  || self.borrow_map.get(&n).is_some_and(|d| d.assigned_at >= ref_data.assigned_at
                    && self.preprocessed_events.iter().any(|(l, e)| *l == d.assigned_at && matches!(e,
                      ExternalEvent::Copy { from: ResourceTy::Value(f), to: ResourceTy::Value(t), .. }
                        if f.name() == lhs_rap.name() && *t.name() == n))))));
          let keeps_old_loan = keeps_old_loan || same_value;
          self.hand_loan_to_carriers(lhs_rap.name(), &ref_data);
          self.hand_loan_to_reborrows(lhs_rap.name(), &ref_data, line_num);
          let to_ro = match ref_data.lender {
            ResourceTy::Anonymous => ResourceTy::Deref(lhs_rap.clone()),
            _ => ref_data.lender.clone()
          };

          // add event
          // (None if the old reference was moved on, `let r2 = r;`
          // before `r = &mut t;`: r had no borrow left to end, #209.)
          let mut borrowers = get_borrowers(&lhs_rty.real_name(), &self.borrow_map);
          // A reference already out of scope (`y` in `r = if c { let y =
          // &a; y } else { &b };`) holds nothing: what of its loan lives
          // on is the copy r took, so r's old loan is the one to end
          // here, and y's ends with its scope (#1335).
          #[expect(clippy::disallowed_methods, reason = "filtered into a sorted Vec")]
          let mut gone: Vec<String> = borrowers.iter()
            .filter(|b| **b != lhs_rty.real_name() && self.went_out_of_scope(b))
            .cloned().collect();
          gone.sort();
          for g in &gone {
            borrowers.remove(g);
          }
          // Only the assignment is the write, not the rest of its line
          // (a one-line `if c { r = &y; }`'s condition, #1188); unless
          // it is part of a condition itself (a let-chain's `{ r = &s;
          // .. }`, #603), whose other paths are drawn as the chain's.
          let write_span = (!self.in_condition(expr)).then_some(expr.span);
          // Another reference to the lender whose loan is over on the
          // paths to this write by the time r's old loan ends (`x2` in
          // `let x2 = &s; x2.len(); r.len(); r = &t;`, or read only in
          // another arm) doesn't keep the lender borrowed after it: r's
          // old loan gives the lender back (#1439). It used to count as
          // still borrowing, so r's end gave nothing back and, when it
          // ended in another arm, wasn't drawn at all.
          // Nor one whose loan was made after r's old loan ended (`o` in
          // `println!("{}", r); let o = Some(&a); r = match o { .. };`,
          // #1432), or had ended before it: it doesn't keep the lender
          // borrowed at that end.
          let me = lhs_rty.real_name();
          // A copied reference (`let mut r = x;`), whose loan rustc
          // doesn't record for r, dead before the conditional whose arm
          // re-points it (r never read again): its old loan ends there
          // on every path, at its last use (#1524).
          let dead_before_arms = (ref_data.loan().is_none() && !matches!(ref_data.lender, ResourceTy::Anonymous))
            .then(|| self.last_live_line(&me, (ref_data.assigned_at, line_num.saturating_sub(1))).unwrap_or(ref_data.assigned_at))
            .filter(|&l| l >= ref_data.assigned_at && self.arm_frames.last().is_some_and(|&(split, _, _, _)| l < split));
          // A let-else binding drawn as a branch is bound on the join row
          // (`borrow_floor`, #1408): rustc ends an unused binding's loan
          // on the `let` row, above it, but its column only starts at the
          // join, so the loan is given back there (#1579).
          let floor = self.borrow_floor.get(&me).copied();
          let lift = |e: usize| floor.map_or(e, |f| e.max(f).min(line_num));
          let old_end = self.loan_end_of(&me, &ref_data).filter(|&end| end < line_num && end >= ref_data.assigned_at)
            .or_else(|| self.old_loan_end(&me, &ref_data, &ref_data.lender, line_num, write_span))
            .map(lift);
          if let Some(old_end) = old_end {
            #[expect(clippy::disallowed_methods, reason = "filtered into a sorted Vec")]
            let mut over: Vec<String> = borrowers.iter()
              .filter(|b| **b != me)
              .filter(|b| self.borrow_map.get(*b).is_some_and(|d| d.lender == ref_data.lender
                && self.old_loan_end(b, d, &d.lender, line_num, write_span).is_some_and(|e| e <= old_end)))
              .cloned().collect();
            over.sort();
            for o in &over {
              borrowers.remove(o);
            }
            borrowers.retain(|b| *b == me || self.borrow_map.get(b).is_none_or(|rd| rd.assigned_at <= old_end
              && self.loan_end(rd).unwrap_or(rd.lifetime).max(rd.lifetime) >= old_end));
          }
          // In a destructuring assignment, another destination may
          // take this borrow over in the same statement: in
          // `(r1, r2) = (r2, r1)` r1 stops borrowing x but r2 now
          // does, so x is still borrowed (#291). The parts not yet
          // drawn still have their new lender precomputed.
          #[expect(clippy::disallowed_methods, reason = "`any` over the map: order never observed")]
          let taken_over = matches!(ref_data.lender, ResourceTy::Value(_))
            && destructure_hid.is_some_and(|me| self.destructure_ref_data.iter()
              .any(|(h, (lender, _, _, _))| *h != me && *lender == ref_data.lender));
          // A mutable loan is the only live loan of its lender: a shared
          // one made before it (`vr` from `vec![s2.as_str()]`, used before
          // `r = downgrade(&mut s2)`) is over. Its end gives the lender
          // back (#1414).
          #[expect(clippy::disallowed_methods, reason = "`all` over the set: order never observed")]
          let only_loan = ref_data.co_lenders.is_empty()
            && crate::expr_visitor::mutable_loan_alone(lhs_rap.name(), &ref_data.lender.real_name(), &self.borrow_map, borrowers.iter());
          // (Also when another reference keeps the lender borrowed: y
          // is gone all the same, #1432.)
          // (Only one whose borrow r's old value could be a copy of: one
          // still in use when r got it. An arm-local `let m2 = &mut s;`
          // used up before `let mut r3 = &s;` gives s back itself, #1515.)
          // Reads of the old lenders before the old loan ends (`println!("{}",
          // b);` between `let mut r = &b;` and `r = &t;`) leave them shared
          // by r (#1492).
          if !self.ref_handed_off(lhs_rap.name()) && !keeps_old_loan && !taken_over {
            let lenders: Vec<String> = ref_data.all_lenders().iter()
              .filter(|l| matches!(l, ResourceTy::Value(r) if !r.is_ref()) && !ref_data.mut_of(l))
              .map(|l| l.real_name())
              .collect();
            let to = match self.loan_end(&ref_data) {
              Some(e) if e >= ref_data.assigned_at => e,
              _ => old_end.unwrap_or(line_num),
            };
            if !lenders.is_empty() {
              self.repointed_loans.push((me.clone(), lenders, ref_data.assigned_at, to));
            }
          }
          let ends_lo = *self.unique_id;
          if self.ref_handed_off(lhs_rap.name()) || keeps_old_loan {
          }
          else if (borrowers.len() > 1 && !only_loan) || taken_over { // there is another active reference at this point, the resource cannot be returned
            // The shared loan lives on in another arm (`r = &t;` in one
            // arm, a read of r in another, x2 read after the `if`): as
            // for the end that gives the lender back below (#806), it
            // ends at that read, in that arm, and
            // `end_outer_borrows_in_arms` ends it in this one too, each
            // saying the lender stays borrowed by the others. A RefDie at
            // the write alone left the other arm's leg of r holding a
            // reference down to the join, which had no dot (#1439).
            let sibling_end = self.loan_end(&ref_data)
              .filter(|&e| e > line_num && self.in_sibling_arm(e) && !taken_over && !ref_data.ref_mutability);
            if let Some(end) = sibling_end {
              #[expect(clippy::disallowed_methods, reason = "sorted below")]
              let mut others: Vec<String> = borrowers.iter().filter(|b| **b != me).cloned().collect();
              others.sort();
              self.defer_old_ends = true;
              self.add_borrow_end_ev(end, to_ro, lhs_rty.clone(), &ref_data, others);
              self.defer_old_ends = false;
            } else {
              // (Where the old loan ends, its last use, when that is
              // before the write, #1432.)
              let die_line = if taken_over { line_num } else { old_end.or(dead_before_arms).unwrap_or(line_num) };
              self.add_external_event(die_line, ExternalEvent::RefDie { from: lhs_rty.clone(), to: to_ro, num_curr_borrowers: borrowers.len() - 1, id: *self.unique_id });
              *self.unique_id += 1;
              // r's other lenders (`b` in `r = if c { &a } else { &b };`
              // with a also borrowed by z) are given back where the old
              // loan ends, unless another reference still borrows them
              // (#1432).
              if !taken_over {
                let r_name = lhs_rty.real_name();
                for co in ref_data.co_lenders.iter().filter(|c| matches!(c, ResourceTy::Value(_))) {
                  #[expect(clippy::disallowed_methods, reason = "`any` over the map: order never observed")]
                  let shared = self.borrow_map.iter().any(|(k, rd)| *k != r_name && !gone.contains(k)
                    && rd.all_lenders().contains(co) && old_end.is_none_or(|e| rd.assigned_at <= e
                      && self.loan_end(rd).unwrap_or(rd.lifetime).max(rd.lifetime) >= e));
                  if shared {
                    continue;
                  }
                  let evt = if ref_data.mut_of(co) { Evt::MDie } else { Evt::SDie };
                  self.add_ev(die_line, evt, co.clone(), lhs_rty.clone(), false);
                }
              }
            }
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
            // `o = &q;` ends the old loan where the borrow checker
            // does: `o` is dead until the write, so that can be
            // earlier than the write itself (#745). (A loan
            // `*w = &c` made through a reference ends the same
            // way.)
            //
            // The loan's end on the paths that reach the write, if
            // it lives on another one (`if c { x += 1; r = &y; }`
            // with `r` read after the `if`), and for each lender
            // of a reference that borrows from either of two
            // (#770). (`write_span`: see above.)
            let end_of = |this: &Self, lender: &ResourceTy| lift(this.old_loan_end(&me, &ref_data, lender, line_num, write_span).unwrap_or(line_num));
            let end_line = lift(match self.loan_end_of(&me, &ref_data).filter(|&end| end < line_num && end >= ref_data.assigned_at) {
              Some(end) => end,
              None => match end_of(self, &ref_data.lender) {
                e if e == line_num => dead_before_arms.unwrap_or(e),
                e => e,
              },
            });
            // The loan lives on in another arm of an enclosing `if` /
            // `match` (`c = &y;` in one arm, a read of `c` in a later
            // one): on this path it ends at the write, on the other at
            // that read, and the borrow checker ends it at each (#806).
            // The end past the write is drawn at the read, in that arm,
            // and ends the loan in this one too, where it is last used.
            let end_line = match self.loan_end(&ref_data).filter(|&e| e > line_num && self.in_sibling_arm(e)) {
              Some(e) if end_line == line_num => e,
              _ => end_line,
            };
            // An end past the write (#778) is added once the whole
            // body is visited, at the top level (`late_old_ends`).
            self.defer_old_ends = end_line > line_num;
            self.add_borrow_end_ev(end_line, to_ro.clone(), lhs_rty.clone(), &ref_data, Vec::new());
            self.defer_old_ends = false;
            // ...and to each further lender (#171), unless another
            // reference still borrows it there (`x = &b` with `r = if c {
            // &a } else { &b };`, #1493), as in the branch above (#1432):
            // then r's borrow just ends (on a line of its own, where the
            // first lender's end isn't).
            for co in ref_data.co_lenders.iter() {
              let evt = if ref_data.mut_of(co) { Evt::MDie } else { Evt::SDie };
              let co_end = end_of(self, co);
              #[expect(clippy::disallowed_methods, reason = "`any` over the map: order never observed")]
              let shared = matches!(co, ResourceTy::Value(_)) && !ref_data.mut_of(co) && self.borrow_map.iter().any(|(k, rd)| *k != me
                && !gone.contains(k) && rd.all_lenders().contains(co) && rd.assigned_at <= co_end
                && self.loan_end(rd).unwrap_or(rd.lifetime).max(rd.lifetime) >= co_end);
              if shared {
                if co_end != end_line {
                  self.defer_old_ends = co_end > line_num;
                  self.add_external_event(co_end, ExternalEvent::RefDie { from: lhs_rty.clone(), to: co.clone(), num_curr_borrowers: 1, id: *self.unique_id });
                  *self.unique_id += 1;
                  self.defer_old_ends = false;
                }
                continue;
              }
              self.defer_old_ends = co_end > line_num;
              self.add_ev(co_end, evt, co.clone(), lhs_rty.clone(), false);
              self.defer_old_ends = false;
            }
          }

          // Use the defining stores of the value this assignment replaces.
          // Its old copy can already be dead here, even though a different
          // holder keeps the loan alive in a sibling arm.
          let mut old = self.body_facts.into_iter().flat_map(|f| f.rows.iter().flat_map(|r| &r.facts)
            .filter_map(|ev| {
              let crate::facts::Fact::Repoint { holder, reaching, .. } = &ev.fact else { return None };
              let span = self.bwf.body.source_info(ev.location).span.with_parent(None);
              (holder == &me && (span == lhs_expr.span.with_parent(None) || span == expr.span.with_parent(None)))
                .then_some(reaching.as_ref()).flatten()
            })).flatten().copied().collect::<std::collections::BTreeSet<_>>();
          if old.is_empty() {
            if let Some(l) = ref_data.borrow_span.and_then(|s| self.loan_at_span(s)) { old.insert(l.loan); }
          }
          if write_span.is_some() && !old.is_empty() {
            for id in ends_lo..*self.unique_id { self.end_loans.insert(id, old.iter().copied().collect()); }
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
            // (With `ref_arms`, the branches' own variables aren't
            // declared yet: they're added as each branch is paired,
            // `add_arm_lenders`.)
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
          // (#1430) The old lenders, still borrowed on the path that
          // keeps r's old value.
          if keeps_old_loan {
            let mut all: Vec<ResourceTy> = Vec::new();
            for l in r.all_lenders().into_iter().chain(ref_data.all_lenders()) {
              if matches!(l, ResourceTy::Value(_) | ResourceTy::Deref(_))
                && l.extract_rap().is_none_or(|x| x.name() != lhs_rap.name()) && !all.contains(&l) {
                all.push(l);
              }
            }
            if !all.is_empty() {
              r.lender = all.remove(0);
              r.co_lenders = all;
            }
          }
          self.note_loan_kinds(lhs_rap.name(), rhs_expr);
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
          repointed = self.raps.get(&modified_ref_name).map(|r| r.rap.clone());
          // The old loan's end, if drawn at the write, is the
          // variable's, like `o = &q`'s (#904).
          let old_end_from = repointed.clone().map_or_else(|| lhs_rty.clone(), ResourceTy::Value);
          // References that copied the old value of the modified
          // reference (`let h = o;` before `w = &mut o; *w = &q;`)
          // keep its loan, as for `o = &q` (#664).
          self.hand_loan_to_carriers(&modified_ref_name, &modified_ref_data);

          // add event
          // (Other references with the same lender, a carrier among
          // them, keep the lender borrowed.)
          let mut borrowers = get_borrowers(&modified_ref_name, &self.borrow_map);
          // `o` stays live across the write (w borrows it), so
          // unlike `o = &q`, which ends the old loan because `o` is
          // dead until then, the borrow checker keeps the old loan
          // live while `o` is: until the loan's end, past the write
          // (#664). `o` no longer points to the lender, so like a
          // field write (#531) the end is drawn on the lender's
          // column only.
          let old_end = self.loan_end(&modified_ref_data).filter(|&end| end > line_num)
            .zip(self.raps.get(&modified_ref_name).map(|r| r.rap.clone()));
          // As for `o = &q`: another reference whose loan is over before
          // the old one's ends doesn't keep the lender borrowed (#1439).
          let o_end = old_end.as_ref().map_or(line_num, |(e, _)| *e);
          #[expect(clippy::disallowed_methods, reason = "filtered into a sorted Vec")]
          let mut over: Vec<String> = borrowers.iter()
            .filter(|b| **b != modified_ref_name)
            .filter(|b| self.borrow_map.get(*b).is_some_and(|d| d.lender == modified_ref_data.lender
              && self.old_loan_end(b, d, &d.lender, line_num, None).is_some_and(|e| e <= o_end)))
            .cloned().collect();
          over.sort();
          for o in &over {
            borrowers.remove(o);
          }
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
            self.add_borrow_end_ev(line_num, to_ro, old_end_from, &modified_ref_data, Vec::new());
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
          self.note_loan_kinds(&modified_ref_name, rhs_expr);
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
      && !self.needs_drop(lhs_ty, lhs_expr.hir_id.owner);
    // Nor does overwriting a `ManuallyDrop<T>` drop the old `T` (#283).
    let lhs_is_manually_drop = lhs_ty.ty_adt_def().map_or(false, |d| d.is_manually_drop());
    // `*x = …` with x a reference to part of a variable's resource
    // (`v.get_mut(0)`, `&mut v[0]`): only that part is overwritten, and
    // the variable isn't given a new resource (#861).
    // (Not `**rb = v` through a reference to a Box: that writes the
    // Box's contents, drawn as `*bx` below, #941.)
    let part_owner = if self.box_deref_through_ref(lhs_expr).is_some() {
      None
    } else {
      self.part_owner_behind_ref(&lhs_rty, lhs_expr, lhs_ty)
    };
    let mut payload_behind_ref = false;
    // `t = (t.1, t.0);`: the right-hand side is evaluated first and
    // moves t's fields out, so the old value they held isn't dropped
    // by the assignment (#1138). Seen by `reassign_parts` as moved.
    // `p = Pair { a: s2, b: p.into_a() }` moves all of p, so each of
    // its fields (#1509).
    self.rhs_self_moved = match (&lhs_rty, expr.kind) {
      (ResourceTy::Value(rap @ ResourceAccessPoint::Struct(_)), ExprKind::Assign(..)) => {
        let moved = locals_moved_by(self.tcx, rhs_expr);
        let whole = moved.contains(rap.name());
        #[expect(clippy::disallowed_methods, reason = "collected into a BTreeSet")]
        let parts: BTreeSet<String> = self.raps.keys()
          .filter(|m| *m != rap.name() && place_within(m, rap.name()) && (whole || moved.contains(*m)))
          .cloned()
          .collect();
        parts
      }
      _ => BTreeSet::new(),
    };
    let drops_old = !lhs_ty.is_ref() && !is_copyable && !lhs_is_union_field && !union_without_drop && !lhs_is_manually_drop && self.needs_drop(lhs_ty, lhs_expr.hir_id.owner);
    let mut contents_gone: Option<u64> = None;
    let drop_target: Option<ResourceAccessPoint> =
      if drops_old {
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
            let all_moved_by_rhs = !self.rhs_self_moved.is_empty()
              && self.reassign_parts(rap.name()).is_some_and(|p| p.dropped.is_empty() && p.maybe_moved.is_empty());
            // A Box whose contents only were moved out still has its
            // heap allocation, which the assignment frees (#1145).
            let gone = rap.box_contents_moved() && self.box_contents_only_moved_now(rap.name());
            if gone {
              contents_gone = Some(*rap.hash());
            }
            if (self.rap_holds_resource_now(rap.name()) || gone)
              && !self.destructure_moved.contains(rap.name()) && !all_moved_by_rhs {
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
            // need rap_holds_resource_now here. A reference to
            // part of the owner's resource overwrites only that part:
            // its drop is stated with the write (`write_part_behind_ref`).
            //
            // `**rb = v` with `rb: &mut Box<String>` writes the
            // Box's contents: only the old String is dropped, the
            // owner keeps its Box (#941), drawn as `*bx`
            // (`payload`). A Box deref further in (`***x` through
            // a `Box<Box<_>>`) isn't modelled: no drop on the owner's
            // column (the reference's dot says the old value is
            // dropped).
            if part_owner.is_some() {
              None
            } else {
              match self.box_deref_through_ref(lhs_expr) {
                // `*b = v` with b a Box drawn as one column (`Box<String>`):
                // the old contents are dropped on b's own column, which
                // keeps its heap allocation, as `**rb = v` draws it on
                // the owner's (#1345). (Only when b's contents are there
                // on every path: `AssignDropsOld`, see `box_contents_write`.
                // Also in a condition, whose events are drawn in its arm:
                // `mutate_branch_lines` keeps the drop on the write's dot,
                // #1391.)
                None if !p_rap.is_ref() && matches!(p_rap.place_write(), PlaceWrite::AssignDropsOld)
                  && matches!(lhs_expr.kind, ExprKind::Unary(UnOp::Deref, inner)
                    if self.tcx.typeck(inner.hir_id.owner).node_type(inner.hir_id).is_box()) => {
                  payload_behind_ref = true;
                  Some(p_rap.clone())
                }
                None => self.owner_behind_ref(p_rap, ""),
                Some(true) => {
                  payload_behind_ref = true;
                  self.owner_behind_ref(p_rap, "")
                }
                Some(false) => None,
              }
            }
          }
          ResourceTy::DerefField(p_rap, _, _, _) => {
            // `r.a = v` after `let r = &mut s;`: the field being
            // overwritten is the lender's `s.a`, so that's where
            // the old value is dropped (#152). Only when the lender
            // is a local with per-field columns; a parameter's
            // lender is the caller, which has no column here.
            self.field_column_behind_ref(lhs_expr, p_rap)
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
        self.struct_fields_holding_resource(rap.name()).into_iter()
          .filter(|f| !self.rhs_self_moved.iter().any(|m| place_within(f.name(), m)))
          .collect()
      }
      _ => Vec::new(),
    };

    // Whether each old value is there on every path, decided
    // before match_rhs too (see above). `*p = x` writes through a
    // live `&mut`, so its lender holds a value.
    let mut maybe_moved: Vec<bool> = drop_target.iter().chain(field_drop_targets.iter())
      .map(|rap| matches!(lhs_rty, ResourceTy::Value(_))
        && self.rap_ownership_now(rap.name()) == Ownership::Maybe)
      .collect();
    // Of those, the ones maybe-moved only because the loop can come
    // back round with them moved (#992).
    let loop_head: Vec<bool> = drop_target.iter().chain(field_drop_targets.iter())
      .zip(maybe_moved.iter())
      .map(|(rap, maybe)| *maybe && self.maybe_moved_by_loop_head_now(rap.name()))
      .collect();
    // And whether a let-chain's condition or a match guard may have
    // moved it (#281, #247): also before match_rhs, whose new value
    // for it ends that.
    let by_conds: Vec<bool> = drop_target.iter().chain(field_drop_targets.iter())
      .zip(maybe_moved.iter())
      .map(|(rap, maybe)| *maybe && self.moved_by_chain_condition(rap))
      .collect();
    // A struct or tuple some of whose parts were moved out before
    // (on every path, or some only): only the parts still there are
    // dropped (#628).
    let parts: Vec<Option<ReassignParts>> = drop_target.iter().chain(field_drop_targets.iter())
      .zip(maybe_moved.iter())
      .map(|(rap, maybe)| match rap {
        ResourceAccessPoint::Struct(_) if !maybe && matches!(lhs_rty, ResourceTy::Value(_)) => self.reassign_parts(rap.name()),
        _ => None,
      })
      .collect();

    // `s = …` in a loop's body, where s holds nothing on the path from
    // before the loop (moved before it) but a later time round may
    // hold what an earlier one gave it (#1001): drawn as a drop on the
    // assignment, as for a `let x;` local (#786). Its field columns
    // likewise. And after a loop that may have ended before giving it
    // a value, a drop flag decides (`InitBy::Loop`).
    let mut earlier_iteration: Vec<ResourceAccessPoint> = Vec::new();
    if let (ResourceTy::Value(rap), ExprKind::Assign(..)) = (&lhs_rty, expr.kind) {
      if drops_old && drop_target.is_none() && maybe_init.is_none() && self.given_by_loop_now(rap) {
        earlier_iteration.push(rap.clone());
        for m in self.struct_members_deep(rap.name()) {
          if !m.never_drops() && !m.is_plain_value() && !m.is_ref() && self.given_by_loop_now(&m) {
            earlier_iteration.push(m);
          }
        }
      }
      if drop_target.is_some() && maybe_init.is_none() {
        // After a loop inside another, every run of which may start
        // with s holding nothing or a value (#1032), only the drop
        // flag knows (as #1027's `DropFlag`).
        maybe_init = match self.maybe_init_by_loop_now(rap) {
          Some(false) => Some(InitBy::Loop),
          Some(true) => Some(InitBy::DropFlag),
          None => None,
        };
      }
      // `s = …` in a loop's body where s only may hold a value as the
      // loop starts (a branch above moved it, #1012), with nothing in
      // between: the first time round, the old value is dropped only
      // if no branch above moved it; a later time round, only if the
      // time round before left s holding one. Marked as an
      // `EarlierIteration` drop that's also `maybe_moved`. In a loop
      // inside another that s was declared before, an earlier run of
      // this loop may have given s a value too (#1019): `OuterLoop`.
      if drop_target.as_ref() == Some(rap) && maybe_init.is_none()
        && maybe_moved.iter().all(|m| *m) && loop_head.iter().all(|l| !*l) {
        let entries: Option<Vec<(Option<bool>, bool)>> = drop_target.iter().chain(field_drop_targets.iter())
          .map(|r| self.maybe_entry_untouched_now(r)).collect();
        if let Some(entries) = entries {
          maybe_init = if entries.iter().any(|(n, _)| n.is_some()) {
            Some(InitBy::OuterLoop { moves: entries.iter().any(|(n, _)| *n != Some(false)) })
          } else if entries.iter().all(|(_, rounds)| !rounds) {
            // A loop that never comes back round (#1467): there is no
            // later time round. The loop may end before this runs (a
            // `while` / `for`, or something before it leaves the loop):
            // then the old value, if any, is still there after it. Or
            // not (a bare `loop` gets here first): as if there were no
            // loop.
            self.loop_may_end_before(expr.span).then_some(InitBy::FirstPass)
          } else {
            Some(InitBy::EarlierIteration)
          };
        }
      }
      // `s = …` in a loop's body that the drawing reaches with s
      // holding a value, where rustc says it only may (#1027): a loop
      // above, inside this one, may have ended before giving s one
      // after a branch above moved it, and on a later time round the
      // value it holds may be one the time round before gave it. Only
      // the drop flag knows. (A struct's field columns go with it.)
      if drop_target.as_ref() == Some(rap) && maybe_init.is_none()
        && maybe_moved.iter().all(|m| !*m) && parts.iter().all(|p| p.is_none())
        && self.exit_frames.iter().any(|f| matches!(f, ExitFrame::Loop { .. }))
        && drop_target.iter().chain(field_drop_targets.iter())
          .all(|r| self.ownership_before_assign(r.name(), lhs_expr.span, expr.span) == Some(Ownership::Maybe)) {
        maybe_init = Some(InitBy::DropFlag);
        maybe_moved.iter_mut().for_each(|m| *m = true);
      }
    }

    // `q = Some(if c { &a } else { &b });` with q holding borrows
    // (#1470): as for `let q = Some(if ..)` (#1225), the constructor takes
    // nothing itself, and q gets each branch's value (and its borrow) in
    // that branch: q is paired with the conditional, as for `q = if c {
    // Some(&a) } else { .. }` (#1463).
    let pair_rhs = match &lhs_rty {
      ResourceTy::Value(rap) if rap.holds_borrow() && !rap.is_ref() && is_assign && destructure_hid.is_none() && !lhs_is_union_field =>
        self.conditional_value(rhs_expr).unwrap_or(rhs_expr),
      _ => rhs_expr,
    };
    // `it = b.iter();` where `it` holds a borrow (#336). (Done before
    // the branches were visited when they pair it themselves.)
    if let (ResourceTy::Value(rap), false) = (&lhs_rty, paired_in_branch) {
      if rap.holds_borrow() && !lhs_is_union_field && matches!(expr.kind, ExprKind::Assign(..)) {
        let rap = rap.clone();
        self.reassign_holder(&rap, pair_rhs, lhs_ty, line_num);
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
    // (Read before the branches that pair the struct in them were visited,
    // #1483.)
    let struct_field_ends = match (&lhs_rty, expr.kind) {
      _ if branch_field_ends.is_some() => branch_field_ends.clone().unwrap_or_default(),
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
    let mut late_holder_borrows: Vec<(String, &'tcx Expr<'tcx>)> = Vec::new();
    if let (ResourceTy::Index(..), ExprKind::AssignOp(..)) = (&lhs_rty, expr.kind) {
      // `v[i] += k`: k is an operand, not the element's new
      // value, so don't draw it as copied into v[i].
      let compound = match &lhs_rty {
        ResourceTy::Index(r, place, w) => ResourceTy::Index(r.clone(), place.clone(), w.into_update()),
        other => other.clone(),
      };
      self.add_ev(line_num, Evt::Bind, compound, ResourceTy::Anonymous, false);
    } else {
      // `**rb = v` / `**rb += v` write the place `**rb`, not `*rb`
      // (the Box, or the reference, rb points to): the reference's
      // dot names it as written (#941).
      let lhs_rty = match lhs_rty.clone() {
        ResourceTy::Deref(rap) if !lhs_ty.is_ref() => match self.multi_deref_place(lhs_expr, &rap) {
          Some(place) => {
            let write = rap.place_write();
            ResourceTy::DerefField(rap, place, write, self.field_holds(lhs_ty, lhs_expr.hir_id.owner))
          }
          None => ResourceTy::Deref(rap),
        },
        other => other,
      };
      let n_events = self.preprocessed_events.len();
      let first_id = *self.unique_id;
      // A whole new value: the borrows its holder fields take are
      // drawn after the old value's ends and drop (#1097).
      let overwrite = matches!(lhs_rty, ResourceTy::Value(_)) && matches!(expr.kind, ExprKind::Assign(..));
      let outer = if overwrite { self.deferred_holder_borrows.replace(Vec::new()) } else { None };
      if ref_arms {
        // (#1393) Each branch pairs its value with r: one naming a
        // variable the branch declares once it is visited (`y` in `{ let
        // y = &a; y }`), the others before, as their branch's events.
        let prev = self.defer_arm_binds.replace(rhs_expr.span);
        self.match_rhs(lhs_rty.clone(), rhs_expr, e);
        self.visit_expr(rhs_expr);
        self.defer_arm_binds = prev;
        self.bind_left_arms(rhs_expr);
        // r's lenders, now that the branches' variables are known (`a`
        // through `y`): as when the conditional was visited first.
        let (lender, aliasing, static_lender, co_lenders) = self.get_ref_data_full(rhs_expr);
        let r_name = lhs_rty.real_name();
        let mut kept: Vec<ResourceTy> = Vec::new();
        if let Some(rd) = self.borrow_map.get_mut(&r_name) {
          // (#1430: with the old lenders, which the branch keeping r's
          // value names through r itself.)
          if keeps_old_loan {
            kept = rd.all_lenders();
          }
          (rd.lender, rd.aliasing, rd.static_lender, rd.co_lenders) = (lender, aliasing, static_lender, co_lenders);
        }
        self.resolve_branch_lenders(&r_name, kept);
        self.mark_branch_results_kept(&lhs_rty.real_name(), rhs_expr);
      } else {
        // `s = p.a` of `Pair { a: s, b: t } = p;` (#1412).
        match destructure_hid.and_then(|h| self.destructure_parts.get(&h))
          .and_then(|n| self.raps.get(n)).map(|rd| rd.rap.to_owned()) {
          Some(part) => {
            self.update_rap(&part, line_num);
            self.add_ev(span_to_line(&rhs_expr.span, &self.tcx), e.clone(), lhs_rty.clone(), ResourceTy::Value(part), false);
          }
          None => self.match_rhs(lhs_rty.clone(), pair_rhs, e),
        }
        if let Some(loan) = self.loan_at_span(rhs_expr.span) {
          for (_, ev) in &self.preprocessed_events[n_events..] {
            if matches!(ev, ExternalEvent::StaticBorrow { to, .. } | ExternalEvent::MutableBorrow { to, .. } if to == &lhs_rty) {
              self.repoint_borrows.insert(ev.get_id(), loan.loan);
            }
          }
        }
        self.arms_paired_in_branch.clear();
        // Only for a value that holds borrows (or has parts that do): their
        // lenders may be split by the branches. (A reference is paired in
        // its branches above, `ref_arms`, #1393.)
        let holds = match &lhs_rty {
          ResourceTy::Value(rap) => {
            let prefix = format!("{}.", rap.name());
            #[expect(clippy::disallowed_methods, reason = "`any` over the map: order never observed")]
            let part_holds = self.raps.iter().any(|(n, rd)| n.starts_with(&prefix) && (rd.rap.is_ref() || rd.rap.holds_borrow()));
            rap.holds_borrow() || part_holds
          }
          _ => false,
        };
        if let (true, Some(cond)) = (holds && is_assign && destructure_hid.is_none() && !lhs_ty.is_ref(), self.conditional_value(rhs_expr)) {
          self.nest_arm_pairings(cond, rhs_ids.clone());
        }
      }
      // A field the right-hand side moved out after it was given its
      // new value (`t.0` in `t = (t.1, t.0)`, whose move into t.1 is
      // drawn after t.1's into t.0) owns its new value again, as for
      // a destructuring swap.
      // A field moved with the whole of p by the right-hand side
      // (`p = Pair { a: s2, b: p.into_a() }`, #1509): its move, recorded
      // while the right-hand side was visited, goes before the new value
      // the literal gives it on its column, as for a destructuring cycle.
      for m in self.rhs_self_moved.iter() {
        let Some(h) = self.raps.get(m).map(|rd| *rd.rap.hash()) else { continue };
        let arrives = self.preprocessed_events[n_events..].iter().any(|(_, ev)| matches!(ev,
          ExternalEvent::Move { to: ResourceTy::Value(t), .. } | ExternalEvent::Copy { to: ResourceTy::Value(t), .. }
            if *t.hash() == h));
        let moved_out = self.preprocessed_events[..n_events].iter().rev()
          .take_while(|(l, _)| *l == line_num)
          .find_map(|(_, ev)| match ev {
            ExternalEvent::Move { from: ResourceTy::Value(f), id, .. } if *f.hash() == h => Some(*id),
            _ => None,
          });
        if let (true, Some(id)) = (arrives, moved_out) {
          if !self.moves_out_first.contains(&id) {
            self.moves_out_first.push(id);
          }
        }
      }
      for m in std::mem::take(&mut self.rhs_self_moved) {
        let Some(rap) = self.raps.get(&m).map(|rd| rd.rap.clone()) else { continue };
        if !rap.never_drops() && !rap.is_ref() && !self.rap_holds_resource_now(&m) {
          self.add_ev(line_num, Evt::Bind, ResourceTy::Value(rap), ResourceTy::Anonymous, false);
        }
      }
      if overwrite {
        late_holder_borrows = std::mem::replace(&mut self.deferred_holder_borrows, outer).unwrap_or_default();
      }
      if let Some(b) = &box_payload {
        self.retarget_into_payload(b, first_id);
      }
      // `*r = &t` with r borrowing the reference variable x: x now
      // borrows t, so the borrow goes to x's column, where its end
      // already is, not r's (#904). r keeps borrowing x.
      if let (Some(x), ResourceTy::Deref(_)) = (&repointed, &lhs_rty) {
        self.retarget_borrows_into(&lhs_rty, x, first_id);
      }
      // `*r = &t` with r pointing to a reference that has no column
      // (`&mut words[0]`): t stays borrowed as long as the place
      // holds the new reference, as for `words[0] = &t` (#897), and
      // that end is drawn on t's column only, like #664's.
      // An element of a local holding borrows (`*el = &t1` with
      // `el = &mut refs[0]`) keeps it in that local instead (#1361).
      if let (false, ExprKind::AddrOf(..), ResourceTy::Deref(r)) = (repoints_ref_var, rhs_expr.kind, &lhs_rty) {
        if !self.store_into_holder(lhs_expr, n_events, &lhs_rty) {
          let place = ResourceTy::Index(r.clone(), format!("*{}", r.name()), PlaceWrite::Update);
          self.end_stored_borrow(lhs_expr, n_events, &lhs_rty, place);
        }
      } else if let (true, ExprKind::AddrOf(..), ResourceTy::Anonymous) = (lhs_ty.is_ref(), rhs_expr.kind, &lhs_rty) {
        // `*refs.get_mut(0).unwrap() = &t1;` (a place with no name):
        // kept in refs (#1384).
        self.store_into_holder(lhs_expr, n_events, &lhs_rty);
      } else if is_assign && lhs_ty.is_ref() && match &lhs_rty {
        ResourceTy::Deref(_) => !repoints_ref_var,
        ResourceTy::Anonymous => true,
        _ => false,
      } {
        // `*el = tr;` / `*refs.get_mut(0).unwrap() = tr;`: refs now
        // holds tr's borrow (#1384).
        self.store_ref_copy_into_holder(lhs_expr, rhs_expr, n_events);
      }
      self.draw_struct_field_borrow_ends(struct_field_ends, false);
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
      // The same for a labeled block whose `break`s carry its value
      // (`r = 'a: { if c { break 'a s; } t };`, #1134).
      let valued_block = matches!(rhs_expr.kind, ExprKind::Block(b, Some(_))
        if breaks_to_id(rhs_expr, b.hir_id).iter().any(|(_, v)| v.is_some()));
      if let (ResourceTy::Value(rap), true) = (&lhs_rty, valued_block || matches!(rhs_expr.kind, ExprKind::Loop(_, _, rustc_hir::LoopSource::Loop, _))) {
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

    // A write through a reference to a local struct's field (`r.n =
    // 2`) also changes the field's own column (#854). When the old
    // value is dropped, that drop is its event; otherwise the field
    // gets an in-place write.
    if drop_target.is_none() && field_drop_targets.is_empty() {
      self.write_field_behind_ref(&lhs_rty, lhs_expr, line_num);
    }
    if let Some(owner) = part_owner {
      let write = match &lhs_rty {
        ResourceTy::Deref(r) if is_assign => r.place_write(),
        _ => PlaceWrite::Update,
      };
      self.write_part_behind_ref(owner, &lhs_rty, lhs_expr, line_num, write);
    } else if let Some(param) = self.whole_param_behind_ref(&lhs_rty, lhs_expr, lhs_ty) {
      // `*r = vec![9]` with `let r = &mut *v;` (#1048): the place is
      // `*v`, written as on v's column for `*v = vec![9]`.
      match &lhs_rty {
        ResourceTy::Deref(r) if is_assign => {
          let place = ResourceTy::Index(param.clone(), format!("*{}", param.name()), r.place_write());
          self.add_ev(line_num, Evt::Bind, place, ResourceTy::Anonymous, false);
        }
        _ => self.write_part_behind_ref(param, &lhs_rty, lhs_expr, line_num, PlaceWrite::Update),
      }
    }

    let n_before_drops = self.preprocessed_events.len();
    let payload_target = drop_target.as_ref().map(|r| *r.hash());
    for ((((rap, maybe_moved), parts), loop_head), by_cond) in drop_target.into_iter().chain(field_drop_targets).zip(maybe_moved).zip(parts).zip(loop_head).zip(by_conds) {
      let drop_line = if crate::facts::is_vec_expansion(self.tcx, rhs_expr.span.ctxt().outer_expn()) {
        match self.reassign_drop_at(lhs_expr, &rap) {
          Some((row, true)) => row,
          Some((_, false)) => continue,
          None => drop_line,
        }
      } else { drop_line };
      let hash = *rap.hash();
      let payload = box_payload.as_ref().is_some_and(|b| b.hash() == rap.hash())
        || (payload_behind_ref && payload_target.is_some_and(|h| h == *rap.hash()));
      self.add_external_event(drop_line, ExternalEvent::OwnerDropAtReassign {
        ro: rap,
        id: *self.unique_id,
        maybe_moved,
        by_cond,
        loop_head,
        parts,
        maybe_init,
        payload,
        contents_gone: contents_gone.is_some_and(|h| h == hash),
      });
      *self.unique_id += 1;
    }
    for ro in earlier_iteration {
      self.add_external_event(drop_line, ExternalEvent::OwnerDropAtReassign {
        ro,
        id: *self.unique_id,
        maybe_moved: false,
        by_cond: false,
        loop_head: false,
        parts: None,
        maybe_init: Some(InitBy::EarlierIteration),
        payload: false,
        contents_gone: false,
      });
      *self.unique_id += 1;
    }
    if let Some(i) = drop_at {
      let n_drops = self.preprocessed_events.len() - n_before_drops;
      self.preprocessed_events[i..].rotate_right(n_drops);
    }
    // `o = vec![&w];` / `o = [&w];` where o holds a borrow: o now
    // borrows w, drawn as for `let o = vec![&w];` (#1095), after the
    // old borrow's end and the old value's drop. The same for a struct
    // literal's holder fields (`s2 = Shelf { items: vec![w.as_str()] }`).
    if let (ResourceTy::Value(rap), ExprKind::Assign(..)) = (&lhs_rty, expr.kind) {
      if rap.holds_borrow() && !lhs_is_union_field {
        let name = rap.name().to_owned();
        self.emit_holder_literal_borrows(&name, rhs_expr);
      }
    }
    for (name, e) in late_holder_borrows {
      self.emit_holder_literal_borrows(&name, e);
    }
    if let (ResourceTy::Value(rap), ExprKind::Assign(..)) = (&lhs_rty, expr.kind) {
      let name = rap.name().to_owned();
      self.resolve_block_local_lenders(&name, rhs_expr);
    }
}

  /// The `ExprKind::If` arm of `visit_expr`: `if guard { then } else { els }`.
  fn visit_if(&mut self, expr: &'tcx Expr<'tcx>, guard_expr: &'tcx Expr<'tcx>, if_expr: &'tcx Expr<'tcx>, else_expr: Option<&'tcx Expr<'tcx>>) {
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
    // The same for the `let` parts of a let chain (`if let (x, _) = (f,
    // s) && c`, #971). What the else branch starts with is
    // `chain_fail`'s; the if branch drops each tuple like a lone `if
    // let` does.
    let mut chain_tuples: Vec<&'tcx rustc_hir::LetExpr<'tcx>> = Vec::new();
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
        let body_end = span_end_line(&if_expr.span, &self.tcx);
        let (bound, first_in_branch) = self.register_iflet_let_bindings(let_expr, None, body_end);
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
        let mut walk = ChainWalk { body_end: span_end_line(&if_expr.span, &self.tcx), in_loop: self.in_loop_body(guard_expr.hir_id), ..ChainWalk::default() };
        self.visit_if_let_chain(guard_expr, &outer, &mut decls, &mut walk);
        chain_tuples = self.chain_tuple_lets(guard_expr);
        if_lo = walk.first_lo.unwrap_or(start);
        let path = if else_expr.is_some() { FailPath::Else } else { FailPath::NoElse };
        chain_fail = self.chain_fail_moves(&walk, chain_lo, path);
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
    // A lone `if let` whose pattern matches every value its scrutinee
    // can have: an irrefutable pattern, or one that takes apart a
    // literal the way it was built (`(x, _)` on `(f, s)`, `Some(x)` on
    // `Some(f)`, #981). Its body always runs.
    let always_matches = iflet_tuple.is_some_and(|l| self.let_always_matches(l.pat, l.init)
      || (pat_has_or(l.pat) && else_expr.is_none() && self.facts.and_then(|f| f.joins.get(&expr.span)).is_some_and(|j| j.dead_implicit_else)));
    // Its `else` block, if any, is dead code (#987): rustc warns
    // `irrefutable_let_patterns`, and the compiled program has no else
    // block (its MIR drops what the block would have moved at scope
    // end, unconditionally). The block is visited, so its variables
    // and calls are known, but its events are discarded and the `if`
    // is drawn as one without an `else`; a dot on each outer variable
    // it used says the branch never runs (`dead_else_notes`).
    let dead_else = always_matches && else_expr.is_some();
    let mut dead_else_notes: Vec<(usize, ResourceAccessPoint)> = Vec::new();
    // Restored, not reset: this `if` may sit inside another branch
    // (#410).
    let prev_inside_branch = self.inside_branch;
    self.inside_branch = true; // need this flag to correctly handle variables that are declared inside blocks
    // Both branches start from the same set of uninitialized
    // `let x;` locals (#234); see join_uninit_locals.
    let uninit_before = self.uninit_locals.clone();
    let maybe_init_before = self.maybe_init_locals.clone();
    // And from the same ones a loop above may not have assigned: an
    // assignment in the if branch doesn't make one in the else branch
    // a branch's to decide (#1036).
    let by_loop_before = self.maybe_init_by_loop.clone();
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
    #[expect(clippy::disallowed_methods, reason = "inserted into a set; order is lost")]
    self.matched_block_temps.extend(scrutinee_temps.iter().cloned());
    self.exit_frames.push(ExitFrame::Branch { len: frame_len, hide, binds: Vec::new(), binds_at: 0, fn_level: false });
    // An exit in the if branch drops the tuple scrutinee (#327), or
    // those of a let chain's parts (#971).
    let if_tuples: Vec<&'tcx rustc_hir::LetExpr<'tcx>> = iflet_tuple.filter(|_| !scrutinee_moves.is_empty())
      .into_iter().chain(chain_tuples.iter().copied()).collect();
    for let_expr in &if_tuples {
      let live_tuple = LiveTuple {
        frame: self.exit_frames.len() - 1,
        len: entry_len,
        scrutinee: let_expr.init,
        tuple: self.tuple_scrutinee_text(let_expr.init),
        pat: let_expr.pat,
        at: TupleDropAt::IfExit,
      };
      self.live_tuples.push(live_tuple);
    }
    let mut arm_borrows = self.arm_borrows_begin();
    let arm_lines = |this: &Self, e: &Expr<'_>| {
      let sm = this.tcx.sess.source_map();
      (sm.lookup_char_pos(expr.span.lo()).line, sm.lookup_char_pos(expr.span.hi()).line,
       sm.lookup_char_pos(e.span.lo()).line, sm.lookup_char_pos(e.span.hi()).line)
    };
    self.arm_frames.push(arm_lines(self, if_expr));
    self.visit_expr(&if_expr);
    self.arm_frames.pop();
    self.bind_deferred_arm(if_expr);
    let live_len = self.live_tuples.len() - if_tuples.len();
    self.live_tuples.truncate(live_len);
    // Everything declared in each branch, also inside an inline
    // construct nested in it (an `else if let`'s bindings, #501):
    // see raps_declared_since. A name an earlier, out-of-scope
    // variable used shares its column, so only the branch's code
    // shows it's declared there (#927). The same goes for a let-chain
    // condition's own variables (`&& { let w = t; … }`, #969): they
    // count as the if branch's (`if_hash_lo` is taken before the
    // chain), and with a name of their own they're found that way.
    let mut if_inner = &self.raps_declared_since(if_hash_lo, &uninit_before) | &bindings_within(if_expr, &self.tcx, &self.raps);
    if is_let_chain(guard_expr) {
      if_inner = &if_inner | &bindings_within(guard_expr, &self.tcx, &self.raps);
    }
    let else_hash_lo = self.rap_hashes;
    let uninit_after_if = std::mem::replace(&mut self.uninit_locals, uninit_before.clone());
    let maybe_init_after_if = std::mem::replace(&mut self.maybe_init_locals, maybe_init_before.clone());
    let by_loop_after_if = std::mem::replace(&mut self.maybe_init_by_loop, by_loop_before);
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
    let path = if else_expr.is_some() { FailPath::Else } else { FailPath::NoElse };
    // Borrows it made that have ended by then (#688) come last.
    self.note_chain_then(&chain_fail, guard_expr.span.to(if_expr.span), expr.span);
    let (fail_events, fail_ends) = self.chain_fail_events(std::mem::take(&mut chain_fail), path,
      else_expr.map(|e| e.span), expr.span, fail_line);
    let fail_lo = fail_events.first().map(|ev| ev.get_id());
    let uses_lo = fail_events.iter()
      .find(|ev| matches!(ev, ExternalEvent::ChainFailUse { .. }))
      .map_or(*self.unique_id, |ev| ev.get_id());
    let uses_ids = uses_lo..*self.unique_id;
    for ev in fail_events {
      self.add_external_event(fail_line, ev);
    }
    // The if-body's events, when there's an `else` (see below).
    let mut if_ev_early: Option<Vec<(usize, ExternalEvent)>> = None;
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
        // The if-body's events aren't on the `else`'s path: take
        // them out before it's visited, as a `match` does arm by arm,
        // so what the `else` asks of the events so far (does `s`
        // still own a value its reassignment drops?) doesn't see the
        // if-body's moves (#245).
        // (A dead `else`'s events are discarded: the if-body's stay
        // where they are, drawn as for an `if` without `else`.)
        if !dead_else {
          if_ev_early = Some(self.take_branch_events(if_ids.clone(), Some(if_expr), &pre_split));
        }
        // Nor are the if-body's `if let` bindings: an exit in the
        // `else` doesn't drop them.
        let outer: Vec<String> = self.in_scope[..entry_len].to_vec();
        self.in_scope.truncate(entry_len);
        if dead_else {
          // Nothing it does to the loans counts either.
          let borrows = self.borrow_map.clone();
          let arm_loans = self.arm_loans.len();
          let lo = *self.unique_id;
          self.arm_frames.push(arm_lines(self, e));
          self.visit_expr(e);
          self.arm_frames.pop();
          self.bind_deferred_arm(e);
          self.borrow_map = borrows;
          self.arm_loans.truncate(arm_loans);
          dead_else_notes = self.discard_dead_else(lo..*self.unique_id, e, &outer);
          HashSet::new()
        } else {
          self.arm_borrows_next(&mut arm_borrows);
          self.arm_frames.push(arm_lines(self, e));
          self.visit_expr(e);
          self.arm_frames.pop();
          self.bind_deferred_arm(e);
          // The ends, on this path, of a new reference's borrows the
          // condition made (#603): among the else branch's events.
          for (line, ev) in fail_ends.iter().cloned() {
            let lo = fail_lo.unwrap_or(0);
            let at = self.preprocessed_events.iter()
              .rposition(|(l, e)| *l <= line && e.get_id() >= lo)
              .map_or(self.preprocessed_events.len(), |i| i + 1);
            self.preprocessed_events.insert(at, (line, ev));
          }
          get_decl_of_expr(e, &self.tcx, &self.raps)
        }
      }
      None => HashSet::new(),
    };
    // From here on, an `if` whose `else` is dead is drawn as one
    // without an `else` (#987).
    let else_expr = else_expr.filter(|_| !dead_else);
    if dead_else {
      // Whatever `let x;` locals it assigned are as the if branch
      // left them: the else never runs.
      self.uninit_locals = uninit_after_if.clone();
      self.maybe_init_locals = maybe_init_after_if.clone();
    }
    let mut else_inner = if dead_else { HashSet::new() } else { self.raps_declared_since(else_hash_lo, &uninit_before) };
    if let Some(e) = else_expr {
      else_inner = &else_inner | &bindings_within(e, &self.tcx, &self.raps);
    }
    self.arm_borrows_end(arm_borrows);
    let uninit_after_else = std::mem::take(&mut self.uninit_locals);
    let maybe_init_after_else = std::mem::take(&mut self.maybe_init_locals);
    #[expect(clippy::disallowed_methods, reason = "collected into a set; order is lost")]
    let known: HashSet<HirId> = uninit_before.keys().chain(maybe_init_before.iter()).copied().collect();
    // The `let x;` locals the if branch surely assigned (only
    // tested with `any` below: order doesn't matter).
    #[expect(clippy::disallowed_methods, reason = "only tested with any; order doesn't matter")]
    let assigned_in_if: Vec<HirId> = known.iter()
      .filter(|id| !uninit_after_if.contains_key(id) && !maybe_init_after_if.contains(id))
      .copied().collect();
    self.maybe_init_by_loop = &self.maybe_init_by_loop | &by_loop_after_if;
    // (A dead `else`, #987: only the if branch, which always runs.)
    let maybe_init = if dead_else {
      self.join_uninit_locals(vec![(Some(if_expr), uninit_after_if, maybe_init_after_if)], &known)
    } else {
      self.join_uninit_locals(vec![
        (Some(if_expr), uninit_after_if, maybe_init_after_if),
        (else_expr, uninit_after_else, maybe_init_after_else),
      ], &known)
    };
    // A `let x;` local the if branch assigned that may still have
    // no value after the `if` (#244). (Not a reference: it owns
    // nothing a drop flag could drop.)
    let assigns_maybe_init = assigned_in_if.iter().any(|id| self.maybe_init_locals.contains(id)
      && !self.tcx.typeck(id.owner).node_type(*id).is_ref());
    let else_ids = if_ids.end..*self.unique_id;
    // A dead `else`'s notes (#987), on its own lines, after the `if`
    // as it is drawn. (Not on a line the if branch ends on, `} else {
    // take(g); }`, where the column may have a dot of the if branch's.)
    let if_end_line = self.tcx.sess.source_map().lookup_char_pos(if_expr.span.hi()).line;
    for (line, ro) in std::mem::take(&mut dead_else_notes).into_iter().filter(|(l, _)| *l > if_end_line) {
      self.preprocessed_events.push((line, ExternalEvent::DeadElseNote { ro, id: *self.unique_id }));
      *self.unique_id += 1;
    }
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
        // A body that assigns a `let x;` local is drawn as a branch
        // too when x may have no value on the implicit `else` path
        // (#244): the join says so.
        // So is a body that always leaves the function after assigning
        // a `let x;` local (`if c { n = 1; return; }`): x still has no
        // value after the `if`, and the next assignment is its first
        // (#930). (Not a `break` out of a loop: x keeps its value
        // after that loop.)
        let assigns_then_exits = self.expr_diverges(if_expr)
          && assigned_in_if.iter().any(|id| self.uninit_locals.contains_key(id))
          && branch_leaves(self.tcx, BranchBody::Expr(if_expr)).exits.iter()
            .all(|x| matches!(x, BranchExit::Return | BranchExit::Panic | BranchExit::Other));
        // So is a body that gives a new value to a variable that may
        // hold none where the `if` starts (#1034): on the implicit
        // `else` path it still may hold none after the `if`, so after
        // it, it only may hold a value.
        let assigns_maybe_held = !always_matches && !self.expr_diverges(if_expr) && {
          let mut body_live = HashSet::new();
          collect_event_live_vars(&body_ev, &mut body_live);
          let body_live = &body_live - &if_scope;
          let start = self.preprocessed_events.iter()
            .position(|(_, e)| body_ev.iter().any(|(_, b)| b.get_id() == e.get_id()))
            .unwrap_or(self.preprocessed_events.len());
          !self.maybe_held_after(&body_live, &[&body_ev, &[]], &maybe_init, &self.deferred_cols(&maybe_init_before),
            |name| self.rap_ownership_before(start, name)).is_empty()
        };
        //
        // An `if let` whose pattern matches every value always runs
        // its body (`always_matches`, also for a literal taken apart
        // the way it was built, #981): there's no implicit empty
        // `else` path, so the body is drawn inline, like a plain
        // block (or a one-arm `match`), whatever it does (#995). (The
        // borrow checker still treats a `let x;` it assigns as maybe
        // unassigned after the `if`, but at run time the body always
        // assigned it, and nothing is dropped with a drop flag.)
        // A scrutinee temporary its pattern moves something out of (or
        // borrows from) forks (#1386).
        #[expect(clippy::disallowed_methods, reason = "any(): order is irrelevant")]
        let takes_temp = scrutinee_temps.iter().any(|t| t.is_split_scrutinee_temporary() && takes_from(&body_ev, t));
        let drawn_as_branch = !always_matches
          && (events_move_out_of_outer(&body_ev, &if_scope) || exit_branch
            || takes_temp
            || assigns_maybe_init || assigns_then_exits || assigns_maybe_held
            || (self.expr_diverges(if_expr) && {
              let sm = self.tcx.sess.source_map();
              let (split, if_end) = (sm.lookup_char_pos(if_expr.span.lo()).line, sm.lookup_char_pos(if_expr.span.hi()).line);
              self.borrow_may_end_at_exit(BranchBody::Expr(if_expr), if_expr, split, if_end, &if_scope)
            }));
        // A tuple scrutinee's variables are moved whether or not the
        // body runs (#327), so that's a second path as well, unless
        // the pattern always matches (#981).
        if (scrutinee_moves.is_empty() || always_matches) && !drawn_as_branch && !chain_fail_ev {
            self.preprocessed_events.retain(|(_, e)| !uses_ids.contains(&e.get_id()));
            for evs in self.event_line_map.values_mut() {
              evs.retain(|e| !uses_ids.contains(&e.get_id()));
            }
            // The body may be skipped, so a `return` / `break` /
            // panic in it doesn't end the columns it's drawn on;
            // its drops aren't drawn (#410). (Nor are they in a body
            // that always runs, #995, as in a one-arm `match`: the
            // code after the `if` is drawn. But a tuple scrutinee's
            // drop is said on the columns of what it took, which
            // have nothing else to say it.)
            if always_matches {
              let tuple_drops: HashSet<usize> = self.preprocessed_events.iter()
                .filter(|(_, e)| if_ids.contains(&e.get_id()) && matches!(e, ExternalEvent::TupleTempDrop { .. }))
                .map(|(_, e)| e.get_id()).collect();
              self.exit_drops.retain(|id| !tuple_drops.contains(id));
            }
            // A body that always runs and always leaves the function
            // (#1004): its exit's drops are drawn, and nothing after.
            let leaves_fn = always_matches && self.at_fn_level(expr) && self.ends_at_exits(if_expr, true);
            // Or, directly in a branch, always leaves that branch: its
            // drops stay on the branch's columns, ended there (#1013).
            let leaves_branch = always_matches && !leaves_fn && self.inline_in_branch(expr) && self.ends_at_exits(if_expr, true);
            if leaves_fn {
              self.keep_inline_exit_drops(if_ids.clone(), expr);
            } else if !leaves_branch {
              self.discard_exit_drops(if_ids.clone());
            }
            #[expect(clippy::iter_over_hash_type, reason = "removing from a set; order is irrelevant")]
            for t in &scrutinee_temps {
              self.matched_block_temps.remove(t);
            }
            let left = self.left_at_exits(leaves_fn || leaves_branch, &if_decl);
            for var in in_decl_order(&if_decl).into_iter().filter(|v| needs_scope_end(v) && !left.contains(v)) {
                self.preprocessed_events.push((
                    if_end,
                    ExternalEvent::GoOutOfScope { ro: var.clone(), id: *self.unique_id, early: None },
                ));
                *self.unique_id += 1;
            }
            // The tuple scrutinee of a pattern that always matches
            // (#981) is dropped at the end of the body, as on the
            // if branch below.
            if !self.expr_diverges(if_expr) {
              if let Some(let_expr) = iflet_tuple.filter(|_| !scrutinee_moves.is_empty()) {
                let tuple_name = self.tuple_scrutinee_text(let_expr.init);
                for e in self.tuple_drop_events(let_expr.init, &tuple_name, Some(let_expr.pat), TupleDropAt::IfEnd) {
                  self.preprocessed_events.push((if_end, e));
                }
              }
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
    let mut if_ev = match if_ev_early {
      Some(ev) => ev,
      None => self.take_branch_events(if_ids, Some(if_expr), &pre_split),
    };
    let mut else_ev = self.take_branch_events(else_ids, else_expr, &HashSet::new());
    #[expect(clippy::iter_over_hash_type, reason = "removing from a set; order is irrelevant")]
    for t in &scrutinee_temps {
      self.matched_block_temps.remove(t);
    }
    // A scrutinee temporary that an exit in the matched block drops
    // forks here (#1360), as a `match` scrutinee's does.
    let forked_temps = self.fork_exited_temps(&scrutinee_temps, &mut if_ev, &mut else_ev, self.expr_diverges(if_expr));
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
    // (Not a forked scrutinee temporary: its drops in the branches end
    // its legs, #1360.)
    let if_scoped = &scoped_in_events(&if_ev, &self.exit_drops) - &forked_temps;
    let else_scoped = &scoped_in_events(&else_ev, &self.exit_drops) - &forked_temps;
    // Variables declared inside a branch (let-stmts within the
    // body, plus if-let pattern bindings) live only for that
    // branch — they're not pre-existing variables that need a
    // branched timeline. Subtract them out.
    let declared_in_branches: HashSet<ResourceAccessPoint> =
      &(&if_scope | &else_scope) | &(&if_scoped | &else_scoped);
    liveness = &(&liveness - &declared_in_branches) | &forked_temps;

    // add gos events for variables declared in each block, except
    // where every path through the branch leaves it at an exit
    // that drops them (#630)
    let if_left = self.left_at_exits(self.ends_at_exits(if_expr, true), &if_decl);
    let else_left = self.left_at_exits(else_expr.is_some_and(|e| self.ends_at_exits(e, true)), &else_decl);
    for var in in_decl_order(&if_decl).into_iter().filter(|v| needs_scope_end(v) && !if_scoped.contains(v) && !if_left.contains(v)) {
      if_ev.push((if_end, ExternalEvent::GoOutOfScope { ro: var.clone(), id: *self.unique_id, early: None }));
      *self.unique_id += 1;
    }

    for var in in_decl_order(&else_decl).into_iter().filter(|v| needs_scope_end(v) && !else_scoped.contains(v) && !else_left.contains(v)) {
      else_ev.push((merge, ExternalEvent::GoOutOfScope { ro: var.clone(), id: *self.unique_id, early: None }));
      *self.unique_id += 1;
    }
    if_decl.extend(if_inner);
    if_decl.extend(if_scoped);
    // (A let chain's scrutinee temporary counts as declared in the if
    // branch, but a forked one is live in both, #1360.)
    let if_decl = &if_decl - &forked_temps;
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
      // (A struct or tuple variable's part columns too, #627.)
      let unbound: Vec<ResourceAccessPoint> = scrutinee_moves.iter()
        .flat_map(|v| self.tuple_scrutinee_unbound(v, &[])).collect();
      for r in &unbound {
        front.push((if_end, ExternalEvent::Move {
          from: ResourceTy::Value(r.clone()),
          to: self.tuple_scrutinee_target(let_expr.init),
          id: *self.unique_id,
          is_partial: false,
          into_closure: false,
          loop_head: LoopHead::No,
          break_alt: None,
          break_out: false,
          carried_with: None, via_try: None,
          closure_value: false,
        }));
        *self.unique_id += 1;
        liveness.insert(r.clone());
      }
      // (A pattern that always matches, #981, is drawn inline above,
      // #995: there's no path where it fails.)
      let at = if else_expr.is_some() { TupleDropAt::Else } else { TupleDropAt::NoElse };
      front.extend(self.tuple_drop_events(let_expr.init, &tuple_name, None, at).into_iter().map(|e| (if_end, e)));
      else_ev.splice(0..0, front);
    }
    // A branch that leaves early drops it at its exit instead: see
    // drop_locals_on_exit. A let chain's later tuple first, as Rust
    // drops them.
    if !if_diverges {
      for let_expr in if_tuples.iter().rev() {
        let tuple_name = self.tuple_scrutinee_text(let_expr.init);
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
    // An `if let` / let-chain condition over several lines (`if let
    // Some(x) = o` / `&& bump(r)` / `{`): what its pattern or later
    // parts do to the variables the arms branch on happens on the
    // if-branch's path, on those lines, before the body's `{`. The
    // arms then split on the `if` line, as for a condition on one line
    // (#1279), and those events are drawn in the if arm, not on rows
    // above where its column starts (#1291).
    let touches_live = |e: &ExternalEvent| {
      let mut vs = HashSet::new();
      collect_event_live_vars(&[(0, e.clone())], &mut vs);
      !vs.is_disjoint(&liveness)
    };
    let split = if (iflet_tuple.is_some() || is_let_chain(guard_expr))
      && if_ev.iter().any(|(l, e)| (line_num..split).contains(l) && touches_live(e))
    {
      line_num
    } else {
      split
    };
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
    let (if_diverges, if_exits, if_carries) = self.branch_divergence(BranchBody::Expr(if_expr), &if_ev, &if_decl);
    let (else_diverges, else_exits, else_carries) = match else_expr {
      Some(e) => self.branch_divergence(BranchBody::Expr(e), &else_ev, &else_decl),
      None => (false, Vec::new(), Vec::new()),
    };
    let (b_labels, b_slices, b_branches) = match else_expr {
      Some(_) => {
        let else_map = create_line_map(&else_ev);
        (
          vec!["If".to_owned(), "Else".to_owned()],
          vec![if_slice, else_slice],
          vec![
            ExtBranchData { source_span: self.arm_span(if_expr.span), e_data: if_ev,   line_map: if_map,   decl_vars: if_decl,   diverges: if_diverges,   exits: if_exits, break_carries: if_carries   },
            ExtBranchData { source_span: else_expr.and_then(|e| self.arm_span(e.span)), e_data: else_ev, line_map: else_map, decl_vars: else_decl, diverges: else_diverges, exits: else_exits, break_carries: else_carries },
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
            ExtBranchData { source_span: self.arm_span(if_expr.span), e_data: if_ev, line_map: if_map, decl_vars: if_decl, diverges: if_diverges, exits: if_exits, break_carries: if_carries },
            ExtBranchData { source_span: else_expr.and_then(|e| self.arm_span(e.span)), e_data: else_ev, line_map: else_map, decl_vars: HashSet::new(), diverges: false, exits: Vec::new(), break_carries: Vec::new() },
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
    let loop_assigned = self.loop_assigned_now(&liveness);
    // (The arms' events are out of the global timeline by now: what
    // it says is the state where the `if` starts.)
    let reaching: Vec<&[(usize, ExternalEvent)]> = b_branches.iter()
      .filter(|b| !b.diverges).map(|b| b.e_data.as_slice()).collect();
    let maybe_held = self.maybe_held_after(&liveness, &reaching, &maybe_init, &self.deferred_cols(&maybe_init_before), |name| self.rap_ownership_now(name));
    self.event_spans.insert(*self.unique_id, guard_expr.span);
    fn patterns(cond:&Expr<'_>, out: &mut Vec<(usize,rustc_span::Span)>) {
      match cond.kind {
        ExprKind::Let(binding) => out.push((0,binding.pat.span)),
        ExprKind::DropTemps(inner) => patterns(inner,out),
        ExprKind::Binary(op,left,right) if op.node==rustc_hir::BinOpKind::And => {
          patterns(left,out); patterns(right,out);
        }
        _=>{}
      }
    }
    let mut arms=Vec::new();
    patterns(guard_expr,&mut arms);
    if !arms.is_empty(){self.arm_patterns.insert(*self.unique_id,arms);}
    self.branch_conditions.insert(*self.unique_id, guard_expr.span);
    self.add_external_event(line_num,
      ExternalEvent::Branch {
        join_span: Some(expr.span),
        live_vars: liveness,
        branches: b_branches,
        on_exhaustion: Default::default(),
        skipped_holds: if else_expr.is_none() { self.skipped_holds(guard_expr, if_expr) } else { Default::default() },
        branch_type: b_ty,
        split_point: split,
        merge_point: merge,
        maybe_init,
        loop_assigned,
        maybe_held,
        id: *self.unique_id });
        *self.unique_id += 1;
    // Both branches leave the function: nothing after the `if` runs.
    if else_expr.is_some() && if_diverges && else_diverges && self.at_fn_level(expr) {
      self.note_all_paths_exit(expr);
    }
}

  /// The `ExprKind::Match` arm of `visit_expr`: a `match`, and the desugarings
  /// that lower to one (`for`, `?`).
  fn visit_match(&mut self, expr: &'tcx Expr<'tcx>, guard_expr: &'tcx Expr<'tcx>, arms: &'tcx [rustc_hir::Arm<'tcx>], source: rustc_hir::MatchSource) {
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

    match source {
      // A normal match (not desugared)
      rustc_hir::MatchSource::Normal => self.visit_match_normal(expr, guard_expr, arms),
      rustc_hir::MatchSource::ForLoopDesugar => self.visit_for_desugar(expr, guard_expr, arms),
      // The scrutinee `Try::branch(e)` was visited above.
      rustc_hir::MatchSource::TryDesugar(_) => self.note_try(expr),
      _ => {}
    }
}

  /// The `MatchSource::Normal` case of `visit_match`: a `match` the user
  /// wrote. One arm at a time, it draws the pattern's bindings, the guard,
  /// the body and the arm's scope ends; several arms become a Branch event.
  fn visit_match_normal(&mut self, expr: &'tcx Expr<'tcx>, guard_expr: &'tcx Expr<'tcx>, arms: &'tcx [rustc_hir::Arm<'tcx>]) {
    let typeck_res = self.tcx.typeck(expr.hir_id.owner);

    // To my knowledge a match has to either contain a singular expression or Tuple
    // get all the 'parents' ie things being matched on and their types
    // (A constructor scrutinee `Some(f)` that a pattern takes apart is
    // split like a tuple literal, #807.)
    let split = match guard_expr.kind {
      ExprKind::Tup(fields) => Some(fields),
      _ => self.literal_elems(guard_expr),
    };
    let (parents, parents_ty) = match split {
      Some(fields) => {
        let mut res = Vec::new();
        let mut res_ty = Vec::new();
        for field in fields.iter() {
          res.push(self.scrutinee_rap(&field));
          res_ty.push(typeck_res.node_type(field.hir_id));
        }
        (res, res_ty)
      }
      _ => {
        // `match self.kind` with self a reference (#266).
        (vec![self.scrutinee_rap(&guard_expr)], vec![typeck_res.node_type(guard_expr.hir_id)])
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
    // A lone arm that always leaves the function (`x => return …`),
    // directly in its body (#1004): see `keep_inline_exit_drops`.
    let leaves_fn = inline && self.at_fn_level(expr) && self.ends_at_exits(arms[0].body, true);
    // Or the branch it is directly in (#1013).
    let leaves_branch = inline && !leaves_fn && self.inline_in_branch(expr) && self.ends_at_exits(arms[0].body, true);
    // Every arm starts from the same uninitialized `let x;`
    // locals (#234); see join_uninit_locals.
    let uninit_before = self.uninit_locals.clone();
    let maybe_init_before = self.maybe_init_locals.clone();
    // Likewise those a loop above may not have assigned (#1036); what
    // each arm leaves of them is joined with the rest below.
    let by_loop_before = self.maybe_init_by_loop.clone();
    let mut by_loop_ends: HashSet<HirId> = HashSet::new();
    let mut uninit_ends = Vec::new();
    // An exit in an arm doesn't see the binding of any arm's
    // value to a `let` (#410): it happens after the match. The
    // earlier arms' own events are out of `preprocessed_events`
    // by then (`take_branch_events` below), except a lone
    // arm's, which has no siblings.
    let entry_len = self.in_scope.len();
    let match_lo = *self.unique_id;
    let hide = self.rhs_ids_of(arms.iter().map(|a| Some(a.body)));
    self.exit_frames.push(ExitFrame::Branch { len: entry_len, hide, binds: Vec::new(), binds_at: 0, fn_level: false });
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
    let mut arm_borrows = self.arm_borrows_begin();
    // What each earlier arm's guard leaves behind when it fails (#247).
    let mut failed_guards: Vec<(ChainFail, FailPath)> = Vec::new();
    for (arm_idx, arm) in arms.iter().enumerate() {
      if arm_idx > 0 {
        self.arm_borrows_next(&mut arm_borrows);
      }
      // An arm's pattern bindings are out of scope in the next.
      self.in_scope.truncate(entry_len);
      let arm_hash_lo = self.rap_hashes;

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

      
      // The arm starts with what the failed guards of earlier arms
      // left behind (#247, #248), before its pattern's events: drawn
      // on its pattern's line. They're also on the global list while
      // its pattern, guard and body are visited, so that, say, a
      // reassignment there sees the drop flag (they're taken off with
      // the pattern's events, by id, after the body).
      let mut fail_evs: Vec<(usize, ExternalEvent)> = Vec::new();
      // The ends of a new reference's borrows a guard made (#603), put
      // among the arm's events after its body.
      let mut fail_ends: Vec<(usize, ExternalEvent)> = Vec::new();
      let pat_id_lo = *self.unique_id;
      for (fail, path) in failed_guards.clone() {
        let (evs, ends) = self.chain_fail_events(fail, path, Some(arm.span), expr.span, pat_line);
        for ev in evs {
          self.preprocessed_events.push((pat_line, ev.clone()));
          fail_evs.push((pat_line, ev));
        }
        fail_ends.extend(ends);
      }
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
      // `(s, n) =>` on `*b`, a Box whose payload's parts have
      // columns: the arm destructures `b.0` / `b.1` (#892).
      let box_parent: Option<Vec<ResourceTy>> = self.box_payload_parent(guard_expr, arm.pat).map(|p| vec![p]);
      let parents: &[ResourceTy] = box_parent.as_deref().unwrap_or(&parents);
      let (arm_pat, or_conflicts) = self.or_arm_stand_in(arm.pat, parents, &parents_ty, Some(guard_expr), pat_line, typeck_res, &scrutinee_moves);
      // add/fetch raps that are initialized in arm expr
      // need to also get their types, in order to figure out if we are moving, copying or borrowing something into the block
      let mut pat_out = MatchArmPat { branch_e_data: fail_evs, ..MatchArmPat::default() };
      let cx = MatchArmCtx { guard_expr, typeck_res, parents, parents_ty: &parents_ty, scrutinee_moves: &scrutinee_moves, tuple_name: &tuple_name, or_conflicts: &or_conflicts, arm_pat, pat_line, end, inline };
      match arm_pat.kind {
        PatKind::TupleStruct(_, pat_list, _) | PatKind::Tuple(pat_list, _) => self.match_arm_tuple_pat(cx, pat_list, &mut pat_out),
        _ => self.match_arm_singleton_pat(cx, &mut pat_out),
      }
      let MatchArmPat { mut pat_decls, mut member_binds, mut branch_e_data, callback_events, arm_moved, or_events, or_end_events } = pat_out;

      // A variable the scrutinee tuple took but this arm's
      // pattern doesn't move out (`_ => …`, `(Some(_), b)`): it
      // was still moved into the tuple, and isn't usable after
      // the match (#327).
      // (A struct or tuple variable's part columns too, but not
      // the ones the pattern took, #627.)
      for v in scrutinee_moves.iter() {
        for r in self.tuple_scrutinee_unbound(v, &arm_moved) {
          let id = *self.unique_id;
          *self.unique_id += 1;
          let ext_ev = self.ext_ev_of_evt(Evt::Move, into_tuple.clone(), ResourceTy::Value(r), id, false);
          if inline {
            self.add_external_event(pat_line, ext_ev);
          } else {
            branch_e_data.push((pat_line, ext_ev));
          }
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

      let failed = self.match_arm_guard(arm, inline, &mut pat_decls, &mut branch_e_data);
      if let Some((fail, _)) = &failed {
        self.note_chain_then(fail, arm.span, expr.span);
      }
      failed_guards.extend(failed);
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
      self.maybe_init_locals = maybe_init_before.clone();
      self.maybe_init_by_loop = by_loop_before.clone();
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
      self.arm_frames.push((split, merge, begin, end));
      self.visit_expr(arm.body);
      self.arm_frames.pop();
      self.bind_deferred_arm(arm.body);
      if live {
        self.live_tuples.pop();
      }
      if let Some(ExitFrame::Branch { binds: b, .. }) = self.exit_frames.last_mut() {
        b.clear();
      }
      let body_ids = body_lo..*self.unique_id;
      uninit_ends.push((Some(arm.body), std::mem::take(&mut self.uninit_locals), std::mem::take(&mut self.maybe_init_locals)));
      by_loop_ends.extend(std::mem::take(&mut self.maybe_init_by_loop));
      self.inside_branch = prev_inside_branch;

      all_pat_decls = &all_pat_decls | &pat_decls;
      let mut arm_decls: HashSet<ResourceAccessPoint> = &pat_decls | &get_decl_of_expr(arm.body, &self.tcx, &self.raps);
      // Everything declared in the arm, also inside an inline
      // construct nested in it (`Some(x) => { if let Some(y) =
      // b { … } }`, #501), exists only in the arm (the nested
      // construct ends what it declares itself). A name an
      // earlier, out-of-scope variable used shares its column
      // (`{ let r = &a; … }` before `0 => { let r = &a; … }`), so
      // only the arm's code shows it's declared there (#927).
      let arm_inner = &self.raps_declared_since(arm_hash_lo, &uninit_before) | &bindings_within(arm.body, &self.tcx, &self.raps);
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

      self.match_arm_borrow_ends(cx, arm, or_end_events, (callback_events, pat_id_lo..body_lo), &mut branch_e_data);
      for (line, ev) in fail_ends {
        if inline {
          self.add_external_event(line, ev);
        } else {
          let at = branch_e_data.iter().rposition(|(l, _)| *l <= line).map_or(0, |i| i + 1);
          branch_e_data.insert(at, (line, ev));
        }
      }

      // add gos events, except in an arm that always leaves at
      // an exit that drops them (#630)
      // (A lone arm that always leaves the function drops them there
      // too, #1004.)
      let arm_left = self.left_at_exits((!inline || leaves_fn || leaves_branch) && self.ends_at_exits(arm.body, true), &arm_decls);
      for r in in_decl_order(&arm_decls).into_iter().filter(|v| needs_scope_end(v) && !arm_scoped.contains(v) && !arm_left.contains(v)) {
        let id = *self.unique_id;
        *self.unique_id += 1;
        let gos = ExternalEvent::GoOutOfScope { ro: r.clone(), id, early: None };
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
        // The arrows binding the arm's value to a `let` (`rhs_arm_ids`)
        // are drawn from the arm's own line map. Take them out of the
        // global one: with another arrow on a one-line match's line
        // (`let p = P { a: match n { 0 => d, _ => e }, b: f };`, #1256)
        // they were drawn twice.
        if let Some((lo, hi)) = self.rhs_arm_ids.get(&arm.body.hir_id).copied() {
          let in_arm: HashSet<usize> = branch_e_data.iter().map(|(_, e)| e.get_id()).filter(|id| (lo..hi).contains(id)).collect();
          for evs in self.event_line_map.values_mut() {
            evs.retain(|e| !in_arm.contains(&e.get_id()));
          }
        }
        let branch_line_map = create_line_map(&branch_e_data);
        // An arm of type `!` (`_ => unreachable!()`) never
        // reaches the merge (#296).
        let (diverges, exits, break_carries) = self.branch_divergence(BranchBody::Expr(arm.body), &branch_e_data, &arm_decls);
        branch_data.push(ExtBranchData { source_span: self.arm_span(arm.span), e_data: branch_e_data, line_map: branch_line_map, decl_vars: arm_decls, diverges, exits, break_carries });
      }
    }
    self.arm_borrows_end(arm_borrows);
    #[expect(clippy::disallowed_methods, reason = "collected into a set; order is lost")]
    let known: HashSet<HirId> = uninit_before.keys().chain(maybe_init_before.iter()).copied().collect();
    self.maybe_init_by_loop = by_loop_ends;
    let maybe_init = self.join_uninit_locals(uninit_ends, &known);
    self.exit_frames.pop();
    self.in_scope.truncate(entry_len);
    // Single-arm: events already routed through
    // add_external_event during the loop, nothing else
    // to do — bail out of the Normal arm before the
    // multi-arm Branch construction below. An exit in the
    // lone arm isn't drawn: the arm isn't a column of its
    // own (#410).
    if inline {
        // Unless it always leaves the function (#1004).
        if leaves_fn {
          self.keep_inline_exit_drops(match_lo..*self.unique_id, expr);
        } else if !leaves_branch {
          self.discard_exit_drops(match_lo..*self.unique_id);
        }
        return;
    }
    // Variables introduced by an arm's pattern only exist
    // inside that arm — strip them out so they don't get a
    // branched timeline of their own.
    liveness = &liveness - &all_pat_decls;
    let held_before = self.deferred_cols(&maybe_init_before);
    self.add_match_branch(expr, (split, merge), liveness, branch_data, BranchType::Match(b_ty_names, b_slices), (maybe_init, held_before));
  }

  /// A `match` arm's tuple-like pattern (`(a, b) =>`, `Ok(x) =>`):
  ///
  /// (<pat>, <pat>, ..) => <expr>
  /// First need to annotate events that occur between parents (the variables being matched upon)
  /// and the pattern bindings in each arm.
  ///
  /// `parents.len() == pat_list.len()` is the
  /// "guard-was-a-tuple-literal" shape (`match (a, b)
  /// { (x, y) => … }`): each inner pattern maps to its
  /// own parent expression. Otherwise the guard is a
  /// single tuple-/enum-typed scrutinee and every
  /// inner pattern destructures out of it (`match
  /// pair { (x, y) => … }`, `match res { Ok(a, b, c)
  /// => … }`); use the single parent for all inner
  /// pattern slots. Without that fallback, `parents[i]`
  /// for i>0 panics with an index-out-of-bounds.
  fn match_arm_tuple_pat(&mut self, cx: MatchArmCtx<'_, 'tcx>, pat_list: &'tcx [Pat<'tcx>], out: &mut MatchArmPat) {
    let MatchArmCtx { guard_expr, typeck_res, parents, parents_ty, scrutinee_moves, tuple_name, or_conflicts, arm_pat, pat_line, end, inline } = cx;
    let MatchArmPat { pat_decls, member_binds, branch_e_data, callback_events, arm_moved, or_events, or_end_events } = out;
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
    // Each sub-pattern, the parent it destructures, and the
    // expression that is (the scrutinee, or its element).
    let mut subs: Vec<(&'tcx rustc_hir::Pat<'tcx>, ResourceTy, rustc_middle::ty::Ty<'tcx>, &'tcx Expr<'tcx>)> = Vec::new();
    // Whether each sub-pattern destructures a part of a
    // variable (parallel to `subs`).
    let mut sub_is_part: Vec<bool> = Vec::new();
    if matches!(arm_pat.kind, PatKind::Tuple(..) | PatKind::TupleStruct(..)) && self.literal_elems(guard_expr).is_some() {
      // `match (a, (b, c)) { (p, (x, _)) => … }`: each
      // sub-pattern with its element, through nested tuple
      // literals and `..` (#627); also `Some(x)` on `Some(f)`
      // (#807).
      subs = self.literal_pat_parents(arm_pat, guard_expr);
      sub_is_part = vec![false; subs.len()];
    } else {
      for (i, p) in pat_list.iter().enumerate() {
        let parent_idx = if same_arity { i } else { 0 };
        let (parent, parent_ty) = match part_parents.as_ref().and_then(|v| v[i].1.clone()) {
          Some(pp) => pp,
          None => (parents[parent_idx].clone(), parents_ty[parent_idx]),
        };
        let scrutinee = match self.literal_elems(guard_expr) {
          Some(fields) if same_arity => &fields[parent_idx],
          _ => guard_expr,
        };
        subs.push((p, parent, parent_ty, scrutinee));
        sub_is_part.push(part_parents.as_ref().is_some_and(|v| v[i].1.is_some()));
      }
    }
    for (sub_idx, (p, parent, parent_ty, scrutinee)) in subs.into_iter().enumerate() {
      let mut associated_ro = Vec::new();
      self.get_dec_of_pat_parts(p, &typeck_res, &parent, parent_ty, end, & mut associated_ro);
      // `(p, q)` on `pair = (&c, &d)` copies the references
      // in `pair.0` / `pair.1`: `p` borrows what `pair.0`
      // borrows, as in `let (p, q) = pair;` (#756).
      if sub_is_part[sub_idx] && parent_ty.is_ref() {
        for (r, _, _, _) in associated_ro.iter() {
          self.adopt_part_borrow(r.name(), &parent.name());
        }
      }
      // A `ref` binding out of `(x, y)` borrows the tuple's
      // element, not x (#580).
      for (r, e, from, _) in associated_ro.iter_mut() {
        *from = self.retarget_tuple_borrow(guard_expr, r, e, from.clone());
        // `(s, n) => …` on `match (&t, 1)`: s borrows t (#1235).
        if let Some(b) = self.addr_of_scrutinee_borrow(scrutinee, p, r, e, from) {
          *e = b;
        }
      }
      let or_evs = self.or_conflict_events(&or_conflicts, &mut associated_ro, pat_line, &scrutinee_moves, &tuple_name, end, false);
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
        let mut ext_ev = self.ext_ev_of_evt(e.clone(), ResourceTy::Value(to_ro.clone()), from.clone(), id, is_partial);
        self.arm_part_borrow(guard_expr, arm_pat, to_ro, from, &mut ext_ev);
        // A reference copied out of a value that holds a
        // borrow also borrows what that value borrows (#391).
        pending_borrows.extend(self.pattern_ref_copy_borrow(to_ro, e, scrutinee));
        // add_external_event gives a struct's fields their
        // part of a whole (non-partial) move; otherwise
        // that's done below.
        if matches!(e, Evt::Move | Evt::Copy) && (!inline || is_partial) {
          member_binds.push(to_ro.clone());
        }
        let pat_line = self.pattern_loans.get(to_ro.hash()).map_or(pat_line, |l| l.line);
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

  /// A `match` arm's pattern that is not tuple-like (`<expr> => <expr>`,
  /// just a singleton variable, or a struct pattern).
  fn match_arm_singleton_pat(&mut self, cx: MatchArmCtx<'_, 'tcx>, out: &mut MatchArmPat) {
    let MatchArmCtx { guard_expr, typeck_res, parents, parents_ty, scrutinee_moves, tuple_name, or_conflicts, arm_pat, pat_line, end, inline } = cx;
    let MatchArmPat { pat_decls, member_binds, branch_e_data, callback_events, arm_moved, or_events, or_end_events } = out;
    let mut pending_borrows: Vec<ExternalEvent> = Vec::new();
    for i in 0..parents.len() {
      let mut associated_ro = Vec::new();
      let mut slot_borrows = BTreeMap::new();
      // `P { a, b } => …` on a struct variable: `a` / `b`
      // come from `p.a` / `p.b` (#326).
      self.get_dec_of_pat_parts(arm_pat, &typeck_res, &parents[i], parents_ty[i], end, & mut associated_ro);
      // A binding by reference into a call's result borrows what the
      // call's reference points into, not the call (#1222).
      if parents.len() == 1 {
        for (r, e, from, _) in associated_ro.iter_mut() {
          *from = self.retarget_tuple_borrow(guard_expr, r, e, from.clone());
        }
      }
      // `s => …` on `match &t`: s is bound to the reference and borrows
      // t, as in `let s = &t;` (#1235).
      if parents.len() == 1 {
        for (r, e, from, _) in associated_ro.iter_mut() {
          // A single-arm match can bind the reference produced by its
          // scrutinee directly. MIR borrows the slot before the arm begins.
          let loan = inline.then(|| self.loan_at_span(guard_expr.span)).flatten()
            .filter(|l| from.is_ref() && l.through.is_none() && l.lender == from.real_name()
              && l.direct_holders.iter().any(|h| h == r.name()));
          if let Some(l) = loan {
            if let Some(rd) = self.borrow_map.get_mut(r.name()) {
              rd.borrow_span = Some(guard_expr.span);
              rd.lender = from.clone();
              rd.ref_mutability = l.kind == "mut";
              slot_borrows.insert(*r.hash(), l.line);
              self.ref_var_borrows.insert(guard_expr.span);
              *e = if rd.ref_mutability { Evt::MBorrow } else { Evt::SBorrow };
              // As `addr_of_scrutinee_borrow` records for a `match &r { s => .. }`.
              self.pattern_loans.insert(*r.hash(), l);
              self.approx_notes.push(ApproxNote::PatternReference { holder: *r.hash() });
            }
          } else if let Some(b) = self.addr_of_scrutinee_borrow(guard_expr, arm_pat, r, e, from) {
            *e = b;
          }
        }
      }
      let or_evs = self.or_conflict_events(&or_conflicts, &mut associated_ro, pat_line, &scrutinee_moves, &tuple_name, end, false);
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
        let mut ext_ev = self.ext_ev_of_evt(e.clone(), ResourceTy::Value(to_ro.clone()), from.clone(), id, partial.unwrap_or(false));
        self.arm_part_borrow(guard_expr, arm_pat, to_ro, from, &mut ext_ev);
        let scrutinee = match self.literal_elems(guard_expr) {
          Some(fields) if fields.len() == parents.len() => &fields[i],
          _ => guard_expr,
        };
        pending_borrows.extend(self.pattern_ref_copy_borrow(to_ro, e, scrutinee));
        if matches!(e, Evt::Move | Evt::Copy) && !inline {
          member_binds.push(to_ro.clone());
        }
        // `x => …` on `match *b`, with b a Box drawn as its payload's
        // part columns: the whole payload moves out, parts and all
        // (#898); x's own parts are bound below (`member_binds`). (An
        // inline arm's event does this in `add_external_event`. These
        // join the arm with the pattern's other events; see the drain
        // after the arm's pattern.)
        let box_payload = match (&ext_ev, inline) {
          (ExternalEvent::Move { from, is_partial: false, .. }, false) => box_payload_of(from).map(|b| b.name().clone()),
          _ => None,
        };
        let pat_line = self.pattern_loans.get(to_ro.hash()).map_or(pat_line, |l| l.line);
        if inline {
          let row = match (&mut ext_ev, slot_borrows.get(to_ro.hash())) {
            (ExternalEvent::MutableBorrow { of_ref_var, .. } | ExternalEvent::StaticBorrow { of_ref_var, .. }, Some(&line)) => {
              *of_ref_var = true;
              line
            }
            _ => pat_line,
          };
          self.add_external_event(row, ext_ev);
        } else {
          branch_e_data.push((pat_line, ext_ev));
        }
        if let Some(b) = box_payload {
          self.move_struct_fields_away(pat_line, &b);
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

  /// A `match` arm's guard (`pat if cond =>`): its events join the arm's
  /// column, and its bindings are declared in the arm. Returns what the
  /// later arms start with when it fails (see `visit_match_guard`).
  fn match_arm_guard(&mut self, arm: &'tcx rustc_hir::Arm<'tcx>, inline: bool, pat_decls: &mut HashSet<ResourceAccessPoint>, branch_e_data: &mut Vec<(usize, ExternalEvent)>) -> Option<(ChainFail, FailPath)> {
    // Match guard: `pat if cond =>` / `pat if let p = e =>`.
    // The guard runs after the pattern matched and before
    // the body, and only then; if it's false, matching goes
    // on with the later arms. We show its events in this
    // arm's own column, next to the pattern's bindings:
    // "when this arm is taken, this happened" is always true.
    // The guard also runs when the pattern matched but the
    // guard fails and a later arm is taken: what it did that
    // lasts, or lent and got back, is returned for the later
    // arms to start with (#247, #248).
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
      let failed = self.visit_match_guard(guard, &outer, &mut guard_decls, span_end_line(&arm.span, &self.tcx));
      if !inline {
        branch_e_data.extend(self.preprocessed_events.drain(first_guard_ev..));
      }
      pat_decls.extend(guard_decls);
      return Some(failed);
    }
    None
  }

  /// The end of the borrows an arm's pattern takes: an or-pattern's
  /// references (#565) and the bindings that borrow from the scrutinee.
  /// `pat_ids`: the ids of the events the arm's pattern and guard made.
  fn match_arm_borrow_ends(&mut self, cx: MatchArmCtx<'_, 'tcx>, arm: &'tcx rustc_hir::Arm<'tcx>, or_end_events: Vec<(usize, ExternalEvent)>, (callback_events, pat_ids): (Vec<(ResourceTy, ResourceTy, Evt)>, std::ops::Range<usize>), branch_e_data: &mut Vec<(usize, ExternalEvent)>) {
    let MatchArmCtx { pat_line, end, inline, .. } = cx;
    // The end of an or-pattern reference's possible borrows
    // (#565), where the borrow checker ends them: at the
    // reference's last use, which can be before the arm ends.
    for (line, ev) in or_end_events {
      // Its borrow is drawn by these events, not `emit_borrow_ends`.
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
    // `let r = match &o { Some(x) => x, None => &t };` (#1327): the
    // arm's value is a reference that is (or borrows through) its
    // binding, so the loan goes on in what the `match` gives, past the
    // arm.
    let value_uses_binding = branch_value_expr(arm.body).is_some_and(|v|
      self.tcx.typeck(v.hir_id.owner).expr_ty_opt(v).is_some_and(|t| t.is_ref())
        && self.mentions_local_declared_in(v, arm.pat.span));
    // The early ends placed so far: the next one follows them, so they
    // keep the pattern's order.
    let mut early: Vec<usize> = Vec::new();
    for (to, from, e) in callback_events {
      let name = from.real_name();
      // A part of a whole the scrutinee borrows (`match &mut t { (a, b)
      // => … }`, `arm_part_borrow`): that one loan ends at the last use
      // of any of the arm's bindings into it (#1215).
      let whole_loan = self.borrow_map.get(&name).and_then(|rd| rd.borrow_span);
      // A binding by reference into what a call's returned reference
      // points to, tied to several arguments (`Some(t)` on `pick(&a,
      // &b)`), was given its lenders after the pattern was read
      // (`pattern_call_ref_borrows`, #1368): it gives the borrow back
      // to each of them.
      let lenders: Vec<ResourceTy> = match (&to, self.borrow_map.get(&name)) {
        (ResourceTy::Anonymous, Some(rd)) if matches!(rd.lender, ResourceTy::Value(_) | ResourceTy::Deref(_)) => rd.all_lenders(),
        _ => vec![to.clone()],
      };
      // The borrow ends where the borrow checker ends it, at
      // the binding's last use (#599): the end of the region
      // of the loan MIR issues for the binding on the pattern's
      // lines. A binding that reborrows through a shared
      // reference (`(x, _)` on `o: &(String, String)`) has no
      // loan of its own; its borrow lasts as long as it is
      // live. Not the loan of the binding itself that a guard takes
      // (`Some(t) if t.len() > 1`: the guard sees t through a reference
      // to it, also named t), which ends with the guard.
      let pattern_loan = from.extract_rap().and_then(|r| self.pattern_loans.get(r.hash()));
      let region_end = if let Some(loan) = pattern_loan {
        // A single dot can represent only an endpoint common to the paths.
        let [line] = loan.end_lines.as_slice() else { continue };
        Some(*line)
      } else {
        mir_b_data.iter()
        .filter(|m| (m.assigned_place.as_deref() == Some(name.as_str()) && m.borrowed_place.as_deref() != Some(name.as_str())
            && (pat_line..=pat_end).contains(&m.region.0))
          || whole_loan == Some(m.span))
        .map(|m| m.region.1)
        .max()
      };
      // A loan the arm's value carries out of the `match` ends where
      // that value's last use ends it, after the branches join: as for
      // `if let Some(x) = &o { x } else { &t }`, `emit_borrow_ends`
      // ends it, not the arm.
      if (value_uses_binding || pattern_loan.is_some()) && region_end.is_none_or(|l| l > end) {
        continue;
      }
      let slot_loan = self.borrow_map.get(&name).and_then(|rd| rd.borrow_span)
        .filter(|span| self.ref_var_borrows.contains(span)).and_then(|span| self.loan_at_span(span));
      self.borrow_map.remove(&name);
      // A binding the arm never uses (`Some(x) => drop(o)` on `match
      // &o`) is live nowhere: rustc's borrow through it ends where the
      // pattern binds it, which is why the arm may move `o` (#1503). Its
      // end goes right after the pattern's events, before the body's on
      // a one-line arm. (A name MIR doesn't know keeps the arm's end, as
      // does a value holding the borrow, `z => {}` binding a tuple with a
      // `&mut` in it: its column goes on to its drop.)
      let live_end = slot_loan.and_then(|l| match l.end_lines.as_slice() { [line] => Some(*line), _ => None }).or(region_end)
        .or_else(|| self.last_live_line(&name, (pat_line, end)));
      let unused = live_end.is_none() && from.extract_rap().is_some_and(|r| r.is_ref()) && self.is_body_local(&name);
      let ends_at = live_end
        .filter(|l| (pat_line..=end).contains(l))
        .unwrap_or(if unused { pat_end } else { end });
      // When the scrutinee is itself a reference (`match o`,
      // `o: &Option<String>`), the binding reborrows through
      // it, so `o`'s borrow lasts at least until here (#372).
      if to.is_ref() {
        let floor = self.borrow_floor.entry(to.real_name()).or_insert(ends_at);
        *floor = (*floor).max(ends_at);
      }
      for to in lenders {
        let id = *self.unique_id;
        *self.unique_id += 1;
        let mut ext_ev = self.ext_ev_of_evt(e.clone(), to, from.clone(), id, false);
        if slot_loan.is_some() {
          if let ExternalEvent::MutableDie { of_ref_var, .. } | ExternalEvent::StaticDie { of_ref_var, .. } = &mut ext_ev {
            *of_ref_var = true;
          }
        }
        // An unused binding's end follows the pattern's (and guard's)
        // events, before the body's on the same line.
        let placed = |i: usize| pat_ids.contains(&i) || early.contains(&i);
        let after_pat = |evs: &[(usize, ExternalEvent)]| evs.iter().rposition(|(l, e)| *l == ends_at && placed(e.get_id())).map(|i| i + 1);
        if inline {
          self.add_external_event(ends_at, ext_ev);
          let at = if unused { after_pat(self.preprocessed_events) } else { None };
          if let Some(at) = at.filter(|_| self.preprocessed_events.last().is_some_and(|(_, e)| e.get_id() == id)) {
            let ev = self.preprocessed_events.pop().expect("just added");
            self.preprocessed_events.insert(at, ev);
            // And among the arrows stacked on its line.
            if let Some(evs) = self.event_line_map.get_mut(&ends_at) {
              if let (Some(k), Some(lo)) = (evs.iter().position(|e| e.get_id() == id), evs.iter().rposition(|e| placed(e.get_id()))) {
                let e = evs.remove(k);
                evs.insert((lo + 1).min(evs.len()), e);
              }
            }
          }
        } else {
          // After the arm's events up to that line.
          let at = if unused { after_pat(branch_e_data) } else { None }
            .unwrap_or_else(|| branch_e_data.iter().rposition(|(l, _)| *l <= ends_at).map_or(0, |i| i + 1));
          branch_e_data.insert(at, (ends_at, ext_ev));
        }
        if unused {
          early.push(id);
        }
      }
    }
  }

  /// The Branch event of a multi-arm `match`, once every arm is drawn.
  /// `maybe_init`: as for the Branch event; `held_before`: the `let x;`
  /// locals that may hold a value as the `match` starts.
  fn add_match_branch(&mut self, expr: &'tcx Expr<'tcx>, (split, merge): (usize, usize), liveness: HashSet<ResourceAccessPoint>, branch_data: Vec<ExtBranchData>, branch_type: BranchType, (maybe_init, held_before): (HashSet<ResourceAccessPoint>, HashSet<ResourceAccessPoint>)) {
    // add branch event. merge_point is the closing-brace
    // line of the match — same convention as If, where
    // merge_point is the line of the else-block's closing
    // `}`. Previously this used `merge - 1` (with a stale
    // TODO marker), which landed the join dot one row above
    // the `};`, on the last arm's body line.
    if !branch_data.is_empty() && branch_data.iter().all(|b| b.diverges) && self.at_fn_level(expr) {
      self.note_all_paths_exit(expr);
    }
    self.note_exit_scope(expr);
    let loop_assigned = self.loop_assigned_now(&liveness);
    let reaching: Vec<&[(usize, ExternalEvent)]> = branch_data.iter()
      .filter(|b| !b.diverges).map(|b| b.e_data.as_slice()).collect();
    let maybe_held = self.maybe_held_after(&liveness, &reaching, &maybe_init, &held_before, |name| self.rap_ownership_now(name));
    if let ExprKind::Match(scrutinee, ..) = expr.kind {
      self.event_spans.insert(*self.unique_id, scrutinee.span);
    }
    if let ExprKind::Match(scrutinee, arms, _) = expr.kind {
      self.arm_patterns.insert(*self.unique_id, arms.iter().enumerate().map(|(i,arm)|(i,arm.pat.span)).collect());
      self.branch_conditions.insert(*self.unique_id, scrutinee.span);
    }
    self.add_external_event(split, ExternalEvent::Branch {
      join_span: Some(expr.span),
      live_vars: liveness,
      on_exhaustion: Default::default(),
      skipped_holds: Default::default(),
      branches: branch_data,
      branch_type,
      split_point: split,
      merge_point: merge,
      maybe_init,
      loop_assigned,
      maybe_held,
      id: *self.unique_id });
    *self.unique_id += 1;
  }

  /// The `MatchSource::ForLoopDesugar` case of `visit_match`: a `for` loop's
  /// desugaring, drawn as one iteration (see the comment inside).
  fn visit_for_desugar(&mut self, expr: &'tcx Expr<'tcx>, guard_expr: &'tcx Expr<'tcx>, arms: &'tcx [rustc_hir::Arm<'tcx>]) {
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
        let head_temp = self.note_for_head_temporary(head_arg, guard_expr, expr);
        // Iterand event: shape of head_arg dictates the arrow.
        //   &v       ⇒ SBorrow from v
        //   &mut v   ⇒ MBorrow from v
        //   v (owned, non-Copy) ⇒ Move from v
        //   anything else (Range, call result, …) ⇒ Bind (below)
        let for_line = self.tcx.sess.source_map()
          .lookup_char_pos(expr.span.lo()).line;
        let head_rty = get_rap(head_arg, &self.tcx, &self.raps);
        // A variable, or a field with a column of its own (`for n in
        // t.names`, `self.names`, `pair.0`; #1164).
        let place_head = match head_arg.kind {
          ExprKind::Path(_) => true,
          ExprKind::Field(..) => matches!(head_rty, ResourceTy::Value(_)),
          _ => false,
        };
        // `for x in r` with `r: &mut Vec<_>`: `IntoIterator::into_iter`
        // takes a generic `Self`, so no reborrow is inserted and r is
        // moved into the loop's iterator (using r after the loop is
        // E0382; #1129). A shared `&` head is copied.
        let head_moves_ref = place_head && {
          let typeck = self.tcx.typeck(head_arg.hir_id.owner);
          typeck.node_type(head_arg.hir_id).ref_mutability() == Some(Mutability::Mut)
            && typeck.expr_adjustments(head_arg).is_empty()
            && head_rty.extract_rap().is_some_and(|r| r.is_mutref())
        };
        let iterand_evt = match head_arg.kind {
          ExprKind::AddrOf(_, Mutability::Not, _) => Some(Evt::SBorrow),
          ExprKind::AddrOf(_, Mutability::Mut, _) => Some(Evt::MBorrow),
          _ if head_moves_ref => Some(Evt::Move),
          _ if place_head => {
            let head_ty = self.tcx.typeck(head_arg.hir_id.owner)
              .node_type(head_arg.hir_id);
            let is_copy = self.tcx.type_is_copy_modulo_regions(
              typing_env(self.tcx, head_arg.hir_id.owner),
              head_ty);
            if head_ty.is_ref() { None }   // a shared ref, copied in
            else if is_copy { None }       // Range / Copy iterand
            else { Some(Evt::Move) }
          }
          _ => None,
        };
        // `for (i, a) in v.iter().enumerate()`: the loop's iterator
        // borrows `v` until the loop ends (#868).
        let loop_loans = self.keep_loop_head_loans(head_arg);
        // `a.iter_mut().zip(b)` with `b: &mut Vec<_>` moves b into the
        // loop's iterator, which writes through it until the loop ends
        // (#1076).
        let mut moved_ref_args: Vec<ResourceAccessPoint> = self.loop_head_ref_args(head_arg)
          .into_iter().filter(|r| r.is_mutref()).collect();
        // The head itself (#1129).
        if head_moves_ref {
          moved_ref_args.extend(head_rty.extract_rap().cloned());
        }
        // Their columns end at the move (#1365): no write through a loop
        // variable, and no borrow end, is drawn on them after it.
        for r in &moved_ref_args {
          if let Some(rd) = self.borrow_map.get(r.name()) {
            self.loop_moved_refs.insert(r.name().to_owned(), rd.assigned_at);
          }
        }
        // `for x in it` where `it` is already an iterator
        // (`let it = v.iter_mut();`): `IntoIterator::into_iter`
        // returns its argument unchanged, so the loop *moves*
        // `it` into itself and each `x` is something `next()`
        // hands out, not something `it` owns or lends (#417).
        // Compare the head's type with the type of the
        // `into_iter(head)` call: they are equal only for an
        // iterator.
        let head_is_iterator = place_head && !head_moves_ref
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
          && place_head
          && self.needs_drop(
            self.tcx.typeck(guard_expr.hir_id.owner).node_type(guard_expr.hir_id),
            guard_expr.hir_id.owner);

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
          let events_at = self.preprocessed_events.len();
          self.exit_frames.push(ExitFrame::Loop { id: outer_arm.body.hir_id, len: entry_len, head_from, events_at });
        }
        // The variable moved into the loop as its iterator, whose
        // drop is stated on its column (#482).
        let mut iterator_head: Option<ResourceAccessPoint> = None;
        // The head's `&v` / `&mut v` when only the loop's iterator holds
        // it (#885): the lender, whether the loan is mutable, and the id
        // of the borrow on the `for` line.
        let mut head_loan: Option<(ResourceTy, bool, usize)> = None;
        // The loop variable's `loop_ref_into` entry, and the one it
        // shadows (a loop nested in another with the same variable).
        let mut loop_ref_prev: Vec<(String, Option<Vec<ResourceAccessPoint>>)> = Vec::new();
        // `for x in v.iter_mut()`: the variable that lends x (v), and
        // whether mutably (#1347).
        let mut call_lender: Option<(ResourceTy, Evt)> = None;
        // `for r in [&e, &g]`: what the literal's references borrow (#1433).
        let mut literal_lenders: Option<Vec<ResourceTy>> = None;
        if let PatKind::Binding(_, pat_hirid, pat_ident, None) = user_pat.kind {
          let pat_name = binding_name(&self.tcx, pat_hirid);
          let pat_ty = self.tcx.typeck(user_pat.hir_id.owner)
            .pat_ty(user_pat);
            let pointee_copy = self.ref_pointee_is_copy(pat_ty, user_pat.hir_id.owner);
          if pat_ty.is_ref() && (head_is_iterator || head_moves_ref) {
            // A reference an iterator hands out: its lender isn't
            // known here, and nothing is "returned" to the
            // iterator (or to anything) when it ends. For `for x in
            // r` with r a `&mut` moved in (#1129), a write through x
            // goes into the resource r points to (#1046).
            if head_moves_ref && pat_ty.ref_mutability() == Some(Mutability::Mut) {
              if let Some(roots) = self.loop_item_roots(head_arg) {
                let prev = self.loop_ref_into.insert(pat_name.clone(), roots);
                loop_ref_prev.push((pat_name.clone(), prev));
              }
            }
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
          } else if let Some((holder, lenders)) = self.moved_ref_holder(head_arg, place_head, &iterand_evt) {
            // `for r in refs` / `for r in refs.into_iter()` with `refs:
            // Vec<&mut String>` (#1389): the loop moves refs into its
            // iterator, and each r is one of the references refs held. r
            // borrows what they borrow (a, b), not refs, and gives that
            // back where the loop ends (the iterator, which holds the
            // rest, keeps the loans live until then). refs has nothing
            // left to end a borrow on after the move.
            let (first, rest) = lenders.split_first().map(|(f, r)| (f.clone(), r.to_vec())).unwrap();
            self.add_ref(
              pat_name.clone(),
              bool_of_mut(pat_ty.ref_mutability().unwrap()),
              false,
              for_line,
              first,
              std::collections::VecDeque::new(),
              self.current_scope,
              false,
              pointee_copy,
            );
            if let Some(rd) = self.borrow_map.get_mut(&pat_name) {
              rd.co_lenders = rest;
            }
            if let Some(rd) = self.borrow_map.get(&holder) {
              self.loop_moved_refs.insert(holder, rd.assigned_at);
            }
          } else if let Some(lenders) = pat_ty.is_ref().then(|| self.literal_head_lenders(head_arg)).flatten() {
            // `for r in [&e, &g]` / `vec![&e, &g]` (#1433): each r is one
            // of the literal's references, which the loop's iterator
            // holds until the loop ends. r borrows what they borrow, as
            // for `for r in refs` (#1389), drawn from each lender.
            let (first, rest) = lenders.split_first().map(|(f, r)| (f.clone(), r.to_vec())).unwrap();
            self.add_ref(
              pat_name.clone(),
              bool_of_mut(pat_ty.ref_mutability().unwrap()),
              false,
              for_line,
              first,
              std::collections::VecDeque::new(),
              self.current_scope,
              false,
              pointee_copy,
            );
            if let Some(rd) = self.borrow_map.get_mut(&pat_name) {
              rd.co_lenders = rest;
            }
            literal_lenders = Some(lenders);
          } else if pat_ty.is_ref() {
            // `for x in v.iter_mut()`: the loan came from `v`;
            // `iter_mut` is the method that lent it, not the
            // variable x hands the borrow back to (#749).
            let lender = match head_arg.kind {
              // Only an owner lends by autoref: a reference
              // receiver (`text.split_whitespace()` with
              // `text: &str`) is copied, so nothing is lent from it.
              ExprKind::MethodCall(_, recv, _, _)
                if !self.tcx.typeck(recv.hir_id.owner).node_type(recv.hir_id).is_ref() =>
              {
                match get_rap(recv, &self.tcx, &self.raps) {
                  r @ ResourceTy::Value(_) => r,
                  _ => head_rty.clone(),
                }
              }
              _ => head_rty.clone(),
            };
            // `for e in v` / `v.into_iter()` with `v: Vec<&str>` (or a
            // `[&str; 2]` array, copied in): the loop takes v's elements by
            // value, and each e is one of the references v stored. e
            // doesn't borrow v, and gives nothing back to it (#1519). (One
            // whose lenders are drawn is `moved_ref_holder`'s, above.)
            let typeck = self.tcx.typeck(head_arg.hir_id.owner);
            let by_value = |e: &'tcx Expr<'tcx>| !typeck.node_type(e.hir_id).is_ref()
              && !typeck.expr_adjustments(e).iter().any(|a| matches!(a.kind, Adjust::Borrow(_)));
            let consumed = match head_arg.kind {
              ExprKind::Path(_) | ExprKind::Field(..) => place_head && by_value(head_arg),
              ExprKind::MethodCall(_, recv, _, _) => matches!(recv.kind, ExprKind::Path(_) | ExprKind::Field(..))
                && by_value(recv) && typeck.type_dependent_def_id(head_arg.hir_id)
                  .is_some_and(|d| self.tcx.item_name(d).as_str() == "into_iter"),
              _ => false,
            };
            // `for x in v.iter().rev()`: the receiver of the last call
            // is the previous call's result, not a variable, and `x`
            // isn't a borrow of a method (it said "Return ... from x
            // to iter"). What x points into isn't known here; the
            // loop's own borrow of `v` is drawn on v (#868).
            let lender = match lender {
              ResourceTy::Value(r) if r.is_fn() => {
                self.unlent_refs.insert(pat_name.clone());
                ResourceTy::Anonymous
              }
              ResourceTy::Value(_) if consumed => {
                self.unlent_refs.insert(pat_name.clone());
                ResourceTy::Anonymous
              }
              l => l,
            };
            // `for e in v.iter_mut()` with v a `&mut Vec<_>` (a
            // parameter, or a local reference), `for e in
            // v.iter_mut().rev()`: e has no lender, but a write through
            // it goes into v's resource, or the resource v points to
            // (#1046).
            if matches!(lender, ResourceTy::Anonymous) && pat_ty.ref_mutability() == Some(Mutability::Mut) {
              if let Some(roots) = self.loop_item_roots(head_arg) {
                let prev = self.loop_ref_into.insert(pat_name.clone(), roots);
                loop_ref_prev.push((pat_name.clone(), prev));
              }
            }
            if matches!(head_arg.kind, ExprKind::MethodCall(..)) && matches!(&lender, ResourceTy::Value(r) if !r.is_fn()) {
              let evt = if pat_ty.ref_mutability() == Some(Mutability::Mut) { Evt::MBorrow } else { Evt::SBorrow };
              call_lender = Some((lender.clone(), evt));
            }
            self.add_ref(
              pat_name.clone(),
              bool_of_mut(pat_ty.ref_mutability().unwrap()),
              false,
              for_line,
              lender,
              std::collections::VecDeque::new(),
              self.current_scope,
              false,
              pointee_copy,
            );
          } else {
            let is_copy = self.tcx.type_is_copy_modulo_regions(
              typing_env(self.tcx, user_pat.hir_id.owner),
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
          } else if head_moves_ref {
            // r's reference is moved into the loop's iterator; each x
            // is a reference `next()` hands out (#1129).
            self.add_ev(for_line, Evt::Move, ResourceTy::Anonymous, head_rty.clone(), false);
            self.add_ev(for_line, Evt::Bind, to_rty, ResourceTy::Anonymous, false);
          } else if let Some(e) = iterand_evt {
            // `for n in a` moving a collection of Copy elements
            // (`Vec<i32>`): `a` is moved into the loop's
            // iterator, and each `n` is a copy `next()` hands
            // out, not something moved from `a` (#772). Draw the
            // move on the head and give `n` a plain binding.
            // The copy is drawn as an arrow from `a` into `n` (a move
            // draws an arrow out of its source, #1372), "Copy of each
            // element of a to n". So for `&T` elements (`Vec<&String>`),
            // which are Copy too.
            let elem_copy = matches!(e, Evt::Move) && self.tcx.type_is_copy_modulo_regions(
              typing_env(self.tcx, user_pat.hir_id.owner),
              pat_ty);
            if elem_copy {
              self.add_loop_head_move_ev(for_line, to_rty, head_rty, LoopHead::CollectionCopy);
            } else if iterator_needs_drop {
              self.add_loop_head_move_ev(for_line, to_rty, head_rty, LoopHead::Collection);
            } else {
              self.add_ev(for_line, e, to_rty, head_rty, false);
            }
          } else if let Some(lenders) = literal_lenders.take() {
            let evt = if pat_ty.ref_mutability() == Some(Mutability::Mut) { Evt::MBorrow } else { Evt::SBorrow };
            for l in lenders {
              match l {
                // `[&mut *w]` / `[w]` with w a local `&mut`: a reborrow
                // through w, drawn from w's column (#1330).
                ResourceTy::Deref(r) => self.add_borrow_ev(for_line, evt.clone(), to_rty.clone(), ResourceTy::Value(r), true),
                l => self.add_ev(for_line, evt.clone(), to_rty.clone(), l, false),
              }
            }
          } else if let Some((lender, evt)) = call_lender.take() {
            // `for x in v.iter_mut()` / `v.iter()`: each x is a
            // reference into v, which v lends (through the iterator
            // `iter_mut` returns) as `for x in &mut v` does; x gives
            // the loan back where it ends (#1347).
            self.add_ev(for_line, evt, to_rty, lender, false);
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
          //
          // Each binding that gets (a copy of) a part of each element
          // draws an arrow from the head, as `let (s, k) = pair;` draws
          // one from each field (#1390): "Move of the `.0` of each element
          // of names to s"; the first one also draws the head's move.
          let moved_in = matches!(iterand_evt, Some(Evt::Move)) && !head_is_iterator;
          let borrowed = matches!(iterand_evt, Some(Evt::SBorrow) | Some(Evt::MBorrow))
            && matches!(&head_rty, ResourceTy::Value(r) if !r.is_ref() && !r.is_fn());
          if moved_in || borrowed {
            let holder = if moved_in { self.moved_ref_holder(head_arg, place_head, &iterand_evt) } else { None };
            self.pattern_elem = Some(PatternElem { head: head_rty.clone(), moved: moved_in, source_pending: moved_in, holder, deferred: Vec::new() });
          }
          // `for (a, b) in &pairs` / `&mut pairs`: each reference
          // binding borrows the head (#576).
          if matches!(iterand_evt, Some(Evt::SBorrow) | Some(Evt::MBorrow)) {
            self.pattern_lender = iterand_evt.clone().map(|e| (e, head_rty.clone()));
          }
          // `for (x, y) in [(&e, &g)]`: x and y copy the literal's
          // references (#1433).
          if iterand_evt.is_none() {
            self.pattern_literal = match head_arg.kind {
              ExprKind::Array(elems) if !head_arg.span.from_expansion() => Some(elems),
              _ => vec_macro_elems(self.tcx, head_arg),
            };
          }
          self.register_pattern_bindings_anon(user_pat, for_line);
          self.pattern_lender = None;
          self.pattern_literal = None;
          let pattern_elem = self.pattern_elem.take();
          let head_move_pending = pattern_elem.as_ref().is_none_or(|p| p.source_pending);
          if matches!(iterand_evt, Some(Evt::Move)) && head_move_pending {
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
          // `for (i, e) in v.iter().enumerate()`: as for `for e in
          // v.iter().rev()`, e points into what the head's call
          // borrowed (drawn on v, #868), so nothing is "returned" when
          // e ends: it wasn't drawn as an arrow from e's column to
          // nowhere ("what it borrows from isn't shown"; #1067).
          if matches!(head_arg.kind, ExprKind::MethodCall(..) | ExprKind::Call(..)) {
            let mut decls = HashSet::new();
            collect_pat_bindings(user_pat, &self.tcx, &self.raps, &mut decls);
            #[expect(clippy::disallowed_methods, clippy::iter_over_hash_type, reason = "each binding is inserted into a set on its own")]
            for d in decls.iter() {
              if self.borrow_map.get(d.name()).is_some_and(|rd| matches!(rd.lender, ResourceTy::Anonymous)) {
                self.unlent_refs.insert(d.name().to_owned());
              }
            }
          }
          // `for (i, e) in v.iter_mut().enumerate()`, `for (x, y) in
          // a.iter_mut().zip(b.iter_mut())` with v, a, b `&mut`
          // references or fields through `&mut self`: as for `for e in
          // v.iter_mut()`, a write through e goes into the resource v
          // points to (#1067), and one through y into b's.
          if !head_is_iterator {
            let mut roots = Vec::new();
            self.loop_pat_write_roots(user_pat, head_arg, &mut roots);
            for (name, roots) in roots {
              let unlent = self.borrow_map.get(&name).is_some_and(|d| matches!(d.lender, ResourceTy::Anonymous));
              if unlent {
                let prev = self.loop_ref_into.insert(name.clone(), roots);
                loop_ref_prev.push((name, prev));
              }
            }
          }
          // `for &y in &v`, `for &(a, b) in &pairs`, `for _ in &v`: no
          // loop variable borrows the head (each is a copy, or there is
          // none), but the loop's iterator still borrows `v` until the
          // loop ends (#885). Draw that borrow on `v`; it ends with the
          // loop, below.
          if let (Some(e @ (Evt::SBorrow | Evt::MBorrow)), ResourceTy::Value(r)) = (&iterand_evt, &head_rty) {
            let mut decls = HashSet::new();
            collect_pat_bindings(user_pat, &self.tcx, &self.raps, &mut decls);
            #[expect(clippy::disallowed_methods, reason = "only tested for membership")]
            let borrowed = decls.iter()
              .filter_map(|d| self.borrow_map.get(d.name()))
              .any(|rd| rd.all_lenders().contains(&head_rty));
            if !borrowed && !r.is_ref() && !r.is_fn() {
              let id = *self.unique_id;
              let is_mut = matches!(e, Evt::MBorrow);
              self.add_ev(for_line, e.clone(), ResourceTy::Anonymous, head_rty.clone(), false);
              head_loan = Some((head_rty.clone(), is_mut, id));
            }
          }
          // The copies out of a borrowed head's elements, after its
          // loan (#1390).
          for (line, to, part) in pattern_elem.map(|p| p.deferred).unwrap_or_default() {
            self.add_loop_head_move_ev(line, to, head_rty.clone(), part);
          }
        }

        // Visit body — events flow into the global timeline.
        let before = self.rap_names();
        if let Some(ro) = &iterator_head {
          if let Some(loop_expr) = arms.first().map(|a| a.body) {
            self.loop_iterators.push((loop_expr.hir_id, ro.clone()));
          }
        }
        // The path to a `break` in the body starts here, after the head
        // moved what it iterates over into the loop (#980).
        let len = self.preprocessed_events.len();
        if let (Some(loop_expr), Some(ExitFrame::Loop { id, events_at, .. })) = (arms.first().map(|a| a.body), self.exit_frames.last_mut()) {
          if *id == loop_expr.hir_id {
            *events_at = len;
          }
        }
        // What the loop may come back round to moved (#992), or
        // given a value after a move before it (#1001).
        let head_moved = match arms.first() {
          Some(a) => {
            let moved = self.note_loop_head_moves(a.body, entry_len);
            self.note_loop_head_given(a.body, entry_len, None);
            moved
          }
          None => Vec::new(),
        };
        // The body may run no times or many (#786).
        let inits = self.enter_loop_inits(arms.first().map_or(body_expr.hir_id, |a| a.body.hir_id));
        let body_events_at = self.preprocessed_events.len();
        self.visit_expr(body_expr);
        for (name, prev) in loop_ref_prev.into_iter().rev() {
          match prev {
            Some(prev) => { self.loop_ref_into.insert(name, prev); }
            None => { self.loop_ref_into.remove(&name); }
          }
        }
        if let Some(loop_expr) = arms.first().map(|a| a.body) {
          self.leave_loop_inits(expr, loop_expr.hir_id, inits, true);
        }
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
        // A body that can run to its end goes round again there:
        // `next()` runs on the loop's iterator, so the loan the loop
        // holds is live to the loop's closing brace, even if a
        // `break` or `return` leaves early on another path (where
        // the loan ends; drawn in that path's arm when it has one)
        // (#872). (the loop's own desugared `None => break` is the
        // one `break` of a body without any.)
        let no_break = arms.first().is_some_and(|a| breaks_to(a.body).len() <= 1);
        let reaches_end = !self.expr_diverges(body_expr);
        // `for x in it`: the loop owns the iterator `it` holds a
        // borrow in, until the loop ends, so the lender is
        // borrowed through the whole body and its borrow ends
        // where the loop does, not at the last use of `x`
        // (#722).
        if head_is_iterator && (no_break || reaches_end) {
          if let Some(name) = expr_to_rap_name(head_arg, &self.tcx) {
            if self.borrow_map.contains_key(&name) {
              if self.raps.get(&name).is_some_and(|r| r.rap.holds_borrow()) {
                self.for_moved_holders.insert(name.clone());
              }
              let floor = self.borrow_floor.entry(name).or_insert(body_end);
              *floor = (*floor).max(body_end);
            }
          }
        }
        let mut loop_decls = get_decl_of_expr(body_expr, &self.tcx, &self.raps);
        collect_pat_bindings(user_pat, &self.tcx, &self.raps, &mut loop_decls);
        // Not where every path breaks or continues first (#631).
        let loop_decls = &loop_decls - &self.left_at_exits(self.ends_at_exits(body_expr, false), &loop_decls);
        // The loop's iterator holds the loan the head made
        // (`&mut m`, `&pairs`) until the loop ends, so the
        // lender is borrowed through the whole body, not just
        // to the last use of the binding: `m.len()` after
        // `s`'s last use is still E0499/E0502 (#739), also in a
        // body with a `break` / `return` that doesn't always run
        // (#872).
        let runs_to_the_end = (no_break && !body_returns(body_expr)) || reaches_end;
        let mut pat_decls = HashSet::new();
        collect_pat_bindings(user_pat, &self.tcx, &self.raps, &mut pat_decls);
        // A `&mut` binding the body moves away (`drop(e)`, `take(e)` with a
        // generic parameter: no reborrow) has nothing left after the move:
        // its borrow ends there, and the loop's iterator holds the loan to
        // the loop's end, as for a loop with no binding to name it (#1511).
        // (A move on every path, into a call: one in a branch of the body
        // is not, nor a move into a variable, which takes the loan on.)
        let moved_in_body = |name: &str| self.preprocessed_events[body_events_at..].iter().find_map(|(line, ev)| match ev {
          ExternalEvent::Move { from: ResourceTy::Value(r), to, .. } if r.name() == name && r.is_mutref()
            && matches!(to, ResourceTy::Anonymous | ResourceTy::Value(ResourceAccessPoint::Function(_))) => Some(*line),
          _ => None,
        });
        // (Nor one something borrows through: `let g = id(e);`.)
        #[expect(clippy::disallowed_methods, reason = "`any` over the map: order never observed")]
        let lends = |name: &str| self.borrow_map.values().any(|rd| rd.all_lenders().iter()
          .any(|l| matches!(l, ResourceTy::Value(r) | ResourceTy::Deref(r) if r.name() == name)));
        #[expect(clippy::disallowed_methods, reason = "collected into a sorted map")]
        let moved_at: BTreeMap<String, usize> = pat_decls.iter()
          .filter(|d| !lends(d.name()))
          .filter(|d| self.borrow_map.get(d.name()).is_some_and(|rd| !matches!(rd.lender, ResourceTy::Anonymous)))
          .filter_map(|d| moved_in_body(d.name()).map(|l| (d.name().to_owned(), l)))
          .collect();
        for (name, line) in moved_at.iter() {
          self.for_loop_moved_bindings.push((name.clone(), *line, body_end));
        }
        let moved_decls: BTreeSet<String> = moved_at.into_keys().collect();
        #[expect(clippy::disallowed_methods, reason = "each binding's floor is raised on its own")]
        for decl in pat_decls.iter().filter(|d| runs_to_the_end && !moved_decls.contains(d.name())) {
          let lent = self.borrow_map.get(decl.name())
            .is_some_and(|rd| !matches!(rd.lender, ResourceTy::Anonymous));
          if lent {
            self.for_loop_loans.insert(decl.name().to_owned());
            let floor = self.borrow_floor.entry(decl.name().to_owned()).or_insert(body_end);
            *floor = (*floor).max(body_end);
          }
        }
        // The lenders whose loan a pattern binding gives back where
        // the loop ends (`for a in v.iter()`: a returns it to v).
        #[expect(clippy::disallowed_methods, reason = "only tested for membership")]
        let carried: Vec<ResourceTy> = pat_decls.iter()
          .filter(|d| !moved_decls.contains(d.name()))
          .filter_map(|d| self.borrow_map.get(d.name()))
          .flat_map(|rd| rd.all_lenders())
          .collect();
        // What the pattern leaves of each element (`.1` in `for (f, _)
        // in v`, all of it in `for _ in v`) is dropped at the end of each
        // pass (#1434): stated on the loop's iterator, which hands the
        // elements out, else on a binding of the pattern.
        if let Some(parts) = self.for_pattern_left_parts(user_pat) {
          #[expect(clippy::disallowed_methods, reason = "the first binding in source order is picked below")]
          let mut bindings: Vec<ResourceAccessPoint> = pat_decls.iter().cloned().collect();
          bindings.sort_by_key(|r| *r.hash());
          let on = head_temp.clone().or_else(|| iterator_head.clone()).or_else(|| bindings.into_iter().next());
          if let Some(ro) = on {
            let id = *self.unique_id;
            *self.unique_id += 1;
            self.add_external_event(body_end, ExternalEvent::PassRestDrop { ro, parts, id });
          }
        }
        self.emit_scope_end(loop_decls, body_end);
        // The loan of a reference it moved in lasts until the loop ends (#1076):
        // its lender (a variable with a column) is borrowed by the loop's
        // iterator up to there. The reference itself was moved, so it
        // draws no end (#1365: its end was drawn on its column after the
        // move); a parameter's caller-side lender isn't drawn.
        let mut moved_ref_loans: Vec<(ResourceTy, bool, StaticLender)> = Vec::new();
        for r in moved_ref_args {
          let Some(rd) = self.borrow_map.get(r.name()) else { continue };
          let is_r = |a: &'tcx Expr<'tcx>| matches!(a.kind, ExprKind::Path(_) | ExprKind::Field(..))
            && get_rap(a, &self.tcx, &self.raps).extract_rap().is_some_and(|x| x.hash() == r.hash());
          let call = head_call_taking(head_arg, &is_r, &self.tcx).unwrap_or_else(|| "into_iter".to_owned());
          for l in rd.all_lenders() {
            if matches!(&l, ResourceTy::Value(x) if !x.is_fn() && self.raps.get(x.name()).is_some_and(|d| d.rap.hash() == x.hash()))
              && !moved_ref_loans.iter().any(|(o, _, _)| *o == l) {
              moved_ref_loans.push((l, true, StaticLender::LoopIterator { call: call.clone() }));
            }
          }
        }
        // The loop's iterator (`v.iter().enumerate()`) holds its loans
        // until the loop ends: `v.push(..)` anywhere in the body is
        // E0502 even after the last use of the loop variable (#868).
        // The end says what held the borrow, so the lender's column
        // can say so in between (#871).
        let loop_loans: Vec<(ResourceTy, bool, StaticLender)> = loop_loans.into_iter()
          .filter(|(l, _, _)| !carried.contains(l))
          .map(|(l, m, call)| (l, m, StaticLender::LoopIterator { call }))
          .chain(head_loan.map(|(l, m, lend)| (l, m, StaticLender::LoopHeadBorrow { lend })))
          .chain(moved_ref_loans)
          .collect();
        // A `&mut` binding moved away in the body (#1511) gave its loan to
        // no one: the loop's iterator holds it to the loop's end (`for e in
        // &mut v`, `for e in [&mut a, &mut b]`), as the borrow drawn on the
        // `for` line says.
        let mut moved_binding_loans: Vec<(ResourceTy, bool, StaticLender)> = Vec::new();
        for name in moved_decls.iter() {
          let (Some(rd), Some(to)) = (self.borrow_map.get(name), self.raps.get(name).map(|r| r.rap.to_owned())) else { continue };
          for l in rd.all_lenders() {
            let drawn = matches!(&l, ResourceTy::Value(x) if !x.is_fn() && self.raps.get(x.name()).is_some_and(|d| d.rap.hash() == x.hash()));
            if !drawn || loop_loans.iter().chain(moved_binding_loans.iter()).any(|(o, _, _)| *o == l) {
              continue;
            }
            let lend = self.preprocessed_events.iter().find_map(|(line, ev)| match ev {
              ExternalEvent::MutableBorrow { from, to: ResourceTy::Value(t), id, .. }
                if *line == for_line && *from == l && t.hash() == to.hash() => Some(*id),
              _ => None,
            });
            let static_lender = match lend {
              Some(lend) => StaticLender::LoopHeadBorrow { lend },
              None => StaticLender::LoopIterator { call: "into_iter".to_owned() },
            };
            moved_binding_loans.push((l, true, static_lender));
          }
        }
        let loop_loans = loop_loans.into_iter().chain(moved_binding_loans);
        for (lender, is_mut, static_lender) in loop_loans {
          if !is_mut {
            let head_call = matches!(static_lender, StaticLender::LoopIterator { .. });
            self.for_loop_iter_loans.push((lender.real_name(), for_line, body_end, head_call));
          }
          // A reference the iterator reborrows through (`v.iter_mut()`
          // with `v: &mut Vec<_>`, `self.items.iter_mut()`) stays live
          // until the loop ends too: its own borrow ends after the
          // iterator's (#1046, #1060).
          if let ResourceTy::Value(r) | ResourceTy::DerefField(r, _, _, _) = &lender {
            if r.is_ref() && self.borrow_map.contains_key(r.name()) {
              let floor = self.borrow_floor.entry(r.name().to_owned()).or_insert(body_end);
              *floor = (*floor).max(body_end);
            }
          }
          let id = *self.unique_id;
          *self.unique_id += 1;
          let (from, to) = (ResourceTy::Anonymous, lender);
          let static_lender = Some(static_lender);
          let ev = if is_mut {
            ExternalEvent::MutableDie { from, to, id, static_lender, still_borrowed_by: Vec::new(), of_ref_var: false, exhaustion: Vec::new() }
          } else {
            ExternalEvent::StaticDie { from, to, id, static_lender, still_borrowed_by: Vec::new(), of_ref_var: false, exhaustion: Vec::new() }
          };
          self.add_external_event(body_end, ev);
        }
        // `break`s in the body target the desugared `loop`
        // (the outer arm's body), which also ends when the
        // iterator runs out.
        if let Some(loop_expr) = arms.first().map(|a| a.body) {
          let broke_moved = self.emit_loop_exit_moves(loop_expr, None, &before, true, None);
          self.emit_loop_round_moves(loop_expr, head_moved, &broke_moved);
          self.emit_loop_exit_states(loop_expr, Vec::new(), &broke_moved, false);
        }
        if arms.first().is_some() {
          self.exit_frames.pop();
        }
        // The head temporary's drop where the loop ends was recorded with
        // the head, ahead of the body's events: put it after them, so a
        // conditional in the body comes first in an arm's events, which
        // are taken in this order (#1535: in an arm, the temporary was
        // drawn dropped before the body's `if`, then "not made yet" on
        // that `if`'s legs). (No exit frame of this loop is left to
        // index into the events; an enclosing one starts before it.)
        if let Some(t) = &head_temp {
          let at = self.preprocessed_events[..body_events_at].iter().rposition(|(_, ev)|
            matches!(ev, ExternalEvent::TemporaryDrop { ro, early: false, .. } if ro == t));
          if let Some(i) = at {
            let ev = self.preprocessed_events.remove(i);
            self.preprocessed_events.push(ev);
          }
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
}

impl<'a, 'tcx> ExprVisitor<'a, 'tcx> {
  /// The parts of each element a `for` loop's pattern leaves unbound
  /// that need drop (`.1` in `for (f, _) in v` over `(String, String)`;
  /// `[""]`, the whole element, for `for _ in v`), which Rust drops at
  /// the end of each pass (#1434). None when there are none, the element
  /// is a reference (nothing of it is owned), or the pattern's shape
  /// isn't one this follows.
  fn for_pattern_left_parts(&self, pat: &'tcx Pat<'tcx>) -> Option<Vec<String>> {
    let typeck = self.tcx.typeck(pat.hir_id.owner);
    let ty = typeck.pat_ty(pat);
    if ty.is_ref() || !self.needs_drop(ty, pat.hir_id.owner) {
      return None;
    }
    let mut out = Vec::new();
    if !self.left_parts_below(pat, String::new(), &mut out) || out.is_empty() {
      return None;
    }
    if out == [""] {
      out.clear();
    }
    Some(out)
  }

  /// `for_pattern_left_parts` below `pat`, at `path` in the element;
  /// false for a shape it doesn't follow.
  fn left_parts_below(&self, pat: &'tcx Pat<'tcx>, path: String, out: &mut Vec<String>) -> bool {
    let typeck = self.tcx.typeck(pat.hir_id.owner);
    let owner = pat.hir_id.owner;
    // Matched through a reference (default binding modes): nothing
    // there is owned by the element.
    if typeck.pat_adjustments().get(pat.hir_id).is_some_and(|a| !a.is_empty()) {
      return true;
    }
    let ty = typeck.pat_ty(pat);
    let leave = |p: String, t: rustc_middle::ty::Ty<'tcx>, out: &mut Vec<String>| {
      if self.needs_drop(t, owner) {
        out.push(p);
      }
    };
    match pat.kind {
      PatKind::Wild => {
        leave(path, ty, out);
        true
      }
      PatKind::Binding(_, hirid, _, _) => {
        // By value, the binding takes its part; `ref x` leaves it in
        // the element.
        // (`ty` is the binding's, a reference to the part.)
        if matches!(typeck.pat_binding_modes().get(hirid).map(|m| m.0), Some(rustc_hir::ByRef::Yes(..))) {
          if let rustc_middle::ty::TyKind::Ref(_, part, _) = ty.kind() {
            leave(path, *part, out);
          }
        }
        true
      }
      // `&x`: a copy out of a reference, which owns nothing.
      PatKind::Ref(..) => true,
      PatKind::Tuple(ps, dd) => {
        let rustc_middle::ty::TyKind::Tuple(tys) = ty.kind() else { return false };
        let n = tys.len();
        let d = dd.as_opt_usize();
        let idx = |i: usize| match d { Some(d) if i >= d => i + n - ps.len(), _ => i };
        // (In the element's order.)
        tys.iter().enumerate().all(|(k, t)| match (0..ps.len()).find(|&i| idx(i) == k) {
          Some(i) => self.left_parts_below(&ps[i], format!("{}.{}", path, k), out),
          None => {
            leave(format!("{}.{}", path, k), t, out);
            true
          }
        })
      }
      PatKind::TupleStruct(ref qpath, ps, dd) => {
        let (Some(adt), rustc_middle::ty::TyKind::Adt(_, args)) = (ty.ty_adt_def(), ty.kind()) else { return false };
        let res = typeck.qpath_res(qpath, pat.hir_id);
        if adt.is_enum() || matches!(res, Res::Err) {
          return false;
        }
        let fields = &adt.variant_of_res(res).fields;
        let n = fields.len();
        let d = dd.as_opt_usize();
        let idx = |i: usize| match d { Some(d) if i >= d => i + n - ps.len(), _ => i };
        fields.iter().enumerate().all(|(k, f)| match (0..ps.len()).find(|&i| idx(i) == k) {
          Some(i) => self.left_parts_below(&ps[i], format!("{}.{}", path, k), out),
          None => {
            leave(format!("{}.{}", path, k), f.ty(self.tcx, args), out);
            true
          }
        })
      }
      PatKind::Struct(ref qpath, fs, _) => {
        let (Some(adt), rustc_middle::ty::TyKind::Adt(_, args)) = (ty.ty_adt_def(), ty.kind()) else { return false };
        let res = typeck.qpath_res(qpath, pat.hir_id);
        if adt.is_enum() || matches!(res, Res::Err) {
          return false;
        }
        adt.variant_of_res(res).fields.iter().all(|f| match fs.iter().find(|pf| pf.ident.name == f.name) {
          Some(pf) => self.left_parts_below(pf.pat, format!("{}.{}", path, f.name), out),
          None => {
            leave(format!("{}.{}", path, f.name), f.ty(self.tcx, args), out);
            true
          }
        })
      }
      _ => false,
    }
  }

  /// A `for` loop's head that is an array literal or `vec![..]` of
  /// references (`[&e, &g]`, `vec![&mut a]`, #1433): the variables they
  /// borrow (drawn columns only). None for any other head, or when none
  /// is drawn.
  fn literal_head_lenders(&self, head: &'tcx Expr<'tcx>) -> Option<Vec<ResourceTy>> {
    let literal = matches!(head.kind, ExprKind::Array(..) | ExprKind::Repeat(..)) && !head.span.from_expansion()
      || vec_macro_elems(self.tcx, head).is_some();
    if !literal {
      return None;
    }
    let lenders: Vec<ResourceTy> = self.value_lenders(head).into_iter()
      .filter(|l| match l {
        ResourceTy::Value(r) => !r.is_fn(),
        ResourceTy::Deref(_) => true,
        _ => false,
      })
      .collect();
    (!lenders.is_empty()).then_some(lenders)
  }

  /// The variables borrowed by the references in the part `path` (`.0`,
  /// `.1.name`; the whole element when None) of each element of a
  /// literal `for` head (`[(&e, &g)]`, #1433). A part that isn't itself
  /// written as a literal counts the whole element's.
  fn literal_part_lenders(&self, elems: &'tcx [Expr<'tcx>], path: Option<&str>) -> Vec<ResourceTy> {
    let mut out: Vec<ResourceTy> = Vec::new();
    for elem in elems {
      let mut e = elem;
      for seg in path.unwrap_or("").split('.').filter(|s| !s.is_empty()) {
        let next = match (e.kind, seg.parse::<usize>()) {
          (ExprKind::Tup(es), Ok(i)) => es.get(i),
          (ExprKind::Call(f, args), Ok(i)) if is_ctor_path(f, self.tcx.typeck(e.hir_id.owner)) => args.get(i),
          (ExprKind::Struct(_, fields, _), _) => fields.iter().find(|f| f.ident.as_str() == seg).map(|f| f.expr),
          _ => None,
        };
        match next {
          Some(n) => e = n,
          None => break,
        }
      }
      for l in self.value_lenders(e) {
        let drawn = match &l {
          ResourceTy::Value(r) => !r.is_fn(),
          ResourceTy::Deref(_) => true,
          _ => false,
        };
        if drawn && !out.contains(&l) {
          out.push(l);
        }
      }
    }
    out
  }

  /// A `for` loop's head that moves a value holding references into the
  /// loop's iterator, whose items are those references (`for r in refs`,
  /// `for r in refs.into_iter()` with `refs: Vec<&mut String>`, #1389):
  /// the holder's name and the variables its references borrow, which
  /// each item borrows. None for a head that lends the items itself
  /// (`refs.iter()`, `&refs`), or whose holder's loans aren't known.
  fn moved_ref_holder(&self, head_arg: &'tcx Expr<'tcx>, place_head: bool, iterand_evt: &Option<Evt>) -> Option<(String, Vec<ResourceTy>)> {
    let typeck = self.tcx.typeck(head_arg.hir_id.owner);
    let holder = match head_arg.kind {
      _ if place_head => {
        // Moved in. (Not an iterator, `for x in it`, whose items
        // `next()` hands out: the caller handles it first.)
        if !matches!(iterand_evt, Some(Evt::Move)) {
          return None;
        }
        head_arg
      }
      // `refs.into_iter()`: a by-value method on the holder.
      ExprKind::MethodCall(seg, recv, [], _) if seg.ident.name.as_str() == "into_iter"
        && matches!(recv.kind, ExprKind::Path(_) | ExprKind::Field(..))
        && !typeck.expr_ty(recv).is_ref()
        && !is_method_autoref(recv, typeck) => recv,
      _ => return None,
    };
    let ty = typeck.expr_ty(holder);
    if ty.is_ref() || self.ty_is_copy(ty, holder.hir_id.owner) {
      return None;
    }
    let ResourceTy::Value(rap) = get_rap(holder, &self.tcx, &self.raps) else { return None };
    if rap.is_ref() || !rap.holds_borrow() {
      return None;
    }
    let rd = self.borrow_map.get(rap.name())?;
    let lenders: Vec<ResourceTy> = rd.all_lenders().into_iter()
      .filter(|l| matches!(l, ResourceTy::Value(r) if !r.is_fn()))
      .collect();
    (!lenders.is_empty()).then(|| (rap.name().to_owned(), lenders))
  }

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
    let env = typing_env(self.tcx, e.hir_id.owner);
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
      _ => match vec_macro_elems(self.tcx, e) {
        Some(items) => items.iter().collect(),
        // `vec![]`, a `format!`: nothing is moved in.
        None => return Some(Vec::new()),
      },
    };
    let vec = vec_macro_elems(self.tcx, e).is_some();
    for x in elems {
      if vec && is_place_expr(x) {
        if let Some(crate::expr_visitor::Transfer { row, kind: Evt::Move, path, to: dest, .. }) = self.transfer_at(x) {
          if let Some(rap) = self.raps.get(path).filter(|_| dest != "<temp>") {
            places.push((rap.rap.clone(), row));
          }
        }
      } else if vec && vec_macro_elems(self.tcx, x).is_some() {
        places.extend(self.literal_moved_places(x)?);
      } else if is_place_expr(x) {
        if self.needs_drop(typeck.expr_ty(x), e.hir_id.owner) {
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
      if fields.iter().any(|ef| ef.ident.name == f.name) || !self.needs_drop_in(f.ty(self.tcx, args), env) {
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
  /// variables that a match or `if let` (or a let-chain part, #971)
  /// scrutinizes already has its moves and its drop drawn as "the
  /// temporary tuple (x, y)" (#327), so it gets no `(temporary)` column
  /// as well.
  /// The same for a constructor scrutinee (`match Some(f)`, #807).
  fn is_drawn_tuple_scrutinee(&self, expr: &'tcx Expr<'tcx>) -> bool {
    self.literal_elems(expr).is_some() && self.tuple_scrutinee_position(expr) && !self.tuple_scrutinee_moves(expr).is_empty()
  }

  /// The elements of a literal that a `match` / `if let` takes apart
  /// element by element (#327): a tuple literal `(x, y)`, or a tuple
  /// struct / variant constructor `Some(f)`, `W(a, b)` (#807) that is
  /// the scrutinee, or an element of such a literal, when a pattern
  /// binds something that needs drop by value (`Some(x)` with `x:
  /// String`). A constructor is never an actor (#768, #873): what goes
  /// into it is drawn going straight to the binding that takes it (`f`
  /// to `x`), and what the pattern leaves is dropped with the temporary
  /// the constructor built, as for a tuple. A constructor whose
  /// patterns bind nothing by value keeps its `(temporary)` column
  /// (#794).
  pub(crate) fn literal_elems(&self, e: &'tcx Expr<'tcx>) -> Option<&'tcx [Expr<'tcx>]> {
    if e.span.from_expansion() {
      return None;
    }
    match e.kind {
      ExprKind::Tup(elems) => Some(elems),
      ExprKind::Call(callee, args) if !args.is_empty()
        && is_ctor_path(callee, self.tcx.typeck(e.hir_id.owner))
        && self.in_literal_scrutinee(e) => Some(args),
      _ => None,
    }
  }

  /// Whether the constructor call `e` is a scrutinee taken apart like a
  /// tuple literal, or an element of one (see `literal_elems`).
  fn in_literal_scrutinee(&self, e: &'tcx Expr<'tcx>) -> bool {
    use rustc_hir::Node;
    let typeck = self.tcx.typeck(e.hir_id.owner);
    let env = typing_env(self.tcx, e.hir_id.owner);
    let mut cur = e;
    loop {
      let Node::Expr(p) = self.tcx.parent_hir_node(cur.hir_id) else { return false };
      if p.span.from_expansion() {
        return false;
      }
      let top = |c: &'tcx Expr<'tcx>| match c.kind {
        ExprKind::Tup(_) => true,
        _ => !self.is_unmoved_temporary(c, typeck, env),
      };
      match p.kind {
        ExprKind::Tup(elems) if elems.iter().any(|x| x.hir_id == cur.hir_id) => cur = p,
        ExprKind::Call(callee, args) if is_ctor_path(callee, typeck) && args.iter().any(|x| x.hir_id == cur.hir_id) => cur = p,
        ExprKind::Match(s, _, rustc_hir::MatchSource::Normal) => return s.hir_id == cur.hir_id && top(cur),
        // Also a part of a let chain (`if let Some(x) = Some(f) &&
        // c`, #949).
        ExprKind::Let(l) => return l.init.hir_id == cur.hir_id && self.is_if_condition_let(p) && top(cur),
        _ => return false,
      }
    }
  }

  /// Whether `pat` matches every value `init` can have: an irrefutable
  /// pattern, or one that takes apart a literal the way it was built
  /// (`Some(x)` on `Some(f)`, #949): a let-chain part that can't fail.
  fn let_always_matches(&self, pat: &'tcx Pat<'tcx>, init: &'tcx Expr<'tcx>) -> bool {
    let typeck = self.tcx.typeck(pat.hir_id.owner);
    if pat_is_irrefutable(pat, typeck) {
      return true;
    }
    if self.literal_elems(init).is_none() {
      return false;
    }
    let mut pairs = Vec::new();
    self.literal_pat_pairs(pat, init, &mut pairs);
    // Each leaf: an irrefutable sub-pattern, with its element. A
    // pattern naming another variant than the constructor builds is a
    // leaf of its own, and refutable.
    pairs.iter().all(|(p, _)| pat_is_irrefutable(p, typeck))
  }

  /// The `let` parts of the let chain `cond` whose scrutinee is a tuple
  /// literal (or a constructor) taken apart like a lone `if let`'s
  /// (#327, #971), in source order.
  fn chain_tuple_lets(&self, cond: &'tcx Expr<'tcx>) -> Vec<&'tcx rustc_hir::LetExpr<'tcx>> {
    let mut out = Vec::new();
    let mut stack = vec![cond];
    while let Some(e) = stack.pop() {
      match e.kind {
        ExprKind::DropTemps(inner) => stack.push(inner),
        ExprKind::Binary(op, l, r) if op.node == rustc_hir::BinOpKind::And => {
          stack.push(r);
          stack.push(l);
        }
        ExprKind::Let(l) if self.is_drawn_tuple_scrutinee(l.init) => out.push(l),
        _ => {}
      }
    }
    out
  }

  /// The text that names a tuple scrutinee's temporary (see
  /// `tuple_scrutinee_target`).
  pub(crate) fn tuple_scrutinee_text(&self, scrutinee: &'tcx Expr<'tcx>) -> String {
    match self.tuple_scrutinee_target(scrutinee) {
      ResourceTy::Temporary { tuple: Some(t), .. } => t,
      _ => String::new(),
    }
  }

  /// Whether the `let` expression `l` is an `if`'s condition, or a part
  /// of a let chain that is (`if a && let P = e && c`, #949).
  pub(crate) fn is_if_condition_let(&self, l: &'tcx Expr<'tcx>) -> bool {
    use rustc_hir::Node;
    let mut cur = l;
    loop {
      let Node::Expr(p) = self.tcx.parent_hir_node(cur.hir_id) else { return false };
      match p.kind {
        ExprKind::If(c, ..) => return c.hir_id == cur.hir_id,
        ExprKind::DropTemps(_) => cur = p,
        ExprKind::Binary(op, ..) if op.node == rustc_hir::BinOpKind::And && is_let_chain(p) => cur = p,
        _ => return false,
      }
    }
  }

  /// A constructor scrutinee taken apart like a tuple literal (#807)
  /// that moves no variable in (`match Some(make()) { .. }`): what a
  /// pattern leaves of it is dropped with its `(temporary)` column.
  fn note_ctor_literal_temporary_drop(&mut self, expr: &'tcx Expr<'tcx>) {
    let ExprKind::Call(callee, _) = expr.kind else { return };
    if self.scrutinee_scope(expr, "").is_none() {
      return;
    }
    // (`record_temporary_drop` doesn't draw the constructor.)
    if let Some(name) = hirid_to_var_name(callee.hir_id, &self.tcx) {
      self.note_temporary_drop(expr, name, Vec::new());
    }
  }

  /// The call whose result is the payload of the constructor `expr`
  /// (`make()` in `Some(make())`): its column, when that is the only
  /// argument. What a pattern binds from it is drawn from that column
  /// too (`literal_pat_parents`).
  fn ctor_payload_call(&self, expr: &'tcx Expr<'tcx>) -> Option<ResourceAccessPoint> {
    let ExprKind::Call(_, [arg]) = expr.kind else { return None };
    if !matches!(arg.kind, ExprKind::Call(..) | ExprKind::MethodCall(..)) {
      return None;
    }
    match self.rap_or_field_through_ref(arg) {
      ResourceTy::Value(f @ ResourceAccessPoint::Function(_)) => Some(f),
      _ => None,
    }
  }

  /// A macro's or literal's temporary. A variable moved into it
  /// (`vec![s];`, `let _ = P { name: t, n: 1 };`) is drawn moving into
  /// its column, so it stops owning the value (#570).
  fn note_container_temporary_drop(&mut self, expr: &'tcx Expr<'tcx>, name: String) {
    let Some(places) = self.literal_moved_places(expr) else { return };
    let ty = self.tcx.typeck(expr.hir_id.owner).expr_ty(expr);
    if !self.needs_drop(ty, expr.hir_id.owner) {
      self.note_plain_literal_moves(expr);
      return;
    }
    self.note_temporary_drop(expr, name, places);
  }

  /// `let _ = (p, 5);`, `[p];` with `p: P { a: i32 }`: a literal with
  /// nothing to drop gets no `(temporary)` column, but the variables in
  /// it are moved into it all the same (P isn't Copy), and can't be used
  /// after it (#1301). Only for a literal nothing takes, discarded at the
  /// end of its statement.
  fn note_plain_literal_moves(&mut self, expr: &'tcx Expr<'tcx>) {
    if expr.span.from_expansion() || self.temp_drops_noted.contains(&expr.hir_id) {
      return;
    }
    let typeck = self.tcx.typeck(expr.hir_id.owner);
    let env = typing_env(self.tcx, expr.hir_id.owner);
    if !self.is_unmoved_temporary(expr, typeck, env)
      || self.temporary_scope_end(expr).is_none_or(|(when, _, _)| when != "at the end of this statement") {
      return;
    }
    self.temp_drops_noted.insert(expr.hir_id);
    let mut todo = vec![expr];
    let mut moved = Vec::new();
    while let Some(e) = todo.pop() {
      let elems: Vec<&'tcx Expr<'tcx>> = match e.kind {
        ExprKind::Struct(_, fields, _) => fields.iter().map(|f| f.expr).collect(),
        ExprKind::Tup(items) | ExprKind::Array(items) => items.iter().collect(),
        _ => Vec::new(),
      };
      for x in elems {
        if is_place_expr(x) {
          if !self.ty_is_copy(typeck.expr_ty(x), expr.hir_id.owner) && !self.premoved.contains(&x.hir_id) {
            if let Some(src) = fetch_rap(x, &self.tcx, &self.raps) {
              moved.push((src, expr_to_line(x, &self.tcx)));
            }
          }
        } else if matches!(x.kind, ExprKind::Struct(..) | ExprKind::Tup(_) | ExprKind::Array(_)) {
          todo.push(x);
        }
      }
    }
    let line = expr_to_line(expr, &self.tcx);
    for (src, src_line) in moved {
      self.update_rap(&src, line);
      self.add_ev(src_line, Evt::Move, ResourceTy::Temporary { extended: false, tuple: None }, ResourceTy::Value(src), false);
    }
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
    let env = typing_env(self.tcx, expr.hir_id.owner);
    if !self.needs_drop(ty, expr.hir_id.owner) {
      return None;
    }
    // Something is moved out of it (a field, what a pattern binds by
    // value), and what may be left needs dropping (#469).
    let part = if self.is_unmoved_temporary(expr, typeck, env) {
      None
    } else {
      Some(self.temp_part_moved(expr, ty, typeck, env)?)
    };
    let (mut when, mut end_line, owner) = self.temporary_scope_end(expr)?;
    // What is matched on: the temporary, or a field of it (#605).
    let scrut = self.field_top(expr);
    match part {
      // A `match` that is a statement, a `let`'s initializer or a block's
      // value: its scrutinee is dropped when the match ends.
      Some(TempPart::Match(_)) => {
        if let rustc_hir::Node::Expr(m) = self.tcx.parent_hir_node(scrut.hir_id) {
          if matches!(self.tcx.parent_hir_node(m.hir_id),
            rustc_hir::Node::Stmt(_) | rustc_hir::Node::LetStmt(_) | rustc_hir::Node::Block(_)) {
            when = "when the match ends";
          }
        }
      }
      // `let Ok(s) = res() else { .. };`: dropped once the pattern is
      // matched, before the `else` block runs (any edition, #605).
      // `(m, _) = pair();`: dropped at the end of the `let` it is
      // lowered to, before the places on the left are assigned (#625).
      Some(TempPart::Assign(_)) => {
        when = "as soon as the assignment has taken it apart (before the places on the left get their new values)";
      }
      Some(TempPart::LetElse(_)) => {
        when = "as soon as the pattern has been matched against it (before the `else` block runs, if it doesn't match)";
        end_line = span_end_line(&scrut.span, &self.tcx);
      }
      _ => {}
    }
    let let_else_literal = matches!(part, Some(TempPart::LetElse(None)))
      && matches!(expr.kind, ExprKind::Tup(_) | ExprKind::Struct(..) | ExprKind::Array(_));
    let tmp = self.record_temporary_drop(expr, strip_module_paths(&ty.to_string()), fn_name, when, end_line, owner, into, part, false);
    if let (true, Some(t)) = (let_else_literal, &tmp) {
      self.note_let_else_literal_temp(expr, t.clone());
    }
    tmp
  }

  /// `let (Some(a), _) = (x, make()) else { .. }`: `tmp` is the one
  /// temporary of the literal `lit`, which the variables in it (x)
  /// were moved into. It is the literal's temporary for `bind_pat` too
  /// (`literal_temps`, so no second one is made, and the variables
  /// aren't moved again: `premoved`), and what the pattern binds by
  /// value from x is moved out of it, not out of x (`match_rhs`, by
  /// `let_else_literal_temps`) (#954).
  fn note_let_else_literal_temp(&mut self, lit: &'tcx Expr<'tcx>, tmp: ResourceAccessPoint) {
    let rustc_hir::Node::LetStmt(l) = self.tcx.parent_hir_node(lit.hir_id) else { return };
    self.literal_temps.insert(l.hir_id, tmp.clone());
    self.let_else_literal_temps.insert(lit.hir_id, tmp.clone());
    let typeck = self.tcx.typeck(lit.hir_id.owner);
    let mut todo = vec![lit];
    while let Some(e) = todo.pop() {
      let elems: Vec<&'tcx Expr<'tcx>> = match e.kind {
        ExprKind::Struct(_, fields, _) => fields.iter().map(|f| f.expr).collect(),
        ExprKind::Tup(items) | ExprKind::Array(items) => items.iter().collect(),
        _ => Vec::new(),
      };
      for x in elems {
        if is_place_expr(x) {
          if self.needs_drop(typeck.expr_ty(x), lit.hir_id.owner) && fetch_rap(x, &self.tcx, &self.raps).is_some() {
            self.let_else_literal_temps.insert(x.hir_id, tmp.clone());
            self.premoved.insert(x.hir_id);
          }
        } else if matches!(x.kind, ExprKind::Struct(..) | ExprKind::Tup(_) | ExprKind::Array(_)) {
          todo.push(x);
        }
      }
    }
  }

  /// What `note_temporary_drop` says is moved out of the temporary
  /// `expr` (of type `ty`), when that leaves something in it that may
  /// need dropping: a field (`let s = make_q().a;`, `take(make_q().b)`),
  /// the fields a struct update takes from its base (`..make_p()`), or
  /// what the pattern of a `match` / `if let` / `while let` / `let`-`else`
  /// binds by value, from the temporary or from a field of it (`match
  /// make_p().name { .. }`, #605), or what a `let` pattern takes apart
  /// (`let (a, _) = pair();`, #623). None for any other use, and when
  /// nothing that needs
  /// dropping can be left (`match make() { Some(s) => .., None => .. }`
  /// on an `Option<String>`: the arm that binds `s` takes all of it).
  fn temp_part_moved(
    &self,
    expr: &'tcx Expr<'tcx>,
    ty: rustc_middle::ty::Ty<'tcx>,
    typeck: &'tcx rustc_middle::ty::TypeckResults<'tcx>,
    env: rustc_middle::ty::TypingEnv<'tcx>,
  ) -> Option<TempPart> {
    use rustc_hir::Node;
    // `make_q().inner.name`: the outermost field projection, its path,
    // and whether a field beside it needs dropping.
    let top = self.field_top(expr);
    let (sty, path, left) = if top.hir_id == expr.hir_id {
      (ty, None, false)
    } else {
      let mut path: Vec<String> = Vec::new();
      let mut left = false;
      let mut e = top;
      while let ExprKind::Field(base, f) = e.kind {
        left |= self.other_fields_need_drop(typeck.expr_ty(base), f.as_str(), env);
        path.push(f.as_str().to_owned());
        if base.hir_id == expr.hir_id {
          break;
        }
        e = base;
      }
      path.reverse();
      (typeck.expr_ty(top), Some(path.join(".")), left)
    };
    match self.tcx.parent_hir_node(top.hir_id) {
      // `match make() { .. }`, `match make_p().name { .. }` (#605).
      Node::Expr(p) => match p.kind {
        ExprKind::Match(s, arms, rustc_hir::MatchSource::Normal) if s.hir_id == top.hir_id => {
          (left || arms.iter().any(|a| self.pat_leaves_temp_drop(a.pat, sty, typeck, env))).then_some(TempPart::Match(path))
        }
        ExprKind::Let(l) if l.init.hir_id == top.hir_id => {
          // The condition of an `if let` / `while let`, or a `let` in the
          // let chain that is one (`if a && let Ok(s) = make()`, edition
          // 2024): the scrutinee lives until the matched block ends or the
          // `else` runs (`temporary_scope_end`, #604).
          let mut cond = p;
          let mut in_chain = false;
          let i = loop {
            match self.tcx.parent_hir_node(cond.hir_id) {
              Node::Expr(b) if matches!(b.kind, ExprKind::Binary(op, ..) if op.node == rustc_hir::BinOpKind::And) => {
                cond = b;
                in_chain = true;
              }
              Node::Expr(i) if matches!(i.kind, ExprKind::If(c, ..) if c.hir_id == cond.hir_id) => break i,
              _ => return None,
            }
          };
          if !left && !self.pat_leaves_temp_drop(l.pat, sty, typeck, env) && !self.pat_rejects_drop(l.pat, sty, typeck, env) {
            return None;
          }
          let is_while = matches!(i.span.desugaring_kind(), Some(rustc_span::DesugaringKind::WhileLoop));
          Some(if in_chain { TempPart::LetChain(path) } else if is_while { TempPart::WhileLet(path) } else { TempPart::IfLet(path) })
        }
        // `P { n: 1, ..make_p() }`: the literal moves the fields it
        // doesn't name out of the base (#605).
        ExprKind::Struct(_, fields, rustc_hir::StructTailExpr::Base(b)) if b.hir_id == top.hir_id => {
          if path.is_some() {
            return None;
          }
          let rustc_middle::ty::TyKind::Adt(def, args) = ty.kind() else { return None };
          if !def.is_struct() {
            return None;
          }
          let mut moved = Vec::new();
          let mut left = false;
          for f in def.non_enum_variant().fields.iter() {
            let fty = f.ty(self.tcx, args);
            if fields.iter().any(|ef| ef.ident.name == f.name) {
              left |= self.needs_drop_in(fty, env);
            } else if !self.tcx.type_is_copy_modulo_regions(env, fty) {
              moved.push(f.name.as_str().to_owned());
            }
          }
          (left && !moved.is_empty()).then_some(TempPart::UpdateBase(moved))
        }
        // `(None, m)` on `match (x, make()) { .. }` (#942), `(Some(a),
        // _)` on `(x, make_p().name)` (#956).
        ExprKind::Tup(_) if path.is_none() || self.tuple_scrutinee_elem(top).is_some() => self.tuple_scrutinee_elem_part(top, path, left, typeck, env),
        _ => self.field_moved_part(top, path, left, typeck, env),
      },
      // `let Ok(s) = res() else { .. };` (#605).
      Node::LetStmt(l) if l.els.is_some() => {
        if l.span.from_expansion() || !l.init.is_some_and(|i| i.hir_id == top.hir_id) {
          return None;
        }
        // `let (Some(ref a), _) = (x, make()) else { .. }`: the `ref`
        // binding keeps the literal's temporary alive to the end of the
        // block, which #439's `literal_temp` draws (#954).
        if matches!(top.kind, ExprKind::Tup(_) | ExprKind::Struct(..) | ExprKind::Array(_)) && !self.binds_by_value_only(l.pat) {
          return None;
        }
        (left || self.pat_leaves_temp_drop(l.pat, sty, typeck, env) || self.pat_rejects_drop(l.pat, sty, typeck, env))
          .then_some(TempPart::LetElse(path))
      }
      // `let t = make_q().a;`.
      Node::LetStmt(l) if matches!(l.pat.kind, PatKind::Binding(ann, .., None) if ann.0 == rustc_hir::ByRef::No) => {
        self.field_moved_part(top, path, left, typeck, env)
      }
      // `let (a, _) = pair();`, `let (b, _) = make().pair;`: the pattern
      // takes the value (or the field) apart, and can leave parts of it
      // in the temporary, which is dropped at the end of the statement
      // (#623). A `ref` binding extends the temporary instead (#483); a
      // destructuring assignment (`(m, _) = pair();`) is lowered to such
      // a `let` (#625). A literal (`let (a, _) = (s, t);`) is #445's.
      Node::LetStmt(l) => {
        let assign = matches!(l.source, rustc_hir::LocalSource::AssignDesugar);
        if (!assign && (l.span.from_expansion() || !matches!(l.source, rustc_hir::LocalSource::Normal)))
          || !l.init.is_some_and(|i| i.hir_id == top.hir_id)
          || matches!(expr.kind, ExprKind::Struct(..) | ExprKind::Tup(_) | ExprKind::Array(_)) {
          return None;
        }
        let mut by_ref = false;
        l.pat.each_binding(|ann, _, _, _| by_ref |= ann.0 != rustc_hir::ByRef::No);
        if by_ref || !(left || self.pat_leaves_temp_drop(l.pat, sty, typeck, env)) {
          return None;
        }
        Some(if assign { TempPart::Assign(path) } else { TempPart::Let(path) })
      }
      _ => self.field_moved_part(top, path, left, typeck, env),
    }
  }

  /// `TempPart::Field` for a field `top` (at `path`) of a temporary that
  /// is moved out, when another field (`left`) needs dropping.
  fn field_moved_part(
    &self,
    top: &'tcx Expr<'tcx>,
    path: Option<String>,
    left: bool,
    typeck: &'tcx rustc_middle::ty::TypeckResults<'tcx>,
    env: rustc_middle::ty::TypingEnv<'tcx>,
  ) -> Option<TempPart> {
    let path = path?;
    // A Copy read or a borrow of the field is `is_unmoved_temporary`'s;
    // a field borrowed by a `let` isn't drawn.
    if !left || self.extended_by_let(top) || self.tcx.type_is_copy_modulo_regions(env, typeck.expr_ty(top)) {
      return None;
    }
    Some(TempPart::Field(path))
  }

  /// The outermost field projection on `expr` (`make_q().inner.name`
  /// for `make_q()`), or `expr` itself.
  fn field_top(&self, expr: &'tcx Expr<'tcx>) -> &'tcx Expr<'tcx> {
    let mut top = expr;
    while let rustc_hir::Node::Expr(q) = self.tcx.parent_hir_node(top.hir_id) {
      match q.kind {
        ExprKind::Field(base, _) if base.hir_id == top.hir_id => top = q,
        _ => break,
      }
    }
    top
  }

  /// True iff a field of `ty` other than `field` needs dropping (true
  /// for a type that isn't a struct or tuple).
  fn other_fields_need_drop(&self, ty: rustc_middle::ty::Ty<'tcx>, field: &str, env: rustc_middle::ty::TypingEnv<'tcx>) -> bool {
    match ty.kind() {
      rustc_middle::ty::TyKind::Adt(def, args) if def.is_struct() => def.non_enum_variant().fields.iter()
        .any(|f| f.name.as_str() != field && self.needs_drop_in(f.ty(self.tcx, args), env)),
      rustc_middle::ty::TyKind::Tuple(tys) => tys.iter().enumerate()
        .any(|(i, t)| i.to_string() != field && self.needs_drop_in(t, env)),
      _ => true,
    }
  }

  /// The sub-patterns of a tuple / tuple-struct / struct pattern
  /// matched against `ty`, paired with the fields they match, plus the
  /// fields no sub-pattern names (`..`). None for other patterns, or
  /// a type we can't take apart.
  fn pat_fields(
    &self,
    pat: &'tcx Pat<'tcx>,
    ty: rustc_middle::ty::Ty<'tcx>,
    typeck: &'tcx rustc_middle::ty::TypeckResults<'tcx>,
  ) -> Option<(Vec<(&'tcx Pat<'tcx>, rustc_middle::ty::Ty<'tcx>)>, Vec<rustc_middle::ty::Ty<'tcx>>)> {
    use rustc_middle::ty::TyKind;
    let field_tys: Vec<rustc_middle::ty::Ty<'tcx>> = match (pat.kind, ty.kind()) {
      (PatKind::Tuple(..), TyKind::Tuple(tys)) => tys.iter().collect(),
      (PatKind::TupleStruct(ref q, ..) | PatKind::Struct(ref q, ..), TyKind::Adt(def, args)) => {
        let v = def.variant_of_res(typeck.qpath_res(q, pat.hir_id));
        v.fields.iter().map(|f| f.ty(self.tcx, args)).collect()
      }
      _ => return None,
    };
    let mut named = vec![false; field_tys.len()];
    let mut subs = Vec::new();
    match pat.kind {
      PatKind::Tuple(ps, ddpos) | PatKind::TupleStruct(_, ps, ddpos) => {
        for (k, sp) in ps.iter().enumerate() {
          let i = match ddpos.as_opt_usize() {
            Some(d) if k >= d => k + field_tys.len().saturating_sub(ps.len()),
            _ => k,
          };
          let t = *field_tys.get(i)?;
          named[i] = true;
          subs.push((sp, t));
        }
      }
      PatKind::Struct(ref q, fields, _) => {
        let TyKind::Adt(def, _) = ty.kind() else { return None };
        let v = def.variant_of_res(typeck.qpath_res(q, pat.hir_id));
        for fp in fields.iter() {
          let i = v.fields.iter().position(|f| f.name == fp.ident.name)?;
          named[i] = true;
          subs.push((fp.pat, field_tys[i]));
        }
      }
      _ => return None,
    }
    let rest = field_tys.iter().zip(named).filter(|(_, n)| !n).map(|(t, _)| *t).collect();
    Some((subs, rest))
  }

  /// True iff matching a value of type `ty` against `pat` can leave a
  /// part of it that needs dropping in it: a `_`, a `ref` binding, a
  /// field the pattern skips with `..`.
  fn pat_leaves_temp_drop(
    &self,
    pat: &'tcx Pat<'tcx>,
    ty: rustc_middle::ty::Ty<'tcx>,
    typeck: &'tcx rustc_middle::ty::TypeckResults<'tcx>,
    env: rustc_middle::ty::TypingEnv<'tcx>,
  ) -> bool {
    if !self.needs_drop_in(ty, env) {
      return false;
    }
    match pat.kind {
      PatKind::Binding(ann, ..) => ann.0 != rustc_hir::ByRef::No,
      PatKind::Wild => true,
      PatKind::Or(ps) => ps.iter().any(|p| self.pat_leaves_temp_drop(p, ty, typeck, env)),
      // `None`, a unit variant: nothing in it. Literals and ranges
      // match values that need no drop.
      PatKind::Expr(_) | PatKind::Range(..) | PatKind::Never | PatKind::Err(_) => false,
      // `[a, b]`, `[first, ..]`: the elements the pattern names, and
      // what its `..` covers.
      PatKind::Slice(before, mid, after) => {
        let elem = match ty.kind() {
          rustc_middle::ty::TyKind::Array(e, _) | rustc_middle::ty::TyKind::Slice(e) => *e,
          _ => return true,
        };
        before.iter().chain(after.iter()).any(|p| self.pat_leaves_temp_drop(p, elem, typeck, env))
          || mid.is_some_and(|m| self.pat_leaves_temp_drop(m, typeck.node_type(m.hir_id), typeck, env))
      }
      PatKind::Tuple(..) | PatKind::TupleStruct(..) | PatKind::Struct(..) => match self.pat_fields(pat, ty, typeck) {
        Some((subs, rest)) => rest.iter().any(|t| self.needs_drop_in(*t, env))
          || subs.iter().any(|(p, t)| self.pat_leaves_temp_drop(p, *t, typeck, env)),
        None => true,
      },
      _ => true,
    }
  }

  /// True iff `pat` may refuse a value of type `ty` that holds
  /// something that needs dropping (`if let Ok(s) = make()` with an
  /// `Err(String)`), which then stays in the temporary.
  fn pat_rejects_drop(
    &self,
    pat: &'tcx Pat<'tcx>,
    ty: rustc_middle::ty::Ty<'tcx>,
    typeck: &'tcx rustc_middle::ty::TypeckResults<'tcx>,
    env: rustc_middle::ty::TypingEnv<'tcx>,
  ) -> bool {
    use rustc_middle::ty::TyKind;
    if !self.needs_drop_in(ty, env) {
      return false;
    }
    match pat.kind {
      PatKind::Wild | PatKind::Binding(.., None) => false,
      PatKind::Binding(.., Some(sub)) => self.pat_rejects_drop(sub, ty, typeck, env),
      PatKind::Tuple(ps, _) => ps.iter().any(|p| pat_refutable(p)),
      PatKind::TupleStruct(ref q, ..) | PatKind::Struct(ref q, ..) if ty.is_adt() => {
        let TyKind::Adt(def, args) = ty.kind() else { return true };
        let subs_refutable = match pat.kind {
          PatKind::TupleStruct(_, ps, _) => ps.iter().any(|p| pat_refutable(p)),
          PatKind::Struct(_, fields, _) => fields.iter().any(|f| pat_refutable(f.pat)),
          _ => true,
        };
        let v = def.variant_of_res(typeck.qpath_res(q, pat.hir_id));
        let holds_drop = |v: &rustc_middle::ty::VariantDef| v.fields.iter().any(|f| self.needs_drop_in(f.ty(self.tcx, args), env));
        (def.is_enum() && def.variants().iter().any(|o| o.def_id != v.def_id && holds_drop(o)))
          || (subs_refutable && holds_drop(v))
      }
      _ => true,
    }
  }

  /// When `expr` is directly the scrutinee of a `match`, `if let`,
  /// `while let`, `let`-`else` or a let chain's `let`: what examines it, and when its
  /// temporary is dropped (`when` is where its scope ends). A `match`
  /// that ends a statement (or a 2024 block's final expression) drops it
  /// when the `match` ends.
  fn scrutinee_scope(&self, expr: &'tcx Expr<'tcx>, when: &str) -> Option<(String, String)> {
    use rustc_hir::Node;
    // A `let`-`else`'s initializer (#954).
    if let Node::LetStmt(l) = self.tcx.parent_hir_node(expr.hir_id) {
      return (l.els.is_some() && l.init.is_some_and(|i| i.hir_id == expr.hir_id) && !l.span.from_expansion())
        .then(|| ("`let`-`else`".to_owned(), when.to_owned()));
    }
    let Node::Expr(parent) = self.tcx.parent_hir_node(expr.hir_id) else { return None };
    match parent.kind {
      ExprKind::Match(s, _, rustc_hir::MatchSource::Normal) if s.hir_id == expr.hir_id && !parent.span.from_expansion() => {
        let ends_match = match self.tcx.parent_hir_node(parent.hir_id) {
          Node::Stmt(_) | Node::LetStmt(_) => true,
          Node::Block(b) => b.expr.is_some_and(|e| e.hir_id == parent.hir_id)
            && expr.span.edition() >= rustc_span::edition::Edition::Edition2024,
          _ => false,
        };
        let when = if ends_match { "when the `match` ends" } else { when };
        Some(("`match`".to_owned(), when.to_owned()))
      }
      ExprKind::Let(l) if l.init.hir_id == expr.hir_id && !parent.span.from_expansion() => {
        let what = match self.tcx.parent_hir_node(parent.hir_id) {
          Node::Expr(i) if matches!(i.kind, ExprKind::If(..)) => {
            if matches!(i.span.desugaring_kind(), Some(rustc_span::DesugaringKind::WhileLoop)) { "`while let`" } else { "`if let`" }
          }
          _ => "`let`",
        };
        Some((what.to_owned(), when.to_owned()))
      }
      _ => None,
    }
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
    part: Option<TempPart>,
    iterator_elements: bool,
  ) -> Option<ResourceAccessPoint> {
    // Branch events are sliced out by line, so a one-line `match` / `if`
    // can't hold a column of its own (the same limit as #354 / #459).
    if self.inside_one_line_branch(expr) {
      return None;
    }
    self.temp_drops_noted.insert(expr.hir_id);
    let scrutinee = self.scrutinee_scope(expr, when);

    // The temporary gets a short column: it acquires the call's result
    // here and is dropped where its scope ends. A constructor scrutinee
    // taken apart by a pattern (`match Some(make()) { .. }`, #807) is no
    // actor (#873): the temporary takes what the call making its payload
    // returns, or, without one (`Some(format!(..))`), comes from nothing
    // drawn.
    let ctor_literal = matches!(expr.kind, ExprKind::Call(..)) && self.literal_elems(expr).is_some();
    let source = if ctor_literal {
      self.ctor_payload_call(expr)
    } else {
      self.add_fn(fn_name.clone());
      Some(self.raps.get(&fn_key(&fn_name)).unwrap().rap.to_owned())
    };
    // Numbered in one sequence with the `let`-extended temporaries
    // (#400): the label doesn't say which kind it is, the tooltips do.
    self.temp_count += 1;
    let tmp_name = temporary_label(self.temp_count);
    // A `ref mut` binding borrows a scrutinee temporary mutably (#1386).
    let ref_mut = scrutinee.is_some() || matches!(part, Some(TempPart::Match(_) | TempPart::IfLet(_) | TempPart::WhileLet(_) | TempPart::LetChain(_)));
    let ref_mut = ref_mut && self.scrutinee_temp_pats(expr).is_some_and(|(path, pats, _)| {
      let typeck = self.tcx.typeck(expr.hir_id.owner);
      let mut found = false;
      for p in pats {
        if let Some(sub) = self.pat_at_path(p, &path, typeck) {
          sub.walk_always(|b| found |= matches!(typeck.pat_binding_modes().get(b.hir_id).map(|m| m.0),
            Some(rustc_hir::ByRef::Yes(_, Mutability::Mut))));
        }
      }
      found
    });
    self.add_owner(tmp_name.clone(), ref_mut, false, false, InteriorMut::No, end_line, !self.inside_branch);
    if let Some(ResourceAccessPoint::Owner(o)) = self.raps.get_mut(&tmp_name).map(|rd| &mut rd.rap) {
      o.unbound_temp = true;
      o.temp_part = part.clone();
      o.ctor_temp = (part.is_none() || ctor_literal) && !expr.span.from_expansion()
        && matches!(expr.kind, ExprKind::Call(callee, _) if is_ctor_path(callee, self.tcx.typeck(expr.hir_id.owner)));
      // (Also when the pattern moves part of it out, #955: what a
      // variable moved into it says is when the temporary is dropped.)
      o.scrutinee = scrutinee;
    }
    self.raps.get_mut(&tmp_name).unwrap().temp_owner = owner;
    let tmp = self.raps.get(&tmp_name).unwrap().rap.to_owned();
    let line = expr_to_line(expr, &self.tcx);
    // `Some(make());`: a constructor isn't an actor (#768); once its
    // arguments are visited, what was moved into it (a call's result, a
    // variable) is redrawn into the temporary instead (#783).
    if matches!(&tmp, ResourceAccessPoint::Owner(Owner { ctor_temp: true, temp_part: None, .. })) {
      self.ctor_temps.insert(expr.hir_id, (tmp.clone(), *self.unique_id));
    }
    match source {
      Some(fn_rap) => self.add_ev(line, Evt::Move, ResourceTy::Value(tmp.clone()), ResourceTy::Value(fn_rap), false),
      None => self.add_ev(line, Evt::Bind, ResourceTy::Value(tmp.clone()), ResourceTy::Anonymous, false),
    }
    // Variables moved into the temporary (`vec![s];`, #570; the fields
    // `P { n: 1, ..p }` takes from p, #574).
    for (src, src_line) in into {
      if iterator_elements { self.approx_notes.push(ApproxNote::IteratorElement { id: *self.unique_id }); }
      self.update_rap(&src, line);
      self.add_ev(src_line, Evt::Move, ResourceTy::Value(tmp.clone()), ResourceTy::Value(src), false);
    }
    // A field moved out (`let s = make_q().a;`, or by a struct update,
    // `let q = P { n: 1, ..make_p() };`) is drawn from this column. A
    // pattern's bindings stay drawn from the call: an arm can't draw a
    // move out of a column that isn't its own (the temporary isn't split
    // into the arms).
    if matches!(part, Some(TempPart::Field(_) | TempPart::UpdateBase(_))) {
      self.temp_cols.insert(expr.hir_id, tmp.clone());
    }
    // So is what a `let` / `let`-`else` pattern or a destructuring
    // assignment takes out of a call's result (`let (d, _) = pair();`,
    // `let Ok(s) = res() else { .. }`): it is the temporary's, not the
    // call's, which no longer has it (#1386, #1481). A `let`-`else` drawn
    // as a branch: the temporary's column forks with the `else` block
    // (`visit_let_else`). (Not a `let`-`else` with a `ref` binding, which
    // borrows from a temporary that lives to the end of the block.)
    let by_value = matches!(self.tcx.parent_hir_node(self.field_top(expr).hir_id), rustc_hir::Node::LetStmt(l) if self.binds_by_value_only(l.pat));
    if (matches!(part, Some(TempPart::Let(_) | TempPart::Assign(_)))
        || (matches!(part, Some(TempPart::LetElse(_))) && by_value))
      && matches!(expr.kind, ExprKind::Call(..) | ExprKind::MethodCall(..)) && !ctor_literal {
      self.temp_cols.insert(expr.hir_id, tmp.clone());
    }
    // A `match` / `if let` / `while let` / let-chain scrutinee: its column
    // forks into the arms, and what a pattern binds by value out of it is
    // drawn moving out of it, what it binds by reference borrowing from
    // it (#1386).
    if tmp.is_split_scrutinee_temporary() {
      let emptiers = self.scrutinee_temp_emptiers(expr);
      if !emptiers.is_empty() {
        self.temp_emptiers.insert(*tmp.hash(), emptiers);
      }
      // A tuple literal or a constructor a pattern takes apart element
      // by element (`literal_pat_parents`): each element it is built
      // from (`make()` in `((make(), 1), make())`, `Some(make())`).
      if self.literal_elems(expr).is_some() {
        let mut leaves = Vec::new();
        self.literal_leaves(expr, &mut leaves);
        // (A variable in it is drawn moving into the temporary: `into`.)
        for l in leaves.into_iter().filter(|l| !is_place_expr(l)) {
          self.temp_cols.insert(l.hir_id, tmp.clone());
        }
      }
      self.temp_cols.insert(expr.hir_id, tmp.clone());
    }
    // Plain text: `hover_messages::styled_name_html` escapes it for the tooltip.
    let id = *self.unique_id;
    self.add_external_event(end_line, ExternalEvent::TemporaryDrop { ro: tmp.clone(), ty: Some(ty), when: when.to_owned(), literal: false, early: false, part, id });
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
  fn note_for_head_temporary(&mut self, head: &'tcx Expr<'tcx>, iter_call: &'tcx Expr<'tcx>, for_expr: &'tcx Expr<'tcx>) -> Option<ResourceAccessPoint> {
    let typeck = self.tcx.typeck(iter_call.hir_id.owner);
    let ty = typeck.expr_ty(iter_call);
    if ty.is_ref() || !self.needs_drop(ty, iter_call.hir_id.owner) || self.temp_drops_noted.contains(&head.hir_id) {
      return None;
    }
    // `for s in it` with `it` an iterator already: `into_iter` hands
    // `it`'s value back unchanged, so no new value is built. It's moved
    // into the loop, and its drop is stated on its own column (#482).
    if matches!(head.kind, ExprKind::Path(QPath::Resolved(_, p)) if matches!(p.res, Res::Local(_)))
      && typeck.expr_ty(head) == ty
    {
      return None;
    }
    let end_line = span_end_line(&for_expr.span, &self.tcx);
    // An adapter chain's type (`Map<IntoIter<String>, {closure@..}>`)
    // isn't something a student wrote.
    let shown = strip_module_paths(&ty.to_string());
    let shown = if shown.contains('{') { "iterator".to_owned() } else { shown };
    let elements = vec_macro_elems(self.tcx, head).and_then(|_| self.literal_moved_places(head)).unwrap_or_default();
    self.record_temporary_drop(head, shown, "into_iter".to_owned(), "when the `for` loop ends", end_line, None, elements, None, true)
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
        // A Copy field read, or a field that is itself borrowed or
        // discarded (`make_q().a.len()`), leaves the whole temporary to
        // drop; moving a field out leaves only the rest (`temp_part_moved`).
        ExprKind::Field(base, _) if is_me(base) => {
          (self.tcx.type_is_copy_modulo_regions(env, typeck.expr_ty(p)) || self.is_unmoved_temporary(p, typeck, env))
            && !self.extended_by_let(p)
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
        // `P { name: s, ..make_p() }` where the fields it takes from the
        // base are all Copy: they are copied out, the base is left whole
        // (#605).
        ExprKind::Struct(_, fields, rustc_hir::StructTailExpr::Base(b)) if is_me(b) => {
          match typeck.expr_ty(expr).kind() {
            rustc_middle::ty::TyKind::Adt(def, args) if def.is_struct() => def.non_enum_variant().fields.iter()
              .filter(|f| !fields.iter().any(|ef| ef.ident.name == f.name))
              .all(|f| self.tcx.type_is_copy_modulo_regions(env, f.ty(self.tcx, args))),
            _ => false,
          }
        }
        ExprKind::Match(scrut, arms, rustc_hir::MatchSource::Normal) if is_me(scrut) => {
          let mut moves_out = false;
          for arm in arms {
            arm.pat.each_binding(|ann, hid, _, _| {
              if ann.0 == rustc_hir::ByRef::No && self.needs_drop_in(typeck.node_type(hid), env) {
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
            if ann.0 == rustc_hir::ByRef::No && self.needs_drop_in(typeck.node_type(hid), env) {
              moves_out = true;
            }
          });
          !moves_out
        }
        // `make()` in `if let (Some(a), _) = (x, make())`: an element of
        // a tuple scrutinee that is drawn as "the temporary tuple" (#327)
        // has no column of its own, so when no pattern takes anything
        // out of it, it is drawn like any other scrutinee temporary
        // (#942).
        ExprKind::Tup(_) => !matches!(expr.kind, ExprKind::Field(..)) && self.unbound_tuple_scrutinee_elem(expr, typeck, env),
        _ => false,
      },
      _ => false,
    }
  }

  /// `make()` in `if let (Some(a), _) = (x, make())` / `match (x,
  /// make()) { .. }`: an element (also inside a nested tuple literal) of
  /// a tuple-literal scrutinee that `is_drawn_tuple_scrutinee`. Its field
  /// path in the tuple, the patterns matched against the tuple, and
  /// whether they are a `match`'s arms (else an `if let` / `while let`'s
  /// one pattern). None for anything else (#942).
  fn tuple_scrutinee_elem(&self, elem: &'tcx Expr<'tcx>) -> Option<(Vec<String>, Vec<&'tcx Pat<'tcx>>, bool)> {
    use rustc_hir::Node;
    if elem.span.from_expansion() || matches!(elem.kind, ExprKind::Tup(_)) {
      return None;
    }
    let mut path: Vec<String> = Vec::new();
    let mut top = elem;
    while let Node::Expr(p) = self.tcx.parent_hir_node(top.hir_id) {
      let ExprKind::Tup(elems) = p.kind else { break };
      if p.span.from_expansion() {
        return None;
      }
      path.insert(0, elems.iter().position(|e| e.hir_id == top.hir_id)?.to_string());
      top = p;
    }
    if path.is_empty() || !self.is_drawn_tuple_scrutinee(top) {
      return None;
    }
    let Node::Expr(m) = self.tcx.parent_hir_node(top.hir_id) else { return None };
    match m.kind {
      ExprKind::Match(_, arms, _) => Some((path, arms.iter().map(|a| a.pat).collect(), true)),
      ExprKind::Let(l) => Some((path, vec![l.pat], false)),
      _ => None,
    }
  }

  /// The patterns matched against the scrutinee temporary of `expr` (a
  /// `match`'s arms, an `if let`'s / `while let`'s one), the place of
  /// the temporary in what they match, and whether a field beside the one
  /// matched needs drop (`match make_p().name { .. }` with another
  /// `String` field: the temporary always keeps it).
  fn scrutinee_temp_pats(&self, expr: &'tcx Expr<'tcx>) -> Option<(Vec<String>, Vec<&'tcx Pat<'tcx>>, bool)> {
    use rustc_hir::Node;
    let typeck = self.tcx.typeck(expr.hir_id.owner);
    let env = typing_env(self.tcx, expr.hir_id.owner);
    let (path, pats, kept) = match self.tuple_scrutinee_elem(expr) {
      Some((path, pats, _)) => (path, pats, false),
      None => {
        let top = self.field_top(expr);
        let mut kept = false;
        let mut e = top;
        while let ExprKind::Field(base, f) = e.kind {
          kept |= self.other_fields_need_drop(typeck.expr_ty(base), f.as_str(), env);
          e = base;
        }
        let pats: Vec<&'tcx Pat<'tcx>> = match self.tcx.parent_hir_node(top.hir_id) {
          Node::Expr(p) => match p.kind {
            ExprKind::Match(s, arms, rustc_hir::MatchSource::Normal) if s.hir_id == top.hir_id => arms.iter().map(|a| a.pat).collect(),
            ExprKind::Let(l) if l.init.hir_id == top.hir_id => vec![l.pat],
            _ => return None,
          },
          _ => return None,
        };
        // `Err(e)` on `Ok(make())` can't match (#807).
        let pats = pats.into_iter().filter(|p| !self.ctor_pat_mismatch(p, top)).collect();
        (Vec::new(), pats, kept)
      }
    };
    Some((path, pats, kept))
  }

  /// For the temporary of `expr`, a `match` / `if let` / `while let` /
  /// let-chain scrutinee: the by-value bindings of each of its patterns
  /// that, when it matches, takes everything in it that needs drop (one
  /// group per pattern; #1386). The last of a group's moves out of it ends
  /// the temporary's leg in that arm (`temp_emptying_moves`).
  fn scrutinee_temp_emptiers(&self, expr: &'tcx Expr<'tcx>) -> Vec<Vec<String>> {
    let Some((path, pats, kept)) = self.scrutinee_temp_pats(expr) else { return Vec::new() };
    let typeck = self.tcx.typeck(expr.hir_id.owner);
    let env = typing_env(self.tcx, expr.hir_id.owner);
    if kept {
      return Vec::new();
    }
    let mut out = Vec::new();
    for p in &pats {
      let Some(sub) = self.pat_at_path(p, &path, typeck) else { continue };
      // Moves out everything that needs drop.
      if self.path_left(p, &path, typeck, env).is_some() {
        continue;
      }
      let mut names: Vec<String> = Vec::new();
      sub.walk_always(|b| {
        if let PatKind::Binding(_, _, ident, _) = b.kind {
          let by_ref = matches!(typeck.pat_binding_modes().get(b.hir_id).map(|m| m.0), Some(rustc_hir::ByRef::Yes(..)));
          let name = ident.to_string();
          if !by_ref && !self.ty_is_copy(typeck.node_type(b.hir_id), b.hir_id.owner) && !names.contains(&name) {
            names.push(name);
          }
        }
      });
      if !names.is_empty() {
        out.push(names);
      }
    }
    out
  }

  /// True iff `elem` is a `tuple_scrutinee_elem` that no pattern binds
  /// anything out of by value: all of it stays in the tuple, and is
  /// dropped with it (#942).
  fn unbound_tuple_scrutinee_elem(
    &self,
    elem: &'tcx Expr<'tcx>,
    typeck: &'tcx rustc_middle::ty::TypeckResults<'tcx>,
    env: rustc_middle::ty::TypingEnv<'tcx>,
  ) -> bool {
    self.tuple_scrutinee_elem(elem)
      .is_some_and(|(path, pats, _)| pats.iter().all(|p| self.path_left(p, &path, typeck, env) == Some(false)))
  }

  /// `temp_part_moved` for a `tuple_scrutinee_elem` that a pattern binds
  /// some or all of by value (`(None, m)` on `(x, make())`, #942): what
  /// is left of it, if anything, is dropped with the tuple. That is
  /// something when an arm leaves some of it, or the `if let` pattern
  /// may not match (then all of it is left). A field of a call's result
  /// (`make_p().name`, `field`; `left` when a field beside it needs
  /// drop) is moved into the tuple, which is dropped with the call's
  /// temporary: drawn as the temporary's field that the pattern is
  /// matched against (#956).
  fn tuple_scrutinee_elem_part(
    &self,
    elem: &'tcx Expr<'tcx>,
    field: Option<String>,
    left: bool,
    typeck: &'tcx rustc_middle::ty::TypeckResults<'tcx>,
    env: rustc_middle::ty::TypingEnv<'tcx>,
  ) -> Option<TempPart> {
    use rustc_hir::Node;
    let (path, pats, is_match) = self.tuple_scrutinee_elem(elem)?;
    let leaves = left || pats.iter().any(|p| self.path_left(p, &path, typeck, env).is_some());
    // `(z, make_q().name)`: the literal moves the field out of the
    // temporary into the tuple, before any pattern is matched; the
    // pattern never sees the rest of the temporary (#1350).
    if let Some(field) = &field {
      let tuple = self.tcx.hir_parent_iter(elem.hir_id)
        .map_while(|(_, n)| match n { Node::Expr(t) if matches!(t.kind, ExprKind::Tup(_)) => Some(t), _ => None })
        .last()
        .and_then(|t| self.tcx.sess.source_map().span_to_snippet(t.span).ok());
      if let Some(tuple) = tuple {
        if !leaves && !pats.iter().any(|p| pat_refutable(p)) {
          return None;
        }
        return Some(TempPart::TupleField(field.clone(), tuple));
      }
    }
    if is_match {
      return leaves.then_some(TempPart::Match(field));
    }
    if !leaves && !pats.iter().any(|p| pat_refutable(p)) {
      return None;
    }
    let is_while = self.tcx.hir_parent_iter(elem.hir_id).find_map(|(_, n)| match n {
      Node::Expr(i) if matches!(i.kind, ExprKind::If(..)) => Some(i),
      _ => None,
    }).is_some_and(|i| matches!(i.span.desugaring_kind(), Some(rustc_span::DesugaringKind::WhileLoop)));
    Some(if is_while { TempPart::WhileLet(field) } else { TempPart::IfLet(field) })
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
        // A match guard is a temporary scope of its own (#1167): its
        // temporaries are dropped once it has been evaluated, before
        // the arm's body (or the next arm) runs. (Not an `if let`
        // guard's scrutinee, which, as in a let chain, may live on into
        // the arm's body.)
        Node::Arm(a) if a.guard.is_some_and(|g| g.hir_id == cur) => {
          let g = a.guard?;
          let has_let = matches!(g.kind, ExprKind::Let(_)) || is_let_chain(g);
          if !has_let {
            return Some(("at the end of the match guard", end(g.span), None));
          }
        }
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
          ExprKind::If(c, then, els) if c.hir_id == cur && matches!(c.kind, ExprKind::Let(_)) => {
            if matches!(e.span.desugaring_kind(), Some(rustc_span::DesugaringKind::WhileLoop)) {
              return Some(("at the end of each iteration of this `while let` loop", end(e.span), None));
            }
            if expr.span.edition() >= rustc_span::edition::Edition::Edition2024 {
              // Edition 2024: dropped before the `else` block runs, or
              // when the matching block ends.
              // With no `else` there is no block to name (#776).
              let w = if els.is_some() {
                "before the `else` block of this `if let` runs, or when the block it matched ends"
              } else {
                "when the pattern of this `if let` doesn't match (before the code after the `if` runs), or when the block it matched ends"
              };
              return Some((w, end(then.span), None));
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
          ExprKind::If(c, then, els) if c.hir_id == cur && is_let_chain(c) => {
            if matches!(e.span.desugaring_kind(), Some(rustc_span::DesugaringKind::WhileLoop)) {
              return Some(("at the end of each iteration of this `while let` loop", end(e.span), None));
            }
            let w = if els.is_some() {
              "before the `else` block of this `if` runs, or when the block its let chain matched ends"
            } else {
              "when the condition of this `if` fails (before the code after the `if` runs), or when the block its let chain matched ends"
            };
            return Some((w, end(then.span), None));
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
  /// that way; the renderer then draws the move out of each variable
  /// whose new value was drawn first before that new value on its
  /// column (`moves_out_first`, #1343). Its old value is never drawn as
  /// dropped.
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

    // `(x, n) = if c { (s, 1) } else { (t, 2) };` (#1253): each branch's
    // value is taken apart and assigned in that branch, as `let (x, n) =
    // if …` binds it (#1007): s moves on its path only, t on the other.
    if matches!(init.kind, ExprKind::If(_, _, Some(_)) | ExprKind::Match(_, _, rustc_hir::MatchSource::Normal))
      && !init.span.from_expansion()
    {
      let mut bodies = Vec::new();
      conditional_branch_bodies(init, &mut bodies);
      for body in bodies {
        self.deferred_arm_binds.entry(body.hir_id).or_insert_with(|| (body, Vec::new())).1.push(ArmBind::Assign(block));
      }
      self.visit_expr(init);
      self.bind_left_arms(init);
      return true;
    }
    // `(x, n) = loop { … break (s, 1); … }` (#1253): the value of the
    // `break` that ended the loop is taken apart and assigned, as
    // `let (x, n) = loop { … }` binds it (#452).
    if matches!(init.kind, ExprKind::Loop(_, _, rustc_hir::LoopSource::Loop, _))
      && !init.span.from_expansion()
      && breaks_to(init).iter().any(|(_, value)| value.is_some())
    {
      let n0 = self.preprocessed_events.len();
      self.break_assigns.push((init.hir_id, block));
      self.visit_expr(init);
      self.break_assigns.retain(|(id, _)| *id != init.hir_id);
      self.drops_before_new_values(n0);
      return true;
    }
    // `(x, n) = 'blk: { if c { break 'blk (s, 1); } (t, 2) };` (#1253):
    // whichever value ended the block, a `break`'s or its last
    // expression, is taken apart and assigned where the block ends, each
    // drawn as one of them, as `let (x, n) = 'blk: { … }` binds it
    // (#1226). (s moves only if that `break` ran, t only if it didn't.)
    if let ExprKind::Block(b, Some(_)) = init.kind {
      let values = crate::expr_visitor_utils::breaks_to_id(init, b.hir_id).iter().filter(|(_, v)| v.is_some()).count()
        + usize::from(b.expr.is_some());
      if values > 1 && !init.span.from_expansion() {
        self.visit_expr(init);
        let n0 = self.preprocessed_events.len();
        let values = self.labeled_block_values(init);
        if values.len() > 1 {
          self.pair_break_alternatives(init, values, |this, v| this.assign_value(block, v));
        } else if let Some((v, _)) = values.first() {
          self.assign_value(block, v);
        }
        self.drops_before_new_values(n0);
        return true;
      }
    }
    // Calls, borrows and reads inside the RHS. A constructor the
    // pattern takes apart (`W(y) = W(g);`) is no actor (#950).
    self.note_let_ctor_literals(local.pat, init);
    self.visit_expr(init);
    self.draw_destructuring_assign(local, init, assigns);
    true
  }

  /// After a destructuring assignment of a `loop`'s or labeled block's
  /// value, whose events start at `preprocessed_events[n0]`: a place's
  /// old value is dropped when the new one is assigned, after the loop
  /// or block: drawn where (and just before) the first new value
  /// arrives, as for `x = loop { … }` (#586), not on the statement's
  /// first line nor on a row of its own after them.
  fn drops_before_new_values(&mut self, n0: usize) {
    let mut i = n0;
    while i < self.preprocessed_events.len() {
      if let ExternalEvent::OwnerDropAtReassign { ro, .. } = &self.preprocessed_events[i].1 {
        let h = *ro.hash();
        let first = (n0..i).find(|&j| matches!(&self.preprocessed_events[j].1,
          ExternalEvent::Move { to: ResourceTy::Value(t), .. }
          | ExternalEvent::Copy { to: ResourceTy::Value(t), .. }
          | ExternalEvent::Bind { to: ResourceTy::Value(t), .. } if *t.hash() == h));
        if let Some(j) = first {
          self.preprocessed_events[i].0 = self.preprocessed_events[j].0;
          self.preprocessed_events[j..=i].rotate_right(1);
        }
      }
      i += 1;
    }
  }

  /// The value of branch body `body` of a destructuring assignment's
  /// right-hand side (the desugared `block`), now visited, assigned to
  /// its places in that branch (#1253). A branch of type `!` gives none.
  pub fn assign_branch_value(&mut self, block: &'tcx rustc_hir::Block<'tcx>, body: &'tcx Expr<'tcx>) {
    let Some(value) = branch_value_expr(body) else { return };
    if self.tcx.typeck(value.hir_id.owner).expr_ty_opt(value).is_none_or(|ty| ty.is_never()) {
      return;
    }
    // Drawn on the branch value's line (`0 => (s, 1),` in a `match`
    // written over several lines), not the statement's first.
    let saved = self.line_override.replace(span_to_line(&value.span, &self.tcx));
    // A literal whose part the pattern leaves (`(a, _) = if c { (f(),
    // g()) } ..`) has a temporary of the branch, as a `let`'s does
    // (#1011, #1387).
    let owner = match body.kind {
      ExprKind::Block(b, _) => b.hir_id,
      _ => body.hir_id,
    };
    let prev_owner = self.branch_value_owner.replace(owner);
    self.assign_value(block, value);
    self.branch_value_owner = prev_owner;
    self.line_override = saved;
    for (line, tmp, ty) in std::mem::take(&mut self.branch_temp_drops) {
      let id = *self.unique_id;
      *self.unique_id += 1;
      self.add_external_event(line, ExternalEvent::TemporaryDrop { ro: tmp, ty: Some(ty), when: "at the end of this statement".to_owned(), literal: true, early: false, part: None, id });
    }
  }

  /// `value` (a branch's, or what a `break` carries out of a loop)
  /// taken apart and assigned to the places of the destructuring
  /// assignment `block` desugars to (#1253).
  fn assign_value(&mut self, block: &'tcx rustc_hir::Block<'tcx>, value: &'tcx Expr<'tcx>) {
    let Some(Stmt { kind: StmtKind::Let(local), .. }) = block.stmts.first() else { return };
    // Assigned where the value is (a lone `break (s, 1);`), unless the
    // caller placed it (a branch's value line, a loop's end).
    let saved = self.line_override;
    self.line_override = saved.or_else(|| Some(span_to_line(&value.span, &self.tcx)));
    let mut assigns: Vec<(&'tcx Expr<'tcx>, &'tcx Expr<'tcx>, HirId)> = Vec::new();
    for stmt in &block.stmts[1..] {
      if let StmtKind::Semi(e) | StmtKind::Expr(e) = stmt.kind {
        if let ExprKind::Assign(place, rhs, _) = e.kind {
          if let Some(hid) = local_of_path(rhs) {
            assigns.push((e, place, hid));
          }
        }
      }
    }
    self.draw_destructuring_assign(local, value, assigns);
    self.line_override = saved;
  }

  /// The assignments `x = lhs; …` of a destructuring assignment whose
  /// pattern (`local`) takes `init` apart.
  fn draw_destructuring_assign(&mut self, local: &'tcx LetStmt<'tcx>, init: &'tcx Expr<'tcx>, assigns: Vec<(&'tcx Expr<'tcx>, &'tcx Expr<'tcx>, HirId)>) {
    let mut pairs: Vec<(HirId, &'tcx Expr<'tcx>)> = Vec::new();
    let mut discarded: Vec<&'tcx Expr<'tcx>> = Vec::new();
    self.pair_destructure(local.pat, init, &mut pairs, &mut discarded);
    // `_ = s;` matches the place itself against `_`, which binds
    // nothing, so nothing is moved (as for `let _ = s;`, #1169). Only a
    // part of a literal the RHS builds (`(a, _) = (s, t)`) is moved, by
    // the literal.
    if matches!(local.pat.kind, PatKind::Wild) {
      discarded.clear();
    }
    let src_of: HashMap<HirId, &'tcx Expr<'tcx>> = pairs.iter().cloned().collect();
    // `Pair { a: s, b: t } = p;` / `(s, n) = t;`: each part of the place
    // goes to its own destination, as in `let Pair { a, b } = p;` (#1412).
    let parts: HashMap<HirId, (String, bool)> = self.destructure_member_parts(local.pat, init)
      .unwrap_or_default().into_iter().collect();

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
    for (hid, e) in &pairs {
      match parts.get(hid) {
        Some((part, true)) => { moved.insert(part.clone()); }
        Some((_, false)) => {}
        None => if let Some(n) = moved_name(self, e) { moved.insert(n); },
      }
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
      .map(|(_, _, hid)| match parts.get(hid) {
        Some((part, moves)) => moves.then(|| part.clone()),
        None => src_of.get(hid).and_then(|e| moved_name(self, e)),
      }).collect();
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
          // A reference part of the place (`h.r` in `Holder { r: x, .. } =
          // h;`, #1412) lends what it borrows.
          let data = match parts.get(hid).and_then(|(n, _)| self.borrow_map.get(n)) {
            Some(rd) => (rd.lender.clone(), get_aliasing_data(&rd.lender, &self.borrow_map), rd.static_lender.clone(), rd.co_lenders.clone()),
            None => self.get_ref_data_full(src),
          };
          ref_data.insert(*hid, data);
          if parts.contains_key(hid) {
            continue;
          }
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
    self.destructure_parts = parts.into_iter().map(|(h, (n, _))| (h, n)).collect();
    self.destructure_moved = moved;
    self.destructure_ref_data = ref_data;
    self.destructure_ref_src = ref_src;
    // Parts nothing is assigned to (`(a, _) = (s, t)`) are moved
    // into the temporary and dropped with it, at the end of the
    // statement. The temporary has no column, so the move's tooltip
    // is where that drop is stated (unless the type has nothing to
    // drop).
    // A fresh value among them (`(a, _) = (make(), make());`, #1387)
    // has no variable to say so: the literal's temporary gets a column,
    // as a `let`'s does (#483), and what is moved in goes there. (rustc
    // lowers the assignment to `{ let (lhs, _) = (..); a = lhs; }`.)
    let in_branch = self.branch_value_owner.is_some();
    // (`init` is the right-hand side, a branch's value or a `break`'s.)
    let literal_init = match init.kind {
      ExprKind::Tup(_) | ExprKind::Array(_) | ExprKind::Struct(..) => true,
      ExprKind::Call(callee, _) => is_ctor_path(callee, self.tcx.typeck(init.hir_id.owner)),
      _ => false,
    };
    let fresh = discarded.iter().copied().find(|e| !e.span.from_expansion() && !matches!(e.kind,
      ExprKind::Path(_) | ExprKind::Field(..) | ExprKind::Index(..) | ExprKind::Unary(UnOp::Deref, _))
      && self.needs_drop(self.tcx.typeck(e.hir_id.owner).expr_ty(e), e.hir_id.owner));
    // (In a branch, the temporary is the literal's, `literal_temp_key`.)
    let prev_existing = self.bind_existing;
    self.bind_existing |= in_branch;
    let column = match fresh {
      Some(e) if literal_init => {
        // What a call among them returns is moved into it, as in the
        // `let` form (#1376).
        let mut calls = Vec::new();
        for &d in &discarded {
          if !matches!(d.kind, ExprKind::Path(_) | ExprKind::Field(..) | ExprKind::Index(..) | ExprKind::Unary(UnOp::Deref, _)) {
            self.literal_calls(d, &mut calls);
          }
        }
        self.start_literal_temp(e, false, calls);
        self.literal_temps.contains_key(&if in_branch { init.hir_id } else { local.hir_id })
      }
      _ => false,
    };
    for e in &discarded {
      if moved_name(self, e).is_some() {
        if column {
          self.move_into_literal(e, false);
          continue;
        }
        let ty = self.tcx.typeck(e.hir_id.owner).node_type(e.hir_id);
        let needs_drop = self.needs_drop(ty, e.hir_id.owner);
        let to = if needs_drop { ResourceTy::Temporary { extended: false, tuple: None } } else { ResourceTy::Anonymous };
        self.match_rhs(to, e, Evt::Move);
      }
    }
    let n0 = self.preprocessed_events.len();
    self.bind_existing = prev_existing;
    for &i in &order {
      self.visit_expr(assigns[i].0);
    }
    // Every part is read before any is assigned, so in a cycle each
    // destination's old value leaves before its new one arrives, though
    // the cycle's first assignment is drawn before the move out of its
    // destination: the renderer puts that move first on the
    // destination's column (#1343). So for a struct's non-Copy fields,
    // which move with it. Each such destination gets a second
    // "acquires" event here, so that the events say it owns a value
    // after the statement (`rap_holds_resource_now`); the renderer
    // leaves it out, as the reordered move already ends with the
    // acquire.
    let line = span_to_line(&local.span, &self.tcx);
    let mut stack: Vec<String> = reacquire;
    stack.reverse();
    let mut hashes: Vec<u64> = Vec::new();
    while let Some(name) = stack.pop() {
      if let Some(rd) = self.raps.get(&name) {
        let rap = rd.rap.clone();
        hashes.push(*rap.hash());
        self.moves_out_first.push(*self.unique_id);
        self.add_ev(line, Evt::Bind, ResourceTy::Value(rap), ResourceTy::Anonymous, false);
        let mut children: Vec<String> = self.struct_children(&name).into_iter()
          .filter(|c| !c.never_drops())
          .map(|c| c.name().clone())
          .collect();
        children.reverse();
        stack.extend(children);
      }
    }
    for (_, ev) in &self.preprocessed_events[n0..] {
      if let ExternalEvent::Move { from: ResourceTy::Value(f), id, .. } = ev {
        if hashes.contains(f.hash()) {
          self.moves_out_first.push(*id);
        }
      }
    }
    self.destructure_srcs.clear();
    self.destructure_parts.clear();
    self.destructure_moved.clear();
    self.destructure_ref_data.clear();
    self.destructure_ref_src.clear();
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
        if is_ctor_path(callee, self.tcx.typeck(src.hir_id.owner)) =>
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

  /// `Pair { a: s, b: t } = p;`, `(s, n) = t;`, `(s, _) = t;`: with the
  /// right-hand side a place whose parts have columns (`member_place_name`),
  /// each binding of the desugared pattern (an `lhs`) with the part it
  /// takes, and whether that moves it (else it's a copy), as `let` binds
  /// them (`bind_pat_from_member`, #1412). A part the pattern leaves
  /// (`_`, `..`) stays where it is. `None` (the whole place is assigned
  /// from, as before) unless every binding takes a part of its own that
  /// isn't a reference.
  fn destructure_member_parts(&self, pat: &'tcx Pat<'tcx>, src: &'tcx Expr<'tcx>) -> Option<Vec<(HirId, (String, bool))>> {
    if !matches!(pat.kind, PatKind::Tuple(..) | PatKind::TupleStruct(..) | PatKind::Struct(..)) {
      return None;
    }
    let place = self.member_place_name(src)?;
    let ty = self.tcx.typeck(src.hir_id.owner).node_type(src.hir_id);
    let parent = ResourceTy::Value(self.raps.get(&place)?.rap.to_owned());
    let mut out = Vec::new();
    self.member_parts_of(pat, &parent, ty, src.hir_id.owner, &mut out)?;
    (!out.is_empty()).then_some(out)
  }

  fn member_parts_of(&self, pat: &'tcx Pat<'tcx>, parent: &ResourceTy, ty: rustc_middle::ty::Ty<'tcx>,
                     owner: rustc_hir::OwnerId, out: &mut Vec<(HirId, (String, bool))>) -> Option<()> {
    for (sp, part) in self.pat_parts(pat, parent, ty)? {
      match (sp.kind, part) {
        (PatKind::Wild, _) => {}
        (PatKind::Binding(ann, hid, _, None), Some((ResourceTy::Value(r), part_ty)))
          if matches!(ann.0, rustc_hir::ByRef::No) =>
          out.push((hid, (r.name().clone(), !self.ty_is_copy(part_ty, owner)))),
        (PatKind::Tuple(..) | PatKind::TupleStruct(..) | PatKind::Struct(..), Some((p @ ResourceTy::Value(_), part_ty))) =>
          self.member_parts_of(sp, &p, part_ty, owner, out)?,
        _ => return None,
      }
    }
    Some(())
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
    // A let-else's `else` block, visited here (#906).
    els: Option<&'tcx rustc_hir::Block<'tcx>>,
  ) {
    // `let y = dbg!(e);` binds y to e's value: dbg! takes e by value
    // and returns it. Treat it as `let y = e;` so the move / copy /
    // borrow (and, for `dbg!(&x)`, the lender) is e's.
    let init = dbg_single_arg(init).unwrap_or(init);
    if self.unfinished_vec(init) {
      if !is_skipped { self.visit_expr(init); }
      return;
    }
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
      if let Some(els) = els {
        self.visit_block(els);
      }
      return;
    }
    // `let r = if c { &String::from("x") } else { &s };` (#1397).
    let branch_temps = if is_skipped { Vec::new() } else { self.declare_branch_temps(init) };
    // `let x = Some(if c { a } else { b });` (#1225): a constructor
    // takes nothing itself (#768), so x is bound from each branch's
    // value in its branch, as for `let x = if …`.
    let ctor_cond = (!is_skipped && els.is_none() && matches!(pat.kind, PatKind::Binding(_, _, _, None)))
      .then(|| self.conditional_value(init)).flatten()
      .filter(|c| c.hir_id != init.hir_id);
    // `let t = (if c { d } else { e }, 1);` / `W { s: match o { … } }`
    // (#1256): an element or field that is an `if` / `match` gives its
    // part of the value from each branch, in that branch, so the literal
    // is bound before it is visited, as a conditional initializer is.
    let elem_conds = !is_skipped && els.is_none() && ctor_cond.is_none() && self.literal_has_conditional_part(init);
    let whole_binding = matches!(pat.kind, PatKind::Binding(_, _, _, None));
    let init_is_branching = !is_skipped
      && (ctor_cond.is_some() || elem_conds || match init.kind {
        ExprKind::If(..) => true,
        // `let r = loop { … break k; }`: the `break`s hand `r` its
        // value as they're visited (#431), so `r` is bound first.
        ExprKind::Loop(_, _, rustc_hir::LoopSource::Loop, _) => breaks_to(init).iter().any(|(_, value)| value.is_some()),
        ExprKind::Match(_, _, rustc_hir::MatchSource::TryDesugar(_)) => false,
        ExprKind::Match(..) => true,
        _ => false,
      });
    if init_is_branching {
      // `let (a, b) = loop { … break (x, y); }`: each `break` value is
      // bound against the pattern (#452).
      if matches!(init.kind, ExprKind::Loop(..)) && !matches!(pat.kind, PatKind::Binding(..)) {
        self.break_pats.push((init.hir_id, pat, self.current_scope, self.inside_branch));
      } else {
        // A branch whose value is a variable declared in it is paired
        // once it has been visited (#994).
        let prev = self.defer_arm_binds.take();
        let conditional = matches!(init.kind, ExprKind::If(..) | ExprKind::Match(..));
        if let Some(cond) = ctor_cond {
          self.defer_arm_binds = Some(cond.span);
          self.bind_pat(pat, cond, is_skipped);
        } else if elem_conds {
          // The other parts are bound once the literal is visited.
          // (A pattern taking the literal apart, `let (x, n) = (if …,
          // 1);`, pairs its parts one by one: all are bound here.)
          self.defer_arm_binds = Some(init.span);
          self.literal_parts = whole_binding.then_some(true);
          self.bind_pat(pat, init, is_skipped);
          self.literal_parts = None;
        } else if conditional && matches!(pat.kind, PatKind::Binding(_, _, _, None)) {
          self.defer_arm_binds = Some(init.span);
          self.bind_pat(pat, init, is_skipped);
        } else if conditional && self.destructures_by_value(pat, init) {
          // `let (a, b) = if …`: each branch's value is taken apart
          // in that branch (#1007).
          self.defer_arm_binds = Some(init.span);
          self.declare_pat_bindings(pat, init);
          self.bind_pat_arms(pat, init);
        } else {
          self.bind_pat(pat, init, is_skipped);
        }
        self.defer_arm_binds = prev;
      }
      self.visit_expr(init);
      if let (true, PatKind::Binding(_, hir_id, _, None)) = (elem_conds, pat.kind) {
        if let Some(rap) = self.raps.get(&binding_name(&self.tcx, hir_id)).map(|r| r.rap.to_owned()) {
          self.literal_parts = Some(false);
          self.match_rhs(ResourceTy::Value(rap), init, Evt::Move);
          self.literal_parts = None;
        }
      }
      let cond = ctor_cond.unwrap_or(init);
      self.bind_left_arms(cond);
      // A reference returned by a call in a branch keeps that call's
      // borrow (#337).
      if let PatKind::Binding(_, hir_id, _, None) = pat.kind {
        let name = binding_name(&self.tcx, hir_id);
        // (`let m: &mut String = if c { let w = &mut s; w } else { .. }`
        // reborrows through w, gone now, #1431.)
        if matches!(init.kind, ExprKind::If(..) | ExprKind::Match(..)) && self.raps.get(&name).is_some_and(|r| r.rap.is_ref()) {
          self.resolve_branch_lenders(&name, Vec::new());
        }
        self.mark_branch_results_kept(&name, cond);
      }
      self.break_pats.retain(|(id, ..)| *id != init.hir_id);
      self.break_binds.retain(|(id, ..)| *id != init.hir_id);
      if let Some(els) = els {
        self.visit_block(els);
      }
    } else {
      let mut nested_temps = Vec::new();
      let init_len = self.in_scope.len();
      if !is_skipped {
        self.note_let_ctor_literals(pat, init);
        self.visit_expr(init);
        nested_temps = self.bind_nested_extended_temps(pat, init);
      }
      // The `else` block runs when the pattern doesn't match, before
      // anything is bound. Drawn as a branch, the bindings happen
      // where it joins: on the path that skipped it.
      let joined_at = match els {
        Some(els) if !is_skipped => self.visit_let_else(pat, init, els, init_len),
        Some(els) => {
          self.visit_block(els);
          None
        }
        None => None,
      };
      let saved_line = joined_at.map(|l| self.line_override.replace(l));
      let first_id = *self.unique_id;
      // `let (x, n) = 'blk: { if c { break 'blk (s, 1); } (t, 2) };`
      // (#1226): the pattern takes apart whichever value ended the
      // block, a `break`'s or its last expression, each drawn as one of
      // them where the block ends (as a plain binding is, #1134).
      let block_values = (!is_skipped && els.is_none() && !matches!(pat.kind, PatKind::Binding(..))
        && matches!(init.kind, ExprKind::Block(_, Some(_))))
        .then(|| self.labeled_block_values(init)).filter(|v| v.len() > 1);
      if let Some(values) = block_values {
        self.pair_break_alternatives(init, values, |this, v| this.bind_pat(pat, v, false));
      } else {
        self.bind_pat(pat, init, is_skipped);
      }
      if let Some(l) = saved_line {
        self.line_override = l;
      }
      // A let-else drawn as a branch binds on the join row (above), so
      // a borrow its pattern binds starts there: it can't be drawn
      // ending before, on the `let` row where MIR ends an unused
      // binding's loan (`let Some(_x) = &o else { .. };`, #1408).
      if let Some(join) = joined_at {
        pat.each_binding(|_, hid, _, _| {
          let floor = self.borrow_floor.entry(binding_name(&self.tcx, hid)).or_insert(join);
          *floor = (*floor).max(join);
        });
      }
      // The temporaries the initializer keeps alive are declared with
      // the pattern's bindings (#856).
      if !nested_temps.is_empty() {
        let mut names = Vec::new();
        pat.each_binding(|_, hid, _, _| names.push(binding_name(&self.tcx, hid)));
        for n in names {
          if let Some(rd) = self.raps.get_mut(&n) {
            rd.extended_temp.extend(nested_temps.iter().cloned());
          }
        }
      }
      // `let (c, d) = *r;`: c and d copy values inside `*r`, as in
      // `match *r` (#886).
      if !is_skipped && !matches!(pat.kind, PatKind::Binding(..) | PatKind::Wild) {
        if let Some((r, pointee_ty)) = self.deref_place_target(init) {
          self.mark_copy_deref(pat, &r, pointee_ty, first_id);
        }
      }
    }
    // The temporaries its branches made go out of scope with the
    // pattern's bindings, as #856's do.
    let made: Vec<String> = branch_temps.into_iter().filter(|n| self.raps.contains_key(n)).collect();
    if !made.is_empty() {
      let mut names = Vec::new();
      pat.each_binding(|_, hid, _, _| names.push(binding_name(&self.tcx, hid)));
      for n in names {
        if let Some(rd) = self.raps.get_mut(&n) {
          rd.extended_temp.extend(made.iter().cloned());
        }
      }
    }
    self.keep_literal_temp_with_refs(pat);
  }

  /// The `else` block of `let pat = init else { … };` (#906), visited
  /// after `init` and before `pat` binds anything: it runs when the
  /// pattern doesn't match, and can't finish (`return`, a panic, `break`
  /// / `continue`), so what's in scope there is dropped at its exit
  /// (`drop_locals_on_exit`), the scrutinee too: it still owns its value.
  /// Drawn as the body of `if no_match { return; }` is (#429): a branch
  /// whose column ends in those drops, beside the path on which the
  /// pattern matched (named after the pattern).
  /// With no drop there, no move out of an outer variable and no borrow
  /// that ends there but lives on past it (#1573), it's drawn inline,
  /// as before.
  ///
  /// `init_len`: the length `in_scope` had before `init` was visited.
  /// The unbound temporaries `init` made are dropped before the `else`
  /// block runs, so its exit doesn't drop them again. Returns the line
  /// the branches join on, when they're drawn.
  fn visit_let_else(&mut self, pat: &'tcx Pat<'tcx>, init: &'tcx Expr<'tcx>, els: &'tcx rustc_hir::Block<'tcx>, init_len: usize) -> Option<usize> {
    let sm = self.tcx.sess.source_map();
    let split = sm.lookup_char_pos(pat.span.lo()).line;
    let merge = sm.lookup_char_pos(els.span.hi()).line;
    let block_lo = sm.lookup_char_pos(els.span.lo()).line;
    let saved_scope = self.in_scope.clone();
    let mut idx = 0;
    self.in_scope.retain(|n| {
      idx += 1;
      idx <= init_len || !self.raps.get(n).is_some_and(|rd| rd.rap.is_unbound_temporary())
    });
    let entry_len = self.in_scope.len();
    let lo = *self.unique_id;
    let hash_lo = self.rap_hashes;
    // The `else` path never gets past the block: what it assigns to a
    // `let x;` local (#234) doesn't count after the `let`.
    let uninit_before = self.uninit_locals.clone();
    let maybe_init_before = self.maybe_init_locals.clone();
    let prev_inside_branch = std::mem::replace(&mut self.inside_branch, true);
    self.exit_frames.push(ExitFrame::Branch { len: entry_len, hide: Vec::new(), binds: Vec::new(), binds_at: 0, fn_level: false });
    let arm_borrows = self.arm_borrows_begin();
    self.arm_frames.push((split, merge, block_lo, merge));
    self.visit_block(els);
    self.arm_frames.pop();
    self.arm_borrows_end(arm_borrows);
    let inner = self.raps_declared_since(hash_lo, &uninit_before);
    self.exit_frames.pop();
    self.in_scope = saved_scope;
    self.inside_branch = prev_inside_branch;
    self.uninit_locals = uninit_before;
    self.maybe_init_locals = maybe_init_before;
    let ids = lo..*self.unique_id;
    // Drawn inline when the block changes nothing the matched path
    // sees: no drop at its exit, and no move out of a variable declared
    // before it (`else { drop(s); return; }`, #1133: drawn inline, s
    // read as moved on the matched path too).
    // Nor a borrow made before it that is live after it, as for `if c
    // { return; }` (#882): on the `else` path it ends as the block
    // starts, so a lender the block reads is no longer borrowed there
    // (#1573). (A one-line `else { return }` that draws nothing of its
    // own stays inline: nothing in it can be drawn against the borrow,
    // and a one-row fork binds the pattern a sub-row above its join,
    // #1594.)
    let moves_outer = |e: &ExternalEvent| matches!(e, ExternalEvent::Move { from: ResourceTy::Value(r), .. }
      if !r.is_fn() && !r.is_temporary() && *r.hash() < hash_lo as u64);
    let draws_something = self.preprocessed_events.iter().any(|(_, e)| ids.contains(&e.get_id()));
    if !self.preprocessed_events.iter().any(|(_, e)| ids.contains(&e.get_id())
      && (self.exit_drops.contains(&e.get_id()) || moves_outer(e)))
      && !((draws_something || !branch_is_collapsed(split, merge))
        && self.borrow_may_end_at_exit(BranchBody::LetElse(els), init, split, merge, &inner))
    {
      return None;
    }
    let mut else_ev = self.take_branch_events(ids, None, &HashSet::new());
    let in_branch: HashSet<usize> = else_ev.iter().map(|(_, e)| e.get_id()).collect();
    for evs in self.event_line_map.values_mut() {
      evs.retain(|e| !in_branch.contains(&e.get_id()));
    }
    let mut liveness: HashSet<ResourceAccessPoint> = HashSet::new();
    collect_event_live_vars(&else_ev, &mut liveness);
    liveness.extend(self.keep_exit_drops(&mut else_ev, true));
    let forked = self.fork_let_else_temporary(init, split, merge, &mut else_ev);
    let scoped = scoped_in_events(&else_ev, &self.exit_drops);
    let decl = get_decl_of_block(els, &self.tcx, &self.raps);
    let mut decl_vars = &(&decl | &inner) | &scoped;
    liveness = &liveness - &decl_vars;
    // (The forked temporary's drop on the `else` path doesn't make it
    // the block's own.)
    if let Some(t) = forked {
      decl_vars.remove(&t);
      liveness.insert(t);
    }
    let left = self.left_at_exits(self.block_ends_at_exits(els, true), &decl);
    for var in in_decl_order(&decl).into_iter().filter(|v| needs_scope_end(v) && !scoped.contains(v) && !left.contains(v)) {
      else_ev.push((merge, ExternalEvent::GoOutOfScope { ro: var.clone(), id: *self.unique_id, early: None }));
      *self.unique_id += 1;
    }
    decl_vars.extend(decl);
    let (diverges, exits, break_carries) = self.branch_divergence(BranchBody::LetElse(els), &else_ev, &decl_vars);
    let collapsed = branch_is_collapsed(split, merge);
    let else_slice = if collapsed { (split, merge) } else { (split + 1, merge) };
    let match_slice = if collapsed { (merge, merge) } else { (merge, merge.saturating_sub(1)) };
    let else_map = create_line_map(&else_ev);
    let matched = ExtBranchData {
      source_span: self.arm_span(pat.span),
      e_data: Vec::new(), line_map: BTreeMap::new(), decl_vars: HashSet::new(),
      diverges: false, exits: Vec::new(), break_carries: Vec::new(),
    };
    let branches = vec![
      ExtBranchData { source_span: self.arm_span(els.span), e_data: else_ev, line_map: else_map, decl_vars, diverges, exits, break_carries },
      matched,
    ];
    // The matched path is named by the pattern as written.
    let pat_text = self.tcx.sess.source_map().span_to_snippet(pat.span).ok()
      .filter(|t| !t.contains('\n') && !pat.span.from_expansion())
      .unwrap_or_else(|| get_name_of_pat(pat, &self.tcx));
    let labels = vec!["Else".to_owned(), pat_text];
    self.note_exit_scope(init);
    let loop_assigned = self.loop_assigned_now(&liveness);
    self.let_else_branches.insert(*self.unique_id);
    self.branch_conditions.insert(*self.unique_id, pat.span);
    self.add_external_event(split, ExternalEvent::Branch {
      join_span: None,
      live_vars: liveness,
      branches,
      on_exhaustion: Default::default(),
      skipped_holds: Default::default(),
      branch_type: BranchType::If(labels, vec![else_slice, match_slice]),
      split_point: split,
      merge_point: merge,
      maybe_init: HashSet::new(),
      loop_assigned,
      maybe_held: HashSet::new(),
      id: *self.unique_id,
    });
    *self.unique_id += 1;
    Some(merge)
  }

  /// `let Some((b, _)) = opt_pair() else { return; };` drawn as a branch
  /// (#1481): the pattern's bindings are drawn where the branches join,
  /// on the path that skipped the `else` block, taking their part out of
  /// the call's temporary (`match_rhs_call`). So that temporary's column
  /// forks with the block: on the `else` path it is dropped as that path
  /// starts (Rust drops it before the block runs), on the matched one
  /// where the bindings take their part, which the rows of the `else`
  /// block don't come between. Its drop moves from the `let` row to that
  /// join row, and a copy of it ends the `else` path's leg. Returns the
  /// temporary, which is live in the branch.
  fn fork_let_else_temporary(
    &mut self,
    init: &'tcx Expr<'tcx>,
    split: usize,
    merge: usize,
    else_ev: &mut Vec<(usize, ExternalEvent)>,
  ) -> Option<ResourceAccessPoint> {
    let tmp = self.temp_cols.get(&init.hir_id).cloned()
      .filter(|t| matches!(t.temp_part(), Some(TempPart::LetElse(None))))?;
    let k = self.preprocessed_events.iter()
      .position(|(_, e)| matches!(e, ExternalEvent::TemporaryDrop { ro, .. } if *ro == tmp))?;
    let (from, mut drop) = self.preprocessed_events[k].clone();
    self.preprocessed_events[k].0 = merge;
    if let Some(evs) = self.event_line_map.get_mut(&from) {
      if let Some(j) = evs.iter().position(|e| e.get_id() == drop.get_id()) {
        let e = evs.remove(j);
        self.event_line_map.entry(merge).or_default().push(e);
      }
    }
    if let ExternalEvent::TemporaryDrop { id, .. } = &mut drop {
      *id = *self.unique_id;
      *self.unique_id += 1;
    }
    // On the `else` block's first row (the `let` row, for a block on it).
    let first = if branch_is_collapsed(split, merge) { split } else { split + 1 };
    else_ev.insert(0, (first, drop));
    Some(tmp)
  }

  /// After the conditional `init` has been visited: pairs the branch
  /// values put off (`deferred_arm_binds`, #994) that its branches'
  /// visits didn't, as a last resort, after it.
  fn bind_left_arms(&mut self, init: &'tcx Expr<'tcx>) {
    #[expect(clippy::disallowed_methods, reason = "sorted by span on the next line")]
    let mut left: Vec<&'tcx Expr<'tcx>> = self.deferred_arm_binds.values()
      .map(|(body, _)| *body).filter(|b| init.span.contains(b.span)).collect();
    left.sort_by_key(|b| b.span.lo());
    // (The conditional's arms were merged already: an enclosing one's
    // mustn't merge these, #1453.)
    let binds = self.arm_value_binds.clone();
    for body in left {
      warn!("bind_left_arms: branch value at {:?} wasn't paired when its branch was visited", body.span);
      self.bind_deferred_arm(body);
    }
    self.arm_value_binds = binds;
  }

  /// Whether `pat` takes a value of `init`'s type apart into
  /// variables that each own (or copy) their part: tuples, tuple
  /// structs, structs and arrays of by-value bindings and `_`, with no
  /// `ref`, `@` or or-pattern, and `init` not a reference (whose parts
  /// would be bound as references). `let (a, b) = if …` with such a
  /// pattern is bound branch by branch (#1007). A part that is itself a
  /// reference (`let One { a } = if c { One { a: &s1 } } else { .. };`)
  /// is copied (or moved) out like any other, and borrows what the
  /// branch's value gives it (#1405).
  fn destructures_by_value(&self, pat: &'tcx Pat<'tcx>, init: &'tcx Expr<'tcx>) -> bool {
    fn simple(p: &Pat<'_>) -> bool {
      match p.kind {
        PatKind::Wild => true,
        PatKind::Binding(ann, _, _, None) => matches!(ann.0, rustc_hir::ByRef::No),
        PatKind::Tuple(ps, _) | PatKind::TupleStruct(_, ps, _) => ps.iter().all(simple),
        PatKind::Struct(_, fs, _) => fs.iter().all(|f| simple(f.pat)),
        // `let [a, b] = if …` (no `..` binding the rest).
        PatKind::Slice(before, mid, after) => before.iter().chain(after.iter()).all(simple)
          && mid.is_none_or(|m| matches!(m.kind, PatKind::Wild)),
        _ => false,
      }
    }
    if !matches!(pat.kind, PatKind::Tuple(..) | PatKind::TupleStruct(..) | PatKind::Struct(..) | PatKind::Slice(..)) || !simple(pat) {
      return false;
    }
    if init.span.from_expansion() || matches!(init.kind, ExprKind::Match(_, _, src) if src != rustc_hir::MatchSource::Normal) {
      return false;
    }
    let typeck = self.tcx.typeck(init.hir_id.owner);
    if typeck.expr_ty(init).is_ref() {
      return false;
    }
    let mut ok = true;
    pat.walk_always(|p| {
      if let PatKind::Binding(_, hirid, _, _) = p.kind {
        if self.own_ref_binding(hirid).is_some() {
          ok = false;
        }
      }
    });
    ok
  }

  /// Declares each variable of `pat` (`a`, `b` in `let (a, b) = if
  /// …`), with no value yet: each branch gives them theirs
  /// (`bind_pat_arms`, #1007).
  fn declare_pat_bindings(&mut self, pat: &'tcx Pat<'tcx>, init: &'tcx Expr<'tcx>) {
    let mut bs = Vec::new();
    binding_pats(pat, &mut bs);
    for b in bs {
      let PatKind::Binding(annotation, hirid, _, _) = b.kind else { continue };
      let lhs_ty = self.tcx.typeck(hirid.owner).node_type(hirid);
      self.define_lhs(binding_name(&self.tcx, hirid), bool_of_mut(annotation.1), init, lhs_ty);
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
    let PatKind::Binding(annotation, hirid, _, None) = pat.kind else { return false };
    if annotation.0 != rustc_hir::ByRef::No || init.span.from_expansion() {
      return false;
    }
    let ExprKind::AddrOf(rustc_hir::BorrowKind::Ref, borrow_mut, operand) = init.kind else { return false };
    // `&x`, `&v[0]`, `&p.f`, `&*r` borrow a place, not a temporary.
    // `&make_user().name` / `&String::from("a")[1..]` borrow part of
    // the temporary `make_user()` / `String::from("a")`, which is
    // extended the same way (#1165).
    let place = operand;
    let Some(operand) = extended_temp_operand(operand) else { return false };
    let typeck = self.tcx.typeck(hirid.owner);
    let lhs_ty = typeck.node_type(hirid);
    let tmp_ty = typeck.expr_ty(operand);
    if !lhs_ty.is_ref() || !self.needs_drop(tmp_ty, hirid.owner) {
      return false;
    }
    self.visit_expr(init);

    // `&P { name: &make() }`: the temporaries the operand borrows are
    // extended too (#856).
    let mut nested = Vec::new();
    self.walk_extending(operand, &mut nested);
    let line_num = expr_to_line(init, &self.tcx);
    let tmp = self.extended_temp_column(operand, borrow_mut, tmp_ty, line_num);
    let tmp_name = tmp.name().to_owned();
    // `&make_user().name`: the borrow is of the temporary's field
    // column, when it has one.
    let tmp = self.temp_field_column(&tmp, place, operand).unwrap_or(tmp);

    let name = binding_name(&self.tcx, hirid);
    self.add_ref(name.clone(), bool_of_mut(lhs_ty.ref_mutability().unwrap()), bool_of_mut(annotation.1),
      line_num, ResourceTy::Value(tmp.clone()), VecDeque::new(), self.current_scope, !self.inside_branch,
      self.ref_pointee_is_copy(lhs_ty, hirid.owner));
    let r_data = self.raps.get_mut(&name).unwrap();
    r_data.extended_temp.push(tmp_name);
    r_data.extended_temp.extend(nested);
    let r = r_data.rap.to_owned();
    let evt = match borrow_mut {
      Mutability::Mut => Evt::MBorrow,
      Mutability::Not => Evt::SBorrow,
    };
    self.add_borrow_ev(line_num, evt, ResourceTy::Value(r), ResourceTy::Value(tmp), false);
    true
  }

  /// The column of the field `place` (`make_user().name`) of the
  /// temporary `base` (`make_user()`) whose column is `tmp`, if it has
  /// one: `(temporary).name`. None through an index.
  fn temp_field_column(&self, tmp: &ResourceAccessPoint, place: &'tcx Expr<'tcx>, base: &'tcx Expr<'tcx>) -> Option<ResourceAccessPoint> {
    let mut fields = Vec::new();
    let mut e = place;
    while e.hir_id != base.hir_id {
      let ExprKind::Field(b, ident) = e.kind else { return None };
      fields.push(ident.as_str().to_owned());
      e = b;
    }
    if fields.is_empty() {
      return None;
    }
    fields.reverse();
    self.raps.get(&format!("{}.{}", tmp.name(), fields.join("."))).map(|rd| rd.rap.to_owned())
  }

  /// The `(temporary N)` column of a temporary that a `let` extends:
  /// it acquires `operand`'s value (of type `tmp_ty`) on `line_num` and
  /// goes out of scope at the end of the enclosing block (#400). A
  /// temporary borrowed by `&mut` is a mutable place.
  fn extended_temp_column(&mut self, operand: &'tcx Expr<'tcx>, borrow_mut: Mutability, tmp_ty: rustc_middle::ty::Ty<'tcx>, line_num: usize) -> ResourceAccessPoint {
    self.temp_count += 1;
    let tmp_name = temporary_label(self.temp_count);
    self.define_lhs(tmp_name.clone(), borrow_mut == Mutability::Mut, operand, tmp_ty);
    let tmp = self.raps.get(&tmp_name).unwrap().rap.to_owned();
    let n_events = self.preprocessed_events.len();
    self.match_rhs(ResourceTy::Value(tmp.clone()), operand, Evt::Move);
    if self.preprocessed_events.len() == n_events {
      // A shape match_rhs has nothing to say about: the temporary
      // still holds the value from here on.
      self.add_ev(line_num, Evt::Bind, ResourceTy::Value(tmp.clone()), ResourceTy::Anonymous, false);
    }
    tmp
  }

  /// `let w = W { r: &String::new() };`, `let t = (&make(), 1);`,
  /// `let s = Some(&make());`: a temporary borrowed inside the `let`'s
  /// initializer, by a `&` that Rust's temporary lifetime extension
  /// reaches (the operand of an extending `&`, and the fields of an
  /// extending struct, tuple or array literal, tuple-struct / variant
  /// constructor or cast), lives until the end of the enclosing block,
  /// like #400's `let r = &String::from("a");`. Give each one that owns
  /// a resource a `(temporary N)` column (#856), recorded by its `&` in
  /// `nested_ext_temps` so whatever holds the reference borrows from it,
  /// and returns their names: they go out of scope with `pat`'s
  /// bindings. Called after `init` is visited and before `pat` is bound.
  fn bind_nested_extended_temps(&mut self, pat: &'tcx Pat<'tcx>, init: &'tcx Expr<'tcx>) -> Vec<String> {
    // `let (a, b) = &(make(), 1);` binds through the reference; not drawn.
    if matches!(init.kind, ExprKind::AddrOf(..)) && !matches!(pat.kind, PatKind::Binding(..)) {
      return Vec::new();
    }
    let mut out = Vec::new();
    self.walk_extending(init, &mut out);
    out
  }

  /// `let r = if c { &String::from("x") } else { &s };` (also `else
  /// if`, a `match` arm, `Some(&..)` / `[&..]` / a struct field in a
  /// branch, nested conditionals, #1397): Rust extends the temporary a
  /// branch borrows to the end of the `let`'s block (since 1.79), but
  /// only the path through that branch makes it, and a drop flag drops
  /// it there. (A plain assignment `r = if ..`, a call's argument or a
  /// `loop { break &.. }` doesn't extend it: E0716.) Each such
  /// temporary is declared here, before `init` is visited, as a `let
  /// tmp;` with no value yet would be (`uninit_locals`, keyed by the
  /// temporary's expression, named in `branch_temp_names`): the branch
  /// that makes it initializes it (`make_branch_temp`), and the
  /// conditional's join leaves it maybe-initialized, as for a `let x;`
  /// assigned in one branch (#244), with its drop-flag drop at the end
  /// of the block.
  fn declare_branch_temps(&mut self, init: &'tcx Expr<'tcx>) -> Vec<String> {
    let mut names = Vec::new();
    let mut borrows = Vec::new();
    self.branch_extended_temps(init, false, &mut borrows);
    let (scope, inside_branch, frames_at) = (self.current_scope, self.inside_branch, self.exit_frames.len());
    for b in borrows {
      let ExprKind::AddrOf(_, borrow_mut, operand) = b.kind else { continue };
      let Some(tmp) = extended_temp_operand(operand) else { continue };
      if self.branch_temp_names.contains_key(&tmp.hir_id) {
        continue;
      }
      self.temp_count += 1;
      let name = temporary_label(self.temp_count);
      self.branch_temp_names.insert(tmp.hir_id, name.clone());
      names.push(name);
      let annotation = BindingMode(rustc_hir::ByRef::No, borrow_mut);
      self.uninit_locals.insert(tmp.hir_id, DeferredLet { annotation, scope, inside_branch, is_skipped: false, frames_at });
      self.in_scope.push(deferred_slot(tmp.hir_id));
    }
    names
  }

  /// The `let x;` local (or branch temporary) `id` was just given its
  /// column, which registered `in_scope[names_at..]`: its slot
  /// (`deferred_slot`) now stands for those names (#1436).
  fn fill_deferred_slot(&mut self, id: HirId, names_at: usize) {
    let names: Vec<String> = self.in_scope[names_at.min(self.in_scope.len())..].to_vec();
    if !names.is_empty() {
      self.deferred_slots.insert(deferred_slot(id), names);
    }
  }

  /// `&temp` (the `AddrOf` `addr`, its operand visited) in the branch
  /// that makes a temporary `declare_branch_temps` declared: the
  /// temporary gets its value here, in this branch, as a `let x;` local
  /// does at its first assignment, and the `&` borrows its column
  /// (`nested_ext_temps`). The `let`'s binding is paired with the
  /// branch's value after the branch is visited (`arm_value_uses_local_in`).
  fn make_branch_temp(&mut self, addr: &'tcx Expr<'tcx>) {
    let ExprKind::AddrOf(_, _, place) = addr.kind else { return };
    let Some(tmp) = extended_temp_operand(place) else { return };
    let Some(name) = self.branch_temp_names.get(&tmp.hir_id).cloned() else { return };
    let Some(decl) = self.uninit_locals.remove(&tmp.hir_id) else { return };
    let tmp_ty = self.tcx.typeck(tmp.hir_id.owner).expr_ty(tmp);
    let saved = (self.current_scope, self.inside_branch);
    (self.current_scope, self.inside_branch) = (decl.scope, decl.inside_branch);
    self.deferred_decl_at.insert(name.clone(), decl.frames_at);
    let names_at = self.in_scope.len();
    self.define_lhs(name.clone(), bool_of_mut(decl.annotation.1), tmp, tmp_ty);
    self.fill_deferred_slot(tmp.hir_id, names_at);
    (self.current_scope, self.inside_branch) = saved;
    let rap = self.raps.get(&name).unwrap().rap.to_owned();
    let field_prefix = format!("{}.", name);
    #[expect(clippy::disallowed_methods, reason = "inserted into a set; order is lost")]
    let cols: Vec<u64> = self.raps.iter()
      .filter(|(n, _)| **n == name || n.starts_with(&field_prefix))
      .map(|(_, rd)| *rd.rap.hash()).collect();
    self.branch_temp_cols.extend(cols);
    let n_events = self.preprocessed_events.len();
    self.match_rhs(ResourceTy::Value(rap.clone()), tmp, Evt::Move);
    if self.preprocessed_events.len() == n_events {
      self.add_ev(expr_to_line(tmp, &self.tcx), Evt::Bind, ResourceTy::Value(rap.clone()), ResourceTy::Anonymous, false);
    }
    // `&make_user().name`: the borrow is of the temporary's field column.
    let col = self.temp_field_column(&rap, place, tmp).unwrap_or(rap);
    self.nested_ext_temps.insert(addr.hir_id, col);
  }

  /// `bind_nested_extended_temps`'s walk over the extending expression `e`.
  fn walk_extending(&mut self, e: &'tcx Expr<'tcx>, out: &mut Vec<String>) {
    if e.span.from_expansion() {
      return;
    }
    let typeck = self.tcx.typeck(e.hir_id.owner);
    match e.kind {
      ExprKind::AddrOf(rustc_hir::BorrowKind::Ref, borrow_mut, operand) => {
        // `&x`, `&v[0]`, `&p.f`, `&*r` borrow a place, not a temporary;
        // `&make().name` borrows part of the temporary `make()` (#1165).
        let place = operand;
        let operand = match extended_temp_operand(operand) {
          Some(o) => o,
          None if matches!(operand.kind, ExprKind::AddrOf(..)) => operand,
          None => return,
        };
        // Temporaries borrowed inside the operand are extended too, and
        // are made first: the operand's value borrows from them.
        self.walk_extending(operand, out);
        let tmp_ty = typeck.expr_ty(operand);
        if matches!(operand.kind, ExprKind::AddrOf(..)) || operand.span.from_expansion()
          || !self.needs_drop(tmp_ty, e.hir_id.owner) {
          return;
        }
        let tmp = self.extended_temp_column(operand, borrow_mut, tmp_ty, expr_to_line(operand, &self.tcx));
        out.push(tmp.name().to_owned());
        // `H { r: &make_user().name }`: what holds the reference borrows
        // the temporary's field column, as `let name = &make_user().name;`
        // does (#1346).
        let tmp = self.temp_field_column(&tmp, place, operand).unwrap_or(tmp);
        self.nested_ext_temps.insert(e.hir_id, tmp);
      }
      ExprKind::Struct(_, fields, _) => {
        for f in fields {
          self.walk_extending(f.expr, out);
        }
      }
      ExprKind::Tup(elems) | ExprKind::Array(elems) => {
        for el in elems {
          self.walk_extending(el, out);
        }
      }
      ExprKind::Call(callee, args) if is_ctor_path(callee, typeck) => {
        for a in args {
          self.walk_extending(a, out);
        }
      }
      ExprKind::Cast(inner, _) => self.walk_extending(inner, out),
      // `let r = { &String::from("x") };`: a block's last expression
      // is extending too (#1383).
      ExprKind::Block(b, None) if b.rules == rustc_hir::BlockCheckMode::DefaultBlock => {
        if let Some(tail) = b.expr {
          self.walk_extending(tail, out);
        }
      }
      _ => {}
    }
  }

  /// Structural pat-against-init decomposition. When the pattern's
  /// shape matches the init expression's shape — tuple-on-tuple,
  /// tuple-struct-on-call, struct-on-struct, ref-on-addrof, or
  /// slice-on-array — recurse element-wise so each binding gets
  /// paired with the corresponding sub-expression as its source.
  /// Otherwise fall back to [`bind_walk`], which registers every
  /// binding in the pattern against the whole init — semantically
  /// approximate, but never silently empty.
  pub(crate) fn bind_pat(
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
      //
      // `let W(a, b) = W(e1, e2);` (also `let Some(a) = Some(e) else ..`)
      // takes the constructor apart the same way: it's no actor, and
      // `a` takes e1's value (#950). A call that isn't a constructor
      // (`let W(a) = wrap(s);`) is bound from as a whole.
      (PatKind::Tuple(..) | PatKind::TupleStruct(..), ExprKind::Tup(_) | ExprKind::Call(..))
        if self.let_literal_pairs(pat, init).is_some() =>
      {
        let (pairs, ignored) = self.let_literal_pairs(pat, init).unwrap();
        self.premove_into_literal(&pairs, ignored, is_skipped);
        for (sp, se) in pairs {
          self.refuse_or_in_literal(sp, se, is_skipped);
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
            Some(ef) => {
              self.refuse_or_in_literal(pf.pat, ef.expr, is_skipped);
              self.bind_pat(pf.pat, ef.expr, is_skipped);
            }
            None => self.bind_walk(pf.pat, init, is_skipped),
          }
        }
      }
      // `let (a, b) = t;` / `let P { a, b } = p;` with `t` / `p` a
      // tuple or struct variable (or a field of one): its parts `t.0`,
      // `p.a`, … have columns, so pair each sub-pattern with its part
      // (#189, #326). Also `let (s, n) = *b;` on a Box, whose payload's
      // parts are drawn as `b.0`, `b.1`: s moves out of `b.0` only, not
      // all of `*b` (#890). The other shapes fall through to bind_walk, which
      // binds everything from the whole of `t`.
      (PatKind::Tuple(..) | PatKind::TupleStruct(..) | PatKind::Struct(..), ExprKind::Path(_) | ExprKind::Field(..) | ExprKind::Unary(UnOp::Deref, _))
        if self.member_place_name(init).is_some() =>
      {
        let place = self.member_place_name(init).unwrap();
        let typeck = self.tcx.typeck(init.hir_id.owner);
        let ty = typeck.node_type(init.hir_id);
        // An or-pattern nested in the pattern (`let (a, (x, _) | (_, x))
        // = t;`) can bind a name from different places too (#612).
        let conflicts = if is_skipped {
          Vec::new()
        } else {
          let parent = ResourceTy::Value(self.raps.get(&place).unwrap().rap.to_owned());
          let line = span_to_line(&pat.span, &self.tcx);
          self.or_arm_stand_in(pat, std::slice::from_ref(&parent), &[ty], None, line, typeck, &[]).1
        };
        self.bind_pat_from_member(pat, &place, ty, init, is_skipped, &conflicts);
      }
      // `let ((x, _) | (_, x)) = t;`: `x` comes from `t.0` or `t.1`,
      // whichever alternative matches, like in such a match arm
      // (#533, #597). Bind the first alternative from `t`'s parts, with
      // `x` a new value and each place moved into it only if its
      // alternative matched. An or-pattern whose alternatives bind
      // every name from the same place falls through to bind_walk.
      (PatKind::Or(_), ExprKind::Path(_) | ExprKind::Field(..) | ExprKind::Unary(UnOp::Deref, _))
        if self.member_place_name(init).is_some() && !is_skipped =>
      {
        let place = self.member_place_name(init).unwrap();
        let typeck = self.tcx.typeck(init.hir_id.owner);
        let ty = typeck.node_type(init.hir_id);
        let parent = ResourceTy::Value(self.raps.get(&place).unwrap().rap.to_owned());
        let line = span_to_line(&pat.span, &self.tcx);
        let (first, conflicts) = self.or_arm_stand_in(pat, std::slice::from_ref(&parent), &[ty], None, line, typeck, &[]);
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
      // `let &b = r;` with `r: &i32`: the `&` pattern matches r's
      // reference, and `b` gets a copy of the value it points to, as
      // in `let b = *r;` (#880).
      (PatKind::Ref(inner_pat, _, _), _) if self.deref_target(init).is_some() => {
        let (r, pointee_ty) = self.deref_target(init).unwrap();
        let first_id = *self.unique_id;
        self.bind_pat(inner_pat, init, is_skipped);
        self.mark_copy_deref(inner_pat, &r, pointee_ty, first_id);
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

  /// True iff no binding in `pat` is an explicit `ref` / `ref mut` one
  /// (`own_ref_binding`).
  fn binds_by_value_only(&self, pat: &'tcx Pat<'tcx>) -> bool {
    let mut by_value = true;
    pat.each_binding(|_, hid, _, _| by_value &= self.own_ref_binding(hid).is_none());
    by_value
  }

  /// `let (a, b) = (e1, e2);`, `let W(a, b) = W(e1, e2);`: each
  /// sub-pattern with the element of the literal it takes apart, by
  /// position. With a `..` (`let (a, ..) = (e1, e2, e3);`) the patterns
  /// after it pair with the last elements; the elements it covers are
  /// returned on their own. A constructor call pairs with a pattern
  /// naming its own variant or struct (#950). `None` when the pattern
  /// doesn't take the init apart this way.
  fn let_literal_pairs(&self, pat: &'tcx Pat<'tcx>, init: &'tcx Expr<'tcx>)
    -> Option<(Vec<(&'tcx Pat<'tcx>, &'tcx Expr<'tcx>)>, Vec<&'tcx Expr<'tcx>>)> {
    let (sub_pats, ddpos, sub_exprs) = match (pat.kind, init.kind) {
      (PatKind::Tuple(sub_pats, ddpos), ExprKind::Tup(sub_exprs)) => (sub_pats, ddpos, sub_exprs),
      (PatKind::TupleStruct(_, sub_pats, ddpos), ExprKind::Call(callee, sub_exprs))
        if !init.span.from_expansion()
          && is_ctor_path(callee, self.tcx.typeck(init.hir_id.owner))
          && !self.ctor_pat_mismatch(pat, init) => (sub_pats, ddpos, sub_exprs),
      _ => return None,
    };
    if sub_pats.len() > sub_exprs.len() || (ddpos.as_opt_usize().is_none() && sub_pats.len() != sub_exprs.len()) {
      return None;
    }
    let d = ddpos.as_opt_usize().unwrap_or(sub_pats.len());
    let tail_start = sub_exprs.len() - (sub_pats.len() - d);
    let pairs = sub_pats[..d].iter().zip(sub_exprs.iter())
      .chain(sub_pats[d..].iter().zip(sub_exprs[tail_start..].iter()))
      .collect();
    Some((pairs, sub_exprs[d..tail_start].iter().collect()))
  }

  /// The constructor calls in `let pat = init;` that `bind_pat` takes
  /// apart like tuple literals (`W(g)` in `let W(y) = W(g);` or `let
  /// (W(y), n) = (W(g), 1);`, #950). A constructor is never an actor
  /// (#873): visiting them draws no move into the constructor, and
  /// `bind_pat` draws each element going where the pattern puts it.
  fn note_let_ctor_literals(&mut self, pat: &'tcx Pat<'tcx>, init: &'tcx Expr<'tcx>) {
    let Some((pairs, _)) = self.let_literal_pairs(pat, init) else { return };
    if matches!(init.kind, ExprKind::Call(..)) {
      self.let_ctor_literals.insert(init.hir_id);
    }
    for (sp, se) in pairs {
      self.note_let_ctor_literals(sp, se);
    }
  }

  /// `let (p, (x, _) | (_, x)) = (s, u);`: the literal moves `u` into
  /// a temporary, which drops what `x` doesn't take at the end of the
  /// `let`. Pairing `(x, _) | (_, x)` with `u` would draw `u.0` and
  /// `u.1` as maybe-moved, each dropped by a drop flag at `u`'s scope
  /// end, so it's refused (#612).
  fn refuse_or_in_literal(&self, sub_pat: &'tcx Pat<'tcx>, elem: &'tcx Expr<'tcx>, is_skipped: bool) {
    if is_skipped || !pat_has_or(sub_pat) {
      return;
    }
    let Some(place) = self.member_place_name(elem) else { return };
    let typeck = self.tcx.typeck(elem.hir_id.owner);
    let parent = ResourceTy::Value(self.raps.get(&place).unwrap().rap.to_owned());
    let line = span_to_line(&sub_pat.span, &self.tcx);
    let (_, conflicts) = self.or_arm_stand_in(sub_pat, std::slice::from_ref(&parent), &[typeck.node_type(elem.hir_id)], None, line, typeck, &[]);
    if let Some(c) = conflicts.first() {
      or_refuse_temporary_parts(&c.name, &place, line);
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
    // Only a chain of fields on a variable (`t`, `p.pair`, `t.0`), or
    // on the payload of a Box variable (`*b`, `(*b).pair`), whose parts
    // are drawn as `b.0`, `b.pair` (#890): expr_to_rap_name also looks
    // through `*` of a reference, `&` and indexing.
    let mut cur = init;
    while let ExprKind::Field(inner, _) = cur.kind {
      cur = inner;
    }
    if let ExprKind::Unary(UnOp::Deref, base) = cur.kind {
      if self.tcx.typeck(base.hir_id.owner).expr_ty(base).is_box() {
        cur = base;
      }
    }
    if !matches!(cur.kind, ExprKind::Path(QPath::Resolved(..))) {
      return None;
    }
    let name = expr_to_rap_name(init, &self.tcx)?;
    let has_members = self.member_fields(ty).iter()
      .any(|(f, _)| self.raps.contains_key(&format!("{}.{}", name, f)));
    has_members.then_some(name)
  }

  /// `match *b { (s, n) => … }` / `if let (s, n) = *b`, with `b` a Box
  /// whose payload's parts have columns (`b.0`, `b.1`): `b`'s own RAP,
  /// to match `pat` against as for a local tuple, so that `s` moves out
  /// of `b.0` only and `n` copies `b.1` (#892), as `let (s, n) = *b;`
  /// does (#890). `None` for any other scrutinee, and for a pattern
  /// that destructures none of those parts (`x => …` takes the whole
  /// payload, `*b`).
  pub(crate) fn box_payload_parent(&self, scrutinee: &'tcx Expr<'tcx>, pat: &'tcx Pat<'tcx>) -> Option<ResourceTy> {
    let ExprKind::Unary(UnOp::Deref, _) = scrutinee.kind else { return None };
    let place = self.member_place_name(scrutinee)?;
    let parent = ResourceTy::Value(self.raps.get(&place)?.rap.to_owned());
    let ty = self.tcx.typeck(scrutinee.hir_id.owner).node_type(scrutinee.hir_id);
    let alts = match pat.kind {
      PatKind::Or(alts) => alts,
      _ => std::slice::from_ref(pat),
    };
    alts.iter().any(|p| self.pat_parts(p, &parent, ty).is_some()).then_some(parent)
  }

  /// `*b = …` with `b` a Box whose payload's parts have columns (`b.0`,
  /// `b.1`), and whose payload needs drop: `b`'s RAP. The new payload's
  /// parts go into those columns, as `t = (..)` does for a local tuple,
  /// so a part moved out earlier holds a value again and is dropped at
  /// `b`'s scope end (#893).
  fn box_payload_write(&self, lhs_expr: &'tcx Expr<'tcx>) -> Option<ResourceAccessPoint> {
    let ExprKind::Unary(UnOp::Deref, _) = lhs_expr.kind else { return None };
    let ty = self.tcx.typeck(lhs_expr.hir_id.owner).node_type(lhs_expr.hir_id);
    if !self.needs_drop(ty, lhs_expr.hir_id.owner) {
      return None;
    }
    let place = self.member_place_name(lhs_expr)?;
    let rap = self.raps.get(&place)?.rap.to_owned();
    matches!(rap, ResourceAccessPoint::Struct(Struct { is_box: true, is_member: false, .. })).then_some(rap)
  }

  /// Borrows made from event id `first_id` on whose borrower is
  /// `place` (`*r`) go to `var`'s column instead: the variable a
  /// write through `*r` re-points (#904).
  fn retarget_borrows_into(&mut self, place: &ResourceTy, var: &ResourceAccessPoint, first_id: usize) {
    let retarget = |e: &mut ExternalEvent| {
      if let ExternalEvent::StaticBorrow { to, id, .. } | ExternalEvent::MutableBorrow { to, id, .. } = e {
        if *id >= first_id && to == place {
          *to = ResourceTy::Value(var.clone());
        }
      }
    };
    // (The line's arrows are drawn from `event_line_map`'s copy.)
    self.preprocessed_events.iter_mut().for_each(|(_, e)| retarget(e));
    self.event_line_map.values_mut().flatten().for_each(retarget);
  }

  /// `**rb = …` (`*(*rb)`, `***rrb`, …) whose operands are references
  /// reaching a `Box`: `Some(true)` when only the outermost `*` goes
  /// into a Box (`rb: &mut Box<String>`), so the write replaces the
  /// Box's contents, not the Box (#941); `Some(false)` when a Box is
  /// dereferenced further in (`***x` with `x: &mut Box<Box<_>>`);
  /// `None` when no operand is a Box (`**r` with `r: &mut &mut
  /// String`, or a single `*r`).
  fn box_deref_through_ref(&self, lhs_expr: &'tcx Expr<'tcx>) -> Option<bool> {
    let typeck = self.tcx.typeck(lhs_expr.hir_id.owner);
    let ExprKind::Unary(UnOp::Deref, outer) = lhs_expr.kind else { return None };
    let mut boxes = Vec::new();
    let mut operand = outer;
    loop {
      boxes.push(typeck.node_type(operand.hir_id).is_box());
      match operand.kind {
        ExprKind::Unary(UnOp::Deref, inner) => operand = inner,
        _ => break,
      }
    }
    if boxes.len() < 2 || !boxes.contains(&true) {
      return None;
    }
    Some(boxes[0] && !boxes[1..].contains(&true))
  }

  /// `**rb` (two or more `*`) with `rb` a reference: the place as
  /// written, for the reference's dot (#941). `None` for a single `*`.
  fn multi_deref_place(&self, lhs_expr: &'tcx Expr<'tcx>, rap: &ResourceAccessPoint) -> Option<String> {
    let ExprKind::Unary(UnOp::Deref, mut inner) = lhs_expr.kind else { return None };
    let mut depth = 1;
    while let ExprKind::Unary(UnOp::Deref, i) = inner.kind {
      inner = i;
      depth += 1;
    }
    if depth < 2 || !rap.is_ref() {
      return None;
    }
    Some(self.plain_source_text(lhs_expr)
      .filter(|p| p.starts_with('*'))
      .unwrap_or_else(|| format!("{}{}", "*".repeat(depth), rap.name())))
  }

  /// `*b = o` / `*b = make()` with `b` a [`box_payload_write`]: the
  /// value goes into `*b`, the Box's payload, not into `b`, which keeps
  /// its heap allocation (#893). Drawn into `b` so that `o`'s parts go
  /// into `b.0`, `b.1`, …, the arrow (from event `first_id` on) is then
  /// retargeted to `*b`: "Move from o to *b".
  fn retarget_into_payload(&mut self, b: &ResourceAccessPoint, first_id: usize) {
    let retarget = |e: &mut ExternalEvent| {
      if let ExternalEvent::Move { to, from, id, .. } = e {
        if *id >= first_id && !matches!(from, ResourceTy::Anonymous)
          && matches!(to, ResourceTy::Value(t) if t.hash() == b.hash()) {
          *to = ResourceTy::Deref(b.clone());
        }
      }
    };
    // (The line's arrows are drawn from `event_line_map`'s copy.)
    self.preprocessed_events.iter_mut().for_each(|(_, e)| retarget(e));
    self.event_line_map.values_mut().flatten().for_each(retarget);
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
      // An or-pattern nested in the pattern: its first alternative
      // stands for it, as in `or_arm_stand_in` (#612). A name it binds
      // from different places is one of `or_conflicts`.
      PatKind::Or(alts) => {
        self.bind_pat_from_member(&alts[0], member, member_ty, init, is_skipped, or_conflicts);
      }
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
          None if or_conflicts.is_empty() || is_skipped => {
            if is_skipped || !self.bind_inside_member(pat, member, member_ty, init) {
              self.bind_walk(pat, init, is_skipped);
            }
          }
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
              // places, as in such a match arm (#565, #734).
              let lhs_ty = self.tcx.typeck(hirid.owner).node_type(hirid);
              let evt = match annotation.0 {
                rustc_hir::ByRef::Yes(_, rustc_hir::Mutability::Mut) => Evt::MBorrow,
                rustc_hir::ByRef::Yes(..) => Evt::SBorrow,
                rustc_hir::ByRef::No if self.ty_is_copy(lhs_ty, hirid.owner) => Evt::Copy,
                rustc_hir::ByRef::No => Evt::Move,
              };
              self.define_lhs(name.clone(), bool_of_mut(annotation.1), init, lhs_ty);
              let to = self.raps.get(&name).unwrap().rap.to_owned();
              self.bind_or_conflict(c, to, evt, span_to_line(&init.span, &self.tcx));
            }
          }
        }
      }
    }
  }

  /// `Some(s)` on `t.0` in `let (Some(s), n) = t else { .. };` (or on
  /// `b.0` in `let (Some(s), n) = *b else { .. };`): a sub-pattern that
  /// doesn't destructure the part `member` into part columns of its own
  /// still takes what it binds out of that part only, so each binding is
  /// bound from `member`, as `if let (Some(s), n) = t` does
  /// (`get_dec_of_pat_parts`), not from the whole `init` (#899). Only
  /// by-value bindings of values holding no reference (a `ref` binding
  /// borrows, and a reference copied out carries its loan), and none
  /// under a `&` / `box` / deref pattern (what that binds is behind a
  /// pointer, #874): false, and nothing bound, otherwise.
  fn bind_inside_member(
    &mut self,
    pat: &'tcx Pat<'tcx>,
    member: &str,
    member_ty: rustc_middle::ty::Ty<'tcx>,
    init: &'tcx Expr<'tcx>,
  ) -> bool {
    let Some(from) = self.raps.get(member).map(|rd| rd.rap.to_owned()) else { return false };
    let mut bs = Vec::new();
    binding_pats(pat, &mut bs);
    let typeck = self.tcx.typeck(pat.hir_id.owner);
    let by_value = |b: &&Pat<'tcx>| matches!(b.kind, PatKind::Binding(ann, hirid, _, _)
      if matches!(ann.0, rustc_hir::ByRef::No)
        && !typeck.node_type(hirid).walk().any(|g| matches!(g.kind(), rustc_middle::ty::GenericArgKind::Lifetime(_))));
    if bs.is_empty() || !bs.iter().all(by_value) || pat_has_pointer_pat(pat) {
      return false;
    }
    let line_num = span_to_line(&init.span, &self.tcx);
    for b in bs {
      let PatKind::Binding(annotation, hirid, _, _) = b.kind else { continue };
      let name = binding_name(&self.tcx, hirid);
      let lhs_ty = typeck.node_type(hirid);
      let evt = self.bind_evt(lhs_ty, hirid);
      self.define_lhs(name.clone(), bool_of_mut(annotation.1), init, lhs_ty);
      let to = self.raps.get(&name).unwrap().rap.to_owned();
      // `let (Some(k), n) = t else { .. }` with `t.0: Option<i32>`: k
      // gets a copy of a value inside t.0.
      let partial = copies_from_inside(&evt, lhs_ty, member_ty, &ResourceTy::Value(from.clone()));
      self.update_rap(&from, line_num);
      self.add_ev(line_num, evt, ResourceTy::Value(to), ResourceTy::Value(from.clone()), partial);
    }
    true
  }

  /// `let ((x, _) | (_, x)) = t;` (#597): `to` is `x`, bound from one
  /// of `c.places`, whichever alternative matches. Drawn as in such a
  /// match arm (`or_conflict_events`): `x` gets a new value, with no
  /// arrow, and when it moves (rather than is copied) each place gets
  /// an `OrAltMove`, so it's maybe-moved afterwards; a struct or tuple
  /// `x` gets its fields with it, and each place's fields may move into
  /// them. A reference `x` (`ref x`, #734) borrows whichever place
  /// matched: every place stays borrowed until `x`'s last use, or the
  /// end of its scope.
  fn bind_or_conflict(&mut self, c: &OrConflict, to: ResourceAccessPoint, evt: Evt, line: usize) {
    if matches!(evt, Evt::Move) {
      for place in &c.places {
        self.update_rap(place, line);
      }
    }
    let scope_end = self.raps.get(to.name()).map_or(line, |rd| rd.scope.max(line));
    let mut bound = vec![(to.clone(), evt, ResourceTy::Anonymous, None)];
    let mut decls = HashSet::new();
    let start = self.if_let_or_conflict_events(std::slice::from_ref(c), &mut bound, line, &[], "", scope_end, true, &mut decls);
    for ev in start {
      self.add_external_event(line, ev);
    }
    // (A reference is drawn by its `OrAltBorrow`s.)
    for (to, event, from, partial) in bound {
      self.add_ev(line, event, ResourceTy::Value(to), from, partial.unwrap_or(false));
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
    self.bind_walk_in(pat, init, is_skipped, false);
  }

  /// `bind_walk`; `nested` when `pat` is inside a destructuring
  /// pattern (`Some(y)`, `(x, y)`) rather than the whole of it: a
  /// reference bound there is copied out of `init`'s value, so it
  /// borrows what that value borrows (`let Some(y) = o else { .. }`
  /// with `o` holding `Part { part: &s }` gives `y.part` the loan of
  /// `s`; #718, #756).
  fn bind_walk_in(
    &mut self,
    pat: &'tcx Pat<'tcx>,
    init: &'tcx Expr<'tcx>,
    is_skipped: bool,
    nested: bool,
  ) {
    match pat.kind {
      PatKind::Binding(annotation, hirid, _, sub_pat) => {
        let name = binding_name(&self.tcx, hirid);
        let first_id = *self.unique_id;
        self.bind_one(name.clone(), annotation, hirid, init, is_skipped);
        if nested {
          self.mark_copy_inside(&name, hirid, init, first_id);
          self.mark_copy_pointee(&name, hirid, first_id);
          self.mark_part_borrow(&name, hirid, init, first_id);
        }
        if nested && !is_skipped && self.own_ref_binding(hirid).is_none() {
          if let Some(rap) = self.raps.get(&name).map(|rd| rd.rap.to_owned()) {
            let lhs_ty = self.tcx.typeck(hirid.owner).node_type(hirid);
            let evt = self.bind_evt(lhs_ty, hirid);
            let line = span_to_line(&init.span, &self.tcx);
            self.narrow_call_result_lenders(&name, &rap, init);
            // `let Some(f) = p else { return };` with `p = v.first()`:
            // the copy out of p (`match_rhs`) gave f p's lender, with
            // no arrow saying so, and `pattern_ref_borrows` only draws
            // a lender not set yet. Draw it as for `if let Some(f) = p`:
            // f borrows v (#1446).
            let held = self.unset_lender_copied_from_holder(&rap, init);
            let borrows = self.pattern_ref_copy_borrow(&rap, &evt, init);
            if let (Some(old), true) = (held, borrows.is_empty()) {
              self.borrow_map.insert(name.clone(), old);
            }
            for b in borrows {
              self.add_external_event(line, b);
            }
          }
        }
        if let Some(sp) = sub_pat {
          self.bind_walk_in(sp, init, is_skipped, nested);
        }
      }
      PatKind::Tuple(pats, _) | PatKind::TupleStruct(_, pats, _) => {
        for p in pats {
          self.bind_walk_in(p, init, is_skipped, true);
        }
      }
      PatKind::Struct(_, fields, _) => {
        for f in fields {
          self.bind_walk_in(f.pat, init, is_skipped, true);
        }
      }
      PatKind::Or(pats) => {
        // Or-pattern alternatives bind the same names; registering
        // from the first alt is enough — the other alts would
        // duplicate the same RAPs.
        if let Some(first) = pats.first() {
          self.bind_walk_in(first, init, is_skipped, nested);
        }
      }
      PatKind::Ref(inner, _, _)
      | PatKind::Box(inner)
      | PatKind::Deref(inner)
      | PatKind::Guard(inner, _) => {
        self.bind_walk_in(inner, init, is_skipped, true);
      }
      PatKind::Slice(before, mid, after) => {
        for p in before { self.bind_walk_in(p, init, is_skipped, true); }
        if let Some(p) = mid { self.bind_walk_in(p, init, is_skipped, true); }
        for p in after { self.bind_walk_in(p, init, is_skipped, true); }
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

  /// For `unset_lender_copied_from_holder`'s caller (`bind_walk_in`):
  /// `reference`, bound inside a pattern of a `let` whose value is a
  /// variable holding a borrow with no columns for its parts (`p:
  /// Option<&String>`, an iterator), was given that variable's lender
  /// by the copy (or move) out of it. Unset it, so
  /// `pattern_ref_copy_borrow` draws the borrow; returns the old loan,
  /// to put back if it draws none.
  fn unset_lender_copied_from_holder(&mut self, reference: &ResourceAccessPoint, init: &'tcx Expr<'tcx>) -> Option<RefData> {
    if !reference.is_ref() {
      return None;
    }
    let mut e = init;
    while let ExprKind::DropTemps(inner) = e.kind {
      e = inner;
    }
    let ExprKind::Path(..) = e.kind else { return None };
    let src = expr_to_rap_name(e, &self.tcx)?;
    let holder = self.raps.get(&src).is_some_and(|rd| matches!(rd.rap, ResourceAccessPoint::Owner(_))
      && rd.rap.holds_borrow());
    let src_lender = self.borrow_map.get(&src).map(|d| d.lender.clone())?;
    let rd = self.borrow_map.get_mut(reference.name())?;
    if !holder || rd.lender != src_lender || matches!(rd.lender, ResourceTy::Anonymous) {
      return None;
    }
    let old = rd.clone();
    rd.lender = ResourceTy::Anonymous;
    rd.co_lenders.clear();
    rd.lender_muts.clear();
    Some(old)
  }

  /// `match &mut t { (a, b) => … }` (#1215): like `let (a, b) = &mut t;`
  /// (`mark_part_borrow`), the scrutinee is one loan of all of t, which a
  /// and b each reach a part of. Name the part on a's borrow, and give a
  /// the scrutinee's loan, which rustc ends at the last use of a or b:
  /// t was given back at a's last use while b still held it.
  fn arm_part_borrow(&mut self, guard_expr: &'tcx Expr<'tcx>, arm_pat: &'tcx Pat<'tcx>, to_ro: &ResourceAccessPoint, from: &ResourceTy, ext_ev: &mut ExternalEvent) {
    let mut e = guard_expr;
    while let ExprKind::DropTemps(inner) = e.kind {
      e = inner;
    }
    let ExprKind::AddrOf(_, _, place) = e.kind else { return };
    let Some(whole) = expr_to_rap_name(place, &self.tcx) else { return };
    let ResourceTy::Value(f) = from else { return };
    if *f.name() != whole {
      return;
    }
    let (ExternalEvent::StaticBorrow { via: v @ None, .. } | ExternalEvent::MutableBorrow { via: v @ None, .. }) = ext_ev else { return };
    let mut hirid = None;
    arm_pat.each_binding(|_, id, _, _| {
      if hirid.is_none() && binding_name(&self.tcx, id) == *to_ro.name() {
        hirid = Some(id);
      }
    });
    let Some(hirid) = hirid else { return };
    let Some(path) = self.binding_part_path(hirid) else { return };
    let part = format!("{}.{}", whole, path);
    if !self.raps.contains_key(&part) {
      return;
    }
    *v = Some(BorrowVia {
      func: part,
      lenders: vec![whole.clone()],
      also_unnamed: false,
      tie: ArgTie::default(),
      carrier: ViaCarrier::Part,
      passed_on: Vec::new(),
    });
    if let Some(rd) = self.borrow_map.get_mut(to_ro.name()) {
      rd.borrow_span = Some(e.span);
    }
  }

  /// `let (a, b) = &mut t;`: `bind_one` drew a's borrow from all of t,
  /// which is right (the pattern makes one loan of t), but a points to
  /// t.0 only. Say so on the borrow (`ViaCarrier::Part`), or t's dot
  /// reads as two mutable borrows of the same value (#1130).
  fn mark_part_borrow(&mut self, name: &str, hirid: HirId, init: &'tcx Expr<'tcx>, first_id: usize) {
    let mut e = init;
    while let ExprKind::DropTemps(inner) = e.kind {
      e = inner;
    }
    let ExprKind::AddrOf(_, _, place) = e.kind else { return };
    let Some(whole) = expr_to_rap_name(place, &self.tcx) else { return };
    let Some(path) = self.binding_part_path(hirid) else { return };
    let part = format!("{}.{}", whole, path);
    if !self.raps.contains_key(&part) {
      return;
    }
    let via = BorrowVia {
      func: part,
      lenders: vec![whole.clone()],
      also_unnamed: false,
      tie: ArgTie::default(),
      carrier: ViaCarrier::Part,
      passed_on: Vec::new(),
    };
    let set = |ev: &mut ExternalEvent| match ev {
      ExternalEvent::StaticBorrow { id, from: ResourceTy::Value(f), to: ResourceTy::Value(t), via: v @ None, .. }
      | ExternalEvent::MutableBorrow { id, from: ResourceTy::Value(f), to: ResourceTy::Value(t), via: v @ None, .. }
        if *id >= first_id && t.name() == name && *f.name() == whole => *v = Some(via.clone()),
      _ => {}
    };
    for (_, ev) in self.preprocessed_events.iter_mut() {
      set(ev);
    }
    for evs in self.event_line_map.values_mut() {
      for ev in evs.iter_mut() {
        set(ev);
      }
    }
  }

  /// The field path (`0`, `name`, `inner.0`) a pattern binding takes out
  /// of the value its pattern matches (`a` in `let (a, b) = …` gives
  /// `0`). None through a `&` / box / deref pattern, a slice, an
  /// or-pattern, or an enum variant: those don't name a field column.
  fn binding_part_path(&self, hirid: HirId) -> Option<String> {
    let typeck = self.tcx.typeck(hirid.owner);
    let mut parts: Vec<String> = Vec::new();
    let mut cur = hirid;
    loop {
      let parent = self.tcx.parent_hir_node(cur);
      match parent {
        rustc_hir::Node::Pat(p) => {
          match p.kind {
            PatKind::Tuple(ps, dotdot) => {
              let i = ps.iter().position(|x| x.hir_id == cur)?;
              let arity = match typeck.pat_ty(p).peel_refs().kind() {
                rustc_middle::ty::TyKind::Tuple(tys) => tys.len(),
                _ => return None,
              };
              let idx = match dotdot.as_opt_usize() {
                Some(d) if i >= d => i + arity - ps.len(),
                _ => i,
              };
              parts.push(idx.to_string());
            }
            PatKind::TupleStruct(..) | PatKind::Struct(..) => {
              // Only a struct's own fields have columns (not an enum's).
              if !typeck.pat_ty(p).peel_refs().ty_adt_def().is_some_and(|a| a.is_struct()) {
                return None;
              }
              match p.kind {
                PatKind::TupleStruct(_, ps, dotdot) => {
                  let i = ps.iter().position(|x| x.hir_id == cur)?;
                  let n = typeck.pat_ty(p).peel_refs().ty_adt_def()?.non_enum_variant().fields.len();
                  let idx = match dotdot.as_opt_usize() {
                    Some(d) if i >= d => i + n - ps.len(),
                    _ => i,
                  };
                  parts.push(idx.to_string());
                }
                _ => {}
              }
            }
            PatKind::Binding(..) => {}
            _ => return None,
          }
          cur = p.hir_id;
        }
        rustc_hir::Node::PatField(f) => {
          parts.push(f.ident.as_str().to_owned());
          // The struct pattern is the field's parent.
          cur = f.hir_id;
        }
        _ => break,
      }
    }
    if parts.is_empty() {
      return None;
    }
    parts.reverse();
    Some(parts.join("."))
  }

  /// `let Some(w) = oi else { .. }` with a Copy payload: `bind_one`
  /// drew w's copy from all of `oi`, but w gets a copy of a value
  /// inside it, the same as `if let Some(w) = oi` (#866). Not through a
  /// `&` pattern: what that copies is behind the reference.
  fn mark_copy_inside(&mut self, name: &str, hirid: HirId, init: &'tcx Expr<'tcx>, first_id: usize) {
    let typeck = self.tcx.typeck(hirid.owner);
    if !copies_from_inside(&Evt::Copy, typeck.node_type(hirid), typeck.expr_ty(init), &ResourceTy::Anonymous) {
      return;
    }
    let mut cur = hirid;
    while let rustc_hir::Node::Pat(p) = self.tcx.parent_hir_node(cur) {
      if matches!(p.kind, PatKind::Ref(..) | PatKind::Deref(_) | PatKind::Box(_)) {
        return;
      }
      cur = p.hir_id;
    }
    // (The line's arrows are drawn from `event_line_map`'s copy.)
    let line_events = self.event_line_map.values_mut().flatten();
    let events = self.preprocessed_events.iter_mut().chain(self.late_old_ends.iter_mut()).map(|(_, e)| e);
    for e in events.chain(line_events) {
      if let ExternalEvent::Copy { to: ResourceTy::Value(t), from: ResourceTy::Value(f), id, is_partial, .. } = e {
        if *id >= first_id && t.name() == name && !f.is_fn() {
          *is_partial = true;
        }
      }
    }
  }

  /// `let Some(&y) = orr else { .. }`: `bind_one` drew y's copy from
  /// orr, but y gets a copy of the value the reference inside orr
  /// points to, as for `if let Some(&y) = orr` (#874).
  fn mark_copy_pointee(&mut self, name: &str, hirid: HirId, first_id: usize) {
    let Some(part) = crate::expr_visitor::ref_pattern_above(&self.tcx, hirid) else { return };
    // (The line's arrows are drawn from `event_line_map`'s copy.)
    let line_events = self.event_line_map.values_mut().flatten();
    let events = self.preprocessed_events.iter_mut().chain(self.late_old_ends.iter_mut()).map(|(_, e)| e);
    for e in events.chain(line_events) {
      if let ExternalEvent::Copy { to: ResourceTy::Value(t), id, is_partial, pointee, .. } = e {
        if *id >= first_id && t.name() == name {
          *pointee = Some(part);
          *is_partial = false;
        }
      }
    }
  }

  /// The variable `init` names and the type its reference points to,
  /// when `init` is a reference variable (`r` with `r: &i32`).
  fn deref_target(&self, init: &'tcx Expr<'tcx>) -> Option<(ResourceAccessPoint, rustc_middle::ty::Ty<'tcx>)> {
    let ty = self.tcx.typeck(init.hir_id.owner).expr_ty(init);
    let rustc_middle::ty::Ref(_, pointee, _) = ty.kind() else { return None };
    if !matches!(init.kind, ExprKind::Path(_)) {
      return None;
    }
    match get_rap(init, &self.tcx, &self.raps) {
      ResourceTy::Value(r) if !r.is_fn() => Some((r, *pointee)),
      _ => None,
    }
  }

  /// The variable `*r` names and the type it points to, when `init`
  /// is a dereference of a reference or Box variable.
  fn deref_place_target(&self, init: &'tcx Expr<'tcx>) -> Option<(ResourceAccessPoint, rustc_middle::ty::Ty<'tcx>)> {
    let ExprKind::Unary(UnOp::Deref, base) = init.kind else { return None };
    if !matches!(base.kind, ExprKind::Path(_)) {
      return None;
    }
    let ty = self.tcx.typeck(init.hir_id.owner).expr_ty(init);
    match get_rap(base, &self.tcx, &self.raps) {
      ResourceTy::Value(r) if !r.is_fn() => Some((r, ty)),
      _ => None,
    }
  }

  /// `let &b = r;`: `bind_pat` drew b's copy from r, but b gets a copy
  /// of the value r points to, `*r`, as for `let b = *r;` (#880): a
  /// value inside it for a binding nested in the `&` pattern (`let &(a,
  /// b) = r;`). A copy through a `&` sub-pattern inside it (#874)
  /// keeps its wording, from `*r`.
  fn mark_copy_deref(&mut self, inner: &'tcx Pat<'tcx>, r: &ResourceAccessPoint, pointee_ty: rustc_middle::ty::Ty<'tcx>, first_id: usize) {
    let typeck = self.tcx.typeck(inner.hir_id.owner);
    let mut names: HashMap<String, bool> = HashMap::new();
    inner.each_binding(|_, hirid, _, _| {
      if matches!(typeck.pat_binding_modes().get(hirid).map(|m| m.0), Some(rustc_hir::ByRef::No)) {
        names.insert(binding_name(&self.tcx, hirid), typeck.node_type(hirid) != pointee_ty);
      }
    });
    // (The line's arrows are drawn from `event_line_map`'s copy.)
    let line_events = self.event_line_map.values_mut().flatten();
    let events = self.preprocessed_events.iter_mut().chain(self.late_old_ends.iter_mut()).map(|(_, e)| e);
    for e in events.chain(line_events) {
      if let ExternalEvent::Copy { to: ResourceTy::Value(t), from, id, is_partial, pointee, into_ctor: None, .. } = e {
        let Some(inside) = names.get(t.name()) else { continue };
        if *id >= first_id && (*from == ResourceTy::Value(r.clone()) || *from == ResourceTy::Deref(r.clone())) {
          *from = ResourceTy::Deref(r.clone());
          *is_partial = *inside && pointee.is_none();
        }
      }
    }
  }

  /// `let (x, y) = two(&c, &d);` with `two<'a, 'b>(&'a T, &'b T) ->
  /// (&'a T, &'b T)`: the call's result was given every lender the
  /// signature ties it to, but each reference in it has only one of
  /// the lifetimes, so `x` borrows `c` and not `d`. When rustc's
  /// lifetimes put fewer of the lenders in `x` than were given, drop
  /// those borrows so the pattern's own (`pattern_ref_copy_borrow`)
  /// can be drawn from the lenders rustc says (#756).
  fn narrow_call_result_lenders(&mut self, name: &str, rap: &ResourceAccessPoint, init: &'tcx Expr<'tcx>) {
    if !rap.is_ref() || !matches!(init.kind, ExprKind::Call(..) | ExprKind::MethodCall(..)) {
      return;
    }
    let Some(rd) = self.borrow_map.get(name) else { return };
    if rd.co_lenders.is_empty() {
      return;
    }
    let lenders = rd.all_lenders();
    let Some(held) = self.loans_held_by(name, &[]) else { return };
    // (Nor the ones x reaches only through another: `let (i, el) =
    // it.next().unwrap();` with `it = refs.iter_mut().enumerate()`, #1399.)
    let through = self.split_through_lenders(name, &mut lenders.clone(), &mut Vec::new());
    let kept = |l: &ResourceTy| match l {
      ResourceTy::Value(r) if !r.is_ref() => {
        held.contains_key(r.name().split('.').next().unwrap_or_default()) && !through.contains(l)
      }
      _ => true,
    };
    if lenders.iter().all(kept) || !lenders.iter().any(kept) {
      return;
    }
    let to = ResourceTy::Value(rap.clone());
    let ours = |ev: &ExternalEvent| matches!(ev,
      ExternalEvent::StaticBorrow { to: t, via: Some(_), .. } | ExternalEvent::MutableBorrow { to: t, via: Some(_), .. } if *t == to);
    self.preprocessed_events.retain(|(_, ev)| !ours(ev));
    for evs in self.event_line_map.values_mut() {
      evs.retain(|ev| !ours(ev));
    }
    if let Some(rd) = self.borrow_map.get_mut(name) {
      rd.lender = ResourceTy::Anonymous;
      rd.co_lenders = Vec::new();
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
    self.register_pattern_bindings_below(pat, line, 0, Some(String::new()));
  }

  /// The paths (`.0`, `.name`) of the sub-patterns of the tuple (or
  /// tuple struct) pattern `pat` with sub-patterns `ps`, `..` at `ddpos`:
  /// None for each when the arity isn't known.
  fn tuple_pat_paths(&self, pat: &'tcx Pat<'tcx>, ps: &'tcx [Pat<'tcx>], ddpos: rustc_hir::DotDotPos) -> Vec<Option<String>> {
    let typeck = self.tcx.typeck(pat.hir_id.owner);
    let arity = match pat.kind {
      PatKind::Tuple(..) => match typeck.pat_ty(pat).kind() {
        rustc_middle::ty::TyKind::Tuple(tys) => Some(tys.len()),
        _ => None,
      },
      PatKind::TupleStruct(ref qpath, ..) => match (typeck.qpath_res(qpath, pat.hir_id), typeck.pat_ty(pat).ty_adt_def()) {
        (res, Some(adt)) if !matches!(res, Res::Err) => Some(adt.variant_of_res(res).fields.len()),
        _ => None,
      },
      _ => None,
    };
    let dd = ddpos.as_opt_usize();
    (0..ps.len()).map(|i| match dd {
      Some(d) if i >= d => arity.map(|n| format!(".{}", i + n - ps.len())),
      _ => Some(format!(".{}", i)),
    }).collect()
  }

  /// `derefs` counts the references the pattern has gone through above
  /// `pat`, written (`&w`) or implicit (default binding modes): the
  /// first is the loop item's own `&` into the head (`pattern_lender`).
  /// `path`: the part of the loop's item `pat` matches (`.0`), when known.
  fn register_pattern_bindings_below(&mut self, pat: &'tcx Pat<'tcx>, line: usize, derefs: usize, path: Option<String>) {
    let typeck = self.tcx.typeck(pat.hir_id.owner);
    let derefs = derefs + typeck.pat_adjustments().get(pat.hir_id).map_or(0, |a| a.len());
    match pat.kind {
      PatKind::Binding(_, hirid, ident, sub_pat) => {
        let name = binding_name(&self.tcx, hirid);
        let pat_ty = typeck.pat_ty(pat);
        // A reference binding borrows the head when it is the item's
        // reference itself (`(i, x)` of `v.iter().enumerate()`), or
        // borrows a place reached through only that one reference
        // (`(a, b)` of `&pairs`, `&ref n` of `&names`). A by-value
        // binding below a deref copies a reference stored in the
        // element (`for &w in &words`, words: Vec<&str>): it points
        // wherever that reference does, not into the head (#888).
        let by_ref = matches!(typeck.pat_binding_modes().get(hirid).map(|m| m.0), Some(rustc_hir::ByRef::Yes(..)));
        let borrows_head = if by_ref { derefs <= 1 } else { derefs == 0 };
        let lender = if pat_ty.is_ref() && borrows_head { self.pattern_lender.clone() } else { None };
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
            typing_env(self.tcx, hirid.owner),
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
        // A by-value binding of a part of each element of a head moved
        // into the loop (`s` in `for (s, k) in names`), or a copy out of
        // an element of a borrowed head (`a` in `for &(a, b) in
        // &pairs`): an arrow from the head (#1390).
        let part = match (&self.pattern_elem, &path) {
          (Some(pe), Some(p)) if !by_ref && lender.is_none()
            && (if pe.moved { derefs == 0 } else { derefs == 1 && self.ty_is_copy(pat_ty, hirid.owner) }) =>
            Some((pe.head.clone(), p.clone(), pe.moved)),
          _ => None,
        };
        match (lender, part) {
          (Some((e, l)), _) => self.add_ev(line, e, to_rty, l, false),
          (None, Some((head, p, moved))) => {
            let copy = self.ty_is_copy(pat_ty, hirid.owner);
            // A reference moved out of a head holding references
            // borrows what they borrow (#1389).
            if pat_ty.is_ref() && !copy {
              if let Some((holder, lenders)) = self.pattern_elem.as_ref().and_then(|pe| pe.holder.clone()) {
                if let Some(rd) = self.borrow_map.get_mut(&name) {
                  rd.lender = lenders[0].clone();
                  rd.co_lenders = lenders[1..].to_vec();
                }
                if let Some(rd) = self.borrow_map.get(&holder) {
                  self.loop_moved_refs.insert(holder, rd.assigned_at);
                }
              }
            }
            let part = LoopHead::Part { path: p, copy, moved };
            match self.pattern_elem.as_mut() {
              Some(pe) if !moved => pe.deferred.push((line, to_rty, part)),
              _ => self.add_loop_head_move_ev(line, to_rty, head, part),
            }
            if let Some(pe) = self.pattern_elem.as_mut() {
              pe.source_pending = false;
            }
          }
          // `x` in `for (x, y) in [(&e, &g)]` (#1433): a copy of the
          // reference the literal holds in that part of each element.
          (None, None) if pat_ty.is_ref() && !by_ref && derefs == 0 && self.pattern_literal.is_some() => {
            let lenders = self.literal_part_lenders(self.pattern_literal.unwrap_or_default(), path.as_deref());
            if lenders.is_empty() {
              self.add_ev(line, Evt::Bind, to_rty, ResourceTy::Anonymous, false);
            } else {
              if let Some(rd) = self.borrow_map.get_mut(&name) {
                rd.lender = lenders[0].clone();
                rd.co_lenders = lenders[1..].to_vec();
              }
              let evt = if pat_ty.ref_mutability() == Some(Mutability::Mut) { Evt::MBorrow } else { Evt::SBorrow };
              for l in lenders {
                match l {
                  ResourceTy::Deref(r) => self.add_borrow_ev(line, evt.clone(), to_rty.clone(), ResourceTy::Value(r), true),
                  l => self.add_ev(line, evt.clone(), to_rty.clone(), l, false),
                }
              }
            }
          }
          (None, None) => self.add_ev(line, Evt::Bind, to_rty, ResourceTy::Anonymous, false),
        }
        if let Some(sub) = sub_pat {
          self.register_pattern_bindings_below(sub, line, derefs, path);
        }
      }
      PatKind::Tuple(ps, dd) | PatKind::TupleStruct(_, ps, dd) => {
        let paths = self.tuple_pat_paths(pat, ps, dd);
        for (p, sub) in ps.iter().zip(paths) {
          let sub = path.as_ref().zip(sub).map(|(a, b)| format!("{}{}", a, b));
          self.register_pattern_bindings_below(p, line, derefs, sub);
        }
      }
      PatKind::Struct(_, fs, _) => {
        for f in fs {
          let sub = path.as_ref().map(|a| format!("{}.{}", a, f.ident));
          self.register_pattern_bindings_below(f.pat, line, derefs, sub);
        }
      }
      PatKind::Or(ps) => {
        // alternatives bind the same names — first is enough. (Each
        // may take it from another part.)
        if let Some(p) = ps.first() { self.register_pattern_bindings_below(p, line, derefs, None); }
      }
      PatKind::Ref(inner, _, _) => {
        self.register_pattern_bindings_below(inner, line, derefs + 1, path);
      }
      PatKind::Box(inner)
      | PatKind::Deref(inner)
      | PatKind::Guard(inner, _) => {
        self.register_pattern_bindings_below(inner, line, derefs, None);
      }
      PatKind::Slice(before, mid, after) => {
        for (i, p) in before.iter().enumerate() {
          let sub = path.as_ref().map(|a| format!("{}[{}]", a, i));
          self.register_pattern_bindings_below(p, line, derefs, sub);
        }
        if let Some(p) = mid { self.register_pattern_bindings_below(p, line, derefs, None); }
        for p in after { self.register_pattern_bindings_below(p, line, derefs, None); }
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
      typing_env(self.tcx, hirid.owner),
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
    // A binding of a pattern that takes a branch's value apart (`let
    // One { a } = if c { One { a: &s1 } } else { .. };`, #1405), declared
    // before the branches: it may hold what any branch gives it.
    let in_branch_value = self.bind_existing && self.raps.contains_key(&name);
    self.define_lhs(name.clone(), bool_of_mut(annotation.1), init, lhs_ty);
    if in_branch_value && !is_skipped {
      self.add_value_lenders(&name, init);
    }
    if is_skipped {
      self.skip_raps.insert(name);
    } else {
      let rap = self.raps.get(&name).unwrap().rap.to_owned();
      // `let [d, _] = duo;`: d is moved out of `duo[0]`, which leaves the
      // rest in duo (#1285).
      match self.slice_elem_source(hirid, &self.rap_or_field_through_ref(init)).filter(|_| matches!(evt, Evt::Move)) {
        Some(elem) => self.add_ev(expr_to_line(init, &self.tcx), Evt::Move, ResourceTy::Value(rap.clone()), elem, false),
        None => self.match_rhs(ResourceTy::Value(rap.clone()), init, evt),
      }
      if in_branch_value {
        // (Only this branch's own lenders.)
        self.emit_arm_holder_literal_borrows(&ResourceTy::Value(rap), init);
      } else {
        self.emit_holder_literal_borrows(&name, init);
        self.resolve_block_local_lenders(&name, init);
      }
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
    let deferred: HashSet<String> = uninit.keys().map(|h| self.deferred_ident(*h)).collect();
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
  ///
  /// An or-pattern nested inside the pattern (`(a, (x, _) | (_, x))`,
  /// #612) is the same: the pattern matches as if each of its
  /// alternatives were spelled out in full (`(a, (x, _)) | (a, (_,
  /// x))`), so the places are collected over every such combination.
  /// A nested or-pattern is left in the returned pattern; the code
  /// that draws the bindings (`get_dec_of_pat_parts`,
  /// `bind_pat_from_member`) draws its first alternative.
  pub(crate) fn or_arm_stand_in(
    &self,
    pat: &'tcx Pat<'tcx>,
    parents: &[ResourceTy],
    parents_ty: &[rustc_middle::ty::Ty<'tcx>],
    scrutinee: Option<&'tcx Expr<'tcx>>,
    line: usize,
    typeck_res: &'tcx rustc_middle::ty::TypeckResults<'tcx>,
    tuple_vars: &[ResourceAccessPoint],
  ) -> (&'tcx Pat<'tcx>, Vec<OrConflict>) {
    let mut stand_in = pat;
    while let PatKind::Or(alts) = stand_in.kind {
      stand_in = &alts[0];
    }
    if !pat_has_or(pat) {
      return (pat, Vec::new());
    }
    // A tuple-literal scrutinee (`match (a, (b, c))`): its elements,
    // through nested tuple literals (#627).
    let from_literal = scrutinee.is_some_and(|e| self.literal_elems(e).is_some());
    let sources = match scrutinee {
      Some(e) if from_literal => self.literal_sources(pat, e, ""),
      _ => self.or_alt_sources(pat, parents, parents_ty),
    };
    let mut conflicts = Vec::new();
    let Some(first) = sources.first() else { return (stand_in, conflicts) };
    for name in first.keys() {
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
      binding_pats(stand_in, &mut bs);
      let binding = bs.iter().find(|b| matches!(b.kind, PatKind::Binding(_, _, ident, _) if ident.to_string() == *name));
      let by_ref = binding.is_some_and(|b| matches!(
        typeck_res.pat_binding_modes().get(b.hir_id).map(|m| m.0),
        Some(rustc_hir::ByRef::Yes(..))
      ));
      let ty = binding.map(|b| typeck_res.node_type(b.hir_id));
      // `match (a, b)` moves `a` and `b` into a temporary tuple, which
      // was drawn only for variables without part columns (#327): with a
      // struct or tuple `a`, what `x` takes or borrows would be drawn as
      // taken from `a` itself, which no longer has it. A tuple literal
      // scrutinee moves each part column into the tuple too now (#627),
      // so `literal_sources` names the part `x` comes from.
      let copied = !by_ref && ty.is_some_and(|t| self.ty_is_copy(t, pat.hir_id.owner));
      if parents.len() > 1 && !copied && !from_literal {
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
      let mut sole_runtime_source = None;
      let mut source_names = BTreeMap::new();
      let mut other_bindings = BTreeMap::new();
      if let (Some(binding), Some(facts)) = (binding, self.facts) {
        let sites: Vec<_> = facts.binding_sources.iter().filter(|s| s.runs && pat.span.contains(s.site)).collect();
        // A parameter column may end at the enclosing tuple. Keep the
        // precise MIR field name even when the dot is drawn on its parent.
        let mapped: Option<BTreeMap<_, _>> = sites.iter().filter(|s| s.declaration == binding.span)
          .map(|s| {
            let from = s.from.as_ref()?;
            let rap = raps.iter().find(|r| r.name() == from)
              .or_else(|| raps.iter().find(|r| s.parents.contains(r.name())))?;
            Some((from.clone(), rap.name().clone()))
          }).collect();
        if let Some(mapped) = mapped.filter(|m| !m.is_empty()
          && m.values().collect::<BTreeSet<_>>().len() == m.len()) {
          if mapped.len() == 1 { sole_runtime_source = mapped.keys().next().cloned(); }
          source_names = mapped.into_iter().map(|(from, column)| (column, from)).collect();
          if !by_ref { raps.retain(|r| source_names.contains_key(r.name())); }
        }
        if !by_ref && !copied {
          for rap in &raps {
            let source = source_names.get(rap.name()).unwrap_or(rap.name());
            for place in std::iter::once(source.clone()).chain(self.struct_members_deep(rap.name()).into_iter().map(|r| r.name().clone())) {
              other_bindings.insert(place.clone(), crate::facts::OtherBindings::for_place(&sites, binding.span, &place));
            }
          }
        }
      }
      conflicts.push(OrConflict { name: name.clone(), places: raps, sole_runtime_source, source_names, other_bindings });
    }
    (stand_in, conflicts)
  }

  /// `or_conflict_events` for an `if let` / `while let` pattern (#597),
  /// or a `let`'s (`in_let`), whose bindings live until `body_end`: the
  /// end of the body (of the match arm, for an `if let` guard; of the
  /// enclosing block, for a `let`). Returns the events at the pattern's
  /// line. Where a reference's possible borrows end is queued in
  /// `or_ref_ends`, for `emit_borrow_ends` (#734), and the references
  /// taken out of `bound` are added to `decls`.
  #[expect(clippy::too_many_arguments, reason = "or_conflict_events' arguments, and where its results go")]
  pub(crate) fn if_let_or_conflict_events(
    &mut self,
    conflicts: &[OrConflict],
    bound: &mut Vec<(ResourceAccessPoint, Evt, ResourceTy, Option<bool>)>,
    line: usize,
    tuple_vars: &[ResourceAccessPoint],
    tuple: &str,
    body_end: usize,
    in_let: bool,
    decls: &mut HashSet<ResourceAccessPoint>,
  ) -> Vec<ExternalEvent> {
    let out = self.or_conflict_events(conflicts, bound, line, tuple_vars, tuple, body_end, in_let);
    self.or_ref_ends.extend(out.end);
    decls.extend(out.decls);
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
  ///
  /// `in_let`: the pattern is a `let`'s, which is no branch (see
  /// `ExternalEvent::OrAltMove`).
  #[expect(clippy::too_many_arguments, reason = "the pattern's bindings, and where it sits")]
  fn or_conflict_events(
    &mut self,
    conflicts: &[OrConflict],
    bound: &mut Vec<(ResourceAccessPoint, Evt, ResourceTy, Option<bool>)>,
    line: usize,
    tuple_vars: &[ResourceAccessPoint],
    tuple: &str,
    arm_end: usize,
    in_let: bool,
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
      let names: Vec<String> = c.sole_runtime_source.as_ref().map_or_else(|| c.places.iter().map(|r| c.source_names.get(r.name()).unwrap_or(r.name()).clone()).collect(), |p| vec![p.clone()]);
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
                ro: place.clone(), into: x.clone(), from: names.clone(), mutable, end, first: i == 0, chain: None, id,
              };
              if end { out.end.push((at, ev)); } else { out.start.push(ev); }
            }
          }
          out.decls.push(x);
          taken.push(k);
        }
        Evt::Move | Evt::Copy => {
          if c.sole_runtime_source.as_ref() == Some(c.places[0].name()) && !Self::in_scrutinee_tuple(tuple_vars, &c.places[0]) {
            entry.2 = ResourceTy::Value(c.places[0].clone());
            continue;
          }
          if matches!(entry.1, Evt::Move) {
            for place in &c.places {
              let id = *self.unique_id;
              *self.unique_id += 1;
              let in_tuple = Self::in_scrutinee_tuple(tuple_vars, place).then(|| tuple.to_owned());
              let source = c.source_names.get(place.name()).unwrap_or(place.name());
              out.start.push(ExternalEvent::OrAltMove {
                ro: place.clone(), into: xname.clone(), from: names.clone(), source: source.clone(), others: c.other_bindings.get(source).cloned().unwrap_or_default(), tuple: in_tuple.clone(), part_of: None, in_let, id,
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
                  ro: member.clone(), into, from: names.clone(), source: member.name().clone(), others: c.other_bindings.get(member.name()).cloned().unwrap_or_default(), tuple: in_tuple.clone(),
                  part_of: Some((xname.clone(), place.name().to_string())), in_let, id,
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

  /// For each way `pat` can match (each alternative of an or-pattern,
  /// with the or-patterns nested in it spelled out, #612): each of its
  /// bindings, and the place it's bound from (as a key that tells
  /// places apart, and the place), the way the arm code below picks it
  /// (one parent per element of a tuple-literal scrutinee, then
  /// `pat_parts`).
  fn or_alt_sources(
    &self,
    pat: &'tcx Pat<'tcx>,
    parents: &[ResourceTy],
    parents_ty: &[rustc_middle::ty::Ty<'tcx>],
  ) -> Vec<BTreeMap<String, (String, Option<ResourceTy>)>> {
    match pat.kind {
      PatKind::Or(alts) => {
        let mut out = Vec::new();
        for alt in alts {
          for m in self.or_alt_sources(alt, parents, parents_ty) {
            if !out.contains(&m) {
              out.push(m);
            }
          }
        }
        out
      }
      PatKind::Tuple(ps, _) | PatKind::TupleStruct(_, ps, _) if parents.len() > 1 && ps.len() == parents.len() => {
        let mut out = vec![BTreeMap::new()];
        for (i, p) in ps.iter().enumerate() {
          out = or_sources_product(out, self.pat_sources(p, &parents[i], parents_ty[i], &format!("{}:", i)));
        }
        out
      }
      _ if parents.len() == 1 => self.pat_sources(pat, &parents[0], parents_ty[0], ""),
      _ => {
        let mut out = BTreeMap::new();
        let mut bs = Vec::new();
        binding_pats(pat, &mut bs);
        for b in bs {
          if let PatKind::Binding(_, _, ident, _) = b.kind {
            out.insert(ident.to_string(), ("*".to_owned(), None));
          }
        }
        vec![out]
      }
    }
  }

  /// `or_alt_sources` for a pattern matched against the tuple literal
  /// `e` (`match (a, (b, c))`): each sub-pattern against its element,
  /// through nested tuple literals (#627), and an element that isn't a
  /// tuple literal the way `pat_sources` does (a variable's parts).
  fn literal_sources(
    &self,
    pat: &'tcx Pat<'tcx>,
    e: &'tcx Expr<'tcx>,
    prefix: &str,
  ) -> Vec<BTreeMap<String, (String, Option<ResourceTy>)>> {
    let Some(elems) = self.literal_elems(e) else {
      let typeck = self.tcx.typeck(e.hir_id.owner);
      return self.pat_sources(pat, &self.rap_or_field_through_ref(e), typeck.node_type(e.hir_id), prefix);
    };
    match pat.kind {
      // `Err(x)` on `Ok(f)` (#807): it can't match, so it binds nothing
      // from `f`.
      PatKind::TupleStruct(..) if self.ctor_pat_mismatch(pat, e) => vec![BTreeMap::new()],
      PatKind::Or(alts) => {
        let mut out = Vec::new();
        for alt in alts {
          for m in self.literal_sources(alt, e, prefix) {
            if !out.contains(&m) {
              out.push(m);
            }
          }
        }
        out
      }
      PatKind::Tuple(ps, dd) | PatKind::TupleStruct(_, ps, dd) => {
        let gap = elems.len().saturating_sub(ps.len());
        let mut out = vec![BTreeMap::new()];
        for (k, p) in ps.iter().enumerate() {
          let i = match dd.as_opt_usize() {
            Some(d) if k >= d => k + gap,
            _ => k,
          };
          let Some(el) = elems.get(i) else { continue };
          out = or_sources_product(out, self.literal_sources(p, el, &format!("{}{}:", prefix, i)));
        }
        out
      }
      // A name (or `_`) matching the whole literal: it has no column.
      _ => {
        let mut out = BTreeMap::new();
        let mut bs = Vec::new();
        binding_pats(pat, &mut bs);
        for b in bs {
          if let PatKind::Binding(_, _, ident, _) = b.kind {
            out.insert(ident.to_string(), (format!("{}*", prefix), None));
          }
        }
        vec![out]
      }
    }
  }

  /// See `or_alt_sources`; mirrors `get_dec_of_pat_parts`.
  fn pat_sources(
    &self,
    pat: &'tcx Pat<'tcx>,
    parent: &ResourceTy,
    parent_ty: rustc_middle::ty::Ty<'tcx>,
    prefix: &str,
  ) -> Vec<BTreeMap<String, (String, Option<ResourceTy>)>> {
    let whole = |p: &'tcx Pat<'tcx>| {
      let mut out = BTreeMap::new();
      let mut bs = Vec::new();
      binding_pats(p, &mut bs);
      for b in bs {
        if let PatKind::Binding(_, _, ident, _) = b.kind {
          out.insert(ident.to_string(), (format!("{}{}", prefix, parent.real_name()), Some(parent.clone())));
        }
      }
      out
    };
    // `(x, _) | (_, x)` nested in the pattern, on a part with columns
    // of its own: each alternative is a way to match it.
    if let PatKind::Or(alts) = pat.kind {
      let mut out = Vec::new();
      for alt in alts {
        for m in self.pat_sources(alt, parent, parent_ty, prefix) {
          if !out.contains(&m) {
            out.push(m);
          }
        }
      }
      return out;
    }
    match self.pat_parts(pat, parent, parent_ty) {
      Some(subs) => {
        let mut out = vec![BTreeMap::new()];
        for (sp, part) in subs {
          let sub = match part {
            Some((part_rty, part_ty)) => self.pat_sources(sp, &part_rty, part_ty, prefix),
            None => vec![whole(sp)],
          };
          out = or_sources_product(out, sub);
        }
        out
      }
      None => vec![whole(pat)],
    }
  }

  /// Set the uninitialized-locals state after a branch construct from
  /// the state at the end of each branch (`None` = the implicit empty
  /// else of an `if`): its definitely and its maybe uninitialized
  /// `let x;` locals. Only branches that reach the join count;
  /// branches of type `!` (`return`, `panic!()`) don't. A `let x;` local
  /// is still definitely uninitialized after the join iff it is so at
  /// the end of every such branch. If it is initialized at the end of
  /// some and not of others, it is maybe-initialized (#244): a later
  /// `x = …` is a reassignment whose old value a drop flag drops. If
  /// every branch that gets here initialized it, it is initialized.
  /// `known`: the locals declared before the construct (definitely or
  /// maybe uninitialized there); one declared inside a branch is gone
  /// after it. Returns the maybe-initialized locals that have a column,
  /// for the conditional's Branch event.
  fn join_uninit_locals(
    &mut self,
    ends: Vec<(Option<&'tcx Expr<'tcx>>, HashMap<HirId, DeferredLet>, HashSet<HirId>)>,
    known: &HashSet<HirId>,
  ) -> HashSet<ResourceAccessPoint> {
    let diverges = |e: &Option<&'tcx Expr<'tcx>>| {
      e.is_some_and(|e| self.expr_diverges(e))
    };
    let mut reaching: Vec<(HashMap<HirId, DeferredLet>, HashSet<HirId>)> =
      ends.iter().filter(|(e, _, _)| !diverges(e)).map(|(_, m, s)| (m.clone(), s.clone())).collect();
    if reaching.is_empty() {
      reaching = ends.into_iter().map(|(_, m, s)| (m, s)).collect();
    }
    let mut joined = reaching.first().map(|(m, _)| m.clone()).unwrap_or_default();
    joined.retain(|id, _| reaching.iter().all(|(m, _)| m.contains_key(id)));
    #[expect(clippy::disallowed_methods, reason = "collected into a set; order is lost")]
    let maybe: HashSet<HirId> = reaching.iter()
      .flat_map(|(m, s)| m.keys().chain(s.iter()).copied())
      .filter(|id| known.contains(id) && !joined.contains_key(id))
      .collect();
    self.uninit_locals = joined;
    // Still maybe-initialized by a loop above only where no branch here
    // assigned it (#786).
    self.maybe_init_by_loop.retain(|id| reaching.iter().all(|(_, s)| s.contains(id)));
    #[expect(clippy::disallowed_methods, reason = "collected into a set; order is lost")]
    let cols = maybe.iter()
      .filter_map(|id| self.raps.get(&self.deferred_name(*id)).map(|rd| rd.rap.clone()))
      .collect();
    self.maybe_init_locals = maybe;
    // A struct's field columns (`p.name`) got their values with it.
    self.with_struct_members(&cols)
  }

  /// The columns of the `let x;` locals `ids` (a struct's fields' too).
  fn deferred_cols(&self, ids: &HashSet<HirId>) -> HashSet<ResourceAccessPoint> {
    #[expect(clippy::disallowed_methods, reason = "collected into a set; order is lost")]
    let cols = ids.iter()
      .filter_map(|id| self.raps.get(&self.deferred_name(*id)).map(|rd| rd.rap.clone()))
      .collect();
    self.with_struct_members(&cols)
  }

  /// A loop's body is about to be visited (#786): remember which `let
  /// x;` locals have no value as it starts. Returns the state to hand
  /// to `leave_loop_inits`. `target`: the loop its `break`s name.
  fn enter_loop_inits(&mut self, target: HirId) -> (HashMap<HirId, DeferredLet>, HashSet<HirId>) {
    #[expect(clippy::disallowed_methods, reason = "collected into a set; order is lost")]
    let entry = self.uninit_locals.keys().copied().collect();
    self.loop_entry_uninit.push((target, entry));
    (self.uninit_locals.clone(), self.maybe_init_locals.clone())
  }

  /// The loop `loop_expr` (whose `break`s name `target`) has been
  /// visited (#786). Join the `let x;` locals' state over the ways out
  /// of it, as `join_uninit_locals` does for a conditional: each
  /// `break`, and for a `while` / `for` (`may_skip`), the condition
  /// failing before the first iteration or after the body. A local
  /// that had no value before the loop and is assigned on some of those
  /// paths only is maybe-initialized after it: a `LoopMaybeInit` event
  /// on its column says so where the loop ends. The state at a `break`
  /// is as of the first time round: on a later one, a local the body
  /// assigned may hold a value there, so it is definitely uninitialized
  /// only if the body's end (where the next time round starts) has it
  /// uninitialized too.
  fn leave_loop_inits(
    &mut self,
    loop_expr: &'tcx Expr<'tcx>,
    target: HirId,
    (uninit_before, maybe_before): (HashMap<HirId, DeferredLet>, HashSet<HirId>),
    may_skip: bool,
  ) {
    self.loop_entry_uninit.pop();
    let (mine, rest): (Vec<_>, Vec<_>) =
      std::mem::take(&mut self.break_uninit).into_iter().partition(|(t, _, _)| *t == target);
    self.break_uninit = rest;
    let mut ends: Vec<(HashSet<HirId>, HashSet<HirId>)> = mine.into_iter().map(|(_, u, m)| (u, m)).collect();
    if may_skip {
      #[expect(clippy::disallowed_methods, reason = "collected into a set; order is lost")]
      let before: HashSet<HirId> = uninit_before.keys().copied().collect();
      #[expect(clippy::disallowed_methods, reason = "collected into a set; order is lost")]
      let after: HashSet<HirId> = self.uninit_locals.keys().copied().collect();
      ends.push((before, maybe_before.clone()));
      ends.push((after, self.maybe_init_locals.clone()));
    }
    // A `loop` with no `break`: nothing after it runs.
    if ends.is_empty() {
      return;
    }
    let mut definite = uninit_before.clone();
    definite.retain(|id, _| ends.iter().all(|(u, _)| u.contains(id)) && self.uninit_locals.contains_key(id));
    #[expect(clippy::disallowed_methods, reason = "collected into a set; order is lost")]
    let maybe: HashSet<HirId> = ends.iter()
      .flat_map(|(u, m)| u.iter().chain(m.iter()).copied())
      .filter(|id| (uninit_before.contains_key(id) || maybe_before.contains(id)) && !definite.contains_key(id))
      .collect();
    // Newly maybe-initialized: no value before the loop.
    #[expect(clippy::disallowed_methods, reason = "sorted by name below")]
    let mut newly: Vec<HirId> = maybe.iter().filter(|id| uninit_before.contains_key(id)).copied().collect();
    newly.sort_by_key(|id| self.deferred_name(*id));
    self.uninit_locals = definite;
    self.maybe_init_locals = maybe;
    let line = span_end_line(&loop_expr.span, &self.tcx);
    for id in newly {
      self.maybe_init_by_loop.insert(id);
      let Some(rd) = self.raps.get(&self.deferred_name(id)) else { continue };
      let ro = rd.rap.clone();
      if self.skip_raps.contains(ro.name()) || ro.is_ref() {
        continue;
      }
      self.add_external_event(line, ExternalEvent::LoopMaybeInit { ro, before_loop: None, nested: false, id: *self.unique_id });
      *self.unique_id += 1;
    }
  }

  /// `x = rhs` just initialized the `let x;` local `hirid` inside the
  /// body of a loop it had no value before (#786): later times round
  /// the loop, the same assignment drops the value an earlier iteration
  /// assigned, if x still holds it. Drawn as a drop on the assignment,
  /// whose text says so.
  fn drop_earlier_iteration(&mut self, hirid: HirId, lhs_expr: &'tcx Expr<'tcx>) {
    // (A loop that never comes back round has no later time round,
    // #1467.)
    if !self.loop_entry_uninit.iter().any(|(id, s)| s.contains(&hirid)
      && self.loop_never_rounds(self.tcx.hir_span(*id)) != Some(true)) {
      return;
    }
    let ty = self.tcx.typeck(hirid.owner).node_type(hirid);
    let env = typing_env(self.tcx, hirid.owner);
    if ty.is_ref() || self.tcx.type_is_copy_modulo_regions(env, ty) || !self.needs_drop(ty, hirid.owner) {
      return;
    }
    let Some(rd) = self.raps.get(&binding_name(&self.tcx, hirid)) else { return };
    let ro = rd.rap.clone();
    if self.skip_raps.contains(ro.name()) {
      return;
    }
    let line = expr_to_line(lhs_expr, &self.tcx);
    self.add_external_event(line, ExternalEvent::OwnerDropAtReassign {
      ro,
      id: *self.unique_id,
      maybe_moved: false,
      by_cond: false,
      loop_head: false,
      parts: None,
      maybe_init: Some(InitBy::EarlierIteration),
      payload: false,
      contents_gone: false,
    });
    *self.unique_id += 1;
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
    self.deferred_decl_at.insert(name.clone(), decl.frames_at);
    let names_at = self.in_scope.len();
    self.define_lhs(name.clone(), bool_of_mut(decl.annotation.1), rhs, lhs_ty);
    self.fill_deferred_slot(hirid, names_at);
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
        rd.extended_temp.push(tmp_name.clone());
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
      // Moved here, into the temporary or (when it has no column) into
      // nothing: either way not again when the pattern's `_` is visited.
      // With no column it used to be moved twice, which drew its
      // fields' moves twice too (#1284).
      let n_events = self.preprocessed_events.len();
      if not_by_value && (self.move_into_literal(se, is_skipped).is_some() || self.preprocessed_events.len() > n_events) {
        self.premoved.insert(se.hir_id);
      }
    }
    // A fresh value (a call's result, a nested literal) that the pattern
    // leaves is part of the same temporary: with no variable moved in,
    // the temporary still exists, and is dropped (or, with a `ref`
    // binding, kept) with everything in it (#483).
    let fresh: Vec<&'tcx Expr<'tcx>> = pairs.iter()
      .filter(|(sp, _)| match sp.kind {
        PatKind::Wild => true,
        PatKind::Binding(ann, ..) => matches!(ann.0, rustc_hir::ByRef::Yes(..)),
        _ => false,
      })
      .map(|(_, se)| *se)
      .chain(ignored.iter().copied())
      .filter(|e| !matches!(
        e.kind,
        ExprKind::Path(_) | ExprKind::Field(..) | ExprKind::Index(..) | ExprKind::Unary(UnOp::Deref, _)
      ))
      .collect();
    for se in ignored {
      self.move_into_literal(se, is_skipped);
    }
    if let Some(e) = fresh.first() {
      // What a call among them returns is moved into it (#1376), drawn
      // like a variable's value: an arrow from the call.
      let mut calls = Vec::new();
      for f in &fresh {
        self.literal_calls(f, &mut calls);
      }
      self.start_literal_temp(e, is_skipped, calls);
    }
  }

  /// The calls (not constructors) among the elements of the literal `e`,
  /// or `e` itself, whose results need drop, with their functions' names
  /// (`mk` in `(1, mk())`, `Some(mk())`, `P { b: mk(), .. }`).
  fn literal_calls(&self, e: &'tcx Expr<'tcx>, out: &mut Vec<(&'tcx Expr<'tcx>, String)>) {
    if e.span.from_expansion() {
      return;
    }
    let typeck = self.tcx.typeck(e.hir_id.owner);
    match e.kind {
      ExprKind::Tup(es) | ExprKind::Array(es) => es.iter().for_each(|x| self.literal_calls(x, out)),
      ExprKind::Struct(_, fs, _) => fs.iter().for_each(|f| self.literal_calls(f.expr, out)),
      ExprKind::Call(callee, args) if is_ctor_path(callee, typeck) => args.iter().for_each(|x| self.literal_calls(x, out)),
      ExprKind::Call(callee, _) | ExprKind::MethodCall(_, callee, ..) if self.needs_drop(typeck.expr_ty(e), e.hir_id.owner) => {
        let name = match e.kind {
          ExprKind::MethodCall(seg, ..) => Some(seg.ident.as_str().to_owned()),
          _ => hirid_to_var_name(callee.hir_id, &self.tcx),
        };
        if let Some(n) = name {
          out.push((e, n));
        }
      }
      _ => {}
    }
  }

  /// The temporary of a literal that nothing was moved into (every
  /// element the pattern leaves is a fresh value): made here, and
  /// holding the literal's value from this line (#483). What the `calls`
  /// among those values return is drawn moving into it (#1376). (When a
  /// variable was moved into it first, its tooltips already say that
  /// what the pattern didn't bind stays in it.)
  fn start_literal_temp(&mut self, e: &'tcx Expr<'tcx>, is_skipped: bool, calls: Vec<(&'tcx Expr<'tcx>, String)>) {
    if is_skipped || e.span.from_expansion() {
      return;
    }
    let Some((key, _)) = self.literal_temp_key(e) else { return };
    if self.literal_temps.contains_key(&key) {
      return;
    }
    let Some(tmp) = self.literal_temp(e) else { return };
    // (A `let`-`else`'s literal temporary is drawn as built by the
    // literal itself, `(..)`.)
    // One arrow per function: `(make(), make())` draws one from make.
    let mut seen: Vec<String> = Vec::new();
    let calls: Vec<(&'tcx Expr<'tcx>, String)> = calls.into_iter().filter(|(_, n)| {
      let new = !seen.contains(n);
      seen.push(n.clone());
      new
    }).collect();
    let fns: Vec<(usize, ResourceAccessPoint)> = if tmp.is_let_literal_temporary() {
      calls.into_iter().filter_map(|(call, name)| {
        self.add_fn(name.clone());
        let f = self.raps.get(&fn_key(&name))?.rap.to_owned();
        Some((expr_to_line(call, &self.tcx), f))
      }).collect()
    } else {
      Vec::new()
    };
    if fns.is_empty() {
      let line = expr_to_line(e, &self.tcx);
      self.add_ev(line, Evt::Bind, ResourceTy::Value(tmp), ResourceTy::Anonymous, false);
      return;
    }
    for (line, f) in fns {
      self.add_ev(line, Evt::Move, ResourceTy::Value(tmp.clone()), ResourceTy::Value(f), false);
    }
  }

  /// The `let` statement whose pattern takes apart the literal that `e`
  /// is an element of, and the key of the literal's temporary in
  /// `literal_temps`: the `let`'s, or, for a literal that is the value
  /// of a branch of the `let`'s `if` / `match` (#1011), the literal's.
  fn literal_temp_key(&self, e: &'tcx Expr<'tcx>) -> Option<(HirId, &'tcx rustc_hir::LetStmt<'tcx>)> {
    use rustc_hir::Node;
    let let_stmt = self.tcx.hir_parent_iter(e.hir_id).find_map(|(_, n)| match n {
      Node::LetStmt(l) => Some(l),
      _ => None,
    })?;
    if self.bind_existing && self.branch_value_owner.is_some() {
      if let Node::Expr(literal) = self.tcx.parent_hir_node(e.hir_id) {
        if !matches!(self.tcx.parent_hir_node(literal.hir_id), Node::LetStmt(_)) {
          return Some((literal.hir_id, let_stmt));
        }
      }
    }
    Some((let_stmt.hir_id, let_stmt))
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
    if ty.is_ref() || !self.needs_drop(ty, init.hir_id.owner) || self.inside_one_line_branch(init) {
      return None;
    }
    let (mut moves_out, mut mutable) = (false, false);
    l.pat.each_binding(|ann, hid, _, _| match ann.0 {
      rustc_hir::ByRef::Yes(_, m) => mutable |= m == Mutability::Mut,
      rustc_hir::ByRef::No => moves_out |= self.needs_drop(typeck.node_type(hid), init.hir_id.owner),
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
    let (key, let_stmt) = self.literal_temp_key(e)?;
    if let Some(t) = self.literal_temps.get(&key) {
      return Some(t.clone());
    }
    let literal = self.tcx.hir_parent_iter(e.hir_id).find_map(|(_, n)| match n {
      Node::Expr(p) => Some(p),
      _ => None,
    })?;
    // The value of a branch of the `let`'s `if` / `match` (`let (a, _)
    // = if c { (f, g) } else { .. };`, #1011): each branch's literal
    // has a temporary of its own, declared in that branch (whose
    // pairing comes after its visit, `pat_leaves_parts`). Rust drops
    // it at the end of the `let`; on the branch's path nothing else
    // happens before then, so its drop is drawn where the literal is.
    // What the pattern leaves in the literal is all that is dropped
    // with it: when none of that needs drop (`let (x, _) = (s, p);`
    // with `p: P { a: i32 }`), nothing is dropped at the end of the
    // `let`, as for a literal with nothing to drop (#1163).
    let typeck = self.tcx.typeck(e.hir_id.owner);
    let env = typing_env(self.tcx, e.hir_id.owner);
    let (mut extended, mut mutable) = (false, false);
    let_stmt.pat.each_binding(|ann, _, _, _| {
      if let rustc_hir::ByRef::Yes(_, m) = ann.0 {
        extended = true;
        mutable |= m == Mutability::Mut;
      }
    });
    // `let (ref x, _) = (p, q);` with `p`, `q` of a type that isn't Copy
    // but has nothing to drop: x borrows from the literal's temporary,
    // which takes p's and q's values and lives to the end of the block
    // (#1363). It is drawn too, holding a plain value.
    let plain_borrowed = extended && key == let_stmt.hir_id
      && !self.ty_is_copy(typeck.expr_ty(literal), e.hir_id.owner)
      && !self.needs_drop(typeck.expr_ty(literal), e.hir_id.owner);
    if !self.pat_leaves_drop(let_stmt.pat, typeck, env) && !plain_borrowed {
      return None;
    }
    if key != let_stmt.hir_id {
      let ty = self.tcx.typeck(e.hir_id.owner).expr_ty(literal);
      if !self.needs_drop(ty, e.hir_id.owner) {
        return None;
      }
      self.temp_count += 1;
      let tmp_name = temporary_label(self.temp_count);
      let end_line = span_end_line(&literal.span, &self.tcx);
      self.add_owner(tmp_name.clone(), false, false, false, InteriorMut::No, end_line, false);
      if let Some(ResourceAccessPoint::Owner(o)) = self.raps.get_mut(&tmp_name).map(|rd| &mut rd.rap) {
        o.unbound_temp = true;
        o.let_literal_temp = true;
        o.assign_literal_temp = matches!(let_stmt.source, rustc_hir::LocalSource::AssignDesugar);
      }
      self.raps.get_mut(&tmp_name).unwrap().temp_owner = self.branch_value_owner;
      let tmp = self.raps.get(&tmp_name).unwrap().rap.to_owned();
      // (Its drop is drawn after what goes into it: `pair_pat_with_value`.)
      self.branch_temp_drops.push((end_line, tmp.clone(), strip_module_paths(&ty.to_string())));
      self.literal_temps.insert(key, tmp.clone());
      return Some(tmp);
    }
    let ty = typeck.expr_ty(literal);
    if (!self.needs_drop(ty, e.hir_id.owner) && !plain_borrowed) || self.inside_one_line_branch(e) {
      return None;
    }
    self.temp_count += 1;
    let tmp_name = temporary_label(self.temp_count);
    // The variables moved into it read as moving into the literal on
    // the right of the `let`, not into one nothing binds (#570).
    if extended {
      self.add_owner(tmp_name.clone(), mutable, false, plain_borrowed, InteriorMut::No, self.current_scope, !self.inside_branch);
      if let Some(ResourceAccessPoint::Owner(o)) = self.raps.get_mut(&tmp_name).map(|rd| &mut rd.rap) {
        o.let_literal_temp = true;
      }
    } else {
      let (when, end_line, owner) = self.temporary_scope_end(e)?;
      self.add_owner(tmp_name.clone(), false, false, false, InteriorMut::No, end_line, !self.inside_branch);
      if let Some(ResourceAccessPoint::Owner(o)) = self.raps.get_mut(&tmp_name).map(|rd| &mut rd.rap) {
        o.unbound_temp = true;
        o.let_literal_temp = true;
        o.assign_literal_temp = matches!(let_stmt.source, rustc_hir::LocalSource::AssignDesugar);
      }
      self.raps.get_mut(&tmp_name).unwrap().temp_owner = owner;
      let tmp = self.raps.get(&tmp_name).unwrap().rap.to_owned();
      let ty = strip_module_paths(&ty.to_string());
      let id = *self.unique_id;
      self.add_external_event(end_line, ExternalEvent::TemporaryDrop { ro: tmp, ty: Some(ty), when: when.to_owned(), literal: true, early: false, part: None, id });
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
      *is_reborrow = is_explicit_reborrow(init, typeck) || is_coerced_ref_var_reborrow(init, typeck);
    }
    self.add_external_event(line_num, ev);
    *self.unique_id += 1;
  }
}

/// True iff `callee` names a tuple struct / variant constructor
/// (`Some`, `Wrapper`).
pub(crate) fn is_ctor_path<'tcx>(callee: &Expr<'tcx>, typeck: &rustc_middle::ty::TypeckResults<'tcx>) -> bool {
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

/// Whether a variable holds a value at some point, on the paths that
/// reach it (`ExprVisitor::ownership_here`).
#[derive(Clone, Copy, PartialEq, Debug)]
enum OwnHere { Yes, No, Maybe }

/// The local variables `e` mentions, pushed onto `out`.
fn mentioned_locals<'tcx>(e: &'tcx Expr<'tcx>, out: &mut Vec<String>) {
  for m in locals_in(e, ClosureLocals::Skip) {
    if let Some(ExprKind::Path(rustc_hir::QPath::Resolved(None, path))) = m.expr.map(|x| &x.kind) {
      if let Some(seg) = path.segments.last() {
        out.push(seg.ident.to_string());
      }
    }
  }
}

/// A branch's body: an expression (an if/else branch, a match arm), or
/// the `else` block of a let-else (#906), which HIR keeps as a bare
/// block.
#[derive(Clone, Copy)]
enum BranchBody<'tcx> {
  Expr(&'tcx Expr<'tcx>),
  LetElse(&'tcx rustc_hir::Block<'tcx>),
}

fn branch_leaves<'tcx>(tcx: rustc_middle::ty::TyCtxt<'tcx>, body: BranchBody<'tcx>) -> BranchLeaves {
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
                // A function of the user's that always panics (#1553).
                if never_fn_kind(self.tcx, def_id) == Some(NeverFn::Panics) {
                  self.panics = true;
                }
              }
            }
          }
          // Or a local closure that does (#1562; rustc may type its call
          // `()`).
          if closure_call_never_kind(self.tcx, e).is_some_and(|(k, _)| k == NeverFn::Panics) {
            self.panics = true;
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
  let block_last = |b: &'tcx rustc_hir::Block<'tcx>| b.expr.or_else(|| match b.stmts.last().map(|s| &s.kind) {
    Some(StmtKind::Semi(e)) | Some(StmtKind::Expr(e)) => Some(*e),
    _ => None,
  });
  let last = match body {
    BranchBody::Expr(body) => {
      f.visit_expr(body);
      match body.kind {
        ExprKind::Block(b, _) => block_last(b),
        _ => Some(body),
      }
    }
    BranchBody::LetElse(b) => {
      f.visit_block(b);
      block_last(b)
    }
  };
  let ends_in_its_only_break = (f.control == [BranchExit::Break] || f.control == [BranchExit::BreakBlock])
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

/// Whether evaluating `e` runs all of it: no `&&` / `||`, `if`,
/// `match` (or `?`), loop, closure, `let … else` or jump inside it
/// (#968). See `ChainEffect::straight`.
fn runs_straight(e: &Expr<'_>) -> bool {
  struct Find(bool);
  impl<'tcx> Visitor<'tcx> for Find {
    fn visit_expr(&mut self, e: &'tcx Expr<'tcx>) {
      match e.kind {
        ExprKind::Binary(op, ..) if matches!(op.node, rustc_hir::BinOpKind::And | rustc_hir::BinOpKind::Or) => self.0 = false,
        ExprKind::If(..) | ExprKind::Match(..) | ExprKind::Loop(..) | ExprKind::Closure(..)
        | ExprKind::Break(..) | ExprKind::Continue(_) | ExprKind::Ret(_) | ExprKind::Become(_) => self.0 = false,
        _ => {}
      }
      intravisit::walk_expr(self, e);
    }
    fn visit_local(&mut self, l: &'tcx LetStmt<'tcx>) {
      if l.els.is_some() {
        self.0 = false;
      }
      intravisit::walk_local(self, l);
    }
  }
  let mut f = Find(true);
  match e.kind {
    // A `let` part: its scrutinee (the pattern is what can fail).
    ExprKind::Let(let_expr) => f.visit_expr(let_expr.init),
    _ => f.visit_expr(e),
  }
  f.0
}

/// The columns (`s`, `p.name`) that an assignment in `e` (outside any
/// closure) gives a new value: `s = …` gives `s` and its fields one,
/// `p.name = …` gives `p.name` one (#1026).
fn assigned_names<'tcx>(tcx: &rustc_middle::ty::TyCtxt<'tcx>, e: &'tcx Expr<'tcx>) -> Vec<String> {
  struct Find<'a, 'tcx> { tcx: &'a rustc_middle::ty::TyCtxt<'tcx>, out: Vec<String> }
  impl<'a, 'tcx> Visitor<'tcx> for Find<'a, 'tcx> {
    fn visit_expr(&mut self, e: &'tcx Expr<'tcx>) {
      match e.kind {
        ExprKind::Closure(..) => return,
        ExprKind::Assign(lhs, ..) => {
          let mut fields: Vec<String> = Vec::new();
          let mut place = lhs;
          while let ExprKind::Field(base, ident) = place.kind {
            fields.push(ident.as_str().to_owned());
            place = base;
          }
          if let Some(l) = local_of_path(place) {
            let mut name = binding_name(self.tcx, l);
            for f in fields.iter().rev() {
              name = format!("{}.{}", name, f);
            }
            self.out.push(name);
          }
        }
        _ => {}
      }
      intravisit::walk_expr(self, e);
    }
  }
  let mut f = Find { tcx, out: Vec::new() };
  f.visit_expr(e);
  f.out
}

/// Whether `e` (outside any closure) contains a `return`, or a `?`,
/// which returns on its error path.
fn body_returns(e: &Expr<'_>) -> bool {
  struct Find(bool);
  impl<'tcx> Visitor<'tcx> for Find {
    fn visit_expr(&mut self, e: &'tcx Expr<'tcx>) {
      match e.kind {
        ExprKind::Closure(..) => return,
        ExprKind::Ret(_) => self.0 = true,
        _ => {}
      }
      intravisit::walk_expr(self, e);
    }
  }
  let mut f = Find(false);
  intravisit::walk_expr(&mut f, e);
  f.0
}

impl<'a, 'tcx> ExprVisitor<'a, 'tcx> {
  /// Whether a branch (`body`, with its `events` and the variables
  /// `decls` it declares) has type `!` and so never reaches the merge
  /// after its conditional (#296), and if so how it leaves. A `break`
  /// out of a loop after a move is recorded for the loop, which then
  /// marks the moved variables as (maybe) moved where it ends (#344).
  fn branch_divergence(
    &mut self,
    body: BranchBody<'tcx>,
    events: &[(usize, ExternalEvent)],
    decls: &HashSet<ResourceAccessPoint>,
  ) -> (bool, Vec<BranchExit>, Vec<String>) {
    // (A let-else's `else` block always diverges: rustc requires it.)
    if matches!(body, BranchBody::Expr(e) if !self.expr_diverges(e)) {
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
          on_path: false,
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
    if let Some((_, block)) = self.break_assigns.iter().rev().find(|(id, _)| *id == target).copied() {
      // Assigned once the `break` has left the loop (and those inside
      // it), which don't come round again to assign them twice: no
      // "a later iteration drops what an earlier one assigned" (#1396).
      let inner = self.loop_entry_uninit.iter().rposition(|(id, _)| *id == target)
        .map(|at| self.loop_entry_uninit.split_off(at));
      self.assign_value(block, value);
      self.loop_entry_uninit.extend(inner.unwrap_or_default());
    } else if let Some((_, pat, scope, inside)) = self.break_pats.iter().rev().find(|(id, ..)| *id == target).cloned() {
      // The bindings live in the `let`'s scope, not the loop's.
      let loop_scope = std::mem::replace(&mut self.current_scope, scope);
      let loop_inside = std::mem::replace(&mut self.inside_branch, inside);
      self.bind_pat(pat, value, false);
      self.current_scope = loop_scope;
      self.inside_branch = loop_inside;
    } else if let Some((_, lhs, evt)) = self.break_binds.iter().rev().find(|(id, ..)| *id == target).cloned() {
      self.inherit_break_lenders(&lhs, target);
      // A value that holds a borrow (`break Some(&u)`, #1435) holds what
      // this `break`'s value borrows.
      if let ResourceTy::Value(r) = &lhs {
        if !r.is_ref() {
          self.add_arm_holder_lenders(r.name(), value);
        }
      }
      self.match_rhs(lhs.clone(), value, evt);
      self.emit_arm_holder_literal_borrows(&lhs, value);
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
    let rustc_hir::Node::Expr(loop_expr) = self.tcx.hir_node(target) else { return 0 };
    breaks_to(loop_expr).iter().filter(|(_, value)| value.is_some()).count()
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
    match loop_expr.kind {
      ExprKind::Loop(body, ..) if !self.block_diverges(body) => {}
      // A labeled block's local (or a field of one) carried out by a
      // `break 'blk`, when the block can also end another way (#1288):
      // that way drops it where the block ends.
      ExprKind::Block(_, Some(_)) => {}
      _ => return Vec::new(),
    }
    let mut names: Vec<String> = Vec::new();
    for value in values {
      let parts: Vec<&Expr> = match value.kind {
        ExprKind::Tup(es) => es.iter().collect(),
        _ => vec![*value],
      };
      for part in parts {
        // A field of one (`break l.name`, `break o.inner.first`) too:
        // only that part leaves with the `break` (#1280).
        let mut place: &Expr = part;
        let mut fields: Vec<String> = Vec::new();
        while let ExprKind::Field(inner, ident) = place.kind {
          fields.push(ident.as_str().to_owned());
          place = inner;
        }
        if let Some(l) = local_of_path(place) {
          if loop_expr.span.contains(self.tcx.hir_span(l)) {
            let mut name = binding_name(&self.tcx, l);
            for f in fields.iter().rev() {
              name.push('.');
              name.push_str(f);
            }
            names.push(name);
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
    let first = local_of_path(values[0].0);
    let one_move = first.is_some() && values.iter().all(|(v, _)| local_of_path(v) == first);
    // (A labeled block all of whose ends move the same local moves it
    // on every path: nothing is left to drop where it ends.)
    let names = if one_move && matches!(loop_expr.kind, ExprKind::Block(..)) { Vec::new() } else { self.break_out_names(loop_expr, &all) };
    let saved_names = std::mem::replace(&mut self.break_out, names);
    if one_move {
      // `if c { break a; } else { break a; }`: a moves to the receiver
      // whichever ran, so that is one plain move.
      pair(self, values[0].0);
    } else {
      for (value, brk) in &values {
        // A field of a local (`break 'blk g.name`) goes with the local
        // (#1288).
        let mut root: &Expr = value;
        while let ExprKind::Field(inner, _) = root.kind {
          root = inner;
        }
        let in_loop = local_of_path(root)
          .is_some_and(|l| loop_expr.span.contains(self.tcx.hir_span(l)));
        // A variable every other way out moves as well (`'a: { if c {
        // break 'a s; } id(s) }`): moved whichever way ran, so this way
        // only says where it went, as for one declared inside (#1504).
        let in_loop = in_loop || local_of_path(root)
          .is_some_and(|l| self.moved_on_other_ways_out(loop_expr, value, &binding_name(&self.tcx, l)));
        // A labeled block's value comes from a `break` or its last
        // expression (#1134).
        let end = match loop_expr.kind {
          ExprKind::Block(b, Some(_)) if b.expr.is_some_and(|t| t.hir_id == value.hir_id) => AltEnd::BlockTail,
          ExprKind::Block(_, Some(_)) => AltEnd::Block,
          _ => AltEnd::Loop,
        };
        self.break_alt = Some(BreakAlt { brk: brk.clone(), in_loop, end });
        pair(self, value);
        self.break_alt = None;
      }
    }
    self.line_override = line;
    self.break_out = saved_names;
  }

  /// Whether every way out of `loop_expr` (a `loop`, or a labeled
  /// block) but the one `value` leaves by (a `break`'s value, or the
  /// block's last expression) moves `name` (`way_out_moves`). A `while`
  /// or `for` loop also ends by its condition or iterator: no.
  fn moved_on_other_ways_out(&self, loop_expr: &'tcx Expr<'tcx>, value: &'tcx Expr<'tcx>, name: &str) -> bool {
    let target = match loop_expr.kind {
      ExprKind::Loop(_, _, rustc_hir::LoopSource::Loop, _) => loop_expr.hir_id,
      ExprKind::Block(b, Some(_)) => b.hir_id,
      _ => return false,
    };
    let mine = match self.tcx.parent_hir_node(value.hir_id) {
      rustc_hir::Node::Expr(e) if matches!(e.kind, ExprKind::Break(..)) => Some(e.hir_id),
      _ => None,
    };
    let moves = |way: Option<HirId>| self.way_out_moves.iter()
      .any(|(t, brk, moved)| *t == target && *brk == way && moved.contains(name));
    // Each other `break` (one never seen, unrecorded, says no), and a
    // labeled block's end if it is reached (recorded then).
    let others: Vec<HirId> = breaks_to_id(loop_expr, target).into_iter()
      .map(|(b, _)| b.hir_id).filter(|b| Some(*b) != mine).collect();
    let end = mine.is_some() && self.way_out_moves.iter().any(|(t, brk, _)| *t == target && brk.is_none());
    (!others.is_empty() || end) && others.iter().all(|b| moves(Some(*b))) && (!end || moves(None))
  }

  /// `brk` (a `break` expression) as written, for a tooltip: `None`
  /// when it is too long or spans lines.
  pub(crate) fn break_text(&self, brk: &Expr) -> Option<String> {
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
  /// may target instead of the block expression's). It also ends by
  /// reaching its end, through the events `through` (`None` when its
  /// end is never reached): there a variable moved on the way is moved
  /// too, so a block all of whose ways out move it moved it.
  fn emit_loop_exit_moves(&mut self, loop_expr: &'tcx Expr<'tcx>, block: Option<HirId>, before: &HashSet<String>, ends_otherwise: bool,
    through: Option<&[(usize, ExternalEvent)]>) -> Vec<ResourceAccessPoint> {
    let (mine, rest): (Vec<BreakMove>, Vec<BreakMove>) =
      std::mem::take(&mut self.break_moves).into_iter()
        .partition(|m| m.target == loop_expr.hir_id || Some(m.target) == block);
    self.break_moves = rest;
    let mut emitted = Vec::new();
    if mine.is_empty() {
      return emitted;
    }
    let breaks: Vec<HirId> = match block {
      Some(id) => breaks_to_id(loop_expr, id),
      None => breaks_to(loop_expr),
    }.into_iter().map(|(b, _)| b.hir_id).collect();
    let line = span_end_line(&loop_expr.span, &self.tcx);
    let mut done: Vec<&ResourceAccessPoint> = Vec::new();
    let mut rounds: Option<Vec<(String, Ownership, bool)>> = None;
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
      // A labeled block also ends at its end, which moves it if the
      // way there does (#1286).
      let ends_otherwise = match block {
        Some(_) => through.is_some_and(|ev| path_own(ev, &m.rap, &[], PathOwn::Yes) != PathOwn::No),
        None => ends_otherwise,
      };
      // (Or whose value moves it: `break 'a s`, #1504.)
      let target = block.unwrap_or(loop_expr.hir_id);
      let carries = |b: &HirId| !definite_breaks.contains(b)
        && self.way_out_moves.iter().any(|(t, brk, moved)| *t == target && *brk == Some(*b) && moved.contains(m.rap.name()));
      let carried = breaks.iter().any(carries);
      let definite = !ends_otherwise && breaks.iter().all(|b| definite_breaks.contains(b) || carries(b));
      // Only seen on the path to a `break`, not in the branch that
      // breaks: the move may be in no branch (#980).
      let nothing_above = mine.iter().all(|o| o.rap != m.rap || o.on_path);
      // Other time rounds come back round with it holding a value
      // (#1029): rustc says it only may hold one at the loop's back
      // edges.
      let kept = !definite && block.is_none() && {
        let rounds = rounds.get_or_insert_with(|| self.round_states_loop(loop_expr.span));
        rounds.iter().any(|(n, own, _)| n == m.rap.name() && *own == Ownership::Maybe)
      };
      self.add_external_event(line, ExternalEvent::LoopExitMove {
        ro: m.rap.clone(), definite,
        at: if block.is_some() { MaybeMovedAt::LabeledBlock { skipped: false, carried } } else { MaybeMovedAt::Loop { nothing_above, kept, carried } },
        id: *self.unique_id,
      });
      *self.unique_id += 1;
      emitted.push(m.rap.clone());
    }
    emitted
  }
}

/// The variables `events` move (in any branch), in order.
fn moved_from(events: &[(usize, ExternalEvent)], out: &mut Vec<ResourceAccessPoint>) {
  for (_, e) in events {
    let rap = match e {
      ExternalEvent::Move { from: ResourceTy::Value(r), .. } => r,
      ExternalEvent::OrAltMove { ro, tuple: None, .. } | ExternalEvent::LoopExitMove { ro, .. } => ro,
      // An `FnOnce` closure its call consumes (#1526).
      ExternalEvent::ClosureCall { closure: ResourceTy::Value(r), .. } => r,
      ExternalEvent::Branch { branches, .. } => {
        for b in branches {
          moved_from(&b.e_data, out);
        }
        continue;
      }
      _ => continue,
    };
    if !rap.is_fn() && !out.contains(rap) {
      out.push(rap.clone());
    }
  }
}

/// The variables moved on every path through `events` (minus the
/// event ids in `hide`) (#1504).
fn moved_on_path(events: &[(usize, ExternalEvent)], hide: &[std::ops::Range<usize>]) -> BTreeSet<String> {
  let mut candidates = Vec::new();
  moved_from(events, &mut candidates);
  candidates.into_iter()
    .filter(|r| path_own(events, r, hide, PathOwn::Yes) == PathOwn::No)
    .map(|r| r.name().to_owned())
    .collect()
}

/// What a variable holds after a stretch of a path (see
/// `ExprVisitor::loop_path_events`), from what it held before it.
#[derive(Clone, Copy, PartialEq, Debug)]
enum PathOwn { Yes, No, Maybe }

/// `PathOwn` of `rap` after `events` (minus the event ids in `hide`),
/// starting from `own`.
fn path_own(events: &[(usize, ExternalEvent)], rap: &ResourceAccessPoint, hide: &[std::ops::Range<usize>], mut own: PathOwn) -> PathOwn {
  let is = |r: &ResourceTy| matches!(r, ResourceTy::Value(v) if v == rap);
  for (_, e) in events {
    if hide.iter().any(|ids| ids.contains(&e.get_id())) {
      continue;
    }
    own = match e {
      ExternalEvent::Move { to, .. } | ExternalEvent::Copy { to, .. } | ExternalEvent::Bind { to, .. }
        if is(to) => PathOwn::Yes,
      ExternalEvent::Move { from, .. } if is(from) => PathOwn::No,
      ExternalEvent::ClosureCall { closure, .. } if is(closure) => PathOwn::No,
      ExternalEvent::OrAltMove { ro, tuple: None, .. } if ro == rap && own == PathOwn::Yes => PathOwn::Maybe,
      // A labeled block that may end through a `break` before the
      // move on the way to its end (#1286).
      ExternalEvent::LoopExitMove { ro, at: MaybeMovedAt::LabeledBlock { skipped: true, .. }, .. }
        if ro == rap && own == PathOwn::No => PathOwn::Maybe,
      // An inner loop's `break` after a move, or a let-chain's
      // condition that moved it before failing.
      ExternalEvent::LoopExitMove { ro, definite: true, .. } if ro == rap => PathOwn::No,
      ExternalEvent::LoopExitMove { ro, .. } if ro == rap && own == PathOwn::Yes => PathOwn::Maybe,
      // An inner loop that owns it at every `break` (#1000), or that
      // may have ended before giving it a value (#786, #1001).
      ExternalEvent::LoopExitKept { ro, .. } if ro == rap => PathOwn::Yes,
      ExternalEvent::LoopMaybeInit { ro, .. } if ro == rap && own == PathOwn::Yes => PathOwn::Maybe,
      // A branch that diverges doesn't reach this point.
      ExternalEvent::Branch { branches, .. } => {
        let mut ends = branches.iter().filter(|b| !b.diverges).map(|b| path_own(&b.e_data, rap, &[], own));
        match ends.next() {
          None => own,
          Some(first) => if ends.all(|o| o == first) { first } else { PathOwn::Maybe },
        }
      }
      _ => own,
    };
  }
  own
}

/// A constructor applied to literals only (`Some(5)`, `Ok(1)`,
/// `Some((1, "a"))`, `None`): a value of the type with nothing in it
/// that came from a variable (#768).
pub(crate) fn is_literal_value<'tcx>(e: &Expr<'tcx>, typeck: &rustc_middle::ty::TypeckResults<'tcx>) -> bool {
  match e.kind {
    ExprKind::Lit(_) => true,
    ExprKind::Unary(rustc_hir::UnOp::Neg, inner) => is_literal_value(inner, typeck),
    ExprKind::Tup(elems) => elems.iter().all(|x| is_literal_value(x, typeck)),
    ExprKind::Path(ref qpath) => matches!(
      typeck.qpath_res(qpath, e.hir_id),
      Res::Def(DefKind::Ctor(_, rustc_hir::def::CtorKind::Const), _)
    ),
    ExprKind::Call(callee, args) => {
      !e.span.from_expansion() && is_ctor_path(callee, typeck)
        && args.iter().all(|x| is_literal_value(x, typeck))
    }
    _ => false,
  }
}


/// The branch bodies of the `if` / `else` or `match` `e` whose values
/// it yields, through nested ones (`else if`), in source order.
pub(crate) fn conditional_branch_bodies<'tcx>(e: &'tcx Expr<'tcx>, out: &mut Vec<&'tcx Expr<'tcx>>) {
  let mut push = |b: &'tcx Expr<'tcx>, out: &mut Vec<&'tcx Expr<'tcx>>| match branch_value_expr(b) {
    Some(v) if !v.span.from_expansion() && matches!(v.kind, ExprKind::If(_, _, Some(_)) | ExprKind::Match(_, _, rustc_hir::MatchSource::Normal)) =>
      conditional_branch_bodies(v, out),
    _ => out.push(b),
  };
  match e.kind {
    ExprKind::If(_, then, Some(els)) => {
      push(then, out);
      push(els, out);
    }
    ExprKind::Match(_, arms, _) => for arm in arms.iter() {
      push(arm.body, out);
    },
    _ => {}
  }
}

/// The name of the call in a `for` loop's head `head` that takes as it
/// is an argument `is_var` picks (`zip` in `a.iter_mut().zip(b)`), if
/// any.
fn head_call_taking<'tcx>(head: &'tcx Expr<'tcx>, is_var: &dyn Fn(&'tcx Expr<'tcx>) -> bool, tcx: &rustc_middle::ty::TyCtxt<'tcx>) -> Option<String> {
  match head.kind {
    ExprKind::MethodCall(seg, rcvr, args, _) => {
      if args.iter().any(|a| is_var(a)) {
        return Some(seg.ident.as_str().to_owned());
      }
      args.iter().find_map(|a| head_call_taking(a, is_var, tcx)).or_else(|| head_call_taking(rcvr, is_var, tcx))
    }
    ExprKind::Call(f, args) => {
      if args.iter().any(|a| is_var(a)) {
        return hirid_to_var_name(f.hir_id, tcx);
      }
      args.iter().find_map(|a| head_call_taking(a, is_var, tcx))
    }
    _ => None,
  }
}

/// True iff an event among `events` (also in a conditional nested in
/// them) moves something out of `t` or borrows from it.
fn takes_from(events: &[(usize, ExternalEvent)], t: &ResourceAccessPoint) -> bool {
  events.iter().any(|(_, e)| match e {
    ExternalEvent::Move { from: ResourceTy::Value(f), .. }
    | ExternalEvent::StaticBorrow { from: ResourceTy::Value(f), .. }
    | ExternalEvent::MutableBorrow { from: ResourceTy::Value(f), .. } => f == t,
    ExternalEvent::Branch { branches, .. } => branches.iter().any(|b| takes_from(&b.e_data, t)),
    _ => false,
  })
}

/// The branch bodies of the conditional `cond`, whose `Branch` event is
/// `events[k]`, each with its path of (Branch event id, arm index) pairs
/// down through the nested `Branch` events of an `else if` (or of a
/// branch whose value is another conditional) (#1463). Nothing for a
/// `Branch` whose arms aren't `cond`'s one for one.
fn arm_value_paths<'tcx>(
  cond: &'tcx Expr<'tcx>,
  events: &[(usize, ExternalEvent)],
  k: usize,
  prefix: Vec<(usize, usize)>,
  out: &mut Vec<(Vec<(usize, usize)>, &'tcx Expr<'tcx>)>,
) {
  let ExternalEvent::Branch { branches, id, .. } = &events[k].1 else { return };
  let bodies: Vec<&'tcx Expr<'tcx>> = match cond.kind {
    ExprKind::If(_, then, Some(els)) => vec![then, els],
    ExprKind::Match(_, arms, rustc_hir::MatchSource::Normal) => arms.iter().map(|a| a.body).collect(),
    _ => return,
  };
  if bodies.len() != branches.len() {
    return;
  }
  for (a, body) in bodies.into_iter().enumerate() {
    let mut path = prefix.clone();
    path.push((*id, a));
    match branch_value_expr(body) {
      Some(v) if !v.span.from_expansion()
        && matches!(v.kind, ExprKind::If(_, _, Some(_)) | ExprKind::Match(_, _, rustc_hir::MatchSource::Normal)) => {
        let inner = &branches[a].e_data;
        if let Some(j) = inner.iter().rposition(|(_, e)| matches!(e, ExternalEvent::Branch { .. })) {
          arm_value_paths(v, inner, j, path, out);
        }
      }
      _ => out.push((path, body)),
    }
  }
}

/// Puts `taken` into the arm `path` (see `arm_value_paths`) leads to,
/// each after that arm's events on its line or earlier, and makes the
/// variables in `touched` that the arms on the way don't declare live
/// in each `Branch` on it (#1463).
fn nest_in_arm(
  events: &mut [(usize, ExternalEvent)],
  path: &[(usize, usize)],
  taken: Vec<(usize, ExternalEvent)>,
  touched: &HashSet<ResourceAccessPoint>,
) {
  let Some(((bid, a), rest)) = path.split_first() else { return };
  let Some((_, ExternalEvent::Branch { branches, live_vars, .. })) = events.iter_mut()
    .find(|(_, e)| matches!(e, ExternalEvent::Branch { .. }) && e.get_id() == *bid) else { return };
  let Some(arm) = branches.get_mut(*a) else { return };
  #[expect(clippy::disallowed_methods, reason = "collected into a set")]
  let outside: HashSet<ResourceAccessPoint> = touched.iter().filter(|v| !arm.decl_vars.contains(*v)).cloned().collect();
  #[expect(clippy::disallowed_methods, reason = "added to a set")]
  live_vars.extend(outside.iter().cloned());
  if !rest.is_empty() {
    return nest_in_arm(&mut arm.e_data, rest, taken, &outside);
  }
  for ev in taken {
    let at = arm.e_data.iter().rposition(|(l, _)| *l <= ev.0).map_or(0, |i| i + 1);
    arm.e_data.insert(at, ev);
  }
  arm.line_map = create_line_map(&arm.e_data);
}

/// The function `e` calls: a path's (`fail()`, `Self::fail()`) or a
/// method's.
fn callee_def_id<'tcx>(tcx: rustc_middle::ty::TyCtxt<'tcx>, e: &'tcx Expr<'tcx>) -> Option<rustc_span::def_id::DefId> {
  let typeck = tcx.typeck(e.hir_id.owner);
  match e.kind {
    ExprKind::Call(f, _) => match f.kind {
      ExprKind::Path(ref qp) => match typeck.qpath_res(qp, f.hir_id) {
        Res::Def(DefKind::Fn | DefKind::AssocFn, d) => Some(d),
        _ => None,
      },
      _ => None,
    },
    ExprKind::MethodCall(..) => typeck.type_dependent_def_id(e.hir_id),
    _ => None,
  }
}

/// Whether `d` is one of core's / std's panic functions (what `panic!()`,
/// `unreachable!()`, `todo!()`, `unimplemented!()` and
/// `std::panic::panic_any` call), which unwind (#410).
fn std_panic_fn(tcx: rustc_middle::ty::TyCtxt<'_>, d: rustc_span::def_id::DefId) -> bool {
  if !matches!(tcx.crate_name(d.krate).as_str(), "core" | "std") || tcx.def_kind(d) != DefKind::Fn {
    return false;
  }
  let def_path = tcx.def_path(d).to_string_no_crate_verbose();
  (def_path.starts_with("::panicking::") && !def_path.contains("nounwind") && !def_path.contains("cannot_unwind"))
    || def_path.ends_with("::begin_panic")
    || def_path == "::panic::panic_any"
    || def_path == "::panic::resume_unwind"
}

/// `std::process::exit` / `abort`, which end the process without
/// running any destructor (#1172): how to name it.
fn std_exit_fn(tcx: rustc_middle::ty::TyCtxt<'_>, d: rustc_span::def_id::DefId) -> Option<&'static str> {
  if tcx.crate_name(d.krate).as_str() != "std" || tcx.def_kind(d) != DefKind::Fn {
    return None;
  }
  match tcx.def_path(d).to_string_no_crate_verbose().as_str() {
    "::process::exit" => Some("process::exit"),
    "::process::abort" => Some("process::abort"),
    _ => None,
  }
}

/// How a call to a function of the user's that returns `!` leaves
/// (`never_fn_kind`, #1553).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum NeverFn {
  /// Every path through its body panics: the call unwinds, dropping
  /// what is in scope, as a `panic!()` there would.
  Panics,
  /// It ends the process, loops forever, or its body doesn't say which
  /// (and it may panic): see `NeverReturns`.
  NoDrop(NeverReturns),
}

/// For a function of this crate whose return type is `!`
/// (`fn fail() -> ! { panic!("x") }`), how a call to it never returns,
/// from its body (#1553): it always panics (and unwinds), always ends
/// the process, loops forever doing nothing that could panic, or, when
/// its paths differ or can't be told apart (a call of something else
/// that never returns, a loop whose body could panic), unknown. None
/// for any other function.
pub(crate) fn never_fn_kind(tcx: rustc_middle::ty::TyCtxt<'_>, d: rustc_span::def_id::DefId) -> Option<NeverFn> {
  never_fn_kind_in(tcx, d, &mut Vec::new())
}

fn never_fn_kind_in(tcx: rustc_middle::ty::TyCtxt<'_>, d: rustc_span::def_id::DefId, seen: &mut Vec<rustc_span::def_id::DefId>) -> Option<NeverFn> {
  let local = d.as_local()?;
  if !matches!(tcx.def_kind(d), DefKind::Fn | DefKind::AssocFn)
    || !tcx.fn_sig(d).skip_binder().output().skip_binder().is_never() {
    return None;
  }
  let body = tcx.hir_maybe_body_owned_by(local)?;
  // (One that calls itself, directly or not, overflows the stack.)
  if seen.contains(&d) {
    return Some(NeverFn::NoDrop(NeverReturns::Unknown));
  }
  seen.push(d);
  let how = never_how(tcx, body.value, seen);
  seen.pop();
  Some(how)
}

/// How `e`, which never completes, leaves (`never_fn_kind`).
fn never_how<'tcx>(tcx: rustc_middle::ty::TyCtxt<'tcx>, e: &'tcx Expr<'tcx>, seen: &mut Vec<rustc_span::def_id::DefId>) -> NeverFn {
  let unknown = NeverFn::NoDrop(NeverReturns::Unknown);
  let typeck = tcx.typeck(e.hir_id.owner);
  let never = |x: &'tcx Expr<'tcx>| never_completes(tcx, typeck, x);
  // What runs first in a block that never completes: its first
  // statement (or `let` value) of type `!`, or its last expression.
  let first_never = |b: &'tcx Block<'tcx>| -> Option<&'tcx Expr<'tcx>> {
    b.stmts.iter().find_map(|st| match st.kind {
      StmtKind::Expr(x) | StmtKind::Semi(x) if never(x) => Some(x),
      StmtKind::Let(l) => l.init.filter(|i| never(i)),
      _ => None,
    }).or(b.expr.filter(|t| never(t)))
  };
  let both = |a: NeverFn, b: NeverFn| if a == b { a } else { unknown };
  match e.kind {
    ExprKind::Block(b, _) => first_never(b).map_or(unknown, |x| never_how(tcx, x, seen)),
    ExprKind::DropTemps(x) | ExprKind::Ret(Some(x)) => never_how(tcx, x, seen),
    ExprKind::Call(_, args) | ExprKind::MethodCall(_, _, args, _) => {
      let operands: Vec<&'tcx Expr<'tcx>> = match e.kind {
        ExprKind::Call(f, _) => std::iter::once(f).chain(args.iter()).collect(),
        ExprKind::MethodCall(_, recv, _, _) => std::iter::once(recv).chain(args.iter()).collect(),
        _ => Vec::new(),
      };
      if let Some(x) = operands.into_iter().find(|x| never(x)) {
        return never_how(tcx, x, seen);
      }
      match callee_def_id(tcx, e) {
        Some(d) if std_panic_fn(tcx, d) => NeverFn::Panics,
        Some(d) if std_exit_fn(tcx, d).is_some() => NeverFn::NoDrop(NeverReturns::Exits),
        Some(d) => never_fn_kind_in(tcx, d, seen).unwrap_or(unknown),
        None => closure_call_never_kind(tcx, e).map_or(unknown, |(k, _)| k),
      }
    }
    ExprKind::If(c, ..) | ExprKind::Match(c, ..) if never(c) => never_how(tcx, c, seen),
    ExprKind::If(_, t, Some(el)) if never(t) && never(el) => {
      let a = never_how(tcx, t, seen);
      both(a, never_how(tcx, el, seen))
    }
    ExprKind::Match(_, arms, _) if !arms.is_empty() && arms.iter().all(|a| never(a.body)) => {
      let mut how = never_how(tcx, arms[0].body, seen);
      for a in &arms[1..] {
        how = both(how, never_how(tcx, a.body, seen));
      }
      how
    }
    // A `loop` nothing breaks out of: one whose body never completes
    // runs it once; any other loops forever, unless its body could
    // panic or leave (`loop { n += 1; }` overflows), which it then may.
    ExprKind::Loop(body, ..) if never(e) => match first_never(body) {
      Some(x) => never_how(tcx, x, seen),
      None if !may_panic_or_leave(body) => NeverFn::NoDrop(NeverReturns::Spins),
      None => unknown,
    },
    _ => unknown,
  }
}

/// A call of a local closure whose body never completes (`let f = ||
/// panic!("x"); f();`, #1562): how the call leaves, from the closure's
/// body as for a `!` function's (`never_fn_kind`), and the name it's
/// called by. (Its own return type needn't be `!`: rustc may infer `()`.)
pub(crate) fn closure_call_never_kind<'tcx>(tcx: rustc_middle::ty::TyCtxt<'tcx>, call: &'tcx Expr<'tcx>) -> Option<(NeverFn, String)> {
  let ExprKind::Call(f, _) = call.kind else { return None };
  let c = local_closure_of(&tcx, f)?;
  let ExprKind::Path(QPath::Resolved(None, path)) = f.kind else { return None };
  let name = path.segments.last()?.ident.to_string();
  let body = tcx.hir_body(c.body);
  if !never_completes(tcx, tcx.typeck(body.value.hir_id.owner), body.value) {
    return None;
  }
  Some((never_how(tcx, body.value, &mut Vec::new()), name))
}

/// Whether `e` never completes: its type is `!`, or it is an `if` / `match`
/// all of whose branches don't, a block with a statement or value that
/// doesn't, or a `loop` with no `break` out of it and no `return` (#1562).
/// Before edition 2024 rustc types such an expression in a closure's body
/// `()` (a diverging type's fallback), so the type alone misses
/// `|k| if k > 3 { panic!() } else { loop {} }`.
fn never_completes<'tcx>(tcx: rustc_middle::ty::TyCtxt<'tcx>, typeck: &rustc_middle::ty::TypeckResults<'tcx>, e: &'tcx Expr<'tcx>) -> bool {
  if typeck.expr_ty_opt(e).is_some_and(|t| t.is_never()) {
    return true;
  }
  match e.kind {
    ExprKind::DropTemps(x) => never_completes(tcx, typeck, x),
    ExprKind::If(c, t, Some(el)) => never_completes(tcx, typeck, c)
      || (never_completes(tcx, typeck, t) && never_completes(tcx, typeck, el)),
    ExprKind::Match(x, arms, _) => never_completes(tcx, typeck, x)
      || (!arms.is_empty() && arms.iter().all(|a| never_completes(tcx, typeck, a.body))),
    ExprKind::Block(b, None) => b.expr.is_some_and(|t| never_completes(tcx, typeck, t))
      || b.stmts.iter().any(|st| match st.kind {
        StmtKind::Expr(x) | StmtKind::Semi(x) => never_completes(tcx, typeck, x),
        StmtKind::Let(l) => l.init.is_some_and(|i| never_completes(tcx, typeck, i)),
        _ => false,
      }),
    ExprKind::Loop(..) => {
      struct Exits { id: HirId, found: bool }
      impl<'tcx> Visitor<'tcx> for Exits {
        fn visit_expr(&mut self, x: &'tcx Expr<'tcx>) {
          match x.kind {
            ExprKind::Closure(..) => return,
            ExprKind::Ret(_) => self.found = true,
            ExprKind::Break(dest, _) if dest.target_id == Ok(self.id) => self.found = true,
            _ => {}
          }
          intravisit::walk_expr(self, x);
        }
      }
      let mut v = Exits { id: e.hir_id, found: false };
      intravisit::walk_expr(&mut v, e);
      !v.found
    }
    _ => false,
  }
}

/// Whether running `b` could panic or leave it: a call, an index, an
/// arithmetic operation (which overflows in a debug build), a `return`,
/// `break` or `continue` to somewhere else, a macro. (Closures' bodies
/// don't run there.)
fn may_panic_or_leave<'tcx>(b: &'tcx Block<'tcx>) -> bool {
  struct V { found: bool }
  impl<'tcx> Visitor<'tcx> for V {
    fn visit_expr(&mut self, e: &'tcx Expr<'tcx>) {
      use rustc_hir::BinOpKind::*;
      if e.span.from_expansion() {
        self.found = true;
        return;
      }
      match e.kind {
        ExprKind::Closure(..) => return,
        ExprKind::Call(..) | ExprKind::MethodCall(..) | ExprKind::Index(..) | ExprKind::Ret(_)
        | ExprKind::Break(..) | ExprKind::Continue(_) | ExprKind::Unary(UnOp::Neg, _) => self.found = true,
        ExprKind::Binary(op, ..) if matches!(op.node, Add | Sub | Mul | Div | Rem | Shl | Shr) => self.found = true,
        ExprKind::AssignOp(op, ..) if matches!(rustc_hir::BinOpKind::from(op.node), Add | Sub | Mul | Div | Rem | Shl | Shr) => self.found = true,
        _ => {}
      }
      intravisit::walk_expr(self, e);
    }
  }
  let mut v = V { found: false };
  v.visit_block(b);
  v.found
}
