/* Event Dot messages: shows up when someone hovers over a dot */

// Add styling to string name with <span>
fn fmt_style(plain: &String) -> String {
    let span_begin = String::from(
        "&lt;span style=&quot;font-family: 'Source Code Pro',
        Consolas, 'Ubuntu Mono', Menlo, 'DejaVu Sans Mono',
        monospace, monospace !important;&quot;&gt;"
    );
    let span_end = "&lt;/span&gt;";

    span_begin + plain + span_end
}

/* The Event dot does not connect to any arrows, we typically follow the following format:
   ... happens
 */

// 1   0 （0 is initialized by let 0 = &1)
//     |
//     *   the star event
// 
// example: 
// fn calculate_length(s: &String) -> usize { // s is a reference to a String
//     s.len()      // a REF_GO_OUT_OF_SCOPE event
// } // Here, s goes out of scope. But because it does not have ownership of what
//   // it refers to, nothing happens.
pub fn event_dot_ref_go_out_out_scope(my_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "{0} goes out of scope",
        my_name_fmt
    )
}

// 0
// |
// *   the star event
//
pub fn event_dot_owner_go_out_out_scope(my_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);

    format!(
        "{0} goes out of scope", //we shouldn't say "the resource is dropped"
        my_name_fmt              //because we don't distinguish if the resource
    )                             //was moved from the variable earlier.
}

// Closure binding goes out of scope. The closure value is dropped,
// which in turn drops each move-captured upvar — `move_capture_count`
// is how many of those there are. Reaches the timeline only when
// the count is at least one (borrow-only and capture-less closures
// take the plain owner-OOS path), so the singular/plural branch
// only needs to handle ≥1.
pub fn event_dot_closure_go_out_of_scope(my_name: &String, move_capture_count: usize) -> String {
    let my_name_fmt = fmt_style(my_name);
    let (noun, verb) = if move_capture_count == 1 {
        ("resource", "is")
    } else {
        ("resources", "are")
    };
    format!(
        "{0} goes out of scope. Its {1} captured {2} {3} dropped.",
        my_name_fmt, move_capture_count, noun, verb
    )
}

// An owned (non-ref) function parameter receives its resource from
// whatever the caller passed. The L-shaped arrow on the param's
// timeline visually anchors that "from outside this scope" origin;
// this is its tooltip and the matching dot tooltip.
pub fn event_dot_owner_init_from_caller(my_name: &String) -> String {
    let my_name_fmt = fmt_style(my_name);
    format!("{0} acquires ownership from the caller", my_name_fmt)
}

// A reference function parameter receives a borrow from the caller.
// Mirror message to `event_dot_owner_init_from_caller` for the ref
// side: makes it clear that the lender lives outside this fn scope.
pub fn event_dot_ref_init_from_caller(my_name: &String, is_mut: bool) -> String {
    let my_name_fmt = fmt_style(my_name);
    if is_mut {
        format!("{0} is a mutable borrow from the caller", my_name_fmt)
    } else {
        format!("{0} is an immutable borrow from the caller", my_name_fmt)
    }
}

// Reassignment drops the previous resource (owned, non-Copy) AND
// acquires a new one — both happen on the same line. The drop dot
// is drawn on top of the regular Acquire dot at the same position,
// so this tooltip is the only one a user can hover; it has to
// communicate both events.
pub fn event_dot_owner_drop_at_reassign(my_name: &String) -> String {
    let my_name_fmt = fmt_style(my_name);
    format!(
        "{0} acquires ownership of a new resource; its previous resource is dropped",
        my_name_fmt
    )
}

//     0
//     
// f1  *   the star event
// example: 
// fn calculate_length(s: &String) -> usize { // here s is initialized to some value
//    /* something happens */
// }
pub fn event_dot_init_param(my_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "{0} is initialized as the function argument",
        my_name_fmt
    )
}


/* The Event dot is the source of an arrow, we typically follow the following format: 
   ... to <Resource Owner 1>
 */

// 1   0
//     |
// o<--*   the star event
// |   |
pub fn event_dot_copy_to(my_name: &String, _target_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "{0}'s resource is copied",
        my_name_fmt
    )
}

// 1   0
//     |
// o<--*   the star event
// |
pub fn event_dot_move_to(my_name: &String, _target_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "{0}'s resource is moved",
        my_name_fmt
    )
}

// def fn(p):
//  p
//  |
//  *-->   the return event
//
pub fn event_dot_move_to_caller(my_name: &String, _target_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "{0}'s resource is moved to the caller",
        my_name_fmt
    )
}

pub fn event_dot_copy_to_caller(my_name: &String, _target_name: &String) -> String {
    let my_name_fmt = fmt_style(my_name);
    format!(
        "{0}'s resource is copied to the caller",
        my_name_fmt
    )
}

// 1   0
//     |
// o<--*   the star event (&)
// |   |
pub fn event_dot_static_lend(my_name: &String, _target_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);

    format!(
        "{0}'s resource is immutably borrowed",
        my_name_fmt
    )
}

// 1   0
//     |
// o<--*   the star event (&mut)
// |
pub fn event_dot_mut_lend(my_name: &String, _target_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);

    format!(
        "{0}'s resource is mutably borrowed",
        my_name_fmt
    )
}

// 0   1
//     |
// o<--o
// |   |
// *-->o   the star event (&)
pub fn event_dot_static_return(my_name: &String, _target_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "{0}'s immutable borrow ends",
        my_name_fmt
    )
}

// 0   1
//     |
// o<--o
// |
// *-->o   the star event (&mut)
pub fn event_dot_mut_return(my_name: &String, _target_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "{0}'s mutable borrow ends",
        my_name_fmt
    )
}

/* The Event dot is the destination of an arrow, we typically follow the following format: 
   ... from <Resource Owner 1>
 */

// 1   0        1   0
// |            |   
// o-->*   or   o-->*     the star event
// |   |            |
pub fn event_dot_acquire(my_name: &String, _target_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);

    format!(
        "{0} acquires ownership of a resource",
        my_name_fmt
    )
}

// Same Acquire event shape, but the destination column holds a Copy
// type (i32, bool, etc. — no heap, no ownership semantics) and the
// source is Anonymous (a literal, an arithmetic expression, or a
// macro-internal expansion). Avoids the misleading "ownership"
// language for primitives. Used for both fresh `let n = 5;` bindings
// and reassignments like `n += 1;`.
pub fn event_dot_acquire_copyable(my_name: &String, _target_name: &String) -> String {
    let my_name_fmt = fmt_style(my_name);

    format!(
        "{0} is bound to a value",
        my_name_fmt
    )
}

// 1   0        1   0
// |            |   
// o-->*   or   o-->*     the star event
// |   |            |
pub fn event_dot_copy_from(my_name: &String, target_name: &String) -> String {
    format!(
        "{0} is initialized by copy from {1}",
        my_name,
        target_name
    )
}

// 0   1
//     |
// *<--o   the star event (&mut)
// |   |
pub fn event_dot_mut_borrow(my_name: &String, _target_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "{0} mutably borrows a resource",
        my_name_fmt
    )
}

// 0   1
//     |
// *<--o   the star event (&)
// |   |
pub fn event_dot_static_borrow(my_name: &String, _target_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "{0} immutably borrows a resource",
        my_name_fmt
    )
}

// ─── Closure capture variants ────────────────────────────────────
//
// Used when the `to`/`is` of an event is a closure binding. Wording
// emphasizes that the move/borrow is happening because the closure
// captured the upvar, not because the user wrote an explicit `let
// y = x` / `let r = &x`.

// Source dot for a `move`-captured upvar. Mirrors event_dot_move_to.
pub fn event_dot_capture_move_to_closure(my_name: &String, target_name: &String) -> String {
    let my_name_fmt = fmt_style(my_name);
    let target_fmt = fmt_style(target_name);
    format!(
        "{0}'s resource is captured (moved) by closure {1}",
        my_name_fmt, target_fmt
    )
}

// Source dot for an immutably captured upvar. Mirrors event_dot_static_lend.
pub fn event_dot_capture_static_lend_to_closure(my_name: &String, target_name: &String) -> String {
    let my_name_fmt = fmt_style(my_name);
    let target_fmt = fmt_style(target_name);
    format!(
        "{0}'s resource is captured (immutably borrowed) by closure {1}",
        my_name_fmt, target_fmt
    )
}

// Source dot for a mutably captured upvar. Mirrors event_dot_mut_lend.
pub fn event_dot_capture_mut_lend_to_closure(my_name: &String, target_name: &String) -> String {
    let my_name_fmt = fmt_style(my_name);
    let target_fmt = fmt_style(target_name);
    format!(
        "{0}'s resource is captured (mutably borrowed) by closure {1}",
        my_name_fmt, target_fmt
    )
}

// Combined tooltip for the closure binding's Bind-Acquire dot
// when one or more upvars are captured at the closure literal.
// `captures` lists (upvar_name, capture_kind_label) pairs in the
// order they were emitted by expr_visitor's Closure arm. Used in
// place of the per-capture closure-side dots, which would
// otherwise stack on the same (x,y) and mask each other.
pub fn event_dot_closure_bind_with_captures(
    my_name: &String,
    captures: &[(String, &'static str)],
) -> String {
    let my_name_fmt = fmt_style(my_name);
    if captures.is_empty() {
        // Closure with no captures (e.g. `let f = || println!("hi")`).
        return format!("Closure {0} is bound", my_name_fmt);
    }
    let parts: Vec<String> = captures
        .iter()
        .map(|(name, kind)| format!("{} ({})", fmt_style(name), kind))
        .collect();
    format!(
        "Closure {0} captures: {1}",
        my_name_fmt,
        parts.join(", ")
    )
}

// Closure-side dot when a `move`-captured upvar lands on `f`'s
// timeline. Mirrors event_dot_acquire.
pub fn event_dot_closure_capture_acquire(my_name: &String, from_name: &String) -> String {
    let my_name_fmt = fmt_style(my_name);
    let from_fmt = fmt_style(from_name);
    format!(
        "Closure {0} captures (moves) {1}'s resource",
        my_name_fmt, from_fmt
    )
}

// Closure-side dot for an immutable capture. Mirrors event_dot_static_borrow.
pub fn event_dot_closure_capture_static_borrow(my_name: &String, from_name: &String) -> String {
    let my_name_fmt = fmt_style(my_name);
    let from_fmt = fmt_style(from_name);
    format!(
        "Closure {0} captures an immutable reference to {1}",
        my_name_fmt, from_fmt
    )
}

// Closure-side dot for a mutable capture. Mirrors event_dot_mut_borrow.
pub fn event_dot_closure_capture_mut_borrow(my_name: &String, from_name: &String) -> String {
    let my_name_fmt = fmt_style(my_name);
    let from_fmt = fmt_style(from_name);
    format!(
        "Closure {0} captures a mutable reference to {1}",
        my_name_fmt, from_fmt
    )
}

// 1   0
//     |
// o<--o
// |   |
// o-->*   the star event (&)
pub fn event_dot_static_reacquire(my_name: &String, _target_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "{0}'s resource is no longer immutably borrowed",
        my_name_fmt
    )
}

// 1   0
//     |
// o<--o
// |
// o-->*   the star event (&mut)
pub fn event_dot_mut_reacquire(my_name: &String, _target_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "{0}'s resource is no longer mutably borrowed",
        my_name_fmt
    )
}



/* Arrow messages: shows up when someone hovers over an arrow */

// 1   0
//     |
// o<--o
// |
pub fn arrow_move_val_to_val(from_name: &String, to_name: &String) -> String {
    // update styling
    let from_name_fmt = fmt_style(from_name);
    let to_name_fmt = fmt_style(to_name);
    
    format!(
        "{0}'s resource is moved to {1}",
        from_name_fmt,
        to_name_fmt
    )
}

// 1   0
//     |
// o<--o
// |   |
pub fn arrow_copy_val_to_val(from_name: &String, to_name: &String) -> String {
    // update styling
    let from_name_fmt = fmt_style(from_name);
    let to_name_fmt = fmt_style(to_name);
    
    format!(
        "{0}'s resource is copied to {1}",
        from_name_fmt,
        to_name_fmt
    )
}

// f1  0
//     |
// o<--o
// |
pub fn arrow_move_val_to_func(from_name: &String, to_name: &String) -> String {
    // update styling
    let from_name_fmt = fmt_style(from_name);
    let to_name_fmt = fmt_style(to_name);
    
    format!(
        "{0}'s resource is moved to function {1}",
        from_name_fmt,
        to_name_fmt
    )
}

// f1  0
//     |
// o<--o
// |   |
pub fn arrow_copy_val_to_func(from_name: &String, to_name: &String) -> String {
    // update styling
    let from_name_fmt = fmt_style(from_name);
    let to_name_fmt = fmt_style(to_name);
    
    format!(
        "{0}'s resource is copied to function {1}",
        from_name_fmt,
        to_name_fmt
    )
}

// 1  f0
//     |
// o<--o
// |
pub fn arrow_move_func_to_val(from_name: &String, to_name: &String) -> String {
    // update styling
    let from_name_fmt = fmt_style(from_name);
    let to_name_fmt = fmt_style(to_name);
    
    format!(
        "Function {0}'s resource is moved to {1}",
        from_name_fmt,
        to_name_fmt
    )
}

// 1   0
//     |
// o<--o
// |   |
pub fn arrow_static_lend_val_to_val(from_name: &String, to_name: &String) -> String {
    // update styling
    let from_name_fmt = fmt_style(from_name);
    let to_name_fmt = fmt_style(to_name);
    
    format!(
        "{0}'s resource is immutably borrowed by {1}",
        from_name_fmt,
        to_name_fmt
    )
}

// 1   0
//     |
// o<->o
//     |
pub fn arrow_static_lend_val_to_func(from_name: &String, to_name: &String) -> String {
    // update styling
    let from_name_fmt = fmt_style(from_name);
    let to_name_fmt = fmt_style(to_name);
    
    format!(
        "{0}'s resource is immutably borrowed by function {1}",
        from_name_fmt,
        to_name_fmt
    )
}

// 1   0
//     |
// o<--o
// |
pub fn arrow_mut_lend_val_to_val(from_name: &String, to_name: &String) -> String {
    // update styling
    let from_name_fmt = fmt_style(from_name);
    let to_name_fmt = fmt_style(to_name);
    
    format!(
        "{0}'s resource is mutably borrowed by {1}",
        from_name_fmt,
        to_name_fmt
    )
}

// 1   0
//     |
// o<->o
//     |
pub fn arrow_mut_lend_val_to_func(from_name: &String, to_name: &String) -> String {
    // update styling
    let from_name_fmt = fmt_style(from_name);
    let to_name_fmt = fmt_style(to_name);
    
    format!(
        "{0}'s resource is mutably borrowed by function {1}",
        from_name_fmt,
        to_name_fmt
    )
}

// 0   1
//     |
// o<--o
// |   |
// o-->o   this arrow (&)
pub fn arrow_static_return(from_name: &String, to_name: &String) -> String {
    // update styling
    let from_name_fmt = fmt_style(from_name);
    let to_name_fmt = fmt_style(to_name);
    
    format!(
        "{0}'s immutable borrow of {1}'s resource ends",
        from_name_fmt,
        to_name_fmt
    )
}

// 0   1
//     |
// o<--o
// |
// o-->o   the star event (&mut)
pub fn arrow_mut_return(from_name: &String, to_name: &String) -> String {
    // update styling
    let from_name_fmt = fmt_style(from_name);
    let to_name_fmt = fmt_style(to_name);
    
    format!(
        "{0}'s mutable borrow of {1}'s resource ends",
        from_name_fmt,
        to_name_fmt
    )
}



/* State messages: shows up on the vertical lines for every value/reference */

// The viable is no longer in the scope after this line.
pub fn state_out_of_scope(my_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "{0} is out of scope",
        my_name_fmt
    )
}

// The resource is transferred on this line or before this line due to move,
// thus it is impossible to access this variable anymore. This is an invisible line in the timeline.
pub fn state_resource_moved(my_name: &String, _to_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "{0}'s resource was moved, so {0} no longer has ownership",
        my_name_fmt
    )
}

// temporarily no read or write access right to the resource, but eventually
// the privilege will come back. Occurs when mutably borrowed. This is an invisible line in the timeline.
pub fn state_resource_revoked(my_name: &String, _to_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "{0}'s resource is mutably borrowed, so it cannot access the resource",
        my_name_fmt,
    )
}

// This ResourceOwner is the unique object that holds the ownership to the underlying resource.
pub fn state_full_privilege(my_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);

    format!(
        "{0} is the owner of the resource", //not necessarily write if let was used rather than let mut
        my_name_fmt
    )
}

// FullPrivilege but the owner is a Copy type (i32 etc.), so the
// "owner of the resource" framing doesn't fit — primitives are
// values, not heap-backed resources. Used for the timeline-stripe
// tooltip and the vertical lifeline.
pub fn state_full_privilege_copyable(my_name: &String) -> String {
    let my_name_fmt = fmt_style(my_name);

    format!(
        "{0} holds a value",
        my_name_fmt
    )
}

// Closure binding's FullPrivilege state. Two flavours so the
// timeline tooltip distinguishes a `move ||` (closure owns the
// captured resources — drop runs at scope-end) from a borrow-only
// closure (captures a reference; nothing of the resource is owned
// by the closure value itself). The move-flavoured variant takes
// the count of move-captured upvars so the tooltip is honest
// about whether the closure owns one or many resources.
pub fn state_closure_full_privilege_with_resource(my_name: &String, move_capture_count: usize) -> String {
    let my_name_fmt = fmt_style(my_name);
    let noun = if move_capture_count == 1 { "resource" } else { "resources" };
    format!(
        "{0} owns a closure which owns {1} {2} via capture",
        my_name_fmt, move_capture_count, noun
    )
}

pub fn state_closure_full_privilege_no_resource(my_name: &String) -> String {
    let my_name_fmt = fmt_style(my_name);
    format!("{0} owns a closure", my_name_fmt)
}

// More than one ResourceOwner has access to the underlying resource
// This means that it is not possible to create a mutable reference
// on the next line.
// About borrow_count: this value is at least one at any time.
//      When the first static reference of this ResourceOwner is created,
//          this value is set to 1;
//      When a new static reference is borrowed from this variable, increment by 1;
//      When a static reference goes out of scope, decrement this value by 1;
//      When a decrement happens while the borrow_count is 1, the state becomes
//          FullPrivilege once again.
pub fn state_partial_privilege(my_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "{0}'s resource is being shared by one or more variables",
        my_name_fmt
    )
}

// should not appear for visualization in a correct program
pub fn state_invalid(my_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "something is wrong with the timeline of {0}",
        my_name_fmt
    )
}

pub fn structure(my_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);

    format!(
        "the components in the box belong to struct {0}",
        my_name_fmt
    )
}

// ── Conditional join-point messages ────────────────────────────────
//
// Tooltip on the per-variable merge dot at the bottom of an `if` /
// `match` / `if let`. Says what happened to the variable across the
// branches, so the reader doesn't have to scan each branch's history
// to figure out whether the resource is still owned. Wording follows
// Rust's actual rule: "moved in any branch above" → unusable here,
// no matter how many branches actually moved it. Resource ownership
// at the join is binary (owned vs. not), and the messaging tracks
// that, not the raw count.

/// Variable was moved (consumed without being rebound) in at least
/// one branch above. After the conditional Rust treats it as
/// possibly-moved → can't be used.
pub fn event_dot_branch_merge_moved(my_name: &String) -> String {
    let my_name_fmt = fmt_style(my_name);
    format!(
        "{0} may have been moved (consumed in at least one branch above)",
        my_name_fmt
    )
}

/// Every branch above ended without the resource — either by a
/// direct move/consume or because a nested merge already ended in
/// an implicit-drop state. Distinct from the "may have been moved"
/// wording: there's no maybe here, the variable definitely doesn't
/// own anything after the conditional.
pub fn event_dot_branch_merge_all_moved(my_name: &String) -> String {
    let my_name_fmt = fmt_style(my_name);
    format!(
        "{0} was moved or dropped in every branch above",
        my_name_fmt
    )
}

/// Every branch above contributed a value that ends up in this
/// variable (the `let s = if … { … } else { … };` shape, plus the
/// match-as-rhs analog). After the conditional the variable owns a
/// freshly bound resource regardless of which branch ran.
pub fn event_dot_branch_merge_bound(my_name: &String) -> String {
    let my_name_fmt = fmt_style(my_name);
    format!(
        "{0} acquired ownership of a resource (in all branches above)",
        my_name_fmt
    )
}

/// Some-moved-some-alive case: at least one branch consumed the
/// variable, at least one didn't. Rust treats the variable as
/// possibly-moved after the conditional (so it can't be used) and
/// inserts an *implicit drop* at the end of every branch where the
/// variable wasn't moved — this keeps the merged state consistent
/// across branches. The drop dot at the join makes that semantics
/// visible; the tooltip names it.
pub fn event_dot_branch_merge_moved_with_drop(my_name: &String) -> String {
    let my_name_fmt = fmt_style(my_name);
    format!(
        "{0} was moved in at least one branch above; \
         in branches that didn't, its resource is dropped at the branch's end.",
        my_name_fmt
    )
}