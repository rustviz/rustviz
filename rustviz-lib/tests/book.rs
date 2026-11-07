//! Rust Book–style regression corpus: `rustviz-plugin/tests/book/*.rs`.
//!
//! 127 short programs in the style of The Rust Programming Language
//! (chapters 4–15, plus a few extras), each reviewed by hand against real
//! Rust semantics. The table below records, per snippet, what the plugin
//! on `main` gets right and what it gets wrong, so that
//!
//!   * a snippet that renders today keeps rendering (well-formed SVG, every
//!     element inside the canvas), and keeps the correct tooltips listed in
//!     `must_contain`;
//!   * a false tooltip that a fix removed stays removed (`must_not_contain`);
//!   * a fix is *noticed*: when a `known_wrong` tooltip disappears, or a
//!     known crash starts rendering, the test prints a note asking you to
//!     update the table (it does not fail).
//!
//! See `rustviz-plugin/tests/book/README.md` for the categories and how to
//! update the table when a fix lands.
//!
//! Tooltip strings are compared after stripping markup and collapsing
//! whitespace. A string written as `"L13 text"` only matches a tooltip
//! drawn on source line 13 (`"L4+ text"` for the unnumbered row after
//! line 4); use it when the same text is right on one line and wrong on
//! another.
//!
//! Environment:
//!   * `RV_BOOK_ONLY=c4_,m_1` runs only snippets whose id starts with one
//!     of the comma-separated prefixes.
//!   * `RV_BOOK_DUMP=<dir>` writes `<dir>/<id>.txt` with every tooltip as
//!     `L<line> <text>`, the format used in this file.
//!   * `RV_BOOK_JOBS=<n>` sets the number of parallel plugin runs (default 4).
//!
//! Like `corpus.rs`, this runs the *installed* plugin (`cargo rv-plugin`).

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use rustviz_lib::Rustviz;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Status {
    /// Renders, and every tooltip reviewed is true.
    Correct,
    /// Renders, but shows something false about Rust (`known_wrong`).
    Wrong,
    /// Renders and nothing shown is false, but an event is missing or drawn
    /// off-canvas.
    Missing,
    /// The plugin panics on this snippet today. (No snippet crashes on main
    /// right now; the variant stays for the next one that does.)
    #[allow(dead_code)]
    Crash,
}
use Status::*;

struct Snippet {
    /// File stem in `rustviz-plugin/tests/book/`; the prefix is the probe id.
    id: &'static str,
    status: Status,
    /// What's wrong or missing, and where it's tracked.
    note: &'static str,
    /// Correct tooltips that must stay. Missing one fails the test.
    must_contain: &'static [&'static str],
    /// False tooltips that a fix removed. Seeing one again fails the test.
    must_not_contain: &'static [&'static str],
    /// False tooltips that main still prints. When one disappears the test
    /// prints a note: move it to `must_not_contain`, and add the corrected
    /// text to `must_contain`.
    known_wrong: &'static [&'static str],
    /// Some elements are drawn below the SVG's height today. When they come
    /// back on canvas the test prints a note asking to clear this flag.
    off_canvas: bool,
}

const E: Snippet = Snippet {
    id: "",
    status: Correct,
    note: "",
    must_contain: &[],
    must_not_contain: &[],
    known_wrong: &[],
    off_canvas: false,
};

/// Old branch-merge wording: Rust drops at the end of the enclosing scope
/// (with a runtime drop flag), not at the end of the branch (#179). A fix
/// removed it; it must not come back.
const JOIN_OPT: &str = "opt was moved in at least one branch above; in branches where it was not, its resource is dropped at the branch's end.";

const BOOK: &[Snippet] = &[
    // ─── Chapter 4: ownership, references, slices ─────────────────────
    Snippet {
        id: "c4_01_clone",
        must_contain: &[
            "clone reads from s1 (implicitly borrowed)",
            "Move from clone to s2",
            "s1 goes out of scope. Its resource is dropped.",
            "s2 goes out of scope. Its resource is dropped.",
        ],
        ..E
    },
    Snippet {
        id: "c4_02_copy_tuple",
        must_contain: &["Copy from p to q", "Copy from p.0 to q.0", "Copy from p.1 to q.1"],
        ..E
    },
    Snippet {
        id: "c4_03_move_inner_block",
        must_contain: &[
            "Move from s1 to s2",
            "L6 s2 goes out of scope. Its resource is dropped.",
            "s1 goes out of scope. No resource is dropped.",
        ],
        ..E
    },
    Snippet {
        id: "c4_04_move_let_plain",
        must_contain: &[
            "Move from s1 to s2",
            "s1 goes out of scope. No resource is dropped.",
            "s2 goes out of scope. Its resource is dropped.",
        ],
        ..E
    },
    Snippet {
        id: "c4_05_gives_ownership",
        must_contain: &[
            "some_string's resource is moved to the caller",
            "some_string goes out of scope. No resource is dropped.",
            "Move from gives_ownership to s1",
        ],
        ..E
    },
    Snippet {
        id: "c4_06_nll_shared_then_mut",
        must_contain: &[
            "Immutable borrow from s to r1",
            "Immutable borrow from s to r2",
            "Mutable borrow from s to r3",
            "r3 holds a mutable reference; it can read and write the borrowed resource",
            "Return mutably borrowed resource from r3 to s",
            "s goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &["r3 is the owner of the resource"],
        ..E
    },
    Snippet {
        id: "c4_07_first_word_loop",
        status: Wrong,
        note: "bytes = s.as_bytes() is shown as copied from as_bytes (#171)",
        must_contain: &[
            "s is an immutable borrow from the caller",
            "bytes holds an immutable reference; it can read the borrowed resource",
            "Copy from first_word to word",
            "L2 as_bytes no longer borrows the resource s points to",
        ],
        must_not_contain: &["bytes is the owner of the resource", "L2 s's resource is no longer immutably borrowed"],
        known_wrong: &["Copy from as_bytes to bytes"],
        ..E
    },
    Snippet {
        id: "c4_08_string_slices",
        must_contain: &[
            "Immutable borrow from s to hello",
            "Immutable borrow from s to world",
            "Return immutably borrowed resource from hello to s",
            "Return immutably borrowed resource from world to s",
        ],
        must_not_contain: &["hello is the owner of the resource", "world is the owner of the resource"],
        ..E
    },
    Snippet {
        id: "c4_09_first_word_str",
        must_contain: &[
            "s is an immutable borrow from the caller",
            "a reference from s is returned to the caller; the data it points to is not copied",
            "L13 word holds an immutable reference",
            "L13 Immutable borrow from my_string to word: first_word's signature gives the reference it returns the same lifetime as its argument",
            "L14 Return immutably borrowed resource from word to my_string",
            "my_string goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "bytes is the owner of the resource",
            "word is the owner of the resource",
            "s's resource is moved to the caller",
            "Return immutably borrowed resource from word to *word",
            "Copy from first_word to word",
            "L13 my_string's resource is no longer immutably borrowed",
        ],
        ..E
    },
    Snippet {
        id: "c4_10_str_literal_copy",
        must_contain: &[
            "s is bound to an immutable reference",
            "Copy from s to t",
            "t is initialized with a copy of s",
            "s's reference is copied (not the resource it points to)",
            "s's borrow of the string literal ends (string literals live for the whole program)",
            "t's borrow of the string literal ends (string literals live for the whole program)",
        ],
        must_not_contain: &[
            "s's immutable borrow ends (what it borrows from isn't shown)",
            "t's immutable borrow ends (what it borrows from isn't shown)",
            "s is the owner of the resource",
            "s acquires ownership of a resource",
            "s's resource is copied",
            "Return immutably borrowed resource from s to *s",
            "Return immutably borrowed resource from t to *t",
        ],
        ..E
    },
    Snippet {
        id: "c4_11_array_slice",
        status: Missing,
        note: "array literal `a` has no creation event (#179)",
        must_contain: &[
            "Immutable borrow from a to slice",
            "Return immutably borrowed value from slice to a",
            "a goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &["Return immutably borrowed resource from slice to a"],
        ..E
    },
    Snippet {
        id: "c4_12_move_then_reassign",
        must_contain: &[
            "s acquires ownership from the caller",
            "Move from s to takes",
            "L8 Move from String::from to s",
            "L10 s goes out of scope. Its resource is dropped.",
        ],
        ..E
    },
    Snippet {
        id: "c4_13_change_then_use",
        must_contain: &[
            "some_string is a mutable borrow from the caller",
            "some_string holds a mutable reference; it can read and write the borrowed resource",
            "push_str reads from/writes to some_string (mutable reborrow — some_string can't be used until push_str returns)",
            "change reads from/writes to s",
            "s goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &["some_string is the owner of the resource"],
        ..E
    },
    Snippet {
        id: "c4_14_partial_move_field",
        note: "nit: \"p.age is the owner of the resource\" for a u32 field",
        must_contain: &[
            "Move from p.name to n",
            "n goes out of scope. Its resource is dropped.",
            "p.name goes out of scope. No resource is dropped.",
        ],
        ..E
    },
    Snippet {
        id: "c4_15_mutate_copy_int",
        must_contain: &[
            "x's value is copied to the caller",
            "Copy from a to add_one",
            "Copy from add_one to b",
        ],
        must_not_contain: &["x's resource is moved to the caller", "x's resource is copied to the caller"],
        ..E
    },
    // ─── Chapter 5: structs and methods ───────────────────────────────
    Snippet {
        id: "c5_01_struct_field_read",
        note: "nit: bool/u64 fields \"acquire ownership of a resource\"",
        must_contain: &[
            "Move from String::from to user1.username",
            "Move from String::from to user1.email",
            "user1.username goes out of scope. Its resource is dropped.",
            "user1.email goes out of scope. Its resource is dropped.",
        ],
        ..E
    },
    Snippet {
        id: "c5_02_struct_field_assign",
        must_contain: &[
            "user1.email acquires ownership of a new resource; its previous resource is dropped",
            "user1.username goes out of scope. Its resource is dropped.",
            "user1.email goes out of scope. Its resource is dropped.",
        ],
        ..E
    },
    Snippet {
        id: "c5_03_build_user_shorthand",
        must_contain: &[
            "email acquires ownership from the caller",
            "username's resource is moved to the caller",
            "email's resource is moved to the caller",
            "email goes out of scope. No resource is dropped.",
            "username goes out of scope. No resource is dropped.",
            "Move from build_user to u",
            "u.username goes out of scope. Its resource is dropped.",
            "u.email goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "email goes out of scope. Its resource is dropped.",
            "username goes out of scope. Its resource is dropped.",
            "u.username goes out of scope. No resource is dropped.",
            "u.email goes out of scope. No resource is dropped.",
        ],
        ..E
    },
    Snippet {
        id: "c5_04_struct_update",
        must_contain: &[
            "Move from String::from to user2.email",
            "Move from user1.username to user2.username",
            "user1.username goes out of scope. No resource is dropped.",
            "user2.username goes out of scope. Its resource is dropped.",
            "user1.email goes out of scope. Its resource is dropped.",
            "user2.email goes out of scope. Its resource is dropped.",
            "user1 goes out of scope. user1.username was moved out earlier, so only user1.email is dropped.",
        ],
        must_not_contain: &[
            "user1.username goes out of scope. Its resource is dropped.",
            "user2.username goes out of scope. No resource is dropped.",
            "user1 goes out of scope. Its resource is dropped.",
        ],
        ..E
    },
    Snippet {
        id: "c5_05_tuple_struct",
        note: "was a crash at c2037dd (`black.0`)",
        must_contain: &[
            "Move from Color to black",
            "Copy from black.0 to r",
            "println! reads from black.2 through an implicit reference, so black.2 isn't moved",
            "black goes out of scope",
        ],
        must_not_contain: &["black goes out of scope. Its resource is dropped."],
        ..E
    },
    Snippet {
        id: "c5_06_method_mut_self",
        status: Missing,
        note: "was a crash at c2037dd. `self.n += 1` through &mut self isn't drawn: c.n's column shows no write (#147)",
        must_contain: &[
            "self is a mutable borrow from the caller",
            "self holds a mutable reference; it can read and write the borrowed value",
            "inc reads from/writes to c (implicitly mutably borrowed)",
            "c goes out of scope",
        ],
        must_not_contain: &[
            "c goes out of scope. Its resource is dropped.",
            "self holds a mutable reference; it can read and write the borrowed resource",
        ],
        ..E
    },
    Snippet {
        id: "c5_07_method_self_consume",
        must_contain: &[
            "self acquires ownership from the caller",
            "self.name's resource is moved to the caller",
            "self.name goes out of scope. No resource is dropped.",
            "Move from p to into_name",
            "Move from into_name to n",
            "p goes out of scope. No resource is dropped.",
            "p.name goes out of scope. No resource is dropped.",
            "self goes out of scope. self.name was moved out earlier, so no resource is dropped.",
        ],
        must_not_contain: &[
            "p goes out of scope. Its resource is dropped.",
            "p.name goes out of scope. Its resource is dropped.",
            "self goes out of scope. Its resource is dropped.",
        ],
        ..E
    },
    Snippet {
        id: "c5_08_assoc_fn_square",
        must_contain: &["Move from Rectangle::square to sq", "sq goes out of scope"],
        must_not_contain: &["sq goes out of scope. Its resource is dropped."],
        ..E
    },
    Snippet {
        id: "c5_09_can_hold",
        must_contain: &[
            "self is an immutable borrow from the caller",
            "other is an immutable borrow from the caller",
            "can_hold reads from rect1 (implicitly borrowed)",
            "can_hold reads from rect2",
        ],
        must_not_contain: &["self is the owner of the resource", "other is the owner of the resource"],
        ..E
    },
    Snippet {
        id: "c5_10_one_field_getter",
        must_contain: &[
            "self is an immutable borrow from the caller",
            "get reads from c (implicitly borrowed)",
            "Copy from get to v",
        ],
        must_not_contain: &["self is the owner of the resource"],
        ..E
    },
    Snippet {
        id: "c5_11_derive_debug",
        must_contain: &[
            "rect1 is bound to a value",
            "println! reads from rect1 through an implicit reference, so rect1 isn't moved",
            "rect1 goes out of scope",
        ],
        must_not_contain: &[
            "rect1 goes out of scope. Its resource is dropped.",
            "rect1 acquires ownership of a resource",
            "self is an immutable borrow from the caller",
            "f holds a mutable reference",
            "f is a mutable borrow from the caller",
        ],
        ..E
    },
    Snippet {
        id: "c5_12_dbg_macro",
        must_contain: &[
            "scale is bound to a copyable value",
            "dbg! reads from rect1",
            "rect1 goes out of scope",
        ],
        must_not_contain: &[
            "rect1 goes out of scope. Its resource is dropped.",
            "self is an immutable borrow from the caller",
            "f holds a mutable reference",
            "f is a mutable borrow from the caller",
        ],
        ..E
    },
    // ─── Chapter 6: enums and match ───────────────────────────────────
    Snippet {
        id: "c6_01_enum_data",
        must_contain: &[
            "Move from home to _a",
            "Move from loopback to _b",
            "loopback goes out of scope. No resource is dropped.",
            "_b goes out of scope. Its resource is dropped.",
        ],
        ..E
    },
    Snippet {
        id: "c6_02_match_coin",
        must_contain: &[
            "Move from c to value_in_cents",
            "coin goes out of scope",
            "Copy from value_in_cents to v",
            "c goes out of scope",
        ],
        must_not_contain: &[
            "c goes out of scope. No resource is dropped.",
            "coin goes out of scope. Its resource is dropped.",
        ],
        ..E
    },
    Snippet {
        id: "c6_03_plus_one",
        must_contain: &["Partial copy from x to i", "Copy from five to plus_one", "Copy from plus_one to six"],
        ..E
    },
    Snippet {
        id: "c6_04_match_moves_string",
        must_contain: &["Partial move from opt to s", "s goes out of scope. Its resource is dropped.", "opt was moved in at least one branch above, so it can't be used after the branches (until it's assigned again). If the branch that ran didn't move it, its resource is dropped when opt goes out of scope.", "opt goes out of scope. Its resource is dropped here only if no branch above moved it (Rust tracks this with a runtime drop flag)."],
        must_not_contain: &[JOIN_OPT],
        ..E
    },
    Snippet {
        id: "c6_05_match_on_ref",
        must_contain: &[
            "Partial immutable borrow from opt to s",
            "Return immutably borrowed resource from s to opt",
            "opt goes out of scope. Its resource is dropped.",
            "L4+ s's immutable borrow ends",
            "L4+ opt's resource is no longer immutably borrowed",
        ],
        must_not_contain: &["L4 s's immutable borrow ends", "L4 opt's resource is no longer immutably borrowed"],
        ..E
    },
    Snippet {
        id: "c6_06_if_let_copy",
        must_contain: &["Copy from config_max to max", "max is initialized with a copy of config_max"],
        ..E
    },
    Snippet {
        id: "c6_07_if_let_else_string",
        note: "on the else path name is dropped at the end of main (drop flag), not at the branch's end (#179)",
        must_contain: &["Move from name to n", "n goes out of scope. Its resource is dropped.", "name was moved in at least one branch above, so it can't be used after the branches (until it's assigned again). If the branch that ran didn't move it, its resource is dropped when name goes out of scope.", "name goes out of scope. Its resource is dropped here only if no branch above moved it (Rust tracks this with a runtime drop flag)."],
        must_not_contain: &[
            "name was moved in at least one branch above; in branches where it was not, its resource is dropped at the branch's end.",
            "name goes out of scope. No resource is dropped.",
        ],
        ..E
    },
    Snippet {
        id: "c6_08_let_else",
        note: "minor: the else-branch `return` is not shown",
        must_contain: &[
            "Move from s to x",
            "x's resource is moved to the caller",
            "Move from describe to d",
            "d goes out of scope. Its resource is dropped.",
        ],
        ..E
    },
    Snippet {
        id: "c6_09_option_unwrap_or",
        must_contain: &["Copy from unwrap_or to y", "is_some reads from x (implicitly borrowed)"],
        ..E
    },
    Snippet {
        id: "c6_10_match_guard",
        must_contain: &[
            "Copy from n to x",
            "msg is bound to an immutable reference (in all branches above)",
            "L8 msg's borrow of the string literal ends (string literals live for the whole program)",
        ],
        must_not_contain: &[
            "L8 msg's immutable borrow ends (what it borrows from isn't shown)",
            "msg is the owner of the resource",
            "msg acquires ownership of a resource",
            "msg acquired ownership of a resource (in all branches above)",
            "Return immutably borrowed resource from msg to *msg",
        ],
        ..E
    },
    Snippet {
        id: "c6_11_enum_method",
        status: Wrong,
        note: "self's own borrow ends at L9, before s reborrows through it at L10 (#372)",
        must_contain: &[
            "Partial immutable borrow from self to s",
            "call reads from m (implicitly borrowed)",
            "m goes out of scope. Its resource is dropped.",
            "L10+ s no longer borrows the resource self points to",
            "L10+ s's immutable borrow ends",
        ],
        must_not_contain: &[
            "L10 self's resource is no longer immutably borrowed",
            "L10 s's immutable borrow ends",
            "L10 s no longer borrows the resource self points to",
        ],
        known_wrong: &["L9 self's immutable borrow ends"],
        ..E
    },
    // ─── Chapter 8: collections ───────────────────────────────────────
    Snippet {
        id: "c8_01_vec_new_push",
        must_contain: &[
            "Move from Vec::new to v",
            "push reads from/writes to v (implicitly mutably borrowed)",
            "v goes out of scope. Its resource is dropped.",
        ],
        ..E
    },
    Snippet {
        id: "c8_02_vec_index_ref",
        must_contain: &[
            "Immutable borrow from v to third",
            "Return immutably borrowed resource from third to v",
        ],
        must_not_contain: &["third is the owner of the resource"],
        ..E
    },
    Snippet {
        id: "c8_03_vec_get_match",
        status: Wrong,
        note: "third: Option<&i32> borrows v until L5, but v's borrow ends at L3 (#171, #336)",
        must_contain: &[
            "get reads from v (implicitly borrowed)",
            "L5 third's immutable borrow ends (what it borrows from isn't shown)",
            "v goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &["Return immutably borrowed resource from third to third"],
        known_wrong: &["L3 v's resource is no longer immutably borrowed"],
        ..E
    },
    Snippet {
        id: "c8_04_vec_for_ref",
        must_contain: &[
            "Immutable borrow from v to i",
            "Return immutably borrowed resource from i to v",
            "v goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &["i is the owner of the resource"],
        ..E
    },
    Snippet {
        id: "c8_05_vec_for_mut",
        must_contain: &[
            "Mutable borrow from v to i",
            "i holds a mutable reference; it can read and write the borrowed value",
            "the value i points to is modified through *i",
            "Return mutably borrowed resource from i to v",
        ],
        must_not_contain: &["i is the owner of the resource", "*i acquires ownership of a resource"],
        ..E
    },
    Snippet {
        id: "c8_06_vec_push_string",
        must_contain: &[
            "Move from s to push",
            "push reads from/writes to v (implicitly mutably borrowed)",
            "s goes out of scope. No resource is dropped.",
            "v goes out of scope. Its resource is dropped.",
        ],
        ..E
    },
    Snippet {
        id: "c8_07_vec_index_copy",
        status: Missing,
        note: "`let first = v[0];` emits nothing (#179)",
        must_contain: &["push reads from/writes to v (implicitly mutably borrowed)", "first goes out of scope. No resource is dropped."],
        ..E
    },
    Snippet {
        id: "c8_08_string_push_char",
        must_contain: &["push reads from/writes to s (implicitly mutably borrowed)", "s goes out of scope. Its resource is dropped."],
        ..E
    },
    Snippet {
        id: "c8_09_string_add",
        must_contain: &[
            "Move from s1 to +",
            "+ reads from s2",
            "s1 goes out of scope. No resource is dropped.",
            "s2 goes out of scope. Its resource is dropped.",
            "s3 goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &["s1 goes out of scope. Its resource is dropped."],
        ..E
    },
    Snippet {
        id: "c8_10_format_macro",
        must_contain: &[
            "s acquires ownership of a resource",
            "s1 goes out of scope. Its resource is dropped.",
            "s goes out of scope. Its resource is dropped.",
        ],
        ..E
    },
    Snippet {
        id: "c8_11_string_chars",
        must_contain: &["chars reads from s (implicitly borrowed)", "s goes out of scope. Its resource is dropped."],
        ..E
    },
    Snippet {
        id: "c8_12_hashmap_insert_get",
        must_contain: &[
            "insert reads from/writes to scores (implicitly mutably borrowed)",
            "get reads from scores (implicitly borrowed)",
            "get reads from team_name",
            "Copy from unwrap_or to score",
        ],
        ..E
    },
    Snippet {
        id: "c8_13_hashmap_moves_keys",
        must_contain: &[
            "Move from field_name to insert",
            "Move from field_value to insert",
            "field_name goes out of scope. No resource is dropped.",
            "map goes out of scope. Its resource is dropped.",
        ],
        ..E
    },
    Snippet {
        id: "c8_14_hashmap_entry",
        must_contain: &[
            "text is bound to an immutable reference",
            "split_whitespace reads from text",
            "L6 text's borrow of the string literal ends (string literals live for the whole program)",
            "L7 Mutable borrow from map to count: or_insert's signature gives the reference it returns the same lifetime as the value it was called on",
            "the value count points to is modified through *count",
            "Return mutably borrowed resource from count to map",
            "map goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "text is the owner of the resource",
            "text acquires ownership of a resource",
            "*count acquires ownership of a resource",
            "map goes out of scope. No resource is dropped.",
            "Return immutably borrowed resource from text to *text",
            "text's immutable borrow ends (what it borrows from isn't shown)",
        ],
        ..E
    },
    Snippet {
        id: "c8_15_vec_for_by_value",
        must_contain: &["Move from v to s", "v goes out of scope. No resource is dropped."],
        ..E
    },
    Snippet {
        id: "c8_16_as_str",
        must_contain: &[
            "as_str reads from s (implicitly borrowed)",
            "Immutable borrow from s to t: as_str's signature gives the reference it returns the same lifetime as the value it was called on",
            "Return immutably borrowed resource from t to s",
        ],
        must_not_contain: &["t is the owner of the resource", "L3 s's resource is no longer immutably borrowed"],
        ..E
    },
    // ─── Chapter 9: errors ────────────────────────────────────────────
    Snippet {
        id: "c9_01_result_match",
        must_contain: &[
            "Partial copy from r to n",
            "Partial move from r to e",
            "e goes out of scope. Its resource is dropped.",
            "r was moved in at least one branch above, so it can't be used after the branches (until it's assigned again). If the branch that ran didn't move it, its resource is dropped when r goes out of scope.",
            "r goes out of scope. Its resource is dropped here only if no branch above moved it (Rust tracks this with a runtime drop flag).",
        ],
        must_not_contain: &[
            "r was moved in at least one branch above; in branches where it was not, its resource is dropped at the branch's end.",
        ],
        ..E
    },
    Snippet {
        id: "c9_02_parse_unwrap",
        must_contain: &["Copy from unwrap to n"],
        ..E
    },
    Snippet {
        id: "c9_03_expect_string",
        must_contain: &[
            "Move from maybe to expect",
            "Move from expect to s",
            "maybe goes out of scope. No resource is dropped.",
            "s goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &["maybe goes out of scope. Its resource is dropped."],
        ..E
    },
    Snippet {
        id: "c9_04_question_option",
        status: Wrong,
        note: "renders since #155 (was a crash in the probe). x (from v.first()?) borrows *v until L3, but v's borrow ends at L2 (#171)",
        must_contain: &[
            "v is an immutable borrow from the caller",
            "first reads from v",
            "x holds an immutable reference; it can read the borrowed value",
            "L3 x's immutable borrow ends (what it borrows from isn't shown)",
            "first_plus_one reads from v",
            "v goes out of scope. Its resource is dropped.",
            "L2 first no longer borrows the resource v points to",
        ],
        must_not_contain: &[
            "x is the owner of the resource",
            "Return immutably borrowed resource from x to *x",
            "L2 v's resource is no longer immutably borrowed",
        ],
        known_wrong: &["L2 v's immutable borrow ends"],
        ..E
    },
    Snippet {
        id: "c9_05_early_err_return",
        status: Missing,
        note: "`return Err(..)` and the tail `Ok(x)` are not shown as returned to the caller",
        must_contain: &["Copy from x to Ok", "Move from check to r", "r goes out of scope. Its resource is dropped."],
        ..E
    },
    Snippet {
        id: "c9_06_panic_branch",
        must_contain: &["len reads from v (implicitly borrowed)", "v goes out of scope. Its resource is dropped."],
        ..E
    },
    Snippet {
        id: "c9_07_question_result_string",
        status: Missing,
        note: "renders since #155 (was a crash in the probe); the tail `Ok(n * 2)` is not shown as returned",
        must_contain: &[
            "s is an immutable borrow from the caller",
            "parse reads from s",
            "Move from double to r",
            "r goes out of scope",
        ],
        must_not_contain: &["r goes out of scope. Its resource is dropped."],
        ..E
    },
    Snippet {
        id: "c9_08_unwrap_or_else",
        must_contain: &[
            "Move from Err to r",
            "Move from r to unwrap_or_else",
            "Copy from unwrap_or_else to n",
            "r goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &["r goes out of scope. Its resource is dropped."],
        ..E
    },
    // ─── Chapter 10: generics, traits, lifetimes ──────────────────────
    Snippet {
        id: "c10_01_largest_generic",
        must_contain: &[
            "list is an immutable borrow from the caller",
            "Immutable reborrow from list to largest",
            "a reference from largest is returned to the caller; the data it points to is not copied",
            "largest reads from number_list",
            "item's reference is copied (not the resource it points to)",
            "L5 largest is assigned a copy of item's reference",
            "L13 result holds an immutable reference",
            "L13 Immutable borrow from number_list to result: largest's signature gives the reference it returns the same lifetime as its argument",
            "L14 Return immutably borrowed resource from result to number_list",
            "number_list goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "result is the owner of the resource",
            "item's resource is copied",
            "Immutable borrow from list to largest",
            "Return immutably borrowed resource from result to *result",
            "L5 largest is initialized with a copy of item",
            "Copy from largest to result",
            "L13 number_list's resource is no longer immutably borrowed",
        ],
        ..E
    },
    Snippet {
        id: "c10_02_generic_struct_method",
        must_contain: &["self is an immutable borrow from the caller", "x reads from p (implicitly borrowed)"],
        must_not_contain: &["self is the owner of the resource"],
        ..E
    },
    Snippet {
        id: "c10_03_trait_impl",
        must_contain: &[
            "self is an immutable borrow from the caller",
            "summarize reads from t (implicitly borrowed)",
            "t.username goes out of scope. Its resource is dropped.",
        ],
        ..E
    },
    Snippet {
        id: "c10_04_trait_default",
        must_contain: &["summarize reads from a (implicitly borrowed)", "Move from summarize to s", "a.title goes out of scope. Its resource is dropped."],
        ..E
    },
    Snippet {
        id: "c10_05_impl_trait_param",
        must_contain: &["item is an immutable borrow from the caller", "notify reads from s"],
        ..E
    },
    Snippet {
        id: "c10_06_longest",
        must_contain: &[
            "x is an immutable borrow from the caller",
            "string2 is bound to an immutable reference",
            "as_str reads from string1 (implicitly borrowed)",
            "L8 string2's borrow of the string literal ends (string literals live for the whole program)",
            "L8 Immutable borrow from string1 to result: longest's signature gives the reference it returns the same lifetime as both arguments (the other is a value with no column here), so string1 stays borrowed while result is in use",
            "L9 Return immutably borrowed resource from result to string1",
        ],
        must_not_contain: &[
            "string2 is the owner of the resource",
            "string2 acquires ownership of a resource",
            "Return immutably borrowed resource from string2 to *string2",
            "Return immutably borrowed resource from result to *result",
            "L8 string1's resource is no longer immutably borrowed",
            "string2's immutable borrow ends (what it borrows from isn't shown)",
        ],
        ..E
    },
    Snippet {
        id: "c10_07_longest_inner_scope",
        must_contain: &[
            "longest reads from string1",
            "longest reads from string2",
            "L9 result holds an immutable reference",
            "L9 Immutable borrow from string1 to result: longest's signature gives the reference it returns the same lifetime as both arguments, so it may point into either, and both stay borrowed while result is in use",
            "L9 Immutable borrow from string2 to result: longest's signature gives the reference it returns the same lifetime as both arguments, so it may point into either, and both stay borrowed while result is in use",
            "L10 Return immutably borrowed resource from result to string1",
            "L10 Return immutably borrowed resource from result to string2",
            "L11 string2 goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "result is the owner of the resource",
            "Return immutably borrowed resource from result to *result",
            "L9 string1's resource is no longer immutably borrowed",
            "L9 string2's resource is no longer immutably borrowed",
        ],
        ..E
    },
    Snippet {
        id: "c10_08_generic_by_value",
        must_contain: &[
            "Move from s to show",
            "Copy from n to show",
            "t goes out of scope. Its resource is dropped.",
            "s goes out of scope. No resource is dropped.",
        ],
        ..E
    },
    // ─── Chapter 13: closures and iterators ───────────────────────────
    Snippet {
        id: "c13_01_closure_param",
        status: Wrong,
        note: "y gets the closure's return value, but the arrow reads as a copy of add_one; on main it now starts at a function icon, not add_one's column. The call isn't drawn as a borrow of add_one (#299)",
        must_contain: &["Closure add_one is bound"],
        must_not_contain: &["add_one's resource is copied"],
        known_wrong: &[
            "y is initialized with a copy of add_one",
            "Copy from add_one to y",
        ],
        ..E
    },
    Snippet {
        id: "c13_02_fnonce_move_return",
        status: Wrong,
        note: "f() consumes f (FnOnce) and moves s out to t; f is still shown dropping its capture",
        must_contain: &[
            "Closure capture (move) from s to f",
            "s goes out of scope. No resource is dropped.",
            "t goes out of scope. Its resource is dropped.",
        ],
        known_wrong: &["f goes out of scope. Its 1 captured resource is dropped."],
        ..E
    },
    Snippet {
        id: "c13_03_fnmut_counter",
        must_contain: &[
            "Closure capture (mutable borrow) from count to inc",
            "Return mutably borrowed value from inc to count",
        ],
        must_not_contain: &["Return mutably borrowed resource from inc to count"],
        ..E
    },
    Snippet {
        id: "c13_04_thread_spawn_move",
        must_contain: &[
            "list acquires ownership of a resource",
            "Move from list into the closure passed to thread::spawn",
            "list's resource is moved into a closure passed to thread::spawn; it's dropped when that closure is dropped",
            "list goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &["list goes out of scope. Its resource is dropped.", "Move from list to thread::spawn"],
        ..E
    },
    Snippet {
        id: "c13_05_closure_to_fn",
        status: Missing,
        note: "closure passed as an argument: the call f(1) isn't drawn as a use of f, and the capture of k is missing (#298)",
        must_contain: &[
            "f acquires ownership from the caller",
            "f goes out of scope. Its resource is dropped.",
            "Copy from apply to r",
        ],
        must_not_contain: &["f is initialized with a copy of Anonymous resource"],
        ..E
    },
    Snippet {
        id: "c13_06_iter_map_collect",
        must_contain: &["iter reads from v1 (implicitly borrowed)", "Move from collect to v2", "v1 goes out of scope. Its resource is dropped."],
        ..E
    },
    Snippet {
        id: "c13_07_into_iter_filter",
        must_contain: &[
            "Move from shoes to shoes_in_size",
            "Move from shoes to into_iter",
            "L7 shoes goes out of scope. No resource is dropped.",
            "Move from shoes_in_size to mine",
            "shoes goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &["L7 shoes goes out of scope. Its resource is dropped."],
        ..E
    },
    Snippet {
        id: "c13_08_iter_mut",
        status: Wrong,
        note: "x's borrow 'returned' to iter_mut (#171)",
        must_contain: &[
            "iter_mut reads from/writes to v (implicitly mutably borrowed)",
            "the value x points to is modified through *x",
            "v goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &["*x acquires ownership of a resource"],
        known_wrong: &["Return mutably borrowed resource from x to iter_mut"],
        ..E
    },
    Snippet {
        id: "c13_09_iter_sum",
        must_contain: &["iter reads from v1 (implicitly borrowed)", "Copy from sum to total"],
        ..E
    },
    Snippet {
        id: "c13_10_iter_next",
        status: Wrong,
        note: "v1_iter borrows v1 through L5, but v1's borrow ends at L3 (#171)",
        must_contain: &["iter reads from v1 (implicitly borrowed)", "next reads from/writes to v1_iter (implicitly mutably borrowed)"],
        known_wrong: &["L3 v1's resource is no longer immutably borrowed"],
        ..E
    },
    Snippet {
        id: "c13_11_sort_by_key_fnmut",
        status: Missing,
        note: "the closure's &mut capture of num_sort_operations is not shown; `list` has no creation event",
        must_contain: &["sort_by_key reads from/writes to list (implicitly mutably borrowed)", "list goes out of scope. No resource is dropped."],
        ..E
    },
    Snippet {
        id: "c13_12_closure_borrow_then_use",
        must_contain: &[
            "Closure capture (immutable borrow) from list to only_borrows",
            "Return immutably borrowed resource from only_borrows to list",
            "list goes out of scope. Its resource is dropped.",
        ],
        ..E
    },
    // ─── Chapter 15: smart pointers ───────────────────────────────────
    Snippet {
        id: "c15_01_box_deref",
        must_contain: &["Copy from x to Box::new", "Move from Box::new to y", "y goes out of scope. Its resource is dropped."],
        ..E
    },
    Snippet {
        id: "c15_02_cons_list",
        note: "was a crash at c2037dd (nested Cons constructor)",
        must_contain: &[
            "Move from Cons to list",
            "Move from list to _l",
            "_l goes out of scope. Its resource is dropped.",
            "list goes out of scope. No resource is dropped.",
        ],
        ..E
    },
    Snippet {
        id: "c15_03_ref_deref",
        must_contain: &["Immutable borrow from x to y", "Return immutably borrowed value from y to x"],
        must_not_contain: &["y is the owner of the resource", "Return immutably borrowed resource from y to x"],
        ..E
    },
    Snippet {
        id: "c15_04_mybox_deref_coercion",
        status: Missing,
        note: "was a crash at c2037dd. Returning `&self.0` from deref has no event (#266)",
        must_contain: &[
            "self is an immutable borrow from the caller",
            "name is an immutable borrow from the caller",
            "hello reads from m",
            "m.0 goes out of scope. Its resource is dropped.",
        ],
        ..E
    },
    Snippet {
        id: "c15_05_rc_strong_count",
        status: Wrong,
        note: "Rc: b's drop only decrements the count; the value is freed when a drops (#373)",
        must_contain: &["Rc::clone reads from a", "Move from Rc::new to a"],
        known_wrong: &["b goes out of scope. Its resource is dropped."],
        ..E
    },
    Snippet {
        id: "c15_06_rc_method_clone",
        status: Wrong,
        note: "Rc: same as c15_05 (#373)",
        must_contain: &["clone reads from a (implicitly borrowed)", "Move from Rc::new to a"],
        known_wrong: &["b goes out of scope. Its resource is dropped."],
        ..E
    },
    Snippet {
        id: "c15_07_refcell_borrow_mut",
        status: Missing,
        note: "the `.push(2)` write through the borrow_mut guard has no event of its own, and RefCell's runtime &mut borrow isn't drawn (README Limitations)",
        must_contain: &[
            "borrow borrows c immutably (a shared borrow); c has interior mutability, so its contents can change even through a shared borrow",
            "borrow_mut borrows c immutably (a shared borrow); exclusive access to its contents is checked at runtime (a second borrow_mut while this one is live would panic)",
            "c goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &["borrow_mut reads from c", "borrow reads from c"],
        ..E
    },
    Snippet {
        id: "c15_08_rc_refcell",
        status: Wrong,
        note: "was a crash at c2037dd. a drops first and only decrements the Rc count; value's drop frees the RefCell (#373). The `+= 10` write through the borrow_mut guard has no event of its own (README Limitations)",
        must_contain: &[
            "Move from Rc::new to value",
            "Rc::clone borrows value immutably (a shared borrow); value has interior mutability, so its contents can change even through a shared borrow",
            "borrow_mut borrows value immutably (a shared borrow); exclusive access to its contents is checked at runtime (a second borrow_mut while this one is live would panic)",
            "Move from Rc::clone to a",
            "println! reads from a through an implicit reference, so a isn't moved",
            "value goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &["borrow_mut reads from value", "Rc::clone reads from value"],
        known_wrong: &["a goes out of scope. Its resource is dropped."],
        ..E
    },
    Snippet {
        id: "c15_09_drop_early",
        must_contain: &[
            "self is a mutable borrow from the caller",
            "self holds a mutable reference; it can read and write the borrowed resource",
            "Move from c to drop",
            "c goes out of scope. No resource is dropped.",
            "c.data goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &["c.data goes out of scope. Its resource is dropped.", "self is the owner of the resource"],
        ..E
    },
    Snippet {
        id: "c15_10_box_string_move",
        must_contain: &[
            "Move from b to c",
            "b goes out of scope. No resource is dropped.",
            "c goes out of scope. Its resource is dropped.",
        ],
        ..E
    },
    // ─── Misc: shadowing, loops, patterns, items ──────────────────────
    Snippet {
        id: "m_01_shadow_type_change",
        status: Wrong,
        note: "both `spaces` share one column (#173), so the &str's borrow is shown ending at L4, where `spaces` is the usize, and the second `let` reads as an assignment to the first binding",
        must_contain: &["spaces is bound to an immutable reference", "len reads from spaces"],
        must_not_contain: &[
            "spaces is the owner of the resource",
            "spaces acquires ownership of a resource",
            "Return immutably borrowed resource from spaces to *spaces",
        ],
        known_wrong: &[
            "L4 spaces's borrow of the string literal ends (string literals live for the whole program)",
            "L4 spaces's immutable borrow ends",
            "L3 spaces is assigned the result of len",
        ],
        ..E
    },
    Snippet {
        id: "m_02_shadow_inner_block",
        status: Wrong,
        note: "inner x merges into the outer x's column and is never dropped at L6 (#173); not expressible as a tooltip string",
        must_contain: &["L2 Move from String::from to x", "L8 x goes out of scope. Its resource is dropped."],
        ..E
    },
    Snippet {
        id: "m_03_nested_block_borrow",
        must_contain: &[
            "Immutable borrow from a to r",
            "Return immutably borrowed resource from r to a",
            "L7 b goes out of scope. Its resource is dropped.",
            "L9 a goes out of scope. Its resource is dropped.",
        ],
        ..E
    },
    Snippet {
        id: "m_04_loop_break_string",
        status: Wrong,
        note: "`break String::from(..)` gives s a String; never shown, and s shown not dropped",
        must_contain: &["counter is bound to a copyable value"],
        known_wrong: &["s goes out of scope. No resource is dropped."],
        ..E
    },
    Snippet {
        id: "m_05_while_let_pop",
        must_contain: &["pop reads from/writes to stack (implicitly mutably borrowed)", "Copy from pop to top", "stack goes out of scope. Its resource is dropped."],
        ..E
    },
    Snippet {
        id: "m_06_tuple_index",
        must_contain: &[
            "Copy from tup.0 to x",
            "z is initialized with a copy of tup.2",
            "Copy from tup.0 to five_hundred",
            "println! reads from five_hundred through an implicit reference, so five_hundred isn't moved",
        ],
        ..E
    },
    Snippet {
        id: "m_07_tuple_var_destructure_move",
        must_contain: &[
            "Move from pair.0 to a",
            "Move from pair.1 to b",
            "pair goes out of scope. pair.0 and pair.1 were moved out earlier, so no resource is dropped.",
            "a goes out of scope. Its resource is dropped.",
        ],
        ..E
    },
    Snippet {
        id: "m_08_early_return_string",
        status: Wrong,
        note: "`return s` is not shown; s shown dropped at the end of pick, false on the return path",
        must_contain: &["Move from s to pick", "Move from pick to r", "s goes out of scope. No resource is dropped."],
        known_wrong: &["L6 s goes out of scope. Its resource is dropped."],
        ..E
    },
    Snippet {
        id: "m_09_struct_destructure",
        must_contain: &["Copy from p.x to x", "Copy from p.y to y"],
        // Destructuring copies each i32 field out of p; it isn't a copy of p as a whole (#326).
        must_not_contain: &["Copy from p to x", "Copy from p to y"],
        ..E
    },
    Snippet {
        id: "m_10_for_range_break",
        must_contain: &["L9 s goes out of scope. Its resource is dropped."],
        ..E
    },
    Snippet {
        id: "m_11_unreachable_arm",
        must_contain: &["L4 Move from String::from to s", "L5 Move from String::from to s", "s goes out of scope. Its resource is dropped.", "L7 s acquired ownership of a resource in every branch that reaches this point (the _ arm panicked)"],
        must_not_contain: &["L6 s acquires ownership of a resource"],
        ..E
    },
    Snippet {
        id: "m_12_method_chain_trim_len",
        must_contain: &["trim reads from s (implicitly borrowed)", "Copy from len to n"],
        ..E
    },
    Snippet {
        id: "m_13_return_ref_from_vec",
        must_contain: &[
            "v is an immutable borrow from the caller",
            "a reference from v is returned to the caller; the data it points to is not copied",
            "first reads from v",
            "L7 f holds an immutable reference",
            "L7 Immutable borrow from v to f: first's signature gives the reference it returns the same lifetime as its argument",
            "L8 Return immutably borrowed resource from f to v",
            "v goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "v's resource is moved to the caller",
            "f is the owner of the resource",
            "Return immutably borrowed resource from f to *f",
            "L7 v's resource is no longer immutably borrowed",
        ],
        ..E
    },
    Snippet {
        id: "m_14_while_counter",
        must_contain: &["number is bound to a copyable value"],
        ..E
    },
    Snippet {
        id: "m_15_struct_vec_field_method",
        must_contain: &[
            "self is a mutable borrow from the caller",
            "self holds a mutable reference; it can read and write the borrowed resource",
            "push reads from/writes to s (implicitly mutably borrowed)",
            "len reads from s.items (implicitly borrowed)",
            "s.items goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &["self is the owner of the resource"],
        ..E
    },
    Snippet {
        id: "m_16_match_range",
        must_contain: &["n is bound to a copyable value"],
        ..E
    },
    Snippet {
        id: "m_17_borrow_mut_then_read",
        must_contain: &[
            "Mutable borrow from x to r",
            "r holds a mutable reference; it can read and write the borrowed value",
            "the value r points to is modified through *r",
            "Return mutably borrowed value from r to x",
            "Copy from x to y",
        ],
        must_not_contain: &["r is the owner of the resource", "*r acquires ownership of a resource", "Return mutably borrowed resource from r to x"],
        ..E
    },
    Snippet {
        id: "m_18_const",
        note: "renders since #155 (was a crash in the probe)",
        must_contain: &["x is bound to a copyable value"],
        ..E
    },
    // ─── Extras: crash minimizations and controls ─────────────────────
    Snippet {
        id: "x01_field_compound_assign_main",
        must_contain: &[
            "L6 c.n is bound to a copyable value",
            "L7 c.n is assigned a new value",
            "c goes out of scope",
        ],
        must_not_contain: &[
            "c goes out of scope. Its resource is dropped.",
            "c.n acquires ownership of a resource",
        ],
        ..E
    },
    Snippet {
        id: "x02_mut_self_field_assign_plain",
        status: Missing,
        note: "was a crash at c2037dd. `self.n = 0` through &mut self isn't drawn: c.n's column shows no write (#147)",
        must_contain: &[
            "self is a mutable borrow from the caller",
            "reset reads from/writes to c (implicitly mutably borrowed)",
            "println! reads from c.n through an implicit reference, so c.n isn't moved",
        ],
        ..E
    },
    Snippet {
        id: "x03_free_fn_mut_ref_field_compound",
        status: Missing,
        note: "was a crash at c2037dd. `c.n += 1` through c: &mut Counter isn't drawn: main's c.n shows no write (#147)",
        must_contain: &[
            "c is a mutable borrow from the caller",
            "inc reads from/writes to c",
            "c goes out of scope",
        ],
        must_not_contain: &["c goes out of scope. Its resource is dropped."],
        ..E
    },
    Snippet {
        id: "x04_tuple_field_simple",
        status: Missing,
        note: "was a crash at c2037dd. `let t = (1, 2)` and `let a = t.0` have no events (#189)",
        must_contain: &["println! reads from a through an implicit reference, so a isn't moved"],
        ..E
    },
    Snippet {
        id: "x05_refcell_deref_assign",
        status: Missing,
        note: "was a crash at c2037dd. The `+= 1` write through the borrow_mut guard has no event of its own (README Limitations)",
        must_contain: &[
            "borrow_mut borrows c immutably (a shared borrow); exclusive access to its contents is checked at runtime (a second borrow_mut while this one is live would panic)",
            "Move from RefCell::new to c",
            "println! reads from c through an implicit reference, so c isn't moved",
            "c goes out of scope",
        ],
        must_not_contain: &[
            "c goes out of scope. Its resource is dropped.",
            "borrow_mut reads from c",
        ],
        ..E
    },
    Snippet {
        id: "x06_const_in_expr",
        note: "renders since #155 (was a crash in the probe)",
        must_contain: &["x is bound to a copyable value"],
        ..E
    },
    Snippet {
        id: "x07_cons_qualified",
        note: "single (non-nested) constructor",
        must_contain: &["Move from List::Cons to list", "Move from list to _l", "_l goes out of scope. Its resource is dropped."],
        ..E
    },
    Snippet {
        id: "x08_mut_ref_deref_compound_main",
        must_contain: &[
            "Mutable borrow from n to r",
            "r holds a mutable reference; it can read and write the borrowed value",
            "the value r points to is modified through *r",
            "Return mutably borrowed value from r to n",
        ],
        must_not_contain: &["*r acquires ownership of a resource", "r is the owner of the resource", "Return mutably borrowed resource from r to n"],
        ..E
    },
    Snippet {
        id: "x09_static_item",
        status: Missing,
        note: "was a crash at c2037dd. The static has no column (README Limitations); only the borrow-end arrow names it",
        must_contain: &[
            "g is bound to an immutable reference",
            "println! reads from g through an implicit reference, so g isn't moved",
            "L5 g's borrow of what the static GREETING points to ends (it lives for the whole program)",
        ],
        must_not_contain: &[
            "Return immutably borrowed resource from g to *g",
            "g's immutable borrow ends (what it borrows from isn't shown)",
        ],
        ..E
    },
    Snippet {
        id: "x10_existing_question_mark",
        status: Missing,
        note: "copy of tests/question_mark.rs; renders since #155. The tail `Ok(n)` is not shown as returned",
        must_contain: &["Copy from parse to n", "Copy from n to Ok"],
        ..E
    },
    Snippet {
        id: "x11_existing_shadowing_basic",
        status: Wrong,
        note: "copy of tests/shadowing_basic.rs: both `s` share one column, so only one drop shows at L9 (#173); not expressible as a tooltip string",
        must_contain: &["L6 Move from String::from to s", "L9 s goes out of scope. Its resource is dropped."],
        ..E
    },
    Snippet {
        id: "x12_cons_qualified_nested",
        note: "was a crash at c2037dd (nested List::Cons)",
        must_contain: &[
            "Move from List::Cons to list",
            "Move from list to _l",
            "_l goes out of scope. Its resource is dropped.",
            "list goes out of scope. No resource is dropped.",
        ],
        ..E
    },
    Snippet {
        id: "x13_nested_same_fn_call",
        must_contain: &["Copy from add to x"],
        ..E
    },
    Snippet {
        id: "x14_nested_string_fn",
        must_contain: &[
            "s acquires ownership from the caller",
            "Move from s to +",
            "L3 s goes out of scope. No resource is dropped.",
            "Move from shout to s",
            "L8 s goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &["L3 s goes out of scope. Its resource is dropped."],
        ..E
    },
    Snippet {
        id: "x15_question_in_main_style",
        status: Missing,
        note: "renders since #155 (was a crash in the probe); the tail `Ok(x + y)` is not shown as returned",
        must_contain: &["a is an immutable borrow from the caller", "parse reads from a", "parse reads from b"],
        ..E
    },
    Snippet {
        id: "x16_match_arm_block_body",
        note: "control for c6_04: a block-bodied arm draws s's drop on canvas",
        must_contain: &["Partial move from opt to s", "L6 s goes out of scope. Its resource is dropped.", "opt was moved in at least one branch above, so it can't be used after the branches (until it's assigned again). If the branch that ran didn't move it, its resource is dropped when opt goes out of scope.", "opt goes out of scope. Its resource is dropped here only if no branch above moved it (Rust tracks this with a runtime drop flag)."],
        must_not_contain: &[JOIN_OPT],
        ..E
    },
    Snippet {
        id: "x17_match_arm_fn_call_body",
        note: "control: fn-call arm body",
        must_contain: &["Partial move from opt to s", "Move from s to show", "s goes out of scope. No resource is dropped.", "opt was moved in at least one branch above, so it can't be used after the branches (until it's assigned again). If the branch that ran didn't move it, its resource is dropped when opt goes out of scope.", "opt goes out of scope. Its resource is dropped here only if no branch above moved it (Rust tracks this with a runtime drop flag)."],
        must_not_contain: &[JOIN_OPT],
        ..E
    },
];

// ─── Harness ───────────────────────────────────────────────────────────

fn book_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("rustviz-plugin")
        .join("tests")
        .join("book")
}

fn ensure_plugin_installed() {
    let ok = Command::new("cargo")
        .args(["rv-plugin", "--help"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    assert!(
        ok,
        "`cargo rv-plugin` not on PATH. Run `cargo install --path rustviz-plugin --locked` \
         (or `./setup.sh`) before `cargo test`."
    );
}

enum Outcome {
    Rendered { code: String, timeline: String },
    /// The plugin panicked; the `panicked at ...` line.
    Crashed(String),
    /// Anything else (compile error, bad output): always a failure.
    Error(String),
}

fn run(id: &str) -> Outcome {
    let path = book_dir().join(format!("{}.rs", id));
    let src = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => return Outcome::Error(format!("read {:?}: {}", path, e)),
    };
    match Rustviz::new(&src) {
        Ok(rv) => Outcome::Rendered {
            code: rv.code_panel_string(),
            timeline: rv.timeline_panel_string(),
        },
        Err(e) => {
            let msg = e.to_string();
            match msg.lines().find(|l| l.contains("panicked at")) {
                Some(line) => Outcome::Crashed(line.trim().to_string()),
                None => Outcome::Error(msg.chars().take(600).collect()),
            }
        }
    }
}

/// Value of attribute `name` in `tag`, matching only whole attribute names
/// (`y` doesn't match inside `cy`).
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let pat = format!("{}=\"", name);
    let mut from = 0;
    while let Some(i) = tag[from..].find(&pat) {
        let at = from + i;
        let boundary = tag[..at]
            .chars()
            .next_back()
            .map_or(true, |c| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'));
        let rest = &tag[at + pat.len()..];
        if boundary {
            return rest.find('"').map(|end| &rest[..end]);
        }
        from = at + pat.len();
    }
    None
}

fn leading_number(s: &str) -> Option<(f64, &str)> {
    let end = s.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(s.len());
    s[..end].parse().ok().map(|n| (n, &s[end..]))
}

/// The y coordinate an element is drawn at, by the same priority as the
/// probe's driver: `cy`, `y1`, second number of `points`, start of a path
/// `d`, then `y`.
fn element_y(own: &str) -> Option<f64> {
    if let Some(v) = attr(own, "cy").or_else(|| attr(own, "y1")) {
        return leading_number(v).map(|(n, _)| n);
    }
    if let Some(v) = attr(own, "points") {
        let (_, rest) = leading_number(v)?;
        return leading_number(rest.strip_prefix([' ', ','])?).map(|(n, _)| n);
    }
    if let Some(v) = attr(own, "d") {
        let v = v.strip_prefix('M')?.trim_start();
        let (_, rest) = leading_number(v)?;
        return leading_number(rest.strip_prefix([' ', ','])?).map(|(n, _)| n);
    }
    attr(own, "y").and_then(leading_number).map(|(n, _)| n)
}

/// Code-panel rows as (y, line label). Rows without a printed line number
/// (wrapped or blank continuation rows) are labelled `<prev>+`.
fn code_rows(code_svg: &str) -> Vec<(f64, String)> {
    const OPEN: &str = "<text class=\"code\" x=\"";
    let mut rows = Vec::new();
    let mut last = "0".to_string();
    for chunk in code_svg.split(OPEN).skip(1) {
        let Some(after_x) = chunk.find("\" y=\"").map(|i| &chunk[i + 5..]) else { continue };
        let Some((y, rest)) = leading_number(after_x) else { continue };
        let Some(rest) = rest.strip_prefix("\"> <tspan fill=\"#AAA\">") else { continue };
        let digits: String = rest.trim_start().chars().take_while(|c| c.is_ascii_digit()).collect();
        last = if digits.is_empty() {
            format!("{}+", last.trim_end_matches('+'))
        } else {
            digits
        };
        rows.push((y, last.clone()));
    }
    rows
}

fn clean_tooltip(raw: &str) -> String {
    let unescaped = raw
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    let mut text = String::with_capacity(unescaped.len());
    let mut in_tag = false;
    for ch in unescaped.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => text.push(ch),
            _ => {}
        }
    }
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Every tooltip in the timeline as (line label, text). The label is the
/// code row nearest the element's y ("hdr" for column headers, "?" if the
/// element has no y).
fn tooltips(code_svg: &str, timeline_svg: &str) -> Vec<(String, String)> {
    const ATTR: &str = "data-tooltip-text=\"";
    let rows = code_rows(code_svg);
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = timeline_svg[from..].find(ATTR) {
        let attr_start = from + i;
        let value_start = attr_start + ATTR.len();
        let Some(len) = timeline_svg[value_start..].find('"') else { break };
        let value_end = value_start + len;
        from = value_end + 1;

        let start = timeline_svg[..attr_start].rfind('<').unwrap_or(0);
        let tag_end = timeline_svg[start..].find(' ').map_or(start, |j| start + j);
        let is_group = &timeline_svg[start..tag_end] == "<g";
        let end = if is_group {
            timeline_svg[from..].find("</g>")
        } else {
            timeline_svg[from..].find('>')
        }
        .map_or(timeline_svg.len(), |j| from + j);
        let own = format!("{}{}", &timeline_svg[start..attr_start], &timeline_svg[from..end]);

        let line = match element_y(&own) {
            Some(y) if y <= 70.0 => "hdr".to_string(),
            Some(y) => rows
                .iter()
                .min_by(|a, b| (a.0 - 5.0 - y).abs().total_cmp(&(b.0 - 5.0 - y).abs()))
                .map_or("?".to_string(), |r| r.1.clone()),
            None => "?".to_string(),
        };
        // Events on one line of a column share one dot (#309); its
        // tooltip has one bulleted line per event. Match each separately.
        for part in timeline_svg[value_start..value_end].split("&lt;br&gt;") {
            let text = clean_tooltip(part);
            let text = text.strip_prefix("• ").map(str::to_owned).unwrap_or(text);
            out.push((line.clone(), text));
        }
    }
    out
}

/// Does a table string match a rendered tooltip? `"L13 text"` must match
/// on line 13; plain text matches on any line.
fn present(want: &str, actual: &[(String, String)]) -> bool {
    if let Some(rest) = want.strip_prefix('L') {
        let label_len = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(0);
        if label_len > 0 {
            let (digits, tail) = rest.split_at(label_len);
            let (label, text) = match tail.strip_prefix("+ ") {
                Some(text) => (format!("{}+", digits), text),
                None => match tail.strip_prefix(' ') {
                    Some(text) => (digits.to_string(), text),
                    None => return actual.iter().any(|(_, t)| t == want),
                },
            };
            return actual.iter().any(|(l, t)| *l == label && t == text);
        }
    }
    actual.iter().any(|(_, t)| t == want)
}

/// y coordinates outside `0..=height` of the panel's root `<svg>`:
/// elements drawn there are never visible.
fn off_canvas_ys(svg: &str) -> Vec<String> {
    let num = |v: &str| v.trim_end_matches("px").parse::<f64>().ok();
    let root = &svg[..svg.find('>').unwrap_or(svg.len())];
    let Some(height) = attr(root, "height").and_then(num) else {
        return vec!["<svg> has no height".to_string()];
    };
    let mut out = Vec::new();
    let mut rest = svg;
    while let Some(eq) = rest.find("=\"") {
        let name_start = rest[..eq]
            .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
            .map_or(0, |i| i + 1);
        let name = &rest[name_start..eq];
        let after = &rest[eq + 2..];
        let end = after.find('"').unwrap_or(after.len());
        let value = &after[..end];
        let ys: Vec<f64> = match name {
            "y" | "y1" | "y2" | "cy" => num(value).into_iter().collect(),
            "points" => value
                .split([' ', ','])
                .filter(|t| !t.is_empty())
                .filter_map(num)
                .skip(1)
                .step_by(2)
                .collect(),
            _ => Vec::new(),
        };
        for y in ys {
            if !(0.0..=height).contains(&y) {
                out.push(format!("{}={} (height {})", name, y, height));
            }
        }
        rest = &after[end.min(after.len())..];
    }
    out
}

fn line_sort_key(label: &str) -> (u32, u32) {
    match label {
        "hdr" => (0, 0),
        "?" => (u32::MAX, 0),
        l => {
            let n = l.trim_end_matches('+').parse().unwrap_or(u32::MAX - 1);
            (n, l.ends_with('+') as u32)
        }
    }
}

fn dump(dir: &str, id: &str, outcome: &Outcome) {
    let mut text = String::new();
    match outcome {
        Outcome::Rendered { code, timeline } => {
            text.push_str("RENDERED\n");
            let set: BTreeSet<_> = tooltips(code, timeline)
                .into_iter()
                .map(|(l, t)| (line_sort_key(&l), l, t))
                .collect();
            for (_, l, t) in set {
                text.push_str(&format!("L{} {}\n", l, t));
            }
            for (panel, svg) in [("code", code), ("timeline", timeline)] {
                for off in off_canvas_ys(svg) {
                    text.push_str(&format!("off-canvas ({}): {}\n", panel, off));
                }
            }
        }
        Outcome::Crashed(p) => text.push_str(&format!("CRASH {}\n", p)),
        Outcome::Error(e) => text.push_str(&format!("ERROR {}\n", e)),
    }
    let _ = fs::create_dir_all(dir);
    let _ = fs::write(PathBuf::from(dir).join(format!("{}.txt", id)), text);
}

/// Checks one snippet's outcome against its table entry, pushing to
/// `failures` (test fails) or `notes` (table is out of date, test passes).
fn check(s: &Snippet, outcome: &Outcome, failures: &mut Vec<String>, notes: &mut Vec<String>) {
    let (code, timeline) = match (outcome, s.status) {
        (Outcome::Error(e), _) => {
            failures.push(format!("{}: did not compile or run: {}", s.id, e));
            return;
        }
        (Outcome::Crashed(_), Crash) => return,
        (Outcome::Crashed(p), _) => {
            failures.push(format!("{}: newly crashes: {}", s.id, p));
            return;
        }
        (Outcome::Rendered { .. }, Crash) => {
            notes.push(format!(
                "{}: known crash now renders. Promote it: set its status (Correct / Wrong / Missing) \
                 after reviewing the tooltips (RV_BOOK_DUMP), and add must_contain / known_wrong.",
                s.id
            ));
            return;
        }
        (Outcome::Rendered { code, timeline }, _) => (code, timeline),
    };

    if !code.trim_start().starts_with("<svg") || !timeline.trim_start().starts_with("<svg") {
        failures.push(format!("{}: a panel is not SVG", s.id));
        return;
    }
    if !timeline.contains("tooltip-trigger") {
        failures.push(format!("{}: timeline has no tooltip-trigger elements", s.id));
        return;
    }

    let mut off = off_canvas_ys(code);
    off.extend(off_canvas_ys(timeline));
    match (off.is_empty(), s.off_canvas) {
        (false, false) => failures.push(format!(
            "{}: {} element(s) drawn outside the SVG: {:?}",
            s.id,
            off.len(),
            &off[..off.len().min(5)]
        )),
        (true, true) => notes.push(format!(
            "{}: every element is on canvas now. Clear `off_canvas` in tests/book.rs.",
            s.id
        )),
        _ => {}
    }

    let actual = tooltips(code, timeline);
    let missing: Vec<_> = s.must_contain.iter().filter(|w| !present(w, &actual)).collect();
    if !missing.is_empty() {
        failures.push(format!("{}: missing correct tooltip(s) {:?}", s.id, missing));
    }
    let back: Vec<_> = s.must_not_contain.iter().filter(|w| present(w, &actual)).collect();
    if !back.is_empty() {
        failures.push(format!("{}: false tooltip(s) are back {:?}", s.id, back));
    }
    let gone: Vec<_> = s.known_wrong.iter().filter(|w| !present(w, &actual)).collect();
    if !gone.is_empty() {
        notes.push(format!(
            "{}: known-wrong tooltip(s) no longer shown {:?}. If a fix removed them, move them to \
             must_not_contain and add the corrected text to must_contain.",
            s.id, gone
        ));
    }
}

/// The table itself must be consistent with the files on disk.
fn check_table() -> Vec<String> {
    let mut problems = Vec::new();
    let mut on_disk: BTreeSet<String> = BTreeSet::new();
    for entry in fs::read_dir(book_dir()).expect("read tests/book") {
        let name = entry.unwrap().file_name().to_string_lossy().into_owned();
        if let Some(stem) = name.strip_suffix(".rs") {
            on_disk.insert(stem.to_string());
        }
    }
    let mut seen = BTreeSet::new();
    for s in BOOK {
        if !seen.insert(s.id) {
            problems.push(format!("{}: listed twice", s.id));
        }
        if !on_disk.contains(s.id) {
            problems.push(format!("{}: no file tests/book/{}.rs", s.id, s.id));
        }
        match s.status {
            Correct if !s.known_wrong.is_empty() || s.off_canvas => {
                problems.push(format!("{}: Correct but has known_wrong / off_canvas", s.id))
            }
            Crash if !(s.must_contain.is_empty() && s.known_wrong.is_empty()) => {
                problems.push(format!("{}: Crash entries carry no tooltip lists", s.id))
            }
            Wrong | Missing if s.note.is_empty() => {
                problems.push(format!("{}: say what's wrong or missing in `note`", s.id))
            }
            _ => {}
        }
    }
    for stem in on_disk.difference(&seen.iter().map(|s| s.to_string()).collect()) {
        problems.push(format!("tests/book/{}.rs has no entry in BOOK", stem));
    }
    problems
}

#[test]
fn book_corpus() {
    let table_problems = check_table();
    assert!(table_problems.is_empty(), "BOOK table:\n  {}", table_problems.join("\n  "));

    ensure_plugin_installed();
    env::set_var("RV_RUNNER", "local");

    let only: Vec<String> = env::var("RV_BOOK_ONLY")
        .map(|v| v.split(',').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect())
        .unwrap_or_default();
    let selected: Vec<&Snippet> = BOOK
        .iter()
        .filter(|s| only.is_empty() || only.iter().any(|p| s.id.starts_with(p.as_str())))
        .collect();
    let jobs: usize = env::var("RV_BOOK_JOBS").ok().and_then(|j| j.parse().ok()).unwrap_or(4).max(1);
    let dump_dir = env::var("RV_BOOK_DUMP").ok();

    let next = AtomicUsize::new(0);
    let outcomes: Mutex<Vec<Option<Outcome>>> = Mutex::new((0..selected.len()).map(|_| None).collect());
    std::thread::scope(|scope| {
        for _ in 0..jobs.min(selected.len()) {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                let Some(s) = selected.get(i) else { break };
                let outcome = run(s.id);
                if let Some(dir) = &dump_dir {
                    dump(dir, s.id, &outcome);
                }
                outcomes.lock().unwrap()[i] = Some(outcome);
            });
        }
    });
    let outcomes = outcomes.into_inner().unwrap();

    let mut failures = Vec::new();
    let mut notes = Vec::new();
    let (mut correct, mut wrong, mut missing, mut crashes, mut promoted) = (0, 0, 0, 0, 0);
    for (s, outcome) in selected.iter().zip(&outcomes) {
        let outcome = outcome.as_ref().expect("every snippet ran");
        check(s, outcome, &mut failures, &mut notes);
        match s.status {
            Correct => correct += 1,
            Wrong => wrong += 1,
            Missing => missing += 1,
            Crash => {
                crashes += 1;
                if matches!(outcome, Outcome::Rendered { .. }) {
                    promoted += 1;
                }
            }
        }
    }

    // Written straight to stderr so it shows without `--nocapture`.
    let mut report = format!(
        "book corpus: {} render ({} correct, {} known-wrong, {} missing), {} known crashes",
        correct + wrong + missing,
        correct,
        wrong,
        missing,
        crashes
    );
    if promoted > 0 {
        report.push_str(&format!(" ({} now render)", promoted));
    }
    report.push_str(&format!("; {} failure(s), {} note(s)\n", failures.len(), notes.len()));
    for n in &notes {
        report.push_str(&format!("  note: {}\n", n));
    }
    let _ = std::io::stderr().write_all(report.as_bytes());

    assert!(
        failures.is_empty(),
        "{} book-corpus regression(s):\n  {}\n(dump tooltips with RV_BOOK_DUMP=<dir> to see what the plugin printed)",
        failures.len(),
        failures.join("\n  ")
    );
}
