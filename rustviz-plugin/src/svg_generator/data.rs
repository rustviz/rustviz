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
// Top level Api that the Timeline object supports
pub trait Visualizable {
    // Function that computes states for variables inside of branches
    fn compute_branch_states(&self, history: & mut Vec<(usize, Event)>, 
                            states: &mut Vec<(usize, usize, State)>, 
                            hash: &u64, 
                            valid_range: (usize, usize), 
                            branch_start: usize, 
                            branch_end: usize, 
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
}

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
}

// a reference of type &mut T
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct MutRef {         // let (mut) r1 = &mut a;
    pub name: String,
    pub hash: u64,
    pub is_mut: bool,
    /// Hash of the parent struct when this ref is a struct field
    /// (e.g. `Excerpt { p: &mut x }` makes p a MutRef whose
    /// member_of is Excerpt's hash). None for free-standing refs.
    pub member_of: Option<u64>,
}

// a reference of type & T
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct StaticRef {                // let (mut) r1 = & a;
    pub name: String,
    pub hash: u64,
    pub is_mut: bool,
    /// Same as MutRef::member_of — set when this ref is a struct
    /// field. Lets the layout pass include it in the parent's
    /// bounding box.
    pub member_of: Option<u64>,
}

#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct Function {
    pub name: String,
    pub hash: u64,
}

// ResourceTy is a wrapper for ResourceAccessPoints
// and what we use to populate an ExternalEvent. The wrapper is 
// necessary because it allows us to distinguish between the RAP being used as a
// Value (itself) or if its being dereferenced. 
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ResourceTy {
    Anonymous, // an anonymous resource holder
    Deref(ResourceAccessPoint), // Dereferencing a RAP
    Caller, // For expressing relationship between return expr and fn
    Value(ResourceAccessPoint) 
}

impl ResourceTy {
    pub fn name(&self) -> String {
        match self {
            ResourceTy::Anonymous => "Anonymous resource".to_owned(),
            ResourceTy::Value(r) => r.name().to_owned(),
            ResourceTy::Deref(r) => format!("*{}", r.name()),
            ResourceTy::Caller => "Caller".to_owned()
        }
    }

    pub fn real_name(&self) -> String {
        match self {
            ResourceTy::Anonymous => "Anonymous resource".to_owned(),
            ResourceTy::Value(r) | ResourceTy::Deref(r) => r.name().to_owned(),
            ResourceTy::Caller => "Caller".to_owned()
        }
    }

    pub fn hash(&self) -> &u64 {
        match self {
            ResourceTy::Caller | ResourceTy::Anonymous => &std::u64::MAX,
            ResourceTy::Value(r) | ResourceTy::Deref(r) => r.hash(),
        }
    }

    pub fn is_ref(&self) -> bool {
      match self {
        ResourceTy::Value(r) | ResourceTy::Deref(r) => r.is_ref(),
        _ => false
      }
    }

    pub fn is_mutref(&self) -> bool {
        match self {
            ResourceTy::Value(r) | ResourceTy::Deref(r) => r.is_mutref(),
            _ => false
          }
    }

    pub fn extract_rap(&self) -> Option<&ResourceAccessPoint> {
        match self {
            ResourceTy::Anonymous | ResourceTy::Caller => None,
            ResourceTy::Deref(r) | ResourceTy::Value(r) => Some(r)
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

impl ResourceAccessPoint {
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
    pub decl_vars: HashSet<ResourceAccessPoint>
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
        if e.is_arrow_ev() {
            res.entry(*l).and_modify(|v| {v.push(e.clone())}).or_insert(vec![e.clone()]);
        }
    }
    res
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ExternalEvent {
    /* let binding, e.g.: let x = 1 */
    Bind {
        from: ResourceTy,
        to: ResourceTy,
        id: usize
    },
    Copy {
        from: ResourceTy,
        to: ResourceTy,
        is_partial: bool,
        id: usize
    },
    Move {
        from: ResourceTy,
        to: ResourceTy,
        is_partial: bool,
        id: usize
    },
    StaticBorrow {
        from: ResourceTy,
        to: ResourceTy,
        is_partial: bool,
        id: usize
    },
    MutableBorrow {
        from: ResourceTy,
        to: ResourceTy,
        is_partial: bool,
        id: usize
    },
    StaticDie {
        // return the resource to "to"
        from: ResourceTy,
        to: ResourceTy,
        id: usize
    },
    MutableDie {
        // return the resource to "to"
        from: ResourceTy,
        to: ResourceTy,
        id: usize
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
    PassByStaticReference {
        from: ResourceTy,
        to: ResourceTy, // must be a function
        id: usize
    },
    PassByMutableReference {
        from: ResourceTy,
        to: ResourceTy, // must be a function
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
    OwnerDropAtReassign {
        ro: ResourceAccessPoint,
        id: usize
    },
    // only use this event to initialize fn parameters
    InitRefParam {
        param: ResourceAccessPoint,
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

    pub fn is_gos_ev(&self) -> Option<&ResourceAccessPoint> {
        match self {
            ExternalEvent::GoOutOfScope { ro, .. } => Some(ro),
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
            &ExternalEvent::RefDie { id, .. } | &ExternalEvent::Bind { id, .. } |
            &ExternalEvent::PassByStaticReference { id, .. } | &ExternalEvent::PassByMutableReference {id, .. } |
            &ExternalEvent::InitRefParam { id, .. }=> id
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
  pub states: Vec<(usize, usize, State)>
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
    // FullPrivilege.
    OwnerDropAtReassign,
    // SPECIAL CASE: use only to initialize a fn's paramter
    // Requires param to be Owner, StaticRef, or MutRef (cannot be Function)
    InitRefParam {
        param: ResourceAccessPoint,
        id: usize
    },
}



#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LineState {
    Full,
    Gray
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
        move_at_line: usize
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
            State::ResourceMoved { move_to: _, move_at_line: _ } => write!(f, "ResourceMoved"),
            State::FullPrivilege { .. } => write!(f, "FullPrivilege"),
            State::PartialPrivilege { .. } => write!(f, "PartialPrivilege"),
            State::RevokedPrivilege { .. } => write!(f, "RevokedPrivilege"),
            State::Invalid => write!(f, "Invalid"),
        }
    }
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
            State::ResourceMoved{ move_to , move_at_line: _ } => {
                safe_message(hover_messages::state_resource_moved, my_name, move_to)
            }
            State::FullPrivilege {..}=> {
                hover_messages::state_full_privilege(my_name)
            }
            State::PartialPrivilege { .. } => {
                hover_messages::state_partial_privilege(my_name)
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


pub fn branch_state_converter(s: &State) -> State {
    match s {
        State::FullPrivilege { .. } => State::FullPrivilege { s: LineState::Gray },
        State::PartialPrivilege { .. } => State::PartialPrivilege { s: LineState::Gray },
        _ => s.clone() 
    }
}

pub fn convert_back(s: &State) -> State {
    match s {
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
            Event::OwnerGoOutOfScope => {
                write!(f, "Goes out of Scope as an owner of resource" ) }
            Event::RefGoOutOfScope => {
                write!(f, "Goes out of Scope as a reference to resource")
            },
            Event::OwnerDropAtReassign => {
                write!(f, "Previous resource is dropped at reassignment")
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
            | Event::Branch { id: x, .. } | Event::InitRefParam { id: x, .. } => {
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
            | Event::Branch { is: x, .. } => {
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
            Event::OwnerGoOutOfScope => {
                hover_messages::event_dot_owner_go_out_out_scope(my_name)
            }
            Event::RefGoOutOfScope => {
                hover_messages::event_dot_ref_go_out_out_scope(my_name)
            }
            Event::OwnerDropAtReassign => {
                hover_messages::event_dot_owner_drop_at_reassign(my_name)
            }
            Event::InitRefParam{ param, .. } => {
                // Owner/Struct params receive ownership from the
                // caller — call that out so the dot hover matches
                // the L-arrow's. Ref params receive a borrow from
                // the caller and get the parallel ref message.
                match param {
                    ResourceAccessPoint::Owner(_) | ResourceAccessPoint::Struct(_) => {
                        hover_messages::event_dot_owner_init_from_caller(my_name)
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
            // arrow going out
            Event::Duplicate{ to ,..} => {
                match to {
                    ResourceTy::Caller => safe_message(hover_messages::event_dot_copy_to_caller, &self.deref_name(my_name), to),
                    _ => safe_message(hover_messages::event_dot_copy_to, &self.deref_name(my_name), to)
                }
            }
            Event::Move{ to ,..} => {
                match to {
                    ResourceTy::Caller => safe_message(hover_messages::event_dot_move_to_caller, &self.deref_name(my_name), to),
                    _ => safe_message(hover_messages::event_dot_move_to, &self.deref_name(my_name), to),
                }
                
            }
            Event::StaticLend{ to ,..} => {
                safe_message(hover_messages::event_dot_static_lend, &self.deref_name(my_name), to)
            }
            Event::MutableLend{ to ,..} => {
                safe_message(hover_messages::event_dot_mut_lend, &self.deref_name(my_name), to)
            }
            Event::StaticDie{ to,.. } => {
                safe_message(hover_messages::event_dot_static_return, &self.deref_name(my_name), to)
            }
            Event::MutableDie{ to ,..} => {
                safe_message(hover_messages::event_dot_mut_return, &self.deref_name(my_name), to)
            }
            // arrow going in
            Event::Acquire{ from ,..} => {
                safe_message(hover_messages::event_dot_acquire, &self.deref_name(my_name), from)
            }
            Event::Copy{ from ,..} => {
                safe_message(hover_messages::event_dot_copy_from, &self.deref_name(my_name), from)
            }
            Event::MutableBorrow{ from ,..} => {
                hover_messages::event_dot_mut_borrow(&self.deref_name(my_name), &from.name())
            }
            Event::StaticBorrow{ from ,..} => {
                hover_messages::event_dot_static_borrow(&self.deref_name(my_name), &from.name())
            }
            Event::StaticReacquire{ from ,..} => {
                safe_message(hover_messages::event_dot_static_reacquire, &self.deref_name(my_name), from)
            }
            Event::MutableReacquire{ from ,..} => {
                safe_message(hover_messages::event_dot_mut_reacquire, &self.deref_name(my_name), from)
            }
            Event::RefDie {..} => {
              panic!("should never be calling this function with this event");
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
    //struct owner hash, x val of struct owner, x val of the rightmost member
    pub structs: Vec<(i64, i64, i64)>,
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

pub fn string_of_external_event(e: &ExternalEvent) -> String {
    match e {
        ExternalEvent::Bind{ .. } => {
            String::from("Bind")
        },
        ExternalEvent::Copy{ is_partial,.. } => {
            if *is_partial { String::from("Partial copy") } 
            else { String::from("Copy") }
        },
        ExternalEvent::Move{ is_partial, .. } => {
            if *is_partial { String::from("Partial move")}
            else { String::from("Move") }
        },
        ExternalEvent::StaticBorrow{ is_partial, .. } => {
            if *is_partial { String::from("Partial immutable borrow") }
            else { String::from("Immutable borrow") }
        },
        ExternalEvent::StaticDie{ .. } => {
            String::from("Return immutably borrowed resource")
        },
        ExternalEvent::MutableBorrow{ is_partial, .. } => {
            if *is_partial { String::from("Partial Mutable borrow") }
            else { String::from("Mutable borrow")}
        },
        ExternalEvent::MutableDie{ .. } => {
           String::from("Return mutably borrowed resource")
        },
        ExternalEvent::PassByMutableReference{ .. } => {
            String::from("Pass by mutable reference")
        },
        ExternalEvent::PassByStaticReference{ .. } => {
            String::from("Pass by immutable reference")
        },
        _ => unreachable!(),
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

        match (previous_state, event) {
            // propogate invalid states
            (State::Invalid, _) => State::Invalid,

            // A variable is initialized through a Move
            (State::OutOfScope, Event::Acquire{ .. }) => State::FullPrivilege {s: LineState::Full},

            // A variable is initialized from a Copy
            (State::OutOfScope, Event::Copy{ from: ro, is: is_ro, ..}) => {
                match ro {
                    ResourceTy::Anonymous => State::FullPrivilege {s: LineState::Full},
                    ResourceTy::Deref(r) | ResourceTy::Value(r) => {
                        // if we are copying a reference - then the State must be PartialPrivilege
                        // ex: 
                        // let a = &b; (ImmutableBorrow(b -> a))
                        // let c = a; (Copy(a -> c)) even though c is technically borrowing from b at the end of the day
                        if r.is_ref() && is_ro.is_ref() {
                            if r.is_mutref() {
                                panic!("Not possible, has to be a move");
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
                    _ => panic!("not possible")
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
                        panic!("Cannot initialize function as as valid parameter!")
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
                State::ResourceMoved{ move_to: to_ro.to_owned(), move_at_line: event_line },

            // The current RAP reaquires a resource 
            (State::ResourceMoved{ .. }, Event::Acquire{ .. }) => {
                if self.is_mut(hash) {
                    State::FullPrivilege{ s: LineState::Full }
                }
                else { // immut variables cannot reacquire resource
                    panic!("Immutable variable {} cannot reacquire resources!", self.get_name_from_hash(hash).unwrap());
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
            (_, Event::OwnerDropAtReassign) => (*previous_state).clone(),


            (_, _) => State::Invalid,
        }
    }

    fn compute_branch_states(&self, 
    history: & mut Vec<(usize, Event)>, 
    states: & mut Vec<(usize, usize, State)>, 
    hash: &u64, 
    valid_range: (usize, usize),
    branch_start: usize,
    branch_end: usize,
    mut previous_state: State) -> State{
        // an empty branch - rendered as the same as previous state
        if history.is_empty() {
            states.push((branch_start, branch_end, branch_state_converter(&previous_state)));
            return previous_state;
        }

        let (begin, end) = valid_range;
        // Render opaque line if branch does not begin at split point
        if begin != branch_start {
            states.push((branch_start, begin, branch_state_converter(&previous_state)));
        }

        let mut previous_line = begin;
        for (l, e) in history {
            states.push((previous_line, *l, previous_state.clone()));
            match e {
                Event::Branch { branch_history, ty, split_point, merge_point, .. } => {
                    // Timeline is empty in the middle during a branch
                    states.push((*split_point, *merge_point, State::OutOfScope));
                    let mut ending_states: Vec<State> = Vec::new();
                    for (i, branch) in branch_history.iter_mut().enumerate() {
                        ending_states.push(self.compute_branch_states(
                            & mut branch.e_data, 
                            & mut branch.states, 
                            hash, 
                            ty.get_start_end(i), 
                            *split_point + 1,
                            *merge_point,
                            previous_state.clone()));
                    }

                    ending_states.sort(); // partial order of the ending states
                    previous_state = convert_back(&ending_states.first().unwrap());
                    previous_line = *merge_point + 1; // timeline starts one line after the curly brace
                }
                _ => {
                    previous_state = self.calc_state(&previous_state, e, *l, hash);
                    previous_line = *l;
                }
            }
        }
        states.push((previous_line, end, previous_state.clone()));

        // Render opaque line if branch does not end at merge point
        if end != branch_end {
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
                    let mut ending_states: Vec<State> = Vec::new();
                    for (i, branch) in branch_history.iter_mut().enumerate() {
                        ending_states.push(self.compute_branch_states(
                            & mut branch.e_data, 
                            & mut branch.states, 
                            hash, 
                            ty.get_start_end(i), 
                            *split_point + 1,
                            *merge_point,
                            previous_state.clone()));
                    }

                    ending_states.sort(); // partial order of the ending states
                    previous_state = convert_back(&ending_states.first().unwrap());
                    previous_line = *merge_point + 1; // timeline starts one line after the curly brace

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
                panic!("Timeline disappeared right after creation or when we could index it. This is impossible.");
            }
        }
    }

    // Just a function to get the respective Events that should be appended to a given RAP's timeline given
    // an ExternalEvent and whether or not the RAP is the receiver of the action
    fn event_of_exteranl_event(&self, line_num: usize, ext_ev: &ExternalEvent, to_o: bool) -> Vec<(usize, Event)> {
      match ext_ev {
        ExternalEvent::Bind { from, to, id } => {
          if to_o { vec![(line_num,  Event::Acquire{from : from.to_owned(), is: to.clone(), id: *id})] }
          else { vec![(line_num, Event::Duplicate{to : to.to_owned(), is: from.clone(), id: *id})] }
        },
        ExternalEvent::Copy{from: from_ro, to: to_ro, id, ..} => {
          if to_o { vec![(line_num,  Event::Copy{from : from_ro.to_owned(), is: to_ro.clone(), id: *id})] }
          else { vec![(line_num, Event::Duplicate{to : to_ro.to_owned(), is: from_ro.clone(), id: *id})] }
        },
        ExternalEvent::Move{from: from_ro, to: to_ro, id, ..} => {
          if to_o { vec![(line_num,  Event::Acquire{from : from_ro.to_owned(), is: to_ro.clone(), id: *id})] }
          else { vec![(line_num, Event::Move{to : to_ro.to_owned(), is: from_ro.clone(), id: *id})] }
        },
        ExternalEvent::StaticBorrow{from: from_ro, to: to_ro, id, ..} => {
          if to_o { vec![(line_num,  Event::StaticBorrow{from : from_ro.to_owned(), is: to_ro.clone(), id: *id})] }
          else { vec![(line_num, Event::StaticLend{to : to_ro.to_owned(), is: from_ro.clone(), id: *id})] }
        },
        ExternalEvent::StaticDie{from: from_ro, to: to_ro, id} => {
          if to_o && !from_ro.is_same_underlying(&to_ro){ 
            vec![(line_num,  Event::StaticReacquire{from : from_ro.to_owned(), is: to_ro.clone(), id: *id})] 
          }
          else { vec![(line_num, Event::StaticDie{to : to_ro.to_owned(), is: from_ro.clone(), id: *id})] }
        },
        ExternalEvent::MutableBorrow{from: from_ro, to: to_ro, id, ..} => {
          if to_o { vec![(line_num,  Event::MutableBorrow{from : from_ro.to_owned(), is: to_ro.clone(), id: *id})] }
          else { vec![(line_num, Event::MutableLend{to : to_ro.to_owned(), is: from_ro.clone(), id: *id})] }          
        },
        ExternalEvent::MutableDie{from: from_ro, to: to_ro, id} => {
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
              _ => panic!("not possible")
          }
        },
        ExternalEvent::PassByStaticReference{from: from_ro, to: to_ro, id} => {
          if to_o { 
            vec![(line_num,  Event::StaticBorrow{from : from_ro.to_owned(), is: to_ro.clone(), id: *id}),
            (line_num,  Event::StaticDie{to : from_ro.to_owned(), is: to_ro.clone(), id: *id})] 
          }
          else { 
            vec![(line_num, Event::StaticLend{to : to_ro.to_owned(), is: from_ro.clone(), id: *id}),
            (line_num,  Event::StaticReacquire{from : to_ro.to_owned(), is: from_ro.clone(), id: *id})]
          }
        },
        ExternalEvent::PassByMutableReference{from: from_ro, to: to_ro, id} => {
          if to_o { 
            vec![(line_num,  Event::MutableBorrow{from : from_ro.to_owned(), is: to_ro.clone(), id: *id}),
            (line_num,  Event::MutableDie{to : from_ro.to_owned(), is: to_ro.clone(), id: *id})]
          }
          else { 
            vec![(line_num, Event::MutableLend{to : to_ro.to_owned(), is: from_ro.clone(), id: *id}),
            (line_num,  Event::MutableReacquire{from : to_ro.to_owned(), is: from_ro.clone(), id: *id})]
          }
        },
        ExternalEvent::InitRefParam{param: ro, id} => {
          vec![(line_num, Event::InitRefParam{param : ro.to_owned(), id: *id})]
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
                  "Functions do not go out of scope! We do not expect to see \"{}\" here.",
                  func.name
              );
            }
          }
        },
        ExternalEvent::OwnerDropAtReassign{..} => {
          vec![(line_num, Event::OwnerDropAtReassign)]
        },

        _ => panic!("should not be calling this on branches")
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
                          }, e_data: new_b_history, width: 0, states: Vec::new() });
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
            _ => {
                let could_be = ResourceTy::Deref(rap.clone());
                let (from, to) = ResourceAccessPoint_extract(event);
                if *from == *is || *from == could_be {
                    b_history.extend(self.event_of_exteranl_event(line_number, event, false));
                }
                else if *to == *is || *to == could_be {
                    b_history.extend(self.event_of_exteranl_event(line_number, event, true));
                }
                else if let Some(r) = event.is_gos_ev() {
                    if *r.name() == is.real_name() {
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
                        for var in branch.decl_vars.iter() {
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
            ResourceTy::Anonymous | ResourceTy::Caller => {}
            ResourceTy::Deref(r) | ResourceTy::Value(r) => {
                vd._append_event(&r, event, &line_number);
            }
          }
        }

        match event {
            // eg let ro_to = String::from("");
            ExternalEvent::Move{from: from_ro, to: to_ro, id, ..} => {
                maybe_append_event(self, &to_ro.clone(), Event::Acquire{from : from_ro.to_owned(), is: to_ro.clone(), id: id}, line_number);
                maybe_append_event(self, &from_ro.clone(), Event::Move{to : to_ro.to_owned(), is: from_ro, id: id}, line_number);
            },
            // eg: let ro_to = 5;
            ExternalEvent::Bind{from: from_ro, to: to_ro, id} => {
                maybe_append_event(self, &to_ro.clone(), Event::Acquire{from : from_ro.to_owned(), is: to_ro.clone(), id: id}, line_number);
                maybe_append_event(self, &from_ro.clone(), Event::Duplicate{to : to_ro.to_owned(), is: from_ro, id: id}, line_number);
            },
            // eg: let x : i64 = y as i64;
            ExternalEvent::Copy{from: from_ro, to: to_ro, id, ..} => {
                maybe_append_event(self, &to_ro.clone(), Event::Copy{from : from_ro.to_owned(), is: to_ro.clone(), id: id}, line_number);
                maybe_append_event(self, &from_ro.clone(), Event::Duplicate{to : to_ro.to_owned(), is: from_ro, id: id}, line_number);
            },
            ExternalEvent::Branch { live_vars, branches, branch_type, split_point, merge_point, id } => {
                // need to add events with more granularity for Branches
                for var in live_vars.iter() { // for all the live variables in the branch
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
                        }, e_data: b_history, width: 0, states: Vec::new() });
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
                    for var in branch.decl_vars.iter() {
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
            ExternalEvent::StaticBorrow{from: from_ro, to: to_ro, id, ..} => {
                maybe_append_event(self, &from_ro, Event::StaticLend{to : to_ro.to_owned(), is: from_ro.clone(), id: id}, line_number);
                maybe_append_event(self, &to_ro.clone(), Event::StaticBorrow{from : from_ro.to_owned(), is: to_ro, id: id}, line_number);
                
            },
            ExternalEvent::StaticDie{from: from_ro, to: to_ro, id} => {
              // this catches when StaticDie(s to s*) to avoid duplicate events
                if !from_ro.is_same_underlying(&to_ro) {
                  maybe_append_event(self, &to_ro.clone(), Event::StaticReacquire{from : from_ro.to_owned(), is: to_ro.clone(), id: id}, line_number);
                }
                maybe_append_event(self, &from_ro.clone(), Event::StaticDie{to : to_ro.to_owned(), is: from_ro, id: id}, line_number);
            },
            ExternalEvent::MutableBorrow{from: from_ro, to: to_ro, id, ..} => {
                maybe_append_event(self, &from_ro, Event::MutableLend{to : to_ro.to_owned(), is: from_ro.clone(), id: id}, line_number);
                maybe_append_event(self, &to_ro.clone(), Event::MutableBorrow{from : from_ro.to_owned(), is: to_ro, id: id}, line_number);
                
            },
            ExternalEvent::MutableDie{from: from_ro, to: to_ro, id} => {
                // this catches when MutableDie(s to s*) to avoid duplicate events
                if !from_ro.is_same_underlying(&to_ro) {
                  maybe_append_event(self, &to_ro.clone(), Event::MutableReacquire{from : from_ro.to_owned(), is: to_ro.clone(), id: id}, line_number);
                }
                maybe_append_event(self, &from_ro.clone(), Event::MutableDie{to : to_ro.to_owned(), is: from_ro, id: id}, line_number);
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
                    _ => panic!("not possible")
                }
            },

            // Technically appending individual events for PassByRef events is not necessary since
            // they do not change the state of either variable - however - it's useful to have when 
            ExternalEvent::PassByStaticReference{from: from_ro, to: to_ro, id} => {
                maybe_append_event(self, &from_ro.to_owned(), Event::StaticLend{to : to_ro.to_owned(), is: from_ro.clone(), id: id}, line_number);
                maybe_append_event(self, &to_ro.to_owned(), Event::StaticBorrow{from : from_ro.to_owned(), is: to_ro.clone(), id: id}, line_number);
                maybe_append_event(self, &from_ro, Event::StaticReacquire{from : to_ro.to_owned(), is: from_ro.clone(), id: id}, line_number);
                maybe_append_event(self, &to_ro.clone(), Event::StaticDie{to : from_ro.to_owned(), is: to_ro, id: id}, line_number);
            },
            ExternalEvent::PassByMutableReference{from: from_ro, to: to_ro, id} => {
                maybe_append_event(self, &from_ro.clone(), Event::MutableLend{to : to_ro.to_owned(), is: from_ro.clone(), id: id}, line_number);
                maybe_append_event(self, &to_ro.clone(), Event::MutableBorrow{from : from_ro.to_owned(), is: to_ro.clone(), id: id}, line_number);
                maybe_append_event(self, &from_ro.clone(), Event::MutableReacquire{from : to_ro.to_owned(), is: from_ro.clone(), id: id}, line_number);
                maybe_append_event(self, &to_ro.clone(), Event::MutableDie{to : from_ro.to_owned(), is: to_ro, id: id}, line_number);
            },
            ExternalEvent::InitRefParam{param: ro, id} => {
                maybe_append_event(self, &ResourceTy::Value(ro.clone()), Event::InitRefParam{param : ro.to_owned(), id: id}, line_number);
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
                            "Functions do not go out of scope! We do not expect to see \"{}\" here.",
                            func.name
                        );
                    }
                }
            },
            ExternalEvent::OwnerDropAtReassign{ro, ..} => {
                maybe_append_event(self, &ResourceTy::Value(ro), Event::OwnerDropAtReassign, line_number);
            },
        }
    }
}