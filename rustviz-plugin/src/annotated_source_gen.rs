//! In this file we create the 'annotated source' see examples from RustViz1 in the examples library
//! This file is necessary for generating the the code panel part of the visualization.
//! Each RAP is assigned a hash (a unique id) and each hash corresponds to a different color (defined in the CSS template)
//! It also allows the Javascript to tell which elements in the svg should be highlighted when hovered over.
//! The logic is quite simple, visit each part of a function body and replace each RAP as it appears in the source string
//! with a new string. (It's a little more complicated but this is the gist)

use crate::{expr_visitor::ExprVisitor, expr_visitor_utils::{binding_name, expr_to_rap_name, fn_key, hirid_to_var_name, overloaded_by_value_binop, overloaded_comparison, path_name, span_to_line, static_of_path, unary_op_operand}};
use rustc_hir::{Expr, ExprKind, QPath, Stmt, StmtKind, LetStmt, Pat, PatKind};
use rustc_hir::intravisit::{self, Visitor};
use rustc_span::{BytePos, Span};

/// The std macros whose arguments the visitor traces as uses (the
/// formatting macros and the assertion macros). Their expansions are
/// synthetic, but the argument expressions inside them are the user's,
/// and get highlighted like arguments of a plain call (#817).
const ARG_TRACING_MACROS: &[&str] = &[
  "assert!", "assert_eq!", "assert_ne!",
  "debug_assert!", "debug_assert_eq!", "debug_assert_ne!",
  // Formatting macros (#824).
  "println!", "print!", "eprintln!", "eprint!", "format!", "write!",
  "writeln!", "panic!", "unreachable!", "todo!", "unimplemented!",
];

/// Byte ranges (start of the opening quote, end of the closing quote) of
/// the string literals in `src`, including raw strings.
fn string_literal_ranges(src: &str) -> Vec<(usize, usize)> {
  let b = src.as_bytes();
  let mut out = Vec::new();
  let mut i = 0;
  while i < b.len() {
    let start = i;
    let mut hashes = 0;
    let mut j = i;
    if b[j] == b'r' && (j == 0 || !(b[j - 1].is_ascii_alphanumeric() || b[j - 1] == b'_')) {
      j += 1;
      while j < b.len() && b[j] == b'#' { hashes += 1; j += 1; }
      if j < b.len() && b[j] == b'"' {
        let mut k = j + 1;
        while k < b.len() {
          if b[k] == b'"' && b[k + 1..].iter().take(hashes).filter(|&&c| c == b'#').count() == hashes {
            break;
          }
          k += 1;
        }
        let end = (k + 1 + hashes).min(b.len());
        out.push((start, end));
        i = end;
        continue;
      }
    }
    if b[i] == b'"' {
      let mut k = i + 1;
      while k < b.len() && b[k] != b'"' {
        if b[k] == b'\\' { k += 1; }
        k += 1;
      }
      let end = (k + 1).min(b.len());
      out.push((start, end));
      i = end;
      continue;
    }
    i += 1;
  }
  out
}

/// Collects the maximal user-written sub-expressions of a macro
/// expansion: the arguments the user typed, not the code the macro
/// built around them.
struct UserArgs<'tcx> {
  args: Vec<&'tcx Expr<'tcx>>,
}

impl<'tcx> Visitor<'tcx> for UserArgs<'tcx> {
  fn visit_expr(&mut self, expr: &'tcx Expr<'tcx>) {
    if expr.span.from_expansion() {
      intravisit::walk_expr(self, expr);
    } else {
      self.args.push(expr);
    }
  }
}




impl <'a, 'tcx> ExprVisitor<'a, 'tcx> {

pub fn annotate_src(&mut self, name: String, s: Span, is_func: bool, hash: u64) {
  // A `// rustviz: skip` variable has no column, so a colour for it
  // would match nothing on the timeline (#1313): leave it plain.
  #[expect(clippy::disallowed_methods, reason = "any() makes order irrelevant")]
  let skipped = !is_func && self.skip_raps.iter().any(|n| {
    self.raps.get(n).is_some_and(|rd| *rd.rap.hash() == hash)
  });
  if skipped {
    return;
  }
  let line: usize = span_to_line(&s, &self.tcx);

  // The tspan markup is written with MARK_OPEN/MARK_CLOSE instead of `<`/`>`
  // because the whole line is HTML-escaped later; generate_annotated_src
  // turns the markers into real angle brackets after escaping.
  // Synthetic spans (e.g. desugared macro expansions) can map to lines outside the user's
  // source; skip annotation for those rather than panic.
  let mut line_contents = match self.source_map.get(&line) {
    Some(c) => c.clone(),
    None => return,
  };
  // Byte offsets within the line, not display columns (#1160). A span
  // that starts on another line than the one its call site maps to
  // (some desugarings) would index the wrong line's text: skip it.
  if self.tcx.sess.source_map().lookup_char_pos(s.lo()).line != line {
    return;
  }
  let Some((left, right)) = crate::utils::span_line_bytes(self.tcx.sess.source_map(), s, &line_contents) else {
    return;
  };
  // A variable is shown as written: its RAP name can differ from the
  // source text (`s₂` for a second `let s`, see `binding_name`), but
  // the code panel shows the code.
  let shown = match line_contents.get(left..right) {
    Some(text) if !is_func => text.to_owned(),
    _ => name,
  };
  let replace_with: String = crate::utils::tspan_markup(&shown, hash, is_func);
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
  // Code that never runs (#1004) wasn't visited.
  if self.dead_code.contains(&expr.hir_id) {
    return;
  }
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
    if crate::visitor::user_macro_name(expr.span).is_some_and(|m| ARG_TRACING_MACROS.contains(&m.as_str())) {
      let mut found = UserArgs { args: Vec::new() };
      intravisit::walk_expr(&mut found, expr);
      let sm = self.tcx.sess.source_map();
      let call = expr.span.source_callsite();
      let call_text = sm.span_to_snippet(call).unwrap_or_default();
      let strings = string_literal_ranges(&call_text);
      for arg in found.args {
        // An implicit format capture (`println!("{s}")`) is an
        // expression whose span sits inside the format string; the
        // code panel can't nest a highlight in the string's own span,
        // so leave it plain.
        if arg.span.lo() >= call.lo() && arg.span.hi() <= call.hi() {
          let (lo, hi) = ((arg.span.lo() - call.lo()).0 as usize, (arg.span.hi() - call.lo()).0 as usize);
          if strings.iter().any(|&(a, b)| lo > a && hi < b) {
            continue;
          }
        }
        self.annotate_expr(arg);
      }
      return;
    }
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
        _ => (path_name(p, &self.tcx), false)
      };
      let key = if matches!(p.res, rustc_hir::def::Res::Local(_)) || static_of_path(p).is_some() { name.clone() } else { fn_key(&name) };
      match self.raps.get(&key) {
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
                    // self.annotate_src(fn_name.clone(), fn_expr.span, true, *self.raps.get(&fn_key(&fn_name)).unwrap().rap.hash());
                    // for arg in a.iter() {
                    //   self.annotate_expr(arg);
                    // }
                  }
                }
              }
              _ => {
                let fn_name = hirid_to_var_name(fn_expr.hir_id, &self.tcx).unwrap();
                if let Some(rd) = self.raps.get(&fn_key(&fn_name)) {
                  let hash = *rd.rap.hash();
                  self.annotate_src(fn_name.clone(), fn_expr.span, true, hash);
                }
                for a in fn_args.iter() {
                  self.annotate_expr(a);
                }
              }
            }
          }
          // Calling a closure variable (`f(1)`): `f` has a column, and
          // gets its colour like any other use of it (#1189).
          ExprKind::Path(QPath::Resolved(_, p)) if matches!(p.res, rustc_hir::def::Res::Local(_))
            && self.raps.contains_key(&path_name(p, &self.tcx)) => {
            self.annotate_expr(fn_expr);
            for a in fn_args.iter() {
              self.annotate_expr(a);
            }
          }
          _ => {
            match hirid_to_var_name(fn_expr.hir_id, &self.tcx) {
              Some(fn_name) => {
                if let Some(rd) = self.raps.get(&fn_key(&fn_name)) {
                  let hash = *rd.rap.hash();
                  self.annotate_src(fn_name.clone(), fn_expr.span, true, hash);
                }
              }
              // A callee that is an expression (`(w.f)()`, `make()(x)`;
              // #703): its variables are highlighted like anywhere else.
              None => self.annotate_expr(fn_expr),
            }
            for a in fn_args.iter() {
              self.annotate_expr(a);
            }
          }
      }
    }
    // `-x` / `!b`: highlight the operator when the visitor gave it a
    // function RAP (see its Unary arm): only an overloaded one, which
    // is a call. Both are one character.
    ExprKind::Unary(rustc_hir::UnOp::Neg | rustc_hir::UnOp::Not, ex) => {
      self.annotate_expr(ex);
      let call = self.tcx.typeck(expr.hir_id.owner).is_method_call(expr);
      if let Some((op_name, _)) = unary_op_operand(expr, &self.tcx, &self.raps).filter(|_| call) {
        if let Some(rd) = self.raps.get(op_name) {
          let hash = *rd.rap.hash();
          let op_span = expr.span.with_hi(expr.span.lo() + BytePos(1));
          self.annotate_src(op_name.to_owned(), op_span, true, hash);
        }
      }
    }
    ExprKind::Unary(_, ex) | ExprKind::AddrOf(_, _, ex) 
    | ExprKind::Ret(Some(ex)) => {
      self.annotate_expr(ex);
    }
    ExprKind::Binary(op, exp1, exp2) => {
      self.annotate_expr(exp1);
      self.annotate_expr(exp2);
      // Overloaded operators get a function RAP named after the
      // operator (see the visitor's Binary arm); highlight it. A
      // built-in one is not an actor and gets none (#908).
      let op_name = overloaded_by_value_binop(expr, &self.tcx)
        .or_else(|| overloaded_comparison(expr, &self.tcx).map(|(op, _)| op));
      if let Some(op_name) = op_name {
        if let Some(rd) = self.raps.get(op_name) {
          let hash = *rd.rap.hash();
          self.annotate_src(op_name.to_owned(), op.span, true, hash);
        }
      }
    }
    ExprKind::MethodCall(name_and_generic_args, rcvr, args, _) => {
      let fn_name = name_and_generic_args.ident.as_str().to_owned();
      // (A call in a closure body has no function RAP: left plain.)
      if let Some(rd) = self.raps.get(&fn_key(&fn_name)) {
        let hash = *rd.rap.hash();
        self.annotate_src(fn_name.clone(), name_and_generic_args.ident.span, true, hash);
      }
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
      } else {
        // A receiver that is itself an expression (`make(v).len()`,
        // `(a, b).0.len()`): its variables are coloured (#1189).
        self.annotate_expr(rcvr);
      }
    }
    ExprKind::Assign(exp1, exp2, _) => {
      self.annotate_expr(exp1);
      self.annotate_expr(exp2);
    }
    // `a op= b`: highlight the operator when the visitor gave it a
    // function RAP (see its AssignOp arm): only an overloaded one,
    // which is a call (#908).
    ExprKind::AssignOp(op, exp1, exp2) => {
      self.annotate_expr(exp1);
      self.annotate_expr(exp2);
      let op_name = op.node.as_str();
      if self.tcx.typeck(expr.hir_id.owner).is_method_call(expr) {
        if let Some(rd) = self.raps.get(op_name) {
          let hash = *rd.rap.hash();
          self.annotate_src(op_name.to_owned(), op.span, true, hash);
        }
      }
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
      // Compiler-desugared struct literals (e.g. `a..b` lowering to a
      // `Range { start, end }`) have no user-written field names to annotate.
      // (Older nightlies marked these with `QPath::LangItem`, since removed.)
      let _ = qpath;
      if expr.span.desugaring_kind().is_some() || expr.span.from_expansion() { return; }
      for field in fields.iter() {
        // Field idents that aren't tied to a registered RAP — most
        // commonly union fields, which we render as opaque single-
        // owner columns and so don't add per-field id_map entries
        // for — get the colorization skipped instead of panicking
        // on an `id_map.get(...).unwrap()`. The expression in the
        // initialiser is still walked normally below.
        // The field of the variable the literal initialises (`p.a` in
        // `let p = P { a: s };`) has a column: the name gets its colour
        // (#1190). Otherwise the field name's own colour.
        let column = self.literal_target(expr)
          .and_then(|t| self.raps.get(&format!("{}.{}", t, field.ident.as_str())))
          .map(|rd| *rd.rap.hash());
        if let Some(hash) = column.or_else(|| self.id_map.get(field.ident.as_str()).map(|h| *h as u64)) {
          self.annotate_src(field.ident.to_string(), field.ident.span, false, hash);
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
      // `p.a` with a `p.a` column: the field name gets that column's
      // colour, so the code matches the column that draws it (#1190).
      let column = expr_to_rap_name(expr, &self.tcx)
        .and_then(|n| self.raps.get(&n))
        .map(|rd| *rd.rap.hash());
      if let Some(hash) = column.or_else(|| self.id_map.get(ident.as_str()).map(|h| *h as u64)) {
        self.annotate_src(ident.to_string(), ident.span, false, hash);
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
        self.annotate_pat(arm.pat);
        // `Some(n) if n > lim =>`: the guard's variables (#1189).
        if let Some(g) = arm.guard {
          self.annotate_expr(g);
        }

        self.annotate_expr(arm.body);
      }
    }
    ExprKind::Tup(expr_list) | ExprKind::Array(expr_list) => {
      for e in expr_list.iter() {
        self.annotate_expr(e);
      }
    }
    // `arr[i]`, `x as u64`, `[s; 3]`, `break v` (#1189).
    ExprKind::Index(base, index, _) => {
      self.annotate_expr(base);
      self.annotate_expr(index);
    }
    ExprKind::Cast(ex, _) | ExprKind::Repeat(ex, _) | ExprKind::Break(_, Some(ex)) => {
      self.annotate_expr(ex);
    }
    // A closure's body: the variables it captures and its own (#1189).
    ExprKind::Closure(closure) => {
      let body = self.tcx.hir_body(closure.body);
      for param in body.params {
        self.annotate_pat(param.pat);
      }
      self.annotate_expr(body.value);
    }
     _ => { 
      //println!("unrecognized expr {:#?}", expr);
      }
  }
}

/// The variable a struct literal initialises or is assigned to (`p` in
/// `let p = P { .. };` or `p = P { .. };`), whose fields have columns.
fn literal_target(&self, lit: &Expr) -> Option<String> {
  use rustc_hir::Node;
  match self.tcx.parent_hir_node(lit.hir_id) {
    Node::LetStmt(l) => match l.pat.kind {
      PatKind::Binding(_, hir_id, ..) => Some(binding_name(&self.tcx, hir_id)),
      _ => None,
    },
    Node::Expr(Expr { kind: ExprKind::Assign(lhs, rhs, _), .. }) if rhs.hir_id == lit.hir_id => expr_to_rap_name(lhs, &self.tcx),
    _ => None,
  }
}

pub fn annotate_local(&mut self, loc: &'tcx LetStmt<'tcx>) {
  match loc.pat.kind {
    rustc_hir::PatKind::Binding(_, hir_id, ident, _) => {
      let lhs_var:String = binding_name(&self.tcx, hir_id);
      // A `let x;` that is never assigned has no RAP (#234).
      if let Some(rd) = self.raps.get(&lhs_var) {
        self.annotate_src(lhs_var.clone(), ident.span, false, *rd.rap.hash());
      }
      match loc.init {
        Some(exp) => {
          self.annotate_expr(exp);
        }
        _ => {}
      }
    }
    // `let (s, t) = (x, y);`, `let P { a, b } = p;`, `let [a, b] =
    // arr;`: the pattern's variables and the initialiser (#1189).
    _ => {
      self.annotate_pat(loc.pat);
      if let Some(exp) = loc.init {
        self.annotate_expr(exp);
      }
    }
  }
  if let Some(els) = loc.els {
    for stmt in els.stmts.iter() {
      self.annotate_stmt(stmt);
    }
    if let Some(ex) = els.expr {
      self.annotate_expr(ex);
    }
  }
}

pub fn annotate_stmt(&mut self, stmt: &'tcx Stmt<'tcx>) {
  if self.dead_code.contains(&stmt.hir_id) {
    return;
  }
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
    PatKind::Binding(_, hir_id, ident, sub) => {
      let name = binding_name(&self.tcx, hir_id);
      match self.raps.get(&name) {
        Some(r) => {
          let hash = *r.rap.hash();
          self.annotate_src(name, ident.span, false, hash);
        }
        None => {}
      }
      if let Some(p) = sub {
        self.annotate_pat(p);
      }
    },
    // `P { a, b }`, `[a, .., b]`, `&x`, `box x` (#1189).
    PatKind::Struct(_, fields, _) => {
      for f in fields.iter() {
        self.annotate_pat(f.pat);
      }
    },
    PatKind::Slice(before, mid, after) => {
      for p in before.iter().chain(mid).chain(after.iter()) {
        self.annotate_pat(p);
      }
    },
    PatKind::Ref(inner, ..) | PatKind::Box(inner) | PatKind::Deref(inner) => {
      self.annotate_pat(inner);
    },
    PatKind::TupleStruct(_, pat_list, _) | PatKind::Tuple(pat_list, _)=> {
      for p in pat_list.iter() {
        self.annotate_pat(p);
      }
    },
    // `Ok(s) | Err(s)`: each alternative's `s` is the same variable (#503).
    PatKind::Or(alts) => {
      for p in alts.iter() {
        self.annotate_pat(p);
      }
    },
    _ => {}
  }
}
}