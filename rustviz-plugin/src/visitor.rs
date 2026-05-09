//! Most of the work happens here, if you want to learn more about the visitor then look here:  
//! https://doc.rust-lang.org/beta/nightly-rustc/rustc_hir/intravisit/trait.Visitor.html
//! Essentially we recursively traverse (walk) the HIR, visiting statements, expressions, etc
//! See ExprKind at : https://doc.rust-lang.org/stable/nightly-rustc/rustc_hir/hir/enum.ExprKind.html
//! See StmtKind at : https://doc.rust-lang.org/stable/nightly-rustc/rustc_hir/hir/enum.StmtKind.html
//! See tcx: https://doc.rust-lang.org/nightly/nightly-rustc/rustc_middle/ty/struct.TyCtxt.html

use log::{info, warn};
use rustc_hir::{StmtKind, Stmt, Expr, ExprKind, UnOp, Param, QPath, Pat, PatKind, BindingMode, HirId, Mutability, LetStmt, def::*};
use rustc_hir::intravisit::{self, Visitor};
use crate::expr_visitor::*;
use crate::expr_visitor_utils::*;
use crate::svg_generator::data::*;
use rustc_middle::ty::adjustment::*;
use std::collections::{HashSet, VecDeque};


impl<'a, 'tcx> ExprVisitor<'a, 'tcx> {
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
    match expr.kind {
      ExprKind::Block(b, _) => {
        for s in b.stmts {
          match s.kind {
            StmtKind::Let(l) => {
              if let Some(init) = l.init {
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
        self.descend_through_expansion(recv);
        for a in args {
          self.descend_through_expansion(a);
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
          Some(e) => {
            if self.fn_ret { // only append this event if parent fn ctxt doesn't return void
              // currently this logic would not be able to handle functions with multiple return points
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
                Some(r) => ResourceTy::Value(r), // todo, technically need to check for deref here
                None => ResourceTy::Anonymous
              };
              
              let line_num = expr_to_line(e, &self.tcx);
              self.add_ev(line_num, evt, to_ro, from_ro, false);
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
    // add RAP corresponding to parameter type
    let line_num=span_to_line(&param.span, &self.tcx);
    let ty = self.tcx.typeck(param.hir_id.owner).pat_ty(param.pat);
    let is_special = ty_is_special_owner(&ty);
    match param.pat.kind {
      PatKind::Binding(binding_annotation, _ann_hirid, ident, _op_pat) =>{
        let name: String = ident.to_string();
        if ty.is_ref() {
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
          self.add_ref(name.clone(),
          bool_of_mut(ty.ref_mutability().unwrap()),
          bool_of_mut(binding_annotation.1), line_num,
          ResourceTy::Caller, VecDeque::new(), self.current_scope, !self.inside_branch);
          // Stretch the loan to the closing brace so the dashed
          // ref-line trapezoid covers the full body.
          if let Some(rd) = self.borrow_map.get_mut(&name) {
            rd.lifetime = self.current_scope;
          }
        }
        else if ty.is_adt() && !is_special{ // kind of weird given we don't have a InitStructParam
          let owner_hash = self.rap_hashes as u64;
          let parent_is_copy = self.ty_is_copy(ty, param.hir_id.owner);
          self.add_struct(name.clone(), owner_hash, false, bool_of_mut(binding_annotation.1), parent_is_copy, self.current_scope, !self.inside_branch);
          let generic_args = match ty.kind() {
            rustc_middle::ty::TyKind::Adt(_, args) => *args,
            _ => unreachable!("ty.is_adt() but kind is not Adt"),
          };
          for field in ty.ty_adt_def().unwrap().all_fields() {
            let field_name = format!("{}.{}", name.clone(), field.name.as_str());
            let field_ty = field.ty(self.tcx, generic_args);
            let field_is_copy = self.ty_is_copy(field_ty, param.hir_id.owner);
            self.add_struct(field_name, owner_hash, true, bool_of_mut(binding_annotation.1), field_is_copy, self.current_scope, !self.inside_branch);
          }
        }
        else {
          let is_copy = self.ty_is_copy(ty, param.hir_id.owner);
          self.add_owner(name.clone(), bool_of_mut(binding_annotation.1), is_copy, self.current_scope, !self.inside_branch);
        }
        self.add_external_event(line_num, ExternalEvent::InitRefParam { param: self.raps.get(&name).unwrap().rap.to_owned(), id: *self.unique_id });
        *self.unique_id += 1;
        self.annotate_src(name.clone(), ident.span, false, *self.raps.get(&name).unwrap().rap.hash());
      }
      _ => {}
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
          }
        }
      }
      
      // <expr>.<function>([args])
      ExprKind::MethodCall(name_and_generic_args, rcvr, args, _) => {
        let line_num = expr_to_line(&rcvr, &self.tcx);
        let fn_name = name_and_generic_args.ident.as_str().to_owned();
        self.add_fn(fn_name.clone());
        // need to recurse down to the variable calling the methods 
        // necessary for chained method calls scenarios: ie a.get().unwrap()
        self.visit_expr(rcvr);
        match rcvr.kind {
          ExprKind::MethodCall(p_seg, ..) => { // return early if not at the base
            let _rcvr_name = p_seg.ident.as_str().to_owned();
            return;
          }
          _ => {}
        }
        // The receiver of `r.s.method()` is a Field expression, not
        // a bare path, so `hirid_to_var_name` can't name it.
        // `expr_to_rap_name` walks Path / Field / AddrOf chains and
        // returns the qualified name we registered the receiver
        // under (e.g. `r.s`). If we can't resolve a RAP for the
        // receiver — unregistered nested field, `self.field` in an
        // impl method, etc. — bail out of the rest of the
        // method-call processing rather than crash; the call site
        // simply produces no event for this method.
        let rcvr_name = match expr_to_rap_name(rcvr, &self.tcx) {
          Some(n) => n,
          None => return,
        };
        let rcvr_rap = match self.raps.get(&rcvr_name) {
          Some(rd) => rd.rap.to_owned(),
          None => return,
        };
        self.update_rap(&rcvr_rap, line_num);
        let fn_rap = self.raps.get(&fn_name).unwrap().rap.to_owned();
        // typecheck
        // Annotate passByRef event, can check the type of borrow by looking at adjusments (usually borrows that are not explicit)
        // for example, in rust you don't have to dereference a reference to access members
        // https://doc.rust-lang.org/nightly/nightly-rustc/rustc_middle/ty/adjustment/struct.Adjustment.html
        let adjustment_map = self.tcx.typeck(name_and_generic_args.hir_id.owner).adjustments();
        match adjustment_map.get(rcvr.hir_id) {
          Some(adj_vec) => { 
            for a in adj_vec.iter() {
              match a.kind {
                Adjust::Borrow(AutoBorrow::Ref(m)) => {
                  match m {
                    AutoBorrowMutability::Mut{allow_two_phase_borrow: AllowTwoPhase::Yes} => {
                      self.add_ev(line_num, Evt::PassByMRef, ResourceTy::Value(fn_rap.clone()), ResourceTy::Value(rcvr_rap.clone()), false);
                    },
                    AutoBorrowMutability::Not => {
                      self.add_ev(line_num, Evt::PassBySRef, ResourceTy::Value(fn_rap.clone()), ResourceTy::Value(rcvr_rap.clone()), false);
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

        for arg in args.iter(){
          self.visit_expr(&arg);
          self.match_arg(&arg, fn_name.clone());
        }
      }
      ExprKind::Binary(_, expra, exprb) => {
        self.visit_expr(expra);
        self.visit_expr(exprb);
      }

      ExprKind::AddrOf(_, _, exp) => {
        self.visit_expr(exp);
      }

      // assignment 
      // ex a = <expr> or a += <expr>
      ExprKind::Assign(lhs_expr, rhs_expr, _,) | ExprKind::AssignOp(_, lhs_expr, rhs_expr) => {
        self.visit_expr(lhs_expr);
        self.visit_expr(rhs_expr);

        // typecheck to figure out what type of event is going to occur
        let line_num = expr_to_line(&lhs_expr, &self.tcx);
        let lhs_rty = self.resource_of_lhs(lhs_expr);
        let lhs_rap = self.raps.get(&lhs_rty.real_name()).unwrap().rap.clone();
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
        // if we are pointing at a new piece of data
        if lhs_ty.is_ref() {
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
              let borrowers = get_borrowers(&lhs_rty.real_name(), &self.borrow_map);
              if borrowers.len() > 1 { // there is another active reference at this point, the resource cannot be returned
                self.add_external_event(line_num, ExternalEvent::RefDie { from: lhs_rty.clone(), to: to_ro, num_curr_borrowers: borrowers.len() - 1, id: *self.unique_id });
                *self.unique_id += 1;
              }
              else {
                match ref_data.ref_mutability {
                  true => self.add_ev(line_num, Evt::MDie, to_ro, lhs_rty.clone(), false),
                  false => self.add_ev(line_num, Evt::SDie, to_ro, lhs_rty.clone(), false)
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

              // update lhs_rty with new lender information
              let (new_lender, new_aliasing) = self.get_ref_data(&rhs_expr);
              let r = self.borrow_map.get_mut(&lhs_rty.real_name()).unwrap();
              r.assigned_at = line_num;
              r.aliasing = new_aliasing;
              r.lender = new_lender;
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
                match modified_ref_data.ref_mutability {
                  true => self.add_ev(line_num, Evt::MDie, to_ro, lhs_rty.clone(), false),
                  false => self.add_ev(line_num, Evt::SDie, to_ro, lhs_rty.clone(), false)
                }
              }

              // update modified (derefed) reference's lender and aliasing data
              let (new_lender, new_aliasing) = self.get_ref_data(&rhs_expr);
              let r = self.borrow_map.get_mut(modified_ref_name).unwrap();
              r.assigned_at = line_num;
              r.aliasing = new_aliasing.clone();
              r.lender = new_lender;

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
            _ => panic!("not possible")
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
        let drop_target: Option<ResourceAccessPoint> =
          if !lhs_ty.is_ref() && !is_copyable {
            match &lhs_rty {
              ResourceTy::Value(rap) => {
                // Plain reassign — only emit the drop if y currently
                // holds a resource (rules out `let y; y = x;` first
                // assignment, and `let y = a; let z = y; y = x;` where
                // y was moved out before reassignment).
                if self.rap_holds_resource_now(rap.name()) {
                  Some(rap.clone())
                } else {
                  None
                }
              }
              ResourceTy::Deref(p_rap) => {
                // Deref reassign through a &mut — the lender is what
                // holds the resource being overwritten. Borrow checker
                // guarantees the lender currently holds (otherwise *p
                // wouldn't be a valid place to write to), so we don't
                // need rap_holds_resource_now here.
                self.borrow_map.get(p_rap.name())
                  .and_then(|rd| rd.lender.extract_rap().cloned())
              }
              _ => None,
            }
          } else {
            None
          };

        self.match_rhs(lhs_rty.clone(), rhs_expr, e);

        if let Some(rap) = drop_target {
          self.add_external_event(line_num, ExternalEvent::OwnerDropAtReassign {
            ro: rap,
            id: *self.unique_id,
          });
          *self.unique_id += 1;
        }
      }

      // a block eg: {}
      ExprKind::Block(block, _) => {
        // this scoping logic isn't necessary except for when defining functions inside of functions
        let prev_scope = self.current_scope;
        let new_scope = self.tcx.sess.source_map().lookup_char_pos(expr.span.hi()).line;
        self.current_scope = new_scope;
        self.visit_block(block); // visit all the statements in the block
          self.current_scope = prev_scope;
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
          _ => ()
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
        let r = &self.raps.get(&name).unwrap().rap.clone();
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
        let iflet_bindings: HashSet<ResourceAccessPoint> =
          if let ExprKind::Let(let_expr) = guard_expr.kind {
            self.visit_expr(let_expr.init);
            self.register_iflet_let_bindings(let_expr, None)
          } else {
            self.visit_expr(&guard_expr);
            HashSet::new()
          };
        self.inside_branch = true; // need this flag to correctly handle variables that are declared inside blocks
        self.visit_expr(&if_expr);
        let else_decl = match else_expr {
          Some(e) => {
            self.visit_expr(e);
            get_decl_of_expr(e, &self.tcx, &self.raps)
          }
          None => HashSet::new(),
        };
        self.inside_branch = false;

        // compute split and merge points
        let line_num = expr_to_line(&guard_expr, &self.tcx);
        let split = self.tcx.sess.source_map().lookup_char_pos(if_expr.span.lo()).line;
        let mut if_end = self.tcx.sess.source_map().lookup_char_pos(if_expr.span.hi()).line;
        let merge = match else_expr {
          Some(e) => self.tcx.sess.source_map().lookup_char_pos(e.span.hi()).line,
          None => self.tcx.sess.source_map().lookup_char_pos(if_expr.span.hi()).line
        };

        // Bindings declared by an `if let` guard belong in the if
        // branch's decl set (they live for the body block, same as a
        // top-level `let` inside it).
        let mut if_decl = get_decl_of_expr(if_expr, &self.tcx, &self.raps);
        if_decl.extend(iflet_bindings.iter().cloned());

        // Filter events that landed inside each branch's body. The
        // ranges deliberately exclude the brace lines: `split` is the
        // line of the if-body's opening `{` (which in normal Rust
        // formatting is the same line as the guard), and the guard's
        // events shouldn't be folded into the if-branch — the guard
        // runs once before either branch, not as part of one. Same
        // logic for the else side: the `} else {` line carries the
        // closing of the if-body and the opening of the else-body,
        // and any events on it are structural noise from either side.
        // Single-line ifs (split == merge) collapse the ranges to
        // empty, intentionally — line-based filtering can't separate
        // body from guard there. The dropdown fix in #117 keeps the
        // canonical examples multi-line for now.
        let _ = line_num; // no longer used for guard-live extension
        // For plain `if cond { … }` we exclude the split line (the
        // line of the body's opening `{`, normally the same line as
        // the guard) so guard reads stay on the global timeline. For
        // `if let pat = expr { … }` the destructure events emitted by
        // `register_iflet_let_bindings` *are* on the split line and
        // belong inside the if-branch — include split when an if-let
        // is in play.
        let if_filter_lo = if iflet_bindings.is_empty() { split + 1 } else { split };
        let if_filter_hi = if_end;
        let else_filter_lo = if_end + 1;
        let else_filter_hi = merge;
        let mut if_ev: Vec<(usize, ExternalEvent)> =
          self.preprocessed_events.iter()
            .filter(|i| filter_ev(i, if_filter_lo, if_filter_hi))
            .cloned().collect();
        let mut else_ev: Vec<(usize, ExternalEvent)> =
          self.preprocessed_events.iter()
            .filter(|i| filter_ev(i, else_filter_lo, else_filter_hi))
            .cloned().collect();
        // Align the global-event purge with the per-branch filters
        // above. Removing the entire `[split, merge]` slab (the old
        // behaviour) drops events that *didn't* make it into either
        // branch — chiefly the let-as-rhs Moves emitted on the split
        // line for single-line ifs, and a plain `if cond { … }`'s
        // guard reads (also on the split line). Those events still
        // belong on the global timeline; only the events the
        // branches claimed should be evicted from there.
        self.preprocessed_events.retain(|(l, _)| {
          let in_if = *l >= if_filter_lo && *l <= if_filter_hi;
          let in_else = *l >= else_filter_lo && *l <= else_filter_hi;
          !(in_if || in_else)
        });

        // Compute liveness from the events actually inside the
        // branches — only variables touched by a real event get a
        // branched timeline. Avoids the previous behaviour where
        // `get_live_of_expr(guard_expr)` always promoted guard
        // variables (e.g. the `n` in `if n > 0 { … }`) into branched
        // columns even when neither branch references them.
        let mut liveness: HashSet<ResourceAccessPoint> = HashSet::new();
        collect_event_live_vars(&if_ev, &mut liveness);
        collect_event_live_vars(&else_ev, &mut liveness);
        // Variables declared inside a branch (let-stmts within the
        // body, plus if-let pattern bindings) live only for that
        // branch — they're not pre-existing variables that need a
        // branched timeline. Subtract them out.
        let declared_in_branches: HashSet<ResourceAccessPoint> =
          if_decl.union(&else_decl).cloned().collect();
        liveness = liveness.difference(&declared_in_branches).cloned().collect();

        // add gos events for variables declared in each block
        for var in if_decl.iter() {
          if_ev.push((if_end, ExternalEvent::GoOutOfScope { ro: var.clone(), id: *self.unique_id }));
          *self.unique_id += 1;
        }

        for var in else_decl.iter() {
          else_ev.push((merge, ExternalEvent::GoOutOfScope { ro: var.clone(), id: *self.unique_id }));
          *self.unique_id += 1;
        }

        
        let if_map = create_line_map(&if_ev);

        // Per-branch "active" line ranges. Each branch is rendered
        // solid inside its active range and dashed outside. We use
        // body content ranges (not body spans) so the if-branch's
        // active range stops at its `}` line — at the line where
        // the else-branch's body starts, the if-branch transitions
        // to dashed and the else-branch becomes active. Ranges
        // can't overlap, otherwise both would draw solid on the
        // shared row.
        //
        // `merge_point - 1` is the last line each branch column is
        // drawn on (the convergence diagonal then bridges that row
        // to the join dot at `merge_point` — see render_timeline's
        // Branch arm). Clamping the else-branch's end there keeps
        // it consistent with the column-trim done in
        // compute_branch_states.
        let merge_minus_one = merge.saturating_sub(1);
        let (b_labels, b_slices, b_branches) = match else_expr {
          Some(_) => {
            let else_map = create_line_map(&else_ev);
            (
              vec!["If".to_owned(), "Else".to_owned()],
              vec![(split + 1, if_end), (if_end + 1, merge_minus_one)],
              vec![
                ExtBranchData { e_data: if_ev,   line_map: if_map,   decl_vars: if_decl   },
                ExtBranchData { e_data: else_ev, line_map: else_map, decl_vars: else_decl },
              ],
            )
          }
          None => (
            vec!["If".to_owned()],
            vec![(split + 1, if_end)],
            vec![ExtBranchData { e_data: if_ev, line_map: if_map, decl_vars: if_decl }],
          ),
        };
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
                // Pass body_end so the helper emits a GoOutOfScope at
                // the body's closing brace — there's no surrounding
                // Branch event to clean up after the binding (loop
                // bodies are visited inline into the global timeline).
                if let ExprKind::Let(let_expr) = cond.kind {
                  self.visit_expr(let_expr.init);
                  let body_end = self.tcx.sess.source_map()
                    .lookup_char_pos(then_body.span.hi()).line;
                  self.register_iflet_let_bindings(let_expr, Some(body_end));
                } else {
                  self.visit_expr(cond);
                }
                self.inside_branch = true;
                self.visit_expr(then_body);
                self.inside_branch = prev;
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
            self.inside_branch = true;
            self.visit_block(block);
            self.inside_branch = prev;
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
              res.push(get_rap(&field, &self.tcx, &self.raps));
              res_ty.push(typeck_res.node_type(field.hir_id));
            }
            (res, res_ty)
          }
          _ => {
            (vec![get_rap(&guard_expr, &self.tcx, &self.raps)], vec![typeck_res.node_type(guard_expr.hir_id)])
          }
        };
        let typeck_res = self.tcx.typeck(guard_expr.hir_id.owner);

        let split = self.tcx.sess.source_map().lookup_char_pos(guard_expr.span.hi()).line; // TODO: might need to alter this
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
            for arm in arms.iter() {
              let mut branch_e_data: Vec<(usize, ExternalEvent)> = Vec::new();
              let mut callback_events: Vec<(ResourceTy, ResourceTy, Evt)> = Vec::new();

              // get line info
              let begin = self.tcx.sess.source_map().lookup_char_pos(arm.body.span.lo()).line;
              let end = self.tcx.sess.source_map().lookup_char_pos(arm.body.span.hi()).line;
              let pat_line = span_to_line(&arm.pat.span, &self.tcx);
              b_slices.push((begin, end));
              b_ty_names.push(get_name_of_pat(arm.pat, &self.tcx));

              
              // add/fetch raps that are initialized in arm expr
              // need to also get their types, in order to figure out if we are moving, copying or borrowing something into the block
              let mut pat_decls: HashSet<ResourceAccessPoint> = HashSet::new();
              // println!("arm pat {:#?}", arm.pat);
              match arm.pat.kind {
                // (<pat>, <pat>, ..) => <expr>
                // First need to annotate events that occur between parents (the variables being matched upon)
                // and the pattern bindings in each arm
                PatKind::TupleStruct(_, pat_list, _) | PatKind::Tuple(pat_list, _)=> {
                  for (i, p) in pat_list.iter().enumerate() {
                    let mut associated_ro = Vec::new();
                    self.get_dec_of_pat2(p, &typeck_res, &parents[i], &parents_ty[i], end, & mut associated_ro);
                    let temp: Vec<ResourceAccessPoint> = associated_ro.iter().map(|(r, _, _)| {r.clone()}).collect();
                    let temp2: HashSet<ResourceAccessPoint> = temp.into_iter().collect();
                    pat_decls.extend(temp2);
                    for (to_ro, e, parent_ty) in associated_ro.iter() {
                      // If the type of the pat is not the same as the parent associated with it then it must be a partial move
                      let is_partial = !(*parent_ty == typeck_res.node_type(p.hir_id));
                      branch_e_data.push((pat_line, self.ext_ev_of_evt(e.clone(), ResourceTy::Value(to_ro.clone()), parents[i].clone(), *self.unique_id, is_partial)));
                      *self.unique_id += 1;
                      match e {
                        Evt::SBorrow => {
                          callback_events.push((parents[i].clone(), ResourceTy::Value(to_ro.clone()), Evt::SDie));
                        }
                        Evt::MBorrow => {
                          callback_events.push((parents[i].clone(), ResourceTy::Value(to_ro.clone()), Evt::MDie));
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
                    self.get_dec_of_pat2(arm.pat, &typeck_res, &parents[i], &parents_ty[i], end, & mut associated_ro);
                    let temp: Vec<ResourceAccessPoint> = associated_ro.iter().map(|(r, _, _)| {r.clone()}).collect();
                    let temp2: HashSet<ResourceAccessPoint> = temp.into_iter().collect();
                    pat_decls.extend(temp2);
                    for (to_ro, e, _) in associated_ro.iter() {
                      branch_e_data.push((pat_line, self.ext_ev_of_evt(e.clone(), ResourceTy::Value(to_ro.clone()),parents[i].clone(), *self.unique_id, false)));
                      *self.unique_id += 1;
                      match e {               // A borrow, mut/immut must be returned at the end of the block
                        Evt::SBorrow => {
                          callback_events.push((parents[i].clone(), ResourceTy::Value(to_ro.clone()), Evt::SDie));
                        }
                        Evt::MBorrow => {
                          callback_events.push((parents[i].clone(), ResourceTy::Value(to_ro.clone()), Evt::MDie));
                        }
                        _ => {}
                      }
                    }
                  }
                }
              }

              // visit expr
              self.inside_branch = true;
              self.visit_expr(arm.body);
              self.inside_branch = false;

              all_pat_decls.extend(pat_decls.iter().cloned());
              let arm_decls: HashSet<ResourceAccessPoint> = pat_decls.union(&get_decl_of_expr(arm.body, &self.tcx, &self.raps)).cloned().collect();


              // get events that occured in the arm
              branch_e_data.extend(
                self.preprocessed_events.iter().filter(|i| filter_ev(i, begin, end)).cloned()
              );

              // contribute events-touched variables to liveness; the
              // arm's own pat-decls are subtracted at the end.
              collect_event_live_vars(&branch_e_data, &mut liveness);

              // remove elements from global container
              self.preprocessed_events.retain(|(l, _)| 
                if *l <= end && *l >= begin {
                  false
                }
                else {
                  true
                }
              );

              // add callback events - events that need to happen at the end of an arm block
              // ex: if a pattern binding borrows from a parent
              for (to, from, e) in callback_events {
                let name = from.real_name();
                self.borrow_map.remove(&name);
                branch_e_data.push((end, self.ext_ev_of_evt(e, to, from, *self.unique_id, false)));
                *self.unique_id += 1;
              }

              // add gos events
              for r in arm_decls.iter() {
                branch_e_data.push((end, ExternalEvent::GoOutOfScope { ro: r.clone(), id: *self.unique_id }));
                *self.unique_id += 1;
              }

              // branch_e_data.push((end, self.ext_ev_of_evt(parent_ev.clone(), ResourceTy::Anonymous, parent.clone())));
              let branch_line_map = create_line_map(&branch_e_data);

              branch_data.push(ExtBranchData { e_data: branch_e_data, line_map: branch_line_map, decl_vars: arm_decls});
            }
            // Variables introduced by an arm's pattern only exist
            // inside that arm — strip them out so they don't get a
            // branched timeline of their own.
            liveness = liveness.difference(&all_pat_decls).cloned().collect();
            // add branch event
            self.add_external_event(split, ExternalEvent::Branch {
              live_vars: liveness,
              branches: branch_data,
              branch_type: BranchType::Match(b_ty_names, b_slices),
              split_point: split,
              merge_point: merge - 1, // TODO: fix
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
                //   anything else (Range, call result, …) ⇒ skip
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
                  if pat_ty.is_ref() {
                    self.add_ref(
                      pat_name.clone(),
                      bool_of_mut(pat_ty.ref_mutability().unwrap()),
                      false,
                      for_line,
                      head_rty.clone(),
                      std::collections::VecDeque::new(),
                      self.current_scope,
                      false,
                    );
                  } else {
                    let is_copy = self.tcx.type_is_copy_modulo_regions(
                      rustc_middle::ty::TypingEnv::post_analysis(self.tcx, user_pat.hir_id.owner),
                      pat_ty);
                    self.add_owner(pat_name.clone(), false, is_copy, self.current_scope, false);
                  }
                  self.annotate_src(pat_name.clone(), pat_ident.span, false,
                    *self.raps.get(&pat_name).unwrap().rap.hash());

                  // Emit the iterand event into the user's pattern binding.
                  if let Some(e) = iterand_evt {
                    let to_rty = ResourceTy::Value(self.raps.get(&pat_name).unwrap().rap.to_owned());
                    self.add_ev(for_line, e, to_rty, head_rty, false);
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
                self.visit_expr(body_expr);
                self.inside_branch = prev_inside_branch;
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
      return;
    }
    let local_line = self.tcx.sess.source_map().lookup_char_pos(local.span.lo()).line;
    let is_skipped = self.skip_lines.contains(&local_line);
    if let Some(init) = local.init {
      self.process_let_binding(local.pat, init, is_skipped);
    }
    // `let x;` (no init) — nothing to bind from. The variable still
    // gets its column on its first assignment downstream, same as
    // pre-fix behaviour.

    if let Some(els) = local.els {
      self.visit_block(els);
    }
  }
}

impl<'a, 'tcx> ExprVisitor<'a, 'tcx> {
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
    // event to fold the bind into.
    let init_is_branching = !is_skipped
      && matches!(init.kind, ExprKind::If(..) | ExprKind::Match(..));
    if init_is_branching {
      self.bind_pat(pat, init, is_skipped);
      self.visit_expr(init);
    } else {
      if !is_skipped {
        self.visit_expr(init);
      }
      self.bind_pat(pat, init, is_skipped);
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
      // `let &x = &y;` — recurse through the borrow on both sides.
      (PatKind::Ref(inner_pat, _), ExprKind::AddrOf(_, _, inner_init)) => {
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
      PatKind::Ref(inner, _)
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
          self.add_ref(
            name.clone(),
            bool_of_mut(pat_ty.ref_mutability().unwrap()),
            false,
            line,
            ResourceTy::Anonymous,
            std::collections::VecDeque::new(),
            self.current_scope,
            false,
          );
        } else {
          let is_copy = self.tcx.type_is_copy_modulo_regions(
            rustc_middle::ty::TypingEnv::post_analysis(self.tcx, hirid.owner),
            pat_ty,
          );
          self.add_owner(name.clone(), false, is_copy, self.current_scope, false);
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
      PatKind::Ref(inner, _)
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
    }
  }
}