use crate::svg_generator::data::ResourceAccessPoint;


/* Event Dot messages: shows up when someone hovers over a dot */

use crate::svg_generator::data::StaticLender;
/// Escape a name (`s`, `p.x`, or an operator such as `<` or `&`) for a
/// tooltip. A tooltip is HTML stored in an SVG attribute that is written
/// without escaping (the span markup around names is already in its
/// escaped form, `&lt;span …&gt;`), so a name needs two levels: `<`
/// becomes `&lt;` in the HTML, which is `&amp;lt;` in the attribute. A
/// bare `<` or `&` made the SVG invalid XML (#376).
pub fn escape_tooltip_name(name: &str) -> String {
    name.replace('&', "&amp;amp;")
        .replace('<', "&amp;lt;")
        .replace('>', "&amp;gt;")
        .replace('"', "&amp;quot;")
}

// Add styling to string name with <span>
fn fmt_style(plain: &String) -> String {
    let span_begin = String::from(
        "&lt;span style=&quot;font-family: 'Source Code Pro',
        Consolas, 'Ubuntu Mono', Menlo, 'DejaVu Sans Mono',
        monospace, monospace !important;&quot;&gt;"
    );
    let span_end = "&lt;/span&gt;";

    span_begin + &escape_tooltip_name(plain) + span_end
}

// What a column holds, as named in tooltips (#274). A Copy type (`i32`,
// `bool`, a `#[derive(Clone, Copy)]` struct) can't implement `Drop`
// (rustc rejects `Copy` + `Drop`, E0184), so it has nothing to clean up
// and nothing "owned" in the drop sense: its columns say "holds a
// value", and the copy / borrow tooltips on them say "value" too.
// Everything else keeps RustViz's "resource".
fn held(is_copy: bool) -> &'static str {
    if is_copy { "value" } else { "resource" }
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

// A struct whose fields were partly moved out goes out of scope. Rust
// drops only the fields that still hold a value (`dropped`); the
// moved-out ones (`moved`) are already gone, and the struct as a whole
// isn't dropped because it no longer holds a complete value. `moved` is
// never empty; `dropped` lists only non-Copy fields (a Copy field has
// nothing to drop).
pub fn event_dot_struct_partial_go_out_of_scope(
    my_name: &String,
    moved: &[String],
    dropped: &[String],
) -> String {
    fn join(names: &[String]) -> (String, bool) {
        let fmt: Vec<String> = names.iter().map(fmt_style).collect();
        let s = match fmt.len() {
            0 | 1 => fmt.concat(),
            n => format!("{} and {}", fmt[..n - 1].join(", "), fmt[n - 1]),
        };
        (s, names.len() > 1)
    }
    let (moved_s, moved_plural) = join(moved);
    let rest = if dropped.is_empty() {
        String::from("no resource is dropped")
    } else {
        let (dropped_s, dropped_plural) = join(dropped);
        format!("only {} {} dropped", dropped_s, if dropped_plural { "are" } else { "is" })
    };
    format!(
        "{0} goes out of scope. {1} {2} moved out earlier, so {3}.",
        fmt_style(my_name),
        moved_s,
        if moved_plural { "were" } else { "was" },
        rest
    )
}

// A union goes out of scope (#191). Rust never drops a union's fields:
// it can't know which field holds a value, so whatever is in a
// `ManuallyDrop` field is left alone (leaked unless dropped by hand).
// True even for a union with its own `Drop` impl, which runs but still
// doesn't drop the fields.
pub fn event_dot_union_go_out_of_scope(my_name: &String) -> String {
    format!(
        "{0} goes out of scope. A union's fields are never dropped automatically.",
        fmt_style(my_name)
    )
}

// A `ManuallyDrop<T>` goes out of scope (#283). It never drops the `T`
// inside it: that only happens through `ManuallyDrop::drop` /
// `into_inner` / `take`, and otherwise the value is leaked.
pub fn event_dot_manually_drop_go_out_of_scope(my_name: &String) -> String {
    format!(
        "{0} goes out of scope. Nothing is dropped: a ManuallyDrop never drops the value inside it automatically.",
        fmt_style(my_name)
    )
}

// `ManuallyDrop::drop(&mut md)`: the value inside `md` is dropped here,
// by hand.
pub fn event_dot_manually_drop_by_hand(my_name: &String) -> String {
    format!(
        "ManuallyDrop::drop drops the value inside {0} (by hand)",
        fmt_style(my_name)
    )
}

// State of a `ManuallyDrop` after `ManuallyDrop::drop`: it holds no
// resource, and using it again is undefined behaviour.
pub fn state_dropped_by_hand(my_name: &String) -> String {
    format!(
        "{0}'s value was dropped by hand; it must not be used again",
        fmt_style(my_name)
    )
}

// A union that already holds a value is written to again (#191):
// `u.a = 5;`, `u.a = y;`, `u = U { .. };`. Nothing is acquired and the
// old contents aren't dropped. `from` names the variable the new
// value was copied / moved from, if any.
pub fn event_dot_union_write(my_name: &String, from: Option<(&String, bool)>) -> String {
    let source = match from {
        Some((name, true)) => format!(" (copied from {})", fmt_style(name)),
        Some((name, false)) => format!(" (moved from {})", fmt_style(name)),
        None => String::new(),
    };
    format!(
        "{0} is written to{1}; writing to a union never drops its old contents",
        fmt_style(my_name),
        source
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
// A `ref` / `ref mut` binding of a by-value parameter (`fn f(ref s:
// String)`, `fn g((ref a, b): (String, i32))`). The caller's value was
// moved (or copied) into `f`, which owns it; the binding borrows that,
// so it mustn't say "from the caller" (#377).
pub fn event_dot_ref_init_own_param(my_name: &String, fn_name: &String, whole: bool, is_mut: bool, drops: bool) -> String {
    let my_name_fmt = fmt_style(my_name);
    let fn_fmt = fmt_style(fn_name);
    let kind = if is_mut { "a mutable" } else { "an immutable" };
    let what = if whole { "the value" } else { "part of the value" };
    let base = format!("{0} is {1} borrow of {2} passed to {3}, which {3} owns", my_name_fmt, kind, what, fn_fmt);
    // The borrowed value has no column, so its drop (with the
    // parameter, when `fn_name` returns) is stated here.
    if drops {
        format!("{}; {} is dropped when {} returns", base, if whole { "that value" } else { "that part" }, fn_fmt)
    } else {
        base
    }
}

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
// Only Copy types are copied, so this always says "value" (#274); a
// `&T` being copied has its own message (event_dot_ref_copied).
pub fn event_dot_copy_to(my_name: &String, _target_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "{0}'s value is copied",
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

// Source dot for a by-value capture of a closure literal passed to a
// call (`thread::spawn(move || ..v..)`). The resource goes into the
// closure, and is dropped when the closure is, not when the call returns.
pub fn event_dot_move_into_closure_arg(my_name: &String, callee: &String) -> String {
    format!(
        "{0}'s resource is moved into a closure passed to {1}; it's dropped when that closure is dropped",
        fmt_style(my_name),
        fmt_style(callee)
    )
}

pub fn event_dot_copy_into_closure_arg(my_name: &String, callee: &String) -> String {
    format!(
        "{0}'s value is copied into a closure passed to {1}",
        fmt_style(my_name),
        fmt_style(callee)
    )
}

// `drop(s);`: the value is moved into `std::mem::drop`, which drops it.
pub fn event_dot_move_to_drop(my_name: &String, fn_name: &String) -> String {
    format!(
        "{0}'s resource is moved into {1}, which drops it",
        fmt_style(my_name),
        fmt_style(fn_name)
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
        "{0}'s value is copied to the caller",
        my_name_fmt
    )
}

/// Returning a shared reference (`&T` is Copy) copies the reference,
/// never the data behind it, so don't say "s's resource is copied".
pub fn event_dot_copy_ref_to_caller(my_name: &String, _target_name: &String) -> String {
    let my_name_fmt = fmt_style(my_name);
    format!(
        "a reference from {0} is returned to the caller; the data it points to is not copied",
        my_name_fmt
    )
}

// A reference variable is itself moved (`let r2 = r;` with r: `&mut T`,
// or returned) or copied (`&T` is Copy). Only the reference moves or is
// copied; the resource it points to stays where it is, still owned by
// its lender (#178).
pub fn event_dot_ref_moved(my_name: &String, to_caller: bool) -> String {
    format!(
        "{0}'s mutable reference is moved{1} (not the resource it points to)",
        fmt_style(my_name),
        if to_caller { " to the caller" } else { "" }
    )
}

pub fn event_dot_ref_copied(my_name: &String) -> String {
    format!(
        "{0}'s reference is copied (not the resource it points to)",
        fmt_style(my_name)
    )
}

// 1   0
//     |
// o<--*   the star event (&)
// |   |
//
// Source-side dot at the start of an immutable borrow. Names the
// borrower so the tooltip ties back to the column the data is
// flowing into. `&T` doesn't exclude further `&T`s but does prevent
// mutating or moving X while the loan is live. The restriction is
// phrased as "while the borrow is live" rather than "until Y's last
// use": the loan can outlive Y (`let z = y;` copies/moves it on, and
// for a function Y a returned reference can carry it past the call).
//
// The clause is omitted (`omit_read_only`) when X is itself a shared
// reference (`&*r`, `&r`, `show(r)` with r: `&T`): X was already
// read-only, so the borrow adds no restriction worth teaching. It is
// also omitted when X has interior mutability (`c.borrow_mut()` on a
// `RefCell`, `cell.set(..)`, `m.lock()`): those methods take `&self`
// and mutate X through the shared borrow, so "read-only" would be
// false.
// `is_copy`: X is a Copy-typed owner, so its "value" is borrowed (#274).
pub fn event_dot_static_lend(my_name: &String, target_name: &String, omit_read_only: bool, is_copy: bool) -> String {
    let my_name_fmt = fmt_style(my_name);
    let target_name_fmt = fmt_style(target_name);
    if omit_read_only {
        format!(
            "{0}'s {2} is immutably borrowed by {1}",
            my_name_fmt, target_name_fmt, held(is_copy)
        )
    } else {
        format!(
            "{0}'s {2} is immutably borrowed by {1}; {0} is read-only while the borrow is live",
            my_name_fmt, target_name_fmt, held(is_copy)
        )
    }
}

// 1   0
//     |
// o<--*   the star event (&mut)
// |
//
// Source-side dot at the start of a mutable borrow. The freeze
// note is the pedagogical payload — `&mut T` is exclusive, so X
// is unusable (not just read-only) while the loan is live. Same
// "while the borrow is live" phrasing as `event_dot_static_lend`,
// for the same reason, so a reader can spot the read-only-vs-unusable
// distinction by hovering both kinds.
pub fn event_dot_mut_lend(my_name: &String, target_name: &String, is_copy: bool) -> String {
    let my_name_fmt = fmt_style(my_name);
    let target_name_fmt = fmt_style(target_name);
    format!(
        "{0}'s {2} is mutably borrowed by {1}; {0} can't be used while the borrow is live",
        my_name_fmt, target_name_fmt, held(is_copy)
    )
}

// Lender dot when the lender r is itself a reference (#313): `let r2 =
// &mut *r;`, `let r3 = &*r;`, `r.push('a')`, `add(r)`. r owns nothing,
// so what's borrowed is what r points to (r's own lender's data),
// reached through r. While a mutable borrow through r is live r can't
// be used; during a shared one a `&mut` r is read-only. A `&T` r can't
// hand out mutable access to its referent, so a mutable borrow from it
// (`&mut r` on a `let mut r: &T`) borrows the reference r itself.
// `pointee_copy`: r points to a Copy type, so it's a "value" (#312).
pub fn event_dot_lend_through_ref(ref_name: &String, target_name: &String, mutable: bool, ref_is_mut: bool, pointee_copy: bool) -> String {
    let r = fmt_style(ref_name);
    let t = fmt_style(target_name);
    let noun = held(pointee_copy);
    match (mutable, ref_is_mut) {
        (true, true) => format!(
            "the {2} {0} points to is mutably borrowed by {1} through {0}; {0} can't be used while the borrow is live",
            r, t, noun),
        (true, false) => format!(
            "{0} is mutably borrowed by {1}; {0} can't be used while the borrow is live",
            r, t),
        (false, true) => format!(
            "the {2} {0} points to is immutably borrowed by {1} through {0}; {0} is read-only while the borrow is live",
            r, t, noun),
        (false, false) => format!(
            "the {2} {0} points to is immutably borrowed by {1}",
            r, t, noun),
    }
}

// Lender dot where a borrow through the reference r ends (#313); the
// counterpart of event_dot_lend_through_ref. `borrower` is the
// reference / function / closure whose borrow ended, when known.
// "the resource r points to is no longer borrowed" alone would be
// false: r's own borrow of it is still live. No "r can be used
// again": a call can consume r instead (`take(r)` with a generic
// parameter moves it; #200), and the borrow can live on in the call's
// result (`let w = first(r);`).
pub fn event_dot_reacquire_through_ref(ref_name: &String, borrower: Option<&String>, mutable: bool, ref_is_mut: bool, pointee_copy: bool) -> String {
    let r = fmt_style(ref_name);
    let noun = held(pointee_copy);
    if mutable && !ref_is_mut {
        return format!("{0} is no longer mutably borrowed", r);
    }
    match borrower {
        Some(b) => format!("{1} no longer borrows the {2} {0} points to", r, fmt_style(b), noun),
        None => format!("the {1} {0} points to is no longer borrowed through {0}", r, noun),
    }
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
        "{0} is bound to a copyable value",
        my_name_fmt
    )
}

// A Copy-typed place that already holds a value is assigned again
// (`n += 1;`, `p.x = 5;`, `n = m;`). Nothing is acquired or
// initialized: the old value is overwritten in place (#178).
pub fn event_dot_copyable_assign(my_name: &String) -> String {
    format!("{0} is assigned a new value", fmt_style(my_name))
}

pub fn event_dot_copyable_assign_copy(my_name: &String, from_name: &String) -> String {
    format!(
        "{0} is assigned a copy of {1}",
        fmt_style(my_name),
        fmt_style(from_name)
    )
}

// A reference variable that already holds a reference is assigned
// another one (#414): r was initialized earlier, so this is not an
// initialization. `from` is where the new value comes from, `copied`
// whether it was copied (`&T` is Copy) or moved (`&mut T`).
// - `r = s;`, s: `&T`: the reference in s is copied; what it points
//   to isn't (the counterpart of "s's reference is copied (not the
//   resource it points to)" on s's column).
// - `r = s;`, s: `&mut T`: the reference is moved out of s.
// - `r = first(..);`: the call's result. Not "the reference first
//   returns": while shadowing bindings share a column (#173), the
//   result can be a new binding's non-reference value
//   (`let spaces = "  "; let spaces = spaces.len();`).
// None for any other source; the caller keeps its usual wording.
pub fn event_dot_ref_assign(my_name: &String, from: &ResourceAccessPoint, copied: bool, is_mut: bool) -> Option<String> {
    let r = fmt_style(my_name);
    let f = fmt_style(from.name());
    let kind = if is_mut { "mutable" } else { "immutable" };
    if from.is_fn() {
        Some(format!("{0} is assigned the result of {1}", r, f))
    } else if from.is_ref() && copied {
        Some(format!("{0} is assigned a copy of {1}'s reference", r, f))
    } else if from.is_ref() {
        Some(format!("{0} is assigned {1}'s {2} reference (moved out of {1})", r, f, kind))
    } else {
        None
    }
}

// Acquire whose destination column is a reference variable
// (`let s: &str = "hi";`, `let r2 = r;` for r: &mut T). A
// reference never owns what it points to, so "acquires ownership"
// would be false; say what the variable is bound to instead.
pub fn event_dot_ref_acquire(my_name: &String, is_mut: bool) -> String {
    let my_name_fmt = fmt_style(my_name);
    if is_mut {
        format!("{0} is bound to a mutable reference", my_name_fmt)
    } else {
        format!("{0} is bound to an immutable reference", my_name_fmt)
    }
}

// Write through a dereference: `*r = v;`, `*r += 1;`. The place
// `*r` is the resource r points to (owned by r's lender, or by the
// Box), so nothing is acquired; the existing resource is modified.
// `ref_name` is r, `deref_name` is *r. `pointee_copy`: r is a
// reference to a Copy type (`&mut i32`), so what it points to is a
// "value", as on the lender's own column (#312).
pub fn event_dot_write_through_deref(ref_name: &String, deref_name: &String, pointee_copy: bool) -> String {
    format!(
        "the {2} {0} points to is modified through {1}",
        fmt_style(ref_name),
        fmt_style(deref_name),
        held(pointee_copy)
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
// `lender_is_copy`: the lender is a Copy-typed owner (#274).
pub fn event_dot_mut_borrow(my_name: &String, _target_name: &String, lender_is_copy: bool) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "{0} mutably borrows a {1}",
        my_name_fmt, held(lender_is_copy)
    )
}

// 0   1
//     |
// *<--o   the star event (&)
// |   |
pub fn event_dot_static_borrow(my_name: &String, _target_name: &String, lender_is_copy: bool) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "{0} immutably borrows a {1}",
        my_name_fmt, held(lender_is_copy)
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

// Source dot for a Copy upvar captured by value (`move || m + 1`
// with m: i32; #311). The closure gets its own copy, so the upvar
// stays usable and the closure has nothing of it to drop. A `&T` is
// Copy too: the closure gets a copy of the reference, and the data it
// points to isn't copied.
pub fn event_dot_capture_copy_to_closure(my_name: &String, target_name: &String, is_ref: bool) -> String {
    let my_name_fmt = fmt_style(my_name);
    let target_fmt = fmt_style(target_name);
    if is_ref {
        format!(
            "{0}'s reference is captured (copied) by closure {1}; the data it points to is not copied",
            my_name_fmt, target_fmt
        )
    } else {
        format!(
            "{0}'s value is captured (copied) by closure {1}",
            my_name_fmt, target_fmt
        )
    }
}

// Source dot for an immutably captured upvar. Mirrors event_dot_static_lend.
pub fn event_dot_capture_static_lend_to_closure(my_name: &String, target_name: &String, is_copy: bool) -> String {
    let my_name_fmt = fmt_style(my_name);
    let target_fmt = fmt_style(target_name);
    format!(
        "{0}'s {2} is captured (immutably borrowed) by closure {1}",
        my_name_fmt, target_fmt, held(is_copy)
    )
}

// Source dot for a mutably captured upvar. Mirrors event_dot_mut_lend.
pub fn event_dot_capture_mut_lend_to_closure(my_name: &String, target_name: &String, is_copy: bool) -> String {
    let my_name_fmt = fmt_style(my_name);
    let target_fmt = fmt_style(target_name);
    format!(
        "{0}'s {2} is captured (mutably borrowed) by closure {1}",
        my_name_fmt, target_fmt, held(is_copy)
    )
}

// Source dot on a reference r when a closure captures `*r` through it
// (`|| r.push('a')`, `|| r.len()`; #277): the closure reborrows what r
// points to. r owns nothing, so "r's resource is captured" was wrong.
// A `&mut` r can't be used while a mutable reborrow is live and is
// read-only during a shared one; a `&T` r was read-only already.
pub fn event_dot_capture_reborrow_through_ref(ref_name: &String, closure_name: &String, mutable: bool, ref_is_mut: bool) -> String {
    let base = format!(
        "the resource {0} points to is captured by closure {1} ({2} reborrow through {0})",
        fmt_style(ref_name),
        fmt_style(closure_name),
        if mutable { "a mutable" } else { "an immutable" }
    );
    if mutable {
        format!("{0}; {1} can't be used while the borrow is live", base, fmt_style(ref_name))
    } else if ref_is_mut {
        format!("{0}; {1} is read-only while the borrow is live", base, fmt_style(ref_name))
    } else {
        base
    }
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
pub fn event_dot_static_reacquire(my_name: &String, _target_name: &String, is_copy: bool) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "{0}'s {1} is no longer immutably borrowed",
        my_name_fmt, held(is_copy)
    )
}

// 1   0
//     |
// o<--o
// |
// o-->*   the star event (&mut)
pub fn event_dot_mut_reacquire(my_name: &String, _target_name: &String, is_copy: bool) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "{0}'s {1} is no longer mutably borrowed",
        my_name_fmt, held(is_copy)
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



// The end of a borrow whose lender has no column (a string literal
// `let s: &str = "hi";`, or a reference returned by a function call
// that isn't linked to its argument). The arrow used to read "Return
// immutably borrowed resource from s to *s", a borrow returned to
// itself (#178).
pub fn arrow_borrow_ends_lender_not_drawn(from_name: &String, is_mut: bool) -> String {
    format!(
        "{0}'s {1} borrow ends (what it borrows from isn't shown)",
        fmt_style(from_name),
        if is_mut { "mutable" } else { "immutable" }
    )
}

// The end of a borrow whose lender has no column but is known to be
// data that lives for the whole program: a string literal (`let s:
// &str = "hi";`), a static (`let g = &GREETING;`), or what a static /
// const of reference type points to (`let g = GREETING;`). The mutable
// form arises for `&mut` to a `static mut`.
pub fn arrow_borrow_of_static_ends(from_name: &String, is_mut: bool, lender: &StaticLender) -> String {
    let what = match lender {
        StaticLender::Literal { noun } =>
            format!("the {0} ends ({0}s live for the whole program)", noun),
        StaticLender::Static(name) =>
            format!("the static {} ends (statics live for the whole program)", fmt_style(name)),
        StaticLender::PointeeOfStatic(name) =>
            format!("what the static {} points to ends (it lives for the whole program)", fmt_style(name)),
        StaticLender::PointeeOfConst(name) =>
            format!("what the constant {} points to ends (it lives for the whole program)", fmt_style(name)),
        StaticLender::PromotedConst(name) =>
            format!("the constant {} ends (Rust stores the borrowed value for the whole program)", fmt_style(name)),
    };
    format!(
        "{0}'s {1}borrow of {2}",
        fmt_style(from_name),
        if is_mut { "mutable " } else { "" },
        what
    )
}

// Call dot for a `&self` method whose receiver has interior mutability
// (`Cell`, `RefCell`, `Mutex`, atomics, `Rc<RefCell<T>>`; #203). At
// compile time this is an ordinary shared borrow: the borrow checker has
// no special case for these types (they're built on `UnsafeCell`).
// What's different is that the value inside can change through it, so
// "reads from" would be false for `c.set(2)`. For `RefCell::borrow_mut`
// the exclusivity a `&mut` would give is checked at runtime instead.
pub fn fn_dot_shared_borrow_interior_mut(fn_name: &String, my_name: &String, refcell_borrow_mut: bool) -> String {
    let f = fmt_style(fn_name);
    let x = fmt_style(my_name);
    if refcell_borrow_mut {
        format!(
            "{0} borrows {1} immutably (a shared borrow); exclusive access to its contents is checked at runtime (a second {0} while this one is live would panic)",
            f, x
        )
    } else {
        format!(
            "{0} borrows {1} immutably (a shared borrow); {1} has interior mutability, so its contents can change even through a shared borrow",
            f, x
        )
    }
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

// `state_resource_moved` for a reference variable whose reference was
// moved out (`let q2 = q;` with q: `&mut T`). A reference never had
// ownership, so there is none to lose; what's gone is the reference
// itself. Only shown for `let mut` bindings (the segment isn't drawn
// otherwise), which can be assigned a new reference (#292).
pub fn state_ref_moved(my_name: &String, is_mut: bool) -> String {
    format!(
        "{0}'s {1} was moved out, so {0} can't be used until it's assigned again",
        fmt_style(my_name),
        if is_mut { "mutable reference" } else { "reference" }
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

// Dashed segment of a variable that has no value yet on this path:
// before a `let s;` is assigned in a branch, or before the branch of
// a `let s = if …` produces the value (#234).
pub fn state_uninitialized(my_name: &String) -> String {
    let my_name_fmt = fmt_style(my_name);

    format!(
        "{0} has no value yet",
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
        "{0} holds a copyable value",
        my_name_fmt
    )
}

// FullPrivilege on a reference's lifeline. A reference never owns
// the resource it points to, so the owner wording above would be
// false; describe what the reference permits instead. `pointee_copy`:
// the referent type is Copy (`&mut i32`), so it's "the borrowed
// value", matching the lender's own column (#312).
pub fn state_mut_ref_full_privilege(my_name: &String, pointee_copy: bool) -> String {
    let my_name_fmt = fmt_style(my_name);
    format!(
        "{0} holds a mutable reference; it can read and write the borrowed {1}",
        my_name_fmt, held(pointee_copy)
    )
}

pub fn state_static_ref_full_privilege(my_name: &String, pointee_copy: bool) -> String {
    let my_name_fmt = fmt_style(my_name);
    format!(
        "{0} holds an immutable reference; it can read the borrowed {1}",
        my_name_fmt, held(pointee_copy)
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
pub fn state_partial_privilege(my_name: &String, is_copy: bool) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "{0}'s {1} is being shared by one or more variables",
        my_name_fmt, held(is_copy)
    )
}

// PartialPrivilege on a `&mut` reference's lifeline: what it points to
// is shared through it (`let r3 = &*r;`), so r can only read until
// those shared borrows end (#313). r owns nothing, so the owner's
// "r's resource is being shared" was wrong.
pub fn state_mut_ref_partial_privilege(my_name: &String, pointee_copy: bool) -> String {
    format!(
        "{0} holds a mutable reference, but the {1} it points to is currently shared through it, so {0} can only read it",
        fmt_style(my_name), held(pointee_copy)
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

/// `event_dot_branch_merge_bound` for a reference variable: every
/// branch bound it to a reference, which never owns its referent.
pub fn event_dot_branch_merge_bound_ref(my_name: &String, is_mut: bool) -> String {
    let my_name_fmt = fmt_style(my_name);
    let kind = if is_mut { "a mutable" } else { "an immutable" };
    format!(
        "{0} is bound to {1} reference (in all branches above)",
        my_name_fmt, kind
    )
}

/// `event_dot_branch_merge_bound` for a Copy variable (`let n = match x
/// { 0 => 1, _ => 2 };`): an `i32` holds a value, not a resource it
/// owns (#316). Matches the per-arm `event_dot_acquire_copyable`.
pub fn event_dot_branch_merge_bound_copy(my_name: &String) -> String {
    let my_name_fmt = fmt_style(my_name);
    format!(
        "{0} is bound to a value (in all branches above)",
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
//
// Rust doesn't drop the value at the end of the non-moving branches: it
// keeps a runtime "drop flag" and, at the end of the variable's scope,
// drops the value only if it wasn't moved (#179). The scope-end dot
// says that (`event_dot_go_out_of_scope_maybe_moved`).
pub fn event_dot_branch_merge_moved_with_drop(my_name: &String) -> String {
    let my_name_fmt = fmt_style(my_name);
    format!(
        "{0} was moved in at least one branch above, so it can't be used after the branches \
         (until it's assigned again). If the branch that ran didn't move it, its resource is \
         dropped when {0} goes out of scope.",
        my_name_fmt
    )
}

// Scope end of a variable that was moved on some paths of a conditional
// and not on others: whether anything is dropped is decided at runtime
// by a drop flag (#179).
pub fn event_dot_go_out_of_scope_maybe_moved(my_name: &String) -> String {
    format!(
        "{0} goes out of scope. Its resource is dropped here only if no branch above moved it \
         (Rust tracks this with a runtime drop flag).",
        fmt_style(my_name)
    )
}

// Merge dot for a variable that no branch above moved (#179; this used
// to be an empty tooltip).
pub fn event_dot_branch_merge_unchanged(my_name: &String) -> String {
    format!("{0} isn't moved in any of the branches above", fmt_style(my_name))
}