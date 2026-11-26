use crate::svg_generator::data::{BranchExit, EarlyReturns, LoopExits, LoopLeft, NeverReturns, NoDropExit, ResourceAccessPoint, TempPart, IF_LET_ARM};


/* Event Dot messages: shows up when someone hovers over a dot */

use crate::svg_generator::data::{ArgTie, ChainFailHow, ChainRef, ChainUse, FailPath, NoValueBefore, RcCall, RcDrop, StaticLender, TryMove, TupleDropAt, TupleLeft, ViaCarrier};
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

/// Make every `data-tooltip-text="..."` value in a finished SVG valid XML
/// (#1049). Names go through `escape_tooltip_name`, but the prose around
/// them is written in many places and may quote Rust operators (`&&`,
/// `<`), so a bare `&` or `<` in an attribute value is escaped here, once,
/// for all of them. An `&` that already starts an entity (`&lt;`, `&amp;`,
/// `&#38;`) is left alone, so the span markup and escaped names are
/// unchanged.
pub fn escape_tooltip_attrs(svg: &str) -> String {
    const KEY: &str = "data-tooltip-text=\"";
    let mut out = String::with_capacity(svg.len() + 16);
    let mut rest = svg;
    while let Some(i) = rest.find(KEY) {
        let (head, tail) = rest.split_at(i + KEY.len());
        out.push_str(head);
        let end = tail.find('"').unwrap_or(tail.len());
        let (value, after) = tail.split_at(end);
        let bytes = value.as_bytes();
        for (j, c) in value.char_indices() {
            match c {
                '<' => out.push_str("&lt;"),
                '&' if !starts_entity(&bytes[j + 1..]) => out.push_str("&amp;"),
                _ => out.push(c),
            }
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

/// True if `b` (the bytes after an `&`) is `name;`, `#digits;` or
/// `#xhex;`.
fn starts_entity(b: &[u8]) -> bool {
    let (skip, n) = match (b.first(), b.get(1)) {
        (Some(b'#'), Some(b'x')) => (2, b[2..].iter().take_while(|c| c.is_ascii_hexdigit()).count()),
        (Some(b'#'), _) => (1, b[1..].iter().take_while(|c| c.is_ascii_digit()).count()),
        _ => (0, b.iter().take_while(|c| c.is_ascii_alphanumeric()).count()),
    };
    n > 0 && b.get(skip + n) == Some(&b';')
}

// What a column holds, as named in tooltips (#274). A Copy type (`i32`,
// `bool`, a `#[derive(Clone, Copy)]` struct) can't implement `Drop`
// (rustc rejects `Copy` + `Drop`, E0184), so it has nothing to clean up
// and nothing "owned" in the drop sense: its columns say "holds a
// value", and the copy / borrow tooltips on them say "value" too.
// A non-Copy type with no drop glue (#484: an iterator over borrowed data,
// a struct of integers that doesn't derive `Copy`) holds a "value" too:
// "resource" means something that needs cleaning up. Callers pass
// `holds_value()` (Copy, or non-Copy with nothing to drop).
// Everything else keeps RustViz's "resource".
fn held(holds_value: bool) -> &'static str {
    if holds_value { "value" } else { "resource" }
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
pub fn event_dot_ref_go_out_of_scope(my_name: &String) -> String {
    // update styling
    let my_name_fmt = styled_name_html(my_name);
    
    format!(
        "{0} goes out of scope",
        my_name_fmt
    )
}

// 0
// |
// *   the star event
//
pub fn event_dot_owner_go_out_of_scope(my_name: &String) -> String {
    // update styling
    let my_name_fmt = styled_name_html(my_name);

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
    let my_name_fmt = styled_name_html(my_name);
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

// A `Box` whose whole contents were moved out (`take(*b)`, #609) goes
// out of scope: the contents' drop isn't the box's, but the heap
// allocation is still freed.
pub fn event_dot_box_contents_moved_go_out_of_scope(my_name: &String) -> String {
    format!(
        "{} goes out of scope. Its contents were moved out earlier, so only the Box's heap allocation is freed.",
        styled_name_html(my_name)
    )
}

// A Box whose contents were moved out (`let s = *b;`) on some paths and
// that, on the others, was given a new Box since (`whole`) or moved away
// whole (`nothing`) goes out of scope (#1220): drop flags decide what is
// dropped.
pub fn event_dot_box_paths_go_out_of_scope(my_name: &String, note: &str, kept: bool, reboxed: bool, nothing: bool) -> String {
    let (holds, rest) = split_drop_note(note);
    let name = styled_name_html(my_name);
    let parts: Vec<String> = [
        Some(format!("where {0} still holds the Box whose contents were moved out earlier, only its heap allocation is freed", name)),
        kept.then(|| format!("where {0} still holds its first Box whole, its resource{1} is dropped{2}", name, holds, rest)),
        reboxed.then(|| format!("where {0} was given a new Box, its resource{1} is dropped{2}", name, holds, rest)),
        nothing.then(|| format!("where {0} was moved away, nothing is dropped", name)),
    ].into_iter().flatten().collect();
    format!(
        "{0} goes out of scope. What is dropped here depends on the path (Rust tracks this with runtime drop flags): {1}.",
        name, parts.join("; ")
    )
}

// The same where the Box is given a new value (#1220): what its old value
// drops depends on the path.
pub fn event_dot_owner_drop_at_reassign_box_paths(my_name: &String, note: &str, kept: bool, reboxed: bool, nothing: bool) -> String {
    let scope_end = event_dot_box_paths_go_out_of_scope(my_name, note, kept, reboxed, nothing);
    let name = styled_name_html(my_name);
    let rest = scope_end.strip_prefix(&format!("{} goes out of scope. What is dropped here depends on the path (Rust tracks this with runtime drop flags): ", name))
        .unwrap_or(&scope_end);
    format!(
        "{0} acquires ownership of a new resource. What its previous value drops here depends on the path (Rust tracks this with runtime drop flags): {1}",
        name, rest
    )
}

// A struct whose fields were partly moved out goes out of scope. Rust
// drops only the fields that still hold a value (`dropped`); the
// moved-out ones (`moved`) are already gone, and the struct as a whole
// isn't dropped because it no longer holds a complete value. `dropped`
// lists only non-Copy fields (a Copy field has nothing to drop).
//
// `maybe_moved` (#395): fields moved out in some branches of a
// conditional above and not in others (a struct pattern in one `match`
// arm, a field moved in one branch of an `if`). Whether they're
// dropped here depends on the path, which Rust tracks with a runtime
// drop flag (same wording as `event_dot_go_out_of_scope_maybe_moved`).
// At least one of `moved` / `maybe_moved` is non-empty.
//
// `by_cond`: at least one of `maybe_moved` was moved out by a let-chain's
// condition (#281) or by a `let` whose or-pattern binds a name from
// different places (`let ((x, _) | (_, x)) = t;`, #597), which is no
// branch: "nothing above", not "no branch above" (true of the
// branch-moved ones too).
pub fn event_dot_struct_partial_go_out_of_scope(
    my_name: &String,
    moved: &[String],
    maybe_moved: &[String],
    dropped: &[String],
    // The struct has nothing to drop at all (#484): no "resource" talk.
    plain: bool,
    // A let-chain's condition, not only a branch, may have moved one of
    // `maybe_moved` (#281): "nothing above" instead of "no branch above".
    by_cond: bool,
    // A loop that moves one of `maybe_moved` on some time rounds only
    // may have moved it (#1031): whether it's dropped isn't down to
    // whether something above moved it, so "only if it still holds it".
    kept: bool,
    // A `Box` a field was moved out of (#602): Rust still frees the
    // box's heap allocation, so the text says so.
    boxed: bool,
) -> String {
    const FREED: &str = " The Box's heap allocation is freed.";
    fn join(names: &[String]) -> (String, bool) {
        let fmt: Vec<String> = names.iter().map(|n| styled_name_html(n)).collect();
        let s = match fmt.len() {
            0 | 1 => fmt.concat(),
            n => format!("{} and {}", fmt[..n - 1].join(", "), fmt[n - 1]),
        };
        (s, names.len() > 1)
    }
    if plain {
        // Nothing here needs drop, so nothing is "dropped" whichever
        // fields moved (#484).
        let mut out = format!("{} goes out of scope.", styled_name_html(my_name));
        if !moved.is_empty() {
            let (moved_s, moved_plural) = join(moved);
            out.push_str(&format!(" {} {} moved out earlier.", moved_s, if moved_plural { "were" } else { "was" }));
        }
        if !maybe_moved.is_empty() {
            let (maybe_s, maybe_plural) = join(maybe_moved);
            if by_cond {
                out.push_str(&format!(" {} may have been moved out above.", maybe_s));
            } else {
                out.push_str(&format!(" {} {} moved out in at least one branch above.", maybe_s, if maybe_plural { "were" } else { "was" }));
            }
        }
        return out;
    }
    if !maybe_moved.is_empty() {
        let mut out = format!("{} goes out of scope.", styled_name_html(my_name));
        if !moved.is_empty() {
            let (moved_s, moved_plural) = join(moved);
            out.push_str(&format!(" {} {} moved out earlier.", moved_s, if moved_plural { "were" } else { "was" }));
        }
        let (maybe_s, maybe_plural) = join(maybe_moved);
        if kept && maybe_plural {
            out.push_str(&format!(
                " {} may have been moved out above, so each is dropped here only if it still holds \
                 its value (Rust tracks this with runtime drop flags).",
                maybe_s
            ));
        } else if kept {
            out.push_str(&format!(
                " {0} may have been moved out above, so it's dropped here only if {0} still holds \
                 it (Rust tracks this with a runtime drop flag).",
                maybe_s
            ));
        } else if by_cond {
            out.push_str(&format!(
                " {} may have been moved out above, so {} dropped here only if nothing above moved \
                 it (Rust tracks this with {}).",
                maybe_s,
                if maybe_plural { "each is" } else { "it's" },
                if maybe_plural { "runtime drop flags" } else { "a runtime drop flag" }
            ));
        } else if maybe_plural {
            out.push_str(&format!(
                " {} were moved out in some of the branches above; each is dropped here only if no \
                 branch above moved it (Rust tracks this with runtime drop flags).",
                maybe_s
            ));
        } else {
            out.push_str(&format!(
                " {} was moved out in at least one branch above, so it's dropped here only if no \
                 branch above moved it (Rust tracks this with a runtime drop flag).",
                maybe_s
            ));
        }
        if !dropped.is_empty() {
            let (dropped_s, dropped_plural) = join(dropped);
            out.push_str(&format!(" {} {} dropped.", dropped_s, if dropped_plural { "are" } else { "is" }));
        }
        if boxed { out.push_str(FREED); }
        return out;
    }
    let (moved_s, moved_plural) = join(moved);
    let rest = if dropped.is_empty() {
        String::from(if boxed { "no field is dropped" } else { "no resource is dropped" })
    } else {
        let (dropped_s, dropped_plural) = join(dropped);
        format!("only {} {} dropped", dropped_s, if dropped_plural { "are" } else { "is" })
    };
    format!(
        "{0} goes out of scope. {1} {2} moved out earlier, so {3}.{4}",
        styled_name_html(my_name),
        moved_s,
        if moved_plural { "were" } else { "was" },
        rest,
        if boxed { FREED } else { "" }
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
        styled_name_html(my_name)
    )
}

// A union whose type has its own `Drop` impl goes out of scope (#1146):
// the impl runs, but the union's fields are still never dropped.
pub fn event_dot_union_drop_go_out_of_scope(my_name: &String) -> String {
    format!(
        "{0} goes out of scope. Its type implements Drop, so Drop::drop runs on it here, \
         but a union's fields are never dropped automatically.",
        styled_name_html(my_name)
    )
}

// A `MaybeUninit<T>` goes out of scope (#1146). Like a `ManuallyDrop<T>`
// it never drops the `T` inside it (it can't know whether it holds one).
pub fn event_dot_maybe_uninit_go_out_of_scope(my_name: &String) -> String {
    format!(
        "{0} goes out of scope. Nothing is dropped: a MaybeUninit never drops the value inside it automatically.",
        styled_name_html(my_name)
    )
}

// A `ManuallyDrop<T>` goes out of scope (#283). It never drops the `T`
// inside it: that only happens through `ManuallyDrop::drop` /
// `into_inner` / `take`, and otherwise the value is leaked.
pub fn event_dot_manually_drop_go_out_of_scope(my_name: &String) -> String {
    format!(
        "{0} goes out of scope. Nothing is dropped: a ManuallyDrop never drops the value inside it automatically.",
        styled_name_html(my_name)
    )
}

// `ManuallyDrop::drop(&mut md)`: the value inside `md` is dropped here,
// by hand.
pub fn event_dot_manually_drop_by_hand(my_name: &String) -> String {
    format!(
        "ManuallyDrop::drop drops the value inside {0} (by hand)",
        styled_name_html(my_name)
    )
}

// State of a `ManuallyDrop` after `ManuallyDrop::drop`: it holds no
// resource, and using it again is undefined behaviour.
pub fn state_dropped_by_hand(my_name: &String) -> String {
    format!(
        "{0}'s value was dropped by hand; it must not be used again",
        styled_name_html(my_name)
    )
}

// A union that already holds a value is written to again (#191):
// `u.a = 5;`, `u.a = y;`, `u = U { .. };`. It reads like a plain
// assignment (2026-11-05 decision) plus the clause that states the
// no-drop fact. `is_copy` is whether the value written is Copy;
// `source` names the variable it was copied from, if any (a literal
// or call result has none).
pub fn event_dot_union_write(my_name: &String, is_copy: bool, source: Option<&String>) -> String {
    let head = match (is_copy, source) {
        (true, Some(from)) => event_dot_copyable_assign_copy(my_name, from),
        (true, None) => event_dot_copyable_assign(my_name),
        (false, _) => event_dot_acquire(my_name),
    };
    format!("{0}; writing to a union never drops its old contents", head)
}

// An owned (non-ref) function parameter receives its resource from
// whatever the caller passed. The L-shaped arrow on the param's
// timeline visually anchors that "from outside this scope" origin;
// this is its tooltip and the matching dot tooltip.
pub fn event_dot_owner_init_from_caller_value(my_name: &String) -> String {
    format!("{0} is bound to a value from the caller", styled_name_html(my_name))
}

// A Copy parameter (`n: i32`, `c: bool`, `a: Option<i32>`): the caller's
// value is copied in; nothing is owned or moved (#505).
pub fn event_dot_copy_init_from_caller(my_name: &String) -> String {
    format!("{0} is a copy of the caller's value", styled_name_html(my_name))
}

// The parts of a parameter's argument its pattern doesn't bind (`P {
// name, .. }: P`, `(a, _): (String, String)`) stay in the parameter and
// are dropped when the function returns (#1181). `parts` are field paths.
pub fn param_rest_dropped(fn_name: &str, parts: &[String]) -> String {
    // A tuple element reads as Rust writes it (`.1`), a field by name.
    let names: Vec<String> = parts.iter()
        .map(|p| if p.starts_with(|c: char| c.is_ascii_digit()) { format!(".{}", p) } else { p.clone() })
        .map(|p| styled_name_html(&p))
        .collect();
    let (list, stays, verb) = match names.as_slice() {
        [one] => (one.clone(), "it stays", "is"),
        [init @ .., last] => (format!("{} and {}", init.join(", "), last), "they stay", "are"),
        [] => (String::new(), "it stays", "is"),
    };
    format!(
        "The pattern doesn't bind the argument's {0}, so {1} in the parameter and {2} dropped when {3} returns",
        list, stays, verb, styled_name_html(fn_name)
    )
}

// `fn f(_: String)`: the pattern binds no name, but f still owns the
// argument, in the column `my_name` (`(parameter)`), until it returns
// (#1243).
pub fn event_dot_unbound_param_init(my_name: &String, fn_name: &String) -> String {
    format!(
        "{0} is the value the caller passed to {1}. The parameter's pattern binds no name, but {1} owns the value until it returns",
        styled_name_html(my_name), styled_name_html(fn_name)
    )
}

pub fn event_dot_owner_init_from_caller(my_name: &String) -> String {
    let my_name_fmt = styled_name_html(my_name);
    format!("{0} acquires ownership from the caller", my_name_fmt)
}

// A `ref` / `ref mut` binding of a by-value parameter (`fn f(ref s:
// String)`, `fn g((ref a, b): (String, i32))`). The caller's value was
// moved (or copied) into `f`, which owns it; the binding borrows that,
// so it mustn't say "from the caller" (#377).
pub fn event_dot_ref_init_own_param(my_name: &String, fn_name: &String, whole: bool, is_mut: bool, drops: bool) -> String {
    let my_name_fmt = styled_name_html(my_name);
    let fn_fmt = styled_name_html(fn_name);
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

// A reference field of a struct bound by a pattern (`if let Some(x) =
// o`, `x.part: &str`). It holds whatever reference the matched value
// held; what that borrows isn't tracked at the pattern, so the message
// doesn't name a lender, and never says "owns" (#352).
pub fn event_dot_ref_init_in_pattern(my_name: &String, bound: &String, is_mut: bool) -> String {
    let my_name_fmt = styled_name_html(my_name);
    let bound_fmt = styled_name_html(bound);
    let kind = if is_mut { "a mutable" } else { "an immutable" };
    format!("{0} holds {1} reference (it came with the value matched into {2})", my_name_fmt, kind, bound_fmt)
}

// The column of a by-value parameter's value that a `ref` binding
// borrows (`fn f(ref s: String)`, #691): the caller's argument was moved
// into `f`, which owns it until it returns.
pub fn event_dot_param_value_init(my_name: &String, fn_name: &String, whole: bool) -> String {
    format!(
        "{0} is {2} the value the caller passed to {1}. {1} owns it, and a ref binding borrows it",
        styled_name_html(my_name), styled_name_html(fn_name), if whole { "" } else { "part of" }
    ).replace("  ", " ")
}

// `let m = MAX;` / `m = MAX;` with a Copy constant (#690): a constant
// has no place in memory of its own; each use of it is a new copy of
// its value.
pub fn event_dot_copy_of_const(my_name: &String, constant: &String) -> String {
    format!("{0} is initialized with a copy of the constant {1}", styled_name_html(my_name), styled_name_html(constant))
}

pub fn event_dot_assign_copy_of_const(my_name: &String, constant: &String) -> String {
    format!("{0} is assigned a copy of the constant {1}", styled_name_html(my_name), styled_name_html(constant))
}

// A `static` item's column (#690), at the start and at the end of the
// function that uses it: a static is one place in memory that exists
// for the whole run of the program, so the function neither creates
// nor drops it.
pub fn event_dot_static(my_name: &String) -> String {
    format!("{0} is a static: it lives for the whole program and is never dropped", styled_name_html(my_name))
}

// A reference function parameter receives a borrow from the caller.
// Mirror message to `event_dot_owner_init_from_caller` for the ref
// side: makes it clear that the lender lives outside this fn scope.
pub fn event_dot_ref_init_from_caller(my_name: &String, is_mut: bool) -> String {
    let my_name_fmt = styled_name_html(my_name);
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
pub fn event_dot_owner_drop_at_reassign(my_name: &String, note: &str) -> String {
    let my_name_fmt = styled_name_html(my_name);
    let (holds, rest) = split_drop_note(note);
    format!(
        "{0} acquires ownership of a new resource; its previous resource{1} is dropped{2}",
        my_name_fmt, holds, rest
    )
}

// Reassignment after a conditional that moved the variable on some
// paths only (#335): like the scope end of such a variable
// (`event_dot_go_out_of_scope_maybe_moved`), the old value is dropped
// only if it's still there, which Rust decides at runtime.
// `by_cond`: a let-chain's condition, not only a branch, may have moved
// it (#281): see `moved_above`.
// `note`: what else the drop drops, or an Rc's count note (#542, #679).
pub fn event_dot_owner_drop_at_reassign_maybe_moved(my_name: &String, note: &str, by_cond: bool) -> String {
    let (holds, rest) = split_drop_note(note);
    format!(
        "{0} acquires ownership of a new resource. Its previous resource{1} is dropped{2} here only if \
         {3} moved it (Rust tracks this with a runtime drop flag).",
        styled_name_html(my_name), holds, rest, moved_above(by_cond)
    )
}

// `b = Box::new(…)` after the old Box's whole contents were moved out
// (`let s = *b;`): the old contents aren't the Box's to drop any more,
// but its heap allocation is freed here (#1145). (Starts ". Its
// previous", so it subsumes the line's plain acquire.)
pub fn event_dot_owner_drop_at_reassign_box_contents_moved(my_name: &String) -> String {
    format!(
        "{} acquires ownership of a new resource. Its previous contents were moved out earlier, \
         so only its previous Box's heap allocation is freed here.",
        styled_name_html(my_name)
    )
}

// `event_dot_owner_drop_at_reassign_maybe_moved` for a Box whose
// contents (`*b`), not the Box itself, were moved out in some branches
// above (#921): the drop flag decides only whether the old contents are
// dropped; the old Box's heap allocation is freed either way. `holds`:
// the Box's `Owner::holds` (`the String it holds`), `None` when the
// contents need no drop.
pub fn event_dot_owner_drop_at_reassign_box_contents_maybe_moved(my_name: &String, holds: Option<&str>, by_cond: bool) -> String {
    // (Starts ". Its previous ", so it subsumes the line's plain
    // acquire: `reassign_subsumes_acquire`.)
    let name = styled_name_html(my_name);
    let mut out = format!("{} acquires ownership of a new resource.", name);
    match holds.map(|h| h.strip_suffix(" it holds").and_then(|t| t.strip_prefix("the "))) {
        Some(Some(ty)) => out.push_str(&format!(
            " Its previous {} is dropped here only if {} moved it out (Rust tracks this with a runtime \
             drop flag). Its previous Box's heap allocation is freed here either way.",
            escape_tooltip_name(ty), moved_above(by_cond)
        )),
        Some(None) => out.push_str(&format!(
            " Its previous contents are dropped here only if {} moved them out (Rust tracks this with \
             a runtime drop flag). Its previous Box's heap allocation is freed here either way.",
            moved_above(by_cond)
        )),
        None => out.push_str(" Its previous Box's heap allocation is freed here."),
    }
    out
}

// `*b = v` on a Box whose old contents were moved out before (`let w =
// *b;`, #928): `maybe` None when they were moved on every path (nothing
// is dropped), `Some(by_cond)` when on some paths only (a drop flag
// decides). The Box keeps its heap allocation either way. `holds`: the
// Box's `Owner::holds` (`the String it holds`).
pub fn event_dot_box_contents_write_after_move(deref_name: &String, holds: Option<&str>, maybe: Option<bool>) -> String {
    // (Starts ". Its previous ", so it subsumes the line's plain
    // acquire: `reassign_subsumes_acquire`.)
    let ty = holds.and_then(|h| h.strip_suffix(" it holds")).and_then(|t| t.strip_prefix("the "));
    let what = ty.map_or_else(|| String::from("contents"), escape_tooltip_name);
    let plural = ty.is_none();
    let rest = match maybe {
        None => format!(
            "Its previous {} {} moved out earlier, so nothing is dropped.",
            what, if plural { "were" } else { "was" }
        ),
        Some(by_cond) => format!(
            "Its previous {} {} dropped here only if {} moved {} out (Rust tracks this with a runtime drop flag).",
            what, if plural { "are" } else { "is" }, moved_above(by_cond), if plural { "them" } else { "it" }
        ),
    };
    format!("{} acquires ownership of a new resource. {}", styled_name_html(deref_name), rest)
}

// Scope end of a Box whose contents need no drop (`Box<P>`, P with no
// drop glue) and were moved out in some branches only (#929): nothing of
// the contents is dropped on any path, so no drop flag matters; the
// Box's heap allocation is freed on every path.
pub fn event_dot_go_out_of_scope_box_plain_contents_maybe_moved(my_name: &String, by_cond: bool) -> String {
    format!(
        "{} goes out of scope. Its contents {}; the Box's heap allocation is freed here either way.",
        styled_name_html(my_name),
        if by_cond { "may have been moved out above" } else { "were moved out in at least one branch above" }
    )
}

/// Who may have moved a maybe-moved variable, for the drop-flag
/// wording: "no branch above", or, when a let-chain's condition may have
/// moved it before failing (#281), "nothing above".
pub fn moved_above(by_cond: bool) -> &'static str {
    if by_cond { "nothing above" } else { "no branch above" }
}

// Reassigning a struct or tuple some of whose parts were moved out
// before (#628): like its scope end (`event_dot_struct_partial_go_out_of_scope`),
// only the parts still there are dropped, those moved on some paths
// only by a drop flag ("nothing above" after a `let`'s or-pattern,
// "no branch above" after a branch, #597). "Its previous t.0" names
// the old value's part, not the new one.
pub fn event_dot_struct_drop_at_reassign_partial(my_name: &String, parts: &crate::svg_generator::data::ReassignParts) -> String {
    fn join(names: &[String]) -> (String, bool) {
        let fmt: Vec<String> = names.iter().map(|n| styled_name_html(n)).collect();
        let s = match fmt.len() {
            0 | 1 => fmt.concat(),
            n => format!("{} and {}", fmt[..n - 1].join(", "), fmt[n - 1]),
        };
        (s, names.len() > 1)
    }
    let mut out = format!("{} acquires ownership of a new resource.", styled_name_html(my_name));
    let (moved_s, moved_plural) = join(&parts.moved);
    let (dropped_s, dropped_plural) = join(&parts.dropped);
    if parts.maybe_moved.is_empty() {
        let rest = if parts.dropped.is_empty() {
            String::from("nothing of it is dropped")
        } else {
            format!("only its previous {} {} dropped", dropped_s, if dropped_plural { "are" } else { "is" })
        };
        out.push_str(&format!(" Its previous {} {} moved out earlier, so {}.",
            moved_s, if moved_plural { "were" } else { "was" }, rest));
        return out;
    }
    if !parts.moved.is_empty() {
        out.push_str(&format!(" Its previous {} {} moved out earlier.", moved_s, if moved_plural { "were" } else { "was" }));
    }
    let (maybe_s, maybe_plural) = join(&parts.maybe_moved);
    out.push_str(&match (parts.by_cond, maybe_plural) {
        (true, true) => format!(
            " Its previous {} may have been moved out above, so each is dropped here only if nothing above \
             moved it (Rust tracks this with runtime drop flags).", maybe_s),
        (true, false) => format!(
            " Its previous {} may have been moved out above, so it's dropped here only if nothing above \
             moved it (Rust tracks this with a runtime drop flag).", maybe_s),
        (false, true) => format!(
            " Its previous {} were moved out in some of the branches above; each is dropped here only if no \
             branch above moved it (Rust tracks this with runtime drop flags).", maybe_s),
        (false, false) => format!(
            " Its previous {} was moved out in at least one branch above, so it's dropped here only if no \
             branch above moved it (Rust tracks this with a runtime drop flag).", maybe_s),
    });
    if !parts.dropped.is_empty() {
        out.push_str(&format!(" Its previous {} {} dropped.", dropped_s, if dropped_plural { "are" } else { "is" }));
    }
    out
}

// A call's result that nothing takes ownership of is a temporary; Rust
// drops it when its statement (or other temporary scope) ends (#457).
// The two dots of its column: where it comes into being, and the drop.
pub fn event_dot_unbound_temporary_acquire(from_name: &String) -> String {
    format!(
        "A temporary holds the value that {0}. Nothing binds it to a variable or moves it on, so Rust drops it as soon as its statement (or enclosing expression) is done",
        temporary_source(from_name)
    )
}

// "make returns", "the vec! macro produces", "the literal P { .. }
// builds": where an unbound temporary's value comes from. The column
// it comes from is a fn's, a std macro's (`vec!`) or a literal's
// (`P { .. }`, `(..)`, `[..]`) (#469).
fn temporary_source(from_name: &String) -> String {
    if from_name.ends_with('!') {
        format!("the {0} macro produces", styled_name_html(from_name))
    } else if from_name.ends_with(" { .. }") || from_name == "(..)" || from_name == "[..]" {
        format!("the literal {0} builds", styled_name_html(from_name))
    } else {
        format!("{0} returns", styled_name_html(from_name))
    }
}

// An unbound temporary that something is moved out of before it is
// dropped (#469): a field (`let s = make_p().name;`) or what a pattern
// binds by value (`match make() { Some(s) => .. }`). Nothing takes the
// rest, which is dropped with the temporary; the drop says when.
pub fn event_dot_unbound_temporary_part_acquire(from_name: &String, part: &TempPart) -> String {
    temporary_part_acquire(temporary_source(from_name), part)
}

// The same for the value a constructor builds when a pattern takes it
// apart (`match Some(make()) { Some(s) if .. => .., _ => .. }`, #807):
// from what the call making its payload returns, if there is one.
pub fn event_dot_ctor_temporary_part_acquire(from_name: Option<&String>, part: &TempPart) -> String {
    let source = match from_name {
        Some(f) => format!("is built from what {}", temporary_source(f)),
        None => "the constructor builds".to_owned(),
    };
    temporary_part_acquire(source, part)
}

// "A temporary holds the value that {source}, ...".
fn temporary_part_acquire(source: String, part: &TempPart) -> String {
    match part {
        TempPart::Field(f) => format!(
            "A temporary holds the value that {0}. Its field {1} is moved out of it; nothing takes the rest, so Rust drops the temporary, and what is left in it, as soon as its statement (or enclosing expression) is done",
            source, styled_name_html(f)
        ),
        // `(z, make_q().name)` as an `if let` / `match` scrutinee (#1350).
        TempPart::TupleField(f, tuple) => format!(
            "A temporary holds the value that {0}. Its field {1} is moved out of it, into the temporary tuple {2}; nothing takes the rest, which is dropped with the temporary",
            source, styled_name_html(f), styled_name_html(tuple)
        ),
        // `P { n: 1, ..make_p() }` (#605).
        TempPart::UpdateBase(fs) => format!(
            "A temporary holds the value that {0}. The struct update `..` moves {1} out of it, into the struct being built; nothing takes the rest, so Rust drops the temporary, and what is left in it, as soon as its statement (or enclosing expression) is done",
            source, its_fields(fs)
        ),
        TempPart::Match(None) => format!(
            "A temporary holds the value that {0}, for the match to inspect. The pattern of the arm that runs can move parts of it into its bindings; what is left in it is dropped with the temporary after the match",
            source
        ),
        // `match make_p().name { .. }` (#605).
        TempPart::Match(Some(f)) => format!(
            "A temporary holds the value that {0}, for the match to inspect its field {1}. The pattern of the arm that runs can move that field, or parts of it, into its bindings; what is left in the temporary is dropped with it after the match",
            source, styled_name_html(f)
        ),
        // `let (a, _) = pair();` (#623): the pattern can't fail.
        TempPart::Let(None) => format!(
            "A temporary holds the value that {0}, for the `let` pattern to take apart. The pattern moves parts of it into its bindings; nothing takes the rest, so Rust drops the temporary, and what is left in it, at the end of the statement",
            source
        ),
        TempPart::Let(Some(f)) => format!(
            "A temporary holds the value that {0}, for the `let` pattern to take its field {1} apart. The pattern moves parts of that field into its bindings; nothing takes the rest, so Rust drops the temporary, and what is left in it, at the end of the statement",
            source, styled_name_html(f)
        ),
        // `(m, _) = pair();` (#625).
        TempPart::Assign(None) => format!(
            "A temporary holds the value that {0}, for the destructuring assignment to take apart. It moves parts of it to the places on the left; nothing takes the rest, so Rust drops the temporary, and what is left in it, before those places get their new values",
            source
        ),
        TempPart::Assign(Some(f)) => format!(
            "A temporary holds the value that {0}, for the destructuring assignment to take its field {1} apart. It moves parts of that field to the places on the left; nothing takes the rest, so Rust drops the temporary, and what is left in it, before those places get their new values",
            source, styled_name_html(f)
        ),
        TempPart::IfLet(None) | TempPart::WhileLet(None) | TempPart::LetElse(None) | TempPart::LetChain(None) => format!(
            "A temporary holds the value that {0}, for the {1} to match against. If the pattern matches, it moves parts of it into its bindings; what is left in it (all of it, if the pattern doesn't match) is dropped with the temporary",
            source, let_kind(part)
        ),
        TempPart::IfLet(Some(f)) | TempPart::WhileLet(Some(f)) | TempPart::LetElse(Some(f)) | TempPart::LetChain(Some(f)) => format!(
            "A temporary holds the value that {0}, for the {1} to match its field {2} against. If the pattern matches, it can move that field, or parts of it, into its bindings; what is left in the temporary (all of it, if the pattern doesn't match) is dropped with it",
            source, let_kind(part), styled_name_html(f)
        ),
    }
}

// On the first dot of a `ref` / `ref mut` binding into a tuple
// scrutinee's element (`a` in `(Some(ref a), _)` on `(x, y)`, #1363): it
// borrows the part of the temporary tuple that holds x's value, not x,
// whose value was moved into the tuple (#580); the tuple has no column.
pub fn approx_note_tuple_elem_borrow(name: &str, var: &str, tuple: &str, mutable: bool) -> String {
    let tuple = if tuple.is_empty() { "the temporary tuple".to_owned() } else { format!("the temporary tuple {}", styled_name_html(tuple)) };
    format!(
        "({0} {1} borrows the part of {2} that holds {3}'s value; the tuple has no column, so no arrow shows this borrow.)",
        styled_name_html(name), if mutable { "mutably" } else { "immutably" }, tuple, styled_name_html(var)
    )
}

// "`if let`", "`while let`", "`let`-`else`".
fn let_kind(part: &TempPart) -> &'static str {
    match part {
        TempPart::WhileLet(_) => "`while let`",
        TempPart::LetElse(_) => "`let`-`else`",
        TempPart::LetChain(_) => "`let` in this let chain",
        _ => "`if let`",
    }
}

// "its field a", "its fields a and b", "its fields a, b and c".
fn its_fields(fs: &[String]) -> String {
    let names: Vec<String> = fs.iter().map(|n| styled_name_html(n)).collect();
    match names.as_slice() {
        [one] => format!("its field {}", one),
        [rest @ .., last] => format!("its fields {} and {}", rest.join(", "), last),
        [] => "its fields".to_owned(),
    }
}

// The move out of such a temporary, on its column.
// `empties`: a pattern's bindings took everything in it that needs drop
// (this move is the last of them, #1386).
pub fn event_dot_move_out_of_temporary(my_name: &String, to_name: &String, part: &TempPart, empties: bool) -> String {
    match part {
        TempPart::Field(f) => format!("{0}'s field {1} is moved to {2}", styled_name_html(my_name), styled_name_html(f), styled_name_html(to_name)),
        // `let q = P { n: 1, ..make_p() };`: a field of the base goes to
        // the field of the same name (`q.name`, #605).
        TempPart::UpdateBase(_) => {
            let f = to_name.rsplit('.').next().unwrap_or(to_name).to_owned();
            format!("The struct update `..` moves {0}'s field {1} to {2}", styled_name_html(my_name), styled_name_html(&f), styled_name_html(to_name))
        }
        _ if empties => format!(
            "The pattern moves what {0} holds into {1}; nothing that needs dropping is left in {0}",
            styled_name_html(my_name), styled_name_html(to_name)
        ),
        TempPart::Match(_) | TempPart::IfLet(_) | TempPart::WhileLet(_) | TempPart::LetChain(_) =>
            format!("The pattern moves part of {0}'s value into {1}; {0} keeps the rest", styled_name_html(my_name), styled_name_html(to_name)),
        // A destructuring assignment's (`(m, _) = pair();`, #625) or a
        // `let` / `let`-`else` pattern's temporary isn't split
        // (`temp_emptying_moves` doesn't look at it).
        TempPart::Assign(_) => format!("The assignment moves part of {0}'s value to {1}", styled_name_html(my_name), styled_name_html(to_name)),
        _ => format!("The pattern moves {0}'s value, or part of it, into {1}", styled_name_html(my_name), styled_name_html(to_name)),
    }
}

// A variable moved into an unbound temporary (`vec![s];`, `let _ = P {
// name: t, n: 1 };`): the variable's value is now the temporary's, and
// is dropped with it (#570). When a pattern moves parts of the
// temporary out (`moved_on`: `if let P { a: Some(s), .. } = (P { a: x,
// .. })`), the column's other dots say what is left and when it is
// dropped (#955).
pub fn event_dot_unbound_temporary_takes(from_name: &String, moved_on: bool) -> String {
    if moved_on {
        return format!("This temporary takes the value of {0}, which is moved into it", styled_name_html(from_name));
    }
    format!(
        "This temporary takes the value of {0}, which is moved into it. Nothing binds the temporary to a variable or moves it on, so Rust drops it, and what it holds, as soon as its statement (or enclosing expression) is done",
        styled_name_html(from_name)
    )
}

// A call's result moved into the value a constructor builds (`Some(make());`)
// and nothing binds that value (#783).
pub fn event_dot_unbound_temporary_takes_result(from_name: &String) -> String {
    format!(
        "This temporary takes the value that {0}, which is moved into it. Nothing binds the temporary to a variable or moves it on, so Rust drops it, and what it holds, as soon as its statement (or enclosing expression) is done",
        temporary_source(from_name)
    )
}

// The drop of a temporary that a `let` borrowed (#400): its column has
// the same label as an unbound temporary's, so the tooltip says why it
// is dropped here and not at the end of its statement.
pub fn event_dot_temporary_go_out_of_scope(my_name: &String) -> String {
    format!(
        "{0} goes out of scope at the end of the block: a let borrowed it, so it was kept alive past the end of its statement. Its resource is dropped.",
        styled_name_html(my_name)
    )
}

// The same temporary dropped where an early exit leaves its block
// (`return`, `break`, `continue`, a panic: #553), so "at the end of
// the block" would be false.
pub fn event_dot_extended_temporary_drop(my_name: &String, when: &String) -> String {
    format!(
        "{0} goes out of scope {1}: a let borrowed it, so it lived past its statement. Its resource is dropped.",
        styled_name_html(my_name), when
    )
}

// The same for a temporary a `let` extends through a branch, which only
// that branch made (#1397, #1436): a drop flag decides.
pub fn event_dot_extended_temporary_drop_made(my_name: &String, when: &String, made_at: Option<usize>) -> String {
    format!(
        "{0} goes out of scope {1}: a let borrowed it, so it lived past its statement. Its resource \
         is dropped only if {2} (Rust tracks this with a runtime drop flag).",
        styled_name_html(my_name), when, temp_made_in(made_at)
    )
}

// Its drop at a `?` (#1436).
pub fn event_dot_temp_drop_if_try_returns_made(my_name: &String, made_at: Option<usize>) -> String {
    format!(
        "Only if the `?` here returns early (on an `Err` or `None`): the function returns and {0}'s \
         resource is dropped if {1} (Rust tracks this with a runtime drop flag). If the `?` succeeds, \
         nothing happens to {0} here.",
        styled_name_html(my_name), temp_made_in(made_at)
    )
}

pub fn event_dot_temporary_drop(ty: &String, when: &String, literal: bool, assign: bool) -> String {
    if literal && assign {
        // `(a, _) = (m, n);` (#1387): rustc lowers it to `{ let (lhs, _)
        // = (m, n); a = lhs; }`, so the temporary goes first.
        return format!(
            "This temporary {0}, built by the literal on the right of the destructuring assignment, is dropped as soon as the assignment has taken it apart (before the places on the left get their new values), and with it whatever the pattern didn't assign to a place. Its resource is dropped",
            styled_name_html(ty)
        );
    }
    if literal {
        // `let (x, _) = (m, n);`: m went into x, n stayed in the literal.
        return format!(
            "This temporary {0}, built by the literal on the right of the let, is dropped {1}. What the pattern didn't bind by value stays in it and is dropped with it. Its resource is dropped",
            styled_name_html(ty), when
        );
    }
    format!(
        "This temporary {0} is dropped {1}: nothing bound it to a variable or moved it on. Its resource is dropped",
        styled_name_html(ty), when
    )
}

// The drop of an unbound temporary that something was moved out of
// (#469): what is left in it goes with it. Whether anything that needs
// dropping is left can depend on the path taken (the arm that ran), so
// this doesn't claim a resource is dropped.
pub fn event_dot_temporary_part_drop(ty: &String, when: &String, part: &TempPart) -> String {
    match part {
        TempPart::Field(f) => format!(
            "This temporary {0} is dropped {1}, and with it everything in it but its field {2}, which was moved out of it",
            styled_name_html(ty), when, styled_name_html(f)
        ),
        TempPart::TupleField(f, tuple) => format!(
            "This temporary {0} is dropped {1}, and with it everything in it but its field {2}, which was moved into the temporary tuple {3}: if no pattern takes {2} out of the tuple, it is dropped with the tuple, at the same time",
            styled_name_html(ty), when, styled_name_html(f), styled_name_html(tuple)
        ),
        TempPart::UpdateBase(fs) => format!(
            "This temporary {0} is dropped {1}, and with it everything in it but {2}, which the struct update moved out of it",
            styled_name_html(ty), when, its_fields(fs)
        ),
        TempPart::Match(None) => format!(
            "This temporary {0} is dropped {1}, after the arm that ran, and with it whatever that arm's pattern didn't move out of it",
            styled_name_html(ty), when
        ),
        TempPart::Match(Some(f)) => format!(
            "This temporary {0} is dropped {1}, after the arm that ran, and with it everything in it but what that arm's pattern moved out of its field {2}",
            styled_name_html(ty), when, styled_name_html(f)
        ),
        TempPart::Assign(None) => format!(
            "This temporary {0} is dropped {1}, and with it whatever the assignment didn't move out of it",
            styled_name_html(ty), when
        ),
        TempPart::Assign(Some(f)) => format!(
            "This temporary {0} is dropped {1}, and with it everything in it but what the assignment moved out of its field {2}",
            styled_name_html(ty), when, styled_name_html(f)
        ),
        TempPart::Let(None) => format!(
            "This temporary {0} is dropped {1}, and with it whatever the pattern didn't move out of it",
            styled_name_html(ty), when
        ),
        TempPart::Let(Some(f)) => format!(
            "This temporary {0} is dropped {1}, and with it everything in it but what the pattern moved out of its field {2}",
            styled_name_html(ty), when, styled_name_html(f)
        ),
        // One path of a temporary forked at an `if let` (#1360, #1386):
        // the matched block's end, where the pattern did match.
        TempPart::IfLet(_) | TempPart::LetChain(_) if when.contains("(its pattern matched)") || when.contains("the block its let chain matched ends") => {
            format!("This temporary {0} is dropped {1}, and with it {2}", styled_name_html(ty), when, what_pattern_left(part))
        }
        // The other: a lone `if let` whose pattern didn't match moved
        // nothing out of it.
        TempPart::IfLet(_) if !when.contains(", or ") && (when.starts_with("before the `else` block") || when.starts_with("when the pattern of this `if let` doesn't match")) => format!(
            "This temporary {0} is dropped {1}, and with it everything in it: the pattern didn't match, so it moved nothing out",
            styled_name_html(ty), when
        ),
        TempPart::IfLet(_) | TempPart::WhileLet(_) | TempPart::LetElse(_) | TempPart::LetChain(_) => format!(
            "This temporary {0} is dropped {1}, and with it {2} (all of it, if the pattern didn't match)",
            styled_name_html(ty), when, what_pattern_left(part)
        ),
    }
}

// What a pattern that matched leaves in the temporary of an `if let` /
// `while let` / `let`-`else` / let-chain scrutinee (`part`).
fn what_pattern_left(part: &TempPart) -> String {
    match part {
        TempPart::IfLet(Some(f)) | TempPart::WhileLet(Some(f)) | TempPart::LetElse(Some(f)) | TempPart::LetChain(Some(f)) =>
            format!("everything in it but what the pattern moved out of its field {}", styled_name_html(f)),
        _ => "whatever the pattern didn't move out of it".to_owned(),
    }
}

/// Whether `acquire` is the plain "acquires ownership of a resource" line
/// that `reassign` (`event_dot_owner_drop_at_reassign`) already says, for
/// the same variable, with more (#506, #519). `*b = (..)` on a Box drawn
/// as its payload's part columns says it of `*b` (#893): the Box's own
/// "b acquires ownership of a resource" on that dot adds nothing either.
pub fn reassign_subsumes_acquire(reassign: &str, acquire: &str) -> bool {
    if let Some(rest) = reassign.strip_prefix(SPAN_BEGIN).and_then(|r| r.strip_prefix('*')) {
        if reassign_subsumes_acquire(&format!("{}{}", SPAN_BEGIN, rest), acquire) {
            return true;
        }
    }
    // "…; its previous resource is dropped" (a holds note, #542, sits
    // before "is dropped" and an Rc / Arc / Weak's drop note after it,
    // #373), or ". Its previous t.0 …" / ". Its previous resource is
    // dropped here only if …" (#335, #628).
    let Some(name) = acquire.strip_suffix(" acquires ownership of a resource") else {
        // `s = r?;` (#652): r's dot says s gets what's inside it.
        return reassign.split_once(" acquires ownership of a new resource; its previous resource")
            .is_some_and(|(name, tail)| (tail.starts_with(" is dropped") || tail.starts_with(" (including "))
                && acquire.strip_prefix(name).is_some_and(|rest| rest.starts_with(" acquires ownership of the resource the `?` takes out of ")));
    };
    reassign.strip_prefix(name)
        .and_then(|rest| rest.strip_prefix(" acquires ownership of a new resource"))
        .is_some_and(|rest| {
            rest.strip_prefix("; its previous resource")
                .is_some_and(|tail| tail.starts_with(" is dropped") || tail.starts_with(" (including "))
                || rest.starts_with(". Its previous ")
                // A re-boxed Box's path-dependent drop (#1220).
                || rest.starts_with(". What its previous value drops ")
        })
}

/// `r = loop { if c { break a; } else { break b; } };` (#561, #586):
/// the reassignment's line (`acquire` and `reassign` are on one dot)
/// and the receiver's "acquires ownership of a resource (from whichever
/// break ended the loop)" say one thing: "r acquires ownership of a new
/// resource (from whichever break ended the loop); its previous
/// resource is dropped". `None` unless `acquire` is that line, for the
/// variable `reassign` is about.
pub fn reassign_with_break_alt(reassign: &str, acquire: &str) -> Option<String> {
    const NEW: &str = " acquires ownership of a new resource";
    [break_alt_receiver_suffix(), break_alt_block_receiver_suffix()].into_iter().find_map(|suffix| {
        let name = acquire.strip_suffix(&format!(" acquires ownership of a resource{}", suffix))?;
        let rest = reassign.strip_prefix(name)?.strip_prefix(NEW)?;
        Some(format!("{name}{NEW}{}{rest}", suffix))
    })
}

// A drop of an `Rc` / `Arc` / `Weak` doesn't necessarily free the value
// it points to: it lowers a count, and only the last `Rc` / `Arc` frees
// the value (#373). Appended wherever a drop of such an owner is
// stated; empty for any other owner. RustViz doesn't count references,
// so it can't say whether this is the last one.
/// Splits a `drop_note` for a sentence where "is dropped" isn't the end:
/// a holds note (" (including every String it holds)", #542) goes right
/// after "resource", before "is dropped"; an Rc note stays after it.
/// Returns (before "is dropped", after it).
pub fn split_drop_note(note: &str) -> (&str, &str) {
    if note.starts_with(" (including ") { (note, "") } else { ("", note) }
}

pub fn rc_drop_note(rc: Option<RcDrop>) -> String {
    match rc {
        None => String::new(),
        Some(RcDrop::Own(k)) if k.is_weak() => String::from(
            " (a Weak doesn't keep the value alive; dropping it frees only the Weak itself)"),
        Some(RcDrop::Own(k)) => format!(
            " (for an {0}, that lowers the reference count; the shared value is freed only when its last {0} is dropped)",
            k.strong_name()),
        Some(RcDrop::Inside { kind, buffer: true, .. }) if kind.is_weak() => String::from(
            " (each Weak in it doesn't keep its value alive; its own buffer is freed)"),
        Some(RcDrop::Inside { kind, buffer: true, .. }) => format!(
            " (each {} in it only lowers its reference count; its own buffer is freed)",
            kind.strong_name()),
        Some(RcDrop::Inside { kind, many: false, .. }) if kind.is_weak() => String::from(
            " (the Weak inside doesn't keep its value alive)"),
        Some(RcDrop::Inside { kind, many: true, .. }) if kind.is_weak() => String::from(
            " (the Weaks inside don't keep their values alive)"),
        Some(RcDrop::Inside { kind, many: false, .. }) => format!(
            " (the {} inside only lowers its reference count)", kind.strong_name()),
        Some(RcDrop::Inside { kind, many: true, .. }) => format!(
            " (the {}s inside only lower their reference counts)", kind.strong_name()),
        Some(RcDrop::Fields(k)) if k.is_weak() => String::from(
            " (its Weak fields don't keep their values alive; dropping them frees only the Weaks)"),
        Some(RcDrop::Fields(k)) => format!(
            " (its {} fields only lower their reference counts)", k.strong_name()),
    }
}

// `let b = Rc::clone(&a);` / `a.clone()` / `Rc::downgrade(&a)`: the new
// owner points to the same value; nothing is copied (#373).
pub fn event_dot_rc_call_acquire(my_name: &String, call: RcCall) -> String {
    let my_name_fmt = styled_name_html(my_name);
    match call {
        RcCall::Clone(k) if k.is_weak() => format!(
            "{0} is another Weak pointing to the same value (Weak::clone increases the weak count; nothing is copied)",
            my_name_fmt),
        RcCall::Clone(k) => format!(
            "{0} is another {1} to the same value ({1}::clone increases the reference count; nothing is copied)",
            my_name_fmt, k.strong_name()),
        RcCall::Downgrade(k) => format!(
            "{0} is a Weak pointing to the same value ({1}::downgrade increases the weak count; nothing is copied)",
            my_name_fmt, k.strong_name()),
    }
}

pub fn arrow_rc_call(fn_name: &String, to_name: &String, call: RcCall) -> String {
    let (fn_fmt, to_fmt) = (styled_name_html(fn_name), styled_name_html(to_name));
    match call {
        RcCall::Clone(k) if k.is_weak() => format!(
            "{0} makes another Weak pointing to the same value (nothing is copied), now owned by {1}",
            fn_fmt, to_fmt),
        RcCall::Clone(k) => format!(
            "{0} makes another {1} to the same value (nothing is copied), now owned by {2}",
            fn_fmt, k.strong_name(), to_fmt),
        RcCall::Downgrade(k) => format!(
            "{0} makes a Weak pointing to the same value ({1}'s strong count is unchanged; nothing is copied), now owned by {2}",
            fn_fmt, k.strong_name(), to_fmt),
    }
}

// `r?` with `r` a non-`Copy` Result / Option moves `r`: `Try::branch`
// takes it by value (#652). On success the value inside goes on to
// `target` (the variable `r?` is bound to, "the caller", or `None`: the
// expression `r?` itself); on failure the function returns early and
// the error (converted with `From`) or the `None` goes to the caller.
// `value`: what `r` holds needs no drop (#484).
pub fn event_dot_move_into_try(my_name: &String, target: Option<&String>, kind: TryMove, value: bool) -> String {
    let name = styled_name_html(my_name);
    let gets = match target {
        Some(t) => format!("{} gets the value inside it", styled_name_html(t)),
        None => format!("`{}?` gives the value inside it", my_name),
    };
    let what = if value { "value" } else { "resource" };
    match kind {
        TryMove::Result => format!(
            "{name}'s {what} is moved into the `?`. If {name} is `Ok`, {gets}; if it is `Err`, the function returns early and the error goes to the caller."
        ),
        TryMove::Option => format!(
            "{name}'s {what} is moved into the `?`. If {name} is `Some`, {gets}; if it is `None`, the function returns `None` early."
        ),
        TryMove::Other => format!(
            "{name}'s {what} is moved into the `?`. If {name} holds a value to go on with, {gets}; otherwise the function returns early."
        ),
    }
}

// The other side of event_dot_move_into_try: `n` in `let n = r?;`.
// `holds`: "is initialized with the value", "acquires ownership of the
// resource", "is bound to the mutable reference", …
pub fn event_dot_acquire_from_try(my_name: &String, from: &String, kind: TryMove, holds: &str) -> String {
    let inside = match kind {
        TryMove::Result => format!("{}'s `Ok`", styled_name_html(from)),
        TryMove::Option => format!("{}'s `Some`", styled_name_html(from)),
        TryMove::Other => styled_name_html(from),
    };
    format!("{} {holds} the `?` takes out of {inside}", styled_name_html(my_name))
}

// `x?` returns from the function on `Err` / `None`; every value the
// function still owns is dropped there (#430). The dot sits on the
// owner's column, which goes on: on success nothing happens here.
// `note`: what else the drop drops, or an Rc's count note (#542).
pub fn event_dot_owner_drop_if_try_returns(my_name: &String, note: &str) -> String {
    let my_name_fmt = styled_name_html(my_name);
    let (holds, rest) = split_drop_note(note);
    format!(
        "Only if the `?` here returns early (on an `Err` or `None`): the function returns and {0}'s \
         resource{1} is dropped{2}. If the `?` succeeds, nothing happens to {0} here.",
        my_name_fmt, holds, rest
    )
}

// `p.f = f()?;` (#1151): p's own dot. If the `?` succeeds, the assignment
// gives the part `part` (drawn on its own column) a new value.
pub fn event_dot_owner_drop_if_try_returns_part(my_name: &String, note: &str, part: &str) -> String {
    let my_name_fmt = styled_name_html(my_name);
    let (holds, rest) = split_drop_note(note);
    format!(
        "Only if the `?` here returns early (on an `Err` or `None`): the function returns and {0}'s \
         resource{1} is dropped{2}. If the `?` succeeds, the assignment to {3} runs.",
        my_name_fmt, holds, rest, styled_name_html(&part.to_string())
    )
}

// A dot that says more than the drop at a `?` (#1151): "If the `?`
// succeeds, nothing happens to s here" would contradict the dot's other
// lines (s is borrowed by `s.push_str(&f()?)`, moved by `(s, f()?)`).
// What is true either way is that the `?` itself doesn't drop it.
pub fn try_drop_with_other_events(drop_text: &str) -> Option<String> {
    let (head, owner) = drop_text.split_once(". If the `?` succeeds, nothing happens to ")?;
    let owner = owner.strip_suffix(" here.")?;
    Some(format!("{head}. If the `?` succeeds, {owner}'s resource isn't dropped here."))
}

// The same for a variable that holds a value on some paths only (moved,
// or given one, in some branches above, #1194): its drop flag decides
// whether the early return drops it.
pub fn event_dot_owner_drop_if_try_returns_maybe(my_name: &String, note: &str) -> String {
    let my_name_fmt = styled_name_html(my_name);
    let (holds, rest) = split_drop_note(note);
    format!(
        "Only if the `?` here returns early (on an `Err` or `None`): the function returns and {0}'s \
         resource{1} is dropped{2} if {0} still holds it (Rust tracks this with a runtime drop flag). \
         If the `?` succeeds, nothing happens to {0} here.",
        my_name_fmt, holds, rest
    )
}

// The same for a `let s;` local that only some branches above assigned
// (#244, #1436).
pub fn event_dot_owner_drop_if_try_returns_maybe_init(my_name: &String, note: &str, moved: bool) -> String {
    let my_name_fmt = styled_name_html(my_name);
    let (holds, rest) = split_drop_note(note);
    format!(
        "Only if the `?` here returns early (on an `Err` or `None`): the function returns and {0}'s \
         resource{1} is dropped{2} if the branch that ran {3} (Rust tracks this with a runtime drop \
         flag). If the `?` succeeds, nothing happens to {0} here.",
        my_name_fmt, holds, rest, assigned_in_branch_that_ran(moved)
    )
}

// The same for a struct some fields of which were moved out before the
// `?` (#1184): `scope_end` is what its scope end would say there
// (`event_dot_struct_partial_go_out_of_scope`), "p goes out of scope. p.a
// was moved out earlier, so only p.b is dropped."
pub fn event_dot_owner_drop_if_try_returns_partial(my_name: &String, scope_end: &str) -> String {
    let my_name_fmt = styled_name_html(my_name);
    let rest = scope_end.strip_prefix(&format!("{} goes out of scope. ", my_name_fmt)).unwrap_or(scope_end);
    format!(
        "Only if the `?` here returns early (on an `Err` or `None`): the function returns, and drops \
         what is left of {0}: {1} If the `?` succeeds, nothing happens to {0} here.",
        my_name_fmt, rest
    )
}

// `let n = a? + b?;`: b's dot holds both a's `?` (b is dropped if it
// returns early) and b's own `?` (moves b). "If the `?` succeeds, nothing
// happens to b here" is about a's `?` only, but read next to the move it
// looks contradictory (#668). When the move is on the same dot, say which
// `?` each line is about, in evaluation order.
pub fn try_drop_before_own_try(drop_text: &str) -> Option<String> {
    let (lead, tail) = ("Only if the `?` here returns early", ". If the `?` succeeds, nothing happens to ");
    let rest = drop_text.strip_prefix(lead)?;
    let (head, owner) = rest.split_once(tail)?;
    let owner = owner.strip_suffix(" here.")?;
    Some(format!(
        "Only if an earlier `?` on this line returns early{head}. If it succeeds, {owner} is not dropped there; {owner}'s own `?` below moves it"
    ))
}

// `s = f()?;` (#1151): the `?` is part of the right-hand side, which runs
// before the assignment. If it returns early, the assignment never
// happens and s's current resource is dropped by the return; if it
// succeeds, the assignment gives s its new resource. The dot holds the
// drop text (event_dot_owner_drop_if_try_returns) and the reassignment's
// own text; this says the reassignment happens only on success, in place
// of "nothing happens to s here".
pub fn try_drop_then_reassign(drop_text: &str, reassign: &str) -> Option<String> {
    let (head, owner) = drop_text.split_once(". If the `?` succeeds, nothing happens to ")?;
    owner.strip_suffix(" here.")?;
    if !reassign.contains(" acquires ownership of a new resource") {
        return None;
    }
    let reassign = reassign.strip_suffix('.').unwrap_or(reassign);
    Some(format!("{head}. If the `?` succeeds, the assignment runs: {reassign}."))
}

// `return if b { p } else { q };` in a branch (#697): the value the
// `return` hands to the caller is one of the `if`'s branch values; `p`
// is the one that isn't it on the paths through the others, where it is
// dropped as the function returns. The dot sits on p's column.
// `value`: p holds a value with nothing to drop. `note`: see
// event_dot_owner_drop_if_try_returns.
pub fn event_dot_owner_drop_unless_returned(my_name: &String, value: bool, note: &str) -> String {
    let my_name_fmt = styled_name_html(my_name);
    if value {
        return format!(
            "The `return` here hands the caller one of the values its `if` / `match` chooses between. \
             If that value isn't {0}'s, {0} goes out of scope here as the function returns (it has \
             nothing to drop). If it is {0}'s, {0}'s value goes to the caller.",
            my_name_fmt
        );
    }
    let (holds, rest) = split_drop_note(note);
    format!(
        "The `return` here hands the caller one of the values its `if` / `match` chooses between. \
         If that value isn't {0}'s, {0}'s resource{1} is dropped here as the function returns{2}. \
         If it is {0}'s, its resource goes to the caller instead and isn't dropped.",
        my_name_fmt, holds, rest
    )
}

// A variable moved (or copied) into the temporary that a tuple / struct
// / array literal on the right of a `let` builds, where the pattern
// doesn't bind it by value (`_`, `..`, a `ref` binding): #439. The
// temporary is dropped at the end of the statement, unless a `ref`
// binding borrows from it, which extends it to the end of the block.
pub fn event_dot_literal_temporary_acquire(from_name: &String, ends_with_statement: bool, assign: bool) -> String {
    if assign {
        return format!(
            "The temporary that the literal on the right of this destructuring assignment builds takes the value of {0}. Rust drops it, with that value, as soon as the assignment has taken it apart, before the places on the left get their new values",
            styled_name_html(from_name)
        );
    }
    let when = if ends_with_statement {
        "No `ref` binding borrows from it, so Rust drops it at the end of this statement"
    } else {
        "A `ref` binding borrows from it, so it lives until the end of the enclosing block, not just to the end of the statement"
    };
    format!(
        "The temporary that the literal on the right of this let builds takes the value of {0}. {1}",
        styled_name_html(from_name), when
    )
}

// The same for a call's result in that literal (`mk()` in `let (a, _) =
// (1, mk());`, #1376), which the pattern doesn't bind by value; `assign`:
// the literal of a destructuring assignment (`(a, _) = (1, mk());`, #1387).
pub fn event_dot_literal_temporary_takes_result(from_name: &String, ends_with_statement: bool, assign: bool) -> String {
    if assign {
        return format!(
            "The temporary that the literal on the right of this destructuring assignment builds takes the value that {0}. Rust drops it, with whatever the pattern doesn't assign to a place, as soon as the assignment has taken it apart, before the places on the left get their new values",
            temporary_source(from_name)
        );
    }
    let when = if ends_with_statement {
        "No `ref` binding borrows from it, so Rust drops it at the end of this statement, with whatever the pattern didn't bind by value"
    } else {
        "A `ref` binding borrows from it, so it lives until the end of the enclosing block, not just to the end of the statement"
    };
    format!(
        "The temporary that the literal on the right of this let builds takes the value that {0}. {1}",
        temporary_source(from_name), when
    )
}

// The start of the temporary of a literal on the right of a `let`, when
// no variable is moved into it (every element the pattern leaves is a
// fresh value, #483). Dropped at the end of the statement: with a `ref`
// binding it is the extended kind, whose start says why it lives on.
pub fn event_dot_literal_temporary_start(assign: bool) -> String {
    if assign {
        // `(x, _) = (m(), n());` (#1387).
        return "The literal on the right of this destructuring assignment builds a temporary. Rust drops it, with whatever the pattern doesn't assign to a place, as soon as the assignment has taken it apart, before the places on the left get their new values".to_owned();
    }
    "The literal on the right of this let builds a temporary. No `ref` binding borrows from it, so Rust drops it at the end of this statement, with whatever the pattern didn't bind by value".to_owned()
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
    let my_name_fmt = styled_name_html(my_name);
    
    format!(
        "{0} is initialized as the function argument",
        my_name_fmt
    )
}


/* The Event dot is the source of an arrow, we typically follow the following format: 
   ... to <Resource Owner 1>
 */

// A pattern copies a value inside my_name out (`if let Some(x) = opt`,
// `Fruit::Apple(z) =>` on `match x`): not all of my_name, which need
// not even be Copy (#866).
pub fn event_dot_copy_inside_to(my_name: &String) -> String {
    format!("a value inside {0} is copied", styled_name_html(my_name))
}

// A `&` sub-pattern copies the value a reference stored inside my_name
// points to (`if let Some(&x) = orr`): not my_name's value, nor a value
// inside it (#874). `part`: only a value inside that one (`Some(&(a,
// b))`).
pub fn event_dot_copy_pointee_to(my_name: &String, part: bool) -> String {
    if part {
        format!("a value inside the value a reference inside {0} points to is copied", styled_name_html(my_name))
    } else {
        format!("the value a reference inside {0} points to is copied", styled_name_html(my_name))
    }
}

// 1   0
//     |
// o<--*   the star event
// |   |
// Only Copy types are copied, so this always says "value" (#274); a
// `&T` being copied has its own message (event_dot_ref_copied).
pub fn event_dot_copy_to(my_name: &String) -> String {
    // update styling
    let my_name_fmt = styled_name_html(my_name);
    
    format!(
        "{0}'s value is copied",
        my_name_fmt
    )
}

// 1   0
//     |
// o<--*   the star event
// |
pub fn event_dot_move_to(my_name: &String, holds_value: bool) -> String {
    format!("{0}'s {1} is moved", styled_name_html(my_name), held(holds_value))
}

// `p = Pair { a: .., b: p.b };` / `t = (s, t.1);` (#1411): the
// right-hand side runs first and moves p.b into the new value, which
// the assignment then puts into p, so p.b holds the same resource again.
pub fn event_dot_move_out_for_new_value(my_name: &String, holds_value: bool) -> String {
    format!("{0}'s {1} is moved into the new value the right-hand side builds (it runs before the assignment)",
        styled_name_html(my_name), held(holds_value))
}

pub fn event_dot_back_with_new_value(my_name: &String, holds_value: bool) -> String {
    format!("{0} gets the same {1} back, as part of the new value assigned here",
        styled_name_html(my_name), held(holds_value))
}

// `let [_, b] = arr;` after `let [a, _] = arr;` (#1302): the move of the
// last element(s) still in the array; nothing is left in it.
pub fn event_dot_move_last_elems(place: &String, arr: &String, holds_value: bool) -> String {
    format!("{0}'s {1} is moved; nothing is left in {2}", styled_name_html(place), held(holds_value), styled_name_html(arr))
}

// Source dot for the head of a `for` loop that is moved into the
// iterator `IntoIterator::into_iter` builds from it (`for s in v`, `v:
// Vec<String>`), when that iterator has drop glue (#482). Its drop is
// stated where the loop ends, on the iterator's `(temporary N)` column
// (#469).
pub fn event_dot_move_into_loop_iterator(my_name: &String) -> String {
    format!(
        "{0}'s resource is moved into the `for` loop's iterator",
        styled_name_html(my_name)
    )
}

// Source dot for a `for` loop's head that is an iterator already (`for
// s in it`, `it: vec::IntoIter<String>`): `into_iter` hands it back
// unchanged, so it becomes the loop's iterator, and the loop drops it
// (`event_dot_loop_iterator_drop`) (#482).
pub fn event_dot_move_iterator_into_loop(my_name: &String) -> String {
    format!(
        "{0}'s resource is moved into the `for` loop, which uses it as its iterator",
        styled_name_html(my_name)
    )
}

// The drop of the iterator a `for` loop took from a variable (`for s in
// it`), on that variable's column: where the loop ends (also when a
// `break` leaves it), or at a `return` from inside the loop (`early`)
// (#482). What the iterator still owns goes with it: the elements it
// hasn't handed out, a `vec::IntoIter`'s buffer.
pub fn event_dot_loop_iterator_drop(my_name: &String, left: LoopLeft, early: &[usize]) -> String {
    match left {
        LoopLeft::Return => format!(
            "{0}'s former resource, the `for` loop's iterator, is dropped here: the function returns from inside the loop. Whatever the iterator still owns is dropped with it (such as elements it hasn't handed out yet)",
            styled_name_html(my_name)
        ),
        LoopLeft::Break => format!(
            "{0}'s former resource, the `for` loop's iterator, is dropped here: this `break` leaves the `for` loop. Whatever the iterator still owns is dropped with it (such as elements it hasn't handed out yet)",
            styled_name_html(my_name)
        ),
        LoopLeft::Try => format!(
            "Only if the `?` here returns early (on an `Err` or `None`): the function returns from inside the `for` loop and {0}'s former resource, the loop's iterator, is dropped, along with whatever it still owns (such as elements it hasn't handed out yet). If the `?` succeeds, nothing happens to the iterator here.",
            styled_name_html(my_name)
        ),
        LoopLeft::Panic => format!(
            "{0}'s former resource, the `for` loop's iterator, is dropped here, as the panic unwinds out of the loop. Whatever the iterator still owns is dropped with it (such as elements it hasn't handed out yet)",
            styled_name_html(my_name)
        ),
        LoopLeft::Continue => format!(
            "{0}'s former resource, the `for` loop's iterator, is dropped here: this `continue` leaves the `for` loop and goes on to the next iteration of the outer loop. Whatever the iterator still owns is dropped with it (such as elements it hasn't handed out yet)",
            styled_name_html(my_name)
        ),
        LoopLeft::End => format!(
            "{0}'s former resource, the `for` loop's iterator, is dropped when the `for` loop ends{1}, along with whatever it still owns (such as elements it hasn't handed out yet)",
            styled_name_html(my_name), or_at_returns(early)
        ),
    }
}

// Source dot for a by-value capture of a closure literal passed to a
// call (`thread::spawn(move || ..v..)`). The resource goes into the
// closure, and is dropped when the closure is, not when the call returns.
pub fn event_dot_move_into_closure_arg(my_name: &String, callee: &String, value: bool) -> String {
    if value {
        return format!(
            "{0}'s value is moved into a closure passed to {1}",
            styled_name_html(my_name),
            styled_name_html(callee)
        );
    }
    format!(
        "{0}'s resource is moved into a closure passed to {1}; it's dropped when that closure is dropped",
        styled_name_html(my_name),
        styled_name_html(callee)
    )
}

// Source dot for a by-value capture of a closure literal that the fn
// returns (`move |x| s.len() + x` as the tail): the closure goes to the
// caller carrying the captured value, so the value isn't dropped here.
pub fn event_dot_move_into_returned_closure(my_name: &String, value: bool) -> String {
    format!(
        "{0}'s {1} is moved into the returned closure, which goes to the caller",
        styled_name_html(my_name),
        held(value)
    )
}

pub fn event_dot_copy_into_returned_closure(my_name: &String) -> String {
    format!(
        "{0}'s value is copied into the returned closure",
        styled_name_html(my_name)
    )
}

pub fn event_dot_copy_into_closure_arg(my_name: &String, callee: &String) -> String {
    format!(
        "{0}'s value is copied into a closure passed to {1}",
        styled_name_html(my_name),
        styled_name_html(callee)
    )
}

// `drop(s);`: the value is moved into `std::mem::drop`, which drops it.
pub fn event_dot_move_to_drop(my_name: &String, fn_name: &String, value: bool, note: &str) -> String {
    format!(
        "{0}'s {2} is moved into {1}, which drops it{3}",
        styled_name_html(my_name),
        styled_name_html(fn_name),
        held(value),
        note
    )
}

// `let (ref a, ..) = (s, t);`: s is moved into the tuple temporary,
// which has no column. That temporary is dropped at the end of the
// statement, or, when a `ref` binding borrows from it, at the end of
// the block; this tooltip is where that drop is stated.
pub fn event_dot_move_to_temporary(my_name: &String, extended: bool) -> String {
    format!(
        "{0}'s resource is moved into a temporary that is dropped at the end of {1}",
        styled_name_html(my_name),
        if extended { "the block (a `ref` binding borrows from it)" } else { "this statement" }
    )
}

// `let _ = (p, 5);` with `p: P { a: i32 }` (#1301): p is moved into the
// tuple, which has nothing to drop and is discarded at the end of the
// statement.
pub fn event_dot_move_value_to_temporary(my_name: &String) -> String {
    format!(
        "{0}'s value is moved into a temporary that is discarded at the end of this statement (it has nothing to drop)",
        styled_name_html(my_name)
    )
}

// `match Some(c) { Some(_) => .., None => .. }`, `if let Some(_) = Some(c)`:
// c is moved into the temporary the `match` / `if let` examines, and no
// arm binds it by value, so it is dropped with the temporary, when the
// scrutinee's scope ends (`when`, e.g. "when the `match` ends").
pub fn event_dot_move_to_scrutinee_temporary(my_name: &String, what: &str, when: &str) -> String {
    format!(
        "{0}'s resource is moved into a temporary that this {1} examines; the temporary is dropped {2}",
        styled_name_html(my_name),
        what,
        when
    )
}

// "the temporary tuple (p, q)", or just "the temporary tuple" when the
// scrutinee is too long to quote. A constructor scrutinee taken apart
// the same way (`match Some(f)`, #807) builds "the temporary value
// Some(f)" (its text never starts with a parenthesis).
fn temporary_tuple(tuple: &String) -> String {
    if tuple.is_empty() {
        "the temporary tuple".to_owned()
    } else if !is_tuple_text(tuple) {
        format!("the temporary value {}", styled_name_html(tuple))
    } else {
        format!("the temporary tuple {}", styled_name_html(tuple))
    }
}

// Whether a scrutinee temporary's text (see `temporary_tuple`) is a
// tuple's, rather than a constructor's.
fn is_tuple_text(tuple: &str) -> bool {
    tuple.is_empty() || tuple.starts_with('(')
}

// "tuple", or "temporary" for a constructor's value (#807).
fn tuple_noun(tuple: &str) -> &'static str {
    if is_tuple_text(tuple) { "tuple" } else { "temporary" }
}

// `if let (Some(_), Some(r)) = (p, q)` / `match (x, y)`: building the
// tuple moves p into it (#327). Where the tuple is dropped, a
// TupleTempDrop dot says so (event_dot_tuple_temp_drop).
pub fn event_dot_move_to_tuple_temporary(my_name: &String, tuple: &String, value: bool) -> String {
    format!("{0}'s {1} is moved into {2}", styled_name_html(my_name), held(value), temporary_tuple(tuple))
}

// The scrutinee tuple is dropped, and with it whatever of each
// variable's value the pattern didn't move out (#327). Timing is for
// edition 2024 (what RustViz compiles with): an `if let` drops its
// scrutinee's temporaries at the end of the if branch, or before the
// else branch when the pattern doesn't match; a `match` keeps them to
// the end of its statement.
pub fn event_dot_tuple_temp_drop(tuple: &String, at: TupleDropAt, with: &[TupleLeft]) -> String {
    let noun = tuple_noun(tuple);
    let tuple = temporary_tuple(tuple);
    let (lead, when) = match at {
        // Worded like event_dot_owner_drop_if_try_returns (#653).
        TupleDropAt::TryReturns => ("Only if the `?` here returns early (on an `Err` or `None`): the function returns and ", ""),
        TupleDropAt::IfEnd => ("", "here, at the end of the if branch"),
        TupleDropAt::IfExit => ("", "when the if branch leaves early"),
        TupleDropAt::Else => ("The pattern didn't match, so ", "here, before the else branch runs"),
        TupleDropAt::NoElse => ("The pattern didn't match, so ", "here"),
        TupleDropAt::MatchArm => ("", "after this arm, when the match ends"),
        TupleDropAt::MatchArmStmt => ("", "after this arm, at the end of the statement the match is in"),
        TupleDropAt::MatchArmExit => ("", "when this arm leaves the match early"),
        TupleDropAt::MatchHere => ("", "here, at the end of the match"),
    };
    let tuple = if lead.is_empty() {
        let mut c = tuple.chars();
        c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
    } else {
        tuple
    };
    // "p's resource", "p's and q's resources", "the rest of x's
    // resource and y's resource". An or-pattern arm whose alternatives
    // take from different variables (`(Some(x), _) | (_, Some(x))`,
    // #533): what's left of them depends on which alternative matched,
    // so "whatever of a's and b's resources x didn't take", listed
    // after the others ("c's resource and whatever of …").
    let (plain, taken): (Vec<&TupleLeft>, Vec<&TupleLeft>) = with.iter().partition(|w| w.takers.is_empty());
    let mut parts: Vec<String> = if plain.len() > 1 && plain.iter().all(|w| !w.rest && w.rc.is_none()) {
        let names: Vec<String> = plain.iter().map(|w| format!("{}'s", styled_name_html(&w.name))).collect();
        vec![format!("{} resources", join_and(&names))]
    } else {
        plain.iter().map(|w| format!(
            "{}{}'s resource{}",
            if w.rest { "the rest of " } else { "" },
            styled_name_html(&w.name),
            rc_drop_note(w.rc),
        )).collect()
    };
    let mut groups: Vec<(&Vec<String>, Vec<&TupleLeft>)> = Vec::new();
    for w in taken {
        // (An Rc's note goes on its own item.)
        match groups.iter_mut().find(|(t, ws)| **t == w.takers && w.rc.is_none() && ws.iter().all(|o| o.rc.is_none())) {
            Some((_, ws)) => ws.push(w),
            None => groups.push((&w.takers, vec![w])),
        }
    }
    for (takers, ws) in groups {
        let names: Vec<String> = ws.iter().map(|w| format!("{}'s", styled_name_html(&w.name))).collect();
        let notes: String = ws.iter().map(|w| rc_drop_note(w.rc)).collect();
        let takers: Vec<String> = takers.iter().map(|n| styled_name_html(n)).collect();
        parts.push(format!("whatever of {} resource{}{} {} didn't take",
            join_and(&names), if ws.len() > 1 { "s" } else { "" }, notes, join_and(&takers)));
    }
    let items = join_and(&parts);
    if at == TupleDropAt::TryReturns {
        return format!("{lead}{tuple} is dropped, and with it {items}. If the `?` succeeds, nothing happens to the {noun} here.");
    }
    format!("{lead}{tuple} is dropped {when}, and with it {items}")
}

// def fn(p):
//  p
//  |
//  *-->   the return event
//
pub fn event_dot_move_to_caller(my_name: &String, holds_value: bool) -> String {
    format!("{0}'s {1} is moved to the caller", styled_name_html(my_name), held(holds_value))
}

/// `let n = *slots[0];`: `place` is `*slots[0]`; the value copied is
/// the one the reference stored in `slots[0]` points to, not slots's own.
/// What is copied is Copy, so a value, never a resource (#1163).
pub fn event_dot_copy_through_element(place: &String) -> String {
    let elem = place.strip_prefix('*').unwrap_or(place).trim_start();
    format!("the value {0} points to is copied", styled_name_html(&elem.to_owned()))
}

pub fn event_dot_move_through_element(place: &String, holds_value: bool) -> String {
    let elem = place.strip_prefix('*').unwrap_or(place).trim_start();
    format!("the {1} {0} points to is moved", styled_name_html(&elem.to_owned()), held(holds_value))
}

pub fn event_dot_copy_to_caller(my_name: &String) -> String {
    let my_name_fmt = styled_name_html(my_name);
    format!(
        "{0}'s value is copied to the caller",
        my_name_fmt
    )
}

/// Returning a shared reference (`&T` is Copy) copies the reference,
/// never the data behind it, so don't say "s's resource is copied".
pub fn event_dot_copy_ref_to_caller(my_name: &String) -> String {
    let my_name_fmt = styled_name_html(my_name);
    format!(
        "a reference from {0} is returned to the caller; the data it points to is not copied",
        my_name_fmt
    )
}

// A reference variable is itself moved (`let r2 = r;` with r: `&mut T`,
// or returned) or copied (`&T` is Copy). Only the reference moves or is
// copied; the resource it points to stays where it is, still owned by
// its lender (#178).
// `pointee_copy`: the referent doesn't need drop, so it's a "value"
// (`&i32`, `&str`), not a "resource" (#551). A `&T` is moved only as
// a field of a struct that is moved (`let hold = other;`, #637):
// `other.r` can't be used after that either.
pub fn event_dot_ref_moved(my_name: &String, to_caller: bool, pointee_copy: bool, is_mut: bool) -> String {
    format!(
        "{0}'s {3} reference is moved{1} (not the {2} it points to)",
        styled_name_html(my_name),
        if to_caller { " to the caller" } else { "" },
        held(pointee_copy),
        if is_mut { "mutable" } else { "immutable" }
    )
}

pub fn event_dot_ref_copied(my_name: &String, pointee_copy: bool) -> String {
    format!(
        "{0}'s reference is copied (not the {1} it points to)",
        styled_name_html(my_name),
        held(pointee_copy)
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
// `holds_value`: X is a Copy-typed owner, so its "value" is borrowed (#274).
pub fn event_dot_static_lend(my_name: &String, target_name: &String, omit_read_only: bool, holds_value: bool) -> String {
    let my_name_fmt = styled_name_html(my_name);
    let target_name_fmt = styled_name_html(target_name);
    if omit_read_only {
        format!(
            "{0}'s {2} is immutably borrowed by {1}",
            my_name_fmt, target_name_fmt, held(holds_value)
        )
    } else {
        format!(
            "{0}'s {2} is immutably borrowed by {1}; {0} is read-only while the borrow is live",
            my_name_fmt, target_name_fmt, held(holds_value)
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
pub fn event_dot_mut_lend(my_name: &String, target_name: &String, holds_value: bool) -> String {
    let my_name_fmt = styled_name_html(my_name);
    let target_name_fmt = styled_name_html(target_name);
    format!(
        "{0}'s {2} is mutably borrowed by {1}; {0} can't be used while the borrow is live",
        my_name_fmt, target_name_fmt, held(holds_value)
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
    let r = styled_name_html(ref_name);
    let t = styled_name_html(target_name);
    let noun = held(pointee_copy);
    match (mutable, ref_is_mut) {
        (true, true) => format!(
            "the {2} {0} points to is mutably borrowed by {1} through {0}; {0} can't be used while the borrow is live",
            r, t, noun),
        (true, false) => event_dot_mut_lend_ref_var(ref_name, target_name),
        (false, true) => format!(
            "the {2} {0} points to is immutably borrowed by {1} through {0}; {0} is read-only while the borrow is live",
            r, t, noun),
        (false, false) => format!(
            "the {2} {0} points to is immutably borrowed by {1}",
            r, t, noun),
    }
}

// Lender dot of `&mut r` with r a reference variable: r itself is
// borrowed (the borrower is a `&mut &mut T` / `&mut &T`), not what it
// points to. Always so for a `&T` r, which can't hand out mutable
// access to its referent; for a `&mut T` r only when written `&mut r`,
// not `&mut *r` (#914).
pub fn event_dot_mut_lend_ref_var(ref_name: &String, target_name: &String) -> String {
    format!(
        "{0} is mutably borrowed by {1}; {0} can't be used while the borrow is live",
        styled_name_html(ref_name), styled_name_html(target_name))
}

// Where the borrow of event_dot_mut_lend_ref_var ends.
pub fn event_dot_mut_reacquire_ref_var(ref_name: &String) -> String {
    format!("{0} is no longer mutably borrowed", styled_name_html(ref_name))
}

// Lender dot of `&r` with r a reference variable: r itself is borrowed
// (the borrower is a `&&T` / `&&mut T`), not what it points to (that's
// `&*r`), so r can't be reassigned, nor a `&mut T` r written through,
// while the borrow is live (#925). The shared counterpart of
// event_dot_mut_lend_ref_var, worded like event_dot_static_lend.
pub fn event_dot_static_lend_ref_var(ref_name: &String, target_name: &String) -> String {
    format!(
        "{0} is immutably borrowed by {1}; {0} is read-only while the borrow is live",
        styled_name_html(ref_name), styled_name_html(target_name))
}

// Where the borrow of event_dot_static_lend_ref_var ends.
pub fn event_dot_static_reacquire_ref_var(ref_name: &String) -> String {
    format!("{0} is no longer immutably borrowed", styled_name_html(ref_name))
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
    let r = styled_name_html(ref_name);
    let noun = held(pointee_copy);
    if mutable && !ref_is_mut {
        return event_dot_mut_reacquire_ref_var(ref_name);
    }
    match borrower {
        Some(b) => format!("{1} no longer borrows the {2} {0} points to", r, styled_name_html(b), noun),
        None => format!("the {1} {0} points to is no longer borrowed through {0}", r, noun),
    }
}

// Lender dot of a method call whose receiver r is a reference to a
// reference (`r.len()` with `r: &&String`, `r.push('!')` with `r: &mut
// &mut String`): the call borrows what `*r` points to (`**r`), not the
// reference `*r` that r points to (#952). `derefs`: how many `*` name
// the reference that points to the borrowed place (1 for `**r`).
// Otherwise as event_dot_lend_through_ref.
// `pointer`: the reference pointing to what's borrowed (`*r`, `*k`, or
// an array element `k[0]`, #1319); `via`: what the call borrows through
// (r, k, k[0]); `ref_name`: the column's variable, which can't be used
// meanwhile (for `k[0]`, rustc's borrow checker counts any use of k's
// elements as a conflict).
pub fn event_dot_lend_deep(ref_name: &String, pointer: &str, via: &str, target_name: &String, mutable: bool, ref_is_mut: bool, holds_value: bool) -> String {
    let r = styled_name_html(ref_name);
    let place = styled_name_html(pointer);
    let via = styled_name_html(via);
    let t = styled_name_html(target_name);
    let noun = held(holds_value);
    match (mutable, ref_is_mut) {
        (true, _) => format!(
            "the {3} {0} points to is mutably borrowed by {1} through {4}; {2} can't be used while the borrow is live",
            place, t, r, noun, via),
        (false, true) => format!(
            "the {3} {0} points to is immutably borrowed by {1} through {4}; {2} is read-only while the borrow is live",
            place, t, r, noun, via),
        (false, false) => format!("the {2} {0} points to is immutably borrowed by {1}", place, t, noun),
    }
}

// Where the borrow of event_dot_lend_deep ends.
pub fn event_dot_reacquire_deep(pointer: &str, borrower: &String, holds_value: bool) -> String {
    let place = styled_name_html(pointer);
    format!("{1} no longer borrows the {2} {0} points to", place, styled_name_html(borrower), held(holds_value))
}

// Lender's dot where a reference copied out of a value that holds the
// borrow ends (`n` in `while let Some(n) = it.next()`, #575): n's part
// of the borrow is given back, but `holders` (`it`) still hold it, so
// the lender is still borrowed.
// `of_ref_var`: the lender is a reference variable x borrowed itself
// (`look(&x)` while `let rr = &x;` is live, #974), as in
// event_dot_static_lend_ref_var: "x is no longer borrowed by look, …".
pub fn event_dot_reacquire_still_borrowed(my_name: &String, from_name: &String, holders: &[String], mutable: bool, holds_value: bool, of_ref_var: bool) -> String {
    let holders: Vec<String> = holders.iter()
        .map(|n| if n == LOOP_ITERATOR_HOLDER { n.clone() } else { styled_name_html(n) })
        .collect();
    let subject = if of_ref_var {
        styled_name_html(my_name)
    } else {
        format!("{}'s {}", styled_name_html(my_name), held(holds_value))
    };
    format!(
        "{0} is no longer borrowed by {1}, but is still {2} borrowed by {3}",
        subject,
        styled_name_html(from_name),
        if mutable { "mutably" } else { "immutably" },
        holders.join(" and "),
    )
}

/// `event_dot_reacquire_still_borrowed` on a reference's column, for a
/// borrow of what it points to (`r = &*m; let r2 = r;`, r2's end on a row
/// above r's in a `match` arm, #1527): "the resource m points to is no
/// longer borrowed by r2, but is still immutably borrowed by r".
pub fn event_dot_reacquire_through_ref_still_borrowed(ref_name: &String, from_name: &String, holders: &[String], mutable: bool, pointee_copy: bool) -> String {
    let holders: Vec<String> = holders.iter()
        .map(|n| if n == LOOP_ITERATOR_HOLDER { n.clone() } else { styled_name_html(n) })
        .collect();
    format!(
        "the {0} {1} points to is no longer borrowed by {2}, but is still {3} borrowed by {4}",
        held(pointee_copy),
        styled_name_html(ref_name),
        styled_name_html(from_name),
        if mutable { "mutably" } else { "immutably" },
        holders.join(" and "),
    )
}

/// A dot where one reference's borrow ends and a copy of it takes the
/// loan over (`Some(third) => ..` on `third: Option<&i32>`): the end
/// (`event_dot_reacquire_still_borrowed`, "v's resource is no longer
/// borrowed by third, but is still immutably borrowed by third₂") already
/// names the new borrower, so the lend's "v's resource is immutably
/// borrowed by third₂; v is read-only while the borrow is live" says it
/// twice (#1450). One line: the end, with the lend's restriction clause.
/// `None` unless `lend` lends the same subject, the same way, to one of
/// the holders `still` names.
pub fn still_borrowed_with_lend<'a>(still: &'a str, lend: &'a str) -> Option<String> {
    let (subject, rest) = still.split_once(" is no longer borrowed by ")?;
    let (_, kind_holders) = rest.split_once(", but is still ")?;
    let (kind, holders) = kind_holders.split_once(" borrowed by ")?;
    // The clause after "; " starts with the lender's styled name (a bare
    // "; " also ends the escaped `&gt;` of every name).
    let split_clause = |s: &'a str| -> (&'a str, Option<&'a str>) {
        match s.find(&format!("; {}", SPAN_BEGIN)) {
            Some(i) => (&s[..i], Some(&s[i + 2..])),
            None => (s, None),
        }
    };
    // Already merged with one lend's clause (two holders, both lent here).
    let (holders, own_tail) = split_clause(holders);
    let lent_to = lend.strip_prefix(subject)?
        .strip_prefix(" is ")?
        .strip_prefix(kind)?
        .strip_prefix(" borrowed by ")?;
    let (holder, tail) = split_clause(lent_to);
    if !holders.split(" and ").any(|h| h == holder) {
        return None;
    }
    Some(match tail {
        Some(t) if own_tail != Some(t) => format!("{still}; {t}"),
        _ => still.to_owned(),
    })
}

// Source-side dot of a borrow stored somewhere RustViz draws no column
// for and can't name (`v.last_mut().unwrap().r = &x`; #282). The
// restriction on X is the same as for event_dot_static_lend /
// event_dot_mut_lend. `through_ref`: X is a reference, so what's
// borrowed is what X points to (Some(pointee_copy); see
// event_dot_lend_through_ref).
// `lender_holds_value`: X holds a "value" (Copy, or nothing to drop), not
// a "resource" (#484).
pub fn event_dot_lend_to_undrawn_place(my_name: &String, is_mut: bool, omit_read_only: bool, through_ref: Option<bool>, lender_holds_value: bool) -> String {
    let my_name_fmt = styled_name_html(my_name);
    let kind = if is_mut { "mutably" } else { "immutably" };
    let subject = match through_ref {
        Some(pointee_copy) => format!("the {} {} points to", held(pointee_copy), my_name_fmt),
        None => format!("{}'s {}", my_name_fmt, held(lender_holds_value)),
    };
    let base = format!(
        "{0} is {1} borrowed, and the reference is stored in a place that isn't drawn",
        subject, kind
    );
    if is_mut {
        format!("{0}; {1} can't be used while the borrow is live", base, my_name_fmt)
    } else if omit_read_only {
        base
    } else {
        format!("{0}; {1} is read-only while the borrow is live", base, my_name_fmt)
    }
}

// 0   1
//     |
// o<--o
// |   |
// *-->o   the star event (&)
pub fn event_dot_static_return(my_name: &String) -> String {
    // update styling
    let my_name_fmt = styled_name_html(my_name);
    
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
pub fn event_dot_mut_return(my_name: &String) -> String {
    // update styling
    let my_name_fmt = styled_name_html(my_name);
    
    format!(
        "{0}'s mutable borrow ends",
        my_name_fmt
    )
}

// The end of a shared reference made from a mutable borrow that gives
// nothing back (#1599): r's use of the loan ends, the loan itself only
// when no reference holding it (a copy r came from, a holder it was
// stored in) is used again.
pub fn event_dot_shared_ref_of_mut_return(my_name: &String) -> String {
    format!(
        "{0}'s borrow ends; a mutable loan behind it lasts while anything that holds it is still used",
        styled_name_html(my_name)
    )
}

// A dot that ends more than one of a reference's loans (`c = &y;` ends
// c's loan of `x` and its unused new loan of `y`, #803): name the lender
// of each, or the dot would say the same sentence twice.
pub fn event_dot_borrow_of_ends(my_name: &String, lender: &String, is_mut: bool) -> String {
    format!(
        "{0}'s {2} borrow of {1} ends",
        styled_name_html(my_name),
        styled_name_html(lender),
        if is_mut { "mutable" } else { "immutable" }
    )
}

/* The Event dot is the destination of an arrow, we typically follow the following format: 
   ... from <Resource Owner 1>
 */

// Copy of one element out of a resource: `let first = v[0];`. The
// element is copied; `holder` (or, when it is a reference, the resource
// it points to) still has all of its contents.
pub fn event_dot_copy_out_of_element(holder: &String, place: &String, holder_is_ref: bool, holds_value: bool) -> String {
    // `holds_value`: the holder (or the referent) needs no drop, so it
    // is a "value", not a "resource" (#484): `[[i32; 2]; 2]` is a value,
    // a `Vec<i32>` is a resource.
    if holder_is_ref {
        format!(
            "{0} is copied out of the {2} {1} points to",
            styled_name_html(place),
            styled_name_html(holder),
            held(holds_value)
        )
    } else {
        format!(
            "{0} is copied out of {1}'s {2}",
            styled_name_html(place),
            styled_name_html(holder),
            held(holds_value)
        )
    }
}

// `v[0]` / `ORIGIN.x` returned (#1052): only that element or field is
// copied to the caller; `holder` keeps all of its contents.
pub fn event_dot_copy_element_to_caller(holder: &String, place: &String, holder_is_ref: bool, holds_value: bool) -> String {
    format!("{} and returned to the caller", event_dot_copy_out_of_element(holder, place, holder_is_ref, holds_value))
}

// `&PRIMES` / `&mut TOTAL` / `&PRIMES[0]` returned (#1053): a reference to
// (part of) the static `holder` goes to the caller. Nothing is copied,
// and the borrow doesn't end in this function. `place` is the expression
// as written, with its `&` / `&mut `.
pub fn event_dot_static_ref_to_caller(holder: &String, place: &String) -> String {
    let (what, target) = match place.strip_prefix("&mut ") {
        Some(t) => ("a mutable reference", t),
        None => ("a reference", place.strip_prefix('&').unwrap_or(place)),
    };
    format!(
        "{what} to {0} is returned to the caller; the data it points to is not copied. {1} is a static, so the borrow can last for the whole program",
        styled_name_html(&target.trim().to_owned()),
        styled_name_html(holder)
    )
}

// In-place update of one element of a resource: `a[i] += 1;`. The
// element lives inside the resource `holder` owns (or, when `holder`
// is a reference, inside the resource it points to), so `holder`
// neither acquires a new resource nor drops its old one. `place` is
// the element as written (`v[i]`). An assignment `v[i] = x;` is not
// this: it reads like a plain assignment (`event_dot_copyable_assign`,
// `event_dot_owner_drop_at_reassign`, `event_dot_acquire`) with the
// place as the subject.
pub fn event_dot_write_element(holder: &String, place: &String, holder_is_ref: bool, holds_value: bool) -> String {
    // `*slots[0] += 10`: the element slots[0] is a reference (or Box);
    // the write goes through it to what it points to (#396). The
    // collection's own resource isn't what changes. `holds_value` is
    // about that pointee here.
    if let Some(elem) = place.strip_prefix('*').map(|e| e.trim_start().to_owned()) {
        format!(
            "the {2} {0} points to is modified through {1}",
            styled_name_html(&elem),
            styled_name_html(place),
            held(holds_value)
        )
    } else if holder_is_ref {
        format!(
            "the {2} {0} points to is modified through {1}",
            styled_name_html(holder),
            styled_name_html(place),
            held(holds_value)
        )
    } else {
        format!(
            "{0}'s {2} is modified through {1}",
            styled_name_html(holder),
            styled_name_html(place),
            held(holds_value)
        )
    }
}

// 1   0        1   0
// |            |   
// o-->*   or   o-->*     the star event
// |   |            |
pub fn event_dot_acquire_value(my_name: &String) -> String {
    format!("{0} is bound to a value", styled_name_html(my_name))
}

// The variable of a `for` loop over a collection moved into the loop
// (`for w in words`, `words: Vec<String>`): `words` goes into the loop's
// iterator, and each iteration moves one element out of it (#850).
pub fn event_dot_acquire_loop_element(my_name: &String, collection: &String) -> String {
    format!(
        "{0} takes ownership of an element of {1}",
        styled_name_html(my_name),
        styled_name_html(collection)
    )
}

// The same with Copy elements (`for y in ys`, `ys: Vec<i32>` or
// `Vec<&String>`): the collection is moved into the iterator, and each
// iteration gives y a copy of one element (#1372).
pub fn event_dot_acquire_loop_element_copy(my_name: &String, collection: &String) -> String {
    format!(
        "{0} is bound to a copy of an element of {1}",
        styled_name_html(my_name),
        styled_name_html(collection)
    )
}

// The same with `&mut` elements (`v: Vec<&mut String>`): r is a
// reference, which never owns; each iteration moves one out (#1372).
pub fn event_dot_acquire_loop_element_ref(my_name: &String, collection: &String) -> String {
    format!(
        "{0} is bound to a mutable reference moved out of {1}",
        styled_name_html(my_name),
        styled_name_html(collection)
    )
}

// `for (s, k) in names` (#1390): "the `.0` of each element of names",
// or "each element of v" for the whole element (`for &y in &v`).
pub fn element_part(element: &str, path: &str, collection: &str) -> String {
    if path.is_empty() {
        format!("{} of {}", element, collection)
    } else {
        format!("the `{}` of {} of {}", path, element, collection)
    }
}

// A binding of a destructuring `for` head (`for (s, k) in names`,
// `for &(a, b) in &pairs`; #1390): what it gets from each element.
pub fn event_dot_acquire_loop_element_part(my_name: &String, collection: &String, path: &str, copy: bool, is_ref: bool) -> String {
    let part = element_part("an element", path, &styled_name_html(collection));
    let name = styled_name_html(my_name);
    if copy {
        format!("{} is bound to a copy of {}", name, part)
    } else if is_ref {
        format!("{} is bound to a mutable reference moved out of {}", name, part)
    } else {
        format!("{} takes ownership of {}", name, part)
    }
}

pub fn event_dot_acquire(my_name: &String) -> String {
    // update styling
    let my_name_fmt = styled_name_html(my_name);

    format!(
        "{0} acquires ownership of a resource",
        my_name_fmt
    )
}

// Acquire of a temporary that a `let` borrows (`let r =
// &String::from("a");`, #400). Without a `let`, a temporary is dropped
// at the end of its statement; borrowing it in a `let` extends its
// lifetime to the end of the enclosing block, which is why it has a
// column until then.
pub fn event_dot_temporary_acquire(my_name: &String) -> String {
    format!(
        "{0} acquires ownership of a resource. This temporary value is borrowed by a let, so it lives until the end of the enclosing block, not just to the end of the statement",
        styled_name_html(my_name)
    )
}

// Same Acquire event shape, but the destination column holds a Copy
// type (i32, bool, etc. — no heap, no ownership semantics) and the
// source is Anonymous (a literal, an arithmetic expression, or a
// macro-internal expansion). Avoids the misleading "ownership"
// language for primitives. Used for both fresh `let n = 5;` bindings
// and reassignments like `n += 1;`.
pub fn event_dot_acquire_copyable(my_name: &String) -> String {
    let my_name_fmt = styled_name_html(my_name);

    format!(
        "{0} is bound to a copyable value",
        my_name_fmt
    )
}

// A Copy-typed place that already holds a value is assigned again
// (`n += 1;`, `p.x = 5;`, `n = m;`). Nothing is acquired or
// initialized: the old value is overwritten in place (#178).
// `r.n = 2;` through a reference to a local struct: the field column
// `s.n` is assigned a new value, reached through the place `r.n` (#854).
pub fn event_dot_assign_through(field: &str, place: &str) -> String {
    format!(
        "{0} is assigned a new value through {1}",
        styled_name_html(&field.to_owned()),
        styled_name_html(&place.to_owned())
    )
}

pub fn event_dot_copyable_assign(my_name: &String) -> String {
    format!("{0} is assigned a new value", styled_name_html(my_name))
}

pub fn event_dot_copyable_assign_copy(my_name: &String, from_name: &String) -> String {
    format!(
        "{0} is assigned a copy of {1}",
        styled_name_html(my_name),
        styled_name_html(from_name)
    )
}

// A reference variable that already holds a reference is assigned
// another one (#414): r was initialized earlier, so this is not an
// initialization. `from` is where the new value comes from, `copied`
// whether it was copied (`&T` is Copy) or moved (`&mut T`).
// - `r = s;`, s: `&T`: the reference in s is copied; what it points
//   to isn't (the counterpart of "s's reference is copied (not the
//   resource it points to)" on s's column).
// - `r = s;`, s: `&mut T`: the reference is moved out of s (the arrow
//   says "Move from s to r"; like any place assignment, the dot reads
//   like `r = s`, with no "(moved out of s)", #650).
// - `r = first(..);`: the call's result. Not "the reference first
//   returns": while shadowing bindings share a column (#173), the
//   result can be a new binding's non-reference value
//   (`let spaces = "  "; let spaces = spaces.len();`).
// None for any other source; the caller keeps its usual wording.
pub fn event_dot_ref_assign(my_name: &String, from: &ResourceAccessPoint, copied: bool, is_mut: bool) -> Option<String> {
    let r = styled_name_html(my_name);
    let f = styled_name_html(from.name());
    let kind = if is_mut { "mutable" } else { "immutable" };
    if from.is_fn() {
        Some(format!("{0} is assigned the result of {1}", r, f))
    } else if from.is_ref() && copied {
        Some(format!("{0} is assigned a copy of {1}'s reference", r, f))
    } else if from.is_ref() {
        Some(format!("{0} is assigned {1}'s {2} reference", r, f, kind))
    } else {
        None
    }
}

// Acquire whose destination column is a reference variable
// (`let s: &str = "hi";`, `let r2 = r;` for r: &mut T). A
// reference never owns what it points to, so "acquires ownership"
// would be false; say what the variable is bound to instead.
pub fn event_dot_ref_acquire(my_name: &String, is_mut: bool) -> String {
    let my_name_fmt = styled_name_html(my_name);
    if is_mut {
        format!("{0} is bound to a mutable reference", my_name_fmt)
    } else {
        format!("{0} is bound to an immutable reference", my_name_fmt)
    }
}

// What a reference that may point into any of several variables'
// resources points into (`for x in a.iter_mut().chain(b.iter_mut())`;
// #1082): "the resource a or b points to", "a's or b's resource", or
// each one in turn when they differ ("a's resource or the value b
// points to"). Each holder: (name, is a reference, holds a value).
fn one_of_holders(alts: &[(String, bool, bool)]) -> String {
    fn or_join(items: &[String]) -> String {
        match items {
            [init @ .., last] if !init.is_empty() => format!("{} or {}", init.join(", "), last),
            _ => items.join(""),
        }
    }
    match alts {
        [(_, is_ref, value), rest @ ..] if rest.iter().all(|(_, r, v)| r == is_ref && v == value) => {
            let names: Vec<String> = alts.iter().map(|(n, ..)| n.clone()).collect();
            if *is_ref {
                format!("the {} {} points to", held(*value), or_list(&names))
            } else {
                let owners: Vec<String> = names.iter().map(|n| format!("{}'s", styled_name_html(n))).collect();
                format!("{} {}", or_join(&owners), held(*value))
            }
        }
        _ => {
            let each: Vec<String> = alts.iter().map(|(n, is_ref, value)| holder_phrase(n, *is_ref, *value)).collect();
            or_join(&each)
        }
    }
}

// "the resource a points to" / "a's resource".
fn holder_phrase(holder: &String, holder_is_ref: bool, holds_value: bool) -> String {
    if holder_is_ref {
        format!("the {} {} points to", held(holds_value), styled_name_html(holder))
    } else {
        format!("{}'s {}", styled_name_html(holder), held(holds_value))
    }
}

// `*x += 1` drawn on a's column with x from `a.iter_mut().chain(b.iter_mut())`
// (#1082): x points into a's resource on the iterations over a and into
// b's on the others, so the write goes into a's only on some of them.
pub fn event_dot_write_element_one_of(holder: &String, place: &String, holder_is_ref: bool, holds_value: bool, ptr: &String, alts: &[(String, bool, bool)]) -> String {
    format!(
        "{} is modified through {} when {} points into it ({} points into {})",
        holder_phrase(holder, holder_is_ref, holds_value),
        styled_name_html(place),
        styled_name_html(ptr),
        styled_name_html(ptr),
        one_of_holders(alts)
    )
}

// An assignment through such an x (`*x = 0`), drawn on a's column: the
// assignment's own text `msg`, and when it lands there.
pub fn event_dot_write_one_of(msg: String, holder: &String, holder_is_ref: bool, holds_value: bool, ptr: &String, alts: &[(String, bool, bool)]) -> String {
    format!(
        "{}, when {} points into {} ({} points into {})",
        msg,
        styled_name_html(ptr),
        holder_phrase(holder, holder_is_ref, holds_value),
        styled_name_html(ptr),
        one_of_holders(alts)
    )
}

// In-place update through a dereference: `*r += 1;`, `r.f += 1;`. The
// place `*r` / `r.f` is (part of) what r points to (owned by r's lender,
// or by the Box), so nothing is acquired; the existing resource is
// modified. `ref_name` is r, `deref_name` is *r / r.f. `pointee_copy`:
// r is a reference to a Copy type (`&mut i32`), so what it points to is
// a "value", as on the lender's own column (#312). An assignment
// `*r = v` / `r.f = v` reads like a plain assignment instead (the place
// is the subject; see `event_dot_copyable_assign` and friends).
pub fn event_dot_write_through_deref(ref_name: &String, deref_name: &String, pointee_copy: bool) -> String {
    format!(
        "the {2} {0} points to is modified through {1}",
        styled_name_html(ref_name),
        styled_name_html(deref_name),
        held(pointee_copy)
    )
}

// `*r` returned with r a reference (`r: &i32`): the caller gets a copy
// of the value r points to, not a reference to it (#670). `**y` reads
// the value behind the reference y points to.
// `&self.s` / `&mut p.items[0]` returned (#784): a new reference to a
// place reached through the reference `r`. `place` is the expression as
// written, with its `&` / `&mut`. The data isn't copied, and `r`'s
// borrow goes on with the reference.
// `place` can also be one of the forms below (#1078).
pub fn event_dot_reborrow_to_caller(ref_name: &String, place: &String) -> String {
    let (what, target) = match place.strip_prefix("&mut ") {
        Some(t) => ("a mutable reference", t),
        None => ("a reference", place.strip_prefix('&').unwrap_or(place)),
    };
    let tail = "is returned to the caller; the data it points to is not copied";
    if let Some(call) = target.strip_prefix(REBORROW_VIA_CALL) {
        let (method, into) = call.split_once(REBORROW_THROUGH).unwrap_or((call, ref_name));
        return format!(
            "{what} into {0} (from {1}), reborrowed through {2}, {tail}",
            styled_name_html(&into.to_owned()),
            styled_name_html(&method.to_owned()),
            styled_name_html(ref_name)
        );
    }
    let (target, through) = match target.split_once(REBORROW_THROUGH) {
        Some((t, through)) => (t, through.to_owned()),
        None => (target, ref_name.to_owned()),
    };
    format!(
        "{what} to {0}, reborrowed through {1}, {tail}",
        styled_name_html(&target.trim().to_owned()),
        styled_name_html(&through)
    )
}

/// Marks, in the place of a returned reborrow (`DerefField`), a
/// reference a call returned rather than a place written out.
const REBORROW_VIA_CALL: char = '\u{1e}';
/// Separates, in the place of a returned reborrow, the place from the
/// reference it is reborrowed through when that isn't the column's own.
const REBORROW_THROUGH: char = '\u{1f}';

/// `v.get_mut(0).unwrap()` returned with `v` a reference: the place
/// `event_dot_reborrow_to_caller` reads as "a mutable reference into
/// v (from get_mut), reborrowed through v" (#1078). `into` is the
/// place the call borrows (`v`, `self.items`).
pub fn reborrow_place_via_call(is_mut: bool, method: &str, into: &str) -> String {
    format!("{}{REBORROW_VIA_CALL}{method}{REBORROW_THROUGH}{into}", if is_mut { "&mut " } else { "&" })
}

/// A place `reborrow_place_via_call` / `reborrow_place_through` made, as
/// a tooltip names it: the reference a call returned (`Some(id(r))` in
/// an `if let`'s scrutinee, `Some(x.trim())`) is "the reference id
/// returns", not the encoded place, whose separators aren't even valid
/// XML characters (#1563). Any other place is itself.
pub fn displayed_place(place: &str) -> String {
    if let Some((_, call)) = place.split_once(REBORROW_VIA_CALL) {
        let method = call.split(REBORROW_THROUGH).next().unwrap_or(call);
        return format!("the reference {} returns", method);
    }
    place.split(REBORROW_THROUGH).next().unwrap_or(place).to_owned()
}

/// `&v[0][1]` returned with `v[0]` a reference stored in what `v`
/// points to: `place` (`&v[0][1]`, with its `&`) is reborrowed through
/// `through` (`v[0]`), not through the column's own reference (#1078).
pub fn reborrow_place_through(place: &str, through: &str) -> String {
    format!("{place}{REBORROW_THROUGH}{through}")
}

pub fn event_dot_copy_deref_to_caller(ref_name: &String, place: &String) -> String {
    let depth = place.chars().take_while(|c| *c == '*').count();
    match depth {
        0 | 1 => format!(
            "a copy of the value {0} points to is returned to the caller",
            styled_name_html(ref_name)
        ),
        2 => format!(
            "a copy of {0}, the value behind the reference {1} points to, is returned to the caller",
            styled_name_html(place),
            styled_name_html(ref_name)
        ),
        _ => format!(
            "a copy of {0}, the value reached by following {1}'s references, is returned to the caller",
            styled_name_html(place),
            styled_name_html(ref_name)
        ),
    }
}

// Copy out of a field reached through a reference: `let a = p.x;`
// with p: &P. The value is read from what p points to (which p
// doesn't own), so it's phrased like the write case above, including
// "value" when p points to a Copy type (#312).
pub fn event_dot_copy_through_deref_field(ref_name: &String, place: &String, target_name: &String, pointee_copy: bool) -> String {
    let place = &displayed_place(place);
    format!(
        "the {3} {0} points to is read through {1} (copied to {2})",
        styled_name_html(ref_name),
        styled_name_html(place),
        styled_name_html(target_name),
        held(pointee_copy)
    )
}

// 1   0        1   0
// |            |   
// o-->*   or   o-->*     the star event
// |   |            |
pub fn event_dot_copy_from(my_name: &String, target_name: &String) -> String {
    format!(
        "{0} is initialized with a copy of {1}",
        my_name,
        target_name
    )
}

// A pattern binds a copy of a value inside `target_name` (`if let
// Some(x) = opt` with opt: Option<i32>, `Fruit::Apple(z) =>` on `match
// x`): the binding gets the payload, not all of opt (#866). "a value":
// a pattern can bind several (`Pair(a, b)`).
pub fn event_dot_copy_from_inside(my_name: &String, target_name: &String) -> String {
    format!(
        "{0} is initialized with a copy of a value inside {1}",
        my_name,
        target_name
    )
}

// A `&` sub-pattern binds a copy of the value a reference stored inside
// `target_name` points to (`let Some(&y) = orr` with orr: Option<&i32>):
// y gets a copy of the i32 behind the reference, which is not inside orr
// (#874). `part`: the binding is a value inside that one (`Some(&(a,
// b))`).
pub fn event_dot_copy_from_pointee(my_name: &String, target_name: &String, part: bool) -> String {
    if part {
        format!("{0} is initialized with a copy of a value inside the value a reference inside {1} points to", my_name, target_name)
    } else {
        format!("{0} is initialized with a copy of the value a reference inside {1} points to", my_name, target_name)
    }
}

// `if let Some(&x) = v.get(0)`: x gets a copy of the value the reference
// the call returned (inside an Option) points to, not the call's result
// or a value inside it (#874). The wrapper is worded as in
// `event_dot_init_from_call`.
pub fn event_dot_copy_pointee_of_call(my_name: &String, callee: &String, inside: Option<&str>, part: bool) -> String {
    let reference = reference_inside_call(callee, callee, inside);
    if part {
        format!("{0} is initialized with a copy of a value inside the value pointed to by {1}", my_name, reference)
    } else {
        format!("{0} is initialized with a copy of the value pointed to by {1}", my_name, reference)
    }
}

/// "the reference inside the Option get() returns": where a `&`
/// sub-pattern on a call's result finds its reference (#874). `shown` is
/// the callee as displayed (styled or not), `callee` its plain name.
fn reference_inside_call(shown: &str, callee: &str, inside: Option<&str>) -> String {
    let last = callee.rsplit("::").next().unwrap_or(callee);
    if last.starts_with(|c: char| c.is_uppercase()) {
        let article = if last.starts_with(|c: char| "AEIOU".contains(c)) { "an" } else { "a" };
        return format!("the reference inside {} {} value", article, last);
    }
    match inside {
        Some(w @ ("Ok" | "Some")) => format!("the reference inside the {} that {}() returns", w, shown),
        Some(w) if !w.is_empty() => format!("the reference inside the {} {}() returns", w, shown),
        _ => format!("a reference inside the value {}() returns", shown),
    }
}

pub fn event_dot_copyable_assign_copy_inside(my_name: &String, from_name: &String) -> String {
    format!(
        "{0} is assigned a copy of a value inside {1}",
        styled_name_html(my_name),
        styled_name_html(from_name)
    )
}

// A Copy-typed variable initialized by a call's result: `let n = v.len();`
// gets the value `len()` returns, not a copy of anything in the code. A
// tuple-struct or enum constructor (`Some(5)`, `None`) builds a value of
// that variant. `cast_to`: the call's result is cast (`v.len() as i32`).
//
// `inside`: the variable gets a value from inside what the call returns
// (`Some(m) = f()`, `f()?`): the call returns the Option / Result, not
// the variable's value. An unknown wrapper falls back to the neutral
// "bound to a copyable value".
// "a Some value" / "an Ok value" for a constructor's path (`Some`,
// `Shape::Circle`).
pub fn ctor_value_phrase(ctor: &str) -> String {
    let last = ctor.rsplit("::").next().unwrap_or(ctor);
    let article = if last.starts_with(|c: char| "AEIOU".contains(c)) { "an" } else { "a" };
    format!("{} {} value", article, last)
}

// `let x = Some(one());`: x holds a Some value built around the value the
// call returns, not that value itself (#1183).
pub fn event_dot_init_from_call_in_ctor(my_name: &String, callee: &String, ctor: &str, cast_to: Option<&str>) -> String {
    let value = match cast_to {
        Some(ty) => format!("the value {}() returns, converted to {}", callee, ty),
        None => format!("the value {}() returns", callee),
    };
    format!("{} is initialized with {} holding {}", my_name, ctor_value_phrase(ctor), value)
}

pub fn event_dot_init_from_call(my_name: &String, callee: &String, cast_to: Option<&str>, inside: Option<&str>) -> String {
    let ctor = callee.rsplit("::").next().unwrap_or(callee).starts_with(|c: char| c.is_uppercase());
    match inside {
        Some("") => return event_dot_acquire_copyable(my_name),
        // A constructor written in the pattern's scrutinee (`Some(5)`)
        // isn't a call that "returns" (#758).
        Some(_) if ctor => {
            let last = callee.rsplit("::").next().unwrap_or(callee);
            let article = if last.starts_with(|c: char| "AEIOU".contains(c)) { "an" } else { "a" };
            return format!("{0} is initialized with the value inside {2} {1} value", my_name, last, article);
        }
        Some(w @ ("Ok" | "Some")) => return format!("{0} is initialized with the value inside the {2} that {1}() returns", my_name, callee, w),
        Some(w) => return format!("{0} is initialized with the value inside the {2} {1}() returns", my_name, callee, w),
        None => {}
    }
    let last = callee.rsplit("::").next().unwrap_or(callee);
    if last.starts_with(|c: char| c.is_uppercase()) {
        let article = if last.starts_with(|c: char| "AEIOU".contains(c)) { "an" } else { "a" };
        return format!("{0} is initialized with {2} {1} value", my_name, last, article);
    }
    match cast_to {
        Some(ty) => format!("{0} is initialized with the value {1}() returns, converted to {2}", my_name, callee, ty),
        None => format!("{0} is initialized with the value {1}() returns", my_name, callee),
    }
}

// 0   1
//     |
// *<--o   the star event (&mut)
// |   |
// `holds_value`: the lender is a Copy-typed owner (#274).
pub fn event_dot_mut_borrow(my_name: &String, holds_value: bool) -> String {
    // update styling
    let my_name_fmt = styled_name_html(my_name);
    
    format!(
        "{0} mutably borrows a {1}",
        my_name_fmt, held(holds_value)
    )
}

// A shared reference holding a mutable loan (`let r: &String = &mut
// s;`, `let r = downgrade(&mut s);` with `fn(&mut String) -> &str`):
// r can only read, but s stays mutably borrowed while r is live
// (#1122).
pub fn event_dot_mut_borrow_by_shared_ref(my_name: &String, holds_value: bool) -> String {
    format!(
        "{0} is a shared reference made from a mutable borrow of a {1}: {0} can only read it, but the borrow stays mutable while {0} is live",
        styled_name_html(my_name), held(holds_value)
    )
}

// `let first = &*r;` with `r = &mut a`, then `r = &mut b;` while first is
// still used (#1182): first's own borrow is immutable, but its use keeps
// r's old mutable loan of a alive, so its end gives a back.
pub fn event_dot_reborrow_kept_mut_loan(my_name: &String, through: &str, lender: &str) -> String {
    format!(
        "{0}'s immutable borrow ends. {0} was made through {1}'s mutable borrow of {2}, which lasts as long as {0} is used, so {2} is mutably borrowed until here",
        styled_name_html(my_name), styled_name_html(&through.to_owned()), styled_name_html(&lender.to_owned())
    )
}

// A `&mut` holding a shared loan (`let r = &mut &x;`): r points to a
// temporary (the `&x` reference) that shares x (#1131).
pub fn event_dot_static_borrow_by_mut_ref(my_name: &String, holds_value: bool) -> String {
    format!(
        "{0} mutably borrows a temporary that immutably borrows a {1}",
        styled_name_html(my_name), held(holds_value)
    )
}

// Its end: r's own borrow of the temporary ends with the shared one.
pub fn event_dot_mut_ref_return_of_shared(my_name: &String) -> String {
    format!("{0}'s borrow ends", styled_name_html(my_name))
}

// 0   1
//     |
// *<--o   the star event (&)
// |   |
pub fn event_dot_static_borrow(my_name: &String, holds_value: bool) -> String {
    // update styling
    let my_name_fmt = styled_name_html(my_name);
    
    format!(
        "{0} immutably borrows a {1}",
        my_name_fmt, held(holds_value)
    )
}

// ─── Closure capture variants ────────────────────────────────────
//
// Used when the `to`/`is` of an event is a closure binding. Wording
// emphasizes that the move/borrow is happening because the closure
// captured the upvar, not because the user wrote an explicit `let
// y = x` / `let r = &x`.

// Source dot for a `move`-captured upvar. Mirrors event_dot_move_to.
pub fn event_dot_capture_move_to_closure(my_name: &String, target_name: &String, holds_value: bool) -> String {
    format!(
        "{0}'s {2} is captured (moved) by closure {1}",
        styled_name_html(my_name), styled_name_html(target_name), held(holds_value)
    )
}

// Source dot for a `&mut` reference a `move` closure captures (`move ||
// r.push('!')`, #1148): the reference moves into the closure, which
// holds the borrow from then on; the data it points to stays put.
pub fn event_dot_capture_move_ref_to_closure(my_name: &String, target_name: &String) -> String {
    format!(
        "{0}'s reference is captured (moved) by closure {1}; the data it points to is not moved",
        styled_name_html(my_name), styled_name_html(target_name)
    )
}

// Source dot for a Copy upvar captured by value (`move || m + 1`
// with m: i32; #311). The closure gets its own copy, so the upvar
// stays usable and the closure has nothing of it to drop. A `&T` is
// Copy too: the closure gets a copy of the reference, and the data it
// points to isn't copied.
pub fn event_dot_capture_copy_to_closure(my_name: &String, target_name: &String, is_ref: bool) -> String {
    let my_name_fmt = styled_name_html(my_name);
    let target_fmt = styled_name_html(target_name);
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
pub fn event_dot_capture_static_lend_to_closure(my_name: &String, target_name: &String, holds_value: bool) -> String {
    let my_name_fmt = styled_name_html(my_name);
    let target_fmt = styled_name_html(target_name);
    format!(
        "{0}'s {2} is captured (immutably borrowed) by closure {1}",
        my_name_fmt, target_fmt, held(holds_value)
    )
}

// Source dot for a reference variable a closure captures itself (`||
// println!("{}", r)`: the macro takes `&r`; #1180), not the place it
// points to. Like `&r` (#925): r is borrowed; r owns nothing.
pub fn event_dot_capture_ref_var_to_closure(ref_name: &String, target_name: &String, mutable: bool) -> String {
    let r = styled_name_html(ref_name);
    let c = styled_name_html(target_name);
    if mutable {
        format!("{0} is captured (mutably borrowed) by closure {1}; {0} can't be used while the borrow is live", r, c)
    } else {
        format!("{0} is captured (immutably borrowed) by closure {1}; {0} is read-only while the borrow is live", r, c)
    }
}

// Source dot for a mutably captured upvar. Mirrors event_dot_mut_lend.
pub fn event_dot_capture_mut_lend_to_closure(my_name: &String, target_name: &String, holds_value: bool) -> String {
    let my_name_fmt = styled_name_html(my_name);
    let target_fmt = styled_name_html(target_name);
    format!(
        "{0}'s {2} is captured (mutably borrowed) by closure {1}",
        my_name_fmt, target_fmt, held(holds_value)
    )
}

// Source dot on a reference r when a closure captures `*r` through it
// (`|| r.push('a')`, `|| r.len()`; #277): the closure reborrows what r
// points to. r owns nothing, so "r's resource is captured" was wrong.
// A `&mut` r can't be used while a mutable reborrow is live and is
// read-only during a shared one; a `&T` r was read-only already.
// `pointee_value`: what r points to is a value (Copy, or nothing to
// drop; #1163).
pub fn event_dot_capture_reborrow_through_ref(ref_name: &String, closure_name: &String, mutable: bool, ref_is_mut: bool, pointee_value: bool) -> String {
    let base = format!(
        "the {3} {0} points to is captured by closure {1} ({2} reborrow through {0})",
        styled_name_html(ref_name),
        styled_name_html(closure_name),
        if mutable { "a mutable" } else { "an immutable" },
        held(pointee_value)
    );
    if mutable {
        format!("{0}; {1} can't be used while the borrow is live", base, styled_name_html(ref_name))
    } else if ref_is_mut {
        format!("{0}; {1} is read-only while the borrow is live", base, styled_name_html(ref_name))
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
    let my_name_fmt = styled_name_html(my_name);
    if captures.is_empty() {
        // Closure with no captures (e.g. `let f = || println!("hi")`).
        return format!("Closure {0} is bound", my_name_fmt);
    }
    let parts: Vec<String> = captures
        .iter()
        .map(|(name, kind)| format!("{} ({})", styled_name_html(name), kind))
        .collect();
    format!(
        "Closure {0} captures: {1}",
        my_name_fmt,
        parts.join(", ")
    )
}

// Dot where a closure variable is bound to the closure a call returned
// (`let g = keep(|| s.len());`, #686). `captures` are the references
// that closure holds which g keeps (see expr_visitor's
// `draw_kept_closure_borrows`), labelled as in
// event_dot_closure_bind_with_captures.
pub fn event_dot_closure_returned(
    my_name: &String,
    func: &String,
    captures: &[(String, &'static str)],
) -> String {
    let my_name_fmt = styled_name_html(my_name);
    let func_fmt = styled_name_html(func);
    if captures.is_empty() {
        return format!("{0} is bound to the closure {1} returned", my_name_fmt, func_fmt);
    }
    let parts: Vec<String> = captures
        .iter()
        .map(|(name, kind)| {
            let how = if kind.starts_with("mutably") { "mutably" } else { "immutably" };
            format!("{} ({})", styled_name_html(name), how)
        })
        .collect();
    format!(
        "{0} is bound to the closure {1} returned, which borrows: {2}",
        my_name_fmt,
        func_fmt,
        parts.join(", ")
    )
}

// Closure-side dot when a `move`-captured upvar lands on `f`'s
// timeline. Mirrors event_dot_acquire.
pub fn event_dot_closure_capture_acquire(my_name: &String, from_name: &String, holds_value: bool) -> String {
    format!(
        "Closure {0} captures (moves) {1}'s {2}",
        styled_name_html(my_name), styled_name_html(from_name), held(holds_value)
    )
}

// Closure-side dot for an immutable capture. Mirrors event_dot_static_borrow.
pub fn event_dot_closure_capture_static_borrow(my_name: &String, from_name: &String) -> String {
    let my_name_fmt = styled_name_html(my_name);
    let from_fmt = styled_name_html(from_name);
    format!(
        "Closure {0} captures an immutable reference to {1}",
        my_name_fmt, from_fmt
    )
}

// Closure-side dot for a mutable capture. Mirrors event_dot_mut_borrow.
pub fn event_dot_closure_capture_mut_borrow(my_name: &String, from_name: &String) -> String {
    let my_name_fmt = styled_name_html(my_name);
    let from_fmt = styled_name_html(from_name);
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
pub fn event_dot_static_reacquire(my_name: &String, holds_value: bool) -> String {
    // update styling
    let my_name_fmt = styled_name_html(my_name);
    
    format!(
        "{0}'s {1} is no longer immutably borrowed",
        my_name_fmt, held(holds_value)
    )
}

// 1   0
//     |
// o<--o
// |
// o-->*   the star event (&mut)
pub fn event_dot_mut_reacquire(my_name: &String, holds_value: bool) -> String {
    // update styling
    let my_name_fmt = styled_name_html(my_name);
    
    format!(
        "{0}'s {1} is no longer mutably borrowed",
        my_name_fmt, held(holds_value)
    )
}



/* Arrow messages: shows up when someone hovers over an arrow */

/// In a `BorrowVia`'s `passed_on`, the `?` operator (`v.first()?`,
/// #1352), which passes the borrow on like `unwrap`.
pub const TRY_PASSES_ON: &str = "\u{1d}?";

// Appended to the "Immutable borrow from s to w" / "Mutable borrow
// from s to w" arrow title when `w` is the reference a call returned
// (`let w = first_word(&s);`, #171): says why, via the lifetimes in the
// callee's signature. With several lenders the signature ties the
// result to several arguments (`longest(&a, &b)`): the borrow checker
// can't know which one it points into, so all of them stay borrowed
// for as long as the result is used.
pub fn arrow_borrow_via_call_suffix(to_name: &String, func: &String, lenders: &[String], also_unnamed: bool, tie: &ArgTie, carrier: ViaCarrier, passed_on: &[String]) -> String {
    let func_fmt = styled_name_html(func);
    let to_fmt = styled_name_html(to_name);
    if carrier == ViaCarrier::Plain {
        return String::new();
    }
    if carrier == ViaCarrier::StoredBy {
        // `refs.push(&s)` (#392): `func` is the method.
        return format!(
            ", stored in {0} by {1}; {0} isn't a reference, but its value now holds one",
            to_fmt, func_fmt
        );
    }
    if carrier == ViaCarrier::StoredThrough {
        // `store_s(&mut r, &b)` (#1353): `func` is the call.
        return format!(": {0}'s signature lets it store this reference in {1}", func_fmt, to_fmt);
    }
    if carrier == ViaCarrier::Part {
        // `let (a, b) = &mut t;` (#1130): `func` is the part a points to.
        let whole = lenders.first().map_or(String::new(), |l| styled_name_html(l));
        return format!(
            ": {0} points to {1}, but the borrow is of all of {2}, which stays borrowed while {0} is in use",
            to_fmt, func_fmt, whole
        );
    }
    if carrier == ViaCarrier::Element {
        // `let r = refs[0];` (#349): `func` is the collection.
        if lenders.len() <= 1 {
            return format!(": {0} is a reference copied out of {1}", to_fmt, func_fmt);
        }
        let names: Vec<String> = lenders.iter().map(|n| styled_name_html(n)).collect();
        let list = match names.len() {
            2 => format!("{} and {}", names[0], names[1]),
            n => format!("{}, and {}", names[..n - 1].join(", "), names[n - 1]),
        };
        let (either, kept) = if names.len() == 2 { ("either", "both") } else { ("any of them", "all of them") };
        return format!(
            ": {4} is a reference copied out of {0}, which holds references to {1}, so it may point into {2}, and {3} stay borrowed while {4} is in use",
            func_fmt, list, either, kept, to_fmt
        );
    }
    if let ViaCarrier::ReturnedClosure { mutable, through_ref } = carrier {
        // `let g = keep(|| s.len());` (#686): `func` is the call. The
        // closure may hold the borrow because it captured s, or got it
        // through a reference it captured (`|| r.len()`), so say only
        // that it borrows s.
        let how = if mutable { "mutably borrows" } else { "borrows" };
        return match lenders.first() {
            Some(l) if through_ref => format!(
                ": {0} holds the closure {1} returned, which {3} what {2} points to, so that data stays borrowed while {0} is in use",
                to_fmt, func_fmt, styled_name_html(l), how
            ),
            Some(l) => format!(
                ": {0} holds the closure {1} returned, which {3} {2}, so {2} stays borrowed while {0} is in use",
                to_fmt, func_fmt, styled_name_html(l), how
            ),
            None => String::new(),
        };
    }
    if carrier == ViaCarrier::PatternRef {
        // `Some(s)` on `pick(&a, &b)` (#1368): s points into the value
        // the returned reference points to, so it may point into
        // whatever that reference may.
        let head = format!(": {0} points into what the reference {1} returned points to", to_fmt, func_fmt);
        let names: Vec<String> = lenders.iter().map(|n| styled_name_html(n)).collect();
        return if !tie.tied.is_empty() {
            format!("{}; {}", head, signature_tie(&func_fmt, "that reference", "point into", to_name, &names, also_unnamed, tie))
        } else if names.len() >= 2 {
            format!("{}, so it may point into {}, and {} while {} is in use",
                head, either_word(names.len()), kept_words(&names, false), to_fmt)
        } else {
            head
        };
    }
    if let ViaCarrier::PatternBinding { from_call, moved, field_of } = carrier {
        // `Some(x)` on `third` / on `v.get(2)` (#391): x is the
        // reference stored in that value (copied, or moved for `&mut`).
        // With `field_of`, the binding is a struct `x` and the borrower
        // its reference field `x.part` (#692).
        let how = if moved { "moved" } else { "copied" };
        let bound = to_name.rsplit_once('.').map_or(to_name.as_str(), |(b, _)| b);
        let head = match (field_of, from_call) {
            (false, true) => format!(": {0} is a reference {1} out of the value {2} returned", to_fmt, how, func_fmt),
            (false, false) => format!(": {0} is a reference {1} out of {2}", to_fmt, how, func_fmt),
            (true, true) => format!(": {0} is a reference that came with the value {1} returned, {2} into {3}", to_fmt, func_fmt, how, styled_name_html(&bound.to_owned())),
            (true, false) => format!(": {0} is a reference that came with the value {1} out of {2} into {3}", to_fmt, how, func_fmt, styled_name_html(&bound.to_owned())),
        };
        // Several lenders, or a signature that also ties the value to
        // an argument with no column: rustc's lifetimes let the
        // reference point into any of them (#681).
        // `tie` is set only when every reference in the value has the
        // lifetime it describes.
        let names: Vec<String> = lenders.iter().map(|n| styled_name_html(n)).collect();
        let tied = !tie.tied.is_empty();
        return if tied && (names.len() >= 2 || tie.tied.len() >= 2 || also_unnamed) {
            format!("{}; {}", head, signature_tie(&func_fmt, "that value", "point into", to_name, &names, also_unnamed, tie))
        } else if names.len() < 2 {
            head
        } else if from_call {
            format!("{}, so it may point into {}, and {} while {} is in use",
                head, either_word(names.len()), kept_words(&names, false), to_fmt)
        } else {
            format!("{}; {} borrows from {}, so it may point into {}, and {} while {} is in use",
                head, func_fmt, join_names(&names), either_word(names.len()), kept_words(&names, false), to_fmt)
        };
    }
    // A value that isn't a reference (`let it = v.iter();`) or a
    // struct with a reference field can carry the borrow too (#336).
    let (noun, not_ref) = match carrier {
        ViaCarrier::Reference => ("reference", String::new()),
        ViaCarrier::Value => ("value", format!("; {0} isn't a reference, but its value holds one", to_fmt)),
        ViaCarrier::StructField => ("struct", String::new()),
        ViaCarrier::TupleField => ("tuple", String::new()),
        ViaCarrier::Element | ViaCarrier::StoredBy | ViaCarrier::PatternBinding { .. } | ViaCarrier::PatternRef | ViaCarrier::ReturnedClosure { .. } | ViaCarrier::Part | ViaCarrier::StoredThrough | ViaCarrier::Plain => unreachable!(),
    };
    // A value can hold borrows of several of them at once (`map` over
    // `v.iter()` with a closure that borrows `extra`).
    let point = if carrier == ViaCarrier::Value { "borrow from" } else { "point into" };
    let names: Vec<String> = lenders.iter().map(|n| styled_name_html(n)).collect();
    // `Some(Pair { l: &s, r: &t })` / `Some([&s, &t])`: the one argument
    // holds every borrow itself; the signature merges nothing.
    if tie.holds_all && names.len() >= 2 && !also_unnamed && tie.unnamed == 0 {
        return format!(
            ": {0} holds the borrows its argument holds: {1}, and {2} while {0} is in use{3}",
            to_fmt, join_names(&names), kept_words(&names, false), not_ref
        );
    }
    // `w.last_mut().unwrap()` (#1266): the borrow is `last_mut`'s, whose
    // result (an `Option`) isn't what `xw` holds; `unwrap` passes it on.
    let (what, passed) = match passed_on {
        [] => (format!("the {} it returns", noun), String::new()),
        fs => {
            let names: Vec<String> = fs.iter().map(|f| if f == TRY_PASSES_ON { "the `?`".to_owned() } else { styled_name_html(f) }).collect();
            ("the value it returns".to_owned(), format!("; {} {} the borrow on", join_names(&names), if fs.len() == 1 { "passes" } else { "pass" }))
        }
    };
    format!(": {}{}{}", signature_tie(&func_fmt, &what, point, to_name, &names, also_unnamed, tie), passed, not_ref)
}

/// `a and b` / `a, b, and c`.
fn join_names(items: &[String]) -> String {
    match items.len() {
        0 => String::new(),
        1 => items[0].clone(),
        2 => format!("{} and {}", items[0], items[1]),
        n => format!("{}, and {}", items[..n - 1].join(", "), items[n - 1]),
    }
}

fn either_word(n: usize) -> &'static str {
    if n == 2 { "either" } else { "any of them" }
}

/// `both stay borrowed` / `all of them stay borrowed`, or the names
/// when some of the tied arguments have no column.
fn kept_words(names: &[String], unnamed: bool) -> String {
    match (names.len(), unnamed) {
        (2, false) => "both stay borrowed".to_owned(),
        (_, false) => "all of them stay borrowed".to_owned(),
        (_, true) => format!("{} stay borrowed", join_names(names)),
    }
}

/// `{func}'s signature gives {what} the same lifetime as both
/// arguments, so it may point into either, and both stay borrowed
/// while x is in use`: which of the call's arguments the signature
/// ties its result to, and what that means for `to_name` when the
/// result borrows from several variables (`names`, already styled).
fn signature_tie(func_fmt: &str, what: &str, point: &str, to_name: &String, names: &[String], also_unnamed: bool, tie: &ArgTie) -> String {
    let join = join_names;
    fn ordinal(i: usize) -> String {
        const NAMES: [&str; 10] = ["first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth", "ninth", "tenth"];
        NAMES.get(i).map(|s| s.to_string()).unwrap_or_else(|| format!("{}th", i + 1))
    }
    fn count_word(n: usize) -> String {
        const NAMES: [&str; 10] = ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine"];
        NAMES.get(n).map(|s| s.to_string()).unwrap_or_else(|| n.to_string())
    }
    // Which arguments the result's lifetime is the same as. A position
    // in `tie.stored_in` is tied only through the references stored in
    // that argument, not the argument itself (#1084).
    let first_arg = usize::from(tie.receiver);
    let n_args = tie.total.saturating_sub(first_arg);
    let positions = |set: &[usize]| -> Option<String> {
        let recv_tied = tie.receiver && set.contains(&0);
        let args: Vec<usize> = set.iter().filter(|&&i| i >= first_arg).map(|i| i - first_arg).collect();
        let arg_part = if args.is_empty() {
            None
        } else if args.len() == n_args && n_args == 1 {
            Some("its argument".to_owned())
        } else if args.len() == n_args && !recv_tied {
            Some(match n_args {
                2 => "both arguments".to_owned(),
                n => format!("all {} arguments", count_word(n)),
            })
        } else if args.len() == 1 {
            Some(format!("its {} argument", ordinal(args[0])))
        } else {
            let ords: Vec<String> = args.iter().map(|&i| ordinal(i)).collect();
            Some(format!("its {} arguments", join(&ords)))
        };
        match (recv_tied, arg_part) {
            (true, Some(a)) => Some(format!("the value it was called on and {}", a)),
            (true, None) => Some("the value it was called on".to_owned()),
            (false, a) => a,
        }
    };
    let direct: Vec<usize> = tie.tied.iter().copied().filter(|i| !tie.stored_in.iter().any(|st| st.index == *i)).collect();
    // `the references stored in rows` (`&rows`; `... in a and b` for
    // several), `t` (`&t` with `t` a reference), or the argument's
    // position when it isn't a variable.
    let mut parts: Vec<String> = positions(&direct).into_iter().collect();
    let holders: Vec<String> = tie.stored_in.iter()
        .filter(|st| !st.referent_is_ref)
        .filter_map(|st| st.name.as_deref().map(styled_name_html))
        .collect();
    let mut holders_said = false;
    for st in &tie.stored_in {
        let pos = || positions(&[st.index]).unwrap_or_else(|| "its argument".to_owned());
        parts.push(match (&st.name, st.referent_is_ref) {
            (Some(n), true) => styled_name_html(n),
            (Some(_), false) if holders_said => continue,
            (Some(_), false) => {
                holders_said = true;
                format!("the references stored in {}", join(&holders))
            }
            (None, true) => format!("the reference {} points to", pos()),
            (None, false) => format!("the references stored in {}", pos()),
        });
    }
    let mut tied = if parts.is_empty() { "its arguments".to_owned() } else { join(&parts) };
    let tied_count = tie.tied.len().max(1);
    if tie.unnamed > 0 && tied_count >= 2 {
        tied.push_str(&match (tie.unnamed, tied_count) {
            (1, 2) => " (the other is a value with no column here)".to_owned(),
            (1, _) => " (one of them is a value with no column here)".to_owned(),
            (k, _) => format!(" ({} of them are values with no column here)", count_word(k)),
        });
    }
    let to_fmt = styled_name_html(to_name);
    let unnamed_slots = tie.unnamed.max(usize::from(also_unnamed));
    let tail = if names.len() >= 2 {
        let either = either_word(names.len() + unnamed_slots);
        let kept = kept_words(names, unnamed_slots > 0);
        format!(", so it may {} {}, and {} while {} is in use", point, either, kept, to_fmt)
    } else if names.len() == 1 && (tied_count >= 2 || also_unnamed) {
        format!(", so {} stays borrowed while {} is in use", names[0], to_fmt)
    } else {
        String::new()
    };
    if tie.via_outlives {
        // `fn shorten<'a, 'b>(x: &'a str, _y: &'b str) -> &'b str where
        // 'a: 'b`: x's lifetime isn't the result's, but outlives it (#1125).
        return format!("{0}'s signature lets {1} point into {2} (a lifetime bound like `'a: 'b` lets a longer-lived reference stand in for a shorter one){3}", func_fmt, what, tied, tail);
    }
    format!("{0}'s signature gives {1} the same lifetime as {2}{3}", func_fmt, what, tied, tail)
}



// The end of a borrow whose lender has no column (a string literal
// `let s: &str = "hi";`, or a reference returned by a function call
// that isn't linked to its argument). The arrow used to read "Return
// immutably borrowed resource from s to *s", a borrow returned to
// itself (#178).
pub fn arrow_borrow_ends_lender_not_drawn(from_name: &String, is_mut: bool) -> String {
    format!(
        "{0}'s {1} borrow ends (what it borrows from isn't shown)",
        styled_name_html(from_name),
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
            format!("the static {} ends (statics live for the whole program)", styled_name_html(name)),
        StaticLender::PointeeOfStatic(name) =>
            format!("what the static {} points to ends (it lives for the whole program)", styled_name_html(name)),
        StaticLender::PointeeOfConst(name) =>
            format!("what the constant {} points to ends (it lives for the whole program)", styled_name_html(name)),
        // Never drawn as an arrow (see the renderer).
        StaticLender::TupleTemporary =>
            "the temporary tuple ends".to_string(),
        StaticLender::PromotedConst(name) =>
            format!("the constant {} ends (Rust stores the borrowed value for the whole program)", styled_name_html(name)),
        // Never drawn as an arrow (the end has no reference to start from).
        StaticLender::LoopIterator { call } =>
            format!("the iterator {} returned ends", styled_name_html(call)),
        StaticLender::LoopHeadBorrow { .. } =>
            "the `for` loop's iterator ends".to_string(),
    };
    format!(
        "{0}'s {1}borrow of {2}",
        styled_name_html(from_name),
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
    let f = styled_name_html(fn_name);
    let x = styled_name_html(my_name);
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

// Call dot for a `&self` method called through a shared reference to a
// value with interior mutability (`r.set(2)` with `r: &Cell<i32>`,
// `r.borrow_mut()` with `r: &RefCell<T>`; #1149). "reads from r" would be
// false: the call can change what r points to. The text doesn't say the
// method itself changes it: `Cell::get` only copies the value out.
pub fn fn_dot_shared_ref_interior_mut(fn_name: &String, my_name: &String, refcell_borrow_mut: bool) -> String {
    let f = styled_name_html(fn_name);
    let r = styled_name_html(my_name);
    if refcell_borrow_mut {
        format!(
            "{0} uses what {1} points to, through the shared reference {1}; exclusive access to its contents is checked at runtime (a second {0} while this one is live would panic)",
            f, r
        )
    } else {
        format!(
            "{0} uses what {1} points to, through the shared reference {1}; that has interior mutability, so its contents can change even through a shared reference",
            f, r
        )
    }
}

/* State messages: shows up on the vertical lines for every value/reference */

// The viable is no longer in the scope after this line.
pub fn state_out_of_scope(my_name: &String) -> String {
    // update styling
    let my_name_fmt = styled_name_html(my_name);
    
    format!(
        "{0} is out of scope",
        my_name_fmt
    )
}

// The resource is transferred on this line or before this line due to move,
// thus it is impossible to access this variable anymore. This is an invisible line in the timeline.
// `mutable`: a `let mut` binding can be assigned again; an immutable one
// never can (E0384).
pub fn state_value_moved(my_name: &String, mutable: bool) -> String {
    format!(
        "{0}'s value was moved, so {0} can't be used{1}",
        styled_name_html(my_name),
        if mutable { " until it's assigned again" } else { " anymore" }
    )
}

// Mutably borrowed, for a column holding a value with nothing to drop.
pub fn state_value_revoked(my_name: &String) -> String {
    format!("{0}'s value is mutably borrowed, so it cannot access the value", styled_name_html(my_name))
}

pub fn state_resource_moved(my_name: &String) -> String {
    // update styling
    let my_name_fmt = styled_name_html(my_name);
    
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
        styled_name_html(my_name),
        if is_mut { "mutable reference" } else { "reference" }
    )
}

// temporarily no read or write access right to the resource, but eventually
// the privilege will come back. Occurs when mutably borrowed. The timeline
// draws `state_lent_out` on that segment instead (#722).
pub fn state_resource_revoked(my_name: &String) -> String {
    // update styling
    let my_name_fmt = styled_name_html(my_name);
    
    format!(
        "{0}'s resource is mutably borrowed, so it cannot access the resource",
        my_name_fmt,
    )
}

// This ResourceOwner is the unique object that holds the ownership to the underlying resource.
pub fn state_full_privilege(my_name: &String) -> String {
    // update styling
    let my_name_fmt = styled_name_html(my_name);

    format!(
        "{0} is the owner of the resource", //not necessarily write if let was used rather than let mut
        my_name_fmt
    )
}

// Dashed segment of a variable that has no value yet on this path:
// before a `let s;` is assigned in a branch, or before the branch of
// a `let s = if …` produces the value (#234).
pub fn state_uninitialized(my_name: &String) -> String {
    let my_name_fmt = styled_name_html(my_name);
    // A temporary a `let` extends through a branch (#1397).
    if is_temporary_name(my_name.split('.').next().unwrap_or("")) {
        return format!("{} hasn't been made yet: only the branch that borrows it makes it", my_name_fmt);
    }

    format!(
        "{0} has no value yet",
        my_name_fmt
    )
}

// Dashed segment of a let chain's scrutinee temporary before the `let`
// that makes it (`if k && let Ok(s) = make()`, #1386).
pub fn state_chain_temp_unmade(my_name: &String) -> String {
    format!(
        "{} hasn't been made yet: the let chain makes it only if its parts before the `let` match",
        styled_name_html(my_name)
    )
}

// `state_uninitialized` in a conditional that comes after one that
// assigned s in only some of its branches (#244): on the rows of an arm
// that hasn't assigned s, s holds a value on some paths only.
pub fn state_maybe_uninitialized(my_name: &String) -> String {
    // A temporary a `let` extends through a branch (#1397).
    if is_temporary_name(my_name.split('.').next().unwrap_or("")) {
        return format!(
            "{0} may not exist here: only some of the branches above make it",
            styled_name_html(my_name)
        );
    }
    format!(
        "{0} may have no value here: it was assigned in only some of the branches above",
        styled_name_html(my_name)
    )
}

// `state_maybe_uninitialized` where what may have left s with no value
// is a loop above that may have ended before assigning it (#786), not
// a conditional above (#1034).
pub fn state_maybe_uninitialized_after_loop(my_name: &String) -> String {
    format!(
        "{0} may have no value here: a loop above may have ended before assigning it",
        styled_name_html(my_name)
    )
}

/// How the tooltips name an arm: "if" / "else" / "if let" for an `if`,
/// the pattern for a `match` arm. `label` is the arm's label (see
/// `BranchType`).
fn arm_name_html(label: &str) -> String {
    let name = match label {
        "If" => "if".to_owned(),
        "Else" => "else".to_owned(),
        l if l == IF_LET_ARM => "if let".to_owned(),
        l => l.to_owned(),
    };
    styled_name_html(&name)
}

/// A dashed arm column that underlines the other arms' code on hover:
/// the rows belong to another arm, so this arm isn't running here (#626).
pub fn state_inactive_arm(label: &str) -> String {
    format!(
        "This segment is for the {} branch. The underlined code is not part of that branch.",
        arm_name_html(label)
    )
}

/// A dashed arm stretch that has no other arm's code to underline.
pub fn state_inactive_arm_unlit(label: &str) -> String {
    format!("This segment is for the {} branch.", arm_name_html(label))
}

/// The diagonal from the parent's column into an arm's column. It
/// underlines nothing (#843).
pub fn state_arm_split(label: &str) -> String {
    format!("This segment leads into the {} branch.", arm_name_html(label))
}

/// The diagonal from an arm's column to the join (#843).
pub fn state_arm_join(label: &str) -> String {
    format!(
        "This segment leads from the {} branch to where the branches join.",
        arm_name_html(label)
    )
}

// FullPrivilege but the owner is a Copy type (i32 etc.), so the
// "owner of the resource" framing doesn't fit — primitives are
// values, not heap-backed resources. Used for the timeline-stripe
// tooltip and the vertical lifeline.
// A non-Copy owner with nothing to drop: also a value, not a resource (#484).
// The line of a struct some of whose fields were moved out (#1157):
// it can't be used as a whole any more, only the fields still in it.
// `maybe`: on some paths through a conditional above only; Rust still
// rejects using the struct as a whole after it.
pub fn state_partly_moved(my_name: &String, moved: &[String], maybe: bool, _others_usable: bool) -> String {
    // (Nothing about the other fields: one may be borrowed, or a field
    // of a field moved, #1619.)
    if maybe {
        return state_fields_maybe_moved(my_name);
    }
    let names: Vec<String> = moved.iter().map(|m| styled_name_html(m)).collect();
    let (list, verb) = match names.as_slice() {
        [one] => (one.clone(), "was"),
        [init @ .., last] => (format!("{} and {}", init.join(", "), last), "were"),
        [] => (String::new(), "was"),
    };
    format!(
        "{0} is partially moved: {1} {2} moved out, so {0} can't be used as a whole",
        styled_name_html(my_name), list, verb
    )
}

// Fields moved on some paths only (#1583, #1619): which ones, and what is
// left where, isn't claimed.
pub fn state_fields_maybe_moved(my_name: &String) -> String {
    format!("Some of {0}'s fields may have been moved out, so {0} can't be used as a whole", styled_name_html(my_name))
}

// The line of a struct every field of which was moved out, some of them
// on some paths through a conditional above only (`let s2 = p.b; if c {
// drop(p.a); }`, #1583): on the other paths it still holds those, but
// neither it nor any field of it can be used (E0382). It isn't
// "partially moved" on every path (each arm may move all of it, #1599);
// what is left is dropped when it is overwritten (a `mut` binding) or at
// its scope end, as those dots say.
pub fn state_fields_maybe_all_moved(my_name: &String, _surely: &[String], _maybe: &[String], _mutable: bool) -> String {
    state_fields_maybe_moved(my_name)
}

// The line of an array a slice pattern moved some elements out of
// (`[c, _] => …`, #1285): the elements `left` stay in it, and are dropped
// at its scope end, but it can't be used as a whole any more. `maybe`:
// moved on some paths through a conditional above only.
pub fn state_elems_moved(my_name: &String, moved: &[String], left: &[String], maybe: bool) -> String {
    fn list(names: &[String]) -> String {
        let fmt: Vec<String> = names.iter().map(|m| styled_name_html(m)).collect();
        match fmt.as_slice() {
            [init @ .., last] if !init.is_empty() => format!("{} and {}", init.join(", "), last),
            _ => fmt.concat(),
        }
    }
    let verb = match (moved.len() > 1, maybe) {
        (_, true) => "may have been",
        (true, false) => "were",
        (false, false) => "was",
    };
    let rest = if left.is_empty() {
        String::new()
    } else {
        format!("; {} {} still in it", list(left), if left.len() > 1 { "are" } else { "is" })
    };
    format!(
        "{0} is partially moved: {1} {2} moved out, so {0} can't be used as a whole{3}",
        styled_name_html(my_name), list(moved), verb, rest
    )
}

pub fn state_full_privilege_value(my_name: &String) -> String {
    format!("{0} holds a value", styled_name_html(my_name))
}

pub fn state_full_privilege_copyable(my_name: &String) -> String {
    let my_name_fmt = styled_name_html(my_name);

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
    let my_name_fmt = styled_name_html(my_name);
    format!(
        "{0} holds a mutable reference; it can read and write the borrowed {1}",
        my_name_fmt, held(pointee_copy)
    )
}

pub fn state_static_ref_full_privilege(my_name: &String, pointee_copy: bool) -> String {
    let my_name_fmt = styled_name_html(my_name);
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
    let my_name_fmt = styled_name_html(my_name);
    let noun = if move_capture_count == 1 { "resource" } else { "resources" };
    format!(
        "{0} owns a closure which owns {1} {2} via capture",
        my_name_fmt, move_capture_count, noun
    )
}

// A non-reference variable whose value holds a borrow (`let it =
// v.iter();`, `let third = v.get(2);`; #336). It owns that value (an
// iterator, an Option), but not the data the value borrows, so the
// generic "is the owner of the resource" is avoided.
pub fn state_holder_full_privilege(my_name: &String) -> String {
    let my_name_fmt = styled_name_html(my_name);
    format!(
        "{0} holds a value that contains a reference ({0} isn't a reference itself)",
        my_name_fmt
    )
}

// A holder whose value starts out without a reference (`let mut refs
// = Vec::new();`, filled later by `refs.push(&s)`; #392). Its type can
// hold references; whether it holds one depends on the line.
pub fn state_holder_starts_empty(my_name: &String) -> String {
    let my_name_fmt = styled_name_html(my_name);
    format!(
        "{0} owns a value that can hold references ({0} isn't a reference itself)",
        my_name_fmt
    )
}

/// What the value of a holder (`it` in `let it = v.iter();`) borrows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HolderLender {
    /// The data a reference points to (`v: &Vec<i32>`; #393).
    Pointee,
    /// A field reached through a reference (`self.items`; #1080).
    FieldThroughRef,
    /// A variable's own resource or value.
    Own,
}

// The holder's dot where its value starts borrowing `from`. When
// `from` is itself a reference (a reference parameter `v: &Vec<i32>`
// in `let it = v.iter();`; #393), the value borrows the data `from`
// points to, not the reference. A lender with nothing to drop (an
// `i32`) holds a "value", not a "resource" (#484).
// A field reached through a reference (`self.items` in `let it =
// self.items.iter();` with `self: &mut Self`; #1080) is named as the
// place it is.
pub fn event_dot_holder_borrow(my_name: &String, from_name: &String, is_mut: bool, from: HolderLender, holds_value: bool) -> String {
    let what = holder_lender_phrase(from_name, from, holds_value);
    format!(
        "{0} holds a value that {1} borrows {2} ({0} isn't a reference itself)",
        styled_name_html(my_name),
        if is_mut { "mutably" } else { "immutably" },
        what
    )
}


/// What a holder borrows, named: "s's resource", "the data p points to"
/// (`*p`'s name is p's: the place is what p points to, #1619), a field.
fn holder_lender_phrase(from_name: &String, from: HolderLender, holds_value: bool) -> String {
    match from {
        HolderLender::Pointee => format!("the data {} points to", styled_name_html(&from_name.trim_start_matches('*').to_owned())),
        HolderLender::FieldThroughRef => styled_name_html(from_name),
        HolderLender::Own => format!("{}'s {}", styled_name_html(from_name), if holds_value { "value" } else { "resource" }),
    }
}

// A holder whose loans' kinds aren't stated (`BorrowVia::uncertain`,
// #1619): only that it keeps its lenders borrowed while it is in use, as
// rustc does; nothing about what it holds.
pub fn event_dot_holder_keeps_borrowed(my_name: &String, lenders: &[String]) -> String {
    let names: Vec<String> = lenders.iter().map(|l| match l.strip_prefix('*') {
        Some(p) => format!("the data {} points to", styled_name_html(&p.to_owned())),
        None => styled_name_html(l),
    }).collect();
    let list = match names.as_slice() {
        [init @ .., last] if !init.is_empty() => format!("{} and {}", init.join(", "), last),
        _ => names.concat(),
    };
    format!("{0} keeps {1} borrowed while {0} is in use ({0} isn't a reference itself)", styled_name_html(my_name), list)
}

// The lender's side of such a loan, and where it ends.
pub fn event_dot_neutral_lend(my_name: &String, target_name: &String, holds_value: bool) -> String {
    format!("{0}'s {2} is borrowed by {1}", styled_name_html(my_name), styled_name_html(target_name), held(holds_value))
}

pub fn event_dot_neutral_reacquire(my_name: &String, target_name: &String, holds_value: bool) -> String {
    format!("{0}'s {2} is no longer borrowed by {1}", styled_name_html(my_name), styled_name_html(target_name), held(holds_value))
}

pub fn arrow_neutral_return(holder: &String, lender: &String) -> String {
    format!("Return borrowed resource from {} to {}", styled_name_html(holder), styled_name_html(lender))
}

pub fn event_dot_neutral_return(my_name: &String) -> String {
    format!("{0}'s borrow ends", styled_name_html(my_name))
}

// `let k = Box::new(r);` with r a `&mut` parameter (#1289): k's value
// holds the reference r held, which borrows the caller's data (no
// column in this fn).
pub fn event_dot_holder_borrow_caller(my_name: &String) -> String {
    format!(
        "{0}'s value holds the reference moved into it, so {0} mutably borrows the caller's data ({0} isn't a reference itself)",
        styled_name_html(my_name)
    )
}

// `let it2 = it;` where `it` holds a borrow: it2 takes over the value
// and, with it, the borrow.
pub fn event_dot_holder_acquire(my_name: &String, from_name: &String) -> String {
    format!(
        "{0} takes over {1}'s value, and the borrow it holds",
        styled_name_html(my_name), styled_name_html(from_name)
    )
}

// `let o = Some(r);`, `let v = vec![r];` with r a `&mut` (#1128): the
// reference r held is moved into o's value, so o now holds the borrow.
pub fn event_dot_holder_takes_ref(my_name: &String, from_name: &String) -> String {
    format!(
        "the reference {1} held is moved into {0}'s value, so {0} now holds its borrow",
        styled_name_html(my_name), styled_name_html(from_name)
    )
}

// `let h = g;` with g a closure (#704): h gets g's closure value, a
// copy (a closure that captures by reference only shared borrows, and
// otherwise Copy values, is Copy) or by a move. Nothing is captured.
// The borrows g's captures made go with the value: a copy holds them
// too, a moved-to h holds them instead of g.
// `assigned`: `h = g;` over a value h had (only for a copy).
pub fn event_dot_closure_value_acquire(my_name: &String, from_name: &String, is_copy: bool, borrows: usize, assigned: bool) -> String {
    let (h, g) = (styled_name_html(my_name), styled_name_html(from_name));
    let s = if borrows == 1 { "" } else { "s" };
    let copy = if assigned { "is assigned a copy of" } else { "is initialized with a copy of" };
    match (is_copy, borrows) {
        (true, 0) => format!("{0} {2} {1}", h, g, copy),
        (true, _) => format!("{0} {3} {1}, and holds the same borrow{2} as {1}", h, g, s, copy),
        (false, 0) => format!("{0} takes over {1}'s value", h, g),
        (false, _) => format!("{0} takes over {1}'s value, and the borrow{2} it holds", h, g, s),
    }
}

// Scope end of a holder that still holds its value and needs drop (a
// `Vec<&T>`, a `Box<dyn Fn() + '_>`): its resource is dropped (#484). A
// holder with nothing to drop (an `Iter`) never gets this suffix: its
// scope end claims no drop.
pub fn event_dot_holder_drop_suffix() -> String {
    ". Its resource is dropped, but not the data it borrowed.".to_owned()
}

pub fn state_closure_full_privilege_no_resource(my_name: &String) -> String {
    let my_name_fmt = styled_name_html(my_name);
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
// The lender's dot of a borrow of all of `whole` by a pattern binding
// that points to its part `part` (`let (a, b) = &mut t;`, #1130): one
// loan of t, which a and b each reach a different part of.
pub fn event_dot_lend_part(whole: &str, part: &str, to: &str, mutable: bool) -> String {
    let (w, p, t) = (styled_name_html(whole), styled_name_html(part), styled_name_html(to));
    if mutable {
        format!("{1} is mutably borrowed by {2}, as part of a mutable borrow of all of {0}; {0} can't be used while the borrow is live", w, p, t)
    } else {
        format!("{1} is immutably borrowed by {2}, as part of an immutable borrow of all of {0}; {0} is read-only while the borrow is live", w, p, t)
    }
}

// A field's segment while the struct it is in is borrowed as a whole
// (`let r = &mut p;`, `let (a, b) = &mut t;`; #1130, #1193): the field is
// borrowed with it. `by`: the one holder of the borrow, if known.
pub fn state_lent_with_parent(my_name: &str, parent: &str, by: Option<&str>, mutable: bool) -> String {
    let me = styled_name_html(my_name);
    let by = by.map_or(String::new(), |b| format!(" by {}", styled_name_html(b)));
    if mutable {
        format!("{0} is mutably borrowed along with {1}{2}; {0} can't be used until the borrow ends", me, styled_name_html(parent), by)
    } else {
        format!("{0} is immutably borrowed along with {1}{2}; {0} is read-only while the borrow is live", me, styled_name_html(parent), by)
    }
}

pub fn state_partial_privilege(my_name: &String, holds_value: bool) -> String {
    // update styling
    let my_name_fmt = styled_name_html(my_name);
    
    format!(
        "{0}'s {1} is being shared by one or more variables",
        my_name_fmt, held(holds_value)
    )
}

// The dot where a `for` loop's iterator borrows a variable whose loop
// variables don't (`for &y in &v`, `for _ in &mut v`; #885).
pub fn event_dot_lend_to_loop_iterator(my_name: &String, is_mut: bool, holds_value: bool) -> String {
    if is_mut {
        format!(
            "{0}'s {1} is mutably borrowed by the `for` loop's iterator; {0} can't be used until the loop ends",
            styled_name_html(my_name), held(holds_value)
        )
    } else {
        state_borrowed_by_loop_iterator(my_name, holds_value)
    }
}

/// The holder named, in `event_dot_reacquire_still_borrowed`, for a
/// borrow only a `for` loop's iterator holds (#900). Not a variable, so
/// not styled as one.
pub const LOOP_ITERATOR_HOLDER: &str = "the `for` loop's iterator";

/// Who holds the `&mut` borrow a column is lent out to, for
/// `state_lent_out`.
pub enum LentTo<'a> {
    /// A variable (or field) holding the reference.
    Var(&'a str),
    /// A closure that captured it mutably.
    Closure(&'a str),
    /// A `for` loop's iterator, which has no column.
    LoopIterator,
    /// Nothing RustViz can name (several holders, or a call's result
    /// that isn't bound).
    Unknown,
}

/// The faint dotted segment of a column while it is mutably lent out
/// (RevokedPrivilege, #722): the column can't be used until the `&mut`
/// borrow ends. Worded like the lender's dot (`event_dot_mut_lend`,
/// `event_dot_lend_through_ref`, `event_dot_lend_to_loop_iterator`),
/// with "until the borrow ends" for the stretch of rows.
///
/// `through_ref`: the column is a `&mut` reference, so what's borrowed is
/// what it points to (Some(pointee_copy)); `ref_itself`: the column is a
/// `&T` binding that was itself borrowed `&mut` (`&mut r`).
pub fn state_lent_out(my_name: &String, to: LentTo, through_ref: Option<bool>, ref_itself: bool, holds_value: bool) -> String {
    let me = styled_name_html(my_name);
    let by = match &to {
        LentTo::Var(v) => format!(" by {}", styled_name_html(v)),
        LentTo::Closure(c) => format!(" by closure {}", styled_name_html(c)),
        LentTo::LoopIterator => " by the `for` loop's iterator".to_owned(),
        LentTo::Unknown => String::new(),
    };
    let until = if matches!(to, LentTo::LoopIterator) { "the loop ends" } else { "the borrow ends" };
    let subject = match through_ref {
        _ if ref_itself => format!("{} is mutably borrowed{}", me, by),
        Some(pointee_copy) => format!("the {} {} points to is mutably borrowed{} through {}", held(pointee_copy), me, by, me),
        None => format!("{}'s {} is mutably borrowed{}", me, held(holds_value), by),
    };
    format!("{}; {} can't be used until {}", subject, me, until)
}

// The dots of a `&mut` reference whose pointee a `for` loop's iterator
// reborrows (`for e in v.iter_mut()` with `v: &mut Vec<_>`; #1046, or
// `v.iter()`, shared; #1059): where the head's call lends it, and where
// the loop ends. The iterator holds the reborrow until then, so v can't
// be used (or is read-only) meanwhile, even after the last use of e.
//
// A `&T` passed as it is to the head's adapter (`a.iter_mut().zip(b)`
// with `b: &Vec<_>`; #1076), or one whose pointee the head's call reads
// (`v.iter()` with `v: &Vec<_>`; #1081), is copied into the iterator,
// which reads through the copy until the loop ends. b is a shared
// reference, so it was read-only all along: no "b is read-only until
// the loop ends".
pub fn event_dot_lend_through_ref_to_loop_iterator(ref_name: &String, is_mut: bool, ref_is_mut: bool, pointee_copy: bool) -> String {
    if !ref_is_mut {
        format!(
            "the {1} {0} points to is immutably borrowed by the `for` loop's iterator, through a copy of {0}, until the loop ends",
            styled_name_html(ref_name), held(pointee_copy)
        )
    } else if is_mut {
        format!(
            "the {1} {0} points to is mutably borrowed by the `for` loop's iterator through {0}; {0} can't be used until the loop ends",
            styled_name_html(ref_name), held(pointee_copy)
        )
    } else {
        format!(
            "the {1} {0} points to is immutably borrowed by the `for` loop's iterator through {0}; {0} is read-only until the loop ends",
            styled_name_html(ref_name), held(pointee_copy)
        )
    }
}

pub fn event_dot_loop_iterator_reacquire_through_ref(ref_name: &String, pointee_copy: bool) -> String {
    format!(
        "the `for` loop's iterator no longer borrows the {1} {0} points to",
        styled_name_html(ref_name), held(pointee_copy)
    )
}

// The same for a field reached through a `&mut` reference
// (`for e in self.items.iter_mut()` with `self: &mut Self`; #1060),
// drawn on the reference's column. Only that field is lent: `self.n
// += 1` in the body compiles, `self.items.len()` doesn't. Through a
// `&T` (`self.items.iter()` with `self: &Self`; #1081) the field was
// read-only all along.
pub fn event_dot_lend_field_through_ref_to_loop_iterator(place: &String, is_mut: bool, ref_is_mut: bool) -> String {
    if !ref_is_mut {
        format!(
            "{0} is immutably borrowed by the `for` loop's iterator until the loop ends",
            styled_name_html(place)
        )
    } else if is_mut {
        format!(
            "{0} is mutably borrowed by the `for` loop's iterator; {0} can't be used until the loop ends",
            styled_name_html(place)
        )
    } else {
        format!(
            "{0} is immutably borrowed by the `for` loop's iterator; {0} is read-only until the loop ends",
            styled_name_html(place)
        )
    }
}

pub fn event_dot_loop_iterator_reacquire_field_through_ref(place: &String) -> String {
    format!("the `for` loop's iterator no longer borrows {0}", styled_name_html(place))
}

// PartialPrivilege of a variable that only a `for` loop's iterator
// borrows (`for c in s.chars()`, `for (i, a) in v.iter().enumerate()`;
// #871): the iterator has no column, so "being shared by one or more
// variables" sent the reader looking for a variable that isn't there.
pub fn state_borrowed_by_loop_iterator(my_name: &String, holds_value: bool) -> String {
    format!(
        "{0}'s {1} is immutably borrowed by the `for` loop's iterator; {0} is read-only until the loop ends",
        styled_name_html(my_name), held(holds_value)
    )
}

// PartialPrivilege on a `&mut` reference's lifeline: what it points to
// is shared through it (`let r3 = &*r;`), so r can only read until
// those shared borrows end (#313). r owns nothing, so the owner's
// "r's resource is being shared" was wrong.
pub fn state_mut_ref_partial_privilege(my_name: &String, pointee_copy: bool) -> String {
    format!(
        "{0} holds a mutable reference, but the {1} it points to is currently shared through it, so {0} can only read it",
        styled_name_html(my_name), held(pointee_copy)
    )
}

// The same while a `for` loop's iterator borrows only a field of what
// r points to (`for e in self.items.iter()` with `self: &mut Self`;
// #1060): the other fields can still be written through r.
pub fn state_mut_ref_field_borrowed_by_loop_iterator(my_name: &String, place: &String, is_mut: bool) -> String {
    if is_mut {
        format!(
            "{0} holds a mutable reference, but {1} is mutably borrowed by the `for` loop's iterator, so {1} can't be used until the loop ends",
            styled_name_html(my_name), styled_name_html(place)
        )
    } else {
        format!(
            "{0} holds a mutable reference, but {1} is immutably borrowed by the `for` loop's iterator, so {1} is read-only until the loop ends",
            styled_name_html(my_name), styled_name_html(place)
        )
    }
}

// The same while a reference variable borrows only that field
// (`let r = &mut self.items;`, `let first = self.items.first_mut()
// .unwrap();`; #1073): the rest of what my_name points to can still
// be used through it, so its line keeps its normal style.
pub fn state_mut_ref_field_borrowed_by(my_name: &String, place: &String, borrower: &String, is_mut: bool) -> String {
    if is_mut {
        format!(
            "{0} holds a mutable reference, but {1} is mutably borrowed by {2}, so {1} can't be used while the borrow is live",
            styled_name_html(my_name), styled_name_html(place), styled_name_html(borrower)
        )
    } else {
        format!(
            "{0} holds a mutable reference, but {1} is immutably borrowed by {2}, so {1} is read-only while the borrow is live",
            styled_name_html(my_name), styled_name_html(place), styled_name_html(borrower)
        )
    }
}

// The same while a reference variable (or a value holding a borrow)
// borrows several fields of what my_name points to (`let it =
// self.items.iter().chain(self.other.iter());`, `let p =
// r.other.iter_mut().zip(r.items.iter());`; #1080): the other fields
// can still be used through it. `places` pairs each field with whether
// it is borrowed mutably.
pub fn state_mut_ref_fields_borrowed_by(my_name: &String, places: &[(String, bool)], borrower: &String) -> String {
    if let [(place, is_mut)] = places {
        return state_mut_ref_field_borrowed_by(my_name, place, borrower, *is_mut);
    }
    fields_borrowed(my_name, places, &styled_name_html(borrower), "while the borrow is live")
}

// The same when a `for` loop's iterator took over that value (`for (o,
// i) in pairs`; #1080).
pub fn state_mut_ref_fields_borrowed_by_loop_iterator(my_name: &String, places: &[(String, bool)]) -> String {
    if let [(place, is_mut)] = places {
        return state_mut_ref_field_borrowed_by_loop_iterator(my_name, place, *is_mut);
    }
    fields_borrowed(my_name, places, "the `for` loop's iterator", "until the loop ends")
}

fn fields_borrowed(my_name: &String, places: &[(String, bool)], borrower: &str, until: &str) -> String {
    let names = |m: bool| -> Vec<String> {
        places.iter().filter(|(_, x)| *x == m).map(|(p, _)| styled_name_html(p)).collect()
    };
    let (muts, shared) = (names(true), names(false));
    let me = styled_name_html(my_name);
    if shared.is_empty() {
        format!(
            "{} holds a mutable reference, but {} are mutably borrowed by {}, so they can't be used {}",
            me, join_and(&muts), borrower, until
        )
    } else if muts.is_empty() {
        format!(
            "{} holds a mutable reference, but {} are immutably borrowed by {}, so they are read-only {}",
            me, join_and(&shared), borrower, until
        )
    } else {
        let verb = |v: &Vec<String>| if v.len() == 1 { "is" } else { "are" };
        format!(
            "{0} holds a mutable reference, but {1} {2} mutably borrowed and {3} immutably borrowed by {4}, so {1} can't be used and {3} {5} read-only {6}",
            me, join_and(&muts), verb(&muts), join_and(&shared), borrower, verb(&shared), until
        )
    }
}

// should not appear for visualization in a correct program
pub fn state_invalid(my_name: &String) -> String {
    // update styling
    let my_name_fmt = styled_name_html(my_name);
    
    format!(
        "something is wrong with the timeline of {0}",
        my_name_fmt
    )
}

pub fn structure(my_name: &String) -> String {
    // update styling
    let my_name_fmt = styled_name_html(my_name);

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
    let my_name_fmt = styled_name_html(my_name);
    format!(
        "{0} may have been moved (consumed in at least one branch above)",
        my_name_fmt
    )
}

/// The join of a scrutinee temporary that the pattern of some branches
/// moved its value, or part of it, out of (#1386). What is still in it
/// is dropped with it, which its drop says.
pub fn event_dot_branch_merge_temp_moved_out(my_name: &String) -> String {
    format!(
        "The pattern of the branch that ran may have moved {0}'s value, or part of it, into its bindings",
        styled_name_html(my_name)
    )
}

/// The join after branches some of which took elements out of the array
/// with a slice pattern (`[a, _, _] => …`, #1285): the array can't be used
/// as a whole after it, but the elements no pattern takes stay in it.
pub fn event_dot_branch_merge_elems_moved(my_name: &String, moved: &[String], left: &[String]) -> String {
    let list = |names: &[String]| {
        let fmt: Vec<String> = names.iter().map(|m| styled_name_html(m)).collect();
        match fmt.as_slice() {
            [init @ .., last] if !init.is_empty() => format!("{} and {}", init.join(", "), last),
            _ => fmt.concat(),
        }
    };
    let rest = if left.is_empty() {
        String::new()
    } else {
        format!("; {} {} still in it", list(left), if left.len() > 1 { "are" } else { "is" })
    };
    format!(
        "{1} may have been moved out of {0} in a branch above, so {0} can't be used as a whole{2}",
        styled_name_html(my_name), list(moved), rest
    )
}

/// The end of a branch of type `!` (`return`, `panic!()`, `break`,
/// `continue`, `unreachable!()`) on a variable's column: the path
/// stops here and never reaches the code after the conditional (#317).
/// The bar sits on the column, so the message doesn't name it.
pub fn branch_never_reaches_merge() -> String {
    "This branch stops here: it returns, panics, breaks or continues, so it never \
     reaches the end of the conditional"
        .to_string()
}

/// Suffix of the scope-end message of a variable that a conditional
/// moved or dropped in every branch: the conditional's join has no dot
/// of its own (no arm carries a line into it, #694), so the reason
/// nothing is dropped is stated here. It's definite: no "maybe", the
/// variable owns nothing after the conditional.
/// `some_diverge`: a branch has type `!` (`return`, `panic!()`) and
/// never gets here; the claim is about the branches that do (#296).
pub fn event_dot_go_out_of_scope_all_moved_suffix(my_name: &String, some_diverge: bool) -> String {
    format!(
        ". No resource is dropped: {0} was moved or dropped in every branch above{1}.",
        styled_name_html(my_name),
        if some_diverge { " that reaches this point" } else { "" }
    )
}

/// Every branch above contributed a value that ends up in this
/// variable (the `let s = if … { … } else { … };` shape, plus the
/// match-as-rhs analog). After the conditional the variable owns a
/// freshly bound resource regardless of which branch ran.
/// `diverged`: the branches of type `!` (`_ => unreachable!()`,
/// `return`), which produce no value and never get here, as (name, how
/// it leaves) pairs; every branch that does get here bound a value
/// (#296).
pub fn event_dot_branch_merge_bound(my_name: &String, diverged: &[(String, Vec<BranchExit>)]) -> String {
    format!(
        "{0} acquired ownership of a resource{1}",
        styled_name_html(my_name),
        bound_in_branches(diverged)
    )
}

/// `event_dot_branch_merge_bound` for a reference variable: every
/// branch bound it to a reference, which never owns its referent.
pub fn event_dot_branch_merge_bound_ref(my_name: &String, is_mut: bool, diverged: &[(String, Vec<BranchExit>)]) -> String {
    let kind = if is_mut { "a mutable" } else { "an immutable" };
    format!(
        "{0} is bound to {1} reference{2}",
        styled_name_html(my_name), kind,
        bound_in_branches(diverged)
    )
}

/// `event_dot_branch_merge_bound` for a Copy variable (`let n = match x
/// { 0 => 1, _ => 2 };`): an `i32` holds a value, not a resource it
/// owns (#316). Matches the per-arm `event_dot_acquire_copyable`.
pub fn event_dot_branch_merge_bound_copy(my_name: &String, diverged: &[(String, Vec<BranchExit>)]) -> String {
    format!(
        "{0} is bound to a value{1}",
        styled_name_html(my_name),
        bound_in_branches(diverged)
    )
}

/// The "in which branches" tail of the bound-at-merge messages: " (in
/// all branches above)", or, when some branches never get here, "in
/// every branch that reaches this point (the _ arm panicked)".
fn bound_in_branches(diverged: &[(String, Vec<BranchExit>)]) -> String {
    if diverged.is_empty() {
        return " (in all branches above)".to_owned();
    }
    format!(" in every branch that reaches this point ({})", how_branches_left(diverged))
}

/// "the _ arm panicked", "the if branch returned", "the 0 and 1 arms
/// returned", "the 0 arm returned, the _ arm panicked". Branch names
/// are "If" / "Else" for an `if`, an arm's pattern for a `match`.
fn how_branches_left(diverged: &[(String, Vec<BranchExit>)]) -> String {
    let noun = |name: &String| match name.as_str() {
        "If" => "the if branch".to_owned(),
        "Else" => "the else branch".to_owned(),
        _ => format!("the {} arm", styled_name_html(name)),
    };
    let same_way = diverged.iter().all(|(_, e)| *e == diverged[0].1);
    if same_way && diverged.iter().all(|(n, _)| n != "If" && n != "Else") && diverged.len() > 1 {
        let names: Vec<String> = diverged.iter().map(|(n, _)| styled_name_html(n)).collect();
        format!("the {} arms {}", join_and(&names), exits_verb(&diverged[0].1, true))
    } else {
        diverged.iter()
            .map(|(n, e)| format!("{} {}", noun(n), exits_verb(e, false)))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn join_and(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} and {}", init.join(", "), last),
    }
}

/// How a branch of type `!` left, as a verb phrase: "returned", "broke
/// out of the loop", "continued to the next iteration", "panicked";
/// several ways: "left early (return, break)".
fn exits_verb(exits: &[BranchExit], plural: bool) -> String {
    match exits {
        [BranchExit::Return] => "returned".to_owned(),
        [BranchExit::Break] => "broke out of the loop".to_owned(),
        [BranchExit::BreakBlock] => "broke out of the labeled block".to_owned(),
        [BranchExit::Continue] => "continued to the next iteration".to_owned(),
        [BranchExit::Panic] => "panicked".to_owned(),
        [BranchExit::Other] | [] => if plural { "never finish" } else { "never finishes" }.to_owned(),
        _ => {
            let mut words: Vec<&str> = exits.iter().map(|e| match e {
                BranchExit::Return => "return",
                BranchExit::Break | BranchExit::BreakBlock => "break",
                BranchExit::Continue => "continue",
                BranchExit::Panic => "panic",
                BranchExit::Other => "a call that never returns",
            }).collect();
            words.dedup();
            format!("left early ({})", words.join(", "))
        }
    }
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
// says that (`event_dot_go_out_of_scope_maybe_moved`). `value`: it holds
// a value or a reference, which owns nothing to drop.
// `mutable`: only a `let mut` binding can be assigned again (a second
// assignment to an immutable one is E0384). `by_cond`: a let-chain's
// condition, not only a branch, may have moved it (`if let … && { let
// w = s; … }`, #953); see `branch_merge_moved`.
pub fn event_dot_branch_merge_moved_with_drop(my_name: &String, value: bool, mutable: bool, note: &str, by_cond: Option<&str>) -> String {
    let my_name_fmt = styled_name_html(my_name);
    let again = if mutable { " (until it's assigned again)" } else { "" };
    let moved = branch_merge_moved(&my_name_fmt, by_cond);
    if value {
        return format!("{}{}.", moved, again);
    }
    let (holds, rest) = split_drop_note(note);
    format!(
        "{0}{3}. {4}, its resource{1} is dropped{2} when {5} goes out of scope.",
        moved, holds, rest, again, branch_merge_unless(by_cond.is_some()), my_name_fmt
    )
}

// `event_dot_branch_merge_moved_with_drop` for a Box whose contents were
// moved out before the branches, some of which gave it a new Box or moved
// it away (#1220): on the paths that didn't move it, it may hold only the
// emptied Box, so what its scope end drops depends on the path.
pub fn event_dot_branch_merge_moved_box_paths(my_name: &String, mutable: bool, by_cond: Option<&str>) -> String {
    let my_name_fmt = styled_name_html(my_name);
    let again = if mutable { " (until it's assigned again)" } else { "" };
    format!(
        "{0}{1}. What is dropped when {2} goes out of scope depends on the path: its contents were moved out before the branches.",
        branch_merge_moved(&my_name_fmt, by_cond), again, my_name_fmt
    )
}

// The start of a join's text for a variable moved on some paths into it:
// "s was moved in at least one branch above, so it can't be used after
// the branches". `by_cond`: a let-chain's condition moved it, possibly
// with no branch body moving it at all (#953): on the path where the
// condition moves s and a later part fails, the branch that runs (the
// else, or the skipped body) doesn't move s, yet s is gone. So it says
// "s may have been moved above, so it can't be used after the if", the
// wording of a Box's contents moved there (#940). A `match` arm's guard
// that moves s and then fails is the same (#247): `by_cond` is the
// conditional's keyword, "if" or "match".
fn branch_merge_moved(name: &str, by_cond: Option<&str>) -> String {
    if let Some(kw) = by_cond {
        format!(
            "{0} may have been moved above, so it can't be used after the {1}",
            name, styled_name_html(&kw.to_owned())
        )
    } else {
        format!("{} was moved in at least one branch above, so it can't be used after the branches", name)
    }
}

// The condition under which a maybe-moved variable's resource is still
// there to drop (see `branch_merge_moved`).
fn branch_merge_unless(by_cond: bool) -> &'static str {
    if by_cond { "If nothing above moved it" } else { "If the branch that ran didn't move it" }
}

/// Where the contents of a Box that some branches moved out of (`let w
/// = *e;`) are dropped on the paths that didn't (#921).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ContentsDropAt {
    /// At e's scope end, together with the Box's heap allocation.
    ScopeEnd,
    /// At `e = …`, which also frees the old Box's heap allocation.
    Reassign,
    /// At `*e = …`, which keeps the Box's heap allocation.
    PayloadWrite,
}

// `event_dot_branch_merge_moved_with_drop` for a Box whose contents (`*e`),
// not the Box itself, were moved out in some branches (#921). e still
// owns its heap allocation on every path, so the drop flag decides only
// whether the contents are dropped. rustc rejects any use of e after the
// join (E0382) except a new value for `*e` or for e (both need `let mut
// e`). `holds`: the Box's `Owner::holds` (`the String it holds`), `None`
// when the contents need no drop. `by_cond`: a let-chain's condition,
// not only a branch, may have moved them (`if let … && { let w = *e;
// … }`, #940): see `moved_above`.
pub fn event_dot_branch_merge_box_contents_moved(my_name: &String, mutable: bool, holds: Option<&str>, at: ContentsDropAt, by_cond: Option<&str>) -> String {
    let name = styled_name_html(my_name);
    let deref = styled_name_html(&format!("*{}", my_name));
    let until = match at {
        ContentsDropAt::Reassign => format!(" until {} is assigned again", name),
        ContentsDropAt::PayloadWrite => format!(" until {} is assigned again", deref),
        ContentsDropAt::ScopeEnd if mutable => format!(" (until {} or {} is assigned again)", deref, name),
        ContentsDropAt::ScopeEnd => String::new(),
    };
    let mut out = if let Some(kw) = by_cond {
        format!(
            "{0}'s contents may have been moved out above, so {0} can't be used after the {1}{2}.",
            name, styled_name_html(&kw.to_owned()), until
        )
    } else {
        format!(
            "{0}'s contents were moved out in at least one branch above, so {0} can't be used after \
             the branches{1}.",
            name, until
        )
    };
    let when = match at {
        ContentsDropAt::ScopeEnd => format!("when {} goes out of scope", name),
        ContentsDropAt::Reassign => format!("when {} is assigned again", name),
        ContentsDropAt::PayloadWrite => format!("when {} is assigned again", deref),
    };
    if let Some(h) = holds {
        let what = h.strip_suffix(" it holds").map_or_else(
            || format!("{}'s contents", name),
            escape_tooltip_name,
        );
        let unless = if by_cond.is_some() { "If nothing above moved them" } else { "If the branch that ran didn't move them" };
        out.push_str(&format!(" {}, {} is dropped {}.", unless, what, when));
        if at != ContentsDropAt::PayloadWrite {
            out.push_str(" The Box's heap allocation is freed then either way.");
        }
    } else if at != ContentsDropAt::PayloadWrite {
        out.push_str(&format!(" The Box's heap allocation is freed {} either way.", when));
    }
    out
}

// `event_dot_branch_merge_moved_with_drop` when the variable is assigned
// again before it goes out of scope: the old value, if it's still there,
// is dropped at that assignment instead (#335). `in_loop`: that
// assignment is in a loop's body, which may end before it runs (#1012).
pub fn event_dot_branch_merge_moved_then_reassigned(my_name: &String, by_cond: Option<&str>, in_loop: bool) -> String {
    let name = styled_name_html(my_name);
    format!(
        "{} until it's assigned again. {}, its resource is dropped when {} is assigned again{}.",
        branch_merge_moved(&name, by_cond), branch_merge_unless(by_cond.is_some()), name,
        if in_loop { " (or, if it isn't, when it goes out of scope)" } else { "" }
    )
}

// The join of a conditional that gives a new value, in only some of its
// branches, to a variable that may have held none where it started
// (#1034): `if c { drop(s); } if x { s = String::from("b"); }`. After
// it, s holds the new value, the old one (if nothing moved it), or
// nothing: rustc rejects a read of s there (E0382), and a drop flag
// decides what is dropped. `value`, `mutable`: as for
// `event_dot_branch_merge_maybe_init`. `reassigned`: as for
// `event_dot_loop_maybe_init`.
pub fn event_dot_branch_merge_maybe_held(my_name: &String, value: bool, mutable: bool, reassigned: Option<bool>) -> String {
    let my_name_fmt = styled_name_html(my_name);
    let mut out = format!(
        "{0} may have no value after the branches: it may have had none where they start, and only some \
         of them assign it, so it can't be used after the branches{1}.",
        my_name_fmt,
        if mutable { " (until it's assigned again)" } else { "" },
    );
    if !value {
        out.push_str(&format!(
            " If it has one, its resource is dropped when {0} {1}.",
            my_name_fmt,
            match reassigned {
                Some(true) => "is assigned again",
                Some(false) => "is assigned again (or, if it isn't, when it goes out of scope)",
                None => "goes out of scope",
            },
        ));
    }
    out
}

// ── Maybe-initialized `let x;` locals (#244) ──────────────────────
//
// `let mut s; if c { s = String::from("a"); }`: after the `if`, s holds
// a value on the path that assigned it and none on the other. rustc
// rejects any read of s there (E0381, "possibly-uninitialized"), and
// keeps a runtime drop flag for it, like for a maybe-moved variable: s's
// next assignment, or else its scope end, drops the old value only if
// there is one. The wording mirrors the maybe-moved messages above.

// `moved` (all four): another branch assigned s and then moved its
// value away, so the drop also needs s not to have been moved there.
fn assigned_in_branch_that_ran(moved: bool) -> &'static str {
    if moved { "assigned it and didn't move it" } else { "assigned it" }
}

// The join of the conditional. `value`: s holds a Copy value, a
// reference or another value with nothing to drop, so nothing is ever
// dropped. `mutable`: s can be assigned again (an immutable `let s;`
// can't be: a second assignment is E0384).
pub fn event_dot_branch_merge_maybe_init(my_name: &String, value: bool, mutable: bool, moved: bool, note: &str) -> String {
    let my_name_fmt = styled_name_html(my_name);
    let mut out = format!(
        "{0} was assigned a value in only some of the branches above, so it can't be used after \
         the branches{1}.",
        my_name_fmt,
        if mutable { " (until it's assigned again)" } else { "" }
    );
    if !value {
        let (holds, rest) = split_drop_note(note);
        out.push_str(&format!(
            " If the branch that ran {3}, its resource{1} is dropped{2} when {0} goes out of scope.",
            my_name_fmt, holds, rest, assigned_in_branch_that_ran(moved)
        ));
    }
    out
}

// ── Temporaries a `let` extends through a branch (#1397) ─────────
//
// `let r = if c { &String::from("x") } else { &s };`: Rust extends the
// String temporary the if branch borrows to the end of the enclosing
// block (since 1.79), but only that path makes it, so a drop flag
// decides whether it is dropped there. Drawn like a `let x;` assigned in
// that branch only (#244). `made_at`: the line the branch makes it on.

fn temp_made_in(made_at: Option<usize>) -> String {
    match made_at {
        Some(l) => format!("the branch that made it (line {}) ran", l),
        None => "the branch that made it ran".to_owned(),
    }
}

// The join.
pub fn event_dot_branch_merge_temp_made(my_name: &String, made_at: Option<usize>) -> String {
    format!(
        "{0} exists after the branches only if {1}. If it did, it lives until the end of the \
         enclosing block: a let borrowed it, so it is kept alive past the end of its statement.",
        styled_name_html(my_name), temp_made_in(made_at)
    )
}

// Its scope end, with the drop flag's drop.
pub fn event_dot_go_out_of_scope_temp_made(my_name: &String, made_at: Option<usize>) -> String {
    format!(
        "{0} goes out of scope at the end of the block: a let borrowed it, so it was kept alive \
         past the end of its statement. Its resource is dropped here only if {1} (Rust tracks \
         this with a runtime drop flag).",
        styled_name_html(my_name), temp_made_in(made_at)
    )
}

// `event_dot_branch_merge_maybe_init` when s is assigned again before it
// goes out of scope: the old value, if there is one, is dropped at that
// assignment instead. `maybe`: the next assignment is in a branch of a
// later conditional, so on the other paths it's dropped at scope end.
pub fn event_dot_branch_merge_maybe_init_then_reassigned(my_name: &String, maybe: bool, moved: bool) -> String {
    format!(
        "{0} was assigned a value in only some of the branches above, so it can't be used after \
         the branches until it's assigned again. If the branch that ran {2}, its resource \
         is dropped when {0} is assigned again{1}.",
        styled_name_html(my_name),
        if maybe { " (or, if it isn't, when it goes out of scope)" } else { "" },
        assigned_in_branch_that_ran(moved)
    )
}

// That next assignment (`event_dot_owner_drop_at_reassign_maybe_moved`'s
// counterpart).
pub fn event_dot_owner_drop_at_reassign_maybe_init(my_name: &String, note: &str, moved: bool) -> String {
    let (holds, rest) = split_drop_note(note);
    format!(
        "{0} acquires ownership of a new resource. Its previous resource{1} is dropped{2} here only \
         if {0} was assigned in the branch that ran{3} (Rust tracks this with a runtime drop flag).",
        styled_name_html(my_name), holds, rest,
        if moved { " and wasn't moved there" } else { "" }
    )
}

// Its scope end, when it isn't assigned again
// (`event_dot_go_out_of_scope_maybe_moved`'s counterpart).
pub fn event_dot_go_out_of_scope_maybe_init(my_name: &String, note: &str, moved: bool) -> String {
    let (holds, rest) = split_drop_note(note);
    format!(
        "{0} goes out of scope. Its resource{1} is dropped{2} here only if {0} was assigned in the \
         branch that ran{3} (Rust tracks this with a runtime drop flag).",
        styled_name_html(my_name), holds, rest,
        if moved { " and wasn't moved there" } else { "" }
    )
}

// The join when every branch that assigned s moved its value away: s
// holds nothing after it, on any path.
pub fn event_dot_branch_merge_init_all_moved(my_name: &String, mutable: bool) -> String {
    format!(
        "{0} was assigned a value in only some of the branches above, and each of them moved it \
         away, so {0} holds nothing after the branches{1}.",
        styled_name_html(my_name),
        if mutable { " (until it's assigned again)" } else { "" }
    )
}

// Its scope end.
pub fn event_dot_go_out_of_scope_init_all_moved(my_name: &String) -> String {
    format!(
        "{0} goes out of scope. No resource is dropped: each branch above that assigned {0} moved it away.",
        styled_name_html(my_name)
    )
}

// ── `let x;` locals assigned in a loop (#786) ─────────────────────
//
// `let mut s; for … { s = String::from("x"); }`: the first time round,
// the assignment initializes s; later times round, it first drops the
// String an earlier iteration assigned, if s still holds it. After the
// loop s holds a value only if the loop assigned it (a `while` / `for`
// may not run at all, a `break` may come first): rustc rejects a read
// there (E0381), and a drop flag decides what's dropped.

// The assignment in the loop body, drawn as a drop on its dot (the
// acquire's text is listed with it).
pub fn event_dot_init_drops_earlier_iteration(my_name: &String) -> String {
    format!(
        "On a later iteration of the loop, the resource an earlier iteration gave {0} is dropped \
         here first, if {0} still holds it (Rust tracks this with a runtime drop flag).",
        styled_name_html(my_name)
    )
}

// Where the loop ends. `value`, `mutable`: as for
// `event_dot_branch_merge_maybe_init`. `reassigned`: s is assigned
// again before its scope ends, on every path (`Some(true)`) or only in
// a branch of a later conditional (`Some(false)`).
//
// `before_loop`: s only may have held a value as the loop started (a
// branch above moved it on some paths, #1012), `Some(by_cond)` (see
// `moved_above`): after the loop s holds one if the loop assigned it or
// if nothing moved it before.
pub fn event_dot_loop_maybe_init(my_name: &String, value: bool, mutable: bool, reassigned: Option<bool>, before_loop: Option<bool>) -> String {
    let my_name_fmt = styled_name_html(my_name);
    let mut out = format!(
        "{0} may have no value after the loop: {2}the loop may end before assigning it, so it can't be \
         used after the loop{1}.",
        my_name_fmt,
        if mutable { " (until it's assigned again)" } else { "" },
        match before_loop {
            Some(by_cond) => format!("{} may have moved it, and ", moved_above_maybe(by_cond)),
            None => String::new(),
        }
    );
    if !value {
        out.push_str(&format!(
            " If the loop assigned it{2}, its resource is dropped when {0} {1}.",
            my_name_fmt,
            match reassigned {
                Some(true) => "is assigned again",
                Some(false) => "is assigned again (or, if it isn't, when it goes out of scope)",
                None => "goes out of scope",
            },
            or_not_moved_above(before_loop)
        ));
    }
    out
}

// Where a loop inside another ends, for a variable declared before the
// outer one that may hold nothing where this loop starts (on the outer
// loop's first time round, a branch above may have moved it) and that
// this loop may end before assigning (#1032). Whatever value it has
// here, from this loop or from the outer loop's time round before, is
// dropped by its next assignment (in the outer loop's body, or this
// loop's next run), or at its scope end: nothing can move it before it
// is assigned again.
pub fn event_dot_loop_maybe_init_nested(my_name: &String, value: bool, mutable: bool) -> String {
    let my_name_fmt = styled_name_html(my_name);
    let mut out = format!(
        "{0} may have no value after the loop: it may have none where the loop starts, and the loop \
         may end before assigning it, so it can't be used after the loop{1}.",
        my_name_fmt,
        if mutable { " (until it's assigned again)" } else { "" },
    );
    if !value {
        out.push_str(&format!(
            " If it has one, its resource is dropped when {0} is assigned again (or, if it isn't, when \
             it goes out of scope).",
            my_name_fmt
        ));
    }
    out
}

// The next assignment after the loop.
// `before_loop`: as for `event_dot_loop_maybe_init`.
pub fn event_dot_owner_drop_at_reassign_after_loop(my_name: &String, note: &str, before_loop: Option<bool>) -> String {
    let (holds, rest) = split_drop_note(note);
    format!(
        "{0} acquires ownership of a new resource. Its previous resource{1} is dropped{2} here only \
         if the loop above assigned {0}{3} (Rust tracks this with a runtime drop flag).",
        styled_name_html(my_name), holds, rest, or_not_moved_above(before_loop)
    )
}

// `event_dot_owner_drop_at_reassign_maybe_entry` in a loop inside
// another that s was declared before (#1019): a run of the inner loop
// may follow an earlier one that gave s a value, so the first time
// round it isn't only the branch above that decides. When the loops
// never move s (`moves` false), s holds a value here unless the branch
// above moved it and nothing gave s a new one since; otherwise only the
// drop flag knows.
pub fn event_dot_owner_drop_at_reassign_outer_loop(my_name: &String, note: &str, by_cond: bool, moves: bool) -> String {
    let (holds, rest) = split_drop_note(note);
    let name = styled_name_html(my_name);
    format!(
        "{0} acquires ownership of a new resource. Its previous resource{1} is dropped{2} here only if \
         {0} still holds it (Rust tracks this with a runtime drop flag){3}.",
        name, holds, rest,
        if moves {
            String::new()
        } else {
            format!(": it does unless {} moved it and {} hasn't been assigned since", moved_above_maybe(by_cond), name)
        }
    )
}

// After a loop that may end after a time round that moved s, when other
// time rounds (or the path into the loop) leave s holding a value
// (`MaybeMovedAt::LoopRound { kept: true }`, #1019): "only if nothing
// above moved it" would read as if one move anywhere above settled it.
pub fn event_dot_go_out_of_scope_maybe_kept(my_name: &String, note: &str) -> String {
    let (holds, rest) = split_drop_note(note);
    format!(
        "{0} goes out of scope. Its resource{1} is dropped{2} here only if {0} still holds it \
         (Rust tracks this with a runtime drop flag).",
        styled_name_html(my_name), holds, rest
    )
}

// The next assignment after such a loop.
pub fn event_dot_owner_drop_at_reassign_maybe_kept(my_name: &String, note: &str) -> String {
    let (holds, rest) = split_drop_note(note);
    format!(
        "{0} acquires ownership of a new resource. Its previous resource{1} is dropped{2} here only if \
         {0} still holds it (Rust tracks this with a runtime drop flag).",
        styled_name_html(my_name), holds, rest
    )
}

// Its scope end, when it isn't assigned again.
pub fn event_dot_go_out_of_scope_after_loop(my_name: &String, note: &str, before_loop: Option<bool>) -> String {
    let (holds, rest) = split_drop_note(note);
    format!(
        "{0} goes out of scope. Its resource{1} is dropped{2} here only if the loop above assigned \
         {0}{3} (Rust tracks this with a runtime drop flag).",
        styled_name_html(my_name), holds, rest, or_not_moved_above(before_loop)
    )
}

// " or no branch above moved it" for `before_loop` (see
// `event_dot_loop_maybe_init`).
fn or_not_moved_above(before_loop: Option<bool>) -> String {
    match before_loop {
        Some(by_cond) => format!(" or {} moved it", moved_above(by_cond)),
        None => String::new(),
    }
}

// "A branch above" / "Something above": what may have moved a variable
// before a loop (`moved_above`'s counterpart).
fn moved_above_maybe(by_cond: bool) -> &'static str {
    if by_cond { "something above" } else { "a branch above" }
}

// A reassignment in a loop body, first thing the body does to s, where
// s only may hold a value as the loop starts (a branch above moved it
// on some paths, #1012): the first time round the old value is there
// only if no branch above moved it; a later time round, only if the
// time round before left s holding one (it may have moved it, or given
// it a new one). Starts ". Its previous " (`reassign_subsumes_acquire`).
pub fn event_dot_owner_drop_at_reassign_maybe_entry(my_name: &String, note: &str, by_cond: bool) -> String {
    let (holds, rest) = split_drop_note(note);
    format!(
        "{0} acquires ownership of a new resource. Its previous resource{1} is dropped{2} here only if \
         {0} still holds it (Rust tracks this with a runtime drop flag): the first time round the loop, \
         only if {3} moved it; a later time round, only if the time round before left {0} holding one.",
        styled_name_html(my_name), holds, rest, moved_above(by_cond)
    )
}

// Scope end of a variable that was moved on some paths of a conditional
// and not on others: whether anything is dropped is decided at runtime
// by a drop flag (#179).
//
// `boxed` (#736): a `Box` whose contents (`*b`) are moved out on some
// path. The drop flag decides only whether the contents are dropped;
// the Box's own heap allocation is freed either way.
pub fn event_dot_go_out_of_scope_maybe_moved(my_name: &String, note: &str, by_cond: bool, boxed: bool) -> String {
    let (holds, rest) = split_drop_note(note);
    format!(
        "{0} goes out of scope. Its resource{1} is dropped{2} here only if {3} moved it \
         (Rust tracks this with a runtime drop flag).{4}",
        styled_name_html(my_name), holds, rest, moved_above(by_cond),
        if boxed { " The Box's heap allocation is freed either way." } else { "" }
    )
}

// `let r = loop { let k = …; if c { break k; } };` (#431): the
// `break` moves `k`, declared inside the loop, to the loop's receiver.
// That is drawn where the loop ends, which is also where `k` goes out
// of scope on the iterations that don't break (they drop it there).
// The same when the loop is what the function returns (#1171).
pub fn event_dot_break_out_move_to_caller(my_name: &String, value: bool) -> String {
    format!(
        "{0}'s {1} is moved to the caller by the {2} that ends the loop, which the function returns",
        styled_name_html(my_name),
        if value { "value" } else { "resource" },
        styled_name_html(&"break".to_owned())
    )
}

pub fn event_dot_break_out_move(my_name: &String, to: &String, value: bool) -> String {
    format!(
        "{0}'s {1} is moved to {2} by the {3} that ends the loop",
        styled_name_html(my_name),
        if value { "value" } else { "resource" },
        styled_name_html(to),
        styled_name_html(&"break".to_owned())
    )
}

pub fn event_dot_break_out_go_out_of_scope(my_name: &String) -> String {
    format!(
        "{0} goes out of scope. Its resource is dropped here on every iteration that doesn't end the loop.",
        styled_name_html(my_name)
    )
}

// The same at the end of a labeled block (#1288): `'blk: { let k = ..;
// if c { break 'blk k; } .. }`. Leaving the block through that `break`
// moves k (or a field of it) out; reaching the block's end drops it.
pub fn event_dot_block_break_out_go_out_of_scope(my_name: &String, alt: &crate::svg_generator::data::BreakAlt) -> String {
    format!(
        "{0} goes out of scope. Its resource is dropped as the block ends, unless the block ended with {1}",
        styled_name_html(my_name), alt_how(alt)
    )
}

// The struct a field of which such a way out carries out (`break 'blk
// g.name`): that way drops the rest of it as it leaves the block.
pub fn event_dot_block_break_out_part_go_out_of_scope(my_name: &String, alt: &crate::svg_generator::data::BreakAlt, part: &String) -> String {
    format!(
        "{0} goes out of scope. Its resource is dropped as the block ends (if the block ended with {1}, that moved {2} out first, and only the rest of {0} is dropped)",
        styled_name_html(my_name), alt_how(alt), styled_name_html(part)
    )
}

pub fn event_dot_block_break_out_go_out_of_scope_moved(my_name: &String) -> String {
    format!(
        "{0} goes out of scope. No resource is dropped: on the way to the block's end, {0} was moved above.",
        styled_name_html(my_name)
    )
}

pub fn event_dot_block_break_out_go_out_of_scope_maybe_moved(my_name: &String, note: &str, alt: &crate::svg_generator::data::BreakAlt) -> String {
    let (holds, rest) = split_drop_note(note);
    format!(
        "{0} goes out of scope. Unless the block ended with {3}, its resource{1} is dropped{2} as the block ends \
         only if no branch above moved it (Rust tracks this with a runtime drop flag).",
        styled_name_html(my_name), holds, rest, alt_how(alt)
    )
}

// `slice = &mut slice[1..];` / `s = &s[1..];` (#1123): the reference now
// points to part of what it pointed to. The new reference is a reborrow
// through the old one, so the loan of `lender` goes on until the
// reference's last use; nothing is given back here.
pub fn event_dot_resliced_in_place(my_name: &String, lender: &String, mutable: bool, part: bool) -> String {
    format!(
        "{0} now points to {3} (a reborrow through {0} itself). \
         It still borrows from {1}, so {1} stays {2} borrowed while {0} is in use",
        styled_name_html(my_name), styled_name_html(lender), if mutable { "mutably" } else { "immutably" },
        if part { "part of the data it pointed to" } else { "the same data again" }
    )
}

// The same when the loop body moved it before reaching its end (#1156):
// on the iterations that don't end the loop it holds nothing here.
pub fn event_dot_break_out_go_out_of_scope_moved(my_name: &String) -> String {
    format!(
        "{0} goes out of scope. No resource is dropped: on the iterations that don't end the loop, {0} was moved above.",
        styled_name_html(my_name)
    )
}

// ... or moved on some paths through the loop body only: the drop flag
// decides on those iterations (#1156).
pub fn event_dot_break_out_go_out_of_scope_maybe_moved(my_name: &String, note: &str) -> String {
    let (holds, rest) = split_drop_note(note);
    format!(
        "{0} goes out of scope. On the iterations that don't end the loop, its resource{1} is dropped{2} here \
         only if no branch above moved it (Rust tracks this with a runtime drop flag).",
        styled_name_html(my_name), holds, rest
    )
}

/// A reference parameter a returned `if` / `match` doesn't return on
/// this path, tied to the result by the signature's lifetime (#775).
/// A struct's field on the row a call borrows the whole struct (#1311):
/// the field is borrowed with it, for the call, and may be changed
/// through a mutable borrow (`reset(&mut self)` writes `self.n`).
pub fn event_dot_lent_with_parent(my_name: &String, parent: &str, to: &str, mutable: bool) -> String {
    let (me, p, t) = (styled_name_html(my_name), styled_name_html(parent), styled_name_html(to));
    if mutable {
        format!("{0} is mutably borrowed along with {1} by {2}, which may change {0}; the borrow ends here", me, p, t)
    } else {
        format!("{0} is immutably borrowed along with {1} by {2}, which can read {0}; the borrow ends here", me, p, t)
    }
}

pub fn event_dot_tied_param_note(my_name: &String, lifetime: &str) -> String {
    format!(
        "{0} isn't returned on this path, but its borrow lasts as long as the result (same lifetime {1})",
        styled_name_html(my_name),
        lifetime
    )
}

/// The `else` block of a lone `if let` whose pattern always matches is
/// dead code (#987): it never runs, so what it does to `my_name` isn't
/// drawn. Its first line there says so (a dot, not a drop).
pub fn event_dot_dead_else_note(my_name: &String) -> String {
    format!(
        "The `if let` pattern above always matches, so this else branch never runs: nothing in it happens to {0}",
        styled_name_html(my_name)
    )
}

/// A `for` loop's pattern leaves `parts` of each element unbound (`.1`
/// in `for (f, _) in v`; empty: the whole element, `for _ in v`, #1434):
/// Rust drops them at the end of each pass, with what is left of that
/// pass's element.
pub fn event_dot_pass_rest_drop(parts: &[String]) -> String {
    if parts.is_empty() {
        return "At the end of each pass, the element that pass took is dropped: the pattern `_` doesn't bind it".to_owned();
    }
    let shown: Vec<String> = parts.iter().map(|p| format!("`{}`", p)).collect();
    let (list, verb, pron) = match shown.len() {
        1 => (shown[0].clone(), "is", "it"),
        n => (format!("{} and {}", shown[..n - 1].join(", "), shown[n - 1]), "are", "them"),
    };
    format!("At the end of each pass, the {0} of that pass's element {1} dropped: the pattern doesn't move {2} out", list, verb, pron)
}

/// A write to `lender` drawn inside its borrow by `borrower`, on a path
/// where that loan is already dead (#796): `let mut c = &x; if cond { x
/// = 2; c = &y; } println!("{}", c);`. The diagram gives a loan one end,
/// the latest over the paths, as the borrow checker does; on the write's
/// path `borrower` gets a new reference (`repoint`: its line, and whether
/// that is before the write) before it is used again, or isn't used
/// again (None). Starts with DEAD_LOAN_NOTE_START (see merged_dot_title).
pub fn event_dot_dead_loan_note(lender: &String, borrower: &String, repoint: Option<(usize, bool)>) -> String {
    let (x, c) = (styled_name_html(lender), styled_name_html(borrower));
    match repoint {
        Some((n, true)) => format!(
            "{DEAD_LOAN_NOTE_START} {c} was given a new reference on line {n}, so {c}'s borrow of {x} is already over here; {x} stays borrowed only on paths that skip line {n}"
        ),
        Some((n, false)) => format!(
            "{DEAD_LOAN_NOTE_START} {c} is given a new reference on line {n} before it is used again, so {c}'s borrow of {x} is already over here; {x} stays borrowed only on paths that skip line {n}"
        ),
        None => format!(
            "{DEAD_LOAN_NOTE_START} {c} isn't used again, so {c}'s borrow of {x} is already over here; {x} stays borrowed only on the other paths"
        ),
    }
}

pub const DEAD_LOAN_NOTE_START: &str = "On this path";

// Merge dot for a variable that no branch above moved (#179; this used
// to be an empty tooltip).
pub fn event_dot_branch_merge_unchanged(my_name: &String) -> String {
    format!("{0} isn't moved in any of the branches above", styled_name_html(my_name))
}
// Merge dot for a variable that a branch above moved and then gave a new
// value (`if c { drop(s); s = String::new(); }`, #1413): it holds a value
// after the branches on every path, though it isn't the one it had.
pub fn event_dot_branch_merge_reassigned_after_move(my_name: &String) -> String {
    format!("{0} holds a value after the branches: each branch above that moved it gave it a new one", styled_name_html(my_name))
}
// Extra text for a column label: for a subscript (`s₂`), a variable
// that shadows an earlier one of the same name (`let s` while the first
// `s` is in scope, see `binding_name`); for a column of a variable whose
// same-named variables in separate scopes are a different kind of value
// and have columns of their own (#598), that it is a different variable
// from those. Empty for any other name.
pub fn shadowed_name_note(name: &String, note: Option<&crate::svg_generator::data::ShadowNote>) -> String {
    let Some(note) = note else { return String::new() };
    let mut out = String::new();
    if let (Some((base, _)), Some(shadowed)) = (crate::expr_visitor_utils::split_numbered_name(name), &note.shadows) {
        let base = String::from(crate::expr_visitor_utils::written_name(base));
        let declared = match note.lines.as_slice() {
            [one] => format!("a new variable named {}, declared on line {}", styled_name_html(&base), one),
            many => {
                let (last, init) = many.split_last().unwrap();
                let init: Vec<String> = init.iter().map(|l| l.to_string()).collect();
                format!(
                    "a new variable named {}, declared on lines {} and {} (in different branches or blocks, so only one exists at a time)",
                    styled_name_html(&base), init.join(", "), last
                )
            }
        };
        out += &format!(". {0} shadows {1}: it is {2}", styled_name_html(name), styled_name_html(shadowed), declared);
    }
    if let Some((last, init)) = note.apart_from.split_last() {
        let lines = if init.is_empty() {
            format!("line {}", last)
        } else {
            let init: Vec<String> = init.iter().map(|l| l.to_string()).collect();
            format!("lines {} and {}", init.join(", "), last)
        };
        out += &format!(
            ". A different variable from the {0} declared on {1} (separate scopes, so only one exists at a time)",
            styled_name_html(name), lines
        );
    }
    out
}

// Merge dot for a reference declared before the conditional whose
// borrow ended in each arm, at its last use there or, in an arm that
// doesn't use it, as the arm starts.
pub fn event_dot_branch_merge_borrow_ended(my_name: &String) -> String {
    format!("{0}'s borrow ended in each branch above", styled_name_html(my_name))
}

// Merge dot for a reference whose borrow ended before the conditional,
// given a new one in some arms that ended there (#1524).
pub fn event_dot_branch_merge_borrow_ended_before(my_name: &String) -> String {
    format!("{0}'s borrow ended before the branches above, and each borrow a branch gave {0} ended in that branch", styled_name_html(my_name))
}

// Dot where a conditional ends whose branches all diverge, some by
// `break`ing out of the loop around it (#1296): nothing gets here, but the
// column goes on after the loop, from the branches that break.
pub fn event_dot_branch_merge_after_break(my_name: &String, breaking: usize) -> String {
    format!(
        "No branch above reaches this point: {0} goes on after the loop from the {1}",
        styled_name_html(my_name),
        if breaking == 1 { "branch that breaks out of it" } else { "branches that break out of it" },
    )
}

// Dashed rest of an `if` / `match` arm after the borrow held by a
// reference declared before the conditional has ended in that arm (or
// before the conditional, until an arm gives it a new one, #1393).
pub fn state_ref_borrow_ended(my_name: &String) -> String {
    format!("{0}'s borrow has ended on this path", styled_name_html(my_name))
}

// Merge dot for a variable that was moved only in branches of type `!`
// (`if n > 0 { return s; }`, #318): those never get here, so on every
// path that does it still holds what it held before. `leavers`: how
// each such branch left. A reference never owns anything, so it
// "holds its reference" instead.
pub fn event_dot_branch_merge_kept(my_name: &String, is_ref: Option<bool>, value: bool, leavers: &[Vec<BranchExit>]) -> String {
    let name = styled_name_html(my_name);
    let holds = match is_ref {
        Some(true) => format!("{} still holds its mutable reference here", name),
        Some(false) => format!("{} still holds its reference here", name),
        // A value with nothing to drop is a "value", not a "resource" (#484).
        None if value => format!("{} still owns its value here", name),
        None => format!("{} still owns its resource here", name),
    };
    format!("{}: {}", holds, leavers_never_got_here("moved", &name, leavers))
}

// "the branch that moved s returned before reaching this point", "the
// branches that assigned x never finish, so they don't reach this
// point": why what the branches in `leavers` (how each left) did to
// the variable `name` doesn't show at the join. `verb`: what they did.
fn leavers_never_got_here(verb: &str, name: &str, leavers: &[Vec<BranchExit>]) -> String {
    let plural = leavers.len() > 1;
    let mut all: Vec<BranchExit> = leavers.iter().flatten().copied().collect();
    all.sort();
    all.dedup();
    let exits = if leavers.iter().all(|e| *e == leavers[0]) { leavers[0].clone() } else { all };
    let subject = if plural { format!("the branches that {} {}", verb, name) } else { format!("the branch that {} {}", verb, name) };
    match exits.as_slice() {
        [BranchExit::Other] | [] => format!(
            "{} {}, so {} reach this point",
            subject, exits_verb(&exits, plural), if plural { "they don't" } else { "it doesn't" }
        ),
        _ => format!("{} {} before reaching this point", subject, exits_verb(&exits, plural)),
    }
}

// Join of a conditional for a `let x;` local that has no value on any
// path that gets here (#919; this used to be an empty tooltip).
// `leavers`: how each branch that assigned it left (`if c { x = …;
// return; }`, a let-else's `else` block); none: no branch assigned it.
pub fn event_dot_branch_merge_still_unassigned(my_name: &String, leavers: &[Vec<BranchExit>]) -> String {
    let name = styled_name_html(my_name);
    if leavers.is_empty() {
        return format!("{} still has no value here: none of the branches above assigned it", name);
    }
    format!("{} still has no value here: {}", name, leavers_never_got_here("assigned", &name, leavers))
}

// Join of a conditional for a `let x;` local that may or may not have a
// value as the conditional starts (an `if` or a loop above may have
// assigned it), where the branches that assigned it return or panic
// (#1037, #1293): the paths that get here leave it as it was.
pub fn event_dot_branch_merge_maybe_still_unassigned(my_name: &String, leavers: &[Vec<BranchExit>]) -> String {
    let name = styled_name_html(my_name);
    format!(
        "{} may still have no value here: {}",
        name, leavers_never_got_here("assigned", &name, leavers)
    )
}

// The same where the branches that leave drop it instead (`if d {
// return; }` after the conditional that assigned it in some branches
// only, #1436). `temp`: a temporary a `let` extends through a branch
// (#1397), which a branch makes rather than assigns.
pub fn event_dot_branch_merge_maybe_init_unchanged(my_name: &String, temp: bool) -> String {
    let name = styled_name_html(my_name);
    if temp {
        format!("{} may not exist here, as before the branches: none of the branches that get here makes it", name)
    } else {
        format!("{} may have no value here, as before the branches: none of the branches that get here assigns it", name)
    }
}

// At the end of a loop, for a variable declared outside it that a
// branch inside moved before a `break` out of the loop. `definite`:
// every way out of the loop is such a `break`.
// `labeled_block`: the same at the end of a labeled block (`'a: { …
// break 'a; … }`). `carried`: a `break` before the move moves it itself
// (`break s`, #1504).
pub fn event_dot_loop_exit_move(my_name: &String, definite: bool, labeled_block: bool, carried: bool) -> String {
    let name = styled_name_html(my_name);
    let brk = styled_name_html(&"break".to_owned());
    let what = if labeled_block { "block" } else { "loop" };
    if definite && carried {
        let end = if labeled_block { format!(", and its end comes after {} is moved", name) } else { String::new() };
        format!(
            "{0} is moved by the time the {2} ends: each {1} out of it either comes after {0} is moved or moves {0} itself{3}, \
             so {0} can't be used after the {2}",
            name, brk, what, end
        )
    } else if definite && labeled_block {
        // A labeled block also ends by reaching its end (#1286).
        format!(
            "{0} was moved before the block ended: every {1} out of the block comes after {0} is moved, \
             and so does the block's end, so {0} can't be used after the block",
            name, brk
        )
    } else if definite {
        format!(
            "{0} was moved before the {2} ended: the only way out of the {2} is a {1} that comes after {0} is moved, so {0} can't be used after the {2}",
            name, brk, what
        )
    } else {
        format!(
            "{0} may have been moved: the {2} can end through a {1} that comes after {0} is moved, so {0} can't be used after the {2}",
            name, brk, what
        )
    }
}

// At the end of a labeled block (#1286), for a variable declared before
// it that the code on the way to the block's end moves, after a `break`
// out of the block that skips that code: `'a: { if c { break 'a 1; }
// drop(s); 2 }`. Which way the block ended decides whether s still holds
// its value after it.
pub fn event_dot_block_break_skips_move(my_name: &String) -> String {
    format!(
        "{0} may have been moved: the block can end through a {1} that comes before {0} is moved, \
         or by reaching its end after {0} is moved, so {0} can't be used after the block",
        styled_name_html(my_name),
        styled_name_html(&"break".to_owned())
    )
}

// At the end of a bare `loop` (#1000), for a variable declared before
// it that owns a value at every `break` out of it, although the body
// may leave it moved where it goes round again: `loop { s = …; if c {
// break; } drop(s); }`. Only a `break` reaches the code after the
// loop. `reassigned`: every `break` comes after the body gives it a
// new value. `value`: a value with nothing to drop is a "value", not
// a "resource" (#484).
pub fn event_dot_loop_exit_kept(my_name: &String, reassigned: bool, value: bool) -> String {
    let name = styled_name_html(my_name);
    let brk = styled_name_html(&"break".to_owned());
    let what = if value { "value" } else { "resource" };
    if reassigned {
        format!(
            "{0} owns a {2} here: the code after the loop is reached only through a {1}, and every {1} comes after {0} gets a new {2}",
            name, brk, what
        )
    } else {
        format!(
            "{0} owns a {2} here: the code after the loop is reached only through a {1}, and {0} owns one at every {1}",
            name, brk, what
        )
    }
}

// At the start of a loop's body, for a variable that holds its value
// where the loop starts but that the body may leave moved where it
// comes back round (#992): `loop { if n >= k { break; } s = …;
// drop(s); }`. The body is drawn once, for every time round (#853), so
// the first time round s still holds its value here, a later one it
// may not. rustc rejects a use before s gets a new value ("value moved
// here, in previous iteration of loop").
pub fn event_dot_loop_head_maybe_moved(my_name: &String) -> String {
    format!(
        "{0} may have been moved: the loop can come back here after {0} is moved further down, so {0} can't be used until it gets a new value",
        styled_name_html(my_name)
    )
}

// At the end of a `while` / `for` loop, for such a variable: the loop
// can end (its condition false, its iterator done) after a time round
// that left it moved (#992).
pub fn event_dot_loop_round_maybe_moved(my_name: &String) -> String {
    format!(
        "{0} may have been moved: the loop can end after a time round that left {0} moved, so {0} can't be used after the loop",
        styled_name_html(my_name)
    )
}

// A reassignment in a loop body whose start has `s` maybe-moved
// (`event_dot_loop_head_maybe_moved`) with nothing in between that
// reaches it (#992): the first time round s holds its value; a later
// time round it does unless the one before left it moved.
pub fn event_dot_owner_drop_at_reassign_loop_head(my_name: &String, note: &str) -> String {
    let (holds, rest) = split_drop_note(note);
    format!(
        "{0} acquires ownership of a new resource. Its previous resource{1} is dropped{2} here only if \
         {0} still holds it (Rust tracks this with a runtime drop flag): it does the first time round \
         the loop, but not if the previous time round left {0} moved.",
        styled_name_html(my_name), holds, rest
    )
}

// What failed: "the condition" of an `if`, or "the guard on line N"
// of a `match` arm (#247).
fn fail_subject(path: FailPath) -> String {
    match path {
        FailPath::Guard(line) => format!("the guard on line {}", line),
        FailPath::Else | FailPath::NoElse => "the condition".to_owned(),
    }
}

// At the start of the `else` branch of an `if` whose let-chain
// condition moves the variable in one part and can still be false in a
// later one (`if let Some(a) = x && a.len() > 5`, #281): on that path
// the else branch runs after the move (rustc: "borrow of partially
// moved value"). `definite`: every way into the else branch passes the
// move (the parts before it can't fail). `path`: the else branch, the
// path where the body is skipped (no `else`), or a later arm of a
// `match` whose guard moved it (#247). `to`: where the condition
// moves it; `dropped`: those of them (the chain's own bindings) that
// are dropped when the condition fails, with what they hold.
pub fn event_dot_chain_fail_move(
    my_name: &String,
    definite: bool,
    to: &[String],
    dropped: &[String],
    path: FailPath,
    how: &ChainFailHow,
) -> String {
    match how {
        ChainFailHow::AssignNoValue { .. } | ChainFailHow::MoveThenAssign { .. } =>
            return chain_fail_new_value(my_name, path, how),
        ChainFailHow::AssignThenMove { .. } => {
            let mut out = chain_fail_assign_then_move(my_name, path, how);
            out.push_str(&cond_fail_dropped(dropped, if matches!(path, FailPath::Guard(_)) { "guard" } else { "condition" }));
            return out;
        }
        _ => {}
    }
    let (name, moves, act) = cond_move_parts(my_name, to, how, &fail_subject(path));
    let if_kw = styled_name_html(&"if".to_owned());
    let mut out = match (definite, path) {
        // A guard (#247): never definite, since a later arm can also be
        // reached when this arm's pattern doesn't match.
        (_, FailPath::Guard(_)) => format!(
            "{0} may have been moved: when {1} and then turns out false, matching goes on with the \
             later arms after that {2}, so {0} can't be used here (the borrow checker counts every \
             later arm as one that may run after a failed guard)",
            name, moves, act),
        (false, FailPath::Else) => format!(
            "{0} may have been moved: when {1} and then turns out false, the else branch runs \
             after that {2}, so {0} can't be used here",
            name, moves, act),
        (true, FailPath::Else) => format!(
            "{0} was moved: {1} before it can turn out false, so the else branch runs after that \
             {2}, and {0} can't be used here",
            name, moves, act),
        (false, FailPath::NoElse) => format!(
            "{0} may have been moved: when {1} and then turns out false, the body is skipped but \
             the {2} has happened, so {0} can't be used after the {3}",
            name, moves, act, if_kw),
        (true, FailPath::NoElse) => format!(
            "{0} was moved: {1} before it can turn out false, so the {2} has happened even when \
             the body is skipped, and {0} can't be used after the {3}",
            name, moves, act, if_kw),
    };
    let what = if matches!(path, FailPath::Guard(_)) { "guard" } else { "condition" };
    // That temporary is dropped when the condition fails, with what the
    // pattern didn't take out of it: all of it if the pattern didn't
    // match (#949).
    if let ChainFailHow::Literal(tuple) = how {
        let tmp = temporary_tuple(tuple);
        if to.is_empty() {
            return format!("{out}. When the {what} fails, {tmp} is dropped, and with it {name}'s resource.");
        }
        out.push_str(&format!(". If the pattern doesn't match, {tmp} is dropped, and with it {name}'s resource"));
        if dropped.is_empty() {
            out.push('.');
            return out;
        }
        let plural = dropped.len() > 1;
        let fmt: Vec<String> = dropped.iter().map(|n| styled_name_html(n)).collect();
        let names = match fmt.len() {
            1 => fmt.concat(),
            n => format!("{} and {}", fmt[..n - 1].join(", "), fmt[n - 1]),
        };
        out.push_str(&format!(
            "; if it matches and the {} fails after that, {} {} out of scope and {} {} dropped.",
            what,
            names,
            if plural { "go" } else { "goes" },
            if plural { "their resources" } else { "its resource" },
            if plural { "are" } else { "is" },
        ));
        return out;
    }
    out.push_str(&cond_fail_dropped(dropped, what));
    out
}

// "a", "a and b", "a, b and c", each styled.
fn names_and(names: &[String]) -> String {
    let fmt: Vec<String> = names.iter().map(|n| styled_name_html(n)).collect();
    match fmt.len() {
        0 | 1 => fmt.concat(),
        n => format!("{} and {}", fmt[..n - 1].join(", "), fmt[n - 1]),
    }
}

// At the start of the `else` branch (`path`: or the path that skips the
// body, or a later arm after a failed guard) of an `if` whose let-chain
// condition gives the variable a new value before a part that can still
// fail, when on that path it may hold the new value or nothing (#603):
// it held none before the condition (`drop(z); if let Some(a) = y && {
// z = a; … }`), or the condition moved it first (`take(z) && { z = a;
// true }`). rustc rejects a use there (E0382), and a drop flag decides
// what is dropped at the scope end.
fn chain_fail_new_value(my_name: &String, path: FailPath, how: &ChainFailHow) -> String {
    let name = styled_name_html(my_name);
    let (subj, short) = match path {
        FailPath::Guard(_) => (fail_subject(path), "the guard"),
        _ => (fail_subject(path), "the condition"),
    };
    let (before, act, old) = match how {
        ChainFailHow::AssignNoValue { before } => {
            let before = match before {
                NoValueBefore::Moved(Some(line)) => format!("{0} held no value before {1} (it was moved on line {2}); ", name, subj, line),
                NoValueBefore::Moved(None) => format!("{0} held no value before {1}; ", name, subj),
                NoValueBefore::MovedByCondition(_) => format!("{0} held no value once the first part of {1} moved it; ", name, subj),
                NoValueBefore::Maybe => format!("{0} may have held no value before {1} (a branch above may have moved it); ", name, subj),
            };
            (before, format!("{} assigns to {}", short, name),
             matches!(how, ChainFailHow::AssignNoValue { before: NoValueBefore::Maybe }))
        }
        ChainFailHow::MoveThenAssign { old, to } => {
            let moves = if to.is_empty() { format!("{} moves {}", subj, name) } else { format!("{} moves {} to {}", subj, name, names_and(to)) };
            (String::new(), format!("{}, then assigns to {}", moves, name), *old)
        }
        _ => unreachable!("chain_fail_new_value of a move"),
    };
    let holds = if old {
        format!("{} holds its previous value, the new one, or nothing", name)
    } else {
        format!("{} holds the new value or nothing", name)
    };
    let runs = match path {
        FailPath::Guard(_) => "matching goes on with the later arms after that assignment, so here".to_owned(),
        FailPath::Else => "the else branch runs after that assignment, so here".to_owned(),
        FailPath::NoElse => format!(
            "the body is skipped but the assignment has happened, so after the {}", styled_name_html(&"if".to_owned())),
    };
    let and = if before.is_empty() { "and" } else { "and then" };
    let why = if matches!(path, FailPath::Guard(_)) {
        " (the borrow checker counts every later arm as one that may run after a failed guard)"
    } else {
        ""
    };
    format!(
        "{0} may have been assigned a new value: {1}when {2} {3} turns out false, {4} {5}, and can't be used until it's assigned again{6}",
        name, before, act, and, runs, holds, why
    )
}

// After a let-chain condition (or a match guard) that may have given
// the variable a new value when it held none, or after moving it (#603,
// `ChainFailHow::may_hold_new`): it holds that value or nothing, so the
// drop flag decides "only if it still holds it", not whether something
// above moved it (it was moved, and may have been given a new value
// since). Its scope end:
pub fn event_dot_go_out_of_scope_chain_new_value(my_name: &String, note: &str) -> String {
    let (holds, rest) = split_drop_note(note);
    format!(
        "{0} goes out of scope. Its resource{1} is dropped{2} here only if {0} still holds it \
         (Rust tracks this with a runtime drop flag).",
        styled_name_html(my_name), holds, rest
    )
}

// Its next assignment:
pub fn event_dot_owner_drop_at_reassign_chain_new_value(my_name: &String, note: &str) -> String {
    let (holds, rest) = split_drop_note(note);
    format!(
        "{0} acquires ownership of a new resource. Its previous resource{1} is dropped{2} here only if \
         {0} still holds it (Rust tracks this with a runtime drop flag).",
        styled_name_html(my_name), holds, rest
    )
}

// The join of the conditional whose condition it was (`kw`: "if" or
// "match"). `value`: what it holds has nothing to drop. `mutable`: it
// can be assigned again. `reassigned`: it is, before its scope ends.
pub fn event_dot_branch_merge_chain_new_value(my_name: &String, kw: &str, value: bool, mutable: bool, reassigned: bool, note: &str) -> String {
    let name = styled_name_html(my_name);
    let kw = styled_name_html(&kw.to_owned());
    let mut out = format!(
        "{0} may have no value after the {1}, so it can't be used after the {1}{2}.",
        name, kw, if mutable { " (until it's assigned again)" } else { "" });
    if !value {
        let (holds, rest) = split_drop_note(note);
        out.push_str(&format!(
            " If it has one, its resource{0} is dropped{1} when {2} {3}.",
            holds, rest, name, if reassigned { "is assigned again" } else { "goes out of scope" }));
    }
    out
}

// `event_dot_chain_fail_move` when the condition gives the variable a
// new value and then moves it (#603): `{ z = a; true } && take(z)`. On
// the failing path it holds its previous value (an earlier part
// failed) or nothing (a part after the move failed): the maybe-moved
// state, in one sentence for both.
fn chain_fail_assign_then_move(my_name: &String, path: FailPath, how: &ChainFailHow) -> String {
    let ChainFailHow::AssignThenMove { place, drops_old, to } = how else {
        unreachable!("chain_fail_assign_then_move of another effect");
    };
    let name = styled_name_html(my_name);
    let target = styled_name_html(place.as_ref().unwrap_or(my_name));
    let cond = fail_subject(path);
    let dropping = if *drops_old { " (dropping its previous one)" } else { "" };
    let moves = if to.is_empty() { format!("moves {}", name) } else { format!("moves {} to {}", name, names_and(to)) };
    let when = format!("when {} assigns {} a new value{}, then {} and turns out false", cond, target, dropping, moves);
    match path {
        FailPath::Guard(_) => format!(
            "{0} may have been moved: {1}, matching goes on with the later arms after that move, so {0} \
             can't be used here (the borrow checker counts every later arm as one that may run after a \
             failed guard)",
            name, when),
        FailPath::Else => format!(
            "{0} may have been moved: {1}, the else branch runs after that move, so {0} can't be used here",
            name, when),
        FailPath::NoElse => format!(
            "{0} may have been moved: {1}, the body is skipped but the move has happened, so {0} can't be \
             used after the {2}",
            name, when, styled_name_html(&"if".to_owned())),
    }
}

// Where a `while` loop ends, when its condition moves the variable
// before it can turn out false (#968): the loop ends after that move.
// `definite`: every way the condition can turn out false passes the
// move. `to`, `dropped`, `how`: as for `event_dot_chain_fail_move`.
pub fn event_dot_while_cond_move(
    my_name: &String,
    definite: bool,
    to: &[String],
    dropped: &[String],
    how: &ChainFailHow,
) -> String {
    let (name, moves, act) = cond_move_parts(my_name, to, how, "the condition");
    let while_kw = styled_name_html(&"while".to_owned());
    let mut out = if definite {
        format!(
            "{0} was moved before the loop ended: {1} before it can turn out false, so the {3} \
             loop ends after that {2}, and {0} can't be used after the loop",
            name, moves, act, while_kw)
    } else {
        format!(
            "{0} may have been moved: when {1} and then turns out false, the {3} loop ends after \
             that {2}, so {0} can't be used after the loop",
            name, moves, act, while_kw)
    };
    out.push_str(&cond_fail_dropped(dropped, "condition"));
    out
}

// What a condition does to the variable (`event_dot_chain_fail_move`,
// `event_dot_while_cond_move`): the name of what it moves (the
// variable, or for `take(*b)` the place `*b`, #579), how it says so,
// and what to call that act. `cond`: what moves it, "the condition" or
// "the guard on line N" (`fail_subject`, #247).
fn cond_move_parts(my_name: &String, to: &[String], how: &ChainFailHow, cond: &str) -> (String, String, &'static str) {
    fn join(names: &[String]) -> String {
        let fmt: Vec<String> = names.iter().map(|n| styled_name_html(n)).collect();
        match fmt.len() {
            0 | 1 => fmt.concat(),
            n => format!("{} and {}", fmt[..n - 1].join(", "), fmt[n - 1]),
        }
    }
    let name = match how {
        ChainFailHow::Out(place) => styled_name_html(place),
        _ => styled_name_html(my_name),
    };
    // A call of an FnOnce closure moves it into the call (#579).
    let (moves, act) = match how {
        ChainFailHow::Call => (format!(
            "{} calls {} (it can only be called once, so the call moves it)", cond, name), "call"),
        // Into the temporary of a literal scrutinee that the pattern
        // may not match (#949).
        ChainFailHow::Literal(tuple) => (format!("{} moves {} into {}", cond, name, temporary_tuple(tuple)), "move"),
        _ if to.is_empty() => (format!("{} moves {}", cond, name), "move"),
        _ => (format!("{} moves {} to {}", cond, name, join(to)), "move"),
    };
    (name, moves, act)
}

// The condition's bindings that took the moved value and are dropped
// when it fails (#281). `what`: "condition", or "guard" (#247).
fn cond_fail_dropped(dropped: &[String], what: &str) -> String {
    if dropped.is_empty() {
        return String::new();
    }
    let plural = dropped.len() > 1;
    let fmt: Vec<String> = dropped.iter().map(|n| styled_name_html(n)).collect();
    let names = match fmt.len() {
        1 => fmt.concat(),
        n => format!("{} and {}", fmt[..n - 1].join(", "), fmt[n - 1]),
    };
    format!(
        ". When the {} fails after that move, {} {} out of scope and {} {} dropped.",
        what,
        names,
        if plural { "go" } else { "goes" },
        if plural { "their resources" } else { "its resource" },
        if plural { "are" } else { "is" },
    )
}

// "t.0 or t.1", "t.0, t.1 or t.2": the places an or-pattern binds a
// name from (#533).
fn or_list(sources: &[String]) -> String {
    let names: Vec<String> = sources.iter().map(|n| styled_name_html(n)).collect();
    match names.as_slice() {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} or {}", init.join(", "), last),
    }
}

// An or-pattern arm (`(x, _) | (_, x) =>`) binds `into` from a different
// place in each alternative, and `my_name` is one of them (#533). Which
// alternative matches is decided at run time, so this place's resource
// is moved into `into` only on some runs; the rest of the time it stays
// here, and the drop flag decides whether it's dropped at scope end.
// `tuple`: `my_name` is a variable a tuple-literal scrutinee (`match (a,
// b)`) moved into its temporary tuple (#327). Then it's that value in
// the tuple which may move into `into`, and what it doesn't take stays
// in the tuple, which the tuple's own drop dot says is dropped: no drop
// flag on `my_name`, which owns nothing either way.
// `part_of`: `Some((x, p))` when `my_name` is a field of the place `p`
// that may move as a whole into the struct or tuple `x`, and `into` is
// `x`'s matching field (#565). `value`: what's moved has nothing to
// drop, so there's no drop flag either (#484).
pub fn event_dot_or_alt_move(my_name: &String, into: &String, sources: &[String], tuple: Option<&str>, part_of: Option<&(String, String)>, value: bool) -> String {
    let me = styled_name_html(my_name);
    let (bound, bound_from) = match part_of {
        Some((x, p)) => (styled_name_html(x), styled_name_html(p)),
        None => (styled_name_html(into), me.clone()),
    };
    if let Some(tuple) = tuple {
        return format!(
            "{1} is bound from {2}, whichever alternative of the pattern matches. If it's the alternative \
             that binds {1} from {3}, {0}'s {4} is moved out of {5} into {6} here; otherwise it stays \
             in the {7}",
            me, bound, or_list(sources), bound_from, held(value), temporary_tuple(&tuple.to_owned()), styled_name_html(into), tuple_noun(tuple)
        );
    }
    let otherwise = if value {
        format!("otherwise {} still holds it", me)
    } else {
        format!("otherwise {} still owns it (Rust tracks this with a runtime drop flag)", me)
    };
    format!(
        "{1} is bound from {2}, whichever alternative of the pattern matches. If it's the alternative \
         that binds {1} from {3}, {0}'s {4} is moved into {5} here; {6}.",
        me, bound, or_list(sources), bound_from, held(value), styled_name_html(into), otherwise
    )
}

// An or-pattern arm (`(ref x, _) | (_, ref x) =>`) binds the reference
// `into` from a different place in each alternative, and `my_name` is
// one of them (#565). Only the alternative that matches borrows its
// place, but which one that is is decided at run time, so the borrow
// checker treats every one of them as borrowed while `into` is live.
pub fn event_dot_or_alt_lend(my_name: &String, into: &String, sources: &[String], mutable: bool, holds_value: bool) -> String {
    let me = styled_name_html(my_name);
    let x = styled_name_html(into);
    let while_live = if mutable {
        format!("{} can't be used while {}'s borrow is live", me, x)
    } else {
        format!("{} is read-only while {}'s borrow is live", me, x)
    };
    format!(
        "{1} borrows {2}, whichever alternative of the pattern matches. If it's the alternative that binds \
         {1} from {0}, {0}'s {3} is {4} borrowed by {1} here. Which alternative matches is decided at run \
         time, so the borrow checker treats {0} as borrowed either way: {5}.",
        me, x, or_list(sources), held(holds_value), if mutable { "mutably" } else { "immutably" }, while_live
    )
}

// The end of the borrow `event_dot_or_alt_lend` started (#565).
pub fn event_dot_or_alt_reacquire(my_name: &String, into: &String, mutable: bool, holds_value: bool) -> String {
    format!(
        "{1}'s borrow ends here, so {0}'s {2} is no longer {3} borrowed (the borrow checker counted it as \
         borrowed whichever alternative of the pattern matched)",
        styled_name_html(my_name), styled_name_html(into), held(holds_value), if mutable { "mutably" } else { "immutably" }
    )
}

// The reference's own dot where `event_dot_or_alt_lend`'s borrow starts
// (#565).
pub fn event_dot_or_alt_borrow(my_name: &String, sources: &[String], mutable: bool) -> String {
    format!(
        "{0} {1} borrows {2}, whichever alternative of the pattern matches",
        styled_name_html(my_name), if mutable { "mutably" } else { "immutably" }, or_list(sources)
    )
}

// Line segment of a place while the borrow `event_dot_or_alt_lend`
// started is live (#565).
pub fn state_or_alt_lent(my_name: &String, into: &String, mutable: bool, holds_value: bool) -> String {
    let me = styled_name_html(my_name);
    let x = styled_name_html(into);
    if mutable {
        format!(
            "{0}'s {2} is mutably borrowed by {1} if the alternative that binds {1} from {0} matched; \
             either way, {0} can't be used until {1}'s borrow ends",
            me, x, held(holds_value)
        )
    } else {
        format!(
            "{0}'s {2} is immutably borrowed by {1} if the alternative that binds {1} from {0} matched; \
             either way, {0} is read-only until {1}'s borrow ends",
            me, x, held(holds_value)
        )
    }
}

// ── A new reference assigned by a failing condition (#603) ─────────
//
// `if let Some(_n) = y && { r = &s; r.len() > 3 } { … } else { … }`:
// when the condition assigns `&s` to `r` and then turns out false, the
// else branch (or the path that skips the body, or a later arm after a
// failed guard) runs after that assignment; when it fails before it,
// `r` still points to `t`. Which one happened is decided at run time, so
// the borrow checker treats both `s` and `t` as borrowed while `r` is
// live there (an `ExternalEvent::OrAltBorrow` with `chain`). `sources`
// is [s, t]: the new lender, then the old one.

// Where `r` may point on the failing path: "to s or to t", "into v or
// to t" (a lender reached through a call's result, which points into
// it, #1068), "to s, into a or into b". `sources`: the new reference's
// lenders, then the old one's (see `ChainRef`).
fn chain_ref_alts(sources: &[String], chain: &ChainRef) -> String {
    let mut items: Vec<String> = sources.iter().enumerate().map(|(i, n)| {
        let into = if i < chain.new { chain.new_via_call } else { chain.old_via_call };
        format!("{} {}", if into { "into" } else { "to" }, styled_name_html(n))
    }).collect();
    // The old reference's data with no column (#1068).
    let what = match &chain.old_static {
        Some(StaticLender::Literal { noun }) => Some(format!("a {}", noun)),
        Some(StaticLender::Static(n)) => Some(format!("the static {}", styled_name_html(n))),
        Some(StaticLender::PointeeOfStatic(n)) => Some(format!("what the static {} points to", styled_name_html(n))),
        Some(StaticLender::PointeeOfConst(n)) => Some(format!("what the constant {} points to", styled_name_html(n))),
        Some(StaticLender::PromotedConst(n)) => Some(format!("the constant {}", styled_name_html(n))),
        _ => None,
    };
    items.extend(what.map(|w| format!("to {}", w)));
    match items.as_slice() {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} or {}", init.join(", "), last),
    }
}

// The reference's dot where that starts.
pub fn event_dot_chain_ref_borrow(my_name: &String, sources: &[String], chain: &ChainRef) -> String {
    let r = styled_name_html(my_name);
    let alts = chain_ref_alts(sources, chain);
    let assigns = format!("{} assigns {} to {}", fail_subject(chain.path), styled_name_html(&chain.rhs), r);
    match chain.path {
        FailPath::Guard(_) => format!(
            "{0} may have been assigned a new reference: when {1} and then turns out false, matching goes on \
             with the later arms after that assignment, so here {0} points {2} (the borrow \
             checker counts every later arm as one that may run after a failed guard)",
            r, assigns, alts),
        FailPath::Else => format!(
            "{0} may have been assigned a new reference: when {1} and then turns out false, the else branch \
             runs after that assignment, so here {0} points {2}",
            r, assigns, alts),
        FailPath::NoElse => format!(
            "{0} may have been assigned a new reference: when {1} and then turns out false, the body is \
             skipped but the assignment has happened, so after the {3} {0} points {2}",
            r, assigns, alts, styled_name_html(&"if".to_owned())),
    }
}

// The dot of `s` or `t` (`my_name`) where that starts, adapted from
// `event_dot_or_alt_lend`.
pub fn event_dot_chain_ref_lend(my_name: &String, into: &String, sources: &[String], mutable: bool, holds_value: bool, chain: &ChainRef) -> String {
    let me = styled_name_html(my_name);
    let r = styled_name_html(into);
    let alts = chain_ref_alts(sources, chain);
    let rhs = styled_name_html(&chain.rhs);
    let cond = fail_subject(chain.path);
    let how = if mutable { "mutably" } else { "immutably" };
    let is_new = sources.iter().position(|n| n == my_name).is_some_and(|i| i < chain.new);
    let when = if is_new {
        format!("If {} assigned {} to {} before turning out false, {}'s {} is {} borrowed by {} here.",
            cond, rhs, r, me, held(holds_value), how, r)
    } else if matches!(chain.path, FailPath::Guard(_)) {
        format!("If {} turned out false before assigning {} to {}, or didn't run (its arm's pattern didn't match), \
                 {}'s {} is still {} borrowed by {} here.", cond, rhs, r, me, held(holds_value), how, r)
    } else {
        format!("If {} turned out false before assigning {} to {}, {}'s {} is still {} borrowed by {} here.",
            cond, rhs, r, me, held(holds_value), how, r)
    };
    let while_live = if mutable {
        format!("{} can't be used while {}'s borrow is live", me, r)
    } else {
        format!("{} is read-only while {}'s borrow is live", me, r)
    };
    format!(
        "{0} points {1} here. {2} Which one {0} points to is decided at run time, so the borrow \
         checker treats {3} as borrowed either way: {4}.",
        r, alts, when, me, while_live)
}

// The end of that borrow, on `s`'s or `t`'s column.
pub fn event_dot_chain_ref_reacquire(my_name: &String, into: &String, mutable: bool, holds_value: bool, chain: &ChainRef) -> String {
    format!(
        "{1}'s borrow ends here, so {0}'s {2} is no longer {3} borrowed (the borrow checker counted it as \
         borrowed whether or not {4} had assigned {5} to {1})",
        styled_name_html(my_name), styled_name_html(into), held(holds_value),
        if mutable { "mutably" } else { "immutably" }, fail_subject(chain.path), styled_name_html(&chain.rhs))
}

// A segment of `s` or `t` while that borrow is live.
pub fn state_chain_ref_lent(my_name: &String, into: &String, sources: &[String], chain: &ChainRef, mutable: bool, holds_value: bool) -> String {
    let me = styled_name_html(my_name);
    let r = styled_name_html(into);
    format!(
        "{0}'s {1} may be {2} borrowed by {3} ({3} points {4}); either way, {0} {5} until {3}'s borrow ends",
        me, held(holds_value), if mutable { "mutably" } else { "immutably" }, r,
        chain_ref_alts(sources, chain),
        if mutable { "can't be used" } else { "is read-only" })
}

// At the start of the `else` branch (`path`: or the path that skips the
// body, or a later arm after a failed guard, #248) of an `if` whose
// let-chain condition lends the variable in a part
// before one that can still be false (#688): `if let Some(a) = &x && let
// Some(b) = y.take()` lends `y` to `take`, and `x` to `a`. The borrows
// have ended by the time the else branch runs: a call's when it returns,
// a binding's when the failed condition ends its scope. `definite`: as
// for event_dot_chain_fail_move. `is_ref`: the variable is a reference,
// and it's the resource it points to that is lent (a value when
// `pointee_value`, #1163).
pub fn event_dot_chain_fail_use(
    my_name: &String,
    uses: &[ChainUse],
    definite: bool,
    path: FailPath,
    is_ref: bool,
    pointee_value: bool,
) -> String {
    fn join(names: &[String]) -> String {
        let fmt: Vec<String> = names.iter().map(|n| styled_name_html(n)).collect();
        match fmt.len() {
            0 | 1 => fmt.concat(),
            n => format!("{} and {}", fmt[..n - 1].join(", "), fmt[n - 1]),
        }
    }
    let name = styled_name_html(my_name);
    let (what, obj) = if is_ref {
        (format!("the {} {} points to", held(pointee_value), name), "it".to_owned())
    } else {
        (name.clone(), name.clone())
    };
    let kind = if uses.iter().all(|u| u.mutable) {
        "mutably "
    } else if uses.iter().all(|u| !u.mutable) {
        "immutably "
    } else {
        ""
    };
    let named: Vec<String> = uses.iter().filter(|u| !u.to.is_empty()).map(|u| u.to.clone()).collect();
    let cond = fail_subject(path);
    let lends = if named.is_empty() {
        format!("{} borrows {}", cond, obj)
    } else {
        format!("{} lends {} to {}", cond, obj, join(&named))
    };
    let bound: Vec<String> = uses.iter().filter(|u| u.binding && !u.to.is_empty()).map(|u| u.to.clone()).collect();
    let gos = match bound.len() {
        0 => String::new(),
        1 => format!("{} goes out of scope, so ", join(&bound)),
        _ => format!("{} go out of scope, so ", join(&bound)),
    };
    let ended = if uses.len() > 1 { "those borrows have ended" } else { "that borrow has ended" };
    let then = match path {
        FailPath::NoElse => format!("the body is skipped after {}", ended),
        FailPath::Else => format!("the else branch runs after {}", ended),
        FailPath::Guard(_) => format!("matching goes on with the later arms after {}", ended),
    };
    let mut out = if definite {
        format!("{} was {}borrowed: {} before it can turn out false; when it does, {}{}",
            what, kind, lends, gos, then)
    } else {
        format!("{} may have been {}borrowed: when {} and then turns out false, {}{}",
            what, kind, lends, gos, then)
    };
    // A mutable borrow can have changed it on the way.
    let by: Vec<String> = uses.iter().filter(|u| u.changes && !u.binding && !u.to.is_empty())
        .map(|u| format!("by {}", styled_name_html(&u.to))).collect();
    let through: Vec<String> = uses.iter().filter(|u| u.changes && u.binding && !u.to.is_empty())
        .map(|u| format!("through {}", styled_name_html(&u.to))).collect();
    let agents: Vec<String> = by.into_iter().chain(through).collect();
    if !agents.is_empty() {
        out.push_str(&format!(". By then, {} may have been changed {}.", what, agents.join(" or ")));
    }
    out
}

// At the start of the `else` branch of an `if` whose let-chain
// condition assigns the variable a new value in one part and can still
// be false in a later one (`if let Some(a) = y && { z = a; z > 0 }`,
// #579): the assignment lasts, so on that path the variable holds its
// new value, and on the paths where the condition failed before the
// assignment its old one. `drops_old`: the assignment drops the old
// value (`rc`: what that drop does for an Rc). `path`: the else branch,
// the path where the body is skipped (no `else`), or a later arm of a
// `match` whose guard assigned it (#247).
pub fn event_dot_chain_fail_assign(
    my_name: &String,
    drops_old: bool,
    rc: Option<RcDrop>,
    path: FailPath,
) -> String {
    let name = styled_name_html(my_name);
    let what = if drops_old { "resource" } else { "value" };
    let mut out = if let FailPath::Guard(line) = path {
        format!(
            "{0} may have been assigned a new {1}: when the guard on line {2} assigns to {0} and \
             then turns out false, matching goes on with the later arms after that assignment, so \
             here {0} holds either its previous {1} or the new one.",
            name, what, line)
    } else if path == FailPath::NoElse {
        format!(
            "{0} may have been assigned a new {1}: when the condition assigns to {0} and then \
             turns out false, the body is skipped but the assignment has happened, so after the \
             {2} {0} holds either its previous {1} or the new one.",
            name, what, styled_name_html(&"if".to_owned()))
    } else {
        format!(
            "{0} may have been assigned a new {1}: when the condition assigns to {0} and then \
             turns out false, the else branch runs after that assignment, so here {0} holds \
             either its previous {1} or the new one.",
            name, what)
    };
    if drops_old {
        out.push_str(&format!(
            " When the assignment runs, it drops {}'s previous resource{}.",
            name, rc_drop_note(rc)));
    }
    out
}

// ── Several `break`s that carry a value (#561) ─────────────────────
//
// `let r = loop { if c { break a; } else { break b; } };`: every
// `break` value is paired with `r` where the loop ends, but only the
// `break` that ended the loop ran. `brk` is that `break` as written
// (`break a`), `None` when it is too long to quote.

fn quoted_break(brk: &Option<String>) -> String {
    match brk {
        Some(b) => styled_name_html(b),
        None => "the break that carries it".to_owned(),
    }
}

/// What ended: "loop" or "block" (a labeled block, #1134).
fn alt_noun(alt: &crate::svg_generator::data::BreakAlt) -> &'static str {
    match alt.end {
        crate::svg_generator::data::AltEnd::Loop => "loop",
        _ => "block",
    }
}

/// How it ended: the `break` as written, or a labeled block's last
/// expression (#1134).
fn alt_how(alt: &crate::svg_generator::data::BreakAlt) -> String {
    match (alt.end, &alt.brk) {
        (crate::svg_generator::data::AltEnd::BlockTail, Some(t)) => format!("its last expression, {}", styled_name_html(t)),
        (crate::svg_generator::data::AltEnd::BlockTail, None) => "its last expression".to_owned(),
        _ => quoted_break(&alt.brk),
    }
}

/// Appended to the receiver's dot: it got the value of one `break`,
/// whichever ran.
pub fn break_alt_receiver_suffix() -> String {
    format!(" (from whichever {} ended the loop)", styled_name_html(&"break".to_owned()))
}

/// break_alt_receiver_suffix for `alt`'s construct: a labeled block can
/// also end at its last expression (#1134).
pub fn break_alt_receiver_suffix_for(alt: &crate::svg_generator::data::BreakAlt) -> String {
    match alt.end {
        crate::svg_generator::data::AltEnd::Loop => break_alt_receiver_suffix(),
        _ => break_alt_block_receiver_suffix(),
    }
}

fn break_alt_block_receiver_suffix() -> String {
    format!(" (from whichever {} or last expression ended the block)", styled_name_html(&"break".to_owned()))
}

/// Appended to the source's dot and to the arrow: the copy or move
/// happened only if this `break` ended the loop.
pub fn break_alt_if_suffix(alt: &crate::svg_generator::data::BreakAlt) -> String {
    format!(", if the {} ended with {}", alt_noun(alt), alt_how(alt))
}

/// The lender's dot when a `break` borrows from it for the receiver
/// (`break &e`, or `break rb` reborrowing a `&mut`, #657): the borrow
/// was made only if this `break` ended the loop. The qualifier goes on
/// the borrow, before what it means for the lender ("; rb can't be
/// used while the borrow is live"): rustc rejects that use while the
/// receiver is live whichever `break` ran.
pub fn with_break_alt_lend(title: &str, alt: &crate::svg_generator::data::BreakAlt) -> String {
    let suffix = break_alt_if_suffix(alt);
    // The clauses' "; ", not the end of an escaped entity (`&gt; `)
    // in a styled name.
    let is_entity = |i: usize| title[..i].rfind('&')
        .is_some_and(|a| title[a + 1..i].chars().all(|c| c.is_ascii_alphanumeric() || c == '#'));
    match title.match_indices("; ").map(|(i, _)| i).find(|&i| !is_entity(i)) {
        Some(i) => format!("{}{suffix}{}", &title[..i], &title[i..]),
        None => format!("{title}{suffix}"),
    }
}

/// The source's dot when a `break` moves it into the receiver (see
/// BreakAlt for `alt`). `what`: "resource", "value", or "reference" (a
/// `&mut` moved), what moves out of it. A variable declared inside the
/// loop goes out of scope with it anyway, so it is only said where it
/// went.
pub fn event_dot_break_alt_move(my_name: &String, to: &String, alt: &crate::svg_generator::data::BreakAlt, what: &str) -> String {
    let name = styled_name_html(my_name);
    let to = styled_name_html(to);
    let (noun, how) = (alt_noun(alt), alt_how(alt));
    let moved = if what == "reference" {
        format!("the reference in {0}", name)
    } else {
        format!("{0}'s {1}", name, what)
    };
    if alt.in_loop {
        format!("{0} is moved to {1} if the {noun} ended with {2}", moved, to, how)
    } else {
        format!(
            "{0} may have been moved: the {noun} can end with {1}, which moves {2} to {3}, so {0} can't be used after the {noun}",
            name, how, moved, to
        )
    }
}

// The same when the loop is what the function returns (#1171): the value
// goes to the caller, and nothing comes after the loop.
pub fn event_dot_break_alt_move_to_caller(my_name: &String, alt: &crate::svg_generator::data::BreakAlt, what: &str) -> String {
    let name = styled_name_html(my_name);
    let moved = if what == "reference" {
        format!("the reference in {0}", name)
    } else {
        format!("{0}'s {1}", name, what)
    };
    // (Or a labeled block, #1288.)
    format!("{0} is moved to the caller if the {1} ended with {2}", moved, alt_noun(alt), alt_how(alt))
}

// `f()` of a closure rustc inferred to be `FnOnce` (#271): `f` is
// moved into the call, and each value it captured by value is dropped
// or moved out of it inside the call. `fates` comes from the closure
// body's MIR (see closure_capture_fates).
pub fn event_dot_closure_call_once(
    my_name: &String,
    fates: &[crate::svg_generator::data::CapturedFate],
) -> String {
    use crate::svg_generator::data::CapturedEnd;
    let f = styled_name_html(my_name);
    let mut msg = format!(
        "{0} is moved into the call (an FnOnce closure can only be called once)", f);
    for fate in fates {
        let s = styled_name_html(&fate.name);
        let tail = match fate.end {
            CapturedEnd::Dropped if fate.needs_drop =>
                "is dropped during the call",
            CapturedEnd::Dropped =>
                "goes out of scope at the end of the call (it has nothing to drop)",
            CapturedEnd::Returned =>
                "is moved out of the closure as the call's result",
            CapturedEnd::Moved =>
                "is moved on or dropped by the call",
        };
        msg.push_str(&format!(". {0}, which {1} captured, {2}", s, f, tail));
    }
    msg
}

// Scope end of a closure that was called (and so moved) on some paths
// of a conditional and not on others: whether its captured resources
// are dropped here is decided at runtime by a drop flag (#179, #271).
// `by_cond`: a let-chain's condition may have moved it (into a call,
// #281); that's not a call of the closure.
/// `called` / `moved`: what the branches did with the closure, an
/// `FnOnce` call (#271) or a move (`v.push(f)`, `let g = f;`, #1158).
pub fn event_dot_closure_go_out_of_scope_maybe_moved(my_name: &String, by_cond: bool, count: usize, called: bool, moved: bool) -> String {
    let what = match (called, moved) {
        (true, true) => "called or moved",
        (true, false) => "called",
        _ => "moved",
    };
    let cond = if by_cond { format!("nothing above {} it", what) } else { format!("no branch above {} it", what) };
    format!(
        "{0} goes out of scope. Its captured {1} dropped here only if {2} \
         (Rust tracks this with a runtime drop flag).",
        styled_name_html(my_name),
        if count == 1 { "resource is" } else { "resources are" },
        cond
    )
}


/* Arrow titles, and the other tooltip prose the renderers used to build
 * inline. The renderers (`data.rs`, `timeline_panel.rs`) choose
 * geometry; the words live here, so every message can be checked
 * against Rust's semantics in one file. */

use crate::svg_generator::data::{DeepPlace, ExternalEvent, Function, LoopHead, ImplicitRef, ResourceTy, StoredIn, holds_copy_value, rc_call_of, returned_closure};

// set style for code string
static SPAN_BEGIN : &'static str = "&lt;span style=&quot;font-family: 'Source Code Pro', Consolas, 'Ubuntu Mono', Menlo, 'DejaVu Sans Mono', monospace, monospace !important;&quot;&gt;";
static SPAN_END : &'static str = "&lt;/span&gt;";

/// A name in code font for a tooltip, escaped (see
/// `escape_tooltip_name`).
pub fn styled_name_html(name: &str) -> String {
    SPAN_BEGIN.to_string() + &escape_tooltip_name(name) + SPAN_END
}

/// Whether `r` is a function (a call's result comes from it).
fn is_fn_rap(r: &ResourceTy) -> bool {
    r.extract_rap().is_some_and(|rap| rap.is_fn())
}

pub fn arrow_title(e: &ExternalEvent) -> String {
    match e {
        ExternalEvent::Bind{ .. } => {
            String::from("Bind")
        },
        ExternalEvent::Copy{ is_partial, from, to, closure_value, .. } => {
            if *is_partial { String::from("Partial copy") }
            // `let g = keep(|| s.len());`: the call returns the closure
            // (#686); nothing is captured from `keep`.
            else if to.is_closure() && is_fn_rap(from) { String::from("Closure returned") }
            else if to.is_closure() && !*closure_value { String::from("Closure capture (copy)") }
            else { String::from("Copy") }
        },
        // Move/StaticBorrow/MutableBorrow into a closure binding are
        // closure captures — the renderer's `from … to …` template
        // (in render_arrow, via `arrow_full_title`) reads e.g. "Closure capture (move)" as
        // "Closure capture (move) from s to f", which scans more
        // clearly than a bare "Move" arrow into a closure.
        ExternalEvent::Move{ is_partial, from, to, closure_value, .. } => {
            if *is_partial { String::from("Partial move") }
            else if to.is_closure() && is_fn_rap(from) { String::from("Closure returned") }
            else if to.is_closure() && !*closure_value { String::from("Closure capture (move)") }
            else { String::from("Move") }
        },
        ExternalEvent::StaticBorrow{ is_partial, is_reborrow, to, via, .. } => {
            if *is_partial { String::from("Partial immutable borrow") }
            // A closure capturing `*r` through a reference r (#277).
            // A closure a call returned keeps the borrow (#686); its
            // suffix says why (`arrow_borrow_via_call_suffix`).
            else if to.is_closure() && *is_reborrow && !returned_closure(via) { String::from("Closure capture (immutable reborrow)") }
            else if to.is_closure() && !returned_closure(via) { String::from("Closure capture (immutable borrow)") }
            else if *is_reborrow { String::from("Immutable reborrow") }
            else { String::from("Immutable borrow") }
        },
        // `&r` borrowed the reference r itself, a value (#925).
        ExternalEvent::StaticDie{ to, of_ref_var, .. } => {
            format!("Return immutably borrowed {}", if *of_ref_var || holds_copy_value(to) { "value" } else { "resource" })
        },
        ExternalEvent::MutableBorrow{ is_partial, is_reborrow, to, via, .. } => {
            if *is_partial { String::from("Partial mutable borrow") }
            else if to.is_closure() && *is_reborrow && !returned_closure(via) { String::from("Closure capture (mutable reborrow)") }
            else if to.is_closure() && !returned_closure(via) { String::from("Closure capture (mutable borrow)") }
            // Reborrow: `let r2 = &mut *r;` with r: `&mut T` borrows
            // *through* r — r can't be used while r2 is live and usable
            // again afterwards. Distinct from `&mut r`, which is a
            // fresh borrow of the variable r itself (r2: `&mut &mut T`).
            else if *is_reborrow { String::from("Mutable reborrow") }
            else { String::from("Mutable borrow") }
        },
        // `&mut r` borrowed the reference r itself, a value (#914).
        ExternalEvent::MutableDie{ to, of_ref_var, .. } => {
           format!("Return mutably borrowed {}", if *of_ref_var || holds_copy_value(to) { "value" } else { "resource" })
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

/// The title of the arrow from a function to the caller (`f` returns a
/// value): `copy` for a Copy-typed value, `named_call` when the callee is
/// a real function rather than a constructor or macro.
pub fn arrow_fn_return_title(copy: bool, named_call: bool, styled_fn: &str) -> String {
    match (copy, named_call) {
        (true, true) => format!("Copy of the value {}() returns to the caller", styled_fn),
        (true, false) => format!("Copy from {} to the caller", styled_fn),
        _ => format!("Move from {} to the caller", styled_fn),
    }
}

/// The full title of an arrow between two columns: `arrow_title`, then
/// where it comes from and goes to, then the special cases (a returned
/// call result, an `Rc` call, a borrow ending, a `break` alternative, a
/// borrow a call result keeps).
pub fn arrow_full_title(external_event: &ExternalEvent, from: &ResourceTy, to: &ResourceTy) -> String {
    let mut title = arrow_title(external_event);
    // complete title
    let styled_from_string = styled_name_html(&from.name());
    title = format!("{} from {}", title, styled_from_string);
    let styled_to_string = styled_name_html(&to.name());
    // A by-value capture of a closure literal passed to a call
    // goes into the closure, which is then passed to the callee.
    let into_closure = matches!(external_event,
        ExternalEvent::Move { into_closure: true, .. } | ExternalEvent::Copy { into_closure: true, .. });
    title = if into_closure {
        format!("{} into the closure passed to {}", title, styled_to_string)
    } else {
        format!("{} to {}", title, styled_to_string)
    };
    // A mutable borrow passed on as a shared reference (`let r2 =
    // id(r);`, `vr.push(r)`, `let o = Some(r);` with `r: &String = &mut
    // s`): what r2 / vr / o gets can only read s, though s stays mutably
    // borrowed while it is held (#1599).
    // A lender reached through a copy, a call, an arm or a reborrow, in a
    // store whose loans aren't all of one kind as written (`vr.push(r)`
    // with `r: &String = &mut s`, `v.push(choose(&mut a, &b))`): its loan
    // kind isn't stated (#1619).
    if let ExternalEvent::MutableBorrow { via: Some(via), is_partial: false, .. } | ExternalEvent::StaticBorrow { via: Some(via), is_partial: false, .. } = external_event {
        if via.uncertain && !to.is_closure() {
            title = format!("Borrow from {} to {}", styled_from_string, styled_to_string);
        }
    }
    // `for w in words`: the arrow into the loop variable moves one
    // element out of the collection, not the collection (#850).
    if let ExternalEvent::Move { loop_head: LoopHead::Collection, .. } = external_event {
        title = format!("Move of each element of {} to {}", styled_from_string, styled_to_string);
    }
    // `for y in ys` with Copy elements: each is copied (#1372).
    if let ExternalEvent::Move { loop_head: LoopHead::CollectionCopy, .. } = external_event {
        title = format!("Copy of each element of {} to {}", styled_from_string, styled_to_string);
    }
    // `for (s, k) in names`: one arrow per binding, for the part of each
    // element it gets (#1390).
    if let ExternalEvent::Move { loop_head: LoopHead::Part { path, copy, .. }, .. } = external_event {
        let what = if *copy { "Copy" } else { "Move" };
        title = format!("{} of {} to {}", what, element_part("each element", path, &styled_from_string), styled_to_string);
    }
    // A Copy-typed value a call returns (`let n = v.len();`): the
    // source is the call's result, not a variable (or anything)
    // named like the callee (#737). Constructors (`Some(5)`) and
    // macros keep the plain label.
    // `if let Some(&x) = v.get(0)`: what x gets is the value the
    // returned reference points to (#874), from a call or a constructor.
    if let (ExternalEvent::Copy { into_closure: false, pointee: Some(part), .. }, ResourceTy::Value(ResourceAccessPoint::Function(f))) = (external_event, from) {
        let what = if *part { "a value inside the value" } else { "the value" };
        title = format!("Copy of {} pointed to by {} to {}", what,
            reference_inside_call(&styled_from_string, &f.name, f.inside.as_deref()), styled_to_string);
    }
    if let (ExternalEvent::Copy { into_closure: false, .. }, ResourceTy::Value(f @ ResourceAccessPoint::Function(_))) = (external_event, from) {
        let callee = f.name();
        let last = callee.rsplit("::").next().unwrap_or(&callee);
        if !last.starts_with(|c: char| c.is_uppercase()) && !callee.ends_with('!') && title.starts_with("Copy from ") {
            // When `to` gets only part of the result (a pattern or
            // `?`), mirror the dot's wording (#733): never claim the
            // call returns the copied value itself.
            let inside = match from { ResourceTy::Value(ResourceAccessPoint::Function(f)) => f.inside.as_deref(), _ => None };
            // `let x = Some(one());`: the value goes into a Some value
            // that x holds (#1183).
            let wrapped = match external_event {
                ExternalEvent::Copy { into_ctor: Some(c), .. } => Some(ctor_value_phrase(c)),
                _ => None,
            };
            title = match (inside, wrapped) {
                (None, Some(w)) => format!("Copy of the value {}() returns into {} held by {}", styled_from_string, w, styled_to_string),
                (None, None) => format!("Copy of the value {}() returns to {}", styled_from_string, styled_to_string),
                (Some(""), _) => format!("Copy to {}", styled_to_string),
                (Some(w @ ("Ok" | "Some")), _) => format!("Copy of the value inside the {} that {}() returns to {}", w, styled_from_string, styled_to_string),
                (Some(w), _) => format!("Copy of the value inside the {} {}() returns to {}", w, styled_from_string, styled_to_string),
            };
        }
    }
    // `Rc::clone(&a)` / `a.clone()` / `Rc::downgrade(&a)` make
    // another owner of the same value; the value isn't moved
    // or copied (#373).
    if matches!(external_event, ExternalEvent::Move { .. }) {
        if let Some(call) = rc_call_of(from) {
            title = arrow_rc_call(&from.name(), &to.name(), call);
        }
    }
    // A borrow "returned" from `s` to `*s`: the lender has no
    // column, so don't describe it as returned to itself.
    // When the visitor knows it borrowed 'static data (a string
    // literal, a static), name that instead.
    if let ExternalEvent::StaticDie { static_lender, .. } | ExternalEvent::MutableDie { static_lender, .. } = external_event {
        if from.is_same_underlying(to) {
            let is_mut = matches!(external_event, ExternalEvent::MutableDie { .. });
            title = match static_lender {
                Some(lender) => arrow_borrow_of_static_ends(&from.name(), is_mut, lender),
                None => arrow_borrow_ends_lender_not_drawn(&from.name(), is_mut),
            };
        }
    }
    if let ExternalEvent::Move { break_alt: Some(alt), .. }
         | ExternalEvent::Copy { break_alt: Some(alt), .. }
         | ExternalEvent::StaticBorrow { break_alt: Some(alt), .. }
         | ExternalEvent::MutableBorrow { break_alt: Some(alt), .. } = external_event {
        title.push_str(&break_alt_if_suffix(alt));
    }
    if let ExternalEvent::StaticBorrow { via: Some(via), .. }
         | ExternalEvent::MutableBorrow { via: Some(via), .. } = external_event {
        title.push_str(&arrow_borrow_via_call_suffix(&to.name(), &via.func, &via.lenders, via.also_unnamed, &via.tie, via.carrier, &via.passed_on));
    }
    title
}

/// The one arrow drawn for a reference field's borrows from, or borrow
/// ends to, several lenders on one row (#1354, `FieldLenders`): the
/// field holds one reference, which may point into any of `lenders`,
/// and the borrow checker keeps all of them borrowed while it is in use.
/// "Immutable borrow from s1 or s2 to d.a: …" / "Return immutably
/// borrowed resource from d.a to s1 or s2 (d.a may point into either;
/// its borrow of both ends here)".
pub fn arrow_field_lenders_title(external_event: &ExternalEvent, from: &ResourceTy, to: &ResourceTy, lenders: &[String], end: bool) -> String {
    let title = arrow_full_title(external_event, from, to);
    let (either, all) = if lenders.len() == 2 { ("either", "both") } else { ("any of them", "all of them") };
    if end {
        let field = styled_name_html(&from.name());
        let title = title.replacen(&format!(" to {}", styled_name_html(&to.name())), &format!(" to {}", or_list(lenders)), 1);
        format!("{} ({} may point into {}; its borrow of {} ends here)", title, field, either, all)
    } else {
        let field = styled_name_html(&to.name());
        let title = title.replacen(&format!(" from {}", styled_name_html(&from.name())), &format!(" from {}", or_list(lenders)), 1);
        // A call's result says why already (`arrow_borrow_via_call_suffix`).
        if matches!(external_event, ExternalEvent::StaticBorrow { via: Some(_), .. } | ExternalEvent::MutableBorrow { via: Some(_), .. }) {
            title
        } else {
            format!("{}: {} may point into {}, and {} stay borrowed while {} is in use", title, field, either, all, field)
        }
    }
}

/// The field's own dot where those borrows start (`arrow_field_lenders_title`).
pub fn event_dot_field_lenders_borrow(my_name: &String, lenders: &[String], mutable: bool) -> String {
    let (either, all) = if lenders.len() == 2 { ("either", "both") } else { ("any of them", "all of them") };
    format!(
        "{0} {1} borrows {2}: it may point into {3}, so {4} stay borrowed while {0} is in use",
        styled_name_html(my_name), if mutable { "mutably" } else { "immutably" }, or_list(lenders), either, all
    )
}

/// The field's own dot where they end.
pub fn event_dot_field_lenders_end(my_name: &String, lenders: &[String], mutable: bool) -> String {
    format!(
        "{0}'s {1} borrow of {2} ends",
        styled_name_html(my_name), if mutable { "mutable" } else { "immutable" }, or_list(lenders)
    )
}

/// The title of the call icon on the dot of an argument passed by
/// reference (`f(&s)`, `f(&mut s)`): `from_variable` is lent to
/// `function`.
pub fn arrow_call_title(external_event: &ExternalEvent, function: &Function, from_variable: &ResourceTy) -> String {
    let styled_fn_name = styled_name_html(&function.name);
    let styled_from_name = styled_name_html(&from_variable.name());
    // `k[0].push('!')` with k an array of `&mut String` (#1319): push
    // writes through the reference in k[0], not through k's value.
    let styled_via = match external_event {
        ExternalEvent::PassByMutableReference { deep: Some(d @ DeepPlace { elem: Some(_), .. }), .. }
        | ExternalEvent::PassByStaticReference { deep: Some(d @ DeepPlace { elem: Some(_), .. }), .. } =>
            styled_name_html(&d.via(&from_variable.name())),
        _ => styled_from_name.clone(),
    };

        let title_fn = match external_event {
            ExternalEvent::PassByStaticReference { .. } => " reads from ",
            // `r` is itself a `&mut` reference (#812): f
            // reads and writes the value r points to, not r,
            // so say "through" like the other `&mut` writes.
            ExternalEvent::PassByMutableReference { is_reborrow: true, .. } => " reads and writes through ",
            ExternalEvent::PassByMutableReference { .. } => " reads from/writes to ",
            _ => panic!("internal state: expected a pass-by-reference event here")
        };

        // Reborrow note for the call-dot tooltip. `f(r)`
        // with r: `&mut T` and f taking `&mut T` is
        // compiler-rewritten to `f(&mut *r)`: r isn't
        // moved, it's unusable while the reborrow is live
        // and usable again afterwards. When f's result
        // can't carry a borrow, the reborrow ends when f
        // returns; otherwise (`let w = first(r);`) it can
        // last as long as the result, so don't promise
        // "until f returns". No note for `&T` arguments:
        // passing a shared reference copies it, and r is
        // read-only before, during and after the call.
        let reborrow_suffix: String = match external_event {
            ExternalEvent::PassByMutableReference { is_reborrow: true, result_has_lifetime: false, .. } => format!(
                " (mutable reborrow — {} can't be used until {} returns)",
                styled_from_name, styled_fn_name,
            ),
            ExternalEvent::PassByMutableReference { is_reborrow: true, result_has_lifetime: true, .. } => format!(
                " (mutable reborrow — {} can't be used while the reborrow is live)",
                styled_from_name,
            ),
            _ => String::new(),
        };
        // The `&` / `&mut` was inserted by Rust, not written
        // (`println!("{}", s)` takes `&s`, `s.len()` is
        // `String::len(&s)`): say so, or the call reads like
        // it moves s. Method calls are everywhere, so their
        // note is the shorter one.
        let implicit_suffix: String = match external_event {
            ExternalEvent::PassByStaticReference { implicit: ImplicitRef::MacroArg, .. } => format!(
                " through an implicit reference, so {} isn't moved", styled_from_name),
            ExternalEvent::PassByMutableReference { implicit: ImplicitRef::MacroArg, .. } => format!(
                " through an implicit mutable reference, so {} isn't moved", styled_from_name),
            ExternalEvent::PassByStaticReference { implicit: ImplicitRef::Autoref, .. } =>
                " (implicitly borrowed)".to_string(),
            // A reborrow through a reference (`rv[0].push(..)`
            // with rv: &mut Vec) already says "mutable
            // reborrow"; "implicitly mutably borrowed" would
            // stack a second parenthetical, and describes an
            // autoref of an owner, which this isn't.
            ExternalEvent::PassByMutableReference { implicit: ImplicitRef::Autoref, is_reborrow: false, .. } =>
                " (implicitly mutably borrowed)".to_string(),
            _ => String::new(),
        };

        // `&self` method on a value with interior mutability
        // (`c.set(2)` on a Cell, `cell.borrow_mut()`,
        // `m.lock()`, atomics): the borrow checker sees a
        // shared borrow, but the value inside can change,
        // so "reads from" would be false (#203).
        let interior_mut = matches!(external_event, ExternalEvent::PassByStaticReference { implicit: ImplicitRef::No | ImplicitRef::Autoref, .. })
            && from_variable.extract_rap().map_or(false, |r| r.has_interior_mut());
        // The same through a shared reference to such a value (`r.set(2)`
        // with `r: &Cell<i32>`, #1149).
        let interior_mut_through_ref = matches!(external_event, ExternalEvent::PassByStaticReference { pointee_interior_mut: true, .. });
        // The callee's signature lets it store the reference
        // (`point(&mut hold, &t)`, #513): the borrow lasts past
        // the call, until the borrow checker ends it.
        let stored_suffix: String = match external_event {
            ExternalEvent::PassByStaticReference { stored_in: StoredIn(Some(place), moved_to, copied_to, alternatives, cloned_to, moves_of_clone, made_to), .. }
            | ExternalEvent::PassByMutableReference { stored_in: StoredIn(Some(place), moved_to, copied_to, alternatives, cloned_to, moves_of_clone, made_to), .. } => {
                let styled_place = styled_name_html(place);
                // The place's value moves on while the borrow is live
                // (`let w = hold;`, `let w = pass(hold);`): the borrow
                // goes with it (#990).
                let moved_on = match moved_to.as_slice() {
                    [] => String::new(),
                    [one] => format!(
                        ", and after the borrow moves on to {0}, while {0} is in use",
                        styled_name_html(one),
                    ),
                    // Moves in different arms: only one happens (#997).
                    many if *alternatives => {
                        let names: Vec<String> = many.iter().map(|n| styled_name_html(n)).collect();
                        let (last, rest) = names.split_last().expect("two or more");
                        format!(
                            ", and after the borrow moves on to {}{} or {} (whichever branch runs), while that variable is in use",
                            rest.join(", "), if rest.len() > 1 { "," } else { "" }, last,
                        )
                    }
                    many => format!(
                        ", and after the borrow moves on to {}, while each of them is in use",
                        many.iter().map(|n| styled_name_html(n)).collect::<Vec<_>>().join(", then "),
                    ),
                };
                // A copy of the place's value (`let w = hold;` with
                // `hold: Option<&String>`) holds the borrow too (#997),
                // and so does a clone (`let w = hold.clone();`, #1003):
                // its references are copies of hold's. (It is called a
                // clone, not a copy: hold's type needn't be Copy.) So
                // does another value made from hold's whose type rustc
                // ties to the borrow (`let w: Vec<&String> =
                // hold.iter().copied().collect();`, #1015).
                let also = |names: &[String], one: &str, many: &str| match names {
                    [] => String::new(),
                    [n] => format!(", and while {}, {} {}, is in use", styled_name_html(n), one, styled_place),
                    names => format!(
                        ", and while {}, {} {}, are in use",
                        join_names(&names.iter().map(|n| styled_name_html(n)).collect::<Vec<_>>()),
                        many, styled_place,
                    ),
                };
                let copied = also(copied_to, "a copy of", "copies of");
                let cloned = also(cloned_to, "a clone of", "clones of");
                // Each value made from another is named with its source
                // (`x`, made from w, after `let w = hold.clone();`).
                let mut made = String::new();
                let mut rest: &[(String, String)] = made_to;
                while let Some((_, src)) = rest.first() {
                    let n = rest.iter().take_while(|(_, s)| s == src).count();
                    let names: Vec<String> = rest[..n].iter().map(|(d, _)| styled_name_html(d)).collect();
                    made.push_str(&format!(
                        ", and while {}, made from {}, {} in use",
                        join_names(&names), styled_name_html(src), if n == 1 { "is" } else { "are" },
                    ));
                    rest = &rest[n..];
                }
                // A move of a clone's value comes after the clone.
                let later = if *moves_of_clone { [copied, cloned, made, moved_on] } else { [moved_on, copied, cloned, made] };
                format!(
                    "; {}'s signature lets it store this reference in {}, so {} stays borrowed while {} is in use{}",
                    styled_fn_name, styled_place, styled_from_name, styled_place, later.concat(),
                )
            }
            _ => String::new(),
        };
        // The argument is one of several `break` values of a
        // `loop` (`take(loop { if c { break ra; } else {
        // break rb; } })`, #662): passed only if that `break`
        // ended the loop. Said right after the argument, as
        // the reborrow note holds whichever `break` ran.
        let alt = match external_event {
            ExternalEvent::PassByStaticReference { break_alt: Some(alt), .. }
            | ExternalEvent::PassByMutableReference { break_alt: Some(alt), .. } => Some(alt),
            _ => None,
        };
        let alt_suffix = alt.map_or(String::new(), break_alt_if_suffix);
        if interior_mut_through_ref {
            let msg = fn_dot_shared_ref_interior_mut(
                &function.name, &from_variable.name(),
                matches!(external_event, ExternalEvent::PassByStaticReference { refcell_borrow_mut: true, .. }));
            alt.map_or(msg.clone(), |a| with_break_alt_lend(&msg, a)) + &stored_suffix
        } else if interior_mut {
            let msg = fn_dot_shared_borrow_interior_mut(
                &function.name, &from_variable.name(),
                matches!(external_event, ExternalEvent::PassByStaticReference { refcell_borrow_mut: true, .. }));
            alt.map_or(msg.clone(), |a| with_break_alt_lend(&msg, a)) + &stored_suffix
        } else {
            format!("{}{}{}{}{}{}{}", styled_fn_name, title_fn, styled_via, alt_suffix, implicit_suffix, reborrow_suffix, stored_suffix)
        }
}

/// A column's name when no message could be built for it (a fallback the
/// dot's own text normally replaces).
pub fn event_dot_unknown_owner() -> String {
    "Unknown Resource Owner Value".to_owned()
}

/// The `return` on each of `lines`, as one phrase: "the `return` on
/// line 3", "the `return` on line 3 or 7".
fn returns_on_lines(lines: &[usize]) -> String {
    match lines.split_last() {
        None => String::new(),
        Some((last, [])) => format!("the `return` on line {}", last),
        Some((last, init)) => {
            let init: Vec<String> = init.iter().map(|l| l.to_string()).collect();
            format!("the `return` on line {} or {}", init.join(", "), last)
        }
    }
}

/// What a drop at the end of a loop (or after it) adds when `return`s
/// directly in the loop body (#1020) may leave the function first and
/// drop it there instead: " (or at the `return` on line 3, if that runs
/// first)". Empty without any.
pub fn or_at_returns(lines: &[usize]) -> String {
    match lines {
        [] => String::new(),
        [_] => format!(" (or at {}, if that runs first)", returns_on_lines(lines)),
        _ => format!(" (or at {}, if one of them runs first)", returns_on_lines(lines)),
    }
}

/// The `return` that runs of those on `lines`: "the `return` on line
/// 3", "whichever `return` (line 3 or 7) runs".
fn whichever_return(lines: &[usize]) -> String {
    match lines {
        [_] => returns_on_lines(lines),
        _ => format!(
            "whichever `return` ({}) runs",
            returns_on_lines(lines).trim_start_matches("the `return` on ")
        ),
    }
}

/// What a scope end said to happen at a `return` adds when a `?` before
/// it may return first (#1044): " (or at the `?` on line 4, if that
/// returns first)". Empty without any.
fn or_at_tries(lines: &[usize]) -> String {
    let on = match lines.split_last() {
        None => return String::new(),
        Some((last, [])) => format!("line {}", last),
        Some((last, init)) => {
            let init: Vec<String> = init.iter().map(|l| l.to_string()).collect();
            format!("line {} or {}", init.join(", "), last)
        }
    };
    let which = if lines.len() == 1 { "that returns" } else { "one of them returns" };
    format!(" (or at the `?` on {on}, if {which} first)")
}

/// A scope end that `return`s directly in a loop body come before
/// (#1020), whose drops aren't drawn on their own rows. `title` is what
/// the dot says otherwise. A variable the loop body declared never gets
/// here on a path through the body (`always`): it goes out of scope at
/// the `return` (or at an exit drawn with its own drops), so the text
/// says so; likewise any variable at a scope end after a `loop` that
/// nothing breaks out of, which the code never reaches (#1041). One
/// declared before the loop (`maybe`) is dropped at the `return` if that
/// runs: said when this dot drops something (`drops`). A `?` that may
/// return before the `return` (`tries`) is named too (#1044).
pub fn event_dot_scope_end_after_loop_return(title: &str, early: &EarlyReturns, drops: bool) -> String {
    let mut out = title.to_owned();
    if !early.always.is_empty() {
        let at = format!("goes out of scope at {}{}", whichever_return(&early.always), or_at_tries(&early.tries));
        out = if out.contains("goes out of scope at the end of the block") {
            out.replacen("goes out of scope at the end of the block", &at, 1)
        } else if out.contains("goes out of scope") {
            out.replacen("goes out of scope", &at, 1)
        } else {
            format!("{}. This happens at {}{}", out.trim_end_matches('.'), whichever_return(&early.always), or_at_tries(&early.tries))
        };
        out = out.replace(" here ", " there ");
    }
    if drops && !early.maybe.is_empty() {
        if !out.ends_with('.') {
            out.push('.');
        }
        out.push_str(&format!(" If {} runs first, this happens there instead.", returns_on_lines(&early.maybe)));
    }
    out
}

/// The end of a column at the closing `}` of a `loop` that nothing
/// breaks out of (#1057): the code never gets past it, but the variable
/// isn't dropped here. It is still in scope (and an owner still owns its
/// resource) on every path that keeps looping, and leaves only where the
/// loop is left: at the exits drawn above it (`return`, `?`, a panic),
/// or never. `owns`: it holds a resource that would be dropped; `moved`:
/// the loop moves it, at one of those exits.
pub fn event_dot_loop_runs_on(my_name: &str, owns: bool, exits: LoopExits, moved: bool) -> String {
    let n = styled_name_html(my_name);
    let head = if owns {
        format!("{n} is still alive while the loop keeps running")
    } else {
        format!("{n} is still in scope while the loop keeps running")
    };
    match (exits, owns) {
        (LoopExits::Dropping, true) if moved => format!("{head}; it's moved or dropped only at the exits above"),
        (LoopExits::Dropping, true) => format!("{head}; it's dropped only at the exits above"),
        (LoopExits::Dropping, false) => format!("{head}; it goes out of scope only at the exits above"),
        (LoopExits::None, true) => format!("{head}; this loop never ends, so {n} is never dropped"),
        (LoopExits::None, false) => format!("{head}; this loop never ends, so {n} never goes out of scope"),
        (LoopExits::Other, _) => head,
    }
}

/// A scope end after the resource was moved out on every path that
/// reaches it: there is nothing left to drop.
pub fn event_dot_no_resource_dropped_suffix() -> String {
    ". No resource is dropped.".to_owned()
}

/// A scope end that drops the column's resource; `drop_note` is the
/// column's own addition (see `ResourceAccessPoint::drop_note`).
pub fn event_dot_resource_dropped_suffix(drop_note: &str) -> String {
    format!(". Its resource is dropped{}.", drop_note)
}

/// What a closure does with a variable it mentions, as the verb the
/// capture list names it by.
pub fn capture_verb_moved() -> &'static str { "moved" }
pub fn capture_verb_copied() -> &'static str { "copied" }
pub fn capture_verb_immutably_borrowed() -> &'static str { "immutably borrowed" }
pub fn capture_verb_mutably_borrowed() -> &'static str { "mutably borrowed" }
pub fn capture_verb_immutably_reborrowed() -> &'static str { "immutably reborrowed" }
pub fn capture_verb_mutably_reborrowed() -> &'static str { "mutably reborrowed" }

/// The title of the line drawn while a reference column is live and can
/// write (`mutable`) or only read what it points to.
pub fn ref_line_holds_reference(styled_name: &str, mutable: bool) -> String {
    if mutable {
        format!("{} holds a mutable reference", styled_name)
    } else {
        format!("{} holds a reference", styled_name)
    }
}

/// The title of the line drawn while a reference column is live and its
/// referent can't be written through it.
pub fn ref_line_holds_immutable_reference(styled_name: &str) -> String {
    format!("{} holds an immutable reference", styled_name)
}

/// What `n` in `let n = r?;` holds, by the kind of column `n` is (#652).
pub enum TryHolds { MutRef, Ref, CopyOwner, Plain, Resource }

pub fn acquire_from_try_holds(kind: TryHolds) -> &'static str {
    match kind {
        TryHolds::MutRef => "is bound to the mutable reference",
        TryHolds::Ref => "is bound to the immutable reference",
        TryHolds::CopyOwner => "is initialized with the value",
        TryHolds::Plain => "is bound to the value",
        TryHolds::Resource => "acquires ownership of the resource",
    }
}

#[cfg(test)]
mod escape_tooltip_attrs_tests {
    use super::escape_tooltip_attrs;

    #[test]
    fn bare_ampersand_and_lt_in_a_tooltip_attribute_are_escaped() {
        let svg = r#"<g data-tooltip-text="at the end of this `&&` / `||` operand a<b"/>"#;
        assert_eq!(
            escape_tooltip_attrs(svg),
            r#"<g data-tooltip-text="at the end of this `&amp;&amp;` / `||` operand a&lt;b"/>"#
        );
    }

    #[test]
    fn existing_entities_and_other_attributes_are_untouched() {
        let svg = r#"<g d="a&b" data-tooltip-text="&lt;span&gt;x&amp;lt;&amp;amp;&#38;&#x26; & &x"/>"#;
        assert_eq!(
            escape_tooltip_attrs(svg),
            r#"<g d="a&b" data-tooltip-text="&lt;span&gt;x&amp;lt;&amp;amp;&#38;&#x26; &amp; &amp;x"/>"#
        );
    }
}

// A variable in scope at a call to `process::exit` / `abort` (#1172):
// the process ends there and runs no destructors. `owns`: whether it
// holds a value that needs dropping (`Some(false)`: nothing to drop;
// `None`: it may hold one, or only part of one).
pub fn event_dot_process_exit(name: &String, owns: Option<bool>, exit: &NoDropExit) -> String {
    let call = format!("`{}`", exit.call);
    // A function of the user's that never returns (#1553).
    match (exit.how, owns) {
        (NeverReturns::Exits, _) => {}
        (NeverReturns::Spins, Some(true)) => return format!(
            "{0} never returns: it loops forever, so the code after it never runs and {1}'s resource is never dropped",
            call, styled_name_html(name)
        ),
        (NeverReturns::Spins, None) => return format!(
            "{0} never returns: it loops forever, so the code after it never runs and whatever {1} still holds is never dropped",
            call, styled_name_html(name)
        ),
        (NeverReturns::Spins, Some(false)) => return format!(
            "{0} never returns: it loops forever, so the code after it never runs. {1} holds no resource to drop",
            call, styled_name_html(name)
        ),
        (NeverReturns::Unknown, Some(true)) => return format!(
            "{0} never returns. If it panics, the unwinding drops {1}'s resource here; if it loops forever or ends the program instead, {1}'s resource is never dropped",
            call, styled_name_html(name)
        ),
        (NeverReturns::Unknown, None) => return format!(
            "{0} never returns. If it panics, the unwinding drops whatever {1} still holds here; if it loops forever or ends the program instead, that's never dropped",
            call, styled_name_html(name)
        ),
        (NeverReturns::Unknown, Some(false)) => return format!(
            "{0} never returns, so the code after it never runs. {1} holds no resource to drop",
            call, styled_name_html(name)
        ),
    }
    match owns {
        Some(true) => format!(
            "{0} ends the program here without running any destructor: {1}'s resource is never dropped",
            call, styled_name_html(name)
        ),
        None => format!(
            "{0} ends the program here without running any destructor: whatever {1} still holds is never dropped",
            call, styled_name_html(name)
        ),
        Some(false) => format!(
            "{0} ends the program here. {1} holds no resource to drop",
            call, styled_name_html(name)
        ),
    }
}

/* Approximation notes (`ApproxNote`): what a figure simplifies, added to
   the tooltip of a dot drawn on the construct anyway. Each is one
   sentence in parentheses that says what RustViz doesn't draw, never
   that what it draws is complete. */

// On a loop's first dot drawn for a pass (#853), one per loop. The
// figure shows the body once; the loop may run it any number of times
// (also none, or once before a `break`), so this says no more than that.
// `declares`: the loop declares variables (its pattern, a `let` in its
// body, a `while let` binding), which every pass that gets there
// makes anew. `at_most_once`: every pass leaves the loop (#1467), so it
// never runs more than once, but a `while` / `for` may not run at all.
pub fn approx_note_loop(declares: bool, at_most_once: bool) -> String {
    if at_most_once {
        "(RustViz draws one pass through this loop, which may not run at all.)".to_owned()
    } else if declares {
        "(RustViz draws one pass through this loop, which may run many times; the variables declared inside it are new on each pass.)".to_owned()
    } else {
        "(RustViz draws one pass through this loop, which may run many times.)".to_owned()
    }
}

// On the receiver's dot of `c.borrow()` / `c.borrow_mut()` (c a RefCell;
// #685). The compile-time shared borrow is what's drawn; RefCell's own
// run-time borrow lasts until the Ref / RefMut is dropped (whose drop
// may itself be drawn, as a temporary's).
pub fn approx_note_refcell(method: &str) -> String {
    let guard = if method == "borrow_mut" { "RefMut" } else { "Ref" };
    format!(
        "(RefCell also tracks this borrow at run time, until the {} that {} returns is dropped; RustViz doesn't draw run-time borrows.)",
        guard, styled_name_html(method)
    )
}

// On the receiver's dot of `m.lock()` (Mutex), `l.read()` / `l.write()`
// (RwLock; #685). `reading`: Some for an RwLock.
pub fn approx_note_lock(kind: &str, method: &str, reading: Option<bool>) -> String {
    let how = match reading {
        Some(true) => " for reading",
        Some(false) => " for writing",
        None => "",
    };
    format!(
        "(The {} stays locked{} until the guard that {} returns is dropped; RustViz doesn't draw the lock.)",
        kind, how, styled_name_html(method)
    )
}

// On p's dot where `*p` reads or writes through the raw pointer p
// (#194): it's drawn on p's column only.
pub fn approx_note_raw_deref(name: &str) -> String {
    format!(
        "({0} is a raw pointer; RustViz doesn't show this on what {0} points to.)",
        styled_name_html(name)
    )
}

// On the first binding's dot at the end of the block of `let r = if c {
// &String::from("x") } else { &s };` (#1383): the temporary a branch
// borrows, which Rust extends to the end of the block, has no column.
pub fn approx_note_branch_extended_temp(name: &str, code: Option<&str>, line: usize) -> String {
    let what = match code {
        Some(c) => format!("{} on line {}", styled_name_html(c), line),
        None => format!("the borrow of a temporary on line {}", line),
    };
    format!(
        "(If {} ran, the temporary it borrows is dropped here: Rust keeps it alive until the end of this block because {} holds a reference to it. RustViz doesn't draw a column for this temporary.)",
        what,
        styled_name_html(name)
    )
}

// On p's first dot for `let p = &mut x as *mut T;` (#194): x's column
// shows no borrow.
pub fn approx_note_raw_cast(name: &str, lender: &str) -> String {
    format!(
        "({0} is a raw pointer to {1}; RustViz doesn't draw raw pointers as borrows, so {1}'s column shows nothing here.)",
        styled_name_html(name), styled_name_html(lender)
    )
}

// On the collection's dot for `*slots[0]` with slots[0] a reference
// (#689): which variable it borrows from isn't known statically.
pub fn approx_note_element_ref(place: &str) -> String {
    format!(
        "(RustViz doesn't follow {} to what it points to, so this isn't drawn on the variable it borrows from.)",
        styled_name_html(place)
    )
}

// On the first dot of a closure with captures (#133, #278, #347).
pub fn approx_note_closure(name: &str) -> String {
    format!(
        "(RustViz draws {0}'s captures here; the uses in {0}'s body aren't drawn where {0} is called.)",
        styled_name_html(name)
    )
}

// On the first dot of a variable holding an async block (#135).
pub fn approx_note_async_block(name: &str) -> String {
    format!(
        "(RustViz doesn't draw the async block's body, which runs when {} is awaited.)",
        styled_name_html(name)
    )
}

// On the first dot of a variable holding the future an async fn call
// returns (#135).
pub fn approx_note_async_call(name: &str, fn_name: &str) -> String {
    format!(
        "({1} is an async fn; RustViz doesn't draw its body, which runs when {0} is awaited.)",
        styled_name_html(name), styled_name_html(fn_name)
    )
}

// On the first dot of each parameter of an async fn (#135).
pub fn approx_note_async_fn(fn_name: &str) -> String {
    format!(
        "({} is an async fn; RustViz doesn't draw its body, which runs when the future it returns is awaited.)",
        styled_name_html(fn_name)
    )
}

/// Whether `name` is a temporary's column label (`temporary_label`).
pub fn is_temporary_name(name: &str) -> bool {
    name == "(temporary)" || (name.starts_with("(temporary ") && name.ends_with(')'))
}

#[cfg(test)]
mod still_borrowed_with_lend_tests {
    use super::*;

    fn n(s: &str) -> String { s.to_owned() }

    #[test]
    fn the_lend_to_a_named_holder_folds_into_the_end() {
        let still = event_dot_reacquire_still_borrowed(&n("v"), &n("third"), &[n("x")], false, false, false);
        let lend = event_dot_static_lend(&n("v"), &n("x"), false, false);
        let merged = still_borrowed_with_lend(&still, &lend).unwrap();
        assert_eq!(merged, format!("{still}; {} is read-only while the borrow is live", styled_name_html("v")));
        // Two holders, both lent on the dot: one line, the clause once.
        let still2 = event_dot_reacquire_still_borrowed(&n("v"), &n("third"), &[n("x"), n("y")], false, false, false);
        let once = still_borrowed_with_lend(&still2, &lend).unwrap();
        let twice = still_borrowed_with_lend(&once, &event_dot_static_lend(&n("v"), &n("y"), false, false)).unwrap();
        assert_eq!(once, twice);
    }

    #[test]
    fn a_lend_to_someone_else_or_of_another_kind_stays() {
        let still = event_dot_reacquire_still_borrowed(&n("v"), &n("third"), &[n("x")], false, false, false);
        assert!(still_borrowed_with_lend(&still, &event_dot_static_lend(&n("v"), &n("z"), false, false)).is_none());
        assert!(still_borrowed_with_lend(&still, &event_dot_mut_lend(&n("v"), &n("x"), false)).is_none());
        assert!(still_borrowed_with_lend(&still, &event_dot_static_lend(&n("w"), &n("x"), false, false)).is_none());
    }
}
