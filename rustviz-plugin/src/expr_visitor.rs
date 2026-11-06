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
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
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
/// reference-typed fields (`None`: register them as plain members;
/// no site does that since #352).
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
  // Further lenders, for a reference returned by a call whose result
  // is tied to several arguments (`longest(&a, &b)` borrows from both
  // `a` and `b`; #171). Empty for every other reference.
  pub co_lenders: Vec<ResourceTy>,
}

impl RefData {
  /// `lender` followed by `co_lenders`.
  pub fn all_lenders(&self) -> Vec<ResourceTy> {
    let mut v = vec![self.lender.clone()];
    v.extend(self.co_lenders.iter().cloned());
    v
  }
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
  // `let r = &String::from("a");`: the column name of the temporary r
  // borrows, which lives as long as r's `let` does (#400). Lets the
  // branch / block code that ends r's scope end the temporary's too.
  pub extended_temp: Option<String>,
  // An unbound temporary's (#457) declaring scope: the block, or match
  // arm body, whose branch declares it, so a branch's decl set covers it.
  pub temp_owner: Option<rustc_hir::HirId>,
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
  /// References that point into something the plugin can't name (the
  /// loop variable of `for x in it`, #417): they get no lender, so no
  /// borrow is "returned" when they end.
  pub unlent_refs: HashSet<String>,
  // Temporaries given a column of their own in this fn body because a
  // `let` borrows them (`let r = &String::from("a");`, #400); numbers
  // the next one's label.
  pub temp_count: usize,

  // Calls whose temporary result's drop has been stated (#457), so a
  // call the visitor reaches twice says it once.
  pub temp_drops_noted: HashSet<rustc_hir::HirId>,
  // State for the destructuring assignment being visited
  // (`(a, b) = (b, a);`, see `visit_destructuring_assign`). rustc
  // lowers it to `{ let (lhs, lhs) = (b, a); a = lhs; b = lhs; }`;
  // the `lhs` temporaries get no columns. Instead each `a = lhs` is
  // drawn as `a = <the part of the RHS that lhs held>`.
  //   destructure_srcs: `lhs` binding → that RHS part.
  //   destructure_moved: RAPs whose value the statement moves out
  //     (their old value isn't dropped when they're assigned).
  //   destructure_ref_data: a reference destination's new lender,
  //     computed before any part of the statement is drawn.
  pub destructure_srcs: HashMap<rustc_hir::HirId, &'tcx Expr<'tcx>>,
  pub destructure_moved: HashSet<String>,
  /// Persistent: (name, first line, last line) of each destination
  /// of a destructuring assignment whose old value the same statement
  /// moves out (`m1` in `(m1, m2) = (m2, m1)`). That move hands the
  /// old borrow on to another reference; it isn't a move of the
  /// value assigned in the statement, so `ref_handed_off` ignores it.
  pub destructure_swapped: Vec<(String, usize, usize)>,
  pub destructure_ref_data: HashMap<rustc_hir::HirId, (ResourceTy, VecDeque<String>, Option<StaticLender>, Vec<ResourceTy>)>,


  // Variables moved in a branch that then `break`s out of a loop
  // (#344), recorded when the branch is built; the loop turns them
  // into `LoopExitMove` events at its end.
  pub break_moves: Vec<BreakMove>,
}

/// `rap` was moved in a branch that leaves through the `break` `brk`
/// out of the loop (or labeled block) `target`. `definite`: on every
/// path to `brk`.
#[derive(Debug, Clone)]
pub struct BreakMove {
  pub target: rustc_hir::HirId,
  pub brk: rustc_hir::HirId,
  pub rap: ResourceAccessPoint,
  pub definite: bool,
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

  /// True iff `ty` is not `Copy` but has nothing to drop (no drop glue):
  /// an iterator over borrowed data, a struct of integers that doesn't
  /// derive `Copy`. Tooltips call what such a column holds a "value",
  /// not a "resource" (#484: a resource is something that needs cleaning
  /// up). A union, a `ManuallyDrop` and a closure keep the resource
  /// wording: they have their own handling.
  ///
  /// `needs_drop` alone is host-dependent for the std lock types: on
  /// macOS (pthread-backed) `Mutex<i32>` has drop glue, on Linux and
  /// Windows (futex / SRW locks) it doesn't. So a type that is, or
  /// contains, a std `Mutex` / `RwLock` / `Condvar` / `Barrier` is
  /// always a resource: they own an OS synchronization object, and the
  /// wording must not depend on the platform's std internals.
  pub fn ty_is_plain(&self, ty: Ty<'tcx>, owner: rustc_hir::OwnerId) -> bool {
    let env = rustc_middle::ty::TypingEnv::post_analysis(self.tcx, owner);
    !self.ty_is_copy(ty, owner)
      && !ty.needs_drop(self.tcx, env)
      && !self.ty_has_std_sync_primitive(ty)
      && !ty.is_union()
      && !ty.is_closure()
      && !ty.ty_adt_def().map_or(false, |d| d.is_manually_drop())
      && !ty.is_ref()
  }

  /// True iff `ty` is, or contains (through generic arguments, tuples,
  /// arrays, or the fields of local ADTs), one of std's lock types.
  /// See `ty_is_plain` for why these are special-cased.
  fn ty_has_std_sync_primitive(&self, ty: Ty<'tcx>) -> bool {
    fn go<'tcx>(tcx: TyCtxt<'tcx>, ty: Ty<'tcx>, depth: usize, seen: &mut HashSet<Ty<'tcx>>) -> bool {
      if depth > 8 || !seen.insert(ty) { return false; }
      match ty.kind() {
        rustc_middle::ty::Adt(def, args) => {
          let did = def.did();
          if !did.is_local()
            && matches!(tcx.crate_name(did.krate).as_str(), "std" | "alloc" | "core")
            && matches!(tcx.item_name(did).as_str(), "Mutex" | "RwLock" | "Condvar" | "Barrier")
          {
            return true;
          }
          args.types().any(|t| go(tcx, t, depth + 1, seen))
            || (did.is_local()
                && def.all_fields().any(|f| go(tcx, f.ty(tcx, args), depth + 1, seen)))
        }
        rustc_middle::ty::Array(t, _) => go(tcx, *t, depth + 1, seen),
        rustc_middle::ty::Tuple(ts) => ts.iter().any(|t| go(tcx, t, depth + 1, seen)),
        _ => false,
      }
    }
    go(self.tcx, ty, 0, &mut HashSet::new())
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
  pub fn add_owner(&mut self, name: String, mutability: bool, is_copy: bool, is_plain_value: bool, interior_mut: bool, scope: usize, is_global: bool) {
    self.add_rap(ResourceAccessPoint::Owner(Owner{name: name, hash: self.rap_hashes as u64, is_mut: mutability, is_copy, is_plain_value, interior_mut, is_closure: false, is_union: false, is_manually_drop: false, unbound_temp: false, write: Default::default(), holds_borrow: false, rc: None, rc_inside: None}), scope, is_global);
  }

  // Adds an owner RAP for a `union`-typed binding (one opaque column;
  // see the AdtKind::Union arms).
  pub fn add_union_owner(&mut self, name: String, mutability: bool, is_copy: bool, is_plain_value: bool, interior_mut: bool, scope: usize, is_global: bool) {
    self.add_rap(ResourceAccessPoint::Owner(Owner{name: name, hash: self.rap_hashes as u64, is_mut: mutability, is_copy, is_plain_value, interior_mut, is_closure: false, is_union: true, is_manually_drop: false, unbound_temp: false, write: Default::default(), holds_borrow: false, rc: None, rc_inside: None}), scope, is_global);
  }

  /// `Some(kind)` iff `ty` is `Rc<T>`, `Arc<T>` or one of their
  /// `Weak<T>`s, identified by std diagnostic item (not by name).
  pub fn rc_kind(&self, ty: Ty<'tcx>) -> Option<RcKind> {
    let did = ty.ty_adt_def()?.did();
    let tcx = self.tcx;
    if tcx.is_diagnostic_item(rustc_span::sym::Rc, did) { Some(RcKind::Rc) }
    else if tcx.is_diagnostic_item(rustc_span::sym::Arc, did) { Some(RcKind::Arc) }
    else if tcx.is_diagnostic_item(rustc_span::sym::RcWeak, did) { Some(RcKind::RcWeak) }
    else if tcx.is_diagnostic_item(rustc_span::sym::ArcWeak, did) { Some(RcKind::ArcWeak) }
    else { None }
  }

  /// `Some(k)` iff `ty` is a struct without a `Drop` impl whose fields
  /// that need drop are all `Rc`/`Arc`/`Weak`s of one kind `k` (at least
  /// one), so dropping it only lowers counts (#373).
  pub fn rc_fields_kind(&self, ty: Ty<'tcx>) -> Option<RcKind> {
    let adt = ty.ty_adt_def().filter(|d| d.is_struct())?;
    if adt.has_dtor(self.tcx) || adt.is_manually_drop() { return None; }
    let rustc_middle::ty::TyKind::Adt(_, args) = ty.kind() else { return None };
    let typing_env = rustc_middle::ty::TypingEnv::fully_monomorphized();
    let mut kind = None;
    for field in adt.all_fields() {
      let fty = field.ty(self.tcx, args);
      if !fty.needs_drop(self.tcx, typing_env) { continue; }
      let k = self.rc_kind(fty)?;
      if kind.is_some_and(|o| o != k) { return None; }
      kind = Some(k);
    }
    kind
  }

  /// `Some(Inside { .. })` iff `ty` is not itself an `Rc`/`Arc`/`Weak` but
  /// every part of it that needs drop is one, of a single kind, reached
  /// only through std containers whose drop just drops their elements:
  /// `Option`, `Box`, `Cell`, `RefCell`, tuples, arrays (no allocation of
  /// their own, apart from `Box`) and `Vec`, `VecDeque`, `HashMap`,
  /// `BTreeMap` (which also free a buffer). Any other type that needs
  /// drop (a `String`, a user type, which may have its own `Drop`, a
  /// trait object, ...) makes the answer `None` (#373).
  pub fn rc_inside_kind(&self, ty: Ty<'tcx>) -> Option<RcDrop> {
    use rustc_span::sym;
    use rustc_middle::ty::TyKind;
    struct Acc { kind: Option<RcKind>, count: usize, buffer: bool }
    fn go<'tcx>(me: &ExprVisitor<'_, 'tcx>, ty: Ty<'tcx>, acc: &mut Acc, depth: usize) -> bool {
      let tcx = me.tcx;
      let env = rustc_middle::ty::TypingEnv::fully_monomorphized();
      if !ty.needs_drop(tcx, env) { return true; }
      if depth > 8 { return false; }
      if let Some(k) = me.rc_kind(ty) {
        if acc.kind.is_some_and(|o| o != k) { return false; }
        acc.kind = Some(k);
        acc.count += 1;
        return true;
      }
      match ty.kind() {
        TyKind::Tuple(ts) => ts.iter().all(|t| go(me, t, acc, depth + 1)),
        TyKind::Array(t, _) => { acc.count += 1; go(me, *t, acc, depth + 1) }
        TyKind::Adt(def, args) => {
          let did = def.did();
          let item = |s| tcx.is_diagnostic_item(s, did);
          let buffer = item(sym::Vec) || (!did.is_local() && tcx.crate_name(did.krate) == sym::alloc && tcx.item_name(did).as_str() == "VecDeque") || item(sym::HashMap) || item(sym::BTreeMap);
          if !(ty.is_box() || buffer || item(sym::Option) || item(sym::Cell) || item(sym::RefCell)) {
            return false;
          }
          if buffer { acc.buffer = true; }
          // Allocator / hasher arguments that need drop make it `Other`.
          args.types().all(|t| go(me, t, acc, depth + 1))
        }
        _ => false,
      }
    }
    if self.rc_kind(ty).is_some() { return None; }
    let mut acc = Acc { kind: None, count: 0, buffer: false };
    if !go(self, ty, &mut acc, 0) { return None; }
    Some(RcDrop::Inside { kind: acc.kind?, many: acc.count > 1 || acc.buffer, buffer: acc.buffer })
  }

  // Marks the owner (or struct field) just registered as `name` from
  // its type: as a `ManuallyDrop<T>` so its scope end and
  // reassignments don't claim to drop the `T` (#283), and as an
  // `Rc` / `Arc` / `Weak` so they say that dropping it only lowers a
  // count (#373). Call right after `add_owner` / `add_struct`, before
  // any event clones the RAP. A name that was already registered (and
  // so not re-added by `add_rap`) is left alone.
  pub fn mark_owner_type(&mut self, name: &str, ty: Ty<'tcx>) {
    let rc = self.rc_kind(ty);
    let manually_drop = ty.ty_adt_def().map_or(false, |d| d.is_manually_drop());
    let rc_fields = self.rc_fields_kind(ty);
    let rc_inside = self.rc_inside_kind(ty);
    if rc.is_none() && !manually_drop && rc_fields.is_none() && rc_inside.is_none() {
      return;
    }
    let Some(just_added) = (self.rap_hashes as u64).checked_sub(1) else { return };
    match self.raps.get_mut(name).map(|rd| &mut rd.rap) {
      Some(ResourceAccessPoint::Owner(o)) if o.hash == just_added => {
        o.is_manually_drop = manually_drop;
        o.rc = rc;
        o.rc_inside = rc_inside;
      }
      Some(ResourceAccessPoint::Struct(s)) if s.hash == just_added => {
        s.is_manually_drop = manually_drop;
        s.rc = rc;
        s.rc_fields = rc_fields;
        s.rc_inside = rc_inside;
      }
      _ => {}
    }
  }

  // Adds an owner RAP that binds a closure literal. Routed through
  // a separate constructor (rather than an extra arg on add_owner) so
  // we don't churn every call site — closures are the only producer.
  pub fn add_closure_owner(&mut self, name: String, mutability: bool, scope: usize, is_global: bool) {
    self.add_rap(ResourceAccessPoint::Owner(Owner{name: name, hash: self.rap_hashes as u64, is_mut: mutability, is_copy: false, is_plain_value: false, interior_mut: false, is_closure: true, is_union: false, is_manually_drop: false, unbound_temp: false, write: Default::default(), holds_borrow: false, rc: None, rc_inside: None}), scope, is_global);
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
    // A reference that shadows a borrow holder of the same name
    // (`let third = v.get(2); match third { Some(third) => .. }`)
    // shares its column (#253). Keep the holder's borrow: the inner
    // binding copies the reference it holds, so its uses keep that
    // borrow alive, and overwriting it would lose the end of v's
    // borrow (#336).
    if self.holder_shadowed(&name) {
      return;
    }
    self.borrow_map.insert(name.clone(), RefData { lender: lender.clone(), assigned_at: line_num, lifetime: line_num, ref_mutability: ref_mutability, aliasing: alia, static_lender: None, borrow_span: None, co_lenders: Vec::new() });
  }
  /// `name`'s column is a borrow holder (#336) whose borrow is
  /// recorded; a reference binding of the same name shadows it.
  pub fn holder_shadowed(&self, name: &String) -> bool {
    self.borrow_map.contains_key(name)
      && self.raps.get(name).is_some_and(|rd| rd.rap.holds_borrow())
  }

  /// True iff `ref_ty` is `&T` / `&mut T` with `T: Copy` (`&i32`) or `T`
  /// a struct / enum that's non-Copy with nothing to drop (`ty_is_plain`).
  /// Such a reference points to a "value", not a "resource" (#312, #484).
  /// Unsized referents (`str`, `[T]`) keep "resource": they are a view of
  /// someone else's buffer.
  pub fn ref_pointee_is_copy(&self, ref_ty: Ty<'tcx>, owner: rustc_hir::OwnerId) -> bool {
    match ref_ty.kind() {
      TyKind::Ref(_, inner, _) => self.ty_is_copy(*inner, owner) || (inner.is_adt() && self.ty_is_plain(*inner, owner)),
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
    self.add_rap(ResourceAccessPoint::Function(Function { name: name, hash: self.rap_hashes as u64, drops_arg: Default::default(), rc_call: None }), self.current_scope, true);
  }

  // Adds a struct RAP
  // To be honest this struct logic is leftover from RV1 and should eventually be reworked
  // It doesn't make much sense that a struct should be represented any different than an owner
  // the reason for this is that in the frontend structs are visualized differently in the timeline header
  pub fn add_struct(&mut self, name: String, owner: u64, mem: bool, mutability: bool, is_copy: bool, is_plain_value: bool, interior_mut: bool, scope: usize, is_global: bool) {
    self.add_rap(ResourceAccessPoint::Struct(Struct {
      name: name,
      hash: self.rap_hashes as u64,
      owner: owner,
      is_mut: mutability,
      is_member: mem,
      is_copy,
      is_plain_value,
      interior_mut,
      is_manually_drop: false, rc: None, rc_fields: None, rc_inside: None }),
      scope, is_global);
  }

  pub fn add_rap(&mut self, r: ResourceAccessPoint, scope: usize, is_global: bool) {
    let fn_start_line = self.current_fn_start;
    // Functions are keyed in their own namespace (see `fn_key`), so a
    // variable and a function with the same name get separate RAPs.
    let key = if r.is_fn() { fn_key(r.name()) } else { r.name().to_string() };
    self.raps.entry(key).or_insert_with(|| {
      self.rap_hashes += 1;
      RapData { rap: r, scope, is_global, fn_start_line, extended_temp: None, temp_owner: None }
    });
  }

  pub fn update_rap(&mut self, r: &ResourceAccessPoint, line_num: usize) {
    // A value holding a borrow keeps it until its last use, like a
    // reference (#336).
    if r.is_ref() || (r.holds_borrow() && self.borrow_map.contains_key(r.name())) {
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
        ResourceTy::Value(rap) | ResourceTy::Deref(rap) | ResourceTy::Index(rap, _, _) | ResourceTy::DerefField(rap, _, _) => {
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
      | ExternalEvent::LoopExitMove { ro, .. }
      | ExternalEvent::OwnerDropAtReassign { ro, .. }
      | ExternalEvent::TemporaryDrop { ro, .. } => {
        self.skip_raps.contains(ro.name())
      }
      ExternalEvent::InitRefParam { param, .. } => {
        self.skip_raps.contains(param.name())
      }
      ExternalEvent::ClosureCall { closure, .. } => touches_ty(closure),
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
      // A borrow stored in an element (`v[0] = &u`, #282) is drawn on
      // the lender's column only: v's column doesn't model it.
      (_, ResourceTy::Index(..), ExternalEvent::StaticBorrow { .. } | ExternalEvent::MutableBorrow { .. }) => {},
      (_, ResourceTy::Temporary { .. }, _) => {},
      (ResourceTy::Value(_) | ResourceTy::Deref(_) | ResourceTy::DerefField(..),
       ResourceTy::Value(_) | ResourceTy::Deref(_) | ResourceTy::DerefField(..), _) // maybe change later
      // `v[i] = s` / `v[i] = y`: an arrow into v's column.
      | (ResourceTy::Value(_), ResourceTy::Index(..), _)
      | (ResourceTy::Deref(_), ResourceTy::Index(..), _)
      // `let n = *slots[0];`: an arrow out of slots's column (#438).
      | (ResourceTy::Index(..), ResourceTy::Value(_), _)
      | (ResourceTy::Index(..), ResourceTy::Deref(_), _)
      | (ResourceTy::Index(..), ResourceTy::Index(..), _) => {
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

  /// Every member column under a struct binding `parent`, at any depth
  /// (`x.name`, `x.tl.x`) and including reference fields (`x.part`),
  /// sorted by hash. A pattern binding's members live exactly as long
  /// as the binding, so they join its arm's declared variables (#405).
  pub fn struct_members_deep(&self, parent: &str) -> Vec<ResourceAccessPoint> {
    let prefix = format!("{}.", parent);
    #[expect(clippy::disallowed_methods, reason = "sorted by hash below")]
    let mut v: Vec<ResourceAccessPoint> = self.raps.iter()
      .filter(|(n, rd)| n.starts_with(&prefix) && rd.rap.is_member())
      .map(|(_, rd)| rd.rap.clone())
      .collect();
    v.sort_by_key(|r| *r.hash());
    v
  }

  /// `decls` (pattern bindings) plus the member columns of the struct
  /// bindings among them.
  pub fn with_struct_members(&self, decls: &HashSet<ResourceAccessPoint>) -> HashSet<ResourceAccessPoint> {
    let mut out = decls.clone();
    #[expect(clippy::disallowed_methods, reason = "sorted by hash below")]
    let mut structs: Vec<&ResourceAccessPoint> = decls.iter()
      .filter(|r| matches!(r, ResourceAccessPoint::Struct(Struct { is_member: false, .. })))
      .collect();
    structs.sort_by_key(|r| *r.hash());
    for r in structs {
      out.extend(self.struct_members_deep(r.name()));
    }
    out
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
  // A value moved on only some paths of a conditional above (#335)
  // counts as held: Rust drops it on the other paths. See
  // `ownership_after`.
  pub fn rap_holds_resource_now(&self, rap_name: &str) -> bool {
    self.rap_ownership_now(rap_name) != Ownership::None
  }

  /// `rap_holds_resource_now`, telling apart a value that is held on
  /// every path from one that a conditional above moved on some paths
  /// only (`Ownership::Maybe`).
  pub fn rap_ownership_now(&self, rap_name: &str) -> Ownership {
    // No prior ownership event: the variable was declared but never
    // initialized (`let mut y; y = x;`), so there's nothing to drop.
    ownership_after(self.preprocessed_events, rap_name, None).unwrap_or(Ownership::None)
  }

  /// True iff `name` is a reference whose value was moved, since it was
  /// last assigned, into something that carries the borrow on and ends
  /// it itself: another reference (`let r2 = r;`, `Holder { r }`) or the
  /// caller (`r` as a fn's result). After such a move `name` holds
  /// nothing, so its own borrow-end / return-to-lender event would be
  /// false (#209). A `&mut` moved into a call (`take(r)`) isn't covered:
  /// nothing else with a column ends that borrow, so `r`'s end stands
  /// for it. Moves inside a branch aren't seen here (Branch events
  /// aren't searched); those keep the old drawing.
  pub fn ref_handed_off(&self, name: &str) -> bool {
    let (Some(rd), Some(rap_data)) = (self.borrow_map.get(name), self.raps.get(name)) else {
      return false;
    };
    let hash = *rap_data.rap.hash();
    let swapped_here = |line: usize| self.destructure_swapped.iter()
      .any(|(n, lo, hi)| n == name && (*lo..=*hi).contains(&line) && (*lo..=*hi).contains(&rd.assigned_at));
    self.preprocessed_events.iter().any(|(line, ev)| match ev {
      ExternalEvent::Move { from: ResourceTy::Value(f), to, .. }
        if *f.hash() == hash && *line >= rd.assigned_at && !swapped_here(*line) =>
      {
        match to {
          ResourceTy::Caller => true,
          ResourceTy::Value(t) => self.borrow_map.contains_key(t.name())
            && self.raps.get(t.name()).map_or(false, |r| r.rap.hash() == t.hash()),
          _ => false,
        }
      }
      _ => false,
    })
  }

  /// `let r2: &mut String = r;`, `Holder { r: a }`, `q = r;`: a `&mut`
  /// place used where the expected type is already a `&mut` (an
  /// annotated `let`, a struct field, an assignment) is coerced with an
  /// implicit reborrow, `&mut *r`, so `r` is usable again once the new
  /// reference is done. Only `let r2 = r;` with an inferred type moves
  /// `r`. rustc records the reborrow as an adjustment on `rhs` (#289).
  pub fn is_coerced_mut_reborrow(&self, lhs: &ResourceTy, rhs_rap: &ResourceAccessPoint, rhs: &Expr, evt: &Evt) -> bool {
    matches!(evt, Evt::Move)
      && matches!(lhs, ResourceTy::Value(_))
      && rhs_rap.is_mutref()
      && is_implicit_mut_reborrow(rhs, self.tcx.typeck(rhs.hir_id.owner))
  }

  pub fn ext_ev_of_evt(&self, evt: Evt, lhs: ResourceTy, rhs: ResourceTy, id: usize, is_partial: bool) -> ExternalEvent{
    match evt {
      Evt::Bind => ExternalEvent::Bind { from: rhs, to: lhs, id },
      Evt::Copy => ExternalEvent::Copy { from: rhs, to: lhs, id, is_partial, into_closure: false },
      Evt::Move => ExternalEvent::Move { from: rhs, to: lhs, id, is_partial, into_closure: false },
      Evt::SBorrow => ExternalEvent::StaticBorrow { from: rhs, to: lhs, id, is_partial, is_reborrow: false, via: None },
      Evt::MBorrow => ExternalEvent::MutableBorrow { from: rhs, to: lhs, id, is_partial, is_reborrow: false, via: None },
      Evt::PassBySRef => ExternalEvent::PassByStaticReference { from: rhs, to: lhs, id, implicit: ImplicitRef::No, refcell_borrow_mut: false, result_keeps_borrow: false, stored_in: StoredIn::default() },
      Evt::PassByMRef => ExternalEvent::PassByMutableReference {
        from: rhs, to: lhs, id, is_reborrow: false, result_has_lifetime: false, implicit: ImplicitRef::No,
        result_keeps_borrow: false, stored_in: StoredIn::default(),
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
      from: arg, to: func, id, is_reborrow, result_has_lifetime, implicit, result_keeps_borrow: false, stored_in: StoredIn::default(),
    });
    *self.unique_id += 1;
  }

  /// `add_ev` for a `PassBySRef` with an explicit `ImplicitRef`.
  pub fn add_pass_by_sref_ev(&mut self, line_num: usize, func: ResourceTy, arg: ResourceTy, implicit: ImplicitRef, refcell_borrow_mut: bool) {
    let id = *self.unique_id;
    self.add_external_event(line_num, ExternalEvent::PassByStaticReference {
      from: arg, to: func, id, implicit, refcell_borrow_mut, result_keeps_borrow: false, stored_in: StoredIn::default(),
    });
    *self.unique_id += 1;
  }

  /// After a call `callee(args)`: if `callee` names a local closure
  /// binding that rustc inferred to be `FnOnce` (`let f = || drop(s);
  /// f();`), record that the call moves the closure and what happens
  /// to what it captured by value (#271). `FnOnce::call_once` takes the
  /// closure by value, so after the call `f` is gone and has nothing
  /// left to drop at its scope end. `Fn` / `FnMut` closures are only
  /// borrowed by their calls; nothing is recorded for them.
  pub fn closure_call_events(&mut self, callee: &'tcx Expr<'tcx>, line_num: usize) {
    let ExprKind::Path(QPath::Resolved(None, path)) = callee.kind else { return };
    if !matches!(path.res, Res::Local(_)) {
      return;
    }
    let typeck = self.tcx.typeck(callee.hir_id.owner);
    let rustc_middle::ty::Closure(closure_def_id, closure_args) = typeck.expr_ty(callee).kind() else { return };
    if closure_args.as_closure().kind() != rustc_middle::ty::ClosureKind::FnOnce {
      return;
    }
    let Some(closure_local) = closure_def_id.as_local() else { return };
    // The capture list names variables of the fn the closure was
    // written in; only that fn's RAPs can stand for them.
    if self.tcx.typeck_root_def_id(*closure_def_id) != callee.hir_id.owner.to_def_id() {
      return;
    }
    let name = self.tcx.hir_name(path.segments[0].hir_id).as_str().to_owned();
    let closure_rap = match self.raps.get(&name) {
      Some(rd) if rd.rap.is_closure() => rd.rap.to_owned(),
      _ => return,
    };
    // What the body does with each capture (all captures, to keep the
    // field index of each), then the by-value, non-Copy ones named.
    let closure_typeck = self.tcx.typeck(closure_local);
    let captures: Vec<_> = closure_typeck.closure_min_captures_flattened(closure_local).collect();
    let bwf = rustc_utils::mir::borrowck_facts::get_body_with_borrowck_facts(self.tcx, closure_local);
    let ends = closure_capture_ends(self.tcx, &bwf.body, captures.len());
    let env = rustc_middle::ty::TypingEnv::post_analysis(self.tcx, callee.hir_id.owner);
    let mut fates = Vec::new();
    for (capture, end) in captures.iter().zip(ends) {
      let by_value = matches!(capture.info.capture_kind,
        rustc_middle::ty::UpvarCapture::ByValue | rustc_middle::ty::UpvarCapture::ByUse);
      // A Copy capture is copied into the closure; there's nothing of
      // it to move or drop.
      if !by_value || self.tcx.type_is_copy_modulo_regions(env, capture.place.ty()) {
        continue;
      }
      let upvar_hir_id = match capture.place.base {
        rustc_middle::hir::place::PlaceBase::Upvar(upvar_id) => upvar_id.var_path.hir_id,
        _ => continue,
      };
      let upvar_name = self.tcx.hir_name(upvar_hir_id).as_str().to_owned();
      let Some(rap) = self.capture_rap(&capture.place, &upvar_name) else { continue };
      fates.push(CapturedFate {
        name: rap.name().to_owned(),
        end,
        needs_drop: capture.place.ty().needs_drop(self.tcx, env),
      });
    }
    let id = *self.unique_id;
    *self.unique_id += 1;
    self.add_external_event(line_num, ExternalEvent::ClosureCall {
      closure: ResourceTy::Value(closure_rap),
      fates,
      id,
    });
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
      // A field reached through a reference (`self.c` in a `&mut self`
      // method, `p.x` for `p: &mut P`, `r.a` after `let r = &mut s`)
      // has no column of its own; it's a write through the reference,
      // drawn on the reference's column like `*r = …` (#147).
      ExprKind::Path(_) | ExprKind::Field(..) => {
        if let Some(rty) = self.field_through_ref(expr) {
          // The write uses the reference: its borrow is live here.
          if let Some(r) = rty.deref_rap() {
            self.update_rap(&r.clone(), expr_to_line(&expr, &self.tcx));
          }
          return Some(rty);
        }
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
        match expr_to_rap_name(expr, &self.tcx).and_then(|name| self.raps.get(&name)) {
          Some(rd) => Some(ResourceTy::Value(rd.rap.to_owned())),
          // A field with no column of its own (`t.0` of a tuple).
          None => self.place_without_column(expr),
        }
      }
      ExprKind::Unary(UnOp::Deref, exp) => {
        let line_num = expr_to_line(&exp, &self.tcx);
        // `*slots[0] += 10` with slots: `Vec<&mut i32>`: the write goes
        // through the reference stored in the element slots[0], into
        // what it points to. slots isn't a reference, so this isn't
        // `*slots` (#396). The same holds for a `Box` element
        // (`*boxes[0] = 5`) and for a field of an element
        // (`*hs[0].r += 1`). Which variable the element points to
        // isn't known statically, so the event goes on the
        // collection's column, as an element write whose place starts
        // with `*` (see `ResourceTy::Index`).
        if index_base(exp).is_some() {
          let pointee_ty = self.tcx.typeck(expr.hir_id.owner).expr_ty(expr);
          // `*refs[0] = &u` (the target is itself a reference): a
          // borrow we don't track, like `v[0] = &u`. Not drawn.
          if pointee_ty.is_ref() {
            return None;
          }
          return self.deref_of_element(expr);
        }
        let rhs_rap = fetch_rap(&expr, &self.tcx, &self.raps);
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
        let Some(name) = expr_to_rap_name(expr, &self.tcx) else { return self.place_without_column(expr) };
        let Some(rap) = self.raps.get(&name).map(|rd| rd.rap.to_owned()) else { return self.place_without_column(expr) };
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
      // Any other place expression (`v.last_mut().unwrap().name`, ...).
      _ => self.place_without_column(expr),
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
  /// the call (#230). The closure itself has no column.
  ///
  /// A by-reference capture borrows the variable for as long as the
  /// closure (or whatever the callee keeps it in) is alive. When that
  /// borrow is over by the closure's last line (`apply(|x| x + k)`,
  /// `o.map(|x| x + k)`), it lasts only for the call, and is drawn
  /// like a `&k` argument, as borrowed by "the closure passed to
  /// apply" (#298). When it outlives the line (`let it =
  /// v.iter().map(|x| x + k);`, consumed later), the plugin can't say
  /// where the borrow went, so it isn't drawn. The end comes from the
  /// MIR loan for the capture, as for `let f = || ...`.
  ///
  /// Returns false if nothing was drawn, so the caller falls back to
  /// the plain argument handling.
  fn match_closure_arg(&mut self, closure: &'tcx rustc_hir::Closure<'tcx>, arg: &'tcx Expr, to_ro: &ResourceTy, fn_name: &str) -> bool {
    let line_num = expr_to_line(&arg, &self.tcx);
    let last_line = self.tcx.sess.source_map().lookup_char_pos(arg.span.hi()).line;
    let typeck = self.tcx.typeck(closure.def_id);
    let env = rustc_middle::ty::TypingEnv::post_analysis(self.tcx, arg.hir_id.owner);
    let mut mir_b_data = None;
    let mut drew_any = false;
    for capture in typeck.closure_min_captures_flattened(closure.def_id) {
      let upvar_hir_id = match capture.place.base {
        rustc_middle::hir::place::PlaceBase::Upvar(upvar_id) => upvar_id.var_path.hir_id,
        _ => continue,
      };
      let root = self.tcx.hir_name(upvar_hir_id).as_str().to_owned();
      let Some(from_rap) = self.capture_rap(&capture.place, &root) else { continue };
      match capture.info.capture_kind {
        rustc_middle::ty::UpvarCapture::ByValue | rustc_middle::ty::UpvarCapture::ByUse => {
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
        rustc_middle::ty::UpvarCapture::ByRef(kind) => {
          // The capture's loan is issued within the closure literal's
          // lines and borrows the root variable; region.1 is the
          // last line it's live on.
          let loans = mir_b_data.get_or_insert_with(|| self.gather_borrow_data(self.bwf));
          let last_live = loans.iter()
            .filter(|b| (line_num..=last_line).contains(&b.region.0)
              && b.borrowed_place.as_deref() == Some(root.as_str()))
            .map(|b| b.region.1)
            .max();
          if !matches!(last_live, Some(l) if l <= last_line) {
            continue;
          }
          let target_name = format!("the closure passed to {}", fn_name);
          if !self.raps.contains_key(&fn_key(&target_name)) {
            self.add_fn(target_name.clone());
          }
          let target = ResourceTy::Value(self.raps.get(&fn_key(&target_name)).unwrap().rap.to_owned());
          self.update_rap(&from_rap, line_num);
          match kind {
            rustc_middle::ty::BorrowKind::Mutable =>
              self.add_pass_by_mref_ev(line_num, target, ResourceTy::Value(from_rap), false, false, ImplicitRef::No),
            _ => self.add_pass_by_sref_ev(line_num, target, ResourceTy::Value(from_rap), ImplicitRef::No, false),
          }
          drew_any = true;
        }
      }
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

  /// A write to a place that has no column of its own (`t.0 = …` on a
  /// tuple, `v.last_mut().unwrap().name = …`): the write goes on the
  /// column of the variable the place is reached from, like an element
  /// write `v[i] = …` (#458), so a tooltip there can say the old value
  /// is dropped. None when the place isn't reached from a local with a
  /// column, or holds a reference (overwriting one drops nothing and
  /// would start a borrow we don't track).
  fn place_without_column(&mut self, expr: &'tcx Expr) -> Option<ResourceTy> {
    let typeck = self.tcx.typeck(expr.hir_id.owner);
    let ty = typeck.node_type(expr.hir_id);
    if ty.is_ref() {
      warn!("unsupported assignment LHS shape; skipping assignment at this location");
      return None;
    }
    // Walk down to the variable the place is reached from. Through a
    // method call (`v.last_mut().unwrap()`) the result must borrow
    // from something, or the place may not be v's at all.
    let mut cur = expr;
    let rap = loop {
      match cur.kind {
        ExprKind::Field(base, _) | ExprKind::Index(base, _, _) | ExprKind::Unary(UnOp::Deref, base) => cur = base,
        ExprKind::MethodCall(_, rcvr, _, _) if call_result_has_lifetime(cur, typeck) => cur = rcvr,
        ExprKind::Path(_) => {
          let name = expr_to_rap_name(cur, &self.tcx)?;
          break self.raps.get(&name)?.rap.to_owned();
        }
        _ => return None,
      }
    };
    if rap.is_fn() {
      return None;
    }
    let place = match self.tcx.sess.source_map().span_to_snippet(expr.span) {
      Ok(text) if !expr.span.from_expansion() && text.chars().count() <= 32
        && !text.contains(|c: char| matches!(c, '<' | '>' | '&' | '"' | '\'' | '\n' | '\r')) => text,
      _ => format!("{}.…", rap.name()),
    };
    // A plain `=` assigns to the place (Copy: "assigned a new value";
    // owned over a value that needs drop: drops it; owned with nothing
    // to drop: acquires). The caller turns this into `Update` for `op=`.
    let env = rustc_middle::ty::TypingEnv::post_analysis(self.tcx, expr.hir_id.owner);
    let write = if self.tcx.type_is_copy_modulo_regions(env, ty) {
      PlaceWrite::AssignCopy
    } else if ty.needs_drop(self.tcx, env) {
      PlaceWrite::AssignDropsOld
    } else {
      PlaceWrite::AssignOwned
    };
    Some(ResourceTy::Index(rap, place, write))
  }

  /// `place = &x` / `place = &mut x` where `place` has no column of its
  /// own: an element that is a reference (`v[0] = &u`), or a field
  /// through a reference (`h.r = &y` with `h: &mut Holder`). The borrow
  /// of `x` is drawn on x's column only, naming the place as the
  /// borrower (`borrowed by v[0]`, `borrowed by h.r`; no borrower is
  /// named if the place can't be, e.g. a field of a call's result). It
  /// ends where the MIR loan does: the last use of whatever holds the
  /// reference (#282). If the place lives behind a reference the
  /// function was passed (`h.r = &y`, `self.r = …`), the borrow outlives
  /// the function, so no end is drawn. It used to say "borrowed by
  /// Anonymous resource" and never end.
  pub fn store_borrow_in_unmodelled_place(&mut self, lhs: &'tcx Expr<'tcx>, rhs: &'tcx Expr<'tcx>, evt: Evt) {
    let root = self.place_root(lhs);
    // Only a place reached by fields / derefs from a variable is named.
    let named_root = root.filter(|&(_, through_call)| !through_call).map(|(root, _)| root);
    let dest = match lhs.kind {
      ExprKind::Index(..) => expr_to_rap_name(lhs, &self.tcx)
        .and_then(|name| {
          let rap = self.raps.get(&name)?.rap.to_owned();
          (!rap.is_fn()).then(|| ResourceTy::Index(rap, self.place_label(lhs, &name), PlaceWrite::Update))
        })
        .unwrap_or(ResourceTy::Anonymous),
      // A field with no column of its own (`h.r` through a reference):
      // named by its source text, or by the field it is when the
      // reference is a local that borrows a struct (`hold.r` for
      // `rh.r` after `let rh = &mut hold;`, #512). `Index` is the
      // place-with-no-column borrower here too; borrows into it stay
      // on the lender's column.
      ExprKind::Field(..) => named_root
        .and_then(|root| {
          let name = expr_to_rap_name(root, &self.tcx)?;
          let rap = self.raps.get(&name)?.rap.to_owned();
          let label = self.field_through_local_ref(root, &name, lhs)
            .or_else(|| self.plain_source_text(lhs))?;
          (!rap.is_fn()).then(|| ResourceTy::Index(rap, label, PlaceWrite::Update))
        })
        .unwrap_or(ResourceTy::Anonymous),
      _ => ResourceTy::Anonymous,
    };
    let n_events = self.preprocessed_events.len();
    self.match_rhs(dest.clone(), rhs, evt);
    // A reference stored behind a reference parameter is handed back to
    // the caller: its borrow lasts past this function's end.
    if root.is_some_and(|(root, _)| self.is_ref_param(root)) {
      return;
    }
    // The borrow match_rhs just recorded, if any (a borrow of a
    // temporary, `v[0] = &String::new()`, has no lender column).
    let borrow = self.preprocessed_events[n_events..].iter().find_map(|(line, ev)| match ev {
      ExternalEvent::StaticBorrow { from: from @ ResourceTy::Value(lender), to, .. } if *to == dest =>
        Some((*line, from.clone(), lender.name().to_owned(), Evt::SDie)),
      ExternalEvent::MutableBorrow { from: from @ ResourceTy::Value(lender), to, .. } if *to == dest =>
        Some((*line, from.clone(), lender.name().to_owned(), Evt::MDie)),
      _ => None,
    });
    let Some((line, lender, lender_name, die)) = borrow else { return };
    if let Some(kill_line) = self.loan_end_line(line, &lender_name) {
      // StaticDie/MutableDie { from: dest, to: lender }.
      self.add_ev(kill_line, die, lender, dest, false);
    }
  }

  /// `arg` is `&x` / `&mut x` passed to a call whose signature lets it
  /// store that reference behind another argument (#513):
  /// `point(&mut hold, &t)` with
  /// `fn point<'a>(h: &mut H<'a>, t: &'a String)` may do `h.r = t`, so
  /// the borrow checker keeps t borrowed while `hold` may still hold
  /// the reference, not just until point returns. Returns the place it
  /// may be stored in (an `Index` naming it, with no column of its own
  /// in this event) and the line the loan ends on. None (the borrow
  /// ends at the call, as before) unless x is a variable with a column
  /// that owns its data, the place is a variable or field with a
  /// column, and the loan outlives the call's line.
  fn stored_borrow(&self, arg: &'tcx Expr<'tcx>, from_ro: &ResourceTy, line: usize) -> Option<(ResourceTy, usize)> {
    let ExprKind::AddrOf(..) = arg.kind else { return None };
    let ResourceTy::Value(lender) = from_ro else { return None };
    if lender.is_ref() || lender.is_fn() {
      return None;
    }
    let rustc_hir::Node::Expr(call) = self.tcx.parent_hir_node(arg.hir_id) else { return None };
    let target = self.arg_storable_behind(call, arg)?;
    let place = self.mut_arg_place(target)?;
    let end_line = self.loan_end_line(line, lender.name()).filter(|&end| end > line)?;
    let label = place.name().to_owned();
    Some((ResourceTy::Index(place, label, PlaceWrite::Update), end_line))
  }

  /// The argument of `call` that its signature lets it store the
  /// reference `arg` behind: a `&mut` parameter whose pointee's type
  /// mentions a lifetime of `arg`'s reference type
  /// (`h: &mut H<'a>` for `t: &'a String`). Read from the signature
  /// only, like the borrow checker; rustc has already filled in
  /// elided lifetimes, and distinct ones never match.
  fn arg_storable_behind(&self, call: &'tcx Expr<'tcx>, arg: &'tcx Expr<'tcx>) -> Option<&'tcx Expr<'tcx>> {
    use rustc_middle::ty;
    let typeck = self.tcx.typeck(call.hir_id.owner);
    let (def_id, args): (_, Vec<&'tcx Expr<'tcx>>) = match call.kind {
      ExprKind::Call(callee, args) => match typeck.node_type_opt(callee.hir_id)?.kind() {
        ty::FnDef(def_id, _) => (*def_id, args.iter().collect()),
        _ => return None,
      },
      ExprKind::MethodCall(_, rcvr, args, _) =>
        (typeck.type_dependent_def_id(call.hir_id)?, std::iter::once(rcvr).chain(args.iter()).collect()),
      _ => return None,
    };
    let sig = self.tcx.fn_sig(def_id).instantiate_identity().skip_norm_wip();
    let sig = self.tcx.liberate_late_bound_regions(def_id, sig);
    let inputs = sig.inputs();
    if inputs.len() != args.len() {
      return None;
    }
    let i = args.iter().position(|a| a.hir_id == arg.hir_id)?;
    let ty::Ref(region, _, _) = inputs[i].kind() else { return None };
    inputs.iter().zip(&args).enumerate().find_map(|(j, (input, other))| {
      let ty::Ref(_, pointee, Mutability::Mut) = input.kind() else { return None };
      (j != i && pointee.walk().any(|g| g.as_region() == Some(*region))).then_some(*other)
    })
  }

  /// The variable or field a `&mut` argument points to, if it has a
  /// column: `hold` for `&mut hold`, for a method receiver `hold`
  /// (auto-referenced), and for a local reference `rh` that borrows
  /// `hold`.
  fn mut_arg_place(&self, target: &'tcx Expr<'tcx>) -> Option<ResourceAccessPoint> {
    let place = match target.kind {
      ExprKind::AddrOf(_, _, inner) => inner,
      _ => target,
    };
    if !matches!(place.kind, ExprKind::Path(_) | ExprKind::Field(..)) {
      return None;
    }
    let name = expr_to_rap_name(place, &self.tcx)?;
    let rap = self.raps.get(&name)?.rap.to_owned();
    if rap.is_fn() {
      return None;
    }
    if !rap.is_ref() {
      return Some(rap);
    }
    // A reference variable passed as is (`point(rh, &t)`): the place
    // is what it borrows.
    if !std::ptr::eq(place, target) {
      return None;
    }
    match &self.borrow_map.get(&name)?.lender {
      ResourceTy::Value(lender) if !lender.is_ref() && !lender.is_fn() => Some(lender.to_owned()),
      _ => None,
    }
  }

  /// The last line of the MIR loan of `lender_name` (a variable or a
  /// field of one) taken on `line`: where the borrow checker ends it.
  fn loan_end_line(&self, line: usize, lender_name: &str) -> Option<usize> {
    // MIR names the borrowed local (`p` for `&p.x`).
    let root = lender_name.split('.').next().unwrap_or(lender_name);
    self.gather_borrow_data(self.bwf).iter().find_map(|b| {
      (b.region.0 == line && b.borrowed_place.as_deref() == Some(root)).then_some(b.region.1)
    })
  }

  /// `rh.r` (or `(*rh).r`) where `rh` is a local reference that
  /// borrows the struct `hold` (`let rh = &mut hold;`): the place is
  /// `hold.r`, which has a column, so name it that way (#512). None if
  /// `root` isn't such a reference, or the field has no column.
  fn field_through_local_ref(&self, root: &'tcx Expr<'tcx>, root_name: &str, place: &'tcx Expr<'tcx>) -> Option<String> {
    if self.is_ref_param(root) || !self.tcx.typeck(root.hir_id.owner).node_type(root.hir_id).is_ref() {
      return None;
    }
    let ResourceTy::Value(lender) = &self.borrow_map.get(root_name)?.lender else { return None };
    if lender.is_ref() || lender.is_fn() {
      return None;
    }
    let rest = expr_to_rap_name(place, &self.tcx)?.strip_prefix(root_name)?.to_owned();
    let name = format!("{}{}", lender.name(), rest);
    self.raps.contains_key(&name).then_some(name)
  }

  /// The variable a place expression starts from: `h` for `h.r`,
  /// `(*h).r` and `h.a.r`, `v` for `v[0]`, and `hs` for
  /// `hs.last_mut().unwrap().r` (with `true`: the place is reached
  /// through a method call's result). None if the place starts anywhere
  /// else (a function call's result, a temporary).
  fn place_root(&self, place: &'tcx Expr<'tcx>) -> Option<(&'tcx Expr<'tcx>, bool)> {
    let mut cur = place;
    let mut through_call = false;
    loop {
      match cur.kind {
        ExprKind::Field(recv, _) | ExprKind::Index(recv, _, _) | ExprKind::Unary(UnOp::Deref, recv) => cur = recv,
        ExprKind::MethodCall(_, recv, _, _) => {
          through_call = true;
          cur = recv;
        }
        ExprKind::Path(_) => return Some((cur, through_call)),
        _ => return None,
      }
    }
  }

  /// `expr` names a parameter of this function whose type is a
  /// reference (`h: &mut Holder`, `&mut self`).
  fn is_ref_param(&self, expr: &Expr<'tcx>) -> bool {
    let ExprKind::Path(rustc_hir::QPath::Resolved(None, path)) = expr.kind else { return false };
    let rustc_hir::def::Res::Local(id) = path.res else { return false };
    self.hir_body.params.iter().any(|p| p.pat.hir_id == id)
      && self.tcx.typeck(expr.hir_id.owner).node_type(expr.hir_id).is_ref()
  }

  /// A place's source text if it's short and plain enough to put in a
  /// tooltip (see `place_label`).
  fn plain_source_text(&self, expr: &Expr) -> Option<String> {
    if expr.span.from_expansion() {
      return None;
    }
    self.tcx.sess.source_map().span_to_snippet(expr.span).ok().filter(|text| text.chars().count() <= 24
      && !text.contains(|c: char| matches!(c, '<' | '>' | '&' | '"' | '\'' | '\n' | '\r')))
  }

  /// `*slots[0]`, `*boxes[0]`, `*hs[0].r`: a deref whose operand is
  /// reached through an index. Both the write side (#396) and the read
  /// side (#438) name the element, not the collection: the event goes
  /// on the collection's column as `ResourceTy::Index` with a place
  /// starting with `*`. None when the collection isn't a registered
  /// variable.
  fn deref_of_element(&mut self, expr: &'tcx Expr) -> Option<ResourceTy> {
    let ExprKind::Unary(UnOp::Deref, exp) = expr.kind else { return None };
    let coll = index_base(exp)?;
    let name = expr_to_rap_name(coll, &self.tcx)?;
    let rap = self.raps.get(&name)?.rap.to_owned();
    if rap.is_fn() {
      return None;
    }
    self.update_rap(&rap, expr_to_line(&exp, &self.tcx));
    let place = match self.place_label(expr, &format!("*{}", name)) {
      p if p.starts_with('*') => p,
      _ => format!("*{}[…]", name),
    };
    // A plain `=` assigns to the place `*slots[0]` (the caller turns
    // this into `Update` for `op=`; reads ignore it).
    let pointee_ty = self.tcx.typeck(expr.hir_id.owner).expr_ty(expr);
    let typing_env = rustc_middle::ty::TypingEnv::post_analysis(self.tcx, expr.hir_id.owner);
    let write = if self.tcx.type_is_copy_modulo_regions(typing_env, pointee_ty) {
      PlaceWrite::AssignCopy
    } else if pointee_ty.needs_drop(self.tcx, typing_env) {
      PlaceWrite::AssignDropsOld
    } else {
      PlaceWrite::AssignOwned
    };
    Some(ResourceTy::Index(rap, place, write))
  }

  /// How an element place (`v[i]`) is named in tooltips: its source
  /// text when that's short and plain, else `v[…]`. Tooltip text goes
  /// into an HTML attribute, so text with markup characters, quotes
  /// or line breaks is never used verbatim.
  fn place_label(&self, expr: &Expr, rap_name: &str) -> String {
    self.plain_source_text(expr).unwrap_or_else(|| format!("{}[…]", rap_name))
  }

  /// `expr` is a field projection whose place lies behind a reference
  /// RAP: `p.x` / `p.inner.v` / `(*p).x` with `p: &mut P`, `self.c`
  /// in a `&mut self` method, or `w.r.x` where the struct field `w.r`
  /// is itself a reference. Returns `DerefField(<that reference>,
  /// <place as written>)`.
  ///
  /// Only non-reference field types are handled: storing a reference
  /// through a reference (`h.r = &x`) would need loan bookkeeping for
  /// a place without a column, so it stays unmodelled.
  fn field_through_ref(&self, expr: &'tcx Expr<'tcx>) -> Option<ResourceTy> {
    let typeck = self.tcx.typeck(expr.hir_id.owner);
    if typeck.node_type(expr.hir_id).is_ref() {
      return None;
    }
    let mut cur = expr;
    let ref_expr = loop {
      let ExprKind::Field(recv, _) = cur.kind else { return None };
      // `p.x` auto-derefs p: the receiver's own type is the reference.
      if typeck.node_type(recv.hir_id).is_ref() {
        break recv;
      }
      // `(*p).x`: an explicit deref of a reference.
      if let ExprKind::Unary(UnOp::Deref, inner) = recv.kind {
        if typeck.node_type(inner.hir_id).is_ref() {
          break inner;
        }
      }
      cur = recv;
    };
    if !matches!(ref_expr.kind, ExprKind::Path(_) | ExprKind::Field(..)) {
      return None;
    }
    let ref_name = expr_to_rap_name(ref_expr, &self.tcx)?;
    let rap = self.raps.get(&ref_name).map(|rd| rd.rap.to_owned())?;
    if !rap.is_ref() {
      return None;
    }
    let place = self.tcx.sess.source_map().span_to_snippet(expr.span).ok()
      .filter(|s| !expr.span.from_expansion() && !s.contains('\n'))
      .or_else(|| expr_to_rap_name(expr, &self.tcx))?;
    Some(ResourceTy::DerefField(rap, place, PlaceWrite::Update))
  }

  // add events for arguments in a function call
  pub fn match_arg(&mut self, arg: &'tcx Expr, fn_name: String) {
    // A tuple built in the argument list (`show((s, 1))`,
    // `v.push((s, n))`): each element is passed into the call with it,
    // so `s` is moved into the call (#189).
    if let ExprKind::Tup(elems) = arg.kind {
      if !arg.span.from_expansion() {
        for elem in elems.iter() {
          self.match_arg(elem, fn_name.clone());
        }
        return;
      }
    }
    self.add_fn(fn_name.clone());
    if let ExprKind::Closure(closure) = arg.kind {
      let to_ro = ResourceTy::Value(self.raps.get(&fn_key(&fn_name)).unwrap().rap.to_owned());
      if self.match_closure_arg(closure, arg, &to_ro, &fn_name) {
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
      // `point(&mut hold, &t)` where point's signature lets it store
      // `&t` in `hold`: t stays borrowed past the call (#513).
      if let Some((place, end_line)) = self.stored_borrow(arg, &from_ro, line_num) {
        let mutable = arg_ty.ref_mutability() == Some(Mutability::Mut);
        let id = *self.unique_id;
        let stored_in = StoredIn(Some(place.name().to_owned()));
        self.add_external_event(line_num, if mutable {
          ExternalEvent::PassByMutableReference {
            from: from_ro.clone(), to: to_ro, id, is_reborrow: false, result_has_lifetime: true,
            implicit: ImplicitRef::No, result_keeps_borrow: false, stored_in,
          }
        } else {
          ExternalEvent::PassByStaticReference {
            from: from_ro.clone(), to: to_ro, id, implicit: ImplicitRef::No, refcell_borrow_mut: false, result_keeps_borrow: false, stored_in,
          }
        });
        *self.unique_id += 1;
        // StaticDie/MutableDie { from: place, to: lender }: drawn on
        // the lender's column only, like a borrow stored in an element.
        self.add_ev(end_line, if mutable { Evt::MDie } else { Evt::SDie }, from_ro, place, false);
        return;
      }
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
          // An existing `&mut` (not a fresh `&mut x` written at the
          // call) that rustc didn't reborrow is moved into the call:
          // `&mut T` isn't Copy, and the reborrow is only inserted
          // when the parameter's type is itself a reference
          // (`push(y)` with `x: &mut String`), not for a generic
          // `take<T>(t: T)` (#200). Using `y` afterwards is E0382.
          if !is_reborrow && !matches!(arg.kind, ExprKind::AddrOf(..))
            && matches!(from_ro, ResourceTy::Value(_)) {
            self.add_ev(line_num, Evt::Move, to_ro, from_ro, false);
            return;
          }
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
  pub fn get_ref_data_full(&self, expr: &'tcx Expr) -> (ResourceTy, VecDeque<String>, Option<StaticLender>, Vec<ResourceTy>) {
    let (lender, aliasing, co_lenders) = self.get_ref_data_co(expr);
    let static_lender = match lender {
      ResourceTy::Anonymous => self.static_lender_of(expr),
      _ => None,
    };
    (lender, aliasing, static_lender, co_lenders)
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
  pub fn get_ref_data(&self, expr: &'tcx Expr<'tcx>) -> (ResourceTy, VecDeque<String>){
    let (lender, aliasing, _) = self.get_ref_data_co(expr);
    (lender, aliasing)
  }

  /// `get_ref_data`, plus the further lenders of a reference that
  /// borrows from several variables (see `RefData::co_lenders`).
  pub fn get_ref_data_co(&self, expr: &'tcx Expr<'tcx>) -> (ResourceTy, VecDeque<String>, Vec<ResourceTy>){
    // A call's result: who it borrows from is decided by the callee's
    // signature (`ref_flow.rs`, #171), not by its receiver.
    if matches!(expr.kind, ExprKind::Call(..) | ExprKind::MethodCall(..)) {
      let mut lenders = self.call_lenders(expr);
      if lenders.is_empty() {
        // Nothing with a column. Keep a non-variable lender the old
        // way found (a `&self` method on a reference parameter yields
        // `Caller`), but never a guessed variable: the receiver of
        // `it.next().unwrap()` is the iterator, not what the result
        // borrows.
        let lender = match find_lender(&expr, &self.tcx, &self.raps, &self.borrow_map) {
          ResourceTy::Value(_) | ResourceTy::Deref(_) => ResourceTy::Anonymous,
          other => other,
        };
        return (lender, VecDeque::new(), Vec::new());
      }
      let lender = lenders.remove(0);
      let aliasing = get_aliasing_data(&lender, &self.borrow_map);
      return (lender, aliasing, lenders);
    }
    // `if c { first_word(&t) } else { "none" }` / a `match`: the
    // reference may come from any branch, so (as rustc sees it) it
    // borrows from every branch's lender until its last use (#337).
    // Each branch draws its own borrow (`match_rhs`'s If / Match
    // arms); this records the union, so all of them are given back.
    if let Some(tails) = branch_tails(expr) {
      let mut lenders: Vec<ResourceTy> = Vec::new();
      for tail in tails {
        let (l, _, co) = self.get_ref_data_co(tail);
        for x in std::iter::once(l).chain(co) {
          if matches!(x, ResourceTy::Value(_) | ResourceTy::Deref(_)) && !lenders.contains(&x) {
            lenders.push(x);
          }
        }
      }
      if !lenders.is_empty() {
        let lender = lenders.remove(0);
        let aliasing = get_aliasing_data(&lender, &self.borrow_map);
        return (lender, aliasing, lenders);
      }
    }
    // `let r = refs[0];` copies a reference out of a collection.
    if let Some(mut lenders) = self.stored_ref_lenders(expr) {
      lenders.retain(|l| matches!(l, ResourceTy::Value(_) | ResourceTy::Deref(_)));
      if lenders.is_empty() {
        return (ResourceTy::Anonymous, VecDeque::new(), Vec::new());
      }
      let lender = lenders.remove(0);
      let aliasing = get_aliasing_data(&lender, &self.borrow_map);
      return (lender, aliasing, lenders);
    }
    let (lender, aliasing) = self.get_ref_data_single(expr);
    // A copy of a reference that borrows from several variables
    // (`let r2 = r;` after `let r = longest(&a, &b);`) borrows from
    // all of them too.
    let co_lenders = match expr.kind {
      ExprKind::Path(..) | ExprKind::Field(..) => expr_to_rap_name(expr, &self.tcx)
        .and_then(|n| self.borrow_map.get(&n))
        .map(|rd| rd.co_lenders.clone())
        .unwrap_or_default(),
      _ => Vec::new(),
    };
    (lender, aliasing, co_lenders)
  }

  fn get_ref_data_single(&self, expr: &'tcx Expr<'tcx>) -> (ResourceTy, VecDeque<String>){
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
          // (get_rap never yields DerefField; it's an assignment-LHS place.)
          ResourceTy::Anonymous | ResourceTy::Caller | ResourceTy::DerefField(..) | ResourceTy::Temporary { .. } => { (lender, aliasing) }
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
      // ... unless rustc reborrows it (`let r2: &mut String = r;`,
      // see is_coerced_mut_reborrow): then, as for `&mut *r`, the new
      // reference borrows through r and gives the borrow back to *r.
      if matches!(expr.kind, ExprKind::Path(_) | ExprKind::Field(..))
        && is_implicit_mut_reborrow(expr, self.tcx.typeck(expr.hir_id.owner)) {
        if let ResourceTy::Value(x) = get_rap(&expr, &self.tcx, &self.raps) {
          if x.is_mutref() {
            let lender = ResourceTy::Deref(x);
            let aliasing = get_aliasing_data(&lender, &self.borrow_map);
            return (lender, aliasing);
          }
        }
      }
      let lender = find_lender(&expr, &self.tcx, &self.raps, &self.borrow_map);
      (find_lender(&expr, &self.tcx, &self.raps, &self.borrow_map), get_aliasing_data(&lender, &self.borrow_map))
    }
  }

  // Add a RAP to our collection of raps for a let stmt
  pub fn define_lhs(&mut self, name: String, mutability: bool, expr: &'tcx Expr, ty: Ty <'tcx>) {
    let is_special = ty_is_special_owner(&ty);
    if ty.is_ref() {
      let (lender, aliasing, static_lender, co_lenders) = self.get_ref_data_full(&expr);
      let pointee_copy = self.ref_pointee_is_copy(ty, expr.hir_id.owner);
      self.add_ref(name.clone(), 
      bool_of_mut(ty.ref_mutability().unwrap()), 
      mutability, 
      expr_to_line(&expr, &self.tcx),
      lender,
      aliasing,
      self.current_scope, !self.inside_branch, pointee_copy);
      if !self.holder_shadowed(&name) {
        if let Some(rd) = self.borrow_map.get_mut(&name) {
          rd.static_lender = static_lender;
          rd.borrow_span = borrow_span_of(expr);
          rd.co_lenders = co_lenders;
        }
      }
    }
    else if matches!(ty.kind(), TyKind::Tuple(_)) && self.has_member_columns(ty) {
      // A tuple local gets a column per element (`t.0`, `t.1`), drawn
      // like a struct's fields (#189): moving `t.0` out leaves `t.1`
      // owned by `t`, and each element drops on its own.
      let owner_hash = self.rap_hashes as u64;
      let parent_is_copy = self.ty_is_copy(ty, expr.hir_id.owner);
      self.add_struct(name.clone(), owner_hash, false, mutability, parent_is_copy, self.ty_is_plain(ty, expr.hir_id.owner), self.ty_has_interior_mut(ty, expr.hir_id.owner), self.current_scope, !self.inside_branch);
      self.mark_owner_type(&name, ty);
      self.register_struct_members(&name, ty, owner_hash, mutability, MemberSite {
        owner_id: expr.hir_id.owner,
        assigned_at: expr_to_line(&expr, &self.tcx),
        scope: self.current_scope,
        is_global: !self.inside_branch,
        ref_lender: Some(ResourceTy::Anonymous),
      });
    }
    else if ty.is_adt() && !is_special {
      match ty.ty_adt_def().unwrap().adt_kind() {
        AdtKind::Struct => {
          let owner_hash = self.rap_hashes as u64;
          let parent_is_copy = self.ty_is_copy(ty, expr.hir_id.owner);
          self.add_struct(name.clone(), owner_hash, false, mutability, parent_is_copy, self.ty_is_plain(ty, expr.hir_id.owner), self.ty_has_interior_mut(ty, expr.hir_id.owner), self.current_scope,!self.inside_branch);
          self.mark_owner_type(&name, ty);
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
          self.register_struct_field_lenders(&name, expr, ty);
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
            self.add_union_owner(name.clone(), mutability, is_copy, self.ty_is_plain(ty, expr.hir_id.owner), interior_mut, self.current_scope, !self.inside_branch);
          } else {
            self.add_owner(name.clone(), mutability, is_copy, self.ty_is_plain(ty, expr.hir_id.owner), interior_mut, self.current_scope, !self.inside_branch);
          }
          self.register_holder(&name, expr, ty);
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
      self.add_owner(name.clone(), mutability, is_copy, self.ty_is_plain(ty, expr.hir_id.owner), self.ty_has_interior_mut(ty, expr.hir_id.owner), self.current_scope, !self.inside_branch);
      self.mark_owner_type(&name, ty);
      self.register_holder(&name, expr, ty);
    }
  }

  /// Fields that get a column of their own under a value of type `ty`,
  /// as (name, type) pairs in declaration order: a local struct's
  /// fields (`p.name`), or a tuple's elements (`t.0`, `t.1`). Empty
  /// for every other type (enums and unions are one opaque column;
  /// `String`, `Vec`, … don't expose their internals; `()` has
  /// nothing in it).
  pub fn member_fields(&self, ty: Ty<'tcx>) -> Vec<(String, Ty<'tcx>)> {
    match ty.kind() {
      TyKind::Adt(adt_def, args)
        if matches!(adt_def.adt_kind(), AdtKind::Struct) && !ty_is_special_owner(&ty) =>
      {
        adt_def.all_fields()
          .map(|f| (f.name.as_str().to_owned(), f.ty(self.tcx, args)))
          .collect()
      }
      TyKind::Tuple(elems) => elems.iter().enumerate()
        .map(|(i, t)| (i.to_string(), t))
        .collect(),
      _ => Vec::new(),
    }
  }

  /// True when a value of type `ty` is drawn as a group of member
  /// columns (see `member_fields`).
  pub fn has_member_columns(&self, ty: Ty<'tcx>) -> bool {
    !self.member_fields(ty).is_empty()
  }

  /// `let ex = Excerpt::new(&novel);`: the reference fields of a
  /// struct a call returns borrow what the callee's signature ties
  /// each field's lifetime to (#336). `register_struct_members` gave
  /// them an Anonymous lender; fill in the real ones.
  fn register_struct_field_lenders(&mut self, name: &str, init: &'tcx Expr<'tcx>, ty: Ty<'tcx>) {
    if !matches!(init.kind, ExprKind::Call(..) | ExprKind::MethodCall(..)) {
      return;
    }
    let Some(adt) = ty.ty_adt_def() else { return };
    for field in adt.all_fields() {
      let fname = format!("{}.{}", name, field.name.as_str());
      if !self.borrow_map.contains_key(&fname) {
        continue;
      }
      let (mut lenders, _, _) = self.call_field_lenders(init, field.name.as_str());
      if lenders.is_empty() {
        continue;
      }
      let lender = lenders.remove(0);
      let aliasing = get_aliasing_data(&lender, &self.borrow_map);
      if let Some(rd) = self.borrow_map.get_mut(&fname) {
        rd.lender = lender;
        rd.co_lenders = lenders;
        rd.aliasing = aliasing;
      }
    }
  }

  /// `h = rhs;` where `h` holds a borrow (#336): the borrow its old
  /// value held ends (at the old value's last use, if MIR says so),
  /// and `h` now holds whatever `rhs` borrows. `match_rhs` then draws
  /// the new borrow from the updated `borrow_map` entry.
  pub fn reassign_holder(&mut self, h: &ResourceAccessPoint, rhs: &'tcx Expr<'tcx>, ty: Ty<'tcx>, line: usize) {
    let name = h.name().to_owned();
    if let Some(old) = self.borrow_map.get(&name).cloned() {
      let mir_b_data = self.gather_borrow_data(self.bwf);
      let end = mir_b_data.iter()
        .find_map(|m| ExprVisitor::borrow_match(&old, m))
        .map_or(line, |kill| kill.min(line).max(old.assigned_at));
      let holder = ResourceTy::Value(h.clone());
      if get_borrowers(&name, &self.borrow_map).len() > 1 {
        let to = old.lender.clone();
        self.add_external_event(end, ExternalEvent::RefDie { from: holder.clone(), to, num_curr_borrowers: 1, id: *self.unique_id });
        *self.unique_id += 1;
      } else {
        for l in old.all_lenders().into_iter().filter(|l| matches!(l, ResourceTy::Value(_) | ResourceTy::Deref(_))) {
          let evt = if old.ref_mutability { Evt::MDie } else { Evt::SDie };
          self.add_ev(end, evt, l, holder.clone(), false);
        }
      }
      self.borrow_map.remove(&name);
    }
    let (mut lenders, is_mut) = self.holder_lenders(rhs, ty);
    if lenders.is_empty() {
      return;
    }
    let lender = lenders.remove(0);
    let aliasing = get_aliasing_data(&lender, &self.borrow_map);
    self.borrow_map.insert(name, RefData {
      lender,
      assigned_at: line,
      lifetime: line,
      ref_mutability: is_mut,
      aliasing,
      static_lender: None,
      borrow_span: None,
      co_lenders: lenders,
    });
  }

  /// After `let h = init;` was drawn: a holder whose initializer
  /// isn't a call (`[&a, &b]`, `vec![&s]`, which `match_rhs` draws
  /// as a plain bind) gets a borrow arrow from each lender (#349).
  /// Calls draw theirs in `match_rhs` (`emit_call_result_borrows`).
  pub fn emit_holder_literal_borrows(&mut self, name: &String, init: &'tcx Expr<'tcx>) {
    let built = init.span.from_expansion() || matches!(init.kind, ExprKind::Array(..) | ExprKind::Repeat(..));
    if !built {
      return;
    }
    let Some(rap) = self.raps.get(name).map(|rd| rd.rap.to_owned()) else { return };
    if !rap.holds_borrow() {
      return;
    }
    let Some(rd) = self.borrow_map.get(name).cloned() else { return };
    let line = expr_to_line(&init, &self.tcx);
    let to = ResourceTy::Value(rap);
    for l in rd.all_lenders().into_iter().filter(|l| matches!(l, ResourceTy::Value(_) | ResourceTy::Deref(_))) {
      let evt = if rd.ref_mutability { Evt::MBorrow } else { Evt::SBorrow };
      self.add_ev(line, evt, to.clone(), l, false);
    }
  }

  /// `name` was just registered as an owner bound to `init`. If its
  /// value holds a borrow of variables we draw (`let it = v.iter();`,
  /// `let third = v.get(2);`; see `holder_lenders`), mark it as a
  /// holder and record that borrow in `borrow_map` like a reference's,
  /// so it lasts until the holder's last use (MIR refines it in
  /// `print_lifetimes`), and a reference later taken out of it
  /// (`it.next().unwrap()`) finds `v` as its lender (#336).
  fn register_holder(&mut self, name: &String, init: &'tcx Expr<'tcx>, ty: Ty<'tcx>) {
    let (mut lenders, is_mut) = self.holder_lenders(init, ty);
    if lenders.is_empty() {
      // `let mut refs = Vec::new();` that a later `refs.push(&s)`
      // fills with borrows (#392): a holder from the start, with no
      // borrow yet; `record_receiver_store` adds each one.
      if self.filled_with_borrows_later(init, ty) {
        if let Some(rd) = self.raps.get_mut(name) {
          if let ResourceAccessPoint::Owner(o) = &mut rd.rap {
            o.holds_borrow = true;
          }
        }
      }
      return;
    }
    if let Some(rd) = self.raps.get_mut(name) {
      if let ResourceAccessPoint::Owner(o) = &mut rd.rap {
        o.holds_borrow = true;
      } else {
        return;
      }
    }
    // `let it2 = it;` moves the value, and the borrow with it: `it`
    // no longer holds it (a Copy holder like an `Option<&T>` is
    // copied, and both hold it).
    if let ExprKind::Path(..) = init.kind {
      if !self.ty_is_copy(ty, init.hir_id.owner) {
        if let Some(src) = expr_to_rap_name(init, &self.tcx) {
          self.borrow_map.remove(&src);
        }
      }
    }
    let line = expr_to_line(&init, &self.tcx);
    let lender = lenders.remove(0);
    let aliasing = get_aliasing_data(&lender, &self.borrow_map);
    self.borrow_map.insert(name.clone(), RefData {
      lender,
      assigned_at: line,
      lifetime: line,
      ref_mutability: is_mut,
      aliasing,
      static_lender: None,
      borrow_span: None,
      co_lenders: lenders,
    });
  }

  /// `let mut refs = Vec::new();` (a type that can hold references)
  /// whose variable is later the receiver of a `&mut self` method that
  /// can store a borrow into it (`refs.push(&s)`; see
  /// `call_stores_into_receiver`, #392).
  fn filled_with_borrows_later(&self, init: &'tcx Expr<'tcx>, ty: Ty<'tcx>) -> bool {
    if ty.is_ref() || !ty.walk().any(|g| matches!(g.kind(), GenericArgKind::Lifetime(_))) {
      return false;
    }
    let rustc_hir::Node::LetStmt(local) = self.tcx.parent_hir_node(init.hir_id) else { return false };
    let PatKind::Binding(_, hir_id, _, None) = local.pat.kind else { return false };
    struct StoreFinder<'v, 'a, 'tcx> {
      ev: &'v ExprVisitor<'a, 'tcx>,
      local: rustc_hir::HirId,
      found: bool,
    }
    impl<'v, 'a, 'tcx> rustc_hir::intravisit::Visitor<'tcx> for StoreFinder<'v, 'a, 'tcx> {
      fn visit_expr(&mut self, e: &'tcx Expr<'tcx>) {
        if let ExprKind::MethodCall(_, rcvr, _, _) = e.kind {
          if let ExprKind::Path(QPath::Resolved(None, path)) = rcvr.kind {
            if path.res == Res::Local(self.local) && self.ev.call_stores_into_receiver(e) {
              self.found = true;
            }
          }
        }
        rustc_hir::intravisit::walk_expr(self, e);
      }
    }
    let mut finder = StoreFinder { ev: self, local: hir_id, found: false };
    rustc_hir::intravisit::Visitor::visit_expr(&mut finder, self.hir_body.value);
    finder.found
  }

  /// `refs.push(&s);` where `refs` holds borrows (#392): the call
  /// stores `s`'s borrow in `refs`, which keeps `s` borrowed until
  /// refs's last use. Record `s` as one of refs's lenders and draw the
  /// borrow into `refs`.
  pub fn record_receiver_store(&mut self, call: &'tcx Expr<'tcx>, rcvr: &ResourceAccessPoint, line: usize) {
    if !rcvr.holds_borrow() {
      return;
    }
    let ExprKind::MethodCall(seg, ..) = call.kind else { return };
    let (lenders, is_mut) = self.receiver_store_lenders(call);
    if lenders.is_empty() {
      return;
    }
    let name = rcvr.name().to_owned();
    match self.borrow_map.get_mut(&name) {
      Some(rd) => {
        for l in lenders.iter() {
          if *l != rd.lender && !rd.co_lenders.contains(l) {
            rd.co_lenders.push(l.clone());
          }
        }
        rd.ref_mutability |= is_mut;
      }
      None => {
        let aliasing = get_aliasing_data(&lenders[0], &self.borrow_map);
        self.borrow_map.insert(name, RefData {
          lender: lenders[0].clone(),
          assigned_at: line,
          lifetime: line,
          ref_mutability: is_mut,
          aliasing,
          static_lender: None,
          borrow_span: None,
          co_lenders: lenders[1..].to_vec(),
        });
      }
    }
    let via = BorrowVia {
      func: seg.ident.as_str().to_owned(),
      lenders: lenders.iter().map(|l| l.name()).collect(),
      also_unnamed: false,
      tie: ArgTie::default(),
      carrier: ViaCarrier::StoredBy,
    };
    let to = ResourceTy::Value(rcvr.clone());
    for lender in lenders.iter() {
      let id = *self.unique_id;
      *self.unique_id += 1;
      let ev = if is_mut {
        ExternalEvent::MutableBorrow { from: lender.clone(), to: to.clone(), is_partial: false, is_reborrow: false, via: Some(via.clone()), id }
      } else {
        ExternalEvent::StaticBorrow { from: lender.clone(), to: to.clone(), is_partial: false, is_reborrow: false, via: Some(via.clone()), id }
      };
      self.add_external_event(line, ev);
    }
    // The call doesn't give the borrow back: `refs` keeps it.
    self.mark_borrow_kept_by_result(call, &lenders);
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
  /// construction-site initializer, the caller for a parameter, or
  /// `Caller` as a "not tracked here" placeholder for a pattern
  /// binding (what the matched value's field borrowed; #352).
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
    let owner_id = site.owner_id;
    for (field, field_ty) in self.member_fields(ty) {
      let field_name = format!("{}.{}", prefix, field);
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
          co_lenders: Vec::new(),
        });
      } else {
        let field_is_copy = self.ty_is_copy(field_ty, owner_id);
        self.add_struct(field_name.clone(), top_owner_hash, true, mutability, field_is_copy, self.ty_is_plain(field_ty, owner_id), self.ty_has_interior_mut(field_ty, owner_id),
            site.scope, site.is_global);
        self.mark_owner_type(&field_name, field_ty);
        // Recurse for nested struct / tuple fields. Skip enums /
        // unions (only structs and tuples are flattened) and the
        // special-owner builtins like `String` whose internals we
        // deliberately don't expose.
        if self.has_member_columns(field_ty) {
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

  /// The parts of `parent` that the sub-patterns of `pat` match, when
  /// `parent` is a place whose parts have columns of their own (a
  /// struct's fields `p.a`, a tuple's elements `t.0`; see
  /// `member_fields`) and `pat` destructures it (`P { a, .. }`,
  /// `P(x, _)`, `(a, b)`). Each sub-pattern comes with
  /// `Some((part, part_ty))`, or `None` for a part without a column of
  /// its own (a reference field). `None` overall when `pat` doesn't
  /// destructure such a place: an enum (`Some(x)`), a value matched
  /// through a reference, a place without part columns.
  pub fn pat_parts<'p>(
    &self,
    pat: &'p Pat<'tcx>,
    parent: &ResourceTy,
    parent_ty: Ty<'tcx>,
  ) -> Option<Vec<(&'p Pat<'tcx>, Option<(ResourceTy, Ty<'tcx>)>)>> {
    let ResourceTy::Value(parent_rap) = parent else { return None };
    if parent_ty.is_ref() || self.tcx.typeck(pat.hir_id.owner).node_type(pat.hir_id) != parent_ty {
      return None;
    }
    let fields = self.member_fields(parent_ty);
    let part = |i: Option<usize>| -> Option<(ResourceTy, Ty<'tcx>)> {
      let (field, field_ty) = fields.get(i?)?;
      if field_ty.is_ref() {
        return None;
      }
      let rd = self.raps.get(&format!("{}.{}", parent_rap.name(), field))?;
      Some((ResourceTy::Value(rd.rap.to_owned()), *field_ty))
    };
    let subs: Vec<(&'p Pat<'tcx>, Option<(ResourceTy, Ty<'tcx>)>)> = match pat.kind {
      PatKind::Tuple(sub_pats, ddpos) | PatKind::TupleStruct(_, sub_pats, ddpos) => {
        // `(a, .., z)`: the sub-patterns after `..` line up with the
        // last parts.
        let gap = fields.len().saturating_sub(sub_pats.len());
        sub_pats.iter().enumerate().map(|(k, sp)| {
          let i = match ddpos.as_opt_usize() {
            Some(d) if k >= d => k + gap,
            _ => k,
          };
          (sp, part(Some(i)))
        }).collect()
      }
      PatKind::Struct(_, pat_fields, _) => pat_fields.iter().map(|pf| {
        let i = fields.iter().position(|(f, _)| f.as_str() == pf.ident.name.as_str());
        (pf.pat, part(i))
      }).collect(),
      _ => return None,
    };
    subs.iter().any(|(_, p)| p.is_some()).then_some(subs)
  }

  /// The bindings of `pat` matched against `parent`, like
  /// [`get_dec_of_pat2`], except that a sub-pattern matching a part of
  /// `parent` with a column of its own (see [`pat_parts`]) binds from
  /// that part: `P { a, b }` on `p` moves `p.a` into `a` and `p.b` into
  /// `b`, rather than both "from p" (#326). Each result names the
  /// parent it binds from. Its last element is `Some(is_partial)` for
  /// a binding matched against a part (whether it takes only part of
  /// that part), and `None` where the caller's own rule applies.
  pub fn get_dec_of_pat_parts(
    &mut self,
    pat: &'tcx Pat<'tcx>,
    ty_results: &'tcx TypeckResults<'tcx>,
    parent: &ResourceTy,
    parent_ty: Ty<'tcx>,
    scope: usize,
    res: &mut Vec<(ResourceAccessPoint, Evt, ResourceTy, Option<bool>)>,
  ) {
    match self.pat_parts(pat, parent, parent_ty) {
      Some(subs) => {
        for (sp, part) in subs {
          match part {
            Some((part_rty, part_ty)) => {
              let mut sub = Vec::new();
              self.get_dec_of_pat_parts(sp, ty_results, &part_rty, part_ty, scope, &mut sub);
              let sp_ty = ty_results.node_type(sp.hir_id);
              res.extend(sub.into_iter().map(|(r, e, from, partial)| {
                (r, e, from, Some(partial.unwrap_or(part_ty != sp_ty)))
              }));
            }
            // A part without a column: bind from the whole parent, as
            // before.
            None => {
              let mut sub = Vec::new();
              self.get_dec_of_pat2(sp, ty_results, parent, &parent_ty, scope, &mut sub);
              res.extend(sub.into_iter().map(|(r, e, _)| (r, e, parent.clone(), None)));
            }
          }
        }
      }
      None => {
        let mut sub = Vec::new();
        self.get_dec_of_pat2(pat, ty_results, parent, &parent_ty, scope, &mut sub);
        res.extend(sub.into_iter().map(|(r, e, _)| (r, e, parent.clone(), None)));
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
            } else if matches!(ty.kind(), TyKind::Tuple(_)) && self.has_member_columns(ty) {
                // A tuple binding gets a column per element, like a
                // struct binding's fields (#189).
                let owner_hash = self.rap_hashes as u64;
                let parent_is_copy = self.ty_is_copy(ty, pat.hir_id.owner);
                self.add_struct(name.clone(), owner_hash, false, muta, parent_is_copy, self.ty_is_plain(ty, pat.hir_id.owner), self.ty_has_interior_mut(ty, pat.hir_id.owner), scope, false);
                self.mark_owner_type(&name, ty);
                for (field, field_ty) in self.member_fields(ty) {
                    // A reference element would need its own loan
                    // bookkeeping; leave it without a column.
                    if field_ty.is_ref() { continue; }
                    let field_is_copy = self.ty_is_copy(field_ty, pat.hir_id.owner);
                    let field_name = format!("{}.{}", name, field);
                    self.add_struct(field_name.clone(), owner_hash, true, muta, field_is_copy, self.ty_is_plain(field_ty, pat.hir_id.owner), self.ty_has_interior_mut(field_ty, pat.hir_id.owner), scope, false);
                    self.mark_owner_type(&field_name, field_ty);
                }
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
                        self.add_struct(name.clone(), owner_hash, false, muta, parent_is_copy, self.ty_is_plain(ty, pat.hir_id.owner), self.ty_has_interior_mut(ty, pat.hir_id.owner), scope, false);
                        self.mark_owner_type(&name, ty);
                        // Like define_lhs: don't expose the internals of
                        // `String` / `Vec` / `Box` / … (`n.vec` for a
                        // `Some(n)` binding of a String). Nested struct
                        // fields get columns too (`r.tl.x`, #239).
                        // A reference field (`x.part: &str`) is a
                        // reference, not an owner. What it borrows is
                        // whatever the matched value's field borrowed,
                        // which isn't known here, so it gets no lender
                        // (the `Caller` placeholder: no return arrow,
                        // like a parameter's reference field) and its
                        // dot says where it came from instead (#352).
                        if !ty_is_special_owner(&ty) {
                            self.register_struct_members(&name, ty, owner_hash, muta, MemberSite {
                                owner_id: pat.hir_id.owner,
                                assigned_at: line_num,
                                scope,
                                is_global: false,
                                ref_lender: Some(ResourceTy::Caller),
                            });
                            let prefix = format!("{}.", name);
                            #[expect(clippy::disallowed_methods, reason = "sorted by RAP hash below")]
                            let mut ref_fields: Vec<ResourceAccessPoint> = self.raps.iter()
                              .filter(|(n, rd)| n.starts_with(&prefix) && rd.rap.is_ref())
                              .map(|(_, rd)| rd.rap.clone())
                              .collect();
                            ref_fields.sort_by_key(|r| *r.hash());
                            for r in ref_fields {
                              let id = *self.unique_id;
                              *self.unique_id += 1;
                              self.add_external_event(line_num, ExternalEvent::InitRefParam { param: r, id, own_param: None, in_pattern: Some(name.clone()) });
                            }
                        }
                    },
                    // Unions render as opaque single-owner columns —
                    // see the matching note on the LHS-of-expr arm.
                    AdtKind::Union | AdtKind::Enum => {
                        let is_copy = self.ty_is_copy(ty, pat.hir_id.owner);
                        let interior_mut = self.ty_has_interior_mut(ty, pat.hir_id.owner);
                        if ty.is_union() {
                            self.add_union_owner(name.clone(), muta, is_copy, self.ty_is_plain(ty, pat.hir_id.owner), interior_mut, scope, false);
                        } else {
                            self.add_owner(name.clone(), muta, is_copy, self.ty_is_plain(ty, pat.hir_id.owner), interior_mut, scope, false);
                        }
                    }
                }
            } else {
                let is_copy = self.ty_is_copy(ty, pat.hir_id.owner);
                self.add_owner(name.clone(), muta, is_copy, self.ty_is_plain(ty, pat.hir_id.owner), self.ty_has_interior_mut(ty, pat.hir_id.owner), scope, false);
                self.mark_owner_type(&name, ty);
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

/// Emit a GoOutOfScope at `line` for each of `vars` (block-local
/// bindings that print_out_of_scope skips because they were registered
/// with is_global = false, e.g. a loop body's `let`s and a `for`
/// loop's pattern bindings). Ordered by RAP hash, i.e. registration
/// order, so the output doesn't depend on HashSet iteration order.
#[expect(clippy::iter_over_hash_type, clippy::disallowed_methods, reason = "the collected RAPs are sorted by hash below")]
pub fn emit_scope_end(&mut self, vars: HashSet<ResourceAccessPoint>, line: usize) {
    // Only variables this block registered itself (is_global = false).
    // A name that shadows an outer variable resolves to the outer RAP
    // (add_rap keeps the first registration, see the README's shadowing
    // limitation); that one keeps its own scope end.
    let is_block_local = |r: &ResourceAccessPoint| {
      self.raps.get(r.name()).map_or(false, |rd| !rd.is_global && rd.rap.hash() == r.hash())
    };
    let mut vars: Vec<ResourceAccessPoint> = vars.into_iter().filter(|r| is_block_local(r)).collect();
    // A struct's field columns (`p.name`) go out of scope with it.
    let owners: HashSet<u64> = vars.iter().filter_map(|r| match r {
      ResourceAccessPoint::Struct(st) if !st.is_member => Some(st.hash),
      _ => None,
    }).collect();
    for rd in self.raps.values() {
      if let ResourceAccessPoint::Struct(st) = &rd.rap {
        if st.is_member && owners.contains(&st.owner) && !rd.is_global {
          vars.push(rd.rap.clone());
        }
      }
    }
    vars.sort_by_key(|r| *r.hash());
    vars.dedup_by_key(|r| *r.hash());
    for var in vars {
        self.add_external_event(line, ExternalEvent::GoOutOfScope { ro: var, id: *self.unique_id });
        *self.unique_id += 1;
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

    let mut decls = Vec::new();
    self.get_dec_of_pat_parts(let_expr.pat, typeck_res, &parent, scrutinee_ty, scope, &mut decls);

    let mut registered: HashSet<ResourceAccessPoint> = HashSet::new();
    for (binding_rap, evt, from, partial) in decls {
        let is_partial = partial.unwrap_or(scrutinee_ty != typeck_res.node_type(let_expr.pat.hir_id));
        let event = self.ext_ev_of_evt(
            evt,
            ResourceTy::Value(binding_rap.clone()),
            from,
            *self.unique_id,
            is_partial,
        );
        self.add_external_event(let_line, event);
        *self.unique_id += 1;

        // A struct binding's field columns (`x.name`, `x.n`) are
        // declared here too: they go out of scope with `x`, and in a
        // Branch they're the arm's own variables, not pre-existing
        // ones (#405).
        let mut scoped = vec![binding_rap.clone()];
        if matches!(binding_rap, ResourceAccessPoint::Struct(Struct { is_member: false, .. })) {
            scoped.extend(self.struct_members_deep(binding_rap.name()));
        }
        for r in scoped {
            if let Some(body_end) = emit_gos_at {
                self.add_external_event(body_end, ExternalEvent::GoOutOfScope {
                    ro: r.clone(),
                    id: *self.unique_id,
                });
                *self.unique_id += 1;
            }
            registered.insert(r);
        }
    }
    registered
}

/// `lhs` is a reference being bound to `call`'s result, and
/// `borrow_map` says it borrows from variables we draw (resolved from
/// the callee's signature in `get_ref_data_full`): emit a borrow from
/// each of them, tagged with the call it came through, and mark the
/// argument-passing events of those lenders inside `call` as kept by
/// the result, so the lender isn't shown getting its resource back
/// when the call returns. Returns false (emitting nothing) otherwise.
pub fn emit_call_result_borrows(&mut self, lhs: &ResourceTy, call: &'tcx Expr<'tcx>, func: String, line_num: usize) -> bool {
  let ResourceTy::Value(lhs_rap) = lhs else { return false };
  // A reference, or a value that holds a borrow (`let it = v.iter();`,
  // registered by `register_holder`; #336).
  if !lhs_rap.is_ref() && !lhs_rap.holds_borrow() {
    return false;
  }
  // Draw only this call's own lenders: when the call is one branch of
  // an `if` / `match` initializer, `borrow_map` holds the union over
  // all branches (#337).
  let own = self.call_lenders(call);
  let (lenders, is_mut): (Vec<ResourceTy>, bool) = match self.borrow_map.get(lhs_rap.name()) {
    Some(rd) => (rd.all_lenders().into_iter()
      .filter(|l| matches!(l, ResourceTy::Value(_) | ResourceTy::Deref(_)) && own.contains(l))
      .collect(),
      if lhs_rap.is_ref() { lhs_rap.is_mutref() } else { rd.ref_mutability }),
    None => return false,
  };
  if lenders.is_empty() {
    return false;
  }
  let via = BorrowVia {
    func,
    lenders: lenders.iter().map(|l| l.name()).collect(),
    also_unnamed: self.call_lenders_full(call).1,
    tie: self.call_arg_tie(call, None),
    carrier: if lhs_rap.is_ref() { ViaCarrier::Reference } else { ViaCarrier::Value },
  };
  for lender in lenders.iter() {
    let id = *self.unique_id;
    let ev = if is_mut {
      ExternalEvent::MutableBorrow { from: lender.clone(), to: lhs.clone(), is_partial: false, is_reborrow: false, via: Some(via.clone()), id }
    } else {
      ExternalEvent::StaticBorrow { from: lender.clone(), to: lhs.clone(), is_partial: false, is_reborrow: false, via: Some(via.clone()), id }
    };
    self.add_external_event(line_num, ev);
    *self.unique_id += 1;
  }
  self.mark_borrow_kept_by_result(call, &lenders);
  true
}

/// `lhs` is a struct bound to `call`'s result whose reference fields
/// `register_struct_field_lenders` linked to the caller's variables:
/// draw a borrow from each lender into each such field, and keep the
/// lenders borrowed past the call (#336).
pub fn emit_struct_field_borrows(&mut self, lhs: &ResourceTy, call: &'tcx Expr<'tcx>, func: String, line_num: usize) {
  let ResourceTy::Value(ResourceAccessPoint::Struct(st)) = lhs else { return };
  if st.is_member {
    return;
  }
  let ty = self.tcx.typeck(call.hir_id.owner).expr_ty(call);
  let Some(adt) = ty.ty_adt_def() else { return };
  let mut all: Vec<ResourceTy> = Vec::new();
  for field in adt.all_fields() {
    let fname = format!("{}.{}", st.name, field.name.as_str());
    let (Some(rd), Some(frap)) = (self.borrow_map.get(&fname), self.raps.get(&fname)) else { continue };
    let frap = frap.rap.to_owned();
    let lenders: Vec<ResourceTy> = rd.all_lenders().into_iter()
      .filter(|l| matches!(l, ResourceTy::Value(_) | ResourceTy::Deref(_)))
      .collect();
    if lenders.is_empty() || !frap.is_ref() {
      continue;
    }
    let via = BorrowVia {
      func: func.clone(),
      lenders: lenders.iter().map(|l| l.name()).collect(),
      also_unnamed: self.call_field_lenders(call, field.name.as_str()).1,
      tie: self.call_arg_tie(call, Some(field.name.as_str())),
      carrier: ViaCarrier::StructField,
    };
    let to = ResourceTy::Value(frap.clone());
    for lender in lenders.iter() {
      let id = *self.unique_id;
      let ev = if frap.is_mutref() {
        ExternalEvent::MutableBorrow { from: lender.clone(), to: to.clone(), is_partial: false, is_reborrow: false, via: Some(via.clone()), id }
      } else {
        ExternalEvent::StaticBorrow { from: lender.clone(), to: to.clone(), is_partial: false, is_reborrow: false, via: Some(via.clone()), id }
      };
      self.add_external_event(line_num, ev);
      *self.unique_id += 1;
      if !all.contains(lender) {
        all.push(lender.clone());
      }
    }
  }
  if !all.is_empty() {
    self.mark_borrow_kept_by_result(call, &all);
  }
}

/// Set `result_keeps_borrow` on the pass-by-reference events that
/// `call`'s evaluation emitted from one of `lenders`: those borrows
/// continue in the call's result (see `emit_call_result_borrows`).
fn mark_borrow_kept_by_result(&mut self, call: &'tcx Expr<'tcx>, lenders: &[ResourceTy]) {
  let lo = span_to_line(&call.span, &self.tcx);
  let hi = span_end_line(&call.span, &self.tcx);
  let hashes: Vec<u64> = lenders.iter().filter_map(|l| l.extract_rap().map(|r| *r.hash())).collect();
  // Only the calls inside `call` (`first` and `unwrap` in
  // `v.first().unwrap()`), not another call on the same line
  // (`t.len()` in `if t.len() > 1 { first_word(&t) } ..`; #337).
  let mut fns: Vec<String> = Vec::new();
  self.call_names(call, &mut fns);
  let mut ids: Vec<usize> = Vec::new();
  mark_kept_in(&mut self.preprocessed_events, lo, hi, &hashes, &fns, &mut ids);
  for line in lo..=hi {
    if let Some(evs) = self.event_line_map.get_mut(&line) {
      for ev in evs.iter_mut() {
        if ids.contains(&ev.get_id()) {
          set_kept(ev);
        }
      }
    }
  }
}

/// The names of the calls in `e` (the Function RAPs their arguments
/// are passed to).
fn call_names(&self, e: &'tcx Expr<'tcx>, out: &mut Vec<String>) {
  match e.kind {
    ExprKind::Call(f, args) => {
      if let Some(n) = hirid_to_var_name(f.hir_id, &self.tcx) {
        out.push(n);
      }
      for a in args.iter() { self.call_names(a, out); }
    }
    ExprKind::MethodCall(seg, rcvr, args, _) => {
      out.push(seg.ident.as_str().to_owned());
      self.call_names(rcvr, out);
      for a in args.iter() { self.call_names(a, out); }
    }
    ExprKind::AddrOf(_, _, inner) | ExprKind::Unary(_, inner) | ExprKind::Field(inner, _)
    | ExprKind::DropTemps(inner) | ExprKind::Cast(inner, _) | ExprKind::Index(inner, _, _) => self.call_names(inner, out),
    _ => {}
  }
}

/// After `let x = if c { first_word(&t) } else { .. };` has been
/// visited: mark the argument-passing events of each branch's call as
/// kept by `x` (#337). `match_rhs` runs before the branches are
/// visited for such an initializer (see `process_let_binding`), so
/// `emit_call_result_borrows` can't mark them itself.
pub fn mark_branch_results_kept(&mut self, name: &str, init: &'tcx Expr<'tcx>) {
  let Some(tails) = branch_tails(init) else { return };
  let Some(lenders) = self.borrow_map.get(name).map(|rd| rd.all_lenders()) else { return };
  if !self.raps.get(name).is_some_and(|rd| rd.rap.is_ref()) {
    return;
  }
  for tail in tails {
    if matches!(tail.kind, ExprKind::Call(..) | ExprKind::MethodCall(..)) {
      let own: Vec<ResourceTy> = self.call_lenders(tail).into_iter().filter(|l| lenders.contains(l)).collect();
      if !own.is_empty() {
        self.mark_borrow_kept_by_result(tail, &own);
      }
    }
  }
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
          if self.is_coerced_mut_reborrow(&lhs, &rhs_rap, rhs, &evt) {
            self.add_borrow_ev(line_num, Evt::MBorrow, lhs, ResourceTy::Value(rhs_rap), true);
            return;
          }
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
      // A returned reference that borrows from the caller's variables
      // (`let w = first_word(&s);`) is drawn as a borrow of each of
      // them, not as a copy out of the function (#171).
      if let Some(func) = hirid_to_var_name(fn_expr.hir_id, &self.tcx) {
        if self.emit_call_result_borrows(&lhs, rhs, func, line_num) {
          return;
        }
      }
      // A call through a local `f` (closure, fn pointer) that leaves
      // `f` in place: the result comes from the call, so its source is
      // the call's Function RAP, not `f`'s column (#254, #299). An
      // `FnOnce` call does consume `f`, and keeps `f` as the source.
      let callee = match local_callee_result_key(rhs, &self.tcx) {
        // A Copy result (`let n = f();` with `f: FnOnce() -> i32`)
        // isn't a copy of anything `f` holds: it comes from the call
        // (#271).
        LocalCallee::Consumed if matches!(evt, Evt::Copy) => {
          match hirid_to_var_name(fn_expr.hir_id, &self.tcx) {
            Some(n) => LocalCallee::Call(fn_key(&n)),
            None => LocalCallee::Consumed,
          }
        }
        other => other,
      };
      match callee {
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
            .map(|rd| ResourceTy::Value(fn_result_rap(rhs, rd.rap.to_owned(), &self.tcx)))
            .unwrap_or(ResourceTy::Anonymous)
        }
        None => ResourceTy::Anonymous,
      };
      self.add_ev(line_num, evt, lhs.clone(), from, false);
      if let Some(func) = hirid_to_var_name(fn_expr.hir_id, &self.tcx) {
        self.emit_struct_field_borrows(&lhs, rhs, func, line_num);
      }
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
        None => panic!("internal state: a borrow expression has no `&`/`&mut` to take the mutability from")
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
          // `let n = *slots[0];` copies out of what the reference
          // stored in slots[0] points to; slots is not a reference
          // (#438). Name the element, as the write side does (#396).
          if index_base(expr).is_some() {
            if let Some(src) = self.deref_of_element(rhs) {
              self.add_ev(line_num, evt, lhs, src, false);
              return;
            }
          }
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

      // If the lhs is a reference that borrows from the caller's
      // variables (`s.as_str()`, `v.first().unwrap()`, `longest(..)`),
      // define_lhs has resolved its lenders from the method's
      // signature (`ref_flow.rs`): draw a borrow from each of them
      // instead of the generic Copy/Move out of the call (#171).
      if self.emit_call_result_borrows(&lhs, rhs, name_and_generic_args.ident.as_str().to_owned(), line_num) {
        return;
      }

      // Same Anonymous fallback as the Call arm — chained / macro /
      // desugared method calls may not have a registered name.
      let from = hirid_to_var_name(name_and_generic_args.hir_id, &self.tcx)
        .and_then(|n| self.raps.get(&fn_key(&n)).map(|rd| fn_result_rap(rhs, rd.rap.to_owned(), &self.tcx)))
        .map_or(ResourceTy::Anonymous, ResourceTy::Value);
      self.add_ev(line_num, evt, lhs.clone(), from, false);
      self.emit_struct_field_borrows(&lhs, rhs, name_and_generic_args.ident.as_str().to_owned(), line_num);
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
            let (lender, aliasing, static_lender, co_lenders) = self.get_ref_data_full(&field.expr);
            if let Some(rd) = self.borrow_map.get_mut(&new_lhs_name) {
              rd.lender = lender;
              rd.aliasing = aliasing;
              rd.static_lender = static_lender;
              rd.co_lenders = co_lenders;
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

    // Tuple literal `(e0, e1, …)`. A tuple local has a column per
    // element (`t.0`, `t.1`; see `member_fields`), so, like a struct
    // literal, each element is moved / copied / borrowed into its own
    // column (#189).
    ExprKind::Tup(elems) if !elems.is_empty() => {
      let line_num = span_to_line(&rhs.span, &self.tcx);
      let typeck = self.tcx.typeck(rhs.hir_id.owner);
      let elem_evt = |this: &Self, e: &Expr| {
        let ty = typeck.node_type(e.hir_id);
        if ty.is_ref() {
          match ty.ref_mutability().unwrap() {
            Mutability::Not => Evt::Copy,
            Mutability::Mut => Evt::Move,
          }
        } else if this.ty_is_copy(ty, e.hir_id.owner) {
          Evt::Copy
        } else {
          Evt::Move
        }
      };
      // Element columns of the destination, when it has them.
      let members: Option<Vec<Option<ResourceAccessPoint>>> = match &lhs {
        ResourceTy::Value(rap @ ResourceAccessPoint::Struct(_)) => {
          let m: Vec<Option<ResourceAccessPoint>> = (0..elems.len())
            .map(|i| self.raps.get(&format!("{}.{}", rap.name(), i)).map(|rd| rd.rap.to_owned()))
            .collect();
          if m.iter().any(|r| r.is_some()) { Some(m) } else { None }
        }
        _ => None,
      };
      if !matches!(lhs, ResourceTy::Anonymous) {
        self.add_ev(line_num, Evt::Bind, lhs.clone(), ResourceTy::Anonymous, false);
      }
      for (i, elem) in elems.iter().enumerate() {
        let e = elem_evt(self, elem);
        match members.as_ref().and_then(|m| m[i].clone()) {
          Some(member) => {
            // A reference element: resolve its lender from the
            // element expression, as the Struct arm does for
            // reference fields.
            if typeck.node_type(elem.hir_id).is_ref() {
              let (lender, aliasing) = self.get_ref_data(elem);
              if let Some(rd) = self.borrow_map.get_mut(member.name()) {
                rd.lender = lender;
                rd.aliasing = aliasing;
              }
            }
            self.match_rhs(ResourceTy::Value(member), elem, e);
          }
          // No column for this element (the tuple is returned to the
          // caller, written through a reference, …). A variable moved
          // into it is still moved: record that, so it isn't shown
          // dropping the value at its scope end.
          None => {
            let is_place = matches!(elem.kind, ExprKind::Path(_) | ExprKind::Field(..));
            if is_place && matches!(e, Evt::Move) && !elem.span.from_expansion()
              && !typeck.node_type(elem.hir_id).is_ref()
            {
              let dest = if matches!(lhs, ResourceTy::Caller) { ResourceTy::Caller } else { ResourceTy::Anonymous };
              self.match_rhs(dest, elem, e);
            }
          }
        }
      }
    }

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
        if self.is_coerced_mut_reborrow(&lhs, &rhs_rap, rhs, &evt) {
          self.add_borrow_ev(line_num, Evt::MBorrow, lhs, ResourceTy::Value(rhs_rap), true);
          return;
        }
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
    // `let r = refs[0];`: a copy of a reference stored in a collection
    // that holds borrows we track borrows what it borrows (#349).
    ExprKind::Index(base, _, _) if lhs.is_ref() && !lhs.is_mutref() => {
      let line_num = span_to_line(&rhs.span, &self.tcx);
      let ResourceTy::Value(lhs_rap) = &lhs else { return };
      if self.stored_ref_lenders(rhs).is_none() {
        return;
      }
      let lenders: Vec<ResourceTy> = match self.borrow_map.get(lhs_rap.name()) {
        Some(rd) => rd.all_lenders().into_iter()
          .filter(|l| matches!(l, ResourceTy::Value(_) | ResourceTy::Deref(_)))
          .collect(),
        None => return,
      };
      let Some(coll) = expr_to_rap_name(base, &self.tcx) else { return };
      let via = BorrowVia {
        func: coll,
        lenders: lenders.iter().map(|l| l.name()).collect(),
        also_unnamed: false,
        tie: ArgTie::default(),
        carrier: ViaCarrier::Element,
      };
      for lender in lenders {
        let id = *self.unique_id;
        self.add_external_event(line_num, ExternalEvent::StaticBorrow {
          from: lender, to: lhs.clone(), is_partial: false, is_reborrow: false, via: Some(via.clone()), id,
        });
        *self.unique_id += 1;
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
  
  /// A borrow held by a reference declared inside an `if` / `match`
  /// arm ends inside that arm (#389). `print_lifetimes` runs after the
  /// whole body is visited, when each arm's events are already inside
  /// its Branch event, so the borrow-end events it adds land on the
  /// fn's top level: the lender got its value back on its parent
  /// column, beside the arm's code, as if that happened whichever arm
  /// ran (and its arm column stayed borrowed, or stopped at a `&mut`
  /// borrow, down to the join). Move each one into the arm that
  /// declares its reference; that reference doesn't exist outside it.
  fn nest_borrow_ends_in_arms(&mut self) {
    let mut i = 0;
    while i < self.preprocessed_events.len() {
      let (line, ev) = &self.preprocessed_events[i];
      let path = match ev {
        ExternalEvent::StaticDie { from: ResourceTy::Value(r), to, .. }
        | ExternalEvent::MutableDie { from: ResourceTy::Value(r), to, .. }
        | ExternalEvent::RefDie { from: ResourceTy::Value(r), to, .. } =>
          to.extract_rap().and_then(|lender| arm_declaring(self.preprocessed_events, r, lender, *line)),
        _ => None,
      };
      match path {
        Some(mut path) => {
          let ev = self.preprocessed_events.remove(i);
          if path[0].0 > i { path[0].0 -= 1; }
          // Arrows listed together on a line are drawn one below the
          // other, with a blank row added per extra arrow. List this
          // event in the arm's line map only when it was stacked with
          // other arrows at the top level (`event_line_map`, which holds
          // arrows between two variables); otherwise it's drawn as the
          // first arrow on its line. Listing it always would add a blank
          // row beside arm-only arrows the top level never counted, e.g.
          // a pattern's own copy (Book `c8_03_vec_get_match`).
          let stacked = self.event_line_map.get(&ev.0).is_some_and(|evs| evs.len() > 1);
          insert_in_arm(self.preprocessed_events, &path, ev, stacked);
        }
        None => i += 1,
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
      // Several loans of the same lender can be issued on the
      // reference's line (`if t.len() > 1 { first_word(&t) } else
      // { .. }` borrows `t` twice). When more than one matches, take
      // the first one still live at the reference's last use seen in
      // the source (`data.lifetime`), not one that ended before it:
      // the borrow can't end before a use (#337). With a single match
      // this is the old behaviour.
      let kills: Vec<usize> = mir_b_data.iter()
        .filter(|m| !by_span.contains(&m.borrow_idx))
        .filter_map(|m_data| ExprVisitor::borrow_match(data, m_data))
        .collect();
      let chosen = match kills.len() {
        0 => None,
        1 => Some(kills[0]),
        _ => kills.iter().copied().find(|k| *k >= data.lifetime)
          .or_else(|| kills.iter().copied().max()),
      };
      if let Some(kill) = chosen {
        data.lifetime = kill;
      }
    }
    // A reference that borrows through another one (`let r = &v[0]` with
    // `v: &Vec<i32>`) keeps that other reference live until its own last
    // use. The visitor only extends `v` at the uses it can see; `r`'s
    // end can come from MIR alone (a use inside `println!`). So raise each
    // lending reference to the end of the references borrowed from it,
    // or `v`'s borrow end lands on the line that creates `r` (#416).
    // Taking the max makes the order of the passes irrelevant.
    #[expect(clippy::disallowed_methods, reason = "collected into a BTreeMap")]
    let ends: BTreeMap<String, (usize, Option<String>)> = self.borrow_map.iter().map(|(n, d)| {
      let lender = d.lender.extract_rap().map(|r| r.name().to_string());
      (n.clone(), (d.lifetime, lender))
    }).collect();
    for (name, (kill, _)) in &ends {
      let mut cur = ends.get(name).and_then(|e| e.1.clone());
      let mut seen: BTreeSet<String> = BTreeSet::new();
      while let Some(l) = cur {
        if !seen.insert(l.clone()) { break; }
        match self.borrow_map.get_mut(&l) {
          Some(d) => {
            if d.lifetime < *kill { d.lifetime = *kill; }
            cur = ends.get(&l).and_then(|e| e.1.clone());
          }
          None => break,
        }
      }
    }

    info!("BORROW MAP {:#?}", self.borrow_map);
    //let lifetime_map = self.lifetime_map.clone();
    // (reference, lender) pairs: a reference with co_lenders can be
    // the last borrower of one lender but not of another.
    let mut ultimate_refs: HashSet<(String, String)> = HashSet::new();
    // References whose value was moved on to another reference or the
    // caller (`let r2 = r;`): the borrow now ends with the new holder,
    // so the moved-from one gets no end event and doesn't take part in
    // picking which ref returns the borrow (#209).
    #[expect(clippy::disallowed_methods, reason = "collected into a set, order never observed")]
    let handed_off: HashSet<String> = self.borrow_map.keys()
      .filter(|k| self.ref_handed_off(k))
      .cloned()
      .collect();
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
    for (lender, refs) in lender_to_refs.iter() {
      let regions = get_regions(refs, &self.borrow_map);
      for region in regions.iter() {
        let region: Vec<&String> = region.iter().filter(|r| !handed_off.contains(*r)).collect();
        let mut max_lifetime: usize = 0;
        for r in region.iter() {
          let lifetime = self.borrow_map.get(*r).unwrap().lifetime;
          if lifetime > max_lifetime {
            max_lifetime = lifetime;
          }
        }
        for r in region.iter() {
          if self.borrow_map.get(*r).unwrap().lifetime == max_lifetime {
            ultimate_refs.insert((r.to_string(), lender.to_owned()));
          }
        }
      }
    }

    // if a reference borrowed from an anonymous owner then it must be an ultimate ref
    #[expect(clippy::disallowed_methods, clippy::iter_over_hash_type, reason = "only inserts into the ultimate_refs set")]
    for (name, ref_data) in self.borrow_map.iter() {
      match ref_data.lender {
        ResourceTy::Anonymous if !self.unlent_refs.contains(name) => {
          ultimate_refs.insert((name.to_owned(), ref_data.lender.real_name()));
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
      let RefData { lender: r_ty, lifetime, ref_mutability: ref_mut, co_lenders, .. } = ref_data;
      if handed_off.contains(k) {
        continue;
      }
      let from_ro = self.raps.get(k).unwrap().rap.to_owned();
      let to_ro = match r_ty {
        ResourceTy::Anonymous => ResourceTy::Deref(from_ro.clone()),
        _ => r_ty.clone()
      };
      // A reference that borrows from several variables gives each of
      // them back at its last use (#171).
      for co in co_lenders.iter().filter(|c| matches!(c, ResourceTy::Value(_) | ResourceTy::Deref(_))) {
        if ultimate_refs.contains(&(k.to_owned(), co.real_name())) {
          let evt = if *ref_mut { Evt::MDie } else { Evt::SDie };
          self.add_ev(*lifetime, evt, co.clone(), ResourceTy::Value(from_ro.clone()), false);
        }
      }
      // if ref is not an ultimate ref then it dies without returning a resource
      if !ultimate_refs.contains(&(k.to_owned(), r_ty.real_name())) { 
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
    self.nest_borrow_ends_in_arms();
    self.end_outer_borrows_in_arms();
  }

  /// A reference declared before an `if` / `match` whose last use is
  /// inside one of its arms: its borrow ends in that arm, and on the
  /// paths through the other arms it has already ended. `print_lifetimes`
  /// adds the borrow's end at that last use's line on the fn's top
  /// level, after the whole body is visited, so it was drawn on the
  /// reference's and the lender's *parent* columns, in the middle of
  /// the branch diamond, as if it happened whichever arm ran; the
  /// reference's column then ran on through the diamond to the join.
  ///
  /// Move it into the arm, and give every other arm its own end of the
  /// borrow: after the reference's last use in that arm, or where the
  /// arm starts if it doesn't use it. The lender then gets its value
  /// back on each path, so it gets a column in the conditional too.
  /// Loops are left alone: a borrow used in a loop body lasts across
  /// iterations.
  fn end_outer_borrows_in_arms(&mut self) {
    let mut i = 0;
    while i < self.preprocessed_events.len() {
      let (line, ev) = &self.preprocessed_events[i];
      let (r, lender) = match ev {
        ExternalEvent::StaticDie { from: ResourceTy::Value(r), to, .. }
        | ExternalEvent::MutableDie { from: ResourceTy::Value(r), to, .. } => {
          (r, to.extract_rap().filter(|l| *l != r))
        }
        // A RefDie gives nothing back to its lender (see print_lifetimes).
        ExternalEvent::RefDie { from: ResourceTy::Value(r), .. } => (r, None),
        _ => { i += 1; continue; }
      };
      // A function's column isn't drawn in a conditional's arms.
      if lender.is_some_and(|l| l.is_fn()) || branch_ending_borrow(self.preprocessed_events, r, *line).is_none() {
        i += 1;
        continue;
      }
      let (r, lender) = (r.clone(), lender.cloned());
      let ev = self.preprocessed_events.remove(i);
      // Arrows listed together on a line are drawn one below the other,
      // with a blank row per extra arrow. An arm's line map also lists
      // arrows to and from functions (#419), so listing the moved arrow
      // there would add a row the top level didn't have; list it only
      // if it was stacked with other arrows at the top level. An arrow
      // its line map doesn't list is drawn first on its line.
      let stacked = self.event_line_map.get(&ev.0).is_some_and(|evs| evs.len() > 1);
      end_borrow_in_arms(self.preprocessed_events, ev, &r, lender.as_ref(), stacked, self.unique_id);
    }
  }
}

/// The `if` / `match` among `events` that `r` is live in (declared
/// before it) and whose arm `a` holds `line`: (its index, a).
fn branch_ending_borrow(events: &[(usize, ExternalEvent)], r: &ResourceAccessPoint, line: usize) -> Option<(usize, usize)> {
  events.iter().enumerate().find_map(|(k, (_, ev))| {
    let ExternalEvent::Branch { branches, live_vars, split_point, merge_point, branch_type, .. } = ev else { return None };
    // Declared before the conditional: a `let` inside an arm is in that
    // arm's `decl_vars` (and, for a `match`, also in `live_vars`; #425).
    if matches!(branch_type, BranchType::Loop(..))
      || !live_vars.contains(r)
      || branches.iter().any(|b| b.decl_vars.contains(r))
    {
      return None;
    }
    // A borrow ending on the line an arm starts or the conditional ends
    // on can't be told apart from the conditional itself; leave it.
    if line <= *split_point || line >= *merge_point {
      return None;
    }
    (0..branches.len())
      .find(|&a| { let (start, end) = branch_type.get_start_end(a); start <= line && line <= end })
      .map(|a| (k, a))
  })
}

/// Whether `e` uses `r`; a conditional `r` is live in does.
fn touches(e: &ExternalEvent, r: &ResourceAccessPoint) -> bool {
  match e {
    ExternalEvent::Branch { live_vars, .. } => live_vars.contains(r),
    _ => {
      let (from, to) = ResourceAccessPoint_extract(e);
      from.extract_rap() == Some(r) || to.extract_rap() == Some(r)
    }
  }
}

/// Where `r` is last used in `events` (one arm's): (line, index just
/// after that event). A nested conditional counts up to its join.
fn last_use_in(events: &[(usize, ExternalEvent)], r: &ResourceAccessPoint) -> Option<(usize, usize)> {
  let i = events.iter().rposition(|(_, e)| touches(e, r))?;
  let line = match &events[i].1 {
    ExternalEvent::Branch { merge_point, .. } => *merge_point,
    _ => events[i].0,
  };
  Some((line, i + 1))
}

/// Put `ev`, the end of `r`'s borrow (see `end_outer_borrows_in_arms`),
/// into `events`: into the arm of the conditional that holds its line
/// (recursing into nested conditionals), with a copy ending the borrow
/// in each other arm; or, if no conditional holds it, in line order.
fn end_borrow_in_arms(
  events: &mut Vec<(usize, ExternalEvent)>,
  ev: (usize, ExternalEvent),
  r: &ResourceAccessPoint,
  lender: Option<&ResourceAccessPoint>,
  in_line_map: bool,
  unique_id: &mut usize,
) {
  let Some((k, a)) = branch_ending_borrow(events, r, ev.0) else {
    // Right after r's last use on or before its line, ahead of other
    // events on that line: in `Some(x) => ..` on `third`, third's
    // borrow ends once x is copied out of it, before x borrows v.
    let at = events.iter()
      .rposition(|(l, e)| *l <= ev.0 && touches(e, r))
      .or_else(|| events.iter().rposition(|(l, _)| *l < ev.0))
      .map_or(0, |i| i + 1);
    events.insert(at, ev);
    return;
  };
  let ExternalEvent::Branch { branches, live_vars, branch_type, .. } = &mut events[k].1 else {
    unreachable!("branch_ending_borrow returns a Branch")
  };
  // The lender gets its value back in each arm, so it has a column in
  // the conditional.
  if let Some(l) = lender {
    live_vars.insert(l.clone());
  }
  for (b, arm) in branches.iter_mut().enumerate() {
    if b == a {
      if in_line_map && ev.1.is_arrow_ev() && branch_ending_borrow(&arm.e_data, r, ev.0).is_none() {
        arm.line_map.entry(ev.0).or_default().push(ev.1.clone());
      }
      end_borrow_in_arms(&mut arm.e_data, ev.clone(), r, lender, in_line_map, unique_id);
      continue;
    }
    let mut copy = ev.1.clone();
    match &mut copy {
      ExternalEvent::StaticDie { id, .. } | ExternalEvent::MutableDie { id, .. } | ExternalEvent::RefDie { id, .. } => {
        *id = *unique_id;
        *unique_id += 1;
      }
      _ => unreachable!("only a borrow's end is moved into arms"),
    }
    let at = match last_use_in(&arm.e_data, r) {
      // After the arm's last use of r.
      Some(at) => at,
      // The arm doesn't use r: on this path the borrow ended before
      // the arm, so end it first thing in the arm.
      None => {
        let start = branch_type.get_start_end(b).0;
        (start, arm.e_data.iter().position(|(el, _)| *el >= start).unwrap_or(arm.e_data.len()))
      }
    };
    arm.e_data.insert(at.1, (at.0, copy));
  }
}

/// Whether a variable owns a resource at some point of the walk.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ownership {
  /// Owns a resource on every path.
  Held,
  /// Owns nothing (moved out, or never initialized).
  None,
  /// A conditional above moved it on some paths and not on others.
  /// Rust drops what's left with a runtime drop flag (#335).
  Maybe,
}

/// Ownership of `rap_name` after `events`, judged from the newest
/// ownership-affecting event (borrows and lends don't count);
/// `start` (the state before `events`) if no event decides it. A
/// finished if/else or match sits in the list as one Branch event,
/// decided by its arms, each starting from the state before the
/// conditional: if all arms end the same way that's the answer,
/// otherwise it's `Maybe`. An `if` without `else` that moves (#257)
/// has an empty `else` arm. Without looking inside, a reassignment
/// after `if c { take(s) } else { … }` said the old value is dropped
/// there unconditionally (#335), and one after `if c { take(s) } else
/// { take(s) }` dropped an already-moved value.
fn ownership_after(events: &[(usize, ExternalEvent)], rap_name: &str, start: Option<Ownership>) -> Option<Ownership> {
  let is_rap = |r: &ResourceTy| r.extract_rap().map_or(false, |r| r.name() == rap_name);
  for (i, (_, ev)) in events.iter().enumerate().rev() {
    match ev {
      ExternalEvent::ManuallyDropByHand { ro, .. } if ro.name() == rap_name => {
        return Some(Ownership::None);
      }
      // Moved before every `break` out of the loop: gone after it. If
      // only some ways out moved it, it's maybe-moved, which (like a
      // conditional that moved it on some paths) doesn't decide here:
      // the `if` that moved it before the `break` is in the loop body's
      // events above, and makes it `Maybe` (#335).
      ExternalEvent::LoopExitMove { ro, definite: true, .. } if ro.name() == rap_name => {
        return Some(Ownership::None);
      }
      ExternalEvent::Move { from, to, .. } => {
        if is_rap(to) {
          return Some(Ownership::Held);
        }
        if is_rap(from) {
          return Some(Ownership::None);
        }
      }
      ExternalEvent::Copy { to, .. } | ExternalEvent::Bind { to, .. } => {
        if is_rap(to) {
          return Some(Ownership::Held);
        }
      }
      ExternalEvent::Branch { branches, .. } if !branches.is_empty() => {
        let before = ownership_after(&events[..i], rap_name, start);
        let ends: Vec<Option<Ownership>> =
          branches.iter().map(|b| ownership_after(&b.e_data, rap_name, before)).collect();
        let holds = |e: &Option<Ownership>| matches!(e, Some(Ownership::Held) | Some(Ownership::Maybe));
        let moved = |e: &Option<Ownership>| matches!(e, Some(Ownership::None) | Some(Ownership::Maybe));
        return if ends.iter().all(|e| *e == ends[0]) {
          ends[0]
        } else if ends.iter().any(holds) && ends.iter().any(moved) {
          // Moved on some paths and not on others.
          Some(Ownership::Maybe)
        } else if ends.iter().any(holds) {
          // Initialized on some paths only (`let mut s; if c { s = …
          // }`, #234): treated as held, as before; README Limitations.
          Some(Ownership::Held)
        } else {
          // Moved, or never initialized, on every path.
          Some(Ownership::None)
        };
      }
      _ => {}
    }
  }
  start
}

/// The arm whose own `let`s or pattern declare `r`, if its conditional
/// spans `line` and `lender` has a column in it (is live in it, or is
/// declared in that arm too): the path of (event index, arm index)
/// pairs from `events` down through nested Branch events. Otherwise
/// the lender is drawn on one column throughout the conditional, and
/// its borrow's end stays where it is.
fn arm_declaring(
  events: &[(usize, ExternalEvent)],
  r: &ResourceAccessPoint,
  lender: &ResourceAccessPoint,
  line: usize,
) -> Option<Vec<(usize, usize)>> {
  for (k, (_, ev)) in events.iter().enumerate() {
    let ExternalEvent::Branch { branches, live_vars, split_point, merge_point, .. } = ev else { continue };
    if line < *split_point || line > *merge_point { continue; }
    for (a, b) in branches.iter().enumerate() {
      if let Some(mut path) = arm_declaring(&b.e_data, r, lender, line) {
        path.insert(0, (k, a));
        return Some(path);
      }
      if b.decl_vars.contains(r) && (live_vars.contains(lender) || b.decl_vars.contains(lender)) {
        return Some(vec![(k, a)]);
      }
    }
  }
  None
}

/// Insert `ev` into the arm `path` leads to (see `arm_declaring`),
/// after that arm's events on its line or earlier, and into its line
/// map if `in_line_map` (see `create_line_map`).
fn insert_in_arm(
  events: &mut [(usize, ExternalEvent)],
  path: &[(usize, usize)],
  ev: (usize, ExternalEvent),
  in_line_map: bool,
) {
  let Some(((k, a), rest)) = path.split_first() else { return };
  let ExternalEvent::Branch { branches, .. } = &mut events[*k].1 else {
    unreachable!("arm_declaring's path leads through Branch events")
  };
  let arm = &mut branches[*a];
  if !rest.is_empty() {
    return insert_in_arm(&mut arm.e_data, rest, ev, in_line_map);
  }
  let at = arm.e_data.iter().rposition(|(l, _)| *l <= ev.0).map_or(0, |i| i + 1);
  if in_line_map && ev.1.is_var_arrow_ev() {
    arm.line_map.entry(ev.0).or_default().push(ev.1.clone());
  }
  arm.e_data.insert(at, ev);
}

/// The expressions an `if` / `match` (or a block ending in one)
/// evaluates to, one per branch; `None` for anything else. A branch
/// that diverges (`panic!()`, `return`) has no value and is left out.
pub fn branch_tails<'tcx>(e: &'tcx Expr<'tcx>) -> Option<Vec<&'tcx Expr<'tcx>>> {
  fn push<'tcx>(e: &'tcx Expr<'tcx>, out: &mut Vec<&'tcx Expr<'tcx>>) {
    match e.kind {
      ExprKind::Block(b, _) => { if let Some(t) = b.expr { push(t, out) } }
      ExprKind::DropTemps(inner) => push(inner, out),
      ExprKind::If(..) | ExprKind::Match(_, _, rustc_hir::MatchSource::Normal) => {
        if let Some(ts) = branch_tails(e) { out.extend(ts) }
      }
      _ => out.push(e),
    }
  }
  let mut out = Vec::new();
  match e.kind {
    ExprKind::If(_, then, Some(els)) => { push(then, &mut out); push(els, &mut out); }
    ExprKind::Match(_, arms, rustc_hir::MatchSource::Normal) => {
      for arm in arms.iter() { push(arm.body, &mut out); }
    }
    ExprKind::Block(b, _) => match b.expr {
      Some(t) if matches!(t.kind, ExprKind::If(..) | ExprKind::Match(_, _, rustc_hir::MatchSource::Normal)) => return branch_tails(t),
      _ => return None,
    },
    ExprKind::DropTemps(inner) => return branch_tails(inner),
    _ => return None,
  }
  Some(out)
}

fn set_kept(ev: &mut ExternalEvent) {
  if let ExternalEvent::PassByStaticReference { result_keeps_borrow, .. }
       | ExternalEvent::PassByMutableReference { result_keeps_borrow, .. } = ev {
    *result_keeps_borrow = true;
  }
}

/// Set `result_keeps_borrow` on the pass-by-reference events from one
/// of `hashes` into a function on lines `lo..=hi`, including those
/// already folded into a branch (recording their ids in `ids`).
fn mark_kept_in(events: &mut Vec<(usize, ExternalEvent)>, lo: usize, hi: usize, hashes: &[u64], fns: &[String], ids: &mut Vec<usize>) {
  for (line, ev) in events.iter_mut() {
    let pass_from_lender = match &*ev {
      ExternalEvent::PassByStaticReference { from, to: ResourceTy::Value(ResourceAccessPoint::Function(f)), .. }
      | ExternalEvent::PassByMutableReference { from, to: ResourceTy::Value(ResourceAccessPoint::Function(f)), .. } =>
        fns.contains(&f.name) && matches!(from, ResourceTy::Value(r) if hashes.contains(r.hash())),
      _ => false,
    };
    if pass_from_lender {
      if *line >= lo && *line <= hi {
        ids.push(ev.get_id());
        set_kept(ev);
      }
      continue;
    }
    match ev {
      ExternalEvent::Branch { branches, .. } => {
        for b in branches.iter_mut() {
          let mut inner: Vec<usize> = Vec::new();
          mark_kept_in(&mut b.e_data, lo, hi, hashes, fns, &mut inner);
          for evs in b.line_map.values_mut() {
            for e in evs.iter_mut() {
              if inner.contains(&e.get_id()) {
                set_kept(e);
              }
            }
          }
          ids.extend(inner);
        }
      }
      _ => {}
    }
  }
}
