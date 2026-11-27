//! This file details all the datastructures that are filled while traversing the HIR.
//! Essentially we care about detailing interactions and relationships between 
//! Resource Access Points (RAPs - RV1 terminology)
//! The frontend (svg-generator crate) requires a list of external events:
//! which are generally events that cause some visually external effect on the visualization
//! For example: Moves, Copies, Borrows are all represented with arrows between timelines.


use log::warn;
use rustc_middle::{
  mir::{Body, Local},
  ty::*,
};
use rustc_hir::{Expr, ExprKind, QPath, PatKind, Mutability, UnOp, def::*, Pat};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use crate::expr_visitor_utils::*;
use crate::svg_generator::data::*;
use rustc_borrowck::consumers::BodyWithBorrowckFacts;
use crate::facts::schema::{BodyFacts, LoanInfo};
use crate::facts::value_flow::{Call, CallSlots, OperandEffect, Read, Site, Source};
use crate::ref_flow::holder_branch_bodies;


pub(crate) struct Transfer<'a> {
  pub row: usize,
  pub kind: Evt,
  pub path: &'a str,
  pub to: &'a str,
  pub location: rustc_middle::mir::Location,
}

pub(crate) struct Reborrow<'a> {
  pub loan: &'a LoanInfo,
  pub through: ResourceAccessPoint,
  pub holder: &'a str,
}

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

/// The loans every reference held when an `if` / `match` began, for the
/// arms to start from (see `ExprVisitor::arm_borrows_next`).
/// The head of `for (s, k) in names` (moved in) or `for &(a, b) in
/// &pairs` (borrowed), while its pattern is registered (#1390): each
/// binding that gets (a copy of) a part of an element draws an arrow from
/// the head, as `let (s, k) = pair;` draws one from each field.
#[derive(Debug, Clone)]
pub struct PatternElem {
  pub head: ResourceTy,
  /// The head is moved into the loop's iterator (else borrowed by it).
  pub moved: bool,
  /// No binding has drawn an arrow from the head yet (the head's move
  /// is then drawn on its own).
  pub source_pending: bool,
  /// A head holding references (`Vec<(&mut String, i32)>`, #1389): its
  /// name and what its references borrow, which a reference binding
  /// moved out of it borrows.
  pub holder: Option<(String, Vec<ResourceTy>)>,
  /// The copies out of a borrowed head, drawn after its loan (line,
  /// binding, part).
  pub deferred: Vec<(usize, ResourceTy, LoopHead)>,
}

pub struct ArmBorrows {
  before: HashMap<String, RefData>,
  /// Variables declared before the conditional that held no loan when
  /// it began (`let r;`, #1069): an arm that gives one its first loan
  /// (`r = &t;`) doesn't give it to the next arm, which starts with r
  /// holding none, as for `before`'s.
  unlent: HashSet<String>,
  /// The loans earlier arms gave a reference, in arm order.
  done: Vec<(String, RefData)>,
  /// `arm_value_binds` as it was when the conditional began (an outer
  /// conditional's), put back when it ends.
  outer_value_binds: HashSet<String>,
  /// The last use of a loan held from before in an arm that used it,
  /// over the arms so far.
  last_use: std::collections::BTreeMap<String, usize>,
}

/// The loan of a variable whose column a later variable of the same name
/// took over (#903; see `ExprVisitor::retired_loans`).
pub struct RetiredLoan {
  /// The column's name, which both variables have.
  pub name: String,
  pub data: RefData,
  /// The line the later variable is declared on: events from there on
  /// are the later variable's.
  pub until: usize,
}

/// The `borrow_map` / `raps` key a retired loan (#903) is put under while
/// `note_sharers_across_calls` reads it: not a variable's name.
fn retired_key(name: &str, i: usize) -> String {
  format!("{name}\u{E001}{i}")
}

/// Adds the lenders `rd` borrows (those with a column) to `out`, once.
fn add_lenders(out: &mut Vec<ResourceTy>, rd: &RefData) {
  for l in rd.all_lenders() {
    if matches!(l, ResourceTy::Value(_) | ResourceTy::Deref(_)) && !out.contains(&l) {
      out.push(l);
    }
  }
}

/// Whether `a` and `b` describe the same loan of the same reference.
fn same_loan(a: &RefData, b: &RefData) -> bool {
  a.local == b.local && a.lender == b.lender && a.assigned_at == b.assigned_at && a.loan_line == b.loan_line
    && a.borrow_span == b.borrow_span && a.co_lenders == b.co_lenders
}

/// `let w = <rhs>;` (or `w = <rhs>;`) with rhs a call, a method call, an
/// index or a deref (see `ExprVisitor::note_derived`).
#[derive(Debug, Clone)]
pub struct Derived {
  /// The lines rhs spans: the event that gives w its value is on one.
  pub lines: (usize, usize),
  /// The places rhs reads (`v` in `v.iter().copied().collect()`, `&vd`
  /// in `firsts(&vd)`, `h.v` in `h.v.iter()`), each with whether its
  /// type's drop may use a borrow in it (a type with its own `Drop`).
  pub srcs: Vec<(String, bool)>,
  pub dest: String,
  /// A clone of its one source (`v.clone()`, `Clone::clone(&v)`,
  /// `v.to_vec()`, `v.to_owned()`, std's, with v's own type).
  pub clone: bool,
  /// w's type's drop may use a borrow in it.
  pub dropped: bool,
}

/// A borrow a call may have stored behind one of its `&mut` arguments
/// (see `ExprVisitor::stored_borrows`).
#[derive(Debug, Clone)]
pub struct StoredBorrow {
  /// The variable or field that may hold it (`v` in `keep(&mut v, &s)`).
  pub holder: String,
  /// The line of the call.
  pub at: usize,
  /// The line the loan ends on.
  pub until: usize,
  /// The variable borrowed (`s`).
  pub lender: String,
  pub is_mut: bool,
  /// The id of the call's PassByStaticReference / PassByMutableReference.
  pub id: usize,
  /// rustc's loans for the borrow (`mir_data::loans_issued_in`).
  pub loans: Vec<usize>,
}

#[derive(Debug, Clone)]
pub struct RefData {
  /// The holder declaration, retained when this slot leaves the active column.
  pub local: Option<Local>,
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
  /// assigned from, when it was one. `emit_borrow_ends` uses it to
  /// pick this reference's own loan out of MIR's borrow data; two
  /// references borrowing the same variable on one line (`let a = &x;
  /// let b = &x;`) are otherwise indistinguishable (#233).
  pub borrow_span: Option<rustc_span::Span>,
  /// For a reference that got its value from a call (`id(&p)`,
  /// `v.first()`), or took over such a reference's loan (see
  /// `ExprVisitor::ref_carriers`): the line that issued the loan it
  /// holds. `emit_borrow_ends` matches it on (this line, lender), as it
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
  /// For a holder (`let it = refs.iter_mut();`, refs: `Vec<&String>`):
  /// the variables its value reaches only through the ones it borrows
  /// (a1, through refs's elements; see `loans_split_by`, #1399). Not
  /// lent to it: no arrow, no return. A reference taken out of it may
  /// still be one of those (`it.copied().next()` copies out a1's
  /// `&String`), so `value_lenders` names them for what flows out.
  pub through: Vec<ResourceTy>,
}

/// The loan a reference holds, as `emit_borrow_ends` finds it in MIR's
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

  /// `all_lenders`, then the ones the value reaches only through them
  /// (`through`): what a reference taken out of it may borrow.
  pub fn reached_lenders(&self) -> Vec<ResourceTy> {
    let mut v = self.all_lenders();
    for l in &self.through {
      if !v.contains(l) {
        v.push(l.clone());
      }
    }
    v
  }
}

/// A column is a presentation choice. Resolve a use to its declaration and
/// MIR local before selecting the column that represents it.
#[derive(Default)]
pub struct BodyRaps {
  pub columns: HashMap<String, RapData>,
  /// Explicitly registered parameters, including the wrapper of an async fn.
  pub parameters: HashMap<rustc_hir::HirId, String>,
  /// Declaration registered for the column at this point in the HIR walk.
  active: HashMap<String, Local>,
  /// RAPs of completed sibling declarations, including their own drop wording.
  earlier: HashMap<Local, (RapData, Option<RefData>)>,
  /// Earlier fields of sibling bindings that use the same visual column.
  earlier_fields: BTreeMap<(Local, String), RapData>,
  /// Ordinary places drawn in the enclosing timeline, as opposed to one
  /// of its legs. The layout boundary is used for non-returning loops.
  top_level_places: BTreeMap<(Local, String), usize>,
  /// Root local of the current struct declaration for each shared field group.
  struct_locals: HashMap<u64, Local>,
  pub bindings: std::rc::Rc<crate::facts::BodyBindings>,
}

impl BodyRaps {
  pub fn binding(&self, id: rustc_hir::HirId) -> Option<&RapData> {
    if let Some(local) = self.bindings.declarations.get(&id) {
      return self.place(*local, self.bindings.locals.get(local)?);
    }
    self.columns.get(self.parameters.get(&id)?)
  }

  /// A shared drawing column can represent several locals. A field column
  /// additionally identifies the projection within that particular local.
  pub(crate) fn place(&self, local: Local, column: &str) -> Option<&RapData> {
    let rd = self.columns.get(column)?;
    if rd.local == Some(local) || rd.rap.scope_local() == Some(local)
      || rd.rap.scope_local().is_none() && rd.rap.is_member() && self.struct_locals.get(&rd.rap.get_owner()) == Some(&local) {
      return Some(rd);
    }
    self.earlier.get(&local).map(|(rd, _)| rd).filter(|rd| rd.rap.name() == column)
      .or_else(|| self.earlier_fields.get(&(local, column.to_owned())))
  }

  pub(crate) fn path_loan<'a>(&'a self, rd: &RapData, loans: &'a HashMap<String, RefData>) -> Option<&'a RefData> {
    let loan = match rd.local.and_then(|local| self.earlier.get(&local)) {
      Some((_, loan)) => loan.as_ref(),
      None => loans.get(rd.rap.name()),
    };
    loan.filter(|slot| slot.local == rd.local)
  }

  pub(crate) fn declare(&mut self, id: rustc_hir::HirId, tcx: &TyCtxt<'_>) -> String {
    let label = binding_name(tcx, id);
    if let Some(&local) = self.bindings.declarations.get(&id) {
      self.active.insert(label.clone(), local);
    } else {
      self.active.remove(&label);
    }
    label
  }

  pub(crate) fn local(&self, column: &str) -> Option<Local> {
    self.active.get(column).copied()
  }

  fn root_local(&self, column: &str) -> Option<Local> {
    self.local(column).or_else(|| self.columns.get(column)?.rap.owner_local())
  }

  pub fn pattern_local(&self, column: &str, pat: &Pat<'_>) -> Option<Local> {
    // Rust permits each identifier once in a pattern. Or-alternatives share its canonical HirId.
    let mut found = None;
    pat.each_binding(|_, id, _, _| {
      if let Some(&l) = self.bindings.declarations.get(&id).filter(|l| self.bindings.locals[*l] == column) { found = Some(l); }
    });
    found
  }

  pub fn place_name(&self, expr: &Expr<'_>, tcx: &TyCtxt<'_>) -> Option<String> {
    map_place_name(expr, &|p| match p.res {
      Res::Local(id) => Some(self.binding(id)?.rap.name().clone()),
      _ => Some(path_name(p, tcx)),
    })
  }

  pub fn path(&self, path: &rustc_hir::Path<'_>, tcx: &TyCtxt<'_>) -> Option<&RapData> {
    match path.res {
      Res::Local(id) => self.binding(id),
      _ => self.columns.get(&path_key(path, tcx)),
    }
  }
}

impl std::ops::Deref for BodyRaps {
  type Target = HashMap<String, RapData>;
  fn deref(&self) -> &Self::Target { &self.columns }
}

impl std::ops::DerefMut for BodyRaps {
  fn deref_mut(&mut self) -> &mut Self::Target { &mut self.columns }
}

#[derive(Debug, Clone)]
pub struct RapData {
  pub rap: ResourceAccessPoint,
  pub scope: usize,
  pub is_global: bool,
  // `let r = &String::from("a");`: the column name of the temporary r
  // borrows, which lives as long as r's `let` does (#400). Lets the
  // branch / block code that ends r's scope end the temporary's too.
  // Several for `let w = W { a: &f(), b: &g() };` (#856).
  pub extended_temp: Vec<String>,
  // An unbound temporary's (#457) declaring scope: the block, or match
  // arm body, whose branch declares it, so a branch's decl set covers it.
  pub temp_owner: Option<rustc_hir::HirId>,
  // Same-named variables of sibling blocks share one column (#308);
  // `scope` is the first one's block end, and this holds the block ends
  // of the later ones declared outside any branch or loop body
  // (registered with is_global = true), each of which also ends the
  // variable (#589), even when the first one was declared in one (#931).
  pub extra_scopes: Vec<(usize, Option<Local>)>,
  pub first_local: Option<Local>,
  pub local: Option<Local>,
  // A later variable sharing the column of a first one declared outside
  // any branch or loop body was declared in one (is_global = false): the
  // construct that ends its block ends it (`emit_scope_end`), like a
  // variable of its own (#931).
  pub local_later: bool,
}

/// How the value of a branch of an `if` / `match` is bound, once the
/// branch has been visited (`deferred_arm_binds`, #994).
#[derive(Debug, Clone)]
pub enum ArmBind<'tcx> {
  /// To one variable (`let v = if …`, `v = if …`).
  Rhs(ResourceTy, Evt),
  /// Taken apart by a pattern whose variables are already declared
  /// (`let (a, b) = if …`, #1007).
  Pat(&'tcx Pat<'tcx>),
  /// To a variable that may hold a value already (`v = if …` with v
  /// owning a String, #1010): its old value is dropped where the
  /// branch's value arrives, if the branch left it there (and its
  /// type needs drop: the `bool`).
  Reassign(ResourceTy, Evt, ResourceAccessPoint, bool),
  /// Taken apart by a destructuring assignment (`(x, n) = if …`, the
  /// block it desugars to; #1253).
  Assign(&'tcx rustc_hir::Block<'tcx>),
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
  // `exit_frames`' length at the `let`: the loops and branches it is
  // inside of.
  pub frames_at: usize,
}


pub struct ExprVisitor<'a, 'tcx:'a> {
  pub facts: Option<&'a crate::facts::BodyFacts>,
  pub tcx: TyCtxt<'tcx>, // type context
  pub event_spans: HashMap<usize, rustc_span::Span>,
  pub(crate) transfer_sites: BTreeMap<usize, Site>,
  argument_slots: BTreeMap<usize, (rustc_middle::mir::Location, usize)>,
  /// Ends drawn for the previous fields of an overwritten whole value.
  pub field_overwrite_ends: BTreeSet<usize>,
  pub emitting_expr: Option<&'tcx Expr<'tcx>>,
  pub row_layout: crate::event_order::RowLayout,
  pub end_loans: BTreeMap<usize, Vec<usize>>,
  /// Renderer event to the precise loan issued while evaluating an aggregate.
  pub aggregate_issues: BTreeMap<usize, usize>,
  /// Renderer event to the MIR write that delivered the result.
  pub closure_result_arrivals: BTreeMap<usize, rustc_middle::mir::Location>,
  /// Operand arrows whose loan need not have been taken on the path to its end.
  conditional_aggregate_ends: Vec<(ResourceTy, ResourceTy, usize)>,
  pub mir_body: &'a Body<'tcx>, 
  pub hir_body: &'a rustc_hir::Body<'tcx>,
  // Exact MIR effects at the source expression being rendered. The same span
  // on another expansion or in another body has its own effects.
  pub pattern_loans: BTreeMap<u64, &'a crate::facts::LoanInfo>,
  pub body_facts: Option<&'a crate::facts::BodyFacts>,
  effects: HashMap<rustc_span::Span, Vec<(usize, &'a crate::facts::Ev)>>,
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
  pub raps: BodyRaps,

  // Used to determine the current scope when visiting expressions
  pub current_scope: usize,

  // Source row for the function's parameter events.
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
  // `let x;` locals that are initialized on some paths to this point
  // and not on others (#244): `let mut s; if c { s = a; }` leaves s
  // maybe-initialized after the `if`. Rust can't read s there (E0381),
  // and keeps a runtime drop flag for it: a later `s = …` drops the old
  // value only if there is one, and so does s's scope end. Set where
  // branches join (`join_uninit_locals`); an assignment takes x out.
  pub maybe_init_locals: HashSet<rustc_hir::HirId>,
  // Those of `maybe_init_locals` that are so because a loop above
  // assigns them but may have ended before it did (#786).
  pub maybe_init_by_loop: HashSet<rustc_hir::HirId>,
  // For each loop being visited, innermost last: the `let x;` locals
  // with no value as it starts (#786). An assignment to one in its body
  // initializes it the first time round, and later times round drops
  // what an earlier iteration assigned. Keyed by the loop (what its
  // `break`s name).
  pub loop_entry_uninit: Vec<(rustc_hir::HirId, HashSet<rustc_hir::HirId>)>,
  // A `let x;` local's name, once assigned, to `DeferredLet::frames_at`
  // (#786): it joins `in_scope` where it's first assigned, maybe inside
  // a loop, but belongs to the scope of its `let`.
  pub deferred_decl_at: HashMap<String, usize>,
  // A `let x;` local (or a temporary a `let` extends through a branch,
  // #1397) has a slot in `in_scope` from its `let` (`deferred_slot`),
  // keyed here, once assigned, to the names its assignment registered:
  // its column joins `in_scope` where it is first assigned, often in a
  // branch, whose locals leave `in_scope` at the branch's end. An exit
  // after the branch drops it from the slot (#1436).
  pub deferred_slots: HashMap<String, Vec<String>>,

  // For `let x = if … { a } else { b };` (and `match`), the events that
  // pair each branch's value with `x` are emitted before the branches
  // are visited (see `process_let_binding`). Keyed by the branch body's
  // HirId: the range of event ids `match_rhs` used for that branch, so
  // the If / Match arm of `visit_expr` can put those events in the
  // right branch (`branch_ids`).
  pub rhs_arm_ids: HashMap<rustc_hir::HirId, (usize, usize)>,
  // While `process_let_binding` pairs the branches' values with `x`
  // ahead of visiting them: the span of the conditional. A branch
  // whose value is (or uses) a variable declared inside it (`if let
  // Some(y) = o { y }`, `{ let y = f; y }`) can't be paired yet, as
  // that variable isn't known until the branch is visited (#994):
  // it is paired once its body has been (`deferred_arm_binds`, keyed
  // by the body's HirId, see `bind_deferred_arm`).
  pub defer_arm_binds: Option<rustc_span::Span>,
  /// Binding a tuple / struct literal with an `if` / `match` part to a
  /// `let` (`let t = (if c { d } else { e }, f);`, #1256): `Some(true)`
  /// binds only those parts, before the literal is visited (their
  /// branches take the events), `Some(false)` only the others, after it,
  /// as a call passes its arguments once they're visited.
  pub literal_parts: Option<bool>,
  pub deferred_arm_binds: HashMap<rustc_hir::HirId, (&'tcx Expr<'tcx>, Vec<ArmBind<'tcx>>)>,
  /// For a branch body whose pairing `deferred_arm_binds` put off: the
  /// reference parts of the value it is bound to whose lenders it adds
  /// to the other branches' (`match_rhs_arms`, #1402), with each one's
  /// loan before the conditional.
  pub arm_union_parts: HashMap<rustc_hir::HirId, Vec<(String, Option<RefData>)>>,
  /// The branch bodies of the conditional assigned to a holder that
  /// were paired with it in their branch (`deferred_arm_binds`, #1472):
  /// `match_rhs_arm` leaves them alone when the rest are paired.
  pub arms_paired_in_branch: HashSet<rustc_hir::HirId>,
  // While a branch's value is matched against the variables of a
  // pattern that were declared ahead of the conditional (`let (a, b) =
  // if …`, #1007): `define_lhs` leaves an existing variable alone.
  pub bind_existing: bool,
  // While the branches' values of `v = if …` are paired with a v that
  // may hold a value (#1010): v. Each branch is paired once it has
  // been visited, where v's old value is dropped (see `ArmBind::Reassign`).
  pub reassign_arms: Option<(ResourceAccessPoint, bool)>,
  // While a branch's value is paired (`pair_pat_with_value`): the
  // scope that branch body is (its block, or a match arm's body), which
  // owns the temporary of a literal value the pattern leaves parts of
  // (#1011).
  pub branch_value_owner: Option<rustc_hir::HirId>,
  // The drops of those temporaries, drawn once the value is paired: line,
  // temporary, its type.
  pub branch_temp_drops: Vec<(usize, ResourceAccessPoint, String)>,

  // A constructor call's HirId -> the range of event ids its arguments
  // used (moves and copies into the constructor). The constructor is
  // never the receiver: whoever takes the built value (the variable it
  // is bound to, or the call it is passed to) is, so these events are
  // retargeted there (#768).
  pub ctor_arg_ids: HashMap<rustc_hir::HirId, (usize, usize, u64)>,

  // Ids of the GoOutOfScope events emitted at a `return` inside a
  // branch for the values still owned there (#363). They make their
  // variables live in that branch (an ordinary scope-end GoOutOfScope
  // doesn't), and are kept only in a branch that diverges: see
  // `keep_exit_drops`.
  pub exit_drops: std::collections::HashSet<usize>,
  /// For the next `drop_locals_on_exit`: the exit is a call that never
  /// returns and drops nothing (or may not, #1553), not a panic.
  pub no_drop_exit: Option<NoDropExit>,
  /// Variables with nothing to drop (a Copy value, a reference) declared
  /// in a branch that an exit in it ended (#1348): like a value that is
  /// dropped there, their scope ends at the exit, not at the block's
  /// `}`, which that path never reaches (see `left_at_exits`).
  pub exit_scope_ended: HashSet<String>,
  /// References that point into something the plugin can't name (the
  /// loop variable of `for x in it`, #417): they get no lender, so no
  /// borrow is "returned" when they end.
  pub unlent_refs: HashSet<String>,
  /// The variable a `for` loop variable with no lender points into
  /// (`for e in v.iter_mut()` with v a `&mut Vec<_>` parameter or a
  /// local reference, `for e in v.iter_mut().rev()`): the receiver the
  /// head's method chain takes by `&mut`. A write `*e += 1` is drawn on
  /// its column, or on the column of what it points into (#1046).
  /// Several when e may point into any of them: `for x in
  /// a.iter_mut().chain(b.iter_mut())` (#1082).
  pub loop_ref_into: HashMap<String, Vec<ResourceAccessPoint>>,
  /// `&mut` reference variables a `for` loop's head moved into the
  /// loop's iterator (`for x in r`, `a.iter_mut().zip(b)`; #1076,
  /// #1129), each with the line it was last assigned on (its
  /// `RefData::assigned_at`) when moved. Its column ends at the move:
  /// the iterator holds the loan until the loop ends, so the variable
  /// draws no write and no borrow end after it (#1365).
  pub loop_moved_refs: HashMap<String, usize>,
  /// `ref` / `ref mut` bindings out of a tuple-literal scrutinee's
  /// element (#580): they borrow the temporary tuple, which has no
  /// column, so their borrow's end is drawn as a plain end dot with no
  /// arrow (#658).
  pub tuple_temp_refs: HashSet<String>,
  /// Spans of the `&mut r` / `&r` expressions that borrow a reference
  /// variable r itself (`ref_var_borrow`, #914, #925). A reference whose loan
  /// (`RefData::borrow_span`) is one of them gives back r, a value,
  /// not what r points to.
  pub ref_var_borrows: HashSet<rustc_span::Span>,
  /// Receiver column and MIR loan of each stored unique immutable capture.
  stored_unique_captures: BTreeSet<(u64, usize)>,
  /// (holder, reference variable r) for a borrow of r itself that a
  /// call stored in the holder's value (`refs.push(&r)` with `refs:
  /// Vec<&&String>`, #973): the holder gives back r, a value, not what
  /// r points to. Holders keep `RefData::borrow_span` unset (their
  /// loans are found by line), so `ref_var_borrows` can't say this.
  pub stored_ref_var_lends: HashSet<(String, String)>,
  /// Ids of the moves of a `&mut` variable into a call whose result
  /// carries its borrow on (`let r2 = id(r);` with `fn id<T>(x: T) ->
  /// T`, `if let Some(x) = Some(r)`, #1128): the holder of the result
  /// ends the borrow, so the moved-from reference doesn't (see
  /// `ref_handed_off`).
  pub moved_into_result: HashSet<usize>,
  /// Borrows a call may have stored behind one of its `&mut`
  /// arguments (`keep(&mut v, &s)`, `stored_borrow`, #513): the
  /// holder, the line of the call, the line the loan ends on, the
  /// lender, and whether the borrow is mutable. `note_sharers_across_calls`
  /// treats each shared one as a sharer of its lender (#974).
  pub stored_borrows: Vec<StoredBorrow>,
  /// Variables given a value computed from places (`let w =
  /// v.clone();`, `let w: Vec<&String> = v.iter().copied().collect();`,
  /// `let r = v[0];`, see `note_derived`). The holder walk of a stored
  /// borrow follows one from a holder when rustc's borrow checker
  /// puts the stored loan in the variable's type (#1003, #1015).
  pub derived: Vec<Derived>,
  /// References a pattern copied (or moved) out of a value that holds
  /// a borrow (`Some(n)` on `it.next()`, #391): their borrow is part of
  /// that value's, and ends at their own last use (#575).
  pub pattern_copies: HashSet<String>,
  /// Bindings under a `&` sub-pattern (`Some(&y)` on `orr`), by RAP
  /// hash: the copy into each, from the matched value named here, is
  /// of the value a reference stored in it points to (#874); `true`
  /// when the binding is only a part of that value (`Some(&(a, b))`).
  pub pointee_copies: HashMap<u64, (ResourceTy, bool)>,
  /// `&` patterns matched against a reference column (`&v` on `r:
  /// &i32`): their bindings are matched against `*r`, like `match *r`
  /// (#880), so they aren't `&` sub-patterns in #874's sense.
  pub deref_ref_pats: HashSet<rustc_hir::HirId>,
  // While the pattern of `for (a, b) in &v` is registered: the borrow
  // of the head that each reference binding is (#576).
  pub pattern_lender: Option<(Evt, ResourceTy)>,
  // While the pattern of `for (s, k) in names` / `for &(a, b) in &pairs`
  // is registered: where each binding's value comes from (#1390).
  pub pattern_elem: Option<PatternElem>,
  // While the pattern of `for (x, y) in [(&e, &g)]` is registered: the
  // elements of the literal head, whose references a reference binding
  // copies (#1433).
  pub pattern_literal: Option<&'tcx [Expr<'tcx>]>,
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
  // before its `else` block runs (edition 2024, #469, #604). They get
  // no column in that `if`'s branches, unless an exit in the matched
  // block drops them (#1360, see `fork_exited_temps`): their column
  // then forks there, like a `match` scrutinee's. A drop at an exit
  // that isn't taken into a branch is a dot on the column that doesn't
  // end it (see `take_branch_events`).
  pub unbranched_temps: HashSet<ResourceAccessPoint>,
  // Those of the `if let`s whose matched block is being visited. A
  // conditional nested in that block takes the drop at an exit in its
  // branch, like any live variable's, so the column forks there and the
  // exit's leg ends in the drop (#1360).
  pub matched_block_temps: HashSet<ResourceAccessPoint>,
  // The variables moved into the `for` loops being visited as their
  // iterators (`for s in it`, `LoopHead::Iterator`), innermost last: a
  // `return` inside drops each loop's iterator (#482), and so does a
  // `break 'outer` out of the loops inside `'outer` (#592). Each is
  // paired with its loop's id (the desugared `loop` expression).
  pub loop_iterators: Vec<(rustc_hir::HirId, ResourceAccessPoint)>,
  // The `return`s directly in a loop body (#1020): see `LoopReturn`.
  pub loop_returns: Vec<LoopReturn>,
  // The lines of the closing braces that a `loop` nothing breaks out of
  // keeps the code from reaching (#1041), and that loop (the innermost
  // one, #1057): see `note_breakless_loop`.
  pub dead_ends: HashMap<usize, EndlessLoop>,
  // Each `?` in the function (not in a closure): its line, and the
  // variables in scope there, which it drops if it returns early
  // (#1044): see `mark_loop_returns`.
  pub fn_tries: Vec<(usize, HashSet<u64>)>,
  // The `(temporary N)` column of each unbound temporary that a field
  // is moved out of (`make_q().a`), by the expression that makes it
  // (#469).
  pub temp_cols: HashMap<rustc_hir::HirId, ResourceAccessPoint>,
  // The `(temporary N)` column of a temporary borrowed inside a `let`'s
  // initializer and extended with it (`let w = W { r: &String::new() };`,
  // #856), by the `&` expression that borrows it.
  pub nested_ext_temps: HashMap<rustc_hir::HirId, ResourceAccessPoint>,
  // A temporary that a `let` extends through an `if` / `else` branch or
  // a `match` arm (`let r = if c { &String::from("x") } else { &s };`,
  // #1397), by the temporary's expression (`String::from("x")`): its
  // `(temporary N)` column's name. Only the path through that branch
  // makes it, so it is tracked like a `let x;` local assigned in that
  // branch: `uninit_locals` until the branch makes it, then
  // maybe-initialized after the join, dropped by a drop flag at the end
  // of the `let`'s block. `uninit_locals` / `maybe_init_locals` key it
  // by this expression's HirId; `deferred_name` names it.
  pub branch_temp_names: HashMap<rustc_hir::HirId, String>,
  // The columns (hashes) of those temporaries, once made, and of their
  // field columns. After the join a temporary may or may not exist, so,
  // as for a `let x;` local, nothing is drawn on its column until its
  // scope ends: the end of a borrow of it (at the borrower's last use,
  // always after the join) isn't drawn.
  pub branch_temp_cols: HashSet<u64>,
  // The `(temporary N)` column of a tuple / struct / array literal that
  // is a `let`-`else`'s initializer (`let (Some(a), _) = (x, make())
  // else { .. }`), by the literal and by each variable moved into it
  // (x): the pattern's bindings are moved out of it (#954).
  pub let_else_literal_temps: HashMap<rustc_hir::HirId, ResourceAccessPoint>,
  // The `(temporary N)` column of an unbound temporary that is a
  // constructor call (`Some(make());`), with the id of the move into it
  // from the constructor, which the constructor's arguments replace
  // once they are visited (#783).
  pub ctor_temps: HashMap<rustc_hir::HirId, (ResourceAccessPoint, usize)>,
  // Constructor calls that a `let` pattern takes apart like a tuple
  // literal (`let W(y) = W(g);`, #950): no actor, see
  // `note_let_ctor_literals`.
  pub let_ctor_literals: HashSet<rustc_hir::HirId>,
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
  /// `lhs` binding → the part column of the RHS place it takes, when the
  /// RHS is a place whose parts have columns (`p.a` in `Pair { a: s, b: t
  /// } = p;`, #1412): `s = p.a`, not `s = p`.
  pub destructure_parts: HashMap<rustc_hir::HirId, String>,
  pub destructure_moved: HashSet<String>,
  /// The parts of an assignment's destination that its right-hand
  /// side moves out (`t.0`, `t.1` in `t = (t.1, t.0)`, #1138): their
  /// old values aren't dropped by the assignment.
  pub rhs_self_moved: BTreeSet<String>,
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
  /// so `h` takes the loan over: `emit_borrow_ends` ends `h`'s borrow
  /// where rustc ends the loan, not at `h`'s last use (#644).
  pub ref_carriers: Vec<(String, String, LoanId)>,
  /// References holding a loan they took over (see `ref_carriers`).
  /// Copying or moving one on (`q = t;` after `let t = p; p = q;`)
  /// hands the loan to the new holder right away: it's the one that
  /// keeps the loan live, and `t`'s borrow ends at `t`'s last use.
  pub taken_over_loans: HashSet<String>,
  /// The end of an old loan drawn on a line after the reassignment that
  /// ends it (#778): `if c { r = &y; } else { use(r); }` keeps the lender
  /// borrowed on the path through the else arm, so the end is at its read.
  /// It can't be added while the arm holding the write is visited, or it
  /// would be drawn in that arm; `emit_borrow_ends` adds them at the top
  /// level, where `end_outer_borrows_in_arms` moves each into the arm that
  /// holds its line and ends the borrow in the other arms.
  pub late_old_ends: Vec<(usize, ExternalEvent)>,
  /// Where the possible borrows of a reference that an or-pattern in an
  /// `if let` / `while let` / `let` binds from different places end
  /// (`OrAltBorrow { end: true, .. }`, #734). They're added in
  /// `emit_borrow_ends`, after the whole body is visited, where
  /// MIR end placement moves each into the arm that declares
  /// the reference, as for its other borrow ends.
  pub or_ref_ends: Vec<(usize, ExternalEvent)>,
  /// Loans a reference was given in an arm of an `if` / `match` that
  /// another arm's, or the conditional's end, replaced in `borrow_map`
  /// (`c = &y;` in one arm, `c = &z;` in the next, #806): the arms are
  /// alternatives, so each loan ends on its own path. `emit_borrow_ends`
  /// ends each one like the loan `borrow_map` holds.
  pub arm_loans: Vec<(String, RefData)>,
  /// References a let-chain or a match guard gave a new loan (#603): the
  /// paths where it fails end that loan with their own events
  /// (`chain_fail_events`), so `emit_borrow_ends` ends it only on the path
  /// into the body: (name, the condition-and-body rows, the conditional's
  /// last row).
  pub chain_then: Vec<(String, (usize, usize), usize)>,
  /// Loans of variables whose column a later variable of the same name
  /// took over: a sibling block's `let r = &v;`, the first of two `for
  /// x in v.iter()` loops (#903). Same-named variables that are never
  /// in scope together share a column (`collect_binding_names`), and
  /// `borrow_map` is keyed by name, so the later variable's loan used to
  /// replace the earlier one's, whose borrow then never ended.
  /// `emit_borrow_ends` ends each one like the loans `borrow_map` holds.
  pub retired_loans: Vec<RetiredLoan>,
  /// The retired loans (#903) with the end line the facts gave them,
  /// for `note_sharers_across_calls`: a call on the lender while the
  /// earlier variable still holds it says so.
  pub retired_sharers: Vec<(String, RefData)>,
  /// The references and holders (and their part columns) the branches
  /// of the conditional being visited each give their value to after the
  /// branch was visited (`ArmBind::Rhs`, #1453): see `arm_borrows_end`.
  pub arm_value_binds: HashSet<String>,
  /// The old loans of re-pointed references (`r = &t;` after `let mut r
  /// = &b;`): (the reference, its shared lenders, where the loan was
  /// made, its last line). `borrow_map` holds the new loan by then, so
  /// `note_sharers_across_calls` didn't see r share b before the write:
  /// a read of b in between said b was no longer borrowed (#1492).
  pub repointed_loans: Vec<(String, Vec<String>, usize, usize)>,
  /// Separates ends emitted during the HIR walk from the subsequent P2-1b slots.
  pub first_synthesized_end: usize,
  pub(crate) reference_effects: crate::facts::repoints::ReferenceEffects<'a, 'tcx>,
  pub(crate) drawn_references: crate::event_order::DrawnReferenceEffects<'a, 'tcx>,
  /// Pattern declarations in each drawn arm of this body.
  pub arm_bindings: BTreeMap<usize, Vec<(usize, Local)>>,
  /// The arms being visited, innermost last: (the conditional's first
  /// and last lines, the arm's first and last lines).
  pub arm_frames: Vec<(usize, usize, usize, usize)>,
  /// While true, `add_external_event` queues events in `late_old_ends`.
  pub defer_old_ends: bool,
  /// While a whole value is overwritten (`s2 = Shelf { items:
  /// vec![w.as_str()] };`), the struct literal's holder fields whose
  /// borrows `emit_holder_literal_borrows` draws: drawn after the old
  /// value's borrow ends and drop, which come first on the dot (#1097).
  pub deferred_holder_borrows: Option<Vec<(String, &'tcx Expr<'tcx>)>>,
  /// (column, line) pairs `note_dead_loan_write` has looked at (#796).
  pub dead_loan_checked: HashSet<(Local, rustc_middle::mir::Location)>,


  // Moves made in a branch that breaks out of a labeled block.
  pub break_moves: Vec<BreakMove>,
  // The labeled blocks being visited, innermost last (#1286).
  pub labeled_frames: Vec<LabeledFrame>,
  // Each way out of a `loop` or labeled block seen so far: the HirId
  // its `break`s target, the `break` (`None`: a labeled block's end,
  // when reached) and the variables moved on every path that leaves
  // that way, its value's moves included (#1504).
  pub way_out_moves: Vec<(rustc_hir::HirId, Option<rustc_hir::HirId>, BTreeSet<String>)>,
  // The loops being visited (by the HirId their `break`s name) and
  // the variables declared before each that hold no value where it
  // starts but may where it comes back round (the backedge,
  // #1001): a later time round, an assignment to one drops what an
  // earlier time round gave it, if it still holds that.
  pub loop_given: Vec<(rustc_hir::HirId, Vec<ResourceAccessPoint>)>,
  // The loops being visited and the variables declared before each
  // that may hold a value where it starts, but only may (a branch
  // above moved them on some paths), each with what it holds where the
  // loop comes back round (#1012). An assignment
  // to one the first time round drops the old value only if no branch
  // above moved it; a later time round, what the time round before
  // left. With each, whether something other than a branch may have
  // moved it (a drop flag's "nothing above", `moved_by_chain_condition`).
  pub loop_maybe_entry: Vec<(rustc_hir::HirId, Vec<MaybeEntry>)>,
  // Set when a conditional directly in the function's body has every
  // branch leaving the function (`return`, panic): the closing brace
  // is unreachable, so it drops nothing (#515).
  pub all_paths_exit: bool,
  // The first line of the earliest such construct: a block that ended
  // above it (`{ let s = …; }` before the conditional) dropped its
  // locals at its own `}`, which does run (#1139).
  pub all_paths_exit_from: usize,
  // Set when, once `all_paths_exit` was set, a block's tail was left
  // unvisited: it never runs (#1004).
  pub dead_tail: bool,
  // The statements and tails left unvisited for that reason; the code
  // panel doesn't annotate them either.
  pub dead_code: HashSet<rustc_hir::HirId>,

  // Earliest line a reference's borrow may be drawn ending at, keyed
  // by the reference's name (#372). A `match` arm binding that
  // reborrows through a reference (`match o { Some(s) => … }`, `o:
  // &Option<String>`) is taken out of `borrow_map` when its arm ends,
  // so `emit_borrow_ends` can't see that `o`'s borrow has to last
  // until then; the arm records it here instead.
  pub borrow_floor: HashMap<String, usize>,
  // Holders (`let it = v.iter_mut();`) moved into a `for` loop (#722).
  // The loop owns what they held from there, so the end of that borrow
  // is not drawn on their (moved-out) columns (#767).
  pub for_moved_holders: HashSet<String>,
  // Drawn let-else branch ids. Arm zero is the diverging block.
  pub let_else_branches: BTreeSet<usize>,
  // The shared loans a `for` loop's iterator holds with no loop
  // variable to name it (`for (i, a) in v.iter().enumerate()`, `for &y
  // in &v`; #868, #885): the lender, the `for` line, the loop's closing
  // line, and whether a call in the head made it (`iter`, rather than
  // `&v`). A call on the lender in the body leaves it shared by the
  // iterator (#900).
  pub for_loop_iter_loans: Vec<(String, usize, usize, bool)>,
  // A `for` loop's `&mut` binding its body moves away on every pass
  // (`for e in v.iter_mut() { drop(e); }`, #1511): the binding, the line
  // of the move, and the loop's closing line. Its borrow ends at the
  // move; the loop's iterator holds the loan on to the loop's end.
  pub for_loop_moved_bindings: Vec<(String, usize, usize)>,
  // `let r = loop { … break k; };` (#431): the loop's id, what its
  // `break` values go to (`r`) and how (move or copy). A `break` with a
  // value, in that loop, pairs the value with `r` the way `let r = k;`
  // would.
  pub break_binds: Vec<(rustc_hir::HirId, ResourceTy, Evt)>,
  // A tuple/struct pattern bound to a loop's value: bound against each
  // `break` value (#452).
  pub break_pats: Vec<(rustc_hir::HirId, &'tcx Pat<'tcx>, usize, bool)>,
  /// A destructuring assignment of a `loop`'s value (`(x, n) = loop {
  /// … break (s, 1); … }`, #1253): the loop's HirId and the block the
  /// assignment desugars to. Each `break` value is taken apart and
  /// assigned as it reaches the receiver.
  pub break_assigns: Vec<(rustc_hir::HirId, &'tcx rustc_hir::Block<'tcx>)>,
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
  // The nearest active unlabeled block targeted by a desugared break.
  pub try_block: Option<TryBlockScope>,

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

  /// Moves of a destructuring assignment's cycle (`(a, b) = (b, a)`) out
  /// of a place whose new value is drawn first (`a = b` before `b = a`):
  /// the renderer puts each such move before that place's new value on
  /// its column (#1343); handed to `VisualizationData::moves_out_first`.
  pub moves_out_first: Vec<usize>,

  // Notes on dots saying what the figure simplifies (a loop drawn as one
  // pass, #853); handed to the renderer's
  // `VisualizationData::approx_notes`.
  pub approx_notes: Vec<ApproxNote>,
  // For each split scrutinee temporary (by hash), the by-value
  // bindings of each pattern that takes everything in it that needs drop
  // (#1386): the last of their moves out of it ends its leg in the arm.
  pub temp_emptiers: HashMap<u64, Vec<Vec<String>>>,
  // The ids of those moves (`VisualizationData::temp_emptying_moves`).
  pub temp_emptying_moves: Vec<usize>,

  // `r?` with `r` a non-`Copy` place moves `r` (#652). Set while
  // `match_rhs` draws that move into the variable `r?` is bound to;
  // the Move events made meanwhile carry it.
  pub try_move: Option<TryMove>,
  try_field_moves: BTreeSet<usize>,
  try_result_titles: BTreeMap<usize, String>,
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
  // Those of them, when every value the returned `if` / `match` yields
  // is a variable or a literal (`note_tied_ref_params`, #775).
  pub tail_plain: HashSet<rustc_hir::HirId>,
  // The same for a call's argument written as an `if` / `match`
  // (`take(if c { a } else { b })`): the value each branch yields is
  // passed to the call named here when its branch runs (#1174).
  pub arg_values: HashMap<rustc_hir::HirId, (String, bool)>,
  // Where the arms of each if/else and match leave to when they exit
  // early, by the Branch event's id (see `place_exit_ends`).
  pub exit_scopes: HashMap<usize, ExitScope>,
}

/// Boundaries of the drop scopes a residual leaves when it targets this
/// block. Outer locals, match temporaries and iterators survive the `?`.
#[derive(Clone, Copy)]
pub struct TryBlockScope {
  pub id: rustc_hir::HirId,
  pub locals: usize,
  pub tuples: usize,
  pub iterators: usize,
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
  pub loop_id: Option<rustc_hir::HirId>,
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

/// A labeled block being visited (`'a: { … }`, #1286): its block's
/// HirId (what its `break`s target), `preprocessed_events.len()`,
/// `exit_frames.len()` and `in_scope.len()` where it starts, and the
/// path to each `break` out of it seen so far.
#[derive(Clone)]
pub struct LabeledFrame {
  pub id: rustc_hir::HirId,
  pub events_at: usize,
  pub frames_at: usize,
  pub len: usize,
  /// The path to each `break` out of it (`path_events_since`), and
  /// the variables its value moves (`break 'a s` moves s, #1504).
  pub paths: Vec<(Vec<(usize, ExternalEvent)>, Vec<std::ops::Range<usize>>, BTreeSet<String>)>,
}

/// See `ExprVisitor::loop_maybe_entry`.
#[derive(Clone)]
pub struct MaybeEntry {
  pub ro: ResourceAccessPoint,
  /// The preceding move may run in a condition or an inline let.
  pub by_cond: bool,
  /// `Some(moves)` when the loop is inside another loop that `ro` was
  /// declared before (#1019): a run of it may follow an earlier one
  /// (an earlier time round of the outer loop), so "the first time
  /// round" isn't only after the branch above. `moves`: whether the
  /// outermost such loop ever moves something out of `ro`.
  pub nested: Option<bool>,
  /// Whether the loop ever comes back round to its start: false when
  /// every pass through its body leaves it (#1467), so that there is
  /// no later time round (`round` is then `Held`, as if it kept what it
  /// had).
  pub rounds: bool,
}

/// A `return` directly in a loop body (#1020), not in an `if` / `match`
/// drawn as a branch: what it drops, which the scope ends after it say.
/// See `note_return_in_loop`.
pub struct LoopReturn {
  /// The `return`'s line.
  pub line: usize,
  /// Each variable (or temporary) in scope there, and whether its scope
  /// end is never reached on a path through the loop body (`always`:
  /// declared in the body, whose closing brace the `return` comes
  /// before).
  pub vars: Vec<(ResourceAccessPoint, bool)>,
  /// The iterator variables of the `for` loops it leaves (`for s in
  /// it`), dropped where those loops end otherwise.
  pub iters: Vec<ResourceAccessPoint>,
}

/// A `loop` that nothing breaks out of (`loop_never_ends`), which keeps
/// the code from reaching the closing braces after it (#1041). While it
/// keeps running, its own `}` is the last row the code reaches (#1057).
#[derive(Clone, Copy)]
pub struct EndlessLoop {
  /// The loop's span, to tell a loop nested in it.
  pub span: rustc_span::Span,
  /// The lines of its `loop` keyword and its closing `}`.
  pub first: usize,
  pub last: usize,
  /// What can leave it.
  pub exits: LoopExits,
  /// Whether it can start its body again (`loop_repeats`): only then
  /// does it keep running.
  pub runs: bool,
}

/// See `ExprVisitor::exit_frames`.
#[derive(Clone)]
pub enum ExitFrame {
  // `hide`: ranges of event ids (see `take_branch_events`) that can
  // be among `preprocessed_events` while a branch of the conditional
  // is visited but aren't on the path through it: for `let x = if … /
  // match …`, the events binding each branch's value to `x`
  // (`rhs_arm_ids`), which only happen once the conditional has
  // finished. (The if-body's, and an earlier arm's, are out of
  // `preprocessed_events` while the `else` or the next arm is
  // visited, #245.)
  //
  // `binds`: events on the path into the branch that aren't in
  // `preprocessed_events` while it's visited (a match arm's pattern
  // bindings and guard, kept in the arm's own list), and `binds_at`
  // the length `preprocessed_events` had when they happened. See
  // `owns_value_here`.
  //
  // `fn_level`: not a branch, but the stand-in frame for an exit
  // directly in the function's body (`drop_locals_at_fn_return`).
  Branch { len: usize, hide: Vec<std::ops::Range<usize>>, binds: Vec<(usize, ExternalEvent)>, binds_at: usize, fn_level: bool },
  // `head_from`: for a `for` loop, where in `in_scope` the temporaries
  // its head made begin (they're dropped where the loop is left, but
  // declared before `len`); `len` for any other loop.
  // `events_at`: the length `preprocessed_events` had where the body
  // (of a `while`, after its condition) starts, so the events since
  // are those of the iteration on the path to a `break` (#980).
  Loop { id: rustc_hir::HirId, len: usize, head_from: usize, events_at: usize },
}

impl<'a, 'tcx> ExprVisitor<'a, 'tcx>{
  /// A visitor for one fn body, with every per-body table empty. The
  /// arguments are the context shared across the bodies of a crate.
  #[allow(clippy::too_many_arguments)]
  pub fn new(
    tcx: TyCtxt<'tcx>,
    mir_body: &'a Body<'tcx>,
    hir_body: &'a rustc_hir::Body<'tcx>,
    bwf: &'a BodyWithBorrowckFacts<'tcx>,
    body_facts: Option<&'a crate::facts::BodyFacts>,
    current_fn_start: usize,
    event_line_map: &'a mut BTreeMap<usize, Vec<ExternalEvent>>,
    preprocessed_events: &'a mut Vec<(usize, ExternalEvent)>,
    rap_hashes: usize,
    source_map: &'a BTreeMap<usize, String>,
    annotated_lines: &'a mut BTreeMap<usize, Vec<String>>,
    id_map: &'a mut HashMap<String, usize>,
    unique_id: &'a mut usize,
    fn_ret: bool,
    skip_lines: &'a HashSet<usize>,
  ) -> Self {
    let mut effects = HashMap::<_, Vec<_>>::new();
    for row in body_facts.into_iter().flat_map(|b| &b.rows) {
      for ev in &row.facts {
        let span = mir_body.source_info(ev.location).span.with_parent(None);
        effects.entry(span).or_default().push((row.line, ev));
      }
    }
    ExprVisitor {
      pattern_loans: BTreeMap::new(),
      body_facts,
      effects,
      event_spans: HashMap::new(),
      transfer_sites: BTreeMap::new(),
      argument_slots: BTreeMap::new(),
      field_overwrite_ends: BTreeSet::new(),
      emitting_expr: None,
      end_loans: BTreeMap::new(),
      aggregate_issues: BTreeMap::new(),
      closure_result_arrivals: BTreeMap::new(),
      conditional_aggregate_ends: Vec::new(),
      row_layout: Default::default(),
      facts: None,
      tcx,
      mir_body,
      hir_body,
      bwf,
      current_scope: 0,
      current_fn_start,
      borrow_map: HashMap::new(),
      raps: BodyRaps { bindings: body_facts.map(|b| b.bindings.clone()).unwrap_or_default(), ..Default::default() },
      event_line_map,
      preprocessed_events,
      rap_hashes,
      source_map,
      annotated_lines,
      id_map,
      unique_id,
      inside_branch: false,
      fn_ret,
      skip_lines,
      skip_raps: HashSet::new(),
      uninit_locals: HashMap::new(),
      maybe_init_locals: HashSet::new(),
      maybe_init_by_loop: HashSet::new(),
      loop_entry_uninit: Vec::new(),
      deferred_decl_at: HashMap::new(),
      deferred_slots: HashMap::new(),
      rhs_arm_ids: HashMap::new(),
      defer_arm_binds: None,
      literal_parts: None,
      deferred_arm_binds: HashMap::new(),
      arm_union_parts: HashMap::new(),
      arms_paired_in_branch: HashSet::new(),
      bind_existing: false,
      reassign_arms: None,
      branch_value_owner: None,
      branch_temp_drops: Vec::new(),
      ctor_arg_ids: HashMap::new(),
      exit_drops: HashSet::new(),
      no_drop_exit: None,
      exit_scope_ended: HashSet::new(),
      unlent_refs: HashSet::new(),
      loop_ref_into: HashMap::new(),
      loop_moved_refs: HashMap::new(),
      tuple_temp_refs: HashSet::new(),
      ref_var_borrows: HashSet::new(),
      stored_ref_var_lends: HashSet::new(),
      stored_unique_captures: BTreeSet::new(),
      moved_into_result: HashSet::new(),
      stored_borrows: Vec::new(),
      derived: Vec::new(),
      pattern_copies: HashSet::new(),
      pointee_copies: HashMap::new(),
      deref_ref_pats: HashSet::new(),
      pattern_lender: None,
      pattern_elem: None,
      pattern_literal: None,
      temp_count: 0,
      param_value_count: 0,
      pending_param_borrows: Vec::new(),
      temp_drops_noted: HashSet::new(),
      unbranched_temps: HashSet::new(),
      matched_block_temps: HashSet::new(),
      loop_iterators: Vec::new(),
      loop_returns: Vec::new(),
      dead_ends: HashMap::new(),
      fn_tries: Vec::new(),
      temp_cols: HashMap::new(),
      nested_ext_temps: HashMap::new(),
      branch_temp_names: HashMap::new(),
      branch_temp_cols: HashSet::new(),
      let_else_literal_temps: HashMap::new(),
      ctor_temps: HashMap::new(),
      let_ctor_literals: HashSet::new(),
      destructure_srcs: HashMap::new(),
      destructure_parts: HashMap::new(),
      destructure_moved: HashSet::new(),
      rhs_self_moved: BTreeSet::new(),
      destructure_swapped: Vec::new(),
      destructure_ref_data: HashMap::new(),
      destructure_ref_src: HashMap::new(),
      ref_carriers: Vec::new(),
      taken_over_loans: HashSet::new(),
      late_old_ends: Vec::new(),
      or_ref_ends: Vec::new(),
      arm_loans: Vec::new(),
      chain_then: Vec::new(),
      retired_loans: Vec::new(),
      retired_sharers: Vec::new(),
      arm_value_binds: HashSet::new(),
      repointed_loans: Vec::new(),
      first_synthesized_end: 0,
      reference_effects: crate::facts::repoints::ReferenceEffects::new(tcx, mir_body, body_facts),
      drawn_references: Default::default(),
      arm_bindings: BTreeMap::new(),
      arm_frames: Vec::new(),
      defer_old_ends: false,
      deferred_holder_borrows: None,
      dead_loan_checked: HashSet::new(),
      break_moves: Vec::new(),
      loop_given: Vec::new(),
      loop_maybe_entry: Vec::new(),
      all_paths_exit: false,
      all_paths_exit_from: usize::MAX,
      dead_tail: false,
      dead_code: HashSet::new(),
      borrow_floor: HashMap::new(),
      for_moved_holders: HashSet::new(),
      let_else_branches: BTreeSet::new(),
      for_loop_iter_loans: Vec::new(),
      for_loop_moved_bindings: Vec::new(),
      in_scope: Vec::new(),
      try_block: None,
      exit_frames: Vec::new(),
      literal_temps: HashMap::new(),
      premoved: HashSet::new(),
      live_tuples: Vec::new(),
      binding_columns: Vec::new(),
      moves_out_first: Vec::new(),
      approx_notes: Vec::new(),
      temp_emptiers: HashMap::new(),
      temp_emptying_moves: Vec::new(),
      break_binds: Vec::new(),
      deferred_breaks: Vec::new(),
      line_override: None,
      deferred_lines: Vec::new(),
      break_out: Vec::new(),
      labeled_frames: Vec::new(),
      way_out_moves: Vec::new(),
      break_alt: None,
      break_pats: Vec::new(),
      break_assigns: Vec::new(),
      loop_breaks: Vec::new(),
      try_move: None,
      try_field_moves: BTreeSet::new(),
      try_result_titles: BTreeMap::new(),
      try_moves: HashMap::new(),
      tail_values: HashSet::new(),
      tail_plain: HashSet::new(),
      arg_values: HashMap::new(),
      exit_scopes: HashMap::new(),
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
      typing_env(self.tcx, owner),
      ty,
    )
  }

  /// Whether a value of type `ty` has drop glue, judged in the typing
  /// environment of `owner` (an `expr.hir_id.owner` or `pat.hir_id.owner`).
  /// The one entry point for the raw question; see `needs_drop` in
  /// `expr_visitor_utils` for what it does not decide.
  pub fn needs_drop(&self, ty: Ty<'tcx>, owner: rustc_hir::OwnerId) -> bool {
    needs_drop(self.tcx, ty, owner)
  }

  /// Like `needs_drop`, for code that already carries a typing
  /// environment (it threads `env` through helpers that also ask whether
  /// a type is `Copy`).
  pub fn needs_drop_in(&self, ty: Ty<'tcx>, env: rustc_middle::ty::TypingEnv<'tcx>) -> bool {
    needs_drop_in(self.tcx, ty, env)
  }

  /// Like `needs_drop`, for a type from a std or library definition (a
  /// field of `Rc`, a const's type) that has no owner body to judge it in.
  pub fn needs_drop_mono(&self, ty: Ty<'tcx>) -> bool {
    needs_drop_in(self.tcx, ty, rustc_middle::ty::TypingEnv::fully_monomorphized())
  }

  /// The value actually consumed by the call to `Try::branch`.
  pub(crate) fn try_moved_place(&self, try_expr: &Expr<'_>) -> Option<(ResourceTy, TryMove, Site)> {
    let (read, kind) = self.body_facts?.value_flow.try_input(try_expr.span)?;
    let OperandEffect::Move(source) = &read.effect else { return None };
    let operand = try_operand(try_expr).unwrap_or(try_expr);
    let from = self.transfer_source(source, operand)?;
    Some((from, *kind, read.site))
  }

  fn mir_callee(&self, expr: &Expr<'_>) -> Option<rustc_hir::def_id::DefId> {
    let typeck = self.tcx.typeck(expr.hir_id.owner);
    typeck.type_dependent_def_id(expr.hir_id).or_else(|| {
      let ExprKind::Call(f, _) = expr.kind else { return None };
      match typeck.expr_ty(f).kind() {
        rustc_middle::ty::FnDef(def, _) => Some(*def),
        _ => None,
      }
    })
  }

  pub(crate) fn mir_call(&self, expr: &Expr<'_>) -> Option<Call> {
    self.body_facts?.value_flow.call(expr.span, self.mir_callee(expr))
  }

  pub(crate) fn mir_constructor(&self, expr: &Expr<'_>) -> bool {
    self.body_facts.is_some_and(|b| b.value_flow.constructor(expr.span))
  }

  pub(crate) fn mir_argument_constructor(&self, expr: &Expr<'_>) -> bool {
    self.body_facts.is_some_and(|b| b.value_flow.argument_constructor(expr.span))
  }

  /// Resolve the column from the MIR place. When the source spells an
  /// index explicitly, retain that spelling as a label on the proved
  /// column, since MIR's dynamic index is only a temporary local.
  fn transfer_source(&self, source: &Source, expr: &Expr<'_>) -> Option<ResourceTy> {
    if source.guard {
      let ExprKind::Path(qpath) = &expr.kind else { return None };
      let rustc_hir::def::Res::Local(id) = self.tcx.typeck(expr.hir_id.owner).qpath_res(qpath, expr.hir_id) else {
        return None;
      };
      return self.raps.binding(id).map(|rd| ResourceTy::Value(rd.rap.clone()));
    }
    if source.downcast {
      return None;
    }
    if let Some(span) = source.temporary {
      let mut base = expr;
      while let ExprKind::Field(inner, _) = base.kind {
        base = inner;
      }
      if base.span.with_parent(None) == span {
        return self.temp_cols.get(&base.hir_id).map(|r| ResourceTy::Value(r.clone()));
      }
      return None;
    }
    let written_place = if let ExprKind::AddrOf(_, _, place) = expr.kind { place } else { expr };
    let rap = |name: &str| self.raps.place(source.local, name).or_else(|| self.raps.get(name)).map(|r| r.rap.clone());
    if let Some(through) = &source.through {
      let via = rap(through).or_else(|| rap(&source.column))?;
      if source.deref {
        return Some(ResourceTy::Deref(via));
      }
      let ty = self.tcx.typeck(expr.hir_id.owner).expr_ty(expr);
      let label = if source.element { self.place_label(written_place, via.name()) } else { source.label.clone() };
      return Some(ResourceTy::DerefField(via, label, PlaceWrite::Update, self.field_holds(ty, expr.hir_id.owner)));
    }
    let r = rap(&source.column)?;
    if let Some(base) = source.container.as_deref().filter(|_| source.deref).and_then(rap) {
      Some(ResourceTy::Deref(base))
    } else if source.element {
      let label = self.place_label(written_place, r.name());
      Some(ResourceTy::Index(r, label, Default::default()))
    } else if let Some(r) = rap(&source.label) {
      Some(ResourceTy::Value(r))
    } else {
      Some(ResourceTy::Index(r, source.label.clone(), Default::default()))
    }
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
  /// wording must not depend on the platform's std internals. A closure
  /// is classified separately (it can be Copy and still plain), but by
  /// the same rule: it is a resource if it captures such a type by value.
  pub fn ty_is_plain(&self, ty: Ty<'tcx>, owner: rustc_hir::OwnerId) -> bool {
    ty_is_plain(self.tcx, ty, owner)
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
    let env = typing_env(self.tcx, owner);
    go(self.tcx, env, ty, 0, &mut HashSet::new())
  }

  // Adds an owner RAP
  pub fn add_owner(&mut self, name: String, mutability: bool, is_copy: bool, is_plain_value: bool, interior_mut: InteriorMut, scope: usize, is_global: bool) {
    self.add_rap(ResourceAccessPoint::Owner(Owner{name: name, hash: self.rap_hashes as u64, is_mut: mutability, is_copy, is_plain_value, interior_mut, ..Default::default()}), scope, is_global);
  }

  // Adds an owner RAP for a `union`-typed binding (one opaque column;
  // see the AdtKind::Union arms).
  pub fn add_union_owner(&mut self, name: String, mutability: bool, is_copy: bool, is_plain_value: bool, interior_mut: InteriorMut, scope: usize, is_global: bool) {
    self.add_rap(ResourceAccessPoint::Owner(Owner{name: name, hash: self.rap_hashes as u64, is_mut: mutability, is_copy, is_plain_value, interior_mut, is_union: true, ..Default::default()}), scope, is_global);
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
    let mut kind = None;
    for field in adt.all_fields() {
      let fty = field.ty(self.tcx, args);
      if !self.needs_drop_mono(fty) { continue; }
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
      if !me.needs_drop_mono(ty) { return true; }
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
      // rustc prints `{closure@src/main.rs:5:13: 5:20}`: no name a
      // student wrote or can read.
      TyKind::Closure(..) => String::from("closure"),
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
    let needs = |t: Ty<'tcx>| self.needs_drop_mono(t);
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
        .filter(|t| self.needs_drop_mono(*t))
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
  // any event clones the RAP. Siblings sharing a column each get
  // the flags of their own type.
  pub fn mark_owner_type(&mut self, name: &str, ty: Ty<'tcx>) {
    let rc = self.rc_kind(ty);
    let manually_drop = ty.ty_adt_def().map_or(false, |d| d.is_manually_drop());
    // `MaybeUninit<T>` is a union in std, but students know it as a
    // wrapper like ManuallyDrop: it never drops its T (#1146).
    let maybe_uninit = ty.ty_adt_def().is_some_and(|d|
      self.tcx.is_lang_item(d.did(), rustc_hir::LangItem::MaybeUninit));
    let union = ty.is_union() && !maybe_uninit;
    // A union's own `Drop` impl runs when the whole union is dropped or
    // overwritten (#1146).
    let union_drop = union && ty.ty_adt_def().is_some_and(|d| d.has_dtor(self.tcx));
    let rc_fields = self.rc_fields_kind(ty);
    let rc_inside = self.rc_inside_kind(ty);
    // The Rc note wins: no double parenthetical (#542).
    let holds = if rc.is_some() || rc_fields.is_some() || rc_inside.is_some() || manually_drop { None } else { self.holds_phrase(ty) };
    let box_contents_moved = self.raps.local(name).is_some_and(|local| self.raps.bindings.emptied_boxes.contains(&local));
    let elems_left = self.raps.local(name).and_then(|local| self.raps.bindings.arrays.get(&local)).cloned();
    if rc.is_none() && !manually_drop && !maybe_uninit && !union && rc_fields.is_none() && rc_inside.is_none() && holds.is_none() && !box_contents_moved && elems_left.is_none() {
      return;
    }
    let Some(just_added) = (self.rap_hashes as u64).checked_sub(1) else { return };
    let declared = self.raps.get(name).is_some_and(|rd| rd.rap.owner_local().is_some());
    match self.raps.get_mut(name).map(|rd| &mut rd.rap) {
      Some(ResourceAccessPoint::Owner(o)) if declared || o.hash == just_added => {
        o.is_manually_drop = manually_drop;
        o.is_maybe_uninit = maybe_uninit;
        o.is_union = union;
        o.union_drop = union_drop;
        o.rc = rc;
        o.rc_inside = rc_inside;
        o.holds = holds;
        o.box_contents_moved = box_contents_moved;
        o.elems_left = elems_left;
      }
      Some(ResourceAccessPoint::Struct(s)) if declared || s.hash == just_added => {
        s.is_manually_drop = manually_drop;
        s.is_maybe_uninit = maybe_uninit;
        s.is_union = union;
        s.union_drop = union_drop;
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
  pub fn add_closure_owner(&mut self, name: String, mutability: bool, is_plain_value: bool, scope: usize, is_global: bool) {
    self.add_rap(ResourceAccessPoint::Owner(Owner{name: name, hash: self.rap_hashes as u64, is_mut: mutability, is_copy: false, is_plain_value, interior_mut: InteriorMut::No, is_closure: true, ..Default::default()}), scope, is_global);
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
    self.retire_loan(&name, line_num);
    self.borrow_map.insert(name.clone(), RefData { local: self.raps.local(&name), lender: lender.clone(), assigned_at: line_num, lifetime: line_num, ref_mutability: ref_mutability, aliasing: alia, static_lender: None, borrow_span: None, loan_line: None, co_lenders: Vec::new(), lender_muts: Vec::new(), through: Vec::new() });
  }
  /// A variable named `name` declared on `line` takes over the column
  /// of an earlier one of that name, which is out of scope by now
  /// (sibling blocks, two `for x` loops, #903): set the earlier one's
  /// loan aside in `retired_loans`, where `emit_borrow_ends` still ends
  /// it, rather than lose it to the new variable's.
  fn retire_loan(&mut self, name: &str, line: usize) {
    let next = self.raps.local(name);
    if !self.borrow_map.get(name).is_some_and(|old| old.assigned_at < line || old.local.zip(next).is_some_and(|(a, b)| a != b)) {
      return;
    }
    let Some(data) = self.borrow_map.remove(name) else { return };
    // (What the walk noted for the earlier one isn't the later one's.)
    self.borrow_floor.remove(name);
    self.pattern_copies.remove(name);
    // An `if` / `match` arm starts from the loans held before it, so
    // the same loan can be set aside once per arm that declares `name`.
    if !self.retired_loans.iter().any(|r| r.name == name && same_loan(&r.data, &data)) {
      self.retired_loans.push(RetiredLoan { name: name.to_owned(), data, until: line });
    }
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
    ref_pointee_is_copy(self.tcx, ref_ty, owner)
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
        binding: RefBinding::default(),
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
        binding: RefBinding::default(),
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
    self.add_function(name, false);
  }

  pub(crate) fn add_operator(&mut self, name: String) {
    self.add_function(name, true);
  }

  fn add_function(&mut self, name: String, operator: bool) {
    self.add_rap(ResourceAccessPoint::Function(Function {
      mir_result: false, mir_constructor: false, operator, mutable_call_receiver: false,
      callable_result: false, result_path: Vec::new(), result_try: None, name, hash: self.rap_hashes as u64,
      drops_arg: Default::default(), rc_call: None, cast_to: None, inside: None,
    }), self.current_scope, true);
  }

  /// Sibling declarations and their fields share the original header group.
  pub(crate) fn struct_column_hash(&self, name: &str) -> u64 {
    self.raps.get(name).map_or(self.rap_hashes as u64, |rd| *rd.rap.hash())
  }

  // Adds a struct RAP
  // To be honest this struct logic is leftover from RV1 and should eventually be reworked
  // It doesn't make much sense that a struct should be represented any different than an owner
  // the reason for this is that in the frontend structs are visualized differently in the timeline header
  pub fn add_struct(&mut self, name: String, owner: u64, mem: bool, mutability: bool, is_copy: bool, is_plain_value: bool, interior_mut: InteriorMut, scope: usize, is_global: bool) {
    self.add_rap(ResourceAccessPoint::Struct(Struct { local: None,
      name: name,
      hash: self.rap_hashes as u64,
      owner: owner,
      is_mut: mutability,
      is_member: mem,
      is_copy,
      is_plain_value,
      interior_mut,
      is_manually_drop: false, is_union: false, union_drop: false, is_maybe_uninit: false, rc: None, rc_fields: None, rc_inside: None, holds: None, is_box: false, holds_borrow: false }),
      scope, is_global);
  }

  pub fn add_rap(&mut self, mut r: ResourceAccessPoint, scope: usize, is_global: bool) {
    // Functions are keyed in their own namespace (see `fn_key`), so a
    // variable and a function with the same name get separate RAPs.
    let key = if r.is_fn() { fn_key(r.name()) } else { r.name().to_string() };
    self.in_scope.push(key.clone());
    let local = self.raps.local(&key);
    let scope_owner = r.is_member().then(|| self.raps.struct_locals.get(&r.get_owner()).copied()).flatten();
    match &mut r {
      ResourceAccessPoint::MutRef(r) => {
        r.binding.local = local;
        r.binding.scope_owner = scope_owner;
      }
      ResourceAccessPoint::StaticRef(r) => {
        r.binding.scope_owner = scope_owner;
        r.binding.local = local;
        r.binding.slice_item = local.is_some_and(|l| self.body_facts.is_some_and(|f| f.slice_items.contains(&l)));
      }
      ResourceAccessPoint::Owner(o) => o.local = local,
      ResourceAccessPoint::Struct(s) => {
        s.local = if s.is_member { self.raps.struct_locals.get(&s.owner).copied() } else { local };
      }
      _ => {}
    }
    if let Some(owner) = r.scope_local() {
      if is_global { self.raps.top_level_places.insert((owner, key.clone()), scope); }
      if let Some(previous) = self.raps.columns.get(&key).filter(|rd| rd.rap.is_member()) {
        if let Some(old) = previous.rap.scope_local().filter(|&old| old != owner) {
          self.raps.earlier_fields.insert((old, key.clone()), previous.clone());
        }
      }
    }
    if let Some(previous) = self.raps.columns.get(&key).filter(|rd| rd.local != local) {
      if let Some(old) = previous.local { self.raps.earlier.insert(old, (previous.clone(), self.borrow_map.get(&key).cloned())); }
    }
    if let ResourceAccessPoint::Struct(s) = &r {
      if let Some(local) = local.filter(|_| !s.is_member) {
        self.raps.struct_locals.insert(s.owner, local);
      }
    }
    match self.raps.columns.entry(key) {
      std::collections::hash_map::Entry::Vacant(v) => {
        self.rap_hashes += 1;
        v.insert(RapData { rap: r, scope, local, first_local: local, is_global, extended_temp: Vec::new(), temp_owner: None, extra_scopes: Vec::new(), local_later: false });
      }
      // A later variable of the same name in the same fn shares the
      // column of the first (sibling blocks, #308) and ends at its own
      // block end (#589), whether or not either one is declared in a
      // branch or loop body (#931).
      std::collections::hash_map::Entry::Occupied(mut o) => {
        let rd = o.get_mut();
        if !r.is_fn() {
          if rd.local != local || rd.rap.scope_local() != r.scope_local() {
            // Each declaration gets its own semantic description on the shared column.
            let hash = *rd.rap.hash();
            rd.rap = r;
            match &mut rd.rap {
              ResourceAccessPoint::Owner(o) => o.hash = hash,
              ResourceAccessPoint::Struct(s) => s.hash = hash,
              ResourceAccessPoint::StaticRef(r) => r.hash = hash,
              ResourceAccessPoint::MutRef(r) => r.hash = hash,
              ResourceAccessPoint::Function(_) => unreachable!(),
            }
            rd.local = local;
          }
          if is_global && (scope != rd.scope || rd.is_global && local != rd.first_local) && !rd.extra_scopes.contains(&(scope, local)) {
            rd.extra_scopes.push((scope, local));
          } else if !is_global && rd.is_global {
            rd.local_later = true;
          }
        }
      }
    }
  }

  pub fn update_rap(&mut self, r: &ResourceAccessPoint, line_num: usize) {
    // A value holding a borrow keeps it until its last use, like a
    // reference (#336).
    // (A `let r;` reference holds no loan at the start of an arm after
    // one that assigned it, #1069.)
    if (r.is_ref() || r.holds_borrow()) && self.borrow_map.contains_key(r.name()) {
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
        ResourceTy::Value(rap) | ResourceTy::Deref(rap) | ResourceTy::Index(rap, _, _) | ResourceTy::DerefField(rap, _, _, _) => {
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
      | ExternalEvent::TiedParamNote { ro, .. }
      | ExternalEvent::ReslicedInPlace { ro, .. }
      | ExternalEvent::AssignmentJoin { ro, .. } | ExternalEvent::DeadElseNote { ro, .. }
      | ExternalEvent::PassRestDrop { ro, .. }
      | ExternalEvent::DeadLoanNote { ro, .. }
      | ExternalEvent::OwnerDropAtReassign { ro, .. }
      | ExternalEvent::OwnerDropIfTryReturns { ro, .. }
      | ExternalEvent::OwnerDropUnlessReturned { ro, .. }
      | ExternalEvent::TemporaryDrop { ro, .. }
      | ExternalEvent::LoopIteratorDrop { ro, .. }
      | ExternalEvent::LoopMaybeInit { ro, .. }
      | ExternalEvent::LoopExitKept { ro, .. } => {
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
      // Or one of its fields, which move with it (#1280).
      if self.break_out.iter().any(|n| from.name() == n
        || from.name().strip_prefix(n.as_str()).is_some_and(|rest| rest.starts_with('.'))) {
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
    // A borrow of a temporary a `let` extends through a branch ends
    // where it may not exist (#1397): see `branch_temp_cols`.
    if let ExternalEvent::StaticDie { to: ResourceTy::Value(l), .. } | ExternalEvent::MutableDie { to: ResourceTy::Value(l), .. } = &event {
      if self.branch_temp_cols.contains(l.hash()) {
        return;
      }
    }
    if let Some(expr) = self.emitting_expr {
      self.event_spans.entry(event.get_id()).or_insert(expr.span);
    }
    if self.defer_old_ends {
      self.late_old_ends.push((line_num, event));
      return;
    }
    // A place moved into the new value of the place itself (`p.b` in
    // `p = Pair { a: .., b: p.b };`, `t.1` in `t = (s, t.1);`): the
    // right-hand side runs first, so the value moves out and then comes
    // back with the new value. Its column draws the move before the
    // acquire, as for a destructuring cycle (#1343, #1411).
    if let ExternalEvent::Move { from, to, id, .. } = &event {
      let reorder = match (from, to) {
        (ResourceTy::Value(from), ResourceTy::Value(to)) => from.hash() == to.hash(),
        // MIR proves there is no old payload to drop when this store runs.
        (ResourceTy::Deref(from), ResourceTy::Deref(to)) =>
          matches!(to.place_write(), PlaceWrite::AssignOverMoved) && from.hash() == to.hash(),
        _ => false,
      };
      if reorder && !self.moves_out_first.contains(id) {
        self.moves_out_first.push(*id);
      }
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
    self.note_dead_loan_write(line_num, &event);
  }

  /// `event` writes to a variable's column (an assignment, or a `&mut`
  /// borrow): if a loan of the variable is drawn live across the write
  /// though it is dead on the write's path (#796), add the note that
  /// says why rustc accepts the write.
  fn note_dead_loan_write(&mut self, line: usize, event: &ExternalEvent) {
    let written = match event {
      ExternalEvent::Bind { to, .. } | ExternalEvent::Copy { to, .. } | ExternalEvent::Move { to, .. } => to,
      ExternalEvent::MutableBorrow { from, .. } | ExternalEvent::PassByMutableReference { from, .. } => from,
      _ => return,
    };
    let (ResourceTy::Value(r) | ResourceTy::Index(r, ..)) = written else { return };
    if r.is_fn() || r.is_ref() || self.bwf.borrow_set.location_map().is_empty() {
      return;
    }
    let Some(local) = r.owner_local() else { return };
    let Some(expr) = self.emitting_expr else { return };
    // match_rhs emits the assignment while visiting its RHS. An indexed
    // destination can also write via the mutable receiver evaluated on the LHS.
    let site = match self.tcx.parent_hir_node(expr.hir_id) {
      rustc_hir::Node::Expr(parent) if matches!(parent.kind, ExprKind::Assign(_, rhs, _) if rhs.hir_id == expr.hir_id) => parent.span,
      _ => expr.span,
    };
    let Some(dead) = self.dead_loan_at_write(local, site, line) else { return };
    if !self.dead_loan_checked.insert((local, dead.location)) { return; }
    let lender = self.raps.bindings.locals[&local].clone();
    // Keep ids unique, as in `propagate_struct_transfer`: most callers
    // bump unique_id after handing us the event, so the note took the
    // write's own id. Placement then treated the note as the write,
    // already laid out, and left it on its source line, a row off when
    // rows were inserted above it, such as by an earlier fn (#1443).
    if *self.unique_id <= event.get_id() {
      *self.unique_id = event.get_id() + 1;
    }
    let id = *self.unique_id;
    *self.unique_id += 1;
    self.add_external_event(line, ExternalEvent::DeadLoanNote {
      ro: r.clone(), lender, borrower: dead.borrower, repoint: dead.repoint, id,
    });
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
    // `let x = *b;`, with b a Box drawn as its payload's part columns:
    // the parts go with the payload (#898); x's own parts get it.
    let try_move = matches!(event, ExternalEvent::Move { via_try: Some(_), .. });
    let move_source = !try_move || self.try_field_moves.insert(id);
    if let (false, Some(b)) = (is_copy, box_payload_of(from)) {
      if *self.unique_id <= id {
        *self.unique_id = id + 1;
      }
      if move_source {
        self.move_struct_fields_away(line_num, b.name(), &format!("*{}", b.name()));
      }
      if let Some(to_name) = struct_name(to) {
        self.bind_struct_fields(line_num, &to_name);
      }
      return;
    }
    let from_struct = struct_name(from);
    let to_struct = struct_name(to);
    let from_children = from_struct.as_ref().map_or(Vec::new(), |n| self.struct_children(n));
    // Keep ids unique: callers either bump unique_id before or after
    // handing us the event, so start past the event's own id.
    if *self.unique_id <= id {
      *self.unique_id = id + 1;
    }
    if try_move {
      // Try::branch consumes the input. Its output can have entirely
      // different fields, even when the types happen to be the same.
      if let Some(name) = from_struct.as_ref().filter(|_| move_source) {
        self.move_struct_fields_away(line_num, name, name);
      }
      if let Some(name) = &to_struct {
        self.bind_struct_fields(line_num, name);
      }
      return;
    }
    if let (Some(from_name), Some(to_name)) = (&from_struct, &to_struct) {
      self.transfer_ref_fields(line_num, from_name, to_name, is_copy);
    }
    if let Some(from_name) = from_struct.filter(|_| !from_children.is_empty()) {
      let pass_through = match to {
        ResourceTy::Caller => true,
        // `let (a, _) = (1, q);`, `let _ = (q, 5);`: q's fields go into
        // the temporary the literal builds, and are dropped with it (#1154).
        ResourceTy::Value(r) => r.is_fn() || r.is_closure() || r.is_temporary(),
        // `(a, _) = (1, q);`: the same for a destructuring assignment's.
        // (A `match (s, q)` scrutinee's tuple moves q's fields itself.)
        ResourceTy::Temporary { tuple: None, .. } => true,
        // `v[i] = p`: p's fields go into the element with it.
        ResourceTy::Index(..) => true,
        // `*slot = s` (#1168): s's fields go into the place slot points to.
        ResourceTy::Deref(_) | ResourceTy::DerefField(..) => true,
        // `let (x, _) = (s, p);` with `p: P { a: i32 }`: the literal's
        // temporary has no column when nothing the pattern leaves needs
        // drop (#1163), so p moves to no column; its fields go with it
        // all the same (#1284).
        ResourceTy::Anonymous => true,
        _ => false,
      };
      if to_struct.is_none() && !pass_through {
        return;
      }
      let parent = match event {
        ExternalEvent::Move { carried_with: Some((parent, _)), to: ResourceTy::Anonymous, .. } => parent,
        _ => &from_name,
      };
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
        // Preserve the root of an anonymous transfer while its nested
        // fields are propagated. Only Copy fields use the Copy wording.
        let carried_with = (!is_copy && (child.is_copy_owner()
          || (matches!(dest, ResourceTy::Anonymous) && matches!(child, ResourceAccessPoint::Struct(_)))))
          .then(|| (parent.clone(), to.name()));
        let from_child = ResourceTy::Value(child);
        let ev_id = *self.unique_id;
        *self.unique_id += 1;
        let ev = if is_copy {
          ExternalEvent::Copy { from: from_child, to: dest, id: ev_id, is_partial: false, into_closure: false, break_alt: None, closure_value: false, into_ctor: None, pointee: None, of_const: None }
        } else {
          ExternalEvent::Move { from: from_child, to: dest, id: ev_id, is_partial: false, into_closure: false, loop_head: LoopHead::No, break_alt: None, break_out: false, carried_with, via_try: None, closure_value: false }
        };
        self.add_external_event(line_num, ev);
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
  /// and span it was made at), so `emit_borrow_ends` ends it where MIR
  /// ends that loan; the field's own Move arrow makes `other.r` hand
  /// the borrow off (`ref_handed_off`), so it draws no end of its own.
  /// A Copy struct copies the reference: both hold the loan.
  fn transfer_ref_fields(&mut self, line_num: usize, from_name: &str, to_name: &str, is_copy: bool) {
    self.transfer_ref_fields_except(line_num, from_name, to_name, is_copy, None);
  }

  /// `transfer_ref_fields` for the fields of `from_name` not in `skip`:
  /// `S { x: 2, ..s1 }` takes s1's reference fields that aren't written
  /// out (#1126). Only those move, so s1 is partly moved: a `&T` field is
  /// copied (both hold the loan, as for `let q = s1.r;`) and a `&mut T`
  /// field is moved, handing its borrow to the new struct's field.
  pub fn transfer_ref_fields_except(&mut self, line_num: usize, from_name: &str, to_name: &str, is_copy: bool, skip: Option<&HashSet<String>>) {
    let partial = skip.is_some();
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
      if skip.is_some_and(|s| s.contains(field)) {
        continue;
      }
      let is_copy = is_copy || (partial && !child.is_mutref());
      let Some(dest) = self.raps.get(&format!("{}.{}", to_name, field)).map(|rd| rd.rap.clone()) else { continue };
      if let Some(mut loan) = self.borrow_map.get(child.name()).cloned() {
        // The field holds the loan from here on, not from where it was
        // made: `..h` after `if c { return; }` (#1530) mustn't end a
        // loan of h2.r's at that `return`, before h2 exists. The loan
        // is still found in MIR by the line it was issued on.
        if line_num > loan.assigned_at {
          if loan.borrow_span.is_none() && loan.loan_line.is_none() {
            loan.loan_line = Some(loan.assigned_at);
          }
          loan.assigned_at = line_num;
          loan.lifetime = loan.lifetime.max(line_num);
        }
        loan.local = self.raps.local(dest.name());
        self.borrow_map.insert(dest.name().clone(), loan);
      }
      let ev_id = *self.unique_id;
      *self.unique_id += 1;
      let (from, to) = (ResourceTy::Value(child), ResourceTy::Value(dest));
      self.add_external_event(line_num, if is_copy {
        ExternalEvent::Copy { from, to, id: ev_id, is_partial: false, into_closure: false, break_alt: None, closure_value: false, into_ctor: None, pointee: None, of_const: None }
      } else {
        ExternalEvent::Move { from, to, id: ev_id, is_partial: false, into_closure: false, loop_head: LoopHead::No, break_alt: None, break_out: false, carried_with: None, via_try: None, closure_value: false }
      });
    }
  }

  /// `lhs` gets a new value that has no column of its own (a literal,
  /// an operator's result, a macro expansion, a `const`). When `lhs`
  /// is a struct, each of its fields gets its part of that value too,
  /// the same as `let x = make();` (`propagate_struct_transfer`).
  /// A constructor applied to literals only (`Some(5)`, `None`) whose
  /// type is Copy: a value like a literal, with nothing to draw an
  /// arrow from (#768).
  pub fn is_copy_literal_value(&self, e: &'tcx Expr<'tcx>) -> bool {
    let typeck = self.tcx.typeck(e.hir_id.owner);
    matches!(e.kind, ExprKind::Call(..) | ExprKind::Path(..))
      && crate::visitor::is_literal_value(e, typeck)
      && self.tcx.type_is_copy_modulo_regions(
        typing_env(self.tcx, e.hir_id.owner),
        typeck.expr_ty(e),
      )
  }

  /// `let o = Some(5);`: `o` is bound to the value the constructor
  /// builds. The constructor stays the event's source so the dot can
  /// say "o is initialized with a Some value", but nothing is drawn
  /// between them (#768).
  pub fn bind_from_ctor(&mut self, line_num: usize, lhs: ResourceTy, e: &'tcx Expr<'tcx>) {
    let callee = match e.kind {
      ExprKind::Call(callee, _) => callee,
      _ => e,
    };
    let from = match hirid_to_var_name(callee.hir_id, &self.tcx) {
      Some(n) => {
        if !self.raps.contains_key(&fn_key(&n)) {
          self.add_fn(n.clone());
        }
        self.raps.get(&fn_key(&n)).map(|rd| ResourceTy::Value(rd.rap.to_owned())).unwrap_or(ResourceTy::Anonymous)
      }
      None => ResourceTy::Anonymous,
    };
    // `let w = Wrapper(a + &b);`: w's fields get their parts of the new
    // value too, as in `bind_from_anonymous`.
    let struct_name = match &lhs {
      ResourceTy::Value(rap @ ResourceAccessPoint::Struct(_)) => Some(rap.name().clone()),
      _ => None,
    };
    self.add_ev(line_num, Evt::Bind, lhs, from, false);
    if let Some(name) = struct_name {
      self.bind_struct_fields(line_num, &name);
    }
  }

  /// The moves and copies of a constructor's arguments, drawn into the
  /// constructor while its arguments were visited, are redrawn into
  /// `new_to` (the variable holding the built value, or the call it is
  /// passed to): a constructor is never an actor (#768). True if any
  /// event was moved.
  pub fn retarget_ctor_arg_events(&mut self, ctor: &'tcx Expr<'tcx>, new_to: ResourceTy) -> bool {
    let Some(&(lo, hi, hash)) = self.ctor_arg_ids.get(&ctor.hir_id) else { return false };
    let mut taken = Vec::new();
    let returned = matches!(new_to, ResourceTy::Caller);
    let mut returned_parts = |id| returned && (self.try_result_titles.contains_key(&id)
      || self.row_layout.returned_call_titles.contains_key(&id));
    take_ctor_arg_events_in(self.preprocessed_events, &(lo..hi), hash, &mut taken, &mut returned_parts);
    let any = !taken.is_empty();
    for (line, mut event) in taken {
      match &mut event {
        ExternalEvent::Copy { from, to, into_ctor, id, .. } => {
          if !returned && from.extract_rap().is_some_and(|r| r.is_ref()) {
            continue;
          }
          self.set_call_result_path(from, self.transfer_sites.get(id).copied(), &new_to);
          *into_ctor = Some(to.name());
          *to = new_to.clone();
        }
        ExternalEvent::Move { from, to, id, via_try, .. } => {
          if let Some(kind) = via_try.filter(|_| returned) {
            let title = self.try_result_titles.get(id).cloned().unwrap_or_else(|| {
              crate::svg_generator::hover_messages::event_dot_move_into_try(
                &from.name(), None, kind, from.extract_rap().is_some_and(|r| r.holds_value()))
            });
            self.row_layout.aggregate_titles.insert(*id, title);
          }
          self.set_call_result_path(from, self.transfer_sites.get(id).copied(), &new_to);
          *to = new_to.clone();
        }
        _ => unreachable!(),
      }
      self.add_external_event(line, event);
    }
    any
  }

  /// `lhs` is a struct with a column for each of the fields `0..n` of
  /// a tuple struct (`h.0`, `h.1`).
  pub fn has_tuple_field_columns(&self, lhs: &ResourceTy, n: usize) -> bool {
    let ResourceTy::Value(rap @ ResourceAccessPoint::Struct(_)) = lhs else { return false };
    n > 0 && (0..n).all(|i| self.raps.contains_key(&format!("{}.{}", rap.name(), i)))
  }

  /// Drops the moves and copies of the constructor `ctor`'s arguments
  /// that were drawn into the constructor `callee` while its arguments
  /// were visited: `match_struct_field` draws them into the fields
  /// (#1197). Those into a nested constructor stay for it.
  pub fn discard_ctor_arg_events(&mut self, ctor: &'tcx Expr<'tcx>) {
    if let Some(&(lo, hi, hash)) = self.ctor_arg_ids.get(&ctor.hir_id) {
      take_ctor_arg_events_in(self.preprocessed_events, &(lo..hi), hash, &mut Vec::new(), &mut |_| false);
    }
  }

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
  /// shows where the value went. Also the parts of a Box's payload
  /// moved out whole (`*b`, [`box_payload_of`]).
  pub(crate) fn move_struct_fields_away(&mut self, line_num: usize, fields_of: &str, parent: &str) {
    for child in self.struct_children(fields_of) {
      let carried_with = Some((parent.to_owned(), String::new()));
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
        carried_with, via_try: None,
        closure_value: false,
      });
    }
  }

  /// Non-Copy fields of `parent` (recursively) that currently own a
  /// resource, i.e. what dropping `parent` right now would drop.
  pub fn struct_fields_holding_resource(&self, parent: &str) -> Vec<ResourceAccessPoint> {
    let mut out = Vec::new();
    for child in self.struct_children(parent) {
      let name = child.name().clone();
      if !child.never_drops() && self.rap_holds_resource_now(&name) {
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

  /// Whether the Box `rap_name` holds nothing now because its whole
  /// contents were moved out (`let s = *b;`) on every path, while the
  /// Box itself wasn't: its heap allocation is still there to free
  /// (#1145). Only a move at the top level counts (one in a branch is
  /// the maybe-moved case, #921).
  pub fn box_contents_only_moved_now(&self, rap_name: &str) -> bool {
    for (_, ev) in self.preprocessed_events.iter().rev() {
      match ev {
        ExternalEvent::Move { from: ResourceTy::Deref(r), .. } if r.name() == rap_name => return true,
        ExternalEvent::Move { from, to, .. }
          if from.extract_rap().is_some_and(|r| r.name() == rap_name) || to.extract_rap().is_some_and(|r| r.name() == rap_name) => return false,
        ExternalEvent::Copy { to, .. } | ExternalEvent::Bind { to, .. }
          if to.extract_rap().is_some_and(|r| r.name() == rap_name) => return false,
        ExternalEvent::InitRefParam { param, .. } if param.name() == rap_name => return false,
        // A conditional that does something to it: the maybe-moved
        // case (#921), or a new value.
        ExternalEvent::Branch { branches, .. }
          if branches.iter().any(|b| ownership_after(&b.e_data, rap_name, None).is_some()) => return false,
        ExternalEvent::ClosureCall { closure, .. } if closure.extract_rap().is_some_and(|r| r.name() == rap_name) => return false,
        ExternalEvent::LoopExitMove { ro, .. } | ExternalEvent::LoopExitKept { ro, .. } | ExternalEvent::OrAltMove { ro, .. }
          | ExternalEvent::ManuallyDropByHand { ro, .. } if ro.name() == rap_name => return false,
        _ => {}
      }
    }
    false
  }

  /// `rap_ownership_now` as of before `preprocessed_events[end..]`.
  pub fn rap_ownership_before(&self, end: usize, rap_name: &str) -> Ownership {
    ownership_after(&self.preprocessed_events[..end], rap_name, None).unwrap_or(Ownership::None)
  }

  /// Of `live`, the variables that may hold no value where a
  /// conditional starts and that only some of its arms that reach the
  /// join give a new value, the others leaving them as they were
  /// (#1034): `if c { drop(s); } if x { s = …; }`, or `drop(s); if x {
  /// s = …; }`. After the join they
  /// may hold a value (the new one, or the old one if nothing moved
  /// it) or not, and only a drop flag knows. `arms`: the events of the
  /// arms that reach the join, with an empty one for an `if` without
  /// an `else`. `before`: what each holds where the conditional
  /// starts. That it may hold nothing there is also told by an arm
  /// whose assignment to it is a drop flag's (rustc's move data says
  /// so where the drawing doesn't, #1027). A `let x;` local that may
  /// be unassigned after the join (`maybe_init`, #244) only if it may
  /// already hold a value where the conditional starts (`held_before`,
  /// #1036): a loop above may have assigned it, or some branches of a
  /// conditional above; otherwise the arms that assign it decide what's
  /// dropped. Not a reference, or a value with nothing to drop.
  pub fn maybe_held_after(
    &self,
    live: &HashSet<ResourceAccessPoint>,
    arms: &[&[(usize, ExternalEvent)]],
    maybe_init: &HashSet<ResourceAccessPoint>,
    held_before: &HashSet<ResourceAccessPoint>,
    before: impl Fn(&str) -> Ownership,
  ) -> HashSet<ResourceAccessPoint> {
    let mut out = HashSet::new();
    #[expect(clippy::iter_over_hash_type, reason = "only collects into a set; order doesn't matter")]
    for r in live {
      let deferred = maybe_init.contains(r);
      if (deferred && !held_before.contains(r)) || r.is_ref() || r.holds_value() {
        continue;
      }
      let name = r.name();
      let ends: Vec<Option<Ownership>> = arms.iter().map(|a| ownership_after(a, name, None)).collect();
      let assigned = ends.iter().filter(|e| **e == Some(Ownership::Held)).count();
      let untouched = ends.iter().filter(|e| e.is_none()).count();
      if assigned == 0 || untouched == 0 || assigned + untouched != ends.len() {
        continue;
      }
      // (Not one whose drop flag is the arm's own doing: `if c { if d {
      // drop(s); } s = …; }` may have moved s in that arm, above the
      // assignment, which says nothing of where the conditional starts,
      // #1413.)
      // (The assignment's own events, its new value among them, are on
      // its line, before the drop.)
      // (Also one in a conditional in the arm, before which the arm
      // didn't change it: `if c { if d { s = a } else { s = b } }` after
      // a loop that may not have assigned s, #1514.)
      fn drop_flag_assign_in(a: &[(usize, ExternalEvent)], name: &str) -> bool {
        a.iter().enumerate().any(|(i, (line, e))| match e {
          ExternalEvent::OwnerDropAtReassign { ro, maybe_moved, maybe_init, .. }
            if ro.name() == name && (*maybe_moved || maybe_init.is_some()) =>
              ownership_after(&a[..a[..i].iter().rposition(|(l, _)| l != line).map_or(0, |j| j + 1)], name, None).is_none(),
          ExternalEvent::Branch { branches, .. } => ownership_after(&a[..i], name, None).is_none()
            && branches.iter().any(|b| drop_flag_assign_in(&b.e_data, name)),
          _ => false,
        })
      }
      let drop_flag_assign = arms.iter().any(|a| drop_flag_assign_in(a, name));
      if deferred || before(name) != Ownership::Held || drop_flag_assign {
        out.insert(r.clone());
      }
    }
    out
  }

  /// True when a let-chain's condition (or a match guard, #247) may
  /// have moved `rap` before it failed (#281): a `MaybeMovedAt::ChainFail`
  /// event for it, also inside a finished conditional, with no new value
  /// for it after that on the same path (`s = …` in the else branch). A
  /// drop-flag message then says "nothing above" moved it rather than
  /// "no branch above". The events are the whole crate's, so this matches
  /// the variable, not its name: a same-named one in another function
  /// doesn't count (#1002).
  pub fn moved_by_chain_condition(&self, rap: &ResourceAccessPoint) -> bool {
    fn lasts_along(mut lasts: bool, events: &[(usize, ExternalEvent)], hash: u64) -> bool {
      for (_, ev) in events {
        match ev {
          ExternalEvent::LoopExitMove { ro, at: MaybeMovedAt::ChainFail { .. } | MaybeMovedAt::WhileCondFail { .. }
            | MaybeMovedAt::Loop { nothing_above: true, .. } | MaybeMovedAt::LoopHead | MaybeMovedAt::LoopRound { .. }, .. }
            if *ro.hash() == hash => lasts = true,
          ExternalEvent::Move { to: ResourceTy::Value(r), .. }
          | ExternalEvent::Copy { to: ResourceTy::Value(r), .. }
          | ExternalEvent::Bind { to: ResourceTy::Value(r), .. } if *r.hash() == hash => lasts = false,
          ExternalEvent::Branch { branches, .. } => {
            lasts = branches.iter().any(|b| lasts_along(lasts, &b.e_data, hash));
          }
          _ => {}
        }
      }
      lasts
    }
    lasts_along(false, self.preprocessed_events, *rap.hash())
  }

  /// Whether what last changed `name`'s state, in the events so far, is
  /// the start of a loop body that the loop can come back to with it
  /// moved (`MaybeMovedAt::LoopHead`, #992): a reassignment then drops
  /// the old value the first time round, and later only if the time
  /// round before didn't leave it moved. A conditional in between
  /// counts only if one of its branches that reach the join changes it
  /// (not `if n >= k { break; }`).
  pub fn maybe_moved_by_loop_head_now(&self, name: &str) -> bool {
    fn changes(e: &ExternalEvent, name: &str) -> bool {
      let is = |r: &ResourceTy| r.extract_rap().is_some_and(|r| r.name() == name);
      match e {
        ExternalEvent::Branch { branches, .. } =>
          branches.iter().filter(|b| !b.diverges).any(|b| b.e_data.iter().any(|(_, e)| changes(e, name))),
        ExternalEvent::LoopExitMove { ro, .. } | ExternalEvent::OrAltMove { ro, .. }
        | ExternalEvent::LoopExitKept { ro, .. } | ExternalEvent::LoopMaybeInit { ro, .. }
        | ExternalEvent::ManuallyDropByHand { ro, .. } | ExternalEvent::OwnerDropAtReassign { ro, .. } => ro.name() == name,
        ExternalEvent::InitRefParam { param, .. } => param.name() == name,
        ExternalEvent::Move { from, to, .. } => is(from) || is(to),
        ExternalEvent::Copy { to, .. } | ExternalEvent::Bind { to, .. } => is(to),
        _ => false,
      }
    }
    for (_, e) in self.preprocessed_events.iter().rev() {
      if let ExternalEvent::LoopExitMove { ro, at, .. } = e {
        if ro.name() == name {
          return *at == MaybeMovedAt::LoopHead;
        }
      }
      if changes(e, name) {
        return false;
      }
    }
    false
  }

  /// What reassigning the struct or tuple `parent` drops of its
  /// previous value, when some of its parts were moved out before, on
  /// every path or on some only (#628): `None` when none were.
  pub fn reassign_parts(&self, parent: &str) -> Option<ReassignParts> {
    let mut out = ReassignParts::default();
    self.collect_reassign_parts(parent, &mut out);
    (!out.moved.is_empty() || !out.maybe_moved.is_empty()).then_some(out)
  }

  fn collect_reassign_parts(&self, parent: &str, out: &mut ReassignParts) {
    for c in self.struct_children(parent) {
      if c.never_drops() || c.is_plain_value() {
        continue;
      }
      let name = c.name().clone();
      if self.rhs_self_moved.iter().any(|m| crate::visitor::place_within(&name, m)) {
        out.moved.push(name);
        continue;
      }
      match self.rap_ownership_now(&name) {
        Ownership::None => out.moved.push(name),
        Ownership::Maybe => {
          out.by_cond |= self.maybe_moved_by_let_now(&name) || self.moved_by_chain_condition(&c);
          out.maybe_moved.push(name);
        }
        Ownership::Held => {
          if self.struct_children(&name).is_empty() {
            out.dropped.push(name);
          } else {
            self.collect_reassign_parts(&name, out);
          }
        }
      }
    }
  }

  /// Whether `name`, maybe-moved now (`Ownership::Maybe`), is so because
  /// of a `let` whose or-pattern binds a name from different places
  /// (`let ((x, _) | (_, x)) = t;`, #597) rather than a branch: the
  /// timeline's `maybe_moved_by_let`, on the events so far.
  pub fn maybe_moved_by_let_now(&self, name: &str) -> bool {
    let is = |r: &ResourceTy| r.extract_rap().is_some_and(|r| r.name() == name);
    for (_, ev) in self.preprocessed_events.iter().rev() {
      match ev {
        ExternalEvent::OrAltMove { ro, tuple: None, in_let, .. } if ro.name() == name => return *in_let,
        ExternalEvent::LoopExitMove { ro, .. } | ExternalEvent::LoopExitKept { ro, .. } if ro.name() == name => return false,
        ExternalEvent::Move { from, to, .. } if is(from) || is(to) => return false,
        ExternalEvent::Copy { to, .. } | ExternalEvent::Bind { to, .. } if is(to) => return false,
        ExternalEvent::Branch { branches, .. }
          if branches.iter().any(|b| ownership_after(&b.e_data, name, Some(Ownership::Held)) != Some(Ownership::Held)) => return false,
        _ => {}
      }
    }
    false
  }

  /// True iff `name` is a reference whose value was moved, since it was
  /// last assigned, into something that carries the borrow on and ends
  /// it itself: another reference (`let r2 = r;`, `Holder { r }`) or the
  /// caller (`r` as a fn's result, also from a branch of its tail `if` / `match`). After such a move `name` holds
  /// nothing, so its own borrow-end / return-to-lender event would be
  /// false (#209). So is a `&mut` moved into a call whose result a
  /// holder keeps (`let r2 = id(r);`, see `moved_into_result`, #1128).
  /// Any other `&mut` moved into a call (`take(r)`) isn't covered:
  /// nothing else with a column ends that borrow, so `r`'s end stands
  /// for it.
  /// Whether reference `name` has gone out of scope in the events drawn
  /// so far (its block or arm ended).
  pub fn went_out_of_scope(&self, name: &str) -> bool {
    let from = self.borrow_map.get(name).map_or(0, |d| d.assigned_at);
    self.raps.get(name).is_some_and(|rd| rd.rap.is_ref()
      && gos_line_from(self.preprocessed_events, &rd.rap, from).is_some())
  }

  pub fn ref_handed_off(&self, name: &str) -> bool {
    self.ref_handed_off_to(name, false)
  }

  fn ref_handed_off_to(&self, name: &str, caller_only: bool) -> bool {
    let (Some(rd), Some(rap_data)) = (self.borrow_map.get(name), self.raps.get(name)) else {
      return false;
    };
    // Moved into a `for` loop's iterator, which ends the loan where the
    // loop ends (#1365).
    if !caller_only && self.loop_moved_refs.get(name) == Some(&rd.assigned_at) {
      return true;
    }
    let hash = *rap_data.rap.hash();
    let real_name = rap_data.rap.name();
    let swapped_here = |line: usize| self.destructure_swapped.iter()
      .any(|(n, lo, hi)| n == real_name && (*lo..=*hi).contains(&line) && (*lo..=*hi).contains(&rd.assigned_at));
    let mine = |line: usize| line >= rd.assigned_at;
    // `in_branch`: the event is inside a branch of a conditional. There
    // only a hand-off to the caller counts: a `&mut` moved or a shared
    // reference copied to the caller (`if c { x } else { y }` as the
    // tail, #742): the reference goes to the caller on
    // that path, so its borrow doesn't end there. The same holds for a
    // shared reference parameter that is the fn's whole result
    // (`fn id(x: &String) -> &String { x }`, #754).
    fn handed(this: &ExprVisitor, events: &[(usize, ExternalEvent)], hash: u64, mine: &dyn Fn(usize) -> bool, swapped_here: &dyn Fn(usize) -> bool, in_branch: bool, caller_only: bool) -> bool {
      events.iter().any(|(line, ev)| match ev {
        ExternalEvent::Move { from: ResourceTy::Value(f), to, id, .. }
          if *f.hash() == hash && mine(*line) && !swapped_here(*line) =>
        {
          match to {
            ResourceTy::Caller => true,
            // Into a call whose result carries the borrow on (#1128).
            ResourceTy::Value(ResourceAccessPoint::Function(_)) => !in_branch && !caller_only
              && this.moved_into_result.contains(id),
            // Moves into another reference inside a branch keep the old
            // drawing, unless the reference was declared in that branch.
            // A tuple struct built around it (`let w = W(r);`): the
            // field holding it carries the borrow on (#1128).
            ResourceTy::Value(t @ ResourceAccessPoint::Struct(_)) if !in_branch && !caller_only
              && !this.borrow_map.contains_key(t.name()) => f.is_mutref()
              && this.struct_members_deep(t.name()).iter().any(|m| (m.is_ref() || m.holds_borrow()) && this.borrow_map.contains_key(m.name())),
            ResourceTy::Value(t) if !in_branch && !caller_only => this.borrow_map.contains_key(t.name())
              && this.raps.get(t.name()).map_or(false, |r| r.rap.hash() == t.hash()),
            _ => false,
          }
        }
        ExternalEvent::Copy { from: ResourceTy::Value(f), to: ResourceTy::Caller, .. }
          if *f.hash() == hash && mine(*line) => true,
        // `&self.s` returned: a reborrow through `self` goes to the caller (#784).
        ExternalEvent::Copy { from: ResourceTy::DerefField(f, place, _, _), to: ResourceTy::Caller, .. }
          if place.starts_with('&') && *f.hash() == hash && mine(*line) => true,
        // A reference declared in the branch (an arm's pattern binding,
        // `Some(x) => { let r = x; .. }`, or a `let` in the arm) exists
        // on that path only: a move into another reference there hands
        // its borrow off for good, as outside a conditional (#1333).
        ExternalEvent::Branch { branches, .. } => branches.iter()
          .any(|b| {
            #[expect(clippy::disallowed_methods, reason = "any() makes order irrelevant")]
            let declared_here = b.decl_vars.iter().any(|v| *v.hash() == hash);
            handed(this, &b.e_data, hash, mine, swapped_here, !declared_here, caller_only)
          }),
        _ => false,
      })
    }
    handed(self, self.preprocessed_events, hash, &mine, &swapped_here, false, caller_only)
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

  /// `let s: &String = r;`, `V { r }` with a `&String` field, `q = r;`
  /// with `q: &String`, r a `&mut String`: the `&mut` is coerced to a
  /// `&` by an implicit `&*r`, an immutable reborrow through r, not a
  /// move (or a copy) of r (#1329). The shared counterpart of
  /// `is_coerced_mut_reborrow`.
  pub fn is_coerced_shared_reborrow(&self, lhs: &ResourceTy, rhs_rap: &ResourceAccessPoint, rhs: &Expr, evt: &Evt) -> bool {
    matches!(evt, Evt::Move | Evt::Copy)
      && matches!(lhs, ResourceTy::Value(_))
      && rhs_rap.is_mutref()
      && is_implicit_shared_reborrow_of_mut(rhs, self.tcx.typeck(rhs.hir_id.owner))
  }

  pub fn ext_ev_of_evt(&self, evt: Evt, lhs: ResourceTy, rhs: ResourceTy, id: usize, is_partial: bool) -> ExternalEvent{
    let pattern_ref = |holder: &ResourceTy, lender: &ResourceTy| holder.extract_rap()
      .and_then(|h| self.pattern_loans.get(h.hash()))
      .is_some_and(|l| lender.is_ref() && lender.real_name() == l.lender);
    match evt {
      Evt::Bind => ExternalEvent::Bind { from: rhs, to: lhs, id, break_alt: None },
      Evt::Copy => {
        // A `&` sub-pattern's binding (#874).
        let pointee = lhs.extract_rap()
          .and_then(|r| self.pointee_copies.get(r.hash()))
          .filter(|(parent, _)| *parent == rhs)
          .map(|(_, part)| *part);
        let is_partial = is_partial && pointee.is_none();
        ExternalEvent::Copy { from: rhs, to: lhs, id, is_partial, into_closure: false, break_alt: None, closure_value: false, into_ctor: None, pointee, of_const: None }
      }
      Evt::Move => ExternalEvent::Move { from: rhs, to: lhs, id, is_partial, into_closure: false, loop_head: LoopHead::No, break_alt: None, break_out: false, carried_with: None, via_try: self.try_move, closure_value: false },
      Evt::SBorrow => ExternalEvent::StaticBorrow { of_ref_var: pattern_ref(&lhs, &rhs), from: rhs, to: lhs, id, is_partial, is_reborrow: false, break_alt: None, via: None },
      Evt::MBorrow => ExternalEvent::MutableBorrow { of_ref_var: pattern_ref(&lhs, &rhs), from: rhs, to: lhs, id, is_partial, is_reborrow: false, break_alt: None, via: None },
      Evt::PassBySRef => ExternalEvent::PassByStaticReference { from: rhs, to: lhs, id, implicit: ImplicitRef::No, refcell_borrow_mut: false, pointee_interior_mut: false, result_keeps_borrow: false, stored_in: StoredIn::default(), break_alt: None, still_borrowed_by: Vec::new(), of_ref_var: false, deep: None },
      Evt::PassByMRef => ExternalEvent::PassByMutableReference {
        manually_takes: false, terminal: None, from: rhs, to: lhs, id, is_reborrow: false, ends_at_call: false, implicit: ImplicitRef::No,
        result_keeps_borrow: false, stored_in: StoredIn::default(), break_alt: None, of_ref_var: false, deep: None,
      },
      Evt::SDie =>  ExternalEvent::StaticDie { slot: id, of_ref_var: pattern_ref(&rhs, &lhs), from: rhs, to: lhs, id, static_lender: None, still_borrowed_by: Vec::new(), exhaustion: Vec::new() },
      Evt::MDie => ExternalEvent::MutableDie { slot: id, of_ref_var: pattern_ref(&rhs, &lhs), from: rhs, to: lhs, id, static_lender: None, still_borrowed_by: Vec::new(), exhaustion: Vec::new() }
    }
  }

  /// Pairs each branch body of a conditional (`if` / `match`) with
  /// `lhs` (`match_rhs_arm`). A reference field (or tuple element) of
  /// `lhs` that the branches point at different lenders (`let d = if c {
  /// One { a: &s1 } } else { One { a: &s2 } };`) may point into any of
  /// them after the conditional, and the borrow checker keeps all of
  /// them borrowed until its last use (#1402): it borrows the union,
  /// as a reference bound to the conditional does (`get_ref_data_co`).
  /// Pairing a branch sets the field's lender to that branch's, so each
  /// branch's lenders are collected after it is paired. (So does a part
  /// that holds a borrow, and a pattern that takes `lhs` apart later
  /// borrows them all too: `let W { r } = km;`, `adopt_part_borrow`,
  /// #1471.)
  fn match_rhs_arms(&mut self, lhs: ResourceTy, bodies: &[&'tcx Expr<'tcx>], evt: Evt) {
    let parts = self.ref_parts_of(&lhs);
    let before: Vec<Option<RefData>> = parts.iter().map(|n| self.borrow_map.get(n).cloned()).collect();
    let mut found: Vec<Vec<ResourceTy>> = vec![Vec::new(); parts.len()];
    for body in bodies {
      self.match_rhs_arm(lhs.clone(), body, evt.clone());
      // One paired once it has been visited (`bind_deferred_arm`)
      // adds its lenders then.
      if !parts.is_empty() && self.deferred_arm_binds.contains_key(&body.hir_id) {
        self.arm_union_parts.insert(body.hir_id, parts.iter().cloned().zip(before.iter().cloned()).collect());
      }
      for (i, n) in parts.iter().enumerate() {
        let Some(rd) = self.borrow_map.get(n) else { continue };
        // (Not one this branch left as it was before the conditional.)
        if before[i].as_ref().is_some_and(|b| same_loan(b, rd)) {
          continue;
        }
        add_lenders(&mut found[i], rd);
      }
    }
    for (n, lenders) in parts.iter().zip(found) {
      self.set_part_lenders(n, lenders);
    }
  }

  /// The columns inside `lhs` that hold a loan, for `match_rhs_arms`:
  /// references (`d.a`, `t.0`, `o.inner.a`) and parts that hold a borrow
  /// (`km.r` an `Option<&mut String>`, `t.0` in `let t = match .. { .. =>
  /// (Some(&s1), 1), .. }`, #1471).
  fn ref_parts_of(&self, lhs: &ResourceTy) -> Vec<String> {
    match lhs {
      ResourceTy::Value(rap) if !rap.is_ref() => {
        let prefix = format!("{}.", rap.name());
        #[expect(clippy::disallowed_methods, reason = "sorted on the next line")]
        let mut v: Vec<String> = self.raps.iter()
          .filter(|(n, rd)| n.starts_with(&prefix) && (rd.rap.is_ref() || rd.rap.holds_borrow()))
          .map(|(n, _)| n.clone())
          .filter(|n| self.borrow_map.contains_key(n))
          .collect();
        v.sort();
        v
      }
      _ => Vec::new(),
    }
  }

  /// Reference part `n` may point into any of `lenders`, what the
  /// branches pointed it at: it borrows all of them (`match_rhs_arms`).
  /// (Also when the last branch's value borrows nothing with a column,
  /// `One { a: "x" }`: the others' lenders stay borrowed.)
  fn set_part_lenders(&mut self, n: &str, mut lenders: Vec<ResourceTy>) {
    let mut cur = Vec::new();
    if let Some(rd) = self.borrow_map.get(n) {
      add_lenders(&mut cur, rd);
    }
    if lenders.is_empty() || lenders == cur {
      return;
    }
    let lender = lenders.remove(0);
    let aliasing = get_aliasing_data(&lender, &self.borrow_map);
    if let Some(rd) = self.borrow_map.get_mut(n) {
      rd.lender = lender;
      rd.aliasing = aliasing;
      rd.static_lender = None;
      rd.co_lenders = lenders;
    }
  }

  /// A branch's value made by a call (`if c { mk(&s1) } else { .. }`):
  /// the reference fields of struct `lhs` borrow what the callee's
  /// signature ties them to, as for `let d = mk(&s1);` (#1402), where
  /// `define_lhs` finds them from the `let`'s initialiser.
  fn register_arm_call_lenders(&mut self, lhs: &ResourceTy, body: &'tcx Expr<'tcx>) {
    let ResourceTy::Value(ResourceAccessPoint::Struct(st)) = lhs else { return };
    let Some(value) = branch_value_expr(body) else { return };
    if value.span.from_expansion() {
      return;
    }
    let Some(ty) = self.tcx.typeck(value.hir_id.owner).expr_ty_opt(value) else { return };
    let name = st.name.clone();
    self.register_struct_field_lenders(&name, value, ty);
  }

  /// `match_rhs` for one branch of an `if` / `match` whose value is
  /// bound to `lhs`, recording the event ids it used (`rhs_arm_ids`).
  fn match_rhs_arm(&mut self, lhs: ResourceTy, body: &'tcx Expr<'tcx>, evt: Evt) {
    // `best = if c { &b } else { best };` (#1430): the branch giving the
    // reference back its own value changes nothing on that path.
    if let ResourceTy::Value(r) = &lhs {
      if r.is_ref() && branch_value_expr(body).is_some_and(|v| matches!(v.kind, ExprKind::Path(QPath::Resolved(None, _)))
        && expr_to_rap_name(v, &self.tcx).as_deref() == Some(r.name())) {
        let lo = *self.unique_id;
        self.rhs_arm_ids.insert(body.hir_id, (lo, lo));
        return;
      }
    }
    if self.arms_paired_in_branch.contains(&body.hir_id) {
      return;
    }
    // `v = if …` reassigning v (#1010): each branch is paired after it
    // is visited, when what it did to v is known. (A nested
    // conditional pairs each of its own branches.)
    if let Some((old, drops)) = self.reassign_arms.clone().filter(|_| !branch_value_is_conditional(body)) {
      self.deferred_arm_binds.entry(body.hir_id).or_insert_with(|| (body, Vec::new())).1.push(ArmBind::Reassign(lhs, evt, old, drops));
      return;
    }
    if self.defer_arm_binds.is_some_and(|within| self.arm_value_uses_local_in(body, within)) {
      self.deferred_arm_binds.entry(body.hir_id).or_insert_with(|| (body, Vec::new())).1.push(ArmBind::Rhs(lhs, evt));
      return;
    }
    let lo = *self.unique_id;
    self.register_arm_call_lenders(&lhs, body);
    self.match_rhs(lhs.clone(), body, evt);
    self.emit_arm_holder_literal_borrows(&lhs, body);
    self.rhs_arm_ids.insert(body.hir_id, (lo, *self.unique_id));
  }

  /// Whether the value branch body `body` produces (its block's tail,
  /// not a nested `if` / `match`, whose own branches are looked at
  /// one by one) can only be paired once the branch has been visited:
  /// it uses a variable declared within `within`, one that doesn't
  /// exist until the branch is visited (#994), or a variable the
  /// branch's statements use before it (`if c { println!("{}", s); s
  /// }`, #1410), whose events there come before the value's.
  pub(crate) fn arm_value_uses_local_in(&self, body: &'tcx Expr<'tcx>, within: rustc_span::Span) -> bool {
    let mut e = body;
    let mut stmts: Vec<&'tcx rustc_hir::Stmt<'tcx>> = Vec::new();
    loop {
      match e.kind {
        ExprKind::Block(b, _) => match b.expr {
          Some(t) => {
            stmts.extend(b.stmts.iter());
            e = t;
          }
          None => return false,
        },
        ExprKind::DropTemps(t) => e = t,
        ExprKind::If(..) | ExprKind::Match(..) => return false,
        _ => break,
      }
    }
    // A call through a local (`f()`, an `FnOnce` closure the call
    // consumes, #1526): the call's events, made as the branch is
    // visited, come before the value's.
    if matches!(e.kind, ExprKind::Call(callee, _) if local_of_path(callee).is_some()) {
      return true;
    }
    // (What a closure captures counts as used where the closure is
    // made: `let c = || s.len(); s`, #1523.)
    struct Locals<'tcx> { tcx: TyCtxt<'tcx>, hids: Vec<rustc_hir::HirId> }
    impl<'tcx> rustc_hir::intravisit::Visitor<'tcx> for Locals<'tcx> {
      fn visit_path(&mut self, path: &rustc_hir::Path<'tcx>, _id: rustc_hir::HirId) {
        if let Res::Local(hid) = path.res {
          self.hids.push(hid);
        }
        rustc_hir::intravisit::walk_path(self, path);
      }
      fn visit_expr(&mut self, e: &'tcx Expr<'tcx>) {
        if let ExprKind::Closure(c) = e.kind {
          if let Some(upvars) = self.tcx.upvars_mentioned(c.def_id) {
            self.hids.extend(upvars.keys().copied());
          }
        }
        rustc_hir::intravisit::walk_expr(self, e);
      }
    }
    let mut v = Locals { tcx: self.tcx, hids: Vec::new() };
    rustc_hir::intravisit::Visitor::visit_expr(&mut v, e);
    if v.hids.iter().any(|&hid| within.contains(self.tcx.hir_span(hid))) {
      return true;
    }
    // A temporary the value borrows and the branch makes (#1397).
    if self.makes_branch_temp(e) {
      return true;
    }
    let mut used = Locals { tcx: self.tcx, hids: Vec::new() };
    for st in stmts.iter() {
      rustc_hir::intravisit::Visitor::visit_stmt(&mut used, st);
    }
    if v.hids.iter().any(|h| used.hids.contains(h)) {
      return true;
    }
    // A branch whose statements can leave it early (`if c { if
    // r.is_empty() { return 1; } s } ..`, #1501): the exit's drops come
    // before the value is given, so `s` still holds its value there.
    stmts.iter().any(|st| stmt_may_exit(self.tcx, st))
  }

  /// Whether `e` contains a temporary that a `let` extends through a
  /// branch (`branch_temp_names`, #1397): it gets its column only as the
  /// branch is visited.
  pub fn makes_branch_temp(&self, e: &'tcx Expr<'tcx>) -> bool {
    if self.branch_temp_names.is_empty() {
      return false;
    }
    struct Temps<'b> { names: &'b HashMap<rustc_hir::HirId, String>, found: bool }
    impl<'tcx> rustc_hir::intravisit::Visitor<'tcx> for Temps<'_> {
      fn visit_expr(&mut self, e: &'tcx Expr<'tcx>) {
        if self.names.contains_key(&e.hir_id) {
          self.found = true;
        }
        rustc_hir::intravisit::walk_expr(self, e);
      }
    }
    let mut v = Temps { names: &self.branch_temp_names, found: false };
    rustc_hir::intravisit::Visitor::visit_expr(&mut v, e);
    v.found
  }

  /// Pairs the value of branch body `body`, now visited, with the
  /// `let` it is bound to, if that was put off (`deferred_arm_binds`,
  /// #994): its events come last in the branch, before its variables
  /// go out of scope.
  pub fn bind_deferred_arm(&mut self, body: &'tcx Expr<'tcx>) {
    let Some((_, binds)) = self.deferred_arm_binds.remove(&body.hir_id) else { return };
    let lo = *self.unique_id;
    for bind in binds {
      match bind {
        ArmBind::Rhs(lhs, evt) => {
          self.add_arm_lenders(&lhs, body);
          // What the branches paired before this one pointed its
          // reference parts at stays among their lenders (#1402).
          let union = self.arm_union_parts.remove(&body.hir_id).unwrap_or_default();
          let earlier: Vec<Vec<ResourceTy>> = union.iter().map(|(n, before)| {
            let mut v = Vec::new();
            if let Some(rd) = self.borrow_map.get(n).filter(|rd| !before.as_ref().is_some_and(|b| same_loan(b, rd))) {
              add_lenders(&mut v, rd);
            }
            v
          }).collect();
          self.register_arm_call_lenders(&lhs, body);
          self.match_rhs(lhs.clone(), body, evt);
          // Each branch gives it its value, and with it its loans (its
          // parts' too: `(Some(r), 1)`), which the arms' loans are merged
          // into (`arm_borrows_end`, #1453).
          let mut bound: Vec<String> = Vec::new();
          if let ResourceTy::Value(r) = &lhs {
            let prefix = format!("{}.", r.name());
            #[expect(clippy::disallowed_methods, reason = "collected into a set")]
            let names: Vec<String> = self.raps.iter()
              .filter(|(n, _)| *n == r.name() || n.starts_with(&prefix))
              .filter(|(_, rd)| rd.rap.is_ref() || rd.rap.holds_borrow())
              .map(|(n, _)| n.clone())
              .collect();
            if !r.is_ref() {
              bound = names.clone();
            }
            self.arm_value_binds.extend(names);
          }
          self.emit_arm_holder_literal_borrows(&lhs, body);
          for ((n, _), mut lenders) in union.iter().zip(earlier) {
            if let Some(rd) = self.borrow_map.get(n) {
              add_lenders(&mut lenders, rd);
            }
            self.set_part_lenders(n, lenders);
          }
          // What a holder (or its part) reborrows through a reference
          // the branch declares is that reference's loan once the branch
          // is over (#1482).
          bound.sort();
          for n in &bound {
            self.resolve_arm_local_lenders(n, body);
          }
        }
        ArmBind::Pat(pat) => self.pair_pat_with_value(pat, body),
        ArmBind::Reassign(lhs, evt, old, drops) => self.reassign_from_arm(lhs, body, evt, old, drops),
        ArmBind::Assign(block) => self.assign_branch_value(block, body),
      }
    }
    self.rhs_arm_ids.insert(body.hir_id, (lo, *self.unique_id));
  }

  /// `let r = match o { Some(x) => x, None => "d" };` with `r` a
  /// reference (#1327): r was declared before the branches were
  /// visited, when the value of branch body `body`, which names a
  /// variable the branch declares (`x`, or `y` in `{ let y = &a; y }`),
  /// had no lender yet (`get_ref_data_co` leaves it out). Now that the
  /// branch has been visited, and before its variables go out of scope,
  /// r also borrows what that value borrows: as rustc sees it, r may
  /// hold either branch's reference.
  fn add_arm_lenders(&mut self, lhs: &ResourceTy, body: &'tcx Expr<'tcx>) {
    let ResourceTy::Value(r) = lhs else { return };
    if !r.is_ref() {
      self.add_arm_holder_lenders(r.name(), body);
      return;
    }
    let Some(value) = branch_value_expr(body) else { return };
    self.add_ref_value_lenders(r.name(), value);
  }

  /// Reference `name`, declared before the conditional whose branch
  /// gives it `value`, may hold `value`'s reference: it also borrows
  /// what `value` borrows (`add_arm_lenders`).
  fn add_ref_value_lenders(&mut self, name: &String, value: &'tcx Expr<'tcx>) {
    if self.holder_shadowed(name) {
      return;
    }
    let (lender, _, co) = self.get_ref_data_co(value);
    let found: Vec<ResourceTy> = std::iter::once(lender).chain(co)
      .filter(|l| matches!(l, ResourceTy::Value(_) | ResourceTy::Deref(_)))
      .collect();
    for l in found {
      let aliasing = get_aliasing_data(&l, &self.borrow_map);
      let Some(rd) = self.borrow_map.get_mut(name) else { return };
      if rd.all_lenders().contains(&l) {
        continue;
      }
      if matches!(rd.lender, ResourceTy::Value(_) | ResourceTy::Deref(_)) {
        rd.co_lenders.push(l);
      } else {
        rd.lender = l;
        rd.aliasing = aliasing;
        rd.static_lender = None;
      }
    }
  }

  /// A binding of a pattern that takes a branch's value apart (`let One
  /// { a } = if c { One { a: &s1 } } else { One { a: &s2 } };`, `let (h,
  /// n) = if c { (Some(&s1), 1) } else { .. };`, #1405), declared before
  /// the branches (`declare_pat_bindings`), gets `value`, its part of
  /// one branch's value: it may hold what any branch gives it, so rustc
  /// keeps every branch's lender borrowed while it is in use. A
  /// reference adds `value`'s lenders; a value that holds a borrow is
  /// made a holder (its declaration saw only the whole conditional) and
  /// adds what `value` holds (`add_arm_holder_lenders`).
  pub fn add_value_lenders(&mut self, name: &String, value: &'tcx Expr<'tcx>) {
    let Some(rap) = self.raps.get(name).map(|rd| rd.rap.to_owned()) else { return };
    let Some(ty) = self.tcx.typeck(value.hir_id.owner).expr_ty_opt(value) else { return };
    if ty.is_never() {
      return;
    }
    if rap.is_ref() {
      self.add_ref_value_lenders(name, value);
      return;
    }
    if !rap.holds_borrow() && (self.holder_lenders(value, ty).0.is_empty() || !self.mark_holder(name)) {
      return;
    }
    self.add_arm_holder_lenders(name, value);
  }

  /// `add_arm_lenders` for a value that holds a borrow (#1435): `let t =
  /// if c { Some(&String::from("x")) } else { None };`, `let t = match o
  /// { Some(n) => { let y = &s; Some(y) } _ => None };`. t was marked a
  /// holder when declared (`register_holder`); now that the branch has
  /// been visited, t also holds what that branch's value borrows.
  pub(crate) fn add_arm_holder_lenders(&mut self, name: &str, body: &'tcx Expr<'tcx>) {
    let Some(value) = branch_value_expr(body) else { return };
    let Some(ty) = self.tcx.typeck(value.hir_id.owner).expr_ty_opt(value) else { return };
    let mut closure_kinds = Vec::new();
    let mut closure_lenders = Vec::new();
    self.extend_closure_result_lenders(name, name, value, &mut closure_lenders, &mut closure_kinds);
    if !self.raps.get(name).is_some_and(|rd| rd.rap.holds_borrow())
      && (closure_lenders.is_empty() || !self.mark_holder(name)) {
      return;
    }
    // A branch of `let k = Some(if c { let r = &s1; r } else { .. });`
    // gives a reference the branch declared (#1453): k's value holds it,
    // so k borrows what it borrows, now that the branch has made it.
    let (mut found, is_mut) = if ty.is_ref() {
      let (lender, _, co) = self.get_ref_data_co(value);
      (std::iter::once(lender).chain(co).collect(), ty.ref_mutability().is_some_and(|m| m.is_mut()))
    } else {
      self.holder_lenders(value, ty)
    };
    found.extend(closure_lenders);
    let line = expr_to_line(value, &self.tcx);
    for l in found.into_iter().filter(|l| matches!(l, ResourceTy::Value(_) | ResourceTy::Deref(_))) {
      let aliasing = get_aliasing_data(&l, &self.borrow_map);
      match self.borrow_map.get_mut(name) {
        Some(rd) if rd.all_lenders().contains(&l) => {}
        Some(rd) if matches!(rd.lender, ResourceTy::Value(_) | ResourceTy::Deref(_)) => rd.co_lenders.push(l),
        Some(rd) => {
          rd.lender = l;
          rd.aliasing = aliasing;
          rd.static_lender = None;
        }
        None => {
          // A borrow of a temporary the branch made is found by its `&`
          // (#1357).
          let borrow_span = match &l {
            ResourceTy::Value(t) => self.ext_temp_borrow_span(t),
            _ => None,
          };
          self.borrow_map.insert(name.to_owned(), RefData {
            local: self.raps.local(&name),
            lender: l,
            assigned_at: line,
            lifetime: line,
            ref_mutability: is_mut,
            aliasing,
            static_lender: None,
            borrow_span,
            loan_line: None,
            co_lenders: Vec::new(),
            lender_muts: Vec::new(),
            through: Vec::new(),
          });
        }
      }
    }
    if let Some(rd) = self.borrow_map.get_mut(name) {
      for (lender, mutable) in closure_kinds {
        Self::record_lender_kind(&mut rd.lender_muts, &lender, mutable);
      }
    }
  }

  /// `let (a, b) = if … / match …;` (#1007): `a` and `b` are declared
  /// first (`declare_pat_bindings`), then each branch's value is taken
  /// apart by the pattern, in that branch, as `let (a, b) = value;`
  /// would: `(h, k)` moves h into a and k into b, a branch's own tuple
  /// `t` moves t.0 into a and t.1 into b. Like `match_rhs_arm`, a value
  /// that uses a variable declared in the branch is paired once the
  /// branch has been visited (#994). A nested conditional as the value
  /// pairs each of its own branches.
  pub fn bind_pat_arms(&mut self, pat: &'tcx Pat<'tcx>, init: &'tcx Expr<'tcx>) {
    match init.kind {
      ExprKind::If(_, then, els) => {
        self.bind_pat_arm(pat, then);
        if let Some(e) = els {
          self.bind_pat_arm(pat, e);
        }
      }
      ExprKind::Match(_, arms, _) => {
        for arm in arms.iter() {
          self.bind_pat_arm(pat, arm.body);
        }
      }
      _ => {}
    }
  }

  fn bind_pat_arm(&mut self, pat: &'tcx Pat<'tcx>, body: &'tcx Expr<'tcx>) {
    let Some(value) = branch_value_expr(body) else { return };
    if matches!(value.kind, ExprKind::If(..) | ExprKind::Match(_, _, rustc_hir::MatchSource::Normal)) && !value.span.from_expansion() {
      self.bind_pat_arms(pat, value);
      return;
    }
    // (Also a literal whose parts the pattern leaves in a temporary
    // of the branch, #1011: it is declared in the branch, once visited.)
    if self.defer_arm_binds.is_some_and(|within| self.arm_value_uses_local_in(body, within))
      || pat_leaves_parts(pat) {
      self.deferred_arm_binds.entry(body.hir_id).or_insert_with(|| (body, Vec::new())).1.push(ArmBind::Pat(pat));
      return;
    }
    let lo = *self.unique_id;
    self.pair_pat_with_value(pat, body);
    self.rhs_arm_ids.insert(body.hir_id, (lo, *self.unique_id));
  }

  /// Takes the value of branch body `body` apart with `pat`, whose
  /// variables are already declared (see `bind_pat_arms`). A branch of
  /// type `!` gives no value.
  fn pair_pat_with_value(&mut self, pat: &'tcx Pat<'tcx>, body: &'tcx Expr<'tcx>) {
    let Some(value) = branch_value_expr(body) else { return };
    if self.tcx.typeck(value.hir_id.owner).expr_ty_opt(value).is_none_or(|ty| ty.is_never()) {
      return;
    }
    let prev = std::mem::replace(&mut self.bind_existing, true);
    let owner = match body.kind {
      ExprKind::Block(b, _) => b.hir_id,
      _ => body.hir_id,
    };
    let prev_owner = self.branch_value_owner.replace(owner);
    self.bind_pat(pat, value, false);
    self.bind_existing = prev;
    self.branch_value_owner = prev_owner;
    for (line, tmp, ty) in std::mem::take(&mut self.branch_temp_drops) {
      let id = *self.unique_id;
      *self.unique_id += 1;
      self.add_external_event(line, ExternalEvent::TemporaryDrop { ro: tmp, ty: Some(ty), when: "at the end of this statement".to_owned(), literal: true, early: false, part: None, id });
    }
  }

  /// Pairs the value of branch body `body`, now visited, with `lhs`, a
  /// variable that may hold a value already (`v = if c { f } else { g
  /// };`, #1010), as `v = f;` in the branch would: v's old value, if
  /// it still has it here, is dropped where the new one arrives. A
  /// branch of type `!` gives no value and drops nothing; nor does one
  /// whose value is v's own (`v = if c { v } else { g };`): v keeps its
  /// value on that path.
  fn reassign_from_arm(&mut self, lhs: ResourceTy, body: &'tcx Expr<'tcx>, evt: Evt, old: ResourceAccessPoint, drops: bool) {
    let Some(value) = branch_value_expr(body) else { return };
    if self.tcx.typeck(value.hir_id.owner).expr_ty_opt(value).is_none_or(|ty| ty.is_never()) {
      return;
    }
    if matches!(value.kind, ExprKind::Path(_)) && fetch_rap(value, &self.tcx, &self.raps).is_some_and(|r| r.hash() == old.hash()) {
      return;
    }
    let held = self.rap_ownership_now(old.name());
    let n_events = self.preprocessed_events.len();
    self.match_rhs(lhs, body, evt);
    let is_old = |r: &ResourceTy| r.hash() == old.hash();
    let new = &self.preprocessed_events[n_events..];
    if !drops || held == Ownership::None {
      return;
    }
    let line = new.iter()
      .find(|(_, ev)| matches!(ev,
        ExternalEvent::Move { to, .. } | ExternalEvent::Copy { to, .. } | ExternalEvent::Bind { to, .. } if is_old(to)))
      .map_or_else(|| span_end_line(&value.span, &self.tcx), |(l, _)| *l);
    let maybe_moved = held == Ownership::Maybe;
    let by_cond = maybe_moved && self.moved_by_chain_condition(&old);
    // Same rule as a plain `v = f;` (see visitor.rs): maybe-moved only
    // because the loop can come back round with it moved (#992).
    let loop_head = maybe_moved && self.maybe_moved_by_loop_head_now(old.name());
    self.add_external_event(line, ExternalEvent::OwnerDropAtReassign {
      ro: old,
      id: *self.unique_id,
      maybe_moved,
      by_cond,
      loop_head,
      parts: None,
      maybe_init: None,
      payload: false,
      contents_gone: false,
      skipped: false,
    });
    *self.unique_id += 1;
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
    let matched_block = &self.matched_block_temps;
    let exit_drops = &self.exit_drops;
    // The end of a loan a field write in the branch leaves to last
    // until the struct's last use (#531) is drawn on a line after the
    // branch when that use is: it happens after the branches join, so
    // it stays on the global timeline, not on a branch's column (#659).
    let body_end = body.map(|b| span_end_line(&b.span, &self.tcx));
    let escapes = |l: usize, ev: &ExternalEvent| body_end.is_some_and(|end| l > end)
      && matches!(ev, ExternalEvent::StaticDie { from: ResourceTy::Index(..), .. } | ExternalEvent::MutableDie { from: ResourceTy::Index(..), .. });
    let mut taken = Vec::new();
    self.preprocessed_events.retain(|(l, ev)| {
      // A drop of a temporary in `unbranched_temps` stays on its own,
      // unsplit column. Except a drop at an exit in the block its `if
      // let` matched, in a branch of a conditional there (or of the `if
      // let` itself): the exit's leg ends in it (#1360).
      let in_matched_block = matches!(ev, ExternalEvent::TemporaryDrop { ro, id, .. } if matched_block.contains(ro) && exit_drops.contains(id));
      if matches!(ev, ExternalEvent::TemporaryDrop { ro, .. } if unbranched.contains(ro)) && !in_matched_block {
        return true;
      }
      if mine(ev.get_id()) && !escapes(*l, ev) {
        let mut ev = ev.clone();
        if let ExternalEvent::TemporaryDrop { early, .. } = &mut ev {
          if in_matched_block {
            *early = false;
          }
        }
        taken.push((*l, ev));
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

  /// Whether `line` is in an arm of an enclosing `if` / `match` other than
  /// the one being visited: a use of a reference there is on a path the
  /// current arm is not on.
  pub fn in_sibling_arm(&self, line: usize) -> bool {
    self.arm_frames.iter().any(|&(split, merge, lo, hi)| split < line && line < merge && !(lo <= line && line <= hi))
  }

  /// The column name of a deferred local (a key of `uninit_locals` /
  /// `maybe_init_locals`): a `let x;` binding's (numbered) name, or a
  /// branch temporary's `(temporary N)` (#1397).
  pub fn deferred_name(&self, id: rustc_hir::HirId) -> String {
    self.branch_temp_names.get(&id).cloned().unwrap_or_else(|| binding_name(&self.tcx, id))
  }

  /// `deferred_name`, with a `let x;` binding named by its identifier.
  pub fn deferred_ident(&self, id: rustc_hir::HirId) -> String {
    self.branch_temp_names.get(&id).cloned().unwrap_or_else(|| self.tcx.hir_name(id).to_string())
  }

  /// Called when an `if` / `match` begins: the loans its arms start from.
  pub fn arm_borrows_begin(&mut self) -> ArmBorrows {
    let outer_value_binds = std::mem::take(&mut self.arm_value_binds);
    // A `let r;` local may be registered when it's first assigned:
    // name it from `uninit_locals` / `maybe_init_locals` too.
    #[expect(clippy::disallowed_methods, reason = "collected into a set; order is lost")]
    let unlent: HashSet<String> = self.raps.iter()
      .filter(|(_, rd)| (*rd.rap.hash() as usize) < self.rap_hashes)
      .map(|(n, _)| n.clone())
      .chain(self.uninit_locals.keys().chain(self.maybe_init_locals.iter()).map(|h| self.deferred_ident(*h)))
      .filter(|n| !self.borrow_map.contains_key(n))
      .collect();
    ArmBorrows { before: self.borrow_map.clone(), unlent, done: Vec::new(), outer_value_binds, last_use: std::collections::BTreeMap::new() }
  }

  /// Called before each arm but the first is visited. The arms are
  /// alternatives: `c = &y;` in one arm doesn't change the loan `c` holds
  /// when another starts (still `x`, #806), so that arm's `c = &z;` ends
  /// `x`'s loan, not `y`'s. Every reference starts the arm with the loan
  /// it held before the conditional.
  pub fn arm_borrows_next(&mut self, ab: &mut ArmBorrows) {
    #[expect(clippy::disallowed_methods, reason = "sorted below")]
    let mut keys: Vec<&String> = ab.before.keys().collect();
    keys.sort();
    for k in keys {
      let old = &ab.before[k];
      if let Some(cur) = self.borrow_map.get(k) {
        if same_loan(cur, old) {
          // (Its last use seen in the source: a later arm's replaces an
          // earlier one's, as when the arms shared one `borrow_map`.)
          if cur.lifetime != old.lifetime {
            ab.last_use.insert(k.clone(), cur.lifetime);
          }
        } else {
          ab.done.push((k.clone(), cur.clone()));
        }
      }
      self.borrow_map.insert(k.clone(), old.clone());
    }
    // One that held no loan before starts the next arm holding none.
    #[expect(clippy::disallowed_methods, reason = "unlent_keys sorts what it returns")]
    let unlent = self.unlent_keys(ab, self.borrow_map.keys());
    for k in unlent {
      if let Some(cur) = self.borrow_map.remove(&k) {
        ab.done.push((k, cur));
      }
    }
  }

  /// `ab.unlent`, and the reference fields among `names` of a struct in
  /// it (`d.a` of `let d: One;`, which its first assignment registers:
  /// `if c { d = One { a: &s1 }; } else { d = One { a: &s2 }; }`, #1402),
  /// sorted.
  fn unlent_keys<'n>(&self, ab: &ArmBorrows, names: impl Iterator<Item = &'n String>) -> Vec<String> {
    #[expect(clippy::disallowed_methods, reason = "sorted below")]
    let mut keys: Vec<String> = ab.unlent.iter().cloned()
      .chain(names.filter(|n| !ab.before.contains_key(*n)
        && n.split_once('.').is_some_and(|(root, _)| ab.unlent.contains(root))
        && self.raps.get(*n).is_some_and(|rd| rd.rap.is_member() && rd.rap.is_ref())).cloned())
      .collect();
    keys.sort();
    keys.dedup();
    keys
  }

  /// Called after the last arm. A reference that only an earlier arm
  /// re-pointed keeps that arm's loan after the conditional (the latest
  /// in source order), as it did before arms started from the same
  /// loans; the other arms' loans are ended by `emit_borrow_ends`
  /// (`arm_loans`). A loan held from before keeps the last use seen in
  /// source order.
  pub fn arm_borrows_end(&mut self, mut ab: ArmBorrows) {
    let mut done = std::mem::take(&mut ab.done);
    // A reference (or holder) the branches give their values to (`let y
    // = match n { 0 => { let r = &s1; r } _ => { let r = &s2; r } };`,
    // #1453) may hold any branch's loan after the conditional: it borrows
    // what each arm gave it, and none of those loans ends at the join.
    #[expect(clippy::disallowed_methods, reason = "sorted on the next line")]
    let mut bound: Vec<String> = self.arm_value_binds.iter()
      .filter(|k| ab.before.contains_key(*k) || ab.unlent.contains(*k))
      .cloned().collect();
    bound.sort();
    self.arm_value_binds = std::mem::take(&mut ab.outer_value_binds);
    for k in bound {
      let arms: Vec<RefData> = done.iter().filter(|(n, _)| *n == k).map(|(_, d)| d.clone()).collect();
      let Some(mut merged) = self.borrow_map.get(&k).cloned().or_else(|| arms.last().cloned()) else { continue };
      for d in arms.iter().chain(ab.before.get(&k)) {
        for l in d.all_lenders() {
          if !matches!(l, ResourceTy::Value(_) | ResourceTy::Deref(_)) || merged.all_lenders().contains(&l) {
            continue;
          }
          if matches!(merged.lender, ResourceTy::Value(_) | ResourceTy::Deref(_)) {
            merged.co_lenders.push(l);
          } else {
            merged.lender = l;
            merged.static_lender = None;
          }
        }
        merged.lifetime = merged.lifetime.max(d.lifetime);
      }
      done.retain(|(n, _)| *n != k);
      self.borrow_map.insert(k, merged);
    }
    let mut keep: Vec<usize> = Vec::new();
    #[expect(clippy::disallowed_methods, clippy::iter_over_hash_type, reason = "`keep` is sorted before use; the inserts are independent")]
    for (k, old) in ab.before.iter() {
      let Some(cur) = self.borrow_map.get(k) else { continue };
      if !same_loan(cur, old) {
        continue;
      }
      if let Some(i) = done.iter().rposition(|(n, _)| n == k) {
        keep.push(i);
        self.borrow_map.insert(k.clone(), done[i].1.clone());
      } else if let Some(&l) = ab.last_use.get(k) {
        if let Some(cur) = self.borrow_map.get_mut(k).filter(|c| c.lifetime == old.lifetime) {
          cur.lifetime = l;
        }
      }
    }
    // One that held no loan before and that the last arm left without
    // one keeps the latest earlier arm's (#1069).
    for k in self.unlent_keys(&ab, done.iter().map(|(n, _)| n)) {
      if self.borrow_map.contains_key(&k) {
        continue;
      }
      if let Some(i) = done.iter().rposition(|(n, _)| *n == k) {
        keep.push(i);
        self.borrow_map.insert(k, done[i].1.clone());
      }
    }
    keep.sort_unstable();
    for i in keep.into_iter().rev() {
      done.remove(i);
    }
    self.arm_loans.extend(done);
  }

  /// The end of reference `from`'s borrow (StaticDie / MutableDie),
  /// returning to `to`; `ref_data` is `from`'s loan, and names the
  /// 'static data it borrowed when the lender has no column.
  ///
  /// `still_borrowed_by`: the values that go on holding the lender's
  /// borrow after this reference's ends (see `emit_borrow_ends`, #575).
  pub fn add_borrow_end_ev(&mut self, line_num: usize, to: ResourceTy, from: ResourceTy, ref_data: &RefData, still_borrowed_by: Vec<String>) {
    let id = *self.unique_id;
    *self.unique_id += 1;
    let static_lender = match ref_data.lender {
      ResourceTy::Anonymous if from.extract_rap().is_some_and(|r| self.tuple_temp_refs.contains(r.name())) =>
        Some(StaticLender::TupleTemporary),
      ResourceTy::Anonymous => ref_data.static_lender.clone(),
      _ => None,
    };
    let returned_pattern_loan = from.is_ref() && to.is_ref() && self.body_facts.is_some_and(|body|
      body.rows.iter().filter(|r| r.line == line_num).flat_map(|r| &r.facts).any(|ev|
        matches!(&ev.fact, crate::facts::Fact::BorrowEnd { loan, holder, lender, .. }
          if holder == &from.real_name() && lender == &to.real_name()
            && self.pattern_loans.values().any(|p| p.loan == *loan))));
    let of_ref_var = returned_pattern_loan || (to == ref_data.lender
      && ref_data.borrow_span.is_some_and(|sp| self.ref_var_borrows.contains(&sp)))
      || matches!((&from, &to), (ResourceTy::Value(h), ResourceTy::Value(l))
        if self.stored_ref_var_lends.contains(&(h.name().to_owned(), l.name().to_owned())));
    let ev = if ref_data.mut_of(&to) {
      ExternalEvent::MutableDie { slot: id, from, to, id, static_lender, still_borrowed_by, of_ref_var, exhaustion: Vec::new() }
    } else {
      ExternalEvent::StaticDie { slot: id, from, to, id, static_lender, still_borrowed_by, of_ref_var, exhaustion: Vec::new() }
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

  /// `add_borrow_ev`, also tagged with whether the borrow is of a
  /// reference variable itself (`&mut r` / `&r`, #914, #925).
  pub fn add_borrow_ev_of_ref_var(&mut self, line_num: usize, evt: Evt, lhs: ResourceTy, rhs: ResourceTy, is_reborrow: bool, of_ref_var: bool) {
    let mut ev = self.ext_ev_of_evt(evt, lhs, rhs, *self.unique_id, false);
    if let ExternalEvent::StaticBorrow { is_reborrow: r, of_ref_var: o, .. }
         | ExternalEvent::MutableBorrow { is_reborrow: r, of_ref_var: o, .. } = &mut ev {
      *r = is_reborrow;
      *o = of_ref_var;
    }
    self.add_external_event(line_num, ev);
    *self.unique_id += 1;
  }

  /// `add_ev` for a `PassByMRef`, tagged with whether the call
  /// reborrows the `&mut` argument, whether the call's result
  /// can carry that reborrow past the call, and whether Rust
  /// inserted the `&mut` (see `ImplicitRef`), and whether `arg` is a
  /// reference variable borrowed itself (`of_ref_var`, #938).
  pub fn add_pass_by_mref_ev(&mut self, line_num: usize, func: ResourceTy, arg: ResourceTy, is_reborrow: bool, ends_at_call: bool, implicit: ImplicitRef, of_ref_var: bool) {
    self.add_pass_by_mref_ev_deep(line_num, func, arg, is_reborrow, ends_at_call, implicit, of_ref_var, None);
  }

  /// `add_pass_by_mref_ev` for a receiver that is a reference to a
  /// reference, the call borrowing what's further away (`deep`, #952).
  #[allow(clippy::too_many_arguments)]
  pub fn add_pass_by_mref_ev_deep(&mut self, line_num: usize, func: ResourceTy, arg: ResourceTy, is_reborrow: bool, ends_at_call: bool, implicit: ImplicitRef, of_ref_var: bool, deep: Option<DeepPlace>) {
    let id = *self.unique_id;
    let terminal = self.emitting_expr.and_then(|e| crate::facts::terminal_argument(self.tcx, self.mir_body, e.span));
    let mut stored_in = StoredIn::default();
    if is_reborrow && !ends_at_call && matches!(implicit, ImplicitRef::Receiver { .. }) {
      if let Some(stores) = self.emitting_expr.and_then(|call| self.body_facts?.argument_stores.get(&(call.span.with_parent(None), 0))) {
        if let [(holder, _)] = stores.as_slice() {
          stored_in.0 = Some(holder.clone());
        }
      }
    }
    self.add_external_event(line_num, ExternalEvent::PassByMutableReference {
      manually_takes: false, terminal, from: arg, to: func, id, is_reborrow, ends_at_call, implicit, result_keeps_borrow: false, stored_in, break_alt: None, of_ref_var, deep,
    });
    *self.unique_id += 1;
  }

  /// `add_ev` for a `PassBySRef` with an explicit `ImplicitRef`;
  /// `of_ref_var` as for `add_pass_by_mref_ev`.
  pub fn add_pass_by_sref_ev(&mut self, line_num: usize, func: ResourceTy, arg: ResourceTy, implicit: ImplicitRef, refcell_borrow_mut: bool, of_ref_var: bool) {
    self.add_pass_by_sref_ev_deep(line_num, func, arg, implicit, refcell_borrow_mut, of_ref_var, None);
  }

  /// `add_pass_by_sref_ev` with `deep` as for `add_pass_by_mref_ev_deep`.
  pub fn add_pass_by_sref_ev_deep(&mut self, line_num: usize, func: ResourceTy, arg: ResourceTy, implicit: ImplicitRef, refcell_borrow_mut: bool, of_ref_var: bool, deep: Option<DeepPlace>) {
    self.add_pass_by_sref_ev_full(line_num, func, arg, implicit, refcell_borrow_mut, false, of_ref_var, deep);
  }

  /// `add_pass_by_sref_ev_deep`, also saying whether the argument is a
  /// shared reference to a value with interior mutability (#1149).
  pub fn add_pass_by_sref_ev_full(&mut self, line_num: usize, func: ResourceTy, arg: ResourceTy, implicit: ImplicitRef, refcell_borrow_mut: bool, pointee_interior_mut: bool, of_ref_var: bool, deep: Option<DeepPlace>) {
    let id = *self.unique_id;
    self.add_external_event(line_num, ExternalEvent::PassByStaticReference {
      from: arg, to: func, id, implicit, refcell_borrow_mut, pointee_interior_mut, result_keeps_borrow: false, stored_in: StoredIn::default(), break_alt: None, still_borrowed_by: Vec::new(), of_ref_var, deep,
    });
    *self.unique_id += 1;
  }

  pub(crate) fn operand_label(&self, call: &Expr<'tcx>, index: usize, operand: &Expr<'tcx>) -> Option<String> {
    crate::facts::call_wording::operand_label(self.tcx, self.mir_body, &self.raps.bindings,
      call.span, index, operand.span, self.tcx.typeck(operand.hir_id.owner).expr_ty(operand))
  }

  /// A built-in dereference chain evaluated by this expression. Use the
  /// borrowck place so a temporary introduced by lowering never becomes a
  /// displayed name. Several events at the same expression must agree.
  pub(crate) fn projected_access(&self, span: rustc_span::Span, rap: &ResourceAccessPoint,
    select: impl Fn(&crate::facts::Fact) -> Option<&str>) -> Option<String> {
    let mut paths = self.body_facts?.rows.iter().flat_map(|r| &r.facts).filter_map(|e| {
      let place = select(&e.fact)?;
      let base = place.trim_start_matches('*');
      (base == rap.name() && self.bwf.body.source_info(e.location).span.source_equal(span)).then_some(place)
    });
    let first = paths.next()?;
    (first.len() - rap.name().len() >= 2 && paths.all(|p| p == first)).then(|| first.to_owned())
  }

  /// `DeepPlace` for a method call's receiver or a call's argument
  /// `e`, if it's a local reference variable r (or `*r`) to a
  /// reference that rustc derefs through twice or more before
  /// borrowing: `r.len()` / `show_str(r)` with `r: &&String` borrow
  /// `**r`, not `*r` (#952).
  ///
  /// Also a local `Box` holding a reference that rustc derefs through
  /// (`k.push('!')` with `k: Box<&mut String>` borrows `**k`, #1282),
  /// and an element of a local array holding one (`k[0].push('!')` with
  /// `k: [&mut String; 1]` borrows `*k[0]`, #1319).
  pub fn deep_place(&self, e: &'tcx Expr<'tcx>) -> Option<DeepPlace> {
    let typeck = self.tcx.typeck(e.hir_id.owner);
    let owner = e.hir_id.owner;
    let holds_value = |place_ty: Ty<'tcx>| matches!(place_ty.kind(), TyKind::Str)
      || self.ty_is_copy(place_ty, owner) || self.ty_is_plain(place_ty, owner);
    if let Some((derefs, place_ty, held_mut)) = boxed_ref_deref_depth(e, typeck) {
      return Some(DeepPlace { derefs: derefs - 1, holds_value: holds_value(place_ty), held_mut: Some(held_mut), elem: None });
    }
    if let Some((elem, derefs, place_ty, held_mut)) = array_elem_ref_deref_depth(e, typeck) {
      let text = self.tcx.sess.source_map().span_to_snippet(elem.span).ok()?;
      return Some(DeepPlace { derefs, holds_value: holds_value(place_ty), held_mut: Some(held_mut), elem: Some(text) });
    }
    let (derefs, place_ty) = borrowed_deref_depth(e, typeck)?;
    if derefs < 2 {
      return None;
    }
    Some(DeepPlace { derefs: derefs - 1, holds_value: holds_value(place_ty), held_mut: None, elem: None })
  }

  /// Reference columns whose hold ends on the edge that bypasses the body.
  pub(crate) fn skipped_holds(&self, branch: rustc_hir::HirId) -> BTreeMap<u64, SkippedHold> {
    use crate::facts::{Fact, SourceArm};
    use rustc_middle::mir::BasicBlock;
    let mut holds = BTreeMap::<u64, SkippedHold>::new();
    let Some(body) = self.body_facts else { return holds };
    let mir = self.mir_body;
    let doms = mir.basic_blocks.dominators();
    let reachable = |start: BasicBlock, test: BasicBlock| {
      let mut seen = BTreeSet::new();
      let mut work = vec![start];
      while let Some(b) = work.pop() {
        // A back edge can reach the same test with a fresh value.
        if b != test && seen.insert(b) {
          work.extend(crate::facts::real_successor_blocks(mir, b).into_iter().filter(|&next| !doms.dominates(next, b)));
        }
      }
      seen
    };
    let facts = || body.rows.iter().flat_map(|row| &row.facts);
    let skip_edge = |ev: &crate::facts::Ev| {
      let edge = body.entered(ev.location, ev.occurrence.successor)?;
      if edge != (SourceArm { branch, index: 1 }) { return None; }
      let skip = ev.occurrence.successor?.block;
      let mut others = body.switches.iter().filter(|e| e.edge.switch == ev.location && e.edge.successor != skip);
      let taken = others.next()?.edge.successor;
      others.next().is_none().then_some((ev.location.block, skip, taken))
    };
    for ev in facts() {
      let Fact::HoldEnd { holder, loan, .. } = &ev.fact else { continue };
      let Some(rap) = self.raps.get(holder).map(|r| &r.rap).filter(|r| r.is_member() && r.is_ref()) else { continue };
      if skip_edge(ev).is_none() || holds.get(rap.hash()).is_some_and(|h| h.loans.contains_key(loan)) { continue; }
      let mut moves = BTreeSet::new();
      let mut bypasses = Vec::new();
      for row in &body.rows {
        for e in &row.facts {
          if !matches!(&e.fact, Fact::HoldEnd { holder: h, loan: l, .. } if h == holder && l == loan) { continue; }
          if crate::facts::location_arms(self.tcx, mir, body, e.location).contains(&SourceArm { branch, index: 0 })
            && row.facts.iter().any(|m| m.location == e.location && matches!(&m.fact, Fact::Move { path, .. }
              if holder.strip_prefix(path.as_str()).is_some_and(|suffix| suffix.starts_with('.')))) {
            moves.insert(e.location.block);
          } else {
            bypasses.push(e);
          }
        }
      }
      let loan_ends = || facts().filter_map(|e| match &e.fact {
          Fact::BorrowEnd { loan: l, holder, lender, why, .. } if l == loan => Some((e.location.block, holder, lender, why)),
          _ => None,
      });
      if moves.is_empty() || loan_ends().any(|(_, h, _, _)| h.split('|').any(|part| part == holder)) { continue; }
      // A short-circuit condition can have several edges into the same
      // implicit else. All must bypass the moves and reach the join.
      let explanations = bypasses.into_iter().map(|end| {
        let (test, skip, taken) = skip_edge(end)?;
        let skipped = reachable(skip, test);
        let taken = reachable(taken, test);
        if !moves.iter().all(|&m| !skipped.contains(&m) && taken.contains(&m) && !reachable(m, test).is_disjoint(&skipped)) {
          return None;
        }
        // Name the continuing restriction only when the ends reachable
        // from the skipped edge identify one lender and last holder.
        let mut ends = loan_ends().filter(|(block, ..)| skipped.contains(block)).map(|(_, h, l, why)| (h, l, why));
        let until = ends.next().filter(|&(h, l, why)| why == "no further use"
          && ends.all(|other| other == (h, l, why))
          && body.columns.iter().any(|c| c.name == *h)
          && body.columns.iter().any(|c| c.name == *l));
        Some(until.map(|(h, l, _)| (l, h)))
      }).collect::<Option<Vec<_>>>();
      if let Some(explanations) = explanations {
        let mut explanations = explanations.into_iter();
        let until = explanations.next().flatten().filter(|&pair| explanations.all(|other| other == Some(pair)))
          .map(|(l, h)| (l.clone(), h.clone()));
        holds.entry(*rap.hash()).or_default().loans.insert(*loan, until);
      }
    }
    holds
  }

  /// An actual read of a named MIR place at this expression. Effects sharing
  /// an expansion span are ambiguous unless exactly one transfer matches.
  pub(crate) fn transfer_at(&self, expr: &Expr<'_>) -> Option<Transfer<'a>> {
    use crate::facts::Fact;
    let mut found = self.effects.get(&expr.span.with_parent(None))?.iter().filter_map(|&(row, ev)| match &ev.fact {
      Fact::Move { path, to, .. } => Some(Transfer { row, kind: Evt::Move, path, to, location: ev.location }),
      Fact::Copy { path, to, .. } => Some(Transfer { row, kind: Evt::Copy, path, to, location: ev.location }),
      _ => None,
    });
    let one = found.next()?;
    found.next().is_none().then_some(one)
  }

  /// The unique MIR borrow issued at this operand, and its destination.
  pub(crate) fn operand_loan(&self, expr: &Expr<'_>) -> Option<(&'a LoanInfo, &'a str)> {
    let mut borrows = self.effects.get(&expr.span.with_parent(None))?.iter().filter_map(|&(_, ev)| match &ev.fact {
      crate::facts::Fact::Borrow { loan, holder, .. } => Some((*loan, holder.as_str())),
      _ => None,
    });
    let (id, holder) = borrows.next()?;
    if borrows.next().is_some() { return None; }
    Some((self.body_facts?.loans.iter().find(|l| l.loan == id)?, holder))
  }

  /// The new loan of *r, distinct from the older loan r itself holds.
  pub(crate) fn mut_ref_reborrow_at(&self, expr: &Expr<'_>) -> Option<Reborrow<'a>> {
    let (loan, holder) = self.operand_loan(expr)?;
    if !matches!(loan.kind, "shared" | "mut") { return None; }
    let through = &self.raps.get(loan.through.as_ref()?)?.rap;
    through.is_mutref().then(|| Reborrow { loan, through: through.clone(), holder })
  }

  /// The receiver of call_once is evaluated before its arguments. Its move
  /// or copy is the MIR operand's. For a local closure body we can also
  /// explain what the call does with each capture (#271, #725, #1704).
  pub fn closure_call_events(&mut self, call: &Expr<'_>, callee: &'tcx Expr<'tcx>) {
    let typeck = self.tcx.typeck(call.hir_id.owner);
    if let Some(result) = self.mir_call(call).filter(|c| c.callable_result
        && c.has_result && !c.returns_unit
        && c.callee.and_then(|d| self.tcx.trait_of_assoc(d)) == self.tcx.lang_items().fn_mut_trait()) {
      let slots = CallSlots { span: Some(call.span), callee: result.callee, ..CallSlots::default() };
      if let Some(read) = self.body_facts.and_then(|b| b.value_flow.argument_read(callee.span, slots)) {
        if let OperandEffect::Borrow { source, mutable: true, .. } = &read.effect {
          if let Some(from) = self.transfer_source(source, callee) {
            if let Some(ResourceAccessPoint::Function(mut to)) = hirid_to_var_name(callee.hir_id, &self.tcx)
                .and_then(|name| self.raps.get(&fn_key(&name))).map(|to| to.rap.clone()) {
              to.mutable_call_receiver = true;
              self.add_read_transfer(&read, Evt::PassByMRef, ResourceTy::Value(ResourceAccessPoint::Function(to)), from);
            }
          }
        }
      }
      return;
    }
    if !typeck.type_dependent_def_id(call.hir_id)
      .is_some_and(|d| self.tcx.trait_of_assoc(d) == self.tcx.lang_items().fn_once_trait()) {
      return;
    }
    if let ExprKind::Closure(literal) = callee.kind {
      // The aggregate initializes the callable. Its operands are captures,
      // each with its own MIR transfer, even if they share a source span.
      let captures = self.effects.get(&callee.span.with_parent(None)).cloned().unwrap_or_default();
      for (row, effect) in captures {
        let loc = effect.location;
        let Some(st) = self.mir_body.basic_blocks[loc.block].statements.get(loc.statement_index) else { continue };
        if !matches!(&st.kind, StatementKind::Assign(a) if matches!(&a.1, Rvalue::Aggregate(k, _)
          if matches!(&**k, rustc_middle::mir::AggregateKind::Closure(d, _) if *d == literal.def_id.to_def_id()))) { continue; }
        let (path, kind) = match &effect.fact {
          crate::facts::Fact::Move { path, .. } => (path, Evt::Move),
          crate::facts::Fact::Copy { path, .. } => (path, Evt::Copy),
          _ => continue,
        };
        let Some(src) = self.raps.get(path).map(|r| ResourceTy::Value(r.rap.clone())) else { continue };
        let mut event = self.ext_ev_of_evt(kind, ResourceTy::Anonymous, src, *self.unique_id, false);
        if let ExternalEvent::Move { into_closure, .. } | ExternalEvent::Copy { into_closure, .. } = &mut event {
          *into_closure = true;
        }
        self.add_external_event(row, event);
        *self.unique_id += 1;
      }
      return;
    }
    let Some(Transfer { row: line_num, kind, path, location, .. }) = self.transfer_at(callee) else { return };
    // A closure aggregate can have the same span as the capture it moves.
    // Only a use of a place of the callable type stages the receiver.
    use rustc_middle::mir::{Rvalue, StatementKind, TerminatorKind};
    let body = self.mir_body;
    let block = &body.basic_blocks[location.block];
    let Some(StatementKind::Assign(assignment)) = block.statements.get(location.statement_index).map(|s| &s.kind) else { return };
    let Rvalue::Use(operand) = &assignment.1 else { return };
    let erase = |ty| self.tcx.erase_and_anonymize_regions(ty);
    if erase(operand.ty(&body.local_decls, self.tcx)) != erase(typeck.expr_ty(callee)) { return; }
    let invocation = body.basic_blocks.iter_enumerated().find_map(|(bb, b)| {
      let TerminatorKind::Call { func, args, .. } = &b.terminator().kind else { return None };
      let (method, _) = func.const_fn_def()?;
      (self.tcx.trait_of_assoc(method) == self.tcx.lang_items().fn_once_trait()
        && args.first()?.node.place() == Some(assignment.0)).then_some(bb)
    });
    let staged = body.source_info(location).span.with_parent(None);
    let skipped = self.body_facts.and_then(|facts| facts.effect_sites.get(&staged))
      .is_some_and(|sites| sites.contains(&(location.block, false)));
    let timing = match invocation {
      _ if skipped => CallTiming::Skipped,
      None => CallTiming::Never,
      Some(bb) if bb != location.block => CallTiming::AfterArguments,
      Some(_) => CallTiming::Direct,
    };
    let Some(closure_rap) = self.raps.get(path).map(|r| r.rap.clone()) else { return };
    if matches!(kind, Evt::Copy) {
      self.add_ev(line_num, Evt::Copy, ResourceTy::Anonymous, ResourceTy::Value(closure_rap), false);
      return;
    }
    let fates = match typeck.expr_ty(callee).kind() {
      rustc_middle::ty::Closure(did, _) if self.tcx.typeck_root_def_id(*did) == callee.hir_id.owner.to_def_id() =>
        did.as_local().map(|local| self.call_capture_fates(callee, local)),
      _ => None,
    };
    let id = *self.unique_id;
    *self.unique_id += 1;
    self.add_external_event(line_num, ExternalEvent::ClosureCall { closure: ResourceTy::Value(closure_rap), fates, timing, id });
  }

  fn call_capture_fates(&self, callee: &Expr<'_>, closure_local: rustc_hir::def_id::LocalDefId) -> Vec<CapturedFate> {
    // What the body does with each capture (all captures, to keep the
    // field index of each), then the by-value, non-Copy ones named.
    let closure_typeck = self.tcx.typeck(closure_local);
    let captures: Vec<_> = closure_typeck.closure_min_captures_flattened(closure_local).collect();
    let bwf = rustc_utils::mir::borrowck_facts::get_body_with_borrowck_facts(self.tcx, closure_local);
    let ends = closure_capture_ends(self.tcx, &bwf.body, captures.len());
    let env = typing_env(self.tcx, callee.hir_id.owner);
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
        needs_drop: self.needs_drop(capture.place.ty(), callee.hir_id.owner),
      });
    }
    fates
  }

  pub(crate) fn note_reference_slot_write(&mut self, span: rustc_span::Span, lhs: &ResourceTy) {
    let ResourceTy::DerefField(_, place, ..) = lhs else { return };
    let Some(effects) = self.effects.get(&span.with_parent(None)) else { return };
    let mut writes = effects.iter().filter(|(_, e)| matches!(&e.fact, crate::facts::Fact::Init { path, .. } if path == place));
    let Some((_, write)) = writes.next().filter(|_| writes.next().is_none()) else { return };
    if let Some(names) = self.body_facts.and_then(|f| f.reference_slot_write(self.mir_body, write.location)) {
      self.row_layout.retained_references.insert(*self.unique_id, names);
    }
  }

  fn nested_reference_place(&self, expr: &Expr<'_>, write: bool) -> Option<ResourceTy> {
    use crate::facts::Fact;
    // Box payloads and ordinary field writes have their own drop events.
    // This path represents a projection through references only.
    let span = expr.span.with_parent(None);
    // rustc gives compound assignment's read and write the whole operator span.
    let parent = self.tcx.parent_hir_node(expr.hir_id);
    let effect_span = match parent {
      rustc_hir::Node::Expr(e) if matches!(e.kind, ExprKind::AssignOp(_, lhs, _) if lhs.hir_id == expr.hir_id) => e.span.with_parent(None),
      _ => span,
    };
    use rustc_middle::mir::{Operand, ProjectionElem, Rvalue, StatementKind};
    let mut places = self.effects.get(&effect_span)?.iter().filter_map(|(_, e)| {
      let loc = e.location;
      let st = self.mir_body.basic_blocks[loc.block].statements.get(loc.statement_index)?;
      let StatementKind::Assign(a) = &st.kind else { return None };
      match (&e.fact, &a.1) {
        (Fact::Init { path, .. }, _) if write => Some((a.0, path)),
        (Fact::Copy { path, .. }, Rvalue::Use(Operand::Copy(p))) if !write => Some((*p, path)),
        _ => None,
      }
    }).filter(|(p, _)| p.projection.len() >= 2 && p.projection.iter().all(|e| e == ProjectionElem::Deref));
    let (place, path) = places.next()?;
    if places.any(|(other, _)| other != place) { return None; }
    let var = self.body_facts?.vars.iter().find(|v| v.local == place.local.index())?;
    let r = self.raps.get(&var.name)?.rap.clone();
    let mut ty = self.mir_body.local_decls[place.local].ty;
    for _ in place.projection {
      let TyKind::Ref(_, inner, _) = ty.kind() else { return None };
      ty = *inner;
    }
    Some(ResourceTy::DerefField(r, path.clone(), PlaceWrite::Update, self.field_holds(ty, expr.hir_id.owner)))
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
        if let Some(place) = self.nested_reference_place(expr, true) {
          return Some(place);
        }
        let rhs_rap = fetch_rap(&expr, &self.tcx, &self.raps);
        match rhs_rap {
          Some(x) => {
            self.update_rap(&x, line_num);
            Some(ResourceTy::Deref(x))
          }
          // `*v.get_mut(0).unwrap() += 1`: a write into v's resource
          // (#861). Otherwise a deref of a temporary with no column.
          None => Some(self.deref_of_mut_call(expr).unwrap_or(ResourceTy::Anonymous)),
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
        let typing_env = typing_env(self.tcx, expr.hir_id.owner);
        let write = if self.tcx.type_is_copy_modulo_regions(typing_env, elem_ty) {
          PlaceWrite::AssignCopy
        } else if self.needs_drop(elem_ty, expr.hir_id.owner) {
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
    let env = typing_env(self.tcx, arg.hir_id.owner);
    let mut drew_any = false;
    for (index, capture) in typeck.closure_min_captures_flattened(closure.def_id).enumerate() {
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
          let Some(sites) = crate::facts::call_wording::capture_borrows(self.mir_body, closure.def_id, index) else { continue };
          let Some(&(_, place)) = sites.first() else { continue };
          // One source dot can describe several guard occurrences when the
          // captured place and the displayed end row agree on every path.
          if !sites.iter().all(|&(issue, p)| p == place && self.facts.and_then(|b| b.loans.iter().find(|l| l.issue == Some(issue)))
            .is_some_and(|l| !l.end_lines.is_empty() && l.end_lines.iter().all(|row| *row <= last_line))) { continue; }
          let target_name = format!("the closure passed to {}", fn_name);
          if !self.raps.contains_key(&fn_key(&target_name)) {
            self.add_fn(target_name.clone());
          }
          let target = ResourceTy::Value(self.raps.get(&fn_key(&target_name)).unwrap().rap.to_owned());
          self.update_rap(&from_rap, line_num);
          // The reference variable itself, not what it points to (#1180).
          let of_ref_var = from_rap.is_ref() && capture.place.projections.is_empty();
          let implicit = ImplicitRef::Capture {
            place: crate::facts::call_wording::binding_place(self.mir_body, &self.raps.bindings, place)
              .unwrap_or_else(|| capture.to_string(self.tcx)),
            unique: matches!(kind, rustc_middle::ty::BorrowKind::UniqueImmutable),
            interior_mut: self.ty_has_interior_mut(capture.place.ty(), arg.hir_id.owner) == InteriorMut::Yes,
          };
          match kind {
            rustc_middle::ty::BorrowKind::Mutable =>
              self.add_pass_by_mref_ev(line_num, target, ResourceTy::Value(from_rap), false, false, implicit, of_ref_var),
            _ => self.add_pass_by_sref_ev(line_num, target, ResourceTy::Value(from_rap), implicit, false, of_ref_var),
          }
          drew_any = true;
        }
      }
    }
    drew_any
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
    let env = typing_env(self.tcx, expr.hir_id.owner);
    let write = if self.tcx.type_is_copy_modulo_regions(env, ty) {
      PlaceWrite::AssignCopy
    } else if self.needs_drop(ty, expr.hir_id.owner) {
      PlaceWrite::AssignDropsOld
    } else {
      PlaceWrite::AssignOwned
    };
    Some(ResourceTy::Index(rap, place, write))
  }

  /// `*v.get_mut(0).unwrap() += 1`, `*m.get_mut(&k).unwrap() = 5`,
  /// `*map.entry(k).or_insert(0) += 1`: a write through the `&mut` a
  /// method chain on a variable returns, into part of the resource the
  /// variable owns (or points to). Drawn on the variable's column like
  /// an element write `v[0] += 1` (#861): "v's resource is modified
  /// through v.get_mut(0).unwrap()" for `op=`, and for `=` a plain
  /// assignment to the place `*v.get_mut(0).unwrap()` (dropping the old
  /// value, if it needs drop). Only when the chain's first call takes
  /// the variable by `&mut`: one taking `&self` (`c.borrow_mut()` on a
  /// RefCell, `m.lock()`) writes through interior mutability, not
  /// through a mutable borrow of the variable.
  fn deref_of_mut_call(&mut self, expr: &'tcx Expr<'tcx>) -> Option<ResourceTy> {
    let mut inner = expr;
    let mut derefs = 0;
    while let ExprKind::Unary(UnOp::Deref, e) = inner.kind {
      inner = e;
      derefs += 1;
    }
    let ExprKind::MethodCall(..) = inner.kind else { return None };
    let mut first_call = inner;
    let mut cur = inner;
    loop {
      match cur.kind {
        ExprKind::MethodCall(_, rcvr, _, _) => {
          first_call = cur;
          cur = rcvr;
        }
        ExprKind::Field(base, _) | ExprKind::Index(base, _, _) | ExprKind::Unary(UnOp::Deref, base) => cur = base,
        _ => break,
      }
    }
    let typeck = self.tcx.typeck(expr.hir_id.owner);
    let def_id = typeck.type_dependent_def_id(first_call.hir_id)?;
    let sig = self.tcx.fn_sig(def_id).instantiate_identity().skip_norm_wip();
    let self_ty = *sig.skip_binder().inputs().first()?;
    if self_ty.ref_mutability() != Some(Mutability::Mut) {
      return None;
    }
    let ResourceTy::Index(rap, _, write) = self.place_without_column(expr)? else { return None };
    let chain = self.call_chain_label(inner).unwrap_or_else(|| format!("{}.…", rap.name()));
    let is_update = matches!(self.tcx.parent_hir_node(expr.hir_id),
      rustc_hir::Node::Expr(Expr { kind: ExprKind::AssignOp(_, lhs, _), .. }) if lhs.hir_id == expr.hir_id);
    // `op=` modifies the place in place: "modified through" the
    // reference the chain returns. `=` assigns to the place itself.
    let place = if is_update { chain } else { format!("{}{}", "*".repeat(derefs), chain) };
    Some(ResourceTy::Index(rap, place, write))
  }

  /// `v.iter_mut()`, `v.iter_mut().rev()`, `self.items.iter_mut()`: the
  /// variable a method chain starts from, when the chain's first call
  /// takes it by `&mut` (so what the chain hands out points into its
  /// resource, or into the resource it points to). For a field reached
  /// through a reference (`self.items` with `self: &mut Self`), the
  /// reference (#1060).
  pub(crate) fn mut_call_chain_root(&self, e: &'tcx Expr<'tcx>) -> Option<ResourceAccessPoint> {
    match self.call_chain_root(e)? {
      (ResourceTy::Value(r) | ResourceTy::DerefField(r, _, _, _), Mutability::Mut) => Some(r),
      _ => None,
    }
  }

  /// The place a method chain starts from and how its first call takes
  /// it (`&self` or `&mut self`): `Value(v)` for a variable v (`v.iter()`),
  /// `DerefField(self, "self.items")` for a field reached through a
  /// reference (`self.items.iter_mut()`; #1060). None when the first
  /// call takes its receiver by value.
  pub(crate) fn call_chain_root(&self, e: &'tcx Expr<'tcx>) -> Option<(ResourceTy, Mutability)> {
    let ExprKind::MethodCall(_, mut rcvr, _, _) = e.kind else { return None };
    let mut first_call = e;
    while let ExprKind::MethodCall(_, inner, _, _) = rcvr.kind {
      first_call = rcvr;
      rcvr = inner;
    }
    let def_id = self.tcx.typeck(e.hir_id.owner).type_dependent_def_id(first_call.hir_id)?;
    let sig = self.tcx.fn_sig(def_id).instantiate_identity().skip_norm_wip();
    let m = sig.skip_binder().inputs().first()?.ref_mutability()?;
    match get_rap(rcvr, &self.tcx, &self.raps) {
      ResourceTy::Value(r) if !r.is_fn() => Some((ResourceTy::Value(r), m)),
      _ => match self.field_access_through_ref(rcvr)? {
        f @ ResourceTy::DerefField(..) => Some((f, m)),
        _ => None,
      },
    }
  }

  /// The places a `for` loop's head borrows through method chains (each
  /// `call_chain_root`): the head's own chain (`a.iter_mut()` in
  /// `a.iter_mut().zip(b.iter())`), and the chains passed to its calls
  /// whose value can hold a borrow (`b.iter()`, `self.b.iter()`; #1066),
  /// not those that can't (`b.len()` in `.take(b.len())`).
  pub(crate) fn loop_head_chain_roots(&self, head: &'tcx Expr<'tcx>) -> Vec<(ResourceTy, Mutability)> {
    let typeck = self.tcx.typeck(head.hir_id.owner);
    let holds = |e: &'tcx Expr<'tcx>| typeck.expr_ty(e).walk().any(|g| matches!(g.kind(), GenericArgKind::Lifetime(_)));
    let mut out: Vec<(ResourceTy, Mutability)> = Vec::new();
    let mut todo: Vec<&'tcx Expr<'tcx>> = vec![head];
    while let Some(e) = todo.pop() {
      if let Some(root) = self.call_chain_root(e) {
        if !out.contains(&root) {
          out.push(root);
        }
      }
      let mut call = e;
      loop {
        match call.kind {
          ExprKind::MethodCall(_, rcvr, args, _) => {
            todo.extend(args.iter().filter(|a| holds(a)));
            call = rcvr;
          }
          ExprKind::Call(_, args) => {
            todo.extend(args.iter().filter(|a| holds(a)));
            break;
          }
          _ => break,
        }
      }
    }
    out
  }

  /// The reference variables a `for` loop's head passes as they are to
  /// its calls (`b` in `a.iter_mut().zip(b)` with `b: &Vec<_>` or `b:
  /// &mut Vec<_>`, also in a chain passed to one: `.zip(c.iter().chain(b))`;
  /// #1076). The loop's iterator holds a copy of a `&T` (or the moved
  /// `&mut T`) and reads or writes through it until the loop ends.
  /// Only the arguments of calls whose value can hold a borrow: not
  /// `make(b)` returning a `Vec<i32>`.
  pub(crate) fn loop_head_ref_args(&self, head: &'tcx Expr<'tcx>) -> Vec<ResourceAccessPoint> {
    let typeck = self.tcx.typeck(head.hir_id.owner);
    let holds = |e: &'tcx Expr<'tcx>| typeck.expr_ty(e).walk().any(|g| matches!(g.kind(), GenericArgKind::Lifetime(_)));
    let mut out: Vec<ResourceAccessPoint> = Vec::new();
    let mut todo: Vec<&'tcx Expr<'tcx>> = vec![head];
    while let Some(e) = todo.pop() {
      let args = match e.kind {
        ExprKind::MethodCall(_, rcvr, args, _) if holds(e) => {
          todo.push(rcvr);
          args
        }
        ExprKind::Call(_, args) if holds(e) => args,
        _ => continue,
      };
      for a in args.iter() {
        match a.kind {
          ExprKind::Path(_) if typeck.expr_ty(a).is_ref() && typeck.expr_adjustments(a).is_empty() => {
            if let ResourceTy::Value(r) = get_rap(a, &self.tcx, &self.raps) {
              if r.is_ref() && !out.contains(&r) {
                out.push(r);
              }
            }
          }
          ExprKind::MethodCall(..) | ExprKind::Call(..) => todo.push(a),
          _ => {}
        }
      }
    }
    out
  }

  /// The fields a call's result borrows when `lenders` (from
  /// `call_lenders`) is just the reference r and the call mentions r
  /// only through fields of what it points to, each with whether it is
  /// borrowed mutably: `self.items` for `self.items.first_mut().unwrap()`
  /// (#1073); `self.items` and `self.other` for
  /// `self.items.iter().chain(self.other.iter())` or `pick(&self.items,
  /// &self.other)` (#1080). rustc then restricts only those fields while
  /// the result is live (`self.n += 1` compiles). A field borrowed only
  /// for the call (`self.other` in `take(self.other.len())`) or read by
  /// copy (`skip(self.n)`) isn't one of them. A closure in the call
  /// that the result holds (`map(|x| *x as usize + self.n)`) borrows
  /// the fields it captures, each the way it captures it (#1093). None
  /// when r is mentioned any other way (by itself, or a closure
  /// capturing all of what it points to).
  pub(crate) fn field_call_lenders(&self, call: &'tcx Expr<'tcx>, lenders: &[ResourceTy]) -> Option<Vec<(ResourceTy, bool)>> {
    use rustc_middle::ty::adjustment::{Adjust, AutoBorrow, DerefAdjustKind};
    let [ResourceTy::Value(r)] = lenders else { return None };
    if !r.is_ref() || !matches!(call.kind, ExprKind::Call(..) | ExprKind::MethodCall(..)) {
      return None;
    }
    let typeck = self.tcx.typeck(call.hir_id.owner);
    let has_lifetime = |t: Ty<'tcx>| t.walk().any(|g| matches!(g.kind(), GenericArgKind::Lifetime(_)));
    let parent_expr = |e: &'tcx Expr<'tcx>| match self.tcx.parent_hir_node(e.hir_id) {
      rustc_hir::Node::Expr(p) => Some(p),
      _ => None,
    };
    let mut out: Vec<(ResourceTy, bool)> = Vec::new();
    let mut add = |l: ResourceTy, is_mut: bool| match out.iter_mut().find(|(x, _)| *x == l) {
      Some(entry) => entry.1 |= is_mut,
      None => out.push((l, is_mut)),
    };
    for m in locals_in(call, ClosureLocals::Captures(self.tcx)) {
      if binding_name(&self.tcx, m.id) != *r.name() {
        continue;
      }
      let Some(path) = m.expr else {
        for (l, is_mut) in self.closure_field_captures(call, m.closure?, m.id, r)? {
          add(l, is_mut);
        }
        continue;
      };
      // The whole field place: `self.items` (or `self.a.b`) from `self`.
      let mut place = path;
      while let Some(p @ Expr { kind: ExprKind::Field(base, _), .. }) = parent_expr(place) {
        if base.hir_id != place.hir_id {
          break;
        }
        place = p;
      }
      if place.hir_id == path.hir_id {
        return None;
      }
      let Some(ResourceTy::DerefField(fr, fname, w, holds)) = self.field_through_ref(place) else { return None };
      if fr.hash() != r.hash() {
        return None;
      }
      // How the field is used: borrowed (`&self.other`, or auto-ref'd
      // as a method's receiver), or its value read.
      let is_mut = match parent_expr(place) {
        Some(Expr { kind: ExprKind::AddrOf(_, mt, _), .. }) => Some(mt.is_mut()),
        _ => typeck.expr_adjustments(place).iter().find_map(|a| match a.kind {
          Adjust::Borrow(AutoBorrow::Ref(am)) => Some(Mutability::from(am).is_mut()),
          Adjust::Deref(DerefAdjustKind::Overloaded(od)) => Some(od.mutbl.is_mut()),
          _ => None,
        }),
      };
      let Some(is_mut) = is_mut else {
        // A copy of a plain value (`self.n` in `skip(self.n)`) holds
        // no borrow.
        let ty = typeck.expr_ty(place);
        if self.ty_is_copy(ty, call.hir_id.owner) && !has_lifetime(ty) {
          continue;
        }
        return None;
      };
      // The borrow reaches the result only if every value from the
      // borrow up to the call's result can hold it (`self.other.len()`
      // returns a `usize`: an instant borrow).
      let mut kept = true;
      let mut cur = place;
      while cur.hir_id != call.hir_id {
        let p = parent_expr(cur)?;
        if !has_lifetime(typeck.expr_ty_adjusted(p)) {
          kept = false;
          break;
        }
        cur = p;
      }
      if !kept {
        continue;
      }
      add(ResourceTy::DerefField(fr, fname, w, holds), is_mut);
    }
    (!out.is_empty()).then_some(out)
  }

  /// The call a closure in `call` that captures the field `place` of
  /// what the reference r points to is passed to (`map` for
  /// `self.items.iter().map(|x| x + self.n)` and `self.n`), and
  /// whether the capture is mutable.
  fn closure_capturing_field(&self, call: &'tcx Expr<'tcx>, r: &ResourceAccessPoint, place: &str) -> Option<(String, bool)> {
    locals_in(call, ClosureLocals::Captures(self.tcx)).into_iter().find_map(|m| {
      let closure = m.closure?;
      if binding_name(&self.tcx, m.id) != *r.name() {
        return None;
      }
      let is_mut = self.closure_field_captures(call, closure, m.id, r)?.into_iter()
        .find_map(|(l, is_mut)| matches!(&l, ResourceTy::DerefField(_, p, _, _) if p == place).then_some(is_mut))?;
      let rustc_hir::Node::Expr(callee) = self.tcx.parent_hir_node(closure.hir_id) else { return None };
      let name = match callee.kind {
        ExprKind::MethodCall(seg, ..) => seg.ident.as_str().to_owned(),
        ExprKind::Call(f, _) => hirid_to_var_name(f.hir_id, &self.tcx)?,
        _ => return None,
      };
      Some((name, is_mut))
    })
  }

  /// For `field_call_lenders`: the fields of what the reference r
  /// (binding `r_id`) points to that `closure`, a closure literal in
  /// `call`, holds borrows of once `call` returns, each with whether
  /// it is borrowed mutably. Edition 2021 closures capture just the
  /// places their body uses: `|x| *x as usize + self.n` captures
  /// `(*self).n` by shared reference, `|x| { self.n += 1; … }` by
  /// mutable reference (#1093). A by-value capture of a Copy field
  /// copies it (no borrow). Empty when the closure's borrows end with
  /// the call; None when the closure captures r some other way (all of
  /// `*self`, for `self.len()`, or `self` itself, as a `move` closure
  /// does), or a value between the closure and the call's result keeps
  /// its borrows without holding the closure. The call's result itself
  /// may (`boxed(|x| … self.n)` returning `Box<dyn Fn + 'a>`, #1104):
  /// when rustc says the closure's loans outlive the call, the result
  /// holds them.
  fn closure_field_captures(&self, call: &'tcx Expr<'tcx>, closure: &'tcx Expr<'tcx>, r_id: rustc_hir::HirId, r: &ResourceAccessPoint) -> Option<Vec<(ResourceTy, bool)>> {
    use rustc_middle::hir::place::{PlaceBase, ProjectionKind};
    let ExprKind::Closure(c) = closure.kind else { return None };
    let typeck = self.tcx.typeck(call.hir_id.owner);
    let has_lifetime = |t: Ty<'tcx>| t.walk().any(|g| matches!(g.kind(), GenericArgKind::Lifetime(_)));
    let holds_closure = |t: Ty<'tcx>| t.walk().any(|g| matches!(g.kind(),
      GenericArgKind::Type(t) if matches!(t.kind(), TyKind::Closure(d, _) if *d == c.def_id.to_def_id())));
    // The closure's borrows reach the result if every value from the
    // closure up to the call's result holds the closure (`map(|x| …)`
    // returns a `Map` that holds it), and end with the call if one of
    // them can't hold a borrow at all. A call that returns a value that
    // can hold borrows but not the closure (`find(|x| …)` returns an
    // `Option<&i32>`) may still keep them through its signature: rustc's
    // borrow checker says whether the closure's loans outlive the call
    // (#1101).
    let mut kept = true;
    let mut cur = closure;
    while cur.hir_id != call.hir_id {
      let rustc_hir::Node::Expr(p) = self.tcx.parent_hir_node(cur.hir_id) else { return None };
      let t = typeck.expr_ty_adjusted(p);
      if !has_lifetime(t) {
        kept = false;
        break;
      }
      if !holds_closure(t) {
        if !matches!(p.kind, ExprKind::Call(..) | ExprKind::MethodCall(..)) {
          return None;
        }
        match self.loans_issued_in_kept_after_call(p.span, closure.span) {
          Some(false) => kept = false,
          // `boxed(|x| … self.n)` returning `Box<dyn Fn + 'a>`: the
          // result doesn't name the closure but holds its borrows (#1104).
          Some(true) if p.hir_id == call.hir_id => {}
          _ => return None,
        }
        break;
      }
      cur = p;
    }
    let mut out = Vec::new();
    for cap in self.tcx.typeck(c.def_id).closure_min_captures_flattened(c.def_id) {
      let PlaceBase::Upvar(u) = cap.place.base else { return None };
      if u.var_path.hir_id != r_id {
        continue;
      }
      // `(*self).n` / `(*self).a.b`: fields of what r points to.
      let projs = &cap.place.projections;
      if projs.len() < 2 || projs[0].kind != ProjectionKind::Deref {
        return None;
      }
      let mut name = r.name().to_owned();
      for (i, proj) in projs.iter().enumerate().skip(1) {
        let ProjectionKind::Field(idx, variant) = proj.kind else { return None };
        let ty = cap.place.ty_before_projection(i);
        if matches!(ty.kind(), TyKind::Tuple(_)) {
          name = format!("{}.{}", name, idx.as_usize());
        } else {
          let adt = ty.ty_adt_def().filter(|a| a.is_struct())?;
          name = format!("{}.{}", name, adt.variant(variant).fields[idx].name.as_str());
        }
      }
      let ty = cap.place.ty();
      if ty.is_ref() {
        return None;
      }
      let is_mut = match cap.info.capture_kind {
        UpvarCapture::ByRef(kind) => !matches!(kind, BorrowKind::Immutable),
        UpvarCapture::ByValue | UpvarCapture::ByUse => {
          if self.ty_is_copy(ty, call.hir_id.owner) && !has_lifetime(ty) {
            continue;
          }
          return None;
        }
      };
      if kept {
        out.push((ResourceTy::DerefField(r.to_owned(), name, PlaceWrite::Update, self.field_holds(ty, call.hir_id.owner)), is_mut));
      }
    }
    Some(out)
  }

  /// A method chain as written (`v.get_mut(0).unwrap()`), for a tooltip.
  /// An argument list that is long or has characters a label leaves
  /// out (`m.get_mut(&k)`, `m.get_mut("k")`) is shortened to `(…)`.
  fn call_chain_label(&self, e: &Expr) -> Option<String> {
    if e.span.from_expansion() {
      return None;
    }
    let text = self.tcx.sess.source_map().span_to_snippet(e.span).ok().filter(|t| t.chars().count() <= 32
      && !t.contains(|c: char| matches!(c, '<' | '>' | '&' | '"' | '\'' | '\n' | '\r')));
    if text.is_some() {
      return text;
    }
    match e.kind {
      ExprKind::MethodCall(seg, rcvr, args, _) => {
        let rcvr = self.call_chain_label(rcvr)?;
        Some(format!("{}.{}({})", rcvr, seg.ident, if args.is_empty() { "" } else { "…" }))
      }
      _ => None,
    }
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
    if !self.store_into_holder(lhs, n_events, &dest) {
      self.end_stored_borrow(lhs, n_events, &dest, dest.clone());
    }
  }

  /// The local whose value holds the borrows (`refs` in `let mut refs
  /// = vec![&a1];`, see `holds_borrow`) that a write to the
  /// reference-typed place `place` replaces one of: an element
  /// `refs[0]`, or `*el` after `let el = &mut refs[0];` (#1361), or
  /// `*refs.get_mut(0).unwrap()`, through a call's result that points
  /// into refs only (#1384). None for anything else (a place behind a
  /// parameter, a holder we don't track).
  fn element_holder(&self, place: &'tcx Expr<'tcx>) -> Option<ResourceAccessPoint> {
    let holder_of = |e: &'tcx Expr<'tcx>| -> Option<ResourceAccessPoint> {
      if !matches!(e.kind, ExprKind::Path(QPath::Resolved(None, _))) || self.is_ref_param(e) {
        return None;
      }
      let name = expr_to_rap_name(e, &self.tcx)?;
      let rap = self.raps.get(&name)?.rap.to_owned();
      (rap.holds_borrow() && self.borrow_map.contains_key(&name)).then_some(rap)
    };
    match place.kind {
      ExprKind::Index(base, _, _) => holder_of(base),
      ExprKind::Unary(UnOp::Deref, inner) if matches!(inner.kind, ExprKind::Call(..) | ExprKind::MethodCall(..))
        && !inner.span.from_expansion() => {
        let lenders = self.pointer_lenders(inner).unwrap_or_else(|| self.call_lenders(inner));
        let [ResourceTy::Value(h)] = lenders.as_slice() else { return None };
        (h.holds_borrow() && self.borrow_map.contains_key(h.name())).then(|| h.to_owned())
      }
      ExprKind::Unary(UnOp::Deref, inner) => {
        if !matches!(inner.kind, ExprKind::Path(QPath::Resolved(None, _))) || self.is_ref_param(inner) {
          return None;
        }
        let name = expr_to_rap_name(inner, &self.tcx)?;
        let ResourceTy::Value(h) = &self.borrow_map.get(&name)?.lender else { return None };
        (h.holds_borrow() && self.borrow_map.contains_key(h.name())).then(|| h.to_owned())
      }
      _ => None,
    }
  }

  /// `refs[0] = &t1;` / `*el = &t1;` (el borrowing `refs[0]`): the new
  /// reference is stored in refs's value, so refs now holds t1's
  /// borrow, which lasts until refs's last use, as for
  /// `refs.push(&t1)` (#392). Retarget the borrows `match_rhs` drew
  /// into `dest` (from event `n_events` on) to refs's column and record
  /// their lenders as refs's. (The old element's lender stays borrowed
  /// too: refs's type has one lifetime for both.) True if it did.
  pub fn store_into_holder(&mut self, place: &'tcx Expr<'tcx>, n_events: usize, dest: &ResourceTy) -> bool {
    let Some(holder) = self.element_holder(place) else { return false };
    let mut lenders: Vec<(ResourceTy, bool)> = Vec::new();
    for (_, ev) in self.preprocessed_events[n_events..].iter() {
      match ev {
        ExternalEvent::StaticBorrow { from: from @ ResourceTy::Value(l), to, .. } if to == dest && !l.is_fn() =>
          lenders.push((from.clone(), false)),
        ExternalEvent::MutableBorrow { from: from @ ResourceTy::Value(l), to, .. } if to == dest && !l.is_fn() =>
          lenders.push((from.clone(), true)),
        _ => {}
      }
    }
    if lenders.is_empty() {
      return false;
    }
    let to = ResourceTy::Value(holder.clone());
    let retarget = |e: &mut ExternalEvent| {
      if let ExternalEvent::StaticBorrow { to: t, .. } | ExternalEvent::MutableBorrow { to: t, .. } = e {
        if t == dest {
          *t = to.clone();
        }
      }
    };
    self.preprocessed_events[n_events..].iter_mut().for_each(|(_, e)| retarget(e));
    let first_id = self.preprocessed_events[n_events..].iter().map(|(_, e)| e.get_id()).min().unwrap_or(usize::MAX);
    self.event_line_map.values_mut().flatten().filter(|e| e.get_id() >= first_id).for_each(retarget);
    if let Some(rd) = self.borrow_map.get_mut(holder.name()) {
      for (l, m) in lenders {
        if l != rd.lender && !rd.co_lenders.contains(&l) {
          rd.co_lenders.push(l);
        }
        rd.ref_mutability |= m;
      }
    }
    true
  }

  /// `refs[0] = tr;` / `*el = tr;` (el borrowing `refs[0]`), with `tr`
  /// a reference (#1384): the element gets a copy of tr (or, for a
  /// `&mut`, tr itself), so refs now holds tr's borrow, which lasts
  /// until refs's last use, as for `refs[0] = &t1` (#1361) and
  /// `refs.push(tr)`. Draws the borrows of what tr borrows into refs's
  /// column and records them as refs's lenders. (`&t1` on the right is
  /// `store_into_holder`'s.)
  /// `match_rhs` drew the write from event `n_events` on.
  pub fn store_ref_copy_into_holder(&mut self, place: &'tcx Expr<'tcx>, rhs: &'tcx Expr<'tcx>, n_events: usize) {
    if matches!(rhs.kind, ExprKind::AddrOf(..)) || rhs.span.from_expansion() {
      return;
    }
    let ty = self.tcx.typeck(rhs.hir_id.owner).expr_ty(rhs);
    let Some(m) = ty.ref_mutability() else { return };
    let Some(holder) = self.element_holder(place) else { return };
    // (A local `&mut` tr is reborrowed, `&mut *tr`: refs borrows
    // through tr, which can't be used while refs holds it, #1329.)
    let lenders: Vec<(ResourceTy, bool)> = self.value_lenders(rhs).into_iter()
      .filter_map(|l| match l {
        ResourceTy::Value(r) if !r.is_fn() && r.hash() != holder.hash() => Some((ResourceTy::Value(r), false)),
        ResourceTy::Deref(r) if r.is_mutref() => Some((ResourceTy::Value(r), true)),
        _ => None,
      })
      .collect();
    if lenders.is_empty() {
      return;
    }
    let line = expr_to_line(rhs, &self.tcx);
    let to = ResourceTy::Value(holder.clone());
    for (from, is_reborrow) in &lenders {
      let (from, to, is_reborrow, id) = (from.clone(), to.clone(), *is_reborrow, *self.unique_id);
      *self.unique_id += 1;
      self.add_external_event(line, if m.is_mut() {
        ExternalEvent::MutableBorrow { from, to, is_partial: false, is_reborrow, break_alt: None, via: None, id, of_ref_var: false }
      } else {
        ExternalEvent::StaticBorrow { from, to, is_partial: false, is_reborrow, break_alt: None, via: None, id, of_ref_var: false }
      });
    }
    // A reborrowed tr isn't moved (`match_rhs` drew a move).
    for (l, _) in lenders.iter().filter(|(_, reborrow)| *reborrow) {
      let moved = |e: &ExternalEvent| matches!(e, ExternalEvent::Move { from: ResourceTy::Value(f), .. } if f.hash() == l.hash());
      let ids: Vec<usize> = self.preprocessed_events[n_events..].iter().filter(|(_, e)| moved(e)).map(|(_, e)| e.get_id()).collect();
      self.preprocessed_events.retain(|(_, e)| !ids.contains(&e.get_id()));
      self.event_line_map.values_mut().for_each(|evs| evs.retain(|e| !ids.contains(&e.get_id())));
    }
    let lenders: Vec<ResourceTy> = lenders.into_iter().map(|(l, _)| l).collect();
    if let Some(rd) = self.borrow_map.get_mut(holder.name()) {
      for l in lenders {
        if l != rd.lender && !rd.co_lenders.contains(&l) {
          rd.co_lenders.push(l);
        }
      }
      rd.ref_mutability |= m.is_mut();
    }
  }

  /// After `match_rhs` drew `&x` (from event `n_events` on) going into
  /// `dest`, a place with no column of its own (`v[0] = &u`, #282;
  /// `*r = &u` with r pointing to an element, #897): x's loan ends where
  /// the borrow checker ends it, drawn as an end from `end_from`. Not
  /// for a place behind a reference parameter: the reference stored
  /// there is handed back to the caller, and its borrow lasts past this
  /// function's end.
  pub fn end_stored_borrow(&mut self, lhs: &'tcx Expr<'tcx>, n_events: usize, dest: &ResourceTy, end_from: ResourceTy) {
    if self.place_root(lhs).is_some_and(|(root, _)| self.is_ref_param(root)) {
      return;
    }
    // The borrow match_rhs just recorded, if any (a borrow of a
    // temporary, `v[0] = &String::new()`, has no lender column).
    let borrow = self.preprocessed_events[n_events..].iter().find_map(|(line, ev)| match ev {
      ExternalEvent::StaticBorrow { from: from @ ResourceTy::Value(lender), to, .. } if to == dest =>
        Some((*line, from.clone(), lender.name().to_owned(), Evt::SDie)),
      ExternalEvent::MutableBorrow { from: from @ ResourceTy::Value(lender), to, .. } if to == dest =>
        Some((*line, from.clone(), lender.name().to_owned(), Evt::MDie)),
      _ => None,
    });
    let Some((line, lender, lender_name, die)) = borrow else { return };
    if let Some(kill_line) = self.loan_end_line(line, &lender_name) {
      // StaticDie/MutableDie { from: end_from, to: lender }.
      self.add_ev(kill_line, die, lender, end_from, false);
    }
  }

  fn call_argument_site(&self, arg: &Expr<'tcx>) -> Option<(rustc_span::Span, usize)> {
    let rustc_hir::Node::Expr(call) = self.tcx.parent_hir_node(arg.hir_id) else { return None };
    let index = match call.kind {
      ExprKind::Call(_, args) => args.iter().position(|a| a.hir_id == arg.hir_id),
      ExprKind::MethodCall(_, receiver, args, _) =>
        std::iter::once(receiver).chain(args.iter()).position(|a| a.hir_id == arg.hir_id),
      _ => None,
    };
    Some((call.span.with_parent(None), index?))
  }

  /// Draw a call's store only when the actual MIR source operand carries
  /// the loan and the callee can write it through the destination operand.
  fn argument_stored_borrows(&mut self, arg: &'tcx Expr<'tcx>, from: &ResourceTy, line: usize, issued: Option<usize>)
      -> Vec<(ResourceTy, ResourceTy, usize, usize)> {
    let Some(facts) = self.body_facts else { return Vec::new() };
    let Some(site) = self.call_argument_site(arg) else { return Vec::new() };
    let Some(stores) = facts.argument_stores.get(&site) else { return Vec::new() };
    let from_call = match arg.kind {
      ExprKind::MethodCall(..) => true,
      ExprKind::Call(callee, _) => !crate::visitor::is_ctor_path(callee, self.tcx.typeck(arg.hir_id.owner)),
      _ => false,
    } && !arg.span.from_expansion();
    let lenders = if from_call {
      self.call_lenders(arg)
    } else if matches!(arg.kind, ExprKind::AddrOf(..)) {
      vec![from.clone()]
    } else {
      return Vec::new();
    };
    let mut out = Vec::new();
    for (holder, ids) in stores {
      let Some(place) = self.raps.get(holder).map(|d| &d.rap) else { continue };
      for lender in &lenders {
        let ResourceTy::Value(l) = lender else { continue };
        if l.is_fn() { continue; }
        for loan in facts.loans.iter().filter(|loan| ids.contains(&loan.loan) && loan.lender == *l.name()) {
          if !from_call && issued != Some(loan.loan) { continue; }
          let Some(end) = loan.end_lines.last().copied().filter(|&end| end > line) else { continue };
          let dest = ResourceTy::Index(place.clone(), holder.clone(), PlaceWrite::Update);
          out.push((lender.clone(), dest, end, loan.loan));
        }
      }
    }
    out.dedup();
    if from_call {
      for (lender, ..) in &out {
        self.mark_borrow_kept_by_result(arg, std::slice::from_ref(lender));
      }
    }
    out
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

  /// `rh.r` / `(*rh).r` / `ri.r`, a reference-typed field reached
  /// through a local reference that borrows a struct with field
  /// columns (`let rh = &mut hold;`, `let ri = &mut o.inner;`): the
  /// field's own column (`hold.r`, `o.inner.r`), for a write to it
  /// (#1361). None for anything else.
  pub fn ref_field_through_local_ref(&self, place: &'tcx Expr<'tcx>) -> Option<ResourceTy> {
    let ExprKind::Field(..) = place.kind else { return None };
    let (root, false) = self.place_root(place)? else { return None };
    let root_name = expr_to_rap_name(root, &self.tcx)?;
    let name = self.field_through_local_ref(root, &root_name, place)?;
    let rap = self.raps.get(&name)?.rap.to_owned();
    (rap.is_ref() && self.borrow_map.contains_key(&name)).then_some(ResourceTy::Value(rap))
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
  pub(crate) fn place_root(&self, place: &'tcx Expr<'tcx>) -> Option<(&'tcx Expr<'tcx>, bool)> {
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
  pub(crate) fn plain_source_text(&self, expr: &Expr) -> Option<String> {
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
    let typing_env = typing_env(self.tcx, expr.hir_id.owner);
    let write = if self.tcx.type_is_copy_modulo_regions(typing_env, pointee_ty) {
      PlaceWrite::AssignCopy
    } else if self.needs_drop(pointee_ty, expr.hir_id.owner) {
      PlaceWrite::AssignDropsOld
    } else {
      PlaceWrite::AssignOwned
    };
    Some(ResourceTy::Index(rap, place, write))
  }

  /// The column of the `static` whose field (or field of a field) the
  /// projection `expr` reads (`ORIGIN.x`, `CFG.size.w`; #690), if the
  /// field isn't a reference: a static has one column, not one per
  /// field.
  fn static_field_holder(&self, expr: &'tcx Expr<'tcx>) -> Option<ResourceAccessPoint> {
    if expr.span.from_expansion() || self.tcx.typeck(expr.hir_id.owner).node_type(expr.hir_id).is_ref() {
      return None;
    }
    let mut base = expr;
    while let ExprKind::Field(b, _) = base.kind {
      base = b;
    }
    let ExprKind::Path(QPath::Resolved(_, p)) = base.kind else { return None };
    static_of_path(p)?;
    self.raps.path(p, &self.tcx).map(|rd| rd.rap.to_owned())
  }

  /// The column of the variable or static that a field of an element
  /// (`pts[0].x`, `PTS[1].y`, `ws[0].inner.n`) is read out of (#1065):
  /// the collection indexed first. None if there's no index, if the
  /// field is a reference, or if the place goes through a reference
  /// (or a smart pointer other than `Box`) after the index: then the
  /// data belongs to whatever that points to, not to the collection.
  fn element_field_holder(&self, expr: &'tcx Expr<'tcx>) -> Option<ResourceAccessPoint> {
    let typeck = self.tcx.typeck(expr.hir_id.owner);
    if expr.span.from_expansion() || !matches!(expr.kind, ExprKind::Field(..)) || typeck.node_type(expr.hir_id).is_ref() {
      return None;
    }
    let base = index_base(expr)?;
    if !self.held_by_base(expr, base) {
      return None;
    }
    expr_to_rap_name(base, &self.tcx)
      .and_then(|n| self.raps.get(&n))
      .map(|rd| rd.rap.to_owned())
      .filter(|r| !r.is_fn())
  }

  /// The column a borrow of the place `place` is drawn on when the
  /// place has none of its own: a field of an element (`people[0].age`,
  /// `arr[1].p.x`) or of what a `Box` holds (`b.age`), whose data is
  /// part of the variable's value (#1176). Returns the nearest enclosing
  /// place with a column (the variable, or a struct field such as
  /// `w.items` in `w.items[0].name`). None when the place is reached
  /// through a reference, a raw pointer or a smart pointer other than
  /// `Box` (the data isn't the variable's then; reborrows through a
  /// reference have their own handling), and when the place is itself
  /// a reference.
  pub fn place_holder(&self, place: &'tcx Expr<'tcx>) -> Option<ResourceAccessPoint> {
    let typeck = self.tcx.typeck(place.hir_id.owner);
    if place.span.from_expansion() || !matches!(place.kind, ExprKind::Field(..)) || typeck.node_type(place.hir_id).is_ref() {
      return None;
    }
    let mut cur = place;
    loop {
      let (ExprKind::Field(recv, _) | ExprKind::Index(recv, _, _)) = cur.kind else { return None };
      let recv_ty = typeck.node_type(recv.hir_id);
      let through_ptr = typeck.expr_adjustments(recv).iter().any(|a| matches!(a.kind, adjustment::Adjust::Deref(_)))
        && !recv_ty.is_box();
      if through_ptr || recv_ty.is_any_ptr() {
        return None;
      }
      if matches!(recv.kind, ExprKind::Path(_) | ExprKind::Field(..)) {
        if let Some(rd) = expr_to_rap_name(recv, &self.tcx).and_then(|n| self.raps.get(&n)) {
          let rap = rd.rap.to_owned();
          return (!rap.is_ref() && !rap.is_fn()).then_some(rap);
        }
      }
      cur = recv;
    }
  }

  /// The place `expr` (fields and indexes applied to `base`) is part of
  /// what `base` holds, or of what it points to if `base` is itself a
  /// reference: no step after `base` goes through a reference or a smart
  /// pointer other than `Box` (`rs[0].x` with `rs: Vec<&Pt>` is not).
  fn held_by_base(&self, expr: &'tcx Expr<'tcx>, base: &'tcx Expr<'tcx>) -> bool {
    let typeck = self.tcx.typeck(expr.hir_id.owner);
    let mut cur = expr;
    while let ExprKind::Field(recv, _) | ExprKind::Index(recv, _, _) = cur.kind {
      if recv.hir_id == base.hir_id {
        break;
      }
      let through_ptr = typeck.expr_adjustments(recv).iter().any(|a| matches!(a.kind, adjustment::Adjust::Deref(_)))
        && !typeck.node_type(recv.hir_id).is_box();
      if through_ptr || typeck.node_type(recv.hir_id).is_any_ptr() {
        return false;
      }
      cur = recv;
    }
    true
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
    Some(ResourceTy::DerefField(rap, place, PlaceWrite::Update, self.field_holds(typeck.node_type(expr.hir_id), expr.hir_id.owner)))
  }

  /// What a place of type `ty` holds, for its tooltips: a value (Copy,
  /// or nothing to drop) or a resource (#484, #1100).
  pub(crate) fn field_holds(&self, ty: Ty<'tcx>, owner: rustc_hir::OwnerId) -> FieldHolds {
    FieldHolds { value: !self.needs_drop(ty, owner) }
  }

  /// The reference variable `base` that the element place `elem`
  /// (`v[0]`, `g[i][j]`, with `base` the collection indexed first) is
  /// reached through (#1071). None if `base` isn't a reference column,
  /// or if the place goes through another reference after it
  /// (`v[0][1]` with `v: &Vec<&Vec<i32>>`): then the element belongs to
  /// what that reference points to, and isn't reborrowed through `base`.
  fn element_holder_ref(&self, elem: &'tcx Expr<'tcx>, base: &'tcx Expr<'tcx>) -> Option<ResourceAccessPoint> {
    if !self.held_by_base(elem, base) {
      return None;
    }
    expr_to_rap_name(base, &self.tcx)
      .and_then(|n| self.raps.get(&n))
      .map(|rd| rd.rap.to_owned())
      .filter(|r| r.is_ref() && !r.is_static() && !r.is_fn())
  }

  /// `&self.s` / `&mut p.items[0]` / `&ps[1].y` / `&mut v[0]`: a
  /// reference to a place reached through the reference variable
  /// `self` / `p` / `ps` / `v`, a reborrow of what it points to (#784,
  /// #1065, #1071). `DerefField(self, "&self.s")`, drawn on the
  /// reference's column. Also `&mut *v` (all of what `v` points to),
  /// `&v[0][1]` reborrowed through the reference `v[0]` held in what
  /// `v` points to, and a reference a call returns into what `v`
  /// points to (`v.get_mut(0).unwrap()`; #1078). None when the place
  /// is itself a reference (`&self.r` with `r: &T`).
  pub fn reborrow_through_ref(&self, e: &'tcx Expr<'tcx>) -> Option<ResourceTy> {
    if e.span.from_expansion() {
      return None;
    }
    let typeck = self.tcx.typeck(e.hir_id.owner);
    // `fn f(r: &mut i32) -> &i32 { r }`: r is coerced to the `&i32` the
    // caller gets by an implicit `&*r`, an immutable reborrow, not
    // moved (#1329).
    if let ExprKind::Path(QPath::Resolved(None, _)) = e.kind {
      if !is_implicit_shared_reborrow_of_mut(e, typeck) {
        return None;
      }
      let rap = expr_to_rap_name(e, &self.tcx)
        .and_then(|n| self.raps.get(&n))
        .map(|rd| rd.rap.to_owned())
        .filter(|r| r.is_mutref() && !r.is_static() && !r.is_fn())?;
      let pointee = typeck.node_type(e.hir_id).builtin_deref(true)?;
      let place = format!("&*{}", rap.name());
      return Some(ResourceTy::DerefField(rap, place, PlaceWrite::Update, self.field_holds(pointee, e.hir_id.owner)));
    }
    // `v.get_mut(0).unwrap()` / `v.first().unwrap()` with `v` a
    // reference: a reference into what v points to (#1078).
    if matches!(e.kind, ExprKind::MethodCall(..) | ExprKind::Call(..))
      || try_operand(e).is_some_and(|op| matches!(op.kind, ExprKind::MethodCall(..) | ExprKind::Call(..))) {
      let r = self.call_reborrows_through(e)?;
      let is_mut = typeck.node_type(e.hir_id).ref_mutability().is_some_and(|m| m.is_mut());
      let place = crate::svg_generator::hover_messages::reborrow_place_via_call(is_mut, &r.method, &r.target);
      let pointee = typeck.node_type(e.hir_id).builtin_deref(true).unwrap_or(typeck.node_type(e.hir_id));
      return Some(ResourceTy::DerefField(r.through, place, PlaceWrite::Update, self.field_holds(pointee, e.hir_id.owner)));
    }
    let ExprKind::AddrOf(_, _, place) = e.kind else { return None };
    let text = || self.tcx.sess.source_map().span_to_snippet(e.span).ok().filter(|s| !s.contains('\n'));
    // `&mut *v` / `&*v` with `v` a reference: an explicit reborrow of
    // all of what v points to (#1078).
    let holds = self.field_holds(typeck.node_type(place.hir_id), e.hir_id.owner);
    if let ExprKind::Unary(UnOp::Deref, inner) = place.kind {
      if !matches!(inner.kind, ExprKind::Path(QPath::Resolved(None, _))) || !typeck.node_type(inner.hir_id).is_ref() {
        return None;
      }
      let rap = expr_to_rap_name(inner, &self.tcx)
        .and_then(|n| self.raps.get(&n))
        .map(|rd| rd.rap.to_owned())
        .filter(|r| r.is_ref() && !r.is_static() && !r.is_fn())?;
      return Some(ResourceTy::DerefField(rap, text()?, PlaceWrite::Update, holds));
    }
    // `&ps[1].y` / `&mut ps[0].x` with `ps` a reference: a field of an
    // element of what ps points to (#1065).
    if let Some(rap) = self.element_field_holder(place).filter(|r| r.is_ref()) {
      return Some(ResourceTy::DerefField(rap, text()?, PlaceWrite::Update, holds));
    }
    let mut p = place;
    while let ExprKind::Index(recv, _, _) = p.kind {
      p = recv;
    }
    // `&mut v[0]` / `&xs[i]` / `&mut g[i][j]` with `v` / `xs` / `g` a
    // reference: an element of what it points to (#1071).
    if !std::ptr::eq(p, place) && matches!(p.kind, ExprKind::Path(_)) {
      if let Some(rap) = self.element_holder_ref(place, p) {
        return Some(ResourceTy::DerefField(rap, text()?, PlaceWrite::Update, holds));
      }
      // `&v[0][1]` with `v: &Vec<&Vec<i32>>`: the element is in what
      // the reference `v[0]` points to, so it is reborrowed through
      // `v[0]`, not through v (#1078).
      let ExprKind::Index(through, _, _) = place.kind else { return None };
      if !typeck.node_type(through.hir_id).is_ref() || !matches!(through.kind, ExprKind::Index(..)) {
        return None;
      }
      let rap = self.element_holder_ref(through, p)?;
      let through_text = self.plain_source_text(through)?;
      return Some(ResourceTy::DerefField(rap, crate::svg_generator::hover_messages::reborrow_place_through(&text()?, &through_text), PlaceWrite::Update, holds));
    }
    let ExprKind::Field(..) = p.kind else { return None };
    let ResourceTy::DerefField(rap, _, w, holds) = self.field_through_ref(p)? else { return None };
    Some(ResourceTy::DerefField(rap, text()?, w, holds))
  }

  /// The value `e`, returned (or passed to a call by value, #1170), is
  /// part of a variable or static that has a column, not all of it: `v[0]` / `g[1][0]` / `ORIGIN.x` (a Copy
  /// element or field copied out, #1052), or `&PRIMES` / `&PRIMES[0]`
  /// (a reference to (part of) a static, #1053), or a field of an
  /// element (`pts[0].x`, #1065). Returns
  /// `Index(<holder>, <place>)`, drawn on the holder's column; for a
  /// reference the place starts with `&` / `&mut `.
  pub fn returned_part(&mut self, e: &'tcx Expr<'tcx>) -> Option<ResourceTy> {
    if e.span.from_expansion() {
      return None;
    }
    let typeck = self.tcx.typeck(e.hir_id.owner);
    let ty = typeck.node_type(e.hir_id);
    let line_num = expr_to_line(e, &self.tcx);
    if let ExprKind::AddrOf(_, mutbl, place) = e.kind {
      let mut base = place;
      while let ExprKind::Index(b, _, _) | ExprKind::Field(b, _) = base.kind {
        base = b;
      }
      let ExprKind::Path(QPath::Resolved(_, p)) = base.kind else { return None };
      static_of_path(p)?;
      let rap = self.raps.path(p, &self.tcx).map(|rd| rd.rap.to_owned())?;
      self.update_rap(&rap, line_num);
      let amp = if mutbl == Mutability::Mut { "&mut " } else { "&" };
      let label = self.place_label(place, rap.name());
      return Some(ResourceTy::Index(rap, format!("{amp}{label}"), Default::default()));
    }
    if ty.is_ref() {
      return None;
    }
    let rap = match e.kind {
      ExprKind::Index(recv, _, _) => expr_to_rap_name(recv, &self.tcx)
        .and_then(|n| self.raps.get(&n))
        .map(|rd| rd.rap.to_owned())
        .filter(|r| !r.is_fn())?,
      // `pts[0].x` / `PTS[1].y`: a field of an element (#1065).
      ExprKind::Field(..) => self.element_field_holder(e).or_else(|| self.static_field_holder(e))?,
      _ => return None,
    };
    self.update_rap(&rap, line_num);
    let place = self.place_label(e, rap.name());
    Some(ResourceTy::Index(rap, place, Default::default()))
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
    Some(ResourceTy::DerefField(rap, place, PlaceWrite::Update, self.field_holds(typeck.node_type(expr.hir_id), expr.hir_id.owner)))
  }

  /// The `e?` whose operand is `e` (see `try_operand`), if any.
  pub fn try_of_operand(&self, e: &'tcx Expr<'tcx>) -> Option<&'tcx Expr<'tcx>> {
    let rustc_hir::Node::Expr(branch) = self.tcx.parent_hir_node(e.hir_id) else { return None };
    let rustc_hir::Node::Expr(m) = self.tcx.parent_hir_node(branch.hir_id) else { return None };
    try_operand(m).filter(|op| op.hir_id == e.hir_id).map(|_| m)
  }

  /// `Part { text: x }` / `(x, 1)` / `[x, y]` returned with `x` a
  /// reference variable: the reference is copied, moved, or reborrowed
  /// into the value the caller gets, so x's borrow goes on
  /// in the caller, as for a bare `x` or `Some(x)` (#1340). A reference
  /// reborrowed through one (`(x.trim(), 1)`, `Part { text: &x[1..] }`)
  /// goes the same way, as a bare `x.trim()` does (#784, #1078). Returns
  /// whether it drew that.
  pub fn ref_part_to(&mut self, elem: &'tcx Expr<'tcx>, to: ResourceTy) -> bool {
    if elem.span.from_expansion() {
      return false;
    }
    let Some(m) = self.tcx.typeck(elem.hir_id.owner).node_type(elem.hir_id).ref_mutability() else { return false };
    let line = expr_to_line(elem, &self.tcx);
    if matches!(elem.kind, ExprKind::Path(_) | ExprKind::Field(..)) {
      let ResourceTy::Value(rap) = get_rap(elem, &self.tcx, &self.raps) else { return false };
      if !rap.is_ref() || rap.is_fn() {
        return false;
      }
      let (row, evt, from) = if m == Mutability::Mut {
        if let Some(Reborrow { loan, through, holder }) = self.mut_ref_reborrow_at(elem) {
          // A local aggregate gets its own borrow arrow. Only a reborrow
          // returned to the caller needs an outgoing reference here.
          if !matches!(to, ResourceTy::Caller) && holder != "<return>" {
            self.update_rap(&through, loan.line);
            return false;
          }
          let ty = self.tcx.typeck(elem.hir_id.owner).expr_ty_adjusted(elem);
          let Some(pointee) = ty.builtin_deref(true) else { return false };
          // A deref coercion can send the loan of *s into Deref::deref
          // and return its result. Only name an actual aggregate operand.
          let direct = self.body_facts.is_some_and(|f| f.returned_operand_loans.contains(&loan.loan));
          let title = crate::svg_generator::hover_messages::aggregate_reference_return(
            direct.then_some((&loan.lender, through.name())), ty.is_mutable_ptr());
          self.row_layout.aggregate_titles.insert(*self.unique_id, title);
          let source = ResourceTy::DerefField(through, format!("&{}", loan.lender), PlaceWrite::Update, self.field_holds(pointee, elem.hir_id.owner));
          (loan.line, Evt::Copy, source)
        } else {
          let Some(t) = self.transfer_at(elem).filter(|t| t.path == rap.name()) else { return false };
          (t.row, t.kind, ResourceTy::Value(rap.clone()))
        }
      } else {
        (line, Evt::Copy, ResourceTy::Value(rap.clone()))
      };
      self.update_rap(&rap, row);
      self.add_ev(row, evt, to, from, false);
      return true;
    }
    // A written borrow stored by a local constructor is drawn at its
    // issue. Emitting a copy here would also draw a read of the pointee.
    if !matches!(to, ResourceTy::Caller) && self.body_facts.is_some_and(|facts|
      self.effects.get(&elem.span.with_parent(None)).into_iter().flatten().any(|(_, ev)|
        matches!(ev.fact, crate::facts::Fact::Borrow { loan, .. } if !facts.returned_operand_loans.contains(&loan)
          && facts.issued_operand_loans.contains(&loan)))) {
      return false;
    }
    let Some(reborrow) = self.reborrow_through_ref(elem) else { return false };
    // The borrow the call takes through `x` goes on in the value.
    if let Some(r) = self.call_reborrows_through(elem) {
      self.mark_borrow_kept_by_result(try_operand(elem).unwrap_or(elem), &[ResourceTy::Value(r.through)]);
    }
    self.add_ev(line, Evt::Copy, to, reborrow, false);
    true
  }

  /// The tuple-struct / variant constructor call whose argument the
  /// literal `e` is, directly or as a part of an enclosing struct or
  /// tuple literal (see `enclosing_ctor_name`).
  pub fn enclosing_ctor(&self, e: &'tcx Expr<'tcx>) -> Option<&'tcx Expr<'tcx>> {
    let mut cur = e;
    loop {
      // A struct literal's field value's parent is the `ExprField`.
      let mut parent = self.tcx.parent_hir_node(cur.hir_id);
      if let rustc_hir::Node::ExprField(f) = parent {
        parent = self.tcx.parent_hir_node(f.hir_id);
      }
      let rustc_hir::Node::Expr(p) = parent else { return None };
      match p.kind {
        ExprKind::Struct(..) | ExprKind::Tup(_) => cur = p,
        ExprKind::Call(callee, args) if args.iter().any(|a| a.hir_id == cur.hir_id)
          && crate::visitor::is_ctor_path(callee, self.tcx.typeck(p.hir_id.owner)) => return Some(p),
        _ => return None,
      }
    }
  }

  /// A reference variable `elem` in a struct or tuple literal built as
  /// the argument of the constructor `fn_name` (`Some(Part { text: x })`,
  /// `Ok((x, 2))`): like `Some(x)`, its copy (or move, for `&mut`) goes
  /// where the built value goes, drawn there once that is known
  /// (`retarget_ctor_arg_events`; #1340). Not when the value is passed
  /// to a call, which borrows through it (`match_arg`'s constructor case).
  /// Returns whether it drew that.
  pub fn ref_part_into_ctor(&mut self, lit: &'tcx Expr<'tcx>, elem: &'tcx Expr<'tcx>, fn_name: &str) -> bool {
    let typeck = self.tcx.typeck(elem.hir_id.owner);
    if !typeck.node_type(elem.hir_id).is_ref() {
      return false;
    }
    let Some(ctor) = self.enclosing_ctor(lit) else { return false };
    if self.enclosing_ctor_name(lit).as_deref() != Some(fn_name) {
      return false;
    }
    let to_call = matches!(self.tcx.parent_hir_node(ctor.hir_id),
      rustc_hir::Node::Expr(p) if matches!(p.kind, ExprKind::Call(..) | ExprKind::MethodCall(..)));
    if to_call {
      return false;
    }
    match self.raps.get(&fn_key(fn_name)).map(|rd| ResourceTy::Value(rd.rap.to_owned())) {
      Some(to) => self.ref_part_to(elem, to),
      None => false,
    }
  }

  /// The elements of an array literal (or `vec![…]`) going into `lhs`.
  /// Each element that is a variable (`[s, t]`, `vec![x, y]`) is moved
  /// into `lhs` (copied if its type is Copy), drawn as an arrow from the
  /// variable. Other elements (literals, calls) have nothing to draw
  /// beyond `lhs` getting its value. A reference element (`[&s]`)
  /// stores a borrow we don't track, so it's left out.
  pub(crate) fn draw_value_receiver(&mut self, receiver: &'tcx Expr<'tcx>, to: ResourceTy) {
    let Some(read) = self.body_facts.and_then(|b| b.value_flow.read(receiver.span)) else { return };
    let (source, event) = match &read.effect {
      OperandEffect::Copy(source) => (source, Evt::Copy),
      OperandEffect::Move(source) => (source, Evt::Move),
      OperandEffect::Borrow { .. } => return,
    };
    if let Some(from) = self.transfer_source(source, receiver) {
      self.add_read_transfer(&read, event, to, from);
    }
  }

  fn add_transfer(&mut self, mut site: Site, event: Evt, to: ResourceTy, from: ResourceTy) {
    site.row = self.line_override.unwrap_or(site.row);
    self.transfer_sites.insert(*self.unique_id, site);
    let partial = matches!(event, Evt::Move) && matches!(from, ResourceTy::Index(..));
    self.add_ev(site.row, event, to, from, partial);
  }

  fn add_read_transfer(&mut self, read: &Read, event: Evt, to: ResourceTy, from: ResourceTy) {
    if read.exact {
      self.add_transfer(read.site, event, to, from);
    } else {
      self.add_ev(read.site.row, event, to, from, false);
    }
  }

  fn set_call_result_path(&self, from: &mut ResourceTy, site: Option<Site>, to: &ResourceTy) {
    let ResourceTy::Value(ResourceAccessPoint::Function(f)) = from else { return };
    if !f.callable_result { return; }
    let ResourceTy::Value(binding) = to else { return };
    if binding.is_member() { return; }
    if let Some(path) = self.body_facts.and_then(|b| b.value_flow.stored_result(site?, binding.owner_local()?)) {
      f.result_path = path.to_vec();
    }
  }

  fn add_call_result(&mut self, call: Call, row: usize, event: Evt, to: ResourceTy, mut from: ResourceTy) {
    let outgoing = matches!(to, ResourceTy::Caller) || to.extract_rap().is_some_and(|r| r.is_fn());
    if let ResourceTy::Value(ResourceAccessPoint::Function(f)) = &mut from {
      f.callable_result = call.callable_result;
      if call.callable_result {
        f.result_try = self.body_facts.and_then(|b| b.value_flow.result_used_by_try(call));
      }
    }
    self.set_call_result_path(&mut from, call.site, &to);
    if let ResourceTy::Value(ResourceAccessPoint::Function(f)) = &from {
      if let Some(path) = self.body_facts.and_then(|b| b.value_flow.returned_result(call))
        .filter(|path| matches!(event, Evt::Copy) || (f.callable_result && (!path.is_empty() || f.result_try.is_some())))
      {
        let title = crate::svg_generator::hover_messages::returned_call_value(f, path);
        self.row_layout.returned_call_titles.insert(*self.unique_id, title);
      }
    }
    if let Some(mut site) = call.site.filter(|_| outgoing || matches!(&to, ResourceTy::Value(r) if !r.is_member())) {
      if !outgoing {
        site.row = row;
      }
      self.add_transfer(site, event, to, from);
    } else {
      self.add_ev(row, event, to, from, false);
    }
  }

  /// The exact call produced a value on its normal successor.
  pub fn is_returned_call_result(&self, e: &Expr<'_>) -> bool {
    !e.span.from_expansion()
      && self.mir_call(e).is_some_and(|call|
        call.has_result && !call.try_branch && !call.returns_reference && !call.returns_unit)
  }

  pub(crate) fn return_operand(&mut self, e: &'tcx Expr<'tcx>) {
    let Some(read) = self.body_facts.and_then(|b| b.value_flow.returned_read(e.span)) else { return };
    let (source, event) = match &read.effect {
      OperandEffect::Copy(source) => (source, Evt::Copy),
      OperandEffect::Move(source) => (source, Evt::Move),
      OperandEffect::Borrow { source, mutable, .. } => {
        let Some(from) = self.transfer_source(source, e) else { return };
        let Some(rap) = from.extract_rap().cloned() else { return };
        if !*mutable && source.deref && rap.is_ref() && !rap.is_mutref() && matches!(e.kind, ExprKind::Path(..)) {
          self.add_read_transfer(&read, Evt::Copy, ResourceTy::Caller, ResourceTy::Value(rap));
          return;
        }
        let ty = self.tcx.typeck(e.hir_id.owner).expr_ty_adjusted(e);
        let Some(pointee) = ty.builtin_deref(true) else { return };
        let written_place = match &from {
          ResourceTy::DerefField(_, label, ..) | ResourceTy::Index(_, label, ..) => label.as_str(),
          _ => source.label.as_str(),
        };
        let written_place = written_place.strip_prefix("&mut ").or_else(|| written_place.strip_prefix('&')).unwrap_or(written_place);
        let place = format!("&{}{written_place}", if *mutable { "mut " } else { "" });
        let from = if source.string_data || source.raw_pointer || (source.through.is_none() && !source.static_item) {
          let title = crate::svg_generator::hover_messages::returned_borrow_of(
            &place, source.through.as_deref(), source.string_data, source.string_data_field.then_some(source.label.as_str()),
            source.raw_pointer, *mutable);
          self.row_layout.aggregate_titles.insert(*self.unique_id, title);
          ResourceTy::DerefField(rap, place, PlaceWrite::Update, self.field_holds(pointee, e.hir_id.owner))
        } else if source.through.is_some() {
          let place = source.through_element
            .and_then(|span| self.tcx.sess.source_map().span_to_snippet(span).ok())
            .map_or(place.clone(), |through| crate::svg_generator::hover_messages::reborrow_place_through(&place, &through));
          ResourceTy::DerefField(rap, place, PlaceWrite::Update, self.field_holds(pointee, e.hir_id.owner))
        } else {
          ResourceTy::Index(rap, place, Default::default())
        };
        self.add_read_transfer(&read, Evt::Copy, ResourceTy::Caller, from);
        return;
      }
    };
    if source.through_element.is_some() {
      return;
    }
    if let Some(mut from) = self.transfer_source(source, e) {
      if source.element {
        if let ResourceTy::DerefField(rap, label, ..) = from {
          from = ResourceTy::Index(rap, label, Default::default());
        }
      } else if source.through.is_some() && source.deref {
        if let ResourceTy::Deref(r) = from {
          let ty = self.tcx.typeck(e.hir_id.owner).expr_ty(e);
          from = ResourceTy::DerefField(r, source.label.clone(), PlaceWrite::Update, self.field_holds(ty, e.hir_id.owner));
        }
      }
      self.add_read_transfer(&read, event, ResourceTy::Caller, from);
    }
  }

  /// `*b` with `b` a `Box` variable (`Box<String>`; not a reference):
  /// `Deref(b)`, drawn as `*b` on b's column, as `let s = *b;` is.
  pub fn box_contents_of(&self, e: &'tcx Expr<'tcx>) -> Option<ResourceTy> {
    let ExprKind::Unary(UnOp::Deref, inner) = e.kind else { return None };
    if e.span.from_expansion() || !self.tcx.typeck(inner.hir_id.owner).node_type(inner.hir_id).is_box() {
      return None;
    }
    match fetch_rap(inner, &self.tcx, &self.raps) {
      Some(b) if !b.is_ref() && !b.is_fn() => Some(ResourceTy::Deref(b)),
      _ => None,
    }
  }

  /// The drop of the Box payload on the normal assignment path.
  pub(crate) fn box_payload_drop_at(&self, lhs: &Expr<'_>, box_owner: &ResourceAccessPoint) -> Option<&'static str> {
    use rustc_middle::mir::{ProjectionElem, TerminatorKind};
    let local = box_owner.owner_local()?;
    let mut kinds = self.effects.get(&lhs.span.with_parent(None))?.iter().filter_map(|(_, ev)| {
      let crate::facts::Fact::Drop { how, at_reassign: true, early_exit: None, .. } = ev.fact else { return None };
      if ev.drop_route != crate::facts::DropRoute::Normal { return None; }
      let TerminatorKind::Drop { place, .. } = self.mir_body.basic_blocks[ev.location.block].terminator().kind else { return None };
      (place.local == local && place.projection.as_slice() == [ProjectionElem::Deref]).then_some(how)
    });
    let how = kinds.next()?;
    kinds.all(|other| other == how).then_some(how)
  }

  /// The outgoing value of an assignment drops after its RHS has finished.
  /// rustc attributes this terminator to the LHS, even for a multiline RHS.
  pub(crate) fn reassign_drop_at(&self, lhs: &Expr<'_>, target: &ResourceAccessPoint) -> Option<(usize, bool)> {
    let mut drops = self.effects.get(&lhs.span.with_parent(None))?.iter().filter(|(_, e)|
      matches!(&e.fact, crate::facts::Fact::Drop { path, at_reassign: true, .. } if path == target.name()));
    let &(row, e) = drops.next()?;
    if drops.next().is_some() { return None; }
    Some((row, !self.body_facts.is_some_and(|b| b.skipped_closure_returns.contains_key(&e.location.block))))
  }

  pub(crate) fn unfinished_vec(&self, expr: &'tcx Expr<'tcx>) -> bool {
    crate::facts::is_vec_expansion(self.tcx, expr.span.ctxt().outer_expn())
      && vec_macro_elems(self.tcx, expr).is_some()
      && self.body_facts.is_some_and(|b| !b.constructed_vecs.contains_key(&expr.span.ctxt().outer_expn()))
  }

  pub(crate) fn vec_element_reached(&self, elem: &Expr<'_>, expansion: rustc_span::hygiene::ExpnId) -> bool {
    let site = elem.span.source_callsite();
    self.body_facts.is_none_or(|facts| facts.constructed_vecs.contains_key(&expansion)
      || facts.evaluated_spans.iter().any(|span| site.contains(span.source_callsite())))
  }

  /// The actual transfer for an element that names a column. A temp is all
  /// MIR proves for an element evaluated before a diverging sibling.
  pub(crate) fn draw_vec_place(&mut self, elem: &'tcx Expr<'tcx>, receiver: Option<&ResourceTy>) -> bool {
    let Some(Transfer { row, kind, path, to: dest, location }) = self.transfer_at(elem) else { return false };
    // Staged reads are emitted while visiting the arguments, before any
    // later argument can reinitialize the source or leave the expression.
    if (dest == "<temp>") != receiver.is_none() { return false; }
    let receiver = receiver.unwrap_or(&ResourceTy::Anonymous);
    let src = self.raps.get(path).map(|r| ResourceTy::Value(r.rap.clone()))
      .or_else(|| self.box_contents_of(elem).filter(|r| r.name() == path));
    let Some(src) = src else { return false };
    // A reference's loan traveling with the vector has its own borrow
    // events. A move of the reference still consumes its column.
    if src.is_ref() && matches!(kind, Evt::Copy) && dest != "<return>" { return false; }
    let to = match receiver {
      _ if dest == "<return>" => ResourceTy::Caller,
      ResourceTy::Value(r) if dest == r.name() || (r.is_fn() && dest.strip_suffix("()") == Some(r.name())) => receiver.clone(),
      _ => ResourceTy::Anonymous,
    };
    // add_ev propagates moves to fields for anonymous, function and
    // caller destinations. A named vector has no columns for its elements.
    let moved_whole = matches!(kind, Evt::Move) && matches!(src, ResourceTy::Value(_))
      && matches!(&to, ResourceTy::Value(r) if !r.is_fn() && !r.is_temporary() && !r.is_closure());
    if let Some(rap) = src.extract_rap() { self.update_rap(&rap, row); }
    if let Some((b, call)) = self.body_facts.and_then(|b| b.skipped_closure_returns.get(&location.block).map(|call| (b, *call))) {
      let endpoints: Vec<_> = [src.extract_rap(), to.extract_rap()].into_iter().flatten().collect();
      let mut scope_ends = Vec::new();
      let mut unwind_drop = None;
      for r in &b.rows {
        for ev in &r.facts {
          let crate::facts::Fact::Drop { path, how, at_reassign: false, early_exit: None, .. } = &ev.fact else { continue };
          if ev.drop_route == crate::facts::DropRoute::Unwind {
            if Some(ev.location) == call && path == &src.name() { unwind_drop = Some(*how); }
            continue;
          }
          if let Some(rap) = endpoints.iter().find(|rap| rap.name() == path) {
            if b.skipped_closure_returns.contains_key(&ev.location.block) {
              scope_ends.push((*rap.hash(), r.line));
            }
          }
        }
      }
      self.approx_notes.push(ApproxNote::UnreachedTransfer { id: *self.unique_id, scope_ends, unwind_drop });
    }
    let staged = dest == "<temp>";
    let drops_on_exit = staged && self.body_facts.is_some_and(|b|
      crate::facts::dropped_on_residual(self.tcx, self.mir_body, b, location));
    let mut expansions = self.tcx.hir_parent_iter(elem.hir_id).filter_map(|(_, n)| match n {
      rustc_hir::Node::Expr(e) => Some(e),
      _ => None,
    }).take_while(|e| crate::facts::is_vec_expansion(self.tcx, e.span.ctxt().outer_expn()));
    let first = expansions.next();
    let repeat = first.is_some_and(|e| matches!(e.kind, ExprKind::Call(_, args) if args.first().is_some_and(|a| a.hir_id == elem.hir_id)));
    let nested = first.is_some_and(|inner| expansions.any(|outer| inner.span.ctxt() != outer.span.ctxt()));
    self.approx_notes.push(ApproxNote::VecElement { id: *self.unique_id, staged, drops_on_exit, repeat, nested });
    self.add_ev(row, kind, to.clone(), src, false);
    if moved_whole {
      self.move_struct_fields_away(row, path, path);
    }
    !matches!(to, ResourceTy::Anonymous)
  }

  pub fn match_array_elems(&mut self, lhs: ResourceTy, elems: &'tcx [Expr<'tcx>], line_num: usize) {
    self.match_literal_parts(lhs, elems.iter().collect(), line_num);
  }

  /// The parts of a literal going into `lhs`, a place with no column per
  /// part: the elements of an array literal or `vec![..]`, or a struct
  /// literal written to `v[i]` / `*r` (#1143). See `match_array_elems`.
  pub fn match_literal_parts(&mut self, lhs: ResourceTy, parts: Vec<&'tcx Expr<'tcx>>, line_num: usize) {
    self.draw_literal_parts(lhs, parts, line_num, false);
  }

  fn draw_literal_parts(&mut self, lhs: ResourceTy, parts: Vec<&'tcx Expr<'tcx>>, line_num: usize, vec_elements: bool) {
    let mut drew_source = false;
    // An element that is itself a literal (`vec![B { name: r }]`,
    // `[(r, 1)]`, `vec![Some(r)]`): what it is built from goes into
    // `lhs` the same way (#1143).
    let mut work: Vec<&'tcx Expr<'tcx>> = parts.into_iter().rev().collect();
    while let Some(elem) = work.pop() {
      if vec_elements {
        if let Some(inner) = vec_macro_elems(self.tcx, elem) {
          work.extend(inner.iter().rev());
          continue;
        }
        if matches!(elem.kind, ExprKind::Path(_) | ExprKind::Field(..)) || self.box_contents_of(elem).is_some() {
          drew_source |= self.draw_vec_place(elem, Some(&lhs));
          continue;
        }
      }
      if !elem.span.from_expansion() {
        match elem.kind {
          ExprKind::Struct(_, fields, tail) => {
            work.extend(fields.iter().rev().map(|f| f.expr));
            if let rustc_hir::StructTailExpr::Base(base) = tail {
              if !matches!(lhs, ResourceTy::Anonymous) {
                let before = *self.unique_id;
                self.move_base_rest(lhs.clone(), base, fields);
                drew_source |= *self.unique_id != before;
              }
            }
            continue;
          }
          ExprKind::Tup(parts) | ExprKind::Array(parts) => {
            work.extend(parts.iter().rev());
            continue;
          }
          ExprKind::Call(callee, _) if !matches!(lhs, ResourceTy::Anonymous)
            && crate::visitor::is_ctor_path(callee, self.tcx.typeck(elem.hir_id.owner)) => {
            drew_source |= self.retarget_ctor_arg_events(elem, lhs.clone());
            continue;
          }
          _ => {}
        }
      }
      let name = match elem.kind {
        ExprKind::Path(_) | ExprKind::Field(..) if !elem.span.from_expansion() => {
          expr_to_rap_name(elem, &self.tcx).filter(|n| self.raps.contains_key(n))
        }
        // `[*b]`: the Box's contents move into the array (#1191).
        _ => self.box_contents_of(elem).and_then(|b| b.extract_rap().map(|r| r.name().to_owned())),
      };
      let ty = self.tcx.typeck(elem.hir_id.owner).node_type(elem.hir_id);
      let stored_callable_result = self.body_facts.and_then(|b| {
        let call = self.mir_call(elem).filter(|c| c.callable_result && c.has_result && !c.returns_reference && !c.returns_unit)?;
        b.value_flow.stored_result(call.site?, lhs.extract_rap()?.owner_local()?)
      }).is_some_and(|path| !path.is_empty());
      if name.is_none() && (stored_callable_result
        || matches!(lhs, ResourceTy::Caller) && self.is_returned_call_result(elem)) {
        let e = if self.ty_is_copy(ty, elem.hir_id.owner) { Evt::Copy } else { Evt::Move };
        self.match_rhs(lhs.clone(), elem, e);
        drew_source = true;
        continue;
      }
      // `[x, y]` returned with x, y references (#1340).
      if ty.is_ref() && matches!(lhs, ResourceTy::Caller) {
        drew_source |= self.ref_part_to(elem, ResourceTy::Caller);
        continue;
      }
      let Some(name) = name else { continue };
      if ty.is_ref() {
        // The holder's borrow is drawn from the lender; a `&mut`
        // moved in (`vec![r]`, no reborrow) can't be used afterwards,
        // and the holder carries its borrow on (#1128).
        let typeck = self.tcx.typeck(elem.hir_id.owner);
        if ty.ref_mutability() == Some(Mutability::Mut) && !is_implicit_reborrow(elem, typeck)
          && !matches!(lhs, ResourceTy::Anonymous | ResourceTy::Caller) {
          if let ResourceTy::Value(r) = get_rap(elem, &self.tcx, &self.raps) {
            if r.is_mutref() {
              self.add_ev(line_num, Evt::Move, lhs.clone(), ResourceTy::Value(r), false);
              drew_source = true;
            }
          }
        }
        continue;
      }
      let is_copy = self.ty_is_copy(ty, elem.hir_id.owner);
      let e = if is_copy { Evt::Copy } else { Evt::Move };
      self.match_rhs(lhs.clone(), elem, e);
      drew_source = true;
      // A struct moved into the array takes its fields with it.
      if !is_copy && self.box_contents_of(elem).is_none() {
        let mut stack = self.struct_children(&name);
        while let Some(child) = stack.pop() {
          stack.extend(self.struct_children(child.name()));
          if !child.never_drops() {
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
  /// What a pattern's bindings are drawn as taken from: the scrutinee's
  /// column, or nothing for a constructor of literals (`Some(5)`), which
  /// has none (#768).
  pub fn scrutinee_rap(&self, expr: &'tcx Expr<'tcx>) -> ResourceTy {
    if self.is_copy_literal_value(expr) {
      return ResourceTy::Anonymous;
    }
    self.rap_or_field_through_ref(expr)
  }

  pub fn rap_or_field_through_ref(&self, expr: &'tcx Expr<'tcx>) -> ResourceTy {
    if let Some(tmp) = self.part_temp_of(expr) {
      return ResourceTy::Value(tmp);
    }
    match get_rap(expr, &self.tcx, &self.raps) {
      ResourceTy::Anonymous => self.field_access_through_ref(expr).unwrap_or(ResourceTy::Anonymous),
      ResourceTy::Value(ResourceAccessPoint::Function(mut f)) => {
        f.mir_result = self.mir_call(expr).is_some_and(|call| call.has_result);
        f.mir_constructor = self.mir_constructor(expr);
        ResourceTy::Value(ResourceAccessPoint::Function(f))
      }
      rty => rty,
    }
  }

  /// `rap_or_field_through_ref` for an expression that borrows
  /// (`&mut people[0].age`, a call argument or a reference's
  /// initializer), also naming the variable whose value holds a place
  /// that has no column (`borrowed_part_holder`, #1176). Not used for
  /// patterns' scrutinees, whose bindings are drawn as copies of it.
  pub fn borrow_source(&self, expr: &'tcx Expr<'tcx>) -> ResourceTy {
    match self.rap_or_field_through_ref(expr) {
      ResourceTy::Anonymous => self.borrowed_part_holder(expr).map_or(ResourceTy::Anonymous, ResourceTy::Value),
      rty => rty,
    }
  }

  /// `&mut people[0].age` / `&b.name` (b a Box): the borrow is of part
  /// of a variable's value that has no column; drawn on the variable's
  /// (`place_holder`, #1176).
  pub fn borrowed_part_holder(&self, expr: &'tcx Expr<'tcx>) -> Option<ResourceAccessPoint> {
    let ExprKind::AddrOf(_, _, place) = expr.kind else { return None };
    self.place_holder(place)
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
  /// `lhs_expr` is `r.f` / `r.inner.f` / `(*r).f` written through the
  /// reference `r`: the field's own column when `r` points into a local
  /// struct that has one (`s.f`, #854). None for a parameter's struct,
  /// which belongs to the caller and has no column here.
  pub fn field_column_behind_ref(&self, lhs_expr: &'tcx Expr<'tcx>, r: &ResourceAccessPoint) -> Option<ResourceAccessPoint> {
    expr_to_rap_name(lhs_expr, &self.tcx)
      .and_then(|full| full.strip_prefix(r.name().as_str()).map(str::to_owned))
      .filter(|sfx| sfx.starts_with('.'))
      .and_then(|sfx| self.owner_behind_ref(r, &sfx))
  }

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
      ResourceTy::DerefField(q, place, _, _) => {
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
  pub(crate) fn enter_deferred(&mut self, loop_id: rustc_hir::HirId) -> (Option<usize>, Vec<String>) {
    match self.deferred_lines.iter().position(|(id, ..)| *id == loop_id) {
      Some(i) => {
        let (_, line, names) = self.deferred_lines.remove(i);
        (self.line_override.replace(line), std::mem::replace(&mut self.break_out, names))
      }
      None => (self.line_override, self.break_out.clone()),
    }
  }

  pub(crate) fn leave_deferred(&mut self, saved: (Option<usize>, Vec<String>)) {
    self.line_override = saved.0;
    self.break_out = saved.1;
  }

  /// The `break` values recorded for `loop_expr` while its receiver
  /// wasn't known (#452), with each `break` as written.
  /// The values that can end the labeled block `e` (#1134): what each
  /// `break` to it carries, in source order, then its last expression
  /// (if it has one and can reach it), each with its text for a tooltip.
  /// The `break`s were visited with the block; their values, waiting for
  /// a receiver (`loop_breaks`), are taken here.
  pub(crate) fn labeled_block_values(&mut self, e: &'tcx Expr<'tcx>) -> Vec<(&'tcx Expr<'tcx>, Option<String>)> {
    let ExprKind::Block(block, Some(_)) = e.kind else { return Vec::new() };
    let breaks: Vec<_> = crate::expr_visitor_utils::breaks_to_id(e, block.hir_id);
    if !breaks.iter().any(|(_, v)| v.is_some()) {
      return Vec::new();
    }
    self.loop_breaks.retain(|(id, ..)| *id != block.hir_id);
    let mut out: Vec<(&'tcx Expr<'tcx>, Option<String>)> = breaks.into_iter()
      .filter_map(|(brk, v)| v.map(|v| (v, self.break_text(brk))))
      .collect();
    if let Some(tail) = block.expr {
      if !self.expr_diverges(tail) {
        let text = (!tail.span.from_expansion()).then(|| self.tcx.sess.source_map().span_to_snippet(tail.span).ok()).flatten()
          .filter(|t| t.chars().count() <= 40 && !t.contains('\n'));
        out.push((tail, text));
      }
    }
    out
  }

  pub(crate) fn take_loop_breaks(&mut self, loop_expr: &'tcx Expr<'tcx>) -> Vec<(&'tcx Expr<'tcx>, Option<String>)> {
    let (mine, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.loop_breaks)
      .into_iter().partition(|(id, ..)| *id == loop_expr.hir_id);
    self.loop_breaks = rest;
    mine.into_iter().map(|(_, v, t)| (v, t)).collect()
  }

  /// Whether `e` mentions a local variable declared inside `scope`.
  pub(crate) fn mentions_local_declared_in(&self, e: &'tcx Expr<'tcx>, scope: rustc_span::Span) -> bool {
    locals_in(e, ClosureLocals::Skip).iter().any(|m| scope.contains(self.tcx.hir_span(m.id)))
  }

  fn redirect_temporary_operand(&mut self, temp: &ResourceAccessPoint, from: &ResourceTy, to: &ResourceTy, read: &Read) -> bool {
    let target = ResourceTy::Value(temp.clone());
    let mut candidates = self.preprocessed_events.iter_mut().filter_map(|(row, e)| {
      match e {
        ExternalEvent::Move { from: source, to, id, .. } if *source == *from && *to == target => Some((*row, to, *id)),
        _ => None,
      }
    });
    let Some((row, dest, id)) = candidates.next().filter(|_| candidates.next().is_none()) else { return false };
    *dest = to.clone();
    if let Some(arrow) = self.event_line_map.get_mut(&row).into_iter().flatten().find(|event| event.get_id() == id) {
      if let ExternalEvent::Move { to: dest, .. } = arrow {
        *dest = to.clone();
      }
    }
    if read.exact {
      let mut site = read.site;
      site.row = row;
      self.transfer_sites.insert(id, site);
    }
    // The direct arrow leaves out the intermediate aggregate. Preserve
    // the original transfer on the temporary's tooltip.
    self.row_layout.temporary_staged_moves.entry(*temp.hash()).or_default().insert(from.name());
    true
  }

  fn call_slots(&self, mut expr: &'tcx Expr<'tcx>) -> CallSlots {
    let parent = loop {
      use rustc_hir::Node;
      let parent = match self.tcx.parent_hir_node(expr.hir_id) {
        Node::Expr(parent) => parent,
        Node::Block(block) if block.expr.is_some_and(|tail| tail.hir_id == expr.hir_id) => {
          let Node::Expr(parent) = self.tcx.parent_hir_node(block.hir_id) else { return CallSlots::default() };
          expr = parent;
          continue;
        }
        Node::ExprField(field) if field.expr.hir_id == expr.hir_id => {
          let Node::Expr(parent) = self.tcx.parent_hir_node(field.hir_id) else { return CallSlots::default() };
          expr = parent;
          continue;
        }
        Node::Arm(arm) if arm.body.hir_id == expr.hir_id => {
          let Node::Expr(parent) = self.tcx.parent_hir_node(arm.hir_id) else { return CallSlots::default() };
          expr = parent;
          continue;
        }
        _ => return CallSlots::default(),
      };
      match parent.kind {
        ExprKind::DropTemps(_) | ExprKind::Cast(..) | ExprKind::Tup(_) | ExprKind::Array(_) => expr = parent,
        ExprKind::If(_, yes, no) if yes.hir_id == expr.hir_id || no.is_some_and(|no| no.hir_id == expr.hir_id) => expr = parent,
        ExprKind::Break(destination, _) => {
          let Some(target) = destination.target_id.ok() else { return CallSlots::default() };
          let rustc_hir::Node::Expr(target) = self.tcx.hir_node(target) else { return CallSlots::default() };
          expr = target;
        }
        _ => break parent,
      }
    };
    let index = match parent.kind {
      ExprKind::Call(_, args) => args.iter().position(|arg| arg.hir_id == expr.hir_id),
      ExprKind::MethodCall(_, receiver, args, _) => std::iter::once(receiver).chain(args)
        .position(|arg| arg.hir_id == expr.hir_id),
      ExprKind::Binary(_, left, right) | ExprKind::AssignOp(_, left, right) => [left, right].iter().position(|arg| arg.hir_id == expr.hir_id),
      ExprKind::Unary(_, operand) if operand.hir_id == expr.hir_id => Some(0),
      _ => None,
    };
    let Some(index) = index else { return CallSlots::default() };
    let callee = self.mir_callee(parent);
    let trait_id = callee.and_then(|callee| self.tcx.trait_of_assoc(callee));
    let lang = self.tcx.lang_items();
    let callable = trait_id.is_some() && [lang.fn_trait(), lang.fn_mut_trait(), lang.fn_once_trait()].contains(&trait_id);
    CallSlots {
      span: Some(parent.span),
      callee,
      index,
      constructor: self.mir_argument_constructor(parent),
      projected_literal: None,
      callable,
    }
  }

  pub fn match_arg(&mut self, arg: &'tcx Expr, fn_name: String) {
    let slots = self.call_slots(arg);
    self.match_arg_at(arg, fn_name, false, slots);
  }

  pub(crate) fn draw_macro_argument(&mut self, arg: &'tcx Expr, name: String) {
    self.match_arg_at(arg, name, true, CallSlots::default());
  }

  pub(crate) fn match_call_arg(&mut self, arg: &'tcx Expr, fn_name: String, manually_takes: bool) {
    let start = self.preprocessed_events.len();
    self.match_arg(arg, fn_name);
    if manually_takes {
      for (_, event) in &mut self.preprocessed_events[start..] {
        if let ExternalEvent::PassByMutableReference { manually_takes, .. } = event {
          *manually_takes = true;
        }
      }
    }
  }

  fn match_arg_at(&mut self, arg: &'tcx Expr, fn_name: String, in_macro: bool, slots: CallSlots) {
    let outer = self.emitting_expr.replace(arg);
    self.match_arg_effects(arg, fn_name, in_macro, slots);
    self.emitting_expr = outer;
  }

  fn match_arg_effects(&mut self, arg: &'tcx Expr, fn_name: String, in_macro: bool, slots: CallSlots) {
    // `sum(&arr as &[i32])` passes the borrow `&arr` (#1175).
    let arg = peel_ref_casts(arg, self.tcx.typeck(arg.hir_id.owner));
    // `f('a: { if c { break 'a s; } t })` (#1134): the argument is what
    // ended the block, a `break`'s value or its last expression.
    if matches!(arg.kind, ExprKind::Block(_, Some(_))) {
      let values = self.labeled_block_values(arg);
      if values.len() > 1 {
        self.pair_break_alternatives(arg, values, |this, v| this.match_arg_at(v, fn_name.clone(), in_macro, slots));
        return;
      }
    }
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
        self.pair_break_alternatives(arg, seen, |this, v| this.match_arg_at(v, fn_name.clone(), in_macro, slots));
      } else {
        for (value, _) in seen {
          self.match_arg_at(value, fn_name.clone(), in_macro, slots);
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
        // `Ok((x, 2))`: the constructor doesn't read x; x's reference
        // goes where the built value goes (#1340).
        let into_ctor = self.enclosing_ctor_name(arg).is_some_and(|c| c == fn_name);
        let typeck = self.tcx.typeck(arg.hir_id.owner);
        for elem in elems.iter() {
          if into_ctor && typeck.node_type(elem.hir_id).is_ref() {
            self.ref_part_into_ctor(arg, elem, &fn_name);
            continue;
          }
          self.match_arg_at(elem, fn_name.clone(), in_macro, slots);
        }
        return;
      }
    }
    // A struct literal built in the argument list (`consume(B { name: r })`,
    // `v.push(B { name })`, `Box::new(P { .. })`, `Some(B { name: r })`):
    // like a tuple's elements, each field's value goes into the call
    // with it, so `r` is moved (or copied, or borrowed) into the call
    // (#1143). With `..base`, base's fields that aren't written out go
    // into the call too, a partial move of `base`.
    if let ExprKind::Struct(_, fields, tail) = arg.kind {
      if !arg.span.from_expansion() {
        self.add_fn(fn_name.clone());
        // `Some(W { a: &s })`: the constructor isn't a borrower; the
        // holder of the built value is (#768).
        let into_ctor = self.enclosing_ctor_name(arg).is_some_and(|c| c == fn_name);
        let typeck = self.tcx.typeck(arg.hir_id.owner);
        for field in fields.iter() {
          if into_ctor && typeck.node_type(field.expr.hir_id).is_ref() {
            // `Some(Part { text: x })` returned: x's reference goes
            // with the value (#1340).
            self.ref_part_into_ctor(arg, field.expr, &fn_name);
            continue;
          }
          self.match_arg_at(field.expr, fn_name.clone(), in_macro, slots);
        }
        if let rustc_hir::StructTailExpr::Base(base) = tail {
          let to = ResourceTy::Value(self.raps.get(&fn_key(&fn_name)).unwrap().rap.to_owned());
          self.move_base_rest(to, base, fields);
        }
        return;
      }
    }
    // `take(Some(&s))`: the constructor isn't an actor. A borrow it
    // wraps is made for the call the value is passed to (#768); the
    // other arguments were moved or copied into the constructor.
    if let ExprKind::Call(_, ctor_args) = arg.kind {
      let typeck = self.tcx.typeck(arg.hir_id.owner);
      if !arg.span.from_expansion() && self.mir_argument_constructor(arg) {
        let mut borrowed = false;
        for a in ctor_args.iter() {
          let nested = self.mir_argument_constructor(a);
          // `take(Some((x, 1)))` / `take(Some(Part { text: &s }))`: the
          // references in the literal are passed to the call with it
          // (the constructor skipped them, #1340).
          let refs = literal_ref_parts(a, typeck);
          if !refs.is_empty() {
            borrowed = true;
            for r in refs {
              self.match_arg_at(r, fn_name.clone(), in_macro, slots);
            }
          }
          if typeck.node_type(a.hir_id).is_ref() {
            borrowed = true;
            self.match_arg_at(a, fn_name.clone(), in_macro, slots);
          } else if nested {
            let before = *self.unique_id;
            self.match_arg_at(a, fn_name.clone(), in_macro, slots);
            borrowed |= *self.unique_id != before;
          }
        }
        // A value moved or copied into the constructor goes into the
        // call, not out of the constructor.
        let fn_rap = self.raps.get(&fn_key(&fn_name)).map(|rd| ResourceTy::Value(rd.rap.to_owned()));
        let moved = fn_rap.is_some_and(|to| self.retarget_ctor_arg_events(arg, to));
        if borrowed || moved || self.is_copy_literal_value(arg) {
          return;
        }
      }
    }
    // An array (or `vec![…]`) built in the argument list
    // (`f([s, t])`, `f(vec![s])`): each element is passed into the
    // call with it, so `s` is moved into the call (#297).
    let built_elems = match arg.kind {
      ExprKind::Array(elems) if !arg.span.from_expansion() => Some(elems),
      _ => vec_macro_elems(self.tcx, arg),
    };
    if let Some(elems) = built_elems {
      self.add_fn(fn_name.clone());
      for elem in elems.iter() {
        // `take([&s])`: the borrow is made for the call too (#705).
        let is_var = matches!(elem.kind, ExprKind::Path(_) | ExprKind::Field(..) | ExprKind::AddrOf(..));
        // `take(vec![B { name: r }])`, `take([(r, 1)])`: what the
        // literal element is built from goes into the call (#1143).
        let is_literal = matches!(elem.kind, ExprKind::Struct(..) | ExprKind::Tup(_)) && !elem.span.from_expansion();
        if arg.span.from_expansion() && !self.vec_element_reached(elem, arg.span.ctxt().outer_expn()) { continue; }
        if vec_macro_elems(self.tcx, elem).is_some() {
          self.match_arg_at(elem, fn_name.clone(), in_macro || arg.span.from_expansion(), slots);
        } else if arg.span.from_expansion() && (matches!(elem.kind, ExprKind::Path(_) | ExprKind::Field(..)) || self.box_contents_of(elem).is_some())
            && !(self.tcx.typeck(elem.hir_id.owner).expr_ty(elem).is_ref()
              && self.body_facts.is_some_and(|b| b.constructed_vecs.contains_key(&arg.span.ctxt().outer_expn()))
              && self.transfer_at(elem).is_none_or(|t| !matches!(t.kind, Evt::Move))) {
          let to = ResourceTy::Value(self.raps[&fn_key(&fn_name)].rap.clone());
          self.draw_vec_place(elem, Some(&to));
        } else if is_var || is_literal {
          self.match_arg_at(elem, fn_name.clone(), in_macro || arg.span.from_expansion(), slots);
        }
      }
      return;
    }
    self.add_fn(fn_name.clone());
    if let ExprKind::Closure(closure) = arg.kind {
      let to_ro = ResourceTy::Value(self.raps.get(&fn_key(&fn_name)).unwrap().rap.to_owned());
      self.match_closure_arg(closure, arg, &to_ro, &fn_name);
      return;
    }
    let try_input = try_operand(arg);
    if let Some(call) = self.mir_call(try_input.unwrap_or(arg)).filter(|c| c.has_result
      && self.body_facts.is_some_and(|b| {
        if try_input.is_some() {
          c.callable_result && b.value_flow.try_result_passes_to(*c, slots)
        } else {
          !c.returns_reference && !c.returns_unit && b.value_flow.result_passes_to(*c, slots)
        }
      })) {
      let to = ResourceTy::Value(self.raps[&fn_key(&fn_name)].rap.clone());
      if !slots.constructor {
        self.row_layout.argument_results.insert(*self.unique_id);
      }
      let copy = if try_input.is_some() {
        self.ty_is_copy(self.tcx.typeck(arg.hir_id.owner).node_type(arg.hir_id), arg.hir_id.owner)
      } else {
        call.copyable
      };
      self.match_rhs(to, arg, if copy { Evt::Copy } else { Evt::Move });
      return;
    }
    let tycheck_results = self.tcx.typeck(arg.hir_id.owner);
    let arg_ty = tycheck_results.node_type(arg.hir_id);
    let mut to_ro = ResourceTy::Value(self.raps[&fn_key(&fn_name)].rap.clone());
    let value_flow = self.body_facts.map(|b| &b.value_flow);
    let read = value_flow.and_then(|flow| {
      if in_macro { flow.read(arg.span) } else { flow.argument_read(arg.span, slots) }
    });
    if read.is_none() && !in_macro && slots.projected_literal.is_none() {
      if let Some(read) = value_flow.and_then(|flow| flow.read(arg.span)) {
        if let OperandEffect::Move(src) = &read.effect {
          if let Some(from) = self.transfer_source(src, arg).filter(|from| !matches!(from, ResourceTy::Index(..))) {
            self.add_read_transfer(&read, Evt::Move, ResourceTy::Anonymous, from);
            return;
          }
        }
      }
    }
    // A projected temporary can hide the places that built it. Visit
    // those operands for display; MIR selects which reached this slot.
    if let ExprKind::Field(base, _) = arg.kind {
      if read.is_none() && matches!(base.kind, ExprKind::Tup(_) | ExprKind::Struct(..)) && slots.span.is_some() {
        let slots = CallSlots { projected_literal: Some(base.hir_id), ..slots };
        self.match_arg_at(base, fn_name, in_macro, slots);
        return;
      }
    }
    let projected_loan = self.argument_loan(arg.span).and_then(|loan| {
      let place = loan.projected.as_ref()?;
      let local = loan.lender_local?;
      let rap = self.raps.place(local, &place.base)?.rap.clone();
      Some((loan.line, ResourceTy::Value(rap), loan.kind == "mut", loan.through.is_some()))
    });
    let (line_num, from_ro, arg_mut, of_ref_var, is_reborrow) = match read {
      Some(read) => match &read.effect {
        OperandEffect::Copy(src) | OperandEffect::Move(src) => {
          let Some(from) = self.transfer_source(src, arg) else { return };
          let line = read.site.row;
          let moving = matches!(read.effect, OperandEffect::Move(_));
          // A field without a drawn column still moves in the table.
          // An enclosing column cannot represent that move.
          if moving && matches!(from, ResourceTy::Index(..)) {
            return;
          }
          if moving && src.needs_drop && value_flow.is_some_and(|flow| flow.drops_argument(&read, slots))
          {
            if let ResourceTy::Value(r) = &to_ro {
              to_ro = ResourceTy::Value(r.with_drops_arg());
            }
          }
          if let Some(r) = from.extract_rap() {
            self.update_rap(r, line);
          }
          if let Some(slot) = value_flow.and_then(|flow| flow.unique_argument(&read, slots)) {
            self.argument_slots.insert(*self.unique_id, slot);
          }
          if moving {
            if let Some(temp) = slots.projected_literal.and_then(|id| self.temp_cols.get(&id)) {
              let temp = temp.clone();
              if self.redirect_temporary_operand(&temp, &from, &to_ro, &read) {
                return;
              }
            }
          }
          if !moving && src.is_ref && value_flow.is_some_and(|flow| flow.reads_argument_referent(&read, slots)) {
            let title = crate::svg_generator::hover_messages::copied_operator_reference(&from.name(), &fn_name);
            self.row_layout.aggregate_titles.insert(*self.unique_id, title);
          }
          self.add_read_transfer(&read, if moving { Evt::Move } else { Evt::Copy }, to_ro, from);
          // The ordinary replay of a short-circuit RHS doesn't see
          // through the parent's move. End each reference field here
          // even if it could be copied out of the holder on its own.
          if moving {
            if let Some(transfer) = self.transfer_at(arg).filter(|t| matches!(t.kind, Evt::Move)) {
              if let Some(moved) = self.body_facts.and_then(|b| b.drop_moves.get(&transfer.location)) {
                for field in &moved.fields {
                  if let Some(rd) = self.raps.place(moved.local, field).filter(|r| r.rap.is_ref()) {
                    self.add_ev(transfer.row, Evt::Move, ResourceTy::Anonymous, ResourceTy::Value(rd.rap.clone()), false);
                  }
                }
              }
            }
          }
          return;
        }
        OperandEffect::Borrow { source, mutable, of_ref_slot } => {
          let Some(mut from) = self.transfer_source(source, arg) else { return };
          // The whole reborrow uses the reference's column. Projected
          // reborrows retain the field name on the same column.
          from = match from {
            ResourceTy::Deref(r) | ResourceTy::Index(r, ..) => ResourceTy::Value(r),
            ResourceTy::DerefField(r, ..) if source.element => ResourceTy::Value(r),
            other => other,
          };
          let (from, mutable, through) = projected_loan.as_ref().map_or(
            (from, *mutable, source.through.is_some()),
            |(_, place, mutable, through)| (place.clone(), *mutable, *through));
          (read.site.row, from, if mutable { Mutability::Mut } else { Mutability::Not }, *of_ref_slot, through)
        }
      },
      // Returning a reference from a call is separate from issuing a
      // loan at this argument. Keep the existing call-relationship
      // explanation when the MIR proves the nested call produced it.
      None if projected_loan.is_some() => {
        let (line, from, mutable, through) = projected_loan.unwrap();
        (line, from, if mutable { Mutability::Mut } else { Mutability::Not }, false, through)
      }
      None if arg_ty.is_ref() && self.mir_call(arg).is_some_and(|c| c.has_result) =>
        (expr_to_line(arg, &self.tcx), self.borrow_source(arg), arg_ty.ref_mutability().unwrap(), false, false),
      None => return,
    };
    if let Some(rap) = from_ro.extract_rap() {
      self.update_rap(rap, line_num);
    }
    {
      let argument_loan = self.argument_loan(arg.span);
      let projected = argument_loan.and_then(|loan| Some((loan.loan, loan.projected.clone()?)));
      // `point(&mut hold, &t)` where point's signature lets it store
      // `&t` in `hold`: t stays borrowed past the call (#513).
      // `store(&mut r, b.as_str())`: the reference a call returns,
      // into b, may be stored in r; b stays borrowed while r is (#1124).
      let stored = self.argument_stored_borrows(arg, &from_ro, line_num, argument_loan.map(|loan| loan.loan));
      for (from_ro, place, end_line, loan) in stored.iter().cloned() {
        let mutable = arg_mut == Mutability::Mut;
        let id = *self.unique_id;
        let loans = vec![loan];
        self.stored_borrows.push(StoredBorrow {
          holder: place.name().to_owned(), at: line_num, until: end_line, lender: from_ro.real_name(), is_mut: mutable, id, loans,
        });
        if of_ref_var {
          self.stored_ref_var_lends.insert((place.name().to_owned(), from_ro.real_name()));
        }
        let stored_in = StoredIn(Some(place.name().to_owned()), Vec::new(), Vec::new(), false, Vec::new(), false, Vec::new());
        self.add_external_event(line_num, if mutable {
          ExternalEvent::PassByMutableReference {
            manually_takes: false, terminal: None, from: from_ro.clone(), to: to_ro.clone(), id, is_reborrow: false, ends_at_call: false,
            implicit: ImplicitRef::No, result_keeps_borrow: false, stored_in, break_alt: None, of_ref_var, deep: None,
          }
        } else {
          ExternalEvent::PassByStaticReference {
            from: from_ro.clone(), to: to_ro.clone(), id, implicit: ImplicitRef::No, refcell_borrow_mut: false, pointee_interior_mut: false, result_keeps_borrow: false, stored_in, break_alt: None, still_borrowed_by: Vec::new(), of_ref_var, deep: None,
          }
        });
        *self.unique_id += 1;
        // `store_s(&mut r, &b)`: the place is the reference variable r
        // (`&mut &'a String`), which may now point to b: draw the
        // borrow into r, and its end as an arrow back from r, as for
        // `r = &b;` (#1353). (A place that isn't a reference, `keep(&mut
        // v, &b)`, keeps the borrow on b's column only.)
        let ref_holder = match &place {
          ResourceTy::Index(p, _, _) if p.is_ref() && self.borrow_map.contains_key(p.name()) => Some(p.clone()),
          _ => None,
        };
        let place = match &ref_holder {
          Some(p) => {
            let via = BorrowVia {
              func: fn_name.clone(),
              lenders: vec![from_ro.name()],
              also_unnamed: false,
              tie: ArgTie::default(),
              carrier: ViaCarrier::StoredThrough,
              passed_on: Vec::new(),
            };
            let (from, to) = (from_ro.clone(), ResourceTy::Value(p.clone()));
            // (Its id comes after the borrow end's: that one is the
            // event after the call's, see `stored_borrows`.)
            let id = *self.unique_id + 1;
            let ev = if mutable {
              ExternalEvent::MutableBorrow { from, to, is_partial: false, is_reborrow: false, break_alt: None, via: Some(via), id, of_ref_var }
            } else {
              ExternalEvent::StaticBorrow { from, to, is_partial: false, is_reborrow: false, break_alt: None, via: Some(via), id, of_ref_var }
            };
            self.add_external_event(line_num, ev);
            ResourceTy::Value(p.clone())
          }
          None => place,
        };
        // StaticDie/MutableDie { from: place, to: lender }: drawn on
        // the lender's column only, like a borrow stored in an element.
        let mut die = self.ext_ev_of_evt(if mutable { Evt::MDie } else { Evt::SDie }, from_ro, place, *self.unique_id, false);
        if let ExternalEvent::StaticDie { of_ref_var: o, .. } | ExternalEvent::MutableDie { of_ref_var: o, .. } = &mut die {
          *o = of_ref_var;
        }
        if let Some(holder) = &ref_holder {
          self.end_loans.insert(*self.unique_id, vec![loan]);
          if holder.is_member() {
            self.row_layout.stored_field_ends.insert(*self.unique_id);
          }
        }
        self.add_external_event(end_line, die);
        *self.unique_id += 1;
        if ref_holder.is_some() {
          *self.unique_id += 1;
        }
      }
      if !stored.is_empty() {
        return;
      }
      // `show_str(r)` with `r: &&String` and `show_str(s: &String)`
      // borrows `**r` (#952).
      let deep = if of_ref_var { None } else { self.deep_place(arg) };
      // `takes(&mut *k[0])` with k an array of references (#1319): drawn
      // on k's column as a reborrow through k[0], not as a borrow of `*k`.
      let from_ro = match (&from_ro, &deep) {
        (ResourceTy::Deref(r), Some(DeepPlace { elem: Some(_), .. })) => ResourceTy::Value(r.clone()),
        _ => from_ro,
      };
      let id = *self.unique_id;
      match arg_mut {
        Mutability::Not => {
          // `bump(rc)` with `rc: &Cell<i32>` (#1149): bump can change
          // what rc points to.
          let pointee_interior_mut = !of_ref_var && matches!(&from_ro, ResourceTy::Value(r) if r.is_ref())
            && !matches!(arg.kind, ExprKind::AddrOf(..))
            && arg_ty.builtin_deref(true).is_some_and(|t| self.ty_has_interior_mut(t, arg.hir_id.owner) == InteriorMut::Yes);
          self.add_pass_by_sref_ev_full(line_num, to_ro, from_ro, ImplicitRef::No, false, pointee_interior_mut, of_ref_var, deep)
        }
        Mutability::Mut => {
          let ends_at_call = self.call_argument_site(arg).is_some_and(|key| self.facts.is_some_and(|b| b.short_call_borrows.contains(&key)));
          self.add_pass_by_mref_ev_deep(line_num, to_ro, from_ro.clone(), is_reborrow, ends_at_call, ImplicitRef::No, of_ref_var, deep);
          // `grow(&mut r.name)` with `r = &mut p`: the call can modify
          // p.name, drawn on its column too (#1216).
          self.write_field_behind_ref(&from_ro, arg, line_num);
        }
      }
      if let Some(projected) = projected {
        self.row_layout.projected_loan_places.insert(id, projected);
        if tycheck_results.expr_ty_adjusted(arg).ref_mutability() == Some(Mutability::Not) {
          self.row_layout.shared_projected_calls.insert(id);
        }
      }
    }

  }

  /// `get_ref_data` plus, when the lender has no column, the
  /// 'static data the reference is known to borrow (see
  /// `static_lender_of`). Every site that (re)sets a RefData's lender
  /// from an initializer goes through here.
  pub fn get_ref_data_full(&self, expr: &'tcx Expr) -> (ResourceTy, VecDeque<String>, Option<StaticLender>, Vec<ResourceTy>) {
    // `&s as &dyn Display` borrows what `&s` does (#1175).
    let expr = peel_ref_casts(expr, self.tcx.typeck(expr.hir_id.owner));
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
              if ty.is_freeze(self.tcx, env) && !self.needs_drop_mono(ty) {
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
    let expr = peel_ref_casts(expr, self.tcx.typeck(expr.hir_id.owner));
    // `let r = { &String::from("x") };`: what the block's last
    // expression borrows (#1383).
    if let Some(tail) = plain_block_tail(expr) {
      return self.get_ref_data_co(tail);
    }
    // `&String::new()` inside `let w = W { r: &String::new() };`: the
    // temporary's own column (#856).
    if let Some(tmp) = self.nested_ext_temps.get(&expr.hir_id) {
      return (ResourceTy::Value(tmp.clone()), VecDeque::new(), Vec::new());
    }
    // `v.first()?`: the `?` passes on the borrow its operand's value
    // holds, as `unwrap` does (#1352).
    if let Some(op) = try_operand(expr) {
      if matches!(op.kind, ExprKind::Call(..) | ExprKind::MethodCall(..)) && !op.span.from_expansion() {
        return self.get_ref_data_co(op);
      }
    }
    // A call's result: who it borrows from is decided by the callee's
    // signature (`ref_flow.rs`, #171), not by its receiver.
    if matches!(expr.kind, ExprKind::Call(..) | ExprKind::MethodCall(..)) {
      // A reference into an element whose value is itself a reference
      // (`refs.last_mut().unwrap()`) points into the collection, not
      // at what the element borrows (#1382).
      let mut lenders = self.pointer_lenders(expr).unwrap_or_else(|| self.call_lenders(expr));
      // Only the fields reached through a reference (#1073, #1080).
      if let Some(fields) = self.field_call_lenders(expr, &lenders) {
        lenders = fields.into_iter().map(|(l, _)| l).collect();
      }
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
    // `let it = &mut v.iter();` (#1209): the reference borrows a
    // temporary (the `Iter`), which rustc keeps alive to the end of the
    // block; the temporary borrows what the call's result borrows, so
    // that stays borrowed while `it` is in use. (Not the function: the
    // old lender was `iter`.)
    if let ExprKind::AddrOf(rustc_hir::BorrowKind::Ref, _, inner) = expr.kind {
      if matches!(inner.kind, ExprKind::Call(..) | ExprKind::MethodCall(..)) && !inner.span.from_expansion() {
        let (lender, aliasing, co) = self.get_ref_data_co(inner);
        if matches!(lender, ResourceTy::Value(_) | ResourceTy::Deref(_)) {
          return (lender, aliasing, co);
        }
      }
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
    // Likewise a branch's value naming a variable the branch itself
    // declares (`x` in `match o { Some(x) => x, None => "d" }`, #1327)
    // when `let r = match …` declares r before the branches are
    // visited: `add_arm_lenders` adds it once its branch has been.
    let loop_tails = || loop_break_tails(expr)
      .map(|ts| ts.into_iter().filter(|t| self.locals_registered(t)).collect::<Vec<_>>());
    let own_tails = || branch_tails(expr)
      .map(|ts| ts.into_iter().filter(|t| !self.uses_unregistered_local_in(t, expr.span)).collect::<Vec<_>>());
    if let Some(tails) = own_tails().or_else(loop_tails) {
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
    struct Check<'b, 'tcx> { tcx: TyCtxt<'tcx>, raps: &'b BodyRaps, ok: bool }
    impl<'b, 'tcx> Visitor<'tcx> for Check<'b, 'tcx> {
      fn visit_expr(&mut self, e: &'tcx Expr<'tcx>) {
        if let ExprKind::Path(QPath::Resolved(None, p)) = e.kind {
          if matches!(p.res, Res::Local(_)) && self.raps.path(p, &self.tcx).is_none() {
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

  /// Whether `e` names a variable declared within `within` that has no
  /// column yet: one a branch of the conditional spanning `within`
  /// declares (an arm's pattern binding, a `let` in the branch), which
  /// isn't registered until that branch is visited (#1327).
  fn uses_unregistered_local_in(&self, e: &'tcx Expr<'tcx>, within: rustc_span::Span) -> bool {
    use rustc_hir::intravisit::{self, Visitor};
    struct Check<'b, 'tcx> { tcx: TyCtxt<'tcx>, raps: &'b BodyRaps, within: rustc_span::Span, found: bool }
    impl<'b, 'tcx> Visitor<'tcx> for Check<'b, 'tcx> {
      fn visit_expr(&mut self, e: &'tcx Expr<'tcx>) {
        if let ExprKind::Path(QPath::Resolved(None, p)) = e.kind {
          if let Res::Local(hid) = p.res {
            if self.within.contains(self.tcx.hir_span(hid)) && self.raps.path(p, &self.tcx).is_none() {
              self.found = true;
            }
          }
        }
        intravisit::walk_expr(self, e);
      }
    }
    let mut c = Check { tcx: self.tcx, raps: &self.raps, within, found: false };
    c.visit_expr(e);
    c.found
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
    self.note_loan_kinds(r.name(), loop_expr);
  }

  fn get_ref_data_single(&self, expr: &'tcx Expr<'tcx>) -> (ResourceTy, VecDeque<String>){
    // ex:
    // let a = &b (where b has type &&i32)
    // a's aliases consist of a -> (b -> ... -> ...)
    if is_addr(&expr) {
      // in this case we are borrowing from b; `&p.name` with p a
      // reference is a reborrow of the field through p (#266).
      let lender = self.borrow_source(&expr);
      // `&compute()` / `&mk()[0]` with no `(temporary)` column (its value
      // has nothing to drop, #400): the reference borrows that temporary,
      // never the function, which lends nothing (#1155).
      let lender = match lender {
        ResourceTy::Value(f) if f.is_fn() => ResourceTy::Anonymous,
        other => other,
      };
      // `&mut m` / `&x` coerced to `&mut String` / `&String` borrows
      // `*m` / `*x`, as `&mut *m` / `&*x` does: the borrow is given
      // back to `*m` / `*x` (#926, #939).
      let lender = match lender {
        ResourceTy::Value(x) if is_coerced_ref_var_reborrow(expr, self.tcx.typeck(expr.hir_id.owner)) =>
          ResourceTy::Deref(x),
        other => other,
      };
      let mut aliasing = get_aliasing_data(&lender, &self.borrow_map);
      // if expr has type &&
      let ty = self.tcx.typeck(expr.hir_id.owner).node_type(expr.hir_id);
      if ty.builtin_deref(false).unwrap().is_ref() {
        match &lender {
          // (`get_rap` never yields DerefField, but `&&p.f` through a
          // reference does: there is no aliasing chain to extend.)
          ResourceTy::Anonymous | ResourceTy::Caller | ResourceTy::DerefField(..) | ResourceTy::Temporary { .. } => { (lender, aliasing) }
          // `&words[1]` with `words: Vec<&str>` also has type `&&str`,
          // but what is borrowed is an element stored in the owner
          // `words` (the lender resolves to `words` itself): words is no
          // reference, so there is nothing to alias, and it has no
          // borrow_map entry for `update_lifetime` to extend (#894).
          // Only a reference borrowed or reborrowed through (`&b`, `&*b`,
          // `&v[1]` with `v: &Vec<&str>`) joins the chain.
          ResourceTy::Value(x) | ResourceTy::Deref(x) | ResourceTy::Index(x, _, _)
            if !self.borrow_map.contains_key(x.name()) => (lender, aliasing),
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
      // (`let s: &String = r;` reborrows `&*r` the same way, #1329.)
      if matches!(expr.kind, ExprKind::Path(_) | ExprKind::Field(..))
        && is_implicit_reborrow(expr, self.tcx.typeck(expr.hir_id.owner)) {
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
        ResourceTy::Value(lender) if !lender.is_fn() => {
          let first = self.loan_end_line(l, lender.name());
          // A loan that ends before a use of the reference isn't the
          // one it holds: `r = if a.len() > 0 { let y = &a; y } ..`
          // holds a copy of y's loan, not `a.len()`'s on the same line
          // (#1335).
          match first {
            Some(end) if end < rd.lifetime => {
              let root = lender.name().split('.').next().unwrap_or(lender.name()).to_owned();
              self.gather_borrow_data(self.bwf).iter()
                .filter(|b| b.region.0 == l && b.borrowed_place.as_deref() == Some(root.as_str()) && b.region.1 >= rd.lifetime)
                .map(|b| b.region.1)
                .min()
                .or(first)
            }
            _ => first,
          }
        }
        _ => None,
      },
    }
  }

  /// `loan_end` for the reference `name`: of several loans of its lender
  /// made on the line its loan is known by, the one rustc says it may
  /// hold (a lifetime of its type contains it), not just the first
  /// (`let mut r = if a.len() > 0 { &a } else { &b };` borrows a twice
  /// on that line, and `a.len()`'s loan ends there, #1557). During the
  /// visit, r's uses after a re-point aren't known yet, so `loan_end`'s
  /// guard (#1335) can't tell them apart.
  pub fn loan_end_of(&self, name: &str, rd: &RefData) -> Option<usize> {
    if let (Some(LoanId::Line(l)), ResourceTy::Value(lender)) = (rd.loan(), &rd.lender) {
      if !lender.is_fn() {
        let root = lender.name().split('.').next().unwrap_or(lender.name()).to_owned();
        let loans = self.gather_borrow_data(self.bwf);
        let on_line: Vec<&crate::mir_data::MIRBorrowData> = loans.iter()
          .filter(|b| b.region.0 == l && b.borrowed_place.as_deref() == Some(root.as_str()))
          .collect();
        if let Some(end) = self.held_loans(name, &on_line).iter().map(|b| b.region.1).max() {
          return Some(end);
        }
      }
    }
    self.loan_end(rd)
  }

  /// Of `loans`, several candidates for the loan the reference `name`
  /// holds, those a lifetime of its type contains (#1557). Empty when
  /// there is at most one candidate, or that doesn't pick any.
  fn held_loans<'b>(&self, name: &str, loans: &[&'b crate::mir_data::MIRBorrowData]) -> Vec<&'b crate::mir_data::MIRBorrowData> {
    if loans.len() < 2 {
      return Vec::new();
    }
    let Some(held) = self.loan_indices_held_by(name) else { return Vec::new() };
    let mine: Vec<&crate::mir_data::MIRBorrowData> = loans.iter().copied().filter(|b| held.contains(&b.borrow_idx)).collect();
    if mine.len() == loans.len() { Vec::new() } else { mine }
  }

  /// The first of `cands` (by line, then as rustc numbers them), or of
  /// several on that line, the first `name` holds (`held_loans`).
  fn first_held_loan<'b>(&self, name: &str, cands: Vec<&'b crate::mir_data::MIRBorrowData>) -> Option<&'b crate::mir_data::MIRBorrowData> {
    let first = cands.iter().copied().min_by_key(|b| (b.region.0, b.borrow_idx))?;
    let line: Vec<&crate::mir_data::MIRBorrowData> = cands.iter().copied().filter(|b| b.region.0 == first.region.0).collect();
    self.held_loans(name, &line).into_iter().min_by_key(|b| b.borrow_idx).or(Some(first))
  }

  /// The line rustc's borrow checker ends the loan `rd` holds on
  /// `lender` (`rd`'s lender or one of its co-lenders), for the write
  /// at `write_line` that gives the reference a new value: the loan's
  /// last line when that is before the write; else its end on the paths
  /// that reach the write (#770), as the loan can be live on another
  /// path, or past the write for a read of the old reference there is
  /// none; None when it is live up to the write.
  /// (`name`: the reference's own, whose loans rustc knows, to pick the
  /// one it holds of several made on one line, #1557.)
  pub fn old_loan_end(&self, name: &str, rd: &RefData, lender: &ResourceTy, write_line: usize, write: Option<rustc_span::Span>) -> Option<usize> {
    let ResourceTy::Value(lender_rap) = lender else { return None };
    if lender_rap.is_fn() {
      return None;
    }
    let root = lender_rap.name().split('.').next().unwrap_or(lender_rap.name()).to_owned();
    let loans = self.gather_borrow_data(self.bwf);
    // Of several loans of the lender on the first line that has one, the
    // ones the reference may hold (`a.len()`'s and `&a`'s in `let mut r =
    // if a.len() > 0 { &a } else { &b };`: r holds `&a`'s, #1557).
    let first_held = |cands| self.first_held_loan(name, cands);
    let own = match rd.loan() {
      Some(LoanId::Span(sp)) if *lender == rd.lender => loans.iter().find(|b| b.span == sp),
      Some(LoanId::Line(l)) if *lender == rd.lender =>
        first_held(loans.iter().filter(|b| b.region.0 == l && b.borrowed_place.as_deref() == Some(root.as_str())).collect()),
      // A co-lender, or a reference with several lenders (#171): the
      // first loan of that lender made from where the reference was
      // given its value.
      _ => first_held(loans.iter()
        .filter(|b| b.borrowed_place.as_deref() == Some(root.as_str())
          && b.region.0 >= rd.assigned_at && b.region.0 <= write_line)
        .collect()),
    }?;
    // Not a loan that ends before a use of the reference: another loan
    // of the lender made on that line is the one it holds (a copy of
    // `y`'s in `r = if a.len() > 0 { let y = &a; y } else { &b };`,
    // not `a.len()`'s, #1335).
    // (`rd.lifetime` counts the write itself as a use: the loan that
    // lasts longest up to it.)
    let own = if own.region.1 < rd.lifetime && !matches!(rd.loan(), Some(LoanId::Span(_))) {
      loans.iter()
        .filter(|b| b.region.0 == own.region.0 && b.borrowed_place == own.borrowed_place
          && b.region.1 < write_line.max(own.region.1 + 1))
        .max_by_key(|b| (b.region.1, std::cmp::Reverse(b.borrow_idx)))
        .unwrap_or(own)
    } else {
      own
    };
    let in_range = |end: usize| end < write_line && end >= rd.assigned_at;
    Some(own.region.1).filter(|&end| in_range(end))
      .or_else(|| {
        // The write is on one path only (`if c { r = &y; }`) and the loan
        // is still in scope on another path after it: the borrow checker
        // keeps the lender borrowed until the latest end over the paths
        // that meet there (#778), not where the loan stops on the path
        // through the write.
        // (On the write's own line before it, in a one-line `if`, #1188:
        // then only the end past the write is one.)
        let path_end = self.loan_end_on_path_to(own.borrow_idx, write_line, write)
          .filter(|&end| end >= rd.assigned_at)?;
        if own.region.1 > write_line { Some(own.region.1) } else { Some(path_end).filter(|&end| in_range(end)) }
      })
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

  /// `std::mem::swap(&mut r1, &mut r2)` with r1 and r2 reference
  /// variables (#1230): each now holds the other's reference, so each
  /// gives back the borrow the other held (both loans are in one
  /// lifetime, `swap<T>(&mut T, &mut T)`, so they end together).
  pub fn swap_ref_vars(&mut self, fn_expr: &'tcx Expr<'tcx>, args: &'tcx [Expr<'tcx>], line: usize) {
    let typeck = self.tcx.typeck(fn_expr.hir_id.owner);
    let Some(rustc_middle::ty::FnDef(def_id, _)) = typeck.node_type_opt(fn_expr.hir_id).map(|t| *t.kind()) else { return };
    let ref_var = |a: &'tcx Expr<'tcx>| match a.kind {
      ExprKind::AddrOf(_, m, inner) if m.is_mut() => expr_to_rap_name(inner, &self.tcx)
        .filter(|n| self.raps.get(n).is_some_and(|r| r.rap.is_ref()) && self.borrow_map.contains_key(n)),
      _ => None,
    };
    if !self.tcx.is_diagnostic_item(rustc_span::sym::mem_swap, def_id) {
      let names: Vec<Option<String>> = args.iter().map(ref_var).collect();
      self.exchangeable_ref_vars(def_id, &names);
      return;
    }
    let [x, y] = args else { return };
    let (Some(n1), Some(n2)) = (ref_var(x), ref_var(y)) else { return };
    if n1 == n2 {
      return;
    }
    let (Some(d1), Some(d2)) = (self.borrow_map.get(&n1).cloned(), self.borrow_map.get(&n2).cloned()) else { return };
    for (n, from) in [(&n1, &d2), (&n2, &d1)] {
      if let Some(rd) = self.borrow_map.get_mut(n) {
        rd.lender = from.lender.clone();
        rd.aliasing = from.aliasing.clone();
        rd.static_lender = from.static_lender.clone();
        rd.borrow_span = from.borrow_span;
        rd.loan_line = from.loan_line;
        rd.co_lenders = from.co_lenders.clone();
        rd.lender_muts = from.lender_muts.clone();
        rd.assigned_at = line;
      }
    }
  }

  /// `exchange(&mut r1, &mut r2)` with `fn exchange<T>(x: &mut T, y:
  /// &mut T)` (any function but `mem::swap`, which `swap_ref_vars`
  /// draws exactly): two `&mut` parameters of one type (one type
  /// parameter, or a reference type with one named lifetime) let the
  /// function exchange or copy the references r1 and r2 hold (#1249).
  /// From the signature alone, each may point to either lender
  /// afterwards, so each gets the other's lenders as co-lenders: its end
  /// gives back both, as for a call result tied to several arguments
  /// (#171). Both loans are in one lifetime, so they end together.
  fn exchangeable_ref_vars(&mut self, def_id: rustc_hir::def_id::DefId, arg_refs: &[Option<String>]) {
    if !matches!(self.tcx.def_kind(def_id), DefKind::Fn | DefKind::AssocFn) {
      return;
    }
    let sig = self.tcx.fn_sig(def_id).instantiate_identity().skip_binder();
    let inputs = sig.inputs();
    let mut groups: Vec<(rustc_middle::ty::Ty<'tcx>, Vec<String>)> = Vec::new();
    for (i, a) in arg_refs.iter().enumerate() {
      let Some(input) = inputs.get(i) else { break };
      let rustc_middle::ty::Ref(_, pointee, Mutability::Mut) = input.kind() else { continue };
      let Some(n) = a.clone() else { continue };
      match groups.iter_mut().find(|(t, _)| t == pointee) {
        Some((_, names)) => if !names.contains(&n) { names.push(n) },
        None => groups.push((*pointee, vec![n])),
      }
    }
    for (_, names) in groups.into_iter().filter(|(_, n)| n.len() > 1) {
      let datas: Vec<RefData> = names.iter().filter_map(|n| self.borrow_map.get(n).cloned()).collect();
      if datas.len() != names.len() {
        continue;
      }
      let all: Vec<ResourceTy> = datas.iter().flat_map(|d| d.all_lenders()).collect();
      for n in &names {
        let Some(rd) = self.borrow_map.get_mut(n) else { continue };
        for l in all.iter() {
          if *l != rd.lender && !rd.co_lenders.contains(l) && matches!(l, ResourceTy::Value(_) | ResourceTy::Deref(_)) {
            rd.co_lenders.push(l.clone());
          }
        }
      }
    }
  }

  /// `src` (its old data `old`) is being given a new value on `line`
  /// while a reference that reborrowed through it (`let first = &*r;`,
  /// `&r[..1]`) is still used after the write (#1182). That reference
  /// points into what `src` pointed to *then*: the borrow checker kills
  /// its own loan of `*src` at the write, but keeps `src`'s old loan live
  /// as long as the reborrow is. So the reborrow takes over `src`'s old
  /// lender: its end, at its last use, gives the lender back (with the
  /// old loan's kind, a `&mut` one for `let r = &mut a`), and `src`
  /// itself is free to hold its new reference (its old borrow ends at
  /// the write without giving the lender back: another reference still
  /// holds it).
  pub fn hand_loan_to_reborrows(&mut self, src: &str, old: &RefData, line: usize) {
    let Some(src_rap) = self.raps.get(src).map(|r| r.rap.clone()) else { return };
    if !matches!(old.lender, ResourceTy::Value(_)) || !old.co_lenders.is_empty() {
      return;
    }
    // Its lender is `*src`, `src` itself (`&r[..1]`, a call's result),
    // or a field of `*src` (`&r.name`). (A reference that borrows the
    // variable `src`, `&r`, can't be live across the write: rustc
    // rejects it.)
    let through = |l: &ResourceTy| match l {
      ResourceTy::Deref(x) | ResourceTy::Value(x) | ResourceTy::DerefField(x, ..) | ResourceTy::Index(x, ..) =>
        x.hash() == src_rap.hash() && x.name() == src_rap.name(),
      _ => false,
    };
    #[expect(clippy::disallowed_methods, reason = "collected and sorted")]
    let mut names: Vec<String> = self.borrow_map.iter()
      .filter(|(n, rd)| n.as_str() != src && through(&rd.lender) && rd.assigned_at <= line)
      .map(|(n, _)| n.clone())
      .collect();
    names.sort();
    let old_mut = old.mut_of(&old.lender);
    for n in names {
      // The reborrow's own loan of `*src` ends at the write: its end
      // is its last use.
      let Some(last) = self.borrow_map[&n].local.and_then(|l| self.local_last_live_line(l, (line, usize::MAX))).filter(|&l| l > line) else { continue };
      let Some(rd) = self.borrow_map.get_mut(&n) else { continue };
      rd.lender = old.lender.clone();
      rd.lender_muts = if old_mut == rd.ref_mutability { Vec::new() } else { vec![(old.lender.clone(), old_mut)] };
      let floor = self.borrow_floor.entry(n).or_insert(last);
      *floor = (*floor).max(last);
    }
  }


  // Add a RAP to our collection of raps for a let stmt
  pub fn define_lhs(&mut self, name: String, mutability: bool, expr: &'tcx Expr, ty: Ty <'tcx>) {
    // (Declared ahead of the conditional whose branch value it now
    // gets, #1007.)
    if self.bind_existing && self.raps.contains_key(&name) {
      return;
    }
    let is_special = ty_is_special_owner(&ty);
    if ty.is_ref() {
      let (lender, aliasing, static_lender, co_lenders) = self.get_ref_data_full(&expr);
      let src_loan = self.source_ref_loan(expr);
      // `let d: &dyn Debug = &gd;`: what d points to is gd's `&String`,
      // known from the type before the unsizing, not from `dyn Debug`
      // (#947).
      let typeck = self.tcx.typeck(expr.hir_id.owner);
      let unsized_from = typeck.expr_adjustments(expr).iter()
        .any(|a| matches!(a.kind, adjustment::Adjust::Pointer(adjustment::PointerCoercion::Unsize)))
        .then(|| typeck.expr_ty(expr));
      let pointee_copy = self.ref_pointee_is_copy(unsized_from.unwrap_or(ty), expr.hir_id.owner);
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
        self.note_loan_kinds(&name, expr);
        self.note_ref_carrier(&name, src_loan);
        self.narrow_ref_call_lenders(&name, expr, ty);
      }
    }
    else if matches!(ty.kind(), TyKind::Tuple(_)) && self.has_member_columns(ty) {
      // A tuple local gets a column per element (`t.0`, `t.1`), drawn
      // like a struct's fields (#189): moving `t.0` out leaves `t.1`
      // owned by `t`, and each element drops on its own.
      let owner_hash = self.struct_column_hash(&name);
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
      self.register_struct_field_lenders(&name, expr, ty);
    }
    // `let b = Box::new(P { .. });` with a field moved out through the
    // box (`take(b.name)`, #602): drawn like the plain struct, with a
    // column per field, so the move and what is left to drop show.
    else if let Some(inner) = self.box_with_field_moved_out(&name, ty) {
      let owner_hash = self.struct_column_hash(&name);
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
          let owner_hash = self.struct_column_hash(&name);
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
            self.mark_owner_type(&name, ty);
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
      // A closure that holds only references (or Copy captures) has
      // nothing to drop: what it holds is a "value", not a "resource"
      // (#484, #791).
      // A captured std lock counts as a resource on every host, as
      // for `ty_is_plain` (#813).
      let plain = !self.needs_drop(ty, expr.hir_id.owner) && !ty_has_std_sync_primitive(self.tcx, ty);
      self.add_closure_owner(name, mutability, plain, self.current_scope, !self.inside_branch);
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

  /// The reference `name` (a variable, or a field `h.r` of one) was
  /// just given `init`'s lenders: record the kind of each one's loan
  /// where it isn't the reference's own kind, which `ref_mutability`
  /// (the reference's type) would otherwise give. A shared reference
  /// can hold a mutable loan (`let r: &String = &mut s;`, `let r =
  /// downgrade(&mut s);` with `fn(&mut String) -> &str`): s can't be
  /// used at all while r is live (E0502), and the borrow ends as a
  /// mutable one (#1122). A mutable reference can hold a shared loan
  /// (`let r = &mut &x;` borrows x shared, into a temporary `&String`
  /// that r borrows mutably; #1131). The kinds come from rustc's loans
  /// issued in `init` that flow into r's lifetimes; a copy of another
  /// reference (`let r2 = r;`) holds that one's loans, of their kinds.
  /// Clears what an earlier value of the reference recorded.
  pub fn note_loan_kinds(&mut self, name: &str, init: &'tcx Expr<'tcx>) {
    let Some(rd) = self.borrow_map.get(name) else { return };
    let lenders: Vec<ResourceTy> = rd.all_lenders().into_iter()
      .filter(|l| matches!(l, ResourceTy::Value(_) | ResourceTy::Deref(_)))
      .collect();
    let own = rd.ref_mutability;
    let mut parts = name.split('.');
    let base = parts.next().unwrap_or_default();
    let fields: Vec<&str> = parts.collect();
    let held = if lenders.is_empty() { None } else { self.loans_held_from(base, &fields, init.span) };
    // `let r2 = r;` / `let q = x.part;`: no loan is issued here.
    let mut src = init;
    while let ExprKind::DropTemps(inner) = src.kind {
      src = inner;
    }
    let copied = match src.kind {
      ExprKind::Path(QPath::Resolved(..)) | ExprKind::Field(..) =>
        expr_to_rap_name(src, &self.tcx).and_then(|n| self.borrow_map.get(&n)).cloned(),
      _ => None,
    };
    // A loan issued here through another reference (`id(r)` reborrows
    // `*r`, with r from `downgrade(&mut s)` or `let r: &String = &mut
    // s;`): the lender reached through r stays borrowed by r's own loan,
    // of its kind. r2 = id(r) keeps s mutably borrowed, not shared (#1510).
    // (A shared reborrow of `*r` with r a `&T` is no loan to rustc, so
    // the references passed to a call count too.)
    let mut via: Vec<String> = held.iter().flat_map(|h| h.keys().cloned()).collect();
    self.ref_args_of(init, &mut via);
    let through_ref = |l: &ResourceTy| -> Option<bool> {
      let kinds: Vec<bool> = via.iter()
        .filter(|k| k.as_str() != base)
        .filter_map(|k| self.borrow_map.get(k))
        .filter(|d| d.all_lenders().contains(l))
        .map(|d| d.mut_of(l))
        .collect();
      (!kinds.is_empty()).then(|| kinds.into_iter().any(|m| m))
    };
    let muts: Vec<(ResourceTy, bool)> = lenders.into_iter().map(|l| {
      let base = l.real_name().split('.').next().unwrap_or_default().to_owned();
      let m = held.as_ref().and_then(|h| h.get(&base).copied())
        .or_else(|| copied.as_ref().filter(|c| c.all_lenders().contains(&l)).map(|c| c.mut_of(&l)))
        .or_else(|| through_ref(&l))
        .unwrap_or(own);
      (l, m)
    }).collect();
    let muts = if muts.iter().all(|(_, m)| *m == own) { Vec::new() } else { muts };
    if let Some(rd) = self.borrow_map.get_mut(name) {
      rd.lender_muts = muts;
    }
  }

  /// The variables passed to `e`'s calls (and their receivers) as they
  /// are, or reborrowed (`&*r`, `&mut *r`): the references a call's
  /// result may keep a loan of.
  fn ref_args_of(&self, e: &'tcx Expr<'tcx>, out: &mut Vec<String>) {
    let mut e = e;
    loop {
      match e.kind {
        ExprKind::DropTemps(inner) | ExprKind::AddrOf(_, _, inner) | ExprKind::Unary(rustc_hir::UnOp::Deref, inner) => e = inner,
        _ => break,
      }
    }
    match e.kind {
      ExprKind::Call(_, args) => args.iter().for_each(|a| self.ref_args_of(a, out)),
      ExprKind::MethodCall(_, recv, args, _) => {
        self.ref_args_of(recv, out);
        args.iter().for_each(|a| self.ref_args_of(a, out));
      }
      ExprKind::Path(QPath::Resolved(..)) | ExprKind::Field(..) => {
        if let Some(n) = expr_to_rap_name(e, &self.tcx) {
          if !out.contains(&n) {
            out.push(n);
          }
        }
      }
      _ => {}
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
    // `let p = opart(&s)?;`: the `?` passes on what the call's value
    // holds (#1352).
    let init = try_operand(init).unwrap_or(init);
    if !matches!(init.kind, ExprKind::Call(..) | ExprKind::MethodCall(..)) || init.span.from_expansion() {
      return;
    }
    self.register_call_member_lenders(name, name, init, ty, false);
  }

  fn register_call_member_lenders(&mut self, root: &str, name: &str, init: &'tcx Expr<'tcx>, ty: Ty<'tcx>, reassigning: bool) {
    // A tuple's elements too (`let q = pair(&s);`, #1341).
    let closure_result = self.closure_result(self.raps.root_local(root), init);
    for (field_name, field_ty) in self.member_fields(ty) {
      let fname = format!("{}.{}", name, field_name);
      let has_fields = self.has_member_columns(field_ty);
      if closure_result.is_some() && has_fields {
        self.register_call_member_lenders(root, &fname, init, field_ty, reassigning);
      }
      if reassigning && !closure_result.is_some_and(|r| r.holders.contains_key(&fname)) {
        continue;
      }
      // A field drawn as one column that holds a borrow (`items:
      // Vec<&'a str>`) becomes a holder, as in a struct literal (#1088).
      let holder = !field_ty.is_ref() && !has_fields
        && field_ty.walk().any(|g| matches!(g.kind(), GenericArgKind::Lifetime(_)))
        && matches!(self.raps.get(&fname).map(|rd| &rd.rap), Some(ResourceAccessPoint::Struct(Struct { is_member: true, .. })));
      if !holder && !self.borrow_map.contains_key(&fname) {
        continue;
      }
      let (mut lenders, is_mut) = if reassigning {
        // Signature lenders were resolved before overwriting the old fields.
        self.borrow_map.get(&fname).map_or_else(|| (Vec::new(), false), |rd| {
          let lenders = rd.all_lenders().into_iter().filter(|l| !matches!(l, ResourceTy::Anonymous)).collect();
          (lenders, rd.ref_mutability)
        })
      } else if root == name {
        let (lenders, _, is_mut) = self.call_field_lenders(init, &field_name);
        (lenders, is_mut)
      } else {
        // The closure facts identify deeper projected fields.
        (Vec::new(), false)
      };
      let mut lender_muts = Vec::new();
      let mut through = Vec::new();
      let narrowed = holder && self.narrow_holder_lenders(&fname, init, &mut lenders, &mut lender_muts, &mut through);
      self.extend_closure_result_lenders(root, &fname, init, &mut lenders, &mut lender_muts);
      if narrowed && lenders.is_empty() {
        self.mark_holder(&fname);
        continue;
      }
      if lenders.is_empty() {
        continue;
      }
      let lender = lenders.remove(0);
      let aliasing = get_aliasing_data(&lender, &self.borrow_map);
      if holder {
        self.mark_holder(&fname);
        let line = expr_to_line(&init, &self.tcx);
        let local = self.raps.local(&fname);
        self.borrow_map.insert(fname, RefData {
          local,
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
          through,
        });
      } else if let Some(rd) = self.borrow_map.get_mut(&fname) {
        rd.lender = lender;
        rd.co_lenders = lenders;
        rd.aliasing = aliasing;
        if lender_muts.is_empty() {
          self.note_loan_kinds(&fname, init);
        } else {
          rd.lender_muts = lender_muts;
        }
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
        self.add_external_event(end, ExternalEvent::RefDie { slot: *self.unique_id, from: holder.clone(), to, num_curr_borrowers: 1, id: *self.unique_id });
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
    let mut lender_muts = self.holder_lender_muts(rhs);
    let mut through = Vec::new();
    self.narrow_holder_lenders(&name, rhs, &mut lenders, &mut lender_muts, &mut through);
    self.extend_closure_result_lenders(&name, &name, rhs, &mut lenders, &mut lender_muts);
    if lenders.is_empty() {
      return;
    }
    self.mark_holder(&name);
    let lender = lenders.remove(0);
    let aliasing = get_aliasing_data(&lender, &self.borrow_map);
    let local = self.raps.local(&name);
    self.borrow_map.insert(name, RefData {
      local,
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
      through,
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
      // A reference field, or a field holding one (`items: Vec<&'a
      // str>`, #1088).
      if !member.is_ref() && !member.holds_borrow() {
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
    let rhs = try_operand(rhs).unwrap_or(rhs);
    let new: Vec<(String, Vec<ResourceTy>)> = match rhs.kind {
      ExprKind::Call(..) | ExprKind::MethodCall(..) if !rhs.span.from_expansion() => self.member_fields(ty).into_iter()
        .map(|(f, _)| (format!("{}.{}", st.name(), f), self.call_field_lenders(rhs, &f).0))
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
        if self.raps.get(member.name()).is_some_and(|r| r.rap.is_ref()) {
          rd.lender_muts = Vec::new();
        }
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
      self.note_loan_kinds(&fname, rhs);
    }
    if self.closure_result(st.owner_local().or_else(|| self.raps.root_local(st.name())), rhs).is_some() {
      self.register_call_member_lenders(st.name(), st.name(), rhs, ty, true);
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
  /// (`all_new`: every borrow of the new value is a new loan.)
  pub fn draw_struct_field_borrow_ends(&mut self, lhs: &Expr<'_>, ends: Vec<(usize, Vec<ResourceTy>, ResourceTy, bool)>, all_new: bool) {
    for (end, lenders, holder, is_mut) in ends {
      let now = holder.extract_rap()
        .filter(|_| !all_new)
        .and_then(|h| self.borrow_map.get(h.name()))
        .map(|rd| rd.all_lenders())
        .unwrap_or_default();
      for l in lenders {
        if now.contains(&l) {
          continue;
        }
        let id = *self.unique_id;
        if let Some(loan) = self.overwritten_drop_loan(lhs, &holder, &l, is_mut) {
          self.end_loans.insert(id, vec![loan]);
        }
        self.add_ev(end, if is_mut { Evt::MDie } else { Evt::SDie }, l, holder.clone(), false);
        self.field_overwrite_ends.extend(id..*self.unique_id);
      }
    }
  }

  /// Identify the old loan by the assignment's destructor. On the
  /// other path the same slot moved into drop before the assignment.
  fn overwritten_drop_loan(&self, lhs: &Expr<'_>, holder: &ResourceTy, lender: &ResourceTy, is_mut: bool) -> Option<usize> {
    use crate::facts::{DropRoute, Fact};
    let (ResourceTy::Value(field), ResourceTy::Value(lender)) = (holder, lender) else { return None };
    let facts = self.body_facts?;
    let mut drops = self.effects.get(&lhs.span.with_parent(None))?.iter().filter_map(|&(_, ev)| {
      if !matches!(ev.fact, Fact::Drop { at_reassign: true, .. }) { return None; }
      let local = ev.place_local?;
      let ids = facts.reassign_drop_loans.get(&ev.location)?;
      self.raps.place(local, field.name()).filter(|r| r.rap == *field)?;
      Some((ev.location, local, ids))
    });
    let (drop, local, ids) = drops.next()?;
    if drops.any(|(other, ..)| other != drop) { return None; }
    let is_field = |p: &crate::facts::EndPlace| p.local == local.index() && p.name == *field.name();
    let mut loans = facts.loans.iter().filter(|l| ids.contains(&l.loan)
      && l.lender == *lender.name()
      && l.lender_local.is_some_and(|local| self.raps.place(local, lender.name()).is_some_and(|r| r.rap == *lender))
      && l.kind == if is_mut { "mut" } else { "shared" }
      && facts.rows.iter().flat_map(|row| &row.facts).any(|ev| ev.location == drop
          && matches!(ev.fact, Fact::BorrowEnd { loan, .. } if loan == l.loan)
          && ev.end_places.iter().any(is_field)));
    let loan = loans.next()?.loan;
    if loans.next().is_some() { return None; }
    let mut overwrite = false;
    let mut explicit = false;
    for ev in facts.rows.iter().flat_map(|row| &row.facts) {
      if !matches!(ev.fact, Fact::BorrowEnd { loan: id, .. } if id == loan) { continue; }
      if ev.drop_route == DropRoute::Unwind { return None; }
      if ev.location == drop && ev.end_places.iter().any(is_field) {
        overwrite = true;
      } else if facts.explicit_drop_ends.get(&(ev.location, loan)).is_some_and(|fields| fields.iter().any(is_field)) {
        explicit = true;
      } else {
        return None;
      }
    }
    (overwrite && explicit).then_some(loan)
  }

  /// After `let h = init;` was drawn: a holder whose initializer
  /// isn't a call (`[&a, &b]`, `vec![&s]`, which `match_rhs` draws
  /// as a plain bind) gets a borrow arrow from each lender (#349).
  /// Calls draw theirs in `match_rhs` (`emit_call_result_borrows`).
  pub fn emit_holder_literal_borrows(&mut self, name: &String, init: &'tcx Expr<'tcx>) {
    // The value of a block (`v = { let r = &mut a; vec![r] };`, #1486),
    // where it is.
    let init = match init.kind {
      ExprKind::Block(..) if !init.span.from_expansion() => match branch_value_expr(init) {
        Some(v) => v,
        None => return,
      },
      _ => init,
    };
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
    self.draw_holder_borrows(&to, &rd.all_lenders(), &rd, &self.aggregate_borrows(init).unwrap_or_default(), line, None);
  }

  /// `emit_holder_literal_borrows` for the value of one branch (`body`)
  /// of the `if` / `match` the holder `lhs` is bound from (`let a = if c
  /// { [&s] } else { [&u] };`, #1435): a borrow arrow from each lender
  /// that value holds, in that branch.
  pub(crate) fn emit_arm_holder_literal_borrows(&mut self, lhs: &ResourceTy, body: &'tcx Expr<'tcx>) {
    let ResourceTy::Value(h) = lhs else { return };
    let Some(v) = branch_value_expr(body) else { return };
    if !(v.span.from_expansion() || matches!(v.kind, ExprKind::Array(..) | ExprKind::Repeat(..))) {
      return;
    }
    let Some(rap) = self.raps.get(h.name()).map(|rd| rd.rap.to_owned()) else { return };
    if rap.is_ref() || !rap.holds_borrow() {
      return;
    }
    let Some(rd) = self.borrow_map.get(h.name()).cloned() else { return };
    let Some(ty) = self.tcx.typeck(v.hir_id.owner).expr_ty_opt(v) else { return };
    let own = self.holder_lenders(v, ty).0;
    let line = expr_to_line(&v, &self.tcx);
    let to = ResourceTy::Value(rap);
    let lenders: Vec<_> = rd.all_lenders().into_iter().filter(|l| own.contains(l)).collect();
    self.draw_holder_borrows(&to, &lenders, &rd, &self.aggregate_borrows(v).unwrap_or_default(), line, None);
  }

  /// `name` was just registered as an owner bound to `init`. If its
  /// value holds a borrow of variables we draw (`let it = v.iter();`,
  /// `let third = v.get(2);`; see `holder_lenders`), mark it as a
  /// holder and record that borrow in `borrow_map` like a reference's,
  /// so it lasts until the holder's last use (MIR refines it in
  /// `emit_borrow_ends`), and a reference later taken out of it
  /// (`it.next().unwrap()`) finds `v` as its lender (#336).
  fn register_holder(&mut self, name: &String, init: &'tcx Expr<'tcx>, ty: Ty<'tcx>) {
    self.retire_loan(name, expr_to_line(&init, &self.tcx));
    let (mut lenders, is_mut) = self.holder_lenders(init, ty);
    let mut lender_muts = self.holder_lender_muts(init);
    // `let w: Vec<&String> = v.iter().copied().collect();` holds the
    // references stored in v, not a borrow of v (#1094): a holder of
    // borrows whose own lenders all went is still one.
    let mut through = Vec::new();
    let narrowed = self.narrow_holder_lenders(name, init, &mut lenders, &mut lender_muts, &mut through);
    self.extend_closure_result_lenders(name, name, init, &mut lenders, &mut lender_muts);
    if narrowed && lenders.is_empty() {
      self.mark_holder(name);
      return;
    }
    if lenders.is_empty() && self.moves_mut_ref_param_in(init) {
      // `let k = Some(r);`, `Box::new(r)`, `vec![r]` with r a `&mut`
      // parameter (#1289): k holds r's reference, which borrows the
      // caller's data. That has no column here: k's borrow comes from
      // nothing drawn, as for `let r2 = id(r);` (#1205).
      lenders.push(ResourceTy::Caller);
    }
    if lenders.is_empty() {
      // `let mut refs = Vec::new();` that a later `refs.push(&s)`
      // fills with borrows (#392): a holder from the start, with no
      // borrow yet; `record_receiver_store` adds each one.
      // So is a value a branch builds around a borrow that isn't known
      // until the branch is visited (`add_arm_holder_lenders`, #1435).
      if self.filled_with_borrows_later(init, ty) || self.holder_lenders_in_branch(init, ty) {
        self.mark_holder(name);
      }
      return;
    }
    if !self.mark_holder(name) {
      return;
    }
    // `let it2 = it;` moves the value, and the borrow with it: `it`
    // no longer holds it (a Copy holder like an `Option<&T>` is
    // copied, and both hold it).
    if let ExprKind::Path(..) = init.kind {
      // (And what it reaches through them, #1399.)
      if let Some(src) = expr_to_rap_name(init, &self.tcx).and_then(|n| self.borrow_map.get(&n)) {
        through.extend(src.through.iter().cloned());
      }
      if !self.ty_is_copy(ty, init.hir_id.owner) {
        if let Some(src) = expr_to_rap_name(init, &self.tcx) {
          self.borrow_map.remove(&src);
        }
      }
    }
    let line = expr_to_line(&init, &self.tcx);
    let lender = lenders.remove(0);
    let aliasing = get_aliasing_data(&lender, &self.borrow_map);
    // A borrow of a temporary the `let` extends (`Some(&mut
    // String::new())`, #1357): MIR names no local for it, so its loan
    // is found by the `&`'s span, as for `let r = &String::new();`.
    let borrow_span = match &lender {
      ResourceTy::Value(t) if lenders.is_empty() => self.ext_temp_borrow_span(t),
      _ => None,
    };
    self.borrow_map.insert(name.clone(), RefData {
      local: self.raps.local(&name),
      lender,
      assigned_at: line,
      lifetime: line,
      ref_mutability: is_mut,
      aliasing,
      static_lender: None,
      borrow_span,
      loan_line: None,
      co_lenders: lenders,
      lender_muts,
      through,
    });
  }

  fn closure_result(&self, holder: Option<Local>, call: &Expr<'_>) -> Option<&'a crate::facts::schema::ClosureResultLoans> {
    let sites = self.body_facts?.closure_result_loans.get(&call.hir_id)?;
    // One HIR expression can evaluate in several copies of a guard. Its
    // single drawn call cannot select a MIR occurrence in that case.
    if sites.len() != 1 { return None; }
    let [Some(result)] = sites.first_key_value()?.1.as_slice() else { return None };
    (Some(result.holder) == holder).then_some(result)
  }

  pub fn has_closure_result(&self, holder: &str, value: &'tcx Expr<'tcx>) -> bool {
    let Some(value) = branch_value_expr(value) else { return false };
    self.closure_result(self.raps.root_local(holder), value).is_some_and(|r| r.holders.contains_key(holder))
      || holder_branch_bodies(value).is_some_and(|arms|
        arms.into_iter().any(|arm| self.has_closure_result(holder, arm)))
  }

  fn closure_result_lender(&self, loan: &crate::facts::LoanInfo) -> Option<ResourceTy> {
    if !matches!(loan.kind, "shared" | "mut") || loan.through.is_some() { return None; }
    self.raps.place(loan.lender_local?, &loan.lender).map(|r| ResourceTy::Value(r.rap.clone()))
  }

  fn record_lender_kind(kinds: &mut Vec<(ResourceTy, bool)>, lender: &ResourceTy, mutable: bool) {
    if let Some((_, was_mutable)) = kinds.iter_mut().find(|(l, _)| l == lender) {
      *was_mutable |= mutable;
    } else {
      kinds.push((lender.clone(), mutable));
    }
  }

  /// Visit the value-producing calls individually so an arm contributes
  /// only the loans recorded at its own MIR call.
  fn extend_closure_result_lenders(&self, root: &str, holder: &str, value: &'tcx Expr<'tcx>,
                                  lenders: &mut Vec<ResourceTy>, kinds: &mut Vec<(ResourceTy, bool)>) {
    let Some(facts) = self.body_facts.filter(|f| !f.closure_result_loans.is_empty()) else { return };
    let Some(value) = branch_value_expr(value) else { return };
    if let Some(arms) = holder_branch_bodies(value) {
      for arm in arms {
        self.extend_closure_result_lenders(root, holder, arm, lenders, kinds);
      }
      return;
    }
    let Some(result) = self.closure_result(self.raps.root_local(root), value).and_then(|r| r.holders.get(holder)) else { return };
    for loan in facts.loans.iter().filter(|l| result.loans.contains(&l.loan)) {
      let Some(lender) = self.closure_result_lender(loan) else { continue };
      Self::record_lender_kind(kinds, &lender, loan.kind == "mut");
      if !lenders.contains(&lender) {
        lenders.push(lender);
      }
    }
  }

  /// Keep the signature arrow only if a distinct ordinary argument loan
  /// can account for it. One loan carried by both operands gets one arrow.
  fn closure_result_supplies_lender(&self, root: Option<Local>, holder: &str, call: &Expr<'_>, lender: &ResourceTy) -> bool {
    let Some(result) = self.closure_result(root, call).and_then(|r| r.holders.get(holder)) else { return false };
    let Some(facts) = self.body_facts else { return false };
    let mut matching = facts.loans.iter().filter(|l| self.closure_result_lender(l).as_ref() == Some(lender));
    matching.clone().any(|l| result.loans.contains(&l.loan))
      && !matching.any(|l| result.argument_loans.contains(&l.loan) && !result.loans.contains(&l.loan))
  }

  /// Whether a branch of the `if` / `match` `init` that a `let` binds a
  /// value of type `ty` from gives it a value that borrows something
  /// known only once the branch is visited: a temporary the branch makes
  /// (`Some(&String::from("x"))`, #1397), or a variable it declares
  /// (`{ let y = &s; Some(y) }`, #994). Such a branch is paired with the
  /// `let` after it is visited (`match_rhs_arm`), when
  /// `add_arm_holder_lenders` adds its lenders (#1435). So does each
  /// `break` of a `loop` initializer (`let l = loop { .. break
  /// Some(&u); }`), as it is visited (`pair_break_value`).
  fn holder_lenders_in_branch(&self, init: &'tcx Expr<'tcx>, ty: Ty<'tcx>) -> bool {
    if ty.is_ref() || !ty.walk().any(|g| matches!(g.kind(), GenericArgKind::Lifetime(_))) {
      return false;
    }
    if matches!(init.kind, ExprKind::Loop(_, _, rustc_hir::LoopSource::Loop, _)) && !init.span.from_expansion() {
      return breaks_to(init).iter().any(|(_, value)| value.is_some());
    }
    let Some(within) = self.defer_arm_binds else { return false };
    crate::ref_flow::holder_branch_bodies(init)
      .is_some_and(|bodies| bodies.iter().any(|b| self.arm_value_uses_local_in(b, within)))
  }

  /// The span of the `&` that borrows the `let`-extended temporary
  /// column `tmp` (or a field column of it), if one does
  /// (`nested_ext_temps`).
  fn ext_temp_borrow_span(&self, tmp: &ResourceAccessPoint) -> Option<rustc_span::Span> {
    #[expect(clippy::disallowed_methods, reason = "a temporary's column is recorded under one `&` only; the minimum keeps it deterministic")]
    let span = self.nested_ext_temps.iter()
      .filter(|(_, t)| t.hash() == tmp.hash() && t.name() == tmp.name())
      .map(|(h, _)| self.tcx.hir_span(*h))
      .min();
    span
  }

  /// Whether `init` moves a `&mut` parameter into the value it builds
  /// (`Some(r)`, `Box::new(r)`, `vec![r]`, `[r]`, `id(Some(r))`: no
  /// reborrow, see `mut_ref_param_moved_into_call`).
  pub fn moves_mut_ref_param_in(&self, init: &'tcx Expr<'tcx>) -> bool {
    let mut found = Vec::new();
    self.mut_ref_params_moved_in(init, &mut found);
    !found.is_empty()
  }

  /// The `&mut` parameters `e` moves into the value it builds (see
  /// `moves_mut_ref_param_in`).
  pub fn mut_ref_params_moved_in(&self, e: &'tcx Expr<'tcx>, out: &mut Vec<&'tcx Expr<'tcx>>) {
    if self.mut_ref_param_moved_into_call(e) {
      out.push(e);
      return;
    }
    let inner: Vec<&'tcx Expr<'tcx>> = match e.kind {
      ExprKind::Call(_, args) => args.iter().collect(),
      ExprKind::MethodCall(_, rcvr, args, _) => std::iter::once(rcvr).chain(args.iter()).collect(),
      ExprKind::Array(elems) | ExprKind::Tup(elems) => elems.iter().collect(),
      ExprKind::DropTemps(x) => vec![x],
      ExprKind::Block(b, _) => b.expr.into_iter().collect(),
      _ => Vec::new(),
    };
    for x in inner {
      self.mut_ref_params_moved_in(x, out);
    }
  }

  /// The lenders `holder_lenders` found for the value a call returns
  /// follow the call's arguments as a whole: `v.iter().copied()
  /// .collect()` gets v, which `iter` borrows, though the result holds
  /// only the references stored in v (`copied` and `collect` keep the
  /// items, `&'a String`, not the iterator's borrow of v). Keep only
  /// those whose loans rustc's borrow checker puts in the lifetimes of
  /// `name`'s type (`loans_held_by`), as for a reference result
  /// (`narrow_call_result_lenders`, #756). A lender with no loan in
  /// this body (a reference, a place behind one, a variable of another
  /// body) stays. Returns whether a lender was dropped (#1094).
  /// The lenders the value reaches only through another one
  /// (`split_through_lenders`) go to `through`.
  fn narrow_holder_lenders(
    &self,
    name: &str,
    init: &'tcx Expr<'tcx>,
    lenders: &mut Vec<ResourceTy>,
    lender_muts: &mut Vec<(ResourceTy, bool)>,
    through: &mut Vec<ResourceTy>,
  ) -> bool {
    if lenders.is_empty() || !matches!(init.kind, ExprKind::Call(..) | ExprKind::MethodCall(..)) {
      return false;
    }
    let mut parts = name.split('.');
    let base = parts.next().unwrap_or_default();
    let fields: Vec<&str> = parts.collect();
    let Some(held) = self.loans_held_by(base, &fields) else { return false };
    let kept = |l: &ResourceTy| match l {
      ResourceTy::Value(r) if !r.is_ref() => {
        let lb = r.name().split('.').next().unwrap_or_default();
        held.contains_key(lb) || !self.is_body_local(lb)
      }
      _ => true,
    };
    let before = lenders.len();
    lenders.retain(|l| kept(l));
    lender_muts.retain(|(l, _)| kept(l));
    let dropped = lenders.len() != before;
    through.extend(self.split_through_lenders(name, lenders, lender_muts));
    dropped
  }

  /// `let r = call;` with r a reference that the callee's signature
  /// ties to several lenders: keep the ones rustc's borrow checker puts
  /// in r's lifetimes (`narrow_holder_lenders`), and only those r
  /// doesn't reach through another one (`split_through_lenders`).
  /// `let el = it.next().unwrap();` with `it = refs.iter_mut()` points
  /// into refs, not at a1 (#1399); `let r = it.copied().next()
  /// .unwrap();` with `it = refs.iter()` is the `&String` copied out of
  /// refs, which borrows a1, not refs.
  /// (Only when r is bound to the whole value: a binding in a pattern,
  /// `let (x, y) = two(&c, &d);`, is `narrow_call_result_lenders`'.)
  fn narrow_ref_call_lenders(&mut self, name: &str, init: &'tcx Expr<'tcx>, ty: Ty<'tcx>) {
    let erase = |t: Ty<'tcx>| self.tcx.erase_and_anonymize_regions(t);
    if erase(self.tcx.typeck(init.hir_id.owner).expr_ty_adjusted(init)) != erase(ty) {
      return;
    }
    let init = try_operand(init).unwrap_or(init);
    if !matches!(init.kind, ExprKind::Call(..) | ExprKind::MethodCall(..)) || init.span.from_expansion() {
      return;
    }
    let Some(rd) = self.borrow_map.get(name) else { return };
    if rd.co_lenders.is_empty() {
      return;
    }
    let before = rd.all_lenders();
    let mut lenders = before.clone();
    let mut muts = rd.lender_muts.clone();
    let mut through = Vec::new();
    self.narrow_holder_lenders(name, init, &mut lenders, &mut muts, &mut through);
    // (Not when rustc puts none of them in r's lifetimes.)
    if lenders == before || lenders.is_empty() {
      return;
    }
    let aliasing = get_aliasing_data(&lenders[0], &self.borrow_map);
    if let Some(rd) = self.borrow_map.get_mut(name) {
      rd.lender = lenders[0].clone();
      rd.co_lenders = lenders[1..].to_vec();
      rd.lender_muts = muts;
      rd.aliasing = aliasing;
      rd.through = through;
    }
  }

  /// Moves out of `lenders` the variables that `name`'s value (a
  /// reference, or a value holding one) reaches only through another
  /// lender, and returns them: `let el = refs.iter_mut().next()
  /// .unwrap();` / `let it = refs.iter_mut();` on a `Vec<&String>`
  /// point into refs; the `&String` in refs borrows a1, but a1 isn't
  /// lent to el or it, as for `&mut refs[0]` (#1382, #1399). rustc's
  /// borrow checker says which loans are in which part of the type
  /// (`loans_split_by`), so this holds through any chain of calls
  /// (`refs.iter_mut().rev().next()`, `it.next()` on a local iterator).
  /// Only a variable of this body, with a loan only there, and only
  /// when another lender stays.
  pub fn split_through_lenders(
    &self,
    name: &str,
    lenders: &mut Vec<ResourceTy>,
    lender_muts: &mut Vec<(ResourceTy, bool)>,
  ) -> Vec<ResourceTy> {
    if lenders.len() < 2 {
      return Vec::new();
    }
    let mut parts = name.split('.');
    let base = parts.next().unwrap_or_default();
    let fields: Vec<&str> = parts.collect();
    let Some((own, through)) = self.loans_split_by(base, &fields) else { return Vec::new() };
    let only_through = |l: &ResourceTy| match l {
      ResourceTy::Value(r) if !r.is_ref() => {
        let lb = r.name().split('.').next().unwrap_or_default();
        through.contains(lb) && !own.contains_key(lb) && self.is_body_local(lb)
      }
      _ => false,
    };
    let (gone, kept): (Vec<ResourceTy>, Vec<ResourceTy>) = lenders.iter().cloned().partition(|l| only_through(l));
    if gone.is_empty() || kept.is_empty() {
      return Vec::new();
    }
    *lenders = kept;
    lender_muts.retain(|(l, _)| !gone.contains(l));
    if lender_muts.iter().all(|(_, m)| *m) || lender_muts.iter().all(|(_, m)| !*m) {
      lender_muts.clear();
    }
    gone
  }

  /// Marks `name`'s column as a borrow holder: an owner, or a struct
  /// member drawn as one column (`shelf.items` with `items: Vec<&'a
  /// str>`, #1088). False for any other column.
  pub fn mark_holder(&mut self, name: &str) -> bool {
    let Some(rd) = self.raps.get_mut(name) else { return true };
    match &mut rd.rap {
      ResourceAccessPoint::Owner(o) => o.holds_borrow = true,
      ResourceAccessPoint::Struct(s) if s.is_member => s.holds_borrow = true,
      _ => return false,
    }
    true
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
        passed_on: Vec::new(),
      };
      let id = *self.unique_id;
      *self.unique_id += 1;
      let ev = if k.is_mut {
        ExternalEvent::MutableBorrow { from: from.clone(), to: to.clone(), is_partial: false, is_reborrow: k.through_ref, break_alt: None, via: Some(via), id, of_ref_var: false }
      } else {
        ExternalEvent::StaticBorrow { from: from.clone(), to: to.clone(), is_partial: false, is_reborrow: k.through_ref, break_alt: None, via: Some(via), id, of_ref_var: false }
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
  /// `call_stores_into_receiver`, #392), or is later assigned a whole
  /// new value (`let mut q: Option<&String> = None; q = Some(r);`,
  /// #875): `reassign_holder` then records the new value's borrow.
  ///
  /// A field of a struct literal bound by a `let` (`let mut shelf =
  /// Shelf { items: Vec::new() };`) is filled the same way through its
  /// place (`shelf.items.push(w.as_str())`, #1096).
  fn filled_with_borrows_later(&self, init: &'tcx Expr<'tcx>, ty: Ty<'tcx>) -> bool {
    if ty.is_ref() || !ty.walk().any(|g| matches!(g.kind(), GenericArgKind::Lifetime(_))) {
      return false;
    }
    // The field names from the `let`'s variable down to `init`,
    // outermost first (`["items"]`).
    let mut path: Vec<rustc_span::Symbol> = Vec::new();
    let mut e = init;
    let local = loop {
      match self.tcx.parent_hir_node(e.hir_id) {
        rustc_hir::Node::LetStmt(local) => break local,
        rustc_hir::Node::ExprField(f) => {
          let rustc_hir::Node::Expr(lit) = self.tcx.parent_hir_node(f.hir_id) else { return false };
          if !matches!(lit.kind, ExprKind::Struct(..)) {
            return false;
          }
          path.insert(0, f.ident.name);
          e = lit;
        }
        _ => return false,
      }
    };
    let PatKind::Binding(_, hir_id, _, None) = local.pat.kind else { return false };
    struct StoreFinder<'v, 'a, 'tcx> {
      ev: &'v ExprVisitor<'a, 'tcx>,
      local: rustc_hir::HirId,
      path: Vec<rustc_span::Symbol>,
      found: bool,
    }
    impl<'v, 'a, 'tcx> rustc_hir::intravisit::Visitor<'tcx> for StoreFinder<'v, 'a, 'tcx> {
      fn visit_expr(&mut self, e: &'tcx Expr<'tcx>) {
        // `x` itself, or the place `x.f.g` for `path` `[f, g]`.
        fn is_place(x: &Expr<'_>, local: rustc_hir::HirId, path: &[rustc_span::Symbol]) -> bool {
          match (x.kind, path.split_last()) {
            (ExprKind::Path(QPath::Resolved(None, p)), None) => p.res == Res::Local(local),
            (ExprKind::Field(base, ident), Some((last, rest))) => ident.name == *last && is_place(base, local, rest),
            _ => false,
          }
        }
        let is_local = |x: &Expr<'_>| is_place(x, self.local, &self.path);
        match e.kind {
          ExprKind::MethodCall(_, rcvr, _, _) if is_local(rcvr) && self.ev.call_stores_into_receiver(e) => {
            self.found = true;
          }
          ExprKind::Assign(lhs, _, _) if is_local(lhs) => self.found = true,
          _ => {}
        }
        rustc_hir::intravisit::walk_expr(self, e);
      }
    }
    let mut finder = StoreFinder { ev: self, local: hir_id, path, found: false };
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
    let name = rcvr.name().to_owned();
    let Some(facts) = self.body_facts else { return };
    let Some(inputs) = facts.receiver_loans.get(&(call.span.with_parent(None), name.clone())) else { return };
    let inputs = self.receiver_input_loans(call, inputs);
    let operands: Vec<_> = self.receiver_aggregate_operands(call).into_iter()
      .filter(|b| b.loan.projected.is_none() || inputs.contains_key(&b.loan.loan)).collect();
    use crate::facts::StoredOperand;
    struct Store {
      place: ResourceTy,
      exclusive: bool,
      binding: bool,
      unique_capture: bool,
      operand: StoredOperand,
      projected: Option<(usize, crate::facts::ProjectedPlace)>,
    }
    let mut stored: Vec<Store> = Vec::new();
    for (&id, &operand) in &inputs {
      let Some(loan) = facts.loans.iter().find(|l| l.loan == id) else { continue };
      let target = match (&loan.through, loan.source) {
        _ if self.raps.contains_key(&loan.lender) => &loan.lender,
        // A field inside an element (`p2.0[0].0`) has no column of its own.
        // The loan restricts the columns that contain it; the first is drawn.
        (None, _) if loan.part_of.iter().any(|c| self.raps.contains_key(c)) => loan.part_of.iter().find(|c| self.raps.contains_key(*c)).unwrap(),
        (Some(through), _) => through,
        (None, "caller") => match loan.direct_holders.first() { Some(param) => param, None => continue },
        _ => &loan.lender,
      };
      // A caller loan describes the incoming reference. Its use through a
      // fresh loan of this binding is already represented by that loan.
      if loan.source == "caller" && facts.loans.iter().any(|other| inputs.contains_key(&other.loan)
          && other.source != "caller" && (other.lender == *target || other.through.as_ref() == Some(target))) { continue; }
      let Some(lender) = self.raps.get(target) else { continue };
      self.row_layout.stored_lender_loans.entry(*lender.rap.hash()).or_default().insert(id);
      let place = ResourceTy::Value(lender.rap.clone());
      let binding = lender.rap.is_ref() && target == &loan.lender;
      let exclusive = matches!(loan.kind, "mut" | "unique");
      let projected = loan.projected.clone().map(|place| (id, place));
      if projected.is_some() && operand == StoredOperand::Carrier {
        self.row_layout.uncertain_projected_stores.insert((*rcvr.hash(), id));
      } else if projected.is_some() && operand == StoredOperand::SharedReference && exclusive {
        self.row_layout.shared_projected_stores.insert((*rcvr.hash(), id));
      }
      if loan.kind == "unique" { self.stored_unique_captures.insert((*rcvr.hash(), id)); }
      if let Some(prior) = stored.iter_mut().find(|s| s.place == place && projected.is_none() && s.projected.is_none()) {
        prior.exclusive |= exclusive;
        prior.binding &= binding;
        prior.unique_capture &= loan.kind == "unique";
        if prior.operand != operand { prior.operand = StoredOperand::Carrier; }
      } else {
        stored.push(Store { place, exclusive, binding, unique_capture: loan.kind == "unique", operand, projected });
      }
    }
    let mut lenders = Vec::new();
    for store in &stored {
      if !lenders.contains(&store.place) { lenders.push(store.place.clone()); }
    }
    if lenders.is_empty() { return; }
    for s in &stored {
      if s.binding { self.stored_ref_var_lends.insert((name.clone(), s.place.real_name())); }
    }
    let any_mut = stored.iter().any(|s| s.exclusive);
    match self.borrow_map.get_mut(&name) {
      Some(rd) => {
        let mut muts: Vec<_> = rd.all_lenders().into_iter().map(|l| {
          let m = rd.mut_of(&l);
          (l, m)
        }).collect();
        for s in &stored {
          let l = &s.place;
          if *l != rd.lender && !rd.co_lenders.contains(l) {
            rd.co_lenders.push(l.clone());
          }
          match muts.iter_mut().find(|(x, _)| x == l) {
            Some(entry) => entry.1 |= s.exclusive,
            None => muts.push((l.clone(), s.exclusive)),
          }
        }
        rd.ref_mutability |= any_mut;
        rd.lender_muts = if muts.iter().all(|(_, m)| *m == rd.ref_mutability) { Vec::new() } else { muts };
      }
      None => {
        let aliasing = get_aliasing_data(&lenders[0], &self.borrow_map);
        let local = self.raps.local(&name);
        self.borrow_map.insert(name, RefData {
          local,
          lender: lenders[0].clone(),
          assigned_at: line,
          lifetime: line,
          ref_mutability: any_mut,
          aliasing,
          static_lender: None,
          borrow_span: None,
          loan_line: None,
          co_lenders: lenders[1..].to_vec(),
          lender_muts: if stored.iter().all(|s| s.exclusive == any_mut) { Vec::new() }
            else { lenders.iter().map(|l| (l.clone(), stored.iter().any(|s| s.place == *l && s.exclusive))).collect() },
          through: Vec::new(),
        });
      }
    }
    let via = BorrowVia {
      func: seg.ident.as_str().to_owned(),
      lenders: lenders.iter().map(|l| l.name()).collect(),
      also_unnamed: false,
      tie: ArgTie::default(),
      carrier: ViaCarrier::StoredBy,
      passed_on: Vec::new(),
    };
    let to = ResourceTy::Value(rcvr.clone());
    let aggregate: Vec<_> = lenders.iter().filter(|l| operands.iter().any(|b| b.lender == **l)).cloned().collect();
    if !aggregate.is_empty() {
      let rd = self.borrow_map[rcvr.name()].clone();
      self.draw_holder_borrows(&to, &aggregate, &rd, &operands, line, Some(&via));
    }
    for Store { place: lender, exclusive: is_mut, binding: of_ref_var, unique_capture, operand, projected } in stored.into_iter().filter(|s| {
      match &s.projected {
        Some((id, _)) => !operands.iter().any(|b| b.lender == s.place && b.loan.loan == *id),
        None => !aggregate.contains(&s.place),
      }
    }) {
      let id = *self.unique_id;
      if let Some(projected) = projected {
        self.row_layout.projected_loan_places.insert(id, projected);
      }
      let detail = if unique_capture {
        Some(crate::event_order::StoredLoan::UniqueCapture)
      } else if operand == StoredOperand::Carrier {
        Some(crate::event_order::StoredLoan::Carrier { mutable: is_mut, binding: of_ref_var })
      } else if operand == StoredOperand::SharedReference && (is_mut || inputs.len() > 1) {
        Some(crate::event_order::StoredLoan::SharedInput { mutable: is_mut, binding: of_ref_var })
      } else { None };
      if let Some(detail) = detail {
        self.row_layout.stored_loans.insert(id, detail);
      }
      *self.unique_id += 1;
      let ev = if is_mut {
        ExternalEvent::MutableBorrow { from: lender.clone(), to: to.clone(), is_partial: false, is_reborrow: false, break_alt: None, via: Some(via.clone()), id, of_ref_var }
      } else {
        ExternalEvent::StaticBorrow { from: lender.clone(), to: to.clone(), is_partial: false, is_reborrow: false, break_alt: None, via: Some(via.clone()), id, of_ref_var }
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
        let local = self.raps.local(&field_name);
        self.borrow_map.insert(field_name, RefData {
          local,
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
          through: Vec::new(),
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
  /// [`collect_pat_binding_events`], except that a sub-pattern matching a part of
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
    // `&v => …` on `match r` with `r: &i32`: the `&` pattern matches
    // r's reference, and `v` is matched against the value it points
    // to, `*r`, exactly as in `v => …` on `match *r` (#880).
    if let (PatKind::Ref(inner, _, _), ResourceTy::Value(r), rustc_middle::ty::Ref(_, pointee, _)) = (pat.kind, parent, parent_ty.kind()) {
      if !r.is_fn() {
        self.deref_ref_pats.insert(pat.hir_id);
        self.get_dec_of_pat_parts(inner, ty_results, &ResourceTy::Deref(r.clone()), *pointee, scope, res);
        return;
      }
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
              self.collect_pat_binding_events(sp, ty_results, parent, &parent_ty, scope, &mut sub);
              res.extend(sub.into_iter().map(|(r, e, ty)| {
                let partial = (copies_from_inside(&e, ty, parent_ty, parent)
                  && !self.pointee_copies.contains_key(r.hash())).then_some(true);
                (r, e, parent.clone(), partial)
              }));
            }
          }
        }
      }
      None => {
        let mut sub = Vec::new();
        self.collect_pat_binding_events(pat, ty_results, parent, &parent_ty, scope, &mut sub);
        res.extend(sub.into_iter().map(|(r, e, ty)| {
          let partial = (copies_from_inside(&e, ty, parent_ty, parent)
            && !self.pointee_copies.contains_key(r.hash())).then_some(true);
          let from = self.slice_elem_from(pat, &r, &e, parent);
          (r, e, from, partial)
        }));
      }
    }
  }

  /// `[c, _] => …` on `match pair`, with pair an array of which some
  /// elements stay (`Owner::elems_left`): c is moved out of `pair[0]`,
  /// not out of all of pair (#1285). The move's source is that element,
  /// `ResourceTy::Index(pair, "pair[0]")`, so pair's dot says only the
  /// element is moved and its column goes on, holding the rest. `from`
  /// otherwise.
  pub fn slice_elem_from(&self, pat: &'tcx Pat<'tcx>, r: &ResourceAccessPoint, e: &Evt, from: &ResourceTy) -> ResourceTy {
    if !matches!(e, Evt::Move) || !matches!(from, ResourceTy::Value(ResourceAccessPoint::Owner(_))) {
      return from.clone();
    }
    let Some(local) = r.owner_local().or_else(|| self.raps.local(r.name())) else { return from.clone() };
    let mut hirid = None;
    pat.each_binding(|_, id, _, _| {
      if self.raps.bindings.declarations.get(&id) == Some(&local) { hirid = Some(id); }
    });
    hirid.and_then(|id| self.slice_elem_source(id, from)).unwrap_or_else(|| from.clone())
  }

  /// The element place (`pair[0]`, or `three[1..3]` for a `rest @ ..`)
  /// that the by-value binding `hirid` takes out of the array `from`
  /// through a slice pattern, when `from` is an array some of whose
  /// elements are left in it (`Owner::elems_left`, #1144, #1166). A name
  /// bound inside an element's pattern (`[Some(a), _]`) takes from that
  /// element. None for anything else.
  pub fn slice_elem_source(&self, hirid: rustc_hir::HirId, from: &ResourceTy) -> Option<ResourceTy> {
    let ResourceTy::Value(rap) = from else { return None };
    if !(rap.elems_left().is_some() || matches!(rap, ResourceAccessPoint::Struct(s) if s.is_member)) { return None; }
    let facts = self.body_facts?;
    let local = facts.bindings.declarations.get(&hirid)?;
    let pattern = self.tcx.hir_span(hirid);
    let mut sites = facts.binding_sources.iter().filter(|s| s.declaration == *local && pattern.contains(s.site) && s.moves);
    let element = sites.next()?.projected_move.as_ref()?;
    if !sites.all(|s| s.projected_move.as_ref() == Some(element)) { return None; }
    let (source, column, place) = element;
    if Some(*source) != rap.scope_local() || column != rap.name() { return None; }
    if rap.is_member() && !facts.scope_ends.for_place(*source, column).iter().any(|e|
        e.effect == crate::facts::ScopeDrop::Remaining && !e.optional_unwind) { return None; }
    Some(ResourceTy::Index(rap.clone(), place.clone(), PlaceWrite::Update))
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
      rd.lender_muts = src.lender_muts;
    }
  }

  // Add a RAP for each variable bound in a pattern, with the event that
  // binds it and its type.
  pub fn collect_pat_binding_events<'t>(
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
                self.collect_pat_binding_events(p, ty_results, parent, parent_ty, scope, res);
            }
        }
        // `Some(x)` / `None` lower to `PatKind::Struct(QPath::LangItem(...))`
        // with one PatField wrapping the user pattern (rustc's
        // `pat_lang_item_variant`). Recurse into each field's inner
        // pattern with the same parent — same flow as TupleStruct, just
        // a different HIR shape.
        PatKind::Struct(_, fields, _) => {
            for field in fields.iter() {
                self.collect_pat_binding_events(field.pat, ty_results, parent, parent_ty, scope, res);
            }
        }
        // `[a, b] => …`, `if let [first, .., last] = arr`, `[x, rest @ ..]`
        // (#1144): each element the pattern names (and a `rest @ ..`
        // sub-slice) destructures out of the same parent, as a tuple's do.
        PatKind::Slice(before, mid, after) => {
            for p in before.iter().chain(mid).chain(after.iter()) {
                self.collect_pat_binding_events(p, ty_results, parent, parent_ty, scope, res);
            }
        }
        PatKind::Binding(mode, id, ident, sub_pat) => {
            // add raps that appear in binding
            let muta = bool_of_mut(mode.1); // You need to define this function
            let line_num = span_to_line(&ident.span, &self.tcx); // Assuming self.span_to_line exists
            let ty: Ty<'tcx> = ty_results.node_type(id);
            let name = self.raps.declare(id, &self.tcx);
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
                let owner_hash = self.struct_column_hash(&name);
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
                        let owner_hash = self.struct_column_hash(&name);
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
                              self.add_external_event(line_num, ExternalEvent::InitRefParam { param: r, id, own_param: None, in_pattern: Some(name.clone()), rest: None });
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
                            self.mark_owner_type(&name, ty);
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
            // Under a `&` sub-pattern (`Some(&y)` on an `Option<&i32>`),
            // a by-value binding copies the value the stored reference
            // points to (#874).
            // Not under one matched against a reference column, which
            // is matched against `*r` instead (#880).
            let behind_ref = ref_pattern_above_at(&self.tcx, id)
              .filter(|(ref_pat, _)| !self.deref_ref_pats.contains(ref_pat))
              .map(|(_, part)| part);
            let evt = if let Some(mutbl) = binding_ref {
                if bool_of_mut(mutbl) { Evt::MBorrow } else { Evt::SBorrow }
            } else if behind_ref.is_some() {
                if self.ty_is_copy(ty, pat.hir_id.owner) { Evt::Copy } else { Evt::Move }
            } else if parent_ty.is_ref() && *parent_ty != ty {
                if bool_of_mut(parent_ty.ref_mutability().unwrap()) { Evt::MBorrow }
                else { Evt::SBorrow }
            } else {
                if self.ty_is_copy(ty, pat.hir_id.owner) { Evt::Copy }
                else { Evt::Move }
            };

            let rap = self.raps.get(&name).unwrap().rap.to_owned();
            if let (Some(part), Evt::Copy) = (behind_ref, &evt) {
                self.pointee_copies.insert(*rap.hash(), (parent.clone(), part));
            }
            res.push((rap, evt, ty));
            // `whole @ Some(n)`: the names bound inside the sub-pattern
            // are bound from the same matched value (copies, or `ref`
            // borrows; #1177).
            if let Some(sp) = sub_pat {
                self.collect_pat_binding_events(sp, ty_results, parent, parent_ty, scope, res);
            }
        }
        // `Some(&y)` on an `Option<&i32>`: y copies the value the
        // reference inside the matched value points to (#874). A `&`
        // pattern matched against a reference column itself (`&v` on
        // `r: &i32`) is matched against `*r` in `get_dec_of_pat_parts`
        // (#880).
        PatKind::Ref(inner, _, _) if !parent_ty.is_ref() => {
            self.collect_pat_binding_events(inner, ty_results, parent, parent_ty, scope, res);
        }
        // `Ok(s) | Err(s)`: every alternative binds the same names, with
        // the same types and binding modes, out of the same matched
        // value, so whichever one matches, the bindings are the first
        // alternative's (#503).
        PatKind::Or(alts) => {
            if let Some(first) = alts.first() {
                self.collect_pat_binding_events(first, ty_results, parent, parent_ty, scope, res);
            }
        }
        _ => {}
    }
}

/// Place the ordinary binding's end where rustc ends its storage or
/// drops it. The enclosing visitor supplies the drawing leg, including
/// a collapsed match arm. Its lexical boundary is only needed for
/// synthetic columns and the explanations of non-returning control flow.
pub(crate) fn scope_event(&mut self, ro: ResourceAccessPoint, layout_end: usize, leg: Option<rustc_span::Span>) -> Option<(usize, ExternalEvent)> {
    let id = *self.unique_id;
    let mut line = layout_end;
    let mut exit_site = None;
    if let Some(local) = ro.scope_local() {
        let facts = self.body_facts?;
        let ends: Vec<_> = facts.scope_ends.for_place(local, ro.name()).iter().filter(|e| !e.optional_unwind).collect();
        let normal: Vec<_> = ends.iter().copied().filter(|end| end.site.is_none()).collect();
        let within_leg: Vec<_> = normal.iter().copied().filter(|end|
            leg.is_some_and(|span| span.contains(end.span))).collect();
        let entered: Vec<_> = normal.iter().copied().filter(|end| leg.is_some_and(|span| end.legs.contains(&span))).collect();
        let candidates = if !entered.is_empty() { &entered } else if !within_leg.is_empty() { &within_leg } else { &normal };
        // An inline exit can represent the end of its enclosing column.
        let end = if candidates.is_empty() {
            crate::facts::ScopeEnd::across(&ends).or_else(|| {
                // All returns diverge before the closing brace. A Copy
                // column can still label its binding's scope without
                // claiming that an instruction executes at the brace.
                let mut summary = (*ends.first()?).clone();
                if !ends.iter().all(|e| e.effect == crate::facts::ScopeDrop::Storage) { return None; }
                summary.span = *facts.scope_ends.boundaries.get(&local)?;
                summary.boundary = self.tcx.sess.source_map().lookup_char_pos(summary.span.source_callsite().hi()).line;
                summary.site = None;
                summary.effect = crate::facts::ScopeDrop::ExitStorage;
                Some(summary)
            })
        } else {
            crate::facts::ScopeEnd::across(candidates)
        };
        if let Some(end) = end {
            line = if self.dead_ends.contains_key(&layout_end) { layout_end } else { end.boundary };
            exit_site = end.site;
            self.row_layout.scope_ends.insert(id, end);
        } else if !self.dead_ends.contains_key(&layout_end) {
            let calls = facts.scope_ends.terminal_calls.get(&local)?;
            let mut reachable = calls.iter().filter(|(site, _)| leg.is_none_or(|span| span.contains(*site)));
            let &(site, row) = reachable.next()?;
            if !reachable.all(|other| other.0 == site) { return None; }
            line = row;
            self.row_layout.terminal_calls.insert(id);
        }
    }
    *self.unique_id += 1;
    Some((line, ExternalEvent::GoOutOfScope { ro, id, exit_site, early: None }))
}

fn add_scope_end(&mut self, ro: ResourceAccessPoint, layout_end: usize) {
    if let Some((row, event)) = self.scope_event(ro, layout_end, None) {
        self.add_external_event(row, event);
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
    // (Or a later variable sharing the column of a global one, #931.)
    let is_block_local = |r: &ResourceAccessPoint| {
      self.raps.get(r.name()).map_or(false, |rd| (!rd.is_global || rd.local_later) && rd.rap.hash() == r.hash())
    };
    // An unbound temporary (#457) is dropped by its own event, at the
    // end of its statement, not with the block.
    let mut vars: Vec<ResourceAccessPoint> = vars.into_iter().filter(|r| is_block_local(r) && needs_scope_end(r)).collect();
    // A struct's field columns (`p.name`) go out of scope with it.
    let owners: HashSet<u64> = vars.iter().filter_map(|r| match r {
      ResourceAccessPoint::Struct(st) if !st.is_member => Some(st.hash),
      _ => None,
    }).collect();
    let mut group = owners.clone();
    for rd in self.raps.values() {
      if let ResourceAccessPoint::Struct(st) = &rd.rap {
        if st.is_member && owners.contains(&st.owner) && (!rd.is_global || rd.local_later) {
          vars.push(rd.rap.clone());
          group.insert(st.hash);
        }
      }
    }
    // So do its reference fields (`h2.r` of `let h2 = Holder { r: h.r };`
    // in an arm, #1421), as at the function's end.
    for rd in self.raps.values() {
      if let ResourceAccessPoint::MutRef(MutRef { member_of: Some(m), .. })
        | ResourceAccessPoint::StaticRef(StaticRef { member_of: Some(m), .. }) = &rd.rap {
        if group.contains(m) && (!rd.is_global || rd.local_later) {
          vars.push(rd.rap.clone());
        }
      }
    }
    vars.sort_by_key(|r| *r.hash());
    vars.dedup_by_key(|r| *r.hash());
    for var in vars {
        self.add_scope_end(var, line);
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
/// `body_end`: the last line of the body the bindings are in scope in
/// (of the match arm, for an `if let` guard). An or-pattern's
/// reference bound from different places (`if let (Some(ref x), _) |
/// (_, Some(ref x)) = t`) keeps every place borrowed until its last use
/// there (#734).
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
    body_end: usize,
) -> (HashSet<ResourceAccessPoint>, usize) {
    let scrutinee = let_expr.init;
    let typeck_res = self.tcx.typeck(scrutinee.hir_id.owner);
    let scrutinee_ty = typeck_res.node_type(scrutinee.hir_id);
    // `if let Some(n) = &cur.next` with cur a reference (#266).
    // `if let (s, n) = *b` on a Box whose payload's parts have
    // columns destructures `b.0` / `b.1` (#892).
    let parent = self.box_payload_parent(scrutinee, let_expr.pat)
        .unwrap_or_else(|| self.scrutinee_rap(scrutinee));

    let let_line = crate::expr_visitor_utils::span_to_line(&let_expr.span, &self.tcx);
    // Body-end is only needed if we're going to emit a GoOutOfScope.
    // collect_pat_binding_events also takes a `scope` param; pass let_line as a
    // safe default for the if-let case (the renderer ignores it for
    // the binding's own RAP since is_global=false).
    let scope = emit_gos_at.unwrap_or(let_line);

    // An or-pattern that binds a name from a different place in each
    // alternative (`if let (x, _) | (_, x) = u`, `if let (Some(x), _) |
    // (_, Some(x)) = (a, b)`): drawn like such a match arm (#533,
    // #597), `x` as a new value and each place as moved into it only if
    // its alternative matched. The parents are the tuple literal's
    // elements, as for `match (a, b)`.
    let (or_parents, or_parents_ty): (Vec<ResourceTy>, Vec<Ty<'tcx>>) = match self.literal_elems(scrutinee) {
        Some(elems) => (
            elems.iter().map(|e| self.rap_or_field_through_ref(e)).collect(),
            elems.iter().map(|e| typeck_res.node_type(e.hir_id)).collect(),
        ),
        _ => (vec![parent.clone()], vec![scrutinee_ty]),
    };
    let moved_into_scrutinee = self.tuple_scrutinee_moves(scrutinee);
    let (pat, or_conflicts) = self.or_arm_stand_in(let_expr.pat, &or_parents, &or_parents_ty, Some(scrutinee), let_line, typeck_res, &moved_into_scrutinee);

    // `if let (Some(a), Some(b)) = (x, y)`: like `match (x, y)`, each
    // sub-pattern destructures its own element of the tuple literal
    // (#327), also of a tuple literal nested in it (#627). Otherwise
    // the whole pattern matches the scrutinee.
    // `if let Some(x) = Some(h)`: `x` takes `h`'s value (#807).
    let pairs: Vec<(&'tcx Pat<'tcx>, ResourceTy, Ty<'tcx>, &'tcx Expr<'tcx>)> = match (pat.kind, self.literal_elems(scrutinee)) {
        (PatKind::Tuple(..) | PatKind::TupleStruct(..), Some(_)) => self.literal_pat_parents(pat, scrutinee),
        _ => vec![(pat, parent.clone(), scrutinee_ty, scrutinee)],
    };
    let mut decls = Vec::new();
    for (sp, sp_parent, sp_parent_ty, sp_expr) in pairs {
        let mut sub = Vec::new();
        self.get_dec_of_pat_parts(sp, typeck_res, &sp_parent, sp_parent_ty, scope, &mut sub);
        let sp_ty = typeck_res.node_type(sp.hir_id);
        for (r, e, from, partial) in sub {
            let mut from = self.retarget_tuple_borrow(scrutinee, &r, &e, from);
            // `if let s = &t`: s borrows t (#1235).
            let e = self.addr_of_scrutinee_borrow(sp_expr, sp, &r, &e, &mut from).unwrap_or(e);
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
    let mut registered: HashSet<ResourceAccessPoint> = HashSet::new();
    // A copied or-pattern name is just bound: nothing moves.
    let _ = self.if_let_or_conflict_events(&or_conflicts, &mut copies, let_line, &moved_into_scrutinee, &tuple_name, body_end, false, &mut registered);
    let mut first_other = None;
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
        // (A reference bound from different places is drawn by its
        // `OrAltBorrow` events, and declared in `registered`.)
        or_events = self.if_let_or_conflict_events(&or_conflicts, &mut groups[1], let_line, &moved_into_scrutinee, &tuple_name, body_end, false, &mut registered);
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
        let row = self.pattern_loans.get(binding_rap.hash()).map_or(let_line, |loan| loan.line);
        self.add_external_event(row, event);
        *self.unique_id += 1;
        borrows.extend(self.pattern_ref_copy_borrow(&binding_rap, &evt_kind, scrutinee, Some(let_expr.pat)));

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
                self.add_scope_end(r.clone(), body_end);
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
    // (A struct or tuple variable's part columns too, but not the ones
    // the pattern took, #627.)
    for v in moved_into_scrutinee {
        for r in self.tuple_scrutinee_unbound(&v, &moved_by_pattern) {
            let event = self.tuple_scrutinee_move(&v, r, into_tuple.clone());
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
  pattern: Option<&Pat<'_>>,
) -> Vec<ExternalEvent> {
  let local = match pattern {
    Some(pat) => self.raps.pattern_local(binding.name(), pat),
    None => self.raps.local(binding.name()),
  };
  if matches!(evt, Evt::SBorrow | Evt::MBorrow) {
    return self.pattern_call_ref_borrows(binding, scrutinee, local);
  }
  if !matches!(evt, Evt::Copy | Evt::Move) {
    return Vec::new();
  }
  let typeck = self.tcx.typeck(scrutinee.hir_id.owner);
  if typeck.expr_ty(scrutinee).is_ref() {
    return Vec::new();
  }
  let moved = matches!(evt, Evt::Move);
  if binding.is_ref() {
    return self.pattern_ref_borrows(binding, binding.name(), local, &[], moved, false, false, scrutinee);
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
    let evs = self.pattern_ref_borrows(&member, &bound, local, &fields, moved, true, false, scrutinee);
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

/// A binding by reference into what the reference a call returned
/// points to (`Some(s)` on `pick(&a, &b)`, `fn pick<'a>(a: &'a
/// Option<String>, b: &'a Option<String>) -> &'a Option<String>`), when
/// the call's signature ties that reference to several arguments: s may
/// point into any of them, and all of them stay borrowed while s is in
/// use, as for `let r = pick(&a, &b);` (#1368). With one,
/// `call_result_lender` names it (#1222).
fn pattern_call_ref_borrows(&mut self, binding: &ResourceAccessPoint, scrutinee: &'tcx Expr<'tcx>, local: Option<Local>) -> Vec<ExternalEvent> {
  let mut call = scrutinee;
  while let ExprKind::DropTemps(inner) = call.kind {
    call = inner;
  }
  if !binding.is_ref() || !matches!(call.kind, ExprKind::Call(..) | ExprKind::MethodCall(..))
    || !self.tcx.typeck(call.hir_id.owner).expr_ty(call).is_ref() || self.call_lenders(call).len() < 2 {
    return Vec::new();
  }
  self.pattern_ref_borrows(binding, binding.name(), local, &[], false, false, true, call)
}

/// `pattern_ref_copy_borrow` for one reference: the binding itself
/// (`fields` empty) or a reference field `bound.f` of a struct binding
/// (`in_field`). With `by_ref`, the binding isn't copied out of the
/// value but bound by reference into what the reference `scrutinee`
/// returned points to (`pattern_call_ref_borrows`).
#[allow(clippy::too_many_arguments)]
fn pattern_ref_borrows(
  &mut self,
  reference: &ResourceAccessPoint,
  bound: &str,
  local: Option<Local>,
  fields: &[&str],
  moved: bool,
  in_field: bool,
  by_ref: bool,
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
  // it) is left out when one of the reference's own kind is there:
  // the field points into that one. Without one, the reference holds
  // the other kind's loan: `if let Some(x) = first(&mut v)`
  // (`fn(&mut Vec<i32>) -> Option<&i32>`) keeps v mutably borrowed
  // while x is live (#1122).
  let is_mut = reference.is_mutref();
  let held = self.loans_held_in(local, fields, None);
  let kind_of = |l: &ResourceTy| match (l, &held) {
    (ResourceTy::Value(r), Some(held)) if !r.is_ref() =>
      held.get(r.name().split('.').next().unwrap_or_default()).copied().unwrap_or(is_mut),
    _ => is_mut,
  };
  match &held {
    Some(held) => {
      // A temporary the `let` extends (`let m = Some(&String::new());`,
      // #1357) has no named local in MIR to find its loan by: it stays.
      let is_temp = |base: &str| self.raps.get(base).is_some_and(|rd| rd.rap.is_temporary());
      let kept = |l: &ResourceTy, own_kind: bool| match l {
        ResourceTy::Value(r) if !r.is_ref() => {
          let base = r.name().split('.').next().unwrap_or_default();
          is_temp(base) || held.get(base).is_some_and(|m| !own_kind || *m == is_mut)
        }
        _ => true,
      };
      let own: Vec<ResourceTy> = lenders.iter().filter(|l| kept(l, true)).cloned().collect();
      if own.is_empty() {
        lenders.retain(|l| kept(l, false));
      } else {
        lenders = own;
      }
    }
    // Without rustc's lifetimes, only a single lender is certain.
    None if lenders.len() > 1 || in_field => return Vec::new(),
    None => {}
  }
  if lenders.is_empty() {
    return Vec::new();
  }
  // `if let Some(el) = it.next()` with `it = refs.iter_mut()`: el points
  // into refs; a1 is reached through it (#1399).
  let full: Vec<&str> = std::iter::once(bound).chain(fields.iter().copied()).collect();
  self.split_through_lenders(&full.join("."), &mut lenders, &mut Vec::new());
  // `if let Some(x) = self.items.first_mut()`: only the field (#1073),
  // or fields (#1080).
  if let Some(fields) = self.field_call_lenders(scrutinee, &lenders) {
    lenders = fields.into_iter().map(|(l, _)| l).collect();
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
  let kinds: Vec<bool> = lenders.iter().map(kind_of).collect();
  if let Some(rd) = self.borrow_map.get_mut(&name) {
    rd.lender = lenders[0].clone();
    rd.co_lenders = lenders[1..].to_vec();
    rd.aliasing = aliasing;
    rd.lender_muts = if kinds.iter().all(|m| *m == is_mut) { Vec::new() } else { lenders.iter().cloned().zip(kinds.iter().copied()).collect() };
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
    carrier: if by_ref { ViaCarrier::PatternRef } else { ViaCarrier::PatternBinding { from_call, moved, field_of: in_field } },
    passed_on: Vec::new(),
  };
  let to = ResourceTy::Value(reference.clone());
  let mut out = Vec::new();
  for (lender, is_mut) in lenders.into_iter().zip(kinds) {
    let id = *self.unique_id;
    *self.unique_id += 1;
    let via = Some(via.clone());
    // Through a local `&mut r` (`r.get_mut(0)`): a reborrow of r (#1152).
    let (lender, is_reborrow) = match lender {
      ResourceTy::Deref(r) if r.is_mutref() => (ResourceTy::Value(r), true),
      l => (l, false),
    };
    out.push(if is_mut {
      ExternalEvent::MutableBorrow { from: lender, to: to.clone(), is_partial: false, is_reborrow, break_alt: None, via, id, of_ref_var: false }
    } else {
      ExternalEvent::StaticBorrow { from: lender, to: to.clone(), is_partial: false, is_reborrow, break_alt: None, via, id, of_ref_var: false }
    });
  }
  out
}

/// The call in the method chain `call` that the borrow its result holds
/// comes from, and the names of the calls chained on it that only pass
/// that borrow on, innermost first: `(w.last_mut(), ["unwrap"])` for
/// `w.last_mut().unwrap()` (#1266). A call is passed through when its
/// signature ties its result to the value it was called on alone and
/// that value is another method call's result. `(call, [])` otherwise.
pub fn borrowing_call(&self, call: &'tcx Expr<'tcx>) -> (&'tcx Expr<'tcx>, Vec<String>) {
  let mut c = call;
  let mut passed: Vec<String> = Vec::new();
  while let ExprKind::MethodCall(seg, recv, ..) = c.kind {
    if c.span.from_expansion() || recv.span.from_expansion() || !matches!(recv.kind, ExprKind::MethodCall(..)) {
      break;
    }
    let tie = self.call_arg_tie(c, None);
    if !(tie.receiver && tie.tied == [0] && tie.stored_in.is_empty() && tie.unnamed == 0) {
      break;
    }
    passed.push(seg.ident.to_string());
    c = recv;
  }
  passed.reverse();
  (c, passed)
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
  let Some(rd) = self.borrow_map.get(lhs_rap.name()).cloned() else { return false };
  let own = self.call_lenders(call);
  // `self.items.first_mut().unwrap()`: only the field (#1073), or
  // fields (#1080).
  let own: Vec<ResourceTy> = match self.field_call_lenders(call, &own) {
    Some(fields) => fields.into_iter().map(|(l, _)| l).collect(),
    None => own,
  };
  let lenders: Vec<_> = rd.all_lenders().into_iter()
    .filter(|l| matches!(l, ResourceTy::Value(_) | ResourceTy::Deref(_) | ResourceTy::DerefField(..)) && own.contains(l))
    .filter(|l| !self.closure_result_supplies_lender(lhs_rap.owner_local().or_else(|| self.raps.root_local(lhs_rap.name())), lhs_rap.name(), call, l))
    .collect();
  if lenders.is_empty() {
    return false;
  }
  // `w.last_mut().unwrap()` (#1266): the borrow comes from `last_mut`;
  // `unwrap` only passes it on.
  // `refs.iter_mut().next().unwrap()` (#1395): from `iter_mut`; `next`
  // and `unwrap` pass it on.
  let (borrower, mut passed_on) = self.narrowed_origin(call).unwrap_or_else(|| self.borrowing_call(call));
  let func = match borrower.kind {
    ExprKind::MethodCall(seg, ..) if !passed_on.is_empty() => seg.ident.to_string(),
    _ => func,
  };
  // `let x = v.first()?;` (#1352).
  if self.try_of_operand(call).is_some() {
    passed_on.push(crate::svg_generator::hover_messages::TRY_PASSES_ON.to_owned());
  }
  // `refs.last().copied().unwrap()`: the reference stored in the
  // element last's result points to, copied out (#1382).
  let via = match self.copied_out_of(call) {
    Some(inner) if lhs_rap.is_ref() => BorrowVia {
      func: match inner.kind {
        ExprKind::MethodCall(seg, ..) => seg.ident.to_string(),
        _ => func,
      },
      lenders: lenders.iter().map(|l| l.name()).collect(),
      also_unnamed: false,
      tie: ArgTie::default(),
      carrier: ViaCarrier::PatternBinding { from_call: true, moved: lhs_rap.is_mutref(), field_of: false },
      passed_on: Vec::new(),
    },
    _ => BorrowVia {
      func,
      lenders: lenders.iter().map(|l| l.name()).collect(),
      also_unnamed: self.call_lenders_full(call).1,
      tie: self.narrowed_tie(call, borrower).unwrap_or_else(|| self.call_arg_tie(borrower, None)),
      carrier: if lhs_rap.is_ref() { ViaCarrier::Reference } else { ViaCarrier::Value },
      passed_on,
    },
  };
  self.draw_holder_borrows(lhs, &lenders, &rd, &self.aggregate_borrows(call).unwrap_or_default(), line_num, Some(&via));
  self.mark_borrow_kept_by_result(call, &lenders);
  true
}

/// Each new loan gets its own event, including repeated borrows of one place.
/// A carried reference instead arrives on the row where the value is built.
fn draw_holder_borrows(&mut self, lhs: &ResourceTy, lenders: &[ResourceTy], rd: &RefData,
                       operands: &[crate::ref_flow::OperandBorrow<'a>], line_num: usize, via: Option<&BorrowVia>) {
  let alternatives = operands.iter().any(|b| b.conditional);
  // The aggregate's fresh reborrow and its siblings share a construction.
  // Keep the legacy ends of unrelated holder operations outside this path.
  let mixed_holder = rd.lender_muts.iter().any(|(_, m)| *m) && rd.lender_muts.iter().any(|(_, m)| !*m);
  let tracks_ends = alternatives || mixed_holder || operands.iter().any(|b| b.issued && matches!(b.lender, ResourceTy::Deref(_)));
  let mut arrows = Vec::new();
  for lender in lenders {
    if !matches!(lender, ResourceTy::Value(_) | ResourceTy::Deref(_) | ResourceTy::DerefField(..)) { continue; }
    let mut matches = operands.iter().filter(|b| b.lender == *lender).peekable();
    if matches.peek().is_none() {
      let mutable = if rd.lender_muts.is_empty() && lhs.is_ref() { lhs.is_mutref() } else { rd.mut_of(lender) };
      arrows.push((lender, mutable, line_num, None));
    } else {
      arrows.extend(matches.map(|b| (lender, b.loan.kind == "mut", if b.issued { b.loan.line } else { line_num }, Some(b))));
    }
  }
  for (lender, is_mut, line, operand) in arrows {
    let id = *self.unique_id;
    // `r.first_mut()` with r a local `&mut`: a reborrow through r, drawn
    // like `&mut *r` (#1152).
    let (from, is_reborrow) = match lender {
      ResourceTy::Deref(r) if r.is_mutref() => (ResourceTy::Value(r.clone()), true),
      l => (l.clone(), false),
    };
    if let Some(b) = operand {
      if b.issued {
        self.aggregate_issues.insert(id, b.loan.loan);
      }
      if b.conditional || (tracks_ends && b.issued && self.body_facts.is_some_and(|f| f.conditional_ends.contains(&b.loan.loan))) {
        self.conditional_aggregate_ends.push((lhs.clone(), lender.clone(), b.loan.loan));
      }
      let projected = via.filter(|v| v.carrier == ViaCarrier::StoredBy).and(b.loan.projected.as_ref());
      if let Some(place) = projected {
        self.row_layout.projected_loan_places.insert(id, (b.loan.loan, place.clone()));
        if b.conditional {
          self.row_layout.uncertain_projected_stores.insert((*lhs.hash(), b.loan.loan));
        }
      }
      let title = if b.issued && b.loan.two_phase {
        Some(crate::svg_generator::hover_messages::aggregate_reservation(&from.name(), &lhs.name()))
      } else if alternatives {
        use crate::svg_generator::hover_messages as hover;
        let place = projected.map_or_else(|| hover::styled_name_html(&lender.name()), hover::projected_place_html);
        Some(hover::aggregate_arm_borrow(&place, &lhs.name(), is_mut, b.conditional))
      } else { None };
      if let Some(title) = title { self.row_layout.aggregate_titles.insert(id, title); }
    }
    let ev = if is_mut {
      ExternalEvent::MutableBorrow { from, to: lhs.clone(), is_partial: false, is_reborrow, break_alt: None, via: via.cloned(), id, of_ref_var: false }
    } else {
      ExternalEvent::StaticBorrow { from, to: lhs.clone(), is_partial: false, is_reborrow, break_alt: None, via: via.cloned(), id, of_ref_var: false }
    };
    self.add_external_event(line, ev);
    *self.unique_id += 1;
  }
}

/// `lhs` is a struct bound to `call`'s result whose reference fields
/// `register_struct_field_lenders` (or `reassign_struct_field_lenders`)
/// linked to the caller's variables: draw a borrow from each lender
/// into each such field, and keep the lenders borrowed past the call
/// (#336). `lhs` may be a struct field of a struct (`o.inner = mk(&t)`,
/// `O { inner: mk(&t), .. }`, #638).
pub fn emit_struct_field_borrows(&mut self, lhs: &ResourceTy, call: &'tcx Expr<'tcx>, func: String, line_num: usize) {
  let ResourceTy::Value(ResourceAccessPoint::Struct(st)) = lhs else { return };
  // `opart(&s)?` (#1352): the value bound is what the `?` yields.
  let try_expr = self.try_of_operand(call);
  let ty = self.tcx.typeck(call.hir_id.owner).expr_ty(try_expr.unwrap_or(call));
  // `opart(&s).unwrap()`: the borrow is `opart`'s; `unwrap` (and a
  // `?`) only pass it on.
  let (borrower, mut passed_on) = self.pass_through_chain(call);
  if try_expr.is_some() {
    passed_on.push(crate::svg_generator::hover_messages::TRY_PASSES_ON.to_owned());
  }
  let func = if std::ptr::eq(borrower, call) {
    func
  } else {
    match borrower.kind {
      ExprKind::MethodCall(seg, ..) => seg.ident.to_string(),
      ExprKind::Call(fn_expr, _) => hirid_to_var_name(fn_expr.hir_id, &self.tcx).unwrap_or(func),
      _ => func,
    }
  };
  let mut all: Vec<ResourceTy> = Vec::new();
  let mut unlent: Vec<(String, ResourceAccessPoint, String)> = Vec::new();
  // A tuple's elements too (`let q = pair(&s);`, #1341).
  for (field_name, _) in self.member_fields(ty) {
    let fname = format!("{}.{}", st.name, field_name);
    let (Some(rd), Some(frap)) = (self.borrow_map.get(&fname), self.raps.get(&fname)) else { continue };
    let frap = frap.rap.to_owned();
    let lenders: Vec<ResourceTy> = rd.all_lenders().into_iter()
      .filter(|l| matches!(l, ResourceTy::Value(_) | ResourceTy::Deref(_)))
      .filter(|l| !self.closure_result_supplies_lender(st.local.or_else(|| self.raps.root_local(&st.name)), &fname, call, l))
      .collect();
    // A reference field whose argument has no column (`part("a")`, a
    // call's result we don't track): the field still gets its value
    // here, or its first use lands on a column that never started
    // (#1356). Like `Part { text: "a" }`, it is bound with no arrow, and
    // its borrow's end names a string literal / static it points to.
    if lenders.is_empty() && frap.is_ref() && matches!(rd.lender, ResourceTy::Anonymous) && rd.co_lenders.is_empty() {
      unlent.push((fname, frap, field_name));
      continue;
    }
    if lenders.is_empty() || !(frap.is_ref() || frap.holds_borrow()) {
      continue;
    }
    let is_mut = if frap.is_ref() { frap.is_mutref() } else { rd.ref_mutability };
    let kinds: Vec<bool> = lenders.iter().map(|l| if rd.lender_muts.is_empty() { is_mut } else { rd.mut_of(l) }).collect();
    let via = BorrowVia {
      func: func.clone(),
      lenders: lenders.iter().map(|l| l.name()).collect(),
      also_unnamed: self.call_field_lenders(call, &field_name).1,
      tie: self.call_arg_tie(borrower, Some(&field_name)),
      carrier: if matches!(ty.kind(), TyKind::Tuple(_)) { ViaCarrier::TupleField } else { ViaCarrier::StructField },
      passed_on: passed_on.clone(),
    };
    let to = ResourceTy::Value(frap.clone());
    for (lender, is_mut) in lenders.iter().zip(kinds) {
      let id = *self.unique_id;
      let ev = if is_mut {
        ExternalEvent::MutableBorrow { from: lender.clone(), to: to.clone(), is_partial: false, is_reborrow: false, break_alt: None, via: Some(via.clone()), id, of_ref_var: false }
      } else {
        ExternalEvent::StaticBorrow { from: lender.clone(), to: to.clone(), is_partial: false, is_reborrow: false, break_alt: None, via: Some(via.clone()), id, of_ref_var: false }
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
  for (fname, frap, field_name) in unlent {
    let static_lender = self.call_field_static_lender(borrower, &field_name);
    if let Some(rd) = self.borrow_map.get_mut(&fname) {
      rd.static_lender = static_lender;
    }
    let id = *self.unique_id;
    *self.unique_id += 1;
    self.add_external_event(line_num, ExternalEvent::Bind { from: ResourceTy::Anonymous, to: ResourceTy::Value(frap), id, break_alt: None });
  }
}

/// Set `result_keeps_borrow` on the pass-by-reference events that
/// `call`'s evaluation emitted from one of `lenders`: those borrows
/// continue in the call's result (see `emit_call_result_borrows`).
pub fn mark_borrow_kept_by_result(&mut self, call: &'tcx Expr<'tcx>, lenders: &[ResourceTy]) {
  let lo = span_to_line(&call.span, &self.tcx);
  let hi = span_end_line(&call.span, &self.tcx);
  let hashes: Vec<u64> = lenders.iter()
    .filter(|l| !matches!(l, ResourceTy::DerefField(..)))
    .filter_map(|l| l.extract_rap().map(|r| *r.hash())).collect();
  // Only the calls inside `call` (`first` and `unwrap` in
  // `v.first().unwrap()`), not another call on the same line
  // (`t.len()` in `if t.len() > 1 { first_word(&t) } ..`; #337).
  let mut fns: Vec<String> = Vec::new();
  self.call_names(call, &mut fns);
  self.mark_kept_on_lines(lo, hi, &hashes, &fns);
  // A `&mut` variable moved into one of those calls (`id(r)` with a
  // generic parameter, `Some(r)`; no reborrow) whose borrow is the
  // one the result keeps: the result's holder takes the borrow over
  // (#1128). The same for a value holding a borrow (`it.map(f)`,
  // `it.copied()`, `o.unwrap()`, #1448): its loan goes on in what the
  // call returns.
  #[expect(clippy::disallowed_methods, reason = "sorted below; used as a set")]
  let mut carried: Vec<u64> = self.borrow_map.iter()
    .filter(|(_, rd)| rd.all_lenders().iter().any(|l| lenders.contains(l)))
    .filter_map(|(k, _)| self.raps.get(k).map(|r| *r.rap.hash()))
    .chain(lenders.iter().filter_map(|l| match l {
      ResourceTy::Value(r) | ResourceTy::Deref(r) if r.is_ref() => Some(*r.hash()),
      _ => None,
    }))
    .collect();
  carried.sort_unstable();
  let mut ids: Vec<usize> = Vec::new();
  moves_into_calls(self.preprocessed_events, lo, hi, &carried, &fns, &mut ids);
  self.moved_into_result.extend(ids);
  // A field reached through a reference (`self.items` in
  // `self.items.first_mut().unwrap()`; #1073).
  for l in lenders {
    if let ResourceTy::DerefField(r, place, _, _) = l {
      self.mark_kept_on_lines_at(lo, hi, &[*r.hash()], Some(place.as_str()), &fns);
    }
  }
}

/// Set `result_keeps_borrow` on the pass-by-reference events from one
/// of `hashes` into one of the functions `fns` on lines `lo..=hi`, and
/// return their ids.
fn mark_kept_on_lines(&mut self, lo: usize, hi: usize, hashes: &[u64], fns: &[String]) -> Vec<usize> {
  self.mark_kept_on_lines_at(lo, hi, hashes, None, fns)
}

/// `mark_kept_on_lines`, for the events from the field `place` reached
/// through one of the references `hashes` (`self.items` in
/// `self.items.iter_mut()`, a `DerefField`) when `place` is given.
fn mark_kept_on_lines_at(&mut self, lo: usize, hi: usize, hashes: &[u64], place: Option<&str>, fns: &[String]) -> Vec<usize> {
  let mut ids: Vec<usize> = Vec::new();
  mark_kept_in(&mut self.preprocessed_events, lo, hi, hashes, place, fns, &mut ids);
  for line in lo..=hi {
    if let Some(evs) = self.event_line_map.get_mut(&line) {
      for ev in evs.iter_mut() {
        if ids.contains(&ev.get_id()) {
          set_kept(ev);
        }
      }
    }
  }
  ids
}

/// `for x in <head>` with `head` a call (`v.iter()`,
/// `v.iter().enumerate()`, `s.chars()`): the loop's iterator is
/// `head`'s value, and it keeps the loans that value holds until the
/// loop ends, since `next()` runs again at the top of every iteration.
/// Mark the argument-passing events of those lenders as kept by the
/// result, so the `for` line doesn't say the lender got its resource
/// back (#868), and return each lender (a variable that owns what it
/// lends) with whether its loan is mutable and the call that lent it. Only the calls in `head`
/// whose result can hold a borrow keep one: `v.len()` in
/// `v.iter().take(v.len())` borrows `v` just for the call. A `&mut`
/// reference the head's first call reborrows (`v.iter_mut()` with `v:
/// &mut Vec<_>` a parameter, or `let v = &mut s;`) is a lender too:
/// the iterator holds that reborrow of `*v` until the loop ends, so v
/// can't be used until then (#1046).
pub fn keep_loop_head_loans(&mut self, head: &'tcx Expr<'tcx>) -> Vec<(ResourceTy, bool, String)> {
  if !matches!(head.kind, ExprKind::Call(..) | ExprKind::MethodCall(..)) {
    return Vec::new();
  }
  let typeck = self.tcx.typeck(head.hir_id.owner);
  let mut fns: Vec<String> = Vec::new();
  self.borrow_holding_call_names(head, typeck, &mut fns);
  let lo = span_to_line(&head.span, &self.tcx);
  let hi = span_end_line(&head.span, &self.tcx);
  let mut out: Vec<(ResourceTy, bool, String)> = Vec::new();
  for l in self.value_lenders(head) {
    let owner = matches!(&l, ResourceTy::Value(r) if !r.is_ref() && !r.is_fn());
    if !owner || out.iter().any(|(o, _, _)| *o == l) {
      continue;
    }
    let Some(hash) = l.extract_rap().map(|r| *r.hash()) else { continue };
    let ids = self.mark_kept_on_lines(lo, hi, &[hash], &fns);
    if let Some((is_mut, call)) = ids.iter().find_map(|id| pass_ref_kind(&self.preprocessed_events, *id)) {
      out.push((l, is_mut, call));
    }
  }
  // Several fields reached through a `&mut` (`self.items.iter()
  // .zip(self.other.iter_mut())`, #1080): each is lent until the loop
  // ends, as one is below.
  let lenders = self.call_lenders(head);
  if let Some(fields) = self.field_call_lenders(head, &lenders).filter(|f| f.len() > 1) {
    for (field, _) in fields {
      let ResourceTy::DerefField(r, place, _, _) = &field else { continue };
      if !r.is_mutref() {
        continue;
      }
      let ids = self.mark_kept_on_lines_at(lo, hi, &[*r.hash()], Some(place.as_str()), &fns);
      if let Some((is_mut, call)) = ids.iter().find_map(|id| pass_ref_kind(&self.preprocessed_events, *id)) {
        out.push((field, is_mut, call));
      } else if let Some((call, is_mut)) = self.closure_capturing_field(head, r, place) {
        // Captured by a closure the loop's iterator holds (`map(|x| x +
        // self.n)`, #1093): no call was passed the field, so its borrow
        // starts here, lent to the closure, which the iterator keeps
        // (the loop's end matches it by that name).
        let target_name = format!("the closure passed to {}", call);
        if !self.raps.contains_key(&fn_key(&target_name)) {
          self.add_fn(target_name.clone());
        }
        let target = ResourceTy::Value(self.raps.get(&fn_key(&target_name)).unwrap().rap.to_owned());
        let id = *self.unique_id;
        *self.unique_id += 1;
        let (from, to) = (field.clone(), target);
        let implicit = ImplicitRef::Capture { place: from.name(), unique: false, interior_mut: false };
        let ev = if is_mut {
          ExternalEvent::PassByMutableReference {
            manually_takes: false, terminal: None, from, to, id, is_reborrow: false, ends_at_call: false, implicit,
            result_keeps_borrow: true, stored_in: StoredIn::default(), break_alt: None,
            of_ref_var: false, deep: None,
          }
        } else {
          ExternalEvent::PassByStaticReference {
            from, to, id, implicit, refcell_borrow_mut: false, pointee_interior_mut: false,
            result_keeps_borrow: true, stored_in: StoredIn::default(), break_alt: None, still_borrowed_by: Vec::new(),
            of_ref_var: false, deep: None,
          }
        };
        self.update_rap(r, lo);
        self.add_external_event(lo, ev);
        out.push((field, is_mut, target_name));
      }
    }
    return out;
  }
  // `v.iter()` reborrows `*v` too, shared, when v is a `&mut` (#1059):
  // v is read-only until the loop ends. A field reached through a
  // `&mut` (`self.items.iter_mut()`, #1060) is lent the same way.
  // So is each chain passed to the head's calls (`b.iter()` in
  // `a.iter_mut().zip(b.iter())`, #1066): the zipped iterator holds both.
  // With a `&T` v (`v: &Vec<_>`, `list: &[T]`; #1081) the iterator reads
  // through a copy of v until the loop ends, and v stays usable.
  // Nothing is kept when the head's value can't hold a borrow
  // (`v.iter().map(..).collect::<Vec<i32>>()`): the loop iterates
  // over a new Vec, and `v.push(..)` in the body compiles.
  if !typeck.expr_ty(head).walk().any(|g| matches!(g.kind(), GenericArgKind::Lifetime(_))) {
    return out;
  }
  for (root, _) in self.loop_head_chain_roots(head) {
    let through = match &root {
      ResourceTy::Value(r) if r.is_ref() => Some((r.clone(), None)),
      ResourceTy::DerefField(r, place, _, _) => Some((r.clone(), Some(place.clone()))),
      _ => None,
    };
    if let Some((r, place)) = through {
      let ids = self.mark_kept_on_lines_at(lo, hi, &[*r.hash()], place.as_deref(), &fns);
      if let Some((is_mut, call)) = ids.iter().find_map(|id| pass_ref_kind(&self.preprocessed_events, *id)) {
        out.push((root, is_mut, call));
      }
    }
  }
  // A `&T` passed as it is to the head's calls (`.zip(b)`, #1076) is
  // copied into the loop's iterator, which reads through the copy
  // until the loop ends. (A `&mut` is moved in: see the `for` lowering.)
  for r in self.loop_head_ref_args(head) {
    if r.is_mutref() || out.iter().any(|(o, _, _)| matches!(o, ResourceTy::Value(x) if *x == r)) {
      continue;
    }
    let ids = self.mark_kept_on_lines_at(lo, hi, &[*r.hash()], None, &fns);
    if let Some((is_mut, call)) = ids.iter().find_map(|id| pass_ref_kind(&self.preprocessed_events, *id)) {
      out.push((ResourceTy::Value(r), is_mut, call));
    }
    else if let Some((id, call)) = self.mir_call(head).filter(|c| c.std_ref_adapter).and_then(|c| c.site)
      .and_then(|call_site| self.preprocessed_events.iter().find_map(|(_, e)| match e {
      ExternalEvent::Copy { from: ResourceTy::Value(from), to: ResourceTy::Value(call), id, .. }
        if from.hash() == r.hash() && call.is_fn()
          && self.argument_slots.get(id) == Some(&(call_site.at, 1)) =>
          Some((*id, call.name().clone())),
      _ => None,
    })) {
      self.row_layout.aggregate_titles.insert(id,
        crate::svg_generator::hover_messages::event_dot_lend_through_ref_to_loop_iterator(
          r.name(), false, false, r.points_to_copy()));
      out.push((ResourceTy::Value(r), false, call));
    }
  }
  out
}

/// `call_names`, keeping only the calls whose result type can hold a
/// borrow (`iter`, `enumerate`, not `len`).
fn borrow_holding_call_names(&self, e: &'tcx Expr<'tcx>, typeck: &'tcx rustc_middle::ty::TypeckResults<'tcx>, out: &mut Vec<String>) {
  let holds = |e: &'tcx Expr<'tcx>| typeck.expr_ty(e).walk().any(|g| matches!(g.kind(), GenericArgKind::Lifetime(_)));
  match e.kind {
    ExprKind::Call(f, args) => {
      if holds(e) {
        if let Some(n) = hirid_to_var_name(f.hir_id, &self.tcx) {
          out.push(n);
        }
      }
      for a in args.iter() { self.borrow_holding_call_names(a, typeck, out); }
    }
    ExprKind::MethodCall(seg, rcvr, args, _) => {
      if holds(e) {
        out.push(seg.ident.as_str().to_owned());
      }
      self.borrow_holding_call_names(rcvr, typeck, out);
      for a in args.iter() { self.borrow_holding_call_names(a, typeck, out); }
    }
    ExprKind::AddrOf(_, _, inner) | ExprKind::Unary(_, inner) | ExprKind::Field(inner, _)
    | ExprKind::DropTemps(inner) | ExprKind::Cast(inner, _) | ExprKind::Index(inner, _, _) => self.borrow_holding_call_names(inner, typeck, out),
    _ => {}
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
/// A struct's reference fields keep theirs the same way (`let d = if c
/// { mk(&s1) } else { .. };`, #1402).
pub fn mark_branch_results_kept(&mut self, name: &str, init: &'tcx Expr<'tcx>) {
  let Some(tails) = branch_tails(init) else { return };
  let Some(rap) = self.raps.get(name).map(|rd| rd.rap.clone()) else { return };
  let lenders: Vec<ResourceTy> = if rap.is_ref() {
    let Some(rd) = self.borrow_map.get(name) else { return };
    rd.all_lenders()
  } else {
    let mut v = Vec::new();
    for part in self.ref_parts_of(&ResourceTy::Value(rap)) {
      add_lenders(&mut v, &self.borrow_map[&part]);
    }
    v
  };
  if lenders.is_empty() {
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
/// that names a variable of a non-Copy, non-reference type, also inside
/// a tuple literal nested in it (`b` and `c` in `(a, (b, c))`, #627).
/// They are moved whether or not the pattern then matches, and whether
/// or not it binds anything from them, so `x` can't be used afterwards
/// (#327). A struct or tuple variable whose parts have columns of their
/// own (`u` in `(s, u)`) is moved too: see `tuple_scrutinee_unbound`
/// for how its parts are drawn.
pub fn tuple_scrutinee_moves(&self, scrutinee: &'tcx Expr<'tcx>) -> Vec<ResourceAccessPoint> {
    self.tuple_scrutinee_vars(scrutinee).into_iter().map(|(r, _, _)| r).collect()
}

/// `tuple_scrutinee_moves`, each with its field path from the scrutinee
/// (`["1", "0"]` for `b` in `(a, (b, c))`) and its type.
fn tuple_scrutinee_vars(&self, scrutinee: &'tcx Expr<'tcx>) -> Vec<(ResourceAccessPoint, Vec<String>, Ty<'tcx>)> {
    let mut out = Vec::new();
    if self.literal_elems(scrutinee).is_some() {
        let typeck = self.tcx.typeck(scrutinee.hir_id.owner);
        self.tuple_literal_vars(scrutinee, &mut Vec::new(), typeck, false, &mut out);
        // A variable's field is moved into the tuple too (`y.name` in
        // `(x, y.name)`, #977), when the tuple is drawn as "the
        // temporary tuple" (`tuple_scrutinee_position`, and a variable
        // in it). Otherwise its literal has a `(temporary)` column,
        // which already takes the field (`(y.name, y.tag)`).
        if !out.is_empty() && self.tuple_scrutinee_position(scrutinee) {
            out.clear();
            self.tuple_literal_vars(scrutinee, &mut Vec::new(), typeck, true, &mut out);
        }
    }
    out
}

/// Whether the tuple literal `expr` is directly the scrutinee of a
/// `match`, or of an `if let` (also a part of an `if`'s let chain,
/// #971): where its variables are drawn moving into "the temporary
/// tuple" rather than into a `(temporary)` column (#327).
pub fn tuple_scrutinee_position(&self, expr: &'tcx Expr<'tcx>) -> bool {
    use rustc_hir::Node;
    match self.tcx.parent_hir_node(expr.hir_id) {
        Node::Expr(m) => match m.kind {
            ExprKind::Match(s, _, rustc_hir::MatchSource::Normal) => s.hir_id == expr.hir_id,
            // Also a part of a let chain (`if let (x, _) = (f, s) && c`,
            // #971).
            ExprKind::Let(l) if l.init.hir_id == expr.hir_id => self.is_if_condition_let(m),
            _ => false,
        },
        _ => false,
    }
}

/// The variant or struct that a constructor call (`Some(f)`) or a tuple
/// struct / struct pattern (`Some(x)`, `P { .. }`) names, as a path
/// (#807). `None` for anything else.
fn variant_key(&self, res: Res) -> Option<String> {
    let id = match res {
        Res::Def(DefKind::Ctor(..), id) => self.tcx.parent(id),
        Res::Def(DefKind::Variant | DefKind::Struct, id) => id,
        _ => return None,
    };
    Some(self.tcx.def_path_str(id))
}

/// `variant_key` of the constructor call `e`.
fn ctor_variant_key(&self, e: &'tcx Expr<'tcx>) -> Option<String> {
    let ExprKind::Call(callee, _) = e.kind else { return None };
    let ExprKind::Path(ref q) = callee.kind else { return None };
    self.variant_key(self.tcx.typeck(e.hir_id.owner).qpath_res(q, callee.hir_id))
}

/// `variant_key` of the tuple struct or struct pattern `pat`.
fn pat_variant_key(&self, pat: &'tcx Pat<'tcx>) -> Option<String> {
    let typeck = self.tcx.typeck(pat.hir_id.owner);
    match pat.kind {
        PatKind::TupleStruct(ref q, ..) | PatKind::Struct(ref q, ..) => self.variant_key(typeck.qpath_res(q, pat.hir_id)),
        _ => None,
    }
}

/// Whether the tuple struct pattern `pat` names another variant than
/// the constructor call `e` builds (`Err(x)` on `Ok(f)`): it can't
/// match, and binds nothing from `e`'s elements.
pub fn ctor_pat_mismatch(&self, pat: &'tcx Pat<'tcx>, e: &'tcx Expr<'tcx>) -> bool {
    match (self.pat_variant_key(pat), self.ctor_variant_key(e)) {
        (Some(p), Some(c)) => p != c,
        _ => false,
    }
}

fn tuple_literal_vars(&self, e: &'tcx Expr<'tcx>, path: &mut Vec<String>,
  typeck: &'tcx rustc_middle::ty::TypeckResults<'tcx>, fields: bool, out: &mut Vec<(ResourceAccessPoint, Vec<String>, Ty<'tcx>)>) {
    if let Some(elems) = self.literal_elems(e) {
        // A constructor's element `i` is `i#<variant>`: a pattern
        // naming another variant doesn't match it (`sub_pat_at`).
        let variant = self.ctor_variant_key(e);
        for (i, el) in elems.iter().enumerate() {
            path.push(match &variant {
                Some(v) => format!("{}#{}", i, v),
                None => i.to_string(),
            });
            self.tuple_literal_vars(el, path, typeck, fields, out);
            path.pop();
        }
        return;
    }
    match e.kind {
        ExprKind::Path(QPath::Resolved(_, p)) if matches!(p.res, Res::Local(_)) => {
            let ty = typeck.node_type(e.hir_id);
            if ty.is_ref() || self.ty_is_copy(ty, e.hir_id.owner) {
                return;
            }
            match crate::expr_visitor_utils::get_rap(e, &self.tcx, &self.raps) {
                ResourceTy::Value(rap @ (ResourceAccessPoint::Owner(_)
                    | ResourceAccessPoint::Struct(Struct { is_member: false, .. }))) => out.push((rap, path.clone(), ty)),
                _ => {}
            }
        }
        // A field of a variable (`y.name`, `p.0`, a Box's `b.name`)
        // whose part has a column of its own: moved into the tuple like
        // a variable, out of the variable, which keeps its other parts
        // (#977). Only a chain of fields on the variable itself: `v[0].f`
        // or `(*r).f` isn't a column (and doesn't compile for a non-Copy
        // field behind a reference).
        ExprKind::Field(..) if fields && !e.span.from_expansion() => {
            let mut base = e;
            while let ExprKind::Field(b, _) = base.kind {
                base = b;
            }
            if !matches!(base.kind, ExprKind::Path(QPath::Resolved(_, p)) if matches!(p.res, Res::Local(_))) {
                return;
            }
            let ty = typeck.node_type(e.hir_id);
            if ty.is_ref() || self.ty_is_copy(ty, e.hir_id.owner) {
                return;
            }
            let Some(name) = crate::expr_visitor_utils::expr_to_rap_name(e, &self.tcx) else { return };
            match self.raps.get(&name).map(|rd| &rd.rap) {
                Some(rap @ (ResourceAccessPoint::Owner(_) | ResourceAccessPoint::Struct(_))) => out.push((rap.clone(), path.clone(), ty)),
                _ => {}
            }
        }
        _ => {}
    }
}

/// Whether `place` is one of `vars` (`tuple_scrutinee_moves`) or a part
/// of one (`u.0` of `u`): its value went into the temporary tuple.
pub fn in_scrutinee_tuple(vars: &[ResourceAccessPoint], place: &ResourceAccessPoint) -> bool {
    vars.iter().any(|v| v == place || place.name().starts_with(&format!("{}.", v.name())))
}

/// What the tuple-literal scrutinee moved into its temporary of `var`
/// (one of `tuple_scrutinee_moves`) that the pattern didn't move out
/// itself (`taken`), each drawn as moved into the temporary: `var`, and
/// for a struct or tuple variable each of its part columns (#627). The
/// literal moves `var` whole, so it's listed even when the pattern took
/// some of its parts (`u` and `u.1` when `(p, (x, _))` moved `u.0` out
/// of `(s, u)`). Copy parts go too: `var` can't be used after it, nor
/// can its `i32` fields (#1284).
pub fn tuple_scrutinee_unbound(&self, var: &ResourceAccessPoint, taken: &[ResourceTy]) -> Vec<ResourceAccessPoint> {
    if taken.contains(&ResourceTy::Value(var.clone())) {
        return Vec::new();
    }
    let mut out = vec![var.clone()];
    for c in self.struct_children(var.name()) {
        out.extend(self.tuple_scrutinee_unbound(&c, taken));
    }
    out
}

pub(crate) fn tuple_scrutinee_move(&mut self, root: &ResourceAccessPoint, field: ResourceAccessPoint, into: ResourceTy) -> ExternalEvent {
    let carried_with = (field != *root && field.is_copy_owner())
        .then(|| (root.name().clone(), into.name()));
    let id = *self.unique_id;
    *self.unique_id += 1;
    let mut event = self.ext_ev_of_evt(Evt::Move, into, ResourceTy::Value(field), id, false);
    if let ExternalEvent::Move { carried_with: parent, .. } = &mut event {
        *parent = carried_with;
    }
    event
}

/// A `ref` / `ref mut` binding out of a tuple-literal scrutinee's
/// element (`(Some(ref a), _)` on `(x, y)`) borrows that element of the
/// temporary tuple, not `x`, whose value was moved into the tuple
/// (#580). Its lender is the tuple, which has no column: drawn like a
/// borrow of any other temporary (`Anonymous`).
pub fn tuple_elem_lender(&self, scrutinee: &'tcx Expr<'tcx>, evt: &Evt, from: ResourceTy) -> ResourceTy {
  if matches!(evt, Evt::SBorrow | Evt::MBorrow) {
    if let ResourceTy::Value(r) = &from {
      if Self::in_scrutinee_tuple(&self.tuple_scrutinee_moves(scrutinee), r) {
        return ResourceTy::Anonymous;
      }
    }
  }
  from
}

/// A binding by reference into what a call's returned reference points
/// to (`Some(x) => …` on `match get(&o)`, `[first, ..]` on `match
/// v.as_slice()`): a function lends nothing, so it borrows what that
/// reference borrows, the one variable the call's signature ties its
/// result to (#1222). With several, none is named; with none (the call
/// borrows nothing we track), `from` stays.
fn call_result_lender(&self, scrutinee: &'tcx Expr<'tcx>, evt: &Evt, from: ResourceTy) -> ResourceTy {
  if !matches!(evt, Evt::SBorrow | Evt::MBorrow) || !from.extract_rap().is_some_and(|r| r.is_fn()) {
    return from;
  }
  let mut call = scrutinee;
  while let ExprKind::DropTemps(inner) = call.kind {
    call = inner;
  }
  if !matches!(call.kind, ExprKind::Call(..) | ExprKind::MethodCall(..))
    || !self.tcx.typeck(call.hir_id.owner).expr_ty(call).is_ref() {
    return from;
  }
  match self.call_lenders(call).as_slice() {
    [one] => one.clone(),
    // `pick(&a, &b)`: it may point into either; no arrow, rather than
    // one from the function.
    [_, _, ..] => ResourceTy::Anonymous,
    [] => from,
  }
}

/// `match &t { s => … }` / `if let s = &mut t`: the pattern `pat` is a
/// plain binding, so `s` is bound to the reference `&t` itself and
/// borrows t, exactly as in `let s = &t;` (#1235). The scrutinee
/// resolves to t's column (`from`), so without this the binding reads as
/// a copy of t. Returns the borrow event and points the binding's loan
/// (registered with no lender by `collect_pat_binding_events`) at t.
pub fn addr_of_scrutinee_borrow(&mut self, scrutinee: &'tcx Expr<'tcx>, pat: &'tcx Pat<'tcx>, binding: &ResourceAccessPoint, evt: &Evt, from: &mut ResourceTy) -> Option<Evt> {
  if !matches!(evt, Evt::Copy | Evt::Move) || !binding.is_ref()
      || !matches!(pat.kind, PatKind::Binding(ann, _, _, None) if matches!(ann.0, rustc_hir::ByRef::No)) {
    return None;
  }
  let mut e = scrutinee;
  while let ExprKind::DropTemps(inner) = e.kind { e = inner; }
  let ExprKind::AddrOf(rustc_hir::BorrowKind::Ref, _, operand) = e.kind else { return None };
  // For an overloaded index the autoref of the container is on the
  // indexed operand. Its loan reaches the binding via the Index result.
  let container = match operand.kind { ExprKind::Index(base, ..) => Some(base.span), _ => None };
  let matching_loan = [Some(e.span), Some(operand.span), container].into_iter().flatten().find_map(|span| {
    let body = self.body_facts?;
    let mut matching = self.effects.get(&span.with_parent(None))?.iter().filter_map(|(_, ev)| match &ev.fact {
      crate::facts::Fact::Borrow { loan, .. } => body.loans.iter().find(|l| l.loan == *loan
        && l.through.is_none() && l.holders.contains(binding.name())
        && matches!(from, ResourceTy::Value(r) if *r.name() == l.lender)),
      _ => None,
    });
    let loan = matching.next()?;
    matching.next().is_none().then_some((span, loan))
  });
  let Some((span, loan)) = matching_loan else {
    // The pattern receives a newly created reference. Without a proven
    // lender, show only the binding, never a transfer from the operand.
    *from = ResourceTy::Anonymous;
    return Some(Evt::Bind);
  };
  let aliasing = get_aliasing_data(from, &self.borrow_map);
  let rd = self.borrow_map.get_mut(binding.name())?;
  rd.lender = from.clone();
  rd.aliasing = aliasing;
  if from.is_ref() {
    rd.borrow_span = Some(span);
    self.ref_var_borrows.insert(span);
    self.pattern_loans.insert(*binding.hash(), loan);
    self.approx_notes.push(ApproxNote::PatternReference { holder: *binding.hash() });
  }
  Some(if loan.kind == "mut" { Evt::MBorrow } else { Evt::SBorrow })
}

/// `tuple_elem_lender` for the binding `binding`, also pointing the
/// borrow its `ref` registered (add_ref) at the tuple, so the borrow's
/// end isn't drawn as giving x its value back (#580).
pub fn retarget_tuple_borrow(&mut self, scrutinee: &'tcx Expr<'tcx>, binding: &ResourceAccessPoint, evt: &Evt, from: ResourceTy) -> ResourceTy {
  let lender = self.tuple_elem_lender(scrutinee, evt, from.clone());
  if lender != from {
    // The tuple has no column, so no arrow shows the borrow: the
    // binding's first dot says what it borrows (#1363).
    if let Some(x) = from.extract_rap() {
      let tuple = self.tuple_scrutinee_text(scrutinee);
      let text = crate::svg_generator::hover_messages::approx_note_tuple_elem_borrow(binding.name(), x.name(), &tuple, matches!(evt, Evt::MBorrow));
      let lines = (span_to_line(&scrutinee.span, &self.tcx), self.tuple_scrutinee_user_end(scrutinee));
      self.approx_notes.push(ApproxNote::At { hash: *binding.hash(), lines, text });
    }
    self.tuple_temp_refs.insert(binding.name().to_string());
    if let Some(rd) = self.borrow_map.get_mut(binding.name()) {
      if rd.lender == from {
        rd.lender = lender.clone();
      }
    }
    return lender;
  }
  let lender = self.call_result_lender(scrutinee, evt, from.clone());
  if lender != from {
    if let Some(rd) = self.borrow_map.get_mut(binding.name()) {
      if rd.lender == from {
        rd.lender = lender.clone();
      }
    }
  }
  lender
}

/// The last line of the `match` / `if let` / `while let` whose
/// scrutinee is `scrutinee` (its line, for anything else).
fn tuple_scrutinee_user_end(&self, scrutinee: &'tcx Expr<'tcx>) -> usize {
  let user = self.tcx.hir_parent_iter(scrutinee.hir_id).find_map(|(_, n)| match n {
    rustc_hir::Node::Expr(e) if matches!(e.kind, ExprKind::Match(..) | ExprKind::If(..) | ExprKind::Loop(..)) => Some(e.span),
    _ => None,
  });
  span_end_line(&user.unwrap_or(scrutinee.span), &self.tcx)
}

/// Where a tuple-literal scrutinee's variables go: the move into the
/// tuple, labelled with its source text (`(p, q)`; empty when it's
/// too long to quote).
pub fn tuple_scrutinee_target(&self, scrutinee: &'tcx Expr<'tcx>) -> ResourceTy {
  let text = self.tcx.sess.source_map().span_to_snippet(scrutinee.span).ok()
    .map(|t| t.split_whitespace().collect::<Vec<_>>().join(" "))
    .filter(|t| t.chars().count() <= 32);
  // A constructor (`Some(f)`, #807) is named by its path when it's too
  // long to quote: `Some(..)`. Its text tells it from a tuple's.
  let text = match (text, scrutinee.kind) {
    (Some(t), _) => t,
    (None, ExprKind::Call(callee, _)) => {
      self.tcx.sess.source_map().span_to_snippet(callee.span).map(|c| format!("{}(..)", c)).unwrap_or_default()
    }
    (None, _) => String::new(),
  };
  ResourceTy::Temporary { extended: false, tuple: Some(text) }
}

/// The tuple-literal scrutinee's variables (`tuple_scrutinee_moves`)
/// whose value, or part of it, is still in the tuple after `pat`
/// matched (`None`: the pattern didn't match, so all of it), and is
/// dropped with the tuple (#327). Only types that need drop: the
/// others have nothing to drop. A struct or tuple variable with part
/// columns is listed whole when all of it is left, otherwise by the
/// parts that are (#627).
pub fn tuple_scrutinee_left(&self, scrutinee: &'tcx Expr<'tcx>, pat: Option<&'tcx Pat<'tcx>>) -> Vec<(ResourceAccessPoint, TupleLeft)> {
  let typeck = self.tcx.typeck(scrutinee.hir_id.owner);
  let env = typing_env(self.tcx, scrutinee.hir_id.owner);
  let mut out = Vec::new();
  for (var, path, ty) in self.tuple_scrutinee_vars(scrutinee) {
    if !self.needs_drop(ty, scrutinee.hir_id.owner) {
      continue;
    }
    // The places that need drop: the variable, or its leaf parts.
    let places: Vec<(ResourceAccessPoint, Vec<String>)> = if matches!(var, ResourceAccessPoint::Struct(_)) {
      let parts: Vec<ResourceAccessPoint> = self.struct_members_deep(var.name());
      parts.iter()
        .filter(|p| !p.never_drops() && !p.is_plain_value())
        .filter(|p| !parts.iter().any(|q| q.name().starts_with(&format!("{}.", p.name()))))
        .map(|p| {
          let mut full = path.clone();
          full.extend(p.name()[var.name().len() + 1..].split('.').map(str::to_owned));
          (p.clone(), full)
        })
        .collect()
    } else {
      Vec::new()
    };
    let whole = places.is_empty();
    let places = if whole { vec![(var.clone(), path)] } else { places };
    let mut entries = Vec::new();
    for (place, full) in &places {
      let left = match pat {
        None => Some(false),
        Some(p) => self.path_left(p, full, typeck, env),
      };
      if let Some(rest) = left {
        let takers = match pat {
          Some(p) => self.path_takers(p, full, typeck),
          None => Vec::new(),
        };
        entries.push((place.clone(), TupleLeft { name: place.name().clone(), rest, rc: place.rc_drop(), takers }));
      }
    }
    if !whole && entries.len() == places.len() && entries.iter().all(|(_, l)| !l.rest && l.takers.is_empty()) {
      out.push((var.clone(), TupleLeft { name: var.name().clone(), rest: false, rc: var.rc_drop(), takers: Vec::new() }));
    } else {
      out.extend(entries);
    }
  }
  out
}

/// The sub-pattern of the tuple or struct pattern `pat` that matches
/// its field `step` (an index for a tuple pattern), and whether there
/// is one: `Err(())` when `pat` isn't a tuple or struct pattern,
/// `Ok(None)` when the field is skipped by `..` (or not named).
fn sub_pat_at(&self, pat: &'tcx Pat<'tcx>, step: &str,
  typeck: &'tcx rustc_middle::ty::TypeckResults<'tcx>) -> Result<Option<&'tcx Pat<'tcx>>, ()> {
  use rustc_middle::ty::TyKind;
  // `0#Option::Some`: field 0 of a constructor scrutinee's `Some` (#807).
  let (step, variant) = match step.split_once('#') {
    Some((s, v)) => (s, Some(v)),
    None => (step, None),
  };
  if let Some(v) = variant {
    if self.pat_variant_key(pat).is_some_and(|p| p != v) {
      // Another variant: the pattern doesn't match it, all of it stays.
      return Ok(None);
    }
  }
  match pat.kind {
    PatKind::Tuple(subs, dd) | PatKind::TupleStruct(_, subs, dd) => {
      let Ok(i) = step.parse::<usize>() else { return Err(()) };
      let n = match typeck.pat_ty(pat).kind() {
        TyKind::Tuple(tys) => tys.len(),
        TyKind::Adt(adt, _) => match pat.kind {
          PatKind::TupleStruct(ref qpath, ..) => adt.variant_of_res(typeck.qpath_res(qpath, pat.hir_id)).fields.len(),
          _ => return Err(()),
        },
        _ => return Err(()),
      };
      let gap = n.saturating_sub(subs.len());
      let k = match dd.as_opt_usize() {
        Some(d) if i >= d && i < d + gap => return Ok(None),
        Some(d) if i >= d + gap => i - gap,
        _ => i,
      };
      Ok(subs.get(k))
    }
    PatKind::Struct(_, fields, _) => Ok(fields.iter().find(|f| f.ident.as_str() == step).map(|f| f.pat)),
    _ => Err(()),
  }
}

/// An alternative is dead when MIR contains its binding assignments
/// and none of them is on an executable edge.
fn pattern_alternative_runs(&self, pat: &Pat<'_>) -> bool {
  let Some(facts) = self.facts else { return true };
  let mut sites = facts.binding_sources.iter().filter(|s| pat.span.contains(s.site)).peekable();
  sites.peek().is_none() || sites.any(|s| s.runs)
}

/// What `pat`, matched against the scrutinee, leaves in the tuple of
/// the place at field path `path` in it (#627: through nested tuple
/// literals and a variable's parts): all of it (`Some(false)`), part of
/// it (`Some(true)`) or nothing (`None`).
pub fn path_left(&self, pat: &'tcx Pat<'tcx>, path: &[String],
  typeck: &'tcx rustc_middle::ty::TypeckResults<'tcx>, env: rustc_middle::ty::TypingEnv<'tcx>) -> Option<bool> {
  let Some((step, rest)) = path.split_first() else { return self.pat_leaves(pat, typeck, env) };
  match pat.kind {
    // `(Some(a), _) | (_, Some(a))`: which alternative matched decides.
    // When they differ, "the rest of" covers both (all of it, or
    // what's left after the move).
    PatKind::Or(alts) => {
      let each: Vec<Option<bool>> = alts.iter().filter(|a| self.pattern_alternative_runs(a)).map(|a| self.path_left(a, path, typeck, env)).collect();
      if each.is_empty() { return Some(true); }
      if each.iter().all(|l| *l == each[0]) { each[0] } else { Some(true) }
    }
    _ => match self.sub_pat_at(pat, step, typeck) {
      Ok(Some(sp)) => self.path_left(sp, rest, typeck, env),
      // Skipped by `..`: all of it stays.
      Ok(None) => Some(false),
      // `t => …` takes the whole of it; `_`, `ref t` leave it.
      Err(()) => self.pat_leaves(pat, typeck, env),
    },
  }
}

/// For an or-pattern whose alternatives leave different amounts of
/// the place at `path` in the tuple (`(Some(x), _) | (_, Some(x))`,
/// #533; also nested, `(p, (x, _) | (_, x))`, #627): the names that
/// move some of it out in the alternatives that do, in order. What's
/// left in the tuple then depends on which alternative matched at run
/// time. Empty otherwise.
fn path_takers(&self, pat: &'tcx Pat<'tcx>, path: &[String],
  typeck: &'tcx rustc_middle::ty::TypeckResults<'tcx>) -> Vec<String> {
  let env = typing_env(self.tcx, pat.hir_id.owner);
  let mut out: Vec<String> = Vec::new();
  match pat.kind {
    PatKind::Or(alts) => {
      let alts: Vec<_> = alts.iter().filter(|a| self.pattern_alternative_runs(a)).collect();
      let each: Vec<Option<bool>> = alts.iter().map(|a| self.path_left(a, path, typeck, env)).collect();
      if each.iter().all(|l| *l == each[0]) {
        // An or-pattern further in may still decide.
        for a in alts {
          for n in self.path_takers(a, path, typeck) {
            if !out.contains(&n) {
              out.push(n);
            }
          }
        }
        return out;
      }
      for alt in alts {
        let Some(sub) = self.pat_at_path(alt, path, typeck) else { continue };
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
    _ => match path.split_first() {
      Some((step, rest)) => match self.sub_pat_at(pat, step, typeck) {
        Ok(Some(sp)) => self.path_takers(sp, rest, typeck),
        _ => out,
      },
      None => out,
    },
  }
}

/// The part of `pat` that matches the place at `path`: as far down as
/// the pattern goes (a name or `_` matching all of it, an or-pattern).
/// `None` when it's skipped by `..`.
pub(crate) fn pat_at_path(&self, pat: &'tcx Pat<'tcx>, path: &[String],
  typeck: &'tcx rustc_middle::ty::TypeckResults<'tcx>) -> Option<&'tcx Pat<'tcx>> {
  let Some((step, rest)) = path.split_first() else { return Some(pat) };
  match self.sub_pat_at(pat, step, typeck) {
    Ok(Some(sp)) => self.pat_at_path(sp, rest, typeck),
    Ok(None) => None,
    Err(()) => Some(pat),
  }
}

/// `literal_pat_pairs` as (sub-pattern, parent, parent's type, the
/// element): the parent is what the element names (`rap_or_field_through_ref`).
/// A name bound from a whole nested tuple literal (`y` in `(p, y)` on
/// `(a, (b, c))`) is bound from each element of it, as `match (b, c) {
/// y => … }` binds `y` from `b` and `c`; the parent's type is the
/// literal's, so it isn't drawn as a partial move.
pub fn literal_pat_parents(&self, pat: &'tcx Pat<'tcx>, scrutinee: &'tcx Expr<'tcx>)
  -> Vec<(&'tcx Pat<'tcx>, ResourceTy, Ty<'tcx>, &'tcx Expr<'tcx>)> {
  let typeck = self.tcx.typeck(scrutinee.hir_id.owner);
  let mut pairs = Vec::new();
  self.literal_pat_pairs(pat, scrutinee, &mut pairs);
  let mut out = Vec::new();
  for (p, el) in pairs {
    let mut stand_in = p;
    while let PatKind::Or(alts) = stand_in.kind {
      stand_in = &alts[0];
    }
    if self.literal_elems(el).is_some() && self.ctor_pat_mismatch(stand_in, el) {
      // `Err(e)` on `Ok(f)`: this arm can't match, and `e` gets
      // nothing from `f` (#807).
      out.push((p, ResourceTy::Anonymous, typeck.node_type(el.hir_id), el));
    } else if self.literal_elems(el).is_some() && !std::ptr::eq(el, scrutinee) {
      let mut ls = Vec::new();
      self.literal_leaves(el, &mut ls);
      for l in ls {
        out.push((p, self.rap_or_field_through_ref(l), typeck.node_type(el.hir_id), l));
      }
    } else {
      out.push((p, self.literal_elem_source(el), typeck.node_type(el.hir_id), el));
    }
  }
  out
}

/// What the element `el` of a scrutinee literal gives the pattern: its
/// variable (or field through a reference), or for a field of a call's
/// result (`make_q().name` in `(z, make_q().name)`), the call, as for
/// the call `mk()` itself: an arm can't draw a move out of the call's
/// temporary, whose column isn't split into the arms (#1350).
fn literal_elem_source(&self, el: &'tcx Expr<'tcx>) -> ResourceTy {
  match self.rap_or_field_through_ref(el) {
    ResourceTy::Anonymous => match field_call_root(el).map(|c| get_rap(c, &self.tcx, &self.raps)) {
      Some(f @ ResourceTy::Value(ResourceAccessPoint::Function(_))) => f,
      _ => ResourceTy::Anonymous,
    },
    r => r,
  }
}

/// Each sub-pattern of `pat` with the element of the tuple literal `e`
/// it matches, through nested tuple literals (`(p, (x, _))` on `(a, (b,
/// c))`: `p` with `a`, `x` with `b`, `_` with `c`; #627) and
/// constructors taken apart the same way (`Some(x)` on `Some(f)`: `x`
/// with `f`; #807, see `literal_elems`). An
/// or-pattern's first alternative stands for it, as in
/// `or_arm_stand_in`. A sub-pattern that isn't a tuple pattern, or an
/// element that isn't a tuple literal, is one pair.
pub fn literal_pat_pairs(&self, pat: &'tcx Pat<'tcx>, e: &'tcx Expr<'tcx>, out: &mut Vec<(&'tcx Pat<'tcx>, &'tcx Expr<'tcx>)>) {
  let mut p = pat;
  while let PatKind::Or(alts) = p.kind {
    p = &alts[0];
  }
  let elems = match (p.kind, self.literal_elems(e)) {
    (PatKind::Tuple(subs, dd), Some(elems)) if matches!(e.kind, ExprKind::Tup(_)) => Some((subs, dd, elems)),
    // `Some(x)` on `Some(f)`: `x` with `f` (#807).
    (PatKind::TupleStruct(_, subs, dd), Some(elems)) if !self.ctor_pat_mismatch(p, e) => Some((subs, dd, elems)),
    _ => None,
  };
  match elems {
    Some((subs, dd, elems)) => {
      let gap = elems.len().saturating_sub(subs.len());
      for (k, sp) in subs.iter().enumerate() {
        let i = match dd.as_opt_usize() {
          Some(d) if k >= d => k + gap,
          _ => k,
        };
        if let Some(el) = elems.get(i) {
          self.literal_pat_pairs(sp, el, out);
        }
      }
    }
    None => out.push((pat, e)),
  }
}

/// The elements of the literal `e` (`literal_elems`), through nested
/// literals: `b` and `c` for `(b, c)` or `Some((b, c))`. A name bound
/// from the whole of it is bound from each of them.
pub(crate) fn literal_leaves(&self, e: &'tcx Expr<'tcx>, out: &mut Vec<&'tcx Expr<'tcx>>) {
  match self.literal_elems(e) {
    Some(elems) => for el in elems { self.literal_leaves(el, out) },
    None => out.push(e),
  }
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
pub(crate) fn pat_leaves_drop(&self, pat: &'tcx Pat<'tcx>, typeck: &'tcx rustc_middle::ty::TypeckResults<'tcx>, env: rustc_middle::ty::TypingEnv<'tcx>) -> bool {
  use rustc_middle::ty::TyKind;
  let tcx = self.tcx;
  let ty = typeck.node_type(pat.hir_id);
  // A `ref` binding's own type is a reference (`a: &String` in `let
  // (ref a, b) = (u, 1);`): what it matched, and leaves in place, is
  // what that points to (#1163).
  if let PatKind::Binding(ann, ..) = pat.kind {
    let by_ref = matches!(ann.0, rustc_hir::ByRef::Yes(..))
      || matches!(typeck.pat_binding_modes().get(pat.hir_id).map(|m| m.0), Some(rustc_hir::ByRef::Yes(..)));
    return by_ref && self.needs_drop_in(ty.builtin_deref(true).unwrap_or(ty), env);
  }
  if !self.needs_drop_in(ty, env) {
    return false;
  }
  // Whether a field the pattern skips with `..` needs drop: `tys` are
  // the fields' types, of which `m` sub-patterns cover all but the
  // ones from index `d` on.
  let skipped_drop = |d: usize, m: usize, tys: Vec<Ty<'tcx>>| {
    let gap = tys.len().saturating_sub(m);
    tys.into_iter().skip(d).take(gap).any(|t| self.needs_drop_in(t, env))
  };
  match pat.kind {
    PatKind::Binding(..) => unreachable!("handled above"),
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
        .any(|f| self.needs_drop_in(f.ty(tcx, args), env))
    }
    PatKind::Or(alts) => alts.iter().any(|a| self.pat_leaves_drop(a, typeck, env)),
    // `_`, a literal, a unit variant, a `box` / deref pattern: what
    // they match stays.
    _ => true,
  }
}

// This function does the work of adding an event for let stmts
pub fn match_rhs(&mut self, lhs: ResourceTy, rhs:&'tcx Expr, evt: Evt){
    let outer = self.emitting_expr.replace(rhs);
    self.match_rhs_at(lhs.clone(), rhs, evt);
    self.draw_closure_result_borrows(&lhs, rhs);
    self.emitting_expr = outer;
  }

  /// The call's usual initialization runs before the arrows for the
  /// loans its closure operands contribute to the returned value.
  fn draw_closure_result_borrows(&mut self, holder: &ResourceTy, call: &Expr<'_>) {
    let ResourceTy::Value(root) = holder else { return };
    let Some(result) = self.closure_result(root.owner_local().or_else(|| self.raps.root_local(root.name())), call) else { return };
    let Some(facts) = self.body_facts else { return };
    let func = match call.kind {
      ExprKind::Call(f, _) => hirid_to_var_name(f.hir_id, &self.tcx),
      ExprKind::MethodCall(segment, ..) => Some(segment.ident.to_string()),
      _ => None,
    };
    for (name, field) in &result.holders {
      let Some(rap) = self.raps.place(result.holder, name).map(|rd| rd.rap.clone()) else { continue };
      if !rap.holds_borrow() && !rap.is_ref() { continue; }
      for loan in facts.loans.iter().filter(|loan| field.loans.contains(&loan.loan)) {
        let Some(lender) = self.closure_result_lender(loan) else { continue };
        let mutable = loan.kind == "mut";
        let id = *self.unique_id;
        let of_ref_var = lender.is_ref();
        self.closure_result_arrivals.insert(id, result.arrival);
        if !rap.is_ref() {
          self.row_layout.closure_result_holders.insert(*rap.hash());
        }
        self.row_layout.aggregate_titles.insert(id, crate::svg_generator::hover_messages::closure_result_borrow(
          &lender.name(), rap.name(), func.as_deref(), mutable, of_ref_var,
        ));
        let evt = if mutable { Evt::MBorrow } else { Evt::SBorrow };
        self.add_borrow_ev_of_ref_var(result.row, evt, ResourceTy::Value(rap.clone()), lender, false, of_ref_var);
      }
    }
  }

  fn match_rhs_at(&mut self, lhs: ResourceTy, rhs:&'tcx Expr, evt: Evt){
  // An expression of type `!` (`unreachable!()`, `panic!()`, `return`,
  // `continue`) never produces a value, so `lhs` gets nothing from it:
  // `_ => unreachable!()` in `let s = match … { … }` doesn't bind `s`
  // (#296). (`return v` hands `v` to the caller, not to `lhs`.)
  if self.tcx.typeck(rhs.hir_id.owner).expr_ty_opt(rhs).is_some_and(|ty| ty.is_never()) {
    return;
  }
  if matches!(lhs, ResourceTy::Caller) && matches!(rhs.kind,
    ExprKind::Path(_) | ExprKind::Field(..) | ExprKind::Index(..) | ExprKind::Unary(UnOp::Deref, _) | ExprKind::AddrOf(..))
  {
    self.return_operand(rhs);
    return;
  }
  // `let (Some(a), _) = (x, make()) else { .. }`: the literal is the
  // `let`-`else`'s temporary, which x was moved into; `a` is moved out
  // of it, not out of x (#954). Keyed by the literal and by each
  // variable in it (`note_let_else_literal_temp`).
  if let Some(tmp) = self.let_else_literal_temps.get(&rhs.hir_id).cloned() {
    let line_num = span_to_line(&rhs.span, &self.tcx);
    self.add_ev(line_num, evt, lhs, ResourceTy::Value(tmp), true);
    return;
  }
  // `let Some((b, _)) = opt_pair() else { .. };`, `.. = w.first() else`:
  // the call's result is a temporary the pattern takes a part of, with a
  // column of its own (`record_temporary_drop`); the binding takes that
  // part out of the temporary, not out of the call (#1481).
  if let Some(tmp) = self.temp_cols.get(&rhs.hir_id).cloned()
    .filter(|t| matches!(t.temp_part(), Some(TempPart::LetElse(None))) && matches!(evt, Evt::Move | Evt::Copy)) {
    let line_num = span_to_line(&rhs.span, &self.tcx);
    self.add_ev(line_num, evt, lhs, ResourceTy::Value(tmp), true);
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
    if let Some(elems) = vec_macro_elems(self.tcx, rhs) {
      // Earlier elements may already have moved into MIR temporaries. The
      // assignment itself never happens if an element returns or diverges.
      let Some(row) = self.body_facts.and_then(|b| b.constructed_vecs.get(&rhs.span.ctxt().outer_expn())) else { return };
      if let Some((b, rap, local)) = self.body_facts.zip(lhs.extract_rap()).and_then(|(b, rap)| {
        let local = rap.owner_local()?;
        b.vars.iter().any(|v| v.local == local.index() && v.local > self.mir_body.arg_count && v.capture.is_none()).then_some((b, rap, local))
      }) {
        let mut inits = b.rows.iter().flat_map(|r| &r.facts)
          .filter(|e| e.place_local == Some(local) && matches!(&e.fact, crate::facts::Fact::Init { .. })).peekable();
        if inits.peek().is_some() && inits.all(|e| b.skipped_closure_returns.contains_key(&e.location.block)) {
          self.approx_notes.push(ApproxNote::UnreachedValue { hash: *rap.hash() });
        }
      }
      self.draw_literal_parts(lhs, elems.iter().collect(), *row, true);
      return;
    }
    self.bind_from_anonymous(line_num, lhs);
    return;
  }
  self.note_derived(&lhs, rhs);
  match rhs.kind {
    ExprKind::Path(QPath::Resolved(_,p)) => self.match_rhs_path(lhs, rhs, evt, p),
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
    ExprKind::Call(fn_expr, _) => self.match_rhs_call(lhs, rhs, evt, fn_expr),
    
    // `let t = s + "!";`, `let f = -wave;` with an overloaded operator:
    // the operator is a call (`Add::add(s, "!")`), and t gets what it
    // returns, drawn from the operator's logo as from a function's
    // (#1351). (For String, that is s's own buffer, moved on.)
    ExprKind::Binary(..) | ExprKind::Unary(UnOp::Neg | UnOp::Not, _) if overloaded_value_op(rhs, &self.tcx).is_some() => {
      let op = overloaded_value_op(rhs, &self.tcx).unwrap().to_owned();
      let Some(call) = self.mir_call(rhs).filter(|c| c.has_result) else { return };
      let evt = if call.copyable { Evt::Copy } else { Evt::Move };
      if !self.raps.contains_key(&fn_key(&op)) {
        self.add_operator(op.clone());
      }
      let from = self.raps.get(&fn_key(&op))
        .map_or(ResourceTy::Anonymous, |rd| ResourceTy::Value(rd.rap.with_mir_result()));
      self.add_call_result(call, expr_to_line(rhs, &self.tcx), evt, lhs, from);
    }
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
    // `let p: *mut i32 = &mut x;` (and `p = &x;`, `S { p: &mut x }`): the
    // reference is coerced to a raw pointer on the spot. A raw pointer
    // carries no lifetime, so the borrow ends right there and `p` is a
    // plain value, as for the explicit cast `&mut x as *mut i32` (#1251).
    ExprKind::AddrOf(..) if self.tcx.typeck(rhs.hir_id.owner).expr_ty_adjusted_opt(rhs).is_some_and(|t| t.is_raw_ptr()) => {
      let line_num = span_to_line(&rhs.span, &self.tcx);
      self.bind_from_anonymous(line_num, lhs);
    }
    ExprKind::AddrOf(_, _,expr) => {
      let line_num = expr_to_line(&expr, &self.tcx);
      match fetch_rap(&expr, &self.tcx, &self.raps) {
        Some(rhs_rap) => {
          self.update_rap(&rhs_rap, expr_to_line(&expr, &self.tcx));
        }
        None => {} // taking addrOf some anonymous resource, ie: &String::from("")
      }
      // A temporary a `let` extends has a column of its own (#856).
      let nested_tmp = self.nested_ext_temps.get(&rhs.hir_id).cloned();
      let res = match nested_tmp.or_else(|| fetch_rap(rhs, &self.tcx, &self.raps)) {
        Some(x) => ResourceTy::Value(x),
        // `&p.name` with p a reference borrows the field through p (#266).
        None => match self.field_access_through_ref(rhs) {
          Some(rty) => {
            if let Some(r) = rty.deref_rap() {
              self.update_rap(&r.clone(), line_num);
            }
            rty
          }
          // `&mut people[0].age` / `&b.name` (b a Box): part of
          // people's / b's value (#1176).
          None => match self.borrowed_part_holder(rhs) {
            Some(h) => {
              self.update_rap(&h, line_num);
              ResourceTy::Value(h)
            }
            None => ResourceTy::Anonymous,
          },
        },
      };
      // `&*r` / `&mut *r` with r a reference borrows *through* r (a
      // reborrow); `&r` / `&mut r` borrows the variable r itself.
      // Both resolve to the same source RAP, so decide here from the
      // syntax rather than from r's kind downstream. `&p.f` through a
      // reference p is a reborrow too.
      // `&mut m` / `&x` coerced to `&mut String` / `&String` reborrows
      // `*m` / `*x` too (#926, #939).
      let is_reborrow = matches!(res, ResourceTy::DerefField(..)) || is_explicit_reborrow(
        innermost_addr_of_operand(rhs),
        self.tcx.typeck(rhs.hir_id.owner),
      ) || is_coerced_ref_var_reborrow(rhs, self.tcx.typeck(rhs.hir_id.owner));
      // `&mut r` / `&r` borrowing the reference variable r itself
      // (#914, #925).
      let of_ref_var = ref_var_borrow(rhs, self.tcx.typeck(rhs.hir_id.owner));
      if of_ref_var {
        self.ref_var_borrows.insert(rhs.span);
      }
      // `let it = &mut v.iter();` (#1209): `it` borrows a temporary
      // (the `Iter`) that borrows v as the call's signature says; rustc
      // keeps the temporary until the end of the block, so v stays
      // borrowed (with the call's loan, here shared) while `it` is in use.
      if let (ResourceTy::Anonymous, ResourceTy::Value(lhs_rap), ExprKind::Call(..) | ExprKind::MethodCall(..)) = (&res, &lhs, expr.kind) {
        let lenders: Vec<ResourceTy> = self.call_lenders(expr).into_iter()
          .filter(|l| matches!(l, ResourceTy::Value(_) | ResourceTy::Deref(_)))
          .collect();
        if lhs_rap.is_ref() && !expr.span.from_expansion() && !lenders.is_empty() {
          let muts: Vec<(ResourceTy, bool)> = self.borrow_map.get(lhs_rap.name()).map(|rd| rd.lender_muts.clone()).unwrap_or_default();
          for lender in &lenders {
            let is_mut = muts.iter().find(|(l, _)| l == lender).map_or(lhs_rap.is_mutref(), |(_, m)| *m);
            let evt = if is_mut { Evt::MBorrow } else { Evt::SBorrow };
            self.add_borrow_ev_of_ref_var(line_num, evt, lhs.clone(), lender.clone(), false, false);
          }
          self.mark_borrow_kept_by_result(expr, &lenders);
          return;
        }
      }
      match fetch_mutability(&rhs) { // fetch last ref mutability in the chain -> &&&mut x
        Some(Mutability::Not) => self.add_borrow_ev_of_ref_var(line_num, Evt::SBorrow, lhs, res, is_reborrow, of_ref_var),
        Some(Mutability::Mut) => self.add_borrow_ev_of_ref_var(line_num, Evt::MBorrow, lhs, res, is_reborrow, of_ref_var),
        None => panic!("internal state: a borrow expression has no `&`/`&mut` to take the mutability from")
      }
    }
    // `let x = 'a: { if c { break 'a s; } t };` (#1134): x gets what
    // the `break` carries or the last expression, whichever ended the
    // block; each is paired as one of them, where the block ends.
    // (Not into a `let` pattern that takes the value apart: not drawn yet.)
    ExprKind::Block(block, Some(_)) if !matches!(self.tcx.parent_hir_node(rhs.hir_id),
        rustc_hir::Node::LetStmt(l) if !matches!(l.pat.kind, PatKind::Binding(..)))
      => {
      let values = self.labeled_block_values(rhs);
      if values.len() > 1 {
        let saved = self.enter_deferred(rhs.hir_id);
        self.pair_break_alternatives(rhs, values, |this, v| this.match_rhs(lhs.clone(), v, evt.clone()));
        self.leave_deferred(saved);
      } else {
        let value = match values.as_slice() {
          [(value, _)] if self.body_facts.is_some_and(|body| {
            let binding = lhs.extract_rap().and_then(ResourceAccessPoint::owner_local);
            (binding.is_some() || matches!(lhs, ResourceTy::Caller))
              && body.value_flow.callable_result_within(value.span, binding)
          }) => Some(*value),
          _ => block.expr,
        };
        if let Some(value) = value {
          self.match_rhs(lhs, value, evt);
        }
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
          if let Some(src) = self.nested_reference_place(rhs, false) {
            self.add_ev(line_num, evt, lhs, src, false);
            return;
          }
          let rhs_rap = fetch_rap(&expr, &self.tcx, &self.raps);
          let res = match rhs_rap {
            Some(x) => {
              self.update_rap(&x, line_num);
              if let Some(place) = self.projected_access(rhs.span, &x, |f| match f {
                crate::facts::Fact::Copy { path, .. } => Some(path), _ => None,
              }) {
                let ty = self.tcx.typeck(rhs.hir_id.owner).expr_ty(rhs);
                ResourceTy::DerefField(x, place, PlaceWrite::Update, self.field_holds(ty, rhs.hir_id.owner))
              } else { ResourceTy::Deref(x) }
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
        .and_then(|n| self.raps.get(&fn_key(&n)).map(|rd| fn_result_rap(rhs, rd.rap.with_mir_result(), &self.tcx)))
        .map_or(ResourceTy::Anonymous, ResourceTy::Value);
      let Some(call) = self.mir_call(rhs).filter(|c| c.has_result) else { return };
      let evt = if matches!(lhs, ResourceTy::Caller) && call.copyable { Evt::Copy } else { evt };
      self.add_call_result(call, line_num, evt, lhs.clone(), from);
      self.emit_struct_field_borrows(&lhs, rhs, name_and_generic_args.ident.as_str().to_owned(), line_num);
    }
    // Struct intializer list:
    // ex struct = {a: <expr>, b: <expr>, c: <expr>}
    ExprKind::Struct(_qpath, expr_fields, base) => self.match_rhs_struct(lhs, rhs, _qpath, expr_fields, base),

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
      let only = self.literal_parts.take();
      if !matches!(lhs, ResourceTy::Anonymous) && only != Some(false) {
        self.add_ev(line_num, Evt::Bind, lhs.clone(), ResourceTy::Anonymous, false);
      }
      for (i, elem) in elems.iter().enumerate() {
        let cond = self.conditional_value(elem);
        if only.is_some_and(|c| c != cond.is_some()) {
          continue;
        }
        let e = elem_evt(self, elem);
        // (The element's type is the whole element's, `Option<&String>`
        // for `Some(if c { &a } else { &b })`, #1470.)
        let elem_ty = typeck.node_type(elem.hir_id);
        // `(Some(if c { d } else { e }), 1)` (#1267): the constructor
        // takes nothing itself; the element gets each branch's value.
        let elem = if only == Some(true) { cond.unwrap_or(elem) } else { elem };
        match members.as_ref().and_then(|m| m[i].clone()) {
          Some(member) => {
            // An element drawn as one column whose value holds a borrow
            // (`(Some(&s1), 1)`, `(if c { Some(&a) } else { .. }, 1)`)
            // holds it, as a struct literal's field does (#1454).
            // `(Some(if c { &a } else { &b }), 1)` (#1470): such an
            // element gets each branch's value, and its borrow, as `(if c
            // { Some(&a) } else { .. }, 1)` does.
            let holder_value = cond.unwrap_or(elem);
            let member = if !elem_ty.is_ref() && !self.has_member_columns(elem_ty)
              && matches!(member, ResourceAccessPoint::Struct(_)) {
              let name = member.name().to_owned();
              self.register_holder(&name, holder_value, elem_ty);
              self.raps.get(&name).map_or(member, |rd| rd.rap.to_owned())
            } else {
              member
            };
            let elem = if member.holds_borrow() && !member.is_ref() { holder_value } else { elem };
            // A reference element: resolve its lender from the
            // element expression, as the Struct arm does for
            // reference fields, and the kind of the loan it holds
            // (`(downgrade(&mut s), 1)` holds a mutable one, #1414).
            if elem_ty.is_ref() {
              let (lender, aliasing) = self.get_ref_data(elem);
              if let Some(rd) = self.borrow_map.get_mut(member.name()) {
                rd.lender = lender;
                rd.aliasing = aliasing;
              }
              self.note_loan_kinds(member.name(), elem);
            }
            self.match_rhs(ResourceTy::Value(member), elem, e);
          }
          // No column for this element (the tuple is returned to the
          // caller, written through a reference, …). A variable moved
          // into it is still moved: record that, so it isn't shown
          // dropping the value at its scope end.
          None => {
            if matches!(lhs, ResourceTy::Caller) && (self.is_returned_call_result(elem) || self.is_nested_literal(elem)) {
              self.match_rhs(ResourceTy::Caller, elem, e);
              continue;
            }
            // `(x, 1)` returned with x a reference (#1340).
            if matches!(lhs, ResourceTy::Caller) && self.ref_part_to(elem, ResourceTy::Caller) {
              continue;
            }
            // `(*b, 1)`: the Box's contents (#1191).
            let is_place = matches!(elem.kind, ExprKind::Path(_) | ExprKind::Field(..)) || self.box_contents_of(elem).is_some();
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
        if self.is_coerced_shared_reborrow(&lhs, &rhs_rap, rhs, &evt) {
          self.add_borrow_ev(line_num, Evt::SBorrow, lhs, ResourceTy::Value(rhs_rap), true);
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
      } else if let Some(rap) = self.element_field_holder(rhs)
        .filter(|_| matches!(evt, Evt::Copy) && matches!(lhs, ResourceTy::Value(_))) {
        // `let a = pts[0].x;`: a copy of a field of an element, drawn
        // on the collection's column like `let a = v[0];` (#1065).
        // (Moving out of an element doesn't compile, E0507 / E0508.)
        // Only into a variable, as for `v[0]` above.
        let line_num = span_to_line(&inner.span, &self.tcx);
        self.update_rap(&rap, line_num);
        let place = self.place_label(rhs, rap.name());
        self.add_ev(line_num, evt, lhs, ResourceTy::Index(rap, place, Default::default()), false);
      } else if let Some(rap) = self.static_field_holder(rhs) {
        // `let x = ORIGIN.x;`: a copy of a part of a static, which has
        // one column for all of it (#690). Drawn like a copy of an
        // element (`v[0]`): out of `ORIGIN.x`, on ORIGIN's column.
        // (Moving out of a static doesn't compile.)
        let line_num = span_to_line(&inner.span, &self.tcx);
        self.update_rap(&rap, line_num);
        let place = self.place_label(rhs, rap.name());
        self.add_ev(line_num, evt, lhs, ResourceTy::Index(rap, place, Default::default()), false);
      } else {
        // A field of a temporary (`let k = (u, 1).1;`): no column to copy
        // or move it from, but the variable still gets its value (#737).
        let line_num = span_to_line(&inner.span, &self.tcx);
        self.add_ev(line_num, Evt::Bind, lhs, ResourceTy::Anonymous, false);
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
      let bodies: Vec<&'tcx Expr<'tcx>> = std::iter::once(if_block).chain(else_block).collect();
      self.match_rhs_arms(lhs, &bodies, evt);
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
              if let Some((from, kind, site)) = self.try_moved_place(rhs) {
                let previous = self.try_moves.insert(rhs.hir_id, None).flatten();
                let id = previous.unwrap_or_else(|| {
                  let id = *self.unique_id;
                  *self.unique_id += 1;
                  id
                });
                if previous.is_some() {
                  self.preprocessed_events.retain(|(_, event)| event.get_id() != id);
                }
                self.transfer_sites.insert(id, site);
                if let Some(path) = self.body_facts.and_then(|b| b.value_flow.returned_try(rhs.span)).filter(|p| !p.is_empty()) {
                  let title = crate::svg_generator::hover_messages::event_dot_try_into_returned_ctor(
                    &from.name(), kind, path, from.extract_rap().is_some_and(|r| r.holds_value()) && matches!(from, ResourceTy::Value(_)));
                  if matches!(lhs, ResourceTy::Caller) {
                    self.row_layout.aggregate_titles.insert(id, title.clone());
                  }
                  self.try_result_titles.insert(id, title);
                }
                self.try_move = Some(kind);
                let partial = matches!(from, ResourceTy::Index(..));
                let event = self.ext_ev_of_evt(Evt::Move, lhs, from, id, partial);
                self.add_external_event(site.row, event);
                self.try_move = None;
                return;
              }
            }
            // `let z = w?;` with `w: Option<&String>` (Copy): z gets a
            // copy of the value inside w, not of w; a reference copied
            // out of w borrows what w borrows, as for `let Some(z) = w`
            // (#1352).
            if let (ResourceTy::Value(rap), Some((read, kind))) = (&lhs,
              self.body_facts.and_then(|b| b.value_flow.try_input(rhs.span)))
            {
              if let OperandEffect::Copy(source) = &read.effect {
                if let Some(from) = self.transfer_source(source, operand) {
                  let rap = rap.clone();
                  let row = read.site.row;
                  if let Some(from) = from.extract_rap() {
                    self.update_rap(from, row);
                  }
                  if *kind == TryMove::Other {
                    let title = crate::svg_generator::hover_messages::copied_custom_try(rap.name(), &from.name());
                    self.row_layout.aggregate_titles.insert(*self.unique_id, title);
                    self.bind_from_anonymous(row, lhs);
                    return;
                  }
                  self.add_ev(row, Evt::Copy, lhs, from, true);
                  for event in self.pattern_ref_copy_borrow(&rap, &Evt::Copy, operand, None) {
                    self.add_external_event(row, event);
                  }
                  return;
                }
              }
              if matches!(read.effect, OperandEffect::Move(_)) {
                self.add_ev(read.site.row, Evt::Bind, lhs, ResourceTy::Anonymous, false);
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
      let bodies: Vec<&'tcx Expr<'tcx>> = arms.iter().map(|a| a.body).collect();
      self.match_rhs_arms(lhs, &bodies, evt);
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
      let uncast = peel_ref_casts(rhs, typeck);
      if !std::ptr::eq(uncast, rhs) {
        // `&mut arr as &mut [i32]` borrows arr, like `&mut arr` (#1175).
        self.match_rhs(lhs, uncast, evt);
      } else if plain(inner) && plain(rhs) {
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
    ExprKind::Closure(closure) => self.match_rhs_closure(lhs, rhs, closure),
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
      // No tracked borrow in the collection (`let s: &str = words[1];`
      // with `words = vec!["a", "b"]`): s still starts here, bound to
      // the reference it copied (#894).
      if lenders.is_empty() {
        self.add_ev(line_num, Evt::Bind, lhs, ResourceTy::Anonymous, false);
        return;
      }
      let Some(coll) = expr_to_rap_name(base, &self.tcx) else { return };
      let via = BorrowVia {
        func: coll,
        lenders: lenders.iter().map(|l| l.name()).collect(),
        also_unnamed: false,
        tie: ArgTie::default(),
        carrier: ViaCarrier::Element,
        passed_on: Vec::new(),
      };
      for lender in lenders {
        let id = *self.unique_id;
        self.add_external_event(line_num, ExternalEvent::StaticBorrow {
          from: lender, to: lhs.clone(), is_partial: false, is_reborrow: false, break_alt: None, via: Some(via.clone()), id, of_ref_var: false,
        });
        *self.unique_id += 1;
      }
    }
    _ => {
      warn!("unmatched rhs {:#?}", rhs);
    }
  }
}

/// The `ExprKind::Path` case of `match_rhs`: `let lhs = <path>;` (a variable, a const or static, a unit struct or variant).
pub fn match_rhs_path(&mut self, lhs: ResourceTy, rhs: &'tcx Expr<'tcx>, evt: Evt, p: &'tcx rustc_hir::Path<'tcx>) {
  // const items aren't function-local RAPs — they're compile-time
  // values baked into the binary. Treat reading one on the RHS of a
  // let as if the RHS were a literal: the LHS gets a Bind from an
  // Anonymous source. Same handling as the path-arm in visitor.rs's
  // main expression walker. (A `static` has a column, #690: reading
  // it is a copy out of it, like reading a variable.)
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
        | rustc_hir::def::DefKind::Fn
        | rustc_hir::def::DefKind::AssocFn,
      _
    )
  ) {
    let line_num = span_to_line(&p.span, &self.tcx);
    // A Copy constant: the variable gets a copy of its value, which the
    // tooltip names (#690). (Not a reference variable: it is bound to
    // what the constant points to, which its borrow's end names.)
    if matches!(p.res, Res::Def(rustc_hir::def::DefKind::Const { .. } | rustc_hir::def::DefKind::AssocConst { .. }, _))
      && matches!(lhs, ResourceTy::Value(ResourceAccessPoint::Owner(_)))
      && self.ty_is_copy(self.tcx.typeck(rhs.hir_id.owner).node_type(rhs.hir_id), rhs.hir_id.owner)
    {
      let name = p.segments.iter().map(|s| s.ident.as_str()).collect::<Vec<_>>().join("::");
      let id = *self.unique_id;
      *self.unique_id += 1;
      self.add_external_event(line_num, ExternalEvent::Copy {
        from: ResourceTy::Anonymous, to: lhs, id, is_partial: false, into_closure: false, break_alt: None,
        closure_value: false, into_ctor: None, pointee: None, of_const: Some(name),
      });
      return;
    }
    self.bind_from_anonymous(line_num, lhs);
    return;
  }
  let line_num = span_to_line(&p.span, &self.tcx);
  // `let n: Option<i32> = None;` is a literal value too (#768), and so
  // is a unit variant or unit struct of any type (`Fruit::Banana`,
  // `None` as an `Option<String>`, `Marker`): the constructor isn't an
  // actor (#873).
  if self.is_copy_literal_value(rhs)
    || matches!(p.res, Res::Def(rustc_hir::def::DefKind::Ctor(_, rustc_hir::def::CtorKind::Const), _)) {
    self.bind_from_ctor(line_num, lhs, rhs);
    return;
  }
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
  let rhs_key = if matches!(p.res, Res::Local(_)) || static_of_path(p).is_some() { rhs_name.clone() } else { fn_key(&rhs_name) };
  match if let Res::Local(id) = p.res { self.raps.binding(id) } else { self.raps.get(&rhs_key) } {
    Some(rd) => {
      let rhs_rap = rd.rap.to_owned();
      self.update_rap(&rhs_rap, line_num);
      if self.is_coerced_mut_reborrow(&lhs, &rhs_rap, rhs, &evt) {
        self.add_borrow_ev(line_num, Evt::MBorrow, lhs, ResourceTy::Value(rhs_rap), true);
        return;
      }
      if self.is_coerced_shared_reborrow(&lhs, &rhs_rap, rhs, &evt) {
        self.add_borrow_ev(line_num, Evt::SBorrow, lhs, ResourceTy::Value(rhs_rap), true);
        return;
      }
      if rhs_rap.is_closure() && lhs.is_closure() && matches!(evt, Evt::Copy | Evt::Move) {
        self.move_or_copy_closure(line_num, evt, lhs, rhs_rap);
        return;
      }
      if self.ref_into_holder_value(&lhs, &rhs_rap, rhs, line_num) {
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
}

/// `lhs`, a value that holds a borrow (not a reference itself), gets
/// the reference in the variable `r` as part of its value: a branch of
/// `let y = Some(if c { let r = &s1; r } else { .. });` (#1225's
/// constructor around a conditional, paired branch by branch) or of `q =
/// Some(if c { r } else { &b });` (#1470). y isn't a copy of r: its
/// value holds a copy of r's reference (or, for a `&mut`, r's reference
/// moved into it), so y borrows what r borrows. Drawn as `let y =
/// Some(r);` and `let y = [r];` are (#1472): a borrow arrow from each of
/// r's lenders into y, and for a `&mut` the move of r's reference into y
/// first. False, drawing nothing, for any other pairing.
fn ref_into_holder_value(&mut self, lhs: &ResourceTy, r: &ResourceAccessPoint, rhs: &'tcx Expr<'tcx>, line_num: usize) -> bool {
  let ResourceTy::Value(h) = lhs else { return false };
  if h.is_ref() || !h.holds_borrow() || !r.is_ref() {
    return false;
  }
  let Some(ty) = self.tcx.typeck(rhs.hir_id.owner).expr_ty_opt(rhs).filter(|t| t.is_ref()) else { return false };
  let (lender, _, co) = self.get_ref_data_co(rhs);
  let lenders: Vec<ResourceTy> = std::iter::once(lender).chain(co)
    .filter(|l| matches!(l, ResourceTy::Value(_) | ResourceTy::Deref(_)))
    .collect();
  if lenders.is_empty() {
    return false;
  }
  let is_mut = ty.ref_mutability().is_some_and(|m| m.is_mut());
  if is_mut {
    self.add_ev(line_num, Evt::Move, lhs.clone(), ResourceTy::Value(r.clone()), false);
  }
  let evt = if is_mut { Evt::MBorrow } else { Evt::SBorrow };
  for l in lenders {
    match l {
      // r reborrows through another reference (`&mut *p`): drawn from
      // that reference's column, as `[r]` draws it (#1330).
      ResourceTy::Deref(p) => self.add_borrow_ev(line_num, evt.clone(), lhs.clone(), ResourceTy::Value(p), true),
      l => self.add_ev(line_num, evt.clone(), lhs.clone(), l, false),
    }
  }
  true
}

/// `let w = <rhs>;` with rhs a call, a method call, an index or a
/// deref that reads places (`v.iter().copied().collect()`,
/// `firsts(&v)`, `v[0]`, `v.clone()`): recorded in `derived`. Whether
/// w holds a borrow stored in one of them is left to rustc's borrow
/// checker (`names_holding_loans`): w's type carries the loan when the
/// signatures along the way tie it to the source's lifetimes (#1015).
/// A clone (`v.clone()`, `Clone::clone(&v)`, `v.to_vec()`,
/// `v.to_owned()`, std's, with w getting v's own type) is marked as one,
/// for the text (#1003).
fn note_derived(&mut self, lhs: &ResourceTy, rhs: &'tcx Expr<'tcx>) {
  use rustc_middle::ty;
  let ResourceTy::Value(dest) = lhs else { return };
  if dest.is_fn() {
    return;
  }
  if !matches!(rhs.kind, ExprKind::Call(..) | ExprKind::MethodCall(..) | ExprKind::Index(..) | ExprKind::Unary(UnOp::Deref, _)) {
    return;
  }
  let typeck = self.tcx.typeck(rhs.hir_id.owner);
  let env = typing_env(self.tcx, rhs.hir_id.owner);
  /// The places an expression reads, outside closures: a local, or a
  /// field of one (`h.v`, not also `h`).
  struct Places<'a, 'tcx> { tcx: &'a TyCtxt<'tcx>, out: Vec<&'tcx Expr<'tcx>> }
  impl<'a, 'tcx> rustc_hir::intravisit::Visitor<'tcx> for Places<'a, 'tcx> {
    fn visit_expr(&mut self, e: &'tcx Expr<'tcx>) {
      match e.kind {
        ExprKind::Closure(..) => {}
        ExprKind::Path(QPath::Resolved(None, p)) if matches!(p.res, Res::Local(_)) => self.out.push(e),
        ExprKind::Field(..) if expr_to_rap_name(e, self.tcx).is_some() && field_base_is_local(e) => self.out.push(e),
        _ => rustc_hir::intravisit::walk_expr(self, e),
      }
    }
  }
  fn field_base_is_local(e: &Expr) -> bool {
    match e.kind {
      ExprKind::Field(inner, _) => field_base_is_local(inner),
      ExprKind::Path(QPath::Resolved(None, p)) => matches!(p.res, Res::Local(_)),
      _ => false,
    }
  }
  let mut places = Places { tcx: &self.tcx, out: Vec::new() };
  rustc_hir::intravisit::Visitor::visit_expr(&mut places, rhs);
  let mut srcs: Vec<(String, bool)> = Vec::new();
  for p in places.out {
    let Some(name) = expr_to_rap_name(p, &self.tcx) else { continue };
    if name == dest.name().as_str() || srcs.iter().any(|(n, _)| *n == name) {
      continue;
    }
    srcs.push((name, typeck.expr_ty(p).has_significant_drop(self.tcx, env)));
  }
  if srcs.is_empty() {
    return;
  }
  let ty = typeck.expr_ty(rhs);
  // A value with no lifetime in its type (`v.len()`) holds no borrow.
  if !ty.walk().any(|g| matches!(g.kind(), rustc_middle::ty::GenericArgKind::Lifetime(_))) {
    return;
  }
  // A clone of v: std's `clone` / `to_vec` / `to_owned` of the place,
  // giving w v's own type.
  let clone_of = match rhs.kind {
    ExprKind::MethodCall(_, recv, [], _) => typeck.type_dependent_def_id(rhs.hir_id).map(|d| (d, recv)),
    ExprKind::Call(callee, [arg]) => match typeck.node_type_opt(callee.hir_id).map(|t| *t.kind()) {
      Some(ty::FnDef(def_id, _)) => Some((def_id, arg)),
      _ => None,
    },
    _ => None,
  }.filter(|(def_id, _)| !def_id.is_local()
    && matches!(self.tcx.crate_name(def_id.krate).as_str(), "core" | "alloc" | "std")
    && matches!(self.tcx.item_name(*def_id).as_str(), "clone" | "to_vec" | "to_owned"))
  .map(|(_, recv)| match recv.kind {
    ExprKind::AddrOf(_, _, inner) => inner,
    _ => recv,
  })
  .filter(|place| matches!(place.kind, ExprKind::Path(_) | ExprKind::Field(..)) && typeck.expr_ty(place).peel_refs() == ty);
  let clone = clone_of.is_some() && srcs.len() == 1;
  // A value whose drop may use the borrow (a type with its own `Drop`)
  // holds it until it is dropped, not until its last use; so does a
  // field of a struct literal of such a type (`D { v: … }`).
  let mut dropped = ty.has_significant_drop(self.tcx, env);
  let mut at = rhs.hir_id;
  while let rustc_hir::Node::ExprField(f) = self.tcx.parent_hir_node(at) {
    let rustc_hir::Node::Expr(lit) = self.tcx.parent_hir_node(f.hir_id) else { break };
    dropped |= typeck.expr_ty(lit).has_significant_drop(self.tcx, env);
    at = lit.hir_id;
  }
  self.derived.push(Derived {
    lines: (span_to_line(&rhs.span, &self.tcx), span_end_line(&rhs.span, &self.tcx)),
    srcs, dest: dest.name().to_owned(), clone, dropped,
  });
}

/// The `ExprKind::Call` case of `match_rhs`: `let lhs = f(args);`.
pub fn match_rhs_call(&mut self, lhs: ResourceTy, rhs: &'tcx Expr<'tcx>, evt: Evt, fn_expr: &'tcx Expr<'tcx>) {
  let line_num = span_to_line(&fn_expr.span, &self.tcx);
  // `let (d, _) = pair();`, `let Ok(s) = res() else { .. }`: the call's
  // result is a temporary the pattern takes a part of, with a column of
  // its own (`record_temporary_drop`); the binding takes that part out
  // of the temporary, not out of the call (#1386).
  if let Some(tmp) = self.temp_cols.get(&rhs.hir_id).cloned()
    .filter(|t| matches!(t.temp_part(), Some(TempPart::Let(_) | TempPart::LetElse(_) | TempPart::Assign(_)))) {
    if matches!(evt, Evt::Move | Evt::Copy) {
      self.add_ev(line_num, evt, lhs, ResourceTy::Value(tmp), true);
      return;
    }
  }
  // `let o = Some(5);`: a constructor of literals is a value of the
  // type, like `let x = 5;`. Nothing is copied from the
  // constructor (#768).
  let constructor = self.mir_constructor(rhs);
  if constructor && self.is_copy_literal_value(rhs) {
    self.bind_from_ctor(line_num, lhs, rhs);
    return;
  }
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
  // `let h = Holder(&x);` / `let w = W(r);`: a tuple struct whose
  // fields have columns is built like the literal `Holder { 0: &x }`,
  // each field getting its argument's value or borrow (#1197, #1206).
  if let ExprKind::Call(_, args) = rhs.kind {
    if constructor
      && self.has_tuple_field_columns(&lhs, args.len())
    {
      self.discard_ctor_arg_events(rhs);
      let from = hirid_to_var_name(fn_expr.hir_id, &self.tcx)
        .and_then(|n| self.raps.get(&fn_key(&n)))
        .map_or(ResourceTy::Anonymous, |rd| ResourceTy::Value(rd.rap.to_owned()));
      self.add_ev(line_num, Evt::Bind, lhs.clone(), from, false);
      for (i, a) in args.iter().enumerate() {
        self.match_struct_field(&lhs, &i.to_string(), a);
      }
      return;
    }
  }
  let retargeted = constructor
    && self.retarget_ctor_arg_events(rhs, lhs.clone());
  if let Some(func) = hirid_to_var_name(fn_expr.hir_id, &self.tcx) {
    if self.emit_call_result_borrows(&lhs, rhs, func, line_num) {
      return;
    }
  }
  // `let r2 = id(r);` with r a `&mut` parameter moved into the call:
  // r2 holds r's reference now, so r's borrow doesn't end at the move
  // (#1205). So does a value that holds it (`let k = Some(r);`, `let k =
  // Box::new(r);`, #1289).
  if let ExprKind::Call(_, args) = rhs.kind {
    if matches!(&lhs, ResourceTy::Value(l) if l.is_ref() || l.holds_borrow()) {
      let mut found = Vec::new();
      for a in args {
        self.mut_ref_params_moved_in(a, &mut found);
      }
      let moved: Vec<ResourceTy> = found.into_iter()
        .filter_map(|a| expr_to_rap_name(a, &self.tcx).and_then(|n| self.raps.get(&n)).map(|rd| ResourceTy::Value(rd.rap.to_owned())))
        .collect();
      if !moved.is_empty() {
        self.mark_borrow_kept_by_result(rhs, &moved);
        // A value built by a call (`Box::new(r)`, `id(Some(r))`) that
        // holds the reference: say so on its column, as a borrow of
        // the caller's data (no column here). (A constructor, `Some(r)`,
        // draws r's move into it instead.)
        if matches!(&lhs, ResourceTy::Value(l) if !l.is_ref() && l.holds_borrow())
          && !constructor {
          // The value comes from the call, then the borrow it holds.
          if let Some(f) = hirid_to_var_name(fn_expr.hir_id, &self.tcx)
            .and_then(|n| self.raps.get(&fn_key(&n)).map(|rd| rd.rap.to_owned())) {
            self.add_ev(line_num, Evt::Move, lhs.clone(), ResourceTy::Value(f), false);
          }
          let id = *self.unique_id;
          self.add_external_event(line_num, ExternalEvent::MutableBorrow { from: ResourceTy::Caller, to: lhs.clone(), is_partial: false, is_reborrow: false, break_alt: None, via: None, id, of_ref_var: false });
          *self.unique_id += 1;
          return;
        }
      }
    }
  }
  // `let moved = Some(m);`: `m` was moved into `moved`; nothing is
  // moved out of the constructor (#768).
  if retargeted {
    return;
  }
  // `opt = Some(x + 1);` (#866), `let o = Some(a + &b);`,
  // `y = Fruit::Apple(c + 1);` (#873): the value the constructor
  // builds from values with no column (an operator's result, a macro's
  // value, literals) is a new value, like a constructor of literals.
  // Nothing is moved or copied out of the constructor: it is never an
  // actor (#768).
  if constructor {
    self.bind_from_ctor(line_num, lhs, rhs);
    return;
  }
  // The receiver operand and the value written on the call's normal
  // successor are separate. The receiver's move is drawn at the call.
  let Some(call) = self.mir_call(rhs).filter(|c| c.has_result) else {
    return;
  };
  let evt = if matches!(lhs, ResourceTy::Caller) && call.copyable { Evt::Copy } else { evt };
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
        .map(|rd| ResourceTy::Value(fn_result_rap(rhs, rd.rap.with_mir_result(), &self.tcx)))
        .unwrap_or(ResourceTy::Anonymous)
    }
    None => ResourceTy::Anonymous,
  };
  self.add_call_result(call, line_num, evt, lhs.clone(), from);
  if let Some(func) = hirid_to_var_name(fn_expr.hir_id, &self.tcx) {
    self.emit_struct_field_borrows(&lhs, rhs, func, line_num);
  }
}

/// The name of the tuple-struct / variant constructor whose argument
/// the literal `e` is, directly or as a part of an enclosing struct or
/// tuple literal (`Some(W { a: &s })`, `Some((W { .. }, 1))`).
pub fn enclosing_ctor_name(&self, e: &'tcx Expr<'tcx>) -> Option<String> {
  match self.enclosing_ctor(e)?.kind {
    ExprKind::Call(callee, _) => hirid_to_var_name(callee.hir_id, &self.tcx),
    _ => None,
  }
}

/// A literal written as a part of a returned literal: a struct
/// literal, tuple, array, `vec![..]`, or a constructor (`Some(b)`) that
/// isn't a value of literals. What it is built from goes to the caller
/// with it (#1143).
pub fn is_nested_literal(&self, e: &'tcx Expr<'tcx>) -> bool {
  if vec_macro_elems(self.tcx, e).is_some() {
    return true;
  }
  !e.span.from_expansion() && match e.kind {
    ExprKind::Struct(..) | ExprKind::Array(_) => true,
    ExprKind::Tup(parts) => !parts.is_empty(),
    ExprKind::Call(callee, args) => !args.is_empty()
      && crate::visitor::is_ctor_path(callee, self.tcx.typeck(e.hir_id.owner))
      && !self.is_copy_literal_value(e),
    _ => false,
  }
}

/// `f(S { a: e, ..base })`: the fields of `base` that aren't written
/// out go to `to` (the call) with the new value, moved if they need it
/// and copied otherwise, a partial move of `base` (#1143). A reference
/// field is copied (`&T`) or moved (`&mut T`) like any other. Nothing
/// to draw when `base` has no field columns (`..Default::default()`).
pub fn move_base_rest(&mut self, to: ResourceTy, base: &'tcx Expr<'tcx>, written: &'tcx [rustc_hir::ExprField<'tcx>]) {
  let line = span_to_line(&base.span, &self.tcx);
  let Some(base_name) = expr_to_rap_name(base, &self.tcx).filter(|n| self.raps.contains_key(n)) else { return };
  let prefix = format!("{}.", base_name);
  #[expect(clippy::disallowed_methods, reason = "sorted by (hash, name) below")]
  let mut members: Vec<ResourceAccessPoint> = self.raps.iter()
    .filter(|(n, rd)| n.starts_with(&prefix) && !n[prefix.len()..].contains('.') && rd.rap.is_member())
    .map(|(_, rd)| rd.rap.clone())
    .collect();
  members.sort_by(|a, b| a.hash().cmp(b.hash()).then_with(|| a.name().cmp(b.name())));
  for m in members {
    let field = &m.name()[prefix.len()..];
    if written.iter().any(|f| f.ident.as_str() == field) {
      continue;
    }
    let e = match &m {
      ResourceAccessPoint::MutRef(_) => Evt::Move,
      _ if m.never_drops() => Evt::Copy,
      _ => Evt::Move,
    };
    self.add_ev(line, e, to.clone(), ResourceTy::Value(m), false);
  }
}

/// One field of a struct literal (`f: expr` in `S { f: expr }`) or
/// of a tuple-struct constructor (`expr` as `W(expr)`'s field `0`,
/// #1197) going into the struct `lhs`, which has per-field columns:
/// `lhs.f` gets expr's value (moved or copied), or the borrow it holds.
pub fn match_struct_field(&mut self, lhs: &ResourceTy, field_name: &str, expr: &'tcx Expr<'tcx>) {
  let ty = self.tcx.typeck(expr.hir_id.owner).node_type(expr.hir_id);
  self.match_struct_field_of(lhs, field_name, expr, ty);
}

/// `match_struct_field` for a field of type `field_ty` given the value
/// of `expr`: the field's own initializer, or, for `W { o: Some(if c {
/// &a } else { &b }) }` (#1267), the conditional inside it.
pub fn match_struct_field_of(&mut self, lhs: &ResourceTy, field_name: &str, expr: &'tcx Expr<'tcx>, field_ty: Ty<'tcx>) {
  let new_lhs_name = format!("{}.{}", lhs.name(), field_name);
  // Skip fields whose RAP we never registered. The most
  // common case is a struct field whose type is itself a
  // struct (`R { a: A { b: ... } }`): define_lhs registers
  // `r.a` but doesn't recurse to add `r.a.b`, so the inner
  // struct's expansion lands here without an entry. Better
  // to lose the per-field event than crash the whole render.
  if !self.raps.contains_key(&new_lhs_name) {
    return;
  }
  // A field drawn as one column whose value holds a borrow
  // (`items: vec![w.as_str()]`, `it: v.iter()`) holds it like
  // `let items = vec![w.as_str()];` would (#1088).
  let holder = !field_ty.is_ref() && !self.has_member_columns(field_ty)
    && matches!(self.raps.get(&new_lhs_name).map(|rd| &rd.rap), Some(ResourceAccessPoint::Struct(_)));
  // `o: Some(if c { &a } else { &b })` (#1470): it gets each branch's
  // value, and its borrow, as `o: if c { Some(&a) } else { .. }` does.
  let expr = match self.conditional_value(expr) {
    Some(c) if holder => c,
    _ => expr,
  };
  if holder {
    self.register_holder(&new_lhs_name, expr, field_ty);
  }
  let field_rap = self.raps.get(&new_lhs_name).unwrap().rap.to_owned();
  let is_copyable = self.ty_is_copy(field_ty, expr.hir_id.owner);
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
  // …` outside a struct) so emit_borrow_ends can pair the
  // borrow with its return-of-borrow at end-of-loan and
  // the ref-line trapezoid covers the right range.
  if field_ty.is_ref() {
    let (lender, aliasing, static_lender, co_lenders) = self.get_ref_data_full(&expr);
    if let Some(rd) = self.borrow_map.get_mut(&new_lhs_name) {
      rd.lender = lender;
      rd.aliasing = aliasing;
      rd.static_lender = static_lender;
      rd.co_lenders = co_lenders;
    }
    self.note_loan_kinds(&new_lhs_name, expr);
  }
  // A struct field initialised by a call (`O { inner: mk(&t), .. }`):
  // its reference fields borrow what the callee's signature ties
  // them to, as for `let inner = mk(&t);` (#638).
  if matches!(field_rap, ResourceAccessPoint::Struct(_)) {
    self.register_struct_field_lenders(&new_lhs_name, expr, field_ty);
  }
  self.match_rhs(ResourceTy::Value(field_rap), expr, e);
  match &mut self.deferred_holder_borrows {
    Some(later) => later.push((new_lhs_name.clone(), expr)),
    None => self.emit_holder_literal_borrows(&new_lhs_name, expr),
  }
}

/// The `ExprKind::Struct` case of `match_rhs`: `let lhs = S { f: v, ..base };`.
pub fn match_rhs_struct(&mut self, lhs: ResourceTy, rhs: &'tcx Expr<'tcx>, _qpath: &'tcx QPath<'tcx>, expr_fields: &'tcx [rustc_hir::ExprField<'tcx>], base: rustc_hir::StructTailExpr<'tcx>) {
  let line_num = span_to_line(&rhs.span, &self.tcx);
  // `v[0] = B { name: r };`, `*slot = B { name: s };`: the place has no
  // column per field, so what the literal is built from goes into the
  // place itself, as for `v[0] = r;` (#1143).
  // The same for a destination drawn as one column: an enum's struct
  // variant (`let m = Msg::Write { text: s };`, #1173).
  let opaque = match &lhs {
    ResourceTy::Index(..) | ResourceTy::Deref(_) | ResourceTy::DerefField(..) => true,
    ResourceTy::Value(rap) => !rap.is_fn() && self.struct_members_deep(rap.name()).is_empty()
      && !expr_fields.is_empty(),
    _ => false,
  };
  let only = self.literal_parts.take();
  if opaque {
    // Its parts aren't split by `literal_parts` (#1256): the whole
    // literal goes in once, after it is visited.
    if only != Some(true) {
      self.match_literal_parts(lhs, vec![rhs], line_num);
    }
    return;
  }
  if only != Some(false) {
    self.add_ev(line_num, Evt::Bind, lhs.clone(), ResourceTy::Anonymous, false);
  }
  for field in expr_fields.iter() {
      let cond = self.conditional_value(field.expr);
      if only.is_some_and(|c| c != cond.is_some()) {
        continue;
      }
      // `W { s: Some(if …) }` (#1267): as for a tuple element. (The
      // field's type is the whole initializer's, `Option<&String>` for
      // `Some(if c { &a } else { &b })`, #1470.)
      let field_ty = self.tcx.typeck(field.expr.hir_id.owner).node_type(field.expr.hir_id);
      let field = &rustc_hir::ExprField { expr: if only == Some(true) { cond.unwrap_or(field.expr) } else { field.expr }, ..*field };
      // Struct literal that isn't bound to a struct RAP: the
      // value is returned (`User { email, .. }` as a fn's tail
      // expression). The field initialisers still move/copy
      // their sources (field-init shorthand `email` moves the
      // local `email`), into the value handed to the caller.
      if matches!(lhs, ResourceTy::Caller) {
        let field_ty = self.tcx.typeck(field.expr.hir_id.owner).node_type(field.expr.hir_id);
        let is_place = matches!(field.expr.kind, ExprKind::Path(_) | ExprKind::Field(..));
        // A literal in the field (`Out { inner: In { name } }`,
        // `W { o: Some(B { name }) }`): what it is built from goes to
        // the caller too (#1143).
        if (is_place || self.is_returned_call_result(field.expr) || self.is_nested_literal(field.expr))
          && !field_ty.is_ref() && !field.expr.span.from_expansion() {
          let e = if self.ty_is_copy(field_ty, field.expr.hir_id.owner) { Evt::Copy } else { Evt::Move };
          self.match_rhs(ResourceTy::Caller, field.expr, e);
        } else if field_ty.is_ref() {
          // `Part { text: x }` returned with x a reference (#1340).
          self.ref_part_to(field.expr, ResourceTy::Caller);
        }
        continue;
      }
      self.match_struct_field_of(&lhs, field.ident.as_str(), field.expr, field_ty);
  }
  // Struct update syntax `S { a: e, ..base }`: every field not
  // written explicitly comes from `base`, moved if its type isn't
  // Copy and copied otherwise. That's a *partial* move of `base`:
  // its explicitly-overridden fields stay put and are still
  // dropped at its scope end.
  if let (rustc_hir::StructTailExpr::Base(base_expr), true) = (base, only != Some(true)) {
    let base_line = span_to_line(&base_expr.span, &self.tcx);
    let base_name = expr_to_rap_name(base_expr, &self.tcx)
      .filter(|n| self.raps.contains_key(n));
    let explicit: HashSet<String> = expr_fields.iter().map(|f| f.ident.as_str().to_owned()).collect();
    // Its reference fields go too, with their borrows (#1126).
    if let (Some(b), ResourceTy::Value(rap @ ResourceAccessPoint::Struct(_))) = (&base_name, &lhs) {
      let (b, to) = (b.clone(), rap.name().clone());
      self.transfer_ref_fields_except(base_line, &b, &to, false, Some(&explicit));
    }
    // Field names come from whichever side has per-field RAPs:
    // the new struct, or (when it's returned to the caller) the base.
    let (field_names, dest_prefix): (Vec<(String, bool)>, Option<String>) = match &lhs {
      ResourceTy::Value(rap @ ResourceAccessPoint::Struct(_)) => {
        let prefix = rap.name().clone();
        (self.struct_children(&prefix).iter()
          .map(|c| (c.name()[prefix.len() + 1..].to_owned(), c.never_drops()))
          .collect(), Some(prefix))
      }
      ResourceTy::Caller => match &base_name {
        Some(b) => (self.struct_children(b).iter()
          .map(|c| (c.name()[b.len() + 1..].to_owned(), c.never_drops()))
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
}

/// The `ExprKind::Closure` case of `match_rhs`: `let lhs = |..| ..;`.
pub fn match_rhs_closure(&mut self, lhs: ResourceTy, rhs: &'tcx Expr<'tcx>, closure: &'tcx rustc_hir::Closure<'tcx>) {
  let line_num = span_to_line(&rhs.span, &self.tcx);
  // Bind for the closure value itself — gives `f` an "acquires
  // ownership of a resource" dot like other Bind sites.
  let bind = *self.unique_id;
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
  for (index, capture) in typeck.closure_min_captures_flattened(closure.def_id).enumerate() {
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
    let env = typing_env(self.tcx, rhs.hir_id.owner);
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
    // A reference variable captured itself (`|| println!("{}", r)`:
    // the macro takes `&r`), not the place it points to: the closure
    // borrows r, as `&r` does (#1180, #914).
    let of_ref_var = upvar_rap.is_ref() && !through_ref && capture.place.projections.is_empty();
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
    let capture_id = *self.unique_id;
    match evt_kind {
      Evt::SBorrow | Evt::MBorrow =>
        self.add_borrow_ev_of_ref_var(line_num, evt_kind.clone(), to, from.clone(), through_ref, of_ref_var),
      _ => self.add_ev(line_num, evt_kind.clone(), to, from.clone(), false),
    }

    self.row_layout.closure_captures.entry(bind).or_default().extend(capture_id..*self.unique_id);

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
    // A reference copied into a `move` closure (`move || r.len()`): the
    // copy borrows what r does, so r's borrow lasts while the closure can
    // still be called, as for a reborrow below (#1419).
    if copied {
      if let (ResourceTy::Value(c), ResourceTy::Value(r)) = (&lhs, &from) {
        if r.is_ref() && self.borrow_map.contains_key(r.name()) {
          if let Some(last) = c.owner_local().and_then(|l| self.local_last_live_line(l, (line_num, usize::MAX))) {
            let floor = self.borrow_floor.entry(r.name().to_owned()).or_insert(last);
            *floor = (*floor).max(last);
          }
        }
      }
    }
    if let Some(die_evt) = die_evt {
      let loan = crate::facts::call_wording::capture_borrows(self.mir_body, closure.def_id, index).and_then(|sites| {
        let [(at, _)] = sites.as_slice() else { return None };
        self.bwf.borrow_set.location_map().get_index_of(at)
      });
      let kill_line = loan.and_then(|id| mir_b_data.iter().find(|b| b.borrow_idx == id)).map(|b| b.region.1);
      // A reference the closure reborrows through or borrows (`|| r.len()`,
      // `|| println!("{}", r)`) stays live, and its own borrow with it,
      // while the closure holds that loan: up to the closure's last use,
      // not the reference's last use outside it. For a reference
      // parameter there is no loan of its own to say so: its borrow from
      // the caller was said to end before the closure captured it, and
      // after a let-chain the capture fell on a dead column (#1419).
      // (Reborrowing through a shared reference, `*r` with `r: &T`, is
      // no loan rustc tracks: the closure's own last use says it.)
      let holds_until = kill_line.or_else(|| match (&lhs, through_ref) {
        (ResourceTy::Value(c), true) => c.owner_local().and_then(|l| self.local_last_live_line(l, (line_num, usize::MAX))),
        _ => None,
      });
      if let (Some(die_line), Some(name)) = (holds_until, from.extract_rap().filter(|r| r.is_ref()).map(|r| r.name().to_owned())) {
        if self.borrow_map.contains_key(&name) {
          let floor = self.borrow_floor.entry(name).or_insert(die_line);
          *floor = (*floor).max(die_line);
        }
      }
      if let Some(die_line) = kill_line {
        // add_ev maps (lhs, rhs) → StaticDie/MutableDie
        // {from: rhs, to: lhs}. We want from=closure (f),
        // to=upvar (s) so the renderer's return arrow goes
        // f → s, mirroring the borrow direction.
        let mut die = self.ext_ev_of_evt(die_evt, from, lhs.clone(), *self.unique_id, false);
        if let ExternalEvent::StaticDie { of_ref_var: o, .. } | ExternalEvent::MutableDie { of_ref_var: o, .. } = &mut die {
          *o = of_ref_var;
        }
        if let Some(loan) = loan { self.end_loans.insert(*self.unique_id, vec![loan]); }
        self.add_external_event(die_line, die);
        *self.unique_id += 1;
      }
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
  // (Including those kept in a branch, for `mark_loop_returns`.)
  let exit_ids = exit_drops.clone();
  self.preprocessed_events.retain(|(_, e)| !exit_drops.contains(&e.get_id())
    || matches!(e, ExternalEvent::TemporaryDrop { early: true, .. }));
  // Visit RAPs in declaration order, not `self.raps`' hash order, so
  // the GoOutOfScope events (and their ids) come out the same on
  // every run (#211).
  #[expect(clippy::disallowed_methods, reason = "sorted by (hash, name) on the next line")]
  let mut raps: Vec<RapData> = self.raps.values().cloned().collect();
  raps.sort_by(|a, b| a.rap.hash().cmp(b.rap.hash()).then_with(|| a.rap.name().cmp(b.rap.name())));
  let mut pictured = BTreeSet::new();
  fn pictured_ends(events: &[(usize, ExternalEvent)], pictured: &mut BTreeSet<(Local, String)>, exits: &HashSet<usize>) {
    for (_, event) in events {
      match event {
        ExternalEvent::GoOutOfScope { ro, id, .. } if !exits.contains(id) => {
          if let Some(local) = ro.scope_local() { pictured.insert((local, ro.name().clone())); }
        }
        ExternalEvent::Branch { branches, .. } => {
          for arm in branches { pictured_ends(&arm.e_data, pictured, exits); }
        }
        _ => {}
      }
    }
  }
  pictured_ends(self.preprocessed_events, &mut pictured, &exit_ids);
  let mut named: Vec<_> = std::mem::take(&mut self.raps.top_level_places).into_iter()
    .filter(|(place, _)| !pictured.contains(place))
    .filter_map(|((local, name), layout_end)| Some((self.raps.place(local, &name)?.rap.clone(), layout_end)))
    .collect();
  named.sort_by_key(|(ro, _)| *ro.hash());
  // Event IDs also determine stacking with synthetic and reference columns.
  // Emit both groups in the column's registration order.
  for rap in raps.iter() {
    if rap.rap.scope_local().is_some() {
      for (ro, layout_end) in named.iter().filter(|(ro, _)| ro.hash() == rap.rap.hash()) {
        let facts = self.body_facts.map(|f| f.scope_ends.for_place(ro.scope_local().unwrap(), ro.name())).unwrap_or_default();
        if self.all_paths_exit && *layout_end >= self.all_paths_exit_from && !ro.never_drops() && !ro.is_ref()
          && !facts.iter().any(|end| end.site.is_none() && end.row < self.all_paths_exit_from) { continue; }
        self.add_scope_end(ro.clone(), *layout_end);
      }
      continue;
    }
    // need this to avoid duplicating out of scope events, this is due to the fact that RAPS is a map that lives over multiple fn ctxts
    // if a rap isn't global it means we annotated its GOS event inside its respective branch already
    // Every path left the function at a `return` / panic above, where
    // owners were dropped or moved out (#515); a reference has no
    // such event, and keeps its scope end.
    // Not a variable of a block that ended above the exit (#1139).
    let (all_exit, exit_from) = (self.all_paths_exit, self.all_paths_exit_from);
    let left_at = |scope: usize| all_exit && scope >= exit_from
      && !rap.rap.never_drops() && !rap.rap.is_ref();
    let left_already = left_at(rap.scope) && rap.extra_scopes.iter().all(|(s, _)| left_at(*s));
    // An unbound temporary is dropped by its own `TemporaryDrop`, which
    // may sit in a branch (a let chain's later part, #604): no scope end.
    // A first variable declared in a branch or loop body was ended
    // there; the later ones that share its column and aren't still are
    // (#931).
    if !rap.rap.is_fn() && (rap.is_global || !rap.extra_scopes.is_empty()) && !left_already && !rap.rap.is_unbound_temporary() {
      // Only a scope end within the first variable's scope is its own:
      // a later variable sharing the column, declared in a branch or
      // loop body, was ended after it (#931).
      let mut duplicate = false;
      for (l, e) in self.preprocessed_events.iter() {
        match e.is_gos_ev() {
          Some(r) => {
            if *r == rap.rap && *l <= rap.scope {
              duplicate = true;
              break;
            }
          }
          None => {}
        }
      }
      let owner = |local| self.raps.earlier.get(&local).map(|(rd, _)| rd.rap.clone());
      let first = rap.first_local.and_then(&owner).unwrap_or_else(|| rap.rap.clone());
      let later: Vec<_> = rap.extra_scopes.iter().filter(|(s, _)| !left_at(*s))
        .map(|(s, local)| (*s, local.and_then(&owner).unwrap_or_else(|| rap.rap.clone()))).collect();
      if rap.is_global && !duplicate && !left_at(rap.scope) {
        self.add_scope_end(first, rap.scope);
      }
      // The later variables that share this column (#589).
      for (scope, ro) in later {
        self.add_scope_end(ro, scope);
      }
    }
  }
  self.mark_loop_returns(&exit_ids);
}
  
  /// Match the guard's MIR FalseEdge whose imaginary target enters this
  /// arm. Exact spans also distinguish sibling conditionals on one line.
  pub(crate) fn imaginary_edge(&self, arm: rustc_span::Span) -> Option<&'a crate::facts::ImaginaryEdge> {
    self.body_facts?.imaginary_edges.iter().find(|edge| edge.id.0 == arm.with_parent(None))
  }

  /// The ends of the borrows the walk left open (RustViz 3 P2-1b): each
  /// reference in `borrow_map`, each retired one (#903) and each earlier
  /// arm's loan (#806), at the row the fact layer ends it (see
  /// `borrow_ends`), then placed into arms and ordered by the passes below.
  pub fn emit_borrow_ends(&mut self) {
    // An or-pattern reference's possible borrows (#734) are drawn by
    // their `OrAltBorrow` events, not by an end here.
    for (_, ev) in &self.or_ref_ends {
      if let ExternalEvent::OrAltBorrow { into, .. } = ev {
        self.borrow_map.remove(into.name());
      }
    }
    self.first_synthesized_end = *self.unique_id;
    self.emit_ends_from_facts(self.body_facts);
    self.place_borrow_ends(self.body_facts);
  }

  /// One end per (reference, lender) the walk left open, where the facts
  /// end it: an arrow back to the lender on the row the reference is the
  /// last holder on some path, else its own end (`RefDie`) where it stops
  /// holding a loan that goes on (a copy, a loop binding each time round:
  /// Q6, Q7). A loan that never ends in the body (returned, or stored where
  /// the caller can reach it) gets none. A reference the facts have no loan
  /// for keeps the walk's line.
  fn emit_ends_from_facts(&mut self, facts: Option<&BodyFacts>) {
    #[derive(Clone)]
    struct Slot {
      key: String,
      rd: RefData,
      rows: (usize, usize),
      arm: bool,
    }
    #[derive(Clone, Copy)]
    enum End {
      Arrow(usize),
      Own(usize),
      /// A loop's iterator gives the lender back on the loop's closing row;
      /// the binding stops holding at its last use each time round.
      LoopExit { own: usize, back: usize },
      None,
    }
    let ix = facts.map(|f| crate::borrow_ends::BodyEnds::new(self.tcx, f, self.hir_body));
    let mut slots: Vec<Slot> = Vec::new();
    // The loans an earlier slot of the same name took (per name).
    let mut claimed: HashMap<String, BTreeSet<usize>> = HashMap::new();
    let mut slot_loans: Vec<Vec<usize>> = Vec::new();
    // Earlier arms' loans first: each is the one taken where it was
    // assigned; then the retired variables', before the later variable of
    // their name is declared; then the current ones.
    let arm_loans = std::mem::take(&mut self.arm_loans);
    let retired = std::mem::take(&mut self.retired_loans);
    let mut later_from: HashMap<String, usize> = HashMap::new();
    for r in &retired {
      let e = later_from.entry(r.name.clone()).or_insert(0);
      *e = (*e).max(r.until);
    }
    for (k, rd) in arm_loans {
      let loans: Vec<usize> = ix.as_ref().map(|ix| ix.loans_held_by(&k, self.raps.get(k.as_str()).is_some_and(|r| r.rap.is_ref())).into_iter()
        .filter(|l| l.line == rd.assigned_at && !claimed.get(&k).is_some_and(|c| c.contains(&l.loan)) && ix.lends(l, &rd.lender.real_name()))
        .map(|l| l.loan).collect()).unwrap_or_default();
      claimed.entry(k.clone()).or_default().extend(loans.iter().copied());
      slot_loans.push(loans);
      slots.push(Slot { key: k, rd, rows: (0, usize::MAX), arm: true });
    }
    for r in retired {
      // (Its loans are those taken before the later variable is declared.)
      let loans: Vec<usize> = ix.as_ref().map(|ix| ix.loans_held_by(&r.name, self.raps.get(r.name.as_str()).is_some_and(|r| r.rap.is_ref())).into_iter()
        .filter(|l| !claimed.get(&r.name).is_some_and(|c| c.contains(&l.loan)) && l.line < r.until)
        // (Not one the later variable holds as its own.)
        .filter(|l| { let e = ix.holder_end(&r.name, &[l.loan], (r.until, usize::MAX)); e.gives_back.is_none() && e.stops.is_none() })
        .map(|l| l.loan).collect()).unwrap_or_default();
      claimed.entry(r.name.clone()).or_default().extend(loans.iter().copied());
      slot_loans.push(loans);
      slots.push(Slot { key: r.name.clone(), rd: r.data, rows: (0, r.until), arm: false });
    }
    #[expect(clippy::disallowed_methods, reason = "sorted below")]
    let mut current: Vec<(String, RefData)> = self.borrow_map.iter().map(|(k, d)| (k.clone(), d.clone())).collect();
    current.sort_by(|a, b| a.0.cmp(&b.0));
    for (k, rd) in current {
      let from = later_from.get(&k).copied().unwrap_or(0);
      let mut held: Vec<&crate::facts::schema::LoanInfo> = ix.as_ref().map(|ix| ix.loans_held_by(&k, self.raps.get(k.as_str()).is_some_and(|r| r.rap.is_ref())).into_iter()
        .filter(|l| !claimed.get(&k).is_some_and(|c| c.contains(&l.loan))).collect()).unwrap_or_default();
      // A pattern's binding moved out of a value holding the borrow (`if
      // let Some(x) = y`) holds what y held.
      if held.is_empty() {
        held = ix.as_ref().map(|ix| ix.loans_reaching(&k).into_iter()
          .filter(|l| !claimed.get(&k).is_some_and(|c| c.contains(&l.loan))).collect()).unwrap_or_default();
      }
      // A loan taken into k before k got its current value (`r = &s;` in
      // an arm after `let mut r = &s;`) is the walk's to end, unless the
      // new value still holds it (`m = if c { &mut t } else { m };`).
      if held.iter().any(|l| l.line >= rd.assigned_at) {
        let renewed = ix.as_ref().map(|ix| ix.renewed_at(&k, rd.assigned_at)).unwrap_or_default();
        held.retain(|l| l.line >= rd.assigned_at || l.direct_holders.first() != Some(&k) || renewed.contains(&l.loan));
      }
      let ended = self.drawn_references.ended_before_install(rd.borrow_span, self.mir_body, &self.reference_effects);
      let loans: Vec<usize> = held.into_iter().map(|l| l.loan).filter(|id| !ended.contains(id)).collect();
      slot_loans.push(loans);
      slots.push(Slot { key: k, rd, rows: (from, usize::MAX), arm: false });
    }

    let slot_keys: Vec<String> = slots.iter().map(|s| s.key.clone()).collect();
    // Per slot and lender: the end, from the facts.
    let lends_to = |rd: &RefData| -> Vec<ResourceTy> {
      let mut v = vec![rd.lender.clone()];
      v.extend(rd.co_lenders.iter().filter(|c| matches!(c, ResourceTy::Value(_) | ResourceTy::Deref(_) | ResourceTy::DerefField(..))).cloned());
      v
    };
    // First where each slot's own facts put it: an arrow where it is the
    // last holder on some path (or never stops holding before the loan
    // ends), its own end where it only stops holding a loan that goes on.
    #[derive(Clone)]
    struct Pending {
      lender: ResourceTy,
      end: End,
      loans: Vec<usize>,
      /// Its own end, if another slot gives the loans back.
      own: Option<(usize, usize)>,
      /// A value holding the borrow (`o: Option<&mut T>`) that also stops
      /// holding it while the loan goes on: the last row it does.
      holder_stops: Option<usize>,
      /// The walk's line (no fact loan of this lender): an arrow unless
      /// another slot gives its loans back later.
      walk: bool,
    }
    // The loans a pattern's binding gives back (`if let Some(e) = o`).
    let mut pattern_gives: BTreeSet<usize> = BTreeSet::new();
    let mut pending: Vec<Vec<Pending>> = Vec::new();
    // The loans an arrow gives back, with the lender each arrow goes to.
    // (lender, row, giver)
    let mut given_back: BTreeMap<usize, Vec<(String, usize, String)>> = BTreeMap::new();
    for (slot, loans) in slots.iter().zip(slot_loans.iter()) {
      let lenders = lends_to(&slot.rd);
      let lender_names: Vec<String> = lenders.iter().map(|l| l.real_name()).collect();
      let mut ends = Vec::new();
      for (i, l) in lenders.iter().enumerate() {
        let Some(ix) = ix.as_ref() else {
          ends.push(Pending { lender: l.clone(), end: End::Arrow(slot.rd.lifetime), loans: Vec::new(), own: None, holder_stops: None, walk: false });
          continue;
        };
        // With several lenders, each gives back the loans of its own;
        // the first takes what none of the others lends.
        // (A reference's earlier loan of another lender, `c = &y` after
        // `c = &x`, is the walk's to end.)
        let info = |id: &usize| ix.facts.loans.iter().find(|x| x.loan == *id);
        let mut mine: Vec<usize> = ix.with_held_sources(&slot.key, loans, &lender_names).into_iter()
          .filter(|id| info(id).is_some_and(|f| ix.lends(f, &l.real_name())))
          .collect();
        // (A temporary k borrows, `im = &mut w.iter_mut()`, borrows l in
        // turn: k holds that loan too.)
        if mine.iter().any(|id| info(id).is_some_and(|f| f.source == "temporary")) {
          let more: Vec<usize> = ix.loans_reaching(&slot.key).into_iter()
            .filter(|f| f.source != "temporary" && !mine.contains(&f.loan) && !claimed.get(&slot.key).is_some_and(|c| c.contains(&f.loan)) && ix.lends(f, &l.real_name()))
            .map(|f| f.loan).collect();
          mine.extend(more);
        }
        // (Or that k holds through what it holds: `v = if c { let r = &mut
        // a; vec![r] } else { .. }` keeps r's loan of a in v.)
        if mine.is_empty() && !loans.is_empty() {
          mine = ix.loans_reaching(&slot.key).into_iter()
            .filter(|f| !claimed.get(&slot.key).is_some_and(|c| c.contains(&f.loan)) && ix.lends(f, &l.real_name()))
            .map(|f| f.loan).collect();
        }
        // (Not for a lender with a column of its own the facts give k no
        // loan of, `r2` after `std::mem::swap(&mut r1, &mut r2)`: the
        // walk's line.)
        let own_column = matches!(l, ResourceTy::Value(x) if self.raps.get(x.name().as_str()).is_some_and(|r| !r.rap.is_fn()));
        if i == 0 && mine.is_empty() && own_column && !loans.is_empty() {
          // (Its arrow gives back k's loans that went through the lender, so
          // copies made before it end on their own, `let m = s; let o =
          // Some(m);` with x out of o giving back to m; and k is a copy
          // that ends on its own if another slot gives them back later.
          // Not `r8`'s old loan of h after `mem::swap(&mut r7, &mut r8)`.)
          let through_l: Vec<usize> = loans.iter().copied()
            .filter(|id| info(id).is_some_and(|f| f.holders.iter().any(|h| crate::borrow_ends::names_match(&l.real_name(), h))))
            .collect();
          for id in &through_l {
            given_back.entry(*id).or_default().push((l.real_name(), slot.rd.lifetime, slot.key.clone()));
          }
          let walk = !through_l.is_empty();
          // (Not before the lender's loan ends: after `mem::swap(&mut r7, &mut
          // r8)` r8 points to g, but r7 keeps g borrowed to its last use,
          // one lifetime for both; rustc's end row.)
          let lender_loans: Vec<usize> = ix.facts.loans.iter()
            .filter(|f| f.source == "borrow" && ix.lends(f, &l.real_name()) && f.line <= slot.rd.lifetime)
            .map(|f| f.loan).collect();
          let at = if walk { slot.rd.lifetime } else {
            ix.last_end_of(&lender_loans).map_or(slot.rd.lifetime, |e| e.max(slot.rd.lifetime))
          };
          ends.push(Pending { lender: l.clone(), end: End::Arrow(at), loans: through_l, own: walk.then_some((at, at)), holder_stops: None, walk });
          continue;
        }
        if i == 0 && mine.is_empty() {
          mine = loans.iter().copied()
            .filter(|id| info(id).is_some_and(|f| !lenders.iter().any(|o| ix.lends(f, &o.real_name()))))
            .collect();
        }
        if mine.is_empty() {
          // No loan the facts know of: the walk's line. (Also for a
          // reference parameter k took a copy of, `vr.push(r)`: the facts
          // track no loan of a caller's borrow into k.)
          let caller_ref = own_column && {
            let mut own = ix.facts.loans.iter().filter(|f| f.direct_holders.iter().any(|h| *h == l.real_name())).peekable();
            own.peek().is_some() && own.all(|f| f.source == "caller")
          };
          let end = if (loans.is_empty() && i == 0) || (i > 0 && caller_ref) { End::Arrow(slot.rd.lifetime) } else { End::None };
          ends.push(Pending { lender: l.clone(), end, loans: Vec::new(), own: None, holder_stops: None, walk: false });
          continue;
        }
        // A let-chain's (or guard's) new loan that doesn't outlive the
        // conditional: only the path into its body (#603).
        // (Its own conditional: the one whose condition took the loan,
        // not an earlier sibling's that re-pointed a variable of the name.)
        let then = (!slot.arm).then(|| self.chain_then.iter()
          .find(|(n, t, whole)| *n == slot.key && ix.last_end_of(&mine).is_some_and(|e| e <= *whole)
            && mine.iter().any(|id| info(id).is_some_and(|f| f.line >= t.0 && f.line <= t.1)))
          .map(|(_, t, _)| *t)).flatten();
        let mut e = match then {
          Some(t) => ix.holder_end_within(&slot.key, &mine, slot.rows, t),
          None => ix.holder_end(&slot.key, &mine, slot.rows),
        };
        // (A local's loan k still holds past every end, on a path that
        // diverges, `if c { panic!("{}", r1); }`: the borrow ends there too,
        // before the lender is dropped while unwinding.)
        if mine.iter().all(|id| info(id).is_some_and(|f| f.source == "borrow" && !f.columns.is_empty())) {
          if let (Some(back), Some(held)) = (e.last_end, e.last_held) {
            if held > back {
              e.last_end = Some(held);
            }
          }
        }
        // A reference to a string literal or a `static` (its loan never ends;
        // a copy, `longest(.., string2)`, may hold it on): its own end at
        // its last use (Q7).
        let all_static = mine.iter().all(|id| info(id).is_some_and(|f| f.source == "static"));
        let (end, own) = match (e.last_end, e.gives_back, e.stops) {
          // A reference copied out of a collection can point to static data.
          // The collection can hold it for longer than this binding is live.
          _ if all_static && matches!(l, ResourceTy::Anonymous)
            && self.raps.get(&slot.key).is_some_and(|r| r.rap.is_ref())
            && mine.iter().all(|id| info(id).is_some_and(|f| !f.direct_holders.contains(&slot.key))) => {
            let last = slot.rd.local.and_then(|l| self.local_last_live_line(l, (slot.rd.assigned_at, slot.rows.1)));
            (End::Arrow(last.unwrap_or(slot.rd.assigned_at)), None)
          }
          (_, _, Some(_)) if all_static => (End::Own(e.last_held.unwrap_or(slot.rd.lifetime)), None),
          // (A local's loan with no end on any path: the path that still
          // holds it diverges, `panic!("{}", r)`; the lender is dropped
          // while unwinding, after the borrow ends where r is last used.)
          // (Not if a copy holds it on from there, `let q = r;` then
          // `never(q)`: r's own end.)
          (None, _, Some(own)) if mine.iter().all(|id| info(id).is_some_and(|f| f.source == "borrow" && !f.columns.is_empty()))
            && !ix.others_hold_after(&slot.key, &mine, own) => (End::Arrow(own), None),
          // (A copy of a loan that never ends here, returned or stored for
          // the caller, `largest = item;`: its own end where it last holds
          // it (Q7); a loop's binding where the loop ends, as 2.x draws it
          // until Q6's per-iteration ends are drawn, #1864.)
          // (Not the parameter itself, `list` returned through `largest`:
          // its borrow doesn't end in this body.)
          (None, _, Some(_)) if !mine.iter().any(|id| info(id).is_some_and(|f| f.source == "caller" && f.direct_holders.first() == Some(&slot.key))) => {
            let at = e.last_held.unwrap_or(slot.rd.lifetime);
            let at = ix.loops.iter().filter(|(h, c)| *h <= slot.rd.assigned_at && slot.rd.assigned_at <= *c && at <= *c)
              .map(|(_, c)| *c).min().unwrap_or(at);
            (End::Own(at), None)
          }
          (None, _, _) => (End::None, None),
          (Some(back), Some(_), Some(own)) if e.only_loop_exits && own < back => (End::LoopExit { own, back }, None),
          // (A field moved away with its part before then, `o.inner =
          // o2.inner;`: what it moved into gives the loan back; its own
          // end is at the move.)
          (Some(back), Some(_), _) if ix.moved_between(&slot.key, slot.rd.assigned_at, back).is_some() =>
            (End::Own(ix.moved_between(&slot.key, slot.rd.assigned_at, back).unwrap_or(back)), None),
          // (Out of scope by the loan's last row, `let m2 = m;` in an arm
          // that returns while the loan goes on on the other path: it
          // gives the lender back where it last holds it, on its own path.)
          (Some(back), Some(g), _) if g < back && g >= slot.rd.assigned_at && !ix.in_scope_after(&slot.key, back) => (End::Arrow(g), None),
          // (Moved away on one path before the loan's last row, `let m2 = m;`
          // in an arm of `m: &mut String`: on the paths that don't move it,
          // where it gives the caller's loan back.)
          // (A parameter's caller loan only: a local's end there sits on the
          // conditional's own row, through the fork, 1128 / 1282.)
          (Some(back), Some(g), _) if g < back && matches!(l, ResourceTy::Caller) && ix.moved_in(&slot.key, g, back) => (End::Arrow(g), None),
          (Some(back), Some(_), _) => (End::Arrow(back), None),
          // (Moved away where it stops holding, `Some(q)`: its column ends
          // there, so its end is there.)
          (Some(back), None, Some(own)) if ix.moved_with_parent_on(&slot.key, own) => (End::Arrow(own), Some((own, back))),
          (Some(back), None, Some(own)) => (End::Arrow(back), Some((own, back))),
          // Neither named where the loan ends nor seen to stop (on the row
          // the loan ends, or a name two variables share): its own end on
          // the walk's line if another slot gives the loan back.
          // (Its value still holding them into a later row, `e` of `let e =
          // (Some(r), 3);` read with the loan's last holder: there.)
          (Some(back), None, None) => {
            let own = ix.held_into(&slot.key, &mine).map_or(slot.rd.lifetime, |h| h.max(slot.rd.lifetime));
            (End::Arrow(back), Some((own.min(back), back)))
          }
        };
        // The lender stays lent after the end if another loan of it goes on
        // (2.x's "ultimate" reference): then this end gives nothing back.
        let column = match l {
          ResourceTy::Value(x) | ResourceTy::Deref(x) | ResourceTy::Index(x, _, _) | ResourceTy::DerefField(x, _, _, _) => Some(x.name().to_owned()),
          _ => None,
        };
        // (A loop binding with no lender to give back to, `for y in
        // v.iter()`, ends where it last holds the loan; the loop's iterator
        // holds it on to where the loop ends, and what it was copied into
        // to their last use.)
        let end = match (end, e.last_held) {
          (End::Arrow(back), Some(o)) if matches!(l, ResourceTy::Anonymous) && o < back
            && ix.loops.iter().any(|(h, c)| *h < o && o <= *c && *c <= back) => End::Own(o),
          (e, _) => e,
        };
        let still_lent = |row: usize| column.as_ref().is_some_and(|c| ix.lent_after(row, c, &mine, &slot_keys));
        let (end, own) = match end {
          End::Arrow(back) if still_lent(back) => (End::Own(own.map_or(back, |(o, _)| o)), None),
          End::LoopExit { own, back } if still_lent(back) => (End::Own(own), None),
          e => (e, own),
        };
        if let (None, End::Arrow(row) | End::LoopExit { back: row, .. }) = (own, end) {
          for id in mine.iter().copied().chain(ix.sources_lending(&mine, &l.real_name())) {
            given_back.entry(id).or_default().push((l.real_name(), row, slot.key.clone()));
          }
        }
        // (A binding of the payload itself, `Some(e)`: the scrutinee’s ending edge
        // ends the holder's loan in the other arms. Not a field of a bound
        // struct, `Some(x)` then `x.part`: the holder's own end does.)
        if self.pattern_copies.contains(&slot.key) && !slot.key.contains('.') && e.gives_back.is_some() {
          pattern_gives.extend(mine.iter().copied());
        }
        // (Only for a lender with a column of its own, as that pass draws.)
        // (And only a call's result: `o = refs.iter_mut().next()`, the
        // borrow that pass finds; not `Some(if c { &mut a } else { .. })`.)
        // (Not the scrutinee itself, `if let Some(e) = o`: on the paths
        // that bind nothing o is the last holder, and gives the lender back.)
        let holder_stops = e.stops.filter(|_| matches!(l, ResourceTy::Value(_)) && !ix.part_taken_from(&slot.key)
          && self.raps.get(&slot.key).is_some_and(|r| !r.rap.is_ref() && r.rap.holds_borrow())
          && mine.iter().all(|id| ix.facts.loans.iter().any(|f| f.loan == *id
            && f.via_call.iter().any(|h| crate::borrow_ends::names_match(&slot.key, h))
            && !f.direct_holders.iter().any(|h| crate::borrow_ends::names_match(&slot.key, h)))));
        ends.push(Pending { lender: l.clone(), end, loans: mine, own, holder_stops, walk: false });
      }
      // Several lenders in the source order of their borrows (`t = if c {
      // (&mut a, 1) } else { (&mut b, 2) }`: "a or b"), not the order the
      // walk last set them in.
      if let Some(ix) = ix.as_ref() {
        let first = |p: &Pending| p.loans.iter().filter_map(|id| ix.facts.loans.iter().find(|f| f.loan == *id)).map(|f| (f.line, f.col)).min();
        if ends.len() > 1 && ends.iter().all(|p| first(p).is_some()) {
          ends.sort_by_key(|p| first(p));
        }
      }
      pending.push(ends);
    }
    // A slot that only stops holding gets its own end when another slot
    // gives the loan back; else it gives it back itself, where the loan
    // ends (its holder from then on has no column: `w.push((r, ..))`).
    // The walk's own ends (a re-pointed reference's old loan, ...).
    let mut walk_ends = Vec::new();
    borrow_ends_in(self.preprocessed_events, &mut walk_ends);
    walk_ends.extend(self.late_old_ends.iter().cloned());
    // A loan the walk gives back where the facts end it, from the holder
    // they name (`c.r` re-pointed while the copy `c2.r` held its old loan).
    let walk_gives = |id: usize, k: &str, from: usize| ix.as_ref().is_some_and(|ix| ix.end_holders(id).iter().filter(|(row, _)| *row >= from).any(|(row, h)|
      walk_ends.iter().any(|(l, e)| *l == *row && !matches!(e, ExternalEvent::RefDie { .. })
        && borrow_end_of(e).is_some_and(|(from, to)| from.name() != k && crate::borrow_ends::names_match(from.name(), h)
          // (Back to the loan's lender: a closure's end gives `*r` back to
          // r, not r's loan of s.)
          && to.extract_rap().is_some_and(|t| ix.facts.loans.iter().any(|f| f.loan == id && ix.lends(f, t.name())))))));
    // Or another slot gives it back where the facts end it, from the holder
    // they name (`c.r`'s arrow when `c` holds the loan c2.r copied).
    let slot_arrows: Vec<(String, String)> = slots.iter().zip(pending.iter())
      .flat_map(|(s, ends)| ends.iter().filter(|p| (p.own.is_none() || p.walk) && matches!(p.end, End::Arrow(_) | End::LoopExit { .. }))
        .map(|p| (s.key.clone(), p.lender.real_name())))
      .collect();
    let other_gives = |id: usize, k: &str, from: usize| ix.as_ref().is_some_and(|ix| ix.end_holders(id).iter().filter(|(row, _)| *row >= from).any(|(_, h)|
      // (Not an arrow back to k itself: k still gives its lender back.)
      slot_arrows.iter().any(|(key, ln)| key != k && ln != k && crate::borrow_ends::names_match(key, h)
        && ix.facts.loans.iter().any(|f| f.loan == id && ix.lends(f, ln)))));
    // The row another slot gives back a loan it holds through a reborrow
    // of one of `loans` (`v = vec![r]` holds `&mut *r`, made through r's
    // loan of a): rustc counts r's loan as used until then (an `a.clear()`
    // before it is E0499), so r's borrow doesn't end before it.
    let reborrowed_until = |loans: &[usize], k: &str, own: usize| -> Option<usize> {
      let ix = ix.as_ref()?;
      // (A reborrow through k itself, `&mut *r`, that borrowck tracks and
      // that something keeps or that outlives k's own last use: not a
      // copy of k, which holds k's loan as it is (Q7), nor a call's
      // passing reborrow, `e.push('x')`.)
      let reborrowed = |l: &usize| ix.facts.loans.iter().any(|x| x.made_through.contains(l) && x.source == "borrow"
        && x.through.as_deref().is_some_and(|t| crate::borrow_ends::root(t) == k)
        && (!x.direct_holders.is_empty() || !x.holders.is_empty() || !x.via_call.is_empty()
          || x.end_lines.iter().max().is_some_and(|e| *e > own)));
      // (Where the facts end that loan, held by someone else.)
      loans.iter().filter(|l| reborrowed(l)).filter_map(|l| ix.end_holders(*l).into_iter()
        .filter(|(_, h)| !crate::borrow_ends::names_match(k, h))
        .map(|(row, _)| row).max()).max()
    };
    let mut decided: Vec<Vec<(ResourceTy, End, Vec<usize>)>> = pending.into_iter().zip(slots.iter()).map(|(ends, slot)| ends.into_iter().map(|p| {
      let end = match p.own {
        // A value whose loan a pattern's binding took over (#1447): the
        // binding gives it back on its arm, the scrutinee's temporary on
        // the others; the value's
        // own end is its last use (`it.next()`), with no arrow.
        _ if p.holder_stops.is_some() && p.loans.iter().any(|l| pattern_gives.contains(l)) => End::Own(p.holder_stops.unwrap_or_default()),
        // (Not by an arrow back to this reference itself, `k = [s]` giving
        // back to s: s still gives its own lender back.)
        // (One on an earlier row gives it back on another path, `let m2 =
        // m;` in an arm that returns: not the end of this one's.)
        Some((own, back)) if p.loans.iter().any(|l| given_back.get(l).is_some_and(|to| to.iter().any(|(t, row, giver)| *t != slot.key && *giver != slot.key && *row >= back)) || walk_gives(*l, &slot.key, back) || other_gives(*l, &slot.key, back)) => End::Own(own),
        _ => p.end,
      };
      // (A `&mut` parameter moved away on some path, `let y = z;` in an
      // arm: its end on the other paths draws no arrow either way, and as
      // its own end the join says it was moved in a branch.)
      let end = match end {
        End::Arrow(row) if matches!(p.lender, ResourceTy::Caller) && ix.as_ref().is_some_and(|ix| ix.moved_anywhere(&slot.key)) => End::Own(row),
        e => e,
      };
      // (Its own end on the row it is moved on, `Some(r)`: the move hands
      // the loan on and ends its column; nothing more to draw.)
      let end = match end {
        End::Own(row) if ix.as_ref().is_some_and(|ix| ix.moved_on(&slot.key, row)) => End::None,
        e => e,
      };
      // (Reborrowed by a holder that gives the lender back later: its own
      // end there, not at its last use; none if it is out of scope by
      // then, `{ let r = &mut a; vec![r] }`: it goes out of scope with a
      // still lent to the holder.)
      let end = match end {
        End::Own(row) => match reborrowed_until(&p.loans, &slot.key, row) {
          Some(until) if until > row =>
            if ix.as_ref().is_some_and(|ix| !ix.in_scope_after(&slot.key, until)) { End::None } else { End::Own(until) },
          _ => End::Own(row),
        },
        e => e,
      };
      (p.lender, end, p.loans)
    }).collect()).collect();
    // A reference something reborrowed from it gives a loan back to (an
    // arrow to it, `k = [s]`, or the walk's end of a re-pointed holder's
    // old loan, `h3.r = r3;` after `h3 = holder(&*m3)`) is still lent until
    // then: its own end comes after that, not at its own last use (#1538).
    // (Also an anonymous holder's give-back, the `for` loop's iterator
    // giving back `text` it borrowed through a copy of it.)
    let mut anon_backs = Vec::new();
    anonymous_gives_in(self.preprocessed_events, &mut anon_backs);
    let back_to = |name: &str, decided: &[Vec<(ResourceTy, End, Vec<usize>)>]| -> Option<usize> {
      let walk = walk_ends.iter().filter(|(_, e)| !matches!(e, ExternalEvent::RefDie { .. }))
        .filter_map(|(l, e)| borrow_end_of(e).and_then(|(_, to)| to.extract_rap()).filter(|t| t.name() == name).map(|_| *l))
        .chain(anon_backs.iter().filter(|(_, to)| to == name).map(|(l, _)| *l));
      let emitted = slots.iter().zip(decided.iter()).filter(|(s, _)| s.key != name)
        .flat_map(|(_, ends)| ends.iter()).filter(|(l, _, _)| l.real_name() == name)
        .filter_map(|(_, e, _)| match e { End::Arrow(l) | End::LoopExit { back: l, .. } => Some(*l), _ => None });
      walk.chain(emitted).max()
    };
    // (To a fixed point: a reference that gives back to one whose end
    // this moves later moves on with it, whatever order they're in.)
    for _ in 0..=slots.len() {
      let mut moved = false;
      for i in 0..slots.len() {
        if slots[i].arm || !self.raps.get(&slots[i].key).is_some_and(|r| r.rap.is_ref()) {
          continue;
        }
        let Some(after) = back_to(&slots[i].key, &decided) else { continue };
        for (_, e, _) in decided[i].iter_mut() {
          match e {
            End::Own(l) | End::Arrow(l) if *l < after => {
              *l = after;
              moved = true;
            }
            _ => {}
          }
        }
      }
      if !moved {
        break;
      }
    }
    // The retired ones' for `note_sharers_across_calls`.
    for (slot, ends) in slots.iter().zip(decided.iter()) {
      if slot.arm || slot.rows.1 == usize::MAX {
        continue;
      }
      let mut rd = slot.rd.clone();
      if let Some(l) = ends.iter().find_map(|(_, e, _)| match e {
        End::Arrow(l) | End::Own(l) | End::LoopExit { own: l, .. } => Some(*l),
        End::None => None,
      }) {
        rd.lifetime = l;
      }
      self.retired_sharers.push((slot.key.clone(), rd));
    }
    // Later passes read a reference's end line from `borrow_map`.
    for (slot, ends) in slots.iter().zip(decided.iter()) {
      if slot.arm {
        continue;
      }
      let line = ends.iter().find_map(|(_, e, _)| match e {
        End::Arrow(l) | End::Own(l) | End::LoopExit { own: l, .. } => Some(*l),
        End::None => None,
      });
      if let (Some(line), Some(d)) = (line, self.borrow_map.get_mut(&slot.key)) {
        if slot.rows.1 == usize::MAX {
          d.lifetime = line;
        }
      }
    }

    #[expect(clippy::disallowed_methods, reason = "collected into a set, order never observed")]
    let handed_off: HashSet<String> = self.borrow_map.keys().filter(|k| self.ref_handed_off(k)).cloned().collect();
    let lender_to_refs = get_non_anon_lenders(&self.borrow_map);
    // The cascade: more aliases first, then deeper reborrow chains (#1415),
    // then where the borrow was made, declaration order and name (#211).
    let b_map = self.borrow_map.clone();
    let depth = |k: &str| {
      let mut n = 0;
      let mut cur = b_map.get(k);
      let mut seen = vec![k.to_owned()];
      while let Some(rd) = cur {
        let next = match &rd.lender {
          ResourceTy::Value(x) | ResourceTy::Deref(x) | ResourceTy::Index(x, _, _) => x.name().to_owned(),
          _ => break,
        };
        if seen.contains(&next) || !b_map.get(&next).is_some_and(|d| !matches!(d.lender, ResourceTy::Caller | ResourceTy::Anonymous)) {
          break;
        }
        n += 1;
        cur = b_map.get(&next);
        seen.push(next);
      }
      n
    };
    let hash_of = |k: &str| self.raps.get(k).map(|r| *r.rap.hash());
    let mut order: Vec<usize> = (0..slots.len()).collect();
    order.sort_by(|&a, &b| {
      let (sa, sb) = (&slots[a], &slots[b]);
      sa.arm.cmp(&sb.arm)
        .then_with(|| sb.rd.aliasing.len().cmp(&sa.rd.aliasing.len()))
        .then_with(|| depth(&sb.key).cmp(&depth(&sa.key)))
        .then_with(|| sa.rd.assigned_at.cmp(&sb.rd.assigned_at))
        .then_with(|| hash_of(&sa.key).cmp(&hash_of(&sb.key)))
        .then_with(|| sa.key.cmp(&sb.key))
    });

    let exhaustion_edges = ix.as_ref().map(|ix| ix.exhaustion_edges(self.tcx, self.mir_body)).unwrap_or_default();
    for i in order {
      let slot = &slots[i];
      let k = &slot.key;
      if !slot.arm && slot.rows.1 == usize::MAX && handed_off.contains(k) {
        continue;
      }
      let Some(current) = self.raps.get(k) else { continue };
      let from_ro = match slot.rd.local {
        Some(local) if current.rap.is_ref() => {
          let Some(binding) = self.raps.place(local, k) else { continue };
          &binding.rap
        }
        _ => &current.rap,
      }.clone();
      let rd = slot.rd.clone();
      let mut ends = Vec::new();
      for (lender, end, loans) in std::mem::take(&mut decided[i]) {
        let separate = from_ro.holds_borrow() && matches!(end, End::Arrow(_)) && loans.len() > 1
          && self.row_layout.stored_lender_loans.get(lender.hash()).is_some_and(|ids| loans.iter().all(|id|
            ids.contains(id) && facts.and_then(|f| f.loans.iter().find(|loan| loan.loan == *id))
              .is_some_and(|loan| loan.projected.is_some())));
        if separate {
          // A lender holding a mutable and a shared loan gets the mutable
          // one back first: it was revoked by that loan.
          let mut loans = loans;
          loans.sort_by_key(|id| !facts.and_then(|f| f.loans.iter().find(|loan| loan.loan == *id))
            .is_some_and(|loan| matches!(loan.kind, "mut" | "unique")));
          ends.extend(loans.into_iter().map(|id| (lender.clone(), end, vec![id])));
        } else {
          ends.push((lender, end, loans));
        }
      }
      for (lender, end, loans) in ends {
        let to_ro = match &lender {
          ResourceTy::Anonymous => ResourceTy::Deref(from_ro.clone()),
          l => l.clone(),
        };
        let co = lender != rd.lender;
        // A reference no one lent to (#1095) gives nothing back.
        let unlent = matches!(rd.lender, ResourceTy::Anonymous) && self.unlent_refs.contains(k);
        // Q6 (the binding's own end each time round, the iterator's arrow
        // where the loop ends) needs the renderer to keep the binding's
        // column after its end: until then the binding gives the lender
        // back where the loop ends, as 2.x draws it.
        // (A binding with no lender of its own to give back to, `for (a, b)
        // in self.items.iter().zip(..)`, draws no arrow: its own end at its
        // last use each time round, as rustc has it.)
        let end = match end {
          End::LoopExit { own, .. } if matches!(lender, ResourceTy::Anonymous) => End::Own(own),
          End::LoopExit { back, .. } => End::Arrow(back),
          e => e,
        };
        // A `for` binding its body moves away (#1511) ends its borrow at
        // the move; the loop's iterator holds the loan on (see
        // `note_loop_iterator_keeps_moved`). (This binding's loop: a
        // later loop's binding of the same name isn't moved by an earlier
        // loop's body.)
        let end = match (end, self.for_loop_moved_bindings.iter().find(|(n, mv, close)| n == k && *mv >= rd.assigned_at && rd.assigned_at <= *close && *mv <= *close)) {
          (End::Arrow(_) | End::Own(_), Some((_, mv, _))) => End::Arrow(*mv),
          (e, _) => e,
        };
        // An end past the reference's scope end (a copy of it outlives it,
        // #913): its own end there, before the scope end.
        let scope_end = (from_ro.is_ref() && self.raps.get(k).is_some_and(|r| r.extra_scopes.is_empty()))
          .then(|| last_gos_line_from(self.preprocessed_events, &from_ro, rd.assigned_at, slot.rows.1)).flatten();
        let end = match end {
          End::Arrow(l) | End::Own(l) if scope_end.is_some_and(|s| s < l) => End::Own(scope_end.unwrap_or(l)),
          End::Arrow(l) if unlent => End::Own(l),
          e => e,
        };
        let first_id = *self.unique_id;
        match end {
          End::None => {}
          End::Arrow(line) => {
            // A holder moved into a `for` loop has no value left to end a
            // borrow on: the loop gives the lender back (#767). A `&mut` a
            // `move` closure captured is held by the closure (#1148).
            let end_from = if self.for_moved_holders.contains(k) { ResourceTy::Anonymous }
              else if let Some(c) = self.closure_moved_into(&from_ro) { ResourceTy::Value(c) }
              else { ResourceTy::Value(from_ro.clone()) };
            let id = *self.unique_id;
            let projected_kind = match loans.as_slice() {
              [id] if self.row_layout.stored_lender_loans.get(lender.hash()).is_some_and(|ids| ids.contains(id)) =>
                facts.and_then(|f| f.loans.iter().find(|l| l.loan == *id))
                  .filter(|loan| loan.projected.is_some()).map(|loan| matches!(loan.kind, "mut" | "unique")),
              _ => None,
            };
            if let Some(mutable) = projected_kind {
              self.add_ev(line, if mutable { Evt::MDie } else { Evt::SDie }, lender.clone(), end_from, false);
            } else if co && !self.stored_ref_var_lends.contains(&(k.clone(), lender.real_name())) {
              let evt = if rd.mut_of(&lender) { Evt::MDie } else { Evt::SDie };
              self.add_ev(line, evt, lender.clone(), end_from, false);
            } else {
              self.add_borrow_end_ev(line, to_ro.clone(), end_from, &rd, Vec::new());
            }
            if let Some((_, ExternalEvent::StaticDie { exhaustion, .. } | ExternalEvent::MutableDie { exhaustion, .. })) =
              self.preprocessed_events.last_mut().filter(|(_, event)| event.get_id() == id) {
              // Keep the edge when arm placement copies this end to a
              // different path and assigns the copy a new event ID.
              for edge in loans.iter().filter_map(|loan| exhaustion_edges.get(&(*loan, k.as_str()))).flatten() {
                if !exhaustion.contains(edge) {
                  exhaustion.push(edge.clone());
                }
              }
            }
          }
          End::LoopExit { own, back } => {
            self.add_own_end(own, &from_ro, to_ro.clone(), k, &[]);
            let evt = if rd.mut_of(&lender) { Evt::MDie } else { Evt::SDie };
            self.add_ev(back, evt, to_ro, ResourceTy::Anonymous, false);
          }
          End::Own(_) if co => {}
          End::Own(line) => 'own: {
            // A copy of a reference to a string literal or a `static`
            // (whose lender has no column, so no end draws an arrow) says
            // what it borrowed at its own end too.
            if matches!(rd.lender, ResourceTy::Anonymous) && rd.static_lender.is_some() {
              self.add_borrow_end_ev(line, to_ro, ResourceTy::Value(from_ro.clone()), &rd, Vec::new());
              break 'own;
            }
            // A reference a pattern copied out of a value that holds the
            // borrow (`while let Some(n) = it.next()`): its end says the
            // value keeps the lender borrowed (#575).
            let holders = if !slot.arm {
              let mut at = rd.clone();
              at.lifetime = line;
              self.holders_still_borrowing(k, &at, &lender_to_refs, &handed_off)
            } else {
              Vec::new()
            };
            if !holders.is_empty() {
              self.add_borrow_end_ev(line, to_ro, ResourceTy::Value(from_ro.clone()), &rd, holders);
              break 'own;
            }
            // A value holding a borrow moved into a call on that line: the
            // column ends at the move (#1448).
            if !from_ro.is_ref() && from_ro.holds_borrow() && self.moved_into_call_on(&from_ro, line) {
              break 'own;
            }
            self.add_own_end(line, &from_ro, to_ro, k, &loans);
          }
        }
        let projected = match (end, loans.as_slice()) {
          (End::Arrow(_), [id]) => facts.and_then(|facts| facts.loans.iter().find(|loan| loan.loan == *id))
            .and_then(|loan| Some((loan.loan, loan.projected.as_ref()?))),
          _ => None,
        };
        let conditional = self.conditional_aggregate_ends.iter().any(|(holder, source, loan)|
          holder.hash() == from_ro.hash() && *source == lender && loans.contains(loan))
          || projected.is_some_and(|(loan, _)| self.row_layout.uncertain_projected_stores.contains(&(*from_ro.hash(), loan)));
        if from_ro.holds_borrow() && matches!(end, End::Arrow(_)) && conditional {
          use crate::svg_generator::hover_messages as hover;
          let place = projected.map_or_else(|| hover::styled_name_html(&lender.name()), |(_, p)| hover::projected_place_html(p));
          for id in first_id..*self.unique_id {
            self.row_layout.aggregate_titles.insert(id, hover::conditional_aggregate_end(k, &place));
          }
        }
        if !loans.is_empty() {
          for id in first_id..*self.unique_id {
            if loans.iter().all(|id| self.stored_unique_captures.contains(&(*from_ro.hash(), *id))) {
              self.row_layout.stored_loans.insert(id, crate::event_order::StoredLoan::UniqueCapture);
            }
            if let Some((loan, place)) = projected {
              self.row_layout.projected_loan_places.insert(id, (loan, place.clone()));
              if self.row_layout.shared_projected_stores.contains(&(*from_ro.hash(), loan)) {
                self.row_layout.stored_loans.insert(id, crate::event_order::StoredLoan::SharedInput { mutable: true, binding: false });
              }
            }
            self.drawn_references.bind_end(id, &loans, rd.borrow_span, *from_ro.hash(), from_ro.ref_local(), matches!(end, End::Arrow(_)), &self.reference_effects);
            self.end_loans.insert(id, loans.clone());
          }
        }
      }
    }
  }

  /// `r`'s own end on `line` (a `RefDie`: no arrow), before its scope end
  /// if that is on the same line (#913).
  fn add_own_end(&mut self, line: usize, r: &ResourceAccessPoint, to: ResourceTy, key: &str, loans: &[usize]) {
    let id = *self.unique_id;
    let others = if self.borrow_map.contains_key(key) { get_borrowers(&key.to_owned(), &self.borrow_map).len().saturating_sub(1) } else { 0 };
    let end = ExternalEvent::RefDie { slot: id, from: ResourceTy::Value(r.clone()), to, num_curr_borrowers: others, id };
    *self.unique_id += 1;
    if r.is_member() && !loans.is_empty() && !self.event_touches_skipped(&end)
      && place_skipped_hold(self.preprocessed_events, r, loans, &end) { return; }
    self.add_external_event(line, end);
  }

  /// The pass pipeline that places the ends into arms and orders them on
  /// their lines (P2-1c / P2-1d replace it).
  fn place_borrow_ends(&mut self, facts: Option<&BodyFacts>) {
    for (line, ev) in std::mem::take(&mut self.late_old_ends) {
      self.add_external_event(line, ev);
    }
    for (line, ev) in std::mem::take(&mut self.or_ref_ends) {
      self.add_external_event(line, ev);
    }
    self.place_repointed_ends();
    self.place_arm_local_ends();
    self.place_scrutinee_ends();
    self.place_last_holders();
    let empty_payloads = self.place_proven_fork_ends();
    self.end_outer_borrows_in_arms();
    self.place_nested_ends();
    // The exit walk must see the MIR returns inside a let-else, including
    // both arms of a nested test. Otherwise it adds another at the join.
    self.place_let_else_ends();
    self.place_exit_ends(&empty_payloads);
    mark_exhaustion_paths(self.preprocessed_events, &self.exit_scopes);
    self.print_out_of_scope();
    if let Some(facts) = facts {
      crate::event_order::order_body(self, facts);
    }
    note_later_borrowers_in_arms(self.preprocessed_events, false);
    // (With the retired loans in `borrow_map` meanwhile, under keys of
    // their own: `w.len()` while an earlier `r` of a reused name still
    // borrows w.)
    let mut keys = Vec::new();
    for (i, (name, rd)) in std::mem::take(&mut self.retired_sharers).into_iter().enumerate() {
      let Some(rap) = self.raps.get(&name).cloned() else { continue };
      let key = retired_key(&name, i);
      self.raps.insert(key.clone(), rap);
      self.borrow_map.insert(key.clone(), rd);
      keys.push(key);
    }
    self.note_sharers_across_calls();
    for key in keys {
      self.raps.remove(&key);
      self.borrow_map.remove(&key);
    }
    self.note_loop_iterator_keeps_moved();
  }

  /// The end of a moved `for` binding's borrow (#1511) gives nothing back
  /// to its lender: the loop's iterator still holds the loan, until the
  /// loop ends. Say so on the end, which leaves the lender borrowed.
  fn note_loop_iterator_keeps_moved(&mut self) {
    fn note(events: &mut [(usize, ExternalEvent)], name: &str, line: usize, ids: &mut Vec<usize>) {
      for (l, ev) in events.iter_mut() {
        match ev {
          ExternalEvent::MutableDie { from: ResourceTy::Value(r), to: ResourceTy::Value(_), still_borrowed_by, id, .. }
            if *l == line && r.name() == name && still_borrowed_by.is_empty() =>
          {
            *still_borrowed_by = vec![crate::svg_generator::hover_messages::LOOP_ITERATOR_HOLDER.to_owned()];
            ids.push(*id);
          }
          ExternalEvent::Branch { branches, .. } => {
            for b in branches.iter_mut() {
              note(&mut b.e_data, name, line, ids);
            }
          }
          _ => {}
        }
      }
    }
    let mut ids = Vec::new();
    for (name, line, _) in self.for_loop_moved_bindings.iter() {
      note(self.preprocessed_events, name, *line, &mut ids);
    }
    for evs in self.event_line_map.values_mut() {
      for e in evs.iter_mut() {
        if let ExternalEvent::MutableDie { id, still_borrowed_by, .. } = e {
          if ids.contains(id) {
            *still_borrowed_by = vec![crate::svg_generator::hover_messages::LOOP_ITERATOR_HOLDER.to_owned()];
          }
        }
      }
    }
  }

  /// A call that borrows a variable already shared by a reference or a
  /// holder (`let r = &v; v.len(); println!("{:?}", r);`) gives back
  /// only its own borrow: `r` still borrows `v` until its last use, so
  /// `v` stays shared after the call (#870). Record, on each such
  /// call's event, who goes on borrowing the lender. Only shared
  /// borrowers count: a call reading `v` while a `&mut` of it is live
  /// doesn't compile.
  fn note_sharers_across_calls(&mut self) {
    #[expect(clippy::disallowed_methods, reason = "collected, then sorted into declaration order")]
    let mut sharers: Vec<Sharer> = self.borrow_map.iter()
      .filter_map(|(key, rd)| {
        // (A retired loan's key isn't its name, #903.)
        let rap = &self.raps.get(key)?.rap;
        let (name, hash) = (rap.name(), *rap.hash());
        // A reference variable r is shared only by a borrow of r itself
        // (`let rr = &r;`, `refs.push(&r)`; #925, #973), not by one of
        // what r points to.
        let of_ref_var = |l: &ResourceTy| (*l == rd.lender
            && rd.borrow_span.is_some_and(|sp| self.ref_var_borrows.contains(&sp)))
          || self.stored_ref_var_lends.contains(&(name.clone(), l.real_name()));
        let lenders: Vec<String> = rd.all_lenders().iter()
          .filter(|l| matches!(l, ResourceTy::Value(r) if !r.is_ref() || of_ref_var(l)) && !rd.mut_of(l))
          .map(|l| l.real_name())
          .collect();
        if lenders.is_empty() {
          return None;
        }
        // A reference that took over another's loan (`let hold =
        // other;` in an arm) has its loan's start line; it exists only
        // from its first event. One whose value moved on (`let c =
        // Some(w);`) no longer holds the loan after the move.
        // (`preprocessed_events` holds the fns visited before this one
        // too: only lines from the loan's start on are this fn's.)
        let base = name.split('.').next().unwrap_or(name);
        let mut seen = (None, None);
        first_and_moved(self.preprocessed_events, base, rd.assigned_at, &mut seen);
        let from = seen.0.unwrap_or(rd.assigned_at);
        let moved = seen.1;
        Some(Sharer { name: name.clone(), hash, from, to: rd.lifetime, moved, lenders, from_kept_call: false, stored: false, gaps: Vec::new() })
      })
      .collect();
    // A re-pointed reference shares its old lenders until its old loan
    // ends (#1492).
    for (name, lenders, from, to) in self.repointed_loans.iter() {
      let Some(rap) = self.raps.get(name).map(|r| &r.rap) else { continue };
      sharers.push(Sharer {
        name: name.clone(), hash: *rap.hash(), from: *from, to: *to, moved: None, lenders: lenders.clone(),
        from_kept_call: false, stored: false, gaps: Vec::new(),
      });
    }
    // An arm whose path ends the reference's loan (it returns, or the
    // last use is in another arm) doesn't hold it after that end (#796).
    for s in sharers.iter_mut() {
      ended_arm_gaps(self.preprocessed_events, &s.name, &s.lenders, s.from, s.to, &self.row_layout, &mut s.gaps);
    }
    // A borrow a call may have stored in `v` (`keep(&mut v, &s)`,
    // #513) keeps s shared until the loan ends, as a live `let r = &s;`
    // does (#974). Once `v` is moved on (`let w = v;`, `let h = H { v
    // };`, `let w = pass(v);`), the borrow goes with its value: w holds
    // it from there on (#990), and the call's text names w too. A copy
    // of v (`v: Option<&String>`) holds it as well as v, each while it
    // is in use; a move in one arm of an `if` / `match` hands it on
    // only on that arm's path (#997).
    let mut moved_on: Vec<(usize, StoredIn)> = Vec::new();
    let raps = &self.raps;
    let hash_of = |name: &str| raps.get(name).map(|rd| *rd.rap.hash());
    for sb in self.stored_borrows.iter() {
      let Some(hash) = hash_of(&sb.holder) else { continue };
      let list = stored_call_list(self.preprocessed_events, sb.id).unwrap_or(self.preprocessed_events);
      // The values made from others whose variable holds the call's
      // loan, by rustc's borrow checker: their type carries the loan's
      // region while they are live (#1015).
      // (Without rustc's loan, as in a closure's body, only clones,
      // which have the holder's own type.)
      let holding = if self.derived.is_empty() || sb.loans.is_empty() { BTreeSet::new() } else { self.names_holding_loans(&sb.loans) };
      // (A field `h.v` is held when its variable h is.)
      let derived: Vec<&Derived> = self.derived.iter()
        .filter(|d| holding.contains(d.dest.split('.').next().unwrap_or(&d.dest)) || (sb.loans.is_empty() && d.clone))
        .collect();
      let mut walk = HolderWalk {
        lender: sb.lender.clone(), hash_of: &hash_of, derived: &derived, seen: vec![sb.holder.clone()], holders: Vec::new(), hops: Vec::new(),
      };
      walk.follow(list, &sb.holder, hash, sb.at, sb.until, false);
      if !sb.is_mut {
        sharers.extend(walk.holders);
      }
      if !walk.hops.is_empty() {
        let pick = |copy: bool, clone: bool, made: bool| walk.hops.iter()
          .filter(|h| h.1 == copy && h.3 == clone && h.4 == made).map(|h| h.0.clone()).collect::<Vec<_>>();
        let moved = pick(false, false, false);
        // Two moves of one holder's value are in different arms, so
        // only one of them happens (`match k { 0 => { let w = v; } 1 =>
        // { let x = v; } }`); otherwise they're one after the other.
        let alternatives = walk.hops.iter().enumerate()
          .any(|(i, h)| !h.1 && walk.hops[i + 1..].iter().any(|o| !o.1 && o.2 == h.2));
        // A move of a clone's value (`let w = v.clone(); let x = w;`),
        // or of another value made from v's, comes after it: the text
        // names the clone first.
        let mut cloned: Vec<&String> = Vec::new();
        let mut of_clone = false;
        for h in walk.hops.iter() {
          if h.3 || h.4 || cloned.contains(&&h.2) {
            of_clone |= !h.1;
            cloned.push(&h.0);
          }
        }
        // A value made from another is named with the holder it was
        // made from (`x`, made from w, after `let w = v.clone();`).
        let made = walk.hops.iter().filter(|h| h.4).map(|h| (h.0.clone(), h.2.clone())).collect();
        moved_on.push((sb.id, StoredIn(None, moved, pick(true, false, false), alternatives, pick(true, true, false), of_clone, made)));
      }
    }
    self.place_stored_ends();
    for (id, holders) in moved_on.iter() {
      set_stored_moved_to(self.preprocessed_events, *id, holders);
      for evs in self.event_line_map.values_mut() {
        for e in evs.iter_mut() {
          if let ExternalEvent::PassByStaticReference { id: i, stored_in, .. }
            | ExternalEvent::PassByMutableReference { id: i, stored_in, .. } = e
          {
            if i == id {
              stored_in.set_holders(holders);
            }
          }
        }
      }
    }
    sharers.sort_by(|a, b| a.hash.cmp(&b.hash).then_with(|| a.name.cmp(&b.name)));
    // A `for` loop's iterator that borrows the lender with no loop
    // variable to name it (`for (i, a) in v.iter().enumerate() {
    // v.len(); }`) shares it until the loop's `}` (#900). It has no
    // column, so it comes after the variables.
    for (lender, from, to, head_call) in &self.for_loop_iter_loans {
      sharers.push(Sharer {
        name: crate::svg_generator::hover_messages::LOOP_ITERATOR_HOLDER.to_owned(),
        hash: u64::MAX,
        from: *from,
        to: *to,
        moved: None,
        lenders: vec![lender.clone()],
        from_kept_call: *head_call,
        stored: false,
        gaps: Vec::new(),
      });
    }
    let mut noted: HashMap<usize, Vec<String>> = HashMap::new();
    note_sharers_in(self.preprocessed_events, &sharers, &self.row_layout.ended_before_call, &mut Vec::new(), &mut noted);
    // A borrow end between two variables is listed by line too.
    for evs in self.event_line_map.values_mut() {
      for e in evs.iter_mut() {
        if let ExternalEvent::StaticDie { id, still_borrowed_by, .. } = e {
          if let Some(names) = noted.get(id) {
            *still_borrowed_by = names.clone();
          }
        }
      }
    }
  }

  /// A reference declared before an `if` / `match` whose borrow ends
  /// after it, on a path through an arm that leaves early (`return`,
  /// a panic, a `break` / `continue` the borrow doesn't outlive): the
  /// reference is never used again on that path, so its borrow ends
  /// there, after the arm's last use of it, or where the arm starts if
  /// it doesn't use it. `emit_borrow_ends` drew the end only at the
  /// later use, which that path never reaches, so the arm left the
  /// lender lent, and an exit that drops the lender dropped it while
  /// lent (#663). Give each such arm its own end of the borrow, ahead
  /// of the exit's drops; several ending on one line end in reverse
  /// declaration order.
  /// The closure a `move` closure literal moved the `&mut` reference
  /// `r` into (`let f = move || r.push('!');`, #1148), anywhere in the
  /// body (also in a branch of a conditional).
  fn closure_moved_into(&self, r: &ResourceAccessPoint) -> Option<ResourceAccessPoint> {
    if !r.is_mutref() {
      return None;
    }
    fn find(events: &[(usize, ExternalEvent)], hash: u64) -> Option<ResourceAccessPoint> {
      events.iter().find_map(|(_, e)| match e {
        ExternalEvent::Move { from: ResourceTy::Value(f), to: ResourceTy::Value(c), closure_value: false, .. }
          if *f.hash() == hash && c.is_closure() => Some(c.clone()),
        ExternalEvent::Branch { branches, .. } => branches.iter().find_map(|b| find(&b.e_data, hash)),
        _ => None,
      })
    }
    find(self.preprocessed_events, *r.hash())
  }

  /// Whether `r`'s value is moved into a call on `line` (top level).
  fn moved_into_call_on(&self, r: &ResourceAccessPoint, line: usize) -> bool {
    self.preprocessed_events.iter().any(|(l, e)| *l == line && matches!(e,
      ExternalEvent::Move { from: ResourceTy::Value(f), to: ResourceTy::Value(ResourceAccessPoint::Function(_)), .. } if f == r))
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
  /// paths through the other arms it has already ended. `emit_borrow_ends`
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
    // What holds a borrow from where it's assigned: a reference or a
    // value holding one, or a struct one of whose reference fields does
    // (`x` for `x.part`).
    let holders = borrow_holders(&self.borrow_map);
    // Until nothing more moves: moving one end into the arms makes its
    // lender live in the conditional (`end_borrow_in_arms`), and that
    // lender's own end, listed before it, ends in the arms too (a
    // parameter `m` reborrowed by `m2 = &mut *m`, m2 last used in one
    // arm, #1513).
    let mut moved = true;
    while moved {
    moved = false;
    let mut i = 0;
    while i < self.preprocessed_events.len() {
      let (line, ev) = &self.preprocessed_events[i];
      let (r, lender) = match ev {
        ExternalEvent::StaticDie { from: ResourceTy::Value(r), to, .. }
        | ExternalEvent::MutableDie { from: ResourceTy::Value(r), to, .. } => {
          (r, to.extract_rap().filter(|l| *l != r))
        }
        // A RefDie gives nothing back to its lender (see emit_borrow_ends).
        ExternalEvent::RefDie { from: ResourceTy::Value(r), .. } => (r, None),
        _ => { i += 1; continue; }
      };
      // A function's column isn't drawn in a conditional's arms.
      let Some((k, a)) = branch_ending_borrow(self.preprocessed_events, r, lender, *line).filter(|_| !lender.is_some_and(|l| l.is_fn())) else {
        i += 1;
        continue;
      };
      // An end moved into the arm a let-chain's later parts are drawn
      // in, on the split line (#1279), stays among the arrows the fn's
      // top level stacks on that line (a pattern's copy out of the
      // scrutinee), so a row is added for it, but is drawn only in the
      // arm, which lists it in its line map (`arm_drawn` in
      // svg_generation.rs).
      let at_split = matches!(&self.preprocessed_events[k].1, ExternalEvent::Branch { split_point, .. } if split_point == line);
      // (Unless that arm moved r on, `&& { let w = r; take(w); true }`,
      // #1262: what holds it now ends the borrow there, and no arm
      // draws this end, so it leaves the top level's stack.)
      let handed_off = at_split && matches!(&self.preprocessed_events[k].1, ExternalEvent::Branch { branches, .. }
        if handed_off_in(&branches[a].e_data, r, &self.moved_into_result, &holders));
      let (r, lender) = (r.clone(), lender.cloned());
      let ev = self.preprocessed_events.remove(i);
      if handed_off {
        let id = ev.1.get_id();
        if let Some(evs) = self.event_line_map.get_mut(&ev.0) {
          evs.retain(|e| e.get_id() != id);
        }
      }
      // Arrows listed together on a line are drawn one below the other,
      // with a blank row per extra arrow. An arm's line map also lists
      // arrows to and from functions (#419), so listing the moved arrow
      // there would add a row the top level didn't have; list it only
      // if it was stacked with other arrows at the top level. An arrow
      // its line map doesn't list is drawn first on its line.
      let stacked = at_split || self.event_line_map.get(&ev.0).is_some_and(|evs| evs.len() > 1);
      // The line the reference's current loan was made on, if `ev` ends
      // that loan. An end of an earlier loan (`c = &y;` ends c's loan of
      // x, made before the conditional; #778, #801) isn't described by it.
      let made = self.borrow_map.get(r.name()).filter(|d| ends_current_loan(&ev.1, d)).map(|d| d.assigned_at);
      let copies = *self.unique_id;
      let original = ev.1.get_id();
      let facts = self.body_facts;
      let ids = self.end_loans.get(&original);
      let mut entry_end = |arm, id| {
        let (row, edge) = facts?.reference_end_entering(arm, r.ref_local()?, ids?)?;
        self.row_layout.record_arm_point(id, edge);
        Some((row, matches!(edge.fact, crate::facts::Fact::HoldEnd { .. })))
      };
      let original_row = ev.0;
      let moved_entry = end_borrow_in_arms(self.preprocessed_events, ev, &r, lender.as_ref(), made, stacked,
        self.unique_id, &self.moved_into_result, &holders, &mut entry_end);
      if moved_entry {
        if let Some(arrows) = self.event_line_map.get_mut(&original_row) { arrows.retain(|e| e.get_id() != original); }
      }
      if let Some(loans) = self.end_loans.get(&original).cloned() {
        for id in copies..*self.unique_id { self.end_loans.insert(id,loans.clone()); }
      }
      moved = true;
    }
    }
  }
}


/// Whether `r`'s loan of `lender` is a mutable one that no other loan
/// of `lender` among `others` (the references drawn borrowing it) can
/// share: each one whose lines overlap r's is a shared loan made before
/// it, which rustc doesn't let anything use while r's loan is live. A
/// loan made while r's is live (`x` from `it.next()`, `it =
/// v.iter_mut()`) may be one derived from it.
pub fn mutable_loan_alone<'a>(r: &str, lender: &str, borrow_map: &HashMap<String, RefData>, others: impl Iterator<Item = &'a String>) -> bool {
  let Some(rd) = borrow_map.get(r) else { return false };
  let of = |d: &RefData| d.all_lenders().into_iter().find(|l| l.real_name() == lender);
  // (A variable lender: the caller's data a parameter borrows isn't one.)
  if !of(rd).is_some_and(|l| rd.mut_of(&l) && lends_column(&l)) {
    return false;
  }
  let mut others = others;
  others.all(|o| o == r || borrow_map.get(o).is_none_or(|d| d.lifetime < rd.assigned_at || d.assigned_at > rd.lifetime
    || (d.assigned_at < rd.assigned_at && of(d).is_none_or(|l| !d.mut_of(&l)))))
}

/// Whether `l` is a variable (or a place in one) that a loan borrows,
/// not the caller's data or a value with no column.
fn lends_column(l: &ResourceTy) -> bool {
  matches!(l, ResourceTy::Value(_) | ResourceTy::Deref(_) | ResourceTy::DerefField(..))
}

/// A reference or holder that shares one or more lenders, for
/// `note_sharers_across_calls`.
#[derive(Clone)]
struct Sharer {
  name: String,
  hash: u64,
  /// The line its loan starts on (or it first appears).
  from: usize,
  /// The line its loan ends on.
  to: usize,
  /// The line its value is moved away on, if it is.
  moved: Option<usize>,
  /// The names of the variables it shares.
  lenders: Vec<String>,
  /// A call on its `from` line counts too, once the call whose result
  /// keeps the loan has borrowed the lender there: `v.len()` in `for x
  /// in v.iter().take(v.len())` runs while `iter`'s result holds v.
  from_kept_call: bool,
  /// A borrow a call may have stored in it (`keep(&mut v, &s)`, #974),
  /// not a loan of its own in `borrow_map`.
  stored: bool,
  /// Lines (first, last) it doesn't hold the loan on: the rest of an
  /// arm that moved its value on (`if c { let w = v; .. }`, #997).
  gaps: Vec<ArmGap>,
}

#[derive(Clone)]
struct ArmGap {
  first: usize,
  last: usize,
  arm: Option<crate::facts::SourceArm>,
}

impl Sharer {
  fn in_gap(&self, line: usize, path: &[crate::facts::SourceArm]) -> bool {
    self.gaps.iter().any(|gap| gap.first <= line && line <= gap.last && gap.arm.is_none_or(|arm| path.contains(&arm)))
  }
}

/// The first line from `start` on that an event of `events` (branches
/// included) involves `base`, and the first line from there on that
/// `base` is moved from, into `seen`.
fn first_and_moved(events: &[(usize, ExternalEvent)], base: &str, start: usize, seen: &mut (Option<usize>, Option<usize>)) {
  let named = |r: &ResourceTy| matches!(r, ResourceTy::Value(_) | ResourceTy::Deref(_) | ResourceTy::DerefField(..))
    && r.real_name().split('.').next() == Some(base);
  for (line, ev) in events {
    if let ExternalEvent::Branch { branches, .. } = ev {
      for b in branches {
        first_and_moved(&b.e_data, base, start, seen);
      }
      continue;
    }
    if *line < start {
      continue;
    }
    let (from, to) = ResourceAccessPoint_extract(ev);
    if named(from) || named(to) {
      seen.0 = Some(seen.0.map_or(*line, |l| l.min(*line)));
    }
    if matches!(ev, ExternalEvent::Move { from: ResourceTy::Value(f), .. } if f.name().split('.').next() == Some(base)) {
      seen.1 = Some(seen.1.map_or(*line, |l| l.min(*line)));
    }
  }
  // A move before the first event (the moves were collected from
  // `start` on) belongs to an earlier binding of the name.
  if let (Some(first), Some(m)) = (seen.0, seen.1) {
    if m < first {
      seen.1 = None;
    }
  }
}

/// The event list (top level or an arm's) that holds the call `call_id`
/// that may have stored a borrow (`keep(&mut v, &s)`).
fn stored_call_list(events: &[(usize, ExternalEvent)], call_id: usize) -> Option<&[(usize, ExternalEvent)]> {
  events.iter().find_map(|(_, ev)| match ev {
    ExternalEvent::Branch { branches, .. } => branches.iter().find_map(|b| stored_call_list(&b.e_data, call_id)),
    _ => None,
  }).or_else(|| events.iter().any(|(_, ev)| ev.get_id() == call_id
    && matches!(ev, ExternalEvent::PassByStaticReference { .. } | ExternalEvent::PassByMutableReference { .. })).then_some(events))
}

/// A move or copy of the value a stored borrow's holder holds (#990,
/// #997), on `line`, to `dest` (None: into something without a column
/// of its own, `take(v)`, or not followed).
struct Hop<'e> {
  line: usize,
  dest: Option<String>,
  copy: bool,
  /// A clone (`let w = v.clone();`, #1003), which is followed like a
  /// copy.
  clone: bool,
  /// Another value made from the holder's that rustc says holds the
  /// borrow (`let w: Vec<&String> = v.iter().copied().collect();`,
  /// #1015), also followed like a copy.
  derived: bool,
  /// `dest`'s drop may use the borrow: it holds it until it is
  /// dropped, not until its last use.
  dropped: bool,
  /// The same for the holder it is made from.
  src_dropped: bool,
  /// The event list it is in: where `dest` goes on from.
  list: &'e [(usize, ExternalEvent)],
  /// The last line of the `if` / `match` arm it is in, when that arm
  /// is nested in the list the walk looks at: only that arm's path
  /// hands the borrow on.
  arm_end: Option<usize>,
}

/// The moves and copies, after line `from` and up to `hi`, of the value
/// `holder` (a variable or a field of one) holds, in `list` and its
/// nested conditionals: `w` for `let w = v;`, `h.v` for `let h = H { v
/// };`, `h2.v` for holder `h.v` and `let h2 = h;`, and for a move `let
/// w = pass(v);` the variable the call's result goes to. A move in a
/// loop body ends the hold there, without a destination. A value made
/// from the holder's that holds the borrow (one of `derived`: `let w =
/// v.clone();`, `let w: Vec<&String> = v.iter().copied().collect();`) is
/// a copy of it here: it holds copies of the references in v (#1003,
/// #1015).
fn holder_hops<'e>(list: &'e [(usize, ExternalEvent)], holder: &str, from: usize, hi: usize, arm_end: Option<usize>, derived: &[&Derived], out: &mut Vec<Hop<'e>>) {
  for (line, ev) in list {
    // The event that gives a value made from the holder's to its
    // variable (the call's result moving to w, `let r = v[0];`'s copy).
    if let ExternalEvent::Move { to: ResourceTy::Value(d), .. } | ExternalEvent::Copy { to: ResourceTy::Value(d), .. }
      | ExternalEvent::Bind { to: ResourceTy::Value(d), .. } | ExternalEvent::StaticBorrow { to: ResourceTy::Value(d), .. }
      | ExternalEvent::MutableBorrow { to: ResourceTy::Value(d), .. } = ev
    {
      let mut found = false;
      for dv in derived.iter().filter(|dv| dv.dest == *d.name() && dv.lines.0 <= *line && *line <= dv.lines.1) {
        if *line <= from || *line > hi || d.is_fn() {
          continue;
        }
        for (src, src_dropped) in dv.srcs.iter() {
          // v itself, a value v is part of (`h.clone()` for holder
          // `h.v`: `h2.v`), or a part of v (`h.v.iter()` for holder h;
          // `h.v.clone()` isn't a clone of h).
          let (dest, clone) = match holder.strip_prefix(src.as_str()) {
            Some(rest) if rest.is_empty() || rest.starts_with('.') =>
              (format!("{}{}", d.name(), if dv.clone { rest } else { "" }), dv.clone),
            _ if src.strip_prefix(holder).is_some_and(|r| r.starts_with('.')) => (d.name().to_owned(), false),
            _ => continue,
          };
          found = true;
          if !out.iter().any(|h| h.line == *line && h.dest.as_ref() == Some(&dest)) {
            out.push(Hop {
              line: *line, dest: Some(dest), copy: true, clone, derived: !clone,
              dropped: dv.dropped, src_dropped: *src_dropped, list, arm_end,
            });
          }
        }
      }
      if found {
        continue;
      }
    }
    let (src, to, copy) = match ev {
      ExternalEvent::Branch { branches, branch_type, .. } => {
        for (a, b) in branches.iter().enumerate() {
          if matches!(branch_type, BranchType::Loop(..)) {
            let mut inner = Vec::new();
            holder_hops(&b.e_data, holder, from, hi, None, derived, &mut inner);
            out.extend(inner.into_iter().filter(|h| !h.copy).map(|h| Hop { dest: None, list, arm_end, ..h }));
          } else {
            let (_, end) = branch_type.get_start_end(a);
            holder_hops(&b.e_data, holder, from, hi.min(end), Some(end), derived, out);
          }
        }
        continue;
      }
      // A call's result moving to w, not made from the holder's value.
      ExternalEvent::Move { from: ResourceTy::Value(c), to: ResourceTy::Value(d), .. } if c.is_fn() && !d.is_fn() => continue,
      ExternalEvent::Move { from, to, .. } => (from, to, false),
      // A copy of the whole value (`let w = v;`, `H { v }`), not of a
      // part of it or of what a reference in it points to.
      ExternalEvent::Copy { from, to, is_partial: false, into_closure: false, closure_value: false, pointee: None, .. } => (from, to, true),
      _ => continue,
    };
    if *line <= from || *line > hi {
      continue;
    }
    let ResourceTy::Value(f) = src else { continue };
    if f.is_fn() {
      continue;
    }
    let Some(rest) = holder.strip_prefix(f.name().as_str()).filter(|r| r.is_empty() || r.starts_with('.')) else { continue };
    let dest = match to {
      // A copy passed to a call isn't followed into its result (`let n
      // = count(v);` returns a number, not the borrow).
      ResourceTy::Value(callee) if callee.is_fn() && !copy => list.iter().find_map(|(l, e)| match e {
        ExternalEvent::Move { from: ResourceTy::Value(c), to: ResourceTy::Value(d), .. } if l == line && c == callee && !d.is_fn() =>
          Some(d.name().to_owned()),
        _ => None,
      }),
      ResourceTy::Value(d) if !d.is_fn() => Some(format!("{}{}", d.name(), rest)),
      _ => None,
    };
    out.push(Hop { line: *line, dest, copy, clone: false, derived: false, dropped: false, src_dropped: false, list, arm_end });
  }
}

/// The last line, after `from` and up to `hi`, that an event of
/// `events` (branches included) uses `holder` or the variable it is a
/// field of: where it stops holding a borrow when nothing drops it
/// later (a Copy value has no drop).
fn last_use(events: &[(usize, ExternalEvent)], holder: &str, from: usize, hi: usize) -> Option<usize> {
  let base = holder.split('.').next().unwrap_or(holder);
  let named = |r: &ResourceTy| matches!(r, ResourceTy::Value(_) | ResourceTy::Deref(_) | ResourceTy::DerefField(..))
    && r.real_name().split('.').next() == Some(base);
  let mut last = None;
  for (line, ev) in events {
    let found = match ev {
      ExternalEvent::Branch { branches, .. } => branches.iter().filter_map(|b| last_use(&b.e_data, holder, from, hi)).max(),
      ExternalEvent::Bind { .. } | ExternalEvent::Copy { .. } | ExternalEvent::Move { .. }
      | ExternalEvent::StaticBorrow { .. } | ExternalEvent::MutableBorrow { .. }
      | ExternalEvent::PassByStaticReference { .. } | ExternalEvent::PassByMutableReference { .. }
        if *line > from && *line <= hi =>
      {
        let (a, b) = ResourceAccessPoint_extract(ev);
        (named(a) || named(b)).then_some(*line)
      }
      _ => None,
    };
    last = last.max(found);
  }
  last
}

/// Follows a borrow a call may have stored (`keep(&mut v, &s)`, #513)
/// through the moves and copies of its holder's value while the loan
/// is live (#990, #997).
struct HolderWalk<'w> {
  lender: String,
  hash_of: &'w dyn Fn(&str) -> Option<u64>,
  /// The values made from others (`ExprVisitor::derived`) whose
  /// variable rustc says holds the borrow.
  derived: &'w [&'w Derived],
  seen: Vec<String>,
  /// Each holder, as a sharer of the lender.
  holders: Vec<Sharer>,
  /// The variables the borrow went on to, in order, whether by a copy
  /// (true) or a move, the holder it went on from, whether the copy is
  /// a clone, and whether it is another value made from the holder's.
  hops: Vec<(String, bool, String, bool, bool)>,
}

impl HolderWalk<'_> {
  /// `holder` holds the borrow from line `from` on, up to `hi` at the
  /// latest; `list` is the event list it got it in. With `trim` (a copy
  /// or clone of the value, or a value moved on in one arm), it holds it
  /// until its last use, not until the loan ends; so does a value that
  /// is copied or cloned (a Copy value has no drop to keep it in use,
  /// and a `Vec`'s drop doesn't use the references in it). A clone of a
  /// value whose own `Drop` may use them isn't trimmed (#1003), nor is
  /// a value made from one (#1015).
  fn follow(&mut self, list: &[(usize, ExternalEvent)], holder: &str, hash: u64, from: usize, hi: usize, trim: bool) {
    let mut hops = Vec::new();
    holder_hops(list, holder, from, hi, None, self.derived, &mut hops);
    hops.sort_by_key(|h| h.line);
    // A move in `list` itself ends its hold; one in an arm only for the
    // rest of that arm.
    let cut = hops.iter().find(|h| !h.copy && h.arm_end.is_none()).map(|h| h.line);
    let end = cut.unwrap_or(hi);
    hops.retain(|h| h.line <= end);
    // (It still holds the value on the move's line.)
    let mut gaps: Vec<(usize, usize)> = hops.iter().filter(|h| !h.copy).filter_map(|h| h.arm_end.map(|e| (h.line + 1, e))).collect();
    let to = if trim || hops.iter().any(|h| h.copy && !h.src_dropped && h.dest.is_some()) {
      last_use(list, holder, from, end).unwrap_or(from)
    } else {
      hi
    };
    dead_arm_gaps(list, holder, from, to, &mut gaps);
    self.holders.push(Sharer {
      name: holder.to_owned(), hash, from, to, moved: cut, lenders: vec![self.lender.clone()], from_kept_call: false, stored: true,
      gaps: gaps.into_iter().map(|(first, last)| ArmGap { first, last, arm: None }).collect(),
    });
    for h in hops {
      let Some(dest) = h.dest else { continue };
      if self.seen.contains(&dest) {
        continue;
      }
      self.seen.push(dest.clone());
      self.hops.push((dest.clone(), h.copy, holder.to_owned(), h.clone, h.derived));
      let hash = (self.hash_of)(&dest).unwrap_or(hash);
      self.follow(h.list, &dest, hash, h.line, h.arm_end.map_or(hi, |e| e.min(hi)), trim || (h.copy && !h.dropped) || h.arm_end.is_some());
    }
  }
}

/// The lines of the arms of an `if` / `match` in `list` that a holder
/// whose hold ends on `to`, in another arm, is dead in from its last use
/// there on (#997): `keepv(&mut v, &s); if c { look(&s); } else {
/// v.len(); }` leaves s unborrowed in the `if` arm, as rustc does.
fn dead_arm_gaps(list: &[(usize, ExternalEvent)], holder: &str, from: usize, to: usize, out: &mut Vec<(usize, usize)>) {
  for (_, ev) in list {
    let ExternalEvent::Branch { branches, branch_type, .. } = ev else { continue };
    if matches!(branch_type, BranchType::Loop(..)) {
      continue;
    }
    let ranges: Vec<(usize, usize)> = (0..branches.len()).map(|a| branch_type.get_start_end(a)).collect();
    let Some(j) = ranges.iter().position(|&(a, b)| a <= to && to <= b) else { continue };
    for (i, b) in branches.iter().enumerate() {
      let (start, end) = ranges[i];
      if i == j {
        dead_arm_gaps(&b.e_data, holder, from, to, out);
      } else if end > from {
        let first = last_use(&b.e_data, holder, from, end).map_or(start.max(from + 1), |u| u + 1);
        if first <= end {
          out.push((first, end));
        }
      }
    }
  }
}

/// The lines of the arms of an `if` / `match` in `list` that come after
/// the end of `holder`'s loan of one of `lenders` drawn in that arm
/// (#796): `let u = &h; if c { println!("{}", h); return; }
/// println!("{}", u);` ends u's borrow at the arm's start, so the arm's
/// `println!` leaves h unborrowed, not "still borrowed by u". The end's
/// own line counts too unless the arm uses `holder` there.
fn ended_arm_gaps(list: &[(usize, ExternalEvent)], holder: &str, lenders: &[String], from: usize, to: usize, layout: &crate::event_order::RowLayout, out: &mut Vec<ArmGap>) {
  for (_, ev) in list {
    let ExternalEvent::Branch { branches, branch_type, .. } = ev else { continue };
    for (i, b) in branches.iter().enumerate() {
      ended_arm_gaps(&b.e_data, holder, lenders, from, to, layout, out);
      if matches!(branch_type, BranchType::Loop(..)) {
        continue;
      }
      let (_, end) = branch_type.get_start_end(i);
      let ended = b.e_data.iter().filter_map(|(line, e)| match e {
        ExternalEvent::StaticDie { from: ResourceTy::Value(r), to: ResourceTy::Value(l), .. }
        | ExternalEvent::MutableDie { from: ResourceTy::Value(r), to: ResourceTy::Value(l), .. }
          if r.name() == holder && lenders.iter().any(|n| n == l.name()) && from <= *line && *line <= to.min(end) => Some((*line, e.get_id())),
        _ => None,
      }).min();
      if let Some((l, id)) = ended {
        let arm = b.source.filter(|_| layout.has_arm_point(id));
        let first = if arm.is_none() && last_use(&b.e_data, holder, l - 1, l) == Some(l) { l + 1 } else { l };
        if first <= end {
          out.push(ArmGap { first, last: end, arm });
        }
      }
    }
  }
}

/// Records, on the call event `id` that may have stored a borrow
/// (`keep(&mut v, &s)`), the variables v's value moves on to (#990)
/// and the copies of it that hold the borrow too (#997).
fn set_stored_moved_to(events: &mut [(usize, ExternalEvent)], id: usize, holders: &StoredIn) {
  for (_, ev) in events.iter_mut() {
    match ev {
      ExternalEvent::Branch { branches, .. } => {
        for b in branches.iter_mut() {
          set_stored_moved_to(&mut b.e_data, id, holders);
        }
      }
      ExternalEvent::PassByStaticReference { id: i, stored_in, .. }
      | ExternalEvent::PassByMutableReference { id: i, stored_in, .. } if *i == id => {
        stored_in.set_holders(holders);
      }
      _ => {}
    }
  }
}

/// In an arm of an `if` / `match`, the end of one reference's shared
/// loan of a lender, before the end of another's that held a loan of it
/// already: the lender stays borrowed by that one in between. The end of
/// `x2`'s borrow copied to the start of the `else` arm in `if c {
/// use(x2); } else { use(r); }` (#1425) said s was no longer borrowed
/// while r still was, until its end in the arm (#1440).
fn note_later_borrowers_in_arms(events: &mut [(usize, ExternalEvent)], in_arm: bool) {
  if in_arm {
    note_later_borrowers(events);
  }
  for (_, e) in events.iter_mut() {
    let ExternalEvent::Branch { branches, branch_type, .. } = e else { continue };
    if matches!(branch_type, BranchType::Loop(..)) {
      continue;
    }
    for b in branches.iter_mut() {
      note_later_borrowers_in_arms(&mut b.e_data, true);
    }
  }
}

/// `note_later_borrowers_in_arms` on one arm's events.
fn note_later_borrowers(events: &mut [(usize, ExternalEvent)]) {
  let end = |e: &ExternalEvent| match e {
    ExternalEvent::StaticDie { from: ResourceTy::Value(r), to: ResourceTy::Value(l), .. } if r != l => Some((r.clone(), *l.hash())),
    _ => None,
  };
  let borrows = |e: &ExternalEvent, l: u64, b: &ResourceAccessPoint| matches!(e,
    ExternalEvent::StaticBorrow { from, to: ResourceTy::Value(t), .. }
    | ExternalEvent::MutableBorrow { from, to: ResourceTy::Value(t), .. }
      if t.hash() == b.hash() && from.extract_rap().is_some_and(|f| *f.hash() == l));
  // The arm's copies of one reference into another (`let _r2 = r;`), as
  // (from, to) hashes.
  let copies: Vec<(u64, u64)> = events.iter().filter_map(|(_, e)| match e {
    ExternalEvent::Copy { from: ResourceTy::Value(f), to: ResourceTy::Value(t), .. } => Some((*f.hash(), *t.hash())),
    _ => None,
  }).collect();
  // (Not a reference `a` is a copy of: they share one loan, which `a`
  // gives back as its last holder on this path; the other isn't used
  // after it here, `Some(_) => { let _r2 = r; .. }` with r read only in
  // another arm.)
  let copied_from = |a: u64| {
    let mut out: Vec<u64> = Vec::new();
    let mut work = vec![a];
    while let Some(n) = work.pop() {
      for (f, t) in &copies {
        if *t == n && !out.contains(f) {
          out.push(*f);
          work.push(*f);
        }
      }
    }
    out
  };
  let mut notes: Vec<(usize, Vec<String>)> = Vec::new();
  for (i, (line, e)) in events.iter().enumerate() {
    let Some((a, l)) = end(e) else { continue };
    if matches!(e, ExternalEvent::StaticDie { still_borrowed_by, .. } if !still_borrowed_by.is_empty()) {
      continue;
    }
    let sources = copied_from(*a.hash());
    let mut names: Vec<String> = Vec::new();
    for (j, (later, o)) in events.iter().enumerate().skip(i + 1) {
      let Some((b, lb)) = end(o) else { continue };
      if *later > *line && lb == l && b.hash() != a.hash() && !names.contains(b.name()) && !sources.contains(b.hash())
        && !events[i + 1..j].iter().any(|(_, m)| borrows(m, l, &b)) {
        names.push(b.name().to_owned());
      }
    }
    if !names.is_empty() {
      notes.push((i, names));
    }
  }
  for (i, names) in notes {
    if let ExternalEvent::StaticDie { still_borrowed_by, .. } = &mut events[i].1 {
      *still_borrowed_by = names;
    }
  }
}

/// `note_sharers_across_calls` over `events`, branches included.
/// The borrow ends it notes are recorded in `noted`, by event id.
fn note_sharers_in(events: &mut [(usize, ExternalEvent)], sharers: &[Sharer], ended: &BTreeMap<usize,BTreeSet<String>>, path: &mut Vec<crate::facts::SourceArm>, noted: &mut HashMap<usize, Vec<String>>) {
  // The lenders a call whose result keeps the borrow has borrowed so
  // far, with its line.
  let mut kept: Vec<(usize, String)> = Vec::new();
  for (line, ev) in events.iter_mut() {
    if let ExternalEvent::PassByStaticReference { from: ResourceTy::Value(lender), result_keeps_borrow: true, .. } = ev {
      kept.push((*line, lender.name().to_owned()));
    }
    match ev {
      ExternalEvent::PassByStaticReference { from: ResourceTy::Value(lender), stored_in, result_keeps_borrow, still_borrowed_by, of_ref_var, id, .. }
        if stored_in.0.is_none() && !*result_keeps_borrow && (!lender.is_ref() || *of_ref_var) =>
      {
        // Each fn's visitor runs this over the events of the fns
        // visited before it too; its sharers are on its own lines, so
        // they leave those events alone.
        let names: Vec<String> = sharers.iter()
          .filter(|s| {
            let started = s.from < *line
              || (s.from_kept_call && s.from == *line && kept.iter().any(|(l, n)| l == line && n == lender.name()));
            !ended.get(id).is_some_and(|names|names.contains(&s.name)) && started && s.to >= *line && s.moved.is_none_or(|m| m >= *line) && !s.in_gap(*line, path)
              && s.lenders.iter().any(|l| l == lender.name())
          })
          .map(|s| s.name.clone())
          .fold(Vec::new(), |mut names, n| {
            // (Nested loops over the lender, or a holder with two loans
            // of it, `keep(&mut v, &s)` twice: one mention.)
            if !names.contains(&n) {
              names.push(n);
            }
            names
          });
        if !names.is_empty() {
          *still_borrowed_by = names;
        }
      }
      // The end of a shared loan while a borrow a call stored keeps
      // the lender shared (#974): a reference's last use while v holds
      // it (`keep(&mut v, &s); let r = &s; println!("{}", r); v.len()`),
      // or the stored borrow's end while a reference still holds one
      // (`keep(&mut v, &s); let r = &s; v.len(); println!("{}", r)`).
      ExternalEvent::StaticDie { from, to: ResourceTy::Value(lender), still_borrowed_by, of_ref_var, id, .. }
        if still_borrowed_by.is_empty() && (!lender.is_ref() || *of_ref_var) =>
      {
        let (holder, stored_end) = match from {
          ResourceTy::Value(r) => (r.name().to_owned(), false),
          ResourceTy::Index(_, place, PlaceWrite::Update) => (place.clone(), true),
          _ => continue,
        };
        let mut names: Vec<String> = Vec::new();
        for s in sharers.iter().filter(|s| (s.stored || stored_end) && s.name != holder
            && s.from <= *line && s.to > *line && s.moved.is_none_or(|m| m > *line) && !s.in_gap(*line, path)
            && s.lenders.iter().any(|l| l == lender.name()))
        {
          if !names.contains(&s.name) {
            names.push(s.name.clone());
          }
        }
        if !names.is_empty() {
          noted.insert(*id, names.clone());
          *still_borrowed_by = names;
        }
      }
      ExternalEvent::Branch { branches, .. } => {
        for b in branches.iter_mut() {
          if let Some(arm) = b.source { path.push(arm); }
          note_sharers_in(&mut b.e_data, sharers, ended, path, noted);
          if b.source.is_some() { path.pop(); }
        }
      }
      _ => {}
    }
  }
}

/// Whether `e`, the end of a borrow `d` describes the reference of,
/// gives back `d`'s loan (rather than one the reference held earlier).
fn ends_current_loan(e: &ExternalEvent, d: &RefData) -> bool {
  let Some(l) = borrow_end_of(e).and_then(|(_, to)| to.extract_rap()) else { return true };
  d.lender.extract_rap() == Some(l) || d.co_lenders.iter().any(|c| c.extract_rap() == Some(l))
}

/// Whether `events` (looking into nested conditionals) make `r` borrow
/// from `lender` on or before `line`: a loan made inside an arm.
fn loan_made_in(events: &[(usize, ExternalEvent)], r: &ResourceAccessPoint, lender: &ResourceAccessPoint, line: usize) -> bool {
  events.iter().any(|(l, e)| match e {
    ExternalEvent::Branch { branches, .. } => branches.iter().any(|b| loan_made_in(&b.e_data, r, lender, line)),
    ExternalEvent::StaticBorrow { from, to: ResourceTy::Value(t), .. }
    | ExternalEvent::MutableBorrow { from, to: ResourceTy::Value(t), .. } => {
      *l <= line && t == r && from.extract_rap() == Some(lender)
    }
    _ => false,
  })
}

/// The `if` / `match` among `events` that `r` is live in (declared
/// before it) and whose arm `a` holds `line`: (its index, a).
pub(crate) fn branch_ending_borrow(events: &[(usize, ExternalEvent)], r: &ResourceAccessPoint, lender: Option<&ResourceAccessPoint>, line: usize) -> Option<(usize, usize)> {
  events.iter().enumerate().find_map(|(k, (if_line, ev))| {
    let ExternalEvent::Branch { branches, live_vars, split_point, merge_point, branch_type, .. } = ev else { return None };
    // Declared before the conditional: a `let` inside an arm is in that
    // arm's `decl_vars` (and, for a `match`, also in `live_vars`; #425).
    // Not live in it either, but holding a loan of a variable that is,
    // and given back on a line of an arm (a closure called there, `c()`;
    // the loan of `rest` in `[x, rest @ ..]` that x's use there keeps
    // alive; one MIR ends as an arm starts): on the lender's column the
    // end landed between the arms' legs, on neither (#1417). It ends in
    // the arms as a live one's does, and r gets a column in the
    // conditional for it (`end_borrow_in_arms`).
    let lends_live = || lender.is_some_and(|l| live_vars.contains(l)) && !r.is_temporary() && !r.is_fn();
    if matches!(branch_type, BranchType::Loop(..))
      || !(live_vars.contains(r) || lends_live())
      || branches.iter().any(|b| b.decl_vars.contains(r))
    {
      return None;
    }
    // Whether arm `a` has an event on `line` that uses r (a conditional
    // nested there that r is live in does).
    let touching = |a: usize| branches[a].e_data.iter().any(|(l, e)| *l == line && touches(e, r));
    // A let-chain's later parts (`if let Some(x) = o && bump(r)`) run
    // only on the way into the `if` arm, and are drawn there, on the
    // split line: a last use of r among them ends its borrow in that arm
    // (and so before the other arms), not on r's column above the split,
    // before that use, or beside the arms (#1279). (A condition over
    // several lines splits on the `if` line, #1291, so its parts are on
    // the arm's own lines.) When the part moves r on, `&& { let w = r;
    // take(w); true }`, what it moved r into ends the borrow in that arm
    // (`end_borrow_in_arms` skips it there); the other arms still end
    // r's own, after the condition, not above the split before r was
    // moved (#1262).
    let chain_part = |line: usize| (matches!(branch_type, BranchType::If(..)) && line < *merge_point)
      .then(|| (0..branches.len()).find(|&a| touching(a)))
      .flatten();
    if line == *split_point && *if_line == *split_point {
      return chain_part(line).map(|a| (k, a));
    }
    // A borrow ending on the line an arm starts or the conditional ends
    // on can't be told apart from the conditional itself; leave it.
    if line < *split_point || line >= *merge_point {
      return None;
    }
    let holding: Vec<usize> = (0..branches.len())
      .filter(|&a| { let (start, end) = branch_type.get_start_end(a); start <= line && line <= end })
      .collect();
    match holding.as_slice() {
      // A `match` arm's pattern and guard written over lines of their
      // own, before the arm's body (`Some(x)` / `if bump(r) =>` / `{`):
      // the guard runs on the way into its arm, drawn there, as when it
      // is on the body's first line (#1291).
      [] if matches!(branch_type, BranchType::Match(..)) => (0..branches.len()).find(|&a| touching(a)).map(|a| (k, a)),
      [] => None,
      [a] => Some((k, *a)),
      // A line two arms share (`} else if let Some(x) = o && bump(r) {`):
      // the one that uses r there (#1279).
      [first, ..] => Some((k, holding.iter().copied().find(|&a| touching(a)).unwrap_or(*first))),
    }
  })
}

/// Whether `events` (an arm's, not looking into nested conditionals)
/// move the reference `r` into another reference, which then holds
/// its borrow, or into a call whose result a holder keeps (`let r2 =
/// id(r);`, `moved_into_result`, #1128; see `ref_handed_off`). Also a
/// value that holds a borrow (`r`: `Vec<&T>`, `Option<Part>`) moved into
/// a variable that holds it from there (`let x = u;`, `if let Some(x) = o`
/// with `x.part`; `holders`, #1198).
fn handed_off_in(events: &[(usize, ExternalEvent)], r: &ResourceAccessPoint, moved_into_result: &HashSet<usize>, holders: &HashSet<String>) -> bool {
  events.iter().any(|(_, e)| matches!(e,
    ExternalEvent::Move { from: ResourceTy::Value(f), to: ResourceTy::Value(t), id, .. }
      if f == r && (t.is_ref() || (t.is_fn() && moved_into_result.contains(id)) || (!r.is_ref() && holders.contains(t.name())))))
}

/// Whether `e` uses `r`; a conditional `r` is live in does.
pub(super) fn touches(e: &ExternalEvent, r: &ResourceAccessPoint) -> bool {
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
  // (Also a dot saying the condition may have moved r, which the arm
  // starts with: r's column runs to it, and not always further, #1279.)
  let i = events.iter().rposition(|(_, e)| touches(e, r)
    || matches!(e, ExternalEvent::LoopExitMove { ro, .. } if ro == r))?;
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
/// `made`: the line the borrow was made on, if `ev` ends the reference's
/// current loan. A borrow made inside an arm (`c = &y;` in the `if` arm,
/// or `c = d;` copying a reference there) doesn't exist on the paths
/// through the other arms, so they get no end of it (#800). The end of a
/// loan made before the conditional (`c`'s old loan of `x`, which `c = &y;`
/// ends on one path and a read ends on the other, #778) still ends on
/// every arm.
pub(crate) fn end_borrow_in_arms(
  events: &mut Vec<(usize, ExternalEvent)>,
  ev: (usize, ExternalEvent),
  r: &ResourceAccessPoint,
  lender: Option<&ResourceAccessPoint>,
  made: Option<usize>,
  in_line_map: bool,
  unique_id: &mut usize,
  moved_into_result: &HashSet<usize>,
  holders: &HashSet<String>,
  entry_end: &mut impl FnMut(crate::facts::SourceArm, usize) -> Option<(usize, bool)>,
) -> bool {
  let Some((k, a)) = branch_ending_borrow(events, r, lender, ev.0) else {
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
    return false;
  };
  let ExternalEvent::Branch { branches, live_vars, branch_type, split_point, merge_point, .. } = &mut events[k].1 else {
    unreachable!("branch_ending_borrow returns a Branch")
  };
  let ev_lender = borrow_end_of(&ev.1).and_then(|(_, to)| to.extract_rap());
  let made_in_arm = made.is_some_and(|m| *split_point < m && m <= ev.0 && m < *merge_point)
    || ev_lender.is_some_and(|l| branches.iter().any(|b| loan_made_in(&b.e_data, r, l, ev.0)));
  // The lender gets its value back in each arm, so it has a column in
  // the conditional, and so does r, which ends its borrow there. (Not a
  // lender an arm declares, `Some(x2) => vr.push(x2.as_str())`: it exists
  // only in that arm, which ends the borrow. Made live, it was drawn
  // forking at the `match` before it existed, its events twice, and
  // joining "still has no value", #1498.)
  if let Some(l) = lender.filter(|l| !branches.iter().any(|b| b.decl_vars.contains(l))) {
    live_vars.insert(l.clone());
  }
  live_vars.insert(r.clone());
  let mut original_at_entry = false;
  for (b, arm) in branches.iter_mut().enumerate() {
    if !made_in_arm {
      let id = if b == a { ev.1.get_id() } else { *unique_id };
      if let Some((row, own)) = arm.source.and_then(|arm| entry_end(arm, id)) {
        if b == a { original_at_entry = true; } else { *unique_id += 1; }
        let mut end = ev.1.clone();
        if own {
          let (_, to) = borrow_end_of(&end).unwrap();
          end = ExternalEvent::RefDie { slot: end.end_slot().unwrap(), from: ResourceTy::Value(r.clone()),
            to: to.clone(), num_curr_borrowers: 1, id };
        } else {
          match &mut end {
            ExternalEvent::StaticDie { id: i, .. } | ExternalEvent::MutableDie { id: i, .. } | ExternalEvent::RefDie { id: i, .. } => *i = id,
            _ => unreachable!(),
          }
        }
        arm.e_data.insert(0, (row, end));
        continue;
      }
    }
    if b == a {
      // `let h2 = h;` in this arm moved r's reference on (a `&mut`, or
      // a struct's field with the struct, #637): what holds it now
      // ends the borrow on this path, so r doesn't (#209).
      if handed_off_in(&arm.e_data, r, moved_into_result, holders) {
        continue;
      }
      let inner = branch_ending_borrow(&arm.e_data, r, lender, ev.0);
      // A let-chain nested here (`} else if let Some(x) = o && bump(r) {`,
      // #1279) whose split line has this arm's own arrows on it (the
      // pattern's copy): the end is listed with them, so a row is added
      // for it, though only the inner arm draws it.
      let inner_split = inner.is_some_and(|(k2, _)| matches!(&arm.e_data[k2].1,
        ExternalEvent::Branch { split_point, .. } if *split_point == ev.0));
      if ev.1.is_arrow_ev() && ((in_line_map && inner.is_none())
        || (inner_split && arm.line_map.get(&ev.0).is_some_and(|evs| !evs.is_empty()))) {
        arm.line_map.entry(ev.0).or_default().push(ev.1.clone());
      }
      original_at_entry = end_borrow_in_arms(&mut arm.e_data, ev.clone(), r, lender, made, in_line_map,
        unique_id, moved_into_result, holders, entry_end);
      if original_at_entry {
        if let Some(arrows) = arm.line_map.get_mut(&ev.0) { arrows.retain(|e| e.get_id() != ev.1.get_id()); }
      }
      continue;
    }
    if made_in_arm {
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
    // An arm that gives `r` a new value (`c = &y;`) ends the loan `r`
    // held before it on that line, ahead of the new borrow: the
    // lender is free again before `r` borrows anything else (#806).
    let new_value = arm.e_data.iter().position(|(_, e)| matches!(e,
      ExternalEvent::StaticBorrow { to: ResourceTy::Value(t), .. }
      | ExternalEvent::MutableBorrow { to: ResourceTy::Value(t), .. }
      | ExternalEvent::Move { to: ResourceTy::Value(t), .. }
      | ExternalEvent::Copy { to: ResourceTy::Value(t), .. } if t == r));
    if let Some(i) = new_value.filter(|_| last_use_in(&arm.e_data, r).is_some()) {
      let line = arm.e_data[i].0;
      arm.e_data.insert(i, (line, copy));
      continue;
    }
    end_after_last_use(&mut arm.e_data, branch_type.get_start_end(b).0, copy, r, lender, unique_id);
  }
  original_at_entry
}

/// Put `end`, a copy of the end of `r`'s borrow, into `events` (an arm's,
/// starting on line `start`) after the arm's last use of r; or, if the
/// arm doesn't use r, first thing in the arm: on that path the borrow
/// ended before it. When that last use is a conditional nested in the
/// arm (r used in some of its arms), the borrow ends on each of its
/// paths, after r's last use there, not at its join: on a path that
/// doesn't use r the lender is free again from the start of that arm
/// (`if n > 2 { println!("{}", a); } else { println!("{}", m); }`
/// with `m = &mut a` used in another outer arm too, #1520).
fn end_after_last_use(
  events: &mut Vec<(usize, ExternalEvent)>,
  start: usize,
  end: ExternalEvent,
  r: &ResourceAccessPoint,
  lender: Option<&ResourceAccessPoint>,
  unique_id: &mut usize,
) {
  let at = match last_use_in(events, r) {
    Some((_, i)) if matches!(&events[i - 1].1, ExternalEvent::Branch { branches, branch_type, .. }
      if !matches!(branch_type, BranchType::Loop(..)) && !branches.iter().any(|b| b.decl_vars.contains(r))) => {
      let ExternalEvent::Branch { branches, live_vars, branch_type, .. } = &mut events[i - 1].1 else { unreachable!("matched above") };
      if let Some(l) = lender {
        live_vars.insert(l.clone());
      }
      for (b, arm) in branches.iter_mut().enumerate() {
        let mut copy = end.clone();
        if b > 0 {
          match &mut copy {
            ExternalEvent::StaticDie { id, .. } | ExternalEvent::MutableDie { id, .. } | ExternalEvent::RefDie { id, .. } => {
              *id = *unique_id;
              *unique_id += 1;
            }
            _ => unreachable!("only a borrow's end is moved into arms"),
          }
        }
        end_after_last_use(&mut arm.e_data, branch_type.get_start_end(b).0, copy, r, lender, unique_id);
      }
      return;
    }
    // A note that r may have been moved where a labeled block ends
    // (`'blk: { if d { break 'blk; } drop(m); }`) after the arm moved r:
    // the loan went with the move, so the end is drawn there, not on
    // r's column after it was moved (#1531).
    Some((_, i)) if matches!(&events[i - 1].1, ExternalEvent::LoopExitMove { ro, .. } if ro == r)
      && events[..i - 1].iter().any(|(_, e)| matches!(e, ExternalEvent::Move { from: ResourceTy::Value(f), .. } if f == r)) => {
      let j = events[..i - 1].iter().rposition(|(_, e)| matches!(e, ExternalEvent::Move { from: ResourceTy::Value(f), .. } if f == r))
        .unwrap_or_default();
      (events[j].0, j + 1)
    }
    // After the arm's last use of r.
    Some(at) => at,
    None => (start, events.iter().position(|(el, _)| *el >= start).unwrap_or(events.len())),
  };
  events.insert(at.1, (at.0, end));
}

/// A HoldEnd on a branch edge belongs to its arm even if the entire
/// conditional occupies one source row. Keep it there during layout.
fn place_skipped_hold(events: &mut [(usize, ExternalEvent)], r: &ResourceAccessPoint, loans: &[usize], end: &ExternalEvent) -> bool {
  for (_, ev) in events {
    let ExternalEvent::Branch { live_vars, skipped_holds, branches, split_point, merge_point, .. } = ev else { continue };
    if let Some(hold) = skipped_holds.get_mut(r.hash()).filter(|hold| live_vars.contains(r)
      && hold.event.is_none() && loans.iter().all(|l| hold.loans.contains_key(l))) {
      if let Some(arm) = branches.get_mut(1) {
        arm.e_data.insert(0, (branch_column_rows(*split_point, *merge_point).0, end.clone()));
        hold.event = Some(end.get_id());
        return true;
      }
    }
    if branches.iter_mut().any(|arm| place_skipped_hold(&mut arm.e_data, r, loans, end)) { return true; }
  }
  false
}

/// If all arms leave this iteration, a later end in the same timeline
/// can still be reached directly from the iterator's exhaustion edge.
/// Match the placed end to the loop by its rustc jump target.
fn mark_exhaustion_paths(
  events: &mut [(usize, ExternalEvent)],
  scopes: &HashMap<usize, ExitScope>,
) {
  let mut later = HashMap::<_, BTreeSet<u64>>::new();
  for (line, event) in events.iter_mut().rev() {
    for (target, column) in exhaustion_at(event, *line) {
      later.entry(target).or_default().insert(column);
    }
    if let ExternalEvent::Branch { branches, id, on_exhaustion, .. } = event {
      if !branches.is_empty() && branches.iter().all(|b| b.diverges) {
        if let Some(columns) = scopes.get(id).and_then(|s| s.loop_id).and_then(|l| later.remove(&l)) {
          *on_exhaustion = columns;
        }
      }
      for arm in branches {
        mark_exhaustion_paths(&mut arm.e_data, scopes);
      }
    }
  }
}

/// The end's loan has an exhaustion edge to this loop's closing row.
pub(super) fn exhaustion_at(event: &ExternalEvent, line: usize) -> impl Iterator<Item = (rustc_hir::HirId, u64)> + '_ {
  let end = match event {
    ExternalEvent::StaticDie { from: ResourceTy::Value(r), exhaustion, .. }
    | ExternalEvent::MutableDie { from: ResourceTy::Value(r), exhaustion, .. } => Some((*r.hash(), exhaustion)),
    _ => None,
  };
  end.into_iter().flat_map(move |(hash, edges)| edges.iter()
    .filter(move |edge| edge.closing_row == line).map(move |edge| (edge.target, hash)))
}

pub(super) fn borrow_end_of(e: &ExternalEvent) -> Option<(&ResourceAccessPoint, &ResourceTy)> {
  match e {
    ExternalEvent::StaticDie { from: ResourceTy::Value(r), to, .. }
    | ExternalEvent::MutableDie { from: ResourceTy::Value(r), to, .. }
    | ExternalEvent::RefDie { from: ResourceTy::Value(r), to, .. } => Some((r, to)),
    _ => None,
  }
}

/// The give-backs from an anonymous holder (a loop's iterator) among
/// `events` and inside their arms: (row, the lender's name).
fn anonymous_gives_in(events: &[(usize, ExternalEvent)], out: &mut Vec<(usize, String)>) {
  for (line, e) in events {
    match e {
      ExternalEvent::Branch { branches, .. } => {
        for b in branches {
          anonymous_gives_in(&b.e_data, out);
        }
      }
      ExternalEvent::StaticDie { from: ResourceTy::Anonymous, to, .. } | ExternalEvent::MutableDie { from: ResourceTy::Anonymous, to, .. } => {
        if let Some(t) = to.extract_rap() {
          out.push((*line, t.name().to_owned()));
        }
      }
      _ => {}
    }
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
      // only some ways out moved it, it's maybe-moved (below).
      ExternalEvent::LoopExitMove { ro, definite: true, .. } if ro.name() == rap_name => {
        return Some(Ownership::None);
      }
      // Owns a value at every `break` out of the loop that just ended,
      // whatever the end of its body left (#1000).
      ExternalEvent::LoopExitKept { ro, .. } if ro.name() == rap_name => {
        return Some(Ownership::Held);
      }
      // The start of an `else` branch that a let-chain condition may
      // have moved it before (#281). It held the value before the
      // condition, which moved it.
      ExternalEvent::LoopExitMove { ro, at: MaybeMovedAt::ChainFail { .. }, .. } if ro.name() == rap_name => {
        return Some(Ownership::Maybe);
      }
      // The end of a `while` loop whose condition may have moved it
      // before it turned out false (#968): the body's events above may
      // have given it a new value, but the loop can end after the move.
      // Likewise a loop some of whose `break`s come after a move (#980):
      // `loop { drop(s); if c { break; } s = …; }` gives s a new value
      // after the `break`, not on the way out.
      // The start of a loop body that the loop can come back to after
      // a move further down (#992), and the end of a `while` / `for`
      // loop that can stop after such a time round.
      ExternalEvent::LoopExitMove { ro, at: MaybeMovedAt::WhileCondFail { .. } | MaybeMovedAt::Loop { .. }
        | MaybeMovedAt::LabeledBlock { .. } | MaybeMovedAt::LoopHead | MaybeMovedAt::LoopRound { .. }, .. } if ro.name() == rap_name => {
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
      // Called as an `FnOnce` closure, which moves it into the call
      // (#271, #1136).
      ExternalEvent::ClosureCall { closure, .. } if is_rap(closure) => {
        return Some(Ownership::None);
      }
      ExternalEvent::Copy { to, .. } | ExternalEvent::Bind { to, .. } => {
        if is_rap(to) {
          return Some(Ownership::Held);
        }
      }
      // A by-value parameter holds the caller's value from the start
      // (#245: `fn f(mut s: String) { s = … }` drops it).
      ExternalEvent::InitRefParam { param, .. } if param.name() == rap_name => {
        return Some(Ownership::Held);
      }
      ExternalEvent::Branch { branches, .. } if !branches.is_empty() => {
        let before = ownership_after(&events[..i], rap_name, start);
        // A branch that diverges (`return`, `break`, `continue`, a
        // panic) doesn't reach this point (#993): `if c { drop(y);
        // return; } y = …;` drops y's old value on every path that gets
        // there. (A `break`'s moves are the loop's `LoopExitMove`s
        // where it ends; a `continue`'s, the next time round's
        // `MaybeMovedAt::LoopHead`.)
        let ends: Vec<Option<Ownership>> = branches.iter().filter(|b| !b.diverges)
          .map(|b| ownership_after(&b.e_data, rap_name, before)).collect();
        if ends.is_empty() {
          return before;
        }
        let holds = |e: &Option<Ownership>| matches!(e, Some(Ownership::Held) | Some(Ownership::Maybe));
        let moved = |e: &Option<Ownership>| matches!(e, Some(Ownership::None) | Some(Ownership::Maybe));
        return if ends.iter().all(|e| *e == ends[0]) {
          ends[0]
        } else if ends.iter().any(holds) && ends.iter().any(moved) {
          // Moved on some paths and not on others.
          Some(Ownership::Maybe)
        } else if ends.iter().any(holds) {
          // Initialized on some paths only (`let mut s; if c { s = …
          // }`, #234): there may be a value to drop. The visitor's
          // `maybe_init_locals` says the drop is the drop flag's to
          // decide (#244).
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

/// What holds a borrow from where it's assigned: a reference or a value
/// holding one, or a struct one of whose reference fields does (`x` for
/// `x.part`). See `handed_off_in`.
pub(crate) fn borrow_holders(borrow_map: &HashMap<String, RefData>) -> HashSet<String> {
  let mut h = HashSet::new();
  #[expect(clippy::disallowed_methods, clippy::iter_over_hash_type, reason = "collected into a set")]
  for k in borrow_map.keys() {
    let mut n = k.as_str();
    h.insert(n.to_owned());
    while let Some((p, _)) = n.rsplit_once('.') {
      h.insert(p.to_owned());
      n = p;
    }
  }
  h
}

/// The values the `break`s of a `loop` carry out of it (`let r = loop
/// { … break ra; }`), as `branch_tails` gives a conditional's: `None`
/// for anything else, or a loop none of whose `break`s carries a value.
pub fn loop_break_tails<'tcx>(e: &'tcx Expr<'tcx>) -> Option<Vec<&'tcx Expr<'tcx>>> {
  // A labeled block's value is a `break`'s or its last expression's
  // (#1134).
  if let ExprKind::Block(b, Some(_)) = e.kind {
    let mut out: Vec<_> = breaks_to_id(e, b.hir_id).into_iter().filter_map(|(_, value)| value).collect();
    if out.is_empty() {
      return None;
    }
    out.extend(b.expr);
    return Some(out);
  }
  let ExprKind::Loop(_, _, rustc_hir::LoopSource::Loop, _) = e.kind else { return None };
  let out: Vec<_> = breaks_to(e).into_iter().filter_map(|(_, value)| value).collect();
  (!out.is_empty()).then_some(out)
}

/// The parts of reference type of the struct or tuple literal `e`, and
/// of the literals nested in it (`x` in `(x, 1)`, `&s` in `Part { text:
/// &s }`). Empty for anything else.
pub fn literal_ref_parts<'tcx>(e: &'tcx Expr<'tcx>, typeck: &rustc_middle::ty::TypeckResults<'tcx>) -> Vec<&'tcx Expr<'tcx>> {
  let mut out = Vec::new();
  let mut work = vec![e];
  while let Some(cur) = work.pop() {
    if cur.span.from_expansion() {
      continue;
    }
    let parts: Vec<&'tcx Expr<'tcx>> = match cur.kind {
      ExprKind::Tup(elems) => elems.iter().collect(),
      ExprKind::Struct(_, fields, _) => fields.iter().map(|f| f.expr).collect(),
      _ => continue,
    };
    for p in parts.into_iter().rev() {
      if typeck.node_type(p.hir_id).is_ref() {
        out.push(p);
      } else {
        work.push(p);
      }
    }
  }
  out
}

/// `e?`'s operand `e`: `e?` is a `MatchSource::TryDesugar` match on
/// `Try::branch(e)`. None for any other expression.
pub fn try_operand<'tcx>(e: &'tcx Expr<'tcx>) -> Option<&'tcx Expr<'tcx>> {
  match e.kind {
    ExprKind::Match(scrutinee, _, rustc_hir::MatchSource::TryDesugar(_)) => match scrutinee.kind {
      ExprKind::Call(_, [operand]) => Some(operand),
      _ => None,
    },
    _ => None,
  }
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

/// Whether branch body `body`'s value is itself an `if` / `match`, each
/// of whose branches is paired on its own.
pub fn branch_value_is_conditional<'tcx>(body: &'tcx Expr<'tcx>) -> bool {
  branch_value_expr(body).is_some_and(|v| !v.span.from_expansion()
    && matches!(v.kind, ExprKind::If(..) | ExprKind::Match(_, _, rustc_hir::MatchSource::Normal)))
}

/// Whether `pat` leaves a part of the value it takes apart unbound (a
/// `_`, a `..`): that part stays in the value (#439, #1011).
pub fn pat_leaves_parts(pat: &Pat<'_>) -> bool {
  let mut leaves = false;
  pat.walk_always(|p| match p.kind {
    PatKind::Wild => leaves = true,
    PatKind::Tuple(_, dd) | PatKind::TupleStruct(_, _, dd) if dd.as_opt_usize().is_some() => leaves = true,
    PatKind::Struct(_, _, rest) if rest.is_some() => leaves = true,
    _ => {}
  });
  leaves
}

/// The expression branch body `body` evaluates to: its block's tail,
/// through nested blocks. `None` for a block with no tail (it ends in
/// a `return` or a panic, or its value is `()`).
pub fn branch_value_expr<'tcx>(body: &'tcx Expr<'tcx>) -> Option<&'tcx Expr<'tcx>> {
  let mut e = body;
  loop {
    match e.kind {
      ExprKind::Block(b, _) => e = b.expr?,
      ExprKind::DropTemps(inner) => e = inner,
      _ => return Some(e),
    }
  }
}

/// Whether the pass-by-reference event `id` (looked up in `events`,
/// branches included) passes a mutable reference, and the function it
/// passes it to.
fn pass_ref_kind(events: &[(usize, ExternalEvent)], id: usize) -> Option<(bool, String)> {
  events.iter().find_map(|(_, ev)| match ev {
    ExternalEvent::PassByStaticReference { id: i, to, .. } if *i == id => Some((false, to.name())),
    ExternalEvent::PassByMutableReference { id: i, to, .. } if *i == id => Some((true, to.name())),
    ExternalEvent::Branch { branches, .. } => branches.iter().find_map(|b| pass_ref_kind(&b.e_data, id)),
    _ => None,
  })
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
/// The ids of the moves, on lines `lo..=hi`, of a `&mut` variable (or a
/// value holding a borrow, #1448)
/// whose hash is in `hashes` into one of the calls `fns` (see
/// `ExprVisitor::moved_into_result`).
fn moves_into_calls(events: &[(usize, ExternalEvent)], lo: usize, hi: usize, hashes: &[u64], fns: &[String], ids: &mut Vec<usize>) {
  for (line, ev) in events.iter() {
    match ev {
      ExternalEvent::Move { from: ResourceTy::Value(f), to: ResourceTy::Value(ResourceAccessPoint::Function(t)), id, .. }
        if (lo..=hi).contains(line) && (f.is_mutref() || (!f.is_ref() && f.holds_borrow()))
          && hashes.contains(f.hash()) && fns.contains(&t.name) => ids.push(*id),
      ExternalEvent::Branch { branches, .. } => {
        for b in branches.iter() {
          moves_into_calls(&b.e_data, lo, hi, hashes, fns, ids);
        }
      }
      _ => {}
    }
  }
}

fn mark_kept_in(events: &mut Vec<(usize, ExternalEvent)>, lo: usize, hi: usize, hashes: &[u64], place: Option<&str>, fns: &[String], ids: &mut Vec<usize>) {
  for (line, ev) in events.iter_mut() {
    let pass_from_lender = match &*ev {
      ExternalEvent::PassByStaticReference { from, to: ResourceTy::Value(ResourceAccessPoint::Function(f)), .. }
      | ExternalEvent::PassByMutableReference { from, to: ResourceTy::Value(ResourceAccessPoint::Function(f)), .. } =>
        fns.contains(&f.name) && match (from, place) {
          // `self.items.first()` with the whole reference as lender: the
          // place passed is inside what it points to (#1078).
          (ResourceTy::Value(r) | ResourceTy::DerefField(r, _, _, _), None) => hashes.contains(r.hash()),
          (ResourceTy::DerefField(r, p, _, _), Some(want)) => hashes.contains(r.hash()) && p == want,
          _ => false,
        },
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
          mark_kept_in(&mut b.e_data, lo, hi, hashes, place, fns, &mut inner);
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

/// See `ExprVisitor::ref_pointee_is_copy`.
pub fn ref_pointee_is_copy<'tcx>(tcx: TyCtxt<'tcx>, ref_ty: Ty<'tcx>, owner: rustc_hir::OwnerId) -> bool {
  let is_copy = |ty: Ty<'tcx>| tcx.type_is_copy_modulo_regions(typing_env(tcx, owner), ty);
  match ref_ty.kind() {
    TyKind::Ref(_, inner, _) => match inner.kind() {
      TyKind::Str => true,
      // A reference to a reference (`&mut &mut String`): what it
      // points to is a reference, which needs no drop (#914).
      TyKind::Ref(..) => true,
      TyKind::Slice(elem) => is_copy(*elem) || ty_is_plain(tcx, *elem, owner),
      _ => is_copy(*inner) || (inner.is_adt() && ty_is_plain(tcx, *inner, owner)),
    },
    _ => false,
  }
}

/// See `ExprVisitor::ty_is_plain`.
pub fn ty_is_plain<'tcx>(tcx: TyCtxt<'tcx>, ty: Ty<'tcx>, owner: rustc_hir::OwnerId) -> bool {
  !tcx.type_is_copy_modulo_regions(typing_env(tcx, owner), ty)
    && !needs_drop(tcx, ty, owner)
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
      // A closure holds what it captures by value (by reference, the
      // upvar is a `Ref`, which this doesn't look through).
      rustc_middle::ty::Closure(_, args) => go(tcx, args.as_closure().tupled_upvars_ty(), depth + 1, seen),
      _ => false,
    }
  }
  go(tcx, ty, 0, &mut HashSet::new())
}

/// The `&` pattern a binding is under, inside the pattern it is part of
/// (`y` in `Some(&y)`, `a` in `Some(&(a, b))`): `Some(part)`, `part`
/// when the binding isn't the `&` pattern's whole operand. `None` when
/// there is none, or when the `&` pattern is the whole pattern (`let &b
/// = r;`), which matches the scrutinee itself rather than a reference
/// stored in it (#874).
pub(crate) fn ref_pattern_above(tcx: &TyCtxt<'_>, binding: rustc_hir::HirId) -> Option<bool> {
  ref_pattern_above_at(tcx, binding).map(|(_, part)| part)
}

/// [`ref_pattern_above`], with the `&` pattern's HirId.
pub(crate) fn ref_pattern_above_at(tcx: &TyCtxt<'_>, binding: rustc_hir::HirId) -> Option<(rustc_hir::HirId, bool)> {
  let in_pattern = |n: rustc_hir::Node<'_>| matches!(n, rustc_hir::Node::Pat(_) | rustc_hir::Node::PatField(_));
  let mut cur = binding;
  loop {
    match tcx.parent_hir_node(cur) {
      rustc_hir::Node::Pat(p) => {
        if let PatKind::Ref(inner, _, _) = p.kind {
          let is_root = !in_pattern(tcx.parent_hir_node(p.hir_id));
          return (!is_root).then_some((p.hir_id, inner.hir_id != binding));
        }
        cur = p.hir_id;
      }
      // `S { f: &y }`: the field's pattern is under a PatField.
      rustc_hir::Node::PatField(f) => cur = f.hir_id,
      _ => return None,
    }
  }
}

/// `pat` is a plain by-value binding (`r`, `mut r`): not `ref r`, not
/// `r @ pat`.
/// A by-value binding of type `ty` copies a value inside the matched
/// `parent_ty` rather than all of it (`Some(x)` on an `Option<i32>`,
/// `Fruit::Apple(z)`): its copy is partial, and reads "a copy of a value
/// inside opt" (#866). Not through a reference: what `&x` copies is
/// behind `parent`, not inside it. Nor out of a call's result, which
/// is worded as "the value inside the Option f() returns" (#733).
/// `*b`, with `b` a Box drawn as its payload's part columns (`b.0`,
/// `b.1`; #602, #890): `b`. Moving it out (`let x = *b;`, `x => …` on
/// `match *b`) moves every part, while b keeps its heap allocation
/// (#898).
pub(crate) fn box_payload_of(r: &ResourceTy) -> Option<&ResourceAccessPoint> {
  match r {
    ResourceTy::Deref(b @ ResourceAccessPoint::Struct(Struct { is_box: true, is_member: false, .. })) => Some(b),
    _ => None,
  }
}

pub(crate) fn copies_from_inside<'tcx>(evt: &Evt, ty: Ty<'tcx>, parent_ty: Ty<'tcx>, parent: &ResourceTy) -> bool {
  matches!(evt, Evt::Copy) && !parent_ty.is_ref() && ty != parent_ty
    && !parent.extract_rap().is_some_and(|p| p.is_fn())
}

fn is_by_value_binding(pat: &Pat<'_>) -> bool {
  matches!(pat.kind, PatKind::Binding(ann, _, _, None) if matches!(ann.0, rustc_hir::ByRef::No))
}

/// The last of `rap`'s scope ends from line `from` on (up to `until`):
/// its block's end, not an early exit's (`return` in an arm drops it
/// there too).
fn last_gos_line_from(evs: &[(usize, ExternalEvent)], rap: &ResourceAccessPoint, from: usize, until: usize) -> Option<usize> {
  evs.iter().filter_map(|(l, e)| match e {
    ExternalEvent::GoOutOfScope { ro, .. } if ro.hash() == rap.hash() && *l >= from && *l <= until => Some(*l),
    // (Not an arm that leaves, whose exit drops it early.)
    ExternalEvent::Branch { branches, .. } => branches.iter().filter(|b| !b.diverges).filter_map(|b| last_gos_line_from(&b.e_data, rap, from, until)).max(),
    _ => None,
  }).max()
}

pub fn gos_line_from(evs: &[(usize, ExternalEvent)], rap: &ResourceAccessPoint, from: usize) -> Option<usize> {
  evs.iter().find_map(|(l, e)| match e {
    ExternalEvent::GoOutOfScope { ro, .. } if ro.hash() == rap.hash() && *l >= from => Some(*l),
    ExternalEvent::Branch { branches, .. } => branches.iter().find_map(|b| gos_line_from(&b.e_data, rap, from)),
    _ => None,
  })
}

/// Takes out of `events` (through nested branches), into `out`, the
/// moves and copies into a constructor among the events `ids` of its
/// arguments (`retarget_ctor_arg_events`, #1455).
fn take_ctor_arg_events_in(events: &mut Vec<(usize, ExternalEvent)>, ids: &std::ops::Range<usize>, hash: u64, out: &mut Vec<(usize, ExternalEvent)>, returned: &mut impl FnMut(usize) -> bool) {
  events.retain_mut(|(line, event)| {
    if let ExternalEvent::Branch { branches, .. } = event {
      for branch in branches.iter_mut() {
        take_ctor_arg_events_in(&mut branch.e_data, ids, hash, out, returned);
      }
    }
    let hit = ids.contains(&event.get_id()) && match event {
      ExternalEvent::Move { from, to, id, .. } | ExternalEvent::Copy { from, to, id, .. } =>
        (*to.hash() == hash || returned(*id)) && from.extract_rap().is_some(),
      _ => false,
    };
    if hit {
      out.push((*line, event.clone()));
    }
    !hit
  });
}

/// Whether statement `st` contains a `return`, `break`, `continue`, `?`
/// or an expression of type `!` (a panic), outside closures: a way out
/// of the branch it is in before the branch's value (#1501).
fn stmt_may_exit<'tcx>(tcx: TyCtxt<'tcx>, st: &'tcx rustc_hir::Stmt<'tcx>) -> bool {
  struct Exits<'tcx> { typeck: &'tcx rustc_middle::ty::TypeckResults<'tcx>, found: bool }
  impl<'tcx> rustc_hir::intravisit::Visitor<'tcx> for Exits<'tcx> {
    fn visit_expr(&mut self, e: &'tcx Expr<'tcx>) {
      if self.found {
        return;
      }
      match e.kind {
        ExprKind::Closure(..) => return,
        ExprKind::Ret(..) | ExprKind::Break(..) | ExprKind::Continue(..)
        | ExprKind::Match(_, _, rustc_hir::MatchSource::TryDesugar(_)) => {
          self.found = true;
          return;
        }
        _ => {}
      }
      if self.typeck.expr_ty_opt(e).is_some_and(|t| t.is_never()) {
        self.found = true;
        return;
      }
      rustc_hir::intravisit::walk_expr(self, e);
    }
  }
  let mut v = Exits { typeck: tcx.typeck(st.hir_id.owner), found: false };
  rustc_hir::intravisit::Visitor::visit_stmt(&mut v, st);
  v.found
}
