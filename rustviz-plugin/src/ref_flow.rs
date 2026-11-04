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
//! What a *local* non-reference value holds (an `Option<&T>` or an
//! iterator stored in a variable) isn't tracked, so content flow out of
//! such a variable yields no lender: an honest "unknown" rather than a
//! guess.

use rustc_hir::{Expr, ExprKind, QPath, UnOp};
use rustc_middle::ty::{self, GenericArgKind, GenericArgsRef, Ty};
use rustc_middle::ty::adjustment::{Adjust, DerefAdjustKind};
use crate::expr_visitor::*;
use crate::expr_visitor_utils::*;
use crate::svg_generator::data::*;

/// How an argument reaches a call's result (see the module docs).
struct ArgFlow<'tcx> {
  index: usize,
  arg: &'tcx Expr<'tcx>,
  place: bool,
  content: bool,
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
  ) -> Option<(ty::FnSig<'tcx>, Option<GenericArgsRef<'tcx>>, Vec<&'tcx Expr<'tcx>>)> {
    let typeck = self.tcx.typeck(call.hir_id.owner);
    match call.kind {
      ExprKind::Call(fn_expr, args) => {
        let fty = typeck.node_type_opt(fn_expr.hir_id)?;
        match fty.kind() {
          ty::FnDef(def_id, gargs) => {
            let sig = self.tcx.fn_sig(*def_id).instantiate_identity().skip_norm_wip();
            Some((self.tcx.liberate_late_bound_regions(*def_id, sig), Some(*gargs), args.iter().collect()))
          }
          ty::FnPtr(..) => {
            let sig = fty.fn_sig(self.tcx);
            let scope = call.hir_id.owner.to_def_id();
            Some((self.tcx.liberate_late_bound_regions(scope, sig), None, args.iter().collect()))
          }
          _ => None,
        }
      }
      ExprKind::MethodCall(_, rcvr, args, _) => {
        let def_id = typeck.type_dependent_def_id(call.hir_id)?;
        let sig = self.tcx.fn_sig(def_id).instantiate_identity().skip_norm_wip();
        let mut all = vec![rcvr];
        all.extend(args.iter());
        Some((self.tcx.liberate_late_bound_regions(def_id, sig), typeck.node_args_opt(call.hir_id), all))
      }
      _ => None,
    }
  }

  /// For each argument of `call`, whether the call's result can borrow
  /// what that argument refers to or holds.
  fn call_arg_flows(&self, call: &'tcx Expr<'tcx>) -> Option<Vec<ArgFlow<'tcx>>> {
    let (sig, gargs, args) = self.callee_sig(call)?;
    // A type parameter matters only if this call instantiates it with
    // something that holds a reference (`T = &i32` in `unwrap`).
    let carries_ref = |index: u32| -> bool {
      match gargs.and_then(|g| g.get(index as usize).copied()) {
        Some(arg) => arg.walk().any(|g| matches!(g.kind(), GenericArgKind::Lifetime(_))),
        None => true,
      }
    };
    let output = sig.output();
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
    for (index, (input, arg)) in sig.inputs().iter().zip(args).enumerate() {
      let (place, content) = match input.kind() {
        ty::Ref(r, inner, _) => (regions.contains(r), shares(*inner)),
        _ => (false, shares(*input)),
      };
      if place || content {
        flows.push(ArgFlow { index, arg, place, content });
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
    let mut out: Vec<ResourceTy> = Vec::new();
    let mut unnamed = false;
    for flow in self.call_arg_flows(call).unwrap_or_default() {
      let ls = self.arg_lenders(flow.arg, flow.place, flow.content);
      if ls.is_empty() {
        unnamed = true;
      }
      for l in ls {
        if !matches!(l, ResourceTy::Value(_) | ResourceTy::Deref(_)) {
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
  pub fn call_arg_tie(&self, call: &'tcx Expr<'tcx>) -> ArgTie {
    let Some((_, _, args)) = self.callee_sig(call) else { return ArgTie::default() };
    let mut tie = ArgTie {
      receiver: matches!(call.kind, ExprKind::MethodCall(..)),
      total: args.len(),
      ..ArgTie::default()
    };
    for flow in self.call_arg_flows(call).unwrap_or_default() {
      let named = self.arg_lenders(flow.arg, flow.place, flow.content).iter()
        .any(|l| matches!(l, ResourceTy::Value(_) | ResourceTy::Deref(_)));
      tie.tied.push(flow.index);
      if !named {
        tie.unnamed += 1;
      }
    }
    tie
  }

  /// Lenders reachable through one argument, after rustc's adjustments
  /// (auto-ref of a method receiver, auto-deref, deref coercion).
  fn arg_lenders(&self, arg: &'tcx Expr<'tcx>, place: bool, content: bool) -> Vec<ResourceTy> {
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
        match expr_to_rap_name(e, &self.tcx).and_then(|n| self.borrow_map.get(&n)) {
          Some(rd) => rd.all_lenders(),
          // A non-reference variable (`opt: Option<&T>`, an iterator):
          // what it borrows isn't tracked.
          None => Vec::new(),
        }
      }
      ExprKind::AddrOf(_, _, inner) => self.place_lenders(inner),
      ExprKind::Call(..) | ExprKind::MethodCall(..) => self.call_lenders(e),
      ExprKind::Block(b, _) => b.expr.map(|x| self.value_lenders(x)).unwrap_or_default(),
      ExprKind::DropTemps(inner) => self.value_lenders(inner),
      _ => Vec::new(),
    }
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
      // A local `Option<&T>` / iterator / struct: not tracked.
      _ => Vec::new(),
    }
  }
}
