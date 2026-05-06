
use std::{cmp::max, collections::{BTreeMap, HashMap, HashSet, VecDeque}, ops::Bound};
use log::warn;
use rustc_hir::Mutability;
use crate::svg_generator::data::{ExternalEvent, ResourceAccessPoint, ResourceTy};
use rustc_middle::ty::{Ty, TyCtxt};
use rustc_span::Span;
use rustc_hir::{Expr, ExprKind, Path, def::Res, Pat, PatKind, QPath, HirId, StmtKind, Stmt, Block, UnOp};

use crate::expr_visitor::{RapData, RefData};

// outdated function don't use this to find a name of a variable/hir_id
pub fn extract_var_name(input_string: &str ) -> Option<String> {
  let start_index = input_string.find('`')? + 1;
  let end_index = input_string.rfind('`')?;
  let rough_string=input_string[start_index..end_index].to_owned();
  if rough_string.contains("String::from"){
    Some(String::from("String::from"))
  }
  else{
    Some(rough_string)
  }
}

// Best-effort name extraction from a HIR id. Originally implemented by
// pretty-printing the node and grepping for backticked tokens; that pretty
// printer (`Map::node_to_string`) was removed in rustc 1.85, so we now walk
// the node directly.
pub fn hirid_to_var_name(id: HirId, tcx: &TyCtxt) -> Option<String> {
  fn name_of_path(p: &Path<'_>) -> String {
    let segs: Vec<String> = p.segments.iter().map(|s| s.ident.as_str().to_owned()).collect();
    segs.join("::")
  }
  // For `T::method`, recover the qself's type name so the returned string
  // covers the same source range that the path's span does (e.g. "String::from"
  // rather than just "from").
  fn name_of_ty(ty: &rustc_hir::Ty<'_>) -> Option<String> {
    match ty.kind {
      rustc_hir::TyKind::Path(QPath::Resolved(_, path)) => Some(name_of_path(path)),
      rustc_hir::TyKind::Path(QPath::TypeRelative(_, segment)) => Some(segment.ident.as_str().to_owned()),
      _ => None,
    }
  }
  let raw = match tcx.hir_node(id) {
    rustc_hir::Node::Expr(e) => match e.kind {
      ExprKind::Path(QPath::Resolved(_, path)) => name_of_path(path),
      ExprKind::Path(QPath::TypeRelative(qself, segment)) => {
        match name_of_ty(qself) {
          Some(qname) => format!("{}::{}", qname, segment.ident.as_str()),
          None => segment.ident.as_str().to_owned(),
        }
      }
      _ => return None,
    },
    n => n.ident().map(|i| i.as_str().to_owned())?,
  };
  // Preserve the historical normalization: any path containing String::from is
  // collapsed to the bare String::from so callers don't see e.g. <String as From<&str>>::from.
  if raw.contains("String::from") {
    Some(String::from("String::from"))
  } else {
    Some(raw)
  }
}

pub fn bool_of_mut (m: Mutability) -> bool {
  match m {
    Mutability::Not => {
      false
    }
    _ => { true }
  }
}


pub fn span_to_line(span:&Span, tcx: &TyCtxt) -> usize{
  tcx.sess.source_map().lookup_char_pos(span.lo()).line
}

pub fn expr_to_line(expr:&Expr, tcx: &TyCtxt) -> usize{
  span_to_line(&expr.span, tcx)
}

pub fn is_addr(expr: &Expr) -> bool { // todo, probably a better way to do this using the typechecker
  match expr.kind {
    ExprKind::AddrOf(..) => true,
    _ => false
  }
}

// The purpose of this function is to override the typechecking done on variables
// when we want to consider them owners rather than structs. This is because for 99.9% of cases
// users don't care about acessing String::len/buf members. 
// We should work towards elimanting all struct members that are not used in the program
// and that way we can just get rid of this hacky function.
pub fn ty_is_special_owner<'tcx> (tcx: &TyCtxt<'tcx>, t: &Ty<'tcx>) -> bool {
  match &*t.sort_string(*tcx) {
    "`std::string::String`" => { true }
    _ => {
      false
    }
  }
}

// Get a string representation of a Path - usually used as a name for a RAP
pub fn string_of_path(p: &Path, tcx: &TyCtxt) -> String {
  match p.res {
    Res::Def(rustc_hir::def::DefKind::Ctor(_, _), _) => {
      let mut name = String::new();
      for (i, segment) in p.segments.iter().enumerate() {
        name.push_str(tcx.hir_name(segment.hir_id).as_str());
        if i < p.segments.len() - 1 {
          name.push_str("::");
        }
      }
      name
    }
    _ => { // technically this is incomplete - there are more cases to cover
    
      tcx.hir_name(p.segments[0].hir_id).as_str().to_owned()
    }
  }
}

// Get the name of a pattern
// used for getting names of arms of Match expr
pub fn get_name_of_pat(pat: &Pat, tcx: &TyCtxt) -> String {
  match pat.kind {
    PatKind::Binding(_, _, ident, _) => ident.to_string(),
    PatKind::TupleStruct(QPath::Resolved(_, p), _, _) => {
      string_of_path(&p, tcx)
    }
    PatKind::Expr(rustc_hir::PatExpr { kind: rustc_hir::PatExprKind::Path(QPath::Resolved(_, p)), .. }) => {
      string_of_path(&p, tcx)
    }
    // Literal-pattern arms like `true =>` / `0 =>` show up in
    // macro-expanded code we have no source-text view of (chiefly
    // `assert!(cond)`, which expands to a match with `true` and `_`
    // arms). Render the literal's Debug form as the arm label rather
    // than panicking; the value is used purely as a label, not for
    // semantic analysis, so a "Bool(true)" / "Int(0, …)" label is
    // sufficient. Avoids needing to import rustc_ast for a typed
    // match on LitKind.
    PatKind::Expr(rustc_hir::PatExpr { kind: rustc_hir::PatExprKind::Lit { lit, .. }, .. }) => {
      format!("{:?}", lit.node)
    }
    PatKind::Wild => {
      String::from("Wild")
    }
    PatKind::Tuple(pat_list, _) => {
      let mut res = String::from("(");
      for (i, p) in pat_list.iter().enumerate() {
        res.push_str(&get_name_of_pat(p, tcx));
        if i < pat_list.len() - 1{
          res.push_str(", ");
        }
        else {
          res.push(')');
        }
      }
      res
    }
    _ => panic!("unexpected pat kind {:#?}", pat)
  }
}

pub fn num_derefs(expr: &Expr) -> usize{
  match expr.kind {
    ExprKind::Unary(UnOp::Deref, exp) => {
      1 + num_derefs(exp)
    }
    _ => 0
  }
}



// LIVENESS + DECLARATION FUNCTIONS
// Functions used for getting variables that are live inside of blocks
// As well as variables that are declared inside of blocks

pub fn get_decl_of_block(block: &Block, tcx: &TyCtxt, raps: &HashMap<String, RapData>) -> HashSet<ResourceAccessPoint>{
  let mut res:HashSet<ResourceAccessPoint> = HashSet::new();
  for stmt in block.stmts.iter() {
    res = res.union(&get_decl_of_stmt(&stmt, tcx, raps)).cloned().collect();
  }
  res
}
// we only care about fetching the declarations in the current block, which is why these functions are not mutually recursive
pub fn get_decl_of_expr(expr: &Expr, tcx: &TyCtxt, raps: &HashMap<String, RapData>) -> HashSet<ResourceAccessPoint> {
  match expr.kind {
    ExprKind::Block(b, _) => get_decl_of_block(b, tcx, raps),
    _ => HashSet::new() // maybe should handle match expressions as well? 
  }
}

pub fn get_decl_of_stmt(stmt: &Stmt, _tcx: &TyCtxt, raps: &HashMap<String, RapData>) -> HashSet<ResourceAccessPoint> {
  let mut res = HashSet::new();
  match stmt.kind {
    StmtKind::Let(ref local) => {
      let name = match local.pat.kind {
        PatKind::Binding(_, _, ident, _) => {
          ident.to_string()
        }
        _ => panic!("unexpected let binding pattern")
      };
      res.insert(raps.get(&name).unwrap().rap.to_owned());
    }
    _ => {}
  }
  res
}

pub fn get_live_of_stmt(stmt: &Stmt, tcx: &TyCtxt, raps: &HashMap<String, RapData>) -> HashSet<ResourceAccessPoint> {
  match stmt.kind {
    StmtKind::Let(ref local) => {
      match local.init {
        Some(expr) => {
          get_live_of_expr(&expr, tcx, raps)
        }
        None => HashSet::new()
      }
    }
    StmtKind::Item(_item) => panic!("not yet able to handle items inside of bodies"),
    StmtKind::Expr(ref expression) | StmtKind::Semi(ref expression) => {
        get_live_of_expr(expression, tcx, raps)
    }
  }
}

// Get all the live variables in the expression
// where live refers to the variables defined OUTSIDE of the expression
// and used inside it. This is distinct from those declared inside it
pub fn get_live_of_expr(expr: &Expr, tcx: &TyCtxt, raps: &HashMap<String, RapData>) -> HashSet<ResourceAccessPoint> {
  match expr.kind {
    ExprKind::Path(QPath::Resolved(_,p)) => {
      match p.res {
        Res::Def(rustc_hir::def::DefKind::Ctor(_, _), _) => {
          // function, so we don't care about it in regards to live vars
          HashSet::new()
        }
        _ => {
          let name = tcx.hir_name(p.segments[0].hir_id).as_str().to_owned();
          HashSet::from([raps.get(&name).unwrap().rap.to_owned()])
        }
      }
    }
    ExprKind::Field(expr, ident) => {
      match expr {
        Expr{kind: ExprKind::Path(QPath::Resolved(_,p)), ..} => {
          let name = tcx.hir_name(p.segments[0].hir_id).as_str().to_owned();
          let field_name: String = ident.as_str().to_owned();
          let total_name = format!("{}.{}", name, field_name);
          HashSet::from([raps.get(&total_name).unwrap().rap.to_owned()])
        }
        _ => { panic!("unexpected field expr") }
      } 
    }
    ExprKind::AddrOf(_, _, exp) | ExprKind::Unary(_, exp) 
    | ExprKind::DropTemps(exp) => {
      get_live_of_expr(exp, tcx, raps)
    }
    ExprKind::Binary(_, lhs_expr, rhs_expr) | ExprKind::Assign(lhs_expr, rhs_expr, _) | ExprKind::AssignOp(_, lhs_expr, rhs_expr) => {
      let lhs = get_live_of_expr(&lhs_expr, tcx, raps);
      let rhs = get_live_of_expr(&rhs_expr, tcx, raps);
      let res = lhs.union(&rhs).cloned().collect();
      res
    }
    ExprKind::Call(fn_expr, args) => {
      let mut res = HashSet::new();
      match fn_expr.kind {
        // Match onto println! macro
        ExprKind::Path(QPath::Resolved(_,rustc_hir::Path{res: rustc_hir::def::Res::Def(_, id), ..})) 
        if !id.is_local() => {
          match args {
            [Expr{kind: ExprKind::Call(_, a),..}] => {
              match a {
                [_, Expr{kind: ExprKind::AddrOf(_, _, 
                  Expr{kind: ExprKind::Array(x),..}),..}] => {
                    for exp in x.iter() {
                      match exp {
                        Expr{kind: ExprKind::Call(_, format_args), ..} => {
                          for a_expr in format_args.iter() {
                            res = res.union(&get_live_of_expr(&a_expr, tcx, raps)).cloned().collect();
                          }
                        }
                        _ => {
                          warn!("getting here to the println 1");
                        }
                      }
                    }
                  }
                _ => {
                  warn!("getting here to the println 2");
                }
              }
            }
            _ => {
              for a_expr in args.iter() {
                res = res.union(&get_live_of_expr(&a_expr, tcx, raps)).cloned().collect();
              }
            }
          }
        }
        _ => {
          for a_expr in args.iter() {
            res = res.union(&get_live_of_expr(&a_expr, tcx, raps)).cloned().collect();
          }
        }
      }
      res
    }
    ExprKind::MethodCall(_, rcvr, args, _) => {
      let rcvr_name = hirid_to_var_name(rcvr.hir_id, tcx).unwrap(); // TODO: will break for chained methodcalls
      let mut res = HashSet::from([raps.get(&rcvr_name).unwrap().rap.to_owned()]);
      for a_expr in args.iter() {
        res = res.union(&get_live_of_expr(&a_expr, tcx, raps)).cloned().collect();
      }
      res
    }
    // Branch expressions need to be handled a bit differently, 
    // We want the variables that are live in each block, but not the ones that were declared in 
    // the blocks (since their timelines should not be split)
    ExprKind::Block(b, _) => {
      let mut res: HashSet<ResourceAccessPoint> = HashSet::new();
      for stmt in b.stmts.iter() {
        res = res.union(&get_live_of_stmt(&stmt, tcx, raps)).cloned().collect();
      }
      match b.expr {
        Some(exp) => {
          res = res.union(&get_live_of_expr(exp, tcx, raps)).cloned().collect();
        }
        None => {}
      }
      res.difference(&get_decl_of_block(b, tcx, raps)).cloned().collect() // need to remove the variables that were declared in the current block 
    }
    ExprKind::If(guard_expr, if_expr, else_expr) => {
      let mut res: HashSet<ResourceAccessPoint> = HashSet::new();
      res = res.union(&get_live_of_expr(&guard_expr, tcx, raps)).cloned().collect();
      res = res.union(&get_live_of_expr(&if_expr, tcx, raps)).cloned().collect();
      match else_expr {
        Some(e) => {
          res = res.union(&get_live_of_expr(&e, tcx, raps)).cloned().collect();
        }
        None => {}
      }
      res
    }
    ExprKind::Tup(expr_list) => {
      let mut res: HashSet<ResourceAccessPoint> = HashSet::new();
      for e in expr_list.iter() {
        res = res.union(&get_live_of_expr(e, tcx, raps)).cloned().collect();
      }
      res
    }
    _ => {
      HashSet::new()
    }
  }
}

// Used for filtering events from the main event container
pub fn filter_ev((line_num, _ev): &(usize, ExternalEvent), split: usize, merge: usize) -> bool {
  if *line_num <= merge && *line_num >= split {
    true
  }
  else {
    false
  }
}

// Fetch the mutability of the borrow
pub fn fetch_mutability(expr: & Expr) -> Option<Mutability> {
  match expr.kind {
    ExprKind::Block(b, _) => {
      match b.expr {
        Some(expr) => { fetch_mutability(expr) }
        None => { panic!("invalid expr for fetching mutability") }
      }
    }
    ExprKind::AddrOf(_, mutability, expr) => {
      match fetch_mutability(&expr) {
        None => Some(mutability), 
        Some(m) => Some(m)
      }
    }
    _ => None
  }
}


// BORROWING LOGIC HELPERS

// Group loans for a certain lender into regions
// A region is defined as an area where multiple loans on the same local are active at the same time
pub fn get_regions(h: &HashSet<String>, borrow_map: &HashMap<String, RefData>) -> Vec<HashSet<String>> {
  let mut res: BTreeMap<usize, (usize, HashSet<String>)>  = BTreeMap::new();

  for borrower in h {
    let b_data = borrow_map.get(borrower).unwrap();
    let a_place = b_data.assigned_at;
    let k_place = b_data.lifetime;

    let mut c = res.upper_bound_mut(Bound::Included(&a_place));
    let mut to_replace: Option<(usize, (usize, HashSet<String>))> = None;
    match c.peek_prev() { // look to our left
      Some((_, (k, map))) => {
        if a_place < *k { // if current borrower was assigned in the same region
          *k = max(*k, k_place); // adjust region to encapsulate all lifetimes (extending the lifetime to the right)
          map.insert(borrower.clone());
        }
        else { // borrower belongs to a different region
          res.insert(a_place, (k_place, HashSet::from([borrower.clone()])));
        }
      }
      None => {
        match c.peek_next() { // look to our right
          Some((a, (k, map))) => {
            if *a < k_place { // need to do this because we can't mutate keys (would break BTreeMap invariants) 
            // although in our case it wouldn't matter because you still wouldn't be able to change the relative ordering of regions (try a proof by contradiction)
              map.insert(borrower.clone());
              to_replace = Some((*a, (max(*k, k_place), map.clone()))); // extending the lifetime to the left
            }
            else {
              res.insert(a_place, (k_place, HashSet::from([borrower.clone()])));
            }
          }
          None => {
            res.insert(a_place, (k_place, HashSet::from([borrower.clone()])));
          }
        }
      }
    }

    // if we need to replace a key
    match to_replace {
      Some((key, (k, map))) => {
        res.remove(&key);
        res.insert(a_place, (k, map));
      }
      None => {}
    }
  }

  let mut z: Vec<HashSet<String>> = Vec::new();
  for (_, (_, map)) in res.into_iter() {
    z.push(map);
  }
  z
}


// get the non anonymous lenders and their respective 'active' borrowers
pub fn get_non_anon_lenders(b_map: &HashMap<String, RefData>) -> HashMap<String, HashSet<String>> {
  let mut res: HashMap<String, HashSet<String>> = HashMap::new();
  for (k, v) in b_map.iter() {
    match v.lender {
      ResourceTy::Anonymous => {},
      _ => {
        let lender = v.lender.real_name();
        res.entry(lender.clone())
          .and_modify(|lendees| { lendees.insert(k.to_owned()); })
          .or_insert(HashSet::from([k.to_owned()]));
      }
    }
  }
  res
}

// get borrowers associated with a single lender
pub fn get_borrowers(borrower: &String, borrow_map: &HashMap<String, RefData>) -> HashSet<String> {
  let lender = borrow_map.get(borrower).unwrap().lender.clone();
  match lender {
    ResourceTy::Anonymous | ResourceTy::Caller => HashSet::from([borrower.to_owned()]),
    _ => {
      let mut res:HashSet<String> = HashSet::new();
      for (k, v) in borrow_map.iter() {
        if v.lender == lender {
          res.insert(k.to_string());
        }
      } 
      res
    }
  }
}

pub fn get_aliasing_data(r: &ResourceTy, borrow_map: &HashMap<String, RefData>) -> VecDeque<String> {
  match r {
    ResourceTy::Anonymous | ResourceTy::Caller => VecDeque::new(),
    ResourceTy::Value(x) | ResourceTy::Deref(x) => {
      match borrow_map.get(x.name()) {
        Some(r_data) => r_data.aliasing.to_owned(),
        None => VecDeque::new()
      }
    }
  }
}

// FETCHING RAP HELPERS
// Gonna be honest these functions are poorly named and are all very similar but subtly different

// Gets the RAP associated with an expr where we expect expr to resolve to a singular RAP
// For example, it will return the RAp/ResourceTy associated with a function/variable/etc
// depending on how its used
pub fn get_rap(expr: &Expr, tcx: &TyCtxt, raps: &HashMap<String, RapData>) -> ResourceTy {
  match expr.kind {
    ExprKind::Path(QPath::Resolved(_,p)) => {
      let name = tcx.hir_name(p.segments[0].hir_id).as_str().to_owned();
      // Fall back to Anonymous when the path resolves to a name we
      // didn't register as a RAP. The most common cause is a path
      // expression inside a macro expansion that references a
      // synthetic local (e.g. modern `println!` expands to refs to
      // an `args` binding that visit_local declines to register).
      // Anonymous is the existing return for "unknown resource";
      // downstream code already handles it.
      match raps.get(&name) {
        Some(rd) => ResourceTy::Value(rd.rap.to_owned()),
        None => ResourceTy::Anonymous,
      }
    }
    // In a deref expression 
    ExprKind::Unary(UnOp::Deref, expr) => {
      let rhs_rap = fetch_rap(&expr, tcx, raps);
      match rhs_rap {
        Some(x) => {
          ResourceTy::Deref(x)
        }
        None => ResourceTy::Anonymous
      }
    }
    ExprKind::AddrOf(_, _, expr) | ExprKind::Unary(_, expr) => get_rap(expr, tcx, raps),
    ExprKind::Call(fn_expr, _) => {
      let fn_name = hirid_to_var_name(fn_expr.hir_id, tcx).unwrap();
      ResourceTy::Value(raps.get(&fn_name).unwrap().rap.to_owned())
    }
    ExprKind::MethodCall(name_and_generic_args, ..) => {
      let fn_name = hirid_to_var_name(name_and_generic_args.hir_id, &tcx).unwrap();
      ResourceTy::Value(raps.get(&fn_name).unwrap().rap.to_owned())
    }
    ExprKind::Block(b, _) => {
      match b.expr {
        Some(expr) => { get_rap(expr, tcx, raps) }
        None => { panic!("invalid expr for getting rap") }
      }
    }
    ExprKind::Field(expr, ident) => {
      match expr {
        Expr{kind: ExprKind::Path(QPath::Resolved(_,p)), ..} => {
          let name = tcx.hir_name(p.segments[0].hir_id).as_str().to_owned();
          let field_name: String = ident.as_str().to_owned();
          let total_name = format!("{}.{}", name, field_name);
          ResourceTy::Value(raps.get(&total_name).unwrap().rap.to_owned())
        }
        _ => { panic!("unexpected field expr") }
      } 
    }
    _ => ResourceTy::Anonymous
  }
}

// Almost the same as get_rap but we don't care about any anonymous owners 
// which is why we return a RAP instead of a ResourceTy
pub fn fetch_rap(expr: &Expr, tcx: &TyCtxt, raps: &HashMap<String, RapData>) -> Option<ResourceAccessPoint> {
  match expr.kind {
    ExprKind::Call(..) | ExprKind::Binary(..) | ExprKind::Lit(_) | ExprKind::MethodCall(..) => None,
    ExprKind::Path(QPath::Resolved(_,p)) => {
      let name = tcx.hir_name(p.segments[0].hir_id).as_str().to_owned();
      Some(raps.get(&name).unwrap().rap.to_owned())
    }
    ExprKind::AddrOf(_, _, expr) | ExprKind::Unary(_, expr) => fetch_rap(expr, tcx, raps),
    ExprKind::Block(b, _) => {
      match b.expr {
        Some(expr) => { fetch_rap(expr, tcx, raps) }
        None => { panic!("invalid expr for fetching rap") }
      }
    }
    ExprKind::Field(expr, ident) => {
      match expr {
        Expr{kind: ExprKind::Path(QPath::Resolved(_,p)), ..} => {
          let name = tcx.hir_name(p.segments[0].hir_id).as_str().to_owned();
          let field_name: String = ident.as_str().to_owned();
          let total_name = format!("{}.{}", name, field_name);
          Some(raps.get(&total_name).unwrap().rap.to_owned())
        }
        _ => { panic!("unexpected field expr") }
      } 
    }
    _ => None
  }
}


// used to find the lender from the rhs of a let expr
// ex: let a = &y; (y is the lender)
// it gets a little interesting when rhs involves a * operator
pub fn find_lender(rhs: &Expr, tcx: &TyCtxt, raps: &HashMap<String, RapData>, borrow_map: &HashMap<String, RefData>) -> ResourceTy {
  match rhs.kind {
    ExprKind::Path(QPath::Resolved(_,p)) => {
      let name = tcx.hir_name(p.segments[0].hir_id).as_str().to_owned();
      if borrow_map.contains_key(&name) {
        borrow_map.get(&name).unwrap().to_owned().lender
      }
      else{
        ResourceTy::Value(raps.get(&name).unwrap().rap.to_owned())
      }
    }
    // For method-call chains that return a reference, the lender is
    // the chain's *base* receiver, not the outer call. Walk down
    // through `.method().method()` to the leftmost receiver and try
    // resolving that as the lender (it's commonly a Path or Field).
    // If we can't resolve it (e.g. the chain bottoms out in a
    // literal or another call), fall back to Anonymous as before.
    ExprKind::MethodCall(_, recv, _, _) => {
      find_lender(recv, tcx, raps, borrow_map)
    }
    ExprKind::Call(..) | ExprKind::Lit(_) => {
      ResourceTy::Anonymous
    }
    ExprKind::AddrOf(_, _, expr) => {
      find_lender(expr, tcx, raps, borrow_map)
    }
    ExprKind::Block(b, _) => {
      match b.expr {
        Some(expr) => { find_lender(expr, tcx, raps, borrow_map) }
        None => { panic!("invalid rhs lender block") }
      }
    }
    ExprKind::Unary(op, expr) => { 
      match op {
        rustc_hir::UnOp::Deref => {
          match find_lender(expr, tcx, raps, borrow_map) {
            ResourceTy::Deref(r) | ResourceTy::Value(r) => {
              borrow_map.get(r.name()).unwrap().to_owned().lender
            }
            _ => { ResourceTy::Anonymous }
          }
        },
        _ => {
          find_lender(expr, tcx, raps, borrow_map)
        }
      }
    }
    ExprKind::Field(expr, ident) => {
      match expr {
        Expr{kind: ExprKind::Path(QPath::Resolved(_,p)), ..} => {
          let mut name = tcx.hir_name(p.segments[0].hir_id).as_str().to_owned();
          name = format!("{}.{}", name, ident.as_str());
          if borrow_map.contains_key(&name) {
            borrow_map.get(&name).unwrap().to_owned().lender
          }
          else{
            ResourceTy::Value(raps.get(&name).unwrap().rap.to_owned())
          }
        }
        _ => { panic!("unexpected field expr") }
      } 
    }
  _ => { panic!("unexpected rhs lender expr {:#?}", rhs) }
  }
}