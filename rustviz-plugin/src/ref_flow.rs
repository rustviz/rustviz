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
use rustc_middle::ty::{self, GenericArgKind, GenericArgsRef, Ty, TypeFoldable};
use rustc_middle::ty::adjustment::{Adjust, DerefAdjustKind};
use rustc_span::def_id::DefId;
use crate::expr_visitor::*;
use crate::expr_visitor_utils::*;
use crate::svg_generator::data::*;

/// A reference a call returns into what the reference variable
/// `through` points to (see `call_reborrows_through`; #1078).
pub struct CallReborrow {
  pub through: ResourceAccessPoint,
  /// The place the reference points into, as written: `v`, `self.items`.
  pub target: String,
  /// The call that borrows it (`get_mut` in `v.get_mut(0).unwrap()`).
  pub method: String,
}

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
  /// The argument reaches the result only through an outlives bound
  /// (`x: &'a str` with result `&'b str` and `'a: 'b`): its lifetime
  /// isn't the result's, but outlives it, so the result may point
  /// there (#1125).
  via_outlives: bool,
  /// Content flow only, and what the parameter points to is itself a
  /// reference carried whole into the result (`r: &&'a String` with
  /// result `&'a String`, or `x: &T` with `T = &String`): the result
  /// has that reference's lifetime (#1084).
  referent_is_ref: bool,
  /// For content flow out of a struct or tuple (`&self` with `Self =
  /// Shelf<'a>`): the fields whose types carry what the result does
  /// (`items: Vec<&'a str>`). `None` for any other type.
  fields: Option<Vec<String>>,
  /// For content flow out of a value of a generic type (`&mut self`
  /// with `Self = IterMut<'a, T>`): the type, and the positions of its
  /// generic arguments that carry what the result does (`'a`, not `T`,
  /// for `next`'s `&'a mut T`). When the argument is itself a call's
  /// result (`refs.iter_mut()`), what those arguments of its type
  /// borrow can be told apart (#1395). `None` for any other type.
  shared_args: Option<(DefId, Vec<usize>)>,
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
  /// The reference the call returns (in an `Option` / `Result` or not),
  /// as a pointer: only its own lifetime, not those of the references
  /// stored in what it points to. `refs.last_mut()` on a `Vec<&String>`
  /// returns `Option<&'a mut T>` with `T = &String`: the result points
  /// into `refs`; the `&String` it points to borrows what refs's
  /// element does, but that is reached through the pointee, not lent
  /// to the result (#1382).
  Pointer,
  /// What the reference the call returns points to (in an `Option` /
  /// `Result` or not): only the lifetimes inside it.
  /// `refs.last().copied()` copies the `&String` out of the `&&String`
  /// `last` returns: it borrows what refs's element does, not refs.
  Pointee,
  /// Some generic arguments (by position) of the struct or enum the
  /// call returns, of this type: `'a` in the `IterMut<'a, T>` that
  /// `refs.iter_mut()` returns, which borrows refs; `T = &String` holds
  /// what refs's elements borrow (#1395).
  Args(DefId, &'s [usize]),
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
            if let Some((sig, gargs, def_id)) = self.impl_view_sig(call, *def_id, gargs) {
              return Some((sig, Some(gargs), args.iter().collect(), Some(def_id)));
            }
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
        let mut all = vec![rcvr];
        all.extend(args.iter());
        if let Some((sig, gargs, def_id)) = typeck.node_args_opt(call.hir_id)
          .and_then(|gargs| self.impl_view_sig(call, def_id, gargs)) {
          return Some((sig, Some(gargs), all, Some(def_id)));
        }
        let sig = self.tcx.fn_sig(def_id).instantiate_identity().skip_norm_wip();
        Some((self.tcx.liberate_late_bound_regions(def_id, sig), typeck.node_args_opt(call.hir_id), all, Some(def_id)))
      }
      _ => None,
    }
  }

  /// A call of a trait method that resolves to one impl's method
  /// (`names.top()` with `impl<'a> Top<'a> for Vec<&'a str>`): the
  /// trait method's signature with the trait's `Self` and parameters
  /// replaced by that impl's (`fn(&Vec<&'a str>) -> &'a str`), the
  /// call's generic arguments for the impl method, and its `DefId`.
  /// This is what the borrow checker sees at the call: the trait's
  /// signature alone (`fn(&Self) -> &'a str`) doesn't say that the
  /// result's `'a` is the lifetime of the references in the receiver
  /// (#1089). `None` for any other call, or when the impl isn't known
  /// at the call (a generic `T: Top<'a>`), or for a provided method the
  /// impl doesn't override.
  fn impl_view_sig(
    &self,
    call: &'tcx Expr<'tcx>,
    def_id: DefId,
    gargs: GenericArgsRef<'tcx>,
  ) -> Option<(ty::FnSig<'tcx>, GenericArgsRef<'tcx>, DefId)> {
    let tcx = self.tcx;
    tcx.trait_of_assoc(def_id)?;
    let env = typing_env(tcx, call.hir_id.owner);
    let inst = ty::Instance::try_resolve(tcx, env, def_id, gargs).ok()??;
    let ty::InstanceKind::Item(impl_m) = inst.def else { return None };
    let impl_id = tcx.trait_impl_of_assoc(impl_m)?;
    let trait_ref = tcx.impl_trait_ref(impl_id).instantiate_identity().skip_norm_wip();
    // The trait method's own parameters follow the trait's; the impl
    // method's follow the impl's.
    let own = ty::GenericArgs::identity_for_item(tcx, impl_m);
    let parent = tcx.generics_of(impl_id).count();
    let args = tcx.mk_args_from_iter(trait_ref.args.iter().chain(own.iter().skip(parent)));
    let sig = tcx.fn_sig(def_id).instantiate(tcx, args).skip_norm_wip();
    Some((tcx.liberate_late_bound_regions(impl_m, sig), inst.args, impl_m))
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
      // `Option<Self::Item>` in `next`'s signature, as the impl sets it
      // (`Option<&'a mut T>` for `IterMut<'a, T>`), to tell its parts
      // apart (#1395).
      FlowTarget::Pointer | FlowTarget::Pointee => self.impl_assoc_types(sig.output(), def_id),
      FlowTarget::Args(want, _) => {
        let out = self.impl_assoc_types(sig.output(), def_id);
        match out.kind() {
          ty::Adt(d, _) if d.did() == want => out,
          _ => return None,
        }
      }
      FlowTarget::Field(f) => match sig.output().kind() {
        ty::Adt(def, fargs) if def.is_struct() =>
          def.all_fields().find(|fd| fd.name.as_str() == f)?.ty(self.tcx, fargs),
        // `pair(&s)` returning `(&'a str, i32)`: element `0` (#1341).
        ty::Tuple(tys) => *tys.get(f.parse::<usize>().ok()?)?,
        // The struct or tuple comes out of an `Option` / `Result`
        // (`opart(&s)?`, `opart(&s).unwrap()`, #1341): with a single
        // lifetime in the result, every reference in it has that one,
        // so the field carries what the whole result does.
        _ if self.call_result_has_one_lifetime(self.passed_through_from(call)) => sig.output(),
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
    let parts: Vec<ty::GenericArg<'tcx>> = match target {
      FlowTarget::Pointer => {
        let mut out = Vec::new();
        outside_pointees(output, &mut out, false);
        out
      }
      FlowTarget::Pointee => {
        let mut out = Vec::new();
        outside_pointees(output, &mut out, true);
        out
      }
      FlowTarget::Args(_, at) => match output.kind() {
        ty::Adt(_, a) => at.iter().filter_map(|i| a.get(*i)).flat_map(|g| g.walk()).collect(),
        _ => return None,
      },
      _ => output.walk().collect(),
    };
    for g in parts {
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
    if matches!(target, FlowTarget::Result | FlowTarget::Pointer | FlowTarget::Pointee | FlowTarget::Args(..)) && !params.is_empty() {
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
    // A parameter type that must outlive a lifetime the result carries
    // can be stored in the result under that lifetime: `boxed<'a>(f:
    // impl Fn() + 'a) -> Box<dyn Fn() + 'a>` boxes `f`, and the result
    // holds the borrows the closure captured (#1104). The result names
    // only `'a`, not the parameter, so this comes from the `F: 'a`
    // bound.
    if let Some(def_id) = def_id {
      for (clause, _) in self.tcx.predicates_of(def_id).predicates.iter() {
        let Some(pred) = clause.as_type_outlives_clause() else { continue };
        let ty::OutlivesPredicate(t, r) = pred.skip_binder();
        if let ty::Param(p) = t.kind() {
          if regions.contains(&r) && carries_ref(p.index) && !params.contains(&p.index) {
            params.push(p.index);
          }
        }
      }
    }
    // A lifetime that outlives one the result carries (`'a: 'b`, also
    // written `<'a: 'b, 'b>` or through a chain `'a: 'c, 'c: 'b`): a
    // reference with it coerces to the shorter one, so the result may
    // point where it does (#1125). The signature alone says so, as it
    // does to the borrow checker.
    let direct = regions.clone();
    if let Some(def_id) = def_id {
      let outlives: Vec<(ty::Region<'tcx>, ty::Region<'tcx>)> = self.tcx.predicates_of(def_id).predicates.iter()
        .filter_map(|(clause, _)| clause.as_region_outlives_clause())
        .map(|pred| { let ty::OutlivesPredicate(a, b) = pred.skip_binder(); (a, b) })
        .collect();
      let mut grew = true;
      while grew {
        grew = false;
        for (a, b) in outlives.iter() {
          if regions.contains(b) && !a.is_static() && !regions.contains(a) {
            regions.push(*a);
            grew = true;
          }
        }
      }
    }
    if regions.is_empty() && params.is_empty() {
      return Some(Vec::new());
    }
    let shares_in = |t: Ty<'tcx>, regions: &[ty::Region<'tcx>]| -> bool {
      t.walk().any(|g| match g.kind() {
        GenericArgKind::Lifetime(r) => regions.contains(&r),
        GenericArgKind::Type(t) => matches!(t.kind(), ty::Param(p) if params.contains(&p.index)),
        GenericArgKind::Const(_) => false,
      })
    };
    let shares = |t: Ty<'tcx>| shares_in(t, &regions);
    let mut flows = Vec::new();
    // The receiver doesn't flow into itself.
    let skip = usize::from(matches!(target, FlowTarget::Receiver));
    for (index, (input, arg)) in sig.inputs().iter().zip(args).enumerate().skip(skip) {
      let (place, content, place_mut) = match input.kind() {
        ty::Ref(r, inner, m) => (regions.contains(r), shares(*inner), m.is_mut()),
        _ => (false, shares(*input), false),
      };
      if place || content {
        let via_outlives = match input.kind() {
          ty::Ref(r, inner, _) => !direct.contains(r) && !shares_in(*inner, &direct),
          _ => !shares_in(*input, &direct),
        };
        let by_value = !matches!(input.kind(), ty::Ref(..));
        let referent_is_ref = !place && match input.kind() {
          ty::Ref(_, inner, _) => match inner.kind() {
            ty::Ref(r, ..) => regions.contains(r),
            ty::Param(p) => params.contains(&p.index)
              && gargs.and_then(|g| g.get(p.index as usize)).and_then(|a| a.as_type()).is_some_and(|t| t.is_ref()),
            _ => false,
          },
          _ => false,
        };
        let held = match input.kind() { ty::Ref(_, inner, _) => *inner, _ => *input };
        let shared_args = match held.kind() {
          ty::Adt(def, hargs) if content => Some((def.did(), hargs.iter().enumerate()
            .filter(|(_, g)| match g.kind() {
              GenericArgKind::Lifetime(r) => regions.contains(&r),
              GenericArgKind::Type(t) => shares(t),
              GenericArgKind::Const(_) => false,
            })
            .map(|(i, _)| i)
            .collect())),
          _ => None,
        };
        let fields = content.then(|| {
          match held.kind() {
            ty::Adt(def, fargs) if def.is_struct() => Some(def.all_fields()
              .filter(|fd| shares(fd.ty(self.tcx, fargs)))
              .map(|fd| fd.name.as_str().to_owned())
              .collect()),
            ty::Tuple(tys) => Some(tys.iter().enumerate()
              .filter(|(_, t)| shares(*t))
              .map(|(i, _)| i.to_string())
              .collect()),
            _ => None,
          }
        }).flatten();
        flows.push(ArgFlow { index, arg, place, content, place_mut: place && place_mut, by_value, via_outlives, referent_is_ref, fields, shared_args });
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
    // `Some(W { a: &s, b: &mut t })` (#708).
    let mixed = self.ctor_mixed_muts(call);
    if !mixed.is_empty() {
      return (mixed.into_iter().map(|(l, _)| l).collect(), false);
    }
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

  /// The 'static data (see `static_lender_of`) that the reference in
  /// field `field` of `call`'s result points to, when every argument
  /// the signature ties to that field is such data: `part("a")` with
  /// `part(c: &'a str) -> Part<'a>` (#1356). None otherwise.
  pub fn call_field_static_lender(&self, call: &'tcx Expr<'tcx>, field: &str) -> Option<StaticLender> {
    // (`opart("e")` returning `Option<(&'a str, i32)>`, then unwrapped:
    // with one lifetime in the result, the field has the result's.)
    let flows = self.call_arg_flows_into(call, FlowTarget::Field(field))
      .or_else(|| self.call_result_has_one_lifetime(call).then(|| self.call_arg_flows_into(call, FlowTarget::Result)).flatten())?;
    let mut found: Option<StaticLender> = None;
    for f in flows {
      // (`opart("e").unwrap()`: what `opart`'s result holds.)
      let l = match f.arg.kind {
        ExprKind::Call(..) | ExprKind::MethodCall(..) => self.call_field_static_lender(f.arg, field),
        _ => self.static_lender_of(f.arg),
      }?;
      match &found {
        Some(x) if *x != l => return None,
        _ => found = Some(l),
      }
    }
    found
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

  /// The variables the reference `call` returns (or a value holding
  /// one) borrows, when that is fewer than `call_lenders` names, through
  /// the calls that only pass the borrow on (`unwrap`, `copied`):
  /// - `refs.last_mut().unwrap()` on `refs: Vec<&String>` (`last_mut`
  ///   returns `Option<&'a mut T>` with `T = &String`) points into refs;
  ///   the `&String` there borrows a1, but the result reaches a1 only
  ///   through what it points to, as `&mut refs[0]` does (#1382);
  /// - `refs.last().copied().unwrap()` is that `&String`, copied out:
  ///   it borrows a1, not refs;
  /// - `refs.iter_mut().next().unwrap()` points into refs as well:
  ///   `next` returns `Option<&'a mut T>` for the `IterMut<'a, T>`
  ///   that `iter_mut` returns, whose `'a` borrows refs (#1395).
  /// None otherwise.
  pub fn pointer_lenders(&self, call: &'tcx Expr<'tcx>) -> Option<Vec<ResourceTy>> {
    let (inner, target) = self.narrowed_target(call)?;
    let (lenders, _) = self.narrowed_lenders_of_flows(self.call_arg_flows_into(inner, target)?);
    let all = self.call_lenders(call);
    (!lenders.is_empty() && lenders.len() < all.len() && lenders.iter().all(|l| all.contains(l))).then_some(lenders)
  }

  /// `call_arg_tie` for the call `pointer_lenders` narrows through
  /// (`last_mut` in `refs.last_mut().unwrap()`), when `borrower` is it
  /// and `call`'s value is the reference it returns.
  pub fn narrowed_tie(&self, call: &'tcx Expr<'tcx>, borrower: &'tcx Expr<'tcx>) -> Option<ArgTie> {
    if let Some((origin, _, def, at)) = self.narrowed_origin_parts(call) {
      return (origin.hir_id == borrower.hir_id).then(|| self.call_arg_tie_to(origin, FlowTarget::Args(def, &at)));
    }
    let (inner, target) = self.narrowed_target(call)?;
    (inner.hir_id == borrower.hir_id && matches!(target, FlowTarget::Pointer) && self.pointer_lenders(call).is_some())
      .then(|| self.call_arg_tie_to(inner, target))
  }

  /// The call a reference `pointer_lenders` narrows comes from when that
  /// is the receiver of the call returning it, and the methods that
  /// pass it on, innermost first: `iter_mut` and `[next, unwrap]` for
  /// `refs.iter_mut().next().unwrap()` (#1395). The borrow of refs is
  /// made by `iter_mut`; `next` hands on the iterator's `'a`.
  pub fn narrowed_origin(&self, call: &'tcx Expr<'tcx>) -> Option<(&'tcx Expr<'tcx>, Vec<String>)> {
    self.narrowed_origin_parts(call).map(|(origin, passed, ..)| (origin, passed))
  }

  /// `narrowed_origin`, plus the type and the positions of its generic
  /// arguments that `FlowTarget::Args` narrows the origin's result to.
  fn narrowed_origin_parts(&self, call: &'tcx Expr<'tcx>) -> Option<(&'tcx Expr<'tcx>, Vec<String>, DefId, Vec<usize>)> {
    let (inner, target) = self.narrowed_target(call)?;
    let ExprKind::MethodCall(seg, recv, ..) = inner.kind else { return None };
    if !matches!(target, FlowTarget::Pointer) || self.pointer_lenders(call).is_none()
      || recv.span.from_expansion() || !matches!(recv.kind, ExprKind::Call(..) | ExprKind::MethodCall(..)) {
      return None;
    }
    let flows = self.call_arg_flows_into(inner, target)?;
    let [flow] = flows.as_slice() else { return None };
    let Some((def, at)) = &flow.shared_args else { return None };
    if flow.index != 0 || flow.place || at.is_empty() {
      return None;
    }
    let (end, mut passed) = self.pass_through_chain(call);
    if end.hir_id != inner.hir_id {
      return None;
    }
    passed.insert(0, seg.ident.to_string());
    Some((recv, passed, *def, at.clone()))
  }

  /// The call whose result `call`'s value is copied out of, when that
  /// value is the reference stored where the result points
  /// (`refs.last().copied().unwrap()`: `last`; see `pointer_lenders`).
  pub fn copied_out_of(&self, call: &'tcx Expr<'tcx>) -> Option<&'tcx Expr<'tcx>> {
    let (inner, target) = self.narrowed_target(call)?;
    (matches!(target, FlowTarget::Pointee) && self.pointer_lenders(call).is_some()).then_some(inner)
  }

  /// The call `call`'s value comes out of (see `passed_through_from`;
  /// the innermost one on the way that returns a reference, `next` in
  /// `refs.iter_mut().next().unwrap()`), and which part of its result
  /// `call`'s value is (see `pointer_lenders`): the reference it
  /// returns, or what that points to. None for the whole result.
  fn narrowed_target(&self, call: &'tcx Expr<'tcx>) -> Option<(&'tcx Expr<'tcx>, FlowTarget<'static>)> {
    let typeck = self.tcx.typeck(call.hir_id.owner);
    let peel = |t: Ty<'tcx>| -> Ty<'tcx> {
      match t.kind() {
        ty::Adt(def, args) if self.tcx.is_diagnostic_item(rustc_span::sym::Option, def.did())
          || self.tcx.is_diagnostic_item(rustc_span::sym::Result, def.did()) => args.type_at(0),
        _ => t,
      }
    };
    let holds_ref = |t: Ty<'tcx>| t.walk().any(|g| matches!(g.kind(), GenericArgKind::Lifetime(_)));
    let erase = |t: Ty<'tcx>| self.tcx.erase_and_anonymize_regions(t);
    let outer = peel(typeck.expr_ty(call));
    let mut steps = self.pass_through_steps(call);
    steps.reverse();
    steps.into_iter().find_map(|inner| {
      let from = peel(typeck.expr_ty(inner));
      let ty::Ref(_, pointee, _) = from.kind() else { return None };
      if !holds_ref(*pointee) {
        None
      } else if erase(from) == erase(outer) {
        Some((inner, FlowTarget::Pointer))
      } else if inner.hir_id != call.hir_id && erase(*pointee) == erase(outer) {
        Some((inner, FlowTarget::Pointee))
      } else {
        None
      }
    })
  }

  /// `lenders_of_flows`, where an argument that is a call's result
  /// (`refs.iter_mut()`) and reaches the result only through some of its
  /// type's generic arguments (`'a` in `IterMut<'a, T>`) lends only what
  /// those borrow (refs, not what refs's elements borrow, #1395).
  fn narrowed_lenders_of_flows(&self, flows: Vec<ArgFlow<'tcx>>) -> (Vec<ResourceTy>, bool) {
    let mut out: Vec<ResourceTy> = Vec::new();
    let mut unnamed = false;
    for flow in flows {
      let sub = match &flow.shared_args {
        Some((def, at)) if !flow.place && !at.is_empty() && !flow.arg.span.from_expansion()
          && matches!(flow.arg.kind, ExprKind::Call(..) | ExprKind::MethodCall(..)) =>
          self.call_arg_flows_into(flow.arg, FlowTarget::Args(*def, at)),
        _ => None,
      };
      let part = match &flow.shared_args {
        Some((def, at)) if !flow.place && !at.is_empty() => self.holder_part_lenders(flow.arg, *def, at),
        _ => None,
      };
      let (ls, u) = match (sub, part) {
        (Some(sub), _) => self.narrowed_lenders_of_flows(sub),
        (None, Some(ls)) => (ls, false),
        (None, None) => self.lenders_of_flows(vec![flow]),
      };
      unnamed |= u;
      for l in ls {
        if !out.contains(&l) {
          out.push(l);
        }
      }
    }
    (out, unnamed)
  }

  /// For a local holder `arg` of the type `def` (`it`, an `Iter<'a,
  /// T>` from `refs.iter()` on a `Vec<&String>`), what the generic
  /// arguments at positions `at` of its type borrow: its own lenders
  /// for its lifetimes (`'a`: refs), and what it reaches through them
  /// (`RefData::through`) for the type parameters it holds behind them
  /// (`T`: a1). So `it.next().unwrap()` points into refs, and
  /// `it.next().copied().unwrap()` is a1's `&String` (#1399). None for
  /// a mix of both, or anything else.
  fn holder_part_lenders(&self, arg: &'tcx Expr<'tcx>, def: DefId, at: &[usize]) -> Option<Vec<ResourceTy>> {
    if !matches!(arg.kind, ExprKind::Path(QPath::Resolved(None, _))) || arg.span.from_expansion() {
      return None;
    }
    let name = expr_to_rap_name(arg, &self.tcx)?;
    if !self.raps.get(&name).is_some_and(|rd| rd.rap.holds_borrow()) {
      return None;
    }
    let rd = self.borrow_map.get(&name)?;
    if rd.through.is_empty() {
      return None;
    }
    let ty::Adt(adt, args) = self.tcx.typeck(arg.hir_id.owner).expr_ty(arg).kind() else { return None };
    if adt.did() != def {
      return None;
    }
    let is_lifetime = |i: &usize| args.get(*i).is_some_and(|g| matches!(g.kind(), GenericArgKind::Lifetime(_)));
    if at.iter().all(is_lifetime) {
      return Some(rd.all_lenders());
    }
    let behind = self.params_behind_borrows(*adt);
    at.iter().all(|i| behind.get(*i).copied().unwrap_or(false)).then(|| rd.through.clone())
  }

  /// `ty` (a trait method's signature type, seen from the impl `def_id`
  /// belongs to, see `impl_view_sig`) with that impl's associated types
  /// put in: `Option<<IterMut<'a, T> as Iterator>::Item>` is
  /// `Option<&'a mut T>`. Unchanged for any other method.
  fn impl_assoc_types(&self, ty: Ty<'tcx>, def_id: Option<DefId>) -> Ty<'tcx> {
    let tcx = self.tcx;
    let Some(impl_id) = def_id.and_then(|d| tcx.trait_impl_of_assoc(d)) else { return ty };
    let trait_ref = tcx.impl_trait_ref(impl_id).instantiate_identity().skip_norm_wip();
    let items = tcx.impl_item_implementor_ids(impl_id);
    ty.fold_with(&mut ty::BottomUpFolder {
      tcx,
      ty_op: |t: Ty<'tcx>| match t.kind() {
        ty::Alias(alias) => match alias.kind {
          ty::AliasTyKind::Projection { def_id: assoc }
            if tcx.parent(assoc) == trait_ref.def_id && alias.args == trait_ref.args
              && tcx.generics_of(assoc).own_params.is_empty() =>
            items.get(&assoc).map_or(t, |item| tcx.type_of(*item).instantiate_identity().skip_norm_wip()),
          _ => t,
        },
        _ => t,
      },
      lt_op: |r| r,
      ct_op: |c| c,
    })
  }

  fn lenders_of_flows(&self, flows: Vec<ArgFlow<'tcx>>) -> (Vec<ResourceTy>, bool) {
    let mut out: Vec<ResourceTy> = Vec::new();
    let mut unnamed = false;
    for flow in flows {
      let ls = self.arg_lenders(&flow);
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

  /// Whether every reference in `call`'s result has the same lifetime
  /// in the callee's signature: its type names exactly one lifetime
  /// (`Option<&'a String>`), and no type parameter that could bring
  /// others. Then a reference copied out of the result has the lifetime
  /// `call_arg_tie` describes; with `(&'a str, &'b str)` it has only
  /// one of them (#681).
  pub fn call_result_has_one_lifetime(&self, call: &'tcx Expr<'tcx>) -> bool {
    let Some((sig, _, _, _)) = self.callee_sig(call) else { return false };
    let mut regions: Vec<ty::Region<'tcx>> = Vec::new();
    for g in sig.output().walk() {
      match g.kind() {
        GenericArgKind::Lifetime(r) => {
          if !regions.contains(&r) {
            regions.push(r);
          }
        }
        GenericArgKind::Type(t) if matches!(t.kind(), ty::Param(_)) => return false,
        _ => {}
      }
    }
    regions.len() == 1 && !regions[0].is_static()
  }

  /// The call whose result `call`'s value comes out of, peeling the
  /// methods that only pass a call's result on (`unwrap`, `expect`):
  /// `opart(&s)` for `opart(&s).unwrap()`. `call` itself otherwise.
  pub fn passed_through_from(&self, call: &'tcx Expr<'tcx>) -> &'tcx Expr<'tcx> {
    self.pass_through_chain(call).0
  }

  /// `passed_through_from`, plus the names of the methods peeled,
  /// innermost first (`["unwrap"]`). Unlike `borrowing_call`, the inner
  /// call may be a function call (`opart(&s).unwrap()`).
  pub fn pass_through_chain(&self, call: &'tcx Expr<'tcx>) -> (&'tcx Expr<'tcx>, Vec<String>) {
    let steps = self.pass_through_steps(call);
    let mut passed: Vec<String> = steps[..steps.len() - 1].iter()
      .filter_map(|c| match c.kind { ExprKind::MethodCall(seg, ..) => Some(seg.ident.to_string()), _ => None })
      .collect();
    passed.reverse();
    (steps[steps.len() - 1], passed)
  }

  /// The calls `pass_through_chain` goes through, outermost (`call`)
  /// first: `[unwrap, opart(&s)]` for `opart(&s).unwrap()`.
  fn pass_through_steps(&self, call: &'tcx Expr<'tcx>) -> Vec<&'tcx Expr<'tcx>> {
    let mut steps = vec![call];
    let mut c = call;
    while let ExprKind::MethodCall(_, recv, ..) = c.kind {
      if c.span.from_expansion() || recv.span.from_expansion()
        || !matches!(recv.kind, ExprKind::MethodCall(..) | ExprKind::Call(..)) {
        break;
      }
      let tie = self.call_arg_tie(c, None);
      if !(tie.receiver && tie.tied == [0] && tie.stored_in.is_empty() && tie.unnamed == 0) {
        break;
      }
      c = recv;
      steps.push(c);
    }
    steps
  }

  /// Which of `call`'s arguments its signature ties the result to,
  /// and how many of those have no column (see `ArgTie`).
  pub fn call_arg_tie(&self, call: &'tcx Expr<'tcx>, field: Option<&str>) -> ArgTie {
    if field.is_none() && !self.ctor_mixed_muts(call).is_empty() {
      return ArgTie { total: 1, tied: vec![0], holds_all: true, ..ArgTie::default() };
    }
    self.call_arg_tie_to(call, match field { Some(f) => FlowTarget::Field(f), None => FlowTarget::Result })
  }

  /// `call_arg_tie` for the part `target` of the result.
  fn call_arg_tie_to(&self, call: &'tcx Expr<'tcx>, target: FlowTarget<'_>) -> ArgTie {
    let Some((_, _, args, _)) = self.callee_sig(call) else { return ArgTie::default() };
    let mut tie = ArgTie {
      receiver: matches!(call.kind, ExprKind::MethodCall(..)),
      total: args.len(),
      ..ArgTie::default()
    };
    let mut holds_all_literal = false;
    for flow in self.call_arg_flows_into(call, target).unwrap_or_default() {
      holds_all_literal = matches!(flow.arg.kind,
        ExprKind::Array(..) | ExprKind::Struct(..) | ExprKind::Tup(..) | ExprKind::Repeat(..));
      let named = self.arg_lenders(&flow).iter()
        .any(|l| matches!(l, ResourceTy::Value(_) | ResourceTy::Deref(_)));
      tie.tied.push(flow.index);
      tie.via_outlives |= flow.via_outlives;
      if flow.content && !flow.place && !flow.by_value {
        tie.stored_in.push(StoredTie {
          index: flow.index,
          name: self.stored_in_name(flow.arg, flow.referent_is_ref),
          referent_is_ref: flow.referent_is_ref,
        });
      }
      // A closure that captures nothing by reference holds no borrow:
      // it isn't "data with no column" (see `lenders_of_flows`).
      if !named && !matches!(flow.arg.kind, ExprKind::Closure(..)) {
        tie.unnamed += 1;
      }
    }
    // A single aggregate argument holds all its borrows, not one of them.
    tie.holds_all = tie.tied.len() == 1 && holds_all_literal;
    tie
  }

  /// The variable a reference argument points to, whose stored
  /// references a call's result can carry: `rows` for `&rows`, or for
  /// `rows` auto-referenced as a method receiver. With `is_ref`, that
  /// variable is itself the carried reference (`t` in `first(&t)`).
  /// When the argument reaches its data through a reference variable
  /// (`rr`, `&*rr`, an auto-dereferenced `rr`), the one variable it
  /// points to (`rows` for `rr = &rows`, #1089). `None` when that
  /// isn't a single variable with a column, or the argument isn't a
  /// plain variable or field.
  fn stored_in_name(&self, arg: &'tcx Expr<'tcx>, is_ref: bool) -> Option<String> {
    let typeck = self.tcx.typeck(arg.hir_id.owner);
    let adjs = typeck.expr_adjustments(arg);
    let place = if let Some(bpos) = adjs.iter().position(|a| matches!(a.kind, Adjust::Borrow(_))) {
      let mut place = arg;
      for a in &adjs[..bpos] {
        match a.kind {
          Adjust::Deref(DerefAdjustKind::Builtin) => match place.kind {
            ExprKind::AddrOf(_, _, inner) => place = inner,
            _ => return self.pointee_name(place, is_ref),
          },
          // An overloaded deref (`Vec<T>` → `[T]`) stays in the same value.
          _ => {}
        }
      }
      place
    } else {
      match arg.kind {
        ExprKind::AddrOf(_, _, inner) => inner,
        _ => return self.pointee_name(arg, is_ref),
      }
    };
    if let ExprKind::Unary(UnOp::Deref, inner) = place.kind {
      if typeck.expr_ty(inner).is_ref() {
        return self.pointee_name(inner, is_ref);
      }
    }
    fn plain<'t>(e: &'t Expr<'t>) -> bool {
      match e.kind {
        ExprKind::Path(QPath::Resolved(None, _)) => true,
        ExprKind::Field(base, _) => plain(base),
        _ => false,
      }
    }
    if !plain(place) || typeck.expr_ty(place).is_ref() != is_ref {
      return None;
    }
    expr_to_rap_name(place, &self.tcx)
  }

  /// `stored_in_name` for an argument that is a reference `e`: the
  /// variable `e` points to, when that is exactly one variable with a
  /// column (and, with `is_ref`, itself a reference).
  fn pointee_name(&self, e: &'tcx Expr<'tcx>, is_ref: bool) -> Option<String> {
    if !matches!(e.kind, ExprKind::Path(QPath::Resolved(None, _)) | ExprKind::Field(..)) {
      return None;
    }
    match self.value_lenders(e).as_slice() {
      [ResourceTy::Value(rap)] if rap.is_ref() == is_ref && !rap.is_fn()
        && expr_to_rap_name(e, &self.tcx).is_some_and(|n| n != *rap.name()) => Some(rap.name().to_owned()),
      _ => None,
    }
  }

  /// `v.get_mut(0).unwrap()` / `v.first().unwrap()` /
  /// `v.iter().next().unwrap()` / `self.items.first().unwrap()` with
  /// `v` / `self` a reference variable: the reference `call` returns
  /// points into what `v` points to, reborrowed through `v` (#1078).
  ///
  /// Decided from the callees' signatures: one call in the chain ties
  /// its result's lifetime to the borrow of `*v` (a place flow from
  /// `v`), and each call after it passes that borrow on in a type
  /// parameter that isn't behind a reference in the argument
  /// (`unwrap(self: Option<T>) -> T`, `next(&mut self) ->
  /// Option<Self::Item>`). `v.first().copied()` gives back the `T`
  /// behind `&T`, a reference stored in the Vec rather than one into
  /// it, so it isn't this. If `v` is a parameter, the function's own
  /// signature must tie the result to it.
  /// `v.first()?` passes the reference on as `unwrap` does (#1352).
  pub fn call_reborrows_through(&self, call: &'tcx Expr<'tcx>) -> Option<CallReborrow> {
    let typeck = self.tcx.typeck(call.hir_id.owner);
    if call.span.from_expansion() || !typeck.node_type(call.hir_id).is_ref() {
      return None;
    }
    let source = try_operand(call).unwrap_or(call);
    let found = self.call_reborrow_source(source)?;
    self.param_tied_to_result(call, found.through.name()).then_some(found)
  }

  fn call_reborrow_source(&self, call: &'tcx Expr<'tcx>) -> Option<CallReborrow> {
    if call.span.from_expansion() {
      return None;
    }
    let method = match call.kind {
      ExprKind::MethodCall(seg, ..) => seg.ident.as_str().to_owned(),
      ExprKind::Call(fn_expr, _) => hirid_to_var_name(fn_expr.hir_id, &self.tcx)?,
      _ => return None,
    };
    let (sig, _, _, _) = self.callee_sig(call)?;
    let out_params: Vec<u32> = sig.output().walk()
      .filter_map(|g| match g.kind() {
        GenericArgKind::Type(t) => match t.kind() { ty::Param(p) => Some(p.index), _ => None },
        _ => None,
      })
      .collect();
    let mut found: Option<CallReborrow> = None;
    for flow in self.call_arg_flows(call)? {
      let this = if flow.place {
        self.reborrowed_ref_arg(flow.arg, method.clone())
      } else {
        let input = *sig.inputs().get(flow.index)?;
        let value_ty = match input.kind() {
          ty::Ref(_, inner, _) if !flow.by_value => *inner,
          _ => input,
        };
        let mut inner = flow.arg;
        while let ExprKind::DropTemps(i) = inner.kind {
          inner = i;
        }
        if param_outside_refs(value_ty, &out_params) {
          self.call_reborrow_source(inner)
        } else {
          None
        }
      };
      let this = this?;
      match &found {
        Some(f) if f.through != this.through || f.target != this.target => return None,
        Some(_) => {}
        None => found = Some(this),
      }
    }
    found
  }

  /// `arg` is used where the callee borrows the place it denotes: a
  /// reference variable with a column, auto-dereferenced or passed as
  /// is (`v` in `v.get_mut(0)`), or a field reached through one
  /// (`self.items` in `self.items.first()`).
  fn reborrowed_ref_arg(&self, arg: &'tcx Expr<'tcx>, method: String) -> Option<CallReborrow> {
    if arg.span.from_expansion() {
      return None;
    }
    let typeck = self.tcx.typeck(arg.hir_id.owner);
    let adjs = typeck.expr_adjustments(arg);
    let (through, target) = match arg.kind {
      ExprKind::Path(QPath::Resolved(None, _)) => {
        // An auto-ref of the reference itself (`&v`) would borrow `v`,
        // not what it points to.
        if !typeck.node_type(arg.hir_id).is_ref() || adjs.first().is_some_and(|a| !matches!(a.kind, Adjust::Deref(_))) {
          return None;
        }
        let r = expr_to_rap_name(arg, &self.tcx).and_then(|n| self.raps.get(&n)).map(|rd| rd.rap.to_owned())?;
        let target = r.name().to_owned();
        (r, target)
      }
      // `self.items` auto-referenced: a place inside what `self`
      // points to.
      ExprKind::Field(..) if adjs.iter().any(|a| matches!(a.kind, Adjust::Borrow(_))) => {
        let ResourceTy::DerefField(r, place, _, _) = self.field_through_ref(arg)? else { return None };
        (r, place)
      }
      _ => return None,
    };
    if !through.is_ref() || through.is_static() || through.is_fn() {
      return None;
    }
    Some(CallReborrow { through, target, method })
  }

  /// If `name` is a reference parameter of the function `e` is in,
  /// whether the function's signature gives its result that
  /// parameter's lifetime (`fn g(v: &mut Vec<i32>) -> &mut i32`, by
  /// elision). True for anything else.
  fn param_tied_to_result(&self, e: &'tcx Expr<'tcx>, name: &str) -> bool {
    let owner = e.hir_id.owner.def_id;
    if self.tcx.hir_enclosing_body_owner(e.hir_id) != owner || !self.tcx.def_kind(owner).is_fn_like() {
      return true;
    }
    let sig = self.tcx.liberate_late_bound_regions(owner.to_def_id(), self.tcx.fn_sig(owner).instantiate_identity().skip_norm_wip());
    let out: Vec<ty::Region<'tcx>> = sig.output().walk()
      .filter_map(|g| match g.kind() { GenericArgKind::Lifetime(r) => Some(r), _ => None })
      .collect();
    let body = self.tcx.hir_body_owned_by(owner);
    for (param, input) in body.params.iter().zip(sig.inputs().iter()) {
      let rustc_hir::PatKind::Binding(_, _, ident, None) = param.pat.kind else { continue };
      if ident.name.as_str() != name {
        continue;
      }
      return match input.kind() {
        ty::Ref(r, _, _) => out.contains(r),
        _ => true,
      };
    }
    true
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
      // (`Some(r)` with r a `&mut` where an `Option<&T>` is expected
      // holds an immutable reborrow, `&*r`, #1329.)
      ExprKind::Path(QPath::Resolved(..)) | ExprKind::Field(..) => {
        !is_implicit_shared_reborrow_of_mut(e, self.tcx.typeck(e.hir_id.owner))
          && expr_to_rap_name(e, &self.tcx)
            .and_then(|n| self.borrow_map.get(&n))
            .is_some_and(|rd| rd.ref_mutability)
      }
      ExprKind::AddrOf(_, m, _) => m.is_mut(),
      ExprKind::Call(..) | ExprKind::MethodCall(..) => self.call_borrow_is_mut(e),
      ExprKind::Closure(closure) => self.closure_ref_captures(closure.def_id).iter().any(|(_, m)| *m),
      // (A tuple literal, `Some((&mut a, &mut b))`, #1459.)
      ExprKind::Array(elems) | ExprKind::Tup(elems) => elems.iter().any(|x| self.value_borrow_is_mut(x)),
      ExprKind::Repeat(x, _) => self.value_borrow_is_mut(x),
      // `Holder { r: &mut z }` (#692).
      ExprKind::Struct(_, fields, tail) => fields.iter().any(|f| self.value_borrow_is_mut(f.expr))
        || matches!(tail, rustc_hir::StructTailExpr::Base(b) if self.value_borrow_is_mut(b)),
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
    // `let t = { Some(&String::from("x")) };` (#1383).
    if let Some(tail) = plain_block_tail(init) {
      return self.holder_lenders(tail, ty);
    }
    // A branch of `let k = Some(if c { &s } else { &u });` (#1225's
    // constructor around a conditional, paired with k branch by branch,
    // #1457): the reference it gives is what k's value holds, so k
    // borrows what that reference borrows. (A temporary the branch
    // makes, `&String::from("x")`, has no lender until the branch is
    // visited: `add_arm_holder_lenders`.)
    if let Some(rty) = self.tcx.typeck(init.hir_id.owner).expr_ty_opt(init).filter(|t| t.is_ref()) {
      let (lender, _, co) = self.get_ref_data_co(init);
      let found: Vec<ResourceTy> = std::iter::once(lender).chain(co)
        .filter(|l| matches!(l, ResourceTy::Value(_) | ResourceTy::Deref(_)))
        .collect();
      return (found, rty.ref_mutability().is_some_and(|m| m.is_mut()));
    }
    // `let t = if c { Some(&s) } else { None };`, `let a = match .. {
    // .. => [&s], .. => [&u] };` (#1435): t may hold what any branch's
    // value holds, as rustc sees it (`s.push('x')` while t is live is
    // E0502). (Each branch draws its own borrows, `match_rhs`.) A value
    // made from a variable the branch declares, or a temporary it makes,
    // isn't known until the branch is visited: `add_arm_lenders`.
    if let Some(bodies) = holder_branch_bodies(init) {
      let typeck = self.tcx.typeck(init.hir_id.owner);
      let mut out: Vec<ResourceTy> = Vec::new();
      let mut is_mut = false;
      for v in bodies.into_iter().filter_map(branch_value_expr) {
        if typeck.expr_ty_opt(v).is_none_or(|t| t.is_never()) {
          continue;
        }
        let (ls, m) = self.holder_lenders(v, ty);
        is_mut |= m && !ls.is_empty();
        for l in ls {
          if !out.contains(&l) {
            out.push(l);
          }
        }
      }
      return (out, is_mut);
    }
    // `Some(W { a: &s, b: &mut t })`: each lender has its own kind (#708).
    let mixed = self.holder_lender_muts(init);
    if !mixed.is_empty() {
      return (mixed.into_iter().map(|(l, _)| l).collect(), false);
    }
    match init.kind {
      ExprKind::Call(..) | ExprKind::MethodCall(..) => {
        // `let el = refs.last_mut();` / `refs.iter_mut().next()` on a
        // `Vec<&String>`: an `Option` of a reference into refs (#1382,
        // #1395), not of what refs's elements borrow.
        let lenders = self.pointer_lenders(init).unwrap_or_else(|| self.call_lenders(init));
        // `let it = self.items.iter().chain(self.other.iter());`: only
        // those fields of what self points to (#1080).
        match self.field_call_lenders(init, &lenders) {
          Some(fields) => (fields.iter().map(|(l, _)| l.clone()).collect(), fields.iter().any(|(_, m)| *m)),
          None => (lenders, self.call_borrow_is_mut(init)),
        }
      }
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

  /// For a value built from `&` and `&mut` borrows together
  /// (`Some(W { a: &s, b: &mut t })`, a tuple, an array), each
  /// lender with whether it is borrowed mutably. Empty unless both
  /// kinds are present: a holder with one kind records it once.
  pub fn holder_lender_muts(&self, init: &'tcx Expr<'tcx>) -> Vec<(ResourceTy, bool)> {
    let mut out: Vec<(ResourceTy, bool)> = Vec::new();
    // Fields of what a reference points to, borrowed each its own way
    // (`r.other.iter_mut().zip(r.items.iter())`, #1080).
    let ctor = matches!(init.kind, ExprKind::Call(f, _) if matches!(f.kind, ExprKind::Path(QPath::Resolved(_, p))
      if matches!(p.res, rustc_hir::def::Res::Def(rustc_hir::def::DefKind::Ctor(..), _))));
    // `vec![(&mut f, &g)]` (#1433): what the elements the user wrote hold.
    if let Some(elems) = vec_macro_elems(init) {
      if !elems.iter().all(|x| self.collect_lender_muts(x, &mut out)) {
        return Vec::new();
      }
    } else if matches!(init.kind, ExprKind::Call(..) | ExprKind::MethodCall(..)) && !ctor {
      if let Some(fields) = self.field_call_lenders(init, &self.call_lenders(init)) {
        out = fields;
      }
    } else if !self.collect_lender_muts(init, &mut out) {
      return Vec::new();
    }
    if out.iter().any(|(_, m)| *m) && out.iter().any(|(_, m)| !*m) { out } else { Vec::new() }
  }

  /// `holder_lender_muts` for a tuple struct / variant constructor
  /// call (`Some(W { a: &s, b: &mut t })`); empty for any other call.
  fn ctor_mixed_muts(&self, call: &'tcx Expr<'tcx>) -> Vec<(ResourceTy, bool)> {
    match call.kind {
      ExprKind::Call(f, _) if matches!(f.kind, ExprKind::Path(QPath::Resolved(_, p))
        if matches!(p.res, rustc_hir::def::Res::Def(rustc_hir::def::DefKind::Ctor(..), _))) => self.holder_lender_muts(call),
      _ => Vec::new(),
    }
  }

  /// Adds the lenders `e`'s value holds, with their kinds, to `out`
  /// (a lender borrowed both ways counts as mutable). False when `e`
  /// has a part this can't account for.
  fn collect_lender_muts(&self, e: &'tcx Expr<'tcx>, out: &mut Vec<(ResourceTy, bool)>) -> bool {
    let mut add = |ls: Vec<ResourceTy>, m: bool| {
      for l in ls {
        match out.iter_mut().find(|(x, _)| *x == l) {
          Some(entry) => entry.1 |= m,
          None => out.push((l, m)),
        }
      }
    };
    match e.kind {
      ExprKind::Struct(_, fields, tail) => {
        let base = match tail {
          rustc_hir::StructTailExpr::Base(b) => Some(b),
          _ => None,
        };
        fields.iter().map(|f| f.expr).chain(base).all(|x| self.collect_lender_muts(x, out))
      }
      ExprKind::Tup(elems) | ExprKind::Array(elems) => elems.iter().all(|x| self.collect_lender_muts(x, out)),
      ExprKind::Repeat(x, _) => self.collect_lender_muts(x, out),
      ExprKind::Block(b, _) => b.expr.is_none_or(|x| self.collect_lender_muts(x, out)),
      ExprKind::DropTemps(inner) => self.collect_lender_muts(inner, out),
      // A tuple struct / variant constructor (`Some(..)`, `W(..)`).
      ExprKind::Call(f, args) if matches!(f.kind, ExprKind::Path(QPath::Resolved(_, p))
        if matches!(p.res, rustc_hir::def::Res::Def(rustc_hir::def::DefKind::Ctor(..), _))) => {
        args.iter().all(|x| self.collect_lender_muts(x, out))
      }
      ExprKind::AddrOf(_, m, inner) => {
        // (A temporary the `let` extends has its own column, #1357.)
        let ls = match self.nested_ext_temps.get(&e.hir_id) {
          Some(tmp) => vec![ResourceTy::Value(tmp.clone())],
          None => self.place_lenders(inner),
        };
        add(ls, m.is_mut());
        true
      }
      ExprKind::Path(QPath::Resolved(..)) | ExprKind::Field(..) => {
        let m = self.value_borrow_is_mut(e);
        match expr_to_rap_name(e, &self.tcx).and_then(|n| self.borrow_map.get(&n)) {
          Some(rd) => {
            for l in rd.all_lenders() {
              let lm = rd.mut_of(&l);
              add(vec![l], lm);
            }
          }
          None => add(self.value_lenders(e), m),
        }
        true
      }
      // A literal, a call, or anything else holding no borrow we track.
      _ => self.value_lenders(e).is_empty(),
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
  fn arg_lenders(&self, flow: &ArgFlow<'tcx>) -> Vec<ResourceTy> {
    let ArgFlow { arg, place, content, by_value, .. } = *flow;
    let fields = flow.fields.as_deref();
    if by_value {
      // Passed by value: whatever the value holds, including the
      // borrow of a reference passed for a type parameter, or what
      // the fields of a struct drawn as field columns hold.
      return if content {
        let ls = self.value_lenders(arg);
        if ls.is_empty() { self.struct_content_lenders(arg, fields) } else { ls }
      } else {
        Vec::new()
      };
    }
    let typeck = self.tcx.typeck(arg.hir_id.owner);
    let adjs = typeck.expr_adjustments(arg);
    let mut res = Vec::new();
    if let Some(bpos) = adjs.iter().position(|a| matches!(a.kind, Adjust::Borrow(_))) {
      // The argument is re-borrowed (auto-ref): walk the deref steps
      // before the borrow to find the place being borrowed.
      let mut referent = Referent::Place(arg);
      let mut ty = typeck.expr_ty(arg);
      for a in &adjs[..bpos] {
        let from_ty = std::mem::replace(&mut ty, a.target);
        if let Adjust::Deref(kind) = a.kind {
          // A `Box` is dereferenced by a built-in deref too (`f(&b)` with
          // `b: Box<String>` and `f(&String)`): like an overloaded one, it
          // stays inside the same owner's data, b's (#1505).
          if from_ty.is_box() {
            continue;
          }
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
          // `r.iter_mut()` / `r.first_mut()` with r a local `&mut`: the
          // result reborrows through r, like `&mut *r`, so r can't be
          // used while it is live (#1152).
          Referent::Through(e) if flow.place_mut && self.local_mut_ref(e).is_some() =>
            vec![ResourceTy::Deref(self.local_mut_ref(e).expect("checked"))],
          Referent::Through(e) => self.value_lenders(e),
          Referent::Unknown => Vec::new(),
        });
      }
      if content {
        match referent {
          Referent::Place(e) => res.extend(self.content_lenders(e, fields)),
          // `rr.top()` with `rr: &Vec<&str>`: what rr points to.
          Referent::Through(e) => res.extend(self.pointee_content_lenders(e, fields)),
          Referent::Unknown => {}
        }
      }
    } else if typeck.expr_ty_adjusted(arg).is_ref() {
      // A reference passed as is (`longest(x, y)`, `f(&s)`).
      if place {
        res.extend(self.value_lenders(arg));
      }
      if content {
        match arg.kind {
          ExprKind::AddrOf(_, _, inner) => res.extend(self.content_lenders(inner, fields)),
          // A reference variable (`inner(rr)` with `rr = &rows`):
          // the references stored in what it points to (#1089).
          _ => res.extend(self.pointee_content_lenders(arg, fields)),
        }
      }
    } else if content {
      // A value passed by value whose type holds references
      // (`opt.unwrap()` with `opt: Option<&T>`).
      res.extend(self.value_lenders(arg));
    }
    res
  }

  /// The local `&mut` reference variable r (see `local_mut_ref`) that
  /// `e` reborrows: `&mut *r`, `&*r`, or `r` itself where rustc inserts
  /// the reborrow (`is_implicit_reborrow`).
  fn reborrowed_local_mut_ref(&self, e: &'tcx Expr<'tcx>) -> Option<ResourceAccessPoint> {
    match e.kind {
      ExprKind::AddrOf(_, _, inner) => match inner.kind {
        ExprKind::Unary(UnOp::Deref, r) => self.local_mut_ref(r),
        _ => None,
      },
      ExprKind::Path(_) if is_implicit_reborrow(e, self.tcx.typeck(e.hir_id.owner)) => self.local_mut_ref(e),
      _ => None,
    }
  }

  /// `e` names a local `&mut` reference variable that borrows a
  /// variable with a column (not a parameter, whose column already
  /// stands for the caller's data, #393), or reborrows through another
  /// reference (`let m2 = &mut *m;`): a call given `&*m2` borrows
  /// through m2, not through m, which m2 still holds (#1415).
  fn local_mut_ref(&self, e: &'tcx Expr<'tcx>) -> Option<ResourceAccessPoint> {
    if !matches!(e.kind, ExprKind::Path(QPath::Resolved(None, _))) {
      return None;
    }
    let name = expr_to_rap_name(e, &self.tcx)?;
    let r = self.raps.get(&name)?.rap.to_owned();
    let rd = self.borrow_map.get(&name)?;
    let lends = match &rd.lender {
      ResourceTy::Value(l) => !l.is_ref() && !l.is_fn(),
      ResourceTy::Deref(l) => l.is_ref() && l != &r,
      _ => false,
    };
    (r.is_mutref() && lends).then_some(r)
  }

  /// What the reference(s) held by the *value* of `e` borrow from.
  /// `e` (a `&mut` variable) is an argument of a call that takes it
  /// by value without a reborrow (a generic `T` parameter): it is
  /// moved into the call (#200, #1128).
  /// `mut_ref_moved_into_call`, for a reference parameter `e`, whose
  /// column stands for the caller's data.
  pub fn mut_ref_param_moved_into_call(&self, e: &'tcx Expr<'tcx>) -> bool {
    matches!(e.kind, ExprKind::Path(QPath::Resolved(..)))
      && expr_to_rap_name(e, &self.tcx).and_then(|n| self.borrow_map.get(&n))
        .is_some_and(|rd| matches!(rd.lender, ResourceTy::Caller) && rd.co_lenders.is_empty())
      && self.mut_ref_moved_into_call(e)
  }

  fn mut_ref_moved_into_call(&self, e: &'tcx Expr<'tcx>) -> bool {
    let typeck = self.tcx.typeck(e.hir_id.owner);
    // (`look(r)` with `look(x: &T)` reborrows `&*r` too, #1329.)
    if !typeck.expr_ty(e).ref_mutability().is_some_and(|m| m.is_mut()) || is_implicit_reborrow(e, typeck) {
      return false;
    }
    match self.tcx.parent_hir_node(e.hir_id) {
      rustc_hir::Node::Expr(p) => match p.kind {
        ExprKind::Call(_, args) => args.iter().any(|a| a.hir_id == e.hir_id),
        ExprKind::MethodCall(_, _, args, _) => args.iter().any(|a| a.hir_id == e.hir_id),
        // An array element (`[r]`, what `vec![r]` expands to) is no
        // coercion site either: r is moved in (#1289).
        ExprKind::Array(_) => true,
        _ => false,
      },
      _ => false,
    }
  }

  pub fn value_lenders(&self, e: &'tcx Expr<'tcx>) -> Vec<ResourceTy> {
    // A local `&mut` r reborrowed: `&mut *r` / `&*r`, or `r` where rustc
    // inserts that (an element of `[a, b]` after the first, coerced to
    // its type; `Some(r)` where an `Option<&T>` is expected). The value
    // holds a borrow through r, like `let k = &mut *r;`, so r can't be
    // used (or is read-only) while it is live; r still holds its own
    // borrow (#1329, #1330).
    if let Some(r) = self.reborrowed_local_mut_ref(e) {
      return vec![ResourceTy::Deref(r)];
    }
    match e.kind {
      ExprKind::Path(QPath::Resolved(..)) | ExprKind::Field(..) => {
        let name = expr_to_rap_name(e, &self.tcx);
        match name.as_ref().and_then(|n| self.borrow_map.get(n)) {
          // A reference parameter (`v: &Vec<i32>`): what it points to
          // is the caller's data, which has no column here. v's own
          // column stands for it, as it already does for `&v[0]`, so
          // `v.iter()` / `first_word(s)` borrow "from v" (#393).
          // A `&mut` parameter moved into a call (`id(r)` with a generic
          // parameter: no reborrow) hands its reference over: the
          // result points to the caller's data, not into r (#1205).
          Some(rd) if matches!(rd.lender, ResourceTy::Caller) && rd.co_lenders.is_empty()
            && self.mut_ref_moved_into_call(e) => rd.all_lenders(),
          Some(rd) if matches!(rd.lender, ResourceTy::Caller) && rd.co_lenders.is_empty() => {
            match name.as_ref().and_then(|n| self.raps.get(n)) {
              Some(r) if r.rap.is_ref() => vec![ResourceTy::Value(r.rap.to_owned())],
              _ => rd.all_lenders(),
            }
          }
          // (A holder's lenders, and what it reaches through them:
          // `it.copied()` on `it = refs.iter()` holds a1's borrow, #1399.)
          Some(rd) => rd.reached_lenders(),
          // A non-reference variable (`opt: Option<&T>`, an iterator)
          // whose borrow we don't track (holders are in borrow_map).
          None => Vec::new(),
        }
      }
      // `&String::from("b")` / `&make_user().name` in `let t =
      // Some(..)` / `[..]`: a temporary the `let` extends, with a
      // column of its own (#856, #1357).
      ExprKind::AddrOf(_, _, inner) => match self.nested_ext_temps.get(&e.hir_id) {
        Some(tmp) => vec![ResourceTy::Value(tmp.clone())],
        None => self.place_lenders(inner),
      },
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
      // A struct literal holds its fields' borrows (`Some(Part { part:
      // &s })` holds `s`'s; #692), and those of the fields it takes
      // from the base of a struct update; a tuple literal inside a value,
      // its elements' (`vec![(&f, &g)]`, #1433; `Some((&a, &b))`, #1459).
      // (A tuple bound by `let` is drawn as its fields' columns instead.)
      // A holder records one kind of borrow, so a literal mixing `&s`
      // and `&mut k` is left untracked here (`holder_lender_muts` has
      // each lender's kind).
      ExprKind::Struct(..) | ExprKind::Tup(..) => {
        let mut out: Vec<ResourceTy> = Vec::new();
        let parts: Vec<&'tcx Expr<'tcx>> = match e.kind {
          ExprKind::Struct(_, fields, tail) => fields.iter().map(|f| f.expr)
            .chain(match tail {
              rustc_hir::StructTailExpr::Base(b) => Some(b),
              _ => None,
            })
            .collect(),
          ExprKind::Tup(elems) => elems.iter().collect(),
          _ => Vec::new(),
        };
        let mut kinds: Vec<bool> = Vec::new();
        for e in parts {
          let ls = self.value_lenders(e);
          if !ls.is_empty() {
            kinds.push(self.value_borrow_is_mut(e));
          }
          for l in ls {
            if !out.contains(&l) {
              out.push(l);
            }
          }
        }
        if kinds.iter().any(|m| *m) && kinds.iter().any(|m| !*m) {
          return Vec::new();
        }
        out
      }
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
        // `*r` with r a local `&mut`: a borrow of this place (`&*r`
        // passed to `f(x: &T)`, the receiver of `(*r).as_str()`)
        // reborrows through r, as `&*r` does in a `let` (#1332).
        if let Some(r) = self.local_mut_ref(inner) {
          vec![ResourceTy::Deref(r)]
        } else if typeck.expr_ty(inner).is_ref() {
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
    // `r.name` / `r[..1]` with r a local `&mut`: the place is reached
    // through r, so a borrow of it reborrows through r (#1332).
    if let Some(r) = self.local_mut_ref(base) {
      vec![ResourceTy::Deref(r)]
    } else if typeck.expr_ty(base).is_ref() {
      self.value_lenders(base)
    } else {
      self.place_lenders(base)
    }
  }

  /// What the references stored *inside* the value at place `p`
  /// borrow from. `fields`: see `ArgFlow::fields`.
  fn content_lenders(&self, p: &'tcx Expr<'tcx>, fields: Option<&[String]>) -> Vec<ResourceTy> {
    let typeck = self.tcx.typeck(p.hir_id.owner);
    match p.kind {
      ExprKind::Call(..) | ExprKind::MethodCall(..) | ExprKind::Block(..) => self.value_lenders(p),
      _ if typeck.expr_ty(p).is_ref() => self.value_lenders(p),
      // `&*rr`: what rr points to (#1089); `&*b` with b a `Box`: b's
      // own data.
      ExprKind::Unary(UnOp::Deref, inner) => {
        if typeck.expr_ty(inner).is_ref() {
          self.pointee_content_lenders(inner, fields)
        } else {
          self.content_lenders(inner, fields)
        }
      }
      // A local `Option<&T>` / iterator that holds a borrow we track
      // (#336), or a struct field drawn as such a holder
      // (`shelf.items`, #1088): `value_lenders` finds it in
      // `borrow_map`. A struct drawn as field columns holds what its
      // fields hold (`shelf.top()`).
      ExprKind::Path(QPath::Resolved(..)) | ExprKind::Field(..) => match expr_to_rap_name(p, &self.tcx)
        .and_then(|n| self.raps.get(&n))
      {
        Some(rd) => self.rap_content_lenders(&rd.rap, fields),
        None => Vec::new(),
      },
      // Any other value: not tracked.
      _ => Vec::new(),
    }
  }

  /// `content_lenders` for `*e`, where `e` is a reference: the
  /// references stored in the variables `e` points to (`rows`, for
  /// `rr = &rows`). Empty when what `e` points to has no column.
  fn pointee_content_lenders(&self, e: &'tcx Expr<'tcx>, fields: Option<&[String]>) -> Vec<ResourceTy> {
    let mut out: Vec<ResourceTy> = Vec::new();
    for l in self.value_lenders(e) {
      let ResourceTy::Value(rap) = &l else { continue };
      for c in self.rap_content_lenders(rap, fields) {
        if !out.contains(&c) {
          out.push(c);
        }
      }
    }
    out
  }

  /// `struct_content_lenders` for a struct value passed by value
  /// (`take(shelf)`): what its field columns hold.
  fn struct_content_lenders(&self, e: &'tcx Expr<'tcx>, fields: Option<&[String]>) -> Vec<ResourceTy> {
    if !matches!(e.kind, ExprKind::Path(QPath::Resolved(..)) | ExprKind::Field(..)) {
      return Vec::new();
    }
    match expr_to_rap_name(e, &self.tcx).and_then(|n| self.raps.get(&n)) {
      Some(rd) if matches!(rd.rap, ResourceAccessPoint::Struct(_)) && !rd.rap.holds_borrow() => self.rap_content_lenders(&rd.rap, fields),
      _ => Vec::new(),
    }
  }

  /// What the references held by the value in column `rap` borrow
  /// from: a holder's recorded borrow (`names`, `shelf.items`), or for
  /// a struct drawn as field columns, what its reference and holder
  /// fields borrow (only those named in `fields`, when given).
  fn rap_content_lenders(&self, rap: &ResourceAccessPoint, fields: Option<&[String]>) -> Vec<ResourceTy> {
    let lenders_of = |name: &str| -> Vec<ResourceTy> {
      self.borrow_map.get(name).map(|rd| rd.reached_lenders()).unwrap_or_default()
    };
    if rap.holds_borrow() {
      return lenders_of(rap.name());
    }
    if !matches!(rap, ResourceAccessPoint::Struct(_)) {
      return Vec::new();
    }
    let prefix = format!("{}.", rap.name());
    let mut out: Vec<ResourceTy> = Vec::new();
    for m in self.struct_members_deep(rap.name()) {
      if !(m.is_ref() || m.holds_borrow()) {
        continue;
      }
      let top = m.name()[prefix.len()..].split('.').next().unwrap_or_default();
      if fields.is_some_and(|fs| !fs.iter().any(|f| f == top)) {
        continue;
      }
      for l in lenders_of(m.name()) {
        if matches!(l, ResourceTy::Value(_) | ResourceTy::Deref(_)) && !out.contains(&l) {
          out.push(l);
        }
      }
    }
    out
  }
}

/// Whether one of the type parameters `params` occurs in `t` other than
/// behind a reference: `T` in `Option<T>` or `Self`, not in
/// `Option<&T>` (see `call_reborrows_through`).
fn param_outside_refs<'tcx>(t: Ty<'tcx>, params: &[u32]) -> bool {
  let mut walk = t.walk();
  while let Some(g) = walk.next() {
    let GenericArgKind::Type(t) = g.kind() else { continue };
    match t.kind() {
      ty::Ref(..) => walk.skip_current_subtree(),
      ty::Param(p) if params.contains(&p.index) => return true,
      _ => {}
    }
  }
  false
}

/// The lifetimes and type parameters of `ty` outside what its
/// references point to (`'a` and `U` in `Option<(&'a mut T, U)>`), for
/// `FlowTarget::Pointer`; with `inside`, those inside what they point
/// to instead (`T`'s), for `FlowTarget::Pointee`. A type this doesn't
/// take apart (a trait object, a projection, a function pointer) gives
/// everything in it either way.
fn outside_pointees<'tcx>(ty: Ty<'tcx>, out: &mut Vec<ty::GenericArg<'tcx>>, inside: bool) {
  match ty.kind() {
    ty::Ref(r, pointee, _) => {
      if inside {
        out.extend(pointee.walk());
      } else {
        out.push((*r).into());
      }
    }
    ty::Adt(_, args) => {
      for a in args.iter() {
        match a.kind() {
          GenericArgKind::Type(t) => outside_pointees(t, out, inside),
          GenericArgKind::Lifetime(_) if !inside => out.push(a),
          _ => {}
        }
      }
    }
    ty::Tuple(tys) => tys.iter().for_each(|t| outside_pointees(t, out, inside)),
    ty::Array(t, _) | ty::Slice(t) => outside_pointees(*t, out, inside),
    ty::Param(_) if !inside => out.push(ty.into()),
    ty::Param(_) => {}
    _ => out.extend(ty.walk()),
  }
}

/// The branch bodies of the `if` / `else` or `match` `init` (through
/// nested ones), when it is one a `let` holder is bound from (#1435).
pub fn holder_branch_bodies<'tcx>(init: &'tcx Expr<'tcx>) -> Option<Vec<&'tcx Expr<'tcx>>> {
  if init.span.from_expansion()
    || !matches!(init.kind, ExprKind::If(_, _, Some(_)) | ExprKind::Match(_, _, rustc_hir::MatchSource::Normal)) {
    return None;
  }
  let mut bodies = Vec::new();
  crate::visitor::conditional_branch_bodies(init, &mut bodies);
  Some(bodies)
}
