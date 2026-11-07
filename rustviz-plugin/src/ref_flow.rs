//! Which variables does a reference returned by a call borrow from?
//!
//! `let w = first_word(&s);` makes `w` a borrow of `s`: `s` stays
//! borrowed until `w`'s last use, not just until `first_word` returns.
//! rustc decides this from the callee's signature alone. After lifetime
//! elision has been filled in (which rustc has already done for us in
//! `tcx.fn_sig`), `fn first_word(s: &String) -> &str` is
//! `for<'a> fn(&'a String) -> &'a str`: the result carries `'a`, so it
//! borrows from whatever the argument of type `&'a String` borrows.
//! `fn longest<'a>(x: &'a str, y: &'a str) -> &'a str` ties the result
//! to *both* arguments, so a call borrows from both (#171).
//!
//! This module reads that information from the signature (never from
//! the callee's body, exactly like the borrow checker) and maps it back
//! to the caller's variables. For each argument `a` of type `D` and the
//! result type `O`:
//!
//! * if `D = &'r X` and `'r` occurs in `O`, the result may point into
//!   the place `a` refers to ("place flow"): `&s` → `s`, a reference
//!   variable `r` → whatever `r` borrows;
//! * if a lifetime or a reference-carrying type parameter that occurs
//!   in `O` also occurs inside `D` (below the outer `&`, or anywhere in
//!   a by-value `D`), the result may carry references that `a` already
//!   holds ("content flow"): `opt.unwrap()` with `opt = v.get(0)`
//!   carries `v.get(0)`'s borrow of `v`.
//!
//! A *local* non-reference value that holds a borrow (`let it =
//! v.iter();`, `let third = v.get(2);`) is recorded in `borrow_map`
//! like a reference (see `holder_lenders`; #336), so content flow out
//! of it (`it.next().unwrap()`) reaches `v`. Any other non-reference
//! local yields no lender: an honest "unknown" rather than a guess.

use rustc_hir::{Expr, ExprKind, QPath, UnOp};
use rustc_middle::ty::{self, GenericArgKind, GenericArgsRef, Ty};
use rustc_middle::ty::adjustment::{Adjust, DerefAdjustKind};
use rustc_span::def_id::DefId;
use crate::expr_visitor::*;
use crate::expr_visitor_utils::*;
use crate::svg_generator::data::*;

/// How an argument reaches a call's result (see the module docs).
struct ArgFlow<'tcx> {
  index: usize,
  arg: &'tcx Expr<'tcx>,
  place: bool,
  content: bool,
  /// The place flow goes through a `&mut` parameter, so the loan the
  /// result keeps is a mutable one (`v.iter_mut()`, `v.get_mut(0)`).
  place_mut: bool,
  /// The parameter isn't a reference type (`x: T`, `self: Self`): the
  /// argument's whole value is carried, so if it is a reference
  /// (`id(&s)`, `Some(&s)`, `refs.push(&s)` with `T = &String`), the
  /// result carries that reference's borrow (#424).
  by_value: bool,
}

/// Which type a call's arguments may flow into.
#[derive(Clone, Copy)]
enum FlowTarget<'s> {
  /// The call's result.
  Result,
  /// One field of the struct the call returns.
  Field(&'s str),
  /// The receiver of a `&mut self` method (`refs.push(&s)`).
  Receiver,
}

/// What an adjusted argument refers to, after rustc's auto-deref steps.
enum Referent<'tcx> {
  /// The place denoted by this expression (`s` in `s.as_str()`).
  Place(&'tcx Expr<'tcx>),
  /// `*e`, where `e` is a reference value (`r` in `r.as_str()`).
  Through(&'tcx Expr<'tcx>),
  Unknown,
}

impl<'a, 'tcx> ExprVisitor<'a, 'tcx> {
  /// The callee's signature with late-bound lifetimes made comparable,
  /// the call's generic arguments (if known), and the arguments in
  /// parameter order (the receiver first for a method call). `None`
  /// for calls through a closure value, whose signature isn't a fn
  /// item's.
  fn callee_sig(
    &self,
    call: &'tcx Expr<'tcx>,
  ) -> Option<(ty::FnSig<'tcx>, Option<GenericArgsRef<'tcx>>, Vec<&'tcx Expr<'tcx>>, Option<DefId>)> {
    let typeck = self.tcx.typeck(call.hir_id.owner);
    match call.kind {
      ExprKind::Call(fn_expr, args) => {
        let fty = typeck.node_type_opt(fn_expr.hir_id)?;
        match fty.kind() {
          ty::FnDef(def_id, gargs) => {
            let sig = self.tcx.fn_sig(*def_id).instantiate_identity().skip_norm_wip();
            Some((self.tcx.liberate_late_bound_regions(*def_id, sig), Some(*gargs), args.iter().collect(), Some(*def_id)))
          }
          ty::FnPtr(..) => {
            let sig = fty.fn_sig(self.tcx);
            let scope = call.hir_id.owner.to_def_id();
            Some((self.tcx.liberate_late_bound_regions(scope, sig), None, args.iter().collect(), None))
          }
          _ => None,
        }
      }
      ExprKind::MethodCall(_, rcvr, args, _) => {
        let def_id = typeck.type_dependent_def_id(call.hir_id)?;
        let sig = self.tcx.fn_sig(def_id).instantiate_identity().skip_norm_wip();
        let mut all = vec![rcvr];
        all.extend(args.iter());
        Some((self.tcx.liberate_late_bound_regions(def_id, sig), typeck.node_args_opt(call.hir_id), all, Some(def_id)))
      }
      _ => None,
    }
  }

  /// For each argument of `call`, whether the call's result can borrow
  /// what that argument refers to or holds.
  fn call_arg_flows(&self, call: &'tcx Expr<'tcx>) -> Option<Vec<ArgFlow<'tcx>>> {
    self.call_arg_flows_into(call, FlowTarget::Result)
  }

  /// `call_arg_flows`, for the whole result (`field = None`) or for
  /// one field of a struct result (`Excerpt::new(&novel)` returns an
  /// `Excerpt<'a>` whose `part: &'a str` borrows `novel`).
  fn call_arg_flows_into(&self, call: &'tcx Expr<'tcx>, target: FlowTarget<'_>) -> Option<Vec<ArgFlow<'tcx>>> {
    let (sig, gargs, args, def_id) = self.callee_sig(call)?;
    let output = match target {
      FlowTarget::Result => sig.output(),
      FlowTarget::Field(f) => match sig.output().kind() {
        ty::Adt(def, fargs) if def.is_struct() =>
          def.all_fields().find(|fd| fd.name.as_str() == f)?.ty(self.tcx, fargs),
        _ => return None,
      },
      // What a `&mut self` method can store into its receiver: the
      // receiver's own type (`Vec<T>` in `push(&mut self, value: T)`).
      FlowTarget::Receiver => match sig.inputs().first()?.kind() {
        ty::Ref(_, inner, m) if m.is_mut() => *inner,
        _ => return Some(Vec::new()),
      },
    };
    // A type parameter matters only if this call instantiates it with
    // something that holds a reference (`T = &i32` in `unwrap`).
    let carries_ref = |index: u32| -> bool {
      match gargs.and_then(|g| g.get(index as usize).copied()) {
        Some(arg) => arg.walk().any(|g| matches!(g.kind(), GenericArgKind::Lifetime(_))),
        None => true,
      }
    };
    let mut regions: Vec<ty::Region<'tcx>> = Vec::new();
    let mut params: Vec<u32> = Vec::new();
    for g in output.walk() {
      match g.kind() {
        GenericArgKind::Lifetime(r) if !r.is_static() => regions.push(r),
        GenericArgKind::Type(t) => {
          if let ty::Param(p) = t.kind() {
            if carries_ref(p.index) {
              params.push(p.index);
            }
          }
        }
        _ => {}
      }
    }
    // A result type parameter bounded by a trait that mentions other
    // parameters can be built from values of those: `collect<B:
    // FromIterator<Self::Item>>(self) -> B` fills `B` with the
    // iterator's items, so `B` carries whatever `Self` carries (#392).
    // Only for the result: a bound on an argument (`retain<F:
    // FnMut(&T) -> bool>(&mut self, f: F)`) doesn't store it.
    if matches!(target, FlowTarget::Result) && !params.is_empty() {
      if let Some(def_id) = def_id {
        let bound: Vec<u32> = params.clone();
        for (clause, _) in self.tcx.predicates_of(def_id).predicates.iter() {
          let Some(pred) = clause.as_trait_clause() else { continue };
          let pred = pred.skip_binder();
          if !matches!(pred.self_ty().kind(), ty::Param(p) if bound.contains(&p.index)) {
            continue;
          }
          for arg in pred.trait_ref.args.iter().skip(1) {
            for g in arg.walk() {
              match g.kind() {
                GenericArgKind::Lifetime(r) if !r.is_static() && !regions.contains(&r) => regions.push(r),
                GenericArgKind::Type(t) => {
                  if let ty::Param(p) = t.kind() {
                    if carries_ref(p.index) && !params.contains(&p.index) {
                      params.push(p.index);
                    }
                  }
                }
                _ => {}
              }
            }
          }
        }
      }
    }
    if regions.is_empty() && params.is_empty() {
      return Some(Vec::new());
    }
    let shares = |t: Ty<'tcx>| -> bool {
      t.walk().any(|g| match g.kind() {
        GenericArgKind::Lifetime(r) => regions.contains(&r),
        GenericArgKind::Type(t) => matches!(t.kind(), ty::Param(p) if params.contains(&p.index)),
        GenericArgKind::Const(_) => false,
      })
    };
    let mut flows = Vec::new();
    // The receiver doesn't flow into itself.
    let skip = usize::from(matches!(target, FlowTarget::Receiver));
    for (index, (input, arg)) in sig.inputs().iter().zip(args).enumerate().skip(skip) {
      let (place, content, place_mut) = match input.kind() {
        ty::Ref(r, inner, m) => (regions.contains(r), shares(*inner), m.is_mut()),
        _ => (false, shares(*input), false),
      };
      if place || content {
        let by_value = !matches!(input.kind(), ty::Ref(..));
        flows.push(ArgFlow { index, arg, place, content, place_mut: place && place_mut, by_value });
      }
    }
    Some(flows)
  }

  /// The variables a reference returned by `call` borrows from, in
  /// argument order, without duplicates. Only lenders that have a
  /// column (`Value` / `Deref`) are returned; empty when the result
  /// borrows nothing we can name (a literal, a temporary, a value we
  /// don't track) or the callee's signature isn't available.
  pub fn call_lenders(&self, call: &'tcx Expr<'tcx>) -> Vec<ResourceTy> {
    self.call_lenders_full(call).0
  }

  /// `call_lenders`, plus whether some argument tied to the result
  /// refers to data without a column (a string literal, a reference
  /// parameter's caller-side data, a value we don't track): the
  /// result may point there instead of into the returned lenders.
  pub fn call_lenders_full(&self, call: &'tcx Expr<'tcx>) -> (Vec<ResourceTy>, bool) {
    self.lenders_of_flows(self.call_arg_flows(call).unwrap_or_default())
  }

  /// `call_lenders_full` for the reference stored in field `field` of
  /// the struct `call` returns, and whether that borrow is mutable.
  pub fn call_field_lenders(&self, call: &'tcx Expr<'tcx>, field: &str) -> (Vec<ResourceTy>, bool, bool) {
    let flows = self.call_arg_flows_into(call, FlowTarget::Field(field)).unwrap_or_default();
    let is_mut = flows.iter().any(|f| f.place_mut || (f.content && self.value_borrow_is_mut(f.arg)));
    let (ls, unnamed) = self.lenders_of_flows(flows);
    (ls, unnamed, is_mut)
  }

  /// Whether `call` is a `&mut self` method whose signature lets it
  /// store a borrow carried by another argument into the receiver:
  /// `push(&mut self, value: T)` on a `Vec<&String>` (#392). Decided
  /// from types alone.
  pub fn call_stores_into_receiver(&self, call: &'tcx Expr<'tcx>) -> bool {
    matches!(call.kind, ExprKind::MethodCall(..))
      && (self.call_arg_flows_into(call, FlowTarget::Receiver).is_some_and(|f| !f.is_empty())
        || !self.receiver_kept_loans(call).is_empty())
  }

  /// The variables whose borrows `call` stores into its receiver
  /// (`refs.push(&s)` → `s`), and whether any of them is mutable.
  pub fn receiver_store_lenders(&self, call: &'tcx Expr<'tcx>) -> (Vec<ResourceTy>, bool) {
    let flows = self.call_arg_flows_into(call, FlowTarget::Receiver).unwrap_or_default();
    let is_mut = flows.iter().any(|f| f.place_mut || (f.content && self.value_borrow_is_mut(f.arg)));
    let lenders = self.lenders_of_flows(flows).0;
    if !lenders.is_empty() {
      return (lenders, is_mut);
    }
    // The signature ties the stored value to the receiver only through
    // a trait bound (`extend<I: IntoIterator<Item = T>>(&mut self, iter:
    // I)`): ask rustc which loans the receiver holds once the call has
    // returned (#682).
    let mut out: Vec<ResourceTy> = Vec::new();
    let mut is_mut = false;
    for k in self.receiver_kept_loans(call) {
      let Some(rd) = self.raps.get(&k.lender) else { continue };
      // A reborrow through a reference variable (`[&*r]`) isn't drawn.
      if rd.rap.is_fn() || rd.rap.is_ref() || k.through_ref {
        continue;
      }
      let l = ResourceTy::Value(rd.rap.to_owned());
      if !out.contains(&l) {
        out.push(l);
      }
      is_mut |= k.is_mut;
    }
    (out, is_mut)
  }

  /// The loans a `&mut self` method call leaves in its receiver
  /// variable (see `loans_kept_after_call`). Only asked when the
  /// receiver's type can hold a borrow and the method takes `&mut
  /// self`, so the (costly) Polonius run is rare.
  fn receiver_kept_loans(&self, call: &'tcx Expr<'tcx>) -> Vec<crate::mir_data::KeptLoan> {
    let ExprKind::MethodCall(_, rcvr, _, _) = call.kind else { return Vec::new() };
    if !matches!(rcvr.kind, ExprKind::Path(QPath::Resolved(None, _))) {
      return Vec::new();
    }
    let Some((sig, ..)) = self.callee_sig(call) else { return Vec::new() };
    if !matches!(sig.inputs().first().map(|t| t.kind()), Some(ty::Ref(_, _, m)) if m.is_mut()) {
      return Vec::new();
    }
    let typeck = self.tcx.typeck(call.hir_id.owner);
    if !typeck.expr_ty(rcvr).walk().any(|g| matches!(g.kind(), GenericArgKind::Lifetime(_))) {
      return Vec::new();
    }
    let Some(name) = expr_to_rap_name(rcvr, &self.tcx) else { return Vec::new() };
    self.loans_kept_after_call(call.span).into_iter().filter(|k| k.holder == name).collect()
  }

  fn lenders_of_flows(&self, flows: Vec<ArgFlow<'tcx>>) -> (Vec<ResourceTy>, bool) {
    let mut out: Vec<ResourceTy> = Vec::new();
    let mut unnamed = false;
    for flow in flows {
      let ls = self.arg_lenders(flow.arg, flow.place, flow.content, flow.by_value);
      // A closure passed by value (`.map(|p| p.len())`) holds only
      // what it captures by reference; one that captures nothing that
      // way holds no borrow at all.
      if ls.is_empty() && !matches!(flow.arg.kind, ExprKind::Closure(..)) {
        unnamed = true;
      }
      for l in ls {
        // A function isn't a lender: a value it returned that we don't
        // track (`for word in text.split_whitespace()`) borrows data
        // with no column.
        if !matches!(&l, ResourceTy::Value(r) | ResourceTy::Deref(r) if !r.is_fn()) {
          unnamed = true;
        } else if !out.contains(&l) {
          out.push(l);
        }
      }
    }
    (out, unnamed)
  }

  /// Which of `call`'s arguments its signature ties the result to,
  /// and how many of those have no column (see `ArgTie`).
  pub fn call_arg_tie(&self, call: &'tcx Expr<'tcx>, field: Option<&str>) -> ArgTie {
    let Some((_, _, args, _)) = self.callee_sig(call) else { return ArgTie::default() };
    let mut tie = ArgTie {
      receiver: matches!(call.kind, ExprKind::MethodCall(..)),
      total: args.len(),
      ..ArgTie::default()
    };
    let target = match field { Some(f) => FlowTarget::Field(f), None => FlowTarget::Result };
    for flow in self.call_arg_flows_into(call, target).unwrap_or_default() {
      let named = self.arg_lenders(flow.arg, flow.place, flow.content, flow.by_value).iter()
        .any(|l| matches!(l, ResourceTy::Value(_) | ResourceTy::Deref(_)));
      tie.tied.push(flow.index);
      // A closure that captures nothing by reference holds no borrow:
      // it isn't "data with no column" (see `lenders_of_flows`).
      if !named && !matches!(flow.arg.kind, ExprKind::Closure(..)) {
        tie.unnamed += 1;
      }
    }
    tie
  }

  /// Whether the borrow `call`'s result keeps is a mutable one: some
  /// argument tied to the result is passed as `&mut` (`v.iter_mut()`),
  /// or the result carries a mutable borrow an argument already held
  /// (`it.next()` with `it = v.iter_mut()`). rustc keeps that `&mut`
  /// loan alive for as long as the result is used, even when the
  /// result itself only hands out shared references.
  pub fn call_borrow_is_mut(&self, call: &'tcx Expr<'tcx>) -> bool {
    self.call_arg_flows(call).unwrap_or_default().iter().any(|flow| {
      flow.place_mut || (flow.content && self.value_borrow_is_mut(flow.arg))
    })
  }

  /// Whether the borrow held by the value of `e` is a mutable one.
  fn value_borrow_is_mut(&self, e: &'tcx Expr<'tcx>) -> bool {
    match e.kind {
      ExprKind::Path(QPath::Resolved(..)) | ExprKind::Field(..) => {
        expr_to_rap_name(e, &self.tcx)
          .and_then(|n| self.borrow_map.get(&n))
          .is_some_and(|rd| rd.ref_mutability)
      }
      ExprKind::AddrOf(_, m, _) => m.is_mut(),
      ExprKind::Call(..) | ExprKind::MethodCall(..) => self.call_borrow_is_mut(e),
      ExprKind::Closure(closure) => self.closure_ref_captures(closure.def_id).iter().any(|(_, m)| *m),
      ExprKind::Array(elems) => elems.iter().any(|x| self.value_borrow_is_mut(x)),
      ExprKind::Repeat(x, _) => self.value_borrow_is_mut(x),
      ExprKind::Block(b, _) => b.expr.is_some_and(|x| self.value_borrow_is_mut(x)),
      ExprKind::DropTemps(inner) => self.value_borrow_is_mut(inner),
      _ => false,
    }
  }

  /// The variables a non-reference value bound by `let h = init;`
  /// borrows from, when its type can hold a borrow at all (it
  /// mentions a lifetime: `Iter<'_, i32>`, `Option<&i32>`), and
  /// whether that borrow is mutable. Empty when `init` isn't a call
  /// or nothing it borrows has a column (#336).
  pub fn holder_lenders(&self, init: &'tcx Expr<'tcx>, ty: Ty<'tcx>) -> (Vec<ResourceTy>, bool) {
    if ty.is_ref() || !ty.walk().any(|g| matches!(g.kind(), GenericArgKind::Lifetime(_))) {
      return (Vec::new(), false);
    }
    match init.kind {
      ExprKind::Call(..) | ExprKind::MethodCall(..) => (self.call_lenders(init), self.call_borrow_is_mut(init)),
      // `let refs = [&a, &b];` (`vec![&a, &b]` is a call; #349).
      ExprKind::Array(..) | ExprKind::Repeat(..) => (self.value_lenders(init), self.value_borrow_is_mut(init)),
      // `let it2 = it;` / `let t2 = third;`: the new variable holds
      // the same borrow.
      ExprKind::Path(QPath::Resolved(..)) => match expr_to_rap_name(init, &self.tcx)
        .filter(|n| self.raps.get(n).is_some_and(|rd| rd.rap.holds_borrow()))
        .and_then(|n| self.borrow_map.get(&n))
      {
        Some(rd) => (rd.all_lenders(), rd.ref_mutability),
        None => (Vec::new(), false),
      },
      _ => (Vec::new(), false),
    }
  }

  /// `let r = refs[0];`: `r` is a copy of a reference stored in the
  /// collection, so it borrows what that reference borrows, never the
  /// collection itself (#349). `Some` when `e` is such a read: the
  /// lenders of a collection that holds a borrow we track (empty when
  /// it isn't tracked, e.g. a `Vec` filled by `push`).
  pub fn stored_ref_lenders(&self, e: &'tcx Expr<'tcx>) -> Option<Vec<ResourceTy>> {
    let ExprKind::Index(base, _, _) = e.kind else { return None };
    let typeck = self.tcx.typeck(e.hir_id.owner);
    if !typeck.expr_ty(e).is_ref() {
      return None;
    }
    let held = expr_to_rap_name(base, &self.tcx)
      .filter(|n| self.raps.get(n).is_some_and(|rd| rd.rap.holds_borrow()))
      .and_then(|n| self.borrow_map.get(&n))
      .map(|rd| rd.all_lenders())
      .unwrap_or_default();
    Some(held)
  }

  /// Lenders reachable through one argument, after rustc's adjustments
  /// (auto-ref of a method receiver, auto-deref, deref coercion).
  fn arg_lenders(&self, arg: &'tcx Expr<'tcx>, place: bool, content: bool, by_value: bool) -> Vec<ResourceTy> {
    if by_value {
      // Passed by value: whatever the value holds, including the
      // borrow of a reference passed for a type parameter.
      return if content { self.value_lenders(arg) } else { Vec::new() };
    }
    let typeck = self.tcx.typeck(arg.hir_id.owner);
    let adjs = typeck.expr_adjustments(arg);
    let mut res = Vec::new();
    if let Some(bpos) = adjs.iter().position(|a| matches!(a.kind, Adjust::Borrow(_))) {
      // The argument is re-borrowed (auto-ref): walk the deref steps
      // before the borrow to find the place being borrowed.
      let mut referent = Referent::Place(arg);
      for a in &adjs[..bpos] {
        if let Adjust::Deref(kind) = a.kind {
          referent = match (referent, kind) {
            (Referent::Place(e), DerefAdjustKind::Builtin) => match e.kind {
              ExprKind::AddrOf(_, _, inner) => Referent::Place(inner),
              _ => Referent::Through(e),
            },
            (Referent::Through(_), DerefAdjustKind::Builtin) => Referent::Unknown,
            // An overloaded deref (`String` → `str`, `Vec<T>` → `[T]`,
            // `Box<T>` → `T`) stays inside the same owner's data.
            (r, _) => r,
          };
        }
      }
      if place {
        res.extend(match referent {
          Referent::Place(e) => self.place_lenders(e),
          Referent::Through(e) => self.value_lenders(e),
          Referent::Unknown => Vec::new(),
        });
      }
      if content {
        if let Referent::Place(e) = referent {
          res.extend(self.content_lenders(e));
        }
      }
    } else if typeck.expr_ty_adjusted(arg).is_ref() {
      // A reference passed as is (`longest(x, y)`, `f(&s)`).
      if place {
        res.extend(self.value_lenders(arg));
      }
      if content {
        if let ExprKind::AddrOf(_, _, inner) = arg.kind {
          res.extend(self.content_lenders(inner));
        }
      }
    } else if content {
      // A value passed by value whose type holds references
      // (`opt.unwrap()` with `opt: Option<&T>`).
      res.extend(self.value_lenders(arg));
    }
    res
  }

  /// What the reference(s) held by the *value* of `e` borrow from.
  pub fn value_lenders(&self, e: &'tcx Expr<'tcx>) -> Vec<ResourceTy> {
    match e.kind {
      ExprKind::Path(QPath::Resolved(..)) | ExprKind::Field(..) => {
        let name = expr_to_rap_name(e, &self.tcx);
        match name.as_ref().and_then(|n| self.borrow_map.get(n)) {
          // A reference parameter (`v: &Vec<i32>`): what it points to
          // is the caller's data, which has no column here. v's own
          // column stands for it, as it already does for `&v[0]`, so
          // `v.iter()` / `first_word(s)` borrow "from v" (#393).
          Some(rd) if matches!(rd.lender, ResourceTy::Caller) && rd.co_lenders.is_empty() => {
            match name.as_ref().and_then(|n| self.raps.get(n)) {
              Some(r) if r.rap.is_ref() => vec![ResourceTy::Value(r.rap.to_owned())],
              _ => rd.all_lenders(),
            }
          }
          Some(rd) => rd.all_lenders(),
          // A non-reference variable (`opt: Option<&T>`, an iterator)
          // whose borrow we don't track (holders are in borrow_map).
          None => Vec::new(),
        }
      }
      ExprKind::AddrOf(_, _, inner) => self.place_lenders(inner),
      ExprKind::Call(..) | ExprKind::MethodCall(..) => self.call_lenders(e),
      ExprKind::Block(b, _) => b.expr.map(|x| self.value_lenders(x)).unwrap_or_default(),
      ExprKind::DropTemps(inner) => self.value_lenders(inner),
      // An array of references (`[&a, &b]`, `[&s; 3]`, and what
      // `vec![&s]` expands to) holds every element's borrow (#349).
      ExprKind::Array(elems) => {
        let mut out: Vec<ResourceTy> = Vec::new();
        for e in elems.iter() {
          for l in self.value_lenders(e) {
            if !out.contains(&l) {
              out.push(l);
            }
          }
        }
        out
      }
      ExprKind::Repeat(e, _) => self.value_lenders(e),
      // A closure value holds a borrow of each variable it captures
      // by reference (the `match_rhs` closure arm draws the same).
      ExprKind::Closure(closure) => self.closure_ref_captures(closure.def_id).into_iter().map(|(l, _)| l).collect(),
      _ => Vec::new(),
    }
  }

  /// The variables a closure captures by reference, and whether
  /// mutably.
  fn closure_ref_captures(&self, def_id: rustc_span::def_id::LocalDefId) -> Vec<(ResourceTy, bool)> {
    let typeck = self.tcx.typeck(def_id);
    let mut out: Vec<(ResourceTy, bool)> = Vec::new();
    for capture in typeck.closure_min_captures_flattened(def_id) {
      let ty::UpvarCapture::ByRef(kind) = capture.info.capture_kind else { continue };
      let rustc_middle::hir::place::PlaceBase::Upvar(upvar_id) = capture.place.base else { continue };
      let name = binding_name(&self.tcx, upvar_id.var_path.hir_id);
      if let Some(rd) = self.raps.get(&name) {
        let l = ResourceTy::Value(rd.rap.to_owned());
        if !out.iter().any(|(x, _)| *x == l) {
          out.push((l, matches!(kind, ty::BorrowKind::Mutable)));
        }
      }
    }
    out
  }

  /// Who is borrowed when the place `p` is borrowed (`&p`).
  fn place_lenders(&self, p: &'tcx Expr<'tcx>) -> Vec<ResourceTy> {
    let typeck = self.tcx.typeck(p.hir_id.owner);
    match p.kind {
      ExprKind::Path(QPath::Resolved(..)) => match get_rap(p, &self.tcx, &self.raps) {
        r @ ResourceTy::Value(_) => vec![r],
        _ => Vec::new(),
      },
      ExprKind::Unary(UnOp::Deref, inner) => {
        if typeck.expr_ty(inner).is_ref() {
          self.value_lenders(inner)
        } else {
          // `*b` with b a `Box`: the place is inside b's own data.
          self.place_lenders(inner)
        }
      }
      ExprKind::Field(base, ident) => {
        if let Some(rap) = field_rap(base, ident.as_str(), &self.tcx, &self.raps) {
          return vec![ResourceTy::Value(rap)];
        }
        self.base_place_lenders(base)
      }
      ExprKind::Index(base, _, _) => self.base_place_lenders(base),
      // A temporary (`&String::from("x")`): nothing with a column.
      _ => Vec::new(),
    }
  }

  /// The base of a field / index projection, which auto-derefs
  /// through references (`r.x` with `r: &P`, `s[..]` with `s: &str`).
  fn base_place_lenders(&self, base: &'tcx Expr<'tcx>) -> Vec<ResourceTy> {
    let typeck = self.tcx.typeck(base.hir_id.owner);
    if typeck.expr_ty(base).is_ref() {
      self.value_lenders(base)
    } else {
      self.place_lenders(base)
    }
  }

  /// What the references stored *inside* the value at place `p`
  /// borrow from.
  fn content_lenders(&self, p: &'tcx Expr<'tcx>) -> Vec<ResourceTy> {
    let typeck = self.tcx.typeck(p.hir_id.owner);
    match p.kind {
      ExprKind::Call(..) | ExprKind::MethodCall(..) | ExprKind::Block(..) => self.value_lenders(p),
      _ if typeck.expr_ty(p).is_ref() => self.value_lenders(p),
      // A local `Option<&T>` / iterator that holds a borrow we track
      // (#336): `value_lenders` finds it in `borrow_map`.
      ExprKind::Path(QPath::Resolved(..)) if expr_to_rap_name(p, &self.tcx)
        .and_then(|n| self.raps.get(&n))
        .is_some_and(|rd| rd.rap.holds_borrow()) => self.value_lenders(p),
      // Any other local `Option<&T>` / iterator / struct: not tracked.
      _ => Vec::new(),
    }
  }
}
