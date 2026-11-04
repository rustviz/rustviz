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
    fn event_of_exteranl_event(&self, line_num: usize, ext_ev: &ExternalEvent, to_o: bool) -> Vec<(usize, Event)>;

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

// when something is not a reference
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
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
    /// True iff this owner's type can be mutated through a shared
    /// reference (`Cell`, `RefCell`, `Mutex`, `Rc<RefCell<T>>`, ...);
    /// see `ExprVisitor::ty_has_interior_mut`. An immutable borrow of
    /// such an owner doesn't make it read-only.
    pub interior_mut: bool,
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
    /// True iff this owner is an unbound temporary (see
    /// `ResourceAccessPoint::is_unbound_temporary`).
    pub unbound_temp: bool,
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
    pub interior_mut: bool,
    /// Same as Owner::is_manually_drop, for a `ManuallyDrop<T>` field.
    pub is_manually_drop: bool,
    /// `Some` iff this member's own type is `Rc<T>`, `Arc<T>` or one of
    /// their `Weak<T>`s (same role as `Owner::rc`).
    pub rc: Option<RcKind>,
    /// `Some(k)` iff this is a struct whose every field that needs drop
    /// is a `k`, and the struct has no `Drop` impl of its own: dropping
    /// it only lowers counts (#373).
    pub rc_fields: Option<RcKind>,
    /// Same as `Owner::rc_inside`.
    pub rc_inside: Option<RcDrop>,
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
    DerefField(ResourceAccessPoint, String, PlaceWrite),
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
    Temporary { extended: bool },
}

impl ResourceTy {
    pub fn name(&self) -> String {
        match self {
            ResourceTy::Anonymous | ResourceTy::Temporary { .. } => "Anonymous resource".to_owned(),
            ResourceTy::Value(r) => r.name().to_owned(),
            ResourceTy::Deref(r) => format!("*{}", r.name()),
            ResourceTy::Index(_, place, _) => place.clone(),
            ResourceTy::DerefField(_, place, _) => place.clone(),
            ResourceTy::Caller => "Caller".to_owned()
        }
    }

    pub fn real_name(&self) -> String {
        match self {
            ResourceTy::Anonymous | ResourceTy::Temporary { .. } => "Anonymous resource".to_owned(),
            ResourceTy::Value(r) | ResourceTy::Deref(r) | ResourceTy::Index(r, _, _) | ResourceTy::DerefField(r, _, _) => r.name().to_owned(),
            ResourceTy::Caller => "Caller".to_owned()
        }
    }

    pub fn hash(&self) -> &u64 {
        match self {
            ResourceTy::Caller | ResourceTy::Anonymous | ResourceTy::Temporary { .. } => &std::u64::MAX,
            ResourceTy::Value(r) | ResourceTy::Deref(r) | ResourceTy::Index(r, _, _) | ResourceTy::DerefField(r, _, _) => r.hash(),
        }
    }

    pub fn is_ref(&self) -> bool {
      match self {
        ResourceTy::Value(r) | ResourceTy::Deref(r) | ResourceTy::Index(r, _, _) | ResourceTy::DerefField(r, _, _) => r.is_ref(),
        _ => false
      }
    }

    pub fn is_mutref(&self) -> bool {
        match self {
            ResourceTy::Value(r) | ResourceTy::Deref(r) | ResourceTy::Index(r, _, _) | ResourceTy::DerefField(r, _, _) => r.is_mutref(),
            _ => false
          }
    }

    pub fn is_closure(&self) -> bool {
        match self {
            ResourceTy::Value(r) | ResourceTy::Deref(r) | ResourceTy::Index(r, _, _) | ResourceTy::DerefField(r, _, _) => r.is_closure(),
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
            ResourceTy::Deref(r) | ResourceTy::Value(r) | ResourceTy::Index(r, _, _) | ResourceTy::DerefField(r, _, _) => Some(r)
        }
    }

    /// The reference RAP a write through `*r` / `r.f` goes through.
    pub fn deref_rap(&self) -> Option<&ResourceAccessPoint> {
        match self {
            ResourceTy::Deref(r) | ResourceTy::DerefField(r, _, _) => Some(r),
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

    /// A copy of this function RAP marked as a call that clones or
    /// downgrades an `Rc` / `Arc`.
    pub fn with_rc_call(&self, call: RcCall) -> ResourceAccessPoint {
        let mut rap = self.clone();
        if let ResourceAccessPoint::Function(f) = &mut rap {
            f.rc_call = Some(call);
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

    /// True iff this RAP is an unbound temporary (#457): a call's
    /// result that nothing binds, dropped by an event of its own. It
    /// is labelled like a lifetime-extended one (`temporary_label`),
    /// so the label can't tell the two apart.
    pub fn is_unbound_temporary(&self) -> bool {
        matches!(self, ResourceAccessPoint::Owner(Owner { unbound_temp: true, .. }))
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

    /// True iff this RAP's type implements Copy. References are
    /// always Copy; functions don't model resources at all, so we
    /// say Copy too (no drop indicator would ever apply to them).
    pub fn is_copy(&self) -> bool {
        match self {
            ResourceAccessPoint::Owner(Owner{is_copy, ..}) => *is_copy,
            ResourceAccessPoint::Struct(Struct{is_copy, ..}) => *is_copy,
            ResourceAccessPoint::MutRef(_) | ResourceAccessPoint::StaticRef(_) => true,
            ResourceAccessPoint::Function(_) => true,
        }
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

    /// True iff this RAP is a non-reference owner whose value holds a
    /// borrow (see `Owner::holds_borrow`).
    pub fn holds_borrow(&self) -> bool {
        matches!(self, ResourceAccessPoint::Owner(Owner { holds_borrow: true, .. }))
    }

    /// True iff this RAP is an owner of a Copy type (i32 etc.). The
    /// "ownership" framing in tooltip text doesn't fit primitives —
    /// they're values, not heap resources — so this lets the tooltip
    /// dispatcher pick non-ownership wording where appropriate.
    pub fn has_interior_mut(&self) -> bool {
        matches!(self, ResourceAccessPoint::Owner(Owner { interior_mut: true, .. })
                     | ResourceAccessPoint::Struct(Struct { interior_mut: true, .. }))
    }

    pub fn is_union(&self) -> bool {
        matches!(self, ResourceAccessPoint::Owner(Owner { is_union: true, .. }))
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

    pub fn is_owner(&self) -> bool {
        match self {
            ResourceAccessPoint::Owner(_) => true,
            _ => false
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
    /// An `if let`: its first arm starts by binding the pattern, on the
    /// split line (see `IF_LET_ARM`).
    pub fn is_if_let(&self) -> bool {
        matches!(self, BranchType::If(labels, _) if labels.first().is_some_and(|l| l == IF_LET_ARM))
    }
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
    // Only used for the merge dot's wording.
    pub exits: Vec<BranchExit>,
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
/// ends it where the borrow checker does. Holds the place's name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StoredIn(pub Option<String>);
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
    /// The borrower is a reference copied out of a collection that
    /// holds references (`let r = refs[0];`; #349). `func` names the
    /// collection.
    Element,
    /// The borrower is the receiver a `&mut self` method stored the
    /// borrow in (`refs.push(&s)`; #392). `func` names the method.
    StoredBy,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ExternalEvent {
    /* let binding, e.g.: let x = 1 */
    Bind {
        from: ResourceTy,
        to: ResourceTy,
        id: usize
    },
    // `into_closure`: a closure literal passed to a call captures
    // `from` by value (`thread::spawn(move || ..v..)`): `from` moves
    // (or is copied) into the closure, not into the callee; `to` is
    // the callee. Only `match_closure_arg` sets it.
    Copy {
        from: ResourceTy,
        to: ResourceTy,
        is_partial: bool,
        into_closure: bool,
        id: usize
    },
    Move {
        from: ResourceTy,
        to: ResourceTy,
        is_partial: bool,
        into_closure: bool,
        id: usize
    },
    // `is_reborrow`: the borrow was written `&*r` / `&mut *r` with
    // `r` a reference — a reborrow through `r`, as opposed to a fresh
    // borrow of an owner (`&s`) or of the reference variable itself
    // (`&r`, `&mut r`). Decided syntactically at the `AddrOf` site in
    // `match_rhs`; every other producer passes `false`.
    // `via`: the borrow reaches `to` through the reference a call
    // returned (`let w = first_word(&s);`, #171); see `BorrowVia`.
    StaticBorrow {
        from: ResourceTy,
        to: ResourceTy,
        is_partial: bool,
        is_reborrow: bool,
        via: Option<BorrowVia>,
        id: usize
    },
    MutableBorrow {
        from: ResourceTy,
        to: ResourceTy,
        is_partial: bool,
        is_reborrow: bool,
        via: Option<BorrowVia>,
        id: usize
    },
    // `static_lender`: what the reference borrowed, when that has no
    // column but is known to be data that lives for the whole program
    // (see StaticLender). Only the arrow's tooltip uses it.
    StaticDie {
        // return the resource to "to"
        from: ResourceTy,
        to: ResourceTy,
        id: usize,
        static_lender: Option<StaticLender>,
    },
    MutableDie {
        // return the resource to "to"
        from: ResourceTy,
        to: ResourceTy,
        id: usize,
        static_lender: Option<StaticLender>,
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
        // See `StoredIn`.
        stored_in: StoredIn,
        result_keeps_borrow: bool,
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
        id: usize
    },
    GoOutOfScope {
        ro: ResourceAccessPoint,
        id: usize
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
    TemporaryDrop {
        ro: ResourceAccessPoint,
        ty: String,
        when: String,
        id: usize
    },
    // At the end of a loop: `ro` (declared outside the loop) was moved
    // in a branch that then `break`s out of the loop, so after the
    // loop it may have been moved (`definite`: it has been, because
    // every way out of the loop is such a `break`).
    // `labeled_block`: the `break` leaves a labeled block (`'a: { … }`),
    // not a loop.
    LoopExitMove {
        ro: ResourceAccessPoint,
        definite: bool,
        labeled_block: bool,
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
            &ExternalEvent::LoopExitMove { id, .. } |
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
    Acquire {
        from: ResourceTy,
        is: ResourceTy,
        id: usize
    },
    // this happens when a ResourceAccessPoint implements copy trait or
    // explicitly calls .clone() function
    // to another ResourceAccessPoint, or a function.
    //
    // e.g.
    // 1. x: i32 = y + 15;              here y duplicate to + op, and x acquire from +
    //                                  at this point, we treat it as y duplicates to None
    // 2. x: MyStruct = y.clone();      here y duplicates to x.
    Duplicate {
        to: ResourceTy,
        is: ResourceTy,
        into_closure: bool,
        id: usize
    },
    // this happens when a ResourceAccessPoint transfers a copy of its contents
    // to another ResourceAccessPoint.
    // Typically, this occurs when a resource owner implements the Copy trait.
    Copy {
        from: ResourceTy,
        is: ResourceTy,
        id: usize
    },
    // this happens when a ResourceAccessPoint transfer the ownership of its resource
    // to another ResourceAccessPoint, or if it is no longer used after this line.
    // Typically, this happens at one of the following two cases:
    //
    // 1. variable is not used after this line.
    // 2. variable's resource has the move trait, and it transfered
    //    its ownership on this line. This includes tranfering its
    //    ownership to a function as well.
    Move {
        to: ResourceTy,
        is: ResourceTy,
        into_closure: bool,
        id: usize
    },

    // Mirrors the ExternalEvent Branch
    Branch {
      is: ResourceTy,
      branch_history: Vec<BranchData>,
      ty: BranchType,
      split_point: usize,
      merge_point: usize,
      id: usize
    },

    MutableLend {
        to: ResourceTy,
        is: ResourceTy,
        id: usize
    },
    MutableBorrow {
        from: ResourceTy,
        is: ResourceTy,
        id: usize
    },
    MutableDie {
        to: ResourceTy,
        is: ResourceTy,
        id: usize
    },
    MutableReacquire {
        from: ResourceTy,
        is: ResourceTy,
        id: usize
    },
    StaticLend {
        to: ResourceTy,
        is: ResourceTy,
        id: usize
    },
    StaticBorrow {
        from: ResourceTy,
        is: ResourceTy,
        id: usize
    },
    StaticDie {
        to: ResourceTy,
        is: ResourceTy,
        id: usize
    },
    StaticReacquire {
        from: ResourceTy,
        is: ResourceTy,
        id: usize
    },

    RefDie {
      from: ResourceTy,
      is: ResourceTy,
      num_curr_borrowers: usize,
      id: usize
    },
    // this happens when a owner is returned this line,
    // or if this owner's scope ends at this line. The data must be dropped.
    OwnerGoOutOfScope,
    // this happens when a vairable that is not an owner goes out of scope.
    // The data is not dropped in this case
    RefGoOutOfScope,
    // The previously-held resource is dropped at a reassignment
    // (`y = x` or `*p = x`). The owner stays alive — its state is
    // unchanged across this event because the matching Acquire from
    // rhs lands on the same line and immediately re-establishes
    // FullPrivilege. `maybe_moved`: see ExternalEvent::OwnerDropAtReassign.
    OwnerDropAtReassign { maybe_moved: bool },
    // `ManuallyDrop::drop(&mut md)`: the value inside is dropped by
    // hand. The owner is left in a moved-out-like state.
    ManuallyDropByHand,
    // An unbound temporary is dropped (see ExternalEvent).
    TemporaryDrop { ty: String, when: String },
    // See ExternalEvent::LoopExitMove.
    LoopExitMove { definite: bool, labeled_block: bool },
    // SPECIAL CASE: use only to initialize a fn's paramter
    // Requires param to be Owner, StaticRef, or MutRef (cannot be Function)
    // (`own_param`, `in_pattern`: see ExternalEvent::InitRefParam)
    InitRefParam {
        param: ResourceAccessPoint,
        id: usize,
        own_param: Option<OwnParam>,
        in_pattern: Option<String>,
    },
    // `f()` of an `FnOnce` closure `is`: `is` is moved into the call
    // (ExternalEvent::ClosureCall).
    ClosureCall {
        is: ResourceTy,
        fates: Vec<CapturedFate>,
        id: usize
    },
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


// Whether `rty` is an owner (`i32`, a Copy struct or field, or a non-Copy
// one with nothing to drop), whose tooltips say "value" rather than
// "resource" (#274, #484). A `*r`
// place isn't: what it holds belongs to r's lender.
fn holds_copy_value(rty: &ResourceTy) -> bool {
    matches!(rty, ResourceTy::Value(r) if r.holds_value())
        // A borrow from / returned to a reference r (or `*r`) is of
        // what r points to (#313), a value if that's Copy (#312).
        || matches!(rty, ResourceTy::Value(r) | ResourceTy::Deref(r) if r.points_to_copy())
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

fn safe_message(
    message_functor: fn(&String, &String) -> String,
    my_name: &String,
    some_target: &ResourceTy
) -> String {

    let target_name = match some_target {
        ResourceTy::Deref(r) => {
            let mut temp = r.name().clone();
            temp.insert(0, '*');
            temp
        }
        _ => some_target.name()
    };
    message_functor(my_name, &target_name)
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
            State::ResourceMoved{ move_to , move_at_line: _, .. } => {
                safe_message(hover_messages::state_resource_moved, my_name, move_to)
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
            State::RevokedPrivilege { to: _, borrow_to , prev_state: _} => {
                safe_message(hover_messages::state_resource_revoked, my_name, borrow_to)
            }
            State::Invalid => {
                hover_messages::state_invalid(my_name)
            }
        }
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
        
        fn rank(s: &State) -> u8 {
            match s {
                FullPrivilege { s: LineState::Uninit } => 3,
                ResourceMoved { .. } | RevokedPrivilege { .. } => 0,
                PartialPrivilege { .. } => 1,
                FullPrivilege {..} => 2,
                Invalid | OutOfScope => 3,
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

pub fn convert_back(s: &State) -> State {
    match s {
        State::FullPrivilege { s: LineState::Uninit } => State::OutOfScope,
        State::FullPrivilege { .. } => State::FullPrivilege { s: LineState::Full },
        State::PartialPrivilege { .. } => State::PartialPrivilege { s: LineState::Full },
        _ => s.clone() 
    }
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
            Event::OwnerGoOutOfScope => {
                write!(f, "Goes out of Scope as an owner of resource" ) }
            Event::RefGoOutOfScope => {
                write!(f, "Goes out of Scope as a reference to resource")
            },
            Event::OwnerDropAtReassign { .. } => {
                write!(f, "Previous resource is dropped at reassignment")
            },
            Event::ManuallyDropByHand => {
                write!(f, "Value is dropped by hand")
            },
            Event::TemporaryDrop { ty, when } => {
                write!(f, "Temporary {} is dropped {}", ty, when)
            },
            Event::LoopExitMove { .. } => {
                write!(f, "May have been moved when the loop ends")
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
    pub fn extract_is(&self) -> &ResourceTy {
        match self {
            Event::Duplicate {is: x , ..} | Event::Move {is: x, ..} | Event::StaticLend {is : x, ..} | 
            Event::MutableLend {is: x, ..} | Event::MutableDie {is: x, ..} | Event::StaticDie {is: x, ..} | 
            Event::Acquire { is: x, .. } | Event::Copy { is: x, .. } | Event::MutableBorrow { is: x, .. } |
            Event::StaticBorrow { is: x, .. } | Event::StaticReacquire { is: x, .. } | Event::MutableReacquire {is: x, ..}
            | Event::Branch { is: x, .. } | Event::ClosureCall { is: x, .. } => {
                x
            }
            _ => &ResourceTy::Anonymous
        }
    }
    pub fn is_branch(&self) -> bool {
        match self {
            Event::Branch { .. } => true, 
            _ => false
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
                    ResourceTy::DerefField(_, place, _) => {
                        *name = place.clone();
                        name
                    },
                    _ => name
                }
            },
            _ => name
        }
    }

    // This function tells us what the text associated with a Dot event should be
    pub fn print_message_with_name(&self, my_name: &mut String) -> String {
        match self {
            // no arrow involved
            Event::ClosureCall { fates, .. } => {
                hover_messages::event_dot_closure_call_once(my_name, fates)
            }
            Event::OwnerGoOutOfScope => {
                hover_messages::event_dot_owner_go_out_out_scope(my_name)
            }
            Event::RefGoOutOfScope => {
                hover_messages::event_dot_ref_go_out_out_scope(my_name)
            }
            Event::OwnerDropAtReassign { maybe_moved: false } => {
                hover_messages::event_dot_owner_drop_at_reassign(my_name, None)
            }
            Event::OwnerDropAtReassign { maybe_moved: true } => {
                hover_messages::event_dot_owner_drop_at_reassign_maybe_moved(my_name)
            }
            Event::ManuallyDropByHand => {
                hover_messages::event_dot_manually_drop_by_hand(my_name)
            }
            Event::LoopExitMove { definite, labeled_block } => {
                hover_messages::event_dot_loop_exit_move(my_name, *definite, *labeled_block)
            }
            Event::InitRefParam{ param, own_param: Some(own), .. } => {
                hover_messages::event_dot_ref_init_own_param(my_name, &own.fn_name, own.whole, matches!(param, ResourceAccessPoint::MutRef(_)), own.drops)
            }
            Event::InitRefParam{ param, in_pattern: Some(bound), .. } => {
                hover_messages::event_dot_ref_init_in_pattern(my_name, bound, matches!(param, ResourceAccessPoint::MutRef(_)))
            }
            Event::TemporaryDrop { ty, when } => {
                hover_messages::event_dot_temporary_drop(ty, when)
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
                    .map_or(true, |r| (r.is_ref() && !r.is_mutref()) || r.has_interior_mut());
                let through_ref = ref_lender(is).map(|r| r.points_to_copy());
                hover_messages::event_dot_lend_to_undrawn_place(&self.deref_name(my_name), false, omit_read_only, through_ref, is.extract_rap().map_or(false, |r| r.holds_value()))
            }
            Event::MutableLend{ to: ResourceTy::Anonymous, is, ..} => {
                let through_ref = ref_lender(is).map(|r| r.points_to_copy());
                hover_messages::event_dot_lend_to_undrawn_place(&self.deref_name(my_name), true, false, through_ref, is.extract_rap().map_or(false, |r| r.holds_value()))
            }
            // arrow going out
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
            // `let first = v[0];`: one element is copied out of the
            // resource v owns (or points to); v keeps all of it.
            Event::Duplicate{ is: ResourceTy::Index(r, place, _), .. } => {
                hover_messages::event_dot_copy_out_of_element(r.name(), place, r.is_ref(), if r.is_ref() { r.points_to_copy() } else { r.holds_value() })
            }
            Event::Duplicate{ to, is: ResourceTy::DerefField(r, place, _), ..} => {
                // `let a = p.x;` with p a reference: the field is
                // copied out of the resource p points to (#266).
                let target = match to {
                    ResourceTy::Caller => "the caller".to_owned(),
                    _ => to.name(),
                };
                hover_messages::event_dot_copy_through_deref_field(r.name(), place, &target, r.points_to_copy())
            }
            Event::Duplicate{ to, is, ..} => {
                match to {
                    // A `move` closure capturing a Copy value copies it (#311).
                    _ if to.is_closure() => {
                        let is_ref = matches!(is, ResourceTy::Value(ResourceAccessPoint::StaticRef(_)));
                        hover_messages::event_dot_capture_copy_to_closure(&self.deref_name(my_name), &to.name(), is_ref)
                    }
                    // Returning a `&T` copies the reference, not the referent.
                    ResourceTy::Caller if matches!(is, ResourceTy::Value(ResourceAccessPoint::StaticRef(_))) =>
                        safe_message(hover_messages::event_dot_copy_ref_to_caller, &self.deref_name(my_name), to),
                    ResourceTy::Caller => safe_message(hover_messages::event_dot_copy_to_caller, &self.deref_name(my_name), to),
                    _ if !to.is_closure() && matches!(is, ResourceTy::Value(ResourceAccessPoint::StaticRef(_))) =>
                        hover_messages::event_dot_ref_copied(my_name),
                    _ => safe_message(hover_messages::event_dot_copy_to, &self.deref_name(my_name), to)
                }
            }
            // Moving a `&mut` moves the reference, not its referent.
            Event::Move{ to, is: ResourceTy::Value(ResourceAccessPoint::MutRef(_)), ..} if !to.is_closure() => {
                hover_messages::event_dot_ref_moved(my_name, matches!(to, ResourceTy::Caller))
            }
            Event::Move{ is: ResourceTy::Index(_, place, _), ..} if place.starts_with('*') => {
                hover_messages::event_dot_move_through_element(place)
            }
            Event::Move{ to, is, ..} => {
                let v = is_plain(is);
                match to {
                    ResourceTy::Caller => safe_message(if v { hover_messages::event_dot_move_to_caller_value } else { hover_messages::event_dot_move_to_caller }, &self.deref_name(my_name), to),
                    ResourceTy::Temporary { extended } => hover_messages::event_dot_move_to_temporary(&self.deref_name(my_name), *extended),
                    ResourceTy::Value(ResourceAccessPoint::Function(f)) if f.drops_arg.0 =>
                        hover_messages::event_dot_move_to_drop(&self.deref_name(my_name), &f.name, v,
                            is.extract_rap().and_then(|r| r.rc_drop())),
                    _ if to.is_closure() => safe_message(if v { hover_messages::event_dot_capture_move_to_closure_value } else { hover_messages::event_dot_capture_move_to_closure }, &self.deref_name(my_name), to),
                    _ => safe_message(if v { hover_messages::event_dot_move_to_value } else { hover_messages::event_dot_move_to }, &self.deref_name(my_name), to),
                }

            }
            Event::StaticLend{ to, is, ..} => {
                // Only an owner's own Copy value here: a reference
                // lender is handled by event_dot_lend_through_ref.
                let is_copy = matches!(is, ResourceTy::Value(r) if r.holds_value());
                if to.is_closure() {
                    let target_name = safe_message(|_, t| t.clone(), my_name, to);
                    hover_messages::event_dot_capture_static_lend_to_closure(&self.deref_name(my_name), &target_name, is_copy)
                } else {
                    // `is` is the lender. A `&T` lender was read-only
                    // already, and a `RefCell` / `Cell` / `Mutex` lender
                    // isn't read-only at all (it can be mutated through
                    // the shared borrow); see event_dot_static_lend.
                    let omit_read_only = is.extract_rap()
                        .map_or(true, |r| (r.is_ref() && !r.is_mutref()) || r.has_interior_mut());
                    let target_name = safe_message(|_, t| t.clone(), my_name, to);
                    if let Some(r) = ref_lender(is) {
                        return hover_messages::event_dot_lend_through_ref(r.name(), &target_name, false, r.is_mutref(), r.points_to_copy());
                    }
                    hover_messages::event_dot_static_lend(&self.deref_name(my_name), &target_name, omit_read_only, is_copy)
                }
            }
            Event::MutableLend{ to, is, ..} => {
                let is_copy = matches!(is, ResourceTy::Value(r) if r.holds_value());
                let target_name = safe_message(|_, t| t.clone(), my_name, to);
                if to.is_closure() {
                    hover_messages::event_dot_capture_mut_lend_to_closure(&self.deref_name(my_name), &target_name, is_copy)
                } else if let Some(r) = ref_lender(is) {
                    hover_messages::event_dot_lend_through_ref(r.name(), &target_name, true, r.is_mutref(), r.points_to_copy())
                } else {
                    hover_messages::event_dot_mut_lend(&self.deref_name(my_name), &target_name, is_copy)
                }
            }
            Event::StaticDie{ to,.. } => {
                safe_message(hover_messages::event_dot_static_return, &self.deref_name(my_name), to)
            }
            Event::MutableDie{ to ,..} => {
                safe_message(hover_messages::event_dot_mut_return, &self.deref_name(my_name), to)
            }
            // arrow going in
            Event::Acquire{ from, is, ..} => {
                if is.is_closure() && from.extract_rap().is_some() {
                    // `f` (closure) acquiring from a real source —
                    // this is a `move`-capture, not a generic
                    // ownership transfer. The Anonymous-from case
                    // (the closure literal's own Bind) keeps the
                    // generic message because there's no captured
                    // upvar to name on the closure side yet.
                    safe_message(if from.extract_rap().map_or(false, |r| r.holds_value()) { hover_messages::event_dot_closure_capture_acquire_value } else { hover_messages::event_dot_closure_capture_acquire }, &self.deref_name(my_name), from)
                } else if let ResourceTy::Index(r, place, write) = is {
                    // `v[i] = s;` / `v[i] = 5;` assigns to the place
                    // `v[i]`: it reads like a plain assignment, and v
                    // doesn't acquire anything. `v[i] += k` modifies
                    // the element in place.
                    match write {
                        PlaceWrite::AssignCopy => place_assign_copy_message(place, from),
                        PlaceWrite::AssignDropsOld => hover_messages::event_dot_owner_drop_at_reassign(place, None),
                        // The old element has nothing to drop (that's what makes
                        // it AssignOwned): a plain assignment (#484).
                        PlaceWrite::AssignOwned => hover_messages::event_dot_copyable_assign(place),
                        PlaceWrite::Update | PlaceWrite::UpdateValue => hover_messages::event_dot_write_element(r.name(), place, r.is_ref(), element_update_holds_value(r, place, write)),
                    }
                } else if is.holds_borrow() && matches!(from, ResourceTy::Value(r) if !r.is_fn()) {
                    // `let it2 = it;`: the value moves, and the borrow
                    // it holds moves with it (#336).
                    hover_messages::event_dot_holder_acquire(&self.deref_name(my_name), &from.name())
                } else if let Some(r) = is.deref_rap() {
                    // `*r += 1;` / `r.f += 1;` modify the resource r
                    // points to in place. `*r = v;` / `r.f = v;` are
                    // assignments to the place `*r` / `r.f`: they read
                    // like a plain assignment (2026-11-05 decision),
                    // and the drop sentence of the old value is the
                    // one plain assignment uses.
                    let write = match is {
                        ResourceTy::DerefField(_, _, w) => *w,
                        _ => r.place_write(),
                    };
                    match write {
                        PlaceWrite::AssignCopy => place_assign_copy_message(&self.deref_name(my_name), from),
                        PlaceWrite::AssignDropsOld => hover_messages::event_dot_owner_drop_at_reassign(&self.deref_name(my_name), None),
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
                } else if matches!(is, ResourceTy::Value(rap) if rap.is_unbound_temporary()) {
                    // `make();`, `make().len()`: nothing takes the value.
                    safe_message(hover_messages::event_dot_unbound_temporary_acquire, &self.deref_name(my_name), from)
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
                    safe_message(hover_messages::event_dot_acquire_copyable, &self.deref_name(my_name), from)
                } else if is_plain(is) {
                    // A non-Copy value with nothing to drop (an iterator
                    // over borrowed data, a struct of integers), or a
                    // Copy field that came along with its parent: a
                    // value, not a resource (#484).
                    safe_message(hover_messages::event_dot_acquire_value, &self.deref_name(my_name), from)
                } else {
                    safe_message(hover_messages::event_dot_acquire, &self.deref_name(my_name), from)
                }
            }
            Event::Copy{ from, is: ResourceTy::Index(r, place, write), ..} => {
                // `v[i] = y;` with y: Copy assigns a new value to the
                // place `v[i]`.
                match write {
                    PlaceWrite::Update | PlaceWrite::UpdateValue => hover_messages::event_dot_write_element(r.name(), place, r.is_ref(), element_update_holds_value(r, place, write)),
                    _ => place_assign_copy_message(place, from),
                }
            }
            Event::Copy{ from, is: is @ (ResourceTy::Deref(r) | ResourceTy::DerefField(r, _, _)), ..} => {
                // `*r = y;` / `r.f = y;` with y: Copy assigns a new value
                // to the place `*r` / `r.f`; it isn't an initialization.
                let deref = self.deref_name(my_name).clone();
                let write = match is {
                    ResourceTy::DerefField(_, _, w) => *w,
                    _ => r.place_write(),
                };
                match write {
                    PlaceWrite::AssignCopy => place_assign_copy_message(&deref, from),
                    // `*r += y` / `r.f += y`: an in-place update; the arrow
                    // shows the copy. `*r` is a Copy value here; `r.f`
                    // is part of what r points to.
                    _ => hover_messages::event_dot_write_through_deref(r.name(), &deref, matches!(is, ResourceTy::Deref(_)) || r.points_to_copy()),
                }
            }
            Event::Copy{ from ,..} => {
                safe_message(hover_messages::event_dot_copy_from, &self.deref_name(my_name), from)
            }
            Event::MutableBorrow{ from, is, ..} => {
                if is.is_closure() {
                    hover_messages::event_dot_closure_capture_mut_borrow(&self.deref_name(my_name), &from.name())
                } else if is.holds_borrow() {
                    hover_messages::event_dot_holder_borrow(&self.deref_name(my_name), &from.name(), true, from.is_ref())
                } else {
                    hover_messages::event_dot_mut_borrow(&self.deref_name(my_name), &from.name(), holds_copy_value(from))
                }
            }
            Event::StaticBorrow{ from, is, ..} => {
                if is.is_closure() {
                    hover_messages::event_dot_closure_capture_static_borrow(&self.deref_name(my_name), &from.name())
                } else if is.holds_borrow() {
                    hover_messages::event_dot_holder_borrow(&self.deref_name(my_name), &from.name(), false, from.is_ref())
                } else {
                    hover_messages::event_dot_static_borrow(&self.deref_name(my_name), &from.name(), holds_copy_value(from))
                }
            }
            Event::StaticReacquire{ from, is, ..} | Event::MutableReacquire{ from, is, ..} if ref_lender(is).is_some() => {
                let r = ref_lender(is).unwrap();
                let mutable = matches!(self, Event::MutableReacquire { .. });
                hover_messages::event_dot_reacquire_through_ref(r.name(), borrower_name(from).as_ref(), mutable, r.is_mutref(), r.points_to_copy())
            }
            Event::StaticReacquire{ from, is, ..} => {
                let from_name = safe_message(|_, t| t.clone(), my_name, from);
                hover_messages::event_dot_static_reacquire(&self.deref_name(my_name), &from_name, holds_copy_value(is))
            }
            Event::MutableReacquire{ from, is, ..} => {
                let from_name = safe_message(|_, t| t.clone(), my_name, from);
                hover_messages::event_dot_mut_reacquire(&self.deref_name(my_name), &from_name, holds_copy_value(is))
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

pub fn string_of_external_event(e: &ExternalEvent) -> String {
    match e {
        ExternalEvent::Bind{ .. } => {
            String::from("Bind")
        },
        ExternalEvent::Copy{ is_partial, to, .. } => {
            if *is_partial { String::from("Partial copy") }
            else if to.is_closure() { String::from("Closure capture (copy)") }
            else { String::from("Copy") }
        },
        // Move/StaticBorrow/MutableBorrow into a closure binding are
        // closure captures — the renderer's `from … to …` template
        // (in render_arrow) reads e.g. "Closure capture (move)" as
        // "Closure capture (move) from s to f", which scans more
        // clearly than a bare "Move" arrow into a closure.
        ExternalEvent::Move{ is_partial, to, .. } => {
            if *is_partial { String::from("Partial move") }
            else if to.is_closure() { String::from("Closure capture (move)") }
            else { String::from("Move") }
        },
        ExternalEvent::StaticBorrow{ is_partial, is_reborrow, to, .. } => {
            if *is_partial { String::from("Partial immutable borrow") }
            // A closure capturing `*r` through a reference r (#277).
            else if to.is_closure() && *is_reborrow { String::from("Closure capture (immutable reborrow)") }
            else if to.is_closure() { String::from("Closure capture (immutable borrow)") }
            else if *is_reborrow { String::from("Immutable reborrow") }
            else { String::from("Immutable borrow") }
        },
        ExternalEvent::StaticDie{ to, .. } => {
            format!("Return immutably borrowed {}", if holds_copy_value(to) { "value" } else { "resource" })
        },
        ExternalEvent::MutableBorrow{ is_partial, is_reborrow, to, .. } => {
            if *is_partial { String::from("Partial Mutable borrow") }
            else if to.is_closure() && *is_reborrow { String::from("Closure capture (mutable reborrow)") }
            else if to.is_closure() { String::from("Closure capture (mutable borrow)") }
            // Reborrow: `let r2 = &mut *r;` with r: `&mut T` borrows
            // *through* r — r can't be used while r2 is live and usable
            // again afterwards. Distinct from `&mut r`, which is a
            // fresh borrow of the variable r itself (r2: `&mut &mut T`).
            else if *is_reborrow { String::from("Mutable reborrow") }
            else { String::from("Mutable borrow") }
        },
        ExternalEvent::MutableDie{ to, .. } => {
           format!("Return mutably borrowed {}", if holds_copy_value(to) { "value" } else { "resource" })
        },
        // Reached only when the callee isn't a Function RAP — a call
        // through a closure variable (`f(&x)`, `g(r)`); calls to real
        // fns get their "f reads from x" title from render_arrow's
        // function-target arm instead. Deliberately no "reborrow"
        // wording here: whether `g(r)` reborrows or moves `r` depends
        // on the callee's parameter type, which this event doesn't
        // carry for the closure case, and passing a `&T` is a copy.
        ExternalEvent::PassByMutableReference{ .. } => {
            String::from("Pass by mutable reference")
        },
        ExternalEvent::PassByStaticReference{ .. } => {
            String::from("Pass by immutable reference")
        },
        _ => panic!("internal state: external event has no pass-by-reference title"),
    }
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

        // A value that holds a borrow without being a reference
        // (`let it = v.iter();`, #336) owns its own value: borrowing
        // through it, and the end of that borrow, don't change what
        // the holder itself can do.
        let holder = self.timelines.get(hash)
            .is_some_and(|t| t.resource_access_point.holds_borrow());

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

            // A variable is initialized through a Move
            (State::OutOfScope, Event::Acquire{ .. }) => State::FullPrivilege {s: LineState::Full},

            // A variable is initialized from a Copy
            (State::OutOfScope, Event::Copy{ from: ro, is: is_ro, ..}) => {
                match ro {
                    ResourceTy::Anonymous => State::FullPrivilege {s: LineState::Full},
                    ResourceTy::Deref(r) | ResourceTy::Value(r) | ResourceTy::DerefField(r, _, _) | ResourceTy::Index(r, _, _) => {
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

            // The current RAP's resource is moved
            (State::FullPrivilege{..}, Event::Move{to: to_ro,..}) =>
                State::ResourceMoved{ move_to: to_ro.to_owned(), move_at_line: event_line, dropped_by_hand: false },

            // Calling an `FnOnce` closure moves it into the call (#271):
            // the body consumes what it captured, so there's nothing
            // left for the closure to drop at scope end.
            (State::FullPrivilege{..}, Event::ClosureCall{..}) =>
                State::ResourceMoved{ move_to: ResourceTy::Anonymous, move_at_line: event_line, dropped_by_hand: false },
            // `let t = f();` also records the call's result as moving
            // out of `f`, on the same line: `f` is already moved.
            (State::ResourceMoved{ move_at_line, .. }, Event::Move{..}) if *move_at_line == event_line && event_line != 0 =>
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

            (State::FullPrivilege{..}, Event::MutableLend{ to: to_ro ,..}) => {
            // Assumption: variables can lend mutably if
            // 1) variable instance is mutable or 2) variable is a mutable reference
            // Use cases: 'mutable_borrow' & 'nll_lexical_scope_different'
                if self.is_mut(hash) | self.is_mutref(hash) {
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

            (State::FullPrivilege{..}, Event::Acquire{ from: _ ,..}) | (State::FullPrivilege{..}, Event::Copy{ from: _ ,..}) => {
                    State::FullPrivilege{ s: LineState::Full }
            },

            (State::FullPrivilege{..}, Event::OwnerGoOutOfScope) =>
                State::OutOfScope,

            (State::FullPrivilege{..}, Event::RefGoOutOfScope) =>
                State::OutOfScope,

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

            (State::PartialPrivilege{ .. }, Event::OwnerGoOutOfScope) =>
                State::OutOfScope,

            (State::PartialPrivilege{ .. }, Event::RefGoOutOfScope) =>
                State::OutOfScope,

            (State::RevokedPrivilege{ prev_state: p,.. }, Event::MutableReacquire{ .. }) => {
              *p.clone()
            }

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
            (_, Event::Duplicate { to: ResourceTy::Caller,..}) => State::OutOfScope,
            (_, Event::Duplicate { .. }) => (*previous_state).clone(),

            (_, Event::Branch { .. }) => { // technically not necessary - this is handled inside the compute_states function
                State::OutOfScope
            }
            (_, Event::OwnerGoOutOfScope) | (_, Event::RefGoOutOfScope) => State::OutOfScope,

            // OwnerDropAtReassign is paired with an Acquire/Move on
            // the same line that immediately re-establishes ownership,
            // so the visible state should be the same as before. We
            // preserve the previous state for any input — the dot is
            // a visual annotation, not an ownership transition.
            (_, Event::OwnerDropAtReassign { .. }) => (*previous_state).clone(),
            // An unbound temporary's column ends where it is dropped.
            (_, Event::TemporaryDrop { .. }) => State::OutOfScope,

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

        // Render opaque line if branch does not begin at split point
        if begin != branch_start {
            states.push((branch_start, begin, branch_state_converter(&previous_state)));
        }

        let mut previous_line = begin;
        for (l, e) in history {
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
                Event::Branch { branch_history, ty, split_point, merge_point, .. } => {
                    // Timeline is empty in the middle during a branch
                    states.push((*split_point, *merge_point, State::OutOfScope));
                    // State of each branch that reaches the merge; a
                    // diverging branch (#296) never gets there.
                    let mut reaching_states: Vec<State> = Vec::new();
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
                        }
                    }
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
                    // Every branch leaves the function (#515): nothing
                    // follows the conditional, and the column ends in
                    // the branches.
                    if !branch_history.is_empty() && branch_history.iter().all(|b| b.diverges) {
                        previous_state = State::OutOfScope;
                    }
                    // Resume the parent timeline at `merge_point`,
                    // not `merge_point + 1`. The join dot sits at
                    // `merge_point` (see render_dot's Branch arm);
                    // resuming a row later leaves a visible gap
                    // between the join and the post-merge state.
                    previous_line = *merge_point;
                }
                _ => {
                    previous_state = self.calc_state(&previous_state, e, *l, hash);
                    previous_line = *l;
                }
            }
        }
        // `max`: an event below the branch's active range (possible
        // when branches share lines) mustn't make the range run upwards.
        let end = end.max(previous_line);
        states.push((previous_line, end, previous_state.clone()));

        // Render opaque line if branch does not end at merge point
        if end < branch_end {
            states.push((end, branch_end, branch_state_converter(&previous_state)));
        }

        *states = clean_states(&states);

        states.last().unwrap().2.clone() // return the last state in the branch
    }


    fn compute_timeline_states(&self, history: & mut Vec<(usize, Event)>, states: & mut Vec<(usize, usize, State)>, hash: &u64) {
        let mut previous_line = 1;
        let mut previous_state = State::OutOfScope;
        for (l, e) in history {
            states.push((previous_line, *l, previous_state.clone()));
            match e {
                Event::Branch { branch_history, ty, split_point, merge_point, .. } => {
                    // Timeline is empty in the middle during a branch
                    states.push((*split_point, *merge_point, State::OutOfScope));
                    // State of each branch that reaches the merge; a
                    // diverging branch (#296) never gets there.
                    let mut reaching_states: Vec<State> = Vec::new();
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
                        }
                    }
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
                    // Every branch leaves the function (#515): nothing
                    // follows the conditional, and the column ends in
                    // the branches.
                    if !branch_history.is_empty() && branch_history.iter().all(|b| b.diverges) {
                        previous_state = State::OutOfScope;
                    }
                    // Resume the parent timeline at `merge_point`,
                    // not `merge_point + 1`. The join dot sits at
                    // `merge_point` (see render_dot's Branch arm);
                    // resuming a row later leaves a visible gap
                    // between the join and the post-merge state.
                    previous_line = *merge_point;

                }
                _ => {
                    previous_state = self.calc_state(&previous_state, e, *l, hash);
                    previous_line = *l;
                }
            }
        }
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
                timeline.history.push(
                    (*line_number, event)
                );
            },
            _ => {
                panic!("internal state: timeline #{} vanished right after it was created", hash);
            }
        }
    }

    // Just a function to get the respective Events that should be appended to a given RAP's timeline given
    // an ExternalEvent and whether or not the RAP is the receiver of the action
    fn event_of_exteranl_event(&self, line_num: usize, ext_ev: &ExternalEvent, to_o: bool) -> Vec<(usize, Event)> {
      match ext_ev {
        ExternalEvent::Bind { from, to, id } => {
          if to_o { vec![(line_num,  Event::Acquire{from : from.to_owned(), is: to.clone(), id: *id})] }
          else { vec![(line_num, Event::Duplicate{to : to.to_owned(), is: from.clone(), into_closure: false, id: *id})] }
        },
        ExternalEvent::Copy{from: from_ro, to: to_ro, id, into_closure, ..} => {
          if to_o { vec![(line_num,  Event::Copy{from : from_ro.to_owned(), is: to_ro.clone(), id: *id})] }
          else { vec![(line_num, Event::Duplicate{to : to_ro.to_owned(), is: from_ro.clone(), into_closure: *into_closure, id: *id})] }
        },
        ExternalEvent::Move{from: from_ro, to: to_ro, id, into_closure, ..} => {
          if to_o { vec![(line_num,  Event::Acquire{from : from_ro.to_owned(), is: to_ro.clone(), id: *id})] }
          else { vec![(line_num, Event::Move{to : to_ro.to_owned(), is: from_ro.clone(), into_closure: *into_closure, id: *id})] }
        },
        ExternalEvent::StaticBorrow{from: from_ro, to: to_ro, id, ..} => {
          if to_o { vec![(line_num,  Event::StaticBorrow{from : from_ro.to_owned(), is: to_ro.clone(), id: *id})] }
          else { vec![(line_num, Event::StaticLend{to : to_ro.to_owned(), is: from_ro.clone(), id: *id})] }
        },
        ExternalEvent::StaticDie{from: from_ro, to: to_ro, id, ..} => {
          if to_o && !from_ro.is_same_underlying(&to_ro){ 
            vec![(line_num,  Event::StaticReacquire{from : from_ro.to_owned(), is: to_ro.clone(), id: *id})] 
          }
          else { vec![(line_num, Event::StaticDie{to : to_ro.to_owned(), is: from_ro.clone(), id: *id})] }
        },
        ExternalEvent::MutableBorrow{from: from_ro, to: to_ro, id, ..} => {
          if to_o { vec![(line_num,  Event::MutableBorrow{from : from_ro.to_owned(), is: to_ro.clone(), id: *id})] }
          else { vec![(line_num, Event::MutableLend{to : to_ro.to_owned(), is: from_ro.clone(), id: *id})] }
        },
        ExternalEvent::MutableDie{from: from_ro, to: to_ro, id, ..} => {
          if to_o && !from_ro.is_same_underlying(&to_ro){ 
            vec![(line_num,  Event::MutableReacquire{from : from_ro.to_owned(), is: to_ro.clone(), id: *id})] 
          }
          else { vec![(line_num, Event::MutableDie{to : to_ro.to_owned(), is: from_ro.clone(), id: *id})] }    
        },
        ExternalEvent::RefDie { from: from_ro, to: to_ro, id ,..} => {
          match from_ro.clone() {
              ResourceTy::Deref(ro_is) | ResourceTy::Value(ro_is) => {
                  if ro_is.is_mutref() && !to_o {
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
        ExternalEvent::PassByStaticReference{from: from_ro, to: to_ro, id, stored_in, result_keeps_borrow, ..} => {
          if to_o { 
            vec![(line_num,  Event::StaticBorrow{from : from_ro.to_owned(), is: to_ro.clone(), id: *id}),
            (line_num,  Event::StaticDie{to : from_ro.to_owned(), is: to_ro.clone(), id: *id})] 
          }
          else if stored_in.0.is_some() || *result_keeps_borrow {
            vec![(line_num, Event::StaticLend{to : to_ro.to_owned(), is: from_ro.clone(), id: *id})]
          }
          else { 
            vec![(line_num, Event::StaticLend{to : to_ro.to_owned(), is: from_ro.clone(), id: *id}),
            (line_num,  Event::StaticReacquire{from : to_ro.to_owned(), is: from_ro.clone(), id: *id})]
          }
        },
        ExternalEvent::PassByMutableReference{from: from_ro, to: to_ro, id, stored_in, result_keeps_borrow, ..} => {
          if to_o { 
            vec![(line_num,  Event::MutableBorrow{from : from_ro.to_owned(), is: to_ro.clone(), id: *id}),
            (line_num,  Event::MutableDie{to : from_ro.to_owned(), is: to_ro.clone(), id: *id})]
          }
          else if stored_in.0.is_some() || *result_keeps_borrow {
            vec![(line_num, Event::MutableLend{to : to_ro.to_owned(), is: from_ro.clone(), id: *id})]
          }
          else { 
            vec![(line_num, Event::MutableLend{to : to_ro.to_owned(), is: from_ro.clone(), id: *id}),
            (line_num,  Event::MutableReacquire{from : to_ro.to_owned(), is: from_ro.clone(), id: *id})]
          }
        },
        ExternalEvent::InitRefParam{param: ro, id, own_param, in_pattern} => {
          vec![(line_num, Event::InitRefParam{param : ro.to_owned(), id: *id, own_param: own_param.clone(), in_pattern: in_pattern.clone()})]
        },
        ExternalEvent::GoOutOfScope{ro,..} => {
          match ro {
            ResourceAccessPoint::Owner(..) | ResourceAccessPoint::Struct(..) => {
              vec![(line_num, Event::OwnerGoOutOfScope)]
            },
            ResourceAccessPoint::MutRef(..) | ResourceAccessPoint::StaticRef(..)=> {
              vec![(line_num, Event::RefGoOutOfScope)]
            },
            ResourceAccessPoint::Function(func) => {
              panic!(
                  "internal state: function \"{}\" went out of scope, but functions never do",
                  func.name
              );
            }
          }
        },
        ExternalEvent::OwnerDropAtReassign{ maybe_moved, .. } => {
          vec![(line_num, Event::OwnerDropAtReassign { maybe_moved: *maybe_moved })]
        },
        ExternalEvent::ManuallyDropByHand{..} => {
          vec![(line_num, Event::ManuallyDropByHand)]
        },
        ExternalEvent::TemporaryDrop{ ty, when, .. } => {
          vec![(line_num, Event::TemporaryDrop { ty: ty.clone(), when: when.clone() })]
        },
        ExternalEvent::LoopExitMove{definite, labeled_block, ..} => {
          vec![(line_num, Event::LoopExitMove { definite: *definite, labeled_block: *labeled_block })]
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
            ExternalEvent::Branch { live_vars, branches, branch_type, split_point, merge_point, id } => {
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
                                        id: *id}));
                }
            }
            ExternalEvent::LoopExitMove { ro, .. } => {
                if ro == rap {
                    b_history.extend(self.event_of_exteranl_event(line_number, event, true));
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
                    b_history.extend(self.event_of_exteranl_event(line_number, event, false));
                }
                else if *to == *is || through_rap(to) || to_elem {
                    b_history.extend(self.event_of_exteranl_event(line_number, event, true));
                }
                else if let Some(r) = event.is_gos_ev() {
                    if *r.name() == is.real_name() {
                        b_history.extend(self.event_of_exteranl_event(line_number, event, true));
                    }
                }
                // A reference field of a struct bound by a pattern
                // (`if let Some(x) = o`, `x.part`) is initialized in
                // the branch (#352); `ResourceAccessPoint_extract`
                // doesn't name it.
                else if let ExternalEvent::InitRefParam { param, .. } = event {
                    if param == rap {
                        b_history.extend(self.event_of_exteranl_event(line_number, event, true));
                    }
                }
                // `x = …` / `*r = …` inside a branch drops the value the
                // variable (or r's lender) held before. The event names
                // that variable in `ro`, not in from/to (#267).
                else if let ExternalEvent::OwnerDropAtReassign { ro, .. } = event {
                    if ro.hash() == rap.hash() {
                        b_history.extend(self.event_of_exteranl_event(line_number, event, true));
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
        self.external_events.push((line_number, event.clone()));        
        // append_event to timelines unless interacting with an anonymous owner
        // Eventually we should collect these interactions with anonymous owners and display them in the code panel
        fn maybe_append_event (vd: & mut VisualizationData, resource_ty: &ResourceTy, event: Event, line_number: usize){
          match resource_ty {
            ResourceTy::Anonymous | ResourceTy::Caller | ResourceTy::Temporary { .. } => {}
            ResourceTy::Deref(r) | ResourceTy::Value(r) | ResourceTy::Index(r, _, _) | ResourceTy::DerefField(r, _, _) => {
                vd._append_event(&r, event, &line_number);
            }
          }
        }

        match event {
            // eg let ro_to = String::from("");
            ExternalEvent::Move{from: from_ro, to: to_ro, id, into_closure, ..} => {
                maybe_append_event(self, &to_ro.clone(), Event::Acquire{from : from_ro.to_owned(), is: to_ro.clone(), id: id}, line_number);
                maybe_append_event(self, &from_ro.clone(), Event::Move{to : to_ro.to_owned(), is: from_ro, into_closure, id: id}, line_number);
            },
            // eg: let ro_to = 5;
            ExternalEvent::Bind{from: from_ro, to: to_ro, id} => {
                maybe_append_event(self, &to_ro.clone(), Event::Acquire{from : from_ro.to_owned(), is: to_ro.clone(), id: id}, line_number);
                maybe_append_event(self, &from_ro.clone(), Event::Duplicate{to : to_ro.to_owned(), is: from_ro, into_closure: false, id: id}, line_number);
            },
            // eg: let x : i64 = y as i64;
            ExternalEvent::Copy{from: from_ro, to: to_ro, id, into_closure, ..} => {
                maybe_append_event(self, &to_ro.clone(), Event::Copy{from : from_ro.to_owned(), is: to_ro.clone(), id: id}, line_number);
                maybe_append_event(self, &from_ro.clone(), Event::Duplicate{to : to_ro.to_owned(), is: from_ro, into_closure, id: id}, line_number);
            },
            ExternalEvent::Branch { live_vars, branches, branch_type, split_point, merge_point, id } => {
                // need to add events with more granularity for Branches
                for var in in_decl_order(&live_vars) { // for all the live variables in the branch (declaration order, #211)
                    let is = ResourceTy::Value(var.clone());
                    let mut branch_history: Vec<BranchData> = Vec::new();
                    for branch in branches.iter() { // for each branch
                        let mut b_history: Vec<(usize, Event)> = Vec::new();
                        for (l, ev) in branch.e_data.iter() { // for each event in each branch
                            self.append_branch_event(ev, *l, &is, & mut b_history);
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
                                        id: id}, line_number);
                }

                // have to append events for variables declared inside the block
                for branch in branches.iter() {
                    for var in in_decl_order(&branch.decl_vars) {
                        let mut b_history: Vec<(usize, Event)> = Vec::new();
                        let is = &ResourceTy::Value(var.clone());
                        for (l, e) in branch.e_data.iter() {
                            self.append_branch_event(e, *l, is, & mut b_history);
                        }
                        for (l, e) in b_history {
                            self._append_event(var, e, &l);
                        }
                    }
                    self.append_decl_branch_events(&branch.e_data);
                }
            }
            // A borrow stored in an element (`v[0] = &u`) has the
            // element as borrower: only the lender's column shows it
            // (#282), so the `Index` side gets no event.
            ExternalEvent::StaticBorrow{from: from_ro, to: to_ro, id, ..} => {
                maybe_append_event(self, &from_ro, Event::StaticLend{to : to_ro.to_owned(), is: from_ro.clone(), id: id}, line_number);
                if !matches!(to_ro, ResourceTy::Index(..)) {
                    maybe_append_event(self, &to_ro.clone(), Event::StaticBorrow{from : from_ro.to_owned(), is: to_ro, id: id}, line_number);
                }

            },
            ExternalEvent::StaticDie{from: from_ro, to: to_ro, id, ..} => {
              // this catches when StaticDie(s to s*) to avoid duplicate events
                if !from_ro.is_same_underlying(&to_ro) {
                  maybe_append_event(self, &to_ro.clone(), Event::StaticReacquire{from : from_ro.to_owned(), is: to_ro.clone(), id: id}, line_number);
                }
                if !matches!(from_ro, ResourceTy::Index(..)) {
                    maybe_append_event(self, &from_ro.clone(), Event::StaticDie{to : to_ro.to_owned(), is: from_ro, id: id}, line_number);
                }
            },
            ExternalEvent::MutableBorrow{from: from_ro, to: to_ro, id, ..} => {
                maybe_append_event(self, &from_ro, Event::MutableLend{to : to_ro.to_owned(), is: from_ro.clone(), id: id}, line_number);
                if !matches!(to_ro, ResourceTy::Index(..)) {
                    maybe_append_event(self, &to_ro.clone(), Event::MutableBorrow{from : from_ro.to_owned(), is: to_ro, id: id}, line_number);
                }

            },
            ExternalEvent::MutableDie{from: from_ro, to: to_ro, id, ..} => {
                // this catches when MutableDie(s to s*) to avoid duplicate events
                if !from_ro.is_same_underlying(&to_ro) {
                  maybe_append_event(self, &to_ro.clone(), Event::MutableReacquire{from : from_ro.to_owned(), is: to_ro.clone(), id: id}, line_number);
                }
                if !matches!(from_ro, ResourceTy::Index(..)) {
                    maybe_append_event(self, &from_ro.clone(), Event::MutableDie{to : to_ro.to_owned(), is: from_ro, id: id}, line_number);
                }
            },
            // don't need to append an event to the to resource since a RefDie doesn't change the state of the to ro
            ExternalEvent::RefDie { from: from_ro, to: _, id, ..} => { // need Ref Event to avoid drawing redundant arrows when rendering timelines
                match from_ro.clone() {
                    ResourceTy::Deref(ro_is) | ResourceTy::Value(ro_is) => {
                        if ro_is.is_mutref() {
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
            ExternalEvent::PassByStaticReference{from: from_ro, to: to_ro, id, stored_in, result_keeps_borrow, ..} => {
                maybe_append_event(self, &from_ro.to_owned(), Event::StaticLend{to : to_ro.to_owned(), is: from_ro.clone(), id: id}, line_number);
                maybe_append_event(self, &to_ro.to_owned(), Event::StaticBorrow{from : from_ro.to_owned(), is: to_ro.clone(), id: id}, line_number);
                if stored_in.0.is_none() && !result_keeps_borrow {
                    maybe_append_event(self, &from_ro, Event::StaticReacquire{from : to_ro.to_owned(), is: from_ro.clone(), id: id}, line_number);
                }
                maybe_append_event(self, &to_ro.clone(), Event::StaticDie{to : from_ro.to_owned(), is: to_ro, id: id}, line_number);
            },
            ExternalEvent::PassByMutableReference{from: from_ro, to: to_ro, id, stored_in, result_keeps_borrow, ..} => {
                maybe_append_event(self, &from_ro.clone(), Event::MutableLend{to : to_ro.to_owned(), is: from_ro.clone(), id: id}, line_number);
                maybe_append_event(self, &to_ro.clone(), Event::MutableBorrow{from : from_ro.to_owned(), is: to_ro.clone(), id: id}, line_number);
                if stored_in.0.is_none() && !result_keeps_borrow {
                    maybe_append_event(self, &from_ro.clone(), Event::MutableReacquire{from : to_ro.to_owned(), is: from_ro.clone(), id: id}, line_number);
                }
                maybe_append_event(self, &to_ro.clone(), Event::MutableDie{to : from_ro.to_owned(), is: to_ro, id: id}, line_number);
            },
            ExternalEvent::ClosureCall { closure, fates, id } => {
                maybe_append_event(self, &closure, Event::ClosureCall { is: closure.clone(), fates, id }, line_number);
            },
            ExternalEvent::InitRefParam{param: ro, id, own_param, in_pattern} => {
                maybe_append_event(self, &ResourceTy::Value(ro.clone()), Event::InitRefParam{param : ro.to_owned(), id: id, own_param, in_pattern}, line_number);
            },
            ExternalEvent::GoOutOfScope{ro, ..} => {
                match ro {
                    ResourceAccessPoint::Owner(..) | ResourceAccessPoint::Struct(..) => {
                        maybe_append_event(self, &ResourceTy::Value(ro), Event::OwnerGoOutOfScope, line_number);
                    },
                    ResourceAccessPoint::MutRef(..) | ResourceAccessPoint::StaticRef(..)=> {
                        maybe_append_event(self, &ResourceTy::Value(ro), Event::RefGoOutOfScope, line_number);
                    },
                    ResourceAccessPoint::Function(func) => {
                        panic!(
                            "internal state: function \"{}\" went out of scope, but functions never do",
                            func.name
                        );
                    }
                }
            },
            ExternalEvent::OwnerDropAtReassign{ro, maybe_moved, ..} => {
                maybe_append_event(self, &ResourceTy::Value(ro), Event::OwnerDropAtReassign { maybe_moved }, line_number);
            },
            ExternalEvent::ManuallyDropByHand{ro, ..} => {
                maybe_append_event(self, &ResourceTy::Value(ro), Event::ManuallyDropByHand, line_number);
            },
            ExternalEvent::TemporaryDrop{ro, ty, when, ..} => {
                maybe_append_event(self, &ResourceTy::Value(ro), Event::TemporaryDrop { ty, when }, line_number);
            },
            ExternalEvent::LoopExitMove{ro, definite, labeled_block, ..} => {
                maybe_append_event(self, &ResourceTy::Value(ro), Event::LoopExitMove { definite, labeled_block }, line_number);
            },
        }
    }
}