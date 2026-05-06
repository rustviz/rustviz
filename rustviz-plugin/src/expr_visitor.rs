//! This file details all the datastructures that are filled while traversing the HIR.
//! Essentially we care about detailing interactions and relationships between 
//! Resource Access Points (RAPs - RV1 terminology)
//! The frontend (svg-generator crate) requires a list of external events:
//! which are generally events that cause some visually external effect on the visualization
//! For example: Moves, Copies, Borrows are all represented with arrows between timelines.


use log::{error, info, warn};
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
#[derive(Debug, Clone)]
pub struct RefData {
  pub lender: ResourceTy, // who this reference borrowed from
  pub assigned_at: usize, // gen point
  pub lifetime: usize, // kill point
  pub ref_mutability: bool, // type
  pub aliasing: VecDeque<String> // aliasing other references
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

  // Vestigial code, look at aquascope permissions_boundary map to see more
  pub analysis_result : HashMap<usize, Vec<String>>,

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
  pub fn_ret: bool
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

  // Adds an owner RAP
  pub fn add_owner(&mut self, name: String, mutability: bool, is_copy: bool, scope: usize, is_global: bool) {
    self.add_rap(ResourceAccessPoint::Owner(Owner{name: name, hash: self.rap_hashes as u64, is_mut: mutability, is_copy}), scope, is_global);
  }

  // Adds a reference RAP
  pub fn add_ref(&mut self, name: String, ref_mutability: bool, lhs_mut: bool, line_num: usize, lender: ResourceTy, alia: VecDeque<String>, scope: usize, is_global: bool) {
    match ref_mutability {
      true => {
        self.add_mut_ref(name.clone(), lhs_mut, scope, is_global);
      }
      false => { 
        self.add_static_ref(name.clone(), lhs_mut, scope, is_global);
      }
    }
    self.borrow_map.insert(name.clone(), RefData { lender: lender.clone(), assigned_at: line_num, lifetime: line_num, ref_mutability: ref_mutability, aliasing: alia });
  }
  
  pub fn add_static_ref(&mut self, name: String, mutability: bool, scope: usize, is_global: bool) {
    self.add_static_ref_member(name, mutability, scope, is_global, None);
  }

  pub fn add_mut_ref(&mut self, name: String, mutability: bool, scope: usize, is_global: bool) {
    self.add_mut_ref_member(name, mutability, scope, is_global, None);
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
  ) {
    self.add_rap(
      ResourceAccessPoint::StaticRef(StaticRef {
        name,
        hash: self.rap_hashes as u64,
        is_mut: mutability,
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
  ) {
    self.add_rap(
      ResourceAccessPoint::MutRef(MutRef {
        name,
        hash: self.rap_hashes as u64,
        is_mut: mutability,
        member_of,
      }),
      scope, is_global,
    );
  }

  // Adds a function RAP
  pub fn add_fn(&mut self, name: String) {
    self.add_rap(ResourceAccessPoint::Function(Function { name: name, hash: self.rap_hashes as u64 }), self.current_scope, true);
  }

  // Adds a struct RAP
  // To be honest this struct logic is leftover from RV1 and should eventually be reworked
  // It doesn't make much sense that a struct should be represented any different than an owner
  // the reason for this is that in the frontend structs are visualized differently in the timeline header
  pub fn add_struct(&mut self, name: String, owner: u64, mem: bool, mutability: bool, is_copy: bool, scope: usize, is_global: bool) {
    self.add_rap(ResourceAccessPoint::Struct(Struct {
      name: name,
      hash: self.rap_hashes as u64,
      owner: owner,
      is_mut: mutability,
      is_member: mem,
      is_copy }),
      scope, is_global);
  }

  pub fn add_rap(&mut self, r: ResourceAccessPoint, scope: usize, is_global: bool) {
    let fn_start_line = self.current_fn_start;
    self.raps.entry(r.name().to_string()).or_insert_with(|| {
      self.rap_hashes += 1;
      RapData { rap: r, scope, is_global, fn_start_line }
    });
  }

  pub fn update_rap(&mut self, r: &ResourceAccessPoint, line_num: usize) {
    if r.is_ref() {
      self.update_lifetime(r.name(), line_num);
    }
  }

  pub fn add_external_event(&mut self, line_num: usize, event: ExternalEvent) {
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
      | (ResourceTy::Deref(_), ResourceTy::Value(_), _) => {
        self.event_line_map.get_mut(&line_num).unwrap().push(event);
      },
      _ => ()
    }
  }

  // Best-effort answer to "does this RAP currently own a resource?",
  // derived from the events emitted so far. Used to decide whether a
  // reassignment should produce an OwnerDropAtReassign — `let mut y;
  // y = x;` and `let mut y = a; let z = y; y = x;` both reach the
  // same reassignment site but only the first has a prior resource
  // to drop; for the second, y was moved out at the prior `let z = y`.
  // Walks `preprocessed_events` newest → oldest, looking for the
  // most recent ownership-affecting event involving the named RAP.
  // Borrow/lend events don't transfer ownership and are ignored.
  pub fn rap_holds_resource_now(&self, rap_name: &str) -> bool {
    for (_, ev) in self.preprocessed_events.iter().rev() {
      match ev {
        ExternalEvent::Move { from, to, .. } => {
          if to.extract_rap().map_or(false, |r| r.name() == rap_name) {
            return true;
          }
          if from.extract_rap().map_or(false, |r| r.name() == rap_name) {
            return false;
          }
        }
        ExternalEvent::Copy { to, .. }
        | ExternalEvent::Bind { to, .. } => {
          if to.extract_rap().map_or(false, |r| r.name() == rap_name) {
            return true;
          }
        }
        _ => {}
      }
    }
    // No prior ownership event — variable was declared but never
    // initialized (`let mut y; y = x;` first iteration), so there's
    // nothing to drop.
    false
  }

  pub fn ext_ev_of_evt(&self, evt: Evt, lhs: ResourceTy, rhs: ResourceTy, id: usize, is_partial: bool) -> ExternalEvent{
    match evt {
      Evt::Bind => ExternalEvent::Bind { from: rhs, to: lhs, id },
      Evt::Copy => ExternalEvent::Copy { from: rhs, to: lhs, id, is_partial },
      Evt::Move => ExternalEvent::Move { from: rhs, to: lhs, id, is_partial },
      Evt::SBorrow => ExternalEvent::StaticBorrow { from: rhs, to: lhs, id, is_partial },
      Evt::MBorrow => ExternalEvent::MutableBorrow { from: rhs, to: lhs, id, is_partial },
      Evt::PassBySRef => ExternalEvent::PassByStaticReference { from: rhs, to: lhs, id },
      Evt::PassByMRef => ExternalEvent::PassByMutableReference { from: rhs, to: lhs, id },
      Evt::SDie =>  ExternalEvent::StaticDie { from: rhs, to: lhs, id },
      Evt::MDie => ExternalEvent::MutableDie { from: rhs, to: lhs, id }
    }
  }

  pub fn add_ev(&mut self, line_num: usize, evt: Evt, lhs: ResourceTy, rhs: ResourceTy, is_partial: bool) {
    self.add_external_event(line_num, self.ext_ev_of_evt(evt, lhs, rhs, *self.unique_id, is_partial));
    *self.unique_id += 1;
  }

  // Return the ResourceTy that corresponds to the LHS of a let stmt
  pub fn resource_of_lhs(&mut self, expr: &'tcx Expr) -> ResourceTy {
    match expr.kind {
      ExprKind::Path(QPath::Resolved(_, p)) => {
        let name = self.tcx.hir_name(p.segments[0].hir_id).as_str().to_owned();
        ResourceTy::Value(self.raps.get(&name).unwrap().rap.to_owned())
      }
      ExprKind::Field(expr, ident) => {
        match expr {
          Expr{kind: ExprKind::Path(QPath::Resolved(_,p)), ..} => {
            let name = self.tcx.hir_name(p.segments[0].hir_id).as_str().to_owned();
            let total_name = format!("{}.{}", name, ident.as_str());
            ResourceTy::Value(self.raps.get(&total_name).unwrap().rap.to_owned())
          }
          _ => { panic!("unexpected field expr") }
        } 
      }
      ExprKind::Unary(UnOp::Deref, exp) => {
        let rhs_rap = fetch_rap(&expr, &self.tcx, &self.raps);
        let line_num = expr_to_line(&exp, &self.tcx);
        match rhs_rap {
          Some(x) => {
            self.update_rap(&x, line_num);
            ResourceTy::Deref(x)
          }
          None => { ResourceTy::Anonymous }
        }
      }
      _ => panic!("invalid lhs")
    }
  }

  // add events for arguments in a function call
  pub fn match_arg(&mut self, arg: &'tcx Expr, fn_name: String) {
    self.add_fn(fn_name.clone());
    let line_num = expr_to_line(&arg, &self.tcx);
    let tycheck_results = self.tcx.typeck(arg.hir_id.owner);
    let arg_ty = tycheck_results.node_type(arg.hir_id);
    let is_copyable = self.tcx.type_is_copy_modulo_regions(rustc_middle::ty::TypingEnv::post_analysis(self.tcx, arg.hir_id.owner), arg_ty);
    let from_ro = get_rap(arg, &self.tcx, &self.raps);
    let to_ro = ResourceTy::Value(self.raps.get(&fn_name).unwrap().rap.to_owned());
    // type-check the arg and add event accordingly
    if arg_ty.is_ref() {
      match arg_ty.ref_mutability().unwrap() {
        Mutability::Not => self.add_ev(line_num, Evt::PassBySRef, to_ro, from_ro, false),
        Mutability::Mut => self.add_ev(line_num, Evt::PassByMRef, to_ro, from_ro, false)
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
          ResourceTy::Value(x) | ResourceTy::Deref(x) => {
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
    let is_special = ty_is_special_owner(&self.tcx, &ty);
    if ty.is_ref() {
      let (lender, aliasing) = self.get_ref_data(&expr);
      self.add_ref(name.clone(), 
      bool_of_mut(ty.ref_mutability().unwrap()), 
      mutability, 
      expr_to_line(&expr, &self.tcx),
      lender,
      aliasing,
      self.current_scope, !self.inside_branch);
    }
    else if ty.is_adt() && !is_special {
      match ty.ty_adt_def().unwrap().adt_kind() {
        AdtKind::Struct => {
          let owner_hash = self.rap_hashes as u64;
          let parent_is_copy = self.ty_is_copy(ty, expr.hir_id.owner);
          self.add_struct(name.clone(), owner_hash, false, mutability, parent_is_copy, self.current_scope,!self.inside_branch);
          // Resolve each field's substituted type so we can tell
          // which fields are references (a `Excerpt<'a> { p: &'a
          // str }` has p: &str at construction time, modelled as
          // a StaticRef RAP that's a member of the parent struct)
          // versus owned values (registered as Struct members
          // exactly as before).
          let generic_args = match ty.kind() {
            TyKind::Adt(_, args) => *args,
            _ => unreachable!("ty.is_adt() but kind is not Adt"),
          };
          for field in ty.ty_adt_def().unwrap().all_fields() {
            let field_name = format!("{}.{}", name.clone(), field.name.as_str());
            let field_ty = field.ty(self.tcx, generic_args);
            if field_ty.is_ref() {
              let ref_mutability = bool_of_mut(field_ty.ref_mutability().unwrap());
              // The field's lender is whatever the user passed in
              // for that field at the construction site; we wire
              // that up later in match_rhs's Struct arm via the
              // already-computed get_ref_data path. For now, seed
              // borrow_map with Anonymous lender as a placeholder
              // so the renderer can still draw the ref-line; the
              // Struct arm will overwrite it.
              if ref_mutability {
                self.add_mut_ref_member(field_name.clone(), mutability,
                    self.current_scope, !self.inside_branch, Some(owner_hash));
              } else {
                self.add_static_ref_member(field_name.clone(), mutability,
                    self.current_scope, !self.inside_branch, Some(owner_hash));
              }
              // The borrow stored in this field is alive for as
              // long as the parent struct is — so the loan
              // extends to the end of the enclosing scope, same
              // policy as fn-param refs which also borrow for
              // the full body. Without this the ref-line
              // trapezoid collapses (assigned_at == lifetime).
              self.borrow_map.insert(field_name, RefData {
                lender: ResourceTy::Anonymous,
                assigned_at: expr_to_line(&expr, &self.tcx),
                lifetime: self.current_scope,
                ref_mutability,
                aliasing: VecDeque::new(),
              });
            } else {
              let field_is_copy = self.ty_is_copy(field_ty, expr.hir_id.owner);
              self.add_struct(field_name, owner_hash, true, mutability, field_is_copy,
                  self.current_scope, !self.inside_branch);
            }
          }
        },
        AdtKind::Union => {
          warn!("lhs union not implemented yet")
        },
        AdtKind::Enum => {
          let is_copy = self.ty_is_copy(ty, expr.hir_id.owner);
          self.add_owner(name, mutability, is_copy, self.current_scope, !self.inside_branch);
        }
      }
    }
    else if ty.is_fn() {
      error!("cannot have fn as lhs of expr");
    }
    else {
      let is_copy = self.ty_is_copy(ty, expr.hir_id.owner);
      self.add_owner(name, mutability, is_copy, self.current_scope, !self.inside_branch);
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
        PatKind::Binding(mode, id, ident, _) => {
            // add raps that appear in binding
            let muta = bool_of_mut(mode.1); // You need to define this function
            let line_num = span_to_line(&ident.span, &self.tcx); // Assuming self.span_to_line exists
            let ty: Ty<'tcx> = ty_results.node_type(id);
            let name = ident.to_string();
            // copied code - should definetly make a function of this
            if ty.is_ref() {
                let ref_mutability = bool_of_mut(ty.ref_mutability().unwrap()); // You need to define this function
                self.add_ref(name.clone(), ref_mutability, muta, line_num, parent.clone(), VecDeque::new(), scope, false);
            } else if ty.is_adt() {
                match ty.ty_adt_def().unwrap().adt_kind() {
                    AdtKind::Struct => {
                        let owner_hash = self.rap_hashes as u64; // Assuming self.rap_hashes exists
                        let parent_is_copy = self.ty_is_copy(ty, pat.hir_id.owner);
                        self.add_struct(name.clone(), owner_hash, false, muta, parent_is_copy, scope, false);
                        let generic_args = match ty.kind() {
                            TyKind::Adt(_, args) => *args,
                            _ => unreachable!("ty.is_adt() but kind is not Adt"),
                        };
                        for field in ty.ty_adt_def().unwrap().all_fields() {
                            let field_name = format!("{}.{}", name, field.name.as_str());
                            let field_ty = field.ty(self.tcx, generic_args);
                            let field_is_copy = self.ty_is_copy(field_ty, pat.hir_id.owner);
                            self.add_struct(field_name, owner_hash, true, muta, field_is_copy, scope, false);
                        }
                    },
                    AdtKind::Union => {
                        panic!("union not implemented yet")
                    },
                    AdtKind::Enum => {
                        let is_copy = self.ty_is_copy(ty, pat.hir_id.owner);
                        self.add_owner(name.clone(), muta, is_copy, scope, false);
                    }
                }
            } else {
                let is_copy = self.ty_is_copy(ty, pat.hir_id.owner);
                self.add_owner(name.clone(), muta, is_copy, scope, false);
            }

            let evt = if parent_ty.is_ref() {
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

// This function does the work of adding an event for let stmts
pub fn match_rhs(&mut self, lhs: ResourceTy, rhs:&'tcx Expr, evt: Evt){
  match rhs.kind {
    ExprKind::Path(QPath::Resolved(_,p)) => {
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
      let rhs_rap = self.raps.get(&rhs_name).unwrap().rap.to_owned();
      self.update_rap(&rhs_rap, line_num);
      self.add_ev(line_num, evt, lhs, ResourceTy::Value(rhs_rap), false);
    },
    // fn_expr: resolves to function itself (Path)
    // second arg, is a list of args to the function
    ExprKind::Call(fn_expr, _) => {
      let line_num = span_to_line(&fn_expr.span, &self.tcx);
      let fn_name = hirid_to_var_name(fn_expr.hir_id, &self.tcx).unwrap();
      let rhs_rap = self.raps.get(&fn_name).unwrap().rap.to_owned();
      self.add_ev(line_num, evt, lhs, ResourceTy::Value(rhs_rap), false);
    },
    
    ExprKind::Lit(_) | ExprKind::Binary(..) | // Any type of literal on RHS implies a bind
    ExprKind::Unary(UnOp::Neg, _) | // ~<expr>
    ExprKind::Unary(UnOp::Not, _) // !<expr>
    => {
      let line_num = span_to_line(&rhs.span, &self.tcx);
      self.add_ev(line_num, Evt::Bind, lhs, ResourceTy::Anonymous, false);
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
      match fetch_mutability(&rhs) { // fetch last ref mutability in the chain -> &&&mut x
        Some(Mutability::Not) => self.add_ev(line_num, Evt::SBorrow, lhs, res, false),
        Some(Mutability::Mut) => self.add_ev(line_num, Evt::MBorrow, lhs, res, false),
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

      let fn_name = hirid_to_var_name(name_and_generic_args.hir_id, &self.tcx).unwrap();
      let rhs_rap = self.raps.get(&fn_name).unwrap().rap.to_owned();
      self.add_ev(line_num, evt, lhs, ResourceTy::Value(rhs_rap), false);
    }
    // Struct intializer list:
    // ex struct = {a: <expr>, b: <expr>, c: <expr>}
    ExprKind::Struct(_qpath, expr_fields, _base) => {
      let line_num = span_to_line(&rhs.span, &self.tcx);
      self.add_ev(line_num, Evt::Bind, lhs.clone(), ResourceTy::Anonymous, false);
      for field in expr_fields.iter() {
          let new_lhs_name = format!("{}.{}", lhs.name(), field.ident.as_str());
          let field_rap = self.raps.get(&new_lhs_name).unwrap().rap.to_owned();
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
            let (lender, aliasing) = self.get_ref_data(&field.expr);
            if let Some(rd) = self.borrow_map.get_mut(&new_lhs_name) {
              rd.lender = lender;
              rd.aliasing = aliasing;
            }
          }
          self.match_rhs(ResourceTy::Value(field_rap), field.expr, e);
      }
    },

    ExprKind::Field(expr, id) => {
      match expr {
        Expr{kind: ExprKind::Path(QPath::Resolved(_,p)), ..} => {
          let line_num = span_to_line(&p.span, &self.tcx);
          let name = self.tcx.hir_name(p.segments[0].hir_id).as_str().to_owned();
          let total_name = format!("{}.{}", name, id.as_str());
          let rhs_rap = self.raps.get(&total_name).unwrap().rap.to_owned();
          self.add_ev(line_num, evt, lhs, ResourceTy::Value(rhs_rap), false);
        }
        _ => panic!("unexpected field expr")
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
    // TODO: implement conditional let bindings:
    // let x = if {} else {};
    // let x = match z {};
    ExprKind::If(_, if_block, else_block) => {
      self.match_rhs(lhs.clone(), &if_block, evt.clone());
      match else_block {
        Some(e) => self.match_rhs(lhs, &e, evt),
        None => {}
      }
    }
    ExprKind::DropTemps(exp) => {
      self.match_rhs(lhs, &exp, evt);
    }
    _ => {
      warn!("unmatched rhs {:#?}", rhs);
    }
  }
}

// add GOS events for RAPs in the current scope  
pub fn print_out_of_scope(&mut self){
  for (_, rap) in self.raps.clone().iter() {
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
    for (_name, data) in self.borrow_map.iter_mut() {
      for m_data in mir_b_data.iter() {
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
    for (_, refs) in lender_to_refs.iter() {
      let regions = get_regions(refs, &self.borrow_map);
      for region in regions.iter() {
        let mut max_lifetime: usize = 0;
        for r in region {
          let lifetime = self.borrow_map.get(r).unwrap().lifetime;
          if lifetime > max_lifetime {
            max_lifetime = lifetime;
          }
        }
        for r in region {
          if self.borrow_map.get(r).unwrap().lifetime == max_lifetime {
            ultimate_refs.insert(r.to_owned());
          }
        }
      }
    }

    // if a reference borrowed from an anonymous owner then it must be an ultimate ref
    for (name, ref_data) in self.borrow_map.iter() {
      match ref_data.lender {
        ResourceTy::Anonymous => {
          ultimate_refs.insert(name.to_owned());
        }
        _ => {}
      }
    }
    let b_map = self.borrow_map.clone();

    // sort by number of aliases so events are ordered in a proper cascading fashion
    let mut vec: Vec<(String, RefData)> = b_map.into_iter().collect();
    vec.sort_by(|a, b| b.1.aliasing.len().cmp(&a.1.aliasing.len()));

    // Add events
    for (k, RefData {lender: r_ty, assigned_at: _, lifetime, ref_mutability: ref_mut, aliasing: _}) in &vec {
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
        match ref_mut {
          true => {
            self.add_ev(*lifetime, Evt::MDie, to_ro, ResourceTy::Value(from_ro), false);
          }
          false => {
            self.add_ev(*lifetime, Evt::SDie, to_ro, ResourceTy::Value(from_ro), false);
          }
        }
      }
    }
  }
}