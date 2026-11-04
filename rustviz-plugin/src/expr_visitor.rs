//! This file details all the datastructures that are filled while traversing the HIR.
//! Essentially we care about detailing interactions and relationships between 
//! Resource Access Points (RAPs - RV1 terminology)
//! The frontend (svg-generator crate) requires a list of external events:
//! which are generally events that cause some visually external effect on the visualization
//! For example: Moves, Copies, Borrows are all represented with arrows between timelines.


use log::{info, warn};
use rustc_middle::{
  mir::Body,
  ty::*,
};
use rustc_hir::{Expr, ExprKind, QPath, PatKind, Mutability, UnOp, def::*, Pat};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use crate::expr_visitor_utils::*;
use crate::svg_generator::data::*;
use rustc_borrowck::consumers::BodyWithBorrowckFacts;


// A struct to help with appending ExternalEvents
#[derive(Debug, Clone)]
pub enum Evt { 
  // A RAP is bound to some resource (this is usually used when the resource is anonymous)
  // ex: let a = 9;
  // technically all variables are bound at initialization, but we don't use the Bind event
  // for each scenario in which this happens.
  Bind,

  Copy, // let x: i32 = y; | let x: &i32 = y;
  Move, // let x:String = y;
  SBorrow, // let x: &i32 = &y;
  MBorrow, // let x: & mut i32 = & mut y;

  // When a RAP returns an immutably borrowed resource
  SDie,

  // When a RAP returns a mutably boirrowed resource
  MDie,

  // Passing a RAP by static (immutable) reference
  PassBySRef,

  // Passing a RAP by mutable reference
  PassByMRef, // String::push_str(& mut s, "text") | s.push_str("text")
}

// RefData struct is used to represent a loan between a borrower and lender 
// at any time in the program lifetime. Ideally we should use the MIR and just grab 
// the list of borrows that occurs and append events accordingly, implementing our own
// logic is more difficult and not sufficient for complex borrowing scenarios.
/// Where a struct's member columns are being registered (see
/// `register_struct_members`): the HIR owner used for type queries, the
/// line the value is bound on, the scope and globalness the members
/// share with their struct, and the lender recorded for its
/// reference-typed fields (`None`: no lender is known, see the
/// pattern-binding site in `get_dec_of_pat2`).
#[derive(Clone)]
pub struct MemberSite {
  pub owner_id: rustc_hir::OwnerId,
  pub assigned_at: usize,
  pub scope: usize,
  pub is_global: bool,
  pub ref_lender: Option<ResourceTy>,
}

#[derive(Debug, Clone)]
pub struct RefData {
  pub lender: ResourceTy, // who this reference borrowed from
  pub assigned_at: usize, // gen point
  pub lifetime: usize, // kill point
  pub ref_mutability: bool, // type
  pub aliasing: VecDeque<String>, // aliasing other references
  // Set when `lender` is Anonymous but the reference is known to
  // borrow data that lives for the whole program (a string literal,
  // a static); the borrow-end arrow then names it.
  pub static_lender: Option<StaticLender>,
  /// Span of the `&x` / `&mut x` expression this reference was
  /// assigned from, when it was one. `print_lifetimes` uses it to
  /// pick this reference's own loan out of MIR's borrow data; two
  /// references borrowing the same variable on one line (`let a = &x;
  /// let b = &x;`) are otherwise indistinguishable (#233).
  pub borrow_span: Option<rustc_span::Span>,
}

#[derive(Debug, Clone)]
pub struct RapData {
  pub rap: ResourceAccessPoint,
  pub scope: usize,
  pub is_global: bool,
  // Line of the fn signature where this RAP was registered. Drives
  // per-fn label and column placement in the renderer; identical
  // RAPs across fns get distinct hashes so `BTreeMap<u64, Timeline>`
  // already disambiguates.
  pub fn_start_line: usize,
}

/// A `let` binding with no initializer (`let x;`), remembered until
/// its first assignment so that assignment can register the RAP with
/// the `let`'s scope and render as initialization (#234).
#[derive(Debug, Clone, Copy)]
pub struct DeferredLet {
  pub annotation: rustc_hir::BindingMode,
  pub scope: usize,
  pub inside_branch: bool,
  pub is_skipped: bool,
}


pub struct ExprVisitor<'a, 'tcx:'a> {
  pub tcx: TyCtxt<'tcx>, // type context
  pub mir_body: &'a Body<'tcx>, 
  pub hir_body: &'a rustc_hir::Body<'tcx>,
  pub bwf: &'a BodyWithBorrowckFacts<'tcx>,

  // Data structure used to represent active loans
  // borrower name -> RefData
  pub borrow_map: HashMap<String, RefData>,

  // RAPs registered in *this* fn body. Each `visit_body` call
  // owns its own map so two fns with the same variable name
  // (e.g. both have `let x = …`) don't clobber each other; the
  // shared `rap_hashes` counter still gives every RAP a globally
  // unique hash, so the rendering pipeline (which keys on hash)
  // sees them as distinct. After the visit, plugin.rs can drain
  // this map into any cross-fn structure it needs.
  pub raps: HashMap<String, RapData>,

  // Used to determine the current scope when visiting expressions
  pub current_scope: usize,

  // Line of the current fn's signature; written into each new
  // RapData so the renderer can group columns + place labels
  // per-fn. Set once in visit_body.
  pub current_fn_start: usize,

  // The event line map stores events that will
  // result in the generation of an arrow by the frontend 
  // Although it's somewhat redundant (events from the preprocessed events)
  // appear in the event_line_map, it's 'necessary' to figure out the arrow orientation
  pub event_line_map: &'a mut BTreeMap<usize, Vec<ExternalEvent>>,

  // Just a list of the events and on which line they occur
  pub preprocessed_events: &'a mut Vec<(usize, ExternalEvent)>,
  
  // These members are necessary for annotated_src computation
  pub rap_hashes: usize,
  pub source_map: & 'a BTreeMap<usize, String>,
  pub annotated_lines: & 'a mut BTreeMap<usize, Vec<String>>,
  pub id_map: & 'a mut HashMap<String, usize>,
  pub unique_id: & 'a mut usize,


  pub inside_branch: bool,
  pub fn_ret: bool,

  // Source lines (1-indexed) carrying a `// rustviz: skip` marker.
  // Used by visit_local to suppress events for marked `let`s and by
  // plugin.rs to bypass body traversal of marked fns. Owned by the
  // plugin's RV1Helper; visitors get a borrow for the duration of
  // each fn-body walk.
  pub skip_lines: &'a std::collections::HashSet<usize>,

  // Names of RAPs registered for `let` bindings that landed on a
  // skip line. We still register the RAP so any non-skipped use of
  // the variable elsewhere can resolve, but every event whose source
  // or destination references one of these names is dropped before
  // it reaches the renderer — which is what gives the variable no
  // timeline column.
  pub skip_raps: std::collections::HashSet<String>,

  // Locals declared without an initializer (`let x;`) that are still
  // definitely uninitialized at this point of the walk, keyed by the
  // binding's HirId (#234). An assignment to one of these initializes
  // it: the RAP is registered there, and nothing is dropped. The If
  // and Match arms start every branch from the same set and keep only
  // what is still uninitialized at the end of each branch, so after
  // `if c { s = a } else { s = b }` a later `s = …` is a reassignment.
  pub uninit_locals: HashMap<rustc_hir::HirId, DeferredLet>,

  // For `let x = if … { a } else { b };` (and `match`), the events that
  // pair each branch's value with `x` are emitted before the branches
  // are visited (see `process_let_binding`). Keyed by the branch body's
  // HirId: the range of event ids `match_rhs` used for that branch, so
  // the If / Match arm of `visit_expr` can put those events in the
  // right branch (`branch_ids`).
  pub rhs_arm_ids: HashMap<rustc_hir::HirId, (usize, usize)>,

  // Ids of the GoOutOfScope events emitted at a `return` inside a
  // branch for the values still owned there (#363). They make their
  // variables live in that branch (an ordinary scope-end GoOutOfScope
  // doesn't), and are kept only in a branch that diverges: see
  // `keep_exit_drops`.
  pub exit_drops: std::collections::HashSet<usize>,
}

impl<'a, 'tcx> ExprVisitor<'a, 'tcx>{
  pub fn return_type_of(&self,fn_expr:&Expr)->Option<Ty<'tcx>>{
    let type_check = self.tcx.typeck(fn_expr.hir_id.owner);
    let type_of_path = type_check.expr_ty(fn_expr);
    let mut fn_sig = type_of_path.fn_sig(self.tcx).skip_binder().output().walk();
    if let Some(return_type)= fn_sig.next(){
      Some(return_type.expect_ty())
    }
    else {
      None
    }
  }

  pub fn is_return_type_ref(&self,fn_expr:&Expr) -> bool{
    if let Some(return_type)=self.return_type_of(fn_expr){
      return_type.is_ref()
    }
    else{
      false
    }
  }

  pub fn is_return_type_copyable(&self,fn_expr:&Expr)->bool{
    if let Some(return_type)=self.return_type_of(fn_expr){
      if return_type.walk().fold(false,|flag,item|{flag||item.expect_ty().is_ref()}) {
        false
      }
      else{
        self.tcx.type_is_copy_modulo_regions(rustc_middle::ty::TypingEnv::post_analysis(self.tcx, fn_expr.hir_id.owner), return_type)
      }
    }
    else{
      false
    }
  }

  // updates the lifetime of a loan (as well as for each of the aliases associated with this loan)
  pub fn update_lifetime(&mut self, name: &String, line:usize){
    self.borrow_map.get_mut(name).unwrap().lifetime = line;
    let aliasing = self.borrow_map.get(name).unwrap().aliasing.clone();
    for r in aliasing.iter() {
      self.update_lifetime(r, line);
    }
  }

  /// True iff `ty` implements Copy in the context of the given
  /// owner DefId (typically `expr.hir_id.owner` or
  /// `pat.hir_id.owner`). Centralised so all RAP-construction sites
  /// agree on the predicate used to populate `Owner::is_copy` /
  /// `Struct::is_copy`.
  pub fn ty_is_copy(&self, ty: Ty<'tcx>, owner: rustc_hir::OwnerId) -> bool {
    self.tcx.type_is_copy_modulo_regions(
      rustc_middle::ty::TypingEnv::post_analysis(self.tcx, owner),
      ty,
    )
  }

  /// True iff a value of type `ty` can be mutated through a shared
  /// reference: `Cell`, `RefCell`, `Mutex`, atomics, or a type that
  /// contains one (`Rc<RefCell<T>>`, `Vec<Cell<T>>`, a local struct
  /// with such a field). Populates `Owner::interior_mut` /
  /// `Struct::interior_mut`, which keeps the lend-dot tooltip from
  /// calling an immutably borrowed `RefCell` "read-only".
  ///
  /// Conservative in the direction that matters: it descends into
  /// generic arguments of every ADT and into the fields of local
  /// ADTs, but not into private fields of foreign types, so `Rc<T>`'s
  /// own refcount cells don't count. A false `true` only drops the
  /// "read-only" clause; a false `false` would state something
  /// untrue, so foreign types that hide a cell behind a pointer are
  /// the known gap.
  pub fn ty_has_interior_mut(&self, ty: Ty<'tcx>, owner: rustc_hir::OwnerId) -> bool {
    fn go<'tcx>(tcx: TyCtxt<'tcx>, env: rustc_middle::ty::TypingEnv<'tcx>, ty: Ty<'tcx>,
                depth: usize, seen: &mut HashSet<Ty<'tcx>>) -> bool {
      if depth > 8 || !seen.insert(ty) { return false; }
      if !ty.is_freeze(tcx, env) { return true; }
      match ty.kind() {
        rustc_middle::ty::Adt(def, args) => {
          args.types().any(|t| go(tcx, env, t, depth + 1, seen))
            || (def.did().is_local()
                && def.all_fields().any(|f| go(tcx, env, f.ty(tcx, args), depth + 1, seen)))
        }
        rustc_middle::ty::Ref(_, t, _) | rustc_middle::ty::Array(t, _)
        | rustc_middle::ty::Slice(t) => go(tcx, env, *t, depth + 1, seen),
        rustc_middle::ty::Tuple(ts) => ts.iter().any(|t| go(tcx, env, t, depth + 1, seen)),
        _ => false,
      }
    }
    let env = rustc_middle::ty::TypingEnv::post_analysis(self.tcx, owner);
    go(self.tcx, env, ty, 0, &mut HashSet::new())
  }

  // Adds an owner RAP
  pub fn add_owner(&mut self, name: String, mutability: bool, is_copy: bool, interior_mut: bool, scope: usize, is_global: bool) {
    self.add_rap(ResourceAccessPoint::Owner(Owner{name: name, hash: self.rap_hashes as u64, is_mut: mutability, is_copy, interior_mut, is_closure: false, is_union: false, is_manually_drop: false, write: Default::default()}), scope, is_global);
  }

  // Adds an owner RAP for a `union`-typed binding (one opaque column;
  // see the AdtKind::Union arms).
  pub fn add_union_owner(&mut self, name: String, mutability: bool, is_copy: bool, interior_mut: bool, scope: usize, is_global: bool) {
    self.add_rap(ResourceAccessPoint::Owner(Owner{name: name, hash: self.rap_hashes as u64, is_mut: mutability, is_copy, interior_mut, is_closure: false, is_union: true, is_manually_drop: false, write: Default::default()}), scope, is_global);
  }

  // Marks the owner (or struct field) just registered as `name` as a
  // `ManuallyDrop<T>` when `ty` is one, so its scope end and
  // reassignments don't claim to drop the `T` (#283). Call right after
  // `add_owner` / `add_struct`, before any event clones the RAP. A name
  // that was already registered (and so not re-added by `add_rap`) is
  // left alone.
  pub fn mark_if_manually_drop(&mut self, name: &str, ty: Ty<'tcx>) {
    if !ty.ty_adt_def().map_or(false, |d| d.is_manually_drop()) {
      return;
    }
    let Some(just_added) = (self.rap_hashes as u64).checked_sub(1) else { return };
    match self.raps.get_mut(name).map(|rd| &mut rd.rap) {
      Some(ResourceAccessPoint::Owner(o)) if o.hash == just_added => o.is_manually_drop = true,
      Some(ResourceAccessPoint::Struct(s)) if s.hash == just_added => s.is_manually_drop = true,
      _ => {}
    }
  }

  // Adds an owner RAP that binds a closure literal. Routed through
  // a separate constructor (rather than an extra arg on add_owner) so
  // we don't churn every call site — closures are the only producer.
  pub fn add_closure_owner(&mut self, name: String, mutability: bool, scope: usize, is_global: bool) {
    self.add_rap(ResourceAccessPoint::Owner(Owner{name: name, hash: self.rap_hashes as u64, is_mut: mutability, is_copy: false, interior_mut: false, is_closure: true, is_union: false, is_manually_drop: false, write: Default::default()}), scope, is_global);
  }

  // Adds a reference RAP
  // `pointee_copy`: the referent type is Copy (see `ref_pointee_is_copy`).
  pub fn add_ref(&mut self, name: String, ref_mutability: bool, lhs_mut: bool, line_num: usize, lender: ResourceTy, alia: VecDeque<String>, scope: usize, is_global: bool, pointee_copy: bool) {
    match ref_mutability {
      true => {
        self.add_mut_ref_member(name.clone(), lhs_mut, scope, is_global, None, pointee_copy);
      }
      false => { 
        self.add_static_ref_member(name.clone(), lhs_mut, scope, is_global, None, pointee_copy);
      }
    }
    self.borrow_map.insert(name.clone(), RefData { lender: lender.clone(), assigned_at: line_num, lifetime: line_num, ref_mutability: ref_mutability, aliasing: alia, static_lender: None, borrow_span: None });
  }
  
  /// True iff `ref_ty` is `&T` / `&mut T` with `T: Copy` (`&i32`).
  /// Such a reference points to a "value", not a "resource" (#312).
  pub fn ref_pointee_is_copy(&self, ref_ty: Ty<'tcx>, owner: rustc_hir::OwnerId) -> bool {
    match ref_ty.kind() {
      TyKind::Ref(_, inner, _) => self.ty_is_copy(*inner, owner),
      _ => false,
    }
  }

  /// `add_static_ref` with optional struct-membership. Pass
  /// `Some(parent_hash)` for ref-typed struct fields so the
  /// layout pass groups them under their parent's bounding box.
  pub fn add_static_ref_member(
    &mut self,
    name: String,
    mutability: bool,
    scope: usize,
    is_global: bool,
    member_of: Option<u64>,
    pointee_copy: bool,
  ) {
    self.add_rap(
      ResourceAccessPoint::StaticRef(StaticRef {
        name,
        hash: self.rap_hashes as u64,
        is_mut: mutability,
        pointee_copy,
        member_of,
      }),
      scope, is_global,
    );
  }

  pub fn add_mut_ref_member(
    &mut self,
    name: String,
    mutability: bool,
    scope: usize,
    is_global: bool,
    member_of: Option<u64>,
    pointee_copy: bool,
  ) {
    self.add_rap(
      ResourceAccessPoint::MutRef(MutRef {
        name,
        hash: self.rap_hashes as u64,
        is_mut: mutability,
        pointee_copy,
        member_of,
        write: Default::default(),
      }),
      scope, is_global,
    );
  }

  // Adds a function RAP
  pub fn add_fn(&mut self, name: String) {
    self.add_rap(ResourceAccessPoint::Function(Function { name: name, hash: self.rap_hashes as u64, drops_arg: Default::default() }), self.current_scope, true);
  }

  // Adds a struct RAP
  // To be honest this struct logic is leftover from RV1 and should eventually be reworked
  // It doesn't make much sense that a struct should be represented any different than an owner
  // the reason for this is that in the frontend structs are visualized differently in the timeline header
  pub fn add_struct(&mut self, name: String, owner: u64, mem: bool, mutability: bool, is_copy: bool, interior_mut: bool, scope: usize, is_global: bool) {
    self.add_rap(ResourceAccessPoint::Struct(Struct {
      name: name,
      hash: self.rap_hashes as u64,
      owner: owner,
      is_mut: mutability,
      is_member: mem,
      is_copy,
      interior_mut,
      is_manually_drop: false }),
      scope, is_global);
  }

  pub fn add_rap(&mut self, r: ResourceAccessPoint, scope: usize, is_global: bool) {
    let fn_start_line = self.current_fn_start;
    // Functions are keyed in their own namespace (see `fn_key`), so a
    // variable and a function with the same name get separate RAPs.
    let key = if r.is_fn() { fn_key(r.name()) } else { r.name().to_string() };
    self.raps.entry(key).or_insert_with(|| {
      self.rap_hashes += 1;
      RapData { rap: r, scope, is_global, fn_start_line }
    });
  }

  pub fn update_rap(&mut self, r: &ResourceAccessPoint, line_num: usize) {
    if r.is_ref() {
      self.update_lifetime(r.name(), line_num);
    }
  }

  /// True iff `event` references a RAP whose name is in `skip_raps`.
  /// Covers every variant of ExternalEvent — `ResourceAccessPoint_extract`
  /// returns Anonymous for `GoOutOfScope` / `OwnerDropAtReassign` /
  /// `InitRefParam` / `RefDie`, so we match them explicitly here
  /// instead of relying on it.
  fn event_touches_skipped(&self, event: &ExternalEvent) -> bool {
    let touches_ty = |r: &ResourceTy| -> bool {
      match r {
        ResourceTy::Value(rap) | ResourceTy::Deref(rap) | ResourceTy::Index(rap, _, _) => {
          self.skip_raps.contains(rap.name())
        }
        _ => false,
      }
    };
    match event {
      ExternalEvent::Bind { from, to, .. }
      | ExternalEvent::Copy { from, to, .. }
      | ExternalEvent::Move { from, to, .. }
      | ExternalEvent::StaticBorrow { from, to, .. }
      | ExternalEvent::StaticDie { from, to, .. }
      | ExternalEvent::MutableBorrow { from, to, .. }
      | ExternalEvent::MutableDie { from, to, .. }
      | ExternalEvent::RefDie { from, to, .. }
      | ExternalEvent::PassByStaticReference { from, to, .. }
      | ExternalEvent::PassByMutableReference { from, to, .. } => {
        touches_ty(from) || touches_ty(to)
      }
      ExternalEvent::GoOutOfScope { ro, .. }
      | ExternalEvent::ManuallyDropByHand { ro, .. }
      | ExternalEvent::OwnerDropAtReassign { ro, .. } => {
        self.skip_raps.contains(ro.name())
      }
      ExternalEvent::InitRefParam { param, .. } => {
        self.skip_raps.contains(param.name())
      }
      // Branch events bundle child events; we don't recurse into
      // them here. A skipped variable that only appears inside a
      // branch could still leak through — fix when the need arises.
      ExternalEvent::Branch { .. } => false,
    }
  }

  pub fn add_external_event(&mut self, line_num: usize, event: ExternalEvent) {
    // Drop events whose source or destination is a `// rustviz: skip`-
    // marked variable. The marked RAP still exists (so non-skipped
    // code that references it doesn't crash on lookup), but every
    // arrow that touches it disappears — no init Bind, no later
    // Move/Copy/Borrow, no end-of-scope return — and because the
    // renderer only materialises a timeline when a hash sees its
    // first event, a RAP with zero events never gets a column.
    if self.event_touches_skipped(&event) {
      return;
    }

    self.preprocessed_events.push((line_num, event.clone()));
    let resourceaccesspoint = ResourceAccessPoint_extract(&event);
    match (resourceaccesspoint.0, resourceaccesspoint.1, &event) {
      (ResourceTy::Value(ResourceAccessPoint::Function(_)), ResourceTy::Value(ResourceAccessPoint::Function(_)), _) => {
      },
      (ResourceTy::Value(ResourceAccessPoint::Function(_)),_,  _) => {  
          // (Some(function), Some(variable), _)
      },
      (_, ResourceTy::Value(ResourceAccessPoint::Function(_function)), 
       ExternalEvent::PassByStaticReference{..}) => { 
           // (Some(variable), Some(function), PassByStatRef)
      },
      (_, ResourceTy::Value(ResourceAccessPoint::Function(_function)), 
       ExternalEvent::PassByMutableReference{..}) => {  
           // (Some(variable), Some(function), PassByMutRef)
      },
      (_, ResourceTy::Value(ResourceAccessPoint::Function(_)), _) => { 
          // (Some(variable), Some(function), _)
      },
      (ResourceTy::Anonymous, ResourceTy::Anonymous, _) | (_, ResourceTy::Caller, _) => {},
      (ResourceTy::Value(_), ResourceTy::Value(_), _) // maybe change later
      | (ResourceTy::Deref(_), ResourceTy::Deref(_), _) 
      | (ResourceTy::Value(_), ResourceTy::Deref(_), _)
      | (ResourceTy::Deref(_), ResourceTy::Value(_), _)
      // `v[i] = s` / `v[i] = y`: an arrow into v's column.
      | (ResourceTy::Value(_), ResourceTy::Index(..), _)
      | (ResourceTy::Deref(_), ResourceTy::Index(..), _) => {
        self.event_line_map.get_mut(&line_num).unwrap().push(event.clone());
      },
      _ => ()
    }
    self.propagate_struct_transfer(line_num, &event);
  }

  /// Registered member-field RAPs directly under `parent` (`parent.f`,
  /// not `parent.f.g`), in registration (= declaration) order.
  /// Reference-typed fields are left out: they're modelled as
  /// StaticRef/MutRef members with their own loan bookkeeping.
  pub fn struct_children(&self, parent: &str) -> Vec<ResourceAccessPoint> {
    let prefix = format!("{}.", parent);
    #[expect(clippy::disallowed_methods, reason = "sorted by (hash, name) below")]
    let mut v: Vec<ResourceAccessPoint> = self.raps.iter()
      .filter(|(n, rd)| {
        n.starts_with(&prefix)
          && !n[prefix.len()..].contains('.')
          && matches!(rd.rap, ResourceAccessPoint::Struct(Struct { is_member: true, .. }))
      })
      .map(|(_, rd)| rd.rap.clone())
      .collect();
    v.sort_by(|a, b| a.hash().cmp(b.hash()).then_with(|| a.name().cmp(b.name())));
    v
  }

  /// Make a struct's field columns follow a whole-struct transfer.
  ///
  /// Each field of a struct value has its own timeline, but most
  /// events name only the struct (`drop(c)`, `let d = c;`, `c`
  /// returned to the caller, `let x = make();`). In Rust, moving the
  /// struct moves every field with it, so after `drop(c)` nothing is
  /// left for `c.data` to drop at scope end, and after
  /// `let d = c;` it's `d.data` that owns (and later drops) the
  /// String. This mirrors the struct-level event onto the fields:
  ///
  /// * struct → struct (`let d = c;`): `c.f` → `d.f` per field
  ///   (Move, or Copy for a Copy struct), recursing into nested
  ///   structs through the synthesized events themselves.
  /// * struct → function / closure (`drop(c)`, `move || c`): each
  ///   field is moved too, with no extra arrow (Anonymous target);
  ///   the struct's own arrow already shows where the value went.
  /// * struct → caller (`c` as the fn's result): each field is moved
  ///   to the caller.
  /// * non-struct source → struct (`let x = make();`): each field
  ///   acquires its part of the new value.
  ///
  /// Partial transfers (`let n = p.name;`, pattern destructuring)
  /// name the field itself and are left alone; so are struct → plain
  /// owner/reference events, which only arise from the approximate
  /// destructuring fallback (`let P { name, .. } = p;` binds `name`
  /// "from p"), where marking every field moved would be wrong.
  fn propagate_struct_transfer(&mut self, line_num: usize, event: &ExternalEvent) {
    let (from, to, is_copy, id) = match event {
      ExternalEvent::Move { from, to, id, is_partial: false, .. } => (from, to, false, *id),
      ExternalEvent::Copy { from, to, id, is_partial: false, .. } => (from, to, true, *id),
      _ => return,
    };
    let struct_name = |r: &ResourceTy| match r {
      ResourceTy::Value(rap @ ResourceAccessPoint::Struct(_)) => Some(rap.name().clone()),
      _ => None,
    };
    let from_struct = struct_name(from);
    let to_struct = struct_name(to);
    let from_children = from_struct.as_ref().map_or(Vec::new(), |n| self.struct_children(n));
    // Keep ids unique: callers either bump unique_id before or after
    // handing us the event, so start past the event's own id.
    if *self.unique_id <= id {
      *self.unique_id = id + 1;
    }
    if let Some(from_name) = from_struct.filter(|_| !from_children.is_empty()) {
      let pass_through = match to {
        ResourceTy::Caller => true,
        ResourceTy::Value(r) => r.is_fn() || r.is_closure(),
        // `v[i] = p`: p's fields go into the element with it.
        ResourceTy::Index(..) => true,
        _ => false,
      };
      if to_struct.is_none() && !pass_through {
        return;
      }
      for child in from_children {
        let field = &child.name()[from_name.len() + 1..];
        let dest = match (&to_struct, to) {
          (Some(to_name), _) => match self.raps.get(&format!("{}.{}", to_name, field)) {
            Some(rd) => ResourceTy::Value(rd.rap.clone()),
            None => ResourceTy::Anonymous,
          },
          (None, ResourceTy::Caller) => ResourceTy::Caller,
          (None, _) => ResourceTy::Anonymous,
        };
        // Copying a Copy struct into a fn / to the caller leaves the
        // source fields untouched; only a struct destination needs
        // its fields initialized.
        if is_copy && to_struct.is_none() {
          continue;
        }
        let child_name = child.name().clone();
        let from_child = ResourceTy::Value(child);
        let ev_id = *self.unique_id;
        *self.unique_id += 1;
        let to_anonymous = matches!(dest, ResourceTy::Anonymous);
        let ev = if is_copy {
          ExternalEvent::Copy { from: from_child, to: dest, id: ev_id, is_partial: false, into_closure: false }
        } else {
          ExternalEvent::Move { from: from_child, to: dest, id: ev_id, is_partial: false, into_closure: false }
        };
        self.add_external_event(line_num, ev);
        // A nested struct field moved into a function / closure
        // (`drop(o)` moves `o.inner.v` too) goes to an Anonymous
        // target, which the recursion through the synthesized event
        // above doesn't follow; move its own fields here.
        if to_anonymous && !is_copy {
          self.move_struct_fields_away(line_num, &child_name);
        }
      }
    } else if let Some(to_name) = to_struct {
      self.bind_struct_fields(line_num, &to_name);
    }
  }

  /// `lhs` gets a new value that has no column of its own (a literal,
  /// an operator's result, a macro expansion, a `const`). When `lhs`
  /// is a struct, each of its fields gets its part of that value too,
  /// the same as `let x = make();` (`propagate_struct_transfer`).
  pub fn bind_from_anonymous(&mut self, line_num: usize, lhs: ResourceTy) {
    let struct_name = match &lhs {
      ResourceTy::Value(rap @ ResourceAccessPoint::Struct(_)) => Some(rap.name().clone()),
      _ => None,
    };
    self.add_ev(line_num, Evt::Bind, lhs, ResourceTy::Anonymous, false);
    if let Some(name) = struct_name {
      self.bind_struct_fields(line_num, &name);
    }
  }

  /// Every field of `parent`, recursively, moves along with it to a
  /// place that has no per-field columns (a function or closure that
  /// took the whole struct). No extra arrows: the struct's own arrow
  /// shows where the value went.
  fn move_struct_fields_away(&mut self, line_num: usize, parent: &str) {
    for child in self.struct_children(parent) {
      let name = child.name().clone();
      let ev_id = *self.unique_id;
      *self.unique_id += 1;
      self.add_external_event(line_num, ExternalEvent::Move {
        from: ResourceTy::Value(child),
        to: ResourceTy::Anonymous,
        id: ev_id,
        is_partial: false,
        into_closure: false,
      });
      self.move_struct_fields_away(line_num, &name);
    }
  }

  /// Non-Copy fields of `parent` (recursively) that currently own a
  /// resource, i.e. what dropping `parent` right now would drop.
  pub fn struct_fields_holding_resource(&self, parent: &str) -> Vec<ResourceAccessPoint> {
    let mut out = Vec::new();
    for child in self.struct_children(parent) {
      let name = child.name().clone();
      if !child.is_copy() && self.rap_holds_resource_now(&name) {
        out.push(child);
      }
      out.extend(self.struct_fields_holding_resource(&name));
    }
    out
  }

  /// `to` just received a whole struct value from a source that has
  /// no per-field columns of its own (a call result, the caller, a
  /// dereference): every field now holds its part of that value.
  pub fn bind_struct_fields(&mut self, line_num: usize, parent: &str) {
    for child in self.struct_children(parent) {
      let name = child.name().clone();
      let ev_id = *self.unique_id;
      *self.unique_id += 1;
      self.add_external_event(line_num, ExternalEvent::Bind {
        from: ResourceTy::Anonymous,
        to: ResourceTy::Value(child),
        id: ev_id,
      });
      self.bind_struct_fields(line_num, &name);
    }
  }

  // Best-effort answer to "does this RAP currently own a resource?",
  // derived from the events emitted so far. Used to decide whether a
  // reassignment should produce an OwnerDropAtReassign — `let mut y =
  // a; y = x;` drops a, but `let mut y = a; let z = y; y = x;` has
  // nothing to drop: y was moved out at the prior `let z = y`.
  // See holds_resource_after.
  pub fn rap_holds_resource_now(&self, rap_name: &str) -> bool {
    holds_resource_after(self.preprocessed_events, rap_name).unwrap_or(false)
  }

  pub fn ext_ev_of_evt(&self, evt: Evt, lhs: ResourceTy, rhs: ResourceTy, id: usize, is_partial: bool) -> ExternalEvent{
    match evt {
      Evt::Bind => ExternalEvent::Bind { from: rhs, to: lhs, id },
      Evt::Copy => ExternalEvent::Copy { from: rhs, to: lhs, id, is_partial, into_closure: false },
      Evt::Move => ExternalEvent::Move { from: rhs, to: lhs, id, is_partial, into_closure: false },
      Evt::SBorrow => ExternalEvent::StaticBorrow { from: rhs, to: lhs, id, is_partial, is_reborrow: false },
      Evt::MBorrow => ExternalEvent::MutableBorrow { from: rhs, to: lhs, id, is_partial, is_reborrow: false },
      Evt::PassBySRef => ExternalEvent::PassByStaticReference { from: rhs, to: lhs, id, implicit: ImplicitRef::No, refcell_borrow_mut: false },
      Evt::PassByMRef => ExternalEvent::PassByMutableReference {
        from: rhs, to: lhs, id, is_reborrow: false, result_has_lifetime: false, implicit: ImplicitRef::No,
      },
      Evt::SDie =>  ExternalEvent::StaticDie { from: rhs, to: lhs, id, static_lender: None },
      Evt::MDie => ExternalEvent::MutableDie { from: rhs, to: lhs, id, static_lender: None }
    }
  }

  /// `match_rhs` for one branch of an `if` / `match` whose value is
  /// bound to `lhs`, recording the event ids it used (`rhs_arm_ids`).
  fn match_rhs_arm(&mut self, lhs: ResourceTy, body: &'tcx Expr<'tcx>, evt: Evt) {
    let lo = *self.unique_id;
    self.match_rhs(lhs, body, evt);
    self.rhs_arm_ids.insert(body.hir_id, (lo, *self.unique_id));
  }

  /// Removes from `preprocessed_events`, and returns in order, the
  /// events of one branch: those whose id was handed out while the
  /// branch was visited (`ids`), plus those `match_rhs_arm` emitted
  /// for the branch's value (`body`). Events are put in branches by
  /// where they came from, not by source line, so branches that share
  /// a line (`if c { take(s); } else { … }`, a one-line `match`, or an
  /// `else if`) get their own events (#117).
  pub fn take_branch_events(&mut self, ids: std::ops::Range<usize>, body: Option<&'tcx Expr<'tcx>>) -> Vec<(usize, ExternalEvent)> {
    let rhs = body.and_then(|b| self.rhs_arm_ids.get(&b.hir_id).copied());
    let mine = |id: usize| ids.contains(&id) || rhs.is_some_and(|(lo, hi)| (lo..hi).contains(&id));
    let mut taken = Vec::new();
    self.preprocessed_events.retain(|(l, ev)| {
      if mine(ev.get_id()) {
        taken.push((*l, ev.clone()));
        false
      } else {
        true
      }
    });
    taken
  }

  pub fn add_ev(&mut self, line_num: usize, evt: Evt, lhs: ResourceTy, rhs: ResourceTy, is_partial: bool) {
    self.add_external_event(line_num, self.ext_ev_of_evt(evt, lhs, rhs, *self.unique_id, is_partial));
    *self.unique_id += 1;
  }

  /// The end of reference `from`'s borrow (StaticDie / MutableDie),
  /// returning to `to`; `ref_data` is `from`'s loan, and names the
  /// 'static data it borrowed when the lender has no column.
  pub fn add_borrow_end_ev(&mut self, line_num: usize, to: ResourceTy, from: ResourceTy, ref_data: &RefData) {
    let id = *self.unique_id;
    *self.unique_id += 1;
    let static_lender = match ref_data.lender {
      ResourceTy::Anonymous => ref_data.static_lender.clone(),
      _ => None,
    };
    let ev = if ref_data.ref_mutability {
      ExternalEvent::MutableDie { from, to, id, static_lender }
    } else {
      ExternalEvent::StaticDie { from, to, id, static_lender }
    };
    self.add_external_event(line_num, ev);
  }

  /// True when `e`'s own type (no auto-deref) is a `union`. Used to
  /// route field projections on a union-typed local (`u._a`) to the
  /// union's single opaque owner column instead of looking up a
  /// per-field RAP that was never registered.
  pub fn is_union_typed(&self, e: &Expr) -> bool {
    self.tcx.typeck(e.hir_id.owner).node_type(e.hir_id).is_union()
  }

  /// `add_ev` for an `SBorrow` / `MBorrow` whose reborrow-ness the
  /// caller has decided (see `is_explicit_reborrow`).
  pub fn add_borrow_ev(&mut self, line_num: usize, evt: Evt, lhs: ResourceTy, rhs: ResourceTy, is_reborrow: bool) {
    let mut ev = self.ext_ev_of_evt(evt, lhs, rhs, *self.unique_id, false);
    if let ExternalEvent::StaticBorrow { is_reborrow: r, .. }
         | ExternalEvent::MutableBorrow { is_reborrow: r, .. } = &mut ev {
      *r = is_reborrow;
    }
    self.add_external_event(line_num, ev);
    *self.unique_id += 1;
  }

  /// `add_ev` for a `PassByMRef`, tagged with whether the call
  /// reborrows the `&mut` argument, whether the call's result
  /// can carry that reborrow past the call, and whether Rust
  /// inserted the `&mut` (see `ImplicitRef`).
  pub fn add_pass_by_mref_ev(&mut self, line_num: usize, func: ResourceTy, arg: ResourceTy, is_reborrow: bool, result_has_lifetime: bool, implicit: ImplicitRef) {
    let id = *self.unique_id;
    self.add_external_event(line_num, ExternalEvent::PassByMutableReference {
      from: arg, to: func, id, is_reborrow, result_has_lifetime, implicit,
    });
    *self.unique_id += 1;
  }

  /// `add_ev` for a `PassBySRef` with an explicit `ImplicitRef`.
  pub fn add_pass_by_sref_ev(&mut self, line_num: usize, func: ResourceTy, arg: ResourceTy, implicit: ImplicitRef, refcell_borrow_mut: bool) {
    let id = *self.unique_id;
    self.add_external_event(line_num, ExternalEvent::PassByStaticReference {
      from: arg, to: func, id, implicit, refcell_borrow_mut,
    });
    *self.unique_id += 1;
  }

  // Return the ResourceTy that corresponds to the LHS of an assignment.
  // `None` means the LHS is a shape the plugin doesn't model (e.g. a
  // tuple field `t.0 = …`), or names a path the plugin never
  // registered as a RAP (synthetic macro local, a field through a
  // reference). The caller short-circuits the assignment instead of
  // crashing.
  pub fn resource_of_lhs(&mut self, expr: &'tcx Expr) -> Option<ResourceTy> {
    match expr.kind {
      // Path / Field (including chained `a.b.c`): walk the projection
      // chain via `expr_to_rap_name` to get the qualified name and
      // look it up. `register_struct_members` recursively registers
      // nested struct fields under qualified names like `r.a.b`, so
      // for value-typed locals chained-field LHS resolves directly.
      // For ref-to-struct params, #147 adds per-field registration so
      // `self.field = …` and `r.field = …` also resolve.
      ExprKind::Path(_) | ExprKind::Field(..) => {
        // `u._a = 5;` on a union-typed local: unions are one opaque
        // owner column with no per-field RAPs (see the AdtKind::Union
        // arms in define_lhs / the pat-binding site), so a field write
        // is a write to the union itself.
        if let ExprKind::Field(recv, _) = expr.kind {
          if self.is_union_typed(recv) {
            let name = expr_to_rap_name(recv, &self.tcx)?;
            return self.raps.get(&name).map(|rd| ResourceTy::Value(rd.rap.to_owned()));
          }
        }
        let name = expr_to_rap_name(expr, &self.tcx)?;
        self.raps.get(&name).map(|rd| ResourceTy::Value(rd.rap.to_owned()))
      }
      ExprKind::Unary(UnOp::Deref, exp) => {
        let rhs_rap = fetch_rap(&expr, &self.tcx, &self.raps);
        let line_num = expr_to_line(&exp, &self.tcx);
        match rhs_rap {
          Some(x) => {
            self.update_rap(&x, line_num);
            Some(ResourceTy::Deref(x))
          }
          None => Some(ResourceTy::Anonymous),
        }
      }
      // `v[i] = …`, `grid[r][c] += …`, `r[i] = …` (r: `&mut Vec<_>`):
      // a write to one element of the resource the receiver owns or
      // points to. Elements have no columns, so the event goes on the
      // receiver's column, tagged as an element write: the receiver
      // neither acquires a new resource nor drops its old one.
      ExprKind::Index(..) => {
        // An element that is itself a reference (`v[0] = &s`) would
        // start a borrow whose end we don't track; leave it undrawn.
        let elem_ty = self.tcx.typeck(expr.hir_id.owner).node_type(expr.hir_id);
        if elem_ty.is_ref() {
          return None;
        }
        let name = expr_to_rap_name(expr, &self.tcx)?;
        let rap = self.raps.get(&name)?.rap.to_owned();
        if rap.is_fn() {
          return None;
        }
        let place = self.place_label(expr, &name);
        // A plain `=` assigns to the place: a Copy value is "assigned a
        // new value", an owned value over one that needs drop drops it,
        // and an owned value with nothing to drop just acquires the
        // new one. The caller turns this into `Update` for `op=`, which
        // calls the operator method and overwrites nothing.
        let typing_env = rustc_middle::ty::TypingEnv::post_analysis(self.tcx, expr.hir_id.owner);
        let write = if self.tcx.type_is_copy_modulo_regions(typing_env, elem_ty) {
          PlaceWrite::AssignCopy
        } else if elem_ty.needs_drop(self.tcx, typing_env) {
          PlaceWrite::AssignDropsOld
        } else {
          PlaceWrite::AssignOwned
        };
        Some(ResourceTy::Index(rap, place, write))
      }
      // Any other place expression: not modelled, skipped.
      _ => {
        warn!("unsupported assignment LHS shape; skipping assignment at this location");
        None
      }
    }
  }

  /// True when the method call `call` resolves to `RefCell::borrow_mut`.
  /// Decided by the callee's DefId (an inherent method whose impl is for
  /// the `RefCell` diagnostic item), so a user type with its own
  /// `borrow_mut(&self)` doesn't match.
  pub fn is_refcell_borrow_mut_call(&self, call: &Expr) -> bool {
    let ExprKind::MethodCall(seg, ..) = call.kind else { return false };
    if seg.ident.name.as_str() != "borrow_mut" {
      return false;
    }
    let Some(def_id) = self.tcx.typeck(call.hir_id.owner).type_dependent_def_id(call.hir_id) else { return false };
    let Some(impl_id) = self.tcx.impl_of_assoc(def_id) else { return false };
    self.tcx.type_of(impl_id).skip_binder().ty_adt_def()
      .map_or(false, |adt| self.tcx.is_diagnostic_item(rustc_span::sym::RefCell, adt.did()))
  }

  /// Registered RAP name for a closure capture's place: the root
  /// variable, plus any struct-field projections (`p.name` for a
  /// disjoint capture of one field, which has its own column). A
  /// capture through a dereference (`*r`) is named after the reference
  /// itself. `None` if a field projection isn't a named struct field.
  fn capture_rap_name(&self, place: &rustc_middle::hir::place::Place<'tcx>, root: &str) -> Option<String> {
    let mut name = root.to_owned();
    for (i, proj) in place.projections.iter().enumerate() {
      match proj.kind {
        rustc_middle::hir::place::ProjectionKind::Field(idx, variant) => {
          let ty = place.ty_before_projection(i);
          let adt = ty.ty_adt_def().filter(|a| a.is_struct())?;
          name = format!("{}.{}", name, adt.variant(variant).fields[idx].name.as_str());
        }
        _ => break,
      }
    }
    Some(name)
  }

  /// The RAP a closure capture is drawn on: the captured place's own
  /// column (`p.name` for a disjoint field capture, see
  /// `capture_rap_name`), or, if that field has no column, the nearest
  /// enclosing place that does (`p.a` for `p.a.b`, then `p`). A tuple
  /// element capture (`move || t.0.len()`) falls back to `t`: tuples
  /// have no element columns yet (#189).
  fn capture_rap(&self, place: &rustc_middle::hir::place::Place<'tcx>, root: &str) -> Option<ResourceAccessPoint> {
    let mut name = self.capture_rap_name(place, root).unwrap_or_else(|| root.to_owned());
    loop {
      if let Some(rd) = self.raps.get(&name) {
        return Some(rd.rap.to_owned());
      }
      name = name.rsplit_once('.')?.0.to_owned();
    }
  }

  /// A closure literal passed straight to a call (`thread::spawn(move
  /// || ..)`, `v.into_iter().for_each(move |x| ..)`): each variable it
  /// captures by value is moved (or, if Copy, copied) into the closure,
  /// which is handed to the callee, so draw it as moved / copied into
  /// the call (#230). The closure itself has no column. By-reference
  /// captures aren't drawn: whether their borrow ends with the call
  /// depends on whether the callee keeps the closure (`map(|x| x + k)`
  /// returns an iterator that still borrows `k`). Returns false if the
  /// closure captures nothing by value, so the caller falls back to the
  /// plain argument handling.
  fn match_closure_arg(&mut self, closure: &'tcx rustc_hir::Closure<'tcx>, arg: &'tcx Expr, to_ro: &ResourceTy) -> bool {
    let line_num = expr_to_line(&arg, &self.tcx);
    let typeck = self.tcx.typeck(closure.def_id);
    let env = rustc_middle::ty::TypingEnv::post_analysis(self.tcx, arg.hir_id.owner);
    let mut drew_any = false;
    for capture in typeck.closure_min_captures_flattened(closure.def_id) {
      if !matches!(capture.info.capture_kind,
        rustc_middle::ty::UpvarCapture::ByValue | rustc_middle::ty::UpvarCapture::ByUse) {
        continue;
      }
      let upvar_hir_id = match capture.place.base {
        rustc_middle::hir::place::PlaceBase::Upvar(upvar_id) => upvar_id.var_path.hir_id,
        _ => continue,
      };
      let root = self.tcx.hir_name(upvar_hir_id).as_str().to_owned();
      let Some(from_rap) = self.capture_rap(&capture.place, &root) else { continue };
      self.update_rap(&from_rap, line_num);
      let evt = if self.tcx.type_is_copy_modulo_regions(env, capture.place.ty()) { Evt::Copy } else { Evt::Move };
      let mut ev = self.ext_ev_of_evt(evt, to_ro.clone(), ResourceTy::Value(from_rap), *self.unique_id, false);
      if let ExternalEvent::Move { into_closure, .. } | ExternalEvent::Copy { into_closure, .. } = &mut ev {
        *into_closure = true;
      }
      self.add_external_event(line_num, ev);
      *self.unique_id += 1;
      drew_any = true;
    }
    drew_any
  }

  /// Is `arg` the argument of a call to `std::mem::drop`?
  fn is_mem_drop_arg(&self, arg: &Expr<'tcx>) -> bool {
    let rustc_hir::Node::Expr(call) = self.tcx.parent_hir_node(arg.hir_id) else { return false };
    let ExprKind::Call(callee, _) = call.kind else { return false };
    let ExprKind::Path(ref qpath) = callee.kind else { return false };
    let res = self.tcx.typeck(callee.hir_id.owner).qpath_res(qpath, callee.hir_id);
    matches!(res, rustc_hir::def::Res::Def(_, def_id) if self.tcx.is_diagnostic_item(rustc_span::sym::mem_drop, def_id))
  }

  /// How an element place (`v[i]`) is named in tooltips: its source
  /// text when that's short and plain, else `v[…]`. Tooltip text goes
  /// into an HTML attribute, so text with markup characters, quotes
  /// or line breaks is never used verbatim.
  fn place_label(&self, expr: &Expr, rap_name: &str) -> String {
    let fallback = format!("{}[…]", rap_name);
    if expr.span.from_expansion() {
      return fallback;
    }
    match self.tcx.sess.source_map().span_to_snippet(expr.span) {
      Ok(text) if text.chars().count() <= 24
        && !text.contains(|c: char| matches!(c, '<' | '>' | '&' | '"' | '\'' | '\n' | '\r')) => text,
      _ => fallback,
    }
  }

  // add events for arguments in a function call
  pub fn match_arg(&mut self, arg: &'tcx Expr, fn_name: String) {
    self.add_fn(fn_name.clone());
    if let ExprKind::Closure(closure) = arg.kind {
      let to_ro = ResourceTy::Value(self.raps.get(&fn_key(&fn_name)).unwrap().rap.to_owned());
      if self.match_closure_arg(closure, arg, &to_ro) {
        return;
      }
    }
    let line_num = expr_to_line(&arg, &self.tcx);
    let tycheck_results = self.tcx.typeck(arg.hir_id.owner);
    let arg_ty = tycheck_results.node_type(arg.hir_id);
    let is_copyable = self.tcx.type_is_copy_modulo_regions(rustc_middle::ty::TypingEnv::post_analysis(self.tcx, arg.hir_id.owner), arg_ty);
    let from_ro = get_rap(arg, &self.tcx, &self.raps);
    // Extend the lifetime of any reference RAP this arg touches so a
    // ref passed as a fn argument shows its dotted lifetime line out
    // to the call site. Critical for macro args like
    // `println!("{}", p)`: visit_expr bails on macro-expanded
    // children before reaching the inner Path that would normally
    // call update_rap, so without this nudge the ref's lifetime
    // stays pinned to its declaration line and the renderer draws a
    // degenerate (zero-height) ref-line.
    if let ResourceTy::Value(rap) | ResourceTy::Deref(rap) = &from_ro {
      self.update_rap(rap, line_num);
    }
    let mut to_ro = ResourceTy::Value(self.raps.get(&fn_key(&fn_name)).unwrap().rap.to_owned());
    // `drop(x)` is where x's value is dropped: say so on the move,
    // unless its type has nothing to drop.
    if !arg_ty.is_ref() && !is_copyable && self.is_mem_drop_arg(arg)
      && arg_ty.needs_drop(self.tcx, rustc_middle::ty::TypingEnv::post_analysis(self.tcx, arg.hir_id.owner))
    {
      if let ResourceTy::Value(rap) = &to_ro {
        to_ro = ResourceTy::Value(rap.with_drops_arg());
      }
    }
    // type-check the arg and add event accordingly
    if arg_ty.is_ref() {
      match arg_ty.ref_mutability().unwrap() {
        Mutability::Not => self.add_ev(line_num, Evt::PassBySRef, to_ro, from_ro, false),
        Mutability::Mut => {
          // Reborrow if written `f(&mut *r)`, or if rustc inserted
          // the reborrow for `f(r)` (param type `&mut T`). `take(r)`
          // into a generic `T` has no such adjustment: r is moved.
          let is_reborrow = match arg.kind {
            ExprKind::AddrOf(_, _, operand) => is_explicit_reborrow(operand, tycheck_results),
            _ => is_implicit_mut_reborrow(arg, tycheck_results),
          };
          // Unknown parent → assume the result may hold the borrow;
          // that only selects the weaker (always-true) wording.
          let result_has_lifetime = match self.tcx.parent_hir_node(arg.hir_id) {
            rustc_hir::Node::Expr(call) => call_result_has_lifetime(call, tycheck_results),
            _ => true,
          };
          self.add_pass_by_mref_ev(line_num, to_ro, from_ro, is_reborrow, result_has_lifetime, ImplicitRef::No)
        }
      }
    }
    else {
      match is_copyable {
        true => self.add_ev(line_num, Evt::Copy, to_ro, from_ro, false),
        false => self.add_ev(line_num, Evt::Move, to_ro, from_ro, false)
      }
    }
  }

  // Update the aliasing data for an alias
  pub fn update_aliasing_data(& mut self, alias: &String, new_data: &BTreeMap<usize, String>, offset: usize) {
    let ref_data = self.borrow_map.get_mut(alias).unwrap();
    for (k, v) in new_data {
      ref_data.aliasing.insert(*k + offset, v.to_owned());
    }
  }

  /// `get_ref_data` plus, when the lender has no column, the
  /// 'static data the reference is known to borrow (see
  /// `static_lender_of`). Every site that (re)sets a RefData's lender
  /// from an initializer goes through here.
  pub fn get_ref_data_full(&self, expr: &'tcx Expr) -> (ResourceTy, VecDeque<String>, Option<StaticLender>) {
    let (lender, aliasing) = self.get_ref_data(expr);
    let static_lender = match lender {
      ResourceTy::Anonymous => self.static_lender_of(expr),
      _ => None,
    };
    (lender, aliasing, static_lender)
  }

  /// What a reference initialized from `expr` borrows, when that is
  /// data living for the whole program that the diagram has no column
  /// for: a string literal, a `static` item (`&GREETING`), or what a
  /// `static` / `const` of reference type points to (`GREETING`).
  /// Copying a reference (`let t = s;`) carries over `s`'s answer.
  /// Decided from the HIR, never from source text. None when unknown
  /// (a call result, `&5`, a match on literals, ...).
  pub fn static_lender_of(&self, expr: &'tcx Expr) -> Option<StaticLender> {
    let item_name = |p: &rustc_hir::Path| match p.res {
      Res::Def(_, def_id) => Some(self.tcx.item_name(def_id).as_str().to_owned()),
      _ => None,
    };
    match expr.kind {
      ExprKind::Lit(lit) => match lit.node {
        rustc_ast::LitKind::Str(..) => Some(StaticLender::Literal { noun: "string literal" }),
        rustc_ast::LitKind::ByteStr(..) => Some(StaticLender::Literal { noun: "byte string literal" }),
        rustc_ast::LitKind::CStr(..) => Some(StaticLender::Literal { noun: "C string literal" }),
        _ => None,
      },
      // `&GREETING` / `&mut COUNTER`: a borrow of the static itself.
      // (`&CONST` borrows a fresh copy of the constant's value, which
      // may or may not be promoted to a static; not named.)
      ExprKind::AddrOf(_, mutbl, inner) => match inner.kind {
        ExprKind::Path(QPath::Resolved(_, p))
          if matches!(p.res, Res::Def(DefKind::Static { .. }, _)) =>
            item_name(p).map(StaticLender::Static),
        // `&CONST`: a fresh copy of the constant's value, which Rust
        // promotes to a `'static` only when the value has no
        // destructor and no interior mutability (judged here from the
        // type, which is conservative). Never for `&mut`.
        ExprKind::Path(QPath::Resolved(_, p))
          if mutbl == rustc_hir::Mutability::Not => match p.res {
            Res::Def(DefKind::Const { .. }, def_id) => {
              let ty = self.tcx.type_of(def_id).instantiate_identity().skip_normalization();
              let env = rustc_middle::ty::TypingEnv::fully_monomorphized();
              if ty.is_freeze(self.tcx, env) && !ty.needs_drop(self.tcx, env) {
                item_name(p).map(StaticLender::PromotedConst)
              } else {
                None
              }
            }
            _ => None,
          },
        _ => None,
      },
      // `if c { "a" } else { "b" }`, `match n { 0 => "zero", _ => "many" }`:
      // the borrow is of 'static data only if every arm that yields a
      // value agrees (arms of type `!`, like `panic!()`, yield none).
      ExprKind::If(_, then, Some(els)) => self.merge_static_lenders(&[then, els]),
      ExprKind::Match(_, arms, _) => {
        let bodies: Vec<&'tcx Expr> = arms.iter().map(|a| a.body).collect();
        self.merge_static_lenders(&bodies)
      }
      ExprKind::Path(QPath::Resolved(_, p)) => match p.res {
        // The binding is a reference, so the item's value is one: the
        // binding gets a copy of it and borrows what it points to.
        Res::Def(DefKind::Static { .. }, _) => item_name(p).map(StaticLender::PointeeOfStatic),
        Res::Def(DefKind::Const { .. } | DefKind::AssocConst { .. }, _) =>
          item_name(p).map(StaticLender::PointeeOfConst),
        Res::Local(_) => self.borrow_map.get(&path_key(p, &self.tcx))
          .and_then(|rd| match rd.lender {
            ResourceTy::Anonymous => rd.static_lender.clone(),
            _ => None,
          }),
        _ => None,
      },
      // `unsafe { &mut COUNTER }`, `{ "hi" }`
      ExprKind::Block(b, _) => b.expr.and_then(|e| self.static_lender_of(e)),
      _ => None,
    }
  }

  /// The one StaticLender shared by every value-yielding branch of an
  /// `if` / `match`; None if any branch is unknown or they differ.
  fn merge_static_lenders(&self, branches: &[&'tcx Expr]) -> Option<StaticLender> {
    let typeck = self.tcx.typeck(branches[0].hir_id.owner);
    let mut found: Option<StaticLender> = None;
    for e in branches {
      if typeck.expr_ty(e).is_never() { continue; }
      let l = self.static_lender_of(e)?;
      match &found {
        Some(f) if *f != l => return None,
        _ => found = Some(l),
      }
    }
    found
  }

  // 
  pub fn get_ref_data(&self, expr: &'tcx Expr) -> (ResourceTy, VecDeque<String>){
    // ex:
    // let a = &b (where b has type &&i32)
    // a's aliases consist of a -> (b -> ... -> ...)
    if is_addr(&expr) {
      let lender = get_rap(&expr, &self.tcx, &self.raps); // in this case we are borrowing from b
      let mut aliasing = get_aliasing_data(&lender, &self.borrow_map);
      // if expr has type &&
      let ty = self.tcx.typeck(expr.hir_id.owner).node_type(expr.hir_id);
      if ty.builtin_deref(false).unwrap().is_ref() {
        match &lender {
          ResourceTy::Anonymous | ResourceTy::Caller => { (lender, aliasing) }
          ResourceTy::Value(x) | ResourceTy::Deref(x) | ResourceTy::Index(x, _, _) => {
            aliasing.push_front(x.name().to_owned()); // here the lender is added to the list of aliases 
            (lender, aliasing)
          }
        }
      }
      else {
        (lender, aliasing)
      }
    }
    else { // if we are copying a reference (ie let a = b (where b is ref))
      // in this case we are borrowing to whatever b refers to
      // likewise, aliasing data is just copied here
      let lender = find_lender(&expr, &self.tcx, &self.raps, &self.borrow_map);
      (find_lender(&expr, &self.tcx, &self.raps, &self.borrow_map), get_aliasing_data(&lender, &self.borrow_map))
    }
  }

  // Add a RAP to our collection of raps for a let stmt
  pub fn define_lhs(&mut self, name: String, mutability: bool, expr: &'tcx Expr, ty: Ty <'tcx>) {
    let is_special = ty_is_special_owner(&ty);
    if ty.is_ref() {
      let (lender, aliasing, static_lender) = self.get_ref_data_full(&expr);
      let pointee_copy = self.ref_pointee_is_copy(ty, expr.hir_id.owner);
      self.add_ref(name.clone(), 
      bool_of_mut(ty.ref_mutability().unwrap()), 
      mutability, 
      expr_to_line(&expr, &self.tcx),
      lender,
      aliasing,
      self.current_scope, !self.inside_branch, pointee_copy);
      if let Some(rd) = self.borrow_map.get_mut(&name) {
        rd.static_lender = static_lender;
        rd.borrow_span = borrow_span_of(expr);
      }
    }
    else if ty.is_adt() && !is_special {
      match ty.ty_adt_def().unwrap().adt_kind() {
        AdtKind::Struct => {
          let owner_hash = self.rap_hashes as u64;
          let parent_is_copy = self.ty_is_copy(ty, expr.hir_id.owner);
          self.add_struct(name.clone(), owner_hash, false, mutability, parent_is_copy, self.ty_has_interior_mut(ty, expr.hir_id.owner), self.current_scope,!self.inside_branch);
          self.register_struct_members(
            &name,
            ty,
            owner_hash,
            mutability,
            MemberSite {
              owner_id: expr.hir_id.owner,
              assigned_at: expr_to_line(&expr, &self.tcx),
              scope: self.current_scope,
              is_global: !self.inside_branch,
              ref_lender: Some(ResourceTy::Anonymous),
            },
          );
        },
        // Unions and enums both render as opaque single-owner columns:
        // there's no statically-known discriminant we can attribute
        // events to per-field, so we don't try. A union is a fixed
        // memory slot whose interpretation depends on the active
        // variant at runtime; modelling per-field timelines for it
        // would be lying about which field is "the" field.
        AdtKind::Union | AdtKind::Enum => {
          let is_copy = self.ty_is_copy(ty, expr.hir_id.owner);
          let interior_mut = self.ty_has_interior_mut(ty, expr.hir_id.owner);
          if ty.is_union() {
            self.add_union_owner(name, mutability, is_copy, interior_mut, self.current_scope, !self.inside_branch);
          } else {
            self.add_owner(name, mutability, is_copy, interior_mut, self.current_scope, !self.inside_branch);
          }
        }
      }
    }
    else if ty.is_closure() {
      // Tag the binding as a closure so the tooltip layer can swap
      // capture-flavoured wording for moves/borrows into `name` and
      // for `name`'s scope-end. The `match_rhs` ExprKind::Closure
      // arm emits the actual capture events on top of this RAP.
      self.add_closure_owner(name, mutability, self.current_scope, !self.inside_branch);
    }
    else {
      // Plain values, including a function item or function pointer
      // (`let f = helper;`, `let f: fn(i32) -> i32 = helper;`). Both
      // are Copy values that own no resource, so `f` is an ordinary
      // Copy owner; calls through it (`f(x)`) are drawn by the visitor's
      // Call arm like calls to a named function (#254).
      let is_copy = self.ty_is_copy(ty, expr.hir_id.owner);
      self.add_owner(name.clone(), mutability, is_copy, self.ty_has_interior_mut(ty, expr.hir_id.owner), self.current_scope, !self.inside_branch);
      self.mark_if_manually_drop(&name, ty);
    }
  }

  /// Register `prefix.field` RAPs for every field of a struct type,
  /// recursing into nested structs so accesses like `r.a.b` resolve.
  ///
  /// `top_owner_hash` is the timeline-grouping key for the outermost
  /// struct — every nested member shares it so the renderer keeps
  /// the whole tree under a single struct group, the same way flat
  /// struct members already share their parent's group today.
  ///
  /// Reference fields (e.g. `Excerpt<'a> { p: &'a str }`) get the
  /// same StaticRef/MutRef-member treatment as before, with the lender
  /// `site.ref_lender`: a placeholder Anonymous for a `let` that the
  /// Struct arm of `match_rhs` overwrites once it sees the
  /// construction-site initializer, or the caller for a parameter.
  /// Owned struct fields recurse so `r.a.b.c` etc. land in `raps`
  /// under their qualified names; Field-arm lookups in
  /// `expr_visitor_utils` then find them and emit events instead of
  /// silently giving up.
  ///
  /// Used for `let` bindings, fn parameters and pattern bindings, so
  /// all three get nested field columns (#239).
  pub fn register_struct_members(
    &mut self,
    prefix: &str,
    ty: Ty<'tcx>,
    top_owner_hash: u64,
    mutability: bool,
    site: MemberSite,
  ) {
    let generic_args = match ty.kind() {
      TyKind::Adt(_, args) => *args,
      _ => unreachable!("register_struct_members called with non-Adt ty"),
    };
    let owner_id = site.owner_id;
    for field in ty.ty_adt_def().unwrap().all_fields() {
      let field_name = format!("{}.{}", prefix, field.name.as_str());
      let field_ty = field.ty(self.tcx, generic_args);
      if let (true, Some(ref_lender)) = (field_ty.is_ref(), &site.ref_lender) {
        let ref_mutability = bool_of_mut(field_ty.ref_mutability().unwrap());
        let pointee_copy = self.ref_pointee_is_copy(field_ty, owner_id);
        if ref_mutability {
          self.add_mut_ref_member(field_name.clone(), mutability,
              site.scope, site.is_global, Some(top_owner_hash), pointee_copy);
        } else {
          self.add_static_ref_member(field_name.clone(), mutability,
              site.scope, site.is_global, Some(top_owner_hash), pointee_copy);
        }
        // Same lender-placeholder rationale as the flat case before
        // the refactor: borrow stored in this field outlives the
        // statement, so stretch the loan to the enclosing scope so
        // the ref-line trapezoid actually has a non-degenerate range.
        self.borrow_map.insert(field_name, RefData {
          lender: ref_lender.clone(),
          assigned_at: site.assigned_at,
          lifetime: site.scope,
          ref_mutability,
          aliasing: VecDeque::new(),
          static_lender: None,
          borrow_span: None,
        });
      } else {
        let field_is_copy = self.ty_is_copy(field_ty, owner_id);
        self.add_struct(field_name.clone(), top_owner_hash, true, mutability, field_is_copy, self.ty_has_interior_mut(field_ty, owner_id),
            site.scope, site.is_global);
        self.mark_if_manually_drop(&field_name, field_ty);
        // Recurse for nested struct fields. Skip enums / unions
        // (only struct ADTs are flattened today) and skip the
        // special-owner builtins like `String` whose internals
        // we deliberately don't expose.
        if field_ty.is_adt() && !ty_is_special_owner(&field_ty) {
          if let Some(adt_def) = field_ty.ty_adt_def() {
            if matches!(adt_def.adt_kind(), AdtKind::Struct) {
              self.register_struct_members(
                &field_name,
                field_ty,
                top_owner_hash,
                mutability,
                site.clone(),
              );
            }
          }
        }
      }
    }
  }

  // Add a RAP for each variable bound in a pattern
  pub fn get_dec_of_pat2<'t>(
    &mut self,
    pat: &Pat,
    ty_results: &'tcx TypeckResults<'tcx>,
    parent: &ResourceTy,
    parent_ty: &'t Ty<'tcx>,
    scope: usize,
    res: &'t mut Vec<(ResourceAccessPoint, Evt, Ty<'tcx>)>,
) {
    match pat.kind {
        PatKind::TupleStruct(_p, tuple_members, _) => {
            for p in tuple_members.iter() {
                self.get_dec_of_pat2(p, ty_results, parent, parent_ty, scope, res);
            }
        }
        // `Some(x)` / `None` lower to `PatKind::Struct(QPath::LangItem(...))`
        // with one PatField wrapping the user pattern (rustc's
        // `pat_lang_item_variant`). Recurse into each field's inner
        // pattern with the same parent — same flow as TupleStruct, just
        // a different HIR shape.
        PatKind::Struct(_, fields, _) => {
            for field in fields.iter() {
                self.get_dec_of_pat2(field.pat, ty_results, parent, parent_ty, scope, res);
            }
        }
        PatKind::Binding(mode, id, ident, _) => {
            // add raps that appear in binding
            let muta = bool_of_mut(mode.1); // You need to define this function
            let line_num = span_to_line(&ident.span, &self.tcx); // Assuming self.span_to_line exists
            let ty: Ty<'tcx> = ty_results.node_type(id);
            let name = ident.to_string();
            // copied code - should definetly make a function of this
            if ty.is_ref() {
                let ref_mutability = bool_of_mut(ty.ref_mutability().unwrap()); // You need to define this function
                // Who the binding borrows from. A binding that borrows
                // the matched place (`Some(ref x)`, or `Some(x)` matched
                // through a reference by default binding modes) borrows
                // the scrutinee. A by-value binding of reference type on
                // a non-reference scrutinee instead *copies* a reference
                // stored inside it (`Some(x)` on an `Option<&T>`): it
                // borrows whatever that stored reference borrows, not the
                // scrutinee. What a non-reference value holds isn't
                // tracked, so leave the lender unknown rather than name
                // the scrutinee (#304).
                let by_ref = matches!(
                    ty_results.pat_binding_modes().get(id).map(|m| m.0),
                    Some(rustc_hir::ByRef::Yes(..))
                );
                let lender = if by_ref || parent_ty.is_ref() { parent.clone() } else { ResourceTy::Anonymous };
                let pointee_copy = self.ref_pointee_is_copy(ty, id.owner);
                self.add_ref(name.clone(), ref_mutability, muta, line_num, lender, VecDeque::new(), scope, false, pointee_copy);
            } else if ty.is_adt() && !ty_is_special_owner(&ty) {
                // Same rule as `define_lhs` and fn params: only
                // user-defined (crate-local) structs get a struct
                // group with per-field columns. Library types like
                // `String` / `Vec` / `Box` are opaque owners, so a
                // binding such as `Some(s)` with `s: String` falls
                // through to `add_owner` below. Without this check `s`
                // became a struct whose private field (`s.vec`) never
                // gets a column, and the renderer drew an empty
                // struct box with a negative width (#192).
                match ty.ty_adt_def().unwrap().adt_kind() {
                    AdtKind::Struct => {
                        let owner_hash = self.rap_hashes as u64; // Assuming self.rap_hashes exists
                        let parent_is_copy = self.ty_is_copy(ty, pat.hir_id.owner);
                        self.add_struct(name.clone(), owner_hash, false, muta, parent_is_copy, self.ty_has_interior_mut(ty, pat.hir_id.owner), scope, false);
                        // Like define_lhs: don't expose the internals of
                        // `String` / `Vec` / `Box` / … (`n.vec` for a
                        // `Some(n)` binding of a String). Nested struct
                        // fields get columns too (`r.tl.x`, #239).
                        // A reference field's lender isn't known here
                        // (it's whatever the matched value's field
                        // borrowed), so reference fields keep the plain
                        // column they had before (#352).
                        if !ty_is_special_owner(&ty) {
                            self.register_struct_members(&name, ty, owner_hash, muta, MemberSite {
                                owner_id: pat.hir_id.owner,
                                assigned_at: line_num,
                                scope,
                                is_global: false,
                                ref_lender: None,
                            });
                        }
                    },
                    // Unions render as opaque single-owner columns —
                    // see the matching note on the LHS-of-expr arm.
                    AdtKind::Union | AdtKind::Enum => {
                        let is_copy = self.ty_is_copy(ty, pat.hir_id.owner);
                        let interior_mut = self.ty_has_interior_mut(ty, pat.hir_id.owner);
                        if ty.is_union() {
                            self.add_union_owner(name.clone(), muta, is_copy, interior_mut, scope, false);
                        } else {
                            self.add_owner(name.clone(), muta, is_copy, interior_mut, scope, false);
                        }
                    }
                }
            } else {
                let is_copy = self.ty_is_copy(ty, pat.hir_id.owner);
                self.add_owner(name.clone(), muta, is_copy, self.ty_has_interior_mut(ty, pat.hir_id.owner), scope, false);
                self.mark_if_manually_drop(&name, ty);
            }

            // A binding whose effective mode is by-reference borrows
            // the matched place, whatever the scrutinee's type: an
            // explicit `ref` / `ref mut` (`Some(ref s)` on an owned
            // `Option<String>`), or a default binding mode through a
            // reference (`Some(s)` on `&opt`). Nothing is copied or
            // moved (#338).
            let binding_ref = match ty_results.pat_binding_modes().get(id).map(|m| m.0) {
                Some(rustc_hir::ByRef::Yes(_, mutbl)) => Some(mutbl),
                _ => None,
            };
            let evt = if let Some(mutbl) = binding_ref {
                if bool_of_mut(mutbl) { Evt::MBorrow } else { Evt::SBorrow }
            } else if parent_ty.is_ref() {
                if bool_of_mut(parent_ty.ref_mutability().unwrap()) { Evt::MBorrow }
                else { Evt::SBorrow }
            } else {
                if self.tcx.type_is_copy_modulo_regions(rustc_middle::ty::TypingEnv::post_analysis(self.tcx, pat.hir_id.owner), ty) { Evt::Copy }
                else { Evt::Move }
            };

            res.push((self.raps.get(&name).unwrap().rap.to_owned(), evt, parent_ty.clone()));
        }
        _ => {}
    }
}

/// Register the pattern bindings of an `if let` / `while let` form
/// against the scrutinee and emit the scrutinee→binding events at the
/// `let_expr` line.
///
/// `let_expr` is the `Let(pat, init, …)` HIR node that lives in the
/// `If`'s guard slot. `emit_gos_at` is `Some(body_end_line)` for the
/// while-let path (where the binding's lifetime is the body and
/// nothing else cleans it up) and `None` for the if-let path (where
/// the surrounding If arm appends GoOutOfScope events from its own
/// `if_decl` set, so we'd double-emit otherwise).
///
/// Returns the set of bindings registered. The if-let caller unions
/// these into its `if_decl` set so `append_decl_branch_events` can
/// populate each binding's timeline with a Branch-shaped event;
/// without that, the renderer's `fetch_timeline` panics looking for
/// the Copy/Move event id in an empty history.
///
/// The caller is responsible for separately visiting the scrutinee
/// (for any reads/borrows it performs).
pub fn register_iflet_let_bindings(
    &mut self,
    let_expr: &'tcx rustc_hir::LetExpr<'tcx>,
    emit_gos_at: Option<usize>,
) -> HashSet<ResourceAccessPoint> {
    let scrutinee = let_expr.init;
    let typeck_res = self.tcx.typeck(scrutinee.hir_id.owner);
    let scrutinee_ty = typeck_res.node_type(scrutinee.hir_id);
    let parent = crate::expr_visitor_utils::get_rap(scrutinee, &self.tcx, &self.raps);

    let let_line = crate::expr_visitor_utils::span_to_line(&let_expr.span, &self.tcx);
    // Body-end is only needed if we're going to emit a GoOutOfScope.
    // get_dec_of_pat2 also takes a `scope` param; pass let_line as a
    // safe default for the if-let case (the renderer ignores it for
    // the binding's own RAP since is_global=false).
    let scope = emit_gos_at.unwrap_or(let_line);

    let mut decls: Vec<(ResourceAccessPoint, Evt, Ty<'tcx>)> = Vec::new();
    self.get_dec_of_pat2(let_expr.pat, typeck_res, &parent, &scrutinee_ty, scope, &mut decls);

    let mut registered: HashSet<ResourceAccessPoint> = HashSet::new();
    for (binding_rap, evt, parent_ty) in decls {
        let is_partial = parent_ty != typeck_res.node_type(let_expr.pat.hir_id);
        let event = self.ext_ev_of_evt(
            evt,
            ResourceTy::Value(binding_rap.clone()),
            parent.clone(),
            *self.unique_id,
            is_partial,
        );
        self.add_external_event(let_line, event);
        *self.unique_id += 1;

        if let Some(body_end) = emit_gos_at {
            self.add_external_event(body_end, ExternalEvent::GoOutOfScope {
                ro: binding_rap.clone(),
                id: *self.unique_id,
            });
            *self.unique_id += 1;
        }
        registered.insert(binding_rap);
    }
    registered
}

// This function does the work of adding an event for let stmts
pub fn match_rhs(&mut self, lhs: ResourceTy, rhs:&'tcx Expr, evt: Evt){
  // An expression of type `!` (`unreachable!()`, `panic!()`, `return`,
  // `continue`) never produces a value, so `lhs` gets nothing from it:
  // `_ => unreachable!()` in `let s = match … { … }` doesn't bind `s`
  // (#296). (`return v` hands `v` to the caller, not to `lhs`.)
  if self.tcx.typeck(rhs.hir_id.owner).expr_ty_opt(rhs).is_some_and(|ty| ty.is_never()) {
    return;
  }
  // Macro-expanded RHS — `let v = vec![1, 2, 3]`, `let s = format!(...)`,
  // etc. — desugars into a `Call` whose function path was never registered
  // as a RAP (visit_expr's from_expansion guard skipped it). Treating
  // such an RHS like a literal — Bind from Anonymous — gives the user a
  // clean "v acquires ownership of a resource" event without trying to
  // name the synthetic call inside the macro.
  if rhs.span.from_expansion() {
    // Walk the macro expansion chain back to the user's source span.
    // Without this, `rhs.span` for `let v = vec![…]` points into the
    // macro's own source file (e.g. `liballoc/macros.rs`), so the
    // Bind event is attributed to a line that doesn't exist in the
    // user's snippet — `compute_states` then sees the Acquire event
    // ordered *after* the binding's GoOutOfScope at end-of-fn, and
    // the rendered timeline column has no visible state segment or
    // drop. `source_callsite` resolves to the user's `let` line.
    let user_span = rhs.span.source_callsite();
    let line_num = span_to_line(&user_span, &self.tcx);
    self.bind_from_anonymous(line_num, lhs);
    return;
  }
  match rhs.kind {
    ExprKind::Path(QPath::Resolved(_,p)) => {
      // const / static items aren't function-local RAPs — they're
      // compile-time values (or `'static` references) baked into
      // the binary. Treat reading one on the RHS of a let as if
      // the RHS were a literal: the LHS gets a Bind from an
      // Anonymous source. Same handling as the path-arm in
      // visitor.rs's main expression walker.
      //
      // A function item (`let f = helper;`, `let d = std::mem::drop;`)
      // isn't a RAP either: the function itself has no column, and
      // the value bound is a Copy function pointer that owns nothing
      // (#254). Without this, a callee registered earlier in the same
      // fn would show up as the source of a copy into `f`.
      if matches!(
        p.res,
        Res::Def(
          rustc_hir::def::DefKind::Const { .. }
            | rustc_hir::def::DefKind::AssocConst { .. }
            | rustc_hir::def::DefKind::Static { .. }
            | rustc_hir::def::DefKind::Fn
            | rustc_hir::def::DefKind::AssocFn,
          _
        )
      ) {
        let line_num = span_to_line(&p.span, &self.tcx);
        self.bind_from_anonymous(line_num, lhs);
        return;
      }
      let line_num = span_to_line(&p.span, &self.tcx);
      let rhs_name: String = match p.res {
        Res::Def(rustc_hir::def::DefKind::Ctor(_, _), _) => {
          let mut name = String::new();
          for (i, segment) in p.segments.iter().enumerate() {
            name.push_str(self.tcx.hir_name(segment.hir_id).as_str());
            if i < p.segments.len() - 1 {
              name.push_str("::");
            }
          }
          name
        }
        _ => {
          self.tcx.hir_name(p.segments[0].hir_id).as_str().to_owned()
        }
      };
      // Last-resort fallback: a path that doesn't resolve to a
      // registered RAP gets an Anonymous source instead of
      // panicking. The `?` desugar used to be the main way to land
      // here (the synthetic `val` in `Continue(val) => val`), but
      // the Match arm below now unwraps `?` to its operand before
      // we get this far. If this fires, some other desugaring is
      // leaking a synthetic local — worth a look, hence the warn.
      let rhs_key = if matches!(p.res, Res::Local(_)) { rhs_name.clone() } else { fn_key(&rhs_name) };
      match self.raps.get(&rhs_key) {
        Some(rd) => {
          let rhs_rap = rd.rap.to_owned();
          self.update_rap(&rhs_rap, line_num);
          self.add_ev(line_num, evt, lhs, ResourceTy::Value(rhs_rap), false);
        }
        None => {
          warn!(
            "match_rhs: path `{}` (line {}) has no registered RAP; binding {} from an anonymous source",
            rhs_name, line_num, lhs.name()
          );
          self.add_ev(line_num, Evt::Bind, lhs, ResourceTy::Anonymous, false);
        }
      }
    },
    // `Type::item` (`let f = String::len;`, `let n = Self::N;`): an
    // associated function or const, never a local, so like the
    // fn-item / const case above the LHS gets a Bind from an
    // Anonymous source.
    ExprKind::Path(QPath::TypeRelative(..)) => {
      let line_num = span_to_line(&rhs.span, &self.tcx);
      self.add_ev(line_num, Evt::Bind, lhs, ResourceTy::Anonymous, false);
    }
    // fn_expr: resolves to function itself (Path)
    // second arg, is a list of args to the function
    ExprKind::Call(fn_expr, _) => {
      let line_num = span_to_line(&fn_expr.span, &self.tcx);
      // Fall back to Anonymous when the call doesn't have a name we
      // can resolve. Two cases land here: (1) a macro-expanded call
      // (e.g. `vec![1,2,3]` lowering to `<[_]>::into_vec(…)`) whose
      // name was never registered via add_fn because visit_expr
      // returned early on `from_expansion`; (2) a desugaring whose
      // fn_expr is `QPath::LangItem` that hirid_to_var_name can't
      // name. Same rationale as the Path / Field arms in get_rap.
      //
      // When match_rhs runs *before* visit_expr has reached this
      // call site (the let-as-rhs reordering for If/Match init in
      // process_let_binding), the fn name has a resolvable identity
      // but isn't in `self.raps` yet. Register it on the fly so the
      // Move/Copy arrow has a labeled source instead of falling back
      // to Anonymous.
      // A call through a local `f` (closure, fn pointer) that leaves
      // `f` in place: the result comes from the call, so its source is
      // the call's Function RAP, not `f`'s column (#254, #299). An
      // `FnOnce` call does consume `f`, and keeps `f` as the source.
      match local_callee_result_key(rhs, &self.tcx) {
        LocalCallee::Call(key) => {
          // Registered on the fly if visit_expr hasn't reached the call yet.
          if !self.raps.contains_key(&key) {
            if let Some(n) = hirid_to_var_name(fn_expr.hir_id, &self.tcx) {
              self.add_fn(n);
            }
          }
          let from = self.raps.get(&key)
            .map(|rd| ResourceTy::Value(rd.rap.to_owned()))
            .unwrap_or(ResourceTy::Anonymous);
          self.add_ev(line_num, evt, lhs, from, false);
          return;
        }
        LocalCallee::Consumed => {
          let from = hirid_to_var_name(fn_expr.hir_id, &self.tcx)
            .and_then(|n| self.raps.get(&n))
            .map(|rd| ResourceTy::Value(rd.rap.to_owned()))
            .unwrap_or(ResourceTy::Anonymous);
          self.add_ev(line_num, evt, lhs, from, false);
          return;
        }
        LocalCallee::NotLocal => {}
      }
      let from = match hirid_to_var_name(fn_expr.hir_id, &self.tcx) {
        Some(n) => {
          if !self.raps.contains_key(&fn_key(&n)) {
            self.add_fn(n.clone());
          }
          self.raps.get(&fn_key(&n))
            .map(|rd| ResourceTy::Value(rd.rap.to_owned()))
            .unwrap_or(ResourceTy::Anonymous)
        }
        None => ResourceTy::Anonymous,
      };
      self.add_ev(line_num, evt, lhs, from, false);
    },
    
    ExprKind::Lit(_) | ExprKind::Binary(..) | // Any type of literal on RHS implies a bind
    ExprKind::Unary(UnOp::Neg, _) | // ~<expr>
    ExprKind::Unary(UnOp::Not, _) // !<expr>
    => {
      // An overloaded operator (`a + b` with `impl Add for Tag`,
      // `-wave` with `impl Neg`) can return a struct; its fields
      // hold their parts of that new value (#314).
      let line_num = span_to_line(&rhs.span, &self.tcx);
      self.bind_from_anonymous(line_num, lhs);
    }
    // ex : &<expr> or &mut <expr>
    ExprKind::AddrOf(_, _,expr) => {
      let line_num = expr_to_line(&expr, &self.tcx);
      match fetch_rap(&expr, &self.tcx, &self.raps) {
        Some(rhs_rap) => {
          self.update_rap(&rhs_rap, expr_to_line(&expr, &self.tcx));
        }
        None => {} // taking addrOf some anonymous resource, ie: &String::from("")
      }
      let res = match fetch_rap(rhs, &self.tcx, &self.raps) {
        Some(x) => ResourceTy::Value(x),
        None => ResourceTy::Anonymous
      };
      // `&*r` / `&mut *r` with r a reference borrows *through* r (a
      // reborrow); `&r` / `&mut r` borrows the variable r itself.
      // Both resolve to the same source RAP, so decide here from the
      // syntax rather than from r's kind downstream.
      let is_reborrow = is_explicit_reborrow(
        innermost_addr_of_operand(rhs),
        self.tcx.typeck(rhs.hir_id.owner),
      );
      match fetch_mutability(&rhs) { // fetch last ref mutability in the chain -> &&&mut x
        Some(Mutability::Not) => self.add_borrow_ev(line_num, Evt::SBorrow, lhs, res, is_reborrow),
        Some(Mutability::Mut) => self.add_borrow_ev(line_num, Evt::MBorrow, lhs, res, is_reborrow),
        None => panic!("Shouldn't have been able to get here")
      }
    }
    //a block: { <stmt1>...<stmt_n>, <expr> };
    ExprKind::Block(block, _) => {
      // set new scope when entering a block (span.hi refers to the ending brace of the block)
      let prev_scope = self.current_scope;
      let new_scope = self.tcx.sess.source_map().lookup_char_pos(rhs.span.hi()).line;
      self.current_scope = new_scope;
      self.current_scope = prev_scope;
      // then, if the block has a return expr
      match block.expr {
        Some(res_expr) => {
          self.match_rhs(lhs.clone(), res_expr, evt);
        }
        None => {}
      }
    }
    ExprKind::Unary(option, expr) => {
      match option {
        rustc_hir::UnOp::Deref => {
          let line_num = expr_to_line(&expr, &self.tcx);
          let rhs_rap = fetch_rap(&expr, &self.tcx, &self.raps);
          let res = match rhs_rap {
            Some(x) => {
              self.update_rap(&x, line_num);
              ResourceTy::Deref(x)
            }
            None => {
              ResourceTy::Anonymous
            }
          };
          self.add_ev(line_num, evt, lhs, res, false);
        },
        _ => {}
      }
    } 
    ExprKind::MethodCall(name_and_generic_args, rcvr, _,  _) => {
      let line_num = span_to_line(&rcvr.span, &self.tcx);

      // If the lhs is a ref, the chain is most likely returning a
      // borrow rooted at some receiver in the chain (think
      // `n.split('.').next().expect(..)` returning `&str` borrowed
      // from n). define_lhs already populated `borrow_map[lhs]` by
      // walking the chain via find_lender; if it found a real
      // lender, emit a StaticBorrow / MutableBorrow from that
      // lender into lhs instead of the generic Copy/Move from the
      // outer function-call result. Otherwise (lender = Anonymous)
      // fall through to the historical behaviour.
      let chain_lender = match &lhs {
        ResourceTy::Value(rap) if rap.is_ref() => {
          self.borrow_map.get(rap.name()).map(|rd| rd.lender.clone())
        }
        _ => None,
      };
      if let Some(ResourceTy::Value(_)) = chain_lender {
        let lender_rty = chain_lender.unwrap();
        let borrow_evt = match &lhs {
          ResourceTy::Value(rap) if rap.is_mutref() => Evt::MBorrow,
          _ => Evt::SBorrow,
        };
        self.add_ev(line_num, borrow_evt, lhs, lender_rty, false);
        return;
      }

      // Same Anonymous fallback as the Call arm — chained / macro /
      // desugared method calls may not have a registered name.
      let from = hirid_to_var_name(name_and_generic_args.hir_id, &self.tcx)
        .and_then(|n| self.raps.get(&fn_key(&n)).map(|rd| rd.rap.to_owned()))
        .map_or(ResourceTy::Anonymous, ResourceTy::Value);
      self.add_ev(line_num, evt, lhs, from, false);
    }
    // Struct intializer list:
    // ex struct = {a: <expr>, b: <expr>, c: <expr>}
    ExprKind::Struct(_qpath, expr_fields, base) => {
      let line_num = span_to_line(&rhs.span, &self.tcx);
      self.add_ev(line_num, Evt::Bind, lhs.clone(), ResourceTy::Anonymous, false);
      for field in expr_fields.iter() {
          // Struct literal that isn't bound to a struct RAP: the
          // value is returned (`User { email, .. }` as a fn's tail
          // expression). The field initialisers still move/copy
          // their sources (field-init shorthand `email` moves the
          // local `email`), into the value handed to the caller.
          if matches!(lhs, ResourceTy::Caller) {
            let field_ty = self.tcx.typeck(field.expr.hir_id.owner).node_type(field.expr.hir_id);
            let is_place = matches!(field.expr.kind, ExprKind::Path(_) | ExprKind::Field(..));
            if is_place && !field_ty.is_ref() && !field.expr.span.from_expansion() {
              let e = if self.ty_is_copy(field_ty, field.expr.hir_id.owner) { Evt::Copy } else { Evt::Move };
              self.match_rhs(ResourceTy::Caller, field.expr, e);
            }
            continue;
          }
          let new_lhs_name = format!("{}.{}", lhs.name(), field.ident.as_str());
          // Skip fields whose RAP we never registered. The most
          // common case is a struct field whose type is itself a
          // struct (`R { a: A { b: ... } }`): define_lhs registers
          // `r.a` but doesn't recurse to add `r.a.b`, so the inner
          // struct's expansion lands here without an entry. Better
          // to lose the per-field event than crash the whole render.
          let field_rap = match self.raps.get(&new_lhs_name) {
            Some(rd) => rd.rap.to_owned(),
            None => continue,
          };
          let field_ty = self.tcx.typeck(field.expr.hir_id.owner).node_type(field.expr.hir_id);
          let is_copyable = self.tcx.type_is_copy_modulo_regions(rustc_middle::ty::TypingEnv::post_analysis(self.tcx, field.expr.hir_id.owner), field_ty);
          let e = if field_ty.is_ref() {
            match field_ty.ref_mutability().unwrap() {
              Mutability::Not => Evt::Copy,
              Mutability::Mut => Evt::Move,
            }
          } else {
            match is_copyable {
              true => Evt::Copy,
              false => Evt::Move
            }
          };
          // For ref-typed fields, define_lhs registered the field
          // as a StaticRef/MutRef with a placeholder Anonymous
          // lender. Resolve the real lender from the field's
          // initialiser expression here (same path as a `let r =
          // …` outside a struct) so print_lifetimes can pair the
          // borrow with its return-of-borrow at end-of-loan and
          // the ref-line trapezoid covers the right range.
          if field_ty.is_ref() {
            let (lender, aliasing, static_lender) = self.get_ref_data_full(&field.expr);
            if let Some(rd) = self.borrow_map.get_mut(&new_lhs_name) {
              rd.lender = lender;
              rd.aliasing = aliasing;
              rd.static_lender = static_lender;
            }
          }
          self.match_rhs(ResourceTy::Value(field_rap), field.expr, e);
      }
      // Struct update syntax `S { a: e, ..base }`: every field not
      // written explicitly comes from `base`, moved if its type isn't
      // Copy and copied otherwise. That's a *partial* move of `base`:
      // its explicitly-overridden fields stay put and are still
      // dropped at its scope end.
      if let rustc_hir::StructTailExpr::Base(base_expr) = base {
        let base_line = span_to_line(&base_expr.span, &self.tcx);
        let base_name = expr_to_rap_name(base_expr, &self.tcx)
          .filter(|n| self.raps.contains_key(n));
        let explicit: HashSet<String> = expr_fields.iter().map(|f| f.ident.as_str().to_owned()).collect();
        // Field names come from whichever side has per-field RAPs:
        // the new struct, or (when it's returned to the caller) the base.
        let (field_names, dest_prefix): (Vec<(String, bool)>, Option<String>) = match &lhs {
          ResourceTy::Value(rap @ ResourceAccessPoint::Struct(_)) => {
            let prefix = rap.name().clone();
            (self.struct_children(&prefix).iter()
              .map(|c| (c.name()[prefix.len() + 1..].to_owned(), c.is_copy()))
              .collect(), Some(prefix))
          }
          ResourceTy::Caller => match &base_name {
            Some(b) => (self.struct_children(b).iter()
              .map(|c| (c.name()[b.len() + 1..].to_owned(), c.is_copy()))
              .collect(), None),
            None => (Vec::new(), None),
          },
          _ => (Vec::new(), None),
        };
        for (field, field_is_copy) in field_names {
          if explicit.contains(&field) {
            continue;
          }
          let dest = match &dest_prefix {
            Some(p) => match self.raps.get(&format!("{}.{}", p, field)) {
              Some(rd) => ResourceTy::Value(rd.rap.clone()),
              None => continue,
            },
            None => ResourceTy::Caller,
          };
          let src = base_name.as_ref()
            .and_then(|b| self.raps.get(&format!("{}.{}", b, field)))
            .map(|rd| rd.rap.clone());
          match src {
            Some(src_rap) => {
              let e = if field_is_copy { Evt::Copy } else { Evt::Move };
              self.add_ev(base_line, e, dest, ResourceTy::Value(src_rap), false);
            }
            // `..Default::default()` and other non-place bases: the
            // field gets its value from a temporary.
            None => {
              if let ResourceTy::Value(d) = &dest {
                let d_name = d.name().clone();
                self.add_ev(base_line, Evt::Bind, dest.clone(), ResourceTy::Anonymous, false);
                self.bind_struct_fields(base_line, &d_name);
              }
            }
          }
        }
      }
    },

    ExprKind::Field(inner, id) => {
      // Walk the receiver to a qualified name; emit the event only
      // if we actually have a RAP under that name. Anything else
      // (nested field access we never registered, `self.field` in
      // an impl method, fields through a deref/borrow) falls
      // through silently rather than panicking.
      // Union fields (`unsafe { u._a }`) resolve to the union's own
      // column via field_rap, so the read shows as a copy / move out
      // of `u`.
      if let Some(rhs_rap) = field_rap(inner, id.as_str(), &self.tcx, &self.raps) {
        let line_num = span_to_line(&inner.span, &self.tcx);
        self.add_ev(line_num, evt, lhs, ResourceTy::Value(rhs_rap), false);
      }
    }
    // explicitly using return keyword
    // ex: return <expr>
    ExprKind::Ret(ret) => {
      match ret {
        Some(ret_expr) => {
          self.match_rhs(lhs, ret_expr, evt);
        }
        // returning void, nothing happens
        None => {}
      }
    },
    // `let x = if cond { e1 } else { e2 };` — recurse into each
    // branch's trailing expression so the move/copy/bind arrow lands
    // on the LHS once per branch. The branches themselves rendered
    // their inner side-effects when visit_expr walked them earlier;
    // this arm is purely about pairing the branch result with `lhs`.
    ExprKind::If(_, if_block, else_block) => {
      self.match_rhs_arm(lhs.clone(), &if_block, evt.clone());
      match else_block {
        Some(e) => self.match_rhs_arm(lhs, &e, evt),
        None => {}
      }
    }
    // `let x = match scrutinee { … };` — same shape as If above.
    // Each arm's body becomes the value bound to `lhs` on the path
    // through that arm. Pre-fix this fell through silently and a
    // match-as-rhs binding got zero move/copy arrows, even though
    // the equivalent if/else form rendered fine (issue #87).
    //
    // Exception: the `?` operator. On the pinned nightly `e?`
    // lowers to
    //
    //   match Try::branch(e) {                      // MatchSource::TryDesugar
    //     ControlFlow::Break(residual) => return FromResidual::from_residual(residual),
    //     ControlFlow::Continue(val) => val,
    //   }
    //
    // Neither arm is user code: the Break arm diverges (it never
    // produces the value bound to `lhs`), and the Continue arm's
    // `val` is a synthetic local with no RAP. Walking them emitted
    // two anonymous Binds on the same line and lost the call the
    // value actually came from. Instead, pair `lhs` with `e`, the
    // argument of the `Try::branch(..)` scrutinee, as if the `?`
    // weren't there.
    ExprKind::Match(scrutinee, arms, source) => {
      if let rustc_hir::MatchSource::TryDesugar(_) = source {
        match scrutinee.kind {
          ExprKind::Call(_, [operand]) => {
            self.match_rhs(lhs, operand, evt);
          }
          _ => {
            // Not the shape we expect from lower_expr_try. Don't
            // walk the synthetic arms; bind anonymously at the `?`
            // expression's line instead.
            warn!("match_rhs: unexpected `?` desugar scrutinee {:?}", scrutinee.kind);
            let line_num = span_to_line(&rhs.span, &self.tcx);
            self.add_ev(line_num, Evt::Bind, lhs, ResourceTy::Anonymous, false);
          }
        }
        return;
      }
      for arm in arms.iter() {
        self.match_rhs_arm(lhs.clone(), arm.body, evt.clone());
      }
    }
    ExprKind::DropTemps(exp) => {
      self.match_rhs(lhs, &exp, evt);
    }
    // `let f = move || ...` / `let f = || ...`. The closure itself
    // is conceptually a small struct that holds the upvars it
    // captures from the enclosing scope. Bind `f` to that struct
    // (Anonymous source — we don't render the closure body), then
    // emit one capture event per upvar pulled from rustc's
    // `closure_min_captures_flattened`. Capture kind decides the
    // event shape:
    //   ByValue (always for `move`, sometimes for inferred captures)
    //     → Move(upvar → f). f now owns the captured resource.
    //     If the captured place is Copy (`move || m + 1` with
    //     m: i32), it's copied instead: Copy(upvar → f), and the
    //     upvar stays usable (#311).
    //   ByRef(Immutable | UniqueImmBorrow) → SBorrow(upvar → f).
    //   ByRef(Mutable)                     → MBorrow(upvar → f).
    //   ByUse (`use` capture clause, recent feature) → Move,
    //     same as ByValue for our purposes.
    //
    // Since Rust 2021 a closure captures only the places it uses
    // (RFC 2229, "disjoint captures"): `move || p.name.len()` takes
    // `p.name` and leaves `p.nick` with `p`. Each capture is drawn on
    // the column of the place it captures (`p.name`), or of its
    // nearest enclosing place that has a column (#328).
    ExprKind::Closure(closure) => {
      let line_num = span_to_line(&rhs.span, &self.tcx);
      // Bind for the closure value itself — gives `f` an "acquires
      // ownership of a resource" dot like other Bind sites.
      self.add_ev(line_num, Evt::Bind, lhs.clone(), ResourceTy::Anonymous, false);

      // MIR loan facts for the enclosing fn body. Each ref capture
      // becomes one polonius loan whose `borrowed_place` is the
      // upvar and whose `region.1` is the NLL kill line — the line
      // where the borrow ends, which under NLL is the closure
      // binding's last use, not its lexical scope end. We use this
      // below to emit a StaticDie / MutableDie at the kill line so
      // the captured borrow visually returns to its lender on the
      // line the user expects.
      let mir_b_data = self.gather_borrow_data(self.bwf);

      let typeck = self.tcx.typeck(closure.def_id);
      for capture in typeck.closure_min_captures_flattened(closure.def_id) {
        let upvar_hir_id = match capture.place.base {
          rustc_middle::hir::place::PlaceBase::Upvar(upvar_id) => upvar_id.var_path.hir_id,
          _ => continue,
        };
        // The root variable's name (MIR loans below are keyed on it)
        // and the captured place's column: `p.name` for a disjoint
        // field capture, `p` for a whole-variable one.
        let upvar_name = self.tcx.hir_name(upvar_hir_id).as_str().to_owned();
        let by_value = matches!(capture.info.capture_kind,
          rustc_middle::ty::UpvarCapture::ByValue | rustc_middle::ty::UpvarCapture::ByUse);
        // A by-value capture of a Copy place copies it into the
        // closure (#311): `m` is still usable afterwards and the
        // closure has nothing of m's to drop. A Copy field
        // (`move || p.x`) is drawn on its own column (#328).
        let env = rustc_middle::ty::TypingEnv::post_analysis(self.tcx, rhs.hir_id.owner);
        let copied = by_value && self.tcx.type_is_copy_modulo_regions(env, capture.place.ty());
        let upvar_rap = match self.capture_rap(&capture.place, &upvar_name) {
          Some(rap) => rap,
          None => continue,
        };
        // A closure that uses a reference variable through it
        // (`|| r.push('a')`, `|| r.len()`) captures the place `*r`,
        // i.e. it reborrows what r points to rather than borrowing r
        // itself (#277). Tagged like `&mut *r` (`is_reborrow`).
        let through_ref = upvar_rap.is_ref()
          && matches!(capture.place.projections.first(),
                      Some(p) if matches!(p.kind, rustc_middle::hir::place::ProjectionKind::Deref));
        let from = ResourceTy::Value(upvar_rap);
        let to = lhs.clone();
        let evt_kind = match capture.info.capture_kind {
          _ if copied => Evt::Copy,
          rustc_middle::ty::UpvarCapture::ByValue
          | rustc_middle::ty::UpvarCapture::ByUse => Evt::Move,
          rustc_middle::ty::UpvarCapture::ByRef(kind) => match kind {
            rustc_middle::ty::BorrowKind::Mutable => Evt::MBorrow,
            // Immutable + UniqueImmBorrow both render as a static
            // (immutable) borrow from the user's perspective —
            // UniqueImmBorrow is an internal closure-borrow flavor
            // they can't write, and the visualization shouldn't
            // distinguish it.
            _ => Evt::SBorrow,
          },
        };
        match evt_kind {
          Evt::SBorrow | Evt::MBorrow =>
            self.add_borrow_ev(line_num, evt_kind.clone(), to, from.clone(), through_ref),
          _ => self.add_ev(line_num, evt_kind.clone(), to, from.clone(), false),
        }

        // For ref captures, find the matching MIR loan and emit
        // a Die event at its kill line so the borrow returns on
        // the closure's last use, not at the closure's lexical
        // scope end. Move captures don't go through the loan
        // machinery — they transfer ownership outright, and the
        // closure's drop is what releases them.
        let die_evt = match evt_kind {
          Evt::SBorrow => Some(Evt::SDie),
          Evt::MBorrow => Some(Evt::MDie),
          _ => None,
        };
        if let Some(die_evt) = die_evt {
          let kill_line = mir_b_data.iter().find_map(|b| {
            // Match on (assignment line, upvar name): the loan
            // for this capture is issued at the closure literal's
            // line and borrows the upvar by name. region.1 is the
            // NLL-refined kill line.
            if b.region.0 == line_num
              && b.borrowed_place.as_deref() == Some(upvar_name.as_str())
            {
              Some(b.region.1)
            } else {
              None
            }
          });
          if let Some(die_line) = kill_line {
            // add_ev maps (lhs, rhs) → StaticDie/MutableDie
            // {from: rhs, to: lhs}. We want from=closure (f),
            // to=upvar (s) so the renderer's return arrow goes
            // f → s, mirroring the borrow direction.
            self.add_ev(die_line, die_evt, from, lhs.clone(), false);
          }
        }
      }
    }
    _ => {
      warn!("unmatched rhs {:#?}", rhs);
    }
  }
}

// add GOS events for RAPs in the current scope  
pub fn print_out_of_scope(&mut self){
  // A `return` whose drops never got folded into a diverging branch
  // (a bare `return` in a loop body, an `if` without `else`, a
  // single-arm match) is drawn as before, without them: on the
  // function's own column they'd cut it short for the other paths.
  let exit_drops = std::mem::take(&mut self.exit_drops);
  self.preprocessed_events.retain(|(_, e)| !exit_drops.contains(&e.get_id()));
  // Visit RAPs in declaration order, not `self.raps`' hash order, so
  // the GoOutOfScope events (and their ids) come out the same on
  // every run (#211).
  #[expect(clippy::disallowed_methods, reason = "sorted by (hash, name) on the next line")]
  let mut raps: Vec<RapData> = self.raps.values().cloned().collect();
  raps.sort_by(|a, b| a.rap.hash().cmp(b.rap.hash()).then_with(|| a.rap.name().cmp(b.rap.name())));
  for rap in raps.iter() {
    // need this to avoid duplicating out of scope events, this is due to the fact that RAPS is a map that lives over multiple fn ctxts
    // if a rap isn't global it means we annotated its GOS event inside its respective branch already
    if !rap.rap.is_fn() && rap.is_global {
      let mut duplicate = false;
      for (_l, e) in self.preprocessed_events.iter() {
        match e.is_gos_ev() {
          Some(r) => {
            if *r == rap.rap {
              duplicate = true;
              break;
            }
          }
          None => {}
        }
      }
      if !duplicate {
        self.add_external_event(rap.scope, ExternalEvent::GoOutOfScope { ro: rap.rap.clone(), id: *self.unique_id });
        *self.unique_id += 1;
      }
    }
  }
}
  
  // This is where we annotate all RefDie events that were not handled by the assignment operator
  pub fn print_lifetimes(&mut self){

    // first refine the loans that we've computed using MIR information
    // this is necessary for some cases where we can't know where a lifetime actually ends
    // see tests/basic_if7.rs for an example
    let mir_b_data = self.gather_borrow_data(&self.bwf);
    // A reference made by `&x` / `&mut x` takes the loan issued at that
    // exact expression (#233). The others fall back to matching on
    // (line, lender name), skipping loans already taken by span, so
    // `let a = &x; let b = &x;` can't hand `b` the loan of `a`. Each
    // pass is independent of borrow_map's iteration order.
    let mut by_span: HashSet<usize> = HashSet::new();
    #[expect(clippy::disallowed_methods, clippy::iter_over_hash_type, reason = "each entry is refined on its own; the by_span set is order-independent")]
    for (_name, data) in self.borrow_map.iter_mut() {
      if let Some(m_data) = mir_b_data.iter().find(|m| ExprVisitor::borrow_span_match(data, m)) {
        data.lifetime = m_data.region.1;
        by_span.insert(m_data.borrow_idx);
      }
    }
    #[expect(clippy::disallowed_methods, clippy::iter_over_hash_type, reason = "each entry is refined on its own from the mir_b_data Vec")]
    for (_name, data) in self.borrow_map.iter_mut() {
      if data.borrow_span.is_some_and(|sp| mir_b_data.iter().any(|m| m.span == sp)) {
        continue; // refined above
      }
      for m_data in mir_b_data.iter().filter(|m| !by_span.contains(&m.borrow_idx)) {
        match ExprVisitor::borrow_match(data, m_data) {
          Some(kill) => {
            data.lifetime = kill;
            break;
          }
          None => {}
        }
      }
    }
    
    info!("BORROW MAP {:#?}", self.borrow_map);
    //let lifetime_map = self.lifetime_map.clone();
    let mut ultimate_refs: HashSet<String> = HashSet::new();
    let lender_to_refs = get_non_anon_lenders(&self.borrow_map);
    info!("lender to refs {:#?}", lender_to_refs);

    // Loop through each lender's active references and pick out the
    // "ultimate" refs in each region — the ones that emit visible
    // return arrows back to the lender at end-of-lifetime. Previously
    // this picked exactly one ref per region (the one with the
    // longest lifetime) and any other ref tied with it became a
    // RefDie (which the renderer drops on the floor). For parallel
    // borrows that die at the same line — e.g. `let r1 = &s; let
    // r2 = &s; assert!(compare_strings(r1, r2));` where r1 and r2
    // both last-use at the assert line — the book's RV1 rendered an
    // arrow for both. Pick *all* refs whose lifetime equals the
    // region's max so they each emit a StaticDie/MutableDie. Refs
    // that die strictly earlier than the region's max stay
    // non-ultimate and remain RefDie (correct: they don't
    // visually represent the lender's borrow being given back).
    #[expect(clippy::disallowed_methods, clippy::iter_over_hash_type, reason = "only inserts into the ultimate_refs set")]
    for (_, refs) in lender_to_refs.iter() {
      let regions = get_regions(refs, &self.borrow_map);
      for region in regions.iter() {
        let mut max_lifetime: usize = 0;
        #[expect(clippy::iter_over_hash_type, reason = "computes a max")]
        for r in region {
          let lifetime = self.borrow_map.get(r).unwrap().lifetime;
          if lifetime > max_lifetime {
            max_lifetime = lifetime;
          }
        }
        #[expect(clippy::iter_over_hash_type, reason = "only inserts into the ultimate_refs set")]
        for r in region {
          if self.borrow_map.get(r).unwrap().lifetime == max_lifetime {
            ultimate_refs.insert(r.to_owned());
          }
        }
      }
    }

    // if a reference borrowed from an anonymous owner then it must be an ultimate ref
    #[expect(clippy::disallowed_methods, clippy::iter_over_hash_type, reason = "only inserts into the ultimate_refs set")]
    for (name, ref_data) in self.borrow_map.iter() {
      match ref_data.lender {
        ResourceTy::Anonymous => {
          ultimate_refs.insert(name.to_owned());
        }
        _ => {}
      }
    }
    let b_map = self.borrow_map.clone();

    // sort by number of aliases so events are ordered in a proper cascading fashion.
    // `b_map` is a HashMap, so refs with the same alias count used to
    // keep its per-process random order, and two return arrows on the
    // same line (`let t = s; println!("{} {}", s, t);`) swapped places
    // from run to run (#211). Break ties in source order: where the
    // borrow was made, then declaration order of the ref, then name.
    let mut vec: Vec<(String, RefData)> = b_map.into_iter().collect();
    vec.sort_by(|a, b| {
      b.1.aliasing.len().cmp(&a.1.aliasing.len())
        .then_with(|| a.1.assigned_at.cmp(&b.1.assigned_at))
        .then_with(|| {
          let hash_of = |k: &String| self.raps.get(k).map(|r| *r.rap.hash());
          hash_of(&a.0).cmp(&hash_of(&b.0))
        })
        .then_with(|| a.0.cmp(&b.0))
    });

    // Add events
    for (k, ref_data) in &vec {
      let RefData { lender: r_ty, lifetime, .. } = ref_data;
      let from_ro = self.raps.get(k).unwrap().rap.to_owned();
      let to_ro = match r_ty {
        ResourceTy::Anonymous => ResourceTy::Deref(from_ro.clone()),
        _ => r_ty.clone()
      };
      // if ref is not an ultimate ref then it dies without returning a resource
      if !ultimate_refs.contains(k) { 
        self.add_external_event(*lifetime, ExternalEvent::RefDie { 
          from: ResourceTy::Value(from_ro.clone()), to: to_ro, num_curr_borrowers: get_borrowers(from_ro.name(), &self.borrow_map).len() - 1, 
          id: *self.unique_id });
        *self.unique_id += 1;
      }
      else {
        // otherwise it does return a resource
        self.add_borrow_end_ev(*lifetime, to_ro, ResourceTy::Value(from_ro), ref_data);
      }
    }
  }
}

/// Whether `rap_name` owns a resource after `events`, judged from the
/// newest ownership-affecting event (borrows and lends don't count);
/// None if no event decides it. A finished if/else or match sits in
/// the list as one Branch event, decided by its arms: the variable
/// holds a resource if it does at the end of some arm, and doesn't if
/// every arm gave it up; otherwise the state from before the branch
/// stands. Without looking inside, a reassignment after
/// `if c { s = a } else { s = b }` (#234) showed no drop, and one
/// after `if c { take(s) } else { take(s) }` a drop of the
/// already-moved value.
fn holds_resource_after(events: &[(usize, ExternalEvent)], rap_name: &str) -> Option<bool> {
  let is_rap = |r: &ResourceTy| r.extract_rap().map_or(false, |r| r.name() == rap_name);
  for (_, ev) in events.iter().rev() {
    match ev {
      ExternalEvent::ManuallyDropByHand { ro, .. } if ro.name() == rap_name => {
        return Some(false);
      }
      ExternalEvent::Move { from, to, .. } => {
        if is_rap(to) {
          return Some(true);
        }
        if is_rap(from) {
          return Some(false);
        }
      }
      ExternalEvent::Copy { to, .. } | ExternalEvent::Bind { to, .. } => {
        if is_rap(to) {
          return Some(true);
        }
      }
      ExternalEvent::Branch { branches, .. } => {
        let arms: Vec<Option<bool>> =
          branches.iter().map(|b| holds_resource_after(&b.e_data, rap_name)).collect();
        if arms.contains(&Some(true)) {
          return Some(true);
        }
        if !arms.is_empty() && arms.iter().all(|a| *a == Some(false)) {
          return Some(false);
        }
        // Otherwise (no arm decides, or it was moved out in some arms
        // and untouched in the others) keep looking before the branch.
      }
      _ => {}
    }
  }
  None
}
