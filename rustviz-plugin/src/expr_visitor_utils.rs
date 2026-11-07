
use std::{cmp::max, collections::{HashMap, HashSet, VecDeque}};
use log::warn;
use rustc_hir::Mutability;
use crate::svg_generator::data::{ShadowNote, ExternalEvent, RcCall, RcKind, ResourceAccessPoint, ResourceAccessPoint_extract, ResourceTy};
use rustc_middle::ty::{Ty, TyCtxt, TypeckResults};
use rustc_middle::ty::adjustment::{Adjust, AutoBorrow, AutoBorrowMutability};
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

// BINDING NAMES
//
// Every local binding (`let s`, a parameter, a pattern binding in a
// `match` arm or `if let`, a `for` loop variable) is its own variable,
// even when it reuses a name: `let s = …; let s = …;` declares two
// variables, and the second shadows the first. RAPs (and so timeline
// columns) are keyed by name, so each binding gets a name of its own
// here, and every place that registers or looks up a variable goes
// through `binding_name` / `path_name` instead of the bare identifier.
//
// Within a fn body, a binding keeps the plain name (`s`) unless it
// shadows: if k variables of the same name are still in scope where it's
// declared, it is `s` with the subscript k+1 (`s₂`, `s₃`, …). Bindings
// that shadow the same number of variables share a column: they are
// never in scope together (sibling blocks, different arms of a `match`,
// the two blocks of an `if`, the two `if let`s of an `else if let`
// chain), so at most one of them exists at a time. A subscript can't be
// part of a Rust identifier, so these names never clash with a real
// variable. (#173)

/// Per-fn-body table built by `collect_binding_names`.
#[derive(Default)]
pub struct BindingNames {
  /// Name of each binding, by the binding pattern's `HirId` (what a
  /// path's `Res::Local` points to).
  pub by_hir: HashMap<HirId, String>,
  /// Identifier span of each binding with its name, in source order;
  /// used to name MIR locals (see `mir_local_names`).
  pub by_span: Vec<(Span, String)>,
  /// For each numbered column name (`s₂`, `s₃`, …): what to say about it
  /// in the column header's hover text (see `ShadowNote`).
  pub notes: HashMap<String, ShadowNote>,
}

thread_local! {
  static BINDING_NAMES: std::cell::RefCell<HashMap<rustc_hir::OwnerId, std::rc::Rc<BindingNames>>> =
    std::cell::RefCell::new(HashMap::new());
}

/// The (cached) binding-name table of the fn body that `owner` owns.
pub fn binding_names(tcx: &TyCtxt, owner: rustc_hir::OwnerId) -> std::rc::Rc<BindingNames> {
  if let Some(t) = BINDING_NAMES.with(|c| c.borrow().get(&owner).cloned()) {
    return t;
  }
  let t = std::rc::Rc::new(collect_binding_names(tcx, owner));
  BINDING_NAMES.with(|c| c.borrow_mut().insert(owner, t.clone()));
  t
}

/// `x`, `x₂`, `x₃`, …: the display name of the `n`th (1-based) variable
/// called `ident` in a fn body.
pub fn numbered_name(ident: &str, n: usize) -> String {
  if n <= 1 {
    return ident.to_owned();
  }
  const SUB: [char; 10] = ['₀', '₁', '₂', '₃', '₄', '₅', '₆', '₇', '₈', '₉'];
  let digits: String = n.to_string().chars().map(|d| SUB[d.to_digit(10).unwrap() as usize]).collect();
  format!("{}{}", ident, digits)
}

/// If `name` was made by `numbered_name` with `n >= 2`, the plain
/// identifier and `n`.
pub fn split_numbered_name(name: &str) -> Option<(&str, usize)> {
  const SUB: [char; 10] = ['₀', '₁', '₂', '₃', '₄', '₅', '₆', '₇', '₈', '₉'];
  let base = name.trim_end_matches(|c| SUB.contains(&c));
  if base.len() == name.len() || base.is_empty() {
    return None;
  }
  let n: usize = name[base.len()..].chars()
    .map(|c| char::from(b'0' + SUB.iter().position(|d| *d == c).unwrap() as u8))
    .collect::<String>().parse().ok()?;
  Some((base, n))
}

/// The source range in which the binding pattern `pat` is in scope
/// (from just after the point it's declared to the end of the block,
/// arm, or `if` body it belongs to), as byte offsets; `None` if the
/// binding isn't of a shape handled here. `body_span` is the fn body,
/// the scope of a parameter.
fn binding_scope(tcx: &TyCtxt, pat: HirId, body_span: Span) -> Option<(u32, u32)> {
  use rustc_hir::Node;
  let mut let_expr: Option<Span> = None;
  for (id, node) in tcx.hir_parent_iter(pat) {
    match (node, let_expr) {
      (Node::Pat(_) | Node::PatField(_), None) => {}
      (Node::Param(_), None) => return Some((body_span.lo().0, body_span.hi().0)),
      // `let s = …;`: in scope after the statement, to the end of its block.
      (Node::LetStmt(l), None) => {
        // (The statement wrapping the `let` sits between it and the block.)
        return tcx.hir_parent_iter(id)
          .find_map(|(_, n)| match n {
            Node::Stmt(_) => None,
            Node::Block(b) => Some(Some((l.span.hi().0, b.span.hi().0))),
            _ => Some(None),
          })
          .flatten();
      }
      (Node::Arm(a), None) => return Some((a.pat.span.lo().0, a.body.span.hi().0)),
      (Node::Expr(e), None) if matches!(e.kind, ExprKind::Let(_)) => let_expr = Some(e.span),
      // `if let` / `while let` / a `let` guard: in scope after the
      // `let`, through the `if`'s then-branch (or the arm's body).
      (Node::Expr(e), Some(l)) => {
        if let ExprKind::If(_, then, _) = e.kind {
          return Some((l.hi().0, then.span.hi().0));
        }
      }
      (Node::Arm(a), Some(l)) => return Some((l.hi().0, a.body.span.hi().0)),
      (_, Some(_)) => {}
      _ => return None,
    }
  }
  None
}

fn collect_binding_names(tcx: &TyCtxt, owner: rustc_hir::OwnerId) -> BindingNames {
  use rustc_hir::intravisit::{self, Visitor};

  struct Binding {
    span: Span,
    hir_id: HirId,
    ident: String,
    // Where the binding is in scope (`binding_scope`).
    scope: (u32, u32),
  }
  struct Collector<'a, 'tcx> {
    tcx: &'a TyCtxt<'tcx>,
    body_span: Span,
    out: Vec<Binding>,
  }
  impl<'v> Visitor<'v> for Collector<'_, '_> {
    // Closure bodies are nested bodies, which the default visitor
    // doesn't enter; the plugin doesn't draw their locals either.
    fn visit_pat(&mut self, p: &'v Pat<'v>) {
      match p.kind {
        PatKind::Binding(_, hir_id, ident, sub) => {
          // Desugaring / macro locals (`iter` of a `for` loop, `val`
          // of `?`, `args` of `println!`) never get a column.
          if !ident.span.from_expansion() {
            // A binding of a shape `binding_scope` doesn't know is
            // taken to stay in scope to the end of the fn.
            let scope = binding_scope(self.tcx, hir_id, self.body_span)
              .unwrap_or((ident.span.lo().0, self.body_span.hi().0));
            self.out.push(Binding { span: ident.span, hir_id, ident: ident.to_string(), scope });
          }
          if let Some(sp) = sub {
            self.visit_pat(sp);
          }
        }
        // The alternatives of an or-pattern bind the same variables,
        // and every use resolves to the first alternative's binding.
        PatKind::Or(alts) => {
          if let Some(first) = alts.first() {
            self.visit_pat(first);
          }
        }
        _ => intravisit::walk_pat(self, p),
      }
    }
  }

  let mut names = BindingNames::default();
  let Some(body) = tcx.hir_maybe_body_owned_by(owner.def_id) else { return names };
  let mut c = Collector { tcx, body_span: body.value.span, out: Vec::new() };
  c.visit_body(body);
  c.out.sort_by_key(|b| (b.span.lo(), b.span.hi()));

  // First pass: how many variables of the same name each binding
  // shadows (those still in scope where it's declared), which numbers
  // its column. Bindings that shadow the same number share a column.
  let n = c.out.len();
  let mut shadowed: Vec<Option<usize>> = vec![None; n];
  let mut col_names: Vec<String> = Vec::with_capacity(n);
  for i in 0..n {
    let at = c.out[i].span.lo().0;
    let in_scope: Vec<usize> = (0..i)
      .filter(|&j| c.out[j].ident == c.out[i].ident && { let (lo, hi) = c.out[j].scope; lo <= at && at < hi })
      .collect();
    shadowed[i] = in_scope.last().copied();
    col_names.push(numbered_name(&c.out[i].ident, in_scope.len() + 1));
  }
  // Second pass: names and header notes.
  for i in 0..n {
    let name = col_names[i].clone();
    if split_numbered_name(&name).is_some() {
      let line = tcx.sess.source_map().lookup_char_pos(c.out[i].span.lo()).line;
      let note = names.notes.entry(name.clone())
        .or_insert_with(|| ShadowNote { shadows: shadowed[i].map(|j| col_names[j].clone()), lines: Vec::new() });
      note.lines.push(line);
    }
    names.by_span.push((c.out[i].span, name.clone()));
    names.by_hir.insert(c.out[i].hir_id, name);
  }
  names
}

/// Name (and `raps` key) of the local variable bound by the binding
/// pattern `hir_id`. Falls back to the identifier for a binding outside
/// the table (e.g. one inside a closure body).
pub fn binding_name(tcx: &TyCtxt, hir_id: HirId) -> String {
  binding_names(tcx, hir_id.owner).by_hir.get(&hir_id).cloned()
    .unwrap_or_else(|| tcx.hir_name(hir_id).as_str().to_owned())
}

/// Name to look up for the path `p`: the variable it refers to if it
/// names a local, its first segment otherwise.
pub fn path_name(p: &Path, tcx: &TyCtxt) -> String {
  match p.res {
    Res::Local(hir_id) => binding_name(tcx, hir_id),
    _ => tcx.hir_name(p.segments[0].hir_id).as_str().to_owned(),
  }
}

/// For a place reached through an index projection (`v[0]`,
/// `ps[0].r`, `grid[1][2]`), the collection that is indexed first:
/// `v`, `ps`, `grid`. None for places without an index (`p.r`, `r`).
pub fn index_base<'a, 'tcx>(expr: &'a Expr<'tcx>) -> Option<&'a Expr<'tcx>> {
  match expr.kind {
    ExprKind::Index(recv, _, _) => Some(index_base(recv).unwrap_or(recv)),
    ExprKind::Field(inner, _) => index_base(inner),
    _ => None,
  }
}

/// Walk a chain of `Path` / `Field` / `AddrOf` / `Unary` / `DropTemps`
/// expressions and produce the qualified RAP name, e.g. `r`, `r.s`,
/// `r.a.b`, or `self.n`. Returns `None` for anything outside that
/// chain — calls, blocks, literals, etc. — since those don't have a
/// stable RAP name to look up.
///
/// Rationale: the visitor used to inline a one-arm match for
/// `Field(Path(...), ident)` everywhere it needed a struct-field RAP
/// name and `panic!("unexpected field expr")` on every other shape.
/// That panicked on `r.a.b` (nested), `(&r).s` (field through a
/// reference), `self.field` (in impl methods where `self.field`
/// isn't a registered RAP), and the receiver of `r.s.method()`.
/// Centralizing the walk lets us return `None` instead, which the
/// callers map to "skip this event" / "Anonymous resource" rather
/// than crashing the plugin.
pub fn expr_to_rap_name(expr: &Expr, tcx: &TyCtxt) -> Option<String> {
  match expr.kind {
    ExprKind::Path(QPath::Resolved(_, p)) => Some(path_name(p, tcx)),
    ExprKind::Field(inner, ident) => {
      let base = expr_to_rap_name(inner, tcx)?;
      Some(format!("{}.{}", base, ident.as_str()))
    }
    ExprKind::AddrOf(_, _, inner)
    | ExprKind::Unary(_, inner)
    | ExprKind::DropTemps(inner) => expr_to_rap_name(inner, tcx),
    // `v[..]` / `s[i]`: attribute the slice/element to the receiver's
    // RAP, the same way we treat `&v` directly. RustViz doesn't render
    // a separate column per index/slice, so collapsing onto the
    // receiver matches how fields share their parent's column.
    ExprKind::Index(recv, _, _) => expr_to_rap_name(recv, tcx),
    _ => None,
  }
}

/// Key of a Function RAP in a `raps` map. Functions live in their own
/// namespace, under a key no identifier can spell, so a variable named
/// like a function or method called in the same fn (`let len =
/// s.len();`, `let area = area(&rect);`) keeps its own RAP instead of
/// being merged with the function's (#232).
pub fn fn_key(name: &str) -> String {
  format!("fn {}", name)
}

/// The `raps` key for a path expression: the variable it names if the
/// path resolves to a local binding (`path_name`), and the name of its
/// first segment among the functions otherwise (fn items, tuple-struct
/// and unit constructors such as `None`).
pub fn path_key(p: &Path, tcx: &TyCtxt) -> String {
  match p.res {
    // A local's own name, `s₂` for a second `let s` (`binding_name`).
    Res::Local(_) => path_name(p, tcx),
    _ => fn_key(&path_name(p, tcx)),
  }
}

/// The local a path expression names (`x` resolving to a `let` or
/// pattern binding), if it is one.
pub fn local_of_path(expr: &Expr) -> Option<HirId> {
  match expr.kind {
    ExprKind::Path(QPath::Resolved(None, p)) => match p.res {
      Res::Local(hid) => Some(hid),
      _ => None,
    },
    _ => None,
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


/// Line (in the user's file) where `span` starts. Spans produced by a
/// macro expansion (`println!(..)`, `vec![..]`, ..) point into the
/// macro's definition — for std macros a line in libstd, which the
/// renderer then places far below the SVG. `source_callsite` maps
/// them back to the macro call the user wrote; plain spans are
/// returned unchanged.
pub fn span_to_line(span:&Span, tcx: &TyCtxt) -> usize{
  tcx.sess.source_map().lookup_char_pos(span.source_callsite().lo()).line
}

/// Line (in the user's file) where `span` ends; see `span_to_line`.
pub fn span_end_line(span:&Span, tcx: &TyCtxt) -> usize{
  tcx.sess.source_map().lookup_char_pos(span.source_callsite().hi()).line
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

// Render an ADT as a single owning column (like a primitive) rather
// than recursing into its struct fields when the type is defined
// outside the user's crate. RustViz is a teaching tool against a
// single-file crate — anything from `std`/`core`/`alloc` or a third-
// party dep (`String`, `Box`, `Rc`, `Arc`, `RefCell`, `Vec`,
// `HashMap`, ...) is by definition opaque from the user's
// perspective, so surfacing `r.ptr.pointer.pointer` or `r.alloc`
// columns just exposes implementation detail. Local structs the user
// defined (`Rectangle`, `Excerpt`, etc.) keep their per-field
// timeline columns, matching what #84 added.
//
// Escape hatch for "I want to expose a non-local struct's fields
// anyway" (e.g. a teaching example that wants to point at `Vec`'s
// `len` / `cap`) is tracked in #90.
pub fn ty_is_special_owner<'tcx>(t: &Ty<'tcx>) -> bool {
  if let Some(adt_def) = t.ty_adt_def() {
    return !adt_def.did().is_local();
  }
  false
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
      path_name(p, tcx)
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
    // Literal-pattern arms (`true =>`, `0 =>`). Prefer the source
    // form via span_to_snippet so the user sees `0` / `true` instead
    // of the debug-printed AST (`Int(Pu128(0), Unsuffixed)` /
    // `Bool(true)`). Fall back to the debug print when the pattern
    // came from macro expansion (no source-text view) — chiefly
    // `assert!(cond)`, which lowers to `match cond { true => {}, _ =>
    // panic!() }` with synthetic arms.
    PatKind::Expr(rustc_hir::PatExpr { kind: rustc_hir::PatExprKind::Lit { lit, .. }, .. }) => {
      if pat.span.from_expansion() {
        format!("{:?}", lit.node)
      } else {
        tcx.sess.source_map().span_to_snippet(pat.span)
          .unwrap_or_else(|_| format!("{:?}", lit.node))
      }
    }
    PatKind::Wild => {
      String::from("_")
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
    // Anything else — slice patterns, struct patterns, or-patterns,
    // ranges, refs/box/deref, etc. We're rendering this purely as a
    // human-readable arm label, so the source span is the right
    // fallback. If span_to_snippet fails (synthetic spans on macro-
    // expanded patterns we haven't classified above), drop a generic
    // placeholder rather than panic.
    _ => {
      tcx.sess.source_map().span_to_snippet(pat.span)
        .unwrap_or_else(|_| String::from("<pat>"))
    }
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

/// Walk every `PatKind::Binding` reachable inside `pat` and accumulate
/// the corresponding RAPs into `out`. Mirror of `bind_walk` in
/// visitor.rs — used by `get_decl_of_stmt` so that destructuring let
/// patterns inside a conditional body register every binding they
/// introduce, not just simple `let x = …;` shapes. Skips bindings
/// that haven't been registered yet (e.g. names skipped by
/// `visit_local`'s from_expansion guard).
pub fn collect_pat_bindings(
  pat: &Pat,
  tcx: &TyCtxt,
  raps: &HashMap<String, RapData>,
  out: &mut HashSet<ResourceAccessPoint>,
) {
  match pat.kind {
    PatKind::Binding(_, hir_id, _, sub_pat) => {
      if let Some(rd) = raps.get(&binding_name(tcx, hir_id)) {
        out.insert(rd.rap.to_owned());
        // The temporary a `let r = &…;` keeps alive is declared with r.
        if let Some(t) = rd.extended_temp.as_ref().and_then(|t| raps.get(t)) {
          out.insert(t.rap.to_owned());
        }
      }
      if let Some(sp) = sub_pat {
        collect_pat_bindings(sp, tcx, raps, out);
      }
    }
    PatKind::Tuple(pats, _) | PatKind::TupleStruct(_, pats, _) => {
      for p in pats { collect_pat_bindings(p, tcx, raps, out); }
    }
    PatKind::Struct(_, fields, _) => {
      for f in fields { collect_pat_bindings(f.pat, tcx, raps, out); }
    }
    PatKind::Or(pats) => {
      // Or-pattern alts bind the same names; walking one alt is enough.
      if let Some(first) = pats.first() {
        collect_pat_bindings(first, tcx, raps, out);
      }
    }
    PatKind::Ref(inner, _, _)
    | PatKind::Box(inner)
    | PatKind::Deref(inner)
    | PatKind::Guard(inner, _) => {
      collect_pat_bindings(inner, tcx, raps, out);
    }
    PatKind::Slice(before, mid, after) => {
      for p in before { collect_pat_bindings(p, tcx, raps, out); }
      if let Some(p) = mid { collect_pat_bindings(p, tcx, raps, out); }
      for p in after { collect_pat_bindings(p, tcx, raps, out); }
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

/// The unbound temporaries (#457) whose scope is `owner` (a block, or a
/// match arm's body expression). They're declared there like a `let`
/// binding, but dropped by their own TemporaryDrop event, not by a
/// GoOutOfScope at the end of the branch (see `needs_scope_end`).
pub fn unbound_temps_of(owner: rustc_hir::HirId, raps: &HashMap<String, RapData>) -> HashSet<ResourceAccessPoint> {
  #[expect(clippy::disallowed_methods, reason = "collected into a set; order is lost")]
  raps.values().filter(|rd| rd.temp_owner == Some(owner)).map(|rd| rd.rap.to_owned()).collect()
}

/// False for a RAP that is dropped by an event of its own: an unbound
/// temporary has no end-of-block GoOutOfScope.
pub fn needs_scope_end(r: &ResourceAccessPoint) -> bool {
  !r.is_unbound_temporary()
}

/// The bindings of the `if let` patterns down an `else if let` chain
/// (`else if let Some(y) = b { … } else if let … `), by name. A binding
/// that reuses an existing variable's name gets no RAP of its own, so
/// `raps_declared_since` can't see it as declared in the `else`.
pub fn else_if_let_bindings(expr: &Expr, tcx: &TyCtxt, raps: &HashMap<String, RapData>) -> HashSet<ResourceAccessPoint> {
  let mut res = HashSet::new();
  let mut cur = Some(expr);
  while let Some(e) = cur {
    let ExprKind::If(cond, _, els) = e.kind else { break };
    if let ExprKind::Let(let_expr) = cond.kind {
      collect_pat_bindings(let_expr.pat, tcx, raps, &mut res);
    }
    cur = els;
  }
  res
}

pub fn get_decl_of_block(block: &Block, tcx: &TyCtxt, raps: &HashMap<String, RapData>) -> HashSet<ResourceAccessPoint>{
  let mut res:HashSet<ResourceAccessPoint> = unbound_temps_of(block.hir_id, raps);
  for stmt in block.stmts.iter() {
    res.extend(get_decl_of_stmt(&stmt, tcx, raps));
  }
  res
}
// we only care about fetching the declarations in the current block, which is why these functions are not mutually recursive
pub fn get_decl_of_expr(expr: &Expr, tcx: &TyCtxt, raps: &HashMap<String, RapData>) -> HashSet<ResourceAccessPoint> {
  match expr.kind {
    ExprKind::Block(b, _) => get_decl_of_block(b, tcx, raps),
    _ => unbound_temps_of(expr.hir_id, raps) // maybe should handle match expressions as well? 
  }
}

pub fn get_decl_of_stmt(stmt: &Stmt, tcx: &TyCtxt, raps: &HashMap<String, RapData>) -> HashSet<ResourceAccessPoint> {
  let mut res = HashSet::new();
  // Synthetic stmts from macro/desugar expansion (the `let args = …`
  // emitted by `println!`, the inner Match arms of `?`, etc.) aren't
  // registered as RAPs by visit_local, so a name lookup here would
  // unwrap None. Mirror visit_local's skip on the read side.
  if stmt.span.from_expansion() { return res; }
  match stmt.kind {
    StmtKind::Let(ref local) => {
      collect_pat_bindings(&local.pat, tcx, raps, &mut res);
    }
    _ => {}
  }
  res
}

pub fn get_live_of_stmt(stmt: &Stmt, tcx: &TyCtxt, raps: &HashMap<String, RapData>) -> HashSet<ResourceAccessPoint> {
  if stmt.span.from_expansion() { return HashSet::new(); }
  match stmt.kind {
    StmtKind::Let(ref local) => {
      match local.init {
        Some(expr) => {
          get_live_of_expr(&expr, tcx, raps)
        }
        None => HashSet::new()
      }
    }
    StmtKind::Item(_item) => panic!("unsupported: items (`fn`, `struct`, `use`, ...) declared inside a function body"),
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
          // Same fallback as the Path arm in get_rap: an unknown
          // name (macro-synthetic local, `self` in an impl method,
          // etc.) becomes an empty live set rather than a crash.
          match raps.get(&path_key(p, tcx)) {
            Some(rd) => HashSet::from([rd.rap.to_owned()]),
            None => HashSet::new(),
          }
        }
      }
    }
    ExprKind::Field(inner, ident) => {
      // Live-set extraction: emit the qualified field RAP if we
      // know it; otherwise return empty rather than panic. Callers
      // accumulate live sets across an expression tree, so a
      // missing one just means we under-report liveness for that
      // sub-expression, which is preferable to crashing.
      if let Some(base) = expr_to_rap_name(inner, tcx) {
        let total_name = format!("{}.{}", base, ident.as_str());
        if let Some(rd) = raps.get(&total_name) {
          return HashSet::from([rd.rap.to_owned()]);
        }
      }
      HashSet::new()
    }
    ExprKind::AddrOf(_, _, exp) | ExprKind::Unary(_, exp)
    | ExprKind::DropTemps(exp) => {
      get_live_of_expr(exp, tcx, raps)
    }
    // `if let pat = expr` / `while let pat = expr`: the scrutinee
    // (`init`) reads variables that are live in the surrounding
    // branch. Pattern bindings declared by `pat` aren't live since
    // they don't yet exist outside the branch.
    ExprKind::Let(let_expr) => {
      get_live_of_expr(let_expr.init, tcx, raps)
    }
    ExprKind::Binary(_, lhs_expr, rhs_expr) | ExprKind::Assign(lhs_expr, rhs_expr, _) | ExprKind::AssignOp(_, lhs_expr, rhs_expr) => {
      let lhs = get_live_of_expr(&lhs_expr, tcx, raps);
      let rhs = get_live_of_expr(&rhs_expr, tcx, raps);
      &lhs | &rhs
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
                            res.extend(get_live_of_expr(&a_expr, tcx, raps));
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
                res.extend(get_live_of_expr(&a_expr, tcx, raps));
              }
            }
          }
        }
        _ => {
          for a_expr in args.iter() {
            res.extend(get_live_of_expr(&a_expr, tcx, raps));
          }
        }
      }
      res
    }
    ExprKind::MethodCall(_, rcvr, args, _) => {
      // Receiver might be anything — a literal (`"5".parse()`), a
      // chained call (`a.foo().bar()`), a desugared expression, etc.
      // Resolve via expr_to_rap_name (handles Path / Field / AddrOf
      // chains) and fall back to "no contribution from the receiver"
      // when we can't name it. We still descend into args so any user
      // variables passed to the method count toward liveness.
      let mut res = HashSet::new();
      if let Some(name) = expr_to_rap_name(rcvr, tcx) {
        if let Some(rd) = raps.get(&name) {
          res.insert(rd.rap.to_owned());
        }
      }
      for a_expr in args.iter() {
        res.extend(get_live_of_expr(&a_expr, tcx, raps));
      }
      res
    }
    // Branch expressions need to be handled a bit differently, 
    // We want the variables that are live in each block, but not the ones that were declared in 
    // the blocks (since their timelines should not be split)
    ExprKind::Block(b, _) => {
      let mut res: HashSet<ResourceAccessPoint> = HashSet::new();
      for stmt in b.stmts.iter() {
        res.extend(get_live_of_stmt(&stmt, tcx, raps));
      }
      match b.expr {
        Some(exp) => {
          res.extend(get_live_of_expr(exp, tcx, raps));
        }
        None => {}
      }
      &res - &get_decl_of_block(b, tcx, raps) // need to remove the variables that were declared in the current block 
    }
    ExprKind::If(guard_expr, if_expr, else_expr) => {
      let mut res: HashSet<ResourceAccessPoint> = HashSet::new();
      res.extend(get_live_of_expr(&guard_expr, tcx, raps));
      res.extend(get_live_of_expr(&if_expr, tcx, raps));
      match else_expr {
        Some(e) => {
          res.extend(get_live_of_expr(&e, tcx, raps));
        }
        None => {}
      }
      res
    }
    ExprKind::Tup(expr_list) => {
      let mut res: HashSet<ResourceAccessPoint> = HashSet::new();
      for e in expr_list.iter() {
        res.extend(get_live_of_expr(e, tcx, raps));
      }
      res
    }
    _ => {
      HashSet::new()
    }
  }
}

/// Walk a per-branch event list and accumulate the resource-owning
/// variables it touches. Used to build a Branch event's `live_vars`
/// from the events that actually landed inside the branches —
/// rather than from `get_live_of_expr`, which over-includes variables
/// only read by the conditional's guard expression and produces a
/// branched (and visually noisy) timeline for them. Function RAPs
/// are filtered out: they're call sites, not per-branch lifecycles.
/// Nested Branch events propagate their inner live_vars upward —
/// anything live in an inner branch is also live in the outer.
pub fn collect_event_live_vars(
    events: &[(usize, ExternalEvent)],
    out: &mut HashSet<ResourceAccessPoint>,
) {
    for (_, ev) in events {
        match ev {
            ExternalEvent::Branch { live_vars, .. } => {
                #[expect(clippy::disallowed_methods, reason = "inserted into a set; order is lost")]
                out.extend(live_vars.iter().cloned());
            }
            // Synthesized cleanup events (added after filtering) and
            // param-init events don't introduce live vars on their
            // own; the param case is handled at fn-entry, not in a
            // conditional's liveness set.
            ExternalEvent::GoOutOfScope { .. }
            | ExternalEvent::InitRefParam { .. } => {}
            ExternalEvent::LoopExitMove { ro, .. } | ExternalEvent::TupleTempDrop { ro, .. } | ExternalEvent::OrAltMove { ro, .. }
            | ExternalEvent::OrAltBorrow { ro, .. } => {
                out.insert(ro.clone());
            }
            // A reassignment in a branch drops the old value of `ro`:
            // for `*r = …` that's r's lender, which no other event in
            // the branch may mention (#267).
            ExternalEvent::OwnerDropAtReassign { ro, .. } => {
                if !ro.is_fn() {
                    out.insert(ro.clone());
                }
            }
            // A call of a closure inside the branch moves the closure.
            ExternalEvent::ClosureCall { closure: ResourceTy::Value(rap), .. } => {
                out.insert(rap.clone());
            }
            // The owner whose drop the `?` may cause is live there.
            ExternalEvent::OwnerDropIfTryReturns { ro, .. } => {
                out.insert(ro.clone());
            }
            _ => {
                let (from, to) = ResourceAccessPoint_extract(ev);
                for rty in [from, to] {
                    // A write through `*r` / `r.f` is drawn on r's
                    // column, so r is live in the branch too.
                    match rty {
                        ResourceTy::Value(rap) | ResourceTy::Deref(rap)
                        | ResourceTy::DerefField(rap, _, _) => {
                            if !rap.is_fn() && !rap.is_unbound_temporary() {
                                out.insert(rap.clone());
                            }
                        }
                        ResourceTy::Anonymous | ResourceTy::Caller | ResourceTy::Index(..)
                        | ResourceTy::Temporary { .. } => {}
                    }
                }
            }
        }
    }
}

/// True when some event in `events` (including inside nested
/// conditionals) moves a value out of a variable that isn't declared
/// in `decls`, i.e. one that outlives the conditional these events
/// belong to. An `if` without `else` whose body does this leaves the
/// variable maybe-moved afterwards (#257).
pub fn events_move_out_of_outer(
    events: &[(usize, ExternalEvent)],
    decls: &HashSet<ResourceAccessPoint>,
) -> bool {
    events.iter().any(|(_, ev)| match ev {
        ExternalEvent::Move { from: ResourceTy::Value(rap), .. }
        // An `FnOnce` call moves the closure (#271).
        | ExternalEvent::ClosureCall { closure: ResourceTy::Value(rap), .. } =>
            !rap.is_fn() && !decls.contains(rap),
        // Moved into a name an or-pattern binds if its alternative
        // matched (#533): `if let (x, _) | (_, x) = t` (#597).
        ExternalEvent::OrAltMove { ro, .. } => !decls.contains(ro),
        ExternalEvent::Branch { branches, .. } => branches.iter().any(|b| {
            let inner: HashSet<ResourceAccessPoint> = decls | &b.decl_vars;
            events_move_out_of_outer(&b.e_data, &inner)
        }),
        _ => false,
    })
}

/// The span of `expr` if it is a borrow expression (`&x`, `&mut x`),
/// also through a `DropTemps` wrapper. Used to tie a reference to the
/// MIR loan issued at that expression (#233).
pub fn borrow_span_of(expr: &Expr) -> Option<rustc_span::Span> {
  match expr.kind {
    ExprKind::AddrOf(..) => Some(expr.span),
    ExprKind::DropTemps(inner) => borrow_span_of(inner),
    _ => None,
  }
}

/// Variables whose scope already ends inside a branch's events: a
/// `for` / `while` / `loop` body's `let`s, a `for` loop's pattern
/// bindings and a `while let` binding all get their GoOutOfScope at the
/// loop body's closing brace (`emit_scope_end`, `register_iflet_let_bindings`).
/// A conditional containing such a loop treats them as declared in that
/// branch: they're not live across it, and they need no second
/// GoOutOfScope at the branch's end. The drops a `return` makes (#363,
/// `exit_drops`, by event id) are excluded: they end variables of the
/// enclosing scope too, which stay live in the branch.
pub fn scoped_in_events(events: &[(usize, ExternalEvent)], exit_drops: &HashSet<usize>) -> HashSet<ResourceAccessPoint> {
  events.iter()
    .filter(|(_, ev)| !exit_drops.contains(&ev.get_id()))
    .filter_map(|(_, ev)| ev.is_gos_ev().cloned())
    .collect()
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

// REBORROW HELPERS
//
// "Reborrow" here means borrowing *through* an existing reference:
// `&mut *r` / `&*r` with `r: &mut T` / `&T`. It is decided from the
// syntax (explicit `*` under the `&`) or from rustc's adjustments
// (implicit `Deref` + `Borrow` that the compiler inserts at coercion
// sites and method receivers), never from the source RAP's kind alone:
// `&mut r` with `r: &mut T` is a fresh borrow of the variable `r`
// (type `&mut &mut T`), not a reborrow, and `take(r)` into a generic
// `T` parameter moves `r` rather than reborrowing it.

/// Strip nested `&` / `&mut` wrappers: for `&&mut *r` returns `*r`.
pub fn innermost_addr_of_operand<'tcx>(expr: &'tcx Expr<'tcx>) -> &'tcx Expr<'tcx> {
  match expr.kind {
    ExprKind::AddrOf(_, _, inner) => innermost_addr_of_operand(inner),
    _ => expr,
  }
}

/// True iff `operand` (the expression under a `&` / `&mut`, or a
/// method receiver) is a place reached through a reference, so that
/// borrowing it is a reborrow:
/// - `*e` with `e` a reference (`&mut *r`). `*b` for a `Box` or `*s`
///   via an overloaded `Deref` impl is not.
/// - an index or field whose base is a reference, which rustc
///   auto-dereferences (#407): `&mut rv[0]` is
///   `IndexMut::index_mut(&mut *rv, 0)`, `&r[1..]` borrows through r,
///   and `&p.name` is `&(*p).name`. Also when the base is itself such
///   a place (`&mut rv[0].name`, `&(*r).name`).
/// `&mut v[0]` / `&s.name` on an owner `v` / `s` are plain borrows, and
/// so is `&(&r).s`: the reference there is a temporary made on the
/// spot, so the borrow is of r.s itself, not through a reference
/// variable.
pub fn is_explicit_reborrow(operand: &Expr, typeck: &TypeckResults) -> bool {
  match operand.kind {
    ExprKind::Unary(UnOp::Deref, inner) => typeck.expr_ty(inner).is_ref(),
    ExprKind::Index(base, _, _) | ExprKind::Field(base, _) => {
      let base_is_ref_place = typeck.expr_ty(base).is_ref() && matches!(
        base.kind,
        ExprKind::Path(_) | ExprKind::Field(..) | ExprKind::Index(..) | ExprKind::Unary(UnOp::Deref, _)
      );
      base_is_ref_place || is_explicit_reborrow(base, typeck)
    }
    _ => false,
  }
}

/// True iff rustc implicitly mutably reborrowed `expr` (a `&mut T`
/// value): its adjustments start with a deref of the reference and
/// include a `&mut` autoborrow — `f(r)` becoming `f(&mut *r)`, or
/// `r.push_str(..)` becoming `String::push_str(&mut *r, ..)`.
pub fn is_implicit_mut_reborrow(expr: &Expr, typeck: &TypeckResults) -> bool {
  if !typeck.expr_ty(expr).is_ref() {
    return false;
  }
  let adjs = typeck.expr_adjustments(expr);
  matches!(adjs.first().map(|a| &a.kind), Some(Adjust::Deref(_)))
    && adjs.iter().any(|a| matches!(
      a.kind,
      Adjust::Borrow(AutoBorrow::Ref(AutoBorrowMutability::Mut { .. }))
    ))
}

/// True iff rustc auto-referenced the method receiver `rcvr` in place:
/// `s.len()` is `String::len(&s)`, `s.push_str(t)` is
/// `String::push_str(&mut s, t)`, `v.iter()` is `<[T]>::iter(&*v)`. The
/// receiver is a place that isn't itself a reference, and its
/// adjustments include an autoborrow (`Adjust::Borrow(AutoBorrow::Ref)`,
/// possibly after auto-derefs through `Box` / `Deref` impls).
///
/// Not an autoref (returns false):
/// - `r.len()` / `r.push_str(..)` with `r: &String` / `&mut String`:
///   rustc derefs and reborrows (`&*r`, `&mut *r`); no new borrow of a
///   place is created. The `&mut` case has its own reborrow note.
/// - `(&s).len()`: the `&` is written; the receiver is a reference.
/// - `(*r).len()`: the place is reached through an explicit `*`, so
///   the inserted `&` reborrows `*r` rather than borrowing a local.
pub fn is_method_autoref(rcvr: &Expr, typeck: &TypeckResults) -> bool {
  let ty = typeck.expr_ty(rcvr);
  if ty.is_ref() || ty.is_raw_ptr() {
    return false;
  }
  if let ExprKind::Unary(UnOp::Deref, _) = rcvr.kind {
    return false;
  }
  typeck.expr_adjustments(rcvr).iter().any(|a| matches!(a.kind, Adjust::Borrow(AutoBorrow::Ref(..))))
}

/// True iff the type of `call` (a call / method-call expression)
/// mentions a lifetime, so the call's result may carry a borrow that
/// outlives the call itself (`let w = first(r);` keeps `r` unusable
/// until `w`'s last use, not just until `first` returns).
pub fn call_result_has_lifetime(call: &Expr, typeck: &TypeckResults) -> bool {
  typeck.expr_ty(call).walk().any(|arg| arg.as_region().is_some())
}

// Fetch the mutability of the borrow
pub fn fetch_mutability(expr: & Expr) -> Option<Mutability> {
  match expr.kind {
    ExprKind::Block(b, _) => {
      match b.expr {
        Some(expr) => fetch_mutability(expr),
        // An empty block (`&{}`) has no inner expression to derive
        // mutability from — propagate None so the outer `AddrOf` arm
        // falls back to its own declared mutability. Borrowing a
        // unit-valued empty block is valid Rust, so this branch is
        // reachable on real user code; don't panic. (#146)
        None => None,
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
//
// Each loan is the line interval [assigned_at, lifetime]; a loan joins
// the current region when it starts strictly before the region's end,
// and extends that end. This is a sort-then-sweep merge: the loans are
// visited by start line (then end line, then name). The previous
// version merged them one at a time in `h`'s HashSet order, which is
// random per process, so overlapping chains (a: 3-5, c: 4-7, b: 6-8)
// came out as one region on some runs and two on others, and the
// plugin drew or omitted the `c` -> lender return arrow at random
// (#211). It also overwrote an existing region that started on the
// same line as a new one that couldn't join it.
pub fn get_regions(h: &HashSet<String>, borrow_map: &HashMap<String, RefData>) -> Vec<HashSet<String>> {
  #[expect(clippy::disallowed_methods, reason = "sorted on a total key (names are unique) below")]
  let mut loans: Vec<(usize, usize, &String)> = h
    .iter()
    .map(|borrower| {
      let b_data = borrow_map.get(borrower).unwrap();
      (b_data.assigned_at, b_data.lifetime, borrower)
    })
    .collect();
  loans.sort();

  // (region end, borrowers in the region)
  let mut regions: Vec<(usize, HashSet<String>)> = Vec::new();
  for (a_place, k_place, borrower) in loans {
    match regions.last_mut() {
      // `<=`: a borrow handed over on the line where the previous
      // one ends (`r2 = t;` as t's last use) keeps the lender
      // borrowed; it isn't returned in between (#291).
      Some((k, members)) if a_place <= *k => {
        *k = max(*k, k_place);
        members.insert(borrower.clone());
      }
      _ => regions.push((k_place, HashSet::from([borrower.clone()]))),
    }
  }
  regions.into_iter().map(|(_, members)| members).collect()
}


// get the non anonymous lenders and their respective 'active' borrowers
pub fn get_non_anon_lenders(b_map: &HashMap<String, RefData>) -> HashMap<String, HashSet<String>> {
  let mut res: HashMap<String, HashSet<String>> = HashMap::new();
  #[expect(clippy::disallowed_methods, clippy::iter_over_hash_type, reason = "builds a map of sets; order is lost")]
  for (k, v) in b_map.iter() {
    // co_lenders: a reference returned by `longest(&a, &b)` is an
    // active borrower of both `a` and `b` (#171).
    for l in v.all_lenders() {
      match l {
        ResourceTy::Anonymous | ResourceTy::Temporary { .. } => {},
        _ => {
          let lender = l.real_name();
          res.entry(lender.clone())
            .and_modify(|lendees| { lendees.insert(k.to_owned()); })
            .or_insert(HashSet::from([k.to_owned()]));
        }
      }
    }
  }
  res
}

// get borrowers associated with a single lender
pub fn get_borrowers(borrower: &String, borrow_map: &HashMap<String, RefData>) -> HashSet<String> {
  let lender = borrow_map.get(borrower).unwrap().lender.clone();
  match lender {
    ResourceTy::Anonymous | ResourceTy::Caller | ResourceTy::Temporary { .. } => HashSet::from([borrower.to_owned()]),
    _ => {
      let mut res:HashSet<String> = HashSet::new();
      #[expect(clippy::disallowed_methods, clippy::iter_over_hash_type, reason = "builds a set; order is lost")]
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
    ResourceTy::Anonymous | ResourceTy::Caller | ResourceTy::DerefField(..) | ResourceTy::Temporary { .. } => VecDeque::new(),
    ResourceTy::Value(x) | ResourceTy::Deref(x) | ResourceTy::Index(x, _, _) => {
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
/// `a <op> b` where `<op>` is an overloaded operator whose trait method
/// takes its operands by value (`String + &str` calls
/// `<String as Add<&str>>::add(self, rhs)`). Returns the operator's
/// source text, which is used as the callee's name. Comparisons
/// (`==`, `<`, ...) take their operands by reference and built-in
/// operators on primitives aren't method calls; both return `None`.
pub fn overloaded_by_value_binop<'tcx>(expr: &Expr<'tcx>, tcx: &TyCtxt<'tcx>) -> Option<&'static str> {
  match expr.kind {
    ExprKind::Binary(op, ..)
      if op.node.is_by_value() && tcx.typeck(expr.hir_id.owner).is_method_call(expr) =>
        Some(op.node.as_str()),
    _ => None,
  }
}

/// `a == b`, `a < b`, … where the comparison is a call to a trait
/// method, i.e. the operands aren't primitives (`String`, `Vec`, a
/// struct with `#[derive(PartialEq)]`). Such a comparison is
/// `PartialEq::eq(&a, &b)` / `PartialOrd::lt(&a, &b)`: rustc inserts a
/// `&` on each operand (an autoref adjustment), so both are borrowed
/// for the call and neither is moved (#366). Returns the operator's
/// text and the operands written in the source (not inside a macro's
/// expansion), in source order. Built-in comparisons on primitives
/// (`i < n` on integers) aren't method calls and return `None`.
pub fn overloaded_comparison<'tcx>(
  expr: &'tcx Expr<'tcx>,
  tcx: &TyCtxt<'tcx>,
) -> Option<(&'static str, Vec<&'tcx Expr<'tcx>>)> {
  let ExprKind::Binary(op, lhs, rhs) = expr.kind else { return None };
  if op.node.is_by_value() || !tcx.typeck(expr.hir_id.owner).is_method_call(expr) {
    return None;
  }
  let operands = [lhs, rhs].into_iter().filter(|e| !e.span.from_expansion()).collect();
  Some((op.node.as_str(), operands))
}

/// True iff rustc inserted a shared borrow (`&`) of `e` (an autoref
/// adjustment), as it does for the operands of an overloaded
/// comparison.
pub fn is_auto_borrowed(e: &Expr, typeck: &TypeckResults) -> bool {
  typeck.expr_adjustments(e).iter().any(|a| matches!(a.kind, Adjust::Borrow(AutoBorrow::Ref(AutoBorrowMutability::Not))))
}

/// `a <op> b` where `<op>` is a built-in arithmetic / bitwise /
/// comparison operator on primitives (`x + y`, `a.x * 2`, `n % 2`,
/// `t < lim` with i32 operands), i.e. not a method call. Every
/// primitive is `Copy`, so each operand is read by copying it into the
/// operator (#241, #497; the comparison's trait method takes `&`, but
/// a built-in one never borrows anything). Returns the operator's text plus
/// the operands that name a column (a local, a struct field, or `*r`),
/// in source order; literals, calls, nested operators and indexing
/// (`v[i]` copies an element, not `v`) are left out. The
/// short-circuiting `&&` / `||` return `None`.
pub fn builtin_binop_copied_operands<'tcx>(
  expr: &'tcx Expr<'tcx>,
  tcx: &TyCtxt<'tcx>,
  raps: &HashMap<String, RapData>,
) -> Option<(&'static str, Vec<&'tcx Expr<'tcx>>)> {
  let ExprKind::Binary(op, lhs, rhs) = expr.kind else { return None };
  if op.node.is_lazy() || tcx.typeck(expr.hir_id.owner).is_method_call(expr) {
    return None;
  }
  let operands: Vec<&'tcx Expr<'tcx>> = [lhs, rhs].into_iter()
    .filter(|e| operand_names_column(e, tcx, raps))
    .collect();
  if operands.is_empty() { None } else { Some((op.node.as_str(), operands)) }
}

/// An operand of a built-in operator that is drawn as copied in: one
/// that names a column (a local, a struct field, or `*r`). Literals,
/// calls, nested operators and indexing (`v[i]` copies an element, not
/// `v`) have no column of their own.
pub fn operand_names_column(e: &Expr, tcx: &TyCtxt, raps: &HashMap<String, RapData>) -> bool {
  !e.span.from_expansion()
    && match e.kind {
      ExprKind::Path(_) | ExprKind::Field(..) => true,
      ExprKind::Unary(UnOp::Deref, inner) => matches!(inner.kind, ExprKind::Path(_) | ExprKind::Field(..)),
      _ => false,
    }
    && !matches!(get_rap(e, tcx, raps), ResourceTy::Anonymous)
}

/// `-x` / `!b`. Like its trait method (`Neg::neg(self)`,
/// `Not::not(self)`), a negation takes its operand by value. Returns
/// the operator's text and the operand when there's something to draw:
/// - an overloaded operator is a method call, and its operand is an
///   argument like any other (moved, copied, or borrowed for `-&x`);
/// - a built-in one on a primitive copies its operand in, which is
///   drawn when the operand names a column (see `operand_names_column`).
pub fn unary_op_operand<'tcx>(
  expr: &'tcx Expr<'tcx>,
  tcx: &TyCtxt<'tcx>,
  raps: &HashMap<String, RapData>,
) -> Option<(&'static str, &'tcx Expr<'tcx>)> {
  let ExprKind::Unary(op @ (UnOp::Neg | UnOp::Not), operand) = expr.kind else { return None };
  if tcx.typeck(expr.hir_id.owner).is_method_call(expr) || operand_names_column(operand, tcx, raps) {
    Some((op.as_str(), operand))
  } else {
    None
  }
}

/// What a call `callee(args)` returns, as far as the `raps` map goes,
/// when `callee` might be a local variable. See `local_callee_result_key`.
pub enum LocalCallee {
  /// The callee isn't a local variable (a fn item, a path to a
  /// constructor, a method, ...): look the call up by its callee's name.
  NotLocal,
  /// A local whose call leaves it in place (an `Fn` / `FnMut` closure,
  /// a fn pointer or item bound to a local): the result comes from the
  /// call, i.e. the Function RAP under this key (`fn_key("f")`).
  Call(String),
  /// A local that the call consumes (`FnOnce::call_once`, e.g. a
  /// closure that moves a captured value out): the value comes out of
  /// the variable itself, which is moved.
  Consumed,
}

/// Classify the callee of `call` (an `ExprKind::Call`). Only a path to
/// a local binding (`Res::Local`) counts as local.
pub fn local_callee_result_key(call: &Expr, tcx: &TyCtxt) -> LocalCallee {
  let ExprKind::Call(callee, _) = call.kind else { return LocalCallee::NotLocal };
  let ExprKind::Path(QPath::Resolved(None, path)) = callee.kind else { return LocalCallee::NotLocal };
  if !matches!(path.res, Res::Local(_)) {
    return LocalCallee::NotLocal;
  }
  // rustc records a call through a closure or a generic `F: Fn*` as a
  // call of the trait method it resolved to; `call_once` takes the
  // callee by value.
  let typeck = tcx.typeck(call.hir_id.owner);
  if let Some(method) = typeck.type_dependent_def_id(call.hir_id) {
    if matches!(tcx.trait_of_assoc(method), Some(t) if Some(t) == tcx.lang_items().fn_once_trait()) {
      return LocalCallee::Consumed;
    }
  }
  LocalCallee::Call(fn_key(tcx.hir_name(path.segments[0].hir_id).as_str()))
}

/// Which `Rc` / `Arc` / `Weak` kind `ty` is, by std diagnostic item.
fn rc_kind_of_ty(tcx: &TyCtxt, ty: rustc_middle::ty::Ty) -> Option<RcKind> {
  use rustc_span::sym;
  let did = ty.ty_adt_def()?.did();
  if tcx.is_diagnostic_item(sym::Rc, did) { Some(RcKind::Rc) }
  else if tcx.is_diagnostic_item(sym::Arc, did) { Some(RcKind::Arc) }
  else if tcx.is_diagnostic_item(sym::RcWeak, did) { Some(RcKind::RcWeak) }
  else if tcx.is_diagnostic_item(sym::ArcWeak, did) { Some(RcKind::ArcWeak) }
  else { None }
}

/// Is the call `call` (a `Call` or `MethodCall`) to `<Rc<T> as
/// Clone>::clone` / `<Arc<T> as Clone>::clone`, or to `Rc::downgrade` /
/// `Arc::downgrade`? Decided from the resolved callee and its `Self`
/// type, not from the name written (#373): a user's own `clone` method,
/// or `String::clone`, is not one.
pub fn rc_call_of_expr(call: &Expr, tcx: &TyCtxt) -> Option<RcCall> {
  let typeck = tcx.typeck(call.hir_id.owner);
  let (callee_id, def_id) = match call.kind {
    ExprKind::MethodCall(..) => (call.hir_id, typeck.type_dependent_def_id(call.hir_id)?),
    ExprKind::Call(fn_expr, _) => {
      let ExprKind::Path(ref qpath) = fn_expr.kind else { return None };
      match typeck.qpath_res(qpath, fn_expr.hir_id) {
        rustc_hir::def::Res::Def(_, def_id) => (fn_expr.hir_id, def_id),
        _ => return None,
      }
    }
    _ => return None,
  };
  if tcx.lang_items().clone_fn() == Some(def_id) {
    // The trait method `Clone::clone`; its `Self` is the first generic arg.
    let self_ty = typeck.node_args(callee_id).types().next()?;
    return rc_kind_of_ty(tcx, self_ty).map(RcCall::Clone);
  }
  // An inherent method of `Rc` / `Arc`: the impl's self type says which.
  if tcx.item_name(def_id).as_str() == "downgrade" {
    let impl_id = tcx.opt_parent(def_id)?;
    if !matches!(tcx.def_kind(impl_id), rustc_hir::def::DefKind::Impl { .. }) { return None; }
    return rc_kind_of_ty(tcx, tcx.type_of(impl_id).skip_binder()).map(RcCall::Downgrade);
  }
  None
}

/// The function RAP for `call`'s result, with `Rc` clone / downgrade
/// calls marked as such.
pub fn fn_result_rap(call: &Expr, rap: ResourceAccessPoint, tcx: &TyCtxt) -> ResourceAccessPoint {
  match rc_call_of_expr(call, tcx) {
    Some(rc_call) => rap.with_rc_call(rc_call),
    None => rap,
  }
}

pub fn get_rap(expr: &Expr, tcx: &TyCtxt, raps: &HashMap<String, RapData>) -> ResourceTy {
  match expr.kind {
    ExprKind::Path(QPath::Resolved(_,p)) => {
      let name = path_key(p, tcx);
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
    // `v[..]` / `s[i]`: the resource is the receiver. See `expr_to_rap_name`.
    ExprKind::Index(recv, _, _) => get_rap(recv, tcx, raps),
    // For Call / MethodCall, we want the fn's RAP — but desugarings
    // produce Calls whose fn_expr is a `QPath::LangItem` (e.g. the
    // `Try::branch(…)` scrutinee of `?`) that hirid_to_var_name can't
    // name, and macro expansions produce calls whose fn_name was
    // never registered via add_fn (visit_expr returns early on
    // from_expansion). Both situations should fall back to Anonymous
    // rather than crash, mirroring the Path / Field arms above.
    ExprKind::Call(fn_expr, _) => {
      // A call through a local (`f(1)` with `f` a closure or fn
      // pointer) yields the call's result, not `f`'s own value: name
      // the call's Function RAP, never the variable (#254, #299).
      let key = match local_callee_result_key(expr, tcx) {
        LocalCallee::Call(key) => Some(key),
        // The variable itself (`f₂` for a shadowing `let f`).
        LocalCallee::Consumed => expr_to_rap_name(fn_expr, tcx),
        LocalCallee::NotLocal => hirid_to_var_name(fn_expr.hir_id, tcx).map(|n| fn_key(&n)),
      };
      key.and_then(|n| raps.get(&n).map(|rd| fn_result_rap(expr, rd.rap.to_owned(), tcx)))
        .map_or(ResourceTy::Anonymous, ResourceTy::Value)
    }
    ExprKind::MethodCall(name_and_generic_args, ..) => {
      hirid_to_var_name(name_and_generic_args.hir_id, &tcx)
        .and_then(|n| raps.get(&fn_key(&n)).map(|rd| fn_result_rap(expr, rd.rap.to_owned(), tcx)))
        .map_or(ResourceTy::Anonymous, ResourceTy::Value)
    }
    ExprKind::Block(b, _) => {
      match b.expr {
        Some(expr) => { get_rap(expr, tcx, raps) }
        // Empty block — no value, no RAP. Used to panic; Anonymous
        // is the right neutral fallback here too.
        None => ResourceTy::Anonymous,
      }
    }
    ExprKind::Field(inner, ident) => {
      // Walk the receiver to a qualified name, then look it up. Any
      // shape we can't name (call result, block, etc.) — or a name
      // we don't have a RAP for — falls back to Anonymous so the
      // surrounding event still records.
      if let Some(rap) = field_rap(inner, ident.as_str(), tcx, raps) {
        return ResourceTy::Value(rap);
      }
      ResourceTy::Anonymous
    }
    _ => ResourceTy::Anonymous
  }
}

/// RAP for the field projection `inner.field`, if we have one. Struct
/// fields have their own `base.field` RAP. Union fields don't: a union
/// is rendered as one opaque owner column (#131), so reading or
/// borrowing `u._a` is attributed to `u` itself.
pub fn field_rap(inner: &Expr, field: &str, tcx: &TyCtxt, raps: &HashMap<String, RapData>) -> Option<ResourceAccessPoint> {
  let base = expr_to_rap_name(inner, tcx)?;
  if let Some(rd) = raps.get(&format!("{}.{}", base, field)) {
    return Some(rd.rap.to_owned());
  }
  if tcx.typeck(inner.hir_id.owner).node_type(inner.hir_id).is_union() {
    return raps.get(&base).map(|rd| rd.rap.to_owned());
  }
  None
}

// Almost the same as get_rap but we don't care about any anonymous owners
// which is why we return a RAP instead of a ResourceTy
pub fn fetch_rap(expr: &Expr, tcx: &TyCtxt, raps: &HashMap<String, RapData>) -> Option<ResourceAccessPoint> {
  match expr.kind {
    ExprKind::Call(..) | ExprKind::Binary(..) | ExprKind::Lit(_) | ExprKind::MethodCall(..) => None,
    ExprKind::Path(QPath::Resolved(_,p)) => {
      // Path expression that resolves to a name we never registered —
      // most commonly a synthetic local from a macro expansion, or a
      // LangItem qualifier reached via desugaring. Return None rather
      // than crash; callers map None to "skip this event".
      raps.get(&path_key(p, tcx)).map(|rd| rd.rap.to_owned())
    }
    ExprKind::AddrOf(_, _, expr) | ExprKind::Unary(_, expr) => fetch_rap(expr, tcx, raps),
    // `v[..]` / `s[i]`: the resource is the receiver. See `expr_to_rap_name`.
    ExprKind::Index(recv, _, _) => fetch_rap(recv, tcx, raps),
    ExprKind::Block(b, _) => {
      match b.expr {
        Some(expr) => { fetch_rap(expr, tcx, raps) }
        // Empty block — no value to fetch.
        None => None,
      }
    }
    ExprKind::Field(inner, ident) => {
      // Same logic as get_rap's Field arm, but return None instead
      // of Anonymous since fetch_rap callers don't carry a separate
      // "anonymous" sentinel.
      field_rap(inner, ident.as_str(), tcx, raps)
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
      // const / static items aren't registered as RAPs — they're
      // compile-time values / `'static` references baked into the
      // binary. Treating them as `Anonymous` (the "external"
      // source we already use for literals and fn-return values)
      // gives `let s = GREETING;` a clean shape: s borrows from
      // an external source rather than from a function-local
      // lender.
      if matches!(
        p.res,
        rustc_hir::def::Res::Def(
          rustc_hir::def::DefKind::Const { .. }
            | rustc_hir::def::DefKind::AssocConst { .. }
            | rustc_hir::def::DefKind::Static { .. },
          _
        )
      ) {
        return ResourceTy::Anonymous;
      }
      let name = path_key(p, tcx);
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
    // `&v[..]` / `&s[i..j]`: the lender is the receiver, the same way
    // `&r.field` resolves through the Field arm.
    ExprKind::Index(recv, _, _) => {
      find_lender(recv, tcx, raps, borrow_map)
    }
    ExprKind::Block(b, _) => {
      match b.expr {
        Some(expr) => find_lender(expr, tcx, raps, borrow_map),
        // Borrow of an empty block — `let r = &{};` borrows unit.
        // No lender to attribute to; same `&{}` shape as #146's
        // fetch_mutability fix.
        None => ResourceTy::Anonymous,
      }
    }
    ExprKind::Unary(op, expr) => { 
      match op {
        rustc_hir::UnOp::Deref => {
          match find_lender(expr, tcx, raps, borrow_map) {
            ResourceTy::Deref(r) | ResourceTy::Value(r) => {
              // `*rr` where rr is a borrower: what rr's target
              // borrows. When rr isn't a borrower (`*rr[0]` with
              // rr: `Vec<&&i32>`: the inner reference is stored in
              // a collection, so which variable it points to isn't
              // known statically; #453) there is no lender to name.
              match borrow_map.get(r.name()) {
                Some(data) => data.lender.to_owned(),
                None => ResourceTy::Anonymous,
              }
            }
            _ => { ResourceTy::Anonymous }
          }
        },
        _ => {
          find_lender(expr, tcx, raps, borrow_map)
        }
      }
    }
    ExprKind::Field(inner, ident) => {
      // Walk to a qualified name; check borrow_map first (the field
      // might itself be a registered borrower with a known lender),
      // then fall back to raps. Unknown names return Anonymous so
      // the surrounding `let p = &r.unknown.field;` still records a
      // borrow site, just without a recorded lender.
      if let Some(base) = expr_to_rap_name(inner, tcx) {
        let name = format!("{}.{}", base, ident.as_str());
        if let Some(rd) = borrow_map.get(&name) {
          return rd.to_owned().lender;
        }
      }
      // `base.field`'s own RAP, or the union itself for `&u._a`.
      if let Some(rap) = field_rap(inner, ident.as_str(), tcx, raps) {
        return ResourceTy::Value(rap);
      }
      ResourceTy::Anonymous
    }
  _ => ResourceTy::Anonymous,
  }
}
/// Where each value an `FnOnce` closure captured by value ends up when
/// the closure is called (#271), read from the closure body's MIR.
/// `body` is the closure's own MIR, whose first argument (`_1`) is the
/// closure value itself, passed by value because the closure is
/// `FnOnce`; its fields are the captures, in the order of
/// `closure_min_captures_flattened`. Entry `i` describes capture `i`:
///
/// * `Returned`: the body's only use of it is to return it.
/// * `Dropped`: the body passes it to `mem::drop`, or never moves it
///   out, so it is dropped with the rest of the closure's environment
///   when the call ends.
/// * `Moved`: it is moved somewhere else, or moved on only some paths
///   (the body has a branch), so all that can be said is that the call
///   uses it up.
pub fn closure_capture_ends<'tcx>(
  tcx: TyCtxt<'tcx>,
  body: &rustc_middle::mir::Body<'tcx>,
  n_captures: usize,
) -> Vec<crate::svg_generator::data::CapturedEnd> {
  use crate::svg_generator::data::CapturedEnd;
  use rustc_middle::mir::{Local, Operand, Place, PlaceElem, Rvalue, StatementKind, TerminatorKind, RETURN_PLACE};

  let all_moved = || vec![CapturedEnd::Moved; n_captures];
  let env = Local::from_u32(1);
  // Not an owned environment: not a by-value `FnOnce` body.
  if body.arg_count < 1 || body.local_decls[env].ty.is_ref() {
    return all_moved();
  }
  // The capture a whole-field place stands for.
  fn field_of(place: &Place<'_>, env: Local) -> Option<(usize, bool)> {
    if place.local != env {
      return None;
    }
    match place.projection.first() {
      Some(PlaceElem::Field(f, _)) => Some((f.index(), place.projection.len() == 1)),
      _ => None,
    }
  }
  // Locals holding a captured value moved out of the environment
  // (`_3 = move (_1.0)`), and which capture, to a fixpoint.
  let mut alias: HashMap<Local, usize> = HashMap::new();
  loop {
    let before = alias.len();
    for bb in body.basic_blocks.iter() {
      for st in &bb.statements {
        if let StatementKind::Assign(asg) = &st.kind {
          let (dest, rv) = &**asg;
          if let Rvalue::Use(Operand::Move(src)) = rv {
          if dest.projection.is_empty() && dest.local != RETURN_PLACE {
            let i = match field_of(src, env) {
              Some((i, true)) => Some(i),
              _ if src.projection.is_empty() => alias.get(&src.local).copied(),
              _ => None,
            };
            if let Some(i) = i {
              alias.insert(dest.local, i);
            }
          }
          }
        }
      }
    }
    if alias.len() == before {
      break;
    }
  }
  // Which capture a moved operand carries, and whether it moved only
  // part of it.
  let carried = |op: &Operand<'tcx>| -> Option<(usize, bool)> {
    let Operand::Move(p) = op else { return None };
    match field_of(p, env) {
      Some(x) => Some(x),
      None if p.projection.is_empty() => alias.get(&p.local).map(|i| (*i, true)),
      None => alias.get(&p.local).map(|i| (*i, false)),
    }
  };
  let mut ends: Vec<Option<CapturedEnd>> = vec![None; n_captures];
  let mut mark = |i: usize, e: CapturedEnd| {
    if let Some(slot) = ends.get_mut(i) {
      *slot = Some(match slot { None => e, Some(old) if *old == e => e, Some(_) => CapturedEnd::Moved });
    }
  };
  let mut has_branch = false;
  for bb in body.basic_blocks.iter() {
    for st in &bb.statements {
      let StatementKind::Assign(asg) = &st.kind else { continue };
      let (dest, rv) = &**asg;
      if let Rvalue::Use(op) = rv {
        if let Some((i, whole)) = carried(op) {
          if dest.projection.is_empty() && dest.local != RETURN_PLACE {
            continue; // an alias, followed above
          }
          if whole && dest.local == RETURN_PLACE && dest.projection.is_empty() {
            mark(i, CapturedEnd::Returned);
          } else {
            mark(i, CapturedEnd::Moved);
          }
        }
        continue;
      }
      let ops: Vec<&Operand<'tcx>> = match rv {
        Rvalue::Repeat(o, _) | Rvalue::Cast(_, o, _) | Rvalue::UnaryOp(_, o) => vec![o],
        Rvalue::BinaryOp(_, ab) => vec![&ab.0, &ab.1],
        Rvalue::Aggregate(_, ops) => ops.iter().collect(),
        _ => vec![],
      };
      for o in ops {
        if let Some((i, _)) = carried(o) {
          mark(i, CapturedEnd::Moved);
        }
      }
    }
    match &bb.terminator().kind {
      TerminatorKind::Call { func, args, .. } => {
        let is_mem_drop = func.const_fn_def()
          .map_or(false, |(def_id, _)| tcx.is_diagnostic_item(rustc_span::sym::mem_drop, def_id));
        for a in args {
          if let Some((i, whole)) = carried(&a.node) {
            mark(i, if is_mem_drop && whole { CapturedEnd::Dropped } else { CapturedEnd::Moved });
          }
        }
      }
      TerminatorKind::TailCall { args, .. } => {
        for a in args {
          if let Some((i, _)) = carried(&a.node) {
            mark(i, CapturedEnd::Moved);
          }
        }
      }
      TerminatorKind::SwitchInt { .. } => has_branch = true,
      _ => {}
    }
  }
  ends.into_iter()
    .map(|e| match e {
      None => CapturedEnd::Dropped,
      Some(_) if has_branch => CapturedEnd::Moved,
      Some(e) => e,
    })
    .collect()
}

/// For an expression expanded from `vec![…]`, the elements the user
/// wrote: `vec![s, t]` (an array literal inside the expansion) gives
/// `[s, t]`, `vec![x; n]` (`from_elem(x, n)`) gives `[x]`. `None` for
/// anything else, including `vec![]`.
pub fn vec_macro_elems<'tcx>(expr: &'tcx Expr<'tcx>) -> Option<&'tcx [Expr<'tcx>]> {
  let from_vec = expr.span.macro_backtrace().any(|d| {
    matches!(d.kind, rustc_span::hygiene::ExpnKind::Macro(_, name) if name.as_str() == "vec")
  });
  if !from_vec {
    return None;
  }
  fn find<'tcx>(e: &'tcx Expr<'tcx>) -> Option<&'tcx [Expr<'tcx>]> {
    if !e.span.from_expansion() {
      return None;
    }
    match e.kind {
      ExprKind::Array(elems) if elems.iter().all(|x| !x.span.from_expansion()) => Some(elems),
      ExprKind::Call(f, args) => {
        let is_from_elem = matches!(f.kind, ExprKind::Path(QPath::Resolved(_, p))
          if p.segments.last().map_or(false, |s| s.ident.as_str() == "from_elem"));
        if is_from_elem && !args.is_empty() && !args[0].span.from_expansion() {
          Some(&args[..1])
        } else {
          args.iter().find_map(find)
        }
      }
      ExprKind::Block(b, _) => b.expr.and_then(find),
      ExprKind::AddrOf(_, _, inner) | ExprKind::Unary(_, inner) | ExprKind::DropTemps(inner) => find(inner),
      _ => None,
    }
  }
  find(expr)
}
