use crate::svg_generator::data::{BranchExit, LoopLeft, ResourceAccessPoint};


/* Event Dot messages: shows up when someone hovers over a dot */

use crate::svg_generator::data::{ArgTie, ChainFailHow, ChainUse, RcCall, RcDrop, StaticLender, TryMove, TupleDropAt, TupleLeft, ViaCarrier};
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
// A non-Copy type with no drop glue (#484: an iterator over borrowed data,
// a struct of integers that doesn't derive `Copy`) holds a "value" too:
// "resource" means something that needs cleaning up. Callers pass
// `holds_value()` (Copy, or non-Copy with nothing to drop).
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

// A `Box` whose whole contents were moved out (`take(*b)`, #609) goes
// out of scope: the contents' drop isn't the box's, but the heap
// allocation is still freed.
pub fn event_dot_box_contents_moved_go_out_of_scope(my_name: &String) -> String {
    format!(
        "{} goes out of scope. Its contents were moved out earlier, so only the Box's heap allocation is freed.",
        fmt_style(my_name)
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
    // A `Box` a field was moved out of (#602): Rust still frees the
    // box's heap allocation, so the text says so.
    boxed: bool,
) -> String {
    const FREED: &str = " The Box's heap allocation is freed.";
    fn join(names: &[String]) -> (String, bool) {
        let fmt: Vec<String> = names.iter().map(fmt_style).collect();
        let s = match fmt.len() {
            0 | 1 => fmt.concat(),
            n => format!("{} and {}", fmt[..n - 1].join(", "), fmt[n - 1]),
        };
        (s, names.len() > 1)
    }
    if plain {
        // Nothing here needs drop, so nothing is "dropped" whichever
        // fields moved (#484).
        let mut out = format!("{} goes out of scope.", fmt_style(my_name));
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
        let mut out = format!("{} goes out of scope.", fmt_style(my_name));
        if !moved.is_empty() {
            let (moved_s, moved_plural) = join(moved);
            out.push_str(&format!(" {} {} moved out earlier.", moved_s, if moved_plural { "were" } else { "was" }));
        }
        let (maybe_s, maybe_plural) = join(maybe_moved);
        if by_cond {
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
        fmt_style(my_name),
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
// `u.a = 5;`, `u.a = y;`, `u = U { .. };`. It reads like a plain
// assignment (2026-11-05 decision) plus the clause that states the
// no-drop fact. `is_copy` is whether the value written is Copy;
// `source` names the variable it was copied from, if any (a literal
// or call result has none).
pub fn event_dot_union_write(my_name: &String, is_copy: bool, source: Option<&String>) -> String {
    let head = match (is_copy, source) {
        (true, Some(from)) => event_dot_copyable_assign_copy(my_name, from),
        (true, None) => event_dot_copyable_assign(my_name),
        (false, _) => event_dot_acquire(my_name, my_name),
    };
    format!("{0}; writing to a union never drops its old contents", head)
}

// An owned (non-ref) function parameter receives its resource from
// whatever the caller passed. The L-shaped arrow on the param's
// timeline visually anchors that "from outside this scope" origin;
// this is its tooltip and the matching dot tooltip.
pub fn event_dot_owner_init_from_caller_value(my_name: &String) -> String {
    format!("{0} is bound to a value from the caller", fmt_style(my_name))
}

// A Copy parameter (`n: i32`, `c: bool`, `a: Option<i32>`): the caller's
// value is copied in; nothing is owned or moved (#505).
pub fn event_dot_copy_init_from_caller(my_name: &String) -> String {
    format!("{0} is a copy of the caller's value", fmt_style(my_name))
}

pub fn event_dot_owner_init_from_caller(my_name: &String) -> String {
    let my_name_fmt = fmt_style(my_name);
    format!("{0} acquires ownership from the caller", my_name_fmt)
}

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

// A reference field of a struct bound by a pattern (`if let Some(x) =
// o`, `x.part: &str`). It holds whatever reference the matched value
// held; what that borrows isn't tracked at the pattern, so the message
// doesn't name a lender, and never says "owns" (#352).
pub fn event_dot_ref_init_in_pattern(my_name: &String, bound: &String, is_mut: bool) -> String {
    let my_name_fmt = fmt_style(my_name);
    let bound_fmt = fmt_style(bound);
    let kind = if is_mut { "a mutable" } else { "an immutable" };
    format!("{0} holds {1} reference (it came with the value matched into {2})", my_name_fmt, kind, bound_fmt)
}

// The column of a by-value parameter's value that a `ref` binding
// borrows (`fn f(ref s: String)`, #691): the caller's argument was moved
// into `f`, which owns it until it returns.
pub fn event_dot_param_value_init(my_name: &String, fn_name: &String, whole: bool) -> String {
    format!(
        "{0} is {2} the value the caller passed to {1}. {1} owns it, and a ref binding borrows it",
        fmt_style(my_name), fmt_style(fn_name), if whole { "" } else { "part of" }
    ).replace("  ", " ")
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
pub fn event_dot_owner_drop_at_reassign(my_name: &String, note: &str) -> String {
    let my_name_fmt = fmt_style(my_name);
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
pub fn event_dot_owner_drop_at_reassign_maybe_moved(my_name: &String, by_cond: bool) -> String {
    format!(
        "{0} acquires ownership of a new resource. Its previous resource is dropped here only if \
         {1} moved it (Rust tracks this with a runtime drop flag).",
        fmt_style(my_name), moved_above(by_cond)
    )
}

/// Who may have moved a maybe-moved variable, for the drop-flag
/// wording: "no branch above", or, when a let-chain's condition may have
/// moved it before failing (#281), "nothing above".
pub fn moved_above(by_cond: bool) -> &'static str {
    if by_cond { "nothing above" } else { "no branch above" }
}

// A call's result that nothing takes ownership of is a temporary; Rust
// drops it when its statement (or other temporary scope) ends (#457).
// The two dots of its column: where it comes into being, and the drop.
pub fn event_dot_unbound_temporary_acquire(_my_name: &String, from_name: &String) -> String {
    // The column the value comes from is a fn's, a std macro's (`vec!`)
    // or a literal's (`P { .. }`, `(..)`, `[..]`) (#469).
    let source = if from_name.ends_with('!') {
        format!("the {0} macro produces", fmt_style(from_name))
    } else if from_name.ends_with(" { .. }") || from_name == "(..)" || from_name == "[..]" {
        format!("the literal {0} builds", fmt_style(from_name))
    } else {
        format!("{0} returns", fmt_style(from_name))
    };
    format!(
        "A temporary holds the value that {0}. Nothing binds it to a variable or moves it on, so Rust drops it as soon as its statement (or enclosing expression) is done",
        source
    )
}

// A variable moved into an unbound temporary (`vec![s];`, `let _ = P {
// name: t, n: 1 };`): the variable's value is now the temporary's, and
// is dropped with it (#570).
pub fn event_dot_unbound_temporary_takes(from_name: &String) -> String {
    format!(
        "This temporary takes the value of {0}, which is moved into it. Nothing binds the temporary to a variable or moves it on, so Rust drops it, and what it holds, as soon as its statement (or enclosing expression) is done",
        fmt_style(from_name)
    )
}

// The drop of a temporary that a `let` borrowed (#400): its column has
// the same label as an unbound temporary's, so the tooltip says why it
// is dropped here and not at the end of its statement.
pub fn event_dot_temporary_go_out_of_scope(my_name: &String) -> String {
    format!(
        "{0} goes out of scope at the end of the block: a let borrowed it, so it was kept alive past the end of its statement. Its resource is dropped.",
        fmt_style(my_name)
    )
}

// The same temporary dropped where an early exit leaves its block
// (`return`, `break`, `continue`, a panic: #553), so "at the end of
// the block" would be false.
pub fn event_dot_extended_temporary_drop(my_name: &String, when: &String) -> String {
    format!(
        "{0} goes out of scope {1}: a let borrowed it, so it lived past its statement. Its resource is dropped.",
        fmt_style(my_name), when
    )
}

pub fn event_dot_temporary_drop(ty: &String, when: &String, literal: bool) -> String {
    if literal {
        // `let (x, _) = (m, n);`: m went into x, n stayed in the literal.
        return format!(
            "This temporary {0}, built by the literal on the right of the let, is dropped {1}. What the pattern didn't bind by value stays in it and is dropped with it. Its resource is dropped",
            fmt_style(ty), when
        );
    }
    format!(
        "This temporary {0} is dropped {1}: nothing bound it to a variable or moved it on. Its resource is dropped",
        fmt_style(ty), when
    )
}

/// Whether `acquire` is the plain "acquires ownership of a resource" line
/// that `reassign` (`event_dot_owner_drop_at_reassign`) already says, for
/// the same variable, with more (#506, #519).
pub fn reassign_subsumes_acquire(reassign: &str, acquire: &str) -> bool {
    const INFIX: &str = " acquires ownership of a new resource; its previous resource";
    // (a holds note, #542, sits before "is dropped" and an Rc / Arc /
    // Weak's drop note after it, #373)
    reassign.split_once(INFIX)
        .is_some_and(|(name, tail)| (tail.starts_with(" is dropped") || tail.starts_with(" (including "))
            && (acquire.strip_suffix(" acquires ownership of a resource") == Some(name)
                // `s = r?;` (#652): r's dot says s gets what's inside it.
                || acquire.strip_prefix(name).is_some_and(|rest| rest.starts_with(" acquires ownership of the resource the `?` takes out of "))))
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
    let name = acquire.strip_suffix(&format!(" acquires ownership of a resource{}", break_alt_receiver_suffix()))?;
    let rest = reassign.strip_prefix(name)?.strip_prefix(NEW)?;
    Some(format!("{name}{NEW}{}{rest}", break_alt_receiver_suffix()))
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
    let my_name_fmt = fmt_style(my_name);
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
    let (fn_fmt, to_fmt) = (fmt_style(fn_name), fmt_style(to_name));
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
    let name = fmt_style(my_name);
    let gets = match target {
        Some(t) => format!("{} gets the value inside it", fmt_style(t)),
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
        TryMove::Result => format!("{}'s `Ok`", fmt_style(from)),
        TryMove::Option => format!("{}'s `Some`", fmt_style(from)),
        TryMove::Other => fmt_style(from),
    };
    format!("{} {holds} the `?` takes out of {inside}", fmt_style(my_name))
}

// `x?` returns from the function on `Err` / `None`; every value the
// function still owns is dropped there (#430). The dot sits on the
// owner's column, which goes on: on success nothing happens here.
// `note`: what else the drop drops, or an Rc's count note (#542).
pub fn event_dot_owner_drop_if_try_returns(my_name: &String, note: &str) -> String {
    let my_name_fmt = fmt_style(my_name);
    let (holds, rest) = split_drop_note(note);
    format!(
        "Only if the `?` here returns early (on an `Err` or `None`): the function returns and {0}'s \
         resource{1} is dropped{2}. If the `?` succeeds, nothing happens to {0} here.",
        my_name_fmt, holds, rest
    )
}

// A variable moved (or copied) into the temporary that a tuple / struct
// / array literal on the right of a `let` builds, where the pattern
// doesn't bind it by value (`_`, `..`, a `ref` binding): #439. The
// temporary is dropped at the end of the statement, unless a `ref`
// binding borrows from it, which extends it to the end of the block.
pub fn event_dot_literal_temporary_acquire(from_name: &String, ends_with_statement: bool) -> String {
    let when = if ends_with_statement {
        "No `ref` binding borrows from it, so Rust drops it at the end of this statement"
    } else {
        "A `ref` binding borrows from it, so it lives until the end of the enclosing block, not just to the end of the statement"
    };
    format!(
        "The temporary that the literal on the right of this let builds takes the value of {0}. {1}",
        fmt_style(from_name), when
    )
}

// The start of the temporary of a literal on the right of a `let`, when
// no variable is moved into it (every element the pattern leaves is a
// fresh value, #483). Dropped at the end of the statement: with a `ref`
// binding it is the extended kind, whose start says why it lives on.
pub fn event_dot_literal_temporary_start() -> String {
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
pub fn event_dot_move_to_value(my_name: &String, _target_name: &String) -> String {
    format!("{0}'s value is moved", fmt_style(my_name))
}

pub fn event_dot_move_to(my_name: &String, _target_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "{0}'s resource is moved",
        my_name_fmt
    )
}

// Source dot for the head of a `for` loop that is moved into the
// iterator `IntoIterator::into_iter` builds from it (`for s in v`, `v:
// Vec<String>`), when that iterator has drop glue (#482). Its drop is
// stated where the loop ends, on the iterator's `(temporary N)` column
// (#469).
pub fn event_dot_move_into_loop_iterator(my_name: &String) -> String {
    format!(
        "{0}'s resource is moved into the `for` loop's iterator",
        fmt_style(my_name)
    )
}

// Source dot for a `for` loop's head that is an iterator already (`for
// s in it`, `it: vec::IntoIter<String>`): `into_iter` hands it back
// unchanged, so it becomes the loop's iterator, and the loop drops it
// (`event_dot_loop_iterator_drop`) (#482).
pub fn event_dot_move_iterator_into_loop(my_name: &String) -> String {
    format!(
        "{0}'s resource is moved into the `for` loop, which uses it as its iterator",
        fmt_style(my_name)
    )
}

// The drop of the iterator a `for` loop took from a variable (`for s in
// it`), on that variable's column: where the loop ends (also when a
// `break` leaves it), or at a `return` from inside the loop (`early`)
// (#482). What the iterator still owns goes with it: the elements it
// hasn't handed out, a `vec::IntoIter`'s buffer.
pub fn event_dot_loop_iterator_drop(my_name: &String, left: LoopLeft) -> String {
    match left {
        LoopLeft::Return => format!(
            "{0}'s former resource, the `for` loop's iterator, is dropped here: the function returns from inside the loop. Whatever the iterator still owns is dropped with it (such as elements it hasn't handed out yet)",
            fmt_style(my_name)
        ),
        LoopLeft::Break => format!(
            "{0}'s former resource, the `for` loop's iterator, is dropped here: this `break` leaves the `for` loop. Whatever the iterator still owns is dropped with it (such as elements it hasn't handed out yet)",
            fmt_style(my_name)
        ),
        LoopLeft::End => format!(
            "{0}'s former resource, the `for` loop's iterator, is dropped when the `for` loop ends, along with whatever it still owns (such as elements it hasn't handed out yet)",
            fmt_style(my_name)
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
            fmt_style(my_name),
            fmt_style(callee)
        );
    }
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
pub fn event_dot_move_to_drop(my_name: &String, fn_name: &String, value: bool, note: &str) -> String {
    format!(
        "{0}'s {2} is moved into {1}, which drops it{3}",
        fmt_style(my_name),
        fmt_style(fn_name),
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
        fmt_style(my_name),
        if extended { "the block (a `ref` binding borrows from it)" } else { "this statement" }
    )
}

// "the temporary tuple (p, q)", or just "the temporary tuple" when the
// scrutinee is too long to quote.
fn temporary_tuple(tuple: &String) -> String {
    if tuple.is_empty() {
        "the temporary tuple".to_owned()
    } else {
        format!("the temporary tuple {}", fmt_style(tuple))
    }
}

// `if let (Some(_), Some(r)) = (p, q)` / `match (x, y)`: building the
// tuple moves p into it (#327). Where the tuple is dropped, a
// TupleTempDrop dot says so (event_dot_tuple_temp_drop).
pub fn event_dot_move_to_tuple_temporary(my_name: &String, tuple: &String, value: bool) -> String {
    format!("{0}'s {1} is moved into {2}", fmt_style(my_name), held(value), temporary_tuple(tuple))
}

// The scrutinee tuple is dropped, and with it whatever of each
// variable's value the pattern didn't move out (#327). Timing is for
// edition 2024 (what RustViz compiles with): an `if let` drops its
// scrutinee's temporaries at the end of the if branch, or before the
// else branch when the pattern doesn't match; a `match` keeps them to
// the end of its statement.
pub fn event_dot_tuple_temp_drop(tuple: &String, at: TupleDropAt, with: &[TupleLeft]) -> String {
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
        let names: Vec<String> = plain.iter().map(|w| format!("{}'s", fmt_style(&w.name))).collect();
        vec![format!("{} resources", join_and(&names))]
    } else {
        plain.iter().map(|w| format!(
            "{}{}'s resource{}",
            if w.rest { "the rest of " } else { "" },
            fmt_style(&w.name),
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
        let names: Vec<String> = ws.iter().map(|w| format!("{}'s", fmt_style(&w.name))).collect();
        let notes: String = ws.iter().map(|w| rc_drop_note(w.rc)).collect();
        let takers: Vec<String> = takers.iter().map(fmt_style).collect();
        parts.push(format!("whatever of {} resource{}{} {} didn't take",
            join_and(&names), if ws.len() > 1 { "s" } else { "" }, notes, join_and(&takers)));
    }
    let items = join_and(&parts);
    if at == TupleDropAt::TryReturns {
        return format!("{lead}{tuple} is dropped, and with it {items}. If the `?` succeeds, nothing happens to the tuple here.");
    }
    format!("{lead}{tuple} is dropped {when}, and with it {items}")
}

// def fn(p):
//  p
//  |
//  *-->   the return event
//
pub fn event_dot_move_to_caller_value(my_name: &String, _target_name: &String) -> String {
    format!("{0}'s value is moved to the caller", fmt_style(my_name))
}

pub fn event_dot_move_to_caller(my_name: &String, _target_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "{0}'s resource is moved to the caller",
        my_name_fmt
    )
}

/// `let n = *slots[0];`: `place` is `*slots[0]`; the resource copied is
/// the one the reference stored in `slots[0]` points to, not slots's own.
pub fn event_dot_copy_through_element(place: &String) -> String {
    let elem = place.strip_prefix('*').unwrap_or(place).trim_start();
    format!("the resource {0} points to is copied", fmt_style(&elem.to_owned()))
}

pub fn event_dot_move_through_element(place: &String) -> String {
    let elem = place.strip_prefix('*').unwrap_or(place).trim_start();
    format!("the resource {0} points to is moved", fmt_style(&elem.to_owned()))
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
// `pointee_copy`: the referent doesn't need drop, so it's a "value"
// (`&i32`, `&str`), not a "resource" (#551). A `&T` is moved only as
// a field of a struct that is moved (`let hold = other;`, #637):
// `other.r` can't be used after that either.
pub fn event_dot_ref_moved(my_name: &String, to_caller: bool, pointee_copy: bool, is_mut: bool) -> String {
    format!(
        "{0}'s {3} reference is moved{1} (not the {2} it points to)",
        fmt_style(my_name),
        if to_caller { " to the caller" } else { "" },
        held(pointee_copy),
        if is_mut { "mutable" } else { "immutable" }
    )
}

pub fn event_dot_ref_copied(my_name: &String, pointee_copy: bool) -> String {
    format!(
        "{0}'s reference is copied (not the {1} it points to)",
        fmt_style(my_name),
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

// Lender's dot where a reference copied out of a value that holds the
// borrow ends (`n` in `while let Some(n) = it.next()`, #575): n's part
// of the borrow is given back, but `holders` (`it`) still hold it, so
// the lender is still borrowed.
pub fn event_dot_reacquire_still_borrowed(my_name: &String, from_name: &String, holders: &[String], mutable: bool, is_copy: bool) -> String {
    let holders: Vec<String> = holders.iter().map(fmt_style).collect();
    format!(
        "{0}'s {1} is no longer borrowed by {2}, but is still {3} borrowed by {4}",
        fmt_style(my_name),
        held(is_copy),
        fmt_style(from_name),
        if mutable { "mutably" } else { "immutably" },
        holders.join(" and "),
    )
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
    let my_name_fmt = fmt_style(my_name);
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
            fmt_style(place),
            fmt_style(holder),
            held(holds_value)
        )
    } else {
        format!(
            "{0} is copied out of {1}'s {2}",
            fmt_style(place),
            fmt_style(holder),
            held(holds_value)
        )
    }
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
            fmt_style(&elem),
            fmt_style(place),
            held(holds_value)
        )
    } else if holder_is_ref {
        format!(
            "the {2} {0} points to is modified through {1}",
            fmt_style(holder),
            fmt_style(place),
            held(holds_value)
        )
    } else {
        format!(
            "{0}'s {2} is modified through {1}",
            fmt_style(holder),
            fmt_style(place),
            held(holds_value)
        )
    }
}

// 1   0        1   0
// |            |   
// o-->*   or   o-->*     the star event
// |   |            |
pub fn event_dot_acquire_value(my_name: &String, _target_name: &String) -> String {
    format!("{0} is bound to a value", fmt_style(my_name))
}

pub fn event_dot_acquire(my_name: &String, _target_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);

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
        fmt_style(my_name)
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
// - `r = s;`, s: `&mut T`: the reference is moved out of s (the arrow
//   says "Move from s to r"; like any place assignment, the dot reads
//   like `r = s`, with no "(moved out of s)", #650).
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
    let my_name_fmt = fmt_style(my_name);
    if is_mut {
        format!("{0} is bound to a mutable reference", my_name_fmt)
    } else {
        format!("{0} is bound to an immutable reference", my_name_fmt)
    }
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
        fmt_style(ref_name),
        fmt_style(deref_name),
        held(pointee_copy)
    )
}

// `*r` returned with r a reference (`r: &i32`): the caller gets a copy
// of the value r points to, not a reference to it (#670). `**y` reads
// the value behind the reference y points to.
pub fn event_dot_copy_deref_to_caller(ref_name: &String, place: &String) -> String {
    let depth = place.chars().take_while(|c| *c == '*').count();
    match depth {
        0 | 1 => format!(
            "a copy of the value {0} points to is returned to the caller",
            fmt_style(ref_name)
        ),
        2 => format!(
            "a copy of {0}, the value behind the reference {1} points to, is returned to the caller",
            fmt_style(place),
            fmt_style(ref_name)
        ),
        _ => format!(
            "a copy of {0}, the value reached by following {1}'s references, is returned to the caller",
            fmt_style(place),
            fmt_style(ref_name)
        ),
    }
}

// Copy out of a field reached through a reference: `let a = p.x;`
// with p: &P. The value is read from what p points to (which p
// doesn't own), so it's phrased like the write case above, including
// "value" when p points to a Copy type (#312).
pub fn event_dot_copy_through_deref_field(ref_name: &String, place: &String, target_name: &String, pointee_copy: bool) -> String {
    format!(
        "the {3} {0} points to is read through {1} (copied to {2})",
        fmt_style(ref_name),
        fmt_style(place),
        fmt_style(target_name),
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
pub fn event_dot_capture_move_to_closure_value(my_name: &String, target_name: &String) -> String {
    format!(
        "{0}'s value is captured (moved) by closure {1}",
        fmt_style(my_name), fmt_style(target_name)
    )
}

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
    let my_name_fmt = fmt_style(my_name);
    let func_fmt = fmt_style(func);
    if captures.is_empty() {
        return format!("{0} is bound to the closure {1} returned", my_name_fmt, func_fmt);
    }
    let parts: Vec<String> = captures
        .iter()
        .map(|(name, kind)| {
            let how = if kind.starts_with("mutably") { "mutably" } else { "immutably" };
            format!("{} ({})", fmt_style(name), how)
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
pub fn event_dot_closure_capture_acquire_value(my_name: &String, from_name: &String) -> String {
    format!(
        "Closure {0} captures (moves) {1}'s value",
        fmt_style(my_name), fmt_style(from_name)
    )
}

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

// Appended to the "Immutable borrow from s to w" / "Mutable borrow
// from s to w" arrow title when `w` is the reference a call returned
// (`let w = first_word(&s);`, #171): says why, via the lifetimes in the
// callee's signature. With several lenders the signature ties the
// result to several arguments (`longest(&a, &b)`): the borrow checker
// can't know which one it points into, so all of them stay borrowed
// for as long as the result is used.
pub fn arrow_borrow_via_call_suffix(to_name: &String, func: &String, lenders: &[String], also_unnamed: bool, tie: &ArgTie, carrier: ViaCarrier) -> String {
    let func_fmt = fmt_style(func);
    let to_fmt = fmt_style(to_name);
    if carrier == ViaCarrier::StoredBy {
        // `refs.push(&s)` (#392): `func` is the method.
        return format!(
            ", stored in {0} by {1}; {0} isn't a reference, but its value now holds one",
            to_fmt, func_fmt
        );
    }
    if carrier == ViaCarrier::Element {
        // `let r = refs[0];` (#349): `func` is the collection.
        if lenders.len() <= 1 {
            return format!(": {0} is a reference copied out of {1}", to_fmt, func_fmt);
        }
        let names: Vec<String> = lenders.iter().map(fmt_style).collect();
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
                to_fmt, func_fmt, fmt_style(l), how
            ),
            Some(l) => format!(
                ": {0} holds the closure {1} returned, which {3} {2}, so {2} stays borrowed while {0} is in use",
                to_fmt, func_fmt, fmt_style(l), how
            ),
            None => String::new(),
        };
    }
    if let ViaCarrier::PatternBinding { from_call, moved } = carrier {
        // `Some(x)` on `third` / on `v.get(2)` (#391): x is the
        // reference stored in that value (copied, or moved for `&mut`).
        let how = if moved { "moved" } else { "copied" };
        return if from_call {
            format!(": {0} is a reference {1} out of the value {2} returned", to_fmt, how, func_fmt)
        } else {
            format!(": {0} is a reference {1} out of {2}", to_fmt, how, func_fmt)
        };
    }
    // A value that isn't a reference (`let it = v.iter();`) or a
    // struct with a reference field can carry the borrow too (#336).
    let (noun, not_ref) = match carrier {
        ViaCarrier::Reference => ("reference", String::new()),
        ViaCarrier::Value => ("value", format!("; {0} isn't a reference, but its value holds one", to_fmt)),
        ViaCarrier::StructField => ("struct", String::new()),
        ViaCarrier::Element | ViaCarrier::StoredBy | ViaCarrier::PatternBinding { .. } | ViaCarrier::ReturnedClosure { .. } => unreachable!(),
    };
    // A value can hold borrows of several of them at once (`map` over
    // `v.iter()` with a closure that borrows `extra`).
    let point = if carrier == ViaCarrier::Value { "borrow from" } else { "point into" };
    fn join(items: &[String]) -> String {
        match items.len() {
            0 => String::new(),
            1 => items[0].clone(),
            2 => format!("{} and {}", items[0], items[1]),
            n => format!("{}, and {}", items[..n - 1].join(", "), items[n - 1]),
        }
    }
    fn ordinal(i: usize) -> String {
        const NAMES: [&str; 10] = ["first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth", "ninth", "tenth"];
        NAMES.get(i).map(|s| s.to_string()).unwrap_or_else(|| format!("{}th", i + 1))
    }
    fn count_word(n: usize) -> String {
        const NAMES: [&str; 10] = ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine"];
        NAMES.get(n).map(|s| s.to_string()).unwrap_or_else(|| n.to_string())
    }
    // Which arguments the result's lifetime is the same as.
    let first_arg = usize::from(tie.receiver);
    let n_args = tie.total.saturating_sub(first_arg);
    let recv_tied = tie.receiver && tie.tied.contains(&0);
    let args: Vec<usize> = tie.tied.iter().filter(|&&i| i >= first_arg).map(|i| i - first_arg).collect();
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
    let mut tied = match (recv_tied, arg_part) {
        (true, Some(a)) => format!("the value it was called on and {}", a),
        (true, None) => "the value it was called on".to_owned(),
        (false, Some(a)) => a,
        (false, None) => "its arguments".to_owned(),
    };
    let tied_count = tie.tied.len().max(1);
    if tie.unnamed > 0 && tied_count >= 2 {
        tied.push_str(&match (tie.unnamed, tied_count) {
            (1, 2) => " (the other is a value with no column here)".to_owned(),
            (1, _) => " (one of them is a value with no column here)".to_owned(),
            (k, _) => format!(" ({} of them are values with no column here)", count_word(k)),
        });
    }
    let names: Vec<String> = lenders.iter().map(fmt_style).collect();
    let unnamed_slots = tie.unnamed.max(usize::from(also_unnamed));
    let tail = if names.len() >= 2 {
        let either = if names.len() + unnamed_slots == 2 { "either" } else { "any of them" };
        let kept = match (names.len(), unnamed_slots > 0) {
            (2, false) => "both stay borrowed".to_owned(),
            (_, false) => "all of them stay borrowed".to_owned(),
            (_, true) => format!("{} stay borrowed", join(&names)),
        };
        format!(", so it may {} {}, and {} while {} is in use", point, either, kept, to_fmt)
    } else if names.len() == 1 && (tied_count >= 2 || also_unnamed) {
        format!(", so {} stays borrowed while {} is in use", names[0], fmt_style(to_name))
    } else {
        String::new()
    };
    format!(": {0}'s signature gives the {1} it returns the same lifetime as {2}{3}{4}", func_fmt, noun, tied, tail, not_ref)
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
        // Never drawn as an arrow (see the renderer).
        StaticLender::TupleTemporary =>
            "the temporary tuple ends".to_string(),
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
pub fn state_value_moved(my_name: &String) -> String {
    format!("{0}'s value was moved, so {0} can't be used until it's assigned again", fmt_style(my_name))
}

// Mutably borrowed, for a column holding a value with nothing to drop.
pub fn state_value_revoked(my_name: &String) -> String {
    format!("{0}'s value is mutably borrowed, so it cannot access the value", fmt_style(my_name))
}

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
// A non-Copy owner with nothing to drop: also a value, not a resource (#484).
pub fn state_full_privilege_value(my_name: &String) -> String {
    format!("{0} holds a value", fmt_style(my_name))
}

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

// A non-reference variable whose value holds a borrow (`let it =
// v.iter();`, `let third = v.get(2);`; #336). It owns that value (an
// iterator, an Option), but not the data the value borrows, so the
// generic "is the owner of the resource" is avoided.
pub fn state_holder_full_privilege(my_name: &String) -> String {
    let my_name_fmt = fmt_style(my_name);
    format!(
        "{0} holds a value that contains a reference ({0} isn't a reference itself)",
        my_name_fmt
    )
}

// A holder whose value starts out without a reference (`let mut refs
// = Vec::new();`, filled later by `refs.push(&s)`; #392). Its type can
// hold references; whether it holds one depends on the line.
pub fn state_holder_starts_empty(my_name: &String) -> String {
    let my_name_fmt = fmt_style(my_name);
    format!(
        "{0} owns a value that can hold references ({0} isn't a reference itself)",
        my_name_fmt
    )
}

// The holder's dot where its value starts borrowing `from`. When
// `from` is itself a reference (a reference parameter `v: &Vec<i32>`
// in `let it = v.iter();`; #393), the value borrows the data `from`
// points to, not the reference.
pub fn event_dot_holder_borrow(my_name: &String, from_name: &String, is_mut: bool, from_is_ref: bool) -> String {
    let what = if from_is_ref {
        format!("the data {} points to", fmt_style(from_name))
    } else {
        format!("{}'s resource", fmt_style(from_name))
    };
    format!(
        "{0} holds a value that {1} borrows {2} ({0} isn't a reference itself)",
        fmt_style(my_name),
        if is_mut { "mutably" } else { "immutably" },
        what
    )
}

// `let it2 = it;` where `it` holds a borrow: it2 takes over the value
// and, with it, the borrow.
pub fn event_dot_holder_acquire(my_name: &String, from_name: &String) -> String {
    format!(
        "{0} takes over {1}'s value, and the borrow it holds",
        fmt_style(my_name), fmt_style(from_name)
    )
}

// Scope end of a holder that still holds its (non-Copy) value.
pub fn event_dot_holder_drop_suffix() -> String {
    ". Its value is dropped, but not the data it borrowed.".to_owned()
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
        fmt_style(my_name),
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
        fmt_style(my_name),
        bound_in_branches(diverged)
    )
}

/// `event_dot_branch_merge_bound` for a reference variable: every
/// branch bound it to a reference, which never owns its referent.
pub fn event_dot_branch_merge_bound_ref(my_name: &String, is_mut: bool, diverged: &[(String, Vec<BranchExit>)]) -> String {
    let kind = if is_mut { "a mutable" } else { "an immutable" };
    format!(
        "{0} is bound to {1} reference{2}",
        fmt_style(my_name), kind,
        bound_in_branches(diverged)
    )
}

/// `event_dot_branch_merge_bound` for a Copy variable (`let n = match x
/// { 0 => 1, _ => 2 };`): an `i32` holds a value, not a resource it
/// owns (#316). Matches the per-arm `event_dot_acquire_copyable`.
pub fn event_dot_branch_merge_bound_copy(my_name: &String, diverged: &[(String, Vec<BranchExit>)]) -> String {
    format!(
        "{0} is bound to a value{1}",
        fmt_style(my_name),
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
        _ => format!("the {} arm", fmt_style(name)),
    };
    let same_way = diverged.iter().all(|(_, e)| *e == diverged[0].1);
    if same_way && diverged.iter().all(|(n, _)| n != "If" && n != "Else") && diverged.len() > 1 {
        let names: Vec<String> = diverged.iter().map(|(n, _)| fmt_style(n)).collect();
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
pub fn event_dot_branch_merge_moved_with_drop(my_name: &String, value: bool, note: &str) -> String {
    let my_name_fmt = fmt_style(my_name);
    if value {
        return format!(
            "{0} was moved in at least one branch above, so it can't be used after the branches \
             (until it's assigned again).",
            my_name_fmt
        );
    }
    let (holds, rest) = split_drop_note(note);
    format!(
        "{0} was moved in at least one branch above, so it can't be used after the branches \
         (until it's assigned again). If the branch that ran didn't move it, its resource{1} is \
         dropped{2} when {0} goes out of scope.",
        my_name_fmt, holds, rest
    )
}

// `event_dot_branch_merge_moved_with_drop` when the variable is assigned
// again before it goes out of scope: the old value, if it's still there,
// is dropped at that assignment instead (#335).
pub fn event_dot_branch_merge_moved_then_reassigned(my_name: &String) -> String {
    format!(
        "{0} was moved in at least one branch above, so it can't be used after the branches \
         until it's assigned again. If the branch that ran didn't move it, its resource is \
         dropped when {0} is assigned again.",
        fmt_style(my_name)
    )
}

// Scope end of a variable that was moved on some paths of a conditional
// and not on others: whether anything is dropped is decided at runtime
// by a drop flag (#179).
pub fn event_dot_go_out_of_scope_maybe_moved(my_name: &String, note: &str, by_cond: bool) -> String {
    let (holds, rest) = split_drop_note(note);
    format!(
        "{0} goes out of scope. Its resource{1} is dropped{2} here only if {3} moved it \
         (Rust tracks this with a runtime drop flag).",
        fmt_style(my_name), holds, rest, moved_above(by_cond)
    )
}

// `let r = loop { let k = …; if c { break k; } };` (#431): the
// `break` moves `k`, declared inside the loop, to the loop's receiver.
// That is drawn where the loop ends, which is also where `k` goes out
// of scope on the iterations that don't break (they drop it there).
pub fn event_dot_break_out_move(my_name: &String, to: &String, value: bool) -> String {
    format!(
        "{0}'s {1} is moved to {2} by the {3} that ends the loop",
        fmt_style(my_name),
        if value { "value" } else { "resource" },
        fmt_style(to),
        fmt_style(&"break".to_owned())
    )
}

pub fn event_dot_break_out_go_out_of_scope(my_name: &String) -> String {
    format!(
        "{0} goes out of scope. Its resource is dropped here on every iteration that doesn't end the loop.",
        fmt_style(my_name)
    )
}

// Merge dot for a variable that no branch above moved (#179; this used
// to be an empty tooltip).
pub fn event_dot_branch_merge_unchanged(my_name: &String) -> String {
    format!("{0} isn't moved in any of the branches above", fmt_style(my_name))
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
            [one] => format!("a new variable named {}, declared on line {}", fmt_style(&base), one),
            many => {
                let (last, init) = many.split_last().unwrap();
                let init: Vec<String> = init.iter().map(|l| l.to_string()).collect();
                format!(
                    "a new variable named {}, declared on lines {} and {} (in different branches or blocks, so only one exists at a time)",
                    fmt_style(&base), init.join(", "), last
                )
            }
        };
        out += &format!(". {0} shadows {1}: it is {2}", fmt_style(name), fmt_style(shadowed), declared);
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
            fmt_style(name), lines
        );
    }
    out
}

// Merge dot for a reference declared before the conditional whose
// borrow ended in each arm, at its last use there or, in an arm that
// doesn't use it, as the arm starts.
pub fn event_dot_branch_merge_borrow_ended(my_name: &String) -> String {
    format!("{0}'s borrow ended in each branch above", fmt_style(my_name))
}

// Dashed rest of an `if` / `match` arm after the borrow held by a
// reference declared before the conditional has ended in that arm.
pub fn state_ref_borrow_ended(my_name: &String) -> String {
    format!("{0}'s borrow has ended on this path", fmt_style(my_name))
}

// Merge dot for a variable that was moved only in branches of type `!`
// (`if n > 0 { return s; }`, #318): those never get here, so on every
// path that does it still holds what it held before. `leavers`: how
// each such branch left. A reference never owns anything, so it
// "holds its reference" instead.
pub fn event_dot_branch_merge_kept(my_name: &String, is_ref: Option<bool>, value: bool, leavers: &[Vec<BranchExit>]) -> String {
    let name = fmt_style(my_name);
    let holds = match is_ref {
        Some(true) => format!("{} still holds its mutable reference here", name),
        Some(false) => format!("{} still holds its reference here", name),
        // A value with nothing to drop is a "value", not a "resource" (#484).
        None if value => format!("{} still owns its value here", name),
        None => format!("{} still owns its resource here", name),
    };
    let plural = leavers.len() > 1;
    let mut all: Vec<BranchExit> = leavers.iter().flatten().copied().collect();
    all.sort();
    all.dedup();
    let exits = if leavers.iter().all(|e| *e == leavers[0]) { leavers[0].clone() } else { all };
    let subject = if plural { format!("the branches that moved {}", name) } else { format!("the branch that moved {}", name) };
    match exits.as_slice() {
        [BranchExit::Other] | [] => format!(
            "{}: {} {}, so {} reach this point",
            holds, subject, exits_verb(&exits, plural), if plural { "they don't" } else { "it doesn't" }
        ),
        _ => format!("{}: {} {} before reaching this point", holds, subject, exits_verb(&exits, plural)),
    }
}

// At the end of a loop, for a variable declared outside it that a
// branch inside moved before a `break` out of the loop. `definite`:
// every way out of the loop is such a `break`.
// `labeled_block`: the same at the end of a labeled block (`'a: { …
// break 'a; … }`).
pub fn event_dot_loop_exit_move(my_name: &String, definite: bool, labeled_block: bool) -> String {
    let name = fmt_style(my_name);
    let brk = fmt_style(&"break".to_owned());
    let what = if labeled_block { "block" } else { "loop" };
    if definite {
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

// At the start of the `else` branch of an `if` whose let-chain
// condition moves the variable in one part and can still be false in a
// later one (`if let Some(a) = x && a.len() > 5`, #281): on that path
// the else branch runs after the move (rustc: "borrow of partially
// moved value"). `definite`: every way into the else branch passes the
// move (the parts before it can't fail). `no_else`: there's no `else`,
// so it's the path where the body is skipped. `to`: where the condition
// moves it; `dropped`: those of them (the chain's own bindings) that
// are dropped when the condition fails, with what they hold.
pub fn event_dot_chain_fail_move(
    my_name: &String,
    definite: bool,
    to: &[String],
    dropped: &[String],
    no_else: bool,
    how: &ChainFailHow,
) -> String {
    fn join(names: &[String]) -> String {
        let fmt: Vec<String> = names.iter().map(fmt_style).collect();
        match fmt.len() {
            0 | 1 => fmt.concat(),
            n => format!("{} and {}", fmt[..n - 1].join(", "), fmt[n - 1]),
        }
    }
    // What is moved: the variable, or for `take(*b)` the place `*b`
    // (#579).
    let name = match how {
        ChainFailHow::Out(place) => fmt_style(place),
        _ => fmt_style(my_name),
    };
    // A call of an FnOnce closure moves it into the call (#579).
    let (moves, act) = match how {
        ChainFailHow::Call => (format!(
            "the condition calls {} (it can only be called once, so the call moves it)", name), "call"),
        _ if to.is_empty() => (format!("the condition moves {}", name), "move"),
        _ => (format!("the condition moves {} to {}", name, join(to)), "move"),
    };
    let if_kw = fmt_style(&"if".to_owned());
    let mut out = match (definite, no_else) {
        (false, false) => format!(
            "{0} may have been moved: when {1} and then turns out false, the else branch runs \
             after that {2}, so {0} can't be used here",
            name, moves, act),
        (true, false) => format!(
            "{0} was moved: {1} before it can turn out false, so the else branch runs after that \
             {2}, and {0} can't be used here",
            name, moves, act),
        (false, true) => format!(
            "{0} may have been moved: when {1} and then turns out false, the body is skipped but \
             the {2} has happened, so {0} can't be used after the {3}",
            name, moves, act, if_kw),
        (true, true) => format!(
            "{0} was moved: {1} before it can turn out false, so the {2} has happened even when \
             the body is skipped, and {0} can't be used after the {3}",
            name, moves, act, if_kw),
    };
    if !dropped.is_empty() {
        let plural = dropped.len() > 1;
        out.push_str(&format!(
            ". When the condition fails after that move, {} {} out of scope and {} {} dropped.",
            join(dropped),
            if plural { "go" } else { "goes" },
            if plural { "their resources" } else { "its resource" },
            if plural { "are" } else { "is" },
        ));
    }
    out
}

// "t.0 or t.1", "t.0, t.1 or t.2": the places an or-pattern binds a
// name from (#533).
fn or_list(sources: &[String]) -> String {
    let names: Vec<String> = sources.iter().map(|n| fmt_style(n)).collect();
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
    let me = fmt_style(my_name);
    let (bound, bound_from) = match part_of {
        Some((x, p)) => (fmt_style(x), fmt_style(p)),
        None => (fmt_style(into), me.clone()),
    };
    if let Some(tuple) = tuple {
        return format!(
            "{1} is bound from {2}, whichever alternative of the pattern matches. If it's the alternative \
             that binds {1} from {3}, {0}'s {4} is moved out of {5} into {6} here; otherwise it stays \
             in the tuple",
            me, bound, or_list(sources), bound_from, held(value), temporary_tuple(&tuple.to_owned()), fmt_style(into)
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
        me, bound, or_list(sources), bound_from, held(value), fmt_style(into), otherwise
    )
}

// An or-pattern arm (`(ref x, _) | (_, ref x) =>`) binds the reference
// `into` from a different place in each alternative, and `my_name` is
// one of them (#565). Only the alternative that matches borrows its
// place, but which one that is is decided at run time, so the borrow
// checker treats every one of them as borrowed while `into` is live.
pub fn event_dot_or_alt_lend(my_name: &String, into: &String, sources: &[String], mutable: bool, is_copy: bool) -> String {
    let me = fmt_style(my_name);
    let x = fmt_style(into);
    let while_live = if mutable {
        format!("{} can't be used while {}'s borrow is live", me, x)
    } else {
        format!("{} is read-only while {}'s borrow is live", me, x)
    };
    format!(
        "{1} borrows {2}, whichever alternative of the pattern matches. If it's the alternative that binds \
         {1} from {0}, {0}'s {3} is {4} borrowed by {1} here. Which alternative matches is decided at run \
         time, so the borrow checker treats {0} as borrowed either way: {5}.",
        me, x, or_list(sources), held(is_copy), if mutable { "mutably" } else { "immutably" }, while_live
    )
}

// The end of the borrow `event_dot_or_alt_lend` started (#565).
pub fn event_dot_or_alt_reacquire(my_name: &String, into: &String, mutable: bool, is_copy: bool) -> String {
    format!(
        "{1}'s borrow ends here, so {0}'s {2} is no longer {3} borrowed (the borrow checker counted it as \
         borrowed whichever alternative of the pattern matched)",
        fmt_style(my_name), fmt_style(into), held(is_copy), if mutable { "mutably" } else { "immutably" }
    )
}

// The reference's own dot where `event_dot_or_alt_lend`'s borrow starts
// (#565).
pub fn event_dot_or_alt_borrow(my_name: &String, sources: &[String], mutable: bool) -> String {
    format!(
        "{0} {1} borrows {2}, whichever alternative of the pattern matches",
        fmt_style(my_name), if mutable { "mutably" } else { "immutably" }, or_list(sources)
    )
}

// Line segment of a place while the borrow `event_dot_or_alt_lend`
// started is live (#565).
pub fn state_or_alt_lent(my_name: &String, into: &String, mutable: bool, is_copy: bool) -> String {
    let me = fmt_style(my_name);
    let x = fmt_style(into);
    if mutable {
        format!(
            "{0}'s {2} is mutably borrowed by {1} if the alternative that binds {1} from {0} matched; \
             either way, {0} can't be used until {1}'s borrow ends",
            me, x, held(is_copy)
        )
    } else {
        format!(
            "{0}'s {2} is immutably borrowed by {1} if the alternative that binds {1} from {0} matched; \
             either way, {0} is read-only until {1}'s borrow ends",
            me, x, held(is_copy)
        )
    }
}

// At the start of the `else` branch (`no_else`: the path that skips the
// body) of an `if` whose let-chain condition lends the variable in a part
// before one that can still be false (#688): `if let Some(a) = &x && let
// Some(b) = y.take()` lends `y` to `take`, and `x` to `a`. The borrows
// have ended by the time the else branch runs: a call's when it returns,
// a binding's when the failed condition ends its scope. `definite`: as
// for event_dot_chain_fail_move. `is_ref`: the variable is a reference,
// and it's the resource it points to that is lent.
pub fn event_dot_chain_fail_use(
    my_name: &String,
    uses: &[ChainUse],
    definite: bool,
    no_else: bool,
    is_ref: bool,
) -> String {
    fn join(names: &[String]) -> String {
        let fmt: Vec<String> = names.iter().map(fmt_style).collect();
        match fmt.len() {
            0 | 1 => fmt.concat(),
            n => format!("{} and {}", fmt[..n - 1].join(", "), fmt[n - 1]),
        }
    }
    let name = fmt_style(my_name);
    let (what, obj) = if is_ref {
        (format!("the resource {} points to", name), "it".to_owned())
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
    let lends = if named.is_empty() {
        format!("the condition borrows {}", obj)
    } else {
        format!("the condition lends {} to {}", obj, join(&named))
    };
    let bound: Vec<String> = uses.iter().filter(|u| u.binding && !u.to.is_empty()).map(|u| u.to.clone()).collect();
    let gos = match bound.len() {
        0 => String::new(),
        1 => format!("{} goes out of scope, so ", join(&bound)),
        _ => format!("{} go out of scope, so ", join(&bound)),
    };
    let ended = if uses.len() > 1 { "those borrows have ended" } else { "that borrow has ended" };
    let then = if no_else {
        format!("the body is skipped after {}", ended)
    } else {
        format!("the else branch runs after {}", ended)
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
        .map(|u| format!("by {}", fmt_style(&u.to))).collect();
    let through: Vec<String> = uses.iter().filter(|u| u.changes && u.binding && !u.to.is_empty())
        .map(|u| format!("through {}", fmt_style(&u.to))).collect();
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
// value (`rc`: what that drop does for an Rc). `no_else`: there's no
// `else`, so it's the path where the body is skipped.
pub fn event_dot_chain_fail_assign(
    my_name: &String,
    drops_old: bool,
    rc: Option<RcDrop>,
    no_else: bool,
) -> String {
    let name = fmt_style(my_name);
    let what = if drops_old { "resource" } else { "value" };
    let mut out = if no_else {
        format!(
            "{0} may have been assigned a new {1}: when the condition assigns to {0} and then \
             turns out false, the body is skipped but the assignment has happened, so after the \
             {2} {0} holds either its previous {1} or the new one.",
            name, what, fmt_style(&"if".to_owned()))
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
        Some(b) => fmt_style(b),
        None => "the break that carries it".to_owned(),
    }
}

/// Appended to the receiver's dot: it got the value of one `break`,
/// whichever ran.
pub fn break_alt_receiver_suffix() -> String {
    format!(" (from whichever {} ended the loop)", fmt_style(&"break".to_owned()))
}

/// Appended to the source's dot and to the arrow: the copy or move
/// happened only if this `break` ended the loop.
pub fn break_alt_if_suffix(brk: &Option<String>) -> String {
    format!(", if the loop ended with {}", quoted_break(brk))
}

/// The lender's dot when a `break` borrows from it for the receiver
/// (`break &e`, or `break rb` reborrowing a `&mut`, #657): the borrow
/// was made only if this `break` ended the loop. The qualifier goes on
/// the borrow, before what it means for the lender ("; rb can't be
/// used while the borrow is live"): rustc rejects that use while the
/// receiver is live whichever `break` ran.
pub fn with_break_alt_lend(title: &str, brk: &Option<String>) -> String {
    let suffix = break_alt_if_suffix(brk);
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
    let name = fmt_style(my_name);
    let to = fmt_style(to);
    let brk = &alt.brk;
    let moved = if what == "reference" {
        format!("the reference in {0}", name)
    } else {
        format!("{0}'s {1}", name, what)
    };
    if alt.in_loop {
        format!("{0} is moved to {1} if the loop ended with {2}", moved, to, quoted_break(brk))
    } else {
        format!(
            "{0} may have been moved: the loop can end with {1}, which moves {2} to {3}, so {0} can't be used after the loop",
            name, quoted_break(brk), moved, to
        )
    }
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
    let f = fmt_style(my_name);
    let mut msg = format!(
        "{0} is moved into the call (an FnOnce closure can only be called once)", f);
    for fate in fates {
        let s = fmt_style(&fate.name);
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
pub fn event_dot_closure_go_out_of_scope_maybe_moved(my_name: &String, by_cond: bool) -> String {
    format!(
        "{0} goes out of scope. Its captured resources are dropped here only if {1} \
         (Rust tracks this with a runtime drop flag).",
        fmt_style(my_name),
        if by_cond { "nothing above moved or called it" } else { "no branch above called it" }
    )
}
