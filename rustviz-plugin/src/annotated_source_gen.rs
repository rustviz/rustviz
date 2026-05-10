//! In this file we create the 'annotated source' see examples from RustViz1 in the examples library
//! This file is necessary for generating the the code panel part of the visualization.
//! Each RAP is assigned a hash (a unique id) and each hash corresponds to a different color (defined in the CSS template)
//! It also allows the Javascript to tell which elements in the svg should be highlighted when hovered over.
//! The logic is quite simple, visit each part of a function body and replace each RAP as it appears in the source string
//! with a new string. (It's a little more complicated but this is the gist)

use crate::{expr_visitor::ExprVisitor, expr_visitor_utils::{expr_to_rap_name, hirid_to_var_name, span_to_line}};
use rustc_hir::{Expr, ExprKind, QPath, Stmt, StmtKind, LetStmt, Pat, PatKind};
use rustc_span::Span;




impl <'a, 'tcx> ExprVisitor<'a, 'tcx> {

// TODO: need to account for /t
pub fn annotate_src(&mut self, name: String, s: Span, is_func: bool, hash: u64) {
  let line: usize = span_to_line(&s, &self.tcx);
  let left:usize = self.tcx.sess.source_map().lookup_char_pos(s.lo()).col_display;
  let right: usize = self.tcx.sess.source_map().lookup_char_pos(s.hi()).col_display;

  // the reason we replace the '<' and '>' characters with [_ and _] is that
  // < and > characters are illegal on their own in html (need to use &gt; / &lt: / &amp; etc)
  // We eventually replace all the [_ and _] after we replace the </>
  // Synthetic spans (e.g. desugared macro expansions) can map to lines outside the user's
  // source; skip annotation for those rather than panic.
  let mut line_contents = match self.source_map.get(&line) {
    Some(c) => c.clone(),
    None => return,
  };
  let replace_with: String = if is_func {
      format!("[_tspan class=\"fn\" data-hash=\"{}\" hash=\"{}\"_]{}[_/tspan_]", 0, hash, name)
    } else {
      format!("[_tspan data-hash=\"{}\"_]{}[_/tspan_]", hash, name)
  };

  if right > line_contents.len() || left > right {
    return;
  }
  line_contents.replace_range(left..right, &replace_with);
  // The reason we have a vector of strings associated with each line instead of just a single
  // string that is constantly being mutated is because it would mess up the positions we get from
  // the span of the variable we want to replace. So we keep a collection of strings and then merge
  // them all together later.
  let Some(v) = self.annotated_lines.get_mut(&line) else { return };
  if !v.contains(&line_contents) {
    v.push(line_contents);
  }
}

pub fn annotate_expr(& mut self, expr: &'tcx Expr) {
  // Most macro/desugar-generated subtrees should be skipped — they
  // refer to synthetic items the visitor never registered. But a few
  // specific desugar shapes wrap *user-written* code in synthetic
  // spans, and we want to keep annotating into them:
  //
  //   while cond { body }                 →  loop { if cond { body } else { break } }
  //   while let pat = expr { body }       →  loop { if let pat = expr { body } else { break } }
  //   for pat in iter { body }            →  match into_iter(iter) { mut iter =>
  //                                            loop { match next(&mut iter) {
  //                                              None => break, Some(pat) => body } } }
  //
  // The wrapping `If` / `Match` / `Loop` carry the DesugaringKind
  // span (so `from_expansion()` returns true), but the user's `pat`,
  // scrutinee, and `body` are themselves user-written. Without this
  // descent, `s` in `while let Some(s) = stack.pop() { … }` and
  // `stack`/`pop` on the same line never get annotated.
  if expr.span.from_expansion() {
    if let ExprKind::If(guard, then_body, _else) = expr.kind {
      // The else arm of a while/if-let desugar is the synthetic
      // `break` — skip it. The guard is either a `Let(pat, init)`
      // (let-form) or a plain condition (regular `while`).
      if let ExprKind::Let(let_expr) = guard.kind {
        self.annotate_pat(let_expr.pat);
        self.annotate_expr(let_expr.init);
      } else {
        self.annotate_expr(guard);
      }
      self.annotate_expr(then_body);
    }
    return;
  }
  match expr.kind {
    ExprKind::Path(QPath::Resolved(_, p)) => {
      let (name, is_func) = match p.res {
        rustc_hir::def::Res::Def(rustc_hir::def::DefKind::Ctor(..), _) => {
          let mut name = String::new();
            for (i, segment) in p.segments.iter().enumerate() {
              name.push_str(self.tcx.hir_name(segment.hir_id).as_str());
              if i < p.segments.len() - 1 {
                name.push_str("::");
              }
            }
            (name, true)
        }
        _ => (self.tcx.hir_name(p.segments[0].hir_id).as_str().to_owned(), false)
      };
      match self.raps.get(&name) {
        Some(r) => {
          self.annotate_src(name.clone(), p.span, is_func, *r.rap.hash());
        }
        None => {}
      }
    }
    ExprKind::Call(fn_expr, fn_args) => {
      match fn_expr.kind {
        ExprKind::Path(QPath::Resolved(_,rustc_hir::Path{res: rustc_hir::def::Res::Def(_, id), ..})) 
          if !id.is_local() => {
            match fn_args {
              [Expr{kind: ExprKind::Call(_, a),..}] => {
                match a {
                  [_, Expr{kind: ExprKind::AddrOf(_, _, 
                    Expr{kind: ExprKind::Array(x),..}),..}] => {
                      for exp in x.iter() {
                        match exp {
                          Expr{kind: ExprKind::Call(_, format_args), ..} => {
                            for arg in format_args.iter() {
                              self.annotate_expr(arg);
                            }
                          }
                          _ => {}
                        }
                      }
                    }
                  _ => {
                    // println!("here2");
                    // println!("args {:#?}", a);
                    // let fn_name: String = self.hirid_to_var_name(fn_expr.hir_id).unwrap();
                    // self.annotate_src(fn_name.clone(), fn_expr.span, true, *self.raps.get(&fn_name).unwrap().rap.hash());
                    // for arg in a.iter() {
                    //   self.annotate_expr(arg);
                    // }
                  }
                }
              }
              _ => {
                let fn_name = hirid_to_var_name(fn_expr.hir_id, &self.tcx).unwrap();
                self.annotate_src(fn_name.clone(), fn_expr.span, true, *self.raps.get(&fn_name).unwrap().rap.hash());
                for a in fn_args.iter() {
                  self.annotate_expr(a);
                }
              }
            }
          }
          _ => {
            let fn_name = hirid_to_var_name(fn_expr.hir_id, &self.tcx).unwrap();
            self.annotate_src(fn_name.clone(), fn_expr.span, true, *self.raps.get(&fn_name).unwrap().rap.hash());
            for a in fn_args.iter() {
              self.annotate_expr(a);
            }
          }
      }
    }
    ExprKind::Unary(_, ex) | ExprKind::AddrOf(_, _, ex) 
    | ExprKind::Ret(Some(ex)) => {
      self.annotate_expr(ex);
    }
    ExprKind::Binary(_, exp1, exp2) => {
      self.annotate_expr(exp1);
      self.annotate_expr(exp2);
    }
    ExprKind::MethodCall(name_and_generic_args, rcvr, args, _) => {
      let fn_name = name_and_generic_args.ident.as_str().to_owned();
      self.annotate_src(fn_name.clone(), name_and_generic_args.ident.span, true, *self.raps.get(&fn_name).unwrap().rap.hash());
      for arg in args.iter() {
        self.annotate_expr(arg);
      }
      match rcvr.kind {
        ExprKind::MethodCall(_p_seg, ..) => {
          self.annotate_expr(rcvr);
          return;
        }
        _ => {}
      }
      // Same handling as the visitor's MethodCall arm: receivers
      // like `r.s` are Field expressions, not bare paths, so use
      // `expr_to_rap_name` to derive the qualified name. If we
      // can't resolve a RAP, skip annotation for this receiver
      // rather than panic — the annotation is purely for source-
      // panel coloring; missing it just leaves the receiver
      // un-styled.
      if let Some(rcvr_name) = expr_to_rap_name(rcvr, &self.tcx) {
        if let Some(rd) = self.raps.get(&rcvr_name) {
          self.annotate_src(rcvr_name.clone(), rcvr.span, false, *rd.rap.hash());
        }
      }
    }
    ExprKind::Assign(exp1, exp2, _) | ExprKind::AssignOp(_, exp1, exp2) => {
      self.annotate_expr(exp1);
      self.annotate_expr(exp2);
    }
    ExprKind::Block(block, _) => {
      for stmt in block.stmts.iter() {
        self.annotate_stmt(stmt);
      }
      match block.expr {
        Some(ex) => {
          self.annotate_expr(ex);
        }
        _ => {}
      }
    }
    ExprKind::Struct(qpath, fields, _) => {
      match qpath {
        QPath::LangItem(..) => { return; }
        _ => {}
      }
      for field in fields.iter() {
        // Field idents that aren't tied to a registered RAP — most
        // commonly union fields, which we render as opaque single-
        // owner columns and so don't add per-field id_map entries
        // for — get the colorization skipped instead of panicking
        // on an `id_map.get(...).unwrap()`. The expression in the
        // initialiser is still walked normally below.
        if let Some(hash) = self.id_map.get(field.ident.as_str()) {
          self.annotate_src(field.ident.to_string(), field.ident.span, false, *hash as u64);
        }
        self.annotate_expr(field.expr);
      }
    }
    ExprKind::Field(exp, ident) => {
      // Same robustness: a field-access projection on a non-struct
      // RAP (a union, an enum variant accessed through `unsafe`,
      // etc.) won't have its field name in id_map. Skip the
      // colorization for that token; the receiver expression is
      // still walked.
      if let Some(hash) = self.id_map.get(ident.as_str()) {
        self.annotate_src(ident.to_string(), ident.span, false, *hash as u64);
      }
      self.annotate_expr(exp);
    }
    ExprKind::DropTemps(exp) => {
      self.annotate_expr(&exp);
    }
    ExprKind::If(guard_expr, if_expr, else_expr) => {
      self.annotate_expr(&guard_expr);
      self.annotate_expr(&if_expr);
      match else_expr {
        Some(e) => self.annotate_expr(&e),
        None => {}
      }
    }
    // `if let pat = expr { … }` user-written. The Let node sits in
    // the If's guard slot; its `pat` carries pattern bindings the
    // user wrote (e.g. the `s` in `if let Some(s) = opt`) and `init`
    // is the scrutinee.
    ExprKind::Let(let_expr) => {
      self.annotate_pat(let_expr.pat);
      self.annotate_expr(let_expr.init);
    }
    ExprKind::Loop(block, _, _loop_ty, _span) => {
      for stmt in block.stmts.iter() {
        self.annotate_stmt(stmt);
      }
      match block.expr {
        Some(ex) => {
          self.annotate_expr(ex);
        }
        _ => {}
      }
    }
    ExprKind::Match(match_expr, arms, _) => {
      self.annotate_expr(&match_expr);
      for arm in arms {
        match &arm.guard {
          Some(_g) => {
            //self.annotate_expr(&g);
          }
          None => {}
        }

        self.annotate_pat(arm.pat);

        self.annotate_expr(arm.body);
      }
    }
    ExprKind::Tup(expr_list) => {
      for e in expr_list.iter() {
        self.annotate_expr(e);
      }
    }
     _ => { 
      //println!("unrecognized expr {:#?}", expr);
      }
  }
}

pub fn annotate_local(&mut self, loc: &'tcx LetStmt<'tcx>) {
  match loc.pat.kind {
    rustc_hir::PatKind::Binding(_, _, ident, _) => {
      let lhs_var:String = ident.to_string();
      self.annotate_src(lhs_var.clone(), ident.span, false, *self.raps.get(&lhs_var).unwrap().rap.hash());
      match loc.init {
        Some(exp) => {
          self.annotate_expr(exp);
        }
        _ => {}
      }
    }
    _ => {}
  }
}

pub fn annotate_stmt(&mut self, stmt: &'tcx Stmt<'tcx>) {
  match stmt.kind {
    StmtKind::Let(ref local) => {
      self.annotate_local(local);
    }
    StmtKind::Item(_) => {}
    StmtKind::Expr(exp) | StmtKind::Semi(exp) => {
      self.annotate_expr(exp);
    }
  }
}

pub fn annotate_pat(&mut self, pat: &Pat) {
  match pat.kind {
    PatKind::Binding(_, _, ident, _) => {
      let name = ident.as_str().to_owned();
      match self.raps.get(&name) {
        Some(r) => {
          let hash = *r.rap.hash();
          self.annotate_src(name, ident.span, false, hash);
        }
        None => {}
      }
    },
    PatKind::TupleStruct(_, pat_list, _) | PatKind::Tuple(pat_list, _)=> {
      for p in pat_list.iter() {
        self.annotate_pat(p);
      }
    },
    _ => {}
  }
}
}