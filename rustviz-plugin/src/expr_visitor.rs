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
  /// For a reference that got its value from a call (`id(&p)`,
  /// `v.first()`), or took over such a reference's loan (see
  /// `ExprVisitor::ref_carriers`): the line that issued the loan it
  /// holds. `print_lifetimes` matches it on (this line, lender), as it
  /// does a reference's own assignment line (#664).
  pub loan_line: Option<usize>,
  // Further lenders, for a reference returned by a call whose result
  // is tied to several arguments (`longest(&a, &b)` borrows from both
  // `a` and `b`; #171). Empty for every other reference.
  pub co_lenders: Vec<ResourceTy>,
  /// For a holder whose value mixes `&` and `&mut` borrows
  /// (`Some(W { a: &s, b: &mut t })`, #708): whether each lender is
  /// borrowed mutably. Empty for every other reference, whose lenders
  /// all share `ref_mutability`.
  pub lender_muts: Vec<(ResourceTy, bool)>,
}

/// The loan a reference holds, as `print_lifetimes` finds it in MIR's
/// borrow data (see `RefData::borrow_span` and `RefData::loan_line`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoanId {
  /// Made by the `&x` / `&mut x` expression at this span.
  Span(rustc_span::Span),
  /// Issued for the reference's lender on this line.
  Line(usize),
}

impl RefData {
  pub fn loan(&self) -> Option<LoanId> {
    self.borrow_span.map(LoanId::Span).or(self.loan_line.map(LoanId::Line))
  }

  /// Whether `lender`'s borrow by this reference is a mutable one.
  pub fn mut_of(&self, lender: &ResourceTy) -> bool {
    self.lender_muts.iter().find(|(l, _)| l == lender).map_or(self.ref_mutability, |(_, m)| *m)
  }

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
  // Same-named variables of sibling blocks share one column (#308);
  // `scope` is the first one's block end, and this holds the block ends
  // of the later ones, each of which also ends the variable (#589).
  pub extra_scopes: Vec<usize>,
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
  /// `ref` / `ref mut` bindings out of a tuple-literal scrutinee's
  /// element (#580): they borrow the temporary tuple, which has no
  /// column, so their borrow's end is drawn as a plain end dot with no
  /// arrow (#658).
  pub tuple_temp_refs: HashSet<String>,
  /// References a pattern copied (or moved) out of a value that holds
  /// a borrow (`Some(n)` on `it.next()`, #391): their borrow is part of
  /// that value's, and ends at their own last use (#575).
  pub pattern_copies: HashSet<String>,
  // While the pattern of `for (a, b) in &v` is registered: the borrow
  // of the head that each reference binding is (#576).
  pub pattern_lender: Option<(Evt, ResourceTy)>,
  // Temporaries given a column of their own in this fn body because a
  // `let` borrows them (`let r = &String::from("a");`, #400); numbers
  // the next one's label.
  pub temp_count: usize,
  /// By-value parameter values given a `(parameter N)` column (#691).
  pub param_value_count: usize,
  /// The `ref` bindings' borrows of those columns, added after the
  /// last parameter (line, mutable, borrower, lender).
  pub pending_param_borrows: Vec<(usize, bool, ResourceTy, ResourceTy)>,

  // Calls whose temporary result's drop has been stated (#457), so a
  // call the visitor reaches twice says it once.
  pub temp_drops_noted: HashSet<rustc_hir::HirId>,
  // Scrutinee temporaries of an `if let` / let chain that are dropped
  // before its `else` block runs (edition 2024, #469, #604). They are
  // never split into a conditional's branches: their column ends at
  // that drop, and a drop at an early exit inside the matched block is
  // a dot on it (see `take_branch_events`).
  pub unbranched_temps: HashSet<ResourceAccessPoint>,
  // The variables moved into the `for` loops being visited as their
  // iterators (`for s in it`, `LoopHead::Iterator`), innermost last: a
  // `return` inside drops each loop's iterator (#482), and so does a
  // `break 'outer` out of the loops inside `'outer` (#592). Each is
  // paired with its loop's id (the desugared `loop` expression).
  pub loop_iterators: Vec<(rustc_hir::HirId, ResourceAccessPoint)>,
  // The `(temporary N)` column of each unbound temporary that a field
  // is moved out of (`make_q().a`), by the expression that makes it
  // (#469).
  pub temp_cols: HashMap<rustc_hir::HirId, ResourceAccessPoint>,
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
  /// A reference destination's source reference and the loan it
  /// holds (see `ref_carriers`), with whether the same statement
  /// reassigns that source (`m2` in `(m1, m2) = (m2, m1)`).
  pub destructure_ref_src: HashMap<rustc_hir::HirId, (String, LoanId, bool)>,
  /// (carrier, source, loan): `let h = o;` / `h = o;` with `o` a
  /// reference made by `&p` (or by a call, `id(&p)`, #664). `h` holds
  /// the same loan. When `o` is then given another value (`o = …`, or
  /// `*w = …` through a `w = &mut o`), the borrow checker still keeps
  /// that loan live while `o` is live (`o`'s type has one lifetime),
  /// so `h` takes the loan over: `print_lifetimes` ends `h`'s borrow
  /// where rustc ends the loan, not at `h`'s last use (#644).
  pub ref_carriers: Vec<(String, String, LoanId)>,
  /// References holding a loan they took over (see `ref_carriers`).
  /// Copying or moving one on (`q = t;` after `let t = p; p = q;`)
  /// hands the loan to the new holder right away: it's the one that
  /// keeps the loan live, and `t`'s borrow ends at `t`'s last use.
  pub taken_over_loans: HashSet<String>,


  // Variables moved in a branch that then `break`s out of a loop
  // (#344), recorded when the branch is built; the loop turns them
  // into `LoopExitMove` events at its end.
  pub break_moves: Vec<BreakMove>,
  // Set when a conditional directly in the function's body has every
  // branch leaving the function (`return`, panic): the closing brace
  // is unreachable, so it drops nothing (#515).
  pub all_paths_exit: bool,

  // Earliest line a reference's borrow may be drawn ending at, keyed
  // by the reference's name (#372). A `match` arm binding that
  // reborrows through a reference (`match o { Some(s) => … }`, `o:
  // &Option<String>`) is taken out of `borrow_map` when its arm ends,
  // so `print_lifetimes` can't see that `o`'s borrow has to last
  // until then; the arm records it here instead.
  pub borrow_floor: HashMap<String, usize>,
  // `let r = loop { … break k; };` (#431): the loop's id, what its
  // `break` values go to (`r`) and how (move or copy). A `break` with a
  // value, in that loop, pairs the value with `r` the way `let r = k;`
  // would.
  pub break_binds: Vec<(rustc_hir::HirId, ResourceTy, Evt)>,
  // A tuple/struct pattern bound to a loop's value: bound against each
  // `break` value (#452).
  pub break_pats: Vec<(rustc_hir::HirId, &'tcx Pat<'tcx>, usize, bool)>,
  // `break` values seen in a loop whose receiver isn't known yet (the
  // loop is the right-hand side of an assignment, a tuple pattern, an
  // argument…): `match_rhs` pairs them once it is.
  // With each `break` as written when the loop has several (#561).
  pub loop_breaks: Vec<(rustc_hir::HirId, &'tcx Expr<'tcx>, Option<String>)>,
  // A `break value` in a branch of a loop (`if c { break s; }`) leaves
  // the loop, so its value reaches the receiver where the loop ends,
  // not in the branch (#350's approach to loop exits): the loop's id
  // and the value, paired at the loop's end. So are all of them when
  // the loop has several (#561); the text is the `break` as written,
  // for tooltips (see BreakAlt).
  pub deferred_breaks: Vec<(rustc_hir::HirId, &'tcx Expr<'tcx>, Option<String>)>,
  // While set, events are placed on this line (the end of the loop
  // whose `break` values are being paired).
  pub line_override: Option<usize>,
  // A loop whose `break` value was deferred to its end but whose
  // receiver isn't known yet (`r = loop`, `f(loop { … })`): its id,
  // the line it ends on and the names `break_out` marks; `match_rhs` /
  // `match_arg` pair the value there once the receiver is known.
  pub deferred_lines: Vec<(rustc_hir::HirId, usize, Vec<String>)>,
  // While pairing a deferred `break` value: the variables it moves
  // that are declared inside the loop, on a loop whose body can reach
  // its end. Their Move is marked `break_out` (see ExternalEvent::Move).
  pub break_out: Vec<String>,
  // While set, the loop whose `break` values are being paired has
  // several of them, so each Move / Copy / Bind into its receiver
  // happened only if its `break` ran (#561).
  pub break_alt: Option<BreakAlt>,

  // Names of the variables in scope right now, in declaration order
  // (#410). `add_rap` pushes; a block, a match arm, an `if let` and a
  // loop truncate it back when they end, which takes their own
  // variables (pattern bindings included) out of scope.
  pub in_scope: Vec<String>,

  // The constructs a `return`, `break`, `continue` or panic leaves,
  // innermost last, each with `in_scope.len()` where it began: an
  // if/else or a match (whose branch the exit is in), and loops, by
  // the HirId a `break` / `continue` names as its target.
  pub exit_frames: Vec<ExitFrame>,
  // The `(temporary N)` RAP of a `let` whose initializer is a tuple /
  // struct / array literal that the pattern doesn't fully take by value
  // (`let (ref a, ..) = (s, t);`, #439), by the `let`'s HirId.
  pub literal_temps: HashMap<rustc_hir::HirId, ResourceAccessPoint>,
  // The elements of such a literal already moved into its temporary.
  pub premoved: HashSet<rustc_hir::HirId>,

  // The tuple-literal scrutinees (`match (x, y)`, `if let … = (x, y)`)
  // whose arm or if branch is being visited, innermost last (#327). A
  // `return`, `break`, `continue` or panic that leaves the arm drops
  // the tuple, and what of the variables' values is still in it.
  pub live_tuples: Vec<LiveTuple<'tcx>>,

  // (source, binding) column pairs of a let-chain's bindings to draw
  // side by side, each binding's column right after its source's
  // (`chain_binding_columns`, #583, #584); handed to the renderer's
  // `VisualizationData::binding_columns`.
  pub binding_columns: Vec<(u64, u64)>,

  // `r?` with `r` a non-`Copy` place moves `r` (#652). Set while
  // `match_rhs` draws that move into the variable `r?` is bound to;
  // the Move events made meanwhile carry it.
  pub try_move: Option<TryMove>,
  // Each such `?` expression whose move is drawn: `Some(id)` if
  // `note_try` drew it as a move into the `?` alone (event `id`), which
  // `match_rhs` replaces with a move to the variable if `r?` is bound
  // to one; `None` if `match_rhs` drew it first.
  pub try_moves: HashMap<rustc_hir::HirId, Option<usize>>,

  // The values a branch of the function's returned `if` / `match`
  // yields (`if b { s } else { t }` as the tail or a `return`'s
  // operand), by HirId: each goes to the caller when its branch runs,
  // drawn where `visit_expr` visits it, inside its branch (#674).
  pub tail_values: HashSet<rustc_hir::HirId>,
  // Where the arms of each if/else and match leave to when they exit
  // early, by the Branch event's id (see `end_borrows_at_exits`).
  pub exit_scopes: HashMap<usize, ExitScope>,
}

/// See `ExprVisitor::live_tuples`.
#[derive(Clone)]
pub struct LiveTuple<'tcx> {
  // The index in `exit_frames` of the if/else or match frame whose
  // branch holds the tuple, and that frame's `len`.
  pub frame: usize,
  pub len: usize,
  pub scrutinee: &'tcx Expr<'tcx>,
  pub tuple: String,
  pub pat: &'tcx Pat<'tcx>,
  // `MatchArmExit` or `IfExit`.
  pub at: TupleDropAt,
}

/// What an early exit (`return`, panic, `break`, `continue`) in an arm
/// of a conditional leaves, recorded when the conditional is built.
#[derive(Debug, Clone, Copy)]
pub struct ExitScope {
  /// The conditional is in a closure: a `return` there leaves only
  /// the closure.
  pub in_closure: bool,
  /// First and last line of the innermost loop around the conditional,
  /// which a `break` / `continue` in it leaves.
  pub loop_lines: Option<(usize, usize)>,
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

/// See `ExprVisitor::exit_frames`.
#[derive(Clone)]
pub enum ExitFrame {
  // `hide`: ranges of event ids (see `take_branch_events`) that can
  // be among `preprocessed_events` while a branch of the conditional
  // is visited but aren't on the path through it: the if-body's while
  // the `else` is visited, and, for `let x = if … / match …`, the
  // events binding each branch's value to `x` (`rhs_arm_ids`), which
  // only happen once the conditional has finished.
  //
  // `binds`: events on the path into the branch that aren't in
  // `preprocessed_events` while it's visited (a match arm's pattern
  // bindings and guard, kept in the arm's own list), and `binds_at`
  // the length `preprocessed_events` had when they happened. See
  // `owns_value_here`.
  Branch { len: usize, hide: Vec<std::ops::Range<usize>>, binds: Vec<(usize, ExternalEvent)>, binds_at: usize },
  // `head_from`: for a `for` loop, where in `in_scope` the temporaries
  // its head made begin (they're dropped where the loop is left, but
  // declared before `len`); `len` for any other loop.
  Loop { id: rustc_hir::HirId, len: usize, head_from: usize },
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

  /// `try_expr` is `r?` (a `MatchSource::TryDesugar` match) with `r` a
  /// variable, or a field of one, that has a column and isn't `Copy`:
  /// `Try::branch(r)` takes it by value, so the `?` moves it (#652).
  /// Its column, which `Try` type it is, and `r`.
  pub fn try_moved_place(&self, try_expr: &'tcx Expr<'tcx>) -> Option<(ResourceAccessPoint, TryMove, &'tcx Expr<'tcx>)> {
    fn is_place(e: &Expr) -> bool {
      match e.kind {
        ExprKind::Path(QPath::Resolved(None, p)) => matches!(p.res, Res::Local(_)),
        ExprKind::Field(base, _) => is_place(base),
        _ => false,
      }
    }
    let ExprKind::Match(scrutinee, _, rustc_hir::MatchSource::TryDesugar(_)) = try_expr.kind else { return None };
    let ExprKind::Call(_, [operand]) = scrutinee.kind else { return None };
    if !is_place(operand) || operand.span.from_expansion() {
      return None;
    }
    let typeck = self.tcx.typeck(operand.hir_id.owner);
    if let ExprKind::Field(base, _) = operand.kind {
      // A union is one opaque column.
      if typeck.expr_ty(base).is_union() {
        return None;
      }
    }
    let ty = typeck.expr_ty(operand);
    if self.ty_is_copy(ty, operand.hir_id.owner) {
      return None;
    }
    let ResourceTy::Value(rap) = get_rap(operand, &self.tcx, &self.raps) else { return None };
    if rap.is_fn() {
      return None;
    }
    let kind = match ty.ty_adt_def() {
      Some(adt) if self.tcx.is_diagnostic_item(rustc_span::sym::Result, adt.did()) => TryMove::Result,
      Some(adt) if self.tcx.is_diagnostic_item(rustc_span::sym::Option, adt.did()) => TryMove::Option,
      _ => TryMove::Other,
    };
    Some((rap, kind, operand))
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
    ty_is_plain(self.tcx, ty, owner)
  }

  /// True iff `ty` is, or contains (through generic arguments, tuples,
  /// arrays, or the fields of local ADTs), one of std's lock types.
  /// See `ty_is_plain` for why these are special-cased.
  fn ty_has_std_sync_primitive(&self, ty: Ty<'tcx>) -> bool {
    ty_has_std_sync_primitive(self.tcx, ty)
  }

  /// Whether a value of type `ty` can be mutated through a shared
  /// reference: it contains an `UnsafeCell` (what `Cell`, `RefCell`,
  /// `Mutex` and the atomics are built on), inline or behind a pointer
  /// (`Rc<RefCell<T>>`, `Vec<Cell<T>>`, `&Cell<T>`, a local struct with
  /// such a field). Populates `Owner::interior_mut` /
  /// `Struct::interior_mut`, which keeps the lend-dot tooltip from
  /// calling an immutably borrowed `RefCell` "read-only" and the call
  /// dot of a `&self` method from saying it only reads.
  ///
  /// Exact where the type is known: a cell is found by looking for
  /// `UnsafeCell` itself, in the generic arguments of every ADT, in all
  /// fields of a local ADT, and in the fields of a foreign ADT that
  /// isn't `Freeze` (so has a cell inline; `Rc<T>`'s own refcount cells,
  /// behind its pointer, don't count). A `dyn Trait`, type parameter or
  /// opaque type gives `Unknown` unless a cell was found elsewhere: it
  /// may or may not hold one (`Vec<Box<dyn Fn()>>` was said to have
  /// interior mutability because `dyn Fn()` isn't `Freeze`; #706).
  pub fn ty_has_interior_mut(&self, ty: Ty<'tcx>, owner: rustc_hir::OwnerId) -> InteriorMut {
    fn any(parts: impl Iterator<Item = InteriorMut>) -> InteriorMut {
      let mut out = InteriorMut::No;
      for p in parts {
        match p {
          InteriorMut::Yes => return InteriorMut::Yes,
          InteriorMut::Unknown => out = InteriorMut::Unknown,
          InteriorMut::No => {}
        }
      }
      out
    }
    fn go<'tcx>(tcx: TyCtxt<'tcx>, env: rustc_middle::ty::TypingEnv<'tcx>, ty: Ty<'tcx>,
                depth: usize, seen: &mut HashSet<Ty<'tcx>>) -> InteriorMut {
      if depth > 8 || !seen.insert(ty) { return InteriorMut::No; }
      match ty.kind() {
        TyKind::Adt(def, args) => {
          if def.is_unsafe_cell() { return InteriorMut::Yes; }
          let mut parts: Vec<Ty<'tcx>> = args.types().collect();
          if def.did().is_local() || !ty.is_freeze(tcx, env) {
            parts.extend(def.all_fields().map(|f| f.ty(tcx, args)));
          }
          any(parts.into_iter().map(|t| go(tcx, env, t, depth + 1, seen)))
        }
        TyKind::Ref(_, t, _) | TyKind::Array(t, _) | TyKind::Slice(t) => go(tcx, env, *t, depth + 1, seen),
        TyKind::Tuple(ts) => any(ts.iter().map(|t| go(tcx, env, t, depth + 1, seen))),
        // A closure holds what it captures.
        TyKind::Closure(_, args) => go(tcx, env, args.as_closure().tupled_upvars_ty(), depth + 1, seen),
        // Opaque: what's inside isn't known here.
        TyKind::Dynamic(..) | TyKind::Param(_) | TyKind::Alias(..) | TyKind::Foreign(_)
        | TyKind::Placeholder(_) | TyKind::Bound(..) | TyKind::Infer(_) | TyKind::Error(_)
        | TyKind::Coroutine(..) | TyKind::CoroutineClosure(..) | TyKind::CoroutineWitness(..) => InteriorMut::Unknown,
        _ => InteriorMut::No,
      }
    }
    let env = rustc_middle::ty::TypingEnv::post_analysis(self.tcx, owner);
    go(self.tcx, env, ty, 0, &mut HashSet::new())
  }

  // Adds an owner RAP
  pub fn add_owner(&mut self, name: String, mutability: bool, is_copy: bool, is_plain_value: bool, interior_mut: InteriorMut, scope: usize, is_global: bool) {
    self.add_rap(ResourceAccessPoint::Owner(Owner{name: name, hash: self.rap_hashes as u64, is_mut: mutability, is_copy, is_plain_value, interior_mut, is_closure: false, is_union: false, is_manually_drop: false, unbound_temp: false, let_literal_temp: false, temp_part: None, write: Default::default(), holds_borrow: false, rc: None, rc_inside: None, holds: None, box_contents_moved: false}), scope, is_global);
  }

  // Adds an owner RAP for a `union`-typed binding (one opaque column;
  // see the AdtKind::Union arms).
  pub fn add_union_owner(&mut self, name: String, mutability: bool, is_copy: bool, is_plain_value: bool, interior_mut: InteriorMut, scope: usize, is_global: bool) {
    self.add_rap(ResourceAccessPoint::Owner(Owner{name: name, hash: self.rap_hashes as u64, is_mut: mutability, is_copy, is_plain_value, interior_mut, is_closure: false, is_union: true, is_manually_drop: false, unbound_temp: false, let_literal_temp: false, temp_part: None, write: Default::default(), holds_borrow: false, rc: None, rc_inside: None, holds: None, box_contents_moved: false}), scope, is_global);
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

  /// `ty` as a student would write it: short std names (`Vec<String>`,
  /// not `alloc::vec::Vec<alloc::string::String>`) (#542).
  pub fn short_ty_name(&self, ty: Ty<'tcx>) -> String {
    use rustc_middle::ty::TyKind;
    let tcx = self.tcx;
    match ty.kind() {
      TyKind::Adt(def, args) => {
        let name = tcx.item_name(def.did()).to_string();
        let targs: Vec<String> = args.types().map(|t| self.short_ty_name(t)).collect();
        // Allocator / hasher parameters are not written by the user.
        let shown: Vec<String> = match name.as_str() {
          "Vec" | "VecDeque" | "Box" | "Rc" | "Arc" | "BinaryHeap" | "LinkedList" => targs.into_iter().take(1).collect(),
          "HashMap" | "BTreeMap" => targs.into_iter().take(2).collect(),
          "HashSet" | "BTreeSet" => targs.into_iter().take(1).collect(),
          _ => targs,
        };
        if shown.is_empty() { name } else { format!("{}<{}>", name, shown.join(", ")) }
      }
      TyKind::Tuple(ts) => {
        let v: Vec<String> = ts.iter().map(|t| self.short_ty_name(t)).collect();
        if v.len() == 1 { format!("({},)", v[0]) } else { format!("({})", v.join(", ")) }
      }
      TyKind::Array(t, _) => format!("[{}; _]", self.short_ty_name(*t)),
      TyKind::Slice(t) => format!("[{}]", self.short_ty_name(*t)),
      TyKind::Ref(_, t, m) => format!("&{}{}", if m.is_mut() { "mut " } else { "" }, self.short_ty_name(*t)),
      TyKind::Dynamic(preds, ..) => match preds.principal_def_id() {
        Some(did) => format!("dyn {}", tcx.item_name(did)),
        None => String::from("dyn Trait"),
      },
      _ => rustc_middle::ty::print::with_no_trimmed_paths!(ty.to_string()),
    }
  }

  /// For `ty` a std collection, `Option` or `Box` whose elements need
  /// drop: what dropping it also drops, as `every String it holds` /
  /// `the String it holds, if any` / `every key and value it holds`
  /// (#542). `None` for anything else, including a collection of
  /// elements that need no drop (`Vec<i32>`). The count isn't known
  /// statically, so none is given.
  pub fn holds_phrase(&self, ty: Ty<'tcx>) -> Option<String> {
    use rustc_span::sym;
    use rustc_middle::ty::TyKind;
    let tcx = self.tcx;
    let env = rustc_middle::ty::TypingEnv::fully_monomorphized();
    let needs = |t: Ty<'tcx>| t.needs_drop(tcx, env);
    if !needs(ty) { return None; }
    let TyKind::Adt(def, args) = ty.kind() else {
      return match ty.kind() {
        TyKind::Array(t, _) if needs(*t) => Some(format!("every {} it holds", self.short_ty_name(*t))),
        _ => None,
      };
    };
    let did = def.did();
    let std_item = |n: &str| !did.is_local()
      && matches!(tcx.crate_name(did.krate).as_str(), "alloc" | "std")
      && tcx.item_name(did).as_str() == n;
    if ty.is_box() {
      let inner = args.type_at(0);
      if !needs(inner) { return None; }
      return match inner.kind() {
        TyKind::Slice(t) => Some(format!("every {} it holds", self.short_ty_name(*t))),
        TyKind::Dynamic(..) => Some(format!("the {} value it holds", self.short_ty_name(inner))),
        _ => Some(format!("the {} it holds", self.short_ty_name(inner))),
      };
    }
    if tcx.is_diagnostic_item(sym::Option, did) {
      let inner = args.type_at(0);
      return Some(format!("the {} it holds, if any", self.short_ty_name(inner)));
    }
    if ["Vec", "VecDeque", "LinkedList", "BinaryHeap", "HashSet", "BTreeSet"].iter().any(|n| std_item(n)) {
      let t = args.type_at(0);
      return needs(t).then(|| format!("every {} it holds", self.short_ty_name(t)));
    }
    if ["HashMap", "BTreeMap"].iter().any(|n| std_item(n)) {
      let (k, v) = (args.type_at(0), args.type_at(1));
      return match (needs(k), needs(v)) {
        (true, true) => Some(String::from("every key and value it holds")),
        (true, false) => Some(String::from("every key it holds")),
        (false, true) => Some(String::from("every value it holds")),
        _ => None,
      };
    }
    if def.is_enum() { return self.enum_holds_phrase(ty); }
    None
  }

  /// `holds_phrase` for an enum with no field columns of its own
  /// (`Result<T, E>`, `Cow`, a local enum): the distinct types of the
  /// variants' fields that need drop. Which variant a value is isn't
  /// known statically, so say `, if any` when some variant holds none,
  /// and `, whichever variant it is` when every variant holds exactly
  /// one value of the same type. (#684)
  fn enum_holds_phrase(&self, ty: Ty<'tcx>) -> Option<String> {
    let tcx = self.tcx;
    let env = rustc_middle::ty::TypingEnv::fully_monomorphized();
    let rustc_middle::ty::TyKind::Adt(def, args) = ty.kind() else { return None };
    let mut names: Vec<String> = Vec::new();
    let mut some_variant_empty = false;
    let mut some_variant_many = false;
    for v in def.variants() {
      let held: Vec<String> = v.fields.iter()
        .map(|f| tcx.normalize_erasing_regions(env, rustc_middle::ty::Unnormalized::new(f.ty(tcx, args))))
        .filter(|t| t.needs_drop(tcx, env))
        .map(|t| self.short_ty_name(t))
        .collect();
      if held.is_empty() { some_variant_empty = true; }
      if held.len() > 1 { some_variant_many = true; }
      for n in held { if !names.contains(&n) { names.push(n); } }
    }
    if names.is_empty() { return None; }
    let list = match names.len() {
      1 => names[0].clone(),
      2 => format!("{} or {}", names[0], names[1]),
      _ => format!("{}, or {}", names[..names.len() - 1].join(", "), names[names.len() - 1]),
    };
    let det = if some_variant_many { "every" } else { "the" };
    let tail = if some_variant_empty { ", if any" }
      else if names.len() == 1 && !some_variant_many { ", whichever variant it is" }
      else { "" };
    Some(format!("{det} {list} it holds{tail}"))
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
    // The Rc note wins: no double parenthetical (#542).
    let holds = if rc.is_some() || rc_fields.is_some() || rc_inside.is_some() || manually_drop { None } else { self.holds_phrase(ty) };
    let box_contents_moved = self.box_contents_moved_out(name, ty);
    if rc.is_none() && !manually_drop && rc_fields.is_none() && rc_inside.is_none() && holds.is_none() && !box_contents_moved {
      return;
    }
    let Some(just_added) = (self.rap_hashes as u64).checked_sub(1) else { return };
    match self.raps.get_mut(name).map(|rd| &mut rd.rap) {
      Some(ResourceAccessPoint::Owner(o)) if o.hash == just_added => {
        o.is_manually_drop = manually_drop;
        o.rc = rc;
        o.rc_inside = rc_inside;
        o.holds = holds;
        o.box_contents_moved = box_contents_moved;
      }
      Some(ResourceAccessPoint::Struct(s)) if s.hash == just_added => {
        s.is_manually_drop = manually_drop;
        s.rc = rc;
        s.rc_fields = rc_fields;
        s.rc_inside = rc_inside;
        s.holds = holds;
      }
      _ => {}
    }
  }

  // Adds an owner RAP that binds a closure literal. Routed through
  // a separate constructor (rather than an extra arg on add_owner) so
  // we don't churn every call site — closures are the only producer.
  pub fn add_closure_owner(&mut self, name: String, mutability: bool, scope: usize, is_global: bool) {
    self.add_rap(ResourceAccessPoint::Owner(Owner{name: name, hash: self.rap_hashes as u64, is_mut: mutability, is_copy: false, is_plain_value: false, interior_mut: InteriorMut::No, is_closure: true, is_union: false, is_manually_drop: false, unbound_temp: false, let_literal_temp: false, temp_part: None, write: Default::default(), holds_borrow: false, rc: None, rc_inside: None, holds: None, box_contents_moved: false}), scope, is_global);
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
    self.borrow_map.insert(name.clone(), RefData { lender: lender.clone(), assigned_at: line_num, lifetime: line_num, ref_mutability: ref_mutability, aliasing: alia, static_lender: None, borrow_span: None, loan_line: None, co_lenders: Vec::new(), lender_muts: Vec::new() });
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
  /// Unsized referents: `str` is a value, `[T]` is a value iff `T` is
  /// Copy or plain (`&[i32]`, `&str`; #549), but `[String]` stays a
  /// resource. `dyn Trait` stays a resource (we can't know).
  pub fn ref_pointee_is_copy(&self, ref_ty: Ty<'tcx>, owner: rustc_hir::OwnerId) -> bool {
    match ref_ty.kind() {
      TyKind::Ref(_, inner, _) => match inner.kind() {
        TyKind::Str => true,
        TyKind::Slice(elem) => self.ty_is_copy(*elem, owner) || self.ty_is_plain(*elem, owner),
        _ => self.ty_is_copy(*inner, owner) || (inner.is_adt() && self.ty_is_plain(*inner, owner)),
      },
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
    self.add_rap(ResourceAccessPoint::Function(Function { name: name, hash: self.rap_hashes as u64, drops_arg: Default::default(), rc_call: None, cast_to: None, inside: None }), self.current_scope, true);
  }

  // Adds a struct RAP
  // To be honest this struct logic is leftover from RV1 and should eventually be reworked
  // It doesn't make much sense that a struct should be represented any different than an owner
  // the reason for this is that in the frontend structs are visualized differently in the timeline header
  pub fn add_struct(&mut self, name: String, owner: u64, mem: bool, mutability: bool, is_copy: bool, is_plain_value: bool, interior_mut: InteriorMut, scope: usize, is_global: bool) {
    self.add_rap(ResourceAccessPoint::Struct(Struct {
      name: name,
      hash: self.rap_hashes as u64,
      owner: owner,
      is_mut: mutability,
      is_member: mem,
      is_copy,
      is_plain_value,
      interior_mut,
      is_manually_drop: false, rc: None, rc_fields: None, rc_inside: None, holds: None, is_box: false }),
      scope, is_global);
  }

  pub fn add_rap(&mut self, r: ResourceAccessPoint, scope: usize, is_global: bool) {
    let fn_start_line = self.current_fn_start;
    // Functions are keyed in their own namespace (see `fn_key`), so a
    // variable and a function with the same name get separate RAPs.
    let key = if r.is_fn() { fn_key(r.name()) } else { r.name().to_string() };
    self.in_scope.push(key.clone());
    match self.raps.entry(key) {
      std::collections::hash_map::Entry::Vacant(v) => {
        self.rap_hashes += 1;
        v.insert(RapData { rap: r, scope, is_global, fn_start_line, extended_temp: None, temp_owner: None, extra_scopes: Vec::new() });
      }
      // A later variable of the same name in the same fn shares the
      // column of the first (sibling blocks, #308) and ends at its own
      // block end (#589).
      std::collections::hash_map::Entry::Occupied(mut o) => {
        let rd = o.get_mut();
        if rd.is_global && is_global && rd.fn_start_line == fn_start_line && !r.is_fn()
          && scope != rd.scope && !rd.extra_scopes.contains(&scope) {
          rd.extra_scopes.push(scope);
        }
      }
    }
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
      | ExternalEvent::TupleTempDrop { ro, .. }
      | ExternalEvent::OrAltMove { ro, .. }
      | ExternalEvent::ChainFailAssign { ro, .. }
      | ExternalEvent::ChainFailUse { ro, .. }
      | ExternalEvent::OwnerDropAtReassign { ro, .. }
      | ExternalEvent::OwnerDropIfTryReturns { ro, .. }
      | ExternalEvent::OwnerDropUnlessReturned { ro, .. }
      | ExternalEvent::TemporaryDrop { ro, .. }
      | ExternalEvent::LoopIteratorDrop { ro, .. } => {
        self.skip_raps.contains(ro.name())
      }
      ExternalEvent::InitRefParam { param, .. } => {
        self.skip_raps.contains(param.name())
      }
      ExternalEvent::OrAltBorrow { ro, into, .. } => {
        self.skip_raps.contains(ro.name()) || self.skip_raps.contains(into.name())
      }
      ExternalEvent::ClosureCall { closure, .. } => touches_ty(closure),
      // Branch events bundle child events; we don't recurse into
      // them here. A skipped variable that only appears inside a
      // branch could still leak through — fix when the need arises.
      ExternalEvent::Branch { .. } => false,
    }
  }

  pub fn add_external_event(&mut self, line_num: usize, mut event: ExternalEvent) {
    let line_num = self.line_override.unwrap_or(line_num);
    if let ExternalEvent::Move { from: ResourceTy::Value(from), break_out, .. } = &mut event {
      if self.break_out.contains(from.name()) {
        *break_out = true;
      }
    }
    if let Some(alt) = &self.break_alt {
      if let ExternalEvent::Move { break_alt, .. } | ExternalEvent::Copy { break_alt, .. }
        | ExternalEvent::Bind { break_alt, .. } | ExternalEvent::StaticBorrow { break_alt, .. }
        | ExternalEvent::MutableBorrow { break_alt, .. }
        | ExternalEvent::PassByStaticReference { break_alt, .. }
        | ExternalEvent::PassByMutableReference { break_alt, .. } = &mut event {
        *break_alt = Some(alt.clone());
      }
    }
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
      // A borrow kept by the closure a call returned (#686) is drawn
      // after that closure arrives from the call on the holder's column,
      // not stacked with the line's other arrows ahead of it.
      (_, _, ExternalEvent::StaticBorrow { via, .. } | ExternalEvent::MutableBorrow { via, .. }) if returned_closure(via) => {},
      // A borrow stored in an element (`v[0] = &u`, #282) is drawn on
      // the lender's column only: v's column doesn't model it.
      (_, ResourceTy::Index(..), ExternalEvent::StaticBorrow { .. } | ExternalEvent::MutableBorrow { .. }) => {},
      (_, ResourceTy::Temporary { .. }, _) => {},
      // A borrow end drawn on the lender's column only (a borrow
      // stored in a place, #512/#531, or the old value of a reference
      // written through `*w`, #664) is no arrow: it mustn't add a row
      // to its line, which pushed the line's other events onto a row
      // below the place's borrow end.
      (ResourceTy::Index(..), _, ExternalEvent::StaticDie { .. } | ExternalEvent::MutableDie { .. }) => {},
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
    if let (Some(from_name), Some(to_name)) = (&from_struct, &to_struct) {
      self.transfer_ref_fields(line_num, from_name, to_name, is_copy);
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
          ExternalEvent::Copy { from: from_child, to: dest, id: ev_id, is_partial: false, into_closure: false, break_alt: None, closure_value: false }
        } else {
          ExternalEvent::Move { from: from_child, to: dest, id: ev_id, is_partial: false, into_closure: false, loop_head: LoopHead::No, break_alt: None, break_out: false, via_try: None, closure_value: false }
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

  /// `let hold = other;` / `hold = other;` (#637): the reference
  /// fields go with the struct, and so do the borrows they hold. In
  /// the borrow checker, the loan `other.r` was made with stays live
  /// as long as any value holding the reference is, so `t` stays
  /// borrowed until `hold.r`'s last use, and `hold.r` is what gives it
  /// back. `hold.r` takes `other.r`'s loan as recorded (lender, line
  /// and span it was made at), so `print_lifetimes` ends it where MIR
  /// ends that loan; the field's own Move arrow makes `other.r` hand
  /// the borrow off (`ref_handed_off`), so it draws no end of its own.
  /// A Copy struct copies the reference: both hold the loan.
  fn transfer_ref_fields(&mut self, line_num: usize, from_name: &str, to_name: &str, is_copy: bool) {
    let prefix = format!("{}.", from_name);
    #[expect(clippy::disallowed_methods, reason = "sorted by (hash, name) below")]
    let mut refs: Vec<ResourceAccessPoint> = self.raps.iter()
      .filter(|(n, rd)| n.starts_with(&prefix) && !n[prefix.len()..].contains('.')
        && rd.rap.is_ref() && rd.rap.is_member())
      .map(|(_, rd)| rd.rap.clone())
      .collect();
    refs.sort_by(|a, b| a.hash().cmp(b.hash()).then_with(|| a.name().cmp(b.name())));
    for child in refs {
      let field = &child.name()[prefix.len()..];
      let Some(dest) = self.raps.get(&format!("{}.{}", to_name, field)).map(|rd| rd.rap.clone()) else { continue };
      if let Some(loan) = self.borrow_map.get(child.name()).cloned() {
        self.borrow_map.insert(dest.name().clone(), loan);
      }
      let ev_id = *self.unique_id;
      *self.unique_id += 1;
      let (from, to) = (ResourceTy::Value(child), ResourceTy::Value(dest));
      self.add_external_event(line_num, if is_copy {
        ExternalEvent::Copy { from, to, id: ev_id, is_partial: false, into_closure: false, break_alt: None, closure_value: false }
      } else {
        ExternalEvent::Move { from, to, id: ev_id, is_partial: false, into_closure: false, loop_head: LoopHead::No, break_alt: None, break_out: false, via_try: None, closure_value: false }
      });
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
        loop_head: LoopHead::No,
        break_alt: None,
        break_out: false,
        via_try: None,
        closure_value: false,
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
        break_alt: None,
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

  /// `rap_ownership_now` as of before `preprocessed_events[end..]`.
  pub fn rap_ownership_before(&self, end: usize, rap_name: &str) -> Ownership {
    ownership_after(&self.preprocessed_events[..end], rap_name, None).unwrap_or(Ownership::None)
  }

  /// True when a let-chain's condition may have moved `name` before it
  /// failed (#281): a `MaybeMovedAt::ChainFail` event for it, also
  /// inside a finished conditional. A drop-flag message then says
  /// "nothing above" moved it rather than "no branch above".
  pub fn moved_by_chain_condition(&self, name: &str) -> bool {
    fn any(events: &[(usize, ExternalEvent)], name: &str) -> bool {
      events.iter().any(|(_, ev)| match ev {
        ExternalEvent::LoopExitMove { ro, at: MaybeMovedAt::ChainFail { .. }, .. } => ro.name() == name,
        ExternalEvent::Branch { branches, .. } => branches.iter().any(|b| any(&b.e_data, name)),
        _ => false,
      })
    }
    any(self.preprocessed_events, name)
  }

  /// True iff `name` is a reference whose value was moved, since it was
  /// last assigned, into something that carries the borrow on and ends
  /// it itself: another reference (`let r2 = r;`, `Holder { r }`) or the
  /// caller (`r` as a fn's result, also from a branch of its tail `if` / `match`). After such a move `name` holds
  /// nothing, so its own borrow-end / return-to-lender event would be
  /// false (#209). A `&mut` moved into a call (`take(r)`) isn't covered:
  /// nothing else with a column ends that borrow, so `r`'s end stands
  /// for it.
  pub fn ref_handed_off(&self, name: &str) -> bool {
    let (Some(rd), Some(rap_data)) = (self.borrow_map.get(name), self.raps.get(name)) else {
      return false;
    };
    let hash = *rap_data.rap.hash();
    let swapped_here = |line: usize| self.destructure_swapped.iter()
      .any(|(n, lo, hi)| n == name && (*lo..=*hi).contains(&line) && (*lo..=*hi).contains(&rd.assigned_at));
    // `in_branch`: the event is inside a branch of a conditional. There
    // only a hand-off to the caller counts: a `&mut` moved or a shared
    // reference copied to the caller (`if c { x } else { y }` as the
    // tail, #742): the reference goes to the caller on
    // that path, so its borrow doesn't end there.
    fn handed(this: &ExprVisitor, events: &[(usize, ExternalEvent)], hash: u64, rd_assigned_at: usize, swapped_here: &dyn Fn(usize) -> bool, in_branch: bool) -> bool {
      events.iter().any(|(line, ev)| match ev {
        ExternalEvent::Move { from: ResourceTy::Value(f), to, .. }
          if *f.hash() == hash && *line >= rd_assigned_at && !swapped_here(*line) =>
        {
          match to {
            ResourceTy::Caller => true,
            // Moves into another reference inside a branch keep the old drawing.
            ResourceTy::Value(t) if !in_branch => this.borrow_map.contains_key(t.name())
              && this.raps.get(t.name()).map_or(false, |r| r.rap.hash() == t.hash()),
            _ => false,
          }
        }
        ExternalEvent::Copy { from: ResourceTy::Value(f), to: ResourceTy::Caller, .. }
          if in_branch && *f.hash() == hash && *line >= rd_assigned_at => true,
        ExternalEvent::Branch { branches, .. } => branches.iter()
          .any(|b| handed(this, &b.e_data, hash, rd_assigned_at, swapped_here, true)),
        _ => false,
      })
    }
    handed(self, self.preprocessed_events, hash, rd.assigned_at, &swapped_here, false)
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
      Evt::Bind => ExternalEvent::Bind { from: rhs, to: lhs, id, break_alt: None },
      Evt::Copy => ExternalEvent::Copy { from: rhs, to: lhs, id, is_partial, into_closure: false, break_alt: None, closure_value: false },
      Evt::Move => ExternalEvent::Move { from: rhs, to: lhs, id, is_partial, into_closure: false, loop_head: LoopHead::No, break_alt: None, break_out: false, via_try: self.try_move, closure_value: false },
      Evt::SBorrow => ExternalEvent::StaticBorrow { from: rhs, to: lhs, id, is_partial, is_reborrow: false, break_alt: None, via: None },
      Evt::MBorrow => ExternalEvent::MutableBorrow { from: rhs, to: lhs, id, is_partial, is_reborrow: false, break_alt: None, via: None },
      Evt::PassBySRef => ExternalEvent::PassByStaticReference { from: rhs, to: lhs, id, implicit: ImplicitRef::No, refcell_borrow_mut: false, result_keeps_borrow: false, stored_in: StoredIn::default(), break_alt: None },
      Evt::PassByMRef => ExternalEvent::PassByMutableReference {
        from: rhs, to: lhs, id, is_reborrow: false, result_has_lifetime: false, implicit: ImplicitRef::No,
        result_keeps_borrow: false, stored_in: StoredIn::default(), break_alt: None,
      },
      Evt::SDie =>  ExternalEvent::StaticDie { from: rhs, to: lhs, id, static_lender: None, still_borrowed_by: Vec::new() },
      Evt::MDie => ExternalEvent::MutableDie { from: rhs, to: lhs, id, static_lender: None, still_borrowed_by: Vec::new() }
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
  /// `else if`) get their own events (#117). Those in `keep` stay
  /// where they are, before the split (a let-chain's copy out of a
  /// variable, #583).
  pub fn take_branch_events(&mut self, ids: std::ops::Range<usize>, body: Option<&'tcx Expr<'tcx>>, keep: &HashSet<usize>) -> Vec<(usize, ExternalEvent)> {
    let rhs = body.and_then(|b| self.rhs_arm_ids.get(&b.hir_id).copied());
    let mine = |id: usize| (ids.contains(&id) && !keep.contains(&id)) || rhs.is_some_and(|(lo, hi)| (lo..hi).contains(&id));
    let unbranched = &self.unbranched_temps;
    // The end of a loan a field write in the branch leaves to last
    // until the struct's last use (#531) is drawn on a line after the
    // branch when that use is: it happens after the branches join, so
    // it stays on the global timeline, not on a branch's column (#659).
    let body_end = body.map(|b| span_end_line(&b.span, &self.tcx));
    let escapes = |l: usize, ev: &ExternalEvent| body_end.is_some_and(|end| l > end)
      && matches!(ev, ExternalEvent::StaticDie { from: ResourceTy::Index(..), .. } | ExternalEvent::MutableDie { from: ResourceTy::Index(..), .. });
    let mut taken = Vec::new();
    self.preprocessed_events.retain(|(l, ev)| {
      // A drop of a temporary in `unbranched_temps` (at an early exit)
      // stays on its own, unsplit column.
      if matches!(ev, ExternalEvent::TemporaryDrop { ro, .. } if unbranched.contains(ro)) {
        return true;
      }
      if mine(ev.get_id()) && !escapes(*l, ev) {
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

  /// A `for` loop's head `from` moved into the loop, whose iterator
  /// has drop glue (#482), with the arrow's target `to`.
  pub fn add_loop_head_move_ev(&mut self, line_num: usize, to: ResourceTy, from: ResourceTy, head: LoopHead) {
    let mut ev = self.ext_ev_of_evt(Evt::Move, to, from, *self.unique_id, false);
    if let ExternalEvent::Move { loop_head, .. } = &mut ev {
      *loop_head = head;
    }
    self.add_external_event(line_num, ev);
    *self.unique_id += 1;
  }

  /// The end of reference `from`'s borrow (StaticDie / MutableDie),
  /// returning to `to`; `ref_data` is `from`'s loan, and names the
  /// 'static data it borrowed when the lender has no column.
  ///
  /// `still_borrowed_by`: the values that go on holding the lender's
  /// borrow after this reference's ends (see `print_lifetimes`, #575).
  pub fn add_borrow_end_ev(&mut self, line_num: usize, to: ResourceTy, from: ResourceTy, ref_data: &RefData, still_borrowed_by: Vec<String>) {
    let id = *self.unique_id;
    *self.unique_id += 1;
    let static_lender = match ref_data.lender {
      ResourceTy::Anonymous if from.extract_rap().is_some_and(|r| self.tuple_temp_refs.contains(r.name())) =>
        Some(StaticLender::TupleTemporary),
      ResourceTy::Anonymous => ref_data.static_lender.clone(),
      _ => None,
    };
    let ev = if ref_data.mut_of(&to) {
      ExternalEvent::MutableDie { from, to, id, static_lender, still_borrowed_by }
    } else {
      ExternalEvent::StaticDie { from, to, id, static_lender, still_borrowed_by }
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

  /// `let h = g;` (or `h = g;`) with `g` a closure (#704): the closure
  /// value is copied (when all it captures by reference are shared
  /// borrows, it is `Copy`) or moved, like any other value; nothing is
  /// captured. The borrows its by-reference captures made go with it:
  /// rustc's loan for each one lasts as long as either closure can
  /// still be used, which is the line `g`'s borrow already ends on (the
  /// closure literal drew it at the loan's end). A copy holds the
  /// borrow too, so `h` also gives it back there; after a move `h`
  /// holds it instead of `g`.
  fn move_or_copy_closure(&mut self, line_num: usize, evt: Evt, lhs: ResourceTy, g: ResourceAccessPoint) {
    let is_copy = matches!(evt, Evt::Copy);
    let mut ev = self.ext_ev_of_evt(evt, lhs.clone(), ResourceTy::Value(g.clone()), *self.unique_id, false);
    if let ExternalEvent::Copy { closure_value, .. } | ExternalEvent::Move { closure_value, .. } = &mut ev {
      *closure_value = true;
    }
    self.add_external_event(line_num, ev);
    *self.unique_id += 1;
    let held_by_g = |e: &ExternalEvent| matches!(e,
      ExternalEvent::StaticDie { from: ResourceTy::Value(r), .. }
      | ExternalEvent::MutableDie { from: ResourceTy::Value(r), .. } if r.hash() == g.hash() && r.name() == g.name());
    if is_copy {
      let ends: Vec<(usize, ExternalEvent)> = self.preprocessed_events.iter()
        .filter(|(_, e)| held_by_g(e))
        .cloned()
        .collect();
      for (line, mut e) in ends {
        let (ExternalEvent::StaticDie { from, id, to, .. } | ExternalEvent::MutableDie { from, id, to, .. }) = &mut e else { continue };
        // `h = g;` again: h already gives this borrow back there.
        let lender = to.clone();
        let already = self.preprocessed_events.iter().any(|(l, x)| *l == line && matches!(x,
          ExternalEvent::StaticDie { from: f, to: t, .. } | ExternalEvent::MutableDie { from: f, to: t, .. }
            if *f == lhs && *t == lender));
        if already {
          continue;
        }
        *from = lhs.clone();
        *id = *self.unique_id;
        *self.unique_id += 1;
        self.add_external_event(line, e);
      }
    } else {
      // Both copies of the event: the renderer draws a line's arrows
      // from `event_line_map`.
      let retarget = |e: &mut ExternalEvent| {
        if held_by_g(e) {
          if let ExternalEvent::StaticDie { from, .. } | ExternalEvent::MutableDie { from, .. } = e {
            *from = lhs.clone();
          }
        }
      };
      self.preprocessed_events.iter_mut().for_each(|(_, e)| retarget(e));
      self.event_line_map.values_mut().flatten().for_each(retarget);
    }
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
      from: arg, to: func, id, is_reborrow, result_has_lifetime, implicit, result_keeps_borrow: false, stored_in: StoredIn::default(), break_alt: None,
    });
    *self.unique_id += 1;
  }

  /// `add_ev` for a `PassBySRef` with an explicit `ImplicitRef`.
  pub fn add_pass_by_sref_ev(&mut self, line_num: usize, func: ResourceTy, arg: ResourceTy, implicit: ImplicitRef, refcell_borrow_mut: bool) {
    let id = *self.unique_id;
    self.add_external_event(line_num, ExternalEvent::PassByStaticReference {
      from: arg, to: func, id, implicit, refcell_borrow_mut, result_keeps_borrow: false, stored_in: StoredIn::default(), break_alt: None,
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
  ///
  /// The same for an `FnOnce` closure stored in a struct field (`(h.f)()`,
  /// #725): the call moves `h.f` out of `h`.
  pub fn closure_call_events(&mut self, callee: &'tcx Expr<'tcx>, line_num: usize) {
    let name = match callee.kind {
      ExprKind::Path(QPath::Resolved(None, path)) if matches!(path.res, Res::Local(_)) => path_name(path, &self.tcx),
      ExprKind::Field(..) => match expr_to_rap_name(callee, &self.tcx) {
        Some(n) => n,
        None => return,
      },
      _ => return,
    };
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
    let closure_rap = match self.raps.get(&name) {
      Some(rd) if rd.rap.is_closure() => rd.rap.to_owned(),
      // A field's column (`h.f`) is a struct member, not a closure binding.
      Some(rd) if matches!(callee.kind, ExprKind::Field(..)) && !rd.rap.is_ref() => rd.rap.to_owned(),
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
      let upvar_name = binding_name(&self.tcx, upvar_hir_id);
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
  /// variable, plus any struct-field or tuple-element projections (`p.name`, `t.0` for a
  /// disjoint capture of one field, which has its own column). A
  /// capture through a dereference (`*r`) is named after the reference
  /// itself. `None` if a field projection isn't a named struct field.
  fn capture_rap_name(&self, place: &rustc_middle::hir::place::Place<'tcx>, root: &str) -> Option<String> {
    let mut name = root.to_owned();
    for (i, proj) in place.projections.iter().enumerate() {
      match proj.kind {
        rustc_middle::hir::place::ProjectionKind::Field(idx, variant) => {
          let ty = place.ty_before_projection(i);
          if matches!(ty.kind(), rustc_middle::ty::TyKind::Tuple(_)) {
            // A tuple element capture (`move || t.0.len()`): the
            // element's column is `t.0` (#341).
            name = format!("{}.{}", name, idx.as_usize());
          } else {
            let adt = ty.ty_adt_def().filter(|a| a.is_struct())?;
            name = format!("{}.{}", name, adt.variant(variant).fields[idx].name.as_str());
          }
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
  /// element capture (`move || t.0.len()`) is drawn on `t.0`.
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
  pub(crate) fn match_closure_arg(&mut self, closure: &'tcx rustc_hir::Closure<'tcx>, arg: &'tcx Expr, to_ro: &ResourceTy, fn_name: &str) -> bool {
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
      let root = binding_name(&self.tcx, upvar_hir_id);
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
  /// `hold.r = &t` writes a reference field of a struct variable
  /// (`lhs_expr` is the field, `lhs_rap` its column) that held a
  /// borrow (`old`). Returns the line the borrow checker ends that old
  /// loan on, if it's after the write; None for anything else (a plain
  /// reference variable, whose old borrow does end at the write because
  /// the variable is dead until then, or a loan that ends by then).
  pub fn field_write_loan_end(&self, lhs_expr: &'tcx Expr<'tcx>, lhs_rap: &ResourceAccessPoint, old: &RefData, line: usize) -> Option<usize> {
    let ExprKind::Field(..) = lhs_expr.kind else { return None };
    let ResourceTy::Value(lender) = &old.lender else { return None };
    if lender.is_ref() || lender.is_fn() || lhs_rap.is_fn() {
      return None;
    }
    self.loan_end_line(old.assigned_at, lender.name()).filter(|&end| end > line)
  }

  pub fn loan_end_line(&self, line: usize, lender_name: &str) -> Option<usize> {
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

  /// The name a call whose callee is an expression, not a path, goes
  /// by (`(w.f)()` → `w.f`; #703): the callee's source text without
  /// the parentheses around it (the HIR span includes them).
  pub fn callee_text(&self, callee: &Expr) -> String {
    fn unwrap_parens(t: &str) -> &str {
      let t = t.trim();
      let Some(inner) = t.strip_prefix('(').and_then(|r| r.strip_suffix(')')) else { return t };
      // Only when the first `(` closes at the end: not `(f)(x)`.
      let mut depth = 0i32;
      for c in inner.chars() {
        match c {
          '(' => depth += 1,
          ')' => { depth -= 1; if depth < 0 { return t; } }
          _ => {}
        }
      }
      if depth == 0 { unwrap_parens(inner) } else { t }
    }
    let text = if callee.span.from_expansion() { None } else {
      self.tcx.sess.source_map().span_to_snippet(callee.span).ok()
    };
    text.map(|t| unwrap_parens(&t).split_whitespace().collect::<Vec<_>>().join(" "))
      .filter(|t| !t.is_empty())
      .unwrap_or_else(|| "callee".to_owned())
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
  pub fn field_through_ref(&self, expr: &'tcx Expr<'tcx>) -> Option<ResourceTy> {
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

  /// `expr` is `*r` (or `**r`, ...) with `r` a reference RAP: the value
  /// read is the one `r` points to, never `r` itself (#670). Returns
  /// `DerefField(r, "*r")`, drawn on r's column like `p.x` above.
  /// The value read is Copy (moving out from behind a reference is
  /// E0507), so a return copies it.
  pub fn deref_of_ref(&self, expr: &'tcx Expr<'tcx>) -> Option<ResourceTy> {
    let typeck = self.tcx.typeck(expr.hir_id.owner);
    let ExprKind::Unary(UnOp::Deref, mut inner) = expr.kind else { return None };
    // Every operand of a `*` here must be a reference; `*b` with b a
    // Box or an `Rc` is not a read through a borrow.
    let mut depth = 1;
    loop {
      if !typeck.node_type(inner.hir_id).is_ref() {
        return None;
      }
      match inner.kind {
        ExprKind::Unary(UnOp::Deref, i) => { inner = i; depth += 1; }
        _ => break,
      }
    }
    if !matches!(inner.kind, ExprKind::Path(_) | ExprKind::Field(..)) {
      return None;
    }
    let ref_name = expr_to_rap_name(inner, &self.tcx)?;
    let rap = self.raps.get(&ref_name).map(|rd| rd.rap.to_owned())?;
    if !rap.is_ref() {
      return None;
    }
    let place = self.plain_source_text(expr)
      .filter(|p| p.starts_with('*'))
      .unwrap_or_else(|| format!("{}{}", "*".repeat(depth), ref_name));
    Some(ResourceTy::DerefField(rap, place, PlaceWrite::Update))
  }

  /// The elements of an array literal (or `vec![…]`) going into `lhs`.
  /// Each element that is a variable (`[s, t]`, `vec![x, y]`) is moved
  /// into `lhs` (copied if its type is Copy), drawn as an arrow from the
  /// variable. Other elements (literals, calls) have nothing to draw
  /// beyond `lhs` getting its value. A reference element (`[&s]`)
  /// stores a borrow we don't track, so it's left out.
  pub fn match_array_elems(&mut self, lhs: ResourceTy, elems: &'tcx [Expr<'tcx>], line_num: usize) {
    let mut drew_source = false;
    for elem in elems {
      let name = match elem.kind {
        ExprKind::Path(_) | ExprKind::Field(..) if !elem.span.from_expansion() => {
          expr_to_rap_name(elem, &self.tcx).filter(|n| self.raps.contains_key(n))
        }
        _ => None,
      };
      let ty = self.tcx.typeck(elem.hir_id.owner).node_type(elem.hir_id);
      let Some(name) = name else { continue };
      if ty.is_ref() {
        continue;
      }
      let is_copy = self.ty_is_copy(ty, elem.hir_id.owner);
      let e = if is_copy { Evt::Copy } else { Evt::Move };
      self.match_rhs(lhs.clone(), elem, e);
      drew_source = true;
      // A struct moved into the array takes its fields with it.
      if !is_copy {
        let mut stack = self.struct_children(&name);
        while let Some(child) = stack.pop() {
          stack.extend(self.struct_children(child.name()));
          if !child.is_copy() {
            self.add_ev(line_num, Evt::Move, ResourceTy::Anonymous, ResourceTy::Value(child), false);
          }
        }
      }
    }
    if !drew_source && !matches!(lhs, ResourceTy::Anonymous) {
      self.add_ev(line_num, Evt::Bind, lhs, ResourceTy::Anonymous, false);
    }
  }

  /// `get_rap`, falling back to `field_access_through_ref` for a field
  /// behind a reference (`match self.kind`, `if let Some(n) =
  /// &cur.next`).
  pub fn rap_or_field_through_ref(&self, expr: &'tcx Expr<'tcx>) -> ResourceTy {
    if let Some(tmp) = self.part_temp_of(expr) {
      return ResourceTy::Value(tmp);
    }
    match get_rap(expr, &self.tcx, &self.raps) {
      ResourceTy::Anonymous => self.field_access_through_ref(expr).unwrap_or(ResourceTy::Anonymous),
      rty => rty,
    }
  }

  /// The `(temporary N)` column `expr` takes something out of, when
  /// `expr` is a field of a temporary that it is moved out of
  /// (`make_p().name`, #469).
  pub fn part_temp_of(&self, expr: &'tcx Expr<'tcx>) -> Option<ResourceAccessPoint> {
    let mut e = expr;
    while let ExprKind::Field(base, _) = e.kind {
      e = base;
    }
    self.temp_cols.get(&e.hir_id).cloned()
  }

  /// The resource read or borrowed by `expr` (`p.x`, `&p.name`,
  /// `&mut r.a`, `self.n`) when it is a field reached through a
  /// reference: `DerefField(p, "p.x")`, drawn on p's column like a
  /// read or reborrow of `*p` (#266, #202). `None` for anything else.
  pub fn field_access_through_ref(&self, expr: &'tcx Expr<'tcx>) -> Option<ResourceTy> {
    let mut place = expr;
    while let ExprKind::AddrOf(_, _, inner) | ExprKind::DropTemps(inner) = place.kind {
      place = inner;
    }
    self.field_through_ref(place)
  }

  /// The owner's column for the place a reference `r` points to, plus
  /// the field path `suffix` (`""` or `".a.b"`): follows `r`'s lender
  /// through reborrows (`&mut *q`) and field reborrows (`&mut q.a`)
  /// until it reaches a local owner, e.g. `s.a` for `let q = &mut s;
  /// let r = &mut q.a;`. `None` when the chain ends anywhere else (a
  /// parameter's caller, an anonymous temporary) or the field has no
  /// column. A reference is never returned: it doesn't own (#323).
  pub fn owner_behind_ref(&self, r: &ResourceAccessPoint, suffix: &str) -> Option<ResourceAccessPoint> {
    self.owner_behind_ref_at(r, suffix, 0)
  }

  fn owner_behind_ref_at(&self, r: &ResourceAccessPoint, suffix: &str, depth: usize) -> Option<ResourceAccessPoint> {
    // `cur = &mut cur.next;` makes cur its own lender; give up rather
    // than loop.
    if depth > 16 {
      return None;
    }
    match &self.borrow_map.get(r.name())?.lender {
      ResourceTy::Value(l) if !l.is_ref() => {
        if suffix.is_empty() {
          return Some(l.clone());
        }
        self.raps.get(&format!("{}{}", l.name(), suffix))
          .map(|rd| rd.rap.clone())
          .filter(|f| matches!(f, ResourceAccessPoint::Struct(_)))
      }
      // `let r = q;` with q a reference (a copy or move of q), or
      // `&mut *q`: r points where q points.
      ResourceTy::Value(q) | ResourceTy::Deref(q) => self.owner_behind_ref_at(q, suffix, depth + 1),
      // `&mut q.a`: r points to q's pointee's field `.a`.
      ResourceTy::DerefField(q, place, _) => {
        let field = place.strip_prefix(q.name().as_str()).filter(|f| f.starts_with('.'))?;
        self.owner_behind_ref_at(q, &format!("{}{}", field, suffix), depth + 1)
      }
      ResourceTy::Anonymous | ResourceTy::Caller | ResourceTy::Index(..) | ResourceTy::Temporary { .. } => None,
    }
  }

  /// Before pairing the `break` value of `loop_id` with a receiver
  /// known only after the loop: if the value was deferred to the
  /// loop's end (a `break` in a branch, #431), place the events there.
  /// Returns what `leave_deferred` restores.
  fn enter_deferred(&mut self, loop_id: rustc_hir::HirId) -> (Option<usize>, Vec<String>) {
    match self.deferred_lines.iter().position(|(id, ..)| *id == loop_id) {
      Some(i) => {
        let (_, line, names) = self.deferred_lines.remove(i);
        (self.line_override.replace(line), std::mem::replace(&mut self.break_out, names))
      }
      None => (self.line_override, self.break_out.clone()),
    }
  }

  fn leave_deferred(&mut self, saved: (Option<usize>, Vec<String>)) {
    self.line_override = saved.0;
    self.break_out = saved.1;
  }

  /// The `break` values recorded for `loop_expr` while its receiver
  /// wasn't known (#452), with each `break` as written.
  fn take_loop_breaks(&mut self, loop_expr: &'tcx Expr<'tcx>) -> Vec<(&'tcx Expr<'tcx>, Option<String>)> {
    let (mine, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.loop_breaks)
      .into_iter().partition(|(id, ..)| *id == loop_expr.hir_id);
    self.loop_breaks = rest;
    mine.into_iter().map(|(_, v, t)| (v, t)).collect()
  }

  /// Whether `e` mentions a local variable declared inside `scope`.
  fn mentions_local_declared_in(&self, e: &'tcx Expr<'tcx>, scope: rustc_span::Span) -> bool {
    struct Mentions<'a, 'tcx>(rustc_middle::ty::TyCtxt<'tcx>, rustc_span::Span, &'a mut bool);
    impl<'a, 'tcx> rustc_hir::intravisit::Visitor<'tcx> for Mentions<'a, 'tcx> {
      fn visit_expr(&mut self, e: &'tcx Expr<'tcx>) {
        if local_of_path(e).is_some_and(|l| self.1.contains(self.0.hir_span(l))) {
          *self.2 = true;
        }
        rustc_hir::intravisit::walk_expr(self, e);
      }
    }
    let mut found = false;
    rustc_hir::intravisit::Visitor::visit_expr(&mut Mentions(self.tcx, scope, &mut found), e);
    found
  }

  // add events for arguments in a function call
  pub fn match_arg(&mut self, arg: &'tcx Expr, fn_name: String) {
    // `f(loop { … break k; })`: what `break` carries is the argument
    // (#452).
    if let ExprKind::Loop(_, _, rustc_hir::LoopSource::Loop, _) = arg.kind {
      let seen = self.take_loop_breaks(arg);
      let deferred = self.deferred_lines.iter().any(|(id, ..)| *id == arg.hir_id);
      let saved = self.enter_deferred(arg.hir_id);
      // A single `break` that wasn't in a branch was paired where it
      // is; the call runs once the loop has ended, on its last line,
      // where its lenders' borrows end too (#677).
      // (Not a value that mentions a variable the loop declares: that
      // goes out of scope at the loop's end, after its move.)
      let end_line = (!deferred && seen.len() == 1
        && !self.mentions_local_declared_in(seen[0].0, arg.span))
        .then(|| span_end_line(&arg.span, &self.tcx));
      let saved_line = end_line.map(|l| self.line_override.replace(l));
      if seen.len() > 1 {
        // Several: only the `break` that ended the loop ran (#561).
        self.pair_break_alternatives(arg, seen, |this, v| this.match_arg(v, fn_name.clone()));
      } else {
        for (value, _) in seen {
          self.match_arg(value, fn_name.clone());
        }
      }
      if let Some(l) = saved_line {
        self.line_override = l;
      }
      self.leave_deferred(saved);
      return;
    }
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
    // An array (or `vec![…]`) built in the argument list
    // (`f([s, t])`, `f(vec![s])`): each element is passed into the
    // call with it, so `s` is moved into the call (#297).
    let built_elems = match arg.kind {
      ExprKind::Array(elems) if !arg.span.from_expansion() => Some(elems),
      _ => vec_macro_elems(arg),
    };
    if let Some(elems) = built_elems {
      self.add_fn(fn_name.clone());
      for elem in elems.iter() {
        // `take([&s])`: the borrow is made for the call too (#705).
        let is_var = matches!(elem.kind, ExprKind::Path(_) | ExprKind::Field(..) | ExprKind::AddrOf(..));
        if is_var {
          self.match_arg(elem, fn_name.clone());
        }
      }
      return;
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
    // `take(p.x)` / `look(&p.name)` with p a reference (#266).
    let from_ro = self.rap_or_field_through_ref(arg);
    // Extend the lifetime of any reference RAP this arg touches so a
    // ref passed as a fn argument shows its dotted lifetime line out
    // to the call site. Critical for macro args like
    // `println!("{}", p)`: visit_expr bails on macro-expanded
    // children before reaching the inner Path that would normally
    // call update_rap, so without this nudge the ref's lifetime
    // stays pinned to its declaration line and the renderer draws a
    // degenerate (zero-height) ref-line.
    if let ResourceTy::Value(rap) | ResourceTy::Deref(rap) | ResourceTy::DerefField(rap, _, _) = &from_ro {
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
            implicit: ImplicitRef::No, result_keeps_borrow: false, stored_in, break_alt: None,
          }
        } else {
          ExternalEvent::PassByStaticReference {
            from: from_ro.clone(), to: to_ro, id, implicit: ImplicitRef::No, refcell_borrow_mut: false, result_keeps_borrow: false, stored_in, break_alt: None,
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
          // `f(&mut r.a)` with r a reference reborrows through r too.
          let is_reborrow = matches!(from_ro, ResourceTy::DerefField(..)) || match arg.kind {
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
    // So does a `loop` for the values its `break`s carry (`let r =
    // loop { break ra; }` borrows what `ra` borrows, #585). A value
    // naming a variable declared in the loop isn't known before the
    // loop is visited; `inherit_break_lenders` adds it when its `break`
    // is paired.
    let loop_tails = || loop_break_tails(expr)
      .map(|ts| ts.into_iter().filter(|t| self.locals_registered(t)).collect::<Vec<_>>());
    if let Some(tails) = branch_tails(expr).or_else(loop_tails) {
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

  /// Whether every variable `e` names has a column already.
  fn locals_registered(&self, e: &'tcx Expr<'tcx>) -> bool {
    use rustc_hir::intravisit::{self, Visitor};
    struct Check<'b, 'tcx> { tcx: TyCtxt<'tcx>, raps: &'b HashMap<String, RapData>, ok: bool }
    impl<'b, 'tcx> Visitor<'tcx> for Check<'b, 'tcx> {
      fn visit_expr(&mut self, e: &'tcx Expr<'tcx>) {
        if let ExprKind::Path(QPath::Resolved(None, p)) = e.kind {
          if matches!(p.res, Res::Local(_)) && !self.raps.contains_key(&path_key(p, &self.tcx)) {
            self.ok = false;
          }
        }
        intravisit::walk_expr(self, e);
      }
    }
    let mut c = Check { tcx: self.tcx, raps: &self.raps, ok: true };
    c.visit_expr(e);
    c.ok
  }

  /// `let r = loop { … break k; }` with `r` a reference: once a
  /// `break` value is paired with `r`, `r` borrows what the loop's
  /// `break` values borrow, as `let r = k;` would (#585).
  /// `define_lhs` could only see the values whose variables were
  /// declared before the loop.
  pub fn inherit_break_lenders(&mut self, lhs: &ResourceTy, loop_id: rustc_hir::HirId) {
    let ResourceTy::Value(r) = lhs else { return };
    if !r.is_ref() || !self.borrow_map.contains_key(r.name()) || self.holder_shadowed(r.name()) {
      return;
    }
    let rustc_hir::Node::Expr(loop_expr) = self.tcx.hir_node(loop_id) else { return };
    let (lender, aliasing, static_lender, co_lenders) = self.get_ref_data_full(loop_expr);
    if let Some(rd) = self.borrow_map.get_mut(r.name()) {
      rd.lender = lender;
      rd.aliasing = aliasing;
      rd.static_lender = static_lender;
      rd.co_lenders = co_lenders;
    }
  }

  fn get_ref_data_single(&self, expr: &'tcx Expr<'tcx>) -> (ResourceTy, VecDeque<String>){
    // ex:
    // let a = &b (where b has type &&i32)
    // a's aliases consist of a -> (b -> ... -> ...)
    if is_addr(&expr) {
      // in this case we are borrowing from b; `&p.name` with p a
      // reference is a reborrow of the field through p (#266).
      let lender = self.rap_or_field_through_ref(&expr);
      let mut aliasing = get_aliasing_data(&lender, &self.borrow_map);
      // if expr has type &&
      let ty = self.tcx.typeck(expr.hir_id.owner).node_type(expr.hir_id);
      if ty.builtin_deref(false).unwrap().is_ref() {
        match &lender {
          // (`get_rap` never yields DerefField, but `&&p.f` through a
          // reference does: there is no aliasing chain to extend.)
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

  /// The reference `expr` names and the loan it holds, for
  /// `ref_carriers`.
  pub fn source_ref_loan(&self, expr: &Expr) -> Option<(String, LoanId)> {
    let expr = match expr.kind {
      ExprKind::DropTemps(inner) => inner,
      _ => expr,
    };
    if !matches!(expr.kind, ExprKind::Path(_) | ExprKind::Field(..)) {
      return None;
    }
    let name = expr_to_rap_name(expr, &self.tcx)?;
    let loan = self.borrow_map.get(&name)?.loan()?;
    Some((name, loan))
  }

  /// `carrier` was just given a new value: `src`'s loan, if it took a
  /// reference's (see `ref_carriers`).
  pub fn note_ref_carrier(&mut self, carrier: &str, src: Option<(String, LoanId)>) {
    self.ref_carriers.retain(|(c, _, _)| c != carrier);
    self.taken_over_loans.remove(carrier);
    let Some((s, sp)) = src else { return };
    if s == carrier {
      return;
    }
    if self.taken_over_loans.remove(&s) {
      if let Some(rd) = self.borrow_map.get_mut(&s) {
        rd.borrow_span = None;
        rd.loan_line = None;
      }
      self.take_over_loan(carrier, sp);
    } else {
      self.ref_carriers.push((carrier.to_owned(), s, sp));
    }
  }

  /// `r` now holds `loan` (see `ref_carriers`).
  pub fn take_over_loan(&mut self, r: &str, loan: LoanId) {
    if let Some(rd) = self.borrow_map.get_mut(r) {
      match loan {
        LoanId::Span(sp) => rd.borrow_span = Some(sp),
        LoanId::Line(l) => rd.loan_line = Some(l),
      }
      self.taken_over_loans.insert(r.to_owned());
    }
  }

  /// The line rustc's borrow checker ends the loan `rd` holds on.
  pub fn loan_end(&self, rd: &RefData) -> Option<usize> {
    match rd.loan()? {
      LoanId::Span(sp) => self.gather_borrow_data(self.bwf).iter()
        .find(|b| b.span == sp).map(|b| b.region.1),
      LoanId::Line(l) => match &rd.lender {
        ResourceTy::Value(lender) if !lender.is_fn() => self.loan_end_line(l, lender.name()),
        _ => None,
      },
    }
  }

  /// `src` (its old data `old`) is being given a new value: the
  /// references that took its loan keep it, and the borrow checker
  /// ends it where it ends the loan (see `ref_carriers`).
  pub fn hand_loan_to_carriers(&mut self, src: &str, old: &RefData) {
    let carriers: Vec<(String, LoanId)> = self.ref_carriers.iter()
      .filter(|(_, s, loan)| s == src && old.loan() == Some(*loan))
      .map(|(c, _, loan)| (c.clone(), *loan))
      .collect();
    for (c, loan) in carriers {
      if self.borrow_map.get(&c).is_some_and(|rd| rd.loan().is_none() && rd.lender == old.lender) {
        self.take_over_loan(&c, loan);
      }
    }
    self.ref_carriers.retain(|(_, s, _)| s != src);
  }

  // Add a RAP to our collection of raps for a let stmt
  pub fn define_lhs(&mut self, name: String, mutability: bool, expr: &'tcx Expr, ty: Ty <'tcx>) {
    let is_special = ty_is_special_owner(&ty);
    if ty.is_ref() {
      let (lender, aliasing, static_lender, co_lenders) = self.get_ref_data_full(&expr);
      let src_loan = self.source_ref_loan(expr);
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
          rd.loan_line = call_loan_line(expr, rd.assigned_at);
          rd.co_lenders = co_lenders;
        }
        self.note_ref_carrier(&name, src_loan);
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
    // `let b = Box::new(P { .. });` with a field moved out through the
    // box (`take(b.name)`, #602): drawn like the plain struct, with a
    // column per field, so the move and what is left to drop show.
    else if let Some(inner) = self.box_with_field_moved_out(&name, ty) {
      let owner_hash = self.rap_hashes as u64;
      self.add_struct(name.clone(), owner_hash, false, mutability, false, false, InteriorMut::No, self.current_scope, !self.inside_branch);
      if let Some(ResourceAccessPoint::Struct(s)) = self.raps.get_mut(&name).map(|rd| &mut rd.rap) {
        s.is_box = true;
      }
      self.register_struct_members(&name, inner, owner_hash, mutability, MemberSite {
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
            self.mark_owner_type(&name, ty);
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
          let evt = if old.mut_of(&l) { Evt::MDie } else { Evt::SDie };
          self.add_ev(end, evt, l, holder.clone(), false);
        }
      }
      self.borrow_map.remove(&name);
    }
    let (mut lenders, is_mut) = self.holder_lenders(rhs, ty);
    let lender_muts = self.holder_lender_muts(rhs);
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
      loan_line: None,
      co_lenders: lenders,
      lender_muts,
    });
  }

  /// `hold = H { .. };` overwrites a struct whose reference fields
  /// borrow: the old value is dropped, so each of those borrows ends
  /// at the overwrite (or earlier, where MIR ends the loan because the
  /// old value was last used), with the usual return to the lender
  /// (#538). Returns the ends to draw, `(line, lenders, field, mutable)`;
  /// call before the struct literal's initialisers overwrite the field
  /// loans in `borrow_map`, and draw them with `draw_struct_field_borrow_ends`.
  pub fn struct_field_borrow_ends(&self, st: &ResourceAccessPoint, line: usize) -> Vec<(usize, Vec<ResourceTy>, ResourceTy, bool)> {
    let mir_b_data = self.gather_borrow_data(self.bwf);
    let mut ends = Vec::new();
    for member in self.struct_members_deep(st.name()) {
      if !member.is_ref() {
        continue;
      }
      let Some(old) = self.borrow_map.get(member.name()) else { continue };
      if !matches!(old.lender, ResourceTy::Value(_) | ResourceTy::Deref(_)) || old.assigned_at > line {
        continue;
      }
      // `let h2 = hold; hold = H { r: &u };`: the old value went to
      // `h2` with its borrow, which ends with `h2` (#637).
      if self.ref_handed_off(member.name()) {
        continue;
      }
      // The loan is live while the old value is used and dead after:
      // if MIR ends it at a later use, it ends there; otherwise (no
      // use since the borrow was stored) it ends at the overwrite.
      let kill = mir_b_data.iter()
        .find_map(|m| ExprVisitor::borrow_match(old, m))
        .filter(|&kill| kill > old.assigned_at);
      let lenders = old.all_lenders().into_iter()
        .filter(|l| matches!(l, ResourceTy::Value(_) | ResourceTy::Deref(_)))
        .collect();
      // A struct field of a struct (`hold.inner = I { r: &t };`) is a
      // partial write: `hold` stays live, and its type has one
      // lifetime for both the old and the new reference, so the borrow
      // checker keeps the old loan until hold's last use, as for
      // `hold.r = &t;` (#531). Drawn there on the lender's column only.
      if st.is_member() {
        if let Some(kill) = kill.filter(|&kill| kill > line) {
          let place = ResourceTy::Index(member.clone(), member.name().to_owned(), PlaceWrite::Update);
          ends.push((kill, lenders, place, old.ref_mutability));
          continue;
        }
      }
      let end = kill.map_or(line, |kill| kill.min(line));
      ends.push((end, lenders, ResourceTy::Value(member), old.ref_mutability));
    }
    ends
  }

  /// `hold = make(&t);` / `hold = other;` (#569): after
  /// `struct_field_borrow_ends` has read the old field loans, the
  /// reference fields hold the new value's borrows instead. A call's are
  /// what its signature ties each field to, as for `let hold = make(&t);`
  /// (`register_struct_field_lenders`), resolved before the old ones are
  /// cleared (`make(hold.r)` borrows what `hold.r` borrowed); for any
  /// other right-hand side the fields keep no lender of their own, as
  /// for `let hold = other;`. (A struct literal's initialisers set them
  /// in `match_rhs`.) Without this, the old lender stayed recorded:
  /// `make`'s borrow was drawn from `s` instead of `t`.
  pub fn reassign_struct_field_lenders(&mut self, st: &ResourceAccessPoint, rhs: &'tcx Expr<'tcx>, ty: Ty<'tcx>, line: usize) {
    let new: Vec<(String, Vec<ResourceTy>)> = match (rhs.kind, ty.ty_adt_def()) {
      (ExprKind::Call(..) | ExprKind::MethodCall(..), Some(adt)) => adt.all_fields()
        .map(|f| (format!("{}.{}", st.name(), f.name.as_str()), self.call_field_lenders(rhs, f.name.as_str()).0))
        .collect(),
      _ => Vec::new(),
    };
    for member in self.struct_members_deep(st.name()) {
      if let Some(rd) = self.borrow_map.get_mut(member.name()) {
        rd.lender = ResourceTy::Anonymous;
        rd.co_lenders = Vec::new();
        rd.aliasing = VecDeque::new();
        rd.static_lender = None;
        rd.borrow_span = None;
        rd.loan_line = None;
        rd.assigned_at = line;
      }
    }
    for (fname, mut lenders) in new {
      if lenders.is_empty() || !self.borrow_map.contains_key(&fname) {
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

  /// `hold = H { r: &t };`: the reference fields' loans in
  /// `borrow_map` are made at this write; the literal's initialisers
  /// set their lenders (`match_rhs`).
  pub fn restart_struct_field_loans(&mut self, st: &ResourceAccessPoint, line: usize) {
    for member in self.struct_members_deep(st.name()) {
      if let Some(rd) = self.borrow_map.get_mut(member.name()) {
        rd.assigned_at = line;
      }
    }
  }

  /// Draws the ends `struct_field_borrow_ends` found, once the new value
  /// is in place. A lender the field's new value borrows too
  /// (`hold = make(hold.r)`, `hold = H { r: hold.r }`) stays borrowed by
  /// it: no end is drawn there, only where the new borrow ends.
  pub fn draw_struct_field_borrow_ends(&mut self, ends: Vec<(usize, Vec<ResourceTy>, ResourceTy, bool)>) {
    for (end, lenders, holder, is_mut) in ends {
      let now = holder.extract_rap()
        .and_then(|h| self.borrow_map.get(h.name()))
        .map(|rd| rd.all_lenders())
        .unwrap_or_default();
      for l in lenders {
        if now.contains(&l) {
          continue;
        }
        self.add_ev(end, if is_mut { Evt::MDie } else { Evt::SDie }, l, holder.clone(), false);
      }
    }
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
      let evt = if rd.mut_of(&l) { Evt::MBorrow } else { Evt::SBorrow };
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
    let lender_muts = self.holder_lender_muts(init);
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
      loan_line: None,
      co_lenders: lenders,
      lender_muts,
    });
  }

  /// `let g = keep(|| s.len());`: `g` holds the closure `keep`
  /// returned, and that closure holds the reference its capture made,
  /// so `s` stays borrowed while `g` is in use, not just during the
  /// call (#686). rustc's facts say which loans `g` holds once the
  /// call has returned (`loans_kept_after_call`). Each is drawn as a
  /// capture borrow into `g` that returns where the loan ends, as for
  /// `let f = || s.len();`. Called after `match_rhs` drew `g`'s value
  /// arriving from the call.
  pub fn draw_kept_closure_borrows(&mut self, name: &str, init: &'tcx Expr<'tcx>) {
    let (func, args): (String, Vec<&'tcx Expr<'tcx>>) = match init.kind {
      ExprKind::Call(f, args) => match hirid_to_var_name(f.hir_id, &self.tcx) {
        Some(n) => (n, args.iter().collect()),
        None => return,
      },
      ExprKind::MethodCall(seg, rcvr, args, _) =>
        (seg.ident.as_str().to_owned(), std::iter::once(rcvr).chain(args.iter()).collect()),
      _ => return,
    };
    let Some(to_rap) = self.raps.get(name).map(|rd| rd.rap.to_owned()) else { return };
    if !to_rap.is_closure() {
      return;
    }
    let to = ResourceTy::Value(to_rap);
    let line = expr_to_line(init, &self.tcx);
    for k in self.loans_kept_after_call(init.span) {
      if k.holder != name {
        continue;
      }
      // A capture made by a closure literal in the arguments is drawn
      // on the captured place's column (`p.name`), as `let f = || ..`
      // draws it, when it's the closure's only capture of that variable.
      let literal_capture = args.iter().find_map(|a| match a.kind {
        ExprKind::Closure(c) if a.span.contains(k.span) => {
          let typeck = self.tcx.typeck(c.def_id);
          let caps: Vec<_> = typeck.closure_min_captures_flattened(c.def_id)
            .filter(|cap| matches!(cap.info.capture_kind, rustc_middle::ty::UpvarCapture::ByRef(_)))
            .filter(|cap| matches!(cap.place.base, rustc_middle::hir::place::PlaceBase::Upvar(u)
              if binding_name(&self.tcx, u.var_path.hir_id) == k.lender))
            .collect();
          match caps.as_slice() {
            [cap] => self.capture_rap(&cap.place, &k.lender),
            _ => None,
          }
        }
        _ => None,
      });
      let Some(lender) = literal_capture.or_else(|| self.raps.get(&k.lender).map(|rd| rd.rap.to_owned())) else { continue };
      if lender.is_fn() {
        continue;
      }
      let from = ResourceTy::Value(lender.clone());
      let via = BorrowVia {
        func: func.clone(),
        lenders: vec![lender.name().to_owned()],
        also_unnamed: false,
        tie: ArgTie::default(),
        carrier: ViaCarrier::ReturnedClosure { mutable: k.is_mut, through_ref: k.through_ref },
      };
      let id = *self.unique_id;
      *self.unique_id += 1;
      let ev = if k.is_mut {
        ExternalEvent::MutableBorrow { from: from.clone(), to: to.clone(), is_partial: false, is_reborrow: k.through_ref, break_alt: None, via: Some(via), id }
      } else {
        ExternalEvent::StaticBorrow { from: from.clone(), to: to.clone(), is_partial: false, is_reborrow: k.through_ref, break_alt: None, via: Some(via), id }
      };
      self.update_rap(&lender, line);
      self.add_external_event(line, ev);
      // add_ev maps (lhs, rhs) to a Die { from: rhs, to: lhs }: the
      // borrow goes back from g to s, as in the closure literal arm.
      let die = if k.is_mut { Evt::MDie } else { Evt::SDie };
      self.add_ev(k.end_line.max(line), die, from, to.clone(), false);
    }
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
          loan_line: None,
          co_lenders: lenders[1..].to_vec(),
          lender_muts: Vec::new(),
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
        ExternalEvent::MutableBorrow { from: lender.clone(), to: to.clone(), is_partial: false, is_reborrow: false, break_alt: None, via: Some(via.clone()), id }
      } else {
        ExternalEvent::StaticBorrow { from: lender.clone(), to: to.clone(), is_partial: false, is_reborrow: false, break_alt: None, via: Some(via.clone()), id }
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
          loan_line: None,
          co_lenders: Vec::new(),
          lender_muts: Vec::new(),
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
    let part = |i: Option<usize>, sub: &Pat<'tcx>| -> Option<(ResourceTy, Ty<'tcx>)> {
      let (field, field_ty) = fields.get(i?)?;
      // A reference part has a column too, but only a plain by-value
      // binding of it (`P { part: r }`) copies / moves the reference
      // out of that part (#709); `ref r`, `&x`, `_` and the like
      // are matched against the whole parent as before.
      if field_ty.is_ref() && !is_by_value_binding(sub) {
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
          (sp, part(Some(i), sp))
        }).collect()
      }
      PatKind::Struct(_, pat_fields, _) => pat_fields.iter().map(|pf| {
        let i = fields.iter().position(|(f, _)| f.as_str() == pf.ident.name.as_str());
        (pf.pat, part(i, pf.pat))
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
    // An or-pattern nested in the pattern (`(a, (x, _) | (_, x))`):
    // like a whole arm's or-pattern, its first alternative stands for
    // it (`or_arm_stand_in`, #612), so a name it binds from the same
    // part in every alternative is bound from that part.
    if let PatKind::Or(alts) = pat.kind {
      if let Some(first) = alts.first() {
        self.get_dec_of_pat_parts(first, ty_results, parent, parent_ty, scope, res);
      }
      return;
    }
    match self.pat_parts(pat, parent, parent_ty) {
      Some(subs) => {
        for (sp, part) in subs {
          match part {
            Some((part_rty, part_ty)) => {
              let mut sub = Vec::new();
              self.get_dec_of_pat_parts(sp, ty_results, &part_rty, part_ty, scope, &mut sub);
              // `P { part: r }` with `part: &T` copies the reference
              // out of `p.part`: `r` borrows what `p.part` borrows (#709).
              if part_ty.is_ref() {
                for (r, _, from, _) in sub.iter() {
                  self.adopt_part_borrow(r.name(), &from.name());
                }
              }
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

  /// The reference `name` was just bound by copying (or, for `&mut`,
  /// moving) the reference stored in the part `part` (`p.part`): it
  /// borrows what `part` borrows, not the struct `part` is in (#709).
  pub fn adopt_part_borrow(&mut self, name: &str, part: &str) {
    let Some(src) = self.borrow_map.get(part).cloned() else { return };
    let aliasing = get_aliasing_data(&src.lender, &self.borrow_map);
    if let Some(rd) = self.borrow_map.get_mut(name) {
      rd.lender = src.lender;
      rd.aliasing = aliasing;
      rd.static_lender = src.static_lender;
      rd.co_lenders = src.co_lenders;
      rd.borrow_span = src.borrow_span;
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
        // `Ok(x)`, and a tuple sub-pattern (`(a, (n, b))` in a match
        // arm, `if let (Some(a), _) = …`; #327): every binding inside
        // destructures out of the same parent.
        PatKind::TupleStruct(_, tuple_members, _) | PatKind::Tuple(tuple_members, _) => {
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
            let name = binding_name(&self.tcx, id);
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
                // A by-value binding of the parent's own reference type
                // is a copy of that reference, not a borrow through it
                // (`P { part: r }` on `p.part: &T`, #709).
                let through_parent = parent_ty.is_ref() && *parent_ty != ty;
                let lender = if by_ref || through_parent { parent.clone() } else { ResourceTy::Anonymous };
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
                            self.mark_owner_type(&name, ty);
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
            } else if parent_ty.is_ref() && *parent_ty != ty {
                if bool_of_mut(parent_ty.ref_mutability().unwrap()) { Evt::MBorrow }
                else { Evt::SBorrow }
            } else {
                if self.tcx.type_is_copy_modulo_regions(rustc_middle::ty::TypingEnv::post_analysis(self.tcx, pat.hir_id.owner), ty) { Evt::Copy }
                else { Evt::Move }
            };

            res.push((self.raps.get(&name).unwrap().rap.to_owned(), evt, parent_ty.clone()));
        }
        // `Ok(s) | Err(s)`: every alternative binds the same names, with
        // the same types and binding modes, out of the same matched
        // value, so whichever one matches, the bindings are the first
        // alternative's (#503).
        PatKind::Or(alts) => {
            if let Some(first) = alts.first() {
                self.get_dec_of_pat2(first, ty_results, parent, parent_ty, scope, res);
            }
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
    // An unbound temporary (#457) is dropped by its own event, at the
    // end of its statement, not with the block.
    let mut vars: Vec<ResourceAccessPoint> = vars.into_iter().filter(|r| is_block_local(r) && needs_scope_end(r)).collect();
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
/// `If`'s guard slot. With `emit_gos_at = Some(line)` the bindings'
/// GoOutOfScope is emitted at `line` right away; the callers pass
/// `None` and end the bindings themselves (the if-let path from its
/// `if_decl` set, the while-let path after visiting the body).
///
/// Returns the set of bindings registered. The if-let caller unions
/// these into its `if_decl` set so `append_decl_branch_events` can
/// populate each binding's timeline with a Branch-shaped event;
/// without that, the renderer's `fetch_timeline` panics looking for
/// the Copy/Move event id in an empty history.
///
/// Also returns the first event id that isn't a copy out of the
/// scrutinee. The copies are emitted first, so a caller that draws the
/// other bindings inside the branch can leave the copies on the
/// timeline before it: copying a value out of the scrutinee changes
/// nothing about it, so it doesn't make the scrutinee's column branch
/// (#496).
///
/// The caller is responsible for separately visiting the scrutinee
/// (for any reads/borrows it performs).
pub fn register_iflet_let_bindings(
    &mut self,
    let_expr: &'tcx rustc_hir::LetExpr<'tcx>,
    emit_gos_at: Option<usize>,
) -> (HashSet<ResourceAccessPoint>, usize) {
    let scrutinee = let_expr.init;
    let typeck_res = self.tcx.typeck(scrutinee.hir_id.owner);
    let scrutinee_ty = typeck_res.node_type(scrutinee.hir_id);
    // `if let Some(n) = &cur.next` with cur a reference (#266).
    let parent = self.rap_or_field_through_ref(scrutinee);

    let let_line = crate::expr_visitor_utils::span_to_line(&let_expr.span, &self.tcx);
    // Body-end is only needed if we're going to emit a GoOutOfScope.
    // get_dec_of_pat2 also takes a `scope` param; pass let_line as a
    // safe default for the if-let case (the renderer ignores it for
    // the binding's own RAP since is_global=false).
    let scope = emit_gos_at.unwrap_or(let_line);

    // An or-pattern that binds a name from a different place in each
    // alternative (`if let (x, _) | (_, x) = u`, `if let (Some(x), _) |
    // (_, Some(x)) = (a, b)`): drawn like such a match arm (#533,
    // #597), `x` as a new value and each place as moved into it only if
    // its alternative matched. The parents are the tuple literal's
    // elements, as for `match (a, b)`.
    let (or_parents, or_parents_ty): (Vec<ResourceTy>, Vec<Ty<'tcx>>) = match scrutinee.kind {
        ExprKind::Tup(elems) if !scrutinee.span.from_expansion() => (
            elems.iter().map(|e| self.rap_or_field_through_ref(e)).collect(),
            elems.iter().map(|e| typeck_res.node_type(e.hir_id)).collect(),
        ),
        _ => (vec![parent.clone()], vec![scrutinee_ty]),
    };
    let moved_into_scrutinee = self.tuple_scrutinee_moves(scrutinee);
    let (pat, or_conflicts) = self.or_arm_stand_in(let_expr.pat, &or_parents, &or_parents_ty, let_line, typeck_res, &moved_into_scrutinee);

    // `if let (Some(a), Some(b)) = (x, y)`: like `match (x, y)`, each
    // sub-pattern destructures its own element of the tuple literal
    // (#327). Otherwise the whole pattern matches the scrutinee.
    let pairs: Vec<(&'tcx Pat<'tcx>, ResourceTy, Ty<'tcx>)> = match (pat.kind, scrutinee.kind) {
        (PatKind::Tuple(sub_pats, ddpos), ExprKind::Tup(elems)) if !scrutinee.span.from_expansion() => {
            let gap = elems.len().saturating_sub(sub_pats.len());
            sub_pats.iter().enumerate().map(|(k, sp)| {
                let i = match ddpos.as_opt_usize() {
                    Some(d) if k >= d => k + gap,
                    _ => k,
                };
                let elem = &elems[i];
                (sp, crate::expr_visitor_utils::get_rap(elem, &self.tcx, &self.raps), typeck_res.node_type(elem.hir_id))
            }).collect()
        }
        _ => vec![(pat, parent.clone(), scrutinee_ty)],
    };
    let mut decls = Vec::new();
    for (sp, sp_parent, sp_parent_ty) in pairs {
        let mut sub = Vec::new();
        self.get_dec_of_pat_parts(sp, typeck_res, &sp_parent, sp_parent_ty, scope, &mut sub);
        let sp_ty = typeck_res.node_type(sp.hir_id);
        for (r, e, from, partial) in sub {
            let from = self.retarget_tuple_borrow(scrutinee, &r, &e, from);
            decls.push((r, e, from, Some(partial.unwrap_or(sp_parent_ty != sp_ty))));
        }
    }

    let tuple_name = match self.tuple_scrutinee_target(scrutinee) {
        ResourceTy::Temporary { tuple: Some(t), .. } => t,
        _ => String::new(),
    };

    // Copies first (stable, so each group keeps pattern order).
    let (mut copies, others): (Vec<_>, Vec<_>) =
        decls.into_iter().partition(|(_, evt, _, _)| matches!(evt, Evt::Copy));
    // A copied or-pattern name is just bound: nothing moves.
    let _ = self.if_let_or_conflict_events(&or_conflicts, &mut copies, let_line, &moved_into_scrutinee, &tuple_name);
    let mut first_other = None;
    let mut registered: HashSet<ResourceAccessPoint> = HashSet::new();
    // The moves into an or-pattern's name happen only when the pattern
    // matches: in the branch (their ids follow the copies), drawn after
    // the bindings and the moves into a tuple scrutinee, which holds
    // the values they may take.
    let mut or_events: Vec<ExternalEvent> = Vec::new();
    let mut moved_by_pattern: Vec<ResourceTy> = Vec::new();
    // The borrows the bindings take from what the scrutinee borrows
    // (#391) follow all the bindings of their group: with `Some((l,
    // r))` on a call, the renderer stacks `a -> l` and `b -> r` on rows
    // of their own, and r's copy listed after `a -> l` was drawn below
    // its own borrow.
    let mut borrows: Vec<ExternalEvent> = Vec::new();
    let mut groups = [copies, others];
    for g in 0..groups.len() {
      if g == 1 && !groups[1].is_empty() {
        for b in std::mem::take(&mut borrows) {
            self.add_external_event(let_line, b);
        }
        first_other = Some(*self.unique_id);
        or_events = self.if_let_or_conflict_events(&or_conflicts, &mut groups[1], let_line, &moved_into_scrutinee, &tuple_name);
      }
      for (binding_rap, evt, from, is_partial) in std::mem::take(&mut groups[g]) {
        if matches!(evt, Evt::Move) {
            moved_by_pattern.push(from.clone());
        }
        let evt_kind = evt.clone();
        let event = self.ext_ev_of_evt(
            evt,
            ResourceTy::Value(binding_rap.clone()),
            from,
            *self.unique_id,
            // (None: an or-pattern's name, bound as a new value.)
            is_partial.unwrap_or(false),
        );
        self.add_external_event(let_line, event);
        *self.unique_id += 1;
        borrows.extend(self.pattern_ref_copy_borrow(&binding_rap, &evt_kind, scrutinee));

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
    }
    for b in borrows {
        self.add_external_event(let_line, b);
    }
    // A tuple-literal scrutinee moves its variables into the tuple
    // whether or not the pattern matches: show the ones the pattern
    // didn't move out itself (`Some(_)`, `_`) as moved too.
    let end_of_bindings = first_other.unwrap_or(*self.unique_id);
    let into_tuple = self.tuple_scrutinee_target(scrutinee);
    for r in moved_into_scrutinee {
        if !moved_by_pattern.contains(&ResourceTy::Value(r.clone())) {
            let event = self.ext_ev_of_evt(Evt::Move, into_tuple.clone(), ResourceTy::Value(r), *self.unique_id, false);
            *self.unique_id += 1;
            self.add_external_event(let_line, event);
        }
    }
    for ev in or_events {
        self.add_external_event(let_line, ev);
    }
    (registered, end_of_bindings)
}

/// A by-value pattern binding of reference type out of a value that
/// isn't a reference (`Some(x)` on `third: Option<&i32>`, or on
/// `v.get(1)` directly) copies a reference stored in that value
/// (#304), so it borrows what that reference borrows. The matched
/// value's lenders are the variables a holder borrows (#336), or those
/// a call's signature ties its result to (#171); rustc's lifetimes say
/// which of them the binding's reference can point into. Record them
/// as the binding's lenders and return the borrow arrows to draw on
/// the pattern's line (#391). `pick(&a, &b)` returning `Option<&'a
/// String>` gives `x` both `a` and `b` (#681); a `(&'a str, &'b str)`
/// gives each element only its own.
///
/// A struct binding (`Some(x)` on `o: Option<Part>`) gets the same for
/// each of its reference fields (`x.part`; #692).
pub fn pattern_ref_copy_borrow(
  &mut self,
  binding: &ResourceAccessPoint,
  evt: &Evt,
  scrutinee: &'tcx Expr<'tcx>,
) -> Vec<ExternalEvent> {
  if !matches!(evt, Evt::Copy | Evt::Move) {
    return Vec::new();
  }
  let typeck = self.tcx.typeck(scrutinee.hir_id.owner);
  if typeck.expr_ty(scrutinee).is_ref() {
    return Vec::new();
  }
  let moved = matches!(evt, Evt::Move);
  if binding.is_ref() {
    return self.pattern_ref_borrows(binding, binding.name(), &[], moved, false, scrutinee);
  }
  if !matches!(binding, ResourceAccessPoint::Struct(Struct { is_member: false, .. })) {
    return Vec::new();
  }
  let bound = binding.name().to_owned();
  let mut out = Vec::new();
  for member in self.struct_members_deep(&bound) {
    if !member.is_ref() {
      continue;
    }
    let path = member.name()[bound.len() + 1..].to_owned();
    let fields: Vec<&str> = path.split('.').collect();
    let evs = self.pattern_ref_borrows(&member, &bound, &fields, moved, true, scrutinee);
    if !evs.is_empty() {
      // The arrow says where the field's reference came from, in
      // place of the dot that couldn't name a lender (#352).
      let is_init = |ev: &ExternalEvent| matches!(ev, ExternalEvent::InitRefParam { param, in_pattern: Some(_), .. } if *param == member);
      self.preprocessed_events.retain(|(_, ev)| !is_init(ev));
      for evs_on_line in self.event_line_map.values_mut() {
        evs_on_line.retain(|ev| !is_init(ev));
      }
    }
    out.extend(evs);
  }
  out
}

/// `pattern_ref_copy_borrow` for one reference: the binding itself
/// (`fields` empty) or a reference field `bound.f` of a struct binding
/// (`in_field`).
fn pattern_ref_borrows(
  &mut self,
  reference: &ResourceAccessPoint,
  bound: &str,
  fields: &[&str],
  moved: bool,
  in_field: bool,
  scrutinee: &'tcx Expr<'tcx>,
) -> Vec<ExternalEvent> {
  let name = reference.name().to_owned();
  // `match third { Some(third) => .. }` shares third's column (#253);
  // the holder's own borrow already covers it.
  if self.holder_shadowed(&name) {
    return Vec::new();
  }
  // Not yet given a lender: Anonymous for a binding, the `Caller`
  // placeholder for a struct binding's reference field (#352).
  // (A `let ... else` binds the struct's fields with `Anonymous`, #718.)
  let unset = |l: Option<&ResourceTy>| match l {
    Some(ResourceTy::Anonymous) => true,
    Some(ResourceTy::Caller) => in_field,
    _ => false,
  };
  if !unset(self.borrow_map.get(&name).map(|rd| &rd.lender)) {
    return Vec::new();
  }
  let mut lenders: Vec<ResourceTy> = Vec::new();
  for l in self.value_lenders(scrutinee) {
    if matches!(l, ResourceTy::Value(_) | ResourceTy::Deref(_)) && !lenders.contains(&l) {
      lenders.push(l);
    }
  }
  // Which of them the reference can point into: the variables whose
  // loans rustc puts in its lifetime. A lender that is itself a
  // reference (a parameter standing for the caller's data, #393)
  // lends without a loan in this body, so it stays. A loan of the
  // other kind (`&mut k` in the lifetime of a `&str` field that shares
  // it) isn't drawn: the arrow would call it a borrow of the
  // reference's own kind.
  let is_mut = reference.is_mutref();
  match self.loans_held_by(bound, fields) {
    Some(held) => lenders.retain(|l| match l {
      ResourceTy::Value(r) if !r.is_ref() => {
        let base = r.name().split('.').next().unwrap_or_default();
        held.get(base).is_some_and(|m| *m == is_mut)
      }
      _ => true,
    }),
    // Without rustc's lifetimes, only a single lender is certain.
    None if lenders.len() > 1 || in_field => return Vec::new(),
    None => {}
  }
  if lenders.is_empty() {
    return Vec::new();
  }
  let (func, from_call) = match scrutinee.kind {
    ExprKind::MethodCall(seg, ..) => (seg.ident.as_str().to_owned(), true),
    ExprKind::Call(fn_expr, _) => match hirid_to_var_name(fn_expr.hir_id, &self.tcx) {
      Some(f) => (f, true),
      None => return Vec::new(),
    },
    _ => match expr_to_rap_name(scrutinee, &self.tcx) {
      Some(n) => (n, false),
      None => return Vec::new(),
    },
  };
  let aliasing = get_aliasing_data(&lenders[0], &self.borrow_map);
  if let Some(rd) = self.borrow_map.get_mut(&name) {
    rd.lender = lenders[0].clone();
    rd.co_lenders = lenders[1..].to_vec();
    rd.aliasing = aliasing;
  }
  self.pattern_copies.insert(name.clone());
  if from_call {
    // The call's result keeps the borrow past the call (#171).
    self.mark_borrow_kept_by_result(scrutinee, &lenders);
  }
  // The signature's tie describes the reference only when the result
  // has a single lifetime; the tooltip leaves it out otherwise.
  let one_lifetime = from_call && self.call_result_has_one_lifetime(scrutinee);
  let via = BorrowVia {
    func,
    lenders: lenders.iter().map(|l| l.name()).collect(),
    also_unnamed: one_lifetime && self.call_lenders_full(scrutinee).1,
    tie: if one_lifetime { self.call_arg_tie(scrutinee, None) } else { ArgTie::default() },
    carrier: ViaCarrier::PatternBinding { from_call, moved, field_of: in_field },
  };
  let to = ResourceTy::Value(reference.clone());
  let mut out = Vec::new();
  for lender in lenders {
    let id = *self.unique_id;
    *self.unique_id += 1;
    let via = Some(via.clone());
    out.push(if is_mut {
      ExternalEvent::MutableBorrow { from: lender, to: to.clone(), is_partial: false, is_reborrow: false, break_alt: None, via, id }
    } else {
      ExternalEvent::StaticBorrow { from: lender, to: to.clone(), is_partial: false, is_reborrow: false, break_alt: None, via, id }
    });
  }
  out
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
  // A holder mixing `&` and `&mut` records each lender's kind itself
  // (`Some(W { a: &s, b: &mut t })`, #708).
  let own = match self.borrow_map.get(lhs_rap.name()) {
    Some(rd) if !rd.lender_muts.is_empty() => rd.lender_muts.iter().map(|(l, _)| l.clone()).collect(),
    _ => self.call_lenders(call),
  };
  let muts: Vec<(ResourceTy, bool)> = self.borrow_map.get(lhs_rap.name()).map(|rd| rd.lender_muts.clone()).unwrap_or_default();
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
    let is_mut = muts.iter().find(|(l, _)| l == lender).map_or(is_mut, |(_, m)| *m);
    let ev = if is_mut {
      ExternalEvent::MutableBorrow { from: lender.clone(), to: lhs.clone(), is_partial: false, is_reborrow: false, break_alt: None, via: Some(via.clone()), id }
    } else {
      ExternalEvent::StaticBorrow { from: lender.clone(), to: lhs.clone(), is_partial: false, is_reborrow: false, break_alt: None, via: Some(via.clone()), id }
    };
    self.add_external_event(line_num, ev);
    *self.unique_id += 1;
  }
  self.mark_borrow_kept_by_result(call, &lenders);
  true
}

/// `lhs` is a struct bound to `call`'s result whose reference fields
/// `register_struct_field_lenders` (or `reassign_struct_field_lenders`)
/// linked to the caller's variables: draw a borrow from each lender
/// into each such field, and keep the lenders borrowed past the call
/// (#336). `lhs` may be a struct field of a struct (`o.inner = mk(&t)`,
/// `O { inner: mk(&t), .. }`, #638).
pub fn emit_struct_field_borrows(&mut self, lhs: &ResourceTy, call: &'tcx Expr<'tcx>, func: String, line_num: usize) {
  let ResourceTy::Value(ResourceAccessPoint::Struct(st)) = lhs else { return };
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
        ExternalEvent::MutableBorrow { from: lender.clone(), to: to.clone(), is_partial: false, is_reborrow: false, break_alt: None, via: Some(via.clone()), id }
      } else {
        ExternalEvent::StaticBorrow { from: lender.clone(), to: to.clone(), is_partial: false, is_reborrow: false, break_alt: None, via: Some(via.clone()), id }
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

/// The variables a tuple-literal scrutinee (`if let … = (x, y)`,
/// `match (x, y) { … }`) moves into the tuple it builds: each element
/// that names a variable of a non-Copy, non-reference type. They are
/// moved whether or not the pattern then matches, and whether or not it
/// binds anything from them, so `x` can't be used afterwards (#327).
/// Variables whose parts have columns of their own (a struct or tuple
/// variable) are left out: marking only the whole as moved would leave
/// their part columns claiming a drop.
pub fn tuple_scrutinee_moves(&self, scrutinee: &'tcx Expr<'tcx>) -> Vec<ResourceAccessPoint> {
    let ExprKind::Tup(elems) = scrutinee.kind else { return Vec::new() };
    if scrutinee.span.from_expansion() {
        return Vec::new();
    }
    let typeck = self.tcx.typeck(scrutinee.hir_id.owner);
    elems.iter().filter_map(|e| {
        let ExprKind::Path(QPath::Resolved(_, path)) = e.kind else { return None };
        if !matches!(path.res, Res::Local(_)) {
            return None;
        }
        let ty = typeck.node_type(e.hir_id);
        if ty.is_ref() || self.ty_is_copy(ty, e.hir_id.owner) {
            return None;
        }
        match crate::expr_visitor_utils::get_rap(e, &self.tcx, &self.raps) {
            ResourceTy::Value(rap @ ResourceAccessPoint::Owner(_)) => Some(rap),
            _ => None,
        }
    }).collect()
}

/// A `ref` / `ref mut` binding out of a tuple-literal scrutinee's
/// element (`(Some(ref a), _)` on `(x, y)`) borrows that element of the
/// temporary tuple, not `x`, whose value was moved into the tuple
/// (#580). Its lender is the tuple, which has no column: drawn like a
/// borrow of any other temporary (`Anonymous`).
pub fn tuple_elem_lender(&self, scrutinee: &'tcx Expr<'tcx>, evt: &Evt, from: ResourceTy) -> ResourceTy {
  if matches!(evt, Evt::SBorrow | Evt::MBorrow) {
    if let ResourceTy::Value(r) = &from {
      if self.tuple_scrutinee_moves(scrutinee).contains(r) {
        return ResourceTy::Anonymous;
      }
    }
  }
  from
}

/// `tuple_elem_lender` for the binding `binding`, also pointing the
/// borrow its `ref` registered (add_ref) at the tuple, so the borrow's
/// end isn't drawn as giving x its value back (#580).
pub fn retarget_tuple_borrow(&mut self, scrutinee: &'tcx Expr<'tcx>, binding: &ResourceAccessPoint, evt: &Evt, from: ResourceTy) -> ResourceTy {
  let lender = self.tuple_elem_lender(scrutinee, evt, from.clone());
  if lender != from {
    self.tuple_temp_refs.insert(binding.name().to_string());
    if let Some(rd) = self.borrow_map.get_mut(binding.name()) {
      if rd.lender == from {
        rd.lender = lender.clone();
      }
    }
  }
  lender
}

/// Where a tuple-literal scrutinee's variables go: the move into the
/// tuple, labelled with its source text (`(p, q)`; empty when it's
/// too long to quote).
pub fn tuple_scrutinee_target(&self, scrutinee: &'tcx Expr<'tcx>) -> ResourceTy {
  let text = self.tcx.sess.source_map().span_to_snippet(scrutinee.span).ok()
    .map(|t| t.split_whitespace().collect::<Vec<_>>().join(" "))
    .filter(|t| t.chars().count() <= 32)
    .unwrap_or_default();
  ResourceTy::Temporary { extended: false, tuple: Some(text) }
}

/// The tuple-literal scrutinee's variables (`tuple_scrutinee_moves`)
/// whose value, or part of it, is still in the tuple after `pat`
/// matched (`None`: the pattern didn't match, so all of it), and is
/// dropped with the tuple (#327). Only types that need drop: the
/// others have nothing to drop.
pub fn tuple_scrutinee_left(&self, scrutinee: &'tcx Expr<'tcx>, pat: Option<&'tcx Pat<'tcx>>) -> Vec<(ResourceAccessPoint, TupleLeft)> {
  let ExprKind::Tup(elems) = scrutinee.kind else { return Vec::new() };
  let moved = self.tuple_scrutinee_moves(scrutinee);
  let typeck = self.tcx.typeck(scrutinee.hir_id.owner);
  let env = rustc_middle::ty::TypingEnv::post_analysis(self.tcx, scrutinee.hir_id.owner);
  let mut out = Vec::new();
  for (i, e) in elems.iter().enumerate() {
    let rap = match crate::expr_visitor_utils::get_rap(e, &self.tcx, &self.raps) {
      ResourceTy::Value(r) if moved.contains(&r) => r,
      _ => continue,
    };
    if !typeck.node_type(e.hir_id).needs_drop(self.tcx, env) {
      continue;
    }
    let left = match pat {
      None => Some(false),
      Some(p) => self.tuple_elem_left(p, i, elems.len(), typeck, env),
    };
    if let Some(rest) = left {
      let takers = match pat {
        Some(p) => self.tuple_elem_takers(p, i, elems.len(), typeck),
        None => Vec::new(),
      };
      out.push((rap.clone(), TupleLeft { name: rap.name().clone(), rest, rc: rap.rc_drop(), takers }));
    }
  }
  out
}

/// What `pat`, matched against a tuple literal of `n` elements, leaves
/// of element `i` in the tuple: all of it (`Some(false)`), part of it
/// (`Some(true)`) or nothing (`None`).
fn tuple_elem_left(&self, pat: &'tcx Pat<'tcx>, i: usize, n: usize,
  typeck: &'tcx rustc_middle::ty::TypeckResults<'tcx>, env: rustc_middle::ty::TypingEnv<'tcx>) -> Option<bool> {
  match pat.kind {
    PatKind::Tuple(subs, dd) => {
      let gap = n.saturating_sub(subs.len());
      let k = match dd.as_opt_usize() {
        Some(d) if i >= d && i < d + gap => return Some(false),
        Some(d) if i >= d + gap => i - gap,
        _ => i,
      };
      subs.get(k).and_then(|sp| self.pat_leaves(sp, typeck, env))
    }
    // `(Some(a), _) | (_, Some(a))`: which alternative matched decides.
    // When they differ, "the rest of" covers both (all of it, or
    // what's left after the move).
    PatKind::Or(alts) => {
      let each: Vec<Option<bool>> = alts.iter().map(|a| self.tuple_elem_left(a, i, n, typeck, env)).collect();
      if each.iter().all(|l| *l == each[0]) { each[0] } else { Some(true) }
    }
    // `t => …` takes the whole tuple; `_`, `ref t` leave it.
    _ => self.pat_leaves(pat, typeck, env),
  }
}

/// For an or-pattern whose alternatives leave different amounts of
/// element `i` of a tuple literal of `n` elements in the tuple (`(Some(x),
/// _) | (_, Some(x))`, #533): the names that move some of it out in the
/// alternatives that do, in order. What's left in the tuple then
/// depends on which alternative matched at run time. Empty otherwise.
fn tuple_elem_takers(&self, pat: &'tcx Pat<'tcx>, i: usize, n: usize,
  typeck: &'tcx rustc_middle::ty::TypeckResults<'tcx>) -> Vec<String> {
  let PatKind::Or(alts) = pat.kind else { return Vec::new() };
  let env = rustc_middle::ty::TypingEnv::post_analysis(self.tcx, pat.hir_id.owner);
  let each: Vec<Option<bool>> = alts.iter().map(|a| self.tuple_elem_left(a, i, n, typeck, env)).collect();
  if each.iter().all(|l| *l == each[0]) {
    return Vec::new();
  }
  let mut out: Vec<String> = Vec::new();
  for alt in alts {
    let PatKind::Tuple(subs, dd) = alt.kind else { continue };
    let gap = n.saturating_sub(subs.len());
    let k = match dd.as_opt_usize() {
      Some(d) if i >= d && i < d + gap => continue,
      Some(d) if i >= d + gap => i - gap,
      _ => i,
    };
    let Some(sub) = subs.get(k) else { continue };
    sub.walk_always(|p| {
      if let PatKind::Binding(_, _, ident, _) = p.kind {
        let by_ref = matches!(typeck.pat_binding_modes().get(p.hir_id).map(|m| m.0), Some(rustc_hir::ByRef::Yes(..)));
        let name = ident.to_string();
        if !by_ref && !self.ty_is_copy(typeck.node_type(p.hir_id), p.hir_id.owner) && !out.contains(&name) {
          out.push(name);
        }
      }
    });
  }
  out
}

/// What matching `pat` against an owned value leaves of it: all of it
/// (`Some(false)`: nothing is moved out), part of it that needs drop
/// (`Some(true)`), or nothing that needs drop (`None`).
fn pat_leaves(&self, pat: &'tcx Pat<'tcx>, typeck: &'tcx rustc_middle::ty::TypeckResults<'tcx>, env: rustc_middle::ty::TypingEnv<'tcx>) -> Option<bool> {
  let mut moves = false;
  pat.walk_always(|p| {
    if let PatKind::Binding(..) = p.kind {
      let by_ref = matches!(typeck.pat_binding_modes().get(p.hir_id).map(|m| m.0), Some(rustc_hir::ByRef::Yes(..)));
      if !by_ref && !self.ty_is_copy(typeck.node_type(p.hir_id), p.hir_id.owner) {
        moves = true;
      }
    }
  });
  if !moves {
    Some(false)
  } else if self.pat_leaves_drop(pat, typeck, env) {
    Some(true)
  } else {
    None
  }
}

/// Whether, after `pat` matched an owned value and its by-value
/// bindings moved out what they bind, a part that needs drop is left
/// in the value: under a `_`, a `ref` binding, a literal or a field
/// skipped by `..`.
fn pat_leaves_drop(&self, pat: &'tcx Pat<'tcx>, typeck: &'tcx rustc_middle::ty::TypeckResults<'tcx>, env: rustc_middle::ty::TypingEnv<'tcx>) -> bool {
  use rustc_middle::ty::TyKind;
  let tcx = self.tcx;
  let ty = typeck.node_type(pat.hir_id);
  if !ty.needs_drop(tcx, env) {
    return false;
  }
  // Whether a field the pattern skips with `..` needs drop: `tys` are
  // the fields' types, of which `m` sub-patterns cover all but the
  // ones from index `d` on.
  let skipped_drop = |d: usize, m: usize, tys: Vec<Ty<'tcx>>| {
    let gap = tys.len().saturating_sub(m);
    tys.into_iter().skip(d).take(gap).any(|t| t.needs_drop(tcx, env))
  };
  match pat.kind {
    PatKind::Binding(..) => {
      matches!(typeck.pat_binding_modes().get(pat.hir_id).map(|m| m.0), Some(rustc_hir::ByRef::Yes(..)))
    }
    PatKind::Tuple(subs, dd) => {
      if subs.iter().any(|sp| self.pat_leaves_drop(sp, typeck, env)) {
        return true;
      }
      let Some(d) = dd.as_opt_usize() else { return false };
      let TyKind::Tuple(tys) = ty.kind() else { return true };
      skipped_drop(d, subs.len(), tys.iter().collect())
    }
    PatKind::TupleStruct(ref qpath, subs, dd) => {
      if subs.iter().any(|sp| self.pat_leaves_drop(sp, typeck, env)) {
        return true;
      }
      let Some(d) = dd.as_opt_usize() else { return false };
      let TyKind::Adt(adt, args) = ty.kind() else { return true };
      let variant = adt.variant_of_res(typeck.qpath_res(qpath, pat.hir_id));
      skipped_drop(d, subs.len(), variant.fields.iter().map(|f| f.ty(tcx, args)).collect())
    }
    PatKind::Struct(ref qpath, fields, rest) => {
      if fields.iter().any(|f| self.pat_leaves_drop(f.pat, typeck, env)) {
        return true;
      }
      if rest.is_none() {
        return false;
      }
      let TyKind::Adt(adt, args) = ty.kind() else { return true };
      let variant = adt.variant_of_res(typeck.qpath_res(qpath, pat.hir_id));
      variant.fields.iter()
        .filter(|f| !fields.iter().any(|fp| fp.ident.name == f.name))
        .any(|f| f.ty(tcx, args).needs_drop(tcx, env))
    }
    PatKind::Or(alts) => alts.iter().any(|a| self.pat_leaves_drop(a, typeck, env)),
    // `_`, a literal, a unit variant, a `box` / deref pattern: what
    // they match stays.
    _ => true,
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
    // `vec![s, t]` / `vec![x; n]`: the elements the user wrote are
    // moved (or copied) into the new Vec, like an array literal's.
    if let Some(elems) = vec_macro_elems(rhs) {
      self.match_array_elems(lhs, elems, line_num);
      return;
    }
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
        _ => path_name(p, &self.tcx),
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
          if rhs_rap.is_closure() && lhs.is_closure() && matches!(evt, Evt::Copy | Evt::Move) {
            self.move_or_copy_closure(line_num, evt, lhs, rhs_rap);
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
          let from = expr_to_rap_name(fn_expr, &self.tcx)
            .and_then(|n| self.raps.get(&n))
            .map(|rd| ResourceTy::Value(rd.rap.to_owned()))
            .unwrap_or(ResourceTy::Anonymous);
          self.add_ev(line_num, evt, lhs, from, false);
          return;
        }
        LocalCallee::NotLocal => {}
      }
      // The callee is an expression (`(w.f)()`, `make()(x)`; #703):
      // the result is a new value the call returned.
      if !matches!(fn_expr.kind, ExprKind::Path(_)) {
        self.bind_from_anonymous(line_num, lhs);
        return;
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
        // `&p.name` with p a reference borrows the field through p (#266).
        None => match self.field_access_through_ref(rhs) {
          Some(rty) => {
            if let Some(r) = rty.deref_rap() {
              self.update_rap(&r.clone(), line_num);
            }
            rty
          }
          None => ResourceTy::Anonymous,
        },
      };
      // `&*r` / `&mut *r` with r a reference borrows *through* r (a
      // reborrow); `&r` / `&mut r` borrows the variable r itself.
      // Both resolve to the same source RAP, so decide here from the
      // syntax rather than from r's kind downstream. `&p.f` through a
      // reference p is a reborrow too.
      let is_reborrow = matches!(res, ResourceTy::DerefField(..)) || is_explicit_reborrow(
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
          // A struct field initialised by a call (`O { inner: mk(&t), .. }`):
          // its reference fields borrow what the callee's signature ties
          // them to, as for `let inner = mk(&t);` (#638).
          if matches!(field_rap, ResourceAccessPoint::Struct(_)) {
            self.register_struct_field_lenders(&new_lhs_name, field.expr, field_ty);
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
            // `..make_p()` whose temporary keeps fields that need
            // dropping: the field is moved out of its column (#605).
            None if !field_is_copy && matches!(dest, ResourceTy::Value(_)) && self.part_temp_of(base_expr).is_some() => {
              let tmp = self.part_temp_of(base_expr).unwrap();
              self.add_ev(base_line, Evt::Move, dest, ResourceTy::Value(tmp), true);
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
    // Array literal `[e0, e1]` / `[e; N]` (#297): the array's elements
    // have no columns, so it's one value; variables written as
    // elements are moved or copied into it.
    ExprKind::Array(elems) => {
      let line_num = span_to_line(&rhs.span, &self.tcx);
      self.match_array_elems(lhs, elems, line_num);
    }
    ExprKind::Repeat(elem, _) => {
      let line_num = span_to_line(&rhs.span, &self.tcx);
      self.match_array_elems(lhs, std::slice::from_ref(elem), line_num);
    }
    // `let first = v[0];` (#297): a copy of one element (moving out of
    // an index doesn't compile, E0507 / E0508). Drawn as a copy out of
    // `v[0]` on v's column; v keeps its resource.
    // Only into a variable: an element-to-element copy (`v[2] = v[0]`)
    // or a write through a reference (`*r = v[0]`) stays undrawn, as
    // before.
    // (A reference copied out of a collection whose stored borrows are
    // tracked, `let r = refs[0];`, is the arm below, #349.)
    ExprKind::Index(recv, _, _) if matches!(lhs, ResourceTy::Value(_))
      && !(lhs.is_ref() && !lhs.is_mutref() && self.stored_ref_lenders(rhs).is_some()) => {
      let line_num = span_to_line(&rhs.span, &self.tcx);
      let elem_ty = self.tcx.typeck(rhs.hir_id.owner).node_type(rhs.hir_id);
      let holder = expr_to_rap_name(recv, &self.tcx)
        .and_then(|n| self.raps.get(&n))
        .map(|rd| rd.rap.to_owned())
        .filter(|r| !r.is_fn());
      match holder {
        // An element that is itself a reference (`let r = refs[0];`)
        // copies a borrow whose lender we don't track; leave it as a
        // plain new binding.
        Some(rap) if !elem_ty.is_ref() && !rhs.span.from_expansion() => {
          self.update_rap(&rap, line_num);
          let place = self.place_label(rhs, rap.name());
          self.add_ev(line_num, evt, lhs, ResourceTy::Index(rap, place, Default::default()), false);
        }
        _ => self.add_ev(line_num, Evt::Bind, lhs, ResourceTy::Anonymous, false),
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
      } else if let Some(tmp) = self.part_temp_of(rhs) {
        // `let s = make_q().a;`: a field moved out of a temporary,
        // which keeps the rest (#469).
        let line_num = span_to_line(&inner.span, &self.tcx);
        self.add_ev(line_num, evt, lhs, ResourceTy::Value(tmp), true);
      } else if let (Evt::Move, Some(ResourceTy::Value(f))) = (&evt, field_call_root(rhs).map(|c| get_rap(c, &self.tcx, &self.raps))) {
        // `let s = make_p().name;` where nothing that needs dropping is
        // left in the temporary: the value comes from the call (#469).
        let line_num = span_to_line(&inner.span, &self.tcx);
        self.add_ev(line_num, evt, lhs, ResourceTy::Value(f), false);
      } else if let Some(rty) = self.field_access_through_ref(rhs) {
        // `let a = p.x;` / `self.n` as a tail expression, with p /
        // self a reference: a copy out of the resource p points to,
        // drawn on p's column (#266). (Moving out from behind a
        // reference doesn't compile, so this is always a copy.)
        let line_num = span_to_line(&inner.span, &self.tcx);
        if let Some(r) = rty.deref_rap() {
          self.update_rap(&r.clone(), line_num);
        }
        self.add_ev(line_num, evt, lhs, rty, false);
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
            // `let n = r?;` with `r` a non-`Copy` place: the `?` moves
            // `r`, and `n` gets the value inside it (#652). Drawn as a
            // move from `r` to `n`, in place of the move into the `?`
            // `note_try` drew if it got here first.
            if matches!(lhs, ResourceTy::Value(_) | ResourceTy::Caller) {
              if let Some((_, kind, _)) = self.try_moved_place(rhs) {
                if let Some(Some(id)) = self.try_moves.insert(rhs.hir_id, None) {
                  self.preprocessed_events.retain(|(_, e)| e.get_id() != id);
                }
                self.try_move = Some(kind);
                self.match_rhs(lhs, operand, Evt::Move);
                self.try_move = None;
                return;
              }
            }
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
    // `let r = loop { … break k; };`: `r` gets what `break` carries
    // out (#431). A `break` is visited with the loop body, which
    // `process_let_binding` does after this, so it only registers where
    // the values go; `visit_expr`'s `Break` arm pairs them.
    ExprKind::Loop(_, _, rustc_hir::LoopSource::Loop, _) => {
      let seen = self.take_loop_breaks(rhs);
      let saved = self.enter_deferred(rhs.hir_id);
      if seen.len() > 1 {
        // Several: only the `break` that ended the loop ran (#561).
        self.pair_break_alternatives(rhs, seen, |this, v| this.match_rhs(lhs.clone(), v, evt.clone()));
      } else {
        for (value, _) in seen {
          self.match_rhs(lhs.clone(), value, evt.clone());
        }
      }
      self.leave_deferred(saved);
      self.break_binds.push((rhs.hir_id, lhs, evt));
    }
    ExprKind::DropTemps(exp) => {
      self.match_rhs(lhs, &exp, evt);
    }
    // `let n = v.len() as i32;` / `let b = a as i64;` (#729): a cast
    // of a number reads its operand and makes a new value of the same
    // kind, so `n` is bound the way its operand would bind it (a copy of
    // `a`, of what `v.len()` returned, a fresh value for `x + 1`). A cast
    // to or from a reference or pointer has no such source: `n` just
    // gets a value.
    ExprKind::Cast(inner, _) => {
      let typeck = self.tcx.typeck(rhs.hir_id.owner);
      let plain = |e: &Expr| typeck.expr_ty_opt(e).is_some_and(|t| t.is_numeric() || t.is_bool() || t.is_char());
      if plain(inner) && plain(rhs) {
        self.match_rhs(lhs, inner, evt);
      } else {
        let line_num = span_to_line(&rhs.span, &self.tcx);
        self.bind_from_anonymous(line_num, lhs);
      }
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
        let upvar_name = binding_name(&self.tcx, upvar_hir_id);
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
          from: lender, to: lhs.clone(), is_partial: false, is_reborrow: false, break_alt: None, via: Some(via.clone()), id,
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
  //
  // Except the drop of an `unbranched_temps` temporary, which is
  // `early`: a dot that doesn't end its column.
  let exit_drops = std::mem::take(&mut self.exit_drops);
  self.preprocessed_events.retain(|(_, e)| !exit_drops.contains(&e.get_id())
    || matches!(e, ExternalEvent::TemporaryDrop { early: true, .. }));
  // Visit RAPs in declaration order, not `self.raps`' hash order, so
  // the GoOutOfScope events (and their ids) come out the same on
  // every run (#211).
  #[expect(clippy::disallowed_methods, reason = "sorted by (hash, name) on the next line")]
  let mut raps: Vec<RapData> = self.raps.values().cloned().collect();
  raps.sort_by(|a, b| a.rap.hash().cmp(b.rap.hash()).then_with(|| a.rap.name().cmp(b.rap.name())));
  for rap in raps.iter() {
    // need this to avoid duplicating out of scope events, this is due to the fact that RAPS is a map that lives over multiple fn ctxts
    // if a rap isn't global it means we annotated its GOS event inside its respective branch already
    // Every path left the function at a `return` / panic above, where
    // owners were dropped or moved out (#515); a reference has no
    // such event, and keeps its scope end.
    let left_already = self.all_paths_exit && !rap.rap.is_copy() && !rap.rap.is_ref();
    // An unbound temporary is dropped by its own `TemporaryDrop`, which
    // may sit in a branch (a let chain's later part, #604): no scope end.
    if !rap.rap.is_fn() && rap.is_global && !left_already && !rap.rap.is_unbound_temporary() {
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
      // The later variables that share this column (#589).
      for &scope in rap.extra_scopes.iter() {
        self.add_external_event(scope, ExternalEvent::GoOutOfScope { ro: rap.rap.clone(), id: *self.unique_id });
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
    // A struct binding's reference field that a pattern gave a lender
    // (`x.part` in `Some(x)`, #692) holds its loan while `x` is live:
    // its last use is a use of `x`, which the visitor doesn't record
    // for a field (`let q = x.part;`).
    let mut field_ends: Vec<(String, usize)> = Vec::new();
    #[expect(clippy::disallowed_methods, clippy::iter_over_hash_type, reason = "collected into a Vec, each entry raised on its own")]
    for name in self.pattern_copies.iter() {
      let Some((bound, _)) = name.split_once('.') else { continue };
      let Some(rd) = self.borrow_map.get(name) else { continue };
      if let Some(last) = self.last_live_line(bound, (rd.assigned_at, usize::MAX)) {
        field_ends.push((name.clone(), last));
      }
    }
    for (name, last) in field_ends {
      if let Some(rd) = self.borrow_map.get_mut(&name) {
        rd.lifetime = rd.lifetime.max(last);
      }
    }
    // A reference that a pattern binding reborrows through stays
    // borrowed as long as that binding does: `s` in `match o {
    // Some(s) => … }` (`o: &Option<String>`) points into `*o`, so the
    // borrow `o` holds can't end before `s`'s does (#372). The arm's
    // bindings left `borrow_map` at the arm's end (see
    // `borrow_floor`); `if let` bindings are still in it, with `o` as
    // their lender. Chains (`r2` borrowing through `r1` borrowing
    // through `o`) need the fixpoint.
    #[expect(clippy::disallowed_methods, clippy::iter_over_hash_type, reason = "each entry only raises its own lifetime")]
    for (name, floor) in self.borrow_floor.iter() {
      if let Some(rd) = self.borrow_map.get_mut(name) {
        rd.lifetime = rd.lifetime.max(*floor);
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
      let RefData { lender: r_ty, lifetime, co_lenders, .. } = ref_data;
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
          let evt = if ref_data.mut_of(co) { Evt::MDie } else { Evt::SDie };
          self.add_ev(*lifetime, evt, co.clone(), ResourceTy::Value(from_ro.clone()), false);
        }
      }
      // if ref is not an ultimate ref then it dies without returning a resource
      if !ultimate_refs.contains(&(k.to_owned(), r_ty.real_name())) { 
        // ... unless it is a reference a pattern copied out of a value
        // that holds the borrow (`while let Some(n) = it.next()`), and
        // only such values keep the lender borrowed past its last use:
        // its borrow is given back there, and the lender stays
        // borrowed by them (#575).
        let holders = self.holders_still_borrowing(k, ref_data, &lender_to_refs, &handed_off);
        if !holders.is_empty() {
          self.add_borrow_end_ev(*lifetime, to_ro, ResourceTy::Value(from_ro), ref_data, holders);
          continue;
        }
        self.add_external_event(*lifetime, ExternalEvent::RefDie { 
          from: ResourceTy::Value(from_ro.clone()), to: to_ro, num_curr_borrowers: get_borrowers(from_ro.name(), &self.borrow_map).len() - 1, 
          id: *self.unique_id });
        *self.unique_id += 1;
      }
      else {
        // otherwise it does return a resource
        self.add_borrow_end_ev(*lifetime, to_ro, ResourceTy::Value(from_ro), ref_data, Vec::new());
      }
    }
    self.nest_borrow_ends_in_arms();
    end_scrutinee_borrows_in_other_arms(self.preprocessed_events, self.unique_id);
    self.end_outer_borrows_in_arms();
    // A lender that got a column in a conditional just now (its borrow
    // through an outer reference ends in an arm) has one for the arm's
    // own references too: `if c { let h2 = h; .. }` hands h's borrow
    // to h2 in the arm, and h2's end belongs there (#637).
    self.nest_borrow_ends_in_arms();
    self.end_borrows_at_exits();
  }

  /// A reference declared before an `if` / `match` whose borrow ends
  /// after it, on a path through an arm that leaves early (`return`,
  /// a panic, a `break` / `continue` the borrow doesn't outlive): the
  /// reference is never used again on that path, so its borrow ends
  /// there, after the arm's last use of it, or where the arm starts if
  /// it doesn't use it. `print_lifetimes` drew the end only at the
  /// later use, which that path never reaches, so the arm left the
  /// lender lent, and an exit that drops the lender dropped it while
  /// lent (#663). Give each such arm its own end of the borrow, ahead
  /// of the exit's drops; several ending on one line end in reverse
  /// declaration order.
  fn end_borrows_at_exits(&mut self) {
    #[expect(clippy::disallowed_methods, reason = "collected into a map")]
    let assigned: HashMap<String, usize> = self.borrow_map.iter()
      .map(|(n, d)| (n.clone(), d.assigned_at))
      .collect();
    let mut cx = ExitCtx { scopes: &self.exit_scopes, assigned: &assigned, unique_id: self.unique_id };
    end_borrows_at_exits_in(self.preprocessed_events, &[], &mut cx);
  }

  /// For a reference `k` that a pattern copied (or moved) out of a
  /// value holding a borrow (`Some(n)` on `it.next()`, #391), and
  /// that isn't the last borrower of its lender: the borrowers that
  /// keep the lender borrowed after `k`'s last use, in declaration
  /// order, if they are all such values (`it`). `k`'s borrow is part
  /// of theirs: it ends at `k`'s last use, and theirs goes on (#575).
  /// Empty otherwise, and for any other reference: a reference that
  /// isn't the last to borrow its lender draws no return arrow.
  fn holders_still_borrowing(
    &self,
    k: &String,
    ref_data: &RefData,
    lender_to_refs: &HashMap<String, HashSet<String>>,
    handed_off: &HashSet<String>,
  ) -> Vec<String> {
    if !self.pattern_copies.contains(k) || !ref_data.co_lenders.is_empty()
      || !matches!(ref_data.lender, ResourceTy::Value(_) | ResourceTy::Deref(_))
    {
      return Vec::new();
    }
    let Some(refs) = lender_to_refs.get(&ref_data.lender.real_name()) else { return Vec::new() };
    let mut later: Vec<&RapData> = Vec::new();
    #[expect(clippy::disallowed_methods, clippy::iter_over_hash_type, reason = "sorted by declaration order below")]
    for r in refs.iter() {
      if r == k || handed_off.contains(r) {
        continue;
      }
      let Some(d) = self.borrow_map.get(r) else { continue };
      // Still borrowing after k's last use, and already before it.
      if d.lifetime > ref_data.lifetime && d.assigned_at <= ref_data.lifetime {
        match self.raps.get(r) {
          Some(rap) if rap.rap.holds_borrow() => later.push(rap),
          _ => return Vec::new(),
        }
      }
    }
    later.sort_by(|a, b| a.rap.hash().cmp(b.rap.hash()).then_with(|| a.rap.name().cmp(b.rap.name())));
    later.into_iter().map(|r| r.rap.name().to_owned()).collect()
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

/// Whether `events` (an arm's, not looking into nested conditionals)
/// move the reference `r` into another reference, which then holds
/// its borrow (see `ref_handed_off`).
fn handed_off_in(events: &[(usize, ExternalEvent)], r: &ResourceAccessPoint) -> bool {
  events.iter().any(|(_, e)| matches!(e,
    ExternalEvent::Move { from: ResourceTy::Value(f), to: ResourceTy::Value(t), .. } if f == r && t.is_ref()))
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
    // Never ahead of an event on an earlier line: an arm's rows are laid
    // out in event order, so an end listed right after the move into
    // the arm's pattern, with the `r`-using lines still to come, pushed
    // those lines a row down (#712).
    let after_touch = events.iter().rposition(|(l, e)| *l <= ev.0 && touches(e, r));
    let after_earlier = events.iter().rposition(|(l, _)| *l < ev.0);
    let at = after_touch.max(after_earlier).map_or(0, |i| i + 1);
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
      // `let h2 = h;` in this arm moved r's reference on (a `&mut`, or
      // a struct's field with the struct, #637): what holds it now
      // ends the borrow on this path, so r doesn't (#209).
      if handed_off_in(&arm.e_data, r) {
        continue;
      }
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

/// What `end_borrows_at_exits_in` needs from the visitor.
struct ExitCtx<'b> {
  scopes: &'b HashMap<usize, ExitScope>,
  /// Each reference's `RefData::assigned_at`.
  assigned: &'b HashMap<String, usize>,
  unique_id: &'b mut usize,
}

/// `(r, lender)` if `e` ends the borrow the reference `r` holds;
/// `lender` is what it gives the value back to (none for a `RefDie`).
fn borrow_end_of(e: &ExternalEvent) -> Option<(&ResourceAccessPoint, &ResourceTy)> {
  match e {
    ExternalEvent::StaticDie { from: ResourceTy::Value(r), to, .. }
    | ExternalEvent::MutableDie { from: ResourceTy::Value(r), to, .. }
    | ExternalEvent::RefDie { from: ResourceTy::Value(r), to, .. } => Some((r, to)),
    _ => None,
  }
}

/// The borrow ends among `events` and inside their conditionals' arms,
/// in order.
fn borrow_ends_in(events: &[(usize, ExternalEvent)], out: &mut Vec<(usize, ExternalEvent)>) {
  for (line, e) in events {
    match e {
      ExternalEvent::Branch { branches, .. } => {
        for b in branches {
          borrow_ends_in(&b.e_data, out);
        }
      }
      _ if borrow_end_of(e).is_some() => out.push((*line, e.clone())),
      _ => {}
    }
  }
}

/// Whether every path through `events` ends `r`'s borrow of `to`.
fn ends_on_every_path(events: &[(usize, ExternalEvent)], r: &ResourceAccessPoint, to: &ResourceTy) -> bool {
  events.iter().any(|(_, e)| match e {
    ExternalEvent::Branch { branches, branch_type, .. } => !matches!(branch_type, BranchType::Loop(..))
      && !branches.is_empty()
      && branches.iter().all(|b| ends_on_every_path(&b.e_data, r, to)),
    _ => borrow_end_of(e) == Some((r, to)),
  })
}

/// See `ExprVisitor::end_borrows_at_exits`. `after`: the borrow ends
/// that come after `events` on the path through them. Returns the
/// variables this gave events in `events`' conditionals, which need a
/// column in a conditional around them too.
fn end_borrows_at_exits_in(
  events: &mut [(usize, ExternalEvent)],
  after: &[(usize, ExternalEvent)],
  cx: &mut ExitCtx,
) -> HashSet<ResourceAccessPoint> {
  let mut out = HashSet::new();
  for k in 0..events.len() {
    if !matches!(events[k].1, ExternalEvent::Branch { .. }) {
      continue;
    }
    // The borrow ends after this conditional: later in `events`, then
    // after them.
    let mut later = Vec::new();
    borrow_ends_in(&events[k + 1..], &mut later);
    later.extend(after.iter().cloned());
    let scope = cx.scopes.get(&events[k].1.get_id()).copied();
    let ExternalEvent::Branch { branches, live_vars, branch_type, split_point, merge_point, .. } = &mut events[k].1 else {
      unreachable!("checked above")
    };
    #[expect(clippy::disallowed_methods, reason = "collected into a set")]
    let declared: HashSet<ResourceAccessPoint> = branches.iter()
      .flat_map(|b| b.decl_vars.iter().cloned())
      .collect();
    let mut added: HashSet<ResourceAccessPoint> = HashSet::new();
    for (b, arm) in branches.iter_mut().enumerate() {
      // Exits in conditionals nested in the arm first.
      added.extend(end_borrows_at_exits_in(&mut arm.e_data, &later, cx).into_iter()
        .filter(|v| !arm.decl_vars.contains(v)));
      let Some(scope) = scope else { continue };
      if !arm.diverges || arm.exits.is_empty() || scope.in_closure || matches!(branch_type, BranchType::Loop(..)) {
        continue;
      }
      // Whether the arm's exits leave a borrow that ends on `line`
      // and was made on `made`: a `return` or a panic leaves the
      // function; a `break` the loop, so only a borrow not used after
      // the loop; a `continue` the iteration, so only a borrow also
      // made in it.
      let leaves = |line: usize, made: usize, carries_r: bool| arm.exits.iter().all(|x| match x {
        BranchExit::Return | BranchExit::Panic | BranchExit::Other => true,
        // A `break` whose value mentions `r` carries the borrow out of
        // the loop, to whatever uses the value: it doesn't end here
        // (#662). In `take(loop { if c { break ra; } else { break rb; } })`
        // `a` stays borrowed until `take` returns on the loop's last line,
        // but on the path through `break rb` `ra` is dead and its borrow
        // of `a` ended before the arm.
        BranchExit::Break => !carries_r && scope.loop_lines.is_some_and(|(_, hi)| line <= hi),
        BranchExit::Continue => scope.loop_lines.is_some_and(|(lo, hi)| lo <= made && line <= hi),
        BranchExit::BreakBlock => false,
      });
      let mut seen: HashSet<(ResourceAccessPoint, ResourceTy)> = HashSet::new();
      // (line, index in the arm, r's declaration, the end).
      let mut ends: Vec<(usize, usize, u64, ExternalEvent)> = Vec::new();
      for (line, e) in &later {
        let Some((r, to)) = borrow_end_of(e) else { continue };
        if !seen.insert((r.clone(), to.clone())) {
          continue; // only the next end of this borrow
        }
        let Some(&made) = cx.assigned.get(r.name()) else { continue };
        // An end on the conditional's own lines that wasn't put into
        // its arms (`end_outer_borrows_in_arms`) can't be told apart
        // from the conditional; leave it.
        if *line < *merge_point || declared.contains(r) || made >= *split_point || !leaves(*line, made, arm.break_carries.iter().any(|n| n == r.name()))
          || to.extract_rap().is_some_and(|l| l.is_fn())
          || ends_on_every_path(&arm.e_data, r, to)
        {
          continue;
        }
        // After the arm's last use of r. An exit's own drop of r (a
        // `&mut` reference) isn't a use: the borrow ends before it.
        let last = arm.e_data.iter().rposition(|(_, ev)|
          !matches!(ev, ExternalEvent::GoOutOfScope { ro, .. } if ro == r) && touches(ev, r));
        let (at_line, at) = match last {
          Some(i) => (match &arm.e_data[i].1 {
            ExternalEvent::Branch { merge_point, .. } => *merge_point,
            _ => arm.e_data[i].0,
          }, i + 1),
          // The arm doesn't use r: on this path the borrow ended
          // before the arm.
          None => {
            let start = branch_type.get_start_end(b).0;
            (start, arm.e_data.iter().position(|(el, _)| *el >= start).unwrap_or(arm.e_data.len()))
          }
        };
        let mut copy = e.clone();
        match &mut copy {
          ExternalEvent::StaticDie { id, .. } | ExternalEvent::MutableDie { id, .. } | ExternalEvent::RefDie { id, .. } => {
            *id = *cx.unique_id;
            *cx.unique_id += 1;
          }
          _ => unreachable!("borrow_end_of matched"),
        }
        added.insert(r.clone());
        if let ExternalEvent::StaticDie { .. } | ExternalEvent::MutableDie { .. } = copy {
          if let Some(l) = to.extract_rap().filter(|l| *l != r) {
            added.insert(l.clone());
          }
        }
        ends.push((at_line, at, *r.hash(), copy));
      }
      // Ends on one line all go after its last use, in reverse
      // declaration order (inserting each in declaration order at the
      // same index puts the later one first).
      for i in 0..ends.len() {
        let last_at = ends.iter().filter(|e| e.0 == ends[i].0).map(|e| e.1).max().unwrap_or(ends[i].1);
        ends[i].1 = last_at;
      }
      ends.sort_by(|a, b| b.1.cmp(&a.1).then(a.2.cmp(&b.2)));
      for (line, at, _, ev) in ends {
        arm.e_data.insert(at, (line, ev));
      }
    }
    // What the arms' new events are on has a column in the conditional.
    added.retain(|v| !declared.contains(v));
    #[expect(clippy::disallowed_methods, reason = "inserted into a set")]
    live_vars.extend(added.iter().cloned());
    out.extend(added);
  }
  out
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
      // The start of an `else` branch that a let-chain condition may
      // have moved it before (#281). It held the value before the
      // condition, which moved it.
      ExternalEvent::LoopExitMove { ro, at: MaybeMovedAt::ChainFail { .. }, .. } if ro.name() == rap_name => {
        return Some(Ownership::Maybe);
      }
      // Moved into a name an or-pattern binds, if its alternative
      // matched (#533): moved on some paths.
      ExternalEvent::OrAltMove { ro, tuple: None, .. } if ro.name() == rap_name => {
        return Some(Ownership::Maybe);
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

/// `if let Some(a) = x.as_mut() { … } else { … }` (and the `match`
/// form, #693): the call lends `x` and its result keeps the borrow, so
/// `x` enters the conditional borrowed. Where a pattern binds that
/// borrow (`a`), it ends at the binding's last use, inside the arm
/// (`nest_borrow_ends_in_arms`). An arm whose pattern binds nothing of
/// it (the `else`, `None =>`) had nothing hold the borrow: it ended
/// before the arm. Without saying so there, `x` reached the join still
/// borrowed on that path, so its scope end said it dropped nothing.
/// End it first thing in each such arm (no arrow: nothing gives it
/// back).
fn end_scrutinee_borrows_in_other_arms(events: &mut Vec<(usize, ExternalEvent)>, unique_id: &mut usize) {
  for k in 0..events.len() {
    let (before, rest) = events.split_at_mut(k);
    let ExternalEvent::Branch { branches, live_vars, branch_type, .. } = &mut rest[0].1 else { continue };
    for arm in branches.iter_mut() {
      end_scrutinee_borrows_in_other_arms(&mut arm.e_data, unique_id);
    }
    if matches!(branch_type, BranchType::Loop(..)) {
      continue;
    }
    #[expect(clippy::disallowed_methods, reason = "sorted by declaration order below")]
    let mut lenders: Vec<ResourceAccessPoint> = live_vars.iter().cloned().collect();
    lenders.sort_by(|a, b| a.hash().cmp(b.hash()).then_with(|| a.name().cmp(b.name())));
    for lender in lenders {
      // The scrutinee's call lent it, and its result keeps the borrow.
      let mutable = match before.iter().rev().find(|(_, e)| touches(e, &lender)) {
        Some((_, ExternalEvent::PassByMutableReference { from: ResourceTy::Value(l), result_keeps_borrow: true, .. })) if *l == lender => true,
        Some((_, ExternalEvent::PassByStaticReference { from: ResourceTy::Value(l), result_keeps_borrow: true, .. })) if *l == lender => false,
        _ => continue,
      };
      // Arms whose pattern binds that borrow, and whether one of them
      // gives it back in the arm.
      let binds: Vec<bool> = branches.iter().map(|arm| binds_scrutinee_borrow(arm, &lender)).collect();
      if !branches.iter().zip(&binds).any(|(arm, b)| *b && gives_back(&arm.e_data, &lender)) {
        continue;
      }
      for (b, arm) in branches.iter_mut().enumerate() {
        if binds[b] {
          continue;
        }
        let id = *unique_id;
        *unique_id += 1;
        let (from, to) = (ResourceTy::Anonymous, ResourceTy::Value(lender.clone()));
        let ev = if mutable {
          ExternalEvent::MutableDie { from, to, id, static_lender: None, still_borrowed_by: Vec::new() }
        } else {
          ExternalEvent::StaticDie { from, to, id, static_lender: None, still_borrowed_by: Vec::new() }
        };
        let start = branch_type.get_start_end(b).0;
        let at = arm.e_data.iter().position(|(l, _)| *l >= start).unwrap_or(arm.e_data.len());
        arm.e_data.insert(at, (start, ev));
      }
    }
  }
}

/// Whether `arm`'s pattern binds a borrow of `lender` out of the value
/// the scrutinee's call returned (`Some(a)` on `x.as_mut()`).
fn binds_scrutinee_borrow(arm: &ExtBranchData, lender: &ResourceAccessPoint) -> bool {
  arm.e_data.iter().any(|(_, e)| match e {
    ExternalEvent::StaticBorrow { from: ResourceTy::Value(l), to: ResourceTy::Value(r), via: Some(via), .. }
    | ExternalEvent::MutableBorrow { from: ResourceTy::Value(l), to: ResourceTy::Value(r), via: Some(via), .. } =>
      l == lender && arm.decl_vars.contains(r) && matches!(via.carrier, ViaCarrier::PatternBinding { from_call: true, .. }),
    _ => false,
  })
}

/// Whether `events` (one arm's, nested conditionals included) give
/// `lender` its value back.
fn gives_back(events: &[(usize, ExternalEvent)], lender: &ResourceAccessPoint) -> bool {
  events.iter().any(|(_, e)| match e {
    ExternalEvent::StaticDie { to: ResourceTy::Value(l), still_borrowed_by, .. }
    | ExternalEvent::MutableDie { to: ResourceTy::Value(l), still_borrowed_by, .. } => l == lender && still_borrowed_by.is_empty(),
    ExternalEvent::Branch { branches, .. } => branches.iter().any(|b| gives_back(&b.e_data, lender)),
    _ => false,
  })
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
  let mut at = arm.e_data.iter().rposition(|(l, _)| *l <= ev.0).map_or(0, |i| i + 1);
  // An exit on the borrow's last line (`return r.len();`, a panic)
  // drops the lender there too (`drop_locals_on_exit`), and it was
  // emitted first. The borrow ends before the lender is dropped: put
  // its end ahead of that drop, or the lender would be dropped while
  // still lent, as owning nothing to drop (#645).
  let lender = match &ev.1 {
    ExternalEvent::StaticDie { to, .. } | ExternalEvent::MutableDie { to, .. }
    | ExternalEvent::RefDie { to, .. } => to.extract_rap(),
    _ => None,
  };
  if let Some(lender) = lender {
    if let Some(i) = arm.e_data[..at].iter().rposition(|(l, e)| *l == ev.0
      && matches!(e, ExternalEvent::GoOutOfScope { ro, .. } if ro == lender))
    {
      at = i;
    }
  }
  if in_line_map && ev.1.is_var_arrow_ev() {
    arm.line_map.entry(ev.0).or_default().push(ev.1.clone());
  }
  arm.e_data.insert(at, ev);
}

/// The values the `break`s of a `loop` carry out of it (`let r = loop
/// { … break ra; }`), as `branch_tails` gives a conditional's: `None`
/// for anything else, or a loop none of whose `break`s carries a value.
pub fn loop_break_tails<'tcx>(e: &'tcx Expr<'tcx>) -> Option<Vec<&'tcx Expr<'tcx>>> {
  use rustc_hir::intravisit::{self, Visitor};
  struct Find<'tcx> { id: rustc_hir::HirId, out: Vec<&'tcx Expr<'tcx>> }
  impl<'tcx> Visitor<'tcx> for Find<'tcx> {
    fn visit_expr(&mut self, e: &'tcx Expr<'tcx>) {
      if let ExprKind::Break(dest, Some(v)) = e.kind {
        if dest.target_id == Ok(self.id) {
          self.out.push(v);
        }
      }
      intravisit::walk_expr(self, e);
    }
  }
  let ExprKind::Loop(_, _, rustc_hir::LoopSource::Loop, _) = e.kind else { return None };
  let mut f = Find { id: e.hir_id, out: Vec::new() };
  intravisit::walk_expr(&mut f, e);
  (!f.out.is_empty()).then_some(f.out)
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

/// See `ExprVisitor::ty_is_plain`.
pub fn ty_is_plain<'tcx>(tcx: TyCtxt<'tcx>, ty: Ty<'tcx>, owner: rustc_hir::OwnerId) -> bool {
  let env = rustc_middle::ty::TypingEnv::post_analysis(tcx, owner);
  !tcx.type_is_copy_modulo_regions(rustc_middle::ty::TypingEnv::post_analysis(tcx, owner), ty)
    && !ty.needs_drop(tcx, env)
    && !ty_has_std_sync_primitive(tcx, ty)
    && !ty.is_union()
    && !ty.is_closure()
    && !ty.ty_adt_def().map_or(false, |d| d.is_manually_drop())
    && !ty.is_ref()
}

fn ty_has_std_sync_primitive<'tcx>(tcx: TyCtxt<'tcx>, ty: Ty<'tcx>) -> bool {
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
  go(tcx, ty, 0, &mut HashSet::new())
}

/// `pat` is a plain by-value binding (`r`, `mut r`): not `ref r`, not
/// `r @ pat`.
fn is_by_value_binding(pat: &Pat<'_>) -> bool {
  matches!(pat.kind, PatKind::Binding(ann, _, _, None) if matches!(ann.0, rustc_hir::ByRef::No))
}
