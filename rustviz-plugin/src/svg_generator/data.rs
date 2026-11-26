//! This file does the work of converting all the information we got from the backend:
//! a list of ExternalEvents to what we need for the frontend (timeline_panel.rs):  
//! a list of ExternalEvents updated with new line numbers if necessary (external_events)
//! and timeline information for each RAP. The timeline is populated with that RAP's personal 
//! history (Events) that help us annotate the dot events you see in the visualization as well as
//! compute the State of a RAP at any point in the program (which allows us to determine what line 
//! we should use when rendering it's respective timeline in the frontend).


use std::collections::{HashMap, HashSet, BTreeMap};
use std::vec::Vec;
use std::fmt::{Formatter, Result, Display};
use std::hash::{Hash, Hasher};
use std::cmp::Ordering;
use crate::svg_generator::{
  svg_frontend::timeline_panel::TimelineColumnData,
  hover_messages,
};
/*
 * Basic Data Structure Needed by Lifetime Visualization
 */
pub static LINE_SPACE: i64 = 30;
/// Baseline y of the first code row. Both panels hang off this one value
/// (the code rows, the timeline's row grid, its column-label row and the
/// default struct box), so they stay aligned. The one-line caption sits
/// one caption-line (20px) above it.
pub const FIRST_ROW_Y: i64 = 70;
/// Where the first row used to sit, under the old two-line caption.
pub const OLD_FIRST_ROW_Y: i64 = 90;

/// A conditional whose branches share source lines: all on one line
/// (`let s = if c { a } else { b };`), or with the join one line below
/// the split. There's no row between split and merge for the branch
/// columns, so they are drawn on the split..merge rows themselves (a
/// small diamond around them, see `render_branches`) instead of
/// between them (#117).
pub fn branch_is_collapsed(split_point: usize, merge_point: usize) -> bool {
    merge_point < split_point + 2
}

/// First and last row of a conditional's branch columns: the rows
/// strictly between the split and the join, or the split..join rows
/// themselves when there are none (`branch_is_collapsed`).
pub fn branch_column_rows(split_point: usize, merge_point: usize) -> (usize, usize) {
    if branch_is_collapsed(split_point, merge_point) {
        (split_point, merge_point.max(split_point))
    } else {
        (split_point + 1, merge_point - 1)
    }
}
// Top level Api that the Timeline object supports
pub trait Visualizable {
    // Function that computes states for variables inside of branches
    fn compute_branch_states(&self, history: & mut Vec<(usize, Event)>, 
                            states: &mut Vec<(usize, usize, State)>, 
                            hash: &u64, 
                            valid_range: (usize, usize), 
                            column_rows: (usize, usize), 
                            previous_state: State) -> State;

    // Computes states for variables outside of branches (ie global events)
    fn compute_timeline_states(&self, history: & mut Vec<(usize, Event)>, states: &mut Vec<(usize, usize, State)>, hash: &u64);

    // Top-level function that computes states of all RAPS and stores them inside their respective timelines
    fn compute_states(&mut self);

    // Appends Events to RAPs that were declared inside of branches
    fn append_decl_branch_events(&mut self, b_history: &Vec<(usize, ExternalEvent)>);

    // Appends Events to RAPs that are live inside of branches
    fn append_branch_event(&self, event: &ExternalEvent, line_number: usize, is: &ResourceTy, b_history: &mut Vec<(usize, Event)>);

    // Appends Events to 
    fn event_of_external_event(&self, line_num: usize, ext_ev: &ExternalEvent, to_o: bool) -> Vec<(usize, Event)>;

    // returns None if the hash does not exist
    fn get_name_from_hash(&self, hash: &u64) -> Option<String>;

    fn _append_event(&mut self, resource_access_point: &ResourceAccessPoint, event: Event, line_number: &usize);
    
    // add an event to the Visualizable data structure
    fn append_processed_external_event(&mut self, event: ExternalEvent, line_number: usize);
    
    // if resource_access_point with hash is mutable
    fn is_mut(&self, hash: &u64 ) -> bool;
    // if resource_access_point with hash is a function
    fn is_mutref(&self, hash: &u64) -> bool;

    fn is_ref(&self, hash: &u64) -> bool;

    fn calc_state(&self, previous_state: & State, event: & Event, event_line: usize, hash: &u64) -> State;
}


// Every object in Rust should belong in one of these catagories
// A ResourceAccessPoint is either an Owner, a reference, or a Function that
// have ownership to a memory object, during some stage of
// a the program execution.
// TODO: rework the Struct option: it really doesn't make sense that
// a struct should be different from an owner, maybe change an owner so that
// it can have possible children (members)
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub enum ResourceAccessPoint {
    Owner(Owner),
    MutRef(MutRef),
    StaticRef(StaticRef),
    Function(Function),
    Struct(Struct),
}

/// Marks the copy of a RAP carried by a move event (`drop(x)`) whose
/// argument is dropped by the call: the argument's type needs drop. Not
/// part of the RAP's identity: it compares equal and hashes to nothing,
/// so a flagged copy is still the same RAP as the one in the table.
#[derive(Clone, Copy, Default, Debug)]
pub struct DropsOld(pub bool);

impl PartialEq for DropsOld { fn eq(&self, _: &Self) -> bool { true } }
impl Eq for DropsOld {}
impl Hash for DropsOld { fn hash<H: Hasher>(&self, _: &mut H) {} }

/// What a write through a place (`*r = e`, `r.f = e`, `v[i] = e`) does to the place's
/// old value. Carried on the copy of the RAP a write event holds (and on
/// `ResourceTy::DerefField`), and used only to pick the dot's tooltip
/// (2026-11-05 decision: a place assignment reads like a plain
/// assignment). It is not part of the RAP's identity: it compares equal
/// and hashes to nothing, so a flagged copy is still the same RAP as the
/// one in the table.
#[derive(Clone, Copy, Default, Debug)]
pub enum PlaceWrite {
    /// An in-place update (`*r += 1`), or not an assignment at all.
    #[default]
    Update,
    /// An in-place update of what a reference stored in an element
    /// points to (`*slots[0] += 10`), where that pointee holds a value
    /// (Copy, or nothing to drop) rather than a resource (#396, #484).
    UpdateValue,
    /// `*r = e` with a Copy value.
    AssignCopy,
    /// `*r = e` with an owned value over an old value that needs drop.
    AssignDropsOld,
    /// `*r = e` with an owned value over an old value with nothing to drop.
    AssignOwned,
    /// `*b = e` on a Box whose old contents (which need drop) were
    /// moved out on every path (`let w = *b;`, #928): nothing is
    /// dropped.
    AssignOverMoved,
    /// `*b = e` on a Box whose old contents (which need drop) were
    /// moved out on some paths only (#928): a runtime drop flag decides
    /// whether they are dropped. `by_cond`: see `moved_above`.
    AssignMaybeMoved { by_cond: bool },
}

impl PlaceWrite {
    /// The write an assignment becomes when its operator is `op=`: an
    /// in-place update. Remembers whether the old value is a value (no
    /// drop glue) for the `*slots[0] += 10` wording.
    pub fn into_update(self) -> PlaceWrite {
        match self {
            PlaceWrite::AssignCopy | PlaceWrite::AssignOwned | PlaceWrite::UpdateValue => PlaceWrite::UpdateValue,
            _ => PlaceWrite::Update,
        }
    }
}

impl PartialEq for PlaceWrite { fn eq(&self, _: &Self) -> bool { true } }
impl Eq for PlaceWrite {}
impl Hash for PlaceWrite { fn hash<H: Hasher>(&self, _: &mut H) {} }

/// What a place without a column of its own (`ResourceTy::DerefField`:
/// `self.n`, `p.x` reached through a reference) holds: a "value" (a Copy
/// type, or one with nothing to drop) or a "resource" (#484). Tooltips
/// about borrowing the place use it: `let r = &self.n;` with `n: usize`
/// reads "self.n's value is immutably borrowed by r" (#1100). Like
/// `PlaceWrite`, it is fixed by the place's type and not part of its
/// identity: it compares equal and hashes to nothing.
#[derive(Clone, Copy, Default, Debug)]
pub struct FieldHolds {
    pub value: bool,
}

impl PartialEq for FieldHolds { fn eq(&self, _: &Self) -> bool { true } }
impl Eq for FieldHolds {}
impl Hash for FieldHolds { fn hash<H: Hasher>(&self, _: &mut H) {} }

/// Whether a type can be mutated through a shared reference: it
/// contains an `UnsafeCell` (`Cell`, `RefCell`, `Mutex`, atomics, ...)
/// inline or behind a pointer it owns or borrows. `Unknown` when part
/// of it is opaque (`dyn Trait`, a type parameter, `impl Trait`): such
/// a part could hold a cell, or not, so neither "it has interior
/// mutability" nor "it is read-only" may be said (#706).
#[derive(Clone, Copy, Hash, PartialEq, Eq, Debug, Default)]
pub enum InteriorMut {
    #[default]
    No,
    Yes,
    Unknown,
}

/// See `Owner::elems_left`: element names (`arr[0]`) in index order.
#[derive(Clone, Hash, PartialEq, Eq, Debug, Default)]
pub struct ElemsLeft {
    pub moved: Vec<String>,
    pub left: Vec<String>,
    /// When `left` is empty (patterns on several lines move every
    /// element, #1302): the elements whose move empties the array, on
    /// the last of those lines. Empty when no one move does (the moves
    /// are in different arms).
    pub last: Vec<String>,
}

impl ElemsLeft {
    /// Every element of the array, in index order.
    pub fn all(&self) -> Vec<String> {
        let mut all: Vec<String> = self.moved.iter().chain(self.left.iter()).cloned().collect();
        all.sort_by_key(|n| elem_index(n));
        all
    }

    /// Does moving `place` (`arr[1]`, or `arr[1..3]` for a `rest @ ..`)
    /// empty the array (`last`)?
    pub fn empties(&self, place: &str) -> bool {
        place_elem_names(place).iter().any(|n| self.last.contains(n))
    }

    /// Is `place` the last of the moves that empty the array (it takes
    /// the last element of `last`)? Its dot says nothing is left.
    pub fn empties_last(&self, place: &str) -> bool {
        self.last.last().is_some_and(|l| place_elem_names(place).contains(l))
    }
}

/// `arr[3]` -> 3.
fn elem_index(name: &str) -> u64 {
    name.rsplit('[').next().and_then(|t| t.trim_end_matches(']').parse().ok()).unwrap_or(0)
}

/// The element names a slice pattern's place stands for: `arr[1]` ->
/// [`arr[1]`]; `arr[1..3]` -> [`arr[1]`, `arr[2]`].
pub fn place_elem_names(place: &str) -> Vec<String> {
    let Some((base, idx)) = place.strip_suffix(']').and_then(|p| p.rsplit_once('[')) else { return vec![place.to_string()] };
    match idx.split_once("..") {
        Some((a, b)) => match (a.parse::<u64>(), b.parse::<u64>()) {
            (Ok(a), Ok(b)) => (a..b).map(|i| format!("{}[{}]", base, i)).collect(),
            _ => vec![place.to_string()],
        },
        None => vec![place.to_string()],
    }
}

// when something is not a reference
#[derive(Clone, Hash, PartialEq, Eq, Debug, Default)]
pub struct Owner {
    pub name: String,
    pub hash: u64,
    pub is_mut: bool,                     // let a = 42; vs let mut a = 42;
    /// True iff this owner's type implements Copy — drives the
    /// "drop-at-OOS" rendering. Copy types have no destructor, so
    /// going out of scope reclaims storage but doesn't run any
    /// drop glue; we surface that distinction visually.
    pub is_copy: bool,
    /// True iff this owner's type is not Copy but has nothing to drop
    /// (`ExprVisitor::ty_is_plain`): tooltips say "value", not
    /// "resource" (#484).
    pub is_plain_value: bool,
    /// Whether this owner's type can be mutated through a shared
    /// reference (`Cell`, `RefCell`, `Mutex`, `Rc<RefCell<T>>`, ...);
    /// see `ExprVisitor::ty_has_interior_mut`. An immutable borrow of
    /// such an owner doesn't make it read-only.
    pub interior_mut: InteriorMut,
    /// True iff this owner binds a closure literal (`let f = || ...`
    /// or `let f = move || ...`). Routes capture-related events
    /// (Move/StaticBorrow/MutableBorrow into `f`) and `f`'s scope-end
    /// to closure-flavoured tooltip text.
    pub is_closure: bool,
    /// True iff this owner's type is a `union`. Dropping or writing to
    /// a union never drops its fields (#191).
    pub is_union: bool,
    /// True iff this owner's type is `ManuallyDrop<T>`: it never drops
    /// the `T` inside it, at scope end or when overwritten (#283).
    pub is_manually_drop: bool,
    /// True iff this owner is a union (`is_union`) whose type has its
    /// own `Drop` impl: dropping or overwriting the whole union runs it,
    /// though its fields are still never dropped (#1146).
    pub union_drop: bool,
    /// True iff this owner's type is `MaybeUninit<T>`: like a
    /// `ManuallyDrop<T>`, it never drops the `T` inside it (#1146).
    pub is_maybe_uninit: bool,
    /// True iff this owner is an unbound temporary (see
    /// `ResourceAccessPoint::is_unbound_temporary`).
    pub unbound_temp: bool,
    /// True iff this owner is the temporary that the struct / tuple /
    /// array literal on the right of a `let` builds (#439), which the
    /// variables in the literal are moved into.
    pub let_literal_temp: bool,
    /// With `let_literal_temp`: the literal is on the right of a
    /// destructuring assignment (`(a, _) = (make(), make());`, which
    /// rustc lowers to a `let`; #1387), not of a `let`.
    pub assign_literal_temp: bool,
    /// For an unbound temporary that something is moved out of before
    /// it is dropped (`make_p().name`, a `match` scrutinee whose pattern
    /// binds by value): what that is; the rest is dropped with it (#469).
    pub temp_part: Option<TempPart>,
    /// True iff this unbound temporary is a tuple struct / variant
    /// constructor's value (`Some(make());`): what the constructor was
    /// given is moved into it (#783).
    pub ctor_temp: bool,
    /// For an unbound temporary that is the scrutinee of a `match` /
    /// `if let` / `while let` / let chain (and nothing is moved out of it
    /// by value): what examines it ("`match`") and when it is dropped,
    /// for the tooltip of what moved into it.
    pub scrutinee: Option<(String, String)>,
    /// Set only on the copy a write event carries (see `PlaceWrite`).
    pub write: PlaceWrite,
    /// True iff this owner isn't a reference but its value holds a
    /// borrow of another variable: `let it = v.iter();` (an
    /// `Iter<'_, i32>`), `let third = v.get(2);` (an `Option<&i32>`).
    /// Such a column receives a borrow arrow from what it borrows and
    /// gives it back at the borrow's end, like a reference, while
    /// still owning its own value (#336).
    pub holds_borrow: bool,
    /// `Some` iff this owner's own type is `Rc<T>`, `Arc<T>` or one of
    /// their `Weak<T>`s: dropping it lowers a count and frees the shared
    /// value only if it was the last owner (#373).
    pub rc: Option<RcKind>,
    /// `Some(RcDrop::Inside { .. })` iff this owner's type only contains
    /// `Rc`s / `Arc`s / `Weak`s inside std containers (#373).
    pub rc_inside: Option<RcDrop>,
    /// For a collection / `Option` / `Box` whose elements need drop: the
    /// phrase a drop of it adds, e.g. `every String it holds` (#542).
    pub holds: Option<String>,
    /// True iff this owner is a `Box<T>` whose whole contents are moved
    /// out somewhere in the body (`take(*b)`, #609): the contents' drop
    /// isn't the box's, but its heap allocation is still freed at scope
    /// end.
    pub box_contents_moved: bool,
    /// For an array of values that need drop from which a slice
    /// pattern moves some elements out but not all (`let [a, _] = arr;`,
    /// `match arr { [first, ..] => .. }`; #1144, #1166): the elements
    /// moved out and the ones left, which are dropped at scope end.
    pub elems_left: Option<ElemsLeft>,
    /// True iff this column is a `static` item the function uses (#690):
    /// it exists before the function runs and is never dropped.
    pub is_static: bool,
}

/// What is moved out of an unbound temporary before Rust drops it and
/// what is left in it (#469).
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub enum TempPart {
    /// `make_p().name`: this field (a path like `name` or `inner.name`)
    /// is moved out.
    Field(String),
    /// The scrutinee of a `match` whose arms' patterns bind parts of it
    /// by value. With a field path when the scrutinee is that field of
    /// the temporary (`match make_p().name { .. }`, #605).
    Match(Option<String>),
    /// The scrutinee of an `if let` whose pattern binds parts of it by
    /// value (or of that field of it).
    IfLet(Option<String>),
    /// The same for a `while let`.
    WhileLet(Option<String>),
    /// The same for a `let`-`else` (#605).
    LetElse(Option<String>),
    /// The same for a `let` in a let chain (`if a && let Ok(s) = make()`).
    LetChain(Option<String>),
    /// The same for a plain `let` whose pattern takes the value apart
    /// (`let (a, _) = pair();`, #623).
    Let(Option<String>),
    /// The same for a destructuring assignment (`(m, _) = pair();`,
    /// #625).
    Assign(Option<String>),
    /// The base of a struct update (`P { n: 1, ..make_p() }`): these
    /// fields, which the literal doesn't name, are moved out (#605).
    UpdateBase(Vec<String>),
    /// `make_q().name` as an element of an `if let` / `match` scrutinee
    /// tuple (`(z, make_q().name)`): the field (a path) is moved out into
    /// the temporary tuple (the literal's text) before any pattern is
    /// matched; the pattern takes it, if at all, from the tuple (#1350).
    TupleField(String, String),
}

/// Which reference-counted pointer an `Owner` is (found by the std
/// diagnostic items, not by name).
#[derive(Clone, Copy, Hash, PartialEq, Eq, Debug)]
pub enum RcKind { Rc, Arc, RcWeak, ArcWeak }

impl RcKind {
    pub fn is_weak(self) -> bool { matches!(self, RcKind::RcWeak | RcKind::ArcWeak) }
    /// The strong pointer's name: `Rc` or `Arc` (also for their Weaks'
    /// `upgrade` / `downgrade` counterparts).
    pub fn strong_name(self) -> &'static str {
        match self { RcKind::Rc | RcKind::RcWeak => "Rc", RcKind::Arc | RcKind::ArcWeak => "Arc" }
    }
}

/// What a drop claims about counts: the dropped place is itself an
/// `Rc` / `Arc` / `Weak`, or a struct all of whose droppable fields are.
#[derive(Clone, Copy, Hash, PartialEq, Eq, Debug)]
pub enum RcDrop {
    Own(RcKind),
    Fields(RcKind),
    /// The type wraps `Rc`s / `Arc`s / `Weak`s of one kind in std
    /// containers (`Option`, `Box`, tuples, `Vec`, ...) and owns nothing
    /// else that needs drop. `many`: more than one handle (or a
    /// collection of them); `buffer`: a `Vec` / `HashMap` / ... that also
    /// frees its own allocation.
    Inside { kind: RcKind, many: bool, buffer: bool },
}

/// A call that makes another owner of the same shared value, recognised
/// from the resolved callee (not its name): `<Rc<T> as Clone>::clone`
/// (also `Arc`), and `Rc::downgrade` / `Arc::downgrade` (#373).
#[derive(Clone, Copy, Hash, PartialEq, Eq, Debug)]
pub enum RcCall { Clone(RcKind), Downgrade(RcKind) }

// when something is a struct member
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct Struct {
    pub name: String,
    pub hash: u64,
    pub owner: u64,
    pub is_mut: bool,
    pub is_member: bool,
    /// Copy-ness of this struct (or member's type). Same role as
    /// Owner::is_copy.
    pub is_copy: bool,
    /// Same as Owner::is_plain_value.
    pub is_plain_value: bool,
    /// Same as Owner::interior_mut.
    pub interior_mut: InteriorMut,
    /// Same as Owner::is_manually_drop, for a `ManuallyDrop<T>` field.
    pub is_manually_drop: bool,
    /// Same as Owner::is_union, for a union-typed field (#1146).
    pub is_union: bool,
    /// Same as Owner::union_drop.
    pub union_drop: bool,
    /// Same as Owner::is_maybe_uninit, for a `MaybeUninit<T>` field.
    pub is_maybe_uninit: bool,
    /// `Some` iff this member's own type is `Rc<T>`, `Arc<T>` or one of
    /// their `Weak<T>`s (same role as `Owner::rc`).
    pub rc: Option<RcKind>,
    /// `Some(k)` iff this is a struct whose every field that needs drop
    /// is a `k`, and the struct has no `Drop` impl of its own: dropping
    /// it only lowers counts (#373).
    pub rc_fields: Option<RcKind>,
    /// Same as `Owner::rc_inside`.
    pub rc_inside: Option<RcDrop>,
    /// Same as `Owner::holds`.
    pub holds: Option<String>,
    /// A `Box<T>` drawn as a group of `T`'s field columns (#602): a
    /// field was moved out through the box, so the box is partially
    /// moved. Whatever fields remain, and the box's own heap
    /// allocation, are still freed when it goes out of scope.
    pub is_box: bool,
    /// Same as `Owner::holds_borrow`, for a member that isn't a
    /// reference but whose value holds a borrow (`items: Vec<&'a str>`
    /// initialised with `vec![w.as_str()]`, #1088).
    pub holds_borrow: bool,
}

// a reference of type &mut T
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct MutRef {         // let (mut) r1 = &mut a;
    pub name: String,
    pub hash: u64,
    pub is_mut: bool,
    /// True iff the referent type is Copy (`&mut i32`): the reference's
    /// own tooltips then say "value" rather than "resource", like the
    /// Copy column it borrows from (#312). Decided by the type, so it
    /// holds whichever lender the reference is (re)assigned from.
    pub pointee_copy: bool,
    /// Hash of the parent struct when this ref is a struct field
    /// (e.g. `Excerpt { p: &mut x }` makes p a MutRef whose
    /// member_of is Excerpt's hash). None for free-standing refs.
    pub member_of: Option<u64>,
    /// Set only on the copy a write event carries (see `PlaceWrite`).
    pub write: PlaceWrite,
}

// a reference of type & T
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct StaticRef {                // let (mut) r1 = & a;
    pub name: String,
    pub hash: u64,
    pub is_mut: bool,
    /// Same as MutRef::pointee_copy (`&i32`; #312).
    pub pointee_copy: bool,
    /// Same as MutRef::member_of — set when this ref is a struct
    /// field. Lets the layout pass include it in the parent's
    /// bounding box.
    pub member_of: Option<u64>,
}

#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct Function {
    pub name: String,
    pub hash: u64,
    /// Set only on the copy a move event carries when the callee is
    /// `std::mem::drop` and the argument's type needs drop (see
    /// `DropsOld`): the call is where the value is dropped.
    pub drops_arg: DropsOld,
    /// Set only on the copy a call's result event carries, when the
    /// resolved callee clones an `Rc` / `Arc` or downgrades one.
    pub rc_call: Option<RcCall>,
    /// Set only on the copy a call's result event carries, when the call
    /// is the operand of an `as` cast to another type (`v.len() as i32`):
    /// the value the variable gets is the call's result, converted to
    /// this type.
    pub cast_to: Option<String>,
    /// Set only on the copy a call's result event carries, when the
    /// variable gets a value from INSIDE what the call returns: `Some(m)
    /// = n.checked_mul(2)` or `parse()?`. "Option" / "Result" (a pattern
    /// on the return value), "Ok" / "Some" (`?`), or "" when the
    /// wrapper is not Option / Result.
    pub inside: Option<String>,
}

// ResourceTy is a wrapper for ResourceAccessPoints
// and what we use to populate an ExternalEvent. The wrapper is 
// necessary because it allows us to distinguish between the RAP being used as a
// Value (itself) or if its being dereferenced. 
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ResourceTy {
    Anonymous, // an anonymous resource holder
    Deref(ResourceAccessPoint), // Dereferencing a RAP
    // A field reached through a reference RAP: `p.x` / `self.c` /
    // `(*p).inner.v` where `p: &mut S`. Like `Deref`, the place is part
    // of the resource the reference points to (owned by its lender),
    // so the event is drawn on the reference's column. The String is
    // the place as written in the source, used in messages.
    // The PlaceWrite says whether the write is an assignment (and, if the
    // field's type needs drop, that the old value is dropped: the field
    // has no column to draw that on, so the write's tooltip says so).
    // The FieldHolds says whether the place holds a value (a Copy type,
    // or one with nothing to drop) rather than a resource (#1100).
    DerefField(ResourceAccessPoint, String, PlaceWrite, FieldHolds),
    Caller, // For expressing relationship between return expr and fn
    Value(ResourceAccessPoint),
    // A write to an element of the RAP's resource: the LHS of
    // `v[i] = x` / `a[i] += 1` / `r[i] = x`. Events land on the RAP's
    // column (elements have none of their own), but the RAP neither
    // acquires a new resource nor drops its old one; one element is
    // overwritten. The String is the place as written (`v[i]`), used
    // in tooltips and arrow labels. The PlaceWrite says whether this is
    // an assignment (and what it does to the old element: the element has
    // no column to draw a drop on, so the tooltip states it) or an
    // in-place update.
    Index(ResourceAccessPoint, String, PlaceWrite),
    // The temporary a literal in `let (ref a, ..) = (s, t);` builds: an
    // anonymous holder like `Anonymous`, but one whose drop is not
    // drawn (it has no column), so the move into it says when it is
    // dropped: at the end of the statement, or with a `ref` binding
    // (`extended`) at the end of the block. Only used for a value
    // whose type needs drop.
    //
    // `tuple`: instead, the tuple an `if let` / `match` scrutinee
    // `(x, y)` builds (#327), named by its source text (empty when too
    // long to quote). The move into it doesn't say when it's dropped:
    // a TupleTempDrop dot on the variable's column does, where the
    // tuple is dropped.
    Temporary { extended: bool, tuple: Option<String> },
}

impl ResourceTy {
    pub fn name(&self) -> String {
        match self {
            ResourceTy::Anonymous | ResourceTy::Temporary { .. } => "Anonymous resource".to_owned(),
            ResourceTy::Value(r) => r.name().to_owned(),
            ResourceTy::Deref(r) => format!("*{}", r.name()),
            ResourceTy::Index(_, place, _) => place.clone(),
            ResourceTy::DerefField(_, place, _, _) => hover_messages::displayed_place(place),
            ResourceTy::Caller => "Caller".to_owned()
        }
    }

    pub fn real_name(&self) -> String {
        match self {
            ResourceTy::Anonymous | ResourceTy::Temporary { .. } => "Anonymous resource".to_owned(),
            ResourceTy::Value(r) | ResourceTy::Deref(r) | ResourceTy::Index(r, _, _) | ResourceTy::DerefField(r, _, _, _) => r.name().to_owned(),
            ResourceTy::Caller => "Caller".to_owned()
        }
    }

    pub fn hash(&self) -> &u64 {
        match self {
            ResourceTy::Caller | ResourceTy::Anonymous | ResourceTy::Temporary { .. } => &std::u64::MAX,
            ResourceTy::Value(r) | ResourceTy::Deref(r) | ResourceTy::Index(r, _, _) | ResourceTy::DerefField(r, _, _, _) => r.hash(),
        }
    }

    pub fn is_ref(&self) -> bool {
      match self {
        ResourceTy::Value(r) | ResourceTy::Deref(r) | ResourceTy::Index(r, _, _) | ResourceTy::DerefField(r, _, _, _) => r.is_ref(),
        _ => false
      }
    }

    pub fn is_mutref(&self) -> bool {
        match self {
            ResourceTy::Value(r) | ResourceTy::Deref(r) | ResourceTy::Index(r, _, _) | ResourceTy::DerefField(r, _, _, _) => r.is_mutref(),
            _ => false
          }
    }

    pub fn is_closure(&self) -> bool {
        match self {
            ResourceTy::Value(r) | ResourceTy::Deref(r) | ResourceTy::Index(r, _, _) | ResourceTy::DerefField(r, _, _, _) => r.is_closure(),
            _ => false
        }
    }

    /// A non-reference owner whose value holds a borrow (#336). Only
    /// the `Value` form: `*h` isn't the holder itself.
    pub fn holds_borrow(&self) -> bool {
        matches!(self, ResourceTy::Value(r) if r.holds_borrow())
    }

    pub fn extract_rap(&self) -> Option<&ResourceAccessPoint> {
        match self {
            ResourceTy::Anonymous | ResourceTy::Caller | ResourceTy::Temporary { .. } => None,
            ResourceTy::Deref(r) | ResourceTy::Value(r) | ResourceTy::Index(r, _, _) | ResourceTy::DerefField(r, _, _, _) => Some(r)
        }
    }

    /// The reference RAP a write through `*r` / `r.f` goes through.
    pub fn deref_rap(&self) -> Option<&ResourceAccessPoint> {
        match self {
            ResourceTy::Deref(r) | ResourceTy::DerefField(r, _, _, _) => Some(r),
            _ => None,
        }
    }

    pub fn is_same_underlying(&self, other: &ResourceTy) -> bool{
      match (self.extract_rap(), other.extract_rap()) {
        (Some(r), Some(o)) => {
          r.hash() == o.hash()
        }
        _ => false
      }
    }

    
}

impl Hash for ResourceTy {
  fn hash<H: Hasher>(&self, state: &mut H) {
    self.real_name().hash(state);
  }
}

/// Separates the parts of a place label that names the variables a write
/// may go into (`place_with_alternatives`). Not in any source text.
const ALT_SEP: char = '\u{1f}';
const ALT_ITEM_SEP: char = '\u{1e}';

/// The label of a write through `ptr` (`x` in `*x += 1`) drawn on each
/// of `holders`' columns because x points into one of them, which one
/// depending on the iteration (`for x in a.iter_mut().chain(b.iter_mut())`;
/// #1082): the place as written, then x, then each holder with whether
/// it is a reference and whether what it holds (or points to) is a value.
/// `split_alternatives` reads it back; only the dot's tooltip uses them.
pub fn place_with_alternatives(place: &str, ptr: &str, holders: &[ResourceAccessPoint]) -> String {
    let alts: Vec<String> = holders.iter().map(|h| {
        let (is_ref, value) = if h.is_ref() { (true, h.points_to_copy()) } else { (false, h.holds_value()) };
        format!("{}{}{}", if is_ref { 'r' } else { 'o' }, if value { 'v' } else { 'r' }, h.name())
    }).collect();
    format!("{place}{ALT_SEP}{ptr}{ALT_SEP}{}", alts.join(&ALT_ITEM_SEP.to_string()))
}

/// A place label without the alternatives `place_with_alternatives`
/// added, and those: the reference written through, and each holder
/// (name, is a reference, holds a value).
pub fn split_alternatives(place: &str) -> (&str, Option<(&str, Vec<(&str, bool, bool)>)>) {
    let mut parts = place.splitn(3, ALT_SEP);
    let label = parts.next().unwrap_or(place);
    match (parts.next(), parts.next()) {
        (Some(ptr), Some(alts)) => {
            let alts = alts.split(ALT_ITEM_SEP).filter_map(|a| {
                let mut cs = a.chars();
                let (k, v) = (cs.next()?, cs.next()?);
                Some((cs.as_str(), k == 'r', v == 'v'))
            }).collect();
            (label, Some((ptr, alts)))
        }
        _ => (label, None),
    }
}

/// The tooltip `msg` of a write drawn on r's column, when the reference
/// written through may point into any of `alts` (`split_alternatives`;
/// #1082): it says which ones, and that the write goes into r's only on
/// the iterations where it points there.
fn with_alternatives(msg: String, r: &ResourceAccessPoint, place: &str, write: &PlaceWrite, alts: Option<(&str, Vec<(&str, bool, bool)>)>) -> String {
    let Some((ptr, alts)) = alts else { return msg };
    let ptr = ptr.to_owned();
    let alts: Vec<(String, bool, bool)> = alts.into_iter().map(|(n, is_ref, value)| (n.to_owned(), is_ref, value)).collect();
    match write {
        PlaceWrite::Update | PlaceWrite::UpdateValue =>
            hover_messages::event_dot_write_element_one_of(r.name(), &place.to_owned(), r.is_ref(), element_update_holds_value(r, place, write), &ptr, &alts),
        _ => hover_messages::event_dot_write_one_of(msg, r.name(), r.is_ref(), if r.is_ref() { r.points_to_copy() } else { r.holds_value() }, &ptr, &alts),
    }
}

/// Dot text for a Copy value assigned to the place `place`, worded as a
/// plain assignment is on main: "a copy of x" when the source is a named
/// place (`*r = y`, `v[i] = x`), "a new value" otherwise (a literal, an
/// expression, a call result).
/// Whether an in-place update of `place` (`v[i] += k`, `*slots[0] += 10`)
/// changes a value rather than a resource: for a place through a stored
/// reference, whether its pointee needs no drop; otherwise whether the
/// holder does.
fn element_update_holds_value(r: &ResourceAccessPoint, place: &str, write: &PlaceWrite) -> bool {
    if place.starts_with('*') {
        matches!(write, PlaceWrite::UpdateValue)
    } else if r.is_ref() {
        r.points_to_copy()
    } else {
        r.holds_value()
    }
}

/// Appends `event` to a column's history. A write through a reference
/// to part of v's resource, drawn on v's column (#861), goes before the
/// end of that reference's borrow of v on the same line, which may be
/// recorded first: the write is done through the reference. That is
/// the call's result in `*v.get_mut(0).unwrap() += 1` and
/// `v.last_mut().unwrap().n = 5` (the call's events end its borrow of v,
/// "v's resource is no longer mutably borrowed", as one event), and x
/// in `*x += 1` as x's last use.
/// A borrow end with no arrow (`RefDie`) of a reference declared in
/// an arm, which the renderer moved into that arm (`nest_arm_ref_ends`,
/// #1277): on the reference's column, as at the top level.
/// `append_branch_event` leaves RefDies out (`ResourceAccessPoint_extract`
/// names neither side of one).
/// Its kind comes from the borrow recorded for it in this arm so far,
/// which isn't on its column yet: a shared reference made from a
/// mutable borrow (`let r3 = downgrade(&mut s2);`) ends a mutable one
/// (#1571).
fn append_decl_ref_die(vd: &VisualizationData, e: &ExternalEvent, line: usize, var: &ResourceAccessPoint, b_history: &mut Vec<(usize, Event)>) {
    if let ExternalEvent::RefDie { from: from_ro @ ResourceTy::Value(r), id, .. } = e {
        if r == var {
            if !r.is_mutref() && history_holds_mut_borrow(vd, b_history, r, line, 4) {
                b_history.push((line, Event::MutableDie { to: ResourceTy::Anonymous, is: from_ro.clone(), id: *id }));
            } else {
                b_history.extend(vd.event_of_external_event(line, e, false));
            }
        }
    }
}

fn push_history_event(history: &mut Vec<(usize, Event)>, line: usize, event: Event) {
    if let Event::Acquire { is: ResourceTy::Index(_, place, _), .. } = &event {
        let place = split_alternatives(place).0;
        let ends_through = |e: &Event| match e {
            Event::MutableReacquire { from: ResourceTy::Value(f), .. } if f.is_fn() =>
                place.contains(&format!(".{}(", f.name())),
            Event::MutableReacquire { from: ResourceTy::Value(r), .. } if r.is_mutref() =>
                place.trim_start_matches('*') == r.name(),
            _ => false,
        };
        // The borrow ends recorded last on this line (another borrow of
        // v ending there too, `it` for `first` from `it.next()`, is
        // drawn after the write as well).
        let run = history.iter().rev()
            .take_while(|(l, e)| *l == line && matches!(e, Event::MutableReacquire { .. }))
            .count();
        let start = history.len() - run;
        if history[start..].iter().any(|(_, e)| ends_through(e)) {
            history.insert(start, (line, event));
            return;
        }
    }
    history.push((line, event));
}

pub fn place_assign_copy_message(place: &String, from: &ResourceTy) -> String {
    match from.extract_rap().filter(|f| !f.is_fn()) {
        Some(_) => hover_messages::event_dot_copyable_assign_copy(place, &from.name()),
        None => hover_messages::event_dot_copyable_assign(place),
    }
}

/// Column label of the n-th (1-based) temporary in a fn that a `let`
/// borrows and so keeps alive to the end of the block (#400):
/// `(temporary)`, then `(temporary 2)`, … Not a Rust identifier, so it
/// can't clash with a variable's name. "temporary" is the word rustc's
/// own errors use ("temporary value dropped while borrowed").
pub fn temporary_label(n: usize) -> String {
    match n {
        1 => "(temporary)".to_owned(),
        n => format!("(temporary {})", n),
    }
}

/// Column label of the n-th (1-based) by-value parameter value that a
/// `ref` binding borrows (`fn f(ref s: String)`, #691): `(parameter)`,
/// then `(parameter 2)`, … Not a Rust identifier, like `temporary_label`.
pub fn parameter_label(n: usize) -> String {
    match n {
        1 => "(parameter)".to_owned(),
        n => format!("(parameter {})", n),
    }
}

/// Is `evs` a line of arrows that all run between a `(parameter)` column
/// (#691) and the `ref` binding that borrows it: the borrows on the
/// signature's line, or their returns at the binding's last use? Those
/// arrows of different parameters sit in different columns and never
/// overlap, so they share one level instead of stacking a row each, and
/// the borrows (not the returns) run half a row below the line's row,
/// clear of the arrow bringing the caller's value into the column (#747).
pub fn is_param_value_line(evs: &[ExternalEvent]) -> bool {
    !evs.is_empty() && evs.iter().all(|e| {
        matches!(e, ExternalEvent::StaticBorrow { .. } | ExternalEvent::MutableBorrow { .. }
            | ExternalEvent::StaticDie { .. } | ExternalEvent::MutableDie { .. })
            && {
                let (from, to) = ResourceAccessPoint_extract(e);
                let is_col = |t: &ResourceTy| matches!(t, ResourceTy::Value(r) if matches!(r, ResourceAccessPoint::Owner(_)) && r.name().starts_with("(parameter"));
                is_col(from) || is_col(to)
            }
    })
}

impl ResourceAccessPoint {
    /// `Some` iff this RAP is an owner whose own type is `Rc` / `Arc` /
    /// `Weak`.
    pub fn rc_kind(&self) -> Option<RcKind> {
        match self {
            ResourceAccessPoint::Owner(Owner { rc, .. })
            | ResourceAccessPoint::Struct(Struct { rc, .. }) => *rc,
            _ => None,
        }
    }

    /// What a drop of this RAP should say about reference counts, if
    /// anything (#373): its own type is `Rc` / `Arc` / `Weak`, or it is a
    /// struct whose drop only drops such fields.
    pub fn rc_drop(&self) -> Option<RcDrop> {
        match self {
            ResourceAccessPoint::Struct(Struct { rc_fields: Some(k), .. }) => Some(RcDrop::Fields(*k)),
            ResourceAccessPoint::Owner(Owner { rc_inside: Some(d), .. })
            | ResourceAccessPoint::Struct(Struct { rc_inside: Some(d), .. }) => Some(*d),
            _ => self.rc_kind().map(RcDrop::Own),
        }
    }

    /// The parenthetical a drop of this RAP appends: the reference-count
    /// note (#373) if it has one, else what its elements are (#542),
    /// else empty.
    pub fn drop_note(&self) -> String {
        if let Some(rc) = self.rc_drop() {
            return hover_messages::rc_drop_note(Some(rc));
        }
        match self {
            ResourceAccessPoint::Owner(Owner { holds: Some(h), .. })
            | ResourceAccessPoint::Struct(Struct { holds: Some(h), .. }) =>
                format!(" (including {})", hover_messages::escape_tooltip_name(h)),
            _ => String::new(),
        }
    }

    /// A copy of this function RAP marked as a call that clones or
    /// downgrades an `Rc` / `Arc`.
    pub fn with_rc_call(&self, call: RcCall) -> ResourceAccessPoint {
        let mut rap = self.clone();
        if let ResourceAccessPoint::Function(f) = &mut rap {
            f.rc_call = Some(call);
        }
        rap
    }

    /// A copy of this function RAP whose result is taken apart (a
    /// pattern or `?`) before the variable gets its value.
    pub fn with_inside(&self, wrapper: String) -> ResourceAccessPoint {
        let mut rap = self.clone();
        if let ResourceAccessPoint::Function(f) = &mut rap {
            f.inside = Some(wrapper);
        }
        rap
    }

    /// A copy of this function RAP whose result is cast to `ty`.
    pub fn with_cast_to(&self, ty: String) -> ResourceAccessPoint {
        let mut rap = self.clone();
        if let ResourceAccessPoint::Function(f) = &mut rap {
            f.cast_to = Some(ty);
        }
        rap
    }

    /// A copy of this function RAP marked as `std::mem::drop` called on
    /// a value that needs drop.
    pub fn with_drops_arg(&self) -> ResourceAccessPoint {
        let mut rap = self.clone();
        if let ResourceAccessPoint::Function(f) = &mut rap {
            f.drops_arg = DropsOld(true);
        }
        rap
    }

    /// A copy of this RAP marked as the target of a place write `*r = e`
    /// (see `PlaceWrite`). Only owners (a `Box`) and `&mut` references
    /// can be written through.
    pub fn with_place_write(&self, write: PlaceWrite) -> ResourceAccessPoint {
        let mut rap = self.clone();
        match &mut rap {
            ResourceAccessPoint::Owner(o) => o.write = write,
            ResourceAccessPoint::MutRef(m) => m.write = write,
            _ => {}
        }
        rap
    }

    pub fn place_write(&self) -> PlaceWrite {
        match self {
            ResourceAccessPoint::Owner(o) => o.write,
            ResourceAccessPoint::MutRef(m) => m.write,
            _ => PlaceWrite::Update,
        }
    }

    /// See `Owner::holds`: what dropping this also drops, e.g. `the
    /// String it holds`.
    pub fn holds_phrase(&self) -> Option<&str> {
        match self {
            ResourceAccessPoint::Owner(Owner { holds: Some(h), .. })
            | ResourceAccessPoint::Struct(Struct { holds: Some(h), .. }) => Some(h),
            _ => None,
        }
    }

    /// True iff this RAP is an unbound temporary (#457): a call's
    /// result that nothing binds, dropped by an event of its own. It
    /// is labelled like a lifetime-extended one (`temporary_label`),
    /// so the label can't tell the two apart.
    /// See `Owner::box_contents_moved`.
    pub fn box_contents_moved(&self) -> bool {
        matches!(self, ResourceAccessPoint::Owner(Owner { box_contents_moved: true, .. }))
    }

    /// See `Owner::elems_left`.
    pub fn elems_left(&self) -> Option<&ElemsLeft> {
        match self {
            ResourceAccessPoint::Owner(Owner { elems_left: Some(e), .. }) => Some(e),
            _ => None,
        }
    }

    pub fn is_unbound_temporary(&self) -> bool {
        matches!(self, ResourceAccessPoint::Owner(Owner { unbound_temp: true, .. }))
    }

    /// True iff this RAP is the temporary of a `match` / `if let` /
    /// `while let` / let-chain scrutinee that a pattern can move
    /// something out of: its column forks into the arms (#1386).
    /// So is one nothing is moved out of, which a `ref` binding may
    /// borrow from.
    pub fn is_split_scrutinee_temporary(&self) -> bool {
        matches!(self, ResourceAccessPoint::Owner(Owner {
            unbound_temp: true,
            temp_part: Some(TempPart::Match(_) | TempPart::IfLet(_) | TempPart::WhileLet(_) | TempPart::LetChain(_)),
            ..
        }) | ResourceAccessPoint::Owner(Owner { unbound_temp: true, temp_part: None, scrutinee: Some(_), .. }))
    }

    /// True iff this RAP is the temporary a literal on the right of a
    /// `let` builds (#439); see `Owner::let_literal_temp`.
    pub fn is_let_literal_temporary(&self) -> bool {
        matches!(self, ResourceAccessPoint::Owner(Owner { let_literal_temp: true, .. }))
    }

    /// True iff this RAP is the temporary a literal on the right of a
    /// destructuring assignment builds (#1387); see
    /// `Owner::assign_literal_temp`.
    pub fn is_assign_literal_temporary(&self) -> bool {
        matches!(self, ResourceAccessPoint::Owner(Owner { let_literal_temp: true, assign_literal_temp: true, .. }))
    }

    /// See `Owner::temp_part`.
    pub fn temp_part(&self) -> Option<&TempPart> {
        match self {
            ResourceAccessPoint::Owner(Owner { temp_part, .. }) => temp_part.as_ref(),
            _ => None,
        }
    }

    /// True iff this RAP is a lifetime-extended temporary (see
    /// `temporary_label`), not a variable.
    pub fn is_temporary(&self) -> bool {
        let n = self.name();
        n == "(temporary)" || (n.starts_with("(temporary ") && n.ends_with(')'))
    }

    // get the attribute hash
    pub fn hash(&self) -> &u64 {
        match self {
            ResourceAccessPoint::Owner(Owner{hash, ..}) => hash,
            ResourceAccessPoint::Struct(Struct{hash, ..}) => hash,
            ResourceAccessPoint::MutRef(MutRef{hash, ..}) => hash,
            ResourceAccessPoint::StaticRef(StaticRef{hash, ..}) => hash,
            ResourceAccessPoint::Function(Function{hash, ..}) => hash,
        }
    }

    // get the name field
    pub fn name(&self) -> &String {
        match self {
            ResourceAccessPoint::Owner(Owner{name, ..}) => name,
            ResourceAccessPoint::Struct(Struct{name, ..}) => name,
            ResourceAccessPoint::MutRef(MutRef{name, ..}) => name,
            ResourceAccessPoint::StaticRef(StaticRef{name, ..}) => name,
            ResourceAccessPoint::Function(Function{name, ..}) => name,
        }
    }

    // get the is_mut field, if any
    pub fn is_mut(&self) -> bool {
        match self {
            ResourceAccessPoint::Owner(Owner{is_mut, ..}) => *is_mut,
            ResourceAccessPoint::Struct(Struct{is_mut, ..}) => *is_mut,
            ResourceAccessPoint::MutRef(MutRef{is_mut, ..}) => *is_mut,
            ResourceAccessPoint::StaticRef(StaticRef{is_mut, ..}) => *is_mut,
            ResourceAccessPoint::Function(_) => false,
        }
    }

    pub fn is_ref(&self) -> bool {
        match self {
            ResourceAccessPoint::MutRef(_) | ResourceAccessPoint::StaticRef(_) => true,
            _ => false
        }
    }

    /// True iff dropping this binding never drops a resource. That holds
    /// when its type is `Copy` (no drop glue), for any reference (`&T` or
    /// `&mut T`: a reference never owns what it points to, so nothing is
    /// dropped when it goes out of scope, although `&mut T` is not itself
    /// `Copy`), and for functions, which model no resource at all. Not
    /// the same as "has a `Copy` type": see `is_copy_owner`.
    pub fn never_drops(&self) -> bool {
        match self {
            ResourceAccessPoint::Owner(Owner{is_copy, is_static, ..}) => *is_copy || *is_static,
            ResourceAccessPoint::Struct(Struct{is_copy, ..}) => *is_copy,
            ResourceAccessPoint::MutRef(_) | ResourceAccessPoint::StaticRef(_) => true,
            ResourceAccessPoint::Function(_) => true,
        }
    }

    /// True iff nothing is ever dropped when this binding's value is:
    /// `never_drops`, a non-Copy value with no drop glue (#484: a plain
    /// struct, a closure that captures no resource), or a `ManuallyDrop`
    /// (#283). A drop dot that says "only if ..., its resource is
    /// dropped" has nothing to say for it (#1137).
    ///
    /// Also a `MaybeUninit` and a union without a `Drop` impl of its own
    /// (#1146): neither ever drops what it holds.
    pub fn drops_nothing(&self) -> bool {
        self.never_drops() || self.is_plain_value() || self.is_manually_drop()
            || self.is_maybe_uninit() || (self.is_union() && !self.union_drop())
    }

    pub fn is_mutref(&self) -> bool {
        match self {
            ResourceAccessPoint::MutRef(_) => true,
            _ => false
        }
    }

    /// True iff this RAP is the binding of a closure literal — used
    /// by the tooltip layer to swap in capture-flavoured wording for
    /// events that flow into or out of a closure.
    pub fn is_closure(&self) -> bool {
        matches!(self, ResourceAccessPoint::Owner(Owner { is_closure: true, .. }))
    }

    /// True iff this RAP is a `static` item's column (`Owner::is_static`).
    pub fn is_static(&self) -> bool {
        matches!(self, ResourceAccessPoint::Owner(Owner { is_static: true, .. }))
    }

    /// True iff this RAP is a non-reference owner, or a struct member
    /// drawn as one column, whose value holds a borrow (see
    /// `Owner::holds_borrow`, `Struct::holds_borrow`).
    pub fn holds_borrow(&self) -> bool {
        matches!(self, ResourceAccessPoint::Owner(Owner { holds_borrow: true, .. })
            | ResourceAccessPoint::Struct(Struct { holds_borrow: true, .. }))
    }

    /// True iff this RAP is an owner of a Copy type (i32 etc.). The
    /// "ownership" framing in tooltip text doesn't fit primitives —
    /// they're values, not heap resources — so this lets the tooltip
    /// dispatcher pick non-ownership wording where appropriate.
    pub fn has_interior_mut(&self) -> bool {
        self.interior_mut() == InteriorMut::Yes
    }

    /// Not known to be free of interior mutability: it has some, or
    /// part of its type is opaque (see `InteriorMut::Unknown`).
    pub fn may_have_interior_mut(&self) -> bool {
        self.interior_mut() != InteriorMut::No
    }

    fn interior_mut(&self) -> InteriorMut {
        match self {
            ResourceAccessPoint::Owner(Owner { interior_mut, .. })
            | ResourceAccessPoint::Struct(Struct { interior_mut, .. }) => *interior_mut,
            _ => InteriorMut::No,
        }
    }

    pub fn is_union(&self) -> bool {
        matches!(self, ResourceAccessPoint::Owner(Owner { is_union: true, .. })
                     | ResourceAccessPoint::Struct(Struct { is_union: true, .. }))
    }

    /// True iff this is a union whose type implements `Drop` (#1146).
    pub fn union_drop(&self) -> bool {
        matches!(self, ResourceAccessPoint::Owner(Owner { union_drop: true, .. })
                     | ResourceAccessPoint::Struct(Struct { union_drop: true, .. }))
    }

    /// True iff this owner's (or field's) type is `MaybeUninit<T>` (#1146).
    pub fn is_maybe_uninit(&self) -> bool {
        matches!(self, ResourceAccessPoint::Owner(Owner { is_maybe_uninit: true, .. })
                     | ResourceAccessPoint::Struct(Struct { is_maybe_uninit: true, .. }))
    }

    pub fn is_manually_drop(&self) -> bool {
        matches!(self, ResourceAccessPoint::Owner(Owner { is_manually_drop: true, .. })
                     | ResourceAccessPoint::Struct(Struct { is_manually_drop: true, .. }))
    }

    /// True iff this is a reference whose referent type is Copy
    /// (`&i32`, `&mut bool`): what it points to is a "value" (#312).
    pub fn points_to_copy(&self) -> bool {
        matches!(self, ResourceAccessPoint::MutRef(MutRef { pointee_copy: true, .. })
                     | ResourceAccessPoint::StaticRef(StaticRef { pointee_copy: true, .. }))
    }

    /// True iff this is an owner (or struct / field) whose type isn't
    /// Copy but has no drop glue (#484).
    pub fn is_plain_value(&self) -> bool {
        matches!(self, ResourceAccessPoint::Owner(Owner { is_plain_value: true, .. })
                     | ResourceAccessPoint::Struct(Struct { is_plain_value: true, .. }))
    }

    /// True iff what this owner holds is called a "value" in tooltips:
    /// a Copy type, or a non-Copy one with nothing to drop.
    pub fn holds_value(&self) -> bool {
        self.is_copy_owner() || self.is_plain_value()
    }

    pub fn is_copy_owner(&self) -> bool {
        // A Copy struct or Copy field (`p.x: i32`) holds a value like a
        // Copy owner does (#178).
        matches!(self, ResourceAccessPoint::Owner(Owner { is_copy: true, .. })
                     | ResourceAccessPoint::Struct(Struct { is_copy: true, .. }))
    }

    /// True if this RAP is part of a struct grouping — either the
    /// struct itself or any of its fields (Struct fields and
    /// ref-typed fields modelled as MutRef/StaticRef with
    /// `member_of` set).
    pub fn is_struct_group(&self) -> bool {
        match self {
            ResourceAccessPoint::Struct(_) => true,
            ResourceAccessPoint::MutRef(MutRef{member_of: Some(_), ..})
            | ResourceAccessPoint::StaticRef(StaticRef{member_of: Some(_), ..}) => true,
            _ => false,
        }
    }

    pub fn is_struct(&self) -> bool {
        match self {
            ResourceAccessPoint::Struct(Struct{is_member, ..}) => !is_member,
            _ => false
        }
    }

    pub fn is_member(&self) -> bool {
        match self {
            ResourceAccessPoint::Struct(Struct{is_member, ..}) => *is_member,
            ResourceAccessPoint::MutRef(MutRef{member_of, ..})
            | ResourceAccessPoint::StaticRef(StaticRef{member_of, ..}) => member_of.is_some(),
            _ => false,
        }
    }

    pub fn get_owner(&self) -> u64 {
        match self {
            ResourceAccessPoint::Owner(Owner{hash, ..}) => hash.to_owned(),
            ResourceAccessPoint::Struct(Struct{owner, ..}) => owner.to_owned(),
            ResourceAccessPoint::MutRef(MutRef{hash, member_of, ..}) =>
                member_of.unwrap_or(*hash),
            ResourceAccessPoint::StaticRef(StaticRef{hash, member_of, ..}) =>
                member_of.unwrap_or(*hash),
            ResourceAccessPoint::Function(Function{hash, ..}) => hash.to_owned(),
        }
    }

    pub fn is_fn(&self) -> bool {
        match self {
            ResourceAccessPoint::Function(_) => true,
            _ => false
        }
    }
}

// 1. A list of the Branch names (we need these for the diagram)
//    For example: 
//      match a {
//        Some(_) => ...
//        None => ..
//      }
// Would have branch names ["Some", "None"]
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum BranchType {
  If(Vec<String>, Vec<(usize, usize)>), 
  Loop(Vec<String>, Vec<(usize, usize)>),
  Match(Vec<String>, Vec<(usize, usize)>)
}

/// The label of an `if let`'s first arm (the one its pattern matched),
/// in place of "If".
pub const IF_LET_ARM: &str = "If let";

impl BranchType {
    pub fn get_start_end(&self, index: usize) -> (usize, usize) {
        match self {
            BranchType::If(_, v) 
            | BranchType::Loop(_, v) 
            | BranchType::Match(_, v) => {
                v.get(index).unwrap().to_owned()
            }
        }
    }
    pub fn get_mut_start_end(& mut self, index: usize) -> & mut (usize, usize) {
        match self {
            BranchType::If(_, v) 
            | BranchType::Loop(_, v) 
            | BranchType::Match(_, v) => {
                v.get_mut(index).unwrap()
            }
        }
    }
    pub fn string_of_branch(&self, index: usize) -> String {
      match self {
        BranchType::If(x, _) | BranchType::Loop(x, _) | BranchType::Match(x,_) => {
            x.get(index).unwrap().clone()
        }
    }
    }
}



#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtBranchData {
    // Event Data: a list of External Events
    pub e_data: Vec<(usize, ExternalEvent)>,

    // Mirrors the global Event line map
    pub line_map: BTreeMap<usize, Vec<ExternalEvent>>,

    // Variables declared within the branch
    pub decl_vars: HashSet<ResourceAccessPoint>,

    // The branch's body has type `!` (`unreachable!()`, `panic!()`,
    // `return`, `continue`, `break`): it never reaches the merge
    // point, so it contributes nothing to the state after the
    // conditional (#296).
    pub diverges: bool,

    // How a diverging branch leaves (empty when it reaches the merge).
    // Used for the merge dot's wording, and to tell whether a value it
    // gave a variable can reach the code after the loop it breaks out of.
    pub exits: Vec<BranchExit>,

    // The variables the arm's `break value`s out of a loop mention
    // (sorted): the loop's value carries their borrows out of it, so a
    // borrow of theirs doesn't end at the `break` (#662).
    pub break_carries: Vec<String>,
}

/// How a branch of type `!` leaves, for the merge dot's wording: it
/// ends in a `return`, a `break` out of a loop, a `continue`, a panic
/// (`panic!()`, `unreachable!()`, ...), or some other call that never
/// returns (`std::process::exit`, a user `-> !` fn).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BranchExit {
    Return,
    Break,
    // A `break 'a` out of a labeled block (`'a: { … }`).
    BreakBlock,
    Continue,
    Panic,
    Other,
}

pub fn string_of_branch(b: &BranchType, index: usize) -> String {
    match b {
        BranchType::If(x, _) | BranchType::Loop(x, _) | BranchType::Match(x,_) => {
            x.get(index).unwrap().clone()
        }
    }
}

pub fn create_line_map(v: &Vec<(usize, ExternalEvent)>) -> BTreeMap<usize, Vec<ExternalEvent>> {
    let mut res: BTreeMap<usize, Vec<ExternalEvent>> = BTreeMap::new();
    for (l, e) in v {
        if e.is_var_arrow_ev() {
            res.entry(*l).and_modify(|v| {v.push(e.clone())}).or_insert(vec![e.clone()]);
        }
    }
    res
}

/// What a reference borrows when its lender has no column but the
/// visitor can still name it, because it is data that lives for the
/// whole program. Decided from the reference's initializer (an
/// `ExprKind::Lit`, or a path resolving to a `static` / `const` item)
/// and carried on the StaticDie / MutableDie that ends the borrow, so
/// the arrow can say what the borrow was of instead of "what it borrows
/// from isn't shown".
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum StaticLender {
    /// `"hi"`, `b"hi"`, `c"hi"`. `noun` names the kind of literal
    /// ("string literal", "byte string literal", "C string literal").
    Literal { noun: &'static str },
    /// `&GREETING` for a `static GREETING`: a borrow of the static item.
    Static(String),
    /// `GREETING` for a `static GREETING: &T`: a copy of the static's
    /// reference, so it borrows what the static points to, not the
    /// static itself.
    PointeeOfStatic(String),
    /// `C` for a `const C: &T`: same, for a constant. A constant has
    /// no address of its own; the data its reference points to is
    /// `'static`.
    PointeeOfConst(String),
    /// `&C` for a `const C` whose value has no destructor and no
    /// interior mutability: Rust promotes the borrowed copy to a
    /// `'static`.
    PromotedConst(String),
    /// A `ref` / `ref mut` binding out of a tuple-literal scrutinee's
    /// element (`(Some(ref mut s), _)` on `(p, q)`) borrows the
    /// temporary tuple, which has no column: the borrow's end is a
    /// plain end dot with no arrow (#658).
    TupleTemporary,
    /// Not a reference's lender: the end, where a `for` loop ends, of the
    /// borrow its iterator holds (`for c in s.chars()`; #868). `call` is
    /// the method that lent the variable (`chars`). The renderer says
    /// the variable is borrowed by the loop's iterator from that call to
    /// here (#871). Never drawn as an arrow.
    LoopIterator { call: String },
    /// Like `LoopIterator`, for a head written `&v` / `&mut v` whose
    /// loop variables borrow none of it (`for &y in &v`, `for _ in
    /// &v`; #885): only the loop's iterator holds the borrow. `lend` is
    /// the id of the borrow drawn on the `for` line, which the renderer
    /// also titles as the iterator's. Never drawn as an arrow.
    LoopHeadBorrow { lend: usize },
}

/// Whether the `&` / `&mut` of a pass-by-reference event was written in
/// the source or inserted by Rust. Drives the call dot's tooltip: without
/// a note, `println!("{}", s)` or `s.len()` reads like it moves `s`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImplicitRef {
    /// Written (`f(&s)`, `(&s).len()`), or the argument already is a
    /// reference (`r.len()` with `r: &String`).
    No,
    /// A formatting macro's argument: `println!("{}", s)` goes
    /// through `format_args!`, which takes `&s`.
    MacroArg,
    /// A method receiver that rustc auto-referenced: `s.len()` is
    /// `String::len(&s)`, `s.push_str(t)` is `String::push_str(&mut s, t)`.
    Autoref,
}

/// Where a call may store the reference it is passed, per its
/// signature: `point(&mut hold, &t)` with
/// `fn point<'a>(h: &mut H<'a>, t: &'a String)` may store `&t` in
/// `hold` (#513). The borrow then outlives the call, so the lender's
/// borrow doesn't end at the call; a separate StaticDie / MutableDie
/// ends it where the borrow checker does. Holds the place's name, the
/// variables its value later moves on to while the borrow is live
/// (`let w = hold;`, #990), which hold the borrow after it, and the
/// copies of its value (`let w = hold;` with `hold: Option<&String>`,
/// #997), which hold it as well; true when the moves are in different
/// arms of an `if` / `match`, so only one of them happens; the clones
/// of its value (`let w = hold.clone();`, #1003), which hold it as
/// copies do; true when a move is of a clone's value (`let x = w;`
/// after `let w = hold.clone();`), so it comes after the clone; and the
/// other values made from hold's that rustc says hold the borrow too
/// (`let w: Vec<&String> = hold.iter().copied().collect();`, #1015),
/// each with the holder it was made from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StoredIn(pub Option<String>, pub Vec<String>, pub Vec<String>, pub bool, pub Vec<String>, pub bool, pub Vec<(String, String)>);

impl StoredIn {
    /// Takes the later holders (all but the place) from `other`.
    pub fn set_holders(&mut self, other: &StoredIn) {
        self.1 = other.1.clone();
        self.2 = other.2.clone();
        self.3 = other.3;
        self.4 = other.4.clone();
        self.5 = other.5;
        self.6 = other.6.clone();
    }
}
/// A method call whose receiver r is a reference to a reference
/// borrows what's `derefs + 1` references away, not what r points to:
/// `r.len()` with `r: &&String` is `str::len(&***r)` and borrows the
/// String `**r`, which `*r` points to (#952). `holds_value`: that place
/// is a "value" (Copy or nothing to drop, #484), not a resource.
/// The receiver can also be a `Box` holding a reference (`k.push('!')`
/// with `k: Box<&mut String>` is `String::push(&mut **k, '!')`): the
/// call borrows what the reference `*k` points to, through k, not k's
/// Box (#1282). `held_mut` is then that reference's mutability; `None`
/// for a receiver that is itself a reference.
/// Or an element of a local array that holds a reference (`k[0].push('!')`
/// with `k: [&mut String; 1]`, #1319): `elem` is that element as written
/// (`k[0]`), `derefs` counts the derefs past it, and `held_mut` is set as
/// for a Box.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeepPlace {
    pub derefs: usize,
    pub holds_value: bool,
    pub held_mut: Option<bool>,
    pub elem: Option<String>,
}

impl DeepPlace {
    /// The reference pointing to what the call borrows (`*r`, `*k`,
    /// `k[0]`), for the column of variable `name`.
    pub fn pointer(&self, name: &str) -> String {
        format!("{}{}", "*".repeat(self.derefs), self.via(name))
    }

    /// What the call borrows through: the element (`k[0]`), else the
    /// variable `name` itself.
    pub fn via(&self, name: &str) -> String {
        self.elem.clone().unwrap_or_else(|| name.to_owned())
    }
}
/// Where a value an `FnOnce` closure captured by value ends up when
/// the closure is called, from the closure body's MIR (#271).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum CapturedEnd {
    /// The body doesn't move it on, or passes it to `drop`: it is
    /// dropped during the call.
    Dropped,
    /// The body returns it: it leaves the closure as the call's result.
    Returned,
    /// The body moves it somewhere else, or only on some paths: all we
    /// can say is that the call uses it up.
    Moved,
}

/// A by-value capture of an `FnOnce` closure that needs accounting for
/// at its call: the captured place's name, what happens to it, and
/// whether its type has drop glue.
#[derive(Clone, PartialEq, Eq, Debug, Hash)]
pub struct CapturedFate {
    pub name: String,
    pub end: CapturedEnd,
    pub needs_drop: bool,
}

/// A borrow that reaches a variable through the reference a call
/// returned: `let w = first_word(&s);` makes `w` borrow `s`, and
/// `let r = longest(&a, &b);` makes `r` borrow both `a` and `b`,
/// because the signature ties the result's lifetime to both arguments
/// (#171).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BorrowVia {
    /// The call whose result holds the borrow (`first_word`).
    pub func: String,
    /// Every variable the result borrows from, in argument order.
    pub lenders: Vec<String>,
    /// The signature also ties the result to an argument whose data
    /// has no column (a string literal, the caller's data behind a
    /// reference parameter), so it may point there instead.
    pub also_unnamed: bool,
    /// Which of the call's arguments the signature ties the result to.
    pub tie: ArgTie,
    /// What the call returned that carries the borrow.
    pub carrier: ViaCarrier,
    /// Calls chained on `func`'s result that only pass the borrow on
    /// (`unwrap` in `w.last_mut().unwrap()`, #1266), innermost first.
    pub passed_on: Vec<String>,
}

/// Which arguments of a call a returned reference's lifetime is tied
/// to by the callee's signature. Positions count the receiver of a
/// method call as position 0; it only shapes the tooltip wording.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct ArgTie {
    /// The call is a method call (`s.as_str()`): position 0 is the
    /// receiver, "the value it was called on".
    pub receiver: bool,
    /// Number of arguments, counting the receiver.
    pub total: usize,
    /// Tied positions, in order.
    pub tied: Vec<usize>,
    /// How many of the tied arguments have no column to draw (a
    /// literal, a value that isn't tracked).
    pub unnamed: usize,
    /// The one tied argument is a literal aggregate (`Pair { l: &s, r: &t }`,
    /// `[&s, &t]`): its value holds each of its borrows at once, so the
    /// result holds all of them; nothing is chosen between them.
    pub holds_all: bool,
    /// Tied positions whose parameter is a reference that the result's
    /// lifetime appears only *inside* (`rows: &Vec<&'a Vec<i32>>` with
    /// result `&'a i32`): the result has the lifetime of the references
    /// stored in that argument, not the argument's own, so the caller's
    /// variable isn't kept borrowed (#1084).
    pub stored_in: Vec<StoredTie>,
    /// Some tied argument has a lifetime that only outlives the
    /// result's (`'a: 'b`, #1125): "the same lifetime" isn't true of it.
    pub via_outlives: bool,
}

/// An argument a call's result is tied to only through the references
/// stored in what it points to (see `ArgTie::stored_in`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StoredTie {
    /// Its position, counting the receiver.
    pub index: usize,
    /// The caller's variable it points to, when the argument is `&rows`
    /// or an auto-referenced `rows`.
    pub name: Option<String>,
    /// What it points to is itself the reference the result copies
    /// (`first(&t)` with `t: &String`, parameter `&&'a String`).
    pub referent_is_ref: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViaCarrier {
    /// The borrower is the reference the call returned (#171).
    Reference,
    /// The borrower isn't a reference but a value that holds one
    /// (`let it = v.iter();`, an `Iter<'_, i32>`; #336).
    Value,
    /// The borrower is a reference field of the struct the call
    /// returned (`Excerpt::new(&novel)` → `ex.part`; #336).
    StructField,
    /// The borrower is a reference element of the tuple the call
    /// returned (`pair(&s)` → `q.0`; #1341).
    TupleField,
    /// The borrower is a reference copied out of a collection that
    /// holds references (`let r = refs[0];`; #349). `func` names the
    /// collection.
    Element,
    /// The borrower is the receiver a `&mut self` method stored the
    /// borrow in (`refs.push(&s)`; #392). `func` names the method.
    StoredBy,
    /// The borrower is a reference a pattern copied (or, for `&mut`,
    /// moved) out of a value that holds one (`Some(x)` on `third =
    /// v.get(2)`; #391). `func` names that value, or the function whose
    /// result was matched when `from_call`. With `field_of`, the
    /// borrower is a reference field of a struct the pattern bound
    /// (`x.part` for `Some(x)` on `o: Option<Part>`; #692), and `moved`
    /// says how that struct was bound.
    PatternBinding { from_call: bool, moved: bool, field_of: bool },
    /// The borrower is a binding by reference into what the reference a
    /// call returned points to (`Some(s)` on `pick(&a, &b)` returning
    /// `&Option<String>`; #1368). `func` names the call.
    PatternRef,
    /// The borrower is a closure variable bound to the closure a call
    /// returned (`let g = keep(|| s.len());`; #686), which holds the
    /// reference its capture made. `func` names the call; `through_ref`
    /// when the capture reborrows what a reference variable points to.
    ReturnedClosure { mutable: bool, through_ref: bool },
    /// The borrower is a binding of a pattern matched against a borrow of
    /// a whole struct or tuple (`let (a, b) = &mut t;`), which points to
    /// one part of it (#1130). `func` names the part (`t.0`), `lenders`
    /// the whole: the one loan is of all of it.
    Part,
    /// The borrower is a reference variable a call may have stored the
    /// borrow in, through a `&mut` to it (`store_s(&mut r, &b)` with
    /// `dst: &mut &'a String`; #1353). `func` names the call.
    StoredThrough,
}

/// Where the tuple an `if let` / `match` scrutinee `(x, y)` builds is
/// dropped (#327), as its drop dot says. In edition 2024 an `if let`'s
/// scrutinee temporaries are dropped at the end of the if branch, or,
/// when the pattern doesn't match, before the else branch runs; a
/// `match`'s at the end of the statement holding the match.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TupleDropAt {
    /// At the end of the if branch (the dot is on its last line).
    IfEnd,
    /// The if branch leaves early (`return`, `break`, a panic), and
    /// the tuple is dropped then.
    IfExit,
    /// The pattern didn't match: before the else branch runs.
    Else,
    /// The pattern didn't match, and there is no else.
    NoElse,
    /// After this match arm, when the match ends.
    MatchArm,
    /// After this match arm, at the end of the statement the match is
    /// in (a later line than the match's end).
    MatchArmStmt,
    /// This match arm leaves the match early.
    MatchArmExit,
    /// Here, at the end of the match (a one-arm match, drawn inline).
    MatchHere,
    /// Only if the `?` here, inside the arm or if branch, returns early
    /// (#653).
    TryReturns,
}

/// What a reassignment of a struct or tuple whose parts were moved out
/// earlier drops of its previous value (#628), by part name: like the
/// scope end of such a struct (`event_dot_struct_partial_go_out_of_scope`).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct ReassignParts {
    /// Moved out on every path.
    pub moved: Vec<String>,
    /// Moved out on some paths only: dropped by a drop flag.
    pub maybe_moved: Vec<String>,
    /// Leaf parts that still hold a value that needs drop.
    pub dropped: Vec<String>,
    /// Some of `maybe_moved` were moved by a `let` or-pattern (#597) or
    /// a let-chain's condition (#281), not a branch: "nothing above".
    pub by_cond: bool,
}

/// A variable whose value, or part of it (`rest`: the pattern moved
/// some of it out), is still in a scrutinee tuple when it's dropped.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TupleLeft {
    pub name: String,
    pub rest: bool,
    pub rc: Option<RcDrop>,
    /// An or-pattern arm whose alternatives take different parts of the
    /// tuple (`(Some(x), _) | (_, Some(x))`, #533): the names that may
    /// have taken some of this variable's value, depending on which
    /// alternative matched at run time. Empty otherwise.
    pub takers: Vec<String>,
}

/// Where an `ExternalEvent::LoopExitMove` happens.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum MaybeMovedAt {
    /// The end of a loop. `nothing_above`: a move on the path to a
    /// `break` that isn't in a branch which then breaks (`loop {
    /// drop(s); if c { break; } s = … }`, #980), so a drop-flag text
    /// says "nothing above" moved it, not "no branch above". `kept`:
    /// other time rounds come back round with it holding a value (a
    /// bare `loop` whose body moves it on some time rounds only, left
    /// by a `break` before the move), so whether it holds one after the
    /// loop isn't down to whether something above moved it: as
    /// `LoopRound { kept: true }`, a drop flag's text says "only if it
    /// still holds it" (#1029).
    /// `carried`: a `break` that doesn't come after the move moves it
    /// itself (`break s`, #1504): with `definite`, every way out moves
    /// it, by then or by its value.
    Loop { nothing_above: bool, kept: bool, carried: bool },
    /// The end of a labeled block (`'a: { … break 'a; … }`).
    /// `skipped`: the move is on the way to the block's end, and a
    /// `break` out of it before the move skips it (`'a: { if c { break
    /// 'a; } drop(s); }`, #1286); otherwise a `break` comes after it.
    /// `carried`: as for `Loop`.
    LabeledBlock { skipped: bool, carried: bool },
    /// The start of the `else` branch (`path`: or the implicit empty
    /// one) of an `if` whose let-chain condition moves the variable in
    /// a part that a part which can be false comes after
    /// (`if let Some(a) = x && a.len() > 5`, #281), or of a later arm
    /// of a `match` whose guard moved it and then failed (#247). On that path the
    /// move has happened by the time the condition fails. `to`: where
    /// the condition moves it (variable or function names; empty when
    /// it has no name). `dropped`: the chain's bindings among `to` that
    /// are dropped when the condition fails (they hold what was moved
    /// and need drop). `how`: what the condition does to it (#579).
    ChainFail { to: Vec<String>, dropped: Vec<String>, path: FailPath, how: ChainFailHow },
    /// The end of a `while` loop whose condition moves the variable
    /// (`while let Some(x) = it.next() && { let w = s; … }`, `while
    /// let Some(c) = first(s)`, #968): the loop ends when the condition
    /// turns out false, which may be after that move. `to`, `dropped`,
    /// `how`: as for `ChainFail`. Like `ChainFail`, its drop flag reads
    /// "nothing above" moved it, not "no branch above".
    WhileCondFail { to: Vec<String>, dropped: Vec<String>, how: ChainFailHow },
    /// The start of a loop's body (#992), for a variable that holds its
    /// value where the loop starts but that the body may leave moved
    /// where the loop comes back round (a move further down, or before
    /// a `continue`, with no new value after it): until the body gives
    /// it a new value, it is maybe-moved (rustc: "value moved here, in
    /// previous iteration of loop"). The loop body is drawn once (#853),
    /// so this stands for the later times round.
    LoopHead,
    /// The end of a `while` / `for` loop with a `LoopHead` event for
    /// the variable: the loop can end (its condition false, its
    /// iterator done) after a time round that moved it (#992).
    /// `kept`: other time rounds leave it holding a value (or a branch
    /// above moved it before the loop, #1012), so whether it holds one
    /// after the loop isn't down to whether "something above" moved it:
    /// a drop flag's text says "only if it still holds one" (#1019).
    LoopRound { kept: bool },
}

/// Where a condition leads when it turns out false after an effect that
/// lasts (`MaybeMovedAt::ChainFail`, `ExternalEvent::ChainFailAssign`,
/// `ExternalEvent::ChainFailUse`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FailPath {
    /// The `else` branch of an `if` whose let-chain condition failed
    /// (#281).
    Else,
    /// No `else`: the path that skips the `if`'s body.
    NoElse,
    /// The later arms of a `match`, after the guard on this line failed
    /// (#247, #248): matching goes on with them. The borrow checker
    /// counts every later arm as one that may run after it, whatever
    /// its pattern.
    Guard(usize),
}

/// A new reference a let-chain condition (or a match guard) assigned
/// before it failed (#603, #1068): see `ExternalEvent::OrAltBorrow`.
/// `rhs`: the assigned expression as written (`&s`, `first(&v)`,
/// `longest(&a, &b)`). `new`: how many of the event's `from` names are
/// lenders of that new reference; the rest are the lenders of the one
/// it held before.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ChainRef {
    pub path: FailPath,
    pub rhs: String,
    pub new: usize,
    /// The new / old reference came from a call's result (`first(&v)`):
    /// it points into its lenders, not at them.
    pub new_via_call: bool,
    pub old_via_call: bool,
    /// What the old reference pointed to when that has no column (`let
    /// mut r: &str = "lit";`): not among `from`.
    pub old_static: Option<StaticLender>,
}

/// How a let-chain's condition moves a variable (`MaybeMovedAt::ChainFail`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ChainFailHow {
    /// Moves it (`let Some(a) = x`, `take(s)`).
    Move,
    /// Moves out of the place it owns, written as the String: `*b` of a
    /// `Box` (#579).
    Out(String),
    /// Calls it, a closure that can only be called once (#579).
    Call,
    /// Moves it into the temporary that a literal scrutinee builds
    /// (`Some(o)`, `(a, 1)`), whose text this is, for a pattern that can
    /// fail to take apart (`if let Some(Some(x)) = Some(o) && ..`, #949).
    Literal(String),
    /// Gives it a new value when it may hold none (#603): `drop(z); if
    /// let Some(a) = y && { z = a; … }`. On the failing path it holds the
    /// new value or nothing (or, with `before: NoValueBefore::Maybe`,
    /// also its previous value): rustc rejects a use (E0382) and a drop
    /// flag decides what's dropped. Like a move, it can't be used.
    AssignNoValue { before: NoValueBefore },
    /// Gives it a new value, then moves it (#603): `{ z = a; true } &&
    /// take(z)`. On the failing path it holds its previous value or
    /// nothing. `place`: the write is to the place reached through it
    /// (`*b`), as written. `drops_old`: the write drops the old value.
    AssignThenMove { place: Option<String>, drops_old: bool, to: Vec<String> },
    /// Moves it, then gives it a new value (#603): `take(z) && { z = a;
    /// true }`. On the failing path it holds the new value or nothing,
    /// and with `old` (a part that can fail comes before the move) also
    /// possibly its previous value.
    MoveThenAssign { old: bool, to: Vec<String> },
}

/// Why a variable may hold no value where a condition gives it a new
/// one (`ChainFailHow::AssignNoValue`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum NoValueBefore {
    /// It was moved before the condition (on this line, if known).
    Moved(Option<usize>),
    /// The condition's first part moved it (on this line, if known).
    MovedByCondition(Option<usize>),
    /// It was moved on some paths above only: it may still hold its
    /// previous value.
    Maybe,
}

impl ChainFailHow {
    /// On the failing path it may hold a new value the condition gave it
    /// (or nothing): its drop flag reads "only if it still holds it", not
    /// "only if nothing above moved it".
    pub fn may_hold_new(&self) -> bool {
        matches!(self, ChainFailHow::AssignNoValue { .. } | ChainFailHow::MoveThenAssign { .. })
    }
}

/// The `return`s directly in a loop body (not in an `if` / `match`
/// drawn as a branch) that can leave the function before a scope end is
/// reached (#1020). Their drops aren't drawn on their own rows: the loop
/// body is drawn inline, and it may not run. So the scope end, where the
/// drop is drawn, says it happens at the `return` instead: `always`, on
/// every path that gets that far (a variable of the loop body, whose
/// closing brace the body never reaches; or a scope end after a `loop`
/// that nothing breaks out of, which the code never reaches, #1041), or
/// `maybe`, if that `return` runs (a variable declared before the loop,
/// the loop's iterator).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct EarlyReturns {
    /// Lines of the `return`s where it is always dropped instead: at
    /// whichever of them runs.
    pub always: Vec<usize>,
    /// Lines of the `return`s where it is dropped if they run.
    pub maybe: Vec<usize>,
    /// With `always`: lines of the `?`s it is in scope at before those
    /// `return`s, any of which may return first and drop it there
    /// instead (#1044).
    pub tries: Vec<usize>,
    /// A scope end after a `loop` that nothing breaks out of and no
    /// `return` directly in it leaves (#1043), which the code never
    /// reaches: drawn instead at the loop's closing `}`, the last row
    /// the code reaches while the loop keeps running, where it ends the
    /// column without dropping anything (#1057).
    pub runs_on: Option<LoopRunsOn>,
    /// The scope end of a variable in scope at a call to
    /// `process::exit` (or `abort`), drawn at the call (#1172): the
    /// process ends there without running any destructor, so nothing is
    /// dropped. Names the call. Also at a call to a function of the
    /// user's that never returns and doesn't (always) panic (#1553).
    pub process_exit: Option<NoDropExit>,
}

/// A call that never returns and, unlike a panic, doesn't (or may not)
/// drop what is in scope (#1172, #1553).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct NoDropExit {
    /// The function called, as the tooltip names it (`process::exit`,
    /// `spin`).
    pub call: String,
    pub how: NeverReturns,
}

/// How a call that never returns leaves (`NoDropExit`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NeverReturns {
    /// It ends the process (`process::exit`, `abort`, or a function
    /// of the user's every path of which calls one): no destructor runs.
    Exits,
    /// A function of the user's that loops forever (`loop {}`): the
    /// code after it never runs, and nothing is dropped.
    Spins,
    /// A function of the user's whose body doesn't settle how it never
    /// returns: it may panic (and unwind, dropping what is in scope),
    /// run forever or end the process.
    Unknown,
}

/// A variable still in scope while a `loop` that nothing breaks out of
/// keeps running, at the loop's closing `}` (#1057).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LoopRunsOn {
    /// What can leave the loop.
    pub exits: LoopExits,
    /// Whether the loop moves the variable (at one of its exits:
    /// `return g`), so it isn't always dropped there.
    pub moved: bool,
}

/// What can leave a `loop` that nothing breaks out of (#1057).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LoopExits {
    /// A `return`, a `?` or a panic, which drop what is in scope.
    Dropping,
    /// None that drop anything: only `std::process::exit` / `abort`
    /// (which drop nothing), or a panic in a call or arithmetic, which
    /// isn't drawn.
    Other,
    /// Nothing: the loop never ends (`loop {}`).
    None,
}

/// Where a `for` loop's iterator is dropped (#482, #592).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LoopLeft {
    /// Where the loop ends.
    End,
    /// At a `return` inside the loop.
    Return,
    /// At a `break` out of the loop, or to an outer loop, leaving this one.
    Break,
    /// At a `?` inside the loop, if it returns early (#649).
    Try,
    /// At a `panic!()` inside the loop, as it unwinds (#649).
    Panic,
    /// At a `continue` to an outer loop, leaving this one.
    Continue,
}

/// What a `for` loop does with a head it moves (`for s in v`, `for s
/// in it`), when the loop's iterator has drop glue (#482).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub enum LoopHead {
    /// Not a `for` loop's head (or its iterator has nothing to drop).
    #[default]
    No,
    /// A collection (`v: Vec<String>`): moved into the iterator
    /// `IntoIterator::into_iter` builds from it.
    Collection,
    /// A collection of Copy elements (`ys: Vec<i32>`, `Vec<&String>`):
    /// moved into the iterator like `Collection`; each element the loop
    /// variable gets is a copy (#1372).
    CollectionCopy,
    /// One binding of a destructuring pattern in the head (`for (s, k)
    /// in names`, `for &(a, b) in &pairs`, #1390), drawn like `let (s, k)
    /// = pair;`: an arrow from the collection per binding. `path` is the
    /// part of each element the binding gets (`.0`, `.name`, `.0.1`;
    /// empty for the whole element, `for &y in &v`), `copy` whether it is
    /// a copy. `moved`: the collection is moved into the loop's iterator
    /// (each binding's event says so on its column, once); else it is
    /// borrowed by it (`&pairs`, its loan drawn on its own), and each
    /// binding copies a value inside it.
    Part { path: String, copy: bool, moved: bool },
    /// Already an iterator (`it: vec::IntoIter<String>`):
    /// `into_iter` returns it unchanged, so it becomes the loop's
    /// iterator, dropped when the loop ends (`LoopIteratorDrop`).
    Iterator,
}

/// A move made by the `?` operator (#652). `r?` is
/// `match Try::branch(r) { Continue(v) => v, Break(e) => return
/// FromResidual::from_residual(e) }`: `Try::branch` takes `r` by value,
/// so a non-`Copy` `r` is moved on both paths. On success the value
/// inside goes on (to the variable bound to `r?`, if any); on failure
/// the function returns early, handing the error (converted with
/// `From`) or the `None` to the caller. Which success / failure the
/// tooltips name depends on `r`'s type.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum TryMove {
    Result,
    Option,
    /// Another `Try` type (`ControlFlow`, `Poll<Result<..>>`).
    Other,
}

/// A value that one of several `break`s carries out of a `loop` to
/// the loop's receiver (`let r = loop { if c { break a; } else {
/// break b; } };`, #561). Each is paired with the receiver where the
/// loop ends (as a single `break` in a branch is, #431), but only the
/// `break` that ended the loop ran, so the transfer happened only if
/// it was this one.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BreakAlt {
    /// The `break` as written (`break a`), or `None` when it is too
    /// long to quote in a tooltip.
    pub brk: Option<String>,
    /// The moved variable is declared inside the loop, so it goes out
    /// of scope with it: on the paths through the other `break`s it
    /// was dropped there (#410) or never created, and nothing is left
    /// for a drop flag at the loop's end.
    pub in_loop: bool,
    /// What the value ends: a `loop`, or a labeled block (`let x = 'a:
    /// { if c { break 'a s; } t };`, #1134), through a `break` or by
    /// running to its last expression.
    pub end: AltEnd,
}

/// See BreakAlt::end.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AltEnd {
    Loop,
    Block,
    BlockTail,
}

/// A borrow of a variable declared outside an `if` let-chain that the
/// condition makes in a part before one that fails (#688): it lends the
/// variable to `to`, a function it calls (`y.take()`) or, with
/// `binding`, a binding of the chain (`let Some(a) = &x`); `to` is empty
/// when the borrower has no name. `changes`: a mutable borrow that can
/// change the variable: by a call, or by a binding that a later part
/// writes or lends on through.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ChainUse {
    pub to: String,
    pub mutable: bool,
    pub binding: bool,
    pub changes: bool,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ExternalEvent {
    /* let binding, e.g.: let x = 1 */
    // `break_alt`: see BreakAlt (also on Copy and Move).
    Bind {
        from: ResourceTy,
        to: ResourceTy,
        id: usize,
        break_alt: Option<BreakAlt>,
    },
    // `into_closure`: a closure literal passed to a call captures
    // `from` by value (`thread::spawn(move || ..v..)`): `from` moves
    // (or is copied) into the closure, not into the callee; `to` is
    // the callee. Only `match_closure_arg` sets it.
    // `closure_value`: `from` and `to` are closures and the whole
    // closure value is moved or copied (`let h = g;`, #704). It is not
    // a capture, although `to` is a closure.
    // `into_ctor`: `from` was copied into the value the constructor
    // named here builds, and `to` holds that value (`y =
    // Fruit::Apple(z)`, #866): `to` is not a copy of `from`. Only
    // `retarget_ctor_arg_events` sets it.
    // `pointee`: a `&` sub-pattern copied the value a reference stored
    // inside `from` points to (`let Some(&y) = orr`, #874), not a part
    // of `from`; `Some(true)` when the binding is only a part of that
    // value (`Some(&(a, b))`).
    Copy {
        from: ResourceTy,
        to: ResourceTy,
        is_partial: bool,
        into_closure: bool,
        closure_value: bool,
        into_ctor: Option<String>,
        pointee: Option<bool>,
        id: usize,
        break_alt: Option<BreakAlt>,
        /// `from` is `Anonymous`: `to` gets a copy of this constant (`let
        /// m = MAX;`, #690), which has no column.
        of_const: Option<String>,
    },
    // `loop_head`: a `for` loop's head moved into the loop, whose
    // iterator has drop glue (#482). Only the for-loop arm of the
    // visitor sets it.
    // `break_out`: a `break` in a branch of a loop carries `from`, a
    // variable declared inside the loop, out to the loop's receiver
    // (`if c { break k; }`, #431). The move is drawn where the loop
    // ends, which is also where `from` goes out of scope on the
    // iterations that don't break, and those drop its resource.
    // `via_try`: the move is made by the `?` operator (`let n = r?;`,
    // #652); see TryMove.
    Move {
        from: ResourceTy,
        to: ResourceTy,
        is_partial: bool,
        into_closure: bool,
        loop_head: LoopHead,
        via_try: Option<TryMove>,
        id: usize,
        break_alt: Option<BreakAlt>,
        break_out: bool,
        closure_value: bool,
    },
    // `is_reborrow`: the borrow was written `&*r` / `&mut *r` with
    // `r` a reference — a reborrow through `r`, as opposed to a fresh
    // borrow of an owner (`&s`) or of the reference variable itself
    // (`&r`, `&mut r`). Decided syntactically at the `AddrOf` site in
    // `match_rhs`; every other producer passes `false`.
    // `via`: the borrow reaches `to` through the reference a call
    // returned (`let w = first_word(&s);`, #171); see `BorrowVia`.
    // `break_alt`: the borrow is one of several `break` values
    // reaching a loop's receiver (`if c { break ra; } else { break rb;
    // }` reborrows `rb`, #657); see BreakAlt.
    // `of_ref_var`: see MutableBorrow; `&r` (`&&T` / `&&mut T`, #925).
    StaticBorrow {
        from: ResourceTy,
        to: ResourceTy,
        is_partial: bool,
        is_reborrow: bool,
        break_alt: Option<BreakAlt>,
        via: Option<BorrowVia>,
        id: usize,
        of_ref_var: bool,
    },
    // `of_ref_var`: the borrow is `&mut r` with r a reference variable
    // (`&mut &mut T` / `&mut &T`): r itself is borrowed, not what it
    // points to, and what's borrowed is the reference r holds, a value
    // (#914). Set only where `ref_var_borrow` says so.
    MutableBorrow {
        from: ResourceTy,
        to: ResourceTy,
        is_partial: bool,
        is_reborrow: bool,
        break_alt: Option<BreakAlt>,
        via: Option<BorrowVia>,
        id: usize,
        of_ref_var: bool,
    },
    // `static_lender`: what the reference borrowed, when that has no
    // column but is known to be data that lives for the whole program
    // (see StaticLender). Only the arrow's tooltip uses it.
    // `still_borrowed_by`: values that hold the borrow and go on
    // holding it after this reference's ends (`it` for `n` in `while
    // let Some(n) = it.next()`, #575). The lender stays borrowed.
    // `of_ref_var`: the end of a StaticBorrow with `of_ref_var` (#925).
    StaticDie {
        // return the resource to "to"
        from: ResourceTy,
        to: ResourceTy,
        id: usize,
        static_lender: Option<StaticLender>,
        still_borrowed_by: Vec<String>,
        of_ref_var: bool,
    },
    // `of_ref_var`: the end of a MutableBorrow with `of_ref_var` (#914).
    MutableDie {
        // return the resource to "to"
        from: ResourceTy,
        to: ResourceTy,
        id: usize,
        static_lender: Option<StaticLender>,
        still_borrowed_by: Vec<String>,
        of_ref_var: bool,
    },

    RefDie {
        from: ResourceTy,
        to: ResourceTy,
        num_curr_borrowers: usize,
        id: usize
    },
    // a use of the Owner, happens when var pass by reference
    // its really borrow and return but happens on the same line,
    // use this event instead of borrow and return for more concise visualization 
    // `result_keeps_borrow`: the call returns a reference that is bound
    // to a variable and still borrows `from` (`let w = first_word(&s);`,
    // #171), so `from` isn't given back when the call returns; it is
    // given back when that variable's borrow ends.
    // `implicit`: whether the `&` was inserted by Rust rather than
    // written (see `ImplicitRef`). The tooltip says so, since without
    // it the call reads like a move.
    PassByStaticReference {
        from: ResourceTy,
        to: ResourceTy, // must be a function
        implicit: ImplicitRef,
        // The callee is `RefCell::borrow_mut`, identified by its
        // DefId (not its name: a user type may define its own
        // `borrow_mut(&self)`). Selects the runtime-exclusivity wording.
        refcell_borrow_mut: bool,
        // The argument is a shared reference to a value with interior
        // mutability (`r.set(2)` with `r: &Cell<i32>`, #1149): the call
        // can change what it points to.
        pointee_interior_mut: bool,
        // See `StoredIn`.
        stored_in: StoredIn,
        result_keeps_borrow: bool,
        // The reference is one of several `break` values of a `loop`
        // that is the call's argument (`take(loop { if c { break ra; }
        // else { break rb; } })`, #662): passed only if that `break`
        // ended the loop. See BreakAlt.
        break_alt: Option<BreakAlt>,
        // The references and holders that already borrow `from` and go
        // on borrowing it after the call (`let r = &v; v.len(); use(r)`,
        // #870): `from` stays shared when the call gives its borrow
        // back.
        still_borrowed_by: Vec<String>,
        // The argument is `&r` with r a reference variable, so the call
        // borrows r itself, not what r points to (`show(&x)` with
        // `show(r: &&String)`, #938); see StaticBorrow.
        of_ref_var: bool,
        // The receiver r is a reference to a reference and the call
        // borrows what's further away (`r.len()` with `r: &&String`,
        // #952); see DeepPlace.
        deep: Option<DeepPlace>,
        id: usize
    },
    // `is_reborrow`: `from` is a `&mut T` that the call reborrows
    // (`f(&mut *r)`, or `f(r)` / `r.m()` with rustc inserting the
    // reborrow) rather than moves (`take(r)` into a generic `T`).
    // `result_has_lifetime`: the call's result type mentions a
    // lifetime, so the reborrow may outlive the call
    // (`let w = first(r);`). Both drive the call-site dot's suffix.
    PassByMutableReference {
        from: ResourceTy,
        to: ResourceTy, // must be a function
        is_reborrow: bool,
        result_has_lifetime: bool,
        implicit: ImplicitRef, // see PassByStaticReference
        stored_in: StoredIn, // see PassByStaticReference
        result_keeps_borrow: bool,
        break_alt: Option<BreakAlt>, // see PassByStaticReference
        // `&mut r` with r a reference variable (`bump(&mut m)` with
        // `bump(r: &mut &mut String)`, #938); see PassByStaticReference.
        of_ref_var: bool,
        // `r.push(..)` with `r: &mut &mut String` (#952); see
        // PassByStaticReference.
        deep: Option<DeepPlace>,
        id: usize
    },
    // `early`: `return`s in a loop body that drop `ro` instead, before
    // this scope end (#1020).
    GoOutOfScope {
        ro: ResourceAccessPoint,
        // What rustc's move data says the variable holds at this `}`,
        // on the paths that get there through the code before it
        // (`ownership_at_scope_ends`, #1036, #1037); `None` when it
        // doesn't say. Set once the body has been visited.
        holds: Option<ScopeEndHolds>,
        id: usize,
        early: Option<EarlyReturns>,
    },
    // The previously-held resource of an owner is dropped when that
    // owner is overwritten by a new value (`y = x` where `y: String`,
    // or `*p = x` for `p: &mut String`). The owner stays in scope and
    // immediately reacquires from the rhs — this event records the
    // drop of the OLD value at the assignment line.
    // `maybe_moved`: a conditional above moved the owner on some paths
    // only, so the old value is dropped here only on the others (a
    // runtime drop flag decides; #335).
    OwnerDropAtReassign {
        ro: ResourceAccessPoint,
        id: usize,
        maybe_moved: bool,
        // With `maybe_moved`: a let-chain's condition may have been
        // what moved it (#281), not only a branch.
        by_cond: bool,
        // With `maybe_moved`: it is maybe-moved only because the loop
        // whose body this is can come back round with it moved
        // (`MaybeMovedAt::LoopHead`, #992).
        loop_head: bool,
        // `ro` is a struct or tuple some of whose parts were moved out
        // before (#628): what of its previous value is dropped.
        parts: Option<ReassignParts>,
        // `ro` is a `let x;` local that may have no value here (see
        // InitBy): the old value is dropped here only if there is one
        // (a runtime drop flag decides).
        maybe_init: Option<InitBy>,
        // `*b = …` overwrites the payload of `ro`, a Box drawn as its
        // payload's part columns (#893): the Box itself keeps its heap
        // allocation, so the dot names `*b`.
        payload: bool,
        // `ro` is a Box whose whole contents were moved out before, on
        // every path (`let s = *b;`): only its heap allocation is freed
        // (#1145).
        contents_gone: bool,
    },
    // `ManuallyDrop::drop(&mut md)`: the value inside `md` is dropped
    // by hand. `md` itself stays in scope but must not be used again.
    ManuallyDropByHand {
        ro: ResourceAccessPoint,
        id: usize
    },
    // `ro`, an unbound temporary (the value a call returns that nothing
    // takes: `make();`, `let _ = make();`, `make().len()`,
    // `f(&make())`), is dropped (#457). Plays the part of GoOutOfScope
    // for that RAP. `ty` is the value's type as written in messages;
    // `when` says when ("at the end of this statement").
    // `literal` is true for the temporary of a tuple / struct / array
    // literal a `let` pattern is matched against (#439). `ty` is None
    // for a `let`-extended temporary (#400) that an early exit (`return`,
    // `break`, `continue`, a panic) drops while it is still alive
    // (#553): its type isn't recorded, and the
    // tooltip says why it lived that long as well as when it drops.
    // `early`: the drop at an early exit of a temporary that isn't
    // split into the branches it's in (an `if let` scrutinee's, see
    // `unbranched_temps`): it happens only on the path that leaves, so
    // the column goes on for the others.
    TemporaryDrop {
        ro: ResourceAccessPoint,
        ty: Option<String>,
        when: String,
        literal: bool,
        early: bool,
        /// See `Owner::temp_part`.
        part: Option<TempPart>,
        id: usize
    },
    // `ro` was moved into a `for` loop as its iterator (`for s in it`,
    // `LoopHead::Iterator`), and the loop drops that iterator: where
    // the loop ends, or (`left`) at a `return` or an outer `break` (#482, #592). `ro`
    // doesn't own it any more, so its state doesn't change. `early`
    // (with `LoopLeft::End`): lines of `return`s directly in the loop
    // body, where it is dropped instead if they run (#1020).
    LoopIteratorDrop {
        ro: ResourceAccessPoint,
        left: LoopLeft,
        id: usize,
        early: Vec<usize>,
    },
    // At the end of a loop: `ro`, a `let x;` local declared before the
    // loop, is assigned in the loop's body, but the loop may end before
    // that assignment ran (a `while` / `for` whose body never ran, a
    // `break` before it; #786). After the loop it may have no value: a
    // drop flag decides whether its next assignment or its scope end
    // drops anything. `before_loop`: `ro` isn't a `let x;` local but a
    // variable that only may have held a value where the loop started,
    // a branch above having moved it on some paths (#1012):
    // `Some(by_cond)`, as for `OwnerDropAtReassign`'s `by_cond`. Then
    // it has a value after the loop also if nothing moved it before.
    // `nested`: the loop is inside another that `ro` was declared
    // before (#1032), and every run of it may start with `ro` holding
    // nothing (on the outer loop's first time round, a branch above
    // having moved it) or holding a value (one the outer loop's time
    // round before gave it): only a drop flag knows what its next
    // assignment drops (`InitBy::DropFlag`).
    LoopMaybeInit {
        ro: ResourceAccessPoint,
        before_loop: Option<bool>,
        nested: bool,
        id: usize
    },
    // At the end of a bare `loop` (#1000): `ro`, declared before the
    // loop, owns a value at every `break` out of it, although the body
    // may leave it moved where it goes round again (`loop { s = …; if
    // c { break; } drop(s); }`). After the loop it owns that value.
    // `reassigned`: every `break` comes after the body gives it a new
    // value.
    LoopExitKept {
        ro: ResourceAccessPoint,
        reassigned: bool,
        id: usize
    },
    // At the end of a loop: `ro` (declared outside the loop) was moved
    // in a branch that then `break`s out of the loop, so after the
    // loop it may have been moved (`definite`: it has been, because
    // every way out of the loop is such a `break`).
    // `at`: where that is; also used at the start of the `else` branch
    // of an `if` whose let-chain condition may have moved `ro` before
    // it failed (#281), see `MaybeMovedAt`.
    LoopExitMove {
        ro: ResourceAccessPoint,
        definite: bool,
        at: MaybeMovedAt,
        id: usize
    },
    // The `?` operator returns early on `Err` / `None`, dropping what
    // the function owns at that point (#430). Only on that path: the
    // owner stays alive here, so this is a dot on its column, like
    // `OwnerDropAtReassign`.
    OwnerDropIfTryReturns {
        ro: ResourceAccessPoint,
        id: usize,
        // rustc's move data says it only may hold a value where the `?`
        // returns early (#1600): the drop flag decides. With more than one
        // `?` on the line for it (one event), whether any of them does.
        maybe: bool,
        // rustc's move data says it holds a value there, at every `?` on
        // the line for it: dropped on that early return, whatever the
        // drawing knows of earlier conditionals (#1600 review).
        held: bool,
    },
    // `return if b { p } else { q };` in a branch: `p`, which only some
    // of the returned `if`'s branches move to the caller, is dropped
    // here on the paths through the others (#697). A dot on its
    // column, which goes on.
    OwnerDropUnlessReturned {
        ro: ResourceAccessPoint,
        id: usize
    },
    // The tuple an `if let` / `match` scrutinee `(x, y)` builds is
    // dropped (#327), and with it what's left in it of `ro`'s value
    // (and of the other variables in `with`). The tuple has no column,
    // so a drop dot on `ro`'s (already moved) column says so; `ro`'s
    // state doesn't change.
    TupleTempDrop {
        ro: ResourceAccessPoint,
        tuple: String,
        at: TupleDropAt,
        with: Vec<TupleLeft>,
        id: usize
    },
    // An or-pattern arm (`(x, _) | (_, x) =>`, `(Some(x), _) | (_, Some(x))
    // =>`) binds `x` from a different place in each alternative, and
    // which alternative matched is only known at run time (#533). `ro` is
    // one of those places; `from` names all of them. If the alternative
    // that binds `x` from `ro` is the one that matched, `ro`'s resource
    // moves into `x` here; otherwise it stays in `ro`, so afterwards
    // `ro` is maybe-moved. Drawn as a dot on `ro`'s column only: `x`
    // gets a plain `Bind`, with no arrow to say where it came from.
    // `tuple`: `ro` is a variable a tuple-literal scrutinee (`match (a,
    // b)`) already moved into its temporary tuple (#327), so what may
    // move into `x` is its value in that tuple, and `ro` itself owns
    // nothing either way (the tuple's drop takes what `x` didn't).
    //
    // `part_of`: `Some((x, p))` when `ro` is a field column of the place
    // `p` that may move as a whole into the struct or tuple `x` (#565);
    // `into` is then `x`'s matching field, and `from` still names the
    // places `x` may be bound from.
    //
    // The same for an `if let` / `while let` / `let` pattern (#597);
    // `in_let` for a `let`'s, which is no branch: the place's drop is
    // then "only if nothing above moved it", not "no branch above".
    OrAltMove {
        ro: ResourceAccessPoint,
        into: String,
        from: Vec<String>,
        tuple: Option<String>,
        part_of: Option<(String, String)>,
        in_let: bool,
        id: usize
    },
    // An or-pattern arm binds the reference `into` from a different
    // place in each alternative (`(ref x, _) | (_, ref x) =>`, #565).
    // `ro` is one of those places; `from` names all of them. Which one
    // `into` borrows is only known at run time, but the borrow checker
    // can't tell either, so it treats every one of them as borrowed for
    // as long as `into`'s borrow is live. `end: false` is where that
    // borrow starts (the pattern), `end: true` where it ends. Drawn as a
    // dot on `ro`'s column, with no arrow; the event with `first` also
    // carries `into`'s own dot (its borrow starting / ending).
    //
    // `chain`: not an or-pattern but a let-chain condition (or a match
    // guard) that assigned `into` a new reference before it failed
    // (#603): on the failing path `into` points to the new place or to
    // the one it pointed to before, and the borrow checker treats both
    // as borrowed while it's live. `from` is then the new reference's
    // lenders, then the old one's (`ChainRef::new` says where they
    // split; several for a call tied to several arguments, #1068).
    OrAltBorrow {
        ro: ResourceAccessPoint,
        into: ResourceAccessPoint,
        from: Vec<String>,
        mutable: bool,
        end: bool,
        first: bool,
        chain: Option<ChainRef>,
        id: usize
    },
    // At the start of the `else` branch (or the implicit empty one) of
    // an `if` whose let-chain condition may have assigned `ro` a new
    // value before it failed (#579): `if let Some(a) = y && { z = a;
    // z > 0 }`. `ro` held a value before, so it holds the old or the new
    // one there; nothing about ownership changes. `drops_old`: the
    // assignment drops the old value.
    // `place`: the assignment is to a place reached through `ro` (`*r`,
    // `v[i]`, `r.f`; #603), written as in the source.
    ChainFailAssign {
        ro: ResourceAccessPoint,
        place: Option<String>,
        drops_old: bool,
        path: FailPath,
        id: usize
    },
    // At the start of the `else` branch (or the implicit empty one) of
    // an `if` whose let-chain condition may have lent `ro` before it
    // failed (#688): `if let Some(a) = &x && let Some(b) = y.take()`
    // lends `y` to `take` before the `let` can fail. Those borrows
    // have ended by then, so nothing about ownership changes; the dot
    // says what the condition did. `definite`: as for
    // `MaybeMovedAt::ChainFail`.
    ChainFailUse {
        ro: ResourceAccessPoint,
        uses: Vec<ChainUse>,
        definite: bool,
        path: FailPath,
        id: usize
    },
    // A reference parameter a returned `if` / `match` doesn't return on
    // this path, whose lifetime the signature ties to the result's
    // (#775): its borrow lasts as long as the result in the caller, so
    // nothing ends here. A dot; the state is unchanged.
    // `slice = &mut slice[1..];` / `s = &s[1..];` (#1123): the reference
    // is given a reborrow of part of what it already points to. The loan
    // of `lender` it holds goes on (the new reference is derived from
    // it), so nothing is returned and nothing new is borrowed from
    // `lender`. A dot; the state is unchanged.
    ReslicedInPlace {
        ro: ResourceAccessPoint,
        lender: String,
        mutable: bool,
        // `&s[1..]` (part of the data) rather than `&mut *r` (all of it).
        part: bool,
        id: usize
    },
    TiedParamNote {
        ro: ResourceAccessPoint,
        lifetime: String,
        id: usize
    },
    // The `else` block of a lone `if let` whose pattern always matches
    // (#987) is dead code: its events aren't drawn, and this dot on the
    // column of each outer variable it used, on the line of its first
    // use there, says nothing in it happens to `ro`. A dot; the state is
    // unchanged.
    DeadElseNote {
        ro: ResourceAccessPoint,
        id: usize
    },
    // A `for` loop whose pattern leaves parts of each element unbound
    // (`for (f, _) in v`, `for _ in v`, #1434): at the end of each pass
    // those parts are dropped with the rest of the element. Stated on
    // `ro`, the loop's iterator (or a binding of the pattern) where the
    // body ends; the state is unchanged. `parts`: their paths in the
    // element (`.1`), empty for the whole element.
    PassRestDrop {
        ro: ResourceAccessPoint,
        parts: Vec<String>,
        id: usize
    },
    // A write to `ro` (or the variable `lender` it is part of) on a path
    // where `borrower`'s loan of `lender` is already dead, though the
    // borrow checker keeps it in scope on another path past the write
    // (#796): why rustc accepts the write inside the drawn borrow. A
    // note on the write's dot; the state is unchanged.
    DeadLoanNote {
        ro: ResourceAccessPoint,
        lender: String,
        borrower: String,
        repoint: Option<(usize, bool)>,
        id: usize
    },
    // only use this event to initialize fn parameters
    InitRefParam {
        param: ResourceAccessPoint,
        id: usize,
        // `Some`: `param` is a `ref` binding of a by-value parameter
        // (`fn f(ref s: String)`), so it borrows the value `f` owns,
        // not anything of the caller's (#377).
        own_param: Option<OwnParam>,
        // `Some(x)`: `param` is a reference field of the struct bound
        // to `x` by a pattern (`if let Some(x) = o`, `x.part: &str`).
        // Its value came with what was matched, and what it borrows
        // isn't tracked there, so it isn't "from the caller" (#352).
        // Never set together with `own_param`.
        in_pattern: Option<String>,
        // `Some`: `param` is the first binding of a parameter pattern
        // that leaves parts of the argument unbound (`P { name, .. }:
        // P`, `(a, _): (String, String)`): those are dropped when the
        // function returns, with no column to show it (#1181).
        rest: Option<ParamRest>,
    },
    
    // A call `f()` of a local closure that rustc inferred to be
    // `FnOnce` (#271). The call is `FnOnce::call_once(f)`: `f` is moved
    // into it, the body runs, and what `f` captured by value is dropped
    // or moved out inside the call. `fates` has one entry per by-value
    // capture, in capture order. No arrow; the event is a dot on `f`.
    ClosureCall {
        closure: ResourceTy,
        fates: Vec<CapturedFate>,
        id: usize
    },

    // Branches are the most interesting event
    // they allow for events to be tree-like
    Branch {
      live_vars: HashSet<ResourceAccessPoint>, // variables who were defined outside of the branch but are live inside it
      branches: Vec<ExtBranchData>, 
      branch_type: BranchType,
      split_point: usize,
      merge_point: usize,
      // `let x;` locals initialized in only some of the branches that
      // reach the merge (#244), so maybe-initialized after it (a drop
      // flag decides whether their scope end or next assignment drops
      // anything).
      maybe_init: HashSet<ResourceAccessPoint>,
      // Variables (and field columns) that a loop the conditional is
      // inside, and that they were declared before, gives a new value
      // somewhere in its body (#1026).
      loop_assigned: HashSet<ResourceAccessPoint>,
      // Variables (and field columns) declared before the conditional
      // that may hold no value where it starts (something above moved
      // them, on some paths or on all) and that only some of the branches reaching the
      // merge give a new value, the others leaving them as they were
      // (#1034): after the merge they may hold a value or not, and only
      // a drop flag knows which.
      maybe_held: HashSet<ResourceAccessPoint>,
      id: usize
    }
}

impl Hash for ExternalEvent {
    fn hash<H: Hasher>(&self, state: &mut H) {
      self.get_id().hash(state);
    }
}

impl ExternalEvent {
    pub fn is_arrow_ev(&self) -> bool {
        match self {
            &ExternalEvent::Copy {..} | &ExternalEvent::Move {..} | 
            &ExternalEvent::StaticBorrow {..} | &ExternalEvent::StaticDie {..} | 
            &ExternalEvent::MutableBorrow {..} | &ExternalEvent::MutableDie {..} => true,
            _ => false
        }
    }

    /// An arrow between two variables, the only arrows the line maps
    /// hold: `ExprVisitor::add_external_event` keeps arrows to and from
    /// functions, to the caller and between anonymous values out of
    /// `event_line_map`, and an arm's line map must agree with it, since
    /// the renderer adds a blank row per extra arrow on a line (#419).
    pub fn is_var_arrow_ev(&self) -> bool {
        if !self.is_arrow_ev() {
            return false;
        }
        let (from, to) = ResourceAccessPoint_extract(self);
        let plain = |t: &ResourceTy| matches!(
            t,
            ResourceTy::Value(r) if !matches!(r, ResourceAccessPoint::Function(_))
        ) || matches!(t, ResourceTy::Deref(_));
        plain(from) && plain(to)
    }

    /// The column events of an `OrAltBorrow` (#565): the place's, and
    /// the reference's when the event carries it (`first`).
    pub fn or_alt_borrow_events(&self) -> (Event, Option<Event>) {
        let ExternalEvent::OrAltBorrow { ro, into, from, mutable, end, first, chain, .. } = self else {
            panic!("internal state: or_alt_borrow_events of another event");
        };
        let lend = Event::OrAltLend {
            into: into.clone(), is: ResourceTy::Value(ro.clone()), sources: from.clone(), mutable: *mutable, end: *end, chain: chain.clone(),
        };
        let borrow = first.then(|| Event::OrAltBorrow {
            is: ResourceTy::Value(into.clone()), sources: from.clone(), mutable: *mutable, end: *end, chain: chain.clone(),
        });
        (lend, borrow)
    }

    pub fn is_gos_ev(&self) -> Option<&ResourceAccessPoint> {
        match self {
            ExternalEvent::GoOutOfScope { ro, .. } | ExternalEvent::TemporaryDrop { ro, .. } => Some(ro),
            _ => None
        }
    }
    
    pub fn get_id(&self) -> usize {
        match self {
            &ExternalEvent::Copy {id, ..} | &ExternalEvent::Move {id, ..} |
            &ExternalEvent::StaticBorrow {id, ..} | &ExternalEvent::StaticDie {id, ..} |
            &ExternalEvent::MutableBorrow {id, ..} | &ExternalEvent::MutableDie {id,..} |
            &ExternalEvent::Branch { id, .. } | &ExternalEvent::GoOutOfScope { id , ..} |
            &ExternalEvent::OwnerDropAtReassign { id, .. } |
            &ExternalEvent::ManuallyDropByHand { id, .. } |
            &ExternalEvent::TemporaryDrop { id, .. } |
            &ExternalEvent::LoopIteratorDrop { id, .. } |
            &ExternalEvent::LoopMaybeInit { id, .. } |
            &ExternalEvent::LoopExitKept { id, .. } |
            &ExternalEvent::LoopExitMove { id, .. } |
            &ExternalEvent::OwnerDropIfTryReturns { id, .. } |
            &ExternalEvent::OwnerDropUnlessReturned { id, .. } |
            &ExternalEvent::TupleTempDrop { id, .. } |
            &ExternalEvent::OrAltMove { id, .. } |
            &ExternalEvent::OrAltBorrow { id, .. } |
            &ExternalEvent::ChainFailAssign { id, .. } |
            &ExternalEvent::ChainFailUse { id, .. } |
            &ExternalEvent::TiedParamNote { id, .. } |
            &ExternalEvent::ReslicedInPlace { id, .. } |
            &ExternalEvent::DeadElseNote { id, .. } |
            &ExternalEvent::PassRestDrop { id, .. } |
            &ExternalEvent::DeadLoanNote { id, .. } |
            &ExternalEvent::RefDie { id, .. } | &ExternalEvent::Bind { id, .. } |
            &ExternalEvent::PassByStaticReference { id, .. } | &ExternalEvent::PassByMutableReference {id, .. } |
            &ExternalEvent::InitRefParam { id, .. } | &ExternalEvent::ClosureCall { id, .. } => id
        }
    }
}

// Each branch has a history of events (e_data)
// and timeline data (where the x-axis is)
// as well as the state changes
#[derive(Debug, Clone)]
pub struct BranchData {
  pub t_data: TimelineColumnData,
  pub e_data: Vec<(usize, Event)>,
  pub width: usize,
  pub states: Vec<(usize, usize, State)>,
  // See ExtBranchData::diverges.
  pub diverges: bool,
  // See ExtBranchData::exits.
  pub exits: Vec<BranchExit>,
  // The variable's state as control reaches the split, after every
  // event of the line the branch splits on (a match arm's binding
  // `Some(x) => match b {` comes before the inner split, #502). Set by
  // compute_states; every arm starts from it.
  pub entry_state: Option<State>,
}


/// What the receiver of a Copy got (#866).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum CopyOf {
    /// A copy of all of `from` (`let y = x;`).
    #[default]
    Whole,
    /// A copy of a value inside `from`, bound by a pattern (`if let
    /// Some(x) = opt`, `Fruit::Apple(z) =>` on `match x`): the
    /// ExternalEvent's `is_partial`.
    Inside,
    /// `from` was copied into a value this constructor builds, which
    /// the receiver holds (`y = Fruit::Apple(z)`): ExternalEvent's
    /// `into_ctor`.
    IntoCtor(String),
    /// A `&` sub-pattern copied the value a reference stored inside
    /// `from` points to (`let Some(&y) = orr`, #874); `part` when the
    /// receiver got only a value inside it (`Some(&(a, b))`):
    /// ExternalEvent's `pointee`.
    Pointee { part: bool },
    /// A copy of the constant named here (`let m = MAX;`, #690): the
    /// ExternalEvent's `of_const`. Its `from` is `Anonymous`.
    Const(String),
}

/// What the source of a copy says was copied out of it: a constructor's
/// argument (`IntoCtor`) is copied whole, as before #866.
fn source_copy_of(of: CopyOf) -> CopyOf {
    match of {
        CopyOf::IntoCtor(_) => CopyOf::Whole,
        of => of,
    }
}

impl CopyOf {
    fn of(is_partial: bool, into_ctor: &Option<String>, pointee: Option<bool>, of_const: &Option<String>) -> CopyOf {
        if let Some(c) = of_const {
            return CopyOf::Const(c.clone());
        }
        match (into_ctor, pointee) {
            (Some(c), _) => CopyOf::IntoCtor(c.clone()),
            (None, Some(part)) => CopyOf::Pointee { part },
            (None, None) if is_partial => CopyOf::Inside,
            (None, None) => CopyOf::Whole,
        }
    }
}

// An Event describes the acquisition or release of a
// resource ownership by a Owner on any given line.
// There are six types of them.
#[derive(Debug, Clone)]
pub enum Event {
    // this happens when a variable is initiated, it should obtain
    // its resource from either another variable or from a
    // contructor.
    //
    // E.g. in the case of
    //      let x = Vec::new();
    // x obtained the resource from global resource allocator,
    // the Acquire Event's "from" variable is None.
    // in the case of
    //      let y = x;
    // y obtained its value from x, which means that the Acquire
    // Event's "from" variable is x.
    // TODO do we need mut/static_acquire for get_state?
    // `via_try`: see ExternalEvent::Move.
    // `break_alt` (also on Duplicate, Copy and Move): the event is a
    // `break` value reaching the loop's receiver, one of several (see
    // BreakAlt).
    // `loop_head`: `Collection` when `is` is the variable of a `for` loop
    // over a collection moved into the loop, which takes one element
    // out of it per iteration (`for w in words`, #850).
    Acquire {
        from: ResourceTy,
        is: ResourceTy,
        via_try: Option<TryMove>,
        id: usize,
        break_alt: Option<BreakAlt>,
        loop_head: LoopHead,
    },
    // this happens when a ResourceAccessPoint implements copy trait or
    // explicitly calls .clone() function
    // to another ResourceAccessPoint, or a function.
    //
    // e.g.
    // 1. x: i32 = y + 15;              here y duplicate to + op, and x acquire from +
    //                                  at this point, we treat it as y duplicates to None
    // 2. x: MyStruct = y.clone();      here y duplicates to x.
    // `of`: what was copied out of `is` when it isn't all of it: a
    // value inside it (`if let Some(x) = opt`, #866), or the value a
    // reference inside it points to (`if let Some(&x) = orr`, #874).
    Duplicate {
        to: ResourceTy,
        is: ResourceTy,
        into_closure: bool,
        of: CopyOf,
        id: usize,
        break_alt: Option<BreakAlt>,
    },
    // this happens when a ResourceAccessPoint transfers a copy of its contents
    // to another ResourceAccessPoint.
    // Typically, this occurs when a resource owner implements the Copy trait.
    // `of`: what `is` got a copy of, when it isn't all of `from` (#866).
    Copy {
        from: ResourceTy,
        is: ResourceTy,
        of: CopyOf,
        id: usize,
        break_alt: Option<BreakAlt>,
    },
    // this happens when a ResourceAccessPoint transfer the ownership of its resource
    // to another ResourceAccessPoint, or if it is no longer used after this line.
    // Typically, this happens at one of the following two cases:
    //
    // 1. variable is not used after this line.
    // 2. variable's resource has the move trait, and it transfered
    //    its ownership on this line. This includes tranfering its
    //    ownership to a function as well.
    // `break_out`: see ExternalEvent::Move.
    Move {
        to: ResourceTy,
        is: ResourceTy,
        into_closure: bool,
        loop_head: LoopHead,
        via_try: Option<TryMove>,
        id: usize,
        break_alt: Option<BreakAlt>,
        break_out: bool,
        // A move out of a split scrutinee temporary that leaves nothing
        // that needs drop in it (`temp_emptying_moves`, #1386).
        empties_temp: bool,
    },

    // Mirrors the ExternalEvent Branch
    Branch {
      is: ResourceTy,
      branch_history: Vec<BranchData>,
      ty: BranchType,
      split_point: usize,
      merge_point: usize,
      // This column is a `let x;` local initialized in only some of the
      // branches that reach the merge (#244).
      maybe_init: bool,
      // A loop this conditional is inside, and that this column was
      // declared before, gives it a new value somewhere in its body
      // (#1026): when the loop comes back round, that assignment drops
      // what the join leaves it holding.
      loop_assigned: bool,
      // This column is one of the ExternalEvent's `maybe_held` (#1034).
      maybe_held: bool,
      id: usize
    },

    // `of_ref_var` (on MutableLend, MutableBorrow, MutableReacquire): see
    // ExternalEvent::MutableBorrow. The lender is a reference variable
    // borrowed itself (`&mut r`), not reborrowed through (#914).
    MutableLend {
        to: ResourceTy,
        is: ResourceTy,
        id: usize,
        of_ref_var: bool,
    },
    MutableBorrow {
        from: ResourceTy,
        is: ResourceTy,
        id: usize,
        of_ref_var: bool,
    },
    MutableDie {
        to: ResourceTy,
        is: ResourceTy,
        id: usize
    },
    // `still_borrowed_by`: see ExternalEvent::StaticDie. Non-empty,
    // the borrower gave back its part of a borrow these values still
    // hold, and the state doesn't change (#575).
    MutableReacquire {
        from: ResourceTy,
        is: ResourceTy,
        id: usize,
        still_borrowed_by: Vec<String>,
        of_ref_var: bool,
    },
    // `of_ref_var` (on StaticLend, StaticBorrow, StaticReacquire): the
    // lender is a reference variable borrowed itself (`&r`), not
    // reborrowed through (#925); see MutableLend.
    StaticLend {
        to: ResourceTy,
        is: ResourceTy,
        id: usize,
        of_ref_var: bool,
    },
    StaticBorrow {
        from: ResourceTy,
        is: ResourceTy,
        id: usize,
        of_ref_var: bool,
    },
    StaticDie {
        to: ResourceTy,
        is: ResourceTy,
        id: usize
    },
    StaticReacquire {
        from: ResourceTy,
        is: ResourceTy,
        id: usize,
        still_borrowed_by: Vec<String>,
        of_ref_var: bool,
    },

    RefDie {
      from: ResourceTy,
      is: ResourceTy,
      num_curr_borrowers: usize,
      id: usize
    },
    // this happens when a owner is returned this line,
    // or if this owner's scope ends at this line. The data must be dropped.
    // `holds`, `early`: the ExternalEvent's.
    OwnerGoOutOfScope { holds: Option<ScopeEndHolds>, early: Option<EarlyReturns> },
    // this happens when a vairable that is not an owner goes out of scope.
    // The data is not dropped in this case
    RefGoOutOfScope { early: Option<EarlyReturns> },
    // The previously-held resource is dropped at a reassignment
    // (`y = x` or `*p = x`). The owner stays alive — its state is
    // unchanged across this event because the matching Acquire from
    // rhs lands on the same line and immediately re-establishes
    // FullPrivilege. `maybe_moved`: see ExternalEvent::OwnerDropAtReassign.
    // `maybe_init`: see ExternalEvent::OwnerDropAtReassign.
    // `payload`, `loop_head`: see ExternalEvent::OwnerDropAtReassign.
    OwnerDropAtReassign { maybe_moved: bool, by_cond: bool, loop_head: bool, parts: Option<ReassignParts>, maybe_init: Option<InitBy>, payload: bool, contents_gone: bool },
    // `ManuallyDrop::drop(&mut md)`: the value inside is dropped by
    // hand. The owner is left in a moved-out-like state.
    ManuallyDropByHand,
    // An unbound temporary is dropped (see ExternalEvent).
    TemporaryDrop { ty: Option<String>, when: String, literal: bool, early: bool, part: Option<TempPart>, assign: bool },
    // See ExternalEvent::LoopIteratorDrop.
    LoopIteratorDrop { left: LoopLeft, early: Vec<usize> },
    // See ExternalEvent::LoopMaybeInit.
    LoopMaybeInit { before_loop: Option<bool>, nested: bool },
    // See ExternalEvent::LoopExitKept.
    LoopExitKept { reassigned: bool },
    // See ExternalEvent::LoopExitMove.
    LoopExitMove { definite: bool, at: MaybeMovedAt },
    // The owner's resource is dropped here only if the `?` returns
    // early (#430). The owner's state is unchanged.
    // `maybe`: rustc's move data says the variable only may hold a value
    // where this `?` returns early (#1600): a drop flag decides.
    OwnerDropIfTryReturns { maybe: bool, held: bool },
    // See ExternalEvent::OwnerDropUnlessReturned.
    OwnerDropUnlessReturned,
    // See ExternalEvent::TupleTempDrop.
    TupleTempDrop { tuple: String, at: TupleDropAt, with: Vec<TupleLeft> },
    // See ExternalEvent::OrAltMove. `value`: the place holds a value
    // with nothing to drop, not a resource (#484).
    OrAltMove { into: String, sources: Vec<String>, tuple: Option<String>, part_of: Option<(String, String)>, value: bool, in_let: bool },
    // The place's side of an ExternalEvent::OrAltBorrow: it may be
    // borrowed by `into` (`end: false`), or that borrow ends (`end:
    // true`). Changes the place's state like a StaticLend / MutableLend
    // and StaticReacquire / MutableReacquire would.
    // `chain`: see ExternalEvent::OrAltBorrow (#603).
    OrAltLend { into: ResourceAccessPoint, is: ResourceTy, sources: Vec<String>, mutable: bool, end: bool, chain: Option<ChainRef> },
    // The reference's side of an ExternalEvent::OrAltBorrow: its borrow
    // of one of `sources` starts or ends. Changes its state like a
    // StaticBorrow / MutableBorrow and StaticDie / MutableDie would.
    OrAltBorrow { is: ResourceTy, sources: Vec<String>, mutable: bool, end: bool, chain: Option<ChainRef> },
    // See ExternalEvent::ChainFailAssign. `rc`: the variable's
    // `rc_drop()`, for what dropping the old value does.
    ChainFailAssign { place: Option<String>, drops_old: bool, rc: Option<RcDrop>, path: FailPath },
    // See ExternalEvent::ChainFailUse. `is_ref`: the column is a
    // reference, so what was lent is the resource it points to.
    ChainFailUse { uses: Vec<ChainUse>, definite: bool, path: FailPath, is_ref: bool, pointee_value: bool },
    // See ExternalEvent::TiedParamNote.
    TiedParamNote { lifetime: String },
    // See ExternalEvent::ReslicedInPlace.
    ReslicedInPlace { lender: String, mutable: bool, part: bool },
    // See ExternalEvent::DeadElseNote.
    DeadElseNote,
    // See ExternalEvent::PassRestDrop.
    PassRestDrop { parts: Vec<String> },
    // A struct's field while the struct is borrowed as a whole by a call
    // on this row (`c.reset()`, `inc(&mut c)`, `show(&c)`; #1311): the
    // field is borrowed with it, and the borrow ends on the row. Added to
    // the field's column by `note_call_borrows_on_members` (a column-only
    // note: no state change). `to`: who borrows the struct.
    LentWithParent { parent: String, to: String, mutable: bool },
    // See ExternalEvent::DeadLoanNote. `shown`: the column is drawn
    // borrowed at the write (set while computing its states); a note
    // on a column that isn't is dropped then.
    DeadLoanNote { lender: String, borrower: String, repoint: Option<(usize, bool)>, shown: bool },
    // SPECIAL CASE: use only to initialize a fn's paramter
    // Requires param to be Owner, StaticRef, or MutRef (cannot be Function)
    // (`own_param`, `in_pattern`: see ExternalEvent::InitRefParam)
    InitRefParam {
        param: ResourceAccessPoint,
        id: usize,
        own_param: Option<OwnParam>,
        in_pattern: Option<String>,
        rest: Option<ParamRest>,
    },
    // `f()` of an `FnOnce` closure `is`: `is` is moved into the call
    // (ExternalEvent::ClosureCall).
    ClosureCall {
        is: ResourceTy,
        fates: Vec<CapturedFate>,
        id: usize
    },
}

/// What a fn parameter's pattern leaves of the argument (#1181).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ParamRest {
    /// The function the parameter belongs to.
    pub fn_name: String,
    /// The parts of the argument the pattern leaves unbound that need
    /// drop, as field paths (`bio`, `1`, `inner.tag`). Empty: the
    /// pattern binds no name at all (`_: String`), and the whole
    /// argument is in a `(parameter)` column of its own (#1243).
    pub parts: Vec<String>,
}

/// A `ref` / `ref mut` binding in a by-value fn parameter's pattern
/// borrows the parameter value, which the function owns (#377).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct OwnParam {
    /// The function the parameter belongs to.
    pub fn_name: String,
    /// `ref s: String` (the whole value) rather than `(ref a, b)`
    /// (part of it).
    pub whole: bool,
    /// The borrowed value's type needs drop: no column shows that drop
    /// (when `fn_name` returns), so the dot's tooltip states it.
    pub drops: bool,
}



#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LineState {
    Full,
    Gray,
    // Dashed segment of a variable that has no value yet on this
    // path: `let s;` before a branch assigns it, or the
    // `let s = if … { … }` binding before its branch produces a value
    // (#234). Converts back to OutOfScope.
    Uninit,
}

// A State is a description of a ResourceAccessPoint IMMEDIATELY AFTER a specific line.
// We think of this as what read/write access we have to its resource.
#[derive(Clone, Debug)]
pub enum State {
    // The viable is no longer in the scope after this line.
    OutOfScope,
    // The resource is transferred on this line or before this line,
    // thus it is impossible to access this variable anymore.
    ResourceMoved {
        move_to: ResourceTy,
        move_at_line: usize,
        // The value was dropped by `ManuallyDrop::drop` rather than
        // moved anywhere (`move_to` is then Anonymous).
        dropped_by_hand: bool
    },
    // This ResourceAccessPoint is the unique object that holds the ownership to the underlying resource.
    FullPrivilege {
        s: LineState
    },
    // More than one ResourceAccessPoint has access to the underlying resource
    // This means that it is not possible to create a mutable reference
    // on the next line.
    // About borrow_count: this value is at least one at any time.
    //      When the first static reference of this ResourceAccessPoint is created,
    //          this value is set to 1;
    //      When a new static reference is borrowed from this variable, increment by 1;
    //      When a static reference goes out of scope, decrement this value by 1;
    //      When a decrement happens while the borrow_count is 1, the state becomes
    //          FullPrivilege once again.
    PartialPrivilege {
        s: LineState
    },
    // temporarily no read or write access right to the resource, but eventually
    // the privilege will come back. Occurs when mutably borrowed
    RevokedPrivilege {
        to: ResourceTy,
        borrow_to: ResourceTy,
        prev_state: Box<State>
    },
    // should not appear for visualization in a correct program
    Invalid,
}

/// A diverging branch that leaves only by `break`ing out of a loop
/// (or a labeled block): what follows that is reached.
pub fn breaks_out_only(exits: &[BranchExit]) -> bool {
    !exits.is_empty() && exits.iter().all(|x| matches!(x, BranchExit::Break | BranchExit::BreakBlock))
}

impl std::fmt::Display for State {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result {
        match self {
            State::OutOfScope => write!(f, "OutOfScope"),
            State::ResourceMoved { .. } => write!(f, "ResourceMoved"),
            State::FullPrivilege { .. } => write!(f, "FullPrivilege"),
            State::PartialPrivilege { .. } => write!(f, "PartialPrivilege"),
            State::RevokedPrivilege { .. } => write!(f, "RevokedPrivilege"),
            State::Invalid => write!(f, "Invalid"),
        }
    }
}


/// What a holder's borrow is of, for `event_dot_holder_borrow`: what a
/// reference points to, a field reached through one (`self.items` in
/// `let it = self.items.iter();`, #1080), or a variable's own data.
pub(crate) fn holder_lender_kind(rty: &ResourceTy) -> hover_messages::HolderLender {
    match rty {
        ResourceTy::DerefField(..) => hover_messages::HolderLender::FieldThroughRef,
        _ if rty.is_ref() => hover_messages::HolderLender::Pointee,
        _ => hover_messages::HolderLender::Own,
    }
}

// Whether `rty` is an owner (`i32`, a Copy struct or field, or a non-Copy
// one with nothing to drop), whose tooltips say "value" rather than
// "resource" (#274, #484). A `*r`
// place isn't: what it holds belongs to r's lender.
pub(crate) fn holds_copy_value(rty: &ResourceTy) -> bool {
    lender_holds_value(rty)
        // A borrow from / returned to a reference r (or `*r`) is of
        // what r points to (#313), a value if that's Copy (#312).
        || matches!(rty, ResourceTy::Value(r) | ResourceTy::Deref(r) if r.points_to_copy())
}

// Whether the lender `rty` of a borrow holds a "value" (#274, #484): an
// owner of a Copy type or one with nothing to drop, or such a field
// reached through a reference (`&self.n` with `n: usize`, #1100).
pub(crate) fn lender_holds_value(rty: &ResourceTy) -> bool {
    matches!(rty, ResourceTy::Value(r) if r.holds_value())
        || matches!(rty, ResourceTy::DerefField(_, _, _, FieldHolds { value: true }))
}

// Whether a borrow's referent is a "value" in the dot's tooltip: the
// lender holds one, or the type of what the borrower points to has
// nothing to drop. That covers a referent with no column of its own
// (`&{}`, `&5`, `&S { a: 4 }`, #867; referents that need drop get a
// `Temporary` column and stay a "resource") and a Copy part of a lender
// that needs drop (`&mut v[0]` with `v: Vec<i32>`, `&s[..]`: the
// reference points to an `i32` / a `str`, #1233), as the borrower's own
// line already says. Not for `*el` (el: `&mut &String`): el's pointee is
// the reference, not what `*el` points to.
// Whether what a loan is of is a "value", for the dots that describe
// the loan a reference was made from rather than what the reference
// itself points to (#1122: `ra: &str` made from a mutable borrow of the
// String a; #1131: `r = &mut &x` points to a temporary reference that
// borrows x). The borrower's own pointee (a `str`, a reference) has
// nothing to drop there although the loan's lender does, so #1233's
// pointee rule in `borrow_holds_value` doesn't apply.
fn loan_holds_value(from: &ResourceTy, borrower: &ResourceTy) -> bool {
    holds_copy_value(from) || (matches!(from, ResourceTy::Anonymous) && borrow_holds_value(from, borrower))
}

fn borrow_holds_value(from: &ResourceTy, borrower: &ResourceTy) -> bool {
    holds_copy_value(from)
        || matches!(borrower, ResourceTy::Value(r) if r.points_to_copy())
        || (matches!(from, ResourceTy::Anonymous)
            // A `static` set to `&[3, 4]` borrows a promoted constant,
            // which can't need drop (#1162).
            && borrower.extract_rap().is_some_and(|r| r.points_to_copy() || r.is_static()))
}

// The lender of a borrow, when it is a reference r: the borrow is then
// of what r points to, reached through r (#313).
fn ref_lender(rty: &ResourceTy) -> Option<&ResourceAccessPoint> {
    match rty {
        ResourceTy::Value(r) | ResourceTy::Deref(r) if r.is_ref() => Some(r),
        _ => None,
    }
}

// Name of the variable / function on the other end of a borrow, if any.
fn borrower_name(rty: &ResourceTy) -> Option<String> {
    match rty {
        ResourceTy::Value(r) | ResourceTy::Deref(r) => Some(r.name().to_owned()),
        _ => None,
    }
}

// Whether `rty` is an owner whose contents are a "value", not a resource
// (#484): of a non-Copy type with nothing to drop, or a Copy field that
// moves along with its non-Copy parent (`takes(p)` moves `p.x: i32` too).
// An ordinary Copy value is copied, never moved, so a Move or Acquire of
// one only comes from that parent case.
fn is_plain(rty: &ResourceTy) -> bool {
    matches!(rty, ResourceTy::Value(r) if r.holds_value())
}

// The name a tooltip uses for the other end of an event: a dereferenced
// target reads `*r`.
fn target_name(some_target: &ResourceTy) -> String {
    match some_target {
        ResourceTy::Deref(r) => {
            let mut temp = r.name().clone();
            temp.insert(0, '*');
            temp
        }
        _ => some_target.name()
    }
}


impl State {
    pub fn print_message_with_name(&self, my_name: &String) -> String {
        match self {
            State::OutOfScope => {
                hover_messages::state_out_of_scope(my_name)
            }
            State::ResourceMoved{ dropped_by_hand: true, .. } => {
                hover_messages::state_dropped_by_hand(my_name)
            }
            State::ResourceMoved{ move_at_line: _, .. } => {
                hover_messages::state_resource_moved(my_name)
            }
            State::FullPrivilege { s: LineState::Uninit } => {
                hover_messages::state_uninitialized(my_name)
            }
            State::FullPrivilege {..}=> {
                hover_messages::state_full_privilege(my_name)
            }
            State::PartialPrivilege { .. } => {
                hover_messages::state_partial_privilege(my_name, false)
            }
            State::RevokedPrivilege { to: _, borrow_to: _, prev_state: _} => {
                hover_messages::state_resource_revoked(my_name)
            }
            State::Invalid => {
                hover_messages::state_invalid(my_name)
            }
        }
    }
}

/// A conditional after which the value may be held or not: a
/// `maybe_held` join (#1034), or one with a branch that gets to the join
/// after such a conditional of its own (#1413), or after a loop that may
/// have ended before assigning it (#1495).
pub(crate) fn leaves_maybe_held(e: &Event) -> bool {
    match e {
        Event::Branch { maybe_held: true, .. } => true,
        Event::Branch { branch_history, .. } => branch_history.iter()
            .any(|b| !b.diverges && b.e_data.last().is_some_and(|(_, e)|
                leaves_maybe_held(e) || matches!(e, Event::LoopMaybeInit { .. }))),
        _ => false,
    }
}

impl PartialOrd for State {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for State {
    fn cmp(&self, other: &Self) -> Ordering {
        use State::*;
        
        // The join of a conditional takes the lowest-ranked state of
        // the branches that reach it. A value moved in one branch and
        // mutably lent in another is maybe-moved: it can't be used again
        // until it is assigned, even once the borrow ends, so a move
        // outranks a mutable lend (#1132). With the two tied, the first
        // branch won, and a mutable lend in it gave the value back as
        // "the owner" when the borrow ended.
        fn rank(s: &State) -> u8 {
            match s {
                FullPrivilege { s: LineState::Uninit } => 4,
                ResourceMoved { .. } => 0,
                RevokedPrivilege { .. } => 1,
                PartialPrivilege { .. } => 2,
                FullPrivilege {..} => 3,
                Invalid | OutOfScope => 4,
            }
        }

        let self_rank = rank(self);
        let other_rank = rank(other);

        self_rank.cmp(&other_rank)
    }
}

impl PartialEq for State {
    fn eq(&self, other: &State) -> bool {
        match (self, other) {
            (State::OutOfScope, State::OutOfScope) => true,
            (State::PartialPrivilege { s }, State::PartialPrivilege { s: s2 }) => s == s2,
            (State::FullPrivilege { s }, State::FullPrivilege { s: s2 }) => s == s2,
            (State::ResourceMoved { .. }, State::ResourceMoved { .. }) => true,
            _ => false
        }
    }
}

impl Eq for State {}


/// Convert a state to its "passive" branch flavour — what the
/// rendering layer turns into a dashed, half-opacity vertical so
/// readers can tell which arm is the active one on a given row.
///
/// `OutOfScope` is converted to `FullPrivilege { Gray }` instead of
/// staying `OutOfScope` so let-as-rhs bindings (where the variable
/// doesn't exist before the conditional) still get a visible
/// passive column on their inactive branch — without this, the
/// pre-event segment of every let-as-rhs branch rendered as
/// nothing at all. It becomes the `Uninit` flavour, not `Gray`, so
/// its tooltip says the variable has no value yet instead of calling
/// it the owner (#234).
pub fn branch_state_converter(s: &State) -> State {
    match s {
        State::FullPrivilege { s: LineState::Uninit } => s.clone(),
        State::FullPrivilege { .. } => State::FullPrivilege { s: LineState::Gray },
        State::PartialPrivilege { .. } => State::PartialPrivilege { s: LineState::Gray },
        State::OutOfScope => State::FullPrivilege { s: LineState::Uninit },
        _ => s.clone()
    }
}

/// Why a `let x;` local may have no value where it's assigned, so that
/// a drop flag decides whether the assignment drops an old value.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InitBy {
    /// Only some branches of a conditional above assigned it (#244).
    Branch,
    /// A loop above assigns it, but may have ended before doing so
    /// (#786).
    Loop,
    /// The assignment is in a loop body and x had no value before the
    /// loop: the first time round it initializes x, later times round
    /// it drops what an earlier iteration assigned, if x still holds it
    /// (#786).
    EarlierIteration,
    /// The first thing the body of a loop inside another loop does to
    /// a variable declared before the outer one, which only may hold a
    /// value where the inner loop is entered (a branch above moved it
    /// on some paths): as `EarlierIteration` with `maybe_moved` (#1012),
    /// but a run of the inner loop may follow an earlier one, so its
    /// first time round isn't only after the branch (#1019). `moves`:
    /// the loops may also move it, so only "if it still holds one" is
    /// sure.
    OuterLoop { moves: bool },
    /// An assignment in a loop's body to a variable that rustc says
    /// only may hold a value here (#1027): a loop above, inside this
    /// one, may have ended before giving it a value (`if c { drop(s); }
    /// for … { while … { s = …; } s = …; }`), drawn as a `LoopMaybeInit`
    /// with `nested` at that loop's end (#1032). Only a drop flag knows.
    DropFlag,
    /// As `EarlierIteration` with `maybe_moved`, in a loop that never
    /// comes back round (every pass leaves it, #1467): the old value is
    /// dropped only if no branch above moved it, and the loop may end
    /// before the assignment runs (a `while` / `for` whose body doesn't
    /// run, a `break` before it): then the old value, if any, is still
    /// there after the loop (a `LoopMaybeInit` at its end says so).
    FirstPass,
    /// An assignment outside a loop to a variable that rustc says only
    /// may hold a value here, where nothing drawn above makes it
    /// maybe-moved (#1587): a loop above may have ended before giving it
    /// a value, and a conditional after it gave it one in some branches
    /// only. As `DropFlag`, but not in a loop's body: it runs once.
    MaybeHeld,
}

/// What a variable holds where it goes out of scope at the `}` of its
/// block, on the paths that get there through the code before it, as
/// rustc's move data says (#1036, #1037).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScopeEndHolds {
    /// A value, on every such path: it's dropped there.
    Value,
    /// A value on some paths and not others: a runtime drop flag
    /// decides whether anything is dropped.
    Maybe,
    /// Nothing, on any such path: nothing is dropped.
    Nothing,
}

/// What a conditional's join leaves in a `let x;` local that only some
/// of the branches reaching it assigned (#244; the Branch event's
/// `maybe_init`), the others leaving it with no value.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InitJoin {
    /// Some branches leave x holding a value. Rust tracks whether it
    /// does with a drop flag, and x can't be read after the join until
    /// it's assigned again. `moved`: some other branch assigned x and
    /// then moved its value away.
    Maybe { moved: bool },
    /// Every branch that assigned x moved its value away: x holds
    /// nothing after the join on any path.
    AllMoved,
    /// Not a conditional's join: a loop above assigns x but may have
    /// ended before doing so (`ExternalEvent::LoopMaybeInit`, #786).
    Loop,
}

/// `InitJoin` of a conditional from the end states `ends` of its
/// branches that reach the join (not one that returns or panics).
pub fn init_join<'a>(maybe_init: bool, ends: impl IntoIterator<Item = &'a State> + Clone) -> Option<InitJoin> {
    let unassigned = |s: &State| matches!(s, State::OutOfScope | State::FullPrivilege { s: LineState::Uninit });
    if !maybe_init || !ends.clone().into_iter().any(unassigned) {
        return None;
    }
    let moved = ends.clone().into_iter().any(|s| matches!(s, State::ResourceMoved { .. }));
    let holds = ends.into_iter().any(|s| matches!(s,
        State::FullPrivilege { .. } | State::PartialPrivilege { .. } | State::RevokedPrivilege { .. })
        && !unassigned(s));
    match (holds, moved) {
        (true, _) => Some(InitJoin::Maybe { moved }),
        (false, true) => Some(InitJoin::AllMoved),
        (false, false) => None,
    }
}

pub fn convert_back(s: &State) -> State {
    match s {
        State::FullPrivilege { s: LineState::Uninit } => State::OutOfScope,
        State::FullPrivilege { .. } => State::FullPrivilege { s: LineState::Full },
        State::PartialPrivilege { .. } => State::PartialPrivilege { s: LineState::Full },
        _ => s.clone() 
    }
}

/// The end of a reference column's borrow (StaticDie / MutableDie on
/// the reference itself), or the reference handed to the caller by a
/// `return` (a copy of it, so the column has nothing left).
fn ends_reference(e: &Event) -> bool {
    matches!(e,
        Event::StaticDie { .. } | Event::MutableDie { .. }
        | Event::Duplicate { to: ResourceTy::Caller, .. })
}

/// An event that gives a reference column a (new) reference. Not a
/// write through the column into what it points to (`*r = &t`, `is`
/// is `*r`, #904): r keeps the reference it has, so its own end on
/// that row isn't the end of an old one, and its line ends there.
fn starts_reference(e: &Event) -> bool {
    match e {
        Event::StaticBorrow { is, .. } | Event::MutableBorrow { is, .. }
        | Event::Acquire { is, .. } | Event::Copy { is, .. } =>
            !matches!(is, ResourceTy::Deref(_) | ResourceTy::DerefField(..)),
        Event::InitRefParam { .. } => true,
        _ => false,
    }
}

/// Whether a reference column still holds a reference after the events
/// before `rest`: its next event uses the reference it has, rather than
/// giving it a new one or ending its scope. A borrow made through it
/// (`println!("{}", r)`, `&mut *r`), a move or copy of it, a return to
/// the caller, or the end of its borrow all use it. In a conditional, an
/// arm that uses it first is enough; one that doesn't touch the column
/// is passed over.
fn reference_used_later(rest: &[(usize, Event)]) -> bool {
    for (_, e) in rest {
        match e {
            Event::StaticLend { .. } | Event::MutableLend { .. }
            | Event::StaticDie { .. } | Event::MutableDie { .. }
            | Event::Duplicate { .. } | Event::Move { .. } => return true,
            Event::Branch { branch_history, .. } => {
                if branch_history.iter().any(|b| reference_used_later(&b.e_data)) {
                    return true;
                }
                if branch_history.iter().any(|b| !b.e_data.is_empty()) {
                    return false;
                }
            }
            _ => return false,
        }
    }
    false
}

/// A struct or tuple borrowed as a whole (`let r = &mut p;`, `let (a, b)
/// = &mut t;`) lends every field with it: rustc rejects a use of `p.name`
/// while r is live (E0502), as it does a use of p. The borrow's events are
/// on p's column only, so its fields' columns were drawn as unborrowed
/// owners (#1130, #1193). Draw each field (and a nested struct's fields)
/// in p's state for as long as p is lent, where the field itself is held
/// and not borrowed more strongly. Rows of top-level timelines only.
fn lend_struct_borrows_to_members(timelines: &mut BTreeMap<u64, Timeline>) {
    // (parent name, the owner hash its members carry, lent spans)
    let mut lent: Vec<(String, u64, Vec<(usize, usize, State)>)> = Vec::new();
    for (hash, t) in timelines.iter() {
        let ResourceAccessPoint::Struct(st) = &t.resource_access_point else { continue };
        let spans: Vec<(usize, usize, State)> = t.states.iter()
            .filter(|(a, b, s)| a < b && matches!(s, State::RevokedPrivilege { .. } | State::PartialPrivilege { .. }))
            .cloned()
            .collect();
        if !spans.is_empty() {
            lent.push((st.name.clone(), if st.is_member { st.owner } else { *hash }, spans));
        }
    }
    if lent.is_empty() {
        return;
    }
    for t in timelines.values_mut() {
        let (name, owner) = match &t.resource_access_point {
            ResourceAccessPoint::Struct(m) if m.is_member => (m.name.clone(), m.owner),
            ResourceAccessPoint::Owner(_) | ResourceAccessPoint::Struct(_) | ResourceAccessPoint::MutRef(_)
            | ResourceAccessPoint::StaticRef(_) | ResourceAccessPoint::Function(_) => continue,
        };
        for (pname, powner, spans) in lent.iter() {
            if *powner != owner || !name.starts_with(&format!("{}.", pname)) {
                continue;
            }
            for (a, b, pstate) in spans {
                let mut out: Vec<(usize, usize, State)> = Vec::new();
                for (x, y, s) in t.states.drain(..) {
                    let held = matches!(s, State::FullPrivilege { s: LineState::Full })
                        || (matches!(s, State::PartialPrivilege { .. }) && matches!(pstate, State::RevokedPrivilege { .. }));
                    let (lo, hi) = (x.max(*a), y.min(*b));
                    if !held || lo >= hi {
                        out.push((x, y, s));
                        continue;
                    }
                    let lent_state = match pstate {
                        State::RevokedPrivilege { to, borrow_to, .. } =>
                            State::RevokedPrivilege { to: to.clone(), borrow_to: borrow_to.clone(), prev_state: Box::new(s.clone()) },
                        _ => State::PartialPrivilege { s: LineState::Full },
                    };
                    if x < lo { out.push((x, lo, s.clone())); }
                    out.push((lo, hi, lent_state));
                    if hi < y { out.push((hi, y, s)); }
                }
                t.states = out;
            }
        }
    }
}


/// #1311: a struct borrowed as a whole by a call (`c.reset()`, `inc(&mut
/// c)`, `show(&c)`) is lent and given back on one row, so it has no lent
/// span for `lend_struct_borrows_to_members` to draw on its fields: their
/// columns showed nothing on that row, as if the call couldn't touch them
/// (`reset` writes `c.n`). Each field column that holds its value there
/// gets a dot saying it is borrowed along with the struct. Not where the
/// field has an event of its own on the row (its own dot speaks for it),
/// nor where its column branches around the row (its top-level states
/// don't hold it there).
fn note_call_borrows_on_members(timelines: &mut BTreeMap<u64, Timeline>) {
    // A lend given back to the same borrower on the same row, also in
    // a branch's arm: (row, borrower, mutable).
    fn call_lends(history: &[(usize, Event)], out: &mut Vec<(usize, String, bool)>) {
        for (i, (l, e)) in history.iter().enumerate() {
            match e {
                Event::MutableLend { to, .. } | Event::StaticLend { to, .. } => {
                    let mutable = matches!(e, Event::MutableLend { .. });
                    let back = history[i + 1..].iter().take_while(|(l2, _)| l2 == l).any(|(_, e2)| match e2 {
                        Event::MutableReacquire { from, .. } if mutable => from.name() == to.name(),
                        Event::StaticReacquire { from, .. } if !mutable => from.name() == to.name(),
                        _ => false,
                    });
                    if back && !out.iter().any(|(l2, t2, m2)| l2 == l && *t2 == to.name() && *m2 == mutable) {
                        out.push((*l, to.name(), mutable));
                    }
                }
                Event::Branch { branch_history, .. } => {
                    for b in branch_history {
                        call_lends(&b.e_data, out);
                    }
                }
                _ => {}
            }
        }
    }
    fn has_event_at(history: &[(usize, Event)], line: usize) -> bool {
        history.iter().any(|(l, e)| *l == line || match e {
            Event::Branch { branch_history, .. } => branch_history.iter().any(|b| has_event_at(&b.e_data, line)),
            _ => false,
        })
    }
    // (parent name, the owner hash its members carry, row, borrower, mutable)
    let mut calls: Vec<(String, u64, usize, String, bool)> = Vec::new();
    for (hash, t) in timelines.iter() {
        let ResourceAccessPoint::Struct(st) = &t.resource_access_point else { continue };
        let mut lends = Vec::new();
        call_lends(&t.history, &mut lends);
        for (line, to, mutable) in lends {
            calls.push((st.name.clone(), if st.is_member { st.owner } else { *hash }, line, to, mutable));
        }
    }
    if calls.is_empty() {
        return;
    }
    // The innermost struct first (`c.inner` before `c`), so a field says
    // what it is borrowed along with most directly.
    calls.sort_by_key(|(p, _, l, _, _)| (*l, std::cmp::Reverse(p.len())));
    for t in timelines.values_mut() {
        let (name, owner) = match &t.resource_access_point {
            ResourceAccessPoint::Struct(m) if m.is_member => (m.name.clone(), m.owner),
            _ => continue,
        };
        for (pname, powner, line, to, mutable) in calls.iter() {
            if *powner != owner || !name.starts_with(&format!("{}.", pname)) || has_event_at(&t.history, *line) {
                continue;
            }
            let held = t.states.iter().any(|(a, b, s)| a < b && *a <= *line && *line <= *b
                && matches!(s, State::FullPrivilege { s: LineState::Full } | State::PartialPrivilege { .. }));
            if !held {
                continue;
            }
            let at = t.history.iter().position(|(l, _)| *l > *line).unwrap_or(t.history.len());
            t.history.insert(at, (*line, Event::LentWithParent { parent: pname.clone(), to: to.clone(), mutable: *mutable }));
        }
    }
}

/// #700, #722: a reference column's borrow end recorded on the row
/// where it gets a new reference, after that new reference (`y = &x;`
/// ends y's old borrow and starts a new one), is the old value's end
/// when it gives back a lender none of the row's new borrows borrow
/// from, or when the column goes on using a reference afterwards (so
/// the new one doesn't end here). Put such ends before the new
/// reference, so the row's dot reads "y's immutable borrow ends • y
/// immutably borrows …" and the column goes on from it holding the new
/// one. An end of the new reference (`r = &b;` with r unused after)
/// stays after it. The rows and arrows don't change: only the order of
/// the column's own events on that row.
fn old_borrow_ends_first(history: &mut [(usize, Event)]) {
    // A named lender's column (not `Deref(r)`, the stand-in for a
    // temporary or a value the plugin doesn't show).
    let lender = |r: &ResourceTy| match r {
        ResourceTy::Value(r) => Some(*r.hash()),
        _ => None,
    };
    let mut i = 0;
    while i < history.len() {
        let row = history[i].0;
        let mut j = i;
        while j < history.len() && history[j].0 == row {
            j += 1;
        }
        if let Some(first_start) = (i..j).find(|&k| starts_reference(&history[k].1)) {
            let used_later = reference_used_later(&history[j..]);
            // The named lenders of the row's new borrows, or None if the
            // new reference is a copy or move of another one (whose
            // lender isn't recorded here) or borrows something unnamed.
            let new_lenders: Option<Vec<u64>> = history[first_start..j].iter()
                .filter(|(_, e)| starts_reference(e))
                .map(|(_, e)| match e {
                    Event::StaticBorrow { from, .. } | Event::MutableBorrow { from, .. } => lender(from),
                    _ => None,
                })
                .collect();
            let old_end = |e: &Event| match e {
                Event::StaticDie { to, .. } | Event::MutableDie { to, .. } => used_later
                    || new_lenders.as_ref().zip(lender(to)).is_some_and(|(ls, l)| !ls.contains(&l)),
                _ => false,
            };
            // A read of the old value that the new one's right-hand
            // side makes (`r = if *r > 0 { &b } else { &c };` copies
            // `*r`), recorded after the new borrows because the row's
            // arrows were appended first (#771): evaluated first, so it
            // goes before the end too. Its id is older than the new
            // reference's.
            let first_id = history[first_start..j].iter()
                .filter(|(_, e)| starts_reference(e))
                .map(|(_, e)| e.get_id())
                .min()
                .unwrap_or(0);
            let is_read = |e: &Event| matches!(e,
                Event::Duplicate { .. } | Event::StaticLend { .. } | Event::MutableLend { .. } | Event::Move { .. })
                && e.get_id() < first_id;
            let (reads, others): (Vec<_>, Vec<_>) = history[first_start..j].iter().cloned()
                .partition(|(_, e)| is_read(e));
            let (ends, rest): (Vec<_>, Vec<_>) = others.into_iter()
                .partition(|(_, e)| old_end(e));
            for (slot, ev) in history[first_start..j].iter_mut().zip(reads.into_iter().chain(ends).chain(rest)) {
                *slot = ev;
            }
        }
        i = j;
    }
}

/// For each event of a reference column's `history`: whether it is an
/// end (`ends_reference`) that leaves the column holding a reference,
/// because the column goes on using one (`reference_used_later`). The
/// column's line then runs on through it. Two causes: the end is of
/// the reference the column held before an assignment, drawn on a row
/// after the new one (`(r1, r2) = (r2, r1);`, `t = first(&y, &x);`),
/// or the new reference has no event of its own (`hold = other;`
/// before #637); or it is a `return` of the reference on one path of a
/// conditional that isn't drawn as branches (inside a loop).
fn ends_kept_alive(is_ref: bool, history: &[(usize, Event)]) -> Vec<bool> {
    (0..history.len())
        .map(|i| is_ref && ends_reference(&history[i].1) && reference_used_later(&history[i + 1..]))
        .collect()
}

fn holds_reference(state: &State) -> bool {
    matches!(state, State::FullPrivilege { .. } | State::PartialPrivilege { .. })
}

// merge redundant states together
pub fn clean_states(states: &Vec<(usize, usize, State)>) -> Vec<(usize, usize, State)> {
    let mut cleaned_states: Vec<(usize, usize, State)> = Vec::new();
    let mut i = 0;
    while i < states.len() {
        let mut j = i + 1;
        let mut ending_range = states[i].1;
        while j < states.len() && states[j].2 == states[i].2 {
            ending_range = states[j].1;
            j += 1;
        }
        cleaned_states.push((states[i].0, ending_range, states[i].2.clone()));
        i = j;
    }

    cleaned_states
}



// provide string output for usages like format!("{}", eventA)
impl Display for Event {
    fn fmt(&self, f: &mut Formatter) -> Result {       
        match self {
            Event::Acquire{ from , ..} => { write!(f, "Acquiring resource from {}", from.name()) },
            Event::Duplicate{ to , ..} => { write!(f, "Duplicating resource to {}", to.name())},
            Event::Copy{ from , ..} => { write!(f, "Copying resource from {}", from.name())},
            Event::Move{ to , ..} => {write!(f, "Moving resource to {}", to.name())},
            Event::MutableLend{ to , ..} => {write!(f, "Mutable lend to {}", to.name())},
            Event::MutableBorrow{ from , ..} => { write!(f, "Fully borrows resource from {}", from.name())},
            Event::MutableDie{ to , ..} => { write!(f, "Fully returns resource to {}", to.name())},
            Event::MutableReacquire{ from, .. } => {write!(f, "Fully reacquires resource from {}", from.name())},
            Event::StaticLend{ to , ..} => {write!(f, "Partially lends resource to {}", to.name())},
            Event::StaticBorrow{ from , ..} => { write!(f, "Partially borrows resource from {}", from.name())},
            Event::StaticDie{ to , ..} => { write!(f, "Partially returns resource to {}", to.name())},
            Event::StaticReacquire{ from , ..} => { write!(f, "Partially acquires resource from {}", from.name()) },
            Event::InitRefParam{ param: _ , ..} => { write!(f, "Function parameter is initialized") },
            Event::ClosureCall{ is, .. } => { write!(f, "{} is moved into the call", is.name()) },
            Event::OwnerGoOutOfScope { .. } => {
                write!(f, "Goes out of Scope as an owner of resource" ) }
            Event::RefGoOutOfScope { .. } => {
                write!(f, "Goes out of Scope as a reference to resource")
            },
            Event::OwnerDropAtReassign { .. } => {
                write!(f, "Previous resource is dropped at reassignment")
            },
            Event::ManuallyDropByHand => {
                write!(f, "Value is dropped by hand")
            },
            Event::TemporaryDrop { ty, when, .. } => {
                write!(f, "Temporary {} is dropped {}", ty.as_deref().unwrap_or("(let-borrowed)"), when)
            },
            Event::LoopIteratorDrop { .. } => {
                write!(f, "The for loop's iterator is dropped")
            },
            Event::LoopMaybeInit { .. } => {
                write!(f, "May have no value when the loop ends")
            },
            Event::LoopExitKept { .. } => {
                write!(f, "Owns a value when the loop ends")
            },
            Event::LoopExitMove { .. } => {
                write!(f, "May have been moved when the loop ends")
            },
            Event::OwnerDropIfTryReturns { .. } => {
                write!(f, "Resource is dropped if the `?` returns early")
            },
            Event::OwnerDropUnlessReturned => {
                write!(f, "Resource is dropped unless the `return` hands it to the caller")
            },
            Event::TupleTempDrop { tuple, .. } => {
                write!(f, "Temporary tuple {} is dropped", tuple)
            },
            Event::OrAltMove { into, .. } => {
                write!(f, "May have been moved into {}", into)
            },
            Event::OrAltLend { into, end, .. } => {
                if *end { write!(f, "May have been borrowed by {} until here", into.name()) }
                else { write!(f, "May be borrowed by {}", into.name()) }
            },
            Event::OrAltBorrow { end, .. } => {
                if *end { write!(f, "Borrow of one of several places ends") }
                else { write!(f, "Borrows one of several places") }
            },
            Event::ChainFailAssign { .. } => {
                write!(f, "May have been assigned by the condition")
            },
            Event::ChainFailUse { .. } => {
                write!(f, "May have been borrowed by the condition")
            },
            Event::TiedParamNote { .. } => {
                write!(f, "Not returned on this path; its borrow lasts as long as the result")
            },
            Event::ReslicedInPlace { .. } => {
                write!(f, "Points to part of what it pointed to; its borrow goes on")
            },
            Event::DeadElseNote => {
                write!(f, "This else branch never runs")
            },
            Event::PassRestDrop { .. } => {
                write!(f, "What the pattern leaves of each element is dropped")
            },
            Event::LentWithParent { parent, .. } => {
                write!(f, "Borrowed along with {}", parent)
            },
            Event::DeadLoanNote { .. } => {
                write!(f, "Written where the borrow is already over on this path")
            },
            Event::RefDie { from, .. } => {
              write!(f, "{} reference dies", from.name())
            }
            Event::Branch { is, .. } => {
              write!(f, "{} branch occuring ", is.real_name())
            }
        }
    }
}

impl Event {
    /// The move of `*b` out of a Box drawn as its payload's part columns
    /// (`x => …` on `match *b`, `let x = *b;`, #898): the parts' own
    /// moves say what left; b keeps its heap allocation, so on b's
    /// column this isn't a move of b.
    pub fn is_box_payload_move(&self) -> bool {
        matches!(self, Event::Move { is: ResourceTy::Deref(ResourceAccessPoint::Struct(Struct { is_box: true, .. })), .. })
    }

    /// For an or-pattern's possible borrow (#565), the plain borrow
    /// event that changes the column's state the same way.
    pub fn or_alt_plain_equivalent(&self) -> Option<Event> {
        let id = 1000000000;
        match self {
            Event::OrAltLend { into, is, mutable, end, .. } => {
                let other = ResourceTy::Value(into.clone());
                let is = is.clone();
                Some(match (*mutable, *end) {
                    (false, false) => Event::StaticLend { to: other, is, id, of_ref_var: false },
                    (true, false) => Event::MutableLend { to: other, is, id, of_ref_var: false },
                    (false, true) => Event::StaticReacquire { from: other, is, id, still_borrowed_by: Vec::new(), of_ref_var: false },
                    (true, true) => Event::MutableReacquire { from: other, is, id, still_borrowed_by: Vec::new(), of_ref_var: false },
                })
            }
            Event::OrAltBorrow { is, mutable, end, .. } => {
                let other = ResourceTy::Anonymous;
                let is = is.clone();
                Some(match (*mutable, *end) {
                    (false, false) => Event::StaticBorrow { from: other, is, id, of_ref_var: false },
                    (true, false) => Event::MutableBorrow { from: other, is, id, of_ref_var: false },
                    (false, true) => Event::StaticDie { to: other, is, id },
                    (true, true) => Event::MutableDie { to: other, is, id },
                })
            }
            _ => None,
        }
    }

    pub fn get_id(&self) -> usize {
        match self {
            Event::Duplicate {id: x , ..} | Event::Move {id: x, ..} | Event::StaticLend {id : x, ..} |
            Event::MutableLend {id: x, ..} | Event::MutableDie {id: x, ..} | Event::StaticDie {id: x, ..} |
            Event::Acquire { id: x, .. } | Event::Copy { id: x, .. } | Event::MutableBorrow { id: x, .. } |
            Event::StaticBorrow { id: x, .. } | Event::StaticReacquire { id: x, .. } | Event::MutableReacquire {id: x, ..}
            | Event::Branch { id: x, .. } | Event::InitRefParam { id: x, .. } | Event::ClosureCall { id: x, .. } => {
                *x
            }
            _ => 1000000000
        }
    }
    pub fn deref_name <'a>(&self, name: &'a mut String) -> & 'a String {
        match self {
            Event::Duplicate {is: x , ..} | Event::Move {is: x, ..} | Event::StaticLend {is : x, ..} | 
            Event::MutableLend {is: x, ..} | Event::MutableDie {is: x, ..} | Event::StaticDie {is: x, ..} | 
            Event::Acquire { is: x, .. } | Event::Copy { is: x, .. } | Event::MutableBorrow { is: x, .. } |
            Event::StaticBorrow { is: x, .. } | Event::StaticReacquire { is: x, .. } | Event::MutableReacquire {is: x, ..} => {
                match x {
                    ResourceTy::Deref(_) => {
                        name.insert(0, '*');
                        name
                    },
                    ResourceTy::DerefField(_, place, _, _) => {
                        *name = place.clone();
                        name
                    },
                    _ => name
                }
            },
            _ => name
        }
    }

    /// The text of a borrow's end that names the lender ("c's immutable
    /// borrow of x ends"), for a dot that ends more than one of the
    /// reference's loans (#803). None for any other event, or when the
    /// lender has no name of its own.
    pub fn borrow_end_naming_lender(&self, my_name: &mut String) -> Option<String> {
        let (to, is_mut) = match self {
            Event::StaticDie { to, .. } => (to, false),
            Event::MutableDie { to, .. } => (to, true),
            _ => return None,
        };
        let lender = match to {
            ResourceTy::Value(r) => r.name().clone(),
            ResourceTy::Deref(r) => format!("*{}", r.name()),
            _ => return None,
        };
        let shown = self.deref_name(my_name).clone();
        Some(hover_messages::event_dot_borrow_of_ends(&shown, &lender, is_mut))
    }

    /// The name of the lender a borrow end gives its value back to
    /// (see `borrow_end_naming_lender`).
    pub fn borrow_end_lender_name(&self) -> Option<String> {
        match self {
            Event::StaticDie { to: ResourceTy::Value(r), .. } | Event::MutableDie { to: ResourceTy::Value(r), .. } => Some(r.name().clone()),
            Event::StaticDie { to: ResourceTy::Deref(r), .. } | Event::MutableDie { to: ResourceTy::Deref(r), .. } => Some(format!("*{}", r.name())),
            _ => None,
        }
    }

    // This function tells us what the text associated with a Dot event should be
    pub fn print_message_with_name(&self, my_name: &mut String) -> String {
        match self {
            // no arrow involved
            Event::ClosureCall { fates, .. } => {
                hover_messages::event_dot_closure_call_once(my_name, fates)
            }
            Event::OwnerGoOutOfScope { .. } => {
                hover_messages::event_dot_owner_go_out_of_scope(my_name)
            }
            Event::RefGoOutOfScope { .. } => {
                hover_messages::event_dot_ref_go_out_of_scope(my_name)
            }
            Event::OwnerDropAtReassign { parts: Some(p), .. } => {
                hover_messages::event_dot_struct_drop_at_reassign_partial(my_name, p)
            }
            Event::OwnerDropAtReassign { maybe_moved: false, .. } => {
                hover_messages::event_dot_owner_drop_at_reassign(my_name, "")
            }
            Event::OwnerDropAtReassign { maybe_moved: true, loop_head: true, .. } => {
                hover_messages::event_dot_owner_drop_at_reassign_loop_head(my_name, "")
            }
            Event::OwnerDropAtReassign { maybe_moved: true, by_cond, .. } => {
                hover_messages::event_dot_owner_drop_at_reassign_maybe_moved(my_name, "", *by_cond)
            }
            Event::ManuallyDropByHand => {
                hover_messages::event_dot_manually_drop_by_hand(my_name)
            }
            Event::LoopExitMove { definite, at: MaybeMovedAt::Loop { carried, .. } } => {
                hover_messages::event_dot_loop_exit_move(my_name, *definite, false, *carried)
            }
            Event::LoopExitMove { at: MaybeMovedAt::LabeledBlock { skipped: true, .. }, .. } => {
                hover_messages::event_dot_block_break_skips_move(my_name)
            }
            Event::LoopExitMove { definite, at: MaybeMovedAt::LabeledBlock { skipped: false, carried } } => {
                hover_messages::event_dot_loop_exit_move(my_name, *definite, true, *carried)
            }
            Event::LoopExitMove { definite, at: MaybeMovedAt::ChainFail { to, dropped, path, how } } => {
                hover_messages::event_dot_chain_fail_move(my_name, *definite, to, dropped, *path, how)
            }
            Event::LoopExitMove { definite, at: MaybeMovedAt::WhileCondFail { to, dropped, how } } => {
                hover_messages::event_dot_while_cond_move(my_name, *definite, to, dropped, how)
            }
            Event::LoopExitMove { at: MaybeMovedAt::LoopHead, .. } => {
                hover_messages::event_dot_loop_head_maybe_moved(my_name)
            }
            Event::LoopExitMove { at: MaybeMovedAt::LoopRound { .. }, .. } => {
                hover_messages::event_dot_loop_round_maybe_moved(my_name)
            }
            Event::ChainFailAssign { place, drops_old, rc, path } => {
                hover_messages::event_dot_chain_fail_assign(place.as_ref().unwrap_or(my_name), *drops_old, *rc, *path)
            }
            Event::OrAltMove { into, sources, tuple, part_of, value, .. } => {
                hover_messages::event_dot_or_alt_move(my_name, into, sources, tuple.as_deref(), part_of.as_ref(), *value)
            }
            Event::OrAltLend { into, is, sources, mutable, end, chain: Some(chain) } => {
                let holds_value = lender_holds_value(is);
                if *end {
                    hover_messages::event_dot_chain_ref_reacquire(my_name, into.name(), *mutable, holds_value, chain)
                } else {
                    hover_messages::event_dot_chain_ref_lend(my_name, into.name(), sources, *mutable, holds_value, chain)
                }
            }
            Event::OrAltLend { into, is, sources, mutable, end, chain: None } => {
                let holds_value = lender_holds_value(is);
                if *end {
                    hover_messages::event_dot_or_alt_reacquire(my_name, into.name(), *mutable, holds_value)
                } else {
                    hover_messages::event_dot_or_alt_lend(my_name, into.name(), sources, *mutable, holds_value)
                }
            }
            Event::OrAltBorrow { sources, end: false, chain: Some(chain), .. } => {
                hover_messages::event_dot_chain_ref_borrow(my_name, sources, chain)
            }
            Event::OrAltBorrow { sources, mutable, end, .. } => {
                match (*end, *mutable) {
                    (false, _) => hover_messages::event_dot_or_alt_borrow(my_name, sources, *mutable),
                    (true, false) => hover_messages::event_dot_static_return(my_name),
                    (true, true) => hover_messages::event_dot_mut_return(my_name),
                }
            }
            // What the parameter's pattern leaves unbound is dropped when
            // the function returns (#1181).
            Event::InitRefParam{ rest: Some(rest), .. } => {
                let mut whole = self.clone();
                if let Event::InitRefParam { rest, .. } = &mut whole {
                    *rest = None;
                }
                if rest.parts.is_empty() {
                    hover_messages::event_dot_unbound_param_init(my_name, &rest.fn_name)
                } else {
                    format!("{}. {}", whole.print_message_with_name(my_name), hover_messages::param_rest_dropped(&rest.fn_name, &rest.parts))
                }
            }
            // The column of the value a `ref` binding borrows (#691).
            Event::InitRefParam{ param: ResourceAccessPoint::Owner(_), own_param: Some(own), .. } => {
                hover_messages::event_dot_param_value_init(my_name, &own.fn_name, own.whole)
            }
            Event::ChainFailUse { uses, definite, path, is_ref, pointee_value } => {
                hover_messages::event_dot_chain_fail_use(my_name, uses, *definite, *path, *is_ref, *pointee_value)
            }
            Event::DeadElseNote => {
                hover_messages::event_dot_dead_else_note(my_name)
            }
            Event::PassRestDrop { parts } => {
                hover_messages::event_dot_pass_rest_drop(parts)
            }
            Event::LentWithParent { parent, to, mutable } => {
                hover_messages::event_dot_lent_with_parent(my_name, parent, to, *mutable)
            }
            Event::TiedParamNote { lifetime, .. } => {
                hover_messages::event_dot_tied_param_note(my_name, lifetime)
            }
            Event::ReslicedInPlace { lender, mutable, part } => {
                hover_messages::event_dot_resliced_in_place(my_name, lender, *mutable, *part)
            }
            Event::DeadLoanNote { lender, borrower, repoint, .. } => {
                hover_messages::event_dot_dead_loan_note(lender, borrower, *repoint)
            }
            Event::InitRefParam{ param, own_param: Some(own), .. } => {
                hover_messages::event_dot_ref_init_own_param(my_name, &own.fn_name, own.whole, matches!(param, ResourceAccessPoint::MutRef(_)), own.drops)
            }
            Event::InitRefParam{ param, in_pattern: Some(bound), .. } => {
                hover_messages::event_dot_ref_init_in_pattern(my_name, bound, matches!(param, ResourceAccessPoint::MutRef(_)))
            }
            Event::TemporaryDrop { ty: Some(ty), when, literal, part: None, assign, .. } => {
                hover_messages::event_dot_temporary_drop(ty, when, *literal, *assign)
            }
            Event::OwnerDropIfTryReturns { .. } => {
                hover_messages::event_dot_owner_drop_if_try_returns(my_name, "")
            }
            Event::OwnerDropUnlessReturned => {
                hover_messages::event_dot_owner_drop_unless_returned(my_name, false, "")
            }
            Event::TemporaryDrop { ty: None, when, .. } => {
                hover_messages::event_dot_extended_temporary_drop(my_name, when)
            }
            Event::TupleTempDrop { tuple, at, with } => {
                hover_messages::event_dot_tuple_temp_drop(tuple, *at, with)
            }
            Event::LoopIteratorDrop { left, early } => {
                hover_messages::event_dot_loop_iterator_drop(my_name, *left, early)
            }
            // (The timeline panel says "value" for a value.)
            Event::LoopExitKept { reassigned } => {
                hover_messages::event_dot_loop_exit_kept(my_name, *reassigned, false)
            }
            // (The timeline panel words it with what follows.)
            Event::LoopMaybeInit { nested: true, .. } => {
                hover_messages::event_dot_loop_maybe_init_nested(my_name, true, true)
            }
            Event::LoopMaybeInit { before_loop, .. } => {
                hover_messages::event_dot_loop_maybe_init(my_name, true, true, None, *before_loop)
            }
            Event::TemporaryDrop { ty: Some(ty), when, part: Some(part), .. } => {
                hover_messages::event_dot_temporary_part_drop(ty, when, part)
            }
            // A `static` the function uses (#690): it was there before
            // the function ran.
            Event::InitRefParam{ param, .. } if param.is_static() => {
                hover_messages::event_dot_static(my_name)
            }
            Event::InitRefParam{ param, .. } => {
                // Owner/Struct params receive ownership from the
                // caller — call that out so the dot hover matches
                // the L-arrow's. Ref params receive a borrow from
                // the caller and get the parallel ref message.
                match param {
                    ResourceAccessPoint::Owner(_) | ResourceAccessPoint::Struct(_) => {
                        if param.is_copy_owner() {
                            hover_messages::event_dot_copy_init_from_caller(my_name)
                        } else if param.is_plain_value() {
                            hover_messages::event_dot_owner_init_from_caller_value(my_name)
                        } else {
                            hover_messages::event_dot_owner_init_from_caller(my_name)
                        }
                    }
                    ResourceAccessPoint::MutRef(_) => {
                        hover_messages::event_dot_ref_init_from_caller(my_name, true)
                    }
                    ResourceAccessPoint::StaticRef(_) => {
                        hover_messages::event_dot_ref_init_from_caller(my_name, false)
                    }
                    _ => hover_messages::event_dot_init_param(my_name),
                }
            }
            // A borrow whose borrower isn't drawn and can't be named
            // (stored in a field of a call's result,
            // `v.last_mut().unwrap().r = &x`): don't call it "Anonymous
            // resource" (#282).
            Event::StaticLend{ to: ResourceTy::Anonymous, is, ..} => {
                let omit_read_only = is.extract_rap()
                    .map_or(true, |r| (r.is_ref() && !r.is_mutref()) || r.may_have_interior_mut());
                let through_ref = ref_lender(is).map(|r| r.points_to_copy());
                hover_messages::event_dot_lend_to_undrawn_place(&self.deref_name(my_name), false, omit_read_only, through_ref, is.extract_rap().map_or(false, |r| r.holds_value()))
            }
            Event::MutableLend{ to: ResourceTy::Anonymous, is, ..} => {
                let through_ref = ref_lender(is).map(|r| r.points_to_copy());
                hover_messages::event_dot_lend_to_undrawn_place(&self.deref_name(my_name), true, false, through_ref, is.extract_rap().map_or(false, |r| r.holds_value()))
            }
            // arrow going out
            // A closure literal returned from the fn (#760): its by-value
            // captures go to the caller inside it.
            Event::Duplicate{ to: ResourceTy::Caller, into_closure: true, ..} => {
                hover_messages::event_dot_copy_into_returned_closure(&self.deref_name(my_name))
            }
            Event::Move{ to: ResourceTy::Caller, is, into_closure: true, ..} => {
                hover_messages::event_dot_move_into_returned_closure(&self.deref_name(my_name), is_plain(is))
            }
            Event::Duplicate{ to, into_closure: true, ..} => {
                hover_messages::event_dot_copy_into_closure_arg(&self.deref_name(my_name), &to.name())
            }
            Event::Move{ to, is, into_closure: true, ..} => {
                hover_messages::event_dot_move_into_closure_arg(&self.deref_name(my_name), &to.name(), is_plain(is))
            }
            Event::Duplicate{ is: ResourceTy::Index(_, place, _), ..} if place.starts_with('*') => {
                // `let n = *slots[0];`: what is read is the resource
                // the reference in slots[0] points to (#438).
                hover_messages::event_dot_copy_through_element(place)
            }
            // `&PRIMES` returned: a reference to the static (#1053).
            Event::Duplicate{ to: ResourceTy::Caller, is: ResourceTy::Index(r, place, _), .. } if place.starts_with('&') => {
                hover_messages::event_dot_static_ref_to_caller(r.name(), place)
            }
            // `v[0]` returned: a copy of the element (#1052).
            Event::Duplicate{ to: ResourceTy::Caller, is: ResourceTy::Index(r, place, _), .. } => {
                hover_messages::event_dot_copy_element_to_caller(r.name(), place, r.is_ref(), if r.is_ref() { r.points_to_copy() } else { r.holds_value() })
            }
            // `let first = v[0];`: one element is copied out of the
            // resource v owns (or points to); v keeps all of it.
            Event::Duplicate{ is: ResourceTy::Index(r, place, _), .. } => {
                hover_messages::event_dot_copy_out_of_element(r.name(), place, r.is_ref(), if r.is_ref() { r.points_to_copy() } else { r.holds_value() })
            }
            // `&self.s` returned: a reborrow through `self` (#784).
            Event::Duplicate{ to: ResourceTy::Caller, is: ResourceTy::DerefField(r, place, _, _), ..} if place.starts_with('&') => {
                hover_messages::event_dot_reborrow_to_caller(r.name(), place)
            }
            // `*r` returned with r a reference (#670).
            Event::Duplicate{ to: ResourceTy::Caller, is: ResourceTy::DerefField(r, place, _, _), ..} if place.starts_with('*') => {
                hover_messages::event_dot_copy_deref_to_caller(r.name(), place)
            }
            Event::Duplicate{ to, is: ResourceTy::DerefField(r, place, _, _), ..} => {
                // `let a = p.x;` with p a reference: the field is
                // copied out of the resource p points to (#266).
                let target = match to {
                    ResourceTy::Caller => "the caller".to_owned(),
                    _ => to.name(),
                };
                hover_messages::event_dot_copy_through_deref_field(r.name(), place, &target, r.points_to_copy())
            }
            Event::Move{ loop_head: LoopHead::Collection | LoopHead::CollectionCopy | LoopHead::Part { .. }, ..} => {
                hover_messages::event_dot_move_into_loop_iterator(&self.deref_name(my_name))
            }
            Event::Move{ loop_head: LoopHead::Iterator, ..} => {
                hover_messages::event_dot_move_iterator_into_loop(&self.deref_name(my_name))
            }
            Event::Duplicate{ to, is, of, ..} => {
                match to {
                    // A `move` closure capturing a Copy value copies it (#311).
                    _ if to.is_closure() => {
                        let is_ref = matches!(is, ResourceTy::Value(ResourceAccessPoint::StaticRef(_)));
                        hover_messages::event_dot_capture_copy_to_closure(&self.deref_name(my_name), &to.name(), is_ref)
                    }
                    // Returning a `&T` copies the reference, not the referent.
                    ResourceTy::Caller if matches!(is, ResourceTy::Value(ResourceAccessPoint::StaticRef(_))) =>
                        hover_messages::event_dot_copy_ref_to_caller(&self.deref_name(my_name)),
                    ResourceTy::Caller => hover_messages::event_dot_copy_to_caller(&self.deref_name(my_name)),
                    _ if !to.is_closure() && matches!(is, ResourceTy::Value(ResourceAccessPoint::StaticRef(_))) =>
                        hover_messages::event_dot_ref_copied(my_name, is.extract_rap().map_or(false, |r| r.points_to_copy())),
                    // `if let Some(x) = opt`: only the value inside opt is
                    // copied (#866).
                    _ if *of == CopyOf::Inside => hover_messages::event_dot_copy_inside_to(&self.deref_name(my_name)),
                    // `if let Some(&x) = orr`: the value a reference
                    // inside orr points to is copied (#874).
                    _ if matches!(of, CopyOf::Pointee { .. }) =>
                        hover_messages::event_dot_copy_pointee_to(&self.deref_name(my_name), *of == CopyOf::Pointee { part: true }),
                    _ => hover_messages::event_dot_copy_to(&self.deref_name(my_name))
                }
            }
            // `let n = r?;` / `r?`: the `?` moves r (#652).
            Event::Move{ to, is, via_try: Some(kind), ..} => {
                let target = match to {
                    ResourceTy::Value(r) => Some(r.name().to_owned()),
                    ResourceTy::Caller => Some("the caller".to_owned()),
                    _ => None,
                };
                hover_messages::event_dot_move_into_try(&self.deref_name(my_name), target.as_ref(), *kind, is_plain(is))
            }
            // Moving a `&mut` moves the reference, not its referent; so
            // does moving a struct with a `&T` field (#637).
            Event::Move{ to, is: ResourceTy::Value(ResourceAccessPoint::MutRef(r)), ..} if !to.is_closure() => {
                hover_messages::event_dot_ref_moved(my_name, matches!(to, ResourceTy::Caller), r.pointee_copy, true)
            }
            Event::Move{ to, is: ResourceTy::Value(ResourceAccessPoint::StaticRef(r)), ..} if !to.is_closure() => {
                hover_messages::event_dot_ref_moved(my_name, matches!(to, ResourceTy::Caller), r.pointee_copy, false)
            }
            Event::Move{ is: ResourceTy::Index(_, place, write), ..} if place.starts_with('*') => {
                hover_messages::event_dot_move_through_element(place, !matches!(write, PlaceWrite::AssignDropsOld))
            }
            // `return loop { … break k; }` (#1171): to the caller.
            Event::Move{ to: ResourceTy::Caller, is, break_out: true, ..} => {
                hover_messages::event_dot_break_out_move_to_caller(&self.deref_name(my_name), is_plain(is))
            }
            Event::Move{ to, is, break_out: true, ..} => {
                hover_messages::event_dot_break_out_move(&self.deref_name(my_name), &to.name(), is_plain(is))
            }
            // A field or a pattern's binding moved out of an unbound
            // temporary, which keeps the rest (#469).
            Event::Move{ to, is: ResourceTy::Value(ResourceAccessPoint::Owner(Owner { temp_part: Some(part), .. })), empties_temp, ..} => {
                hover_messages::event_dot_move_out_of_temporary(&self.deref_name(my_name), &to.name(), part, *empties_temp)
            }
            // `[c, _] => …`: only the element `pair[0]` moves out (#1285).
            Event::Move{ is: is @ ResourceTy::Index(r, place, _), ..} if r.elems_left().is_some() => {
                if r.elems_left().is_some_and(|e| e.empties_last(place)) {
                    hover_messages::event_dot_move_last_elems(place, &self.deref_name(my_name), is_plain(is))
                } else {
                    hover_messages::event_dot_move_to(place, is_plain(is))
                }
            }
            // `p.b` in `p = Pair { a: .., b: p.b };` (#1411): moved out
            // by the right-hand side, back with the new value (the
            // acquire on the same dot).
            Event::Move{ to: ResourceTy::Value(t), is, ..} if is.extract_rap().is_some_and(|r| r.hash() == t.hash()) => {
                hover_messages::event_dot_move_out_for_new_value(&self.deref_name(my_name), is_plain(is))
            }
            Event::Move{ to, is, ..} => {
                let v = is_plain(is);
                match to {
                    ResourceTy::Caller => hover_messages::event_dot_move_to_caller(&self.deref_name(my_name), v),
                    ResourceTy::Temporary { tuple: Some(t), .. } => hover_messages::event_dot_move_to_tuple_temporary(&self.deref_name(my_name), t, v),
                    // With nothing to drop (`let _ = (p, 5);`, `p: P { a: i32 }`, #1301).
                    ResourceTy::Temporary { extended: false, tuple: None } if v => hover_messages::event_dot_move_value_to_temporary(&self.deref_name(my_name)),
                    ResourceTy::Temporary { extended, tuple: None } => hover_messages::event_dot_move_to_temporary(&self.deref_name(my_name), *extended),
                    // `vec![s];`: s is moved into the temporary the macro or
                    // literal builds, which nothing takes (#570). (A `let`'s
                    // literal, #439, says where it's dropped on its column.)
                    ResourceTy::Value(ResourceAccessPoint::Owner(Owner { scrutinee: Some((what, when)), unbound_temp: true, let_literal_temp: false, .. })) =>
                        hover_messages::event_dot_move_to_scrutinee_temporary(&self.deref_name(my_name), what, when),
                    ResourceTy::Value(rap) if rap.is_unbound_temporary() && !rap.is_let_literal_temporary() =>
                        hover_messages::event_dot_move_to_temporary(&self.deref_name(my_name), false),
                    ResourceTy::Value(ResourceAccessPoint::Function(f)) if f.drops_arg.0 =>
                        hover_messages::event_dot_move_to_drop(&self.deref_name(my_name), &f.name, v,
                            &is.extract_rap().map_or(String::new(), |r| r.drop_note())),
                    // `move || r.push('!')` with `r: &mut String` moves the
                    // reference, not what it points to (#1148).
                    _ if to.is_closure() && is.extract_rap().is_some_and(|r| r.is_ref()) =>
                        hover_messages::event_dot_capture_move_ref_to_closure(&self.deref_name(my_name), &target_name(to)),
                    _ if to.is_closure() => hover_messages::event_dot_capture_move_to_closure(&self.deref_name(my_name), &target_name(to), v),
                    _ => hover_messages::event_dot_move_to(&self.deref_name(my_name), v),
                }

            }
            Event::StaticLend{ to, is, of_ref_var, ..} => {
                // Only an owner's own Copy value here: a reference
                // lender is handled by event_dot_static_lend_ref_var
                // (`&r`, #925) or event_dot_lend_through_ref.
                let holds_value = lender_holds_value(is);
                if let (true, true, Some(r)) = (to.is_closure(), *of_ref_var, ref_lender(is)) {
                    // `|| println!("{}", r)` captures r itself (#1180).
                    hover_messages::event_dot_capture_ref_var_to_closure(r.name(), &target_name(to), false)
                } else if to.is_closure() {
                    let target_name = target_name(to);
                    hover_messages::event_dot_capture_static_lend_to_closure(&self.deref_name(my_name), &target_name, holds_value)
                } else {
                    // `is` is the lender. A `&T` lender was read-only
                    // already, and a `RefCell` / `Cell` / `Mutex` lender
                    // isn't read-only at all (it can be mutated through
                    // the shared borrow); see event_dot_static_lend.
                    let omit_read_only = is.extract_rap()
                        .map_or(true, |r| (r.is_ref() && !r.is_mutref()) || r.may_have_interior_mut());
                    let target_name = target_name(to);
                    if let (true, Some(r)) = (*of_ref_var, ref_lender(is)) {
                        return hover_messages::event_dot_static_lend_ref_var(r.name(), &target_name);
                    }
                    if let Some(r) = ref_lender(is) {
                        return hover_messages::event_dot_lend_through_ref(r.name(), &target_name, false, r.is_mutref(), r.points_to_copy());
                    }
                    hover_messages::event_dot_static_lend(&self.deref_name(my_name), &target_name, omit_read_only, holds_value)
                }
            }
            Event::MutableLend{ to, is, of_ref_var, ..} => {
                let holds_value = lender_holds_value(is);
                let target_name = target_name(to);
                if let (true, true, Some(r)) = (to.is_closure(), *of_ref_var, ref_lender(is)) {
                    hover_messages::event_dot_capture_ref_var_to_closure(r.name(), &target_name, true)
                } else if to.is_closure() {
                    hover_messages::event_dot_capture_mut_lend_to_closure(&self.deref_name(my_name), &target_name, holds_value)
                } else if let (true, Some(r)) = (*of_ref_var, ref_lender(is)) {
                    hover_messages::event_dot_mut_lend_ref_var(r.name(), &target_name)
                } else if let Some(r) = ref_lender(is) {
                    hover_messages::event_dot_lend_through_ref(r.name(), &target_name, true, r.is_mutref(), r.points_to_copy())
                } else {
                    hover_messages::event_dot_mut_lend(&self.deref_name(my_name), &target_name, holds_value)
                }
            }
            Event::StaticDie{ is: ResourceTy::Value(r), .. } if r.is_mutref() => {
                hover_messages::event_dot_mut_ref_return_of_shared(&self.deref_name(my_name))
            }
            Event::StaticDie{ .. } => {
                hover_messages::event_dot_static_return(&self.deref_name(my_name))
            }
            Event::MutableDie{ .. } => {
                hover_messages::event_dot_mut_return(&self.deref_name(my_name))
            }
            // arrow going in
            // `n` in `let n = r?;` (#652).
            Event::Acquire{ from: ResourceTy::Value(from), is, via_try: Some(kind), ..} => {
                let holds = match is.extract_rap() {
                    Some(r) if r.is_mutref() => hover_messages::acquire_from_try_holds(hover_messages::TryHolds::MutRef),
                    Some(r) if r.is_ref() => hover_messages::acquire_from_try_holds(hover_messages::TryHolds::Ref),
                    Some(r) if r.is_copy_owner() => hover_messages::acquire_from_try_holds(hover_messages::TryHolds::CopyOwner),
                    _ if is_plain(is) => hover_messages::acquire_from_try_holds(hover_messages::TryHolds::Plain),
                    _ => hover_messages::acquire_from_try_holds(hover_messages::TryHolds::Resource),
                };
                hover_messages::event_dot_acquire_from_try(&self.deref_name(my_name), from.name(), *kind, holds)
            }
            // `let o = Some(5);`: the value a constructor of literals
            // builds; nothing is copied from the constructor (#768).
            // `let y = Fruit::Banana;` with nothing to drop reads the
            // same (#873); one that needs drop acquires a resource.
            Event::Acquire{ from: ResourceTy::Value(ResourceAccessPoint::Function(f)), is, via_try: None, ..}
                if is.extract_rap().is_some_and(|r| r.holds_value()) && f.name.rsplit("::").next().is_some_and(|l| l.starts_with(|c: char| c.is_uppercase())) => {
                hover_messages::event_dot_init_from_call(&self.deref_name(my_name), &f.name, None, f.inside.as_deref())
            }
            // `for w in words`: each iteration moves one element of the
            // collection into `w` (#850).
            // A reference never owns: `for r in refs` with `refs:
            // Vec<&mut String>` binds r to a reference moved out.
            Event::Acquire{ from: ResourceTy::Value(from), is, loop_head: LoopHead::Collection, ..}
                if is.extract_rap().is_some_and(|r| r.is_ref()) => {
                hover_messages::event_dot_acquire_loop_element_ref(&self.deref_name(my_name), &from.name())
            }
            Event::Acquire{ from: ResourceTy::Value(from), loop_head: LoopHead::Collection, ..} => {
                hover_messages::event_dot_acquire_loop_element(&self.deref_name(my_name), &from.name())
            }
            // `for y in ys` with `ys: Vec<i32>` (or `Vec<&String>`):
            // each y is a copy of an element (#1372).
            Event::Acquire{ from: ResourceTy::Value(from), loop_head: LoopHead::CollectionCopy, ..} => {
                hover_messages::event_dot_acquire_loop_element_copy(&self.deref_name(my_name), &from.name())
            }
            // `for (s, k) in names`: s gets the `.0` of each element, k a
            // copy of the `.1` (#1390).
            Event::Acquire{ from: ResourceTy::Value(from), is, loop_head: LoopHead::Part { path, copy, .. }, ..} => {
                let is_ref = is.extract_rap().is_some_and(|r| r.is_ref());
                hover_messages::event_dot_acquire_loop_element_part(&self.deref_name(my_name), &from.name(), path, *copy, is_ref)
            }
            Event::Acquire{ from: ResourceTy::Value(f), is, ..} if is.extract_rap().is_some_and(|r| r.hash() == f.hash()) => {
                hover_messages::event_dot_back_with_new_value(&self.deref_name(my_name), is_plain(is))
            }
            Event::Acquire{ from, is, ..} => {
                if is.is_closure() && from.extract_rap().is_some() {
                    // `f` (closure) acquiring from a real source —
                    // this is a `move`-capture, not a generic
                    // ownership transfer. The Anonymous-from case
                    // (the closure literal's own Bind) keeps the
                    // generic message because there's no captured
                    // upvar to name on the closure side yet.
                    hover_messages::event_dot_closure_capture_acquire(&self.deref_name(my_name), &target_name(from), from.extract_rap().map_or(false, |r| r.holds_value()))
                } else if let ResourceTy::Index(r, place, write) = is {
                    // `v[i] = s;` / `v[i] = 5;` assigns to the place
                    // `v[i]`: it reads like a plain assignment, and v
                    // doesn't acquire anything. `v[i] += k` modifies
                    // the element in place.
                    let (place, alts) = split_alternatives(place);
                    let place = &place.to_owned();
                    let msg = match write {
                        // `r.n = 2` through a reference to a local struct (#854):
                        // drawn on the field's own column `s.n`, the place
                        // written through is `r.n`.
                        PlaceWrite::AssignCopy | PlaceWrite::AssignOwned | PlaceWrite::AssignDropsOld
                        | PlaceWrite::AssignOverMoved | PlaceWrite::AssignMaybeMoved { .. }
                            if r.name().contains('.') && !place.starts_with('*') && !place.starts_with(r.name().as_str()) =>
                            hover_messages::event_dot_assign_through(r.name(), place),
                        PlaceWrite::AssignCopy => place_assign_copy_message(place, from),
                        PlaceWrite::AssignDropsOld | PlaceWrite::AssignOverMoved | PlaceWrite::AssignMaybeMoved { .. } =>
                            hover_messages::event_dot_owner_drop_at_reassign(place, ""),
                        // The old element has nothing to drop (that's what makes
                        // it AssignOwned): a plain assignment (#484).
                        PlaceWrite::AssignOwned => hover_messages::event_dot_copyable_assign(place),
                        PlaceWrite::Update | PlaceWrite::UpdateValue => hover_messages::event_dot_write_element(r.name(), place, r.is_ref(), element_update_holds_value(r, place, write)),
                    };
                    with_alternatives(msg, r, place, write, alts)
                } else if is.holds_borrow() && matches!(from, ResourceTy::Value(r) if r.is_mutref()) {
                    // `let o = Some(r);` / `vec![r]` with r a `&mut`:
                    // the reference moves into o's value (#1128).
                    hover_messages::event_dot_holder_takes_ref(&self.deref_name(my_name), &from.name())
                } else if is.holds_borrow() && matches!(from, ResourceTy::Value(r) if !r.is_fn()) {
                    // `let it2 = it;`: the value moves, and the borrow
                    // it holds moves with it (#336).
                    hover_messages::event_dot_holder_acquire(&self.deref_name(my_name), &from.name())
                } else if matches!(is, ResourceTy::Deref(ResourceAccessPoint::Struct(Struct { is_box: true, .. })))
                    && matches!(from, ResourceTy::Value(_)) {
                    // `*b = o` / `*b = make()` on a Box drawn as its
                    // payload's part columns (#893): the payload takes
                    // the value; b keeps its heap allocation.
                    hover_messages::event_dot_acquire(&self.deref_name(my_name))
                } else if let Some(r) = is.deref_rap() {
                    // `*r += 1;` / `r.f += 1;` modify the resource r
                    // points to in place. `*r = v;` / `r.f = v;` are
                    // assignments to the place `*r` / `r.f`: they read
                    // like a plain assignment (2026-11-05 decision),
                    // and the drop sentence of the old value is the
                    // one plain assignment uses.
                    let write = match is {
                        ResourceTy::DerefField(_, _, w, _) => *w,
                        _ => r.place_write(),
                    };
                    match write {
                        PlaceWrite::AssignCopy => place_assign_copy_message(&self.deref_name(my_name), from),
                        PlaceWrite::AssignDropsOld => hover_messages::event_dot_owner_drop_at_reassign(&self.deref_name(my_name), ""),
                        // `*b = v` after b's contents were moved out
                        // (#928), on every path or some only.
                        PlaceWrite::AssignOverMoved =>
                            hover_messages::event_dot_box_contents_write_after_move(&self.deref_name(my_name), r.holds_phrase(), None),
                        PlaceWrite::AssignMaybeMoved { by_cond } =>
                            hover_messages::event_dot_box_contents_write_after_move(&self.deref_name(my_name), r.holds_phrase(), Some(by_cond)),
                        // The old value has nothing to drop (that's what
                        // makes it AssignOwned): a plain assignment (#484).
                        PlaceWrite::AssignOwned => hover_messages::event_dot_copyable_assign(&self.deref_name(my_name)),
                        PlaceWrite::Update | PlaceWrite::UpdateValue => hover_messages::event_dot_write_through_deref(r.name(), &self.deref_name(my_name), r.points_to_copy()),
                    }
                } else if let Some(call) = rc_call_of(from) {
                    // `let b = Rc::clone(&a);`, `a.clone()`,
                    // `Rc::downgrade(&a)`: another owner of the same
                    // value; nothing is copied (#373).
                    hover_messages::event_dot_rc_call_acquire(&self.deref_name(my_name), call)
                } else if let Some(rap) = is.extract_rap().filter(|rap| rap.is_ref()) {
                    // `let s: &str = "hi";`, `let r2 = r;` — the
                    // variable is bound to a reference, which never
                    // owns its referent.
                    hover_messages::event_dot_ref_acquire(&self.deref_name(my_name), rap.is_mutref())
                } else if matches!(is, ResourceTy::Value(rap) if rap.is_let_literal_temporary())
                    && matches!(from, ResourceTy::Value(f) if !f.is_fn()) {
                    // `let (ref a, ..) = (s, t);`: s is moved into the
                    // temporary the literal builds (#439).
                    let ends_with_statement = matches!(is, ResourceTy::Value(rap) if rap.is_unbound_temporary());
                    let from_name = match from { ResourceTy::Deref(r) => format!("*{}", r.name()), _ => from.name() };
                    let assign = matches!(is, ResourceTy::Value(rap) if rap.is_assign_literal_temporary());
                    hover_messages::event_dot_literal_temporary_acquire(&from_name, ends_with_statement, assign)
                } else if matches!(is, ResourceTy::Value(rap) if rap.is_let_literal_temporary())
                    && matches!(from, ResourceTy::Value(f) if f.is_fn()) {
                    // `let (a, _) = (1, mk());`: what mk returns is moved
                    // into the temporary (#1376); `(a, _) = (1, mk());`
                    // (#1387).
                    let ends_with_statement = matches!(is, ResourceTy::Value(rap) if rap.is_unbound_temporary());
                    let assign = matches!(is, ResourceTy::Value(rap) if rap.is_assign_literal_temporary());
                    hover_messages::event_dot_literal_temporary_takes_result(&from.name(), ends_with_statement, assign)
                } else if let ResourceTy::Value(ResourceAccessPoint::Owner(Owner { unbound_temp: true, ctor_temp: true, temp_part: Some(part), .. })) = is {
                    // `match Some(make()) { Some(s) if .. => .., _ => .. }`:
                    // a constructor a pattern takes apart (#807).
                    let from_name = from.extract_rap().map(|r| r.name().clone());
                    hover_messages::event_dot_ctor_temporary_part_acquire(from_name.as_ref(), part)
                } else if matches!(is, ResourceTy::Value(rap) if rap.is_unbound_temporary()) && matches!(from, ResourceTy::Anonymous) {
                    // `let (x, _) = (m(), n());`: the literal's own value
                    // (#483); `(x, _) = (m(), n());` (#1387).
                    hover_messages::event_dot_literal_temporary_start(matches!(is, ResourceTy::Value(rap) if rap.is_assign_literal_temporary()))
                } else if matches!(is, ResourceTy::Value(rap) if rap.is_unbound_temporary())
                    && matches!(from, ResourceTy::Value(f) if !f.is_fn()) {
                    // `vec![s];`: s is moved into the temporary (#570).
                    // `if let P { a: Some(s), .. } = (P { a: x, .. })`:
                    // the pattern can move it on (#955).
                    let from_name = match from { ResourceTy::Deref(r) => format!("*{}", r.name()), _ => from.name() };
                    let moved_on = matches!(is, ResourceTy::Value(ResourceAccessPoint::Owner(Owner { temp_part: Some(_), .. })));
                    hover_messages::event_dot_unbound_temporary_takes(&from_name, moved_on)
                } else if matches!(is, ResourceTy::Value(ResourceAccessPoint::Owner(Owner { unbound_temp: true, ctor_temp: true, temp_part: None, .. })))
                    && matches!(from, ResourceTy::Value(ResourceAccessPoint::Function(f))
                        if !f.name.rsplit("::").next().is_some_and(|l| l.starts_with(|c: char| c.is_uppercase()))) {
                    // `Some(make());`: what `make` returned is moved into
                    // the value the constructor builds (#783).
                    hover_messages::event_dot_unbound_temporary_takes_result(&from.name())
                } else if let ResourceTy::Value(ResourceAccessPoint::Owner(Owner { unbound_temp: true, temp_part: Some(part), .. })) = is {
                    // `make_p().name`, `match make() { Some(s) => .. }`:
                    // part of the value is moved out (#469).
                    hover_messages::event_dot_unbound_temporary_part_acquire(&from.name(), part)
                } else if matches!(is, ResourceTy::Value(rap) if rap.is_unbound_temporary()) {
                    // `make();`, `make().len()`: nothing takes the value.
                    hover_messages::event_dot_unbound_temporary_acquire(&target_name(from))
                } else if matches!(is, ResourceTy::Value(rap) if rap.is_temporary()) {
                    // `let r = &String::from("a");`: say why this
                    // temporary has a column until the end of the
                    // block (#400).
                    hover_messages::event_dot_temporary_acquire(&self.deref_name(my_name))
                } else if matches!(is.extract_rap(), Some(rap) if rap.is_copy_owner()) {
                    // i32 / bool / other primitive being assigned a
                    // literal or arithmetic value (`let n = 5;` or
                    // `n += 1;`), or an element of an owned collection
                    // (`for x in xs` with `xs: Vec<i32>`, a Move event
                    // for the iterand, #504). "Acquires ownership" is the wrong
                    // mental model for Copy types — there's no heap
                    // resource to own.
                    hover_messages::event_dot_acquire_copyable(&self.deref_name(my_name))
                } else if is_plain(is) {
                    // A non-Copy value with nothing to drop (an iterator
                    // over borrowed data, a struct of integers), or a
                    // Copy field that came along with its parent: a
                    // value, not a resource (#484).
                    hover_messages::event_dot_acquire_value(&self.deref_name(my_name))
                } else {
                    hover_messages::event_dot_acquire(&self.deref_name(my_name))
                }
            }
            Event::Copy{ from, is: ResourceTy::Index(r, place, write), of, ..} => {
                // `v[i] = y;` with y: Copy assigns a new value to the
                // place `v[i]`.
                let (place, alts) = split_alternatives(place);
                let place = &place.to_owned();
                let msg = match write {
                    PlaceWrite::Update | PlaceWrite::UpdateValue => hover_messages::event_dot_write_element(r.name(), place, r.is_ref(), element_update_holds_value(r, place, write)),
                    // `v[i] = Some(y);`: a new value, built from y (#866).
                    _ if matches!(of, CopyOf::IntoCtor(_)) => hover_messages::event_dot_copyable_assign(place),
                    _ => place_assign_copy_message(place, from),
                };
                with_alternatives(msg, r, place, write, alts)
            }
            Event::Copy{ from, is: is @ (ResourceTy::Deref(r) | ResourceTy::DerefField(r, _, _, _)), of, ..} => {
                // `*r = y;` / `r.f = y;` with y: Copy assigns a new value
                // to the place `*r` / `r.f`; it isn't an initialization.
                let deref = self.deref_name(my_name).clone();
                let write = match is {
                    ResourceTy::DerefField(_, _, w, _) => *w,
                    _ => r.place_write(),
                };
                match write {
                    PlaceWrite::AssignCopy if matches!(of, CopyOf::IntoCtor(_)) => hover_messages::event_dot_copyable_assign(&deref),
                    PlaceWrite::AssignCopy => place_assign_copy_message(&deref, from),
                    // `*r += y` / `r.f += y`: an in-place update; the arrow
                    // shows the copy. `*r` is a Copy value here; `r.f`
                    // is part of what r points to.
                    _ => hover_messages::event_dot_write_through_deref(r.name(), &deref, matches!(is, ResourceTy::Deref(_)) || r.points_to_copy()),
                }
            }
            // `let m = MAX;`: a copy of the constant (#690).
            Event::Copy{ from: ResourceTy::Anonymous, of: CopyOf::Const(c), ..} => {
                hover_messages::event_dot_copy_of_const(&self.deref_name(my_name), c)
            }
            // A copy out of a value with no name of its own (`let n =
            // match o { Some(x) => x.len(), None => 0 };`): there is no
            // source to name, and "Anonymous resource" is an internal
            // placeholder, not something in the student's code (#547).
            Event::Copy{ from: ResourceTy::Anonymous, is, ..} => {
                match is.extract_rap().filter(|rap| rap.is_ref()) {
                    Some(rap) => hover_messages::event_dot_ref_acquire(&self.deref_name(my_name), rap.is_mutref()),
                    None => hover_messages::event_dot_acquire_copyable(&self.deref_name(my_name)),
                }
            }
            // The result of a call (`let n = v.len();`, `v.len() as i32`)
            // isn't a copy of anything named in the code: the variable
            // gets the value the call returns (#733).
            // `if let Some(&x) = v.get(0)`: x gets a copy of the value
            // the reference the call returned points to (#874).
            Event::Copy{ from: ResourceTy::Value(ResourceAccessPoint::Function(f)), of: CopyOf::Pointee { part }, ..} => {
                hover_messages::event_dot_copy_pointee_of_call(&self.deref_name(my_name), &f.name, f.inside.as_deref(), *part)
            }
            // `let x = Some(one());`: x holds a Some value around the
            // value one() returns (#1183).
            Event::Copy{ from: ResourceTy::Value(ResourceAccessPoint::Function(f)), of: CopyOf::IntoCtor(ctor), ..}
                if f.inside.is_none() && !f.name.rsplit("::").next().unwrap_or(&f.name).starts_with(|c: char| c.is_uppercase()) => {
                hover_messages::event_dot_init_from_call_in_ctor(&self.deref_name(my_name), &f.name, ctor, f.cast_to.as_deref())
            }
            Event::Copy{ from: ResourceTy::Value(ResourceAccessPoint::Function(f)), ..} => {
                hover_messages::event_dot_init_from_call(&self.deref_name(my_name), &f.name, f.cast_to.as_deref(), f.inside.as_deref())
            }
            // `let f = Fruit::Apple(z);`: f holds the value the
            // constructor builds, not a copy of z (#866).
            Event::Copy{ of: CopyOf::IntoCtor(ctor), ..} => {
                hover_messages::event_dot_init_from_call(&self.deref_name(my_name), ctor, None, None)
            }
            // `if let Some(x) = opt`: x gets a copy of the value inside
            // opt, not of opt (#866).
            Event::Copy{ from, of: CopyOf::Inside, ..} => {
                hover_messages::event_dot_copy_from_inside(&self.deref_name(my_name), &target_name(from))
            }
            // `let Some(&y) = orr`: y gets a copy of n, which the
            // reference inside orr points to, not a copy of orr (#874).
            Event::Copy{ from, of: CopyOf::Pointee { part }, ..} => {
                hover_messages::event_dot_copy_from_pointee(&self.deref_name(my_name), &target_name(from), *part)
            }
            Event::Copy{ from ,..} => {
                hover_messages::event_dot_copy_from(&self.deref_name(my_name), &target_name(from))
            }
            Event::MutableBorrow{ from, is, of_ref_var, ..} => {
                if is.is_closure() {
                    hover_messages::event_dot_closure_capture_mut_borrow(&self.deref_name(my_name), &from.name())
                } else if is.holds_borrow() && matches!(from, ResourceTy::Caller) {
                    // `let k = Box::new(r);` with r a `&mut` parameter (#1289).
                    hover_messages::event_dot_holder_borrow_caller(&self.deref_name(my_name))
                } else if is.holds_borrow() {
                    // `refs.push(&mut m)` stores a borrow of m itself (#973).
                    hover_messages::event_dot_holder_borrow(&self.deref_name(my_name), &from.name(), true, if *of_ref_var { hover_messages::HolderLender::Own } else { holder_lender_kind(from) }, *of_ref_var || holds_copy_value(from))
                } else if matches!(is, ResourceTy::Value(r) if r.is_ref() && !r.is_mutref()) {
                    hover_messages::event_dot_mut_borrow_by_shared_ref(&self.deref_name(my_name), *of_ref_var || loan_holds_value(from, is))
                } else {
                    // `&mut r` borrows the reference r holds, a value (#914).
                    hover_messages::event_dot_mut_borrow(&self.deref_name(my_name), *of_ref_var || borrow_holds_value(from, is))
                }
            }
            Event::StaticBorrow{ from, is, of_ref_var, ..} => {
                if is.is_closure() {
                    hover_messages::event_dot_closure_capture_static_borrow(&self.deref_name(my_name), &from.name())
                } else if is.holds_borrow() {
                    // `refs.push(&x)` stores a borrow of x itself (#973).
                    hover_messages::event_dot_holder_borrow(&self.deref_name(my_name), &from.name(), false, if *of_ref_var { hover_messages::HolderLender::Own } else { holder_lender_kind(from) }, *of_ref_var || holds_copy_value(from))
                } else if matches!(is, ResourceTy::Value(r) if r.is_mutref()) {
                    hover_messages::event_dot_static_borrow_by_mut_ref(&self.deref_name(my_name), *of_ref_var || loan_holds_value(from, is))
                } else {
                    // `&r` borrows the reference r holds, a value (#925).
                    hover_messages::event_dot_static_borrow(&self.deref_name(my_name), *of_ref_var || borrow_holds_value(from, is))
                }
            }
            // A reference copied out of `it` gave back its part of the
            // borrow `it` still holds (#575).
            Event::StaticReacquire{ from, is, still_borrowed_by, of_ref_var, ..} | Event::MutableReacquire{ from, is, still_borrowed_by, of_ref_var, ..}
                if !still_borrowed_by.is_empty() =>
            {
                let mutable = matches!(self, Event::MutableReacquire { .. });
                let from_name = target_name(from);
                // `look(&x)` with x a reference variable borrowed x
                // itself (#938, #974).
                let of_ref_var = *of_ref_var && ref_lender(is).is_some();
                if let (false, Some(r)) = (of_ref_var, ref_lender(is)) {
                    return hover_messages::event_dot_reacquire_through_ref_still_borrowed(r.name(), &from_name, still_borrowed_by, mutable, r.points_to_copy());
                }
                hover_messages::event_dot_reacquire_still_borrowed(&self.deref_name(my_name), &from_name, still_borrowed_by, mutable, holds_copy_value(is), of_ref_var)
            }
            Event::MutableReacquire{ is, of_ref_var: true, ..} if ref_lender(is).is_some() => {
                hover_messages::event_dot_mut_reacquire_ref_var(ref_lender(is).unwrap().name())
            }
            Event::StaticReacquire{ is, of_ref_var: true, ..} if ref_lender(is).is_some() => {
                hover_messages::event_dot_static_reacquire_ref_var(ref_lender(is).unwrap().name())
            }
            Event::StaticReacquire{ from, is, ..} | Event::MutableReacquire{ from, is, ..} if ref_lender(is).is_some() => {
                let r = ref_lender(is).unwrap();
                let mutable = matches!(self, Event::MutableReacquire { .. });
                hover_messages::event_dot_reacquire_through_ref(r.name(), borrower_name(from).as_ref(), mutable, r.is_mutref(), r.points_to_copy())
            }
            Event::StaticReacquire{ is, ..} => {
                hover_messages::event_dot_static_reacquire(&self.deref_name(my_name), holds_copy_value(is))
            }
            Event::MutableReacquire{ is, ..} => {
                hover_messages::event_dot_mut_reacquire(&self.deref_name(my_name), holds_copy_value(is))
            }
            Event::RefDie {..} => {
              panic!("internal state: a RefDie event has no hover text of its own");
            },
            Event::Branch { .. } => {
              format!("{} is live in a conditional expression ", my_name)
            }
        } 
    }
}

#[derive(Debug, Clone)]
pub struct Timeline {
    pub resource_access_point: ResourceAccessPoint, 

    // List of events associated with this RAP
    pub history: Vec<(usize, Event)>, 

    // States derived from the history
    pub states: Vec<(usize, usize, State)>
}

// a vector of structs information
#[derive(Debug)]
pub struct StructsInfo {
    // struct owner hash, x val of struct owner, x val of the rightmost
    // member, half header-label width of the owner and of the rightmost
    // member
    pub structs: Vec<(i64, i64, i64, i64, i64)>,
}

/// A field's borrows from, or borrow ends to, several lenders on one row
/// (`let d = two(&s1, &s2);` with `Two<'a> { a: &'a str, b: &'a str }`,
/// or the end of a field a conditional's arms pointed at different
/// lenders): a field holds one reference, so it points into one of them,
/// and the borrow checker keeps all of them borrowed while it is in use.
/// One arrow is drawn per field (#1354), from or to the first lender in
/// source order, and its tooltip names all of them.
#[derive(Clone, Debug)]
pub struct FieldLenders {
    /// Every lender, in source order.
    pub lenders: Vec<String>,
    /// This event's arrow is the one drawn (the others are left out).
    pub drawn: bool,
    /// The group is the field's borrow ending, not starting.
    pub end: bool,
}

// VisualizationData supplies all the information we need in the frontend,
// from rendering a PNG to px roducing an interactive HTML guide.
// The internal data is simple: a map from variable hash to its Timeline.
#[derive(Debug)]
pub struct VisualizationData {
    // When displaying all timelines in the frontend of choice, one should
    // consider picking a hash function that gives the BTreeMap a sensible order.
    //      timelines: an orderred map from a Variable's hash to 
    //      the Variable's Timeline.
    pub timelines: BTreeMap<u64, Timeline>,

    pub external_events: Vec<(usize, ExternalEvent)>,
    //temp container for external_events
    pub preprocess_external_events: Vec<(usize, ExternalEvent)>,
    //line_info
    pub event_line_map: BTreeMap<usize, Vec<ExternalEvent>>,

    pub num_valid_raps: usize,

    /// rap.hash() → line of the fn signature where the RAP was
    /// registered. Used by the renderer to group columns and place
    /// labels per-fn instead of in one shared row at the top of
    /// the SVG. Populated from plugin.rs's merged rap_map.
    pub fn_start_lines: HashMap<u64, usize>,

    /// rap.hash() → what to say in the hover text of a numbered column
    /// header (`s₂`); only columns whose name is numbered have one.
    pub shadow_notes: HashMap<u64, ShadowNote>,

    /// (source, binding) column pairs of pattern bindings that are drawn
    /// side by side on their `if let` line, each binding's column right
    /// after its source's (see `side_by_side_bindings`, #510). Starts with
    /// the let-chain bindings the plugin found (`chain_binding_columns`,
    /// #583, #584); `render_svg` adds the arms' pattern bindings.
    pub binding_columns: Vec<(u64, u64)>,

    /// Ids of moves out of a place that the place's column draws before
    /// the new value it gets on the same line, though the plugin
    /// emitted the new value first: a destructuring assignment's cycle
    /// (`(a, b) = (b, a)`) reads every part before assigning any, so a
    /// gives its value to b before it gets b's (#1343). Also the ids of
    /// the plugin's re-acquire `Bind`s for those places, which aren't
    /// drawn.
    pub moves_out_first: HashSet<usize>,

    /// Ids of the moves out of a split scrutinee temporary that leave
    /// nothing in it that needs drop: its leg in that arm ends there
    /// (#1386). Other moves out of an unbound temporary leave it
    /// holding the rest.
    pub temp_emptying_moves: HashSet<usize>,

    /// Arrows of a struct write that share a track (#624): event id ->
    /// the id of its track's first arrow. Filled in by `render_svg` (see
    /// `struct_write_tracks`); the renderer draws a line's tracks half a
    /// row apart instead of a whole row per arrow.
    pub arrow_tracks: HashMap<usize, usize>,

    /// The borrows into, and ends out of, a reference field that may
    /// point into any of several lenders (#1354): event id -> the
    /// lenders and whether that event's arrow is the one drawn. Filled
    /// in by `render_svg` (`timeline_panel::field_lender_groups`).
    pub field_lenders: HashMap<usize, FieldLenders>,

    /// Notes on dots saying what is simplified (`ApproxNote`), in the
    /// order the plugin found them.
    pub approx_notes: Vec<ApproxNote>,

    /// The source line the code panel shows on each visual row (`None`
    /// on rows it adds), set by `render_svg` before the timeline is
    /// drawn: approximation notes are placed by source line.
    pub row_lines: Vec<Option<usize>>,

    /// Columns (rap.hash()) of fns whose column headers go two rows above
    /// the signature instead of one, set by `render_svg`: a conditional
    /// whose arms share the signature's line (`fn pick(x, y) -> &str { if
    /// … { x } else { y } }`) forks a row above it, on the row the headers
    /// sit on (#1342).
    pub raised_headers: HashSet<u64>,
}

impl VisualizationData {
    /// Whether `event` is the lend of a call that reborrows through a
    /// `&mut` the column's value holds (`k.push('!')` with `k: Box<&mut
    /// String>`; `DeepPlace::held_mut`, #1282): k needn't be `mut`.
    fn lends_through_held_ref(&self, event: &Event) -> bool {
        fn find(events: &[(usize, ExternalEvent)], id: usize) -> bool {
            events.iter().any(|(_, e)| match e {
                ExternalEvent::PassByMutableReference { id: i, deep: Some(DeepPlace { held_mut: Some(true), .. }), .. } => *i == id,
                ExternalEvent::Branch { branches, .. } => branches.iter().any(|b| find(&b.e_data, id)),
                _ => false,
            })
        }
        let Event::MutableLend { id, .. } = event else { return false };
        find(&self.external_events, *id)
    }

    /// Whether a DeadLoanNote on column `hash` is shown: the column is
    /// drawn borrowed at the write (`state`, its state there). A struct
    /// field's column doesn't draw its parent's borrows, so the note on
    /// a field write is always shown. (See `call_lend_line` for a call
    /// that borrows the column for itself.)
    fn dead_loan_note_shown(&self, state: &State, hash: &u64) -> bool {
        matches!(state, State::PartialPrivilege { .. } | State::RevokedPrivilege { .. })
            || matches!(self.timelines.get(hash).map(|t| &t.resource_access_point),
                Some(ResourceAccessPoint::Struct(Struct { is_member: true, .. })))
    }
}

/// A call that borrows a column mutably for itself only (`s.push('!')`,
/// `take(&mut s)`, `s += ..`, #1452) lends and gives back on one line,
/// with one id, so the column isn't drawn borrowed after those events,
/// where a DeadLoanNote for that write comes. As for `let m = &mut s;`
/// there, whose lend leaves the column borrowed, the note is shown when
/// the column was drawn borrowed as the line began: the other loan is
/// drawn ending at this dot. (An assignment `s = ..` there gets none: the
/// dot shows the loan ending, #796's near misses.) Some(line) when `e`,
/// at `line`, is such a call's reacquire right after its lend
/// (`last_lend`).
fn call_lend_line(last_lend: &Option<(usize, usize)>, e: &Event, line: usize) -> Option<usize> {
    match e {
        Event::MutableReacquire { id, .. } if *last_lend == Some((line, *id)) => Some(line),
        _ => None,
    }
}

/// A note saying that what the figure shows of a construct is a
/// simplification RustViz makes (a loop drawn as one pass, #853). It is
/// added to the tooltip of an event dot that is drawn anyway, never
/// drawn as a dot of its own: a column with no dot where a note would
/// go gets none.
#[derive(Debug, Clone)]
pub enum ApproxNote {
    /// `text` on the first event dot of column `hash` on a source line
    /// in `lines` (inclusive).
    At { hash: u64, lines: (usize, usize), text: String },
    /// A loop whose first line is `head`. It gets one note, on its
    /// first event dot in source order (then leftmost) that is drawn for
    /// a pass: on a line in `pass` (inclusive), or on `head` for a
    /// column in `head_cols` (a `for` loop's pattern). Only columns the
    /// loop declares (`locals`) or that have a dot before `head` count:
    /// a column first given a value inside the loop is the loop's
    /// receiver (`let n = loop { … break v; }`, bound once) or a
    /// temporary. Dots on the lines of a loop inside it (`nested`) don't
    /// count: a loop with no other dot gets no note. `declares`: whether the loop declares
    /// any variable (the note then says each pass has its own).
    /// `at_most_once`: a `while` / `for` loop that never comes back
    /// round (every pass leaves it, #1467), so it runs its body once or
    /// not at all. (A bare `loop` that never comes back round runs it
    /// once and gets no note.)
    Loop {
        head: usize,
        pass: (usize, usize),
        head_cols: Vec<u64>,
        locals: Vec<u64>,
        nested: Vec<(usize, usize)>,
        declares: bool,
        at_most_once: bool,
    },
}

impl ApproxNote {
    /// The note with its source lines mapped by `f` (lines hidden from
    /// the figure removed before it).
    pub fn map_lines(&mut self, f: impl Fn(usize) -> usize) {
        match self {
            ApproxNote::At { lines, .. } => *lines = (f(lines.0), f(lines.1)),
            ApproxNote::Loop { head, pass, nested, .. } => {
                *head = f(*head);
                *pass = (f(pass.0), f(pass.1));
                for n in nested.iter_mut() {
                    *n = (f(n.0), f(n.1));
                }
            }
        }
    }
}

/// About a numbered column (`s₂`): a variable of the same name as an
/// earlier one in its fn.
#[derive(Debug, Clone)]
pub struct ShadowNote {
    /// The variable it shadows (its column name, e.g. `s`), if it
    /// shadows one.
    pub shadows: Option<String>,
    /// Source lines of the declarations drawn in this column: several
    /// when same-named variables of different branches or blocks share
    /// it.
    pub lines: Vec<usize>,
    /// Declaration lines of same-named variables in separate scopes
    /// that have columns of their own because they are a different
    /// kind of value (a reference, an owner, …; #598).
    pub apart_from: Vec<usize>,
}

/// The RAPs of `raps` in declaration order: by RAP hash (assigned from
/// a counter as the visitor registers each RAP, so it follows the
/// source), then by name.
///
/// Use this instead of iterating a `HashSet`/`HashMap` of RAPs directly
/// whenever the loop emits events or assigns event ids: std's hash
/// collections iterate in a per-process random order, so the plugin's
/// output would differ from run to run (#211).
pub fn in_decl_order<'a>(raps: impl IntoIterator<Item = &'a ResourceAccessPoint>) -> Vec<&'a ResourceAccessPoint> {
    let mut v: Vec<&ResourceAccessPoint> = raps.into_iter().collect();
    v.sort_by(|a, b| ResourceAccessPoint::hash(a).cmp(ResourceAccessPoint::hash(b)).then_with(|| a.name().cmp(b.name())));
    v
}

#[allow(non_snake_case)]
pub fn ResourceAccessPoint_extract (external_event : &ExternalEvent) -> (&ResourceTy, &ResourceTy){
    let (from, to) = match external_event {
        ExternalEvent::Bind{from: from_ro, to: to_ro, ..} => (from_ro, to_ro),
        ExternalEvent::Copy{from: from_ro, to: to_ro, ..} => (from_ro, to_ro),
        ExternalEvent::Move{from: from_ro, to: to_ro, .. } => (from_ro, to_ro),
        ExternalEvent::StaticBorrow{from: from_ro, to: to_ro, .. } => (from_ro, to_ro),
        ExternalEvent::StaticDie{from: from_ro, to: to_ro, .. } => (from_ro, to_ro),
        ExternalEvent::MutableBorrow{from: from_ro, to: to_ro, .. } => (from_ro, to_ro),
        ExternalEvent::MutableDie{from: from_ro, to: to_ro, .. } => (from_ro, to_ro),
        ExternalEvent::PassByMutableReference{from: from_ro, to: to_ro, .. } => (from_ro, to_ro),
        ExternalEvent::PassByStaticReference{from: from_ro, to: to_ro, .. } => (from_ro, to_ro),
        _ => (&ResourceTy::Anonymous, &ResourceTy::Anonymous)
    };
    (from, to)
}

/// `Some` iff `from` is the result of an `Rc` / `Arc` clone or
/// downgrade call: the new owner shares the value, it doesn't copy it.
pub fn rc_call_of(from: &ResourceTy) -> Option<RcCall> {
    match from.extract_rap() {
        Some(ResourceAccessPoint::Function(f)) => f.rc_call,
        _ => None,
    }
}

/// Whether a borrow is kept by a closure a call returned (#686).
pub fn returned_closure(via: &Option<BorrowVia>) -> bool {
    matches!(via, Some(BorrowVia { carrier: ViaCarrier::ReturnedClosure { .. }, .. }))
}


// fulfills the promise that we can support all the methods that a
// frontend would need.
impl Visualizable for VisualizationData {
    fn get_name_from_hash(&self, hash: &u64) -> Option<String> {
        match self.timelines.get(hash) {
            Some(timeline) => Some(timeline.resource_access_point.name().to_owned()),
            _ => None
        }
    }

    // if the ResourceAccessPoint is declared mutable
    fn is_mut(&self, hash: &u64) -> bool {
        self.timelines[hash].resource_access_point.is_mut()
    }

    // if the ResourceAccessPoint is a function
    fn is_mutref(&self, hash: &u64) -> bool {
        self.timelines[hash].resource_access_point.is_mutref()
    }

    fn is_ref(&self, hash: &u64) -> bool {
      self.timelines[hash].resource_access_point.is_ref()
    }

    // This function calculates the next State of a RAP given the previous State and the current event
    // It should honestly be rewritten since it was born out of RV1 code and since changed a lot given the changes
    // with RV2. For example - there really shouldn't ever be 'Invalid' States for a RAP (this had to be accounted for
    // in RV1 since the code didn't have to actually compile for a visualization to be generated) since in RV2 the example code
    // actually has to be valid for our plugin to even run. I keep Invalid around though because its helpful for debugging
    fn calc_state(&self, previous_state: & State, event: & Event, event_line: usize, hash: &u64) -> State {
        /* a Variable cannot borrow or return resource from Functions, 
        but can 'lend' or 'reaquire' to Functions (pass itself by reference and take it back); */
        fn event_invalid(event: & Event) -> bool {
            match event {
                Event::StaticBorrow{ from: ResourceTy::Value(ResourceAccessPoint::Function(_)) ,..} => true,
                Event::MutableBorrow{ from: ResourceTy::Value(ResourceAccessPoint::Function(_)) ,..} => true,
                Event::StaticDie{ to: ResourceTy::Value(ResourceAccessPoint::Function(_)) ,..} => true,
                Event::MutableDie{ to: ResourceTy::Value(ResourceAccessPoint::Function(_)) ,..} => true,
                _ => false,
            }
        }
        if event_invalid(event) { return State::Invalid; }
        // An or-pattern's possible borrow (#565) changes the state the
        // way the plain borrow event it stands for does.
        if let Some(plain) = event.or_alt_plain_equivalent() {
            return self.calc_state(previous_state, &plain, event_line, hash);
        }

        // A value that holds a borrow without being a reference
        // (`let it = v.iter();`, #336) owns its own value: borrowing
        // through it, and the end of that borrow, don't change what
        // the holder itself can do.
        // So does an owner a stored borrow's end is drawn on after the
        // value holding it was moved there (`let y = w;` with w given a
        // borrow by `keep_mut(&mut w, &mut m)`, #1198): the end of that
        // borrow isn't the end of y's own value.
        let holder = self.timelines.get(hash)
            .is_some_and(|t| t.resource_access_point.holds_borrow()
                || (matches!(event, Event::MutableDie { .. } | Event::StaticDie { .. })
                    && matches!(t.resource_access_point, ResourceAccessPoint::Owner(_))));

        // A borrow of one field of what a reference points to
        // (`&mut self.items`, `self.items.iter_mut()` in a `for` head,
        // `self.items.first_mut()` kept by its result), and the end of
        // that borrow: rustc restricts only that field (`self.n += 1`
        // still compiles; #1073), so the reference itself keeps its
        // state, as an owner `s` does while `s.items` is lent. The
        // texts name the field.
        let field_of_this_ref = |is: &ResourceTy| self.is_ref(hash)
            && matches!(is, ResourceTy::DerefField(r, _, _, _) if r.hash() == hash);
        match event {
            Event::MutableLend { is, .. } | Event::StaticLend { is, .. }
            | Event::MutableReacquire { is, .. } | Event::StaticReacquire { is, .. }
                if field_of_this_ref(is)
                    && matches!(previous_state, State::FullPrivilege { .. } | State::PartialPrivilege { .. }) =>
                return previous_state.clone(),
            _ => {}
        }

        match (previous_state, event) {
            // propogate invalid states
            (State::Invalid, _) => State::Invalid,

            (State::OutOfScope, Event::StaticBorrow { .. })
            | (State::OutOfScope, Event::MutableBorrow { .. }) if holder =>
                State::FullPrivilege { s: LineState::Full },
            (State::FullPrivilege { .. }, Event::StaticBorrow { .. })
            | (State::FullPrivilege { .. }, Event::MutableBorrow { .. })
            | (State::FullPrivilege { .. }, Event::StaticDie { .. })
            | (State::FullPrivilege { .. }, Event::MutableDie { .. })
            | (State::FullPrivilege { .. }, Event::RefDie { .. }) if holder =>
                (*previous_state).clone(),
            // The holder was moved (`let total: i32 = it.sum();`) on
            // the line its borrow ends, or lent out: the end of the
            // borrow it held doesn't change that.
            (State::ResourceMoved { .. }, Event::StaticDie { .. })
            | (State::ResourceMoved { .. }, Event::MutableDie { .. })
            | (State::ResourceMoved { .. }, Event::RefDie { .. })
            | (State::RevokedPrivilege { .. }, Event::StaticDie { .. })
            | (State::RevokedPrivilege { .. }, Event::MutableDie { .. })
            | (State::RevokedPrivilege { .. }, Event::RefDie { .. })
            | (State::PartialPrivilege { .. }, Event::StaticDie { .. })
            | (State::PartialPrivilege { .. }, Event::MutableDie { .. })
            | (State::PartialPrivilege { .. }, Event::RefDie { .. }) if holder =>
                (*previous_state).clone(),
            // A borrow stored into the holder through the `&mut` it is
            // lent to (`*el = &t1` with `el = &mut refs[0]`, #1361): it
            // stays lent until that borrow ends.
            (State::RevokedPrivilege { .. }, Event::StaticBorrow { .. })
            | (State::RevokedPrivilege { .. }, Event::MutableBorrow { .. }) if holder =>
                (*previous_state).clone(),

            // A `&mut` moved into a call (`take(x)`) has its borrow's
            // end drawn on it, on the same dot (#200): it stays moved.
            (State::ResourceMoved { .. }, Event::StaticDie { .. })
            | (State::ResourceMoved { .. }, Event::MutableDie { .. })
            | (State::ResourceMoved { .. }, Event::RefDie { .. }) if self.is_ref(hash) =>
                (*previous_state).clone(),
            // Borrows of fields of what a reference points to that end
            // on the row its own borrow ends (`&mut r.a` and `&r.b`
            // last used where r is, #1080): it holds nothing either way.
            (State::OutOfScope, Event::StaticReacquire { .. })
            | (State::OutOfScope, Event::MutableReacquire { .. }) if self.is_ref(hash) =>
                State::OutOfScope,
            // `break k` out of a loop whose body declares `k` (#431): the
            // move is drawn on the row where `k` goes out of scope on the
            // other iterations, after that.
            (State::OutOfScope, Event::Move { break_out: true, .. }) => State::OutOfScope,

            // A reference copied out of `it` ended its part of the
            // borrow `it` still holds: the lender stays borrowed (#575).
            (_, Event::StaticReacquire { still_borrowed_by, .. })
            | (_, Event::MutableReacquire { still_borrowed_by, .. }) if !still_borrowed_by.is_empty() =>
                (*previous_state).clone(),

            // A reference's line shows what it can do through itself,
            // which its own type decides, not the kind of loan it
            // holds. A shared reference can hold a mutable loan (`let
            // r: &String = &mut s;`, `downgrade(&mut s)` returning a
            // `&str`): it can still only read (#1122). A `&mut` can hold
            // a shared loan (`let r = &mut &x;`, a `&mut` to a temporary
            // that shares x): it can write what it points to, the
            // temporary (#1131).
            // (Also after the copy that binds it in a pattern, or from a
            // further lender.)
            (State::OutOfScope | State::FullPrivilege { .. } | State::PartialPrivilege { .. }, Event::MutableBorrow { .. })
                if self.is_ref(hash) && !self.is_mutref(hash) =>
                State::PartialPrivilege { s: LineState::Full },
            (State::PartialPrivilege { .. }, Event::MutableDie { .. }) if self.is_ref(hash) && !self.is_mutref(hash) =>
                State::OutOfScope,
            (State::OutOfScope, Event::StaticBorrow { .. }) if self.is_mutref(hash) =>
                State::FullPrivilege { s: LineState::Full },
            (State::FullPrivilege { .. }, Event::StaticDie { .. }) if self.is_mutref(hash) =>
                State::OutOfScope,
            // A shared reference bound to (or given) a value that borrows
            // no variable here (`let mut r: &str = "hello";`, `r =
            // "world";`, `q = name();` with `name() -> &'static str`), or
            // a copy of another reference
            // can only read, like one made by `&a`: its line is drawn the
            // same way throughout, not solid until its first read (#1150).
            (State::OutOfScope | State::FullPrivilege { .. } | State::PartialPrivilege { .. }, Event::Acquire { .. } | Event::Copy { .. })
                if self.is_ref(hash) && !self.is_mutref(hash) =>
                State::PartialPrivilege { s: LineState::Full },

            // A variable is initialized through a Move
            (State::OutOfScope, Event::Acquire{ .. }) => State::FullPrivilege {s: LineState::Full},

            // A variable is initialized from a Copy
            (State::OutOfScope, Event::Copy{ from: ro, is: is_ro, ..}) => {
                match ro {
                    ResourceTy::Anonymous => State::FullPrivilege {s: LineState::Full},
                    ResourceTy::Deref(r) | ResourceTy::Value(r) | ResourceTy::DerefField(r, _, _, _) | ResourceTy::Index(r, _, _) => {
                        // if we are copying a reference - then the State must be PartialPrivilege
                        // ex: 
                        // let a = &b; (ImmutableBorrow(b -> a))
                        // let c = a; (Copy(a -> c)) even though c is technically borrowing from b at the end of the day
                        if r.is_ref() && is_ro.is_ref() {
                            if r.is_mutref() {
                                panic!("internal state: copying a `&mut` reference (it should have been a move)");
                            }
                            else {
                                // we have to be copying an immutable reference, so the state must be partial
                                State::PartialPrivilege { s: LineState::Full }
                            }   
                        }
                        else {
                            State::FullPrivilege {s: LineState::Full}
                        }
                    }
                    _ => panic!("internal state: a copy-initialization from a resource that is not a value, deref or anonymous")
                }
            }

            // variable intialized with an immutable borrow
            (State::OutOfScope, Event::StaticBorrow{ from: _ro,.. }) =>
                State::PartialPrivilege {
                    s: LineState::Full
                },
            
            // variable intialized with a mutable borrow
            (State::OutOfScope, Event::MutableBorrow{ .. }) => State::FullPrivilege{s: LineState::Full},

            // A function parameter is initialized
            (State::OutOfScope, Event::InitRefParam{ param: ro, .. })  => {
                match ro {
                    ResourceAccessPoint::Function(..) => {
                        panic!("internal state: a function was initialized as a parameter")
                    },
                    ResourceAccessPoint::Owner(..) | ResourceAccessPoint::MutRef(..) => {
                        State::FullPrivilege{s:LineState::Full}
                    },
                    ResourceAccessPoint::Struct(..) => {
                        State::FullPrivilege { s: LineState::Full }
                    },
                    ResourceAccessPoint::StaticRef(..) => {
                        State::PartialPrivilege { s: LineState::Full }
                    }
                }
            },

            // A field or a pattern's binding moved out of an unbound
            // temporary: it still holds the rest, until its drop (#469).
            (State::FullPrivilege{..}, Event::Move{is: ResourceTy::Value(r), empties_temp: false, ..}) if r.temp_part().is_some() =>
                (*previous_state).clone(),
            // `x => …` on `match *b` / `let x = *b;`, with b a Box
            // drawn as its payload's part columns: the payload moves
            // out, which those columns show; b keeps its heap
            // allocation, freed at its scope end (#898).
            (State::FullPrivilege{..}, Event::Move{is: ResourceTy::Deref(ResourceAccessPoint::Struct(Struct { is_box: true, .. })), ..}) =>
                (*previous_state).clone(),
            // A labeled block's local carried out by one of the block's
            // ways out (#1288): on the paths that reach the block's end
            // it still holds its value, which is dropped there.
            (State::FullPrivilege{..}, Event::Move{ break_out: true, break_alt: Some(BreakAlt { end: AltEnd::Block | AltEnd::BlockTail, .. }), .. }) =>
                (*previous_state).clone(),
            // `[c, _] => …` on an array that keeps its other elements
            // (`Owner::elems_left`): one element moves out, and the array
            // still holds the rest until its scope end, where they are
            // dropped (#1285).
            // Unless it takes the last elements left in it (#1302).
            (State::FullPrivilege{..}, Event::Move{is: ResourceTy::Index(r, place, _), ..})
                if !place.starts_with('*') && r.elems_left().is_some_and(|e| !e.empties(place)) =>
                (*previous_state).clone(),
            // The current RAP's resource is moved
            (State::FullPrivilege{..}, Event::Move{to: to_ro,..}) =>
                State::ResourceMoved{ move_to: to_ro.to_owned(), move_at_line: event_line, dropped_by_hand: false },

            // Calling an `FnOnce` closure moves it into the call (#271):
            // the body consumes what it captured, so there's nothing
            // left for the closure to drop at scope end.
            (State::FullPrivilege{..}, Event::ClosureCall{..}) =>
                State::ResourceMoved{ move_to: ResourceTy::Anonymous, move_at_line: event_line, dropped_by_hand: false },
            // `'blk: { if c { break 'blk k; } drop(k); .. }` (#1288): the
            // `break`'s move is drawn where the block ends, below the
            // move on the way there; on its own path it came first.
            (State::ResourceMoved{ .. }, Event::Move{ break_alt: Some(BreakAlt { end: AltEnd::Block, in_loop: true, .. }), .. }) =>
                (*previous_state).clone(),
            // `let t = f();` also records the call's result as moving
            // out of `f`, on the same line: `f` is already moved.
            (State::ResourceMoved{ move_at_line, .. }, Event::Move{..}) if *move_at_line == event_line && event_line != 0 =>
                (*previous_state).clone(),

            // A write through what a `&mut` reference moved into a
            // `for` loop's iterator hands out (`*y += 1` with y from
            // `a.iter_mut().zip(b)`, #1076): it goes into the resource
            // b points to, but b itself stays moved.
            (State::ResourceMoved{ .. }, Event::Acquire{ is: ResourceTy::Index(..), .. }) if self.is_mutref(hash) =>
                (*previous_state).clone(),

            // The current RAP reaquires a resource 
            (State::ResourceMoved{ .. }, Event::Acquire{ .. }) => {
                if self.is_mut(hash) {
                    State::FullPrivilege{ s: LineState::Full }
                }
                else { // immut variables cannot reacquire resource
                    panic!("internal state: immutable variable `{}` re-acquires a resource after a move", self.get_name_from_hash(hash).unwrap_or_else(|| format!("#{}", hash)));
                }
            },

            // A `&T` field given another's reference with its struct
            // (`o.inner = other;` moves other.r into o.inner.r, #1199):
            // it holds that one now, as after `o.inner.r = other.r;`.
            (State::PartialPrivilege{..}, Event::Acquire{ .. }) if self.is_ref(hash) && !self.is_mutref(hash) =>
                State::PartialPrivilege { s: LineState::Full },
            // A `&T` is only moved as part of a struct (`let hold =
            // other;` moves other.r): the field has no reference left.
            (State::PartialPrivilege{..}, Event::Move{to: to_ro,..}) if self.is_ref(hash) && !self.is_mutref(hash) =>
                State::ResourceMoved{ move_to: to_ro.to_owned(), move_at_line: event_line, dropped_by_hand: false },
            // An owner drawn as lent is moved: rustc allows it only where
            // the loan is already dead (`r = &t; let m = s;` after `r =
            // &s`, #1178). It is moved; this used to fall through to
            // Invalid, which the join of a conditional then ignored.
            (State::PartialPrivilege{..}, Event::Move{to: to_ro,..})
            | (State::RevokedPrivilege{..}, Event::Move{to: to_ro,..}) if !self.is_ref(hash) =>
                State::ResourceMoved{ move_to: to_ro.to_owned(), move_at_line: event_line, dropped_by_hand: false },
            // A reference whose reference was moved out (`let q2 = q;`)
            // is given a new one (`q = &mut s;`, #722): it holds that,
            // as when it was first initialized with a borrow.
            (State::ResourceMoved{ .. }, Event::StaticBorrow{ .. })
            | (State::ResourceMoved{ .. }, Event::Copy{ .. }) if self.is_ref(hash) && !self.is_mutref(hash) =>
                State::PartialPrivilege { s: LineState::Full },
            (State::ResourceMoved{ .. }, Event::MutableBorrow{ .. }) if self.is_mutref(hash) =>
                State::FullPrivilege { s: LineState::Full },
            // `r2 = r1;` while r2 still holds its old `&T` (its end is
            // drawn on a row below, `(r1, r2) = (r2, r1);`, #722): r2
            // holds the copy.
            (State::PartialPrivilege{ .. }, Event::Copy{ .. }) if self.is_ref(hash) && !self.is_mutref(hash) =>
                State::PartialPrivilege { s: LineState::Full },

            (State::FullPrivilege{..}, Event::MutableLend{ to: to_ro ,..}) => {
            // Assumption: variables can lend mutably if
            // 1) variable instance is mutable or 2) variable is a mutable reference
            // Use cases: 'mutable_borrow' & 'nll_lexical_scope_different'
            // 3) or it holds a `&mut` the call reborrows through: `k.push('!')`
            // with `k: Box<&mut String>`, k not `mut` (#1282).
                if self.is_mut(hash) | self.is_mutref(hash) | holder | self.lends_through_held_ref(event) {
                    State::RevokedPrivilege{ to: ResourceTy::Anonymous, borrow_to: to_ro.to_owned(), prev_state: Box::from(previous_state.to_owned()) }
                } else {
                    State::Invalid
                }
            },
            
            // Closure binding takes a mutable capture: the closure
            // is at FullPrivilege from its Bind-Acquire, takes a
            // mutable borrow of an upvar, and stays at
            // FullPrivilege — the borrow is internal to the
            // closure's value, not a state change of `f` itself.
            // Mirror for MutableDie at the closure's NLL last use:
            // the borrow ends but the closure binding survives,
            // unlike an ordinary MutRef binding (which the next
            // arm covers).
            (State::FullPrivilege{..}, Event::MutableBorrow { is, .. })
            | (State::FullPrivilege{..}, Event::MutableDie { is, .. })
                if is.is_closure() =>
            {
                State::FullPrivilege { s: LineState::Full }
            }

            // Second lender of a `&mut` returned by a call tied to two
            // `&mut` arguments (see the StaticBorrow arm below).
            (State::FullPrivilege{..}, Event::MutableBorrow { .. }) if self.is_mutref(hash) =>
                State::FullPrivilege { s: LineState::Full },

            // happends when a mutable reference returns, invalid otherwise
            (State::FullPrivilege{..}, Event::MutableDie{ .. }) =>
                State::OutOfScope,
            // A `&mut` whose own borrow ends while a shared reborrow
            // through it is still live (`let first = &*r; r = &mut b;`
            // with `first` used later, #1182): r holds no reference
            // until it is given its next one.
            (State::PartialPrivilege{..}, Event::MutableDie{ .. }) if self.is_mutref(hash) =>
                State::OutOfScope,

            (State::FullPrivilege{..}, Event::Acquire{ from: _ ,..}) | (State::FullPrivilege{..}, Event::Copy{ from: _ ,..}) => {
                    State::FullPrivilege{ s: LineState::Full }
            },

            (State::FullPrivilege{..}, Event::OwnerGoOutOfScope { .. }) =>
                State::OutOfScope,

            (State::FullPrivilege{..}, Event::RefGoOutOfScope { .. }) =>
                State::OutOfScope,

            // A write to a lender inside the span drawn for its loan (#778):
            // the borrow checker keeps the loan until its latest end over
            // the paths that meet after an `if`, but on the path through
            // the write the loan is already dead (the reference was
            // overwritten), so rustc accepts the write. The line goes on
            // to the loan's end.
            (State::PartialPrivilege{ .. }, Event::Acquire{ .. })
            | (State::PartialPrivilege{ .. }, Event::Copy{ .. })
            | (State::RevokedPrivilege{ .. }, Event::Acquire{ .. })
            | (State::RevokedPrivilege{ .. }, Event::Copy{ .. }) if !self.is_ref(hash) =>
                (*previous_state).clone(),
            // A write to an element through the reborrow a call made
            // (`*v.get_mut(0).unwrap() = 5` with v a `&mut Vec` parameter,
            // #861) is drawn before the reborrow ends: v keeps its state.
            (State::RevokedPrivilege{ .. }, Event::Acquire{ is: ResourceTy::Index(..), .. }) =>
                (*previous_state).clone(),

            (State::FullPrivilege{..}, Event::StaticLend{ ..}) =>
                State::PartialPrivilege { s: LineState::Full },

            (State::PartialPrivilege{ .. }, Event::MutableLend{ to: to_ro, .. }) => 
            State::RevokedPrivilege { to: ResourceTy::Anonymous, borrow_to: to_ro.to_owned(), prev_state: Box::from(previous_state.to_owned()) },

            (State::PartialPrivilege{ .. }, Event::StaticLend{ ..}) => {
                State::PartialPrivilege { s: LineState::Full }
            }

            // A reference returned by a call that ties its result to
            // several arguments (`longest(&a, &b)`) borrows from each
            // of them: one borrow event per lender, and one end per
            // lender when the reference's last use is reached (#171).
            (State::PartialPrivilege{ .. }, Event::StaticBorrow{ .. }) =>
                State::PartialPrivilege { s: LineState::Full },
            (State::OutOfScope, Event::StaticDie{ .. })
            | (State::OutOfScope, Event::MutableDie{ .. }) => State::OutOfScope,

            // `let w = first_mut(&mut v);`: v is mutably lent to the
            // call, and the call's result keeps that borrow (no
            // reacquire in between), so the lend to `w` continues the
            // same revocation rather than starting a second one.
            (State::RevokedPrivilege{ .. }, Event::MutableLend{ .. }) => (*previous_state).clone(),

            // A reference variable re-pointed through the `&mut` that
            // borrows it (`*r = &t` with `r = &mut x`, #904): x still
            // can't be used while r's borrow is live, and holds the new
            // reference (or none, after the old one's end) once it is
            // given back.
            (State::RevokedPrivilege{ to, borrow_to, .. }, Event::StaticBorrow{ .. })
            | (State::RevokedPrivilege{ to, borrow_to, .. }, Event::MutableBorrow{ .. })
            | (State::RevokedPrivilege{ to, borrow_to, .. }, Event::StaticDie{ .. })
            | (State::RevokedPrivilege{ to, borrow_to, .. }, Event::MutableDie{ .. }) if self.is_ref(hash) => {
                // (By the reference's own type, as above.)
                let holds = match event {
                    Event::StaticBorrow { .. } | Event::MutableBorrow { .. } if self.is_mutref(hash) => State::FullPrivilege { s: LineState::Full },
                    Event::StaticBorrow { .. } | Event::MutableBorrow { .. } => State::PartialPrivilege { s: LineState::Full },
                    _ => State::OutOfScope,
                };
                State::RevokedPrivilege { to: to.clone(), borrow_to: borrow_to.clone(), prev_state: Box::new(holds) }
            }
                
            // self statically borrowed resource, and it returns; TODO what about references to self?
            (State::PartialPrivilege{ .. }, Event::StaticDie{ .. }) =>
                State::OutOfScope,

            (State::PartialPrivilege{ .. }, Event::StaticReacquire{ is, ..}) => {
                if is.is_ref() && !is.is_mutref() { // TODO: think about if this is correct
                    State::PartialPrivilege { s: LineState::Full }
                }
                else {
                    State::FullPrivilege {s: LineState::Full}
                }
            }
            (State::PartialPrivilege { .. }, Event::RefDie { .. })=> {
              State::PartialPrivilege{s: LineState::Full}
            }

            (State::PartialPrivilege{ .. }, Event::OwnerGoOutOfScope { .. }) =>
                State::OutOfScope,

            (State::PartialPrivilege{ .. }, Event::RefGoOutOfScope { .. }) =>
                State::OutOfScope,

            (State::RevokedPrivilege{ prev_state: p,.. }, Event::MutableReacquire{ .. }) => {
              *p.clone()
            }
            // Maybe-moved after a conditional (one branch moved it,
            // another lent it out): the end of that borrow doesn't
            // give it back. It stays unusable until it is assigned
            // again (#1132); without this arm the state became Invalid
            // and the assignment drew no line either.
            (State::ResourceMoved{ .. }, Event::MutableReacquire{ .. })
            | (State::ResourceMoved{ .. }, Event::StaticReacquire{ .. }) =>
              (*previous_state).clone(),

            (State::FullPrivilege{..}, Event::StaticDie { .. }) |
            (State::FullPrivilege{..}, Event::StaticBorrow { .. }) => {
              State::FullPrivilege{s: LineState::Full}
            }

            // Multiple parallel borrows all returning at the same
            // line: the first reacquire transitions PartialPrivilege
            // → FullPrivilege (the lender now holds the resource
            // again), and any further reacquires at the same line
            // (one per parallel borrower — see the matching change
            // in expr_visitor.rs's ultimate-ref selection) need to
            // be no-ops on the lender's state. Without this, the
            // second reacquire fell through to the (_, _) catch-all
            // and made the state Invalid, blanking out the lender's
            // vertical timeline line for the rest of its scope.
            (State::FullPrivilege{..}, Event::StaticReacquire { .. }) => {
              State::FullPrivilege { s: LineState::Full }
            }
            // Symmetric handling for MutableReacquire on a
            // FullPrivilege lender. We don't currently emit multiple
            // mutable reacquires at the same line (Rust forbids
            // simultaneous mut borrows), but a no-op transition is
            // the right semantics regardless and protects against
            // future events landing in this state.
            (State::FullPrivilege{..}, Event::MutableReacquire { .. }) => {
              State::FullPrivilege { s: LineState::Full }
            }
            // A copy to the caller leaves the value where it is: in a
            // branch of the returned `if` / `match` (#674), what the
            // other branches do with it still counts.
            (_, Event::Duplicate { to: ResourceTy::Caller,..}) => (*previous_state).clone(),
            (_, Event::Duplicate { .. }) => (*previous_state).clone(),

            (_, Event::Branch { .. }) => { // technically not necessary - this is handled inside the compute_states function
                State::OutOfScope
            }
            (_, Event::OwnerGoOutOfScope { .. }) | (_, Event::RefGoOutOfScope { .. }) => State::OutOfScope,

            // OwnerDropAtReassign is paired with an Acquire/Move on
            // the same line that immediately re-establishes ownership,
            // so the visible state should be the same as before. We
            // preserve the previous state for any input — the dot is
            // a visual annotation, not an ownership transition.
            (_, Event::OwnerDropAtReassign { .. }) => (*previous_state).clone(),
            // Dropped only on the path that leaves early; the column
            // goes on for the others.
            (_, Event::TemporaryDrop { early: true, .. }) => (*previous_state).clone(),
            // An unbound temporary's column ends where it is dropped.
            (_, Event::TemporaryDrop { .. }) => State::OutOfScope,
            // Dropped only on the path that returns; the code after the
            // `?` runs on the other one.
            (_, Event::OwnerDropIfTryReturns { .. }) => (*previous_state).clone(),
            (_, Event::OwnerDropUnlessReturned) => (*previous_state).clone(),
            // A dot on a column whose value already went into the tuple:
            // no change.
            (_, Event::TupleTempDrop { .. }) => (*previous_state).clone(),
            // The iterator the variable was moved into is dropped: the
            // variable itself is unchanged (already moved).
            (_, Event::LoopIteratorDrop { .. }) => (*previous_state).clone(),
            // May have no value after the loop (#786): no line until
            // it's assigned again, like a maybe-moved one.
            (_, Event::LoopMaybeInit { .. }) => State::OutOfScope,
            // Owns a value at every `break` (#1000): what the body did
            // after the last one led back round, not out of the loop.
            (_, Event::LoopExitKept { .. }) => State::FullPrivilege { s: LineState::Full },

            // `ManuallyDrop::drop(&mut md)`: no resource left, and md
            // must not be used again.
            (_, Event::ManuallyDropByHand) =>
                State::ResourceMoved{ move_to: ResourceTy::Anonymous, move_at_line: event_line, dropped_by_hand: true },

            // Moved before a `break` out of the loop that just ended:
            // not usable after the loop, like after a conditional that
            // moved it on some paths (the scope-end dot says whether a
            // drop flag decides the drop).
            (_, Event::LoopExitMove { .. }) =>
                State::ResourceMoved{ move_to: ResourceTy::Anonymous, move_at_line: event_line, dropped_by_hand: false },
            // Moved into a name an or-pattern binds, if its alternative
            // matched: not usable afterwards, like after a conditional
            // that moved it on some paths (#533). A variable already
            // moved into a scrutinee tuple stays as it is.
            (_, Event::OrAltMove { tuple: Some(_), .. }) => (*previous_state).clone(),
            (_, Event::OrAltMove { .. }) =>
                State::ResourceMoved{ move_to: ResourceTy::Anonymous, move_at_line: event_line, dropped_by_hand: false },
            // Mapped to their plain equivalents above.
            (_, Event::OrAltLend { .. }) | (_, Event::OrAltBorrow { .. }) => State::Invalid,

            // Holds the old value or the new one: no change (#579).
            (_, Event::ChainFailAssign { .. }) => (*previous_state).clone(),
            // The borrows have ended (#688): no change.
            (_, Event::ChainFailUse { .. }) => (*previous_state).clone(),
            (_, Event::TiedParamNote { .. }) => (*previous_state).clone(),
            (_, Event::ReslicedInPlace { .. }) => (*previous_state).clone(),
            (_, Event::DeadElseNote) => (*previous_state).clone(),
            (_, Event::PassRestDrop { .. }) => (*previous_state).clone(),
            (_, Event::LentWithParent { .. }) => (*previous_state).clone(),
            (_, Event::DeadLoanNote { .. }) => (*previous_state).clone(),


            (_, _) => State::Invalid,
        }
    }

    fn compute_branch_states(&self,
    history: & mut Vec<(usize, Event)>,
    states: & mut Vec<(usize, usize, State)>,
    hash: &u64,
    valid_range: (usize, usize),
    (branch_start, branch_end): (usize, usize),
    mut previous_state: State) -> State{
        // Clamp valid_range to [branch_start, branch_end]. With the
        // convergence-one-line-earlier work, callers now pass
        // branch_end = merge_point - 1 so the branch column ends
        // where the convergence diagonal starts. The body span used
        // to derive valid_range still reaches `merge_point` (the
        // closing brace), which would push reversed-range states
        // (e.g. (7, 6, Gray)) without this clamp.
        let (begin, end) = valid_range;
        let begin = begin.clamp(branch_start, branch_end);
        let end = end.clamp(branch_start, branch_end);
        // A branch on one line can be handed a range that ends before
        // it begins (`(split + 1, if_end)` with `if_end == split`).
        let begin = begin.min(end);

        // Empty branches still render: dashed leading segment from
        // branch_start to begin (the OTHER branch's territory),
        // solid segment from begin to end (this branch's body
        // span — even with no per-line events the body range is
        // active here, so we want it solid), and a dashed trailing
        // segment from end to branch_end if any. Pre-fix, an empty
        // branch dropped to a single Gray segment over the entire
        // column, which made e.g. `else { println!(...) }` look
        // entirely dashed even though else is the active path on
        // its own body lines.
        if history.is_empty() {
            // No value before the branch and none assigned in it
            // (`let s;` initialized only in another branch, #234):
            // the whole column is "no value yet".
            if let State::OutOfScope = previous_state {
                states.push((branch_start, branch_end, branch_state_converter(&previous_state)));
                return previous_state;
            }
            if begin > branch_start {
                states.push((branch_start, begin, branch_state_converter(&previous_state)));
            }
            if begin < end {
                states.push((begin, end, previous_state.clone()));
            }
            if end < branch_end {
                states.push((end, branch_end, branch_state_converter(&previous_state)));
            }
            // A one-row column (`if c {\n take(s);\n }` with no
            // `else`, or `} else {}`) pushes nothing above. Record the
            // unchanged state anyway (zero-length, so nothing is drawn
            // for it): the merge reads each branch's end state from
            // `states.last()`, and with no entry an empty `else` read
            // as having moved the value (#257).
            if states.is_empty() {
                states.push((begin, end, previous_state.clone()));
            }
            return previous_state;
        }

        // Render opaque line if branch does not begin at split point.
        // Up to the arm's first event if that comes first: an event on
        // a row above the arm's body (the `else` of an `if let` chain
        // whose condition moved the value, `loop_exit_move`; a one-line
        // `if` in the chain's block on the split line, #1298) happens
        // before those rows, so they get the state it leaves.
        let lead_end = match history.first() {
            Some((l, _)) if *l < begin => (*l).max(branch_start),
            _ => begin,
        };
        if lead_end != branch_start {
            states.push((branch_start, lead_end, branch_state_converter(&previous_state)));
        }

        let is_ref = self.is_ref(hash);
        if is_ref {
            old_borrow_ends_first(history);
        }
        let kept = ends_kept_alive(is_ref, history);
        let mut previous_line = lead_end;
        let mut line_start: (usize, State) = (0, State::OutOfScope);
        let (mut last_lend, mut call_lent): (Option<(usize, usize)>, Option<usize>) = (None, None);
        for (i, (l, e)) in history.iter_mut().enumerate() {
            // For let-as-rhs (`let s = if … { … } else { … };`) the
            // parent's pre-branch state is OOS — the variable
            // doesn't exist yet — but the rows between this arm's
            // body span starting and its first acquire event are
            // structurally passive (a column-position is reserved,
            // we just haven't acquired into it yet). Render them
            // as Gray rather than blank so the column stays
            // visually continuous from the leading converge into
            // the acquire dot. After the first state-changing
            // event `previous_state` is no longer OOS so this
            // conversion no-ops for subsequent pushes; the nested-
            // branch OOS push below is in a different arm of the
            // match and stays unaffected.
            let pre_event_state = match previous_state {
                State::OutOfScope => branch_state_converter(&previous_state),
                ref s => s.clone(),
            };
            states.push((previous_line, *l, pre_event_state));
            match e {
                Event::Branch { branch_history, ty, split_point, merge_point, maybe_init, maybe_held, .. } => {
                    let entry_state = previous_state.clone();
                    // Timeline is empty in the middle during a branch
                    states.push((*split_point, *merge_point, State::OutOfScope));
                    // State of each branch that reaches the merge; a
                    // diverging branch (#296) never gets there.
                    let mut reaching_states: Vec<State> = Vec::new();
                    // State of each diverging branch that only breaks
                    // out of a loop: the code after the loop is reached
                    // in it (#759).
                    let mut broke_states: Vec<State> = Vec::new();
                    for (i, branch) in branch_history.iter_mut().enumerate() {
                        let diverges = branch.diverges;
                        branch.entry_state = Some(previous_state.clone());
                        let end_state = self.compute_branch_states(
                            & mut branch.e_data,
                            & mut branch.states,
                            hash,
                            ty.get_start_end(i),
                            // Branch column ends one line above the
                            // join dot — the convergence diagonal
                            // bridges that last row — unless the
                            // branches share lines (#117).
                            branch_column_rows(*split_point, *merge_point),
                            previous_state.clone());
                        if !diverges {
                            reaching_states.push(end_state);
                        } else if breaks_out_only(&branch.exits) {
                            broke_states.push(end_state);
                        }
                    }
                    // A `let x;` local assigned in only some of the
                    // branches (#244) has no line after the join: it
                    // can't be used until it's assigned again (like a
                    // maybe-moved one), and it doesn't surely own
                    // anything.
                    let unassigned_on_some_path = matches!(init_join(*maybe_init, reaching_states.iter()), Some(InitJoin::Maybe { .. }));
                    // So has one that may have held nothing where the
                    // conditional started and that only some branches
                    // assign (#1034).
                    let unassigned_on_some_path = unassigned_on_some_path || *maybe_held;
                    // If every recorded branch diverges, only a path
                    // the user didn't write can get past the
                    // conditional (the implicit empty `else` of an
                    // `if`), and it leaves the state as it was.
                    let mut ending_states = if reaching_states.is_empty() {
                        vec![previous_state.clone()]
                    } else {
                        reaching_states
                    };

                    ending_states.sort(); // partial order of the ending states
                    previous_state = convert_back(&ending_states.first().unwrap());
                    // (Inside an arm, the arm's column goes on, dashed, to
                    // the arm's end from that join: it may have no value
                    // there, #1437.)
                    if unassigned_on_some_path {
                        previous_state = State::FullPrivilege { s: LineState::Uninit };
                    }
                    // So does it from a join where no branch that gets there
                    // assigned it, when it had no value (or maybe none) as the
                    // conditional started (`if c { return; }` in the arm,
                    // #1533): the arm's line goes on from the join, not a
                    // row below it.
                    let no_value = |s: &State| matches!(s, State::OutOfScope | State::FullPrivilege { s: LineState::Uninit });
                    if matches!(previous_state, State::OutOfScope) && no_value(&entry_state) && ending_states.iter().all(no_value) {
                        previous_state = State::FullPrivilege { s: LineState::Uninit };
                    }
                    // In a branch, a value that may be held or not after
                    // this conditional (`if c { take(p); if d { p = … } }`,
                    // #1413) is drawn as after a move: nothing for the rest
                    // of the branch (not "no value yet"), and the
                    // conditional the branch is in joins as one that may
                    // have moved it.
                    if *maybe_held {
                        previous_state = State::ResourceMoved { move_to: ResourceTy::Anonymous, move_at_line: *merge_point, dropped_by_hand: false };
                    }
                    // Every branch leaves the function (#515): nothing
                    // follows the conditional, and the column ends in
                    // the branches.
                    // Unless the branches `break` out of a loop: the code
                    // after the loop follows them, with what they left
                    // (#759: `a` was borrowed in an arm and gave its
                    // resource back there; it is still dropped).
                    if !branch_history.is_empty() && branch_history.iter().all(|b| b.diverges) {
                        if broke_states.is_empty() {
                            previous_state = State::OutOfScope;
                        } else {
                            broke_states.sort();
                            previous_state = convert_back(&broke_states[0]);
                        }
                    }
                    // Resume the parent timeline at `merge_point`,
                    // not `merge_point + 1`. The join dot sits at
                    // `merge_point` (see render_dot's Branch arm);
                    // resuming a row later leaves a visible gap
                    // between the join and the post-merge state.
                    previous_line = *merge_point;
                }
                // A loop in this branch may have ended before assigning
                // it (#1495): as for a value a nested conditional may have
                // left with none (#1413), nothing is drawn for the rest of
                // the branch (not "no value yet", and no gap before that),
                // and the conditional joins as one that may have moved it.
                Event::LoopMaybeInit { .. } => {
                    previous_state = State::ResourceMoved { move_to: ResourceTy::Anonymous, move_at_line: *l, dropped_by_hand: false };
                    previous_line = *l;
                }
                _ => {
                    if line_start.0 != *l {
                        line_start = (*l, previous_state.clone());
                    }
                    if let Event::DeadLoanNote { shown, .. } = e {
                        *shown = self.dead_loan_note_shown(&previous_state, hash)
                            || (call_lent == Some(*l) && self.dead_loan_note_shown(&line_start.1, hash));
                    }
                    call_lent = call_lend_line(&last_lend, e, *l);
                    last_lend = match e { Event::MutableLend { id, .. } => Some((*l, *id)), _ => None };
                    // An end that leaves the reference column holding a
                    // reference (`ends_kept_alive`) doesn't end its line.
                    if !(kept[i] && holds_reference(&previous_state)) {
                        previous_state = self.calc_state(&previous_state, e, *l, hash);
                    }
                    // A `?` on a value the arm got with none or maybe one
                    // (`OutOfScope` here, drawn as "may have no value" up
                    // to the `?`, #1582): so is the rest of the arm, from
                    // the `?`'s dot, not from mid-air rows below it.
                    if matches!(e, Event::OwnerDropIfTryReturns { .. }) && matches!(previous_state, State::OutOfScope) {
                        previous_state = branch_state_converter(&previous_state);
                    }
                    previous_line = *l;
                }
            }
        }
        history.retain(|(_, e)| !matches!(e, Event::DeadLoanNote { shown: false, .. }));
        // `max`: an event below the branch's active range (possible
        // when branches share lines) mustn't make the range run upwards.
        let end = end.max(previous_line);
        states.push((previous_line, end, previous_state.clone()));

        // Render opaque line if branch does not end at merge point
        // (Not after the drop of a forked scrutinee temporary, #1360: its
        // leg ends there. A `let x;` local this branch doesn't assign
        // has no value yet in those rows.)
        let dropped_temp = matches!(previous_state, State::OutOfScope)
            && self.timelines[hash].resource_access_point.is_unbound_temporary();
        if end < branch_end && !dropped_temp {
            states.push((end, branch_end, branch_state_converter(&previous_state)));
        }

        // The rows between an event above the arm's body and the body are
        // still another arm's code: drawn dashed like the leading stretch,
        // in the state that event left (nothing, if it moved the value).
        if lead_end < begin {
            let inactive = |st: &State| match st {
                State::FullPrivilege { s: LineState::Uninit } => st.clone(),
                State::FullPrivilege { .. } | State::PartialPrivilege { .. } => branch_state_converter(st),
                _ => st.clone(),
            };
            *states = states.drain(..).flat_map(|(top, bot, st)| {
                if top > bot || top >= begin {
                    vec![(top, bot, st)]
                } else if bot <= begin {
                    vec![(top, bot, inactive(&st))]
                } else {
                    vec![(top, begin, inactive(&st)), (begin, bot, st)]
                }
            }).collect();
        }

        *states = clean_states(&states);

        states.last().unwrap().2.clone() // return the last state in the branch
    }


    fn compute_timeline_states(&self, history: & mut Vec<(usize, Event)>, states: & mut Vec<(usize, usize, State)>, hash: &u64) {
        let is_ref = self.is_ref(hash);
        if is_ref {
            old_borrow_ends_first(history);
        }
        let kept = ends_kept_alive(is_ref, history);
        let mut previous_line = 1;
        let mut previous_state = State::OutOfScope;
        let mut line_start: (usize, State) = (0, State::OutOfScope);
        let (mut last_lend, mut call_lent): (Option<(usize, usize)>, Option<usize>) = (None, None);
        for (i, (l, e)) in history.iter_mut().enumerate() {
            states.push((previous_line, *l, previous_state.clone()));
            match e {
                Event::Branch { branch_history, ty, split_point, merge_point, maybe_init, maybe_held, .. } => {
                    // Timeline is empty in the middle during a branch
                    states.push((*split_point, *merge_point, State::OutOfScope));
                    // State of each branch that reaches the merge; a
                    // diverging branch (#296) never gets there.
                    let mut reaching_states: Vec<State> = Vec::new();
                    // State of each diverging branch that only breaks
                    // out of a loop: the code after the loop is reached
                    // in it (#759).
                    let mut broke_states: Vec<State> = Vec::new();
                    for (i, branch) in branch_history.iter_mut().enumerate() {
                        let diverges = branch.diverges;
                        branch.entry_state = Some(previous_state.clone());
                        let end_state = self.compute_branch_states(
                            & mut branch.e_data,
                            & mut branch.states,
                            hash,
                            ty.get_start_end(i),
                            // Branch column ends one line above the
                            // join dot — the convergence diagonal
                            // bridges that last row — unless the
                            // branches share lines (#117).
                            branch_column_rows(*split_point, *merge_point),
                            previous_state.clone());
                        if !diverges {
                            reaching_states.push(end_state);
                        } else if breaks_out_only(&branch.exits) {
                            broke_states.push(end_state);
                        }
                    }
                    // A `let x;` local assigned in only some of the
                    // branches (#244) has no line after the join: it
                    // can't be used until it's assigned again (like a
                    // maybe-moved one), and it doesn't surely own
                    // anything.
                    let unassigned_on_some_path = matches!(init_join(*maybe_init, reaching_states.iter()), Some(InitJoin::Maybe { .. }));
                    // So has one that may have held nothing where the
                    // conditional started and that only some branches
                    // assign (#1034).
                    let unassigned_on_some_path = unassigned_on_some_path || *maybe_held;
                    // If every recorded branch diverges, only a path
                    // the user didn't write can get past the
                    // conditional (the implicit empty `else` of an
                    // `if`), and it leaves the state as it was.
                    let mut ending_states = if reaching_states.is_empty() {
                        vec![previous_state.clone()]
                    } else {
                        reaching_states
                    };

                    ending_states.sort(); // partial order of the ending states
                    previous_state = convert_back(&ending_states.first().unwrap());
                    if unassigned_on_some_path {
                        previous_state = State::OutOfScope;
                    }
                    // Every branch leaves the function (#515): nothing
                    // follows the conditional, and the column ends in
                    // the branches.
                    // Unless the branches `break` out of a loop: the code
                    // after the loop follows them, with what they left
                    // (#759: `a` was borrowed in an arm and gave its
                    // resource back there; it is still dropped).
                    if !branch_history.is_empty() && branch_history.iter().all(|b| b.diverges) {
                        if broke_states.is_empty() {
                            previous_state = State::OutOfScope;
                        } else {
                            broke_states.sort();
                            previous_state = convert_back(&broke_states[0]);
                        }
                    }
                    // Resume the parent timeline at `merge_point`,
                    // not `merge_point + 1`. The join dot sits at
                    // `merge_point` (see render_dot's Branch arm);
                    // resuming a row later leaves a visible gap
                    // between the join and the post-merge state.
                    previous_line = *merge_point;

                }
                _ => {
                    if line_start.0 != *l {
                        line_start = (*l, previous_state.clone());
                    }
                    if let Event::DeadLoanNote { shown, .. } = e {
                        *shown = self.dead_loan_note_shown(&previous_state, hash)
                            || (call_lent == Some(*l) && self.dead_loan_note_shown(&line_start.1, hash));
                    }
                    call_lent = call_lend_line(&last_lend, e, *l);
                    last_lend = match e { Event::MutableLend { id, .. } => Some((*l, *id)), _ => None };
                    // An end that leaves the reference column holding a
                    // reference (`ends_kept_alive`) doesn't end its line.
                    if !(kept[i] && holds_reference(&previous_state)) {
                        previous_state = self.calc_state(&previous_state, e, *l, hash);
                    }
                    previous_line = *l;
                }
            }
        }
        history.retain(|(_, e)| !matches!(e, Event::DeadLoanNote { shown: false, .. }));
        states.push((previous_line, previous_line, previous_state));
        *states = clean_states(&states);
    }


    fn compute_states(&mut self) {
        let mut timelines = self.timelines.clone(); // I am a lazy rust programmer
        for (hash, timeline) in timelines.iter_mut() {
            // Function RAPs are call-site labels keyed by name, not
            // owners: every `Cons(..)` / `id(..)` call shares one
            // timeline. Their states are never rendered (no column),
            // and running the owner state machine over them panics as
            // soon as the same name receives a second value — nested
            // constructors `Cons(1, Box::new(Cons(2, ..)))`, or simply
            // calling `id(s1)` then `id(s2)` (ResourceMoved -> Acquire
            // on a non-`mut` RAP).
            if matches!(timeline.resource_access_point, ResourceAccessPoint::Function(_)) {
                continue;
            }
            self.compute_timeline_states(& mut timeline.history, &mut timeline.states, hash);
        }
        lend_struct_borrows_to_members(&mut timelines);
        note_call_borrows_on_members(&mut timelines);
        self.timelines = timelines;
    }

    fn _append_event(&mut self, resource_access_point: &ResourceAccessPoint, event: Event, line_number: &usize) {
        let hash = &resource_access_point.hash();
        // if this event belongs to a new ResourceAccessPoint hash,
        // create a new Timeline first, thenResourceAccessPoint bind it to the corresponding hash.
        match self.timelines.get(hash) {
            None => {
                let timeline = Timeline {
                    resource_access_point: resource_access_point.clone(),
                    history: Vec::new(),
                    states: Vec::new()
                };
                self.timelines.insert(**hash, timeline);
            },
            _ => {}
        }

        // append the event to the end of the timeline of the corresponding hash
        match self.timelines.get_mut(hash) {
            Some(timeline) => {
                push_history_event(&mut timeline.history, *line_number, event);
            },
            _ => {
                panic!("internal state: timeline #{} vanished right after it was created", hash);
            }
        }
    }

    // Just a function to get the respective Events that should be appended to a given RAP's timeline given
    // an ExternalEvent and whether or not the RAP is the receiver of the action
    fn event_of_external_event(&self, line_num: usize, ext_ev: &ExternalEvent, to_o: bool) -> Vec<(usize, Event)> {
      match ext_ev {
        ExternalEvent::Bind { from, to, id, break_alt } => {
          if to_o { vec![(line_num,  Event::Acquire{from : from.to_owned(), is: to.clone(), via_try: None, id: *id, break_alt: break_alt.clone(), loop_head: LoopHead::No})] }
          else { vec![(line_num, Event::Duplicate{to : to.to_owned(), is: from.clone(), into_closure: false, of: CopyOf::Whole, id: *id, break_alt: break_alt.clone()})] }
        },
        ExternalEvent::Copy{from: from_ro, to: to_ro, id, into_closure, break_alt, is_partial, into_ctor, pointee, of_const, ..} => {
          let of = CopyOf::of(*is_partial, into_ctor, *pointee, of_const);
          if to_o { vec![(line_num,  Event::Copy{from : from_ro.to_owned(), is: to_ro.clone(), of, id: *id, break_alt: break_alt.clone()})] }
          else { vec![(line_num, Event::Duplicate{to : to_ro.to_owned(), is: from_ro.clone(), into_closure: *into_closure, of: source_copy_of(of), id: *id, break_alt: break_alt.clone()})] }
        },
        ExternalEvent::Move{from: from_ro, to: to_ro, id, into_closure, loop_head, break_alt, break_out, via_try, ..} => {
          if to_o { vec![(line_num,  Event::Acquire{from : from_ro.to_owned(), is: to_ro.clone(), via_try: *via_try, id: *id, break_alt: break_alt.clone(), loop_head: loop_head.clone()})] }
          // A destructuring `for` head borrowed by the loop: a value
          // inside it is copied (#1390).
          else if matches!(loop_head, LoopHead::Part { moved: false, .. }) { vec![(line_num, Event::Duplicate{to : to_ro.to_owned(), is: from_ro.clone(), into_closure: false, of: CopyOf::Inside, id: *id, break_alt: break_alt.clone()})] }
          else { vec![(line_num, Event::Move{to : to_ro.to_owned(), is: from_ro.clone(), into_closure: *into_closure, loop_head: loop_head.clone(), via_try: *via_try, id: *id, break_alt: break_alt.clone(), break_out: *break_out, empties_temp: self.temp_emptying_moves.contains(id)})] }
        },
        ExternalEvent::StaticBorrow{from: from_ro, to: to_ro, id, of_ref_var, ..} => {
          if to_o { vec![(line_num,  Event::StaticBorrow{from : from_ro.to_owned(), is: to_ro.clone(), id: *id, of_ref_var: *of_ref_var})] }
          else { vec![(line_num, Event::StaticLend{to : to_ro.to_owned(), is: from_ro.clone(), id: *id, of_ref_var: *of_ref_var})] }
        },
        ExternalEvent::StaticDie{from: from_ro, to: to_ro, id, still_borrowed_by, of_ref_var, ..} => {
          if to_o && !from_ro.is_same_underlying(&to_ro){ 
            vec![(line_num,  Event::StaticReacquire{from : from_ro.to_owned(), is: to_ro.clone(), id: *id, still_borrowed_by: still_borrowed_by.clone(), of_ref_var: *of_ref_var})] 
          }
          else { vec![(line_num, Event::StaticDie{to : to_ro.to_owned(), is: from_ro.clone(), id: *id})] }
        },
        ExternalEvent::MutableBorrow{from: from_ro, to: to_ro, id, of_ref_var, ..} => {
          if to_o { vec![(line_num,  Event::MutableBorrow{from : from_ro.to_owned(), is: to_ro.clone(), id: *id, of_ref_var: *of_ref_var})] }
          else { vec![(line_num, Event::MutableLend{to : to_ro.to_owned(), is: from_ro.clone(), id: *id, of_ref_var: *of_ref_var})] }
        },
        ExternalEvent::MutableDie{from: from_ro, to: to_ro, id, still_borrowed_by, of_ref_var, ..} => {
          if to_o && !from_ro.is_same_underlying(&to_ro){ 
            vec![(line_num,  Event::MutableReacquire{from : from_ro.to_owned(), is: to_ro.clone(), id: *id, still_borrowed_by: still_borrowed_by.clone(), of_ref_var: *of_ref_var})] 
          }
          else { vec![(line_num, Event::MutableDie{to : to_ro.to_owned(), is: from_ro.clone(), id: *id})] }    
        },
        ExternalEvent::RefDie { from: from_ro, to: to_ro, id ,..} => {
          match from_ro.clone() {
              ResourceTy::Deref(ro_is) | ResourceTy::Value(ro_is) => {
                  if (ro_is.is_mutref() || holds_mut_borrow(self, &ro_is)) && !to_o {
                    vec![(line_num, Event::MutableDie {to: ResourceTy::Anonymous, is: from_ro.clone(), id: *id})]
                  }
                  else {
                    if to_o { vec![] }
                    else { vec![(line_num, Event::StaticDie{to : to_ro.to_owned(), is: from_ro.clone(), id: *id})] }    
                  }
              }
              _ => panic!("internal state: a RefDie event whose source is not a reference")
          }
        },
        // A reference the call may store (`StoredIn`) is given back
        // later, by its own StaticDie / MutableDie.
        ExternalEvent::PassByStaticReference{from: from_ro, to: to_ro, id, stored_in, result_keeps_borrow, still_borrowed_by, of_ref_var, ..} => {
          if to_o { 
            vec![(line_num,  Event::StaticBorrow{from : from_ro.to_owned(), is: to_ro.clone(), id: *id, of_ref_var: *of_ref_var}),
            (line_num,  Event::StaticDie{to : from_ro.to_owned(), is: to_ro.clone(), id: *id})] 
          }
          else if stored_in.0.is_some() || *result_keeps_borrow {
            vec![(line_num, Event::StaticLend{to : to_ro.to_owned(), is: from_ro.clone(), id: *id, of_ref_var: *of_ref_var})]
          }
          else { 
            vec![(line_num, Event::StaticLend{to : to_ro.to_owned(), is: from_ro.clone(), id: *id, of_ref_var: *of_ref_var}),
            (line_num,  Event::StaticReacquire{from : to_ro.to_owned(), is: from_ro.clone(), id: *id, still_borrowed_by: still_borrowed_by.clone(), of_ref_var: *of_ref_var})]
          }
        },
        ExternalEvent::PassByMutableReference{from: from_ro, to: to_ro, id, stored_in, result_keeps_borrow, of_ref_var, ..} => {
          if to_o { 
            vec![(line_num,  Event::MutableBorrow{from : from_ro.to_owned(), is: to_ro.clone(), id: *id, of_ref_var: *of_ref_var}),
            (line_num,  Event::MutableDie{to : from_ro.to_owned(), is: to_ro.clone(), id: *id})]
          }
          else if stored_in.0.is_some() || *result_keeps_borrow {
            vec![(line_num, Event::MutableLend{to : to_ro.to_owned(), is: from_ro.clone(), id: *id, of_ref_var: *of_ref_var})]
          }
          else { 
            vec![(line_num, Event::MutableLend{to : to_ro.to_owned(), is: from_ro.clone(), id: *id, of_ref_var: *of_ref_var}),
            (line_num,  Event::MutableReacquire{from : to_ro.to_owned(), is: from_ro.clone(), id: *id, still_borrowed_by: Vec::new(), of_ref_var: *of_ref_var})]
          }
        },
        ExternalEvent::InitRefParam{param: ro, id, own_param, in_pattern, rest} => {
          vec![(line_num, Event::InitRefParam{param : ro.to_owned(), id: *id, own_param: own_param.clone(), in_pattern: in_pattern.clone(), rest: rest.clone()})]
        },
        ExternalEvent::GoOutOfScope{ro, holds, early, ..} => {
          match ro {
            ResourceAccessPoint::Owner(..) | ResourceAccessPoint::Struct(..) => {
              vec![(line_num, Event::OwnerGoOutOfScope { holds: *holds, early: early.clone() })]
            },
            ResourceAccessPoint::MutRef(..) | ResourceAccessPoint::StaticRef(..)=> {
              vec![(line_num, Event::RefGoOutOfScope { early: early.clone() })]
            },
            ResourceAccessPoint::Function(func) => {
              panic!(
                  "internal state: function \"{}\" went out of scope, but functions never do",
                  func.name
              );
            }
          }
        },
        ExternalEvent::OwnerDropAtReassign{ maybe_moved, by_cond, loop_head, parts, maybe_init, payload, contents_gone, .. } => {
          vec![(line_num, Event::OwnerDropAtReassign { maybe_moved: *maybe_moved, by_cond: *by_cond, loop_head: *loop_head, parts: parts.clone(), maybe_init: *maybe_init, payload: *payload, contents_gone: *contents_gone })]
        },
        ExternalEvent::ManuallyDropByHand{..} => {
          vec![(line_num, Event::ManuallyDropByHand)]
        },
        ExternalEvent::TemporaryDrop{ ro, ty, when, literal, early, part, .. } => {
          vec![(line_num, Event::TemporaryDrop { ty: ty.clone(), when: when.clone(), literal: *literal, early: *early, part: part.clone(), assign: ro.is_assign_literal_temporary() })]
        },
        ExternalEvent::LoopIteratorDrop{ left, early, .. } => {
          vec![(line_num, Event::LoopIteratorDrop { left: *left, early: early.clone() })]
        },
        ExternalEvent::LoopMaybeInit{ before_loop, nested, .. } => {
          vec![(line_num, Event::LoopMaybeInit { before_loop: *before_loop, nested: *nested })]
        },
        ExternalEvent::LoopExitKept{ reassigned, .. } => {
          vec![(line_num, Event::LoopExitKept { reassigned: *reassigned })]
        },
        ExternalEvent::LoopExitMove{definite, at, ..} => {
          vec![(line_num, Event::LoopExitMove { definite: *definite, at: at.clone() })]
        },
        ExternalEvent::OwnerDropIfTryReturns{maybe, held, ..} => {
          vec![(line_num, Event::OwnerDropIfTryReturns { maybe: *maybe, held: *held })]
        },
        ExternalEvent::OwnerDropUnlessReturned{..} => {
          vec![(line_num, Event::OwnerDropUnlessReturned)]
        },
        ExternalEvent::TupleTempDrop{ tuple, at, with, .. } => {
          vec![(line_num, Event::TupleTempDrop { tuple: tuple.clone(), at: *at, with: with.clone() })]
          },
        ExternalEvent::OrAltMove{ro, into, from, tuple, part_of, in_let, ..} => {
          vec![(line_num, Event::OrAltMove { into: into.clone(), sources: from.clone(), tuple: tuple.clone(), part_of: part_of.clone(), value: ro.holds_value(), in_let: *in_let })]
        },
        ExternalEvent::ChainFailAssign{ro, place, drops_old, path, ..} => {
          let rc = if place.is_some() { None } else { ro.rc_drop() };
          vec![(line_num, Event::ChainFailAssign { place: place.clone(), drops_old: *drops_old, rc, path: *path })]
        },
        ExternalEvent::DeadElseNote{ .. } => {
          vec![(line_num, Event::DeadElseNote)]
        },
        ExternalEvent::PassRestDrop{ parts, .. } => {
          vec![(line_num, Event::PassRestDrop { parts: parts.clone() })]
        },
        ExternalEvent::TiedParamNote{ lifetime, .. } => {
          vec![(line_num, Event::TiedParamNote { lifetime: lifetime.clone() })]
        },
        ExternalEvent::ReslicedInPlace{ lender, mutable, part, .. } => {
          vec![(line_num, Event::ReslicedInPlace { lender: lender.clone(), mutable: *mutable, part: *part })]
        },
        ExternalEvent::DeadLoanNote{ lender, borrower, repoint, .. } => {
          vec![(line_num, Event::DeadLoanNote { lender: lender.clone(), borrower: borrower.clone(), repoint: *repoint, shown: false })]
        },
        ExternalEvent::ChainFailUse{ro, uses, definite, path, ..} => {
          vec![(line_num, Event::ChainFailUse { uses: uses.clone(), definite: *definite, path: *path, is_ref: ro.is_ref(), pointee_value: ro.points_to_copy() })]
        },

        _ => panic!("internal state: a branch event reached the per-variable event expansion")
      }
    }

    // Invariant: 
    // Events in branches can only occur between two different types of owners: 
    // owners that are 'live' inside the conditional branch (ie defined outside it but used inside it)
    // and owners that are declared inside the branch itself. This means that we can append each 'side' (from/to) of the event 
    // seperately. 
    fn append_branch_event(&self, event: &ExternalEvent, line_number: usize, is: &ResourceTy, b_history: &mut Vec<(usize, Event)>) {
        let rap = is.extract_rap().unwrap();
        match event {
            ExternalEvent::Branch { live_vars, branches, branch_type, split_point, merge_point, maybe_init, loop_assigned, maybe_held, id } => {
                if live_vars.contains(rap) {
                    let mut new_branches: Vec<BranchData> = Vec::new();
                    for b in branches.iter() {
                        let mut new_b_history: Vec<(usize, Event)> = Vec::new();
                        for (l, e) in b.e_data.iter() {
                            self.append_branch_event(e, *l, is, & mut new_b_history);
                        }
                        new_branches.push(BranchData { t_data: TimelineColumnData { // produce some dummy timeline data for now
                            name: "".to_owned(),
                            x_val: -1,
                            title: "".to_owned(),
                            is_ref: false,
                            is_struct_group: false,
                            is_member: false, 
                            owner: 0
                          }, e_data: new_b_history, width: 0, states: Vec::new(), diverges: b.diverges, entry_state: None, exits: b.exits.clone() });
                    }

                    b_history.push((line_number,  
                        Event::Branch { is: is.clone(), 
                                        branch_history: new_branches, 
                                        ty: branch_type.clone(), 
                                        split_point: *split_point, 
                                        merge_point: *merge_point,
                                        maybe_init: maybe_init.contains(rap),
                                        loop_assigned: loop_assigned.contains(rap),
                                        maybe_held: maybe_held.contains(rap),
                                        id: *id}));
                }
                // rap has no column in this conditional, but the end of
                // a borrow of it may have moved into one of its arms,
                // the one declaring the reference (`nest_arm_ref_ends`,
                // #1277): `if let Some(_x) = v.first() { .. }` nested in
                // an arm where v is live. It is rap's "no longer
                // borrowed" on its column here, as for a conditional at
                // the top level (`append_processed_external_event`);
                // without it the end's arrow had no dot on rap (#1418).
                else {
                    fn ends_of(branches: &[ExtBranchData], rap: &ResourceAccessPoint, out: &mut Vec<(usize, ExternalEvent)>) {
                        // (Not in an arm that declares rap, nor in the
                        // conditionals nested there: rap's own column in
                        // that arm draws them, `append_decl_branch_events`.
                        // Collected here too, they were drawn a second
                        // time, ahead of rap's declaration, #1522.)
                        for b in branches.iter().filter(|b| !b.decl_vars.contains(rap)) {
                            for (l, e) in &b.e_data {
                                match e {
                                    ExternalEvent::Branch { branches, .. } => ends_of(branches, rap, out),
                                    ExternalEvent::StaticDie { to: ResourceTy::Value(lender), .. }
                                    | ExternalEvent::MutableDie { to: ResourceTy::Value(lender), .. }
                                        if lender == rap && !b.decl_vars.contains(rap) => out.push((*l, e.clone())),
                                    _ => {}
                                }
                            }
                        }
                    }
                    let mut ends = Vec::new();
                    ends_of(branches, rap, &mut ends);
                    for (l, e) in ends {
                        b_history.extend(self.event_of_external_event(l, &e, true));
                    }
                }
            }
            ExternalEvent::LoopExitMove { ro, .. } | ExternalEvent::TupleTempDrop { ro, .. } | ExternalEvent::OrAltMove { ro, .. } | ExternalEvent::LoopIteratorDrop { ro, .. }
            | ExternalEvent::LoopMaybeInit { ro, .. } | ExternalEvent::LoopExitKept { ro, .. }
            | ExternalEvent::ChainFailAssign { ro, .. } | ExternalEvent::ChainFailUse { ro, .. } | ExternalEvent::TiedParamNote { ro, .. }
            | ExternalEvent::ReslicedInPlace { ro, .. }
            | ExternalEvent::DeadElseNote { ro, .. } | ExternalEvent::DeadLoanNote { ro, .. }
            | ExternalEvent::PassRestDrop { ro, .. } => {
                if ro == rap {
                    b_history.extend(self.event_of_external_event(line_number, event, true));
                }
            }
            ExternalEvent::OrAltBorrow { ro, into, .. } => {
                let (lend, borrow) = event.or_alt_borrow_events();
                if ro == rap {
                    b_history.push((line_number, lend));
                }
                if into == rap {
                    b_history.extend(borrow.map(|e| (line_number, e)));
                }
            }
            ExternalEvent::ClosureCall { closure, fates, id } => {
                if closure.extract_rap() == Some(rap) {
                    b_history.push((line_number, Event::ClosureCall { is: closure.clone(), fates: fates.clone(), id: *id }));
                }
            }
            _ => {
                // `*rap` and `rap.f` (rap a reference) are drawn on
                // rap's own column too.
                let through_rap = |r: &ResourceTy| r.deref_rap() == Some(rap);
                let (from, to) = ResourceAccessPoint_extract(event);
                // An element write (`v[i] = x`) lands on v's column.
                // (A borrow stored in an element, `v[0] = &u`, stays
                // off v's column: #282.)
                let to_elem = matches!(to, ResourceTy::Index(r, _, _) if r == rap)
                    && !matches!(event, ExternalEvent::StaticBorrow { .. } | ExternalEvent::MutableBorrow { .. });
                // ... and a read through an element (`*slots[0]`) leaves it.
                let from_elem = matches!(from, ResourceTy::Index(r, _, _) if r == rap);
                if *from == *is || through_rap(from) || from_elem {
                    b_history.extend(self.event_of_external_event(line_number, event, false));
                    // `p.a` in `p = Pair { a: p.a, .. }` (#1508): moved out
                    // by the right-hand side and back with the new value,
                    // as outside a branch (#1411).
                    if *to == *is && matches!(event, ExternalEvent::Move { .. }) {
                        for (l, e) in self.event_of_external_event(line_number, event, true) {
                            push_history_event(b_history, l, e);
                        }
                    }
                }
                else if *to == *is || through_rap(to) || to_elem {
                    for (l, e) in self.event_of_external_event(line_number, event, true) {
                        push_history_event(b_history, l, e);
                    }
                }
                else if let Some(r) = event.is_gos_ev() {
                    if *r.name() == is.real_name() {
                        b_history.extend(self.event_of_external_event(line_number, event, true));
                    }
                }
                // A reference field of a struct bound by a pattern
                // (`if let Some(x) = o`, `x.part`) is initialized in
                // the branch (#352); `ResourceAccessPoint_extract`
                // doesn't name it.
                else if let ExternalEvent::InitRefParam { param, .. } = event {
                    if param == rap {
                        b_history.extend(self.event_of_external_event(line_number, event, true));
                    }
                }
                // `x = …` / `*r = …` inside a branch drops the value the
                // variable (or r's lender) held before. The event names
                // that variable in `ro`, not in from/to (#267).
                else if let ExternalEvent::OwnerDropAtReassign { ro, .. } = event {
                    if ro.hash() == rap.hash() {
                        b_history.extend(self.event_of_external_event(line_number, event, true));
                    }
                }
                // Likewise the drop a `?` in the branch causes if it
                // returns early (#430, #636).
                else if let ExternalEvent::OwnerDropIfTryReturns { ro, .. } | ExternalEvent::OwnerDropUnlessReturned { ro, .. } = event {
                    if ro.hash() == rap.hash() {
                        b_history.extend(self.event_of_external_event(line_number, event, true));
                    }
                }
            }
        }
    }

    // need to recurse down the tree and add events that belong to variables declared in subtrees
    fn append_decl_branch_events(&mut self, b_history: &Vec<(usize, ExternalEvent)>) {
        for (_l, e) in b_history {   
            match e {
                ExternalEvent::Branch { branches, .. } => {
                    for branch in branches.iter() {
                        // visit node
                        for var in in_decl_order(&branch.decl_vars) {
                            let mut b_history: Vec<(usize, Event)> = Vec::new();
                            let is = &ResourceTy::Value(var.clone());
                            for (l, e) in branch.e_data.iter() {
                                self.append_branch_event(e, *l, is, & mut b_history);
                                append_decl_ref_die(self, e, *l, var, &mut b_history);
                            }
                            for (l, e) in b_history {
                                self._append_event(var, e, &l);
                            }
                        }
                        // then recurse
                        self.append_decl_branch_events(&branch.e_data);
                    }
                }
                _ => {}
            }
        }
    }


    // This function does 2 things: 
    // 1. It appends an external event at a (possibly different) line number. Due to line shifting
    // 2. It takes that 1 external event and appends the respective Events to the resource owners involved,
    //    Essentially splitting the external event. For example say we have the ExternalEvent:
    //    Move {From: x, to: y, ..} at line 14
    //    Then we should append Event::Aquire {from: x} to y's history
    //    and Event::Move{to: y} to x's history
    // Branches are slightly more complicated, and we need to recurse down the tree to append all the events 
    // inside of them.
    fn append_processed_external_event(&mut self, event: ExternalEvent, line_number: usize) {
        // The plugin's own record that a cycle's destination owns again
        // (#1343): its reordered move already ends with the acquire.
        if let ExternalEvent::Bind { id, .. } = &event {
            if self.moves_out_first.contains(id) {
                return;
            }
        }
        self.external_events.push((line_number, event.clone()));        
        // append_event to timelines unless interacting with an anonymous owner
        // Eventually we should collect these interactions with anonymous owners and display them in the code panel
        fn maybe_append_event (vd: & mut VisualizationData, resource_ty: &ResourceTy, event: Event, line_number: usize){
          match resource_ty {
            ResourceTy::Anonymous | ResourceTy::Caller | ResourceTy::Temporary { .. } => {}
            ResourceTy::Deref(r) | ResourceTy::Value(r) | ResourceTy::Index(r, _, _) | ResourceTy::DerefField(r, _, _, _) => {
                vd._append_event(&r, event, &line_number);
            }
          }
        }

        match event {
            // eg let ro_to = String::from("");
            ExternalEvent::Move{from: from_ro, to: to_ro, id, into_closure, loop_head, break_alt, break_out, via_try, ..} => {
                maybe_append_event(self, &to_ro.clone(), Event::Acquire{from : from_ro.to_owned(), is: to_ro.clone(), via_try, id: id, break_alt: break_alt.clone(), loop_head: loop_head.clone()}, line_number);
                let out_first = self.moves_out_first.contains(&id);
                let source = from_ro.extract_rap().map(|r| *r.hash());
                // A destructuring `for` head borrowed by the loop: a value
                // inside it is copied (#1390).
                if matches!(loop_head, LoopHead::Part { moved: false, .. }) {
                    maybe_append_event(self, &from_ro.clone(), Event::Duplicate{to : to_ro.to_owned(), is: from_ro, into_closure: false, of: CopyOf::Inside, id: id, break_alt}, line_number);
                } else {
                    maybe_append_event(self, &from_ro.clone(), Event::Move{to : to_ro.to_owned(), is: from_ro, into_closure, loop_head, via_try, id: id, break_alt, break_out, empties_temp: self.temp_emptying_moves.contains(&id)}, line_number);
                }
                // A destructuring assignment's cycle (#1343): the source's
                // move goes before the new value it got on this line.
                if let (true, Some(h)) = (out_first, source) {
                    if let Some(t) = self.timelines.get_mut(&h) {
                        let n = t.history.len();
                        let first_new = t.history.iter().rposition(|(l, e)| *l == line_number
                            && matches!(e, Event::Acquire { .. } | Event::Copy { .. }));
                        if let Some(i) = first_new.filter(|&i| i + 1 < n) {
                            let mv = t.history.remove(n - 1);
                            t.history.insert(i, mv);
                        }
                    }
                }
            },
            // eg: let ro_to = 5;
            ExternalEvent::Bind{from: from_ro, to: to_ro, id, break_alt} => {
                maybe_append_event(self, &to_ro.clone(), Event::Acquire{from : from_ro.to_owned(), is: to_ro.clone(), via_try: None, id: id, break_alt: break_alt.clone(), loop_head: LoopHead::No}, line_number);
                maybe_append_event(self, &from_ro.clone(), Event::Duplicate{to : to_ro.to_owned(), is: from_ro, into_closure: false, of: CopyOf::Whole, id: id, break_alt}, line_number);
            },
            // eg: let x : i64 = y as i64;
            ExternalEvent::Copy{from: from_ro, to: to_ro, id, into_closure, break_alt, is_partial, into_ctor, pointee, of_const, ..} => {
                let of = CopyOf::of(is_partial, &into_ctor, pointee, &of_const);
                let source_of = source_copy_of(of.clone());
                maybe_append_event(self, &to_ro.clone(), Event::Copy{from : from_ro.to_owned(), is: to_ro.clone(), of, id: id, break_alt: break_alt.clone()}, line_number);
                maybe_append_event(self, &from_ro.clone(), Event::Duplicate{to : to_ro.to_owned(), is: from_ro, into_closure, of: source_of, id: id, break_alt}, line_number);
            },
            ExternalEvent::Branch { live_vars, branches, branch_type, split_point, merge_point, maybe_init, loop_assigned, maybe_held, id } => {
                // need to add events with more granularity for Branches
                for var in in_decl_order(&live_vars) { // for all the live variables in the branch (declaration order, #211)
                    let is = ResourceTy::Value(var.clone());
                    let mut branch_history: Vec<BranchData> = Vec::new();
                    for (i, branch) in branches.iter().enumerate() { // for each branch
                        let mut b_history: Vec<(usize, Event)> = Vec::new();
                        let (lo, hi) = branch_type.get_start_end(i);
                        for (l, ev) in branch.e_data.iter() { // for each event in each branch
                            self.append_branch_event(ev, *l, &is, & mut b_history);
                            // The end with no arrow of a reference declared
                            // before the conditional, on a line of this arm
                            // (its re-pointing's new loan, never read while
                            // another reference holds the lender, #1524):
                            // drawn as for one the arm declares. (Top-level
                            // conditionals only, not a loop's body; the plugin
                            // leaves out one whose reference is read after the
                            // conditional, `drop_arm_ref_ends_live_after`.)
                            if lo <= *l && *l <= hi && !matches!(branch_type, BranchType::Loop(..)) {
                                append_decl_ref_die(self, ev, *l, var, &mut b_history);
                            }
                        }

                        branch_history.push(BranchData { t_data: TimelineColumnData { // produce some dummy timeline data for now
                            name: "".to_owned(),
                            x_val: -1,
                            title: "".to_owned(),
                            is_ref: false,
                            is_struct_group: false,
                            is_member: false, 
                            owner: 0
                        }, e_data: b_history, width: 0, states: Vec::new(), diverges: branch.diverges, entry_state: None, exits: branch.exits.clone() });
                    }
                    maybe_append_event(self, &is.clone(), Event::Branch { 
                                        is: is, 
                                        branch_history: branch_history, 
                                        ty: branch_type.clone(), 
                                        split_point: split_point, 
                                        merge_point: merge_point, 
                                        maybe_init: maybe_init.contains(var),
                                        loop_assigned: loop_assigned.contains(var),
                                        maybe_held: maybe_held.contains(var),
                                        id: id}, line_number);
                }

                // have to append events for variables declared inside the block
                for branch in branches.iter() {
                    for var in in_decl_order(&branch.decl_vars) {
                        let mut b_history: Vec<(usize, Event)> = Vec::new();
                        let is = &ResourceTy::Value(var.clone());
                        for (l, e) in branch.e_data.iter() {
                            self.append_branch_event(e, *l, is, & mut b_history);
                            append_decl_ref_die(self, e, *l, var, &mut b_history);
                        }
                        for (l, e) in b_history {
                            self._append_event(var, e, &l);
                        }
                    }
                    self.append_decl_branch_events(&branch.e_data);
                }

                // A borrow end the renderer moved into the arm declaring
                // its reference (`nest_arm_ref_ends`, #1277) whose lender
                // has no column in the conditional: the lender gets its
                // "no longer borrowed" on its own column, at the end's row.
                let mut decls: Vec<ResourceAccessPoint> = Vec::new();
                fn all_decls(branches: &[ExtBranchData], out: &mut Vec<ResourceAccessPoint>) {
                    for b in branches {
                        out.extend(in_decl_order(&b.decl_vars).into_iter().cloned());
                        for (_, e) in &b.e_data {
                            if let ExternalEvent::Branch { branches, .. } = e { all_decls(branches, out); }
                        }
                    }
                }
                all_decls(&branches, &mut decls);
                fn outside_ends(branches: &[ExtBranchData], skip: &dyn Fn(&ResourceAccessPoint) -> bool, out: &mut Vec<(usize, ExternalEvent)>) {
                    for b in branches {
                        for (l, e) in &b.e_data {
                            match e {
                                ExternalEvent::Branch { branches, .. } => outside_ends(branches, skip, out),
                                ExternalEvent::StaticDie { to: ResourceTy::Value(lender), .. }
                                | ExternalEvent::MutableDie { to: ResourceTy::Value(lender), .. }
                                    if !lender.is_fn() && !skip(lender) => out.push((*l, e.clone())),
                                _ => {}
                            }
                        }
                    }
                }
                let mut ends = Vec::new();
                outside_ends(&branches, &|r| live_vars.contains(r) || decls.contains(r), &mut ends);
                for (l, e) in ends {
                    let (ExternalEvent::StaticDie { to: ResourceTy::Value(lender), .. }
                        | ExternalEvent::MutableDie { to: ResourceTy::Value(lender), .. }) = &e else { continue };
                    for (ln, ev) in self.event_of_external_event(l, &e, true) {
                        self._append_event(lender, ev, &ln);
                    }
                }
            }
            // A borrow stored in an element (`v[0] = &u`) has the
            // element as borrower: only the lender's column shows it
            // (#282), so the `Index` side gets no event.
            ExternalEvent::StaticBorrow{from: from_ro, to: to_ro, id, of_ref_var, ..} => {
                maybe_append_event(self, &from_ro, Event::StaticLend{to : to_ro.to_owned(), is: from_ro.clone(), id: id, of_ref_var}, line_number);
                if !matches!(to_ro, ResourceTy::Index(..)) {
                    maybe_append_event(self, &to_ro.clone(), Event::StaticBorrow{from : from_ro.to_owned(), is: to_ro, id: id, of_ref_var}, line_number);
                }

            },
            ExternalEvent::StaticDie{from: from_ro, to: to_ro, id, still_borrowed_by, of_ref_var, ..} => {
              // this catches when StaticDie(s to s*) to avoid duplicate events
                if !from_ro.is_same_underlying(&to_ro) {
                  maybe_append_event(self, &to_ro.clone(), Event::StaticReacquire{from : from_ro.to_owned(), is: to_ro.clone(), id: id, still_borrowed_by, of_ref_var}, line_number);
                }
                if !matches!(from_ro, ResourceTy::Index(..)) {
                    maybe_append_event(self, &from_ro.clone(), Event::StaticDie{to : to_ro.to_owned(), is: from_ro, id: id}, line_number);
                }
            },
            ExternalEvent::MutableBorrow{from: from_ro, to: to_ro, id, of_ref_var, ..} => {
                maybe_append_event(self, &from_ro, Event::MutableLend{to : to_ro.to_owned(), is: from_ro.clone(), id: id, of_ref_var}, line_number);
                if !matches!(to_ro, ResourceTy::Index(..)) {
                    maybe_append_event(self, &to_ro.clone(), Event::MutableBorrow{from : from_ro.to_owned(), is: to_ro, id: id, of_ref_var}, line_number);
                }

            },
            ExternalEvent::MutableDie{from: from_ro, to: to_ro, id, still_borrowed_by, of_ref_var, ..} => {
                // this catches when MutableDie(s to s*) to avoid duplicate events
                if !from_ro.is_same_underlying(&to_ro) {
                  maybe_append_event(self, &to_ro.clone(), Event::MutableReacquire{from : from_ro.to_owned(), is: to_ro.clone(), id: id, still_borrowed_by, of_ref_var}, line_number);
                }
                if !matches!(from_ro, ResourceTy::Index(..)) {
                    maybe_append_event(self, &from_ro.clone(), Event::MutableDie{to : to_ro.to_owned(), is: from_ro, id: id}, line_number);
                }
            },
            // don't need to append an event to the to resource since a RefDie doesn't change the state of the to ro
            ExternalEvent::RefDie { from: from_ro, to: _, id, ..} => { // need Ref Event to avoid drawing redundant arrows when rendering timelines
                match from_ro.clone() {
                    ResourceTy::Deref(ro_is) | ResourceTy::Value(ro_is) => {
                        if ro_is.is_mutref() || holds_mut_borrow(self, &ro_is) {
                            maybe_append_event(self, &from_ro.clone(), Event::MutableDie { to: ResourceTy::Anonymous, is: from_ro.clone(), id: id }, line_number);
                            //maybe_append_event(self, &to_ro.clone(), Event::MutableReacquire { from: from_ro, is: to_ro }, &line_number);
                        }
                        else {
                            maybe_append_event(self, &from_ro.clone(), Event::StaticDie { to: ResourceTy::Anonymous, is: from_ro.clone(), id: id }, line_number);
                            //maybe_append_event(self, &to_ro.clone(), Event::RefDie { from: from_ro, is: to_ro, num_curr_borrowers, id: id}, line_number);
                        }
                    }
                    _ => panic!("internal state: a RefDie event whose source is not a reference")
                }
            },

            // Technically appending individual events for PassByRef events is not necessary since
            // they do not change the state of either variable - however - it's useful to have when 
            // With `result_keeps_borrow` the lender isn't given back its
            // resource here either: the call's result still borrows it,
            // and the end of that borrow gives it back (#171).
            // A reference the call may store (`StoredIn`, #513) is given
            // back to `from` later, by its own StaticDie / MutableDie.
            ExternalEvent::PassByStaticReference{from: from_ro, to: to_ro, id, stored_in, result_keeps_borrow, still_borrowed_by, of_ref_var, ..} => {
                maybe_append_event(self, &from_ro.to_owned(), Event::StaticLend{to : to_ro.to_owned(), is: from_ro.clone(), id: id, of_ref_var}, line_number);
                maybe_append_event(self, &to_ro.to_owned(), Event::StaticBorrow{from : from_ro.to_owned(), is: to_ro.clone(), id: id, of_ref_var}, line_number);
                if stored_in.0.is_none() && !result_keeps_borrow {
                    maybe_append_event(self, &from_ro, Event::StaticReacquire{from : to_ro.to_owned(), is: from_ro.clone(), id: id, still_borrowed_by, of_ref_var}, line_number);
                }
                maybe_append_event(self, &to_ro.clone(), Event::StaticDie{to : from_ro.to_owned(), is: to_ro, id: id}, line_number);
            },
            ExternalEvent::PassByMutableReference{from: from_ro, to: to_ro, id, stored_in, result_keeps_borrow, of_ref_var, ..} => {
                maybe_append_event(self, &from_ro.clone(), Event::MutableLend{to : to_ro.to_owned(), is: from_ro.clone(), id: id, of_ref_var}, line_number);
                maybe_append_event(self, &to_ro.clone(), Event::MutableBorrow{from : from_ro.to_owned(), is: to_ro.clone(), id: id, of_ref_var}, line_number);
                if stored_in.0.is_none() && !result_keeps_borrow {
                    maybe_append_event(self, &from_ro.clone(), Event::MutableReacquire{from : to_ro.to_owned(), is: from_ro.clone(), id: id, still_borrowed_by: Vec::new(), of_ref_var}, line_number);
                }
                maybe_append_event(self, &to_ro.clone(), Event::MutableDie{to : from_ro.to_owned(), is: to_ro, id: id}, line_number);
            },
            ExternalEvent::ClosureCall { closure, fates, id } => {
                maybe_append_event(self, &closure, Event::ClosureCall { is: closure.clone(), fates, id }, line_number);
            },
            ExternalEvent::InitRefParam{param: ro, id, own_param, in_pattern, rest} => {
                maybe_append_event(self, &ResourceTy::Value(ro.clone()), Event::InitRefParam{param : ro.to_owned(), id: id, own_param, in_pattern, rest}, line_number);
            },
            ExternalEvent::GoOutOfScope{ro, holds, early, ..} => {
                match ro {
                    ResourceAccessPoint::Owner(..) | ResourceAccessPoint::Struct(..) => {
                        maybe_append_event(self, &ResourceTy::Value(ro), Event::OwnerGoOutOfScope { holds, early }, line_number);
                    },
                    ResourceAccessPoint::MutRef(..) | ResourceAccessPoint::StaticRef(..)=> {
                        maybe_append_event(self, &ResourceTy::Value(ro), Event::RefGoOutOfScope { early }, line_number);
                    },
                    ResourceAccessPoint::Function(func) => {
                        panic!(
                            "internal state: function \"{}\" went out of scope, but functions never do",
                            func.name
                        );
                    }
                }
            },
            ExternalEvent::OwnerDropAtReassign{ro, maybe_moved, by_cond, loop_head, parts, maybe_init, payload, contents_gone, ..} => {
                maybe_append_event(self, &ResourceTy::Value(ro), Event::OwnerDropAtReassign { maybe_moved, by_cond, loop_head, parts, maybe_init, payload, contents_gone }, line_number);
            },
            ExternalEvent::ManuallyDropByHand{ro, ..} => {
                maybe_append_event(self, &ResourceTy::Value(ro), Event::ManuallyDropByHand, line_number);
            },
            ExternalEvent::TemporaryDrop{ro, ty, when, literal, early, part, ..} => {
                let assign = ro.is_assign_literal_temporary();
                maybe_append_event(self, &ResourceTy::Value(ro), Event::TemporaryDrop { ty, when, literal, early, part, assign }, line_number);
            },
            ExternalEvent::LoopIteratorDrop{ro, left, early, ..} => {
                maybe_append_event(self, &ResourceTy::Value(ro), Event::LoopIteratorDrop { left, early }, line_number);
            },
            ExternalEvent::LoopMaybeInit{ro, before_loop, nested, ..} => {
                maybe_append_event(self, &ResourceTy::Value(ro), Event::LoopMaybeInit { before_loop, nested }, line_number);
            },
            ExternalEvent::LoopExitKept{ro, reassigned, ..} => {
                maybe_append_event(self, &ResourceTy::Value(ro), Event::LoopExitKept { reassigned }, line_number);
            },
            ExternalEvent::LoopExitMove{ro, definite, at, ..} => {
                maybe_append_event(self, &ResourceTy::Value(ro), Event::LoopExitMove { definite, at }, line_number);
            },
            ExternalEvent::OwnerDropIfTryReturns{ro, maybe, held, ..} => {
                maybe_append_event(self, &ResourceTy::Value(ro), Event::OwnerDropIfTryReturns { maybe, held }, line_number);
            },
            ExternalEvent::OwnerDropUnlessReturned{ro, ..} => {
                maybe_append_event(self, &ResourceTy::Value(ro), Event::OwnerDropUnlessReturned, line_number);
            },
            ExternalEvent::TupleTempDrop{ro, tuple, at, with, ..} => {
                maybe_append_event(self, &ResourceTy::Value(ro), Event::TupleTempDrop { tuple, at, with }, line_number);
            },
            ExternalEvent::OrAltMove{ro, into, from, tuple, part_of, in_let, ..} => {
                let value = ro.holds_value();
                maybe_append_event(self, &ResourceTy::Value(ro), Event::OrAltMove { into, sources: from, tuple, part_of, value, in_let }, line_number);
            },
            ExternalEvent::OrAltBorrow{ref ro, ref into, ..} => {
                let (lend, borrow) = event.or_alt_borrow_events();
                maybe_append_event(self, &ResourceTy::Value(ro.clone()), lend, line_number);
                if let Some(b) = borrow {
                    maybe_append_event(self, &ResourceTy::Value(into.clone()), b, line_number);
                }
            },
            ExternalEvent::ChainFailAssign{ro, place, drops_old, path, ..} => {
                let rc = if place.is_some() { None } else { ro.rc_drop() };
                maybe_append_event(self, &ResourceTy::Value(ro), Event::ChainFailAssign { place, drops_old, rc, path }, line_number);
            },
            ExternalEvent::TiedParamNote{ro, lifetime, ..} => {
                maybe_append_event(self, &ResourceTy::Value(ro), Event::TiedParamNote { lifetime }, line_number);
            },
            ExternalEvent::ReslicedInPlace{ro, lender, mutable, part, ..} => {
                maybe_append_event(self, &ResourceTy::Value(ro), Event::ReslicedInPlace { lender, mutable, part }, line_number);
            },
            ExternalEvent::DeadElseNote{ro, ..} => {
                maybe_append_event(self, &ResourceTy::Value(ro), Event::DeadElseNote, line_number);
            },
            ExternalEvent::PassRestDrop{ro, parts, ..} => {
                maybe_append_event(self, &ResourceTy::Value(ro), Event::PassRestDrop { parts }, line_number);
            },
            ExternalEvent::DeadLoanNote{ro, lender, borrower, repoint, ..} => {
                maybe_append_event(self, &ResourceTy::Value(ro), Event::DeadLoanNote { lender, borrower, repoint, shown: false }, line_number);
            },
            ExternalEvent::ChainFailUse{ro, uses, definite, path, ..} => {
                let (is_ref, pointee_value) = (ro.is_ref(), ro.points_to_copy());
                maybe_append_event(self, &ResourceTy::Value(ro), Event::ChainFailUse { uses, definite, path, is_ref, pointee_value }, line_number);
            },
        }
    }
}
/// A value that holds a borrow without being a reference (`let it =
/// r.iter_mut();`) whose borrow is a mutable one: its column drew a
/// mutable borrow. Its end then says "mutable" too (#1152).
///
/// So does a shared reference whose value came last from a mutable
/// borrow (`let r2 = id(r);` with `r = downgrade(&mut s)`, #1510): its
/// column drew "made from a mutable borrow", and the end that gives
/// nothing back (another reference still holds the loan) ended it as an
/// immutable one.
fn holds_mut_borrow(vd: &VisualizationData, ro: &ResourceAccessPoint) -> bool {
    holds_mut_borrow_by(vd, ro, usize::MAX, 4)
}

/// `holds_mut_borrow` for `ro`'s value as of line `by`. A copy of another
/// reference (`h.r = r4;`, `let Holder { r: r4, .. } = h;` with h.r from
/// `holder(downgrade(&mut s4))`) holds that reference's loan, so its kind
/// is the source's as of the copy (the checker's rule, #1521); `depth`
/// bounds the walk along copies.
fn holds_mut_borrow_by(vd: &VisualizationData, ro: &ResourceAccessPoint, by: usize, depth: usize) -> bool {
    let Some(t) = vd.timelines.get(ro.hash()) else { return false };
    history_holds_mut_borrow(vd, &t.history, ro, by, depth)
}

/// `holds_mut_borrow_by` with `ro`'s events so far in `history`: a
/// reference declared in an arm has its events collected there before
/// they reach its column (`append_decl_branch_events`), so its end,
/// collected with them, can't look at the column (#1571).
fn history_holds_mut_borrow(vd: &VisualizationData, history: &[(usize, Event)], ro: &ResourceAccessPoint, by: usize, depth: usize) -> bool {
    if ro.is_ref() {
        // (A copy of another reference re-points it; a call's result
        // copied in, `n` in `if let Some(n) = Some(id(r))`, comes with the
        // borrow drawn into it.)
        let from_column = |f: &ResourceTy| f.extract_rap().is_some_and(|r| !r.is_fn());
        return history.iter().rev()
            .filter(|(l, _)| *l <= by)
            .find(|(_, e)| match e {
                Event::MutableBorrow { .. } | Event::StaticBorrow { .. } | Event::Acquire { .. } => true,
                Event::Copy { from, .. } => from_column(from),
                _ => false,
            })
            .is_some_and(|(l, e)| match e {
                Event::MutableBorrow { .. } => true,
                Event::Copy { from, .. } => depth > 0 && from.extract_rap()
                    .is_some_and(|src| src.hash() != ro.hash() && holds_mut_borrow_by(vd, src, *l, depth - 1)),
                _ => false,
            });
    }
    ro.holds_borrow() && history.iter().any(|(_, e)| matches!(e, Event::MutableBorrow { .. }))
}
